//! Smoke tests: the fastest "is it alive and wired up" checks.

mod common;

use common::*;

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
