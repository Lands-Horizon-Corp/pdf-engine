//! Browsers can call the HTTP API from another origin when `CORS_ALLOWED_ORIGINS` allows it.

mod common;

use common::*;
use reqwest::Method;

const ORIGIN: &str = "https://app.example.com";

async fn preflight(app: &TestApp, origin: &str) -> reqwest::Response {
    app.client
        .request(Method::OPTIONS, app.url("/api/to-bytes"))
        .header("origin", origin)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn an_allowed_origin_passes_the_preflight_without_a_token() {
    let app = TestApp::builder().cors(&[ORIGIN]).spawn().await;

    let resp = preflight(&app, ORIGIN).await;
    assert!(resp.status().is_success(), "{}", resp.status());
    assert_eq!(resp.headers()["access-control-allow-origin"], ORIGIN);
    let headers = resp.headers()["access-control-allow-headers"]
        .to_str()
        .unwrap()
        .to_lowercase();
    assert!(headers.contains("authorization"), "{headers}");
}

#[tokio::test]
async fn actual_requests_get_the_origin_header_and_can_read_the_filename() {
    let app = TestApp::builder().cors(&[ORIGIN]).spawn().await;

    let resp = app
        .client
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .header("origin", ORIGIN)
        .multipart(form("<p>x</p>"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["access-control-allow-origin"], ORIGIN);
    assert!(
        resp.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .to_lowercase()
            .contains("content-disposition")
    );
}

#[tokio::test]
async fn errors_are_readable_cross_origin_too() {
    let app = TestApp::builder().cors(&[ORIGIN]).spawn().await;

    let resp = app
        .client
        .post(app.url("/api/to-bytes"))
        .header("origin", ORIGIN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert_eq!(resp.headers()["access-control-allow-origin"], ORIGIN);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap()["code"],
        "UNAUTHORIZED"
    );
}

#[tokio::test]
async fn other_origins_and_the_default_get_no_cors_headers() {
    let app = TestApp::builder().cors(&[ORIGIN]).spawn().await;
    let denied = preflight(&app, "https://evil.example.com").await;
    assert!(
        denied
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );

    let off = TestApp::builder().spawn().await;
    let resp = off
        .client
        .post(off.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .header("origin", ORIGIN)
        .multipart(form("<p>x</p>"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn a_wildcard_allows_any_origin() {
    let app = TestApp::builder().cors(&["*"]).spawn().await;
    let resp = preflight(&app, "https://anything.example.org").await;
    assert!(resp.status().is_success());
    assert!(resp.headers().get("access-control-allow-origin").is_some());
}
