//! Zero values, absent values, and dangling resources.
//!
//! Safe Rust has no dangling pointers, so the "dangling" cases here are the resource
//! equivalents: a render that outlives its request, a leaked semaphore permit, or an object written to
//! storage that no response points at.

mod common;

use common::*;
use reqwest::multipart;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

// ---- absent / empty fields ----

#[tokio::test]
async fn empty_multipart_is_400() {
    let app = TestApp::builder().spawn().await;

    let resp = app.to_bytes(multipart::Form::new()).await;

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn zero_length_body_is_4xx() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .client
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .header("content-type", "multipart/form-data; boundary=x")
        .body("")
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_client_error(), "got {}", resp.status());
}

#[tokio::test]
async fn empty_template_renders_a_pdf() {
    let app = TestApp::builder().spawn().await;

    let resp = app.to_bytes(form("")).await;

    assert_eq!(resp.status(), 200);
    assert!(is_pdf(&resp.bytes().await.unwrap()));
}

#[tokio::test]
async fn absent_data_renders_top_level_variables_as_empty() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form(
            "<p>[{{ name }}][{{ name|default('none given') }}]</p>",
        ))
        .await;

    assert_eq!(resp.status(), 200);
    let html = &app.calls()[0].html;
    assert!(html.contains("<p>[][none given]</p>"), "{html}");
}

#[tokio::test]
async fn absent_nested_data_is_a_clean_400() {
    let app = TestApp::builder().spawn().await;

    let resp = app.to_bytes(form("<p>{{ user.email }}</p>")).await;

    assert_eq!(
        resp.status(),
        400,
        "attribute of a missing value is a template error"
    );
    assert_eq!(app.renders(), 0);
}

#[tokio::test]
async fn zero_and_empty_data_values_are_accepted() {
    let app = TestApp::builder().spawn().await;

    for data in [
        "", "null", "{}", "[]", "0", "false", "\"\"", "not json", "{\"a\":",
    ] {
        let resp = app
            .to_bytes(form("<p>{{ a }}</p>").text("data", data))
            .await;
        assert_eq!(resp.status(), 200, "data {data:?}");
    }
}

#[tokio::test]
async fn zero_numbers_and_falsy_values_render_literally() {
    let app = TestApp::builder().spawn().await;

    app.to_bytes(
        form("<p>{{ zero }}|{{ f }}|{{ empty }}|{{ list|length }}</p>")
            .text("data", r#"{"zero":0,"f":false,"empty":"","list":[]}"#),
    )
    .await;

    // minijinja prints booleans/null Python-style: `False`, `None`.
    let html = &app.calls()[0].html;
    assert!(html.contains("<p>0|False||0</p>"), "{html}");
}

#[tokio::test]
async fn empty_page_fields_are_a_clean_400() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(
            form("<p>x</p>")
                .text("width", "")
                .text("height", "")
                .text("orientation", ""),
        )
        .await;

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().unwrap().contains("width/height"));
    assert_eq!(app.renders(), 0);
}

#[tokio::test]
async fn empty_filename_still_has_valid_header() {
    let app = TestApp::builder().spawn().await;

    for name in ["", "\"\"\""] {
        let resp = app.to_bytes(form("<p>x</p>").text("filename", name)).await;
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers()["content-disposition"],
            "inline; filename=\"\""
        );
    }
}

#[tokio::test]
async fn empty_password_means_no_encryption() {
    let app = TestApp::builder().spawn().await;

    let body = app
        .to_bytes(form("<p>x</p>").text("password", ""))
        .await
        .bytes()
        .await
        .unwrap();

    assert!(!load_pdf(&body).is_encrypted());
}

#[tokio::test]
async fn unnamed_and_unknown_fields_are_ignored() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(
            form("<p>x</p>")
                .text("", "nameless")
                .text("bogus", "value")
                .part(
                    "file",
                    multipart::Part::bytes(vec![0u8; 1024]).file_name("x.bin"),
                ),
        )
        .await;

    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn empty_authorization_values_are_401() {
    let app = TestApp::builder().spawn().await;

    for h in ["", "Bearer", "Bearer ", " "] {
        let resp = app
            .client
            .post(app.url("/api/to-bytes"))
            .header("authorization", h)
            .multipart(form("<p>x</p>"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "{h:?}");
    }
}

#[tokio::test]
async fn zero_permits_times_out_without_rendering() {
    let app = TestApp::builder()
        .permits(0)
        .timeout(Duration::from_secs(1))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 408);
    assert_eq!(app.renders(), 0);
}

#[tokio::test]
async fn empty_template_with_default_page_fields() {
    let app = TestApp::builder().spawn().await;

    let resp = app.to_bytes(form("")).await;

    assert_eq!(resp.status(), 200);
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

// ---- dangling resources ----

/// A render can't be interrupted once it's running. The request times out on schedule, and
/// the render keeps its permit until it really finishes, then gives it back.
#[tokio::test]
async fn timed_out_render_returns_its_permit_when_it_finishes() {
    let finished = Arc::new(AtomicUsize::new(0));
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Slow(
            Duration::from_secs(2),
            finished.clone(),
        )))
        .permits(1)
        .timeout(Duration::from_millis(500))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 408);
    assert_eq!(
        app.available_permits(),
        0,
        "the running render holds its permit"
    );

    assert!(
        eventually(Duration::from_secs(5), || finished.load(Ordering::SeqCst)
            == 1)
        .await
    );
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn client_disconnect_does_not_leak_the_render() {
    let finished = Arc::new(AtomicUsize::new(0));
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Slow(
            Duration::from_secs(1),
            finished.clone(),
        )))
        .permits(1)
        .timeout(Duration::from_secs(3))
        .spawn()
        .await;

    let impatient = reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();
    let r = impatient
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .multipart(form("<p>x</p>"))
        .send()
        .await;
    assert!(r.is_err());

    assert!(
        eventually(Duration::from_secs(5), || finished.load(Ordering::SeqCst)
            == 1)
        .await
    );
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn failed_render_leaves_no_object_in_storage() {
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Fails))
        .spawn()
        .await;

    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 500);
    assert!(app.s3.puts().is_empty());
}

#[tokio::test]
async fn presigned_url_points_at_the_object_that_was_written() {
    let app = TestApp::builder().spawn().await;

    let json: serde_json::Value = app.to_s3(form("<p>x</p>")).await.json().await.unwrap();
    let url = reqwest::Url::parse(json["url"].as_str().unwrap()).unwrap();

    assert_eq!(url.path(), app.s3.puts()[0].path, "presigned URL dangles");
}
