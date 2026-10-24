//! Every error the HTTP API sends has a stable `code`, a readable `error` message and a status,
//! and server-side details never leak. (IPC sends the same objects; see tests/ipc.rs.)

mod common;

use common::*;
use serde_json::Value;

async fn error_of(resp: reqwest::Response) -> (u16, Value) {
    let status = resp.status().as_u16();
    (status, resp.json().await.expect("error body must be JSON"))
}

#[tokio::test]
async fn errors_carry_code_message_status_and_hint() {
    let app = TestApp::builder().spawn().await;

    let anon = app
        .client
        .post(app.url("/api/to-bytes"))
        .send()
        .await
        .unwrap();
    let (status, json) = error_of(anon).await;
    assert_eq!((status, json["code"].as_str()), (401, Some("UNAUTHORIZED")));
    assert_eq!(json["status"], 401);
    assert!(json["hint"].as_str().unwrap().contains("Bearer"));

    let (status, json) = error_of(
        app.to_bytes(reqwest::multipart::Form::new().text("data", "{}"))
            .await,
    )
    .await;
    assert_eq!(
        (status, json["code"].as_str()),
        (400, Some("MISSING_FIELD"))
    );
    assert_eq!(json["error"], "Missing required field: template");

    let bad_page = form("<p>x</p>").text("width", "wide");
    let (status, json) = error_of(app.to_bytes(bad_page).await).await;
    assert_eq!(
        (status, json["code"].as_str()),
        (400, Some("INVALID_FIELD"))
    );
    assert!(json["error"].as_str().unwrap().contains("width/height"));
}

#[tokio::test]
async fn template_errors_point_at_the_line() {
    let app = TestApp::builder().spawn().await;

    let (status, json) = error_of(app.to_bytes(form("<p>fine</p>\n<p>{% if %}</p>\n")).await).await;
    assert_eq!(
        (status, json["code"].as_str()),
        (400, Some("TEMPLATE_ERROR"))
    );
    assert_eq!(json["line"], 2);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .starts_with("Template rendering failed")
    );
    assert!(json["hint"].as_str().unwrap().contains("template syntax"));
}

#[tokio::test]
async fn runaway_templates_are_refused_with_a_code() {
    let app = TestApp::builder().spawn().await;

    let nested = "{% for a in range(100000) %}{% for b in range(100000) %}{% endfor %}{% endfor %}";
    let (status, json) = error_of(app.to_bytes(form(nested)).await).await;
    assert_eq!(
        (status, json["code"].as_str()),
        (400, Some("TEMPLATE_TOO_EXPENSIVE"))
    );
}

#[tokio::test]
async fn storage_failures_have_a_code_but_hide_details() {
    let app = TestApp::builder()
        .s3_mode(S3Mode::Status(500))
        .spawn()
        .await;

    let (status, json) = error_of(app.to_s3(form("<p>x</p>")).await).await;
    assert_eq!(
        (status, json["code"].as_str()),
        (500, Some("STORAGE_ERROR"))
    );
    assert_eq!(json["error"], "Internal server error");
    assert!(!json.to_string().contains("127.0.0.1"), "{json}");
}
