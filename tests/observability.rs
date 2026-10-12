//! `/health` and the opt-in Prometheus `/metrics` endpoint.

mod common;

use common::*;
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::OnceLock;

/// The Prometheus recorder is process-global: install it once for this test binary.
fn recorder() -> PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| pdf_engine::telemetry::install_metrics_recorder().unwrap())
        .clone()
}

async fn get(app: &TestApp, path: &str, token: Option<&str>) -> reqwest::Response {
    let mut req = app.client.get(app.url(path));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await.unwrap()
}

#[tokio::test]
async fn health_needs_no_auth_and_reports_version() {
    let app = TestApp::builder().spawn().await;

    let resp = get(&app, "/health", None).await;

    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn metrics_endpoint_is_absent_unless_enabled() {
    let app = TestApp::builder().spawn().await;

    assert_eq!(get(&app, "/metrics", None).await.status(), 404);
}

#[tokio::test]
async fn metrics_report_renders_requests_and_permits() {
    let app = TestApp::builder().metrics(recorder(), None).spawn().await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 200);
    assert_eq!(app.to_bytes(form("{% broken")).await.status(), 400);
    let resp = get(&app, "/metrics", None).await;

    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    for needle in [
        "pdf_renders_total{outcome=\"ok\"}",
        "pdf_render_seconds{quantile=\"0.99\"}",
        "pdf_template_seconds{quantile=\"0.5\"}",
        "pdf_render_permits_in_use",
        "pdf_render_permits_total",
        "http_requests_total{route=\"/api/to-bytes\",method=\"POST\",status=\"200\"}",
        "http_requests_total{route=\"/api/to-bytes\",method=\"POST\",status=\"400\"}",
        "http_request_duration_seconds",
    ] {
        assert!(text.contains(needle), "missing {needle} in:\n{text}");
    }
}

#[tokio::test]
async fn metrics_token_is_enforced_when_set() {
    let app = TestApp::builder()
        .metrics(recorder(), Some("scrape-secret"))
        .spawn()
        .await;

    assert_eq!(get(&app, "/metrics", None).await.status(), 401);
    assert_eq!(get(&app, "/metrics", Some("wrong")).await.status(), 401);
    assert_eq!(
        get(&app, "/metrics", Some("scrape-secret")).await.status(),
        200
    );
}

#[tokio::test]
async fn s3_uploads_are_counted() {
    let app = TestApp::builder()
        .s3_mode(S3Mode::Status(500))
        .metrics(recorder(), None)
        .spawn()
        .await;

    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 500);
    let text = get(&app, "/metrics", None).await.text().await.unwrap();

    assert!(
        text.contains("pdf_s3_uploads_total{outcome=\"error\"}"),
        "{text}"
    );
}

#[tokio::test]
async fn font_reload_requires_auth() {
    let app = TestApp::builder().spawn().await;

    let anon = app
        .client
        .post(app.url("/api/fonts/reload"))
        .send()
        .await
        .unwrap();
    let authed = app
        .client
        .post(app.url("/api/fonts/reload"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();

    assert_eq!(anon.status(), 401);
    assert_eq!(authed.status(), 200);
    let json: serde_json::Value = authed.json().await.unwrap();
    assert_eq!(json["status"], "reloaded");
    assert_healthy(&app).await;
}
