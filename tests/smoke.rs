//! Smoke tests: the fastest "is it alive and wired up" checks.
//!
//! In-process checks run with plain `cargo test`. The `live_*` checks hit a deployed instance
//! and are ignored by default:
//!
//!   SMOKE_URL=https://pdf.example.com API_BEARER_TOKEN=... \
//!     cargo test --test smoke -- --ignored live_

mod common;

use common::*;
use reqwest::Client;
use std::time::Duration;

#[tokio::test]
async fn smoke_in_process() {
    let app = TestApp::builder().spawn().await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 200);
    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 200);
    let anon = app
        .client
        .post(app.url("/api/to-bytes"))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);
}

/// Mirrors the startup warm-up: the renderer handles mixed scripts.
#[tokio::test]
async fn smoke_warmup_content() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form("<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>"))
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

// ---- live deployment ----

fn live() -> (Client, String, String) {
    dotenvy::dotenv().ok();
    let base = std::env::var("SMOKE_URL").unwrap_or_else(|_| "http://localhost:6767".into());
    let token = std::env::var("API_BEARER_TOKEN").expect("API_BEARER_TOKEN must be set");
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    (client, base.trim_end_matches('/').to_string(), token)
}

#[tokio::test]
#[ignore]
async fn live_rejects_anonymous() {
    let (c, base, _) = live();
    let r = c
        .post(format!("{base}/api/to-bytes"))
        .multipart(form("<p>x</p>"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
#[ignore]
async fn live_to_bytes() {
    let (c, base, token) = live();
    let r = c
        .post(format!("{base}/api/to-bytes"))
        .bearer_auth(token)
        .multipart(form("<h1>smoke {{ n }}</h1>").text("data", r#"{"n":1}"#))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "application/pdf");
    assert_eq!(page_count(&r.bytes().await.unwrap()), 1);
}

#[tokio::test]
#[ignore]
async fn live_to_s3_url_is_downloadable() {
    let (c, base, token) = live();
    let r = c
        .post(format!("{base}/api/to-s3"))
        .bearer_auth(token)
        .multipart(form("<h1>smoke</h1>"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let json: serde_json::Value = r.json().await.unwrap();

    let pdf = c.get(json["url"].as_str().unwrap()).send().await.unwrap();
    assert_eq!(pdf.status(), 200, "presigned URL not downloadable");
    let bytes = pdf.bytes().await.unwrap();
    assert_eq!(bytes.len() as i64, json["file_size"].as_i64().unwrap());
    assert!(is_pdf(&bytes));
}
