//! Sad path: ordinary bad requests get clean 4xx answers and never reach Prince or S3.

mod common;

use common::*;
use reqwest::{StatusCode, multipart};

async fn error_message(resp: reqwest::Response) -> String {
    let json: serde_json::Value = resp.json().await.expect("error body must be JSON");
    json["error"].as_str().expect("error body must have `error`").to_string()
}

#[tokio::test]
async fn missing_token_is_401_on_both_routes() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    for route in ["/api/to-bytes", "/api/to-s3"] {
        let resp = app
            .client
            .post(app.url(route))
            .multipart(form("<p>x</p>"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{route}");
        assert_eq!(error_message(resp).await, "Invalid or missing Bearer token");
    }
    assert_eq!(prince.invocations(), 0);
    assert_eq!(app.s3.request_count(), 0);
}

#[tokio::test]
async fn wrong_credentials_are_401() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let bad = [
        "Bearer wrong".to_string(),
        format!("Basic {TOKEN}"),
        format!("Bearer  {TOKEN}"),
        format!("Bearer {}", &TOKEN[..TOKEN.len() - 1]),
        format!("Bearer {TOKEN}x"),
        TOKEN.to_string(),
    ];
    for header in bad {
        let resp = app
            .client
            .post(app.url("/api/to-bytes"))
            .header("authorization", &header)
            .multipart(form("<p>x</p>"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "accepted bad header {header:?}");
    }
    assert_eq!(prince.invocations(), 0);
}

#[tokio::test]
async fn missing_template_is_400() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    for resp in [
        app.to_bytes(multipart::Form::new().text("width", "10in")).await,
        app.to_s3(multipart::Form::new().text("data", "{}")).await,
    ] {
        assert_eq!(resp.status(), 400);
        assert_eq!(error_message(resp).await, "Missing required field: template");
    }
    assert_eq!(prince.invocations(), 0);
    assert_eq!(app.s3.request_count(), 0);
}

#[tokio::test]
async fn broken_template_syntax_is_400() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    for t in [
        "{% for x in %}broken",
        "{{ unclosed",
        "{% if x %}no endif",
        "{% endfor %}",
        "{{ x | no_such_filter }}",
        "{{ no_such_function() }}",
    ] {
        let resp = app.to_bytes(form(t)).await;
        assert_eq!(resp.status(), 400, "template {t:?}");
        let msg = error_message(resp).await;
        assert!(msg.starts_with("Template rendering failed"), "{t:?} -> {msg}");
    }
    assert_eq!(prince.invocations(), 0);
}

#[tokio::test]
async fn template_error_on_to_s3_writes_nothing() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app.to_s3(form("{% for %}")).await;

    assert_eq!(resp.status(), 400);
    assert_eq!(app.s3.request_count(), 0);
}

#[tokio::test]
async fn non_multipart_body_is_rejected() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let json = app
        .client
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .json(&serde_json::json!({ "template": "<p>x</p>" }))
        .send()
        .await
        .unwrap();
    let no_boundary = app
        .client
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .header("content-type", "multipart/form-data")
        .body("template=x")
        .send()
        .await
        .unwrap();

    for resp in [json, no_boundary] {
        assert!(resp.status().is_client_error(), "got {}", resp.status());
    }
    assert_eq!(prince.invocations(), 0);
}

#[tokio::test]
async fn wrong_method_and_unknown_route() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let get = app.client.get(app.url("/api/to-bytes")).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);

    let unknown = app.client.post(app.url("/api/nope")).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unauthenticated_request_is_rejected_before_reading_body() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let huge = "x".repeat(20 * 1024 * 1024);
    let start = std::time::Instant::now();
    let resp = app
        .client
        .post(app.url("/api/to-bytes"))
        .multipart(form(&huge))
        .send()
        .await;

    // The server may answer 401 and close before the client has finished uploading.
    if let Ok(resp) = resp {
        assert_eq!(resp.status(), 401);
    }
    assert!(start.elapsed().as_secs() < 10);
    assert_eq!(prince.invocations(), 0);
}
