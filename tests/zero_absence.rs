//! Zero values, absent values, and dangling resources.
//!
//! Safe Rust has no dangling pointers, so the "dangling" cases here are the resource
//! equivalents: an orphaned Prince process, a leaked semaphore permit, or an object written to
//! storage that no response points at.

mod common;

use common::*;
use reqwest::multipart;
use std::time::Duration;

// ---- absent / empty fields ----

#[tokio::test]
async fn empty_multipart_is_400() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app.to_bytes(multipart::Form::new()).await;

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn zero_length_body_is_4xx() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

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
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app.to_bytes(form("")).await;

    assert_eq!(resp.status(), 200);
    assert!(is_pdf(&resp.bytes().await.unwrap()));
}

#[tokio::test]
async fn absent_data_renders_top_level_variables_as_empty() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app
        .to_bytes(form(
            "<p>[{{ name }}][{{ name|default('none given') }}]</p>",
        ))
        .await;

    assert_eq!(resp.status(), 200);
    assert!(
        prince.stdins()[0].contains("<p>[][none given]</p>"),
        "{}",
        prince.stdins()[0]
    );
}

#[tokio::test]
async fn absent_nested_data_is_a_clean_400() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app.to_bytes(form("<p>{{ user.email }}</p>")).await;

    assert_eq!(
        resp.status(),
        400,
        "attribute of a missing value is a template error"
    );
    assert_eq!(prince.invocations(), 0);
}

#[tokio::test]
async fn zero_and_empty_data_values_are_accepted() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

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
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    app.to_bytes(
        form("<p>{{ zero }}|{{ f }}|{{ empty }}|{{ list|length }}</p>")
            .text("data", r#"{"zero":0,"f":false,"empty":"","list":[]}"#),
    )
    .await;

    // minijinja prints booleans/null Python-style: `False`, `None`.
    assert!(
        prince.stdins()[0].contains("<p>0|False||0</p>"),
        "{}",
        prince.stdins()[0]
    );
}

#[tokio::test]
async fn empty_page_fields_are_passed_through() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app
        .to_bytes(
            form("<p>x</p>")
                .text("width", "")
                .text("height", "")
                .text("orientation", ""),
        )
        .await;

    assert_eq!(resp.status(), 200);
    assert!(
        prince.args()[0].contains("size:   ;"),
        "{}",
        prince.args()[0]
    );
}

#[tokio::test]
async fn empty_filename_still_has_valid_header() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

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
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

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
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

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
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

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
async fn zero_permits_times_out_without_spawning_prince() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .permits(0)
        .timeout(Duration::from_secs(1))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 408);
    assert_eq!(prince.invocations(), 0);
}

#[tokio::test]
async fn real_empty_template_and_page_fields() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let resp = app
        .to_bytes(
            form("")
                .text("width", "")
                .text("height", "")
                .text("orientation", ""),
        )
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

// ---- dangling resources ----

#[tokio::test]
async fn timeout_kills_prince_and_returns_permit() {
    let prince = FakePrince::new(r#"echo $$ > "$D/pid.$$"; exec sleep 3600"#);
    let app = TestApp::builder(prince.path())
        .permits(1)
        .timeout(Duration::from_secs(1))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 408);

    let pid = prince.pids()[0];
    let dead = eventually(Duration::from_secs(5), || !process_alive(pid)).await;
    assert!(
        dead,
        "Prince pid {pid} outlived its request (orphaned process)"
    );
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn client_disconnect_does_not_leave_prince_running() {
    let prince = FakePrince::new(r#"echo $$ > "$D/pid.$$"; exec sleep 3600"#);
    let render_timeout = Duration::from_secs(3);
    let app = TestApp::builder(prince.path())
        .permits(1)
        .timeout(render_timeout)
        .spawn()
        .await;

    let impatient = reqwest::Client::builder()
        .timeout(Duration::from_millis(300))
        .build()
        .unwrap();
    let r = impatient
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .multipart(form("<p>x</p>"))
        .send()
        .await;
    assert!(r.is_err());

    // At worst it lives until the render timeout; it must never outlive that.
    assert!(eventually(Duration::from_secs(2), || !prince.pids().is_empty()).await);
    let pid = prince.pids()[0];
    let dead = eventually(render_timeout + Duration::from_secs(3), || {
        !process_alive(pid)
    })
    .await;
    assert!(dead, "Prince pid {pid} outlived the abandoned request");
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn failed_render_leaves_no_object_in_storage() {
    let prince = FakePrince::new("cat > /dev/null; exit 1");
    let app = TestApp::builder(prince.path()).spawn().await;

    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 500);
    assert!(app.s3.puts().is_empty());
}

#[tokio::test]
async fn presigned_url_points_at_the_object_that_was_written() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let json: serde_json::Value = app.to_s3(form("<p>x</p>")).await.json().await.unwrap();
    let url = reqwest::Url::parse(json["url"].as_str().unwrap()).unwrap();

    assert_eq!(url.path(), app.s3.puts()[0].path, "presigned URL dangles");
}
