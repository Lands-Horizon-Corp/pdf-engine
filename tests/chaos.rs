//! Chaos: the renderer misbehaves in every way we can produce. The engine must answer with a
//! clean error (never hang, never leak internals), give its permit back, and keep serving.

mod common;

use common::*;
use futures_util::future::join_all;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

async fn assert_generic_500(resp: reqwest::Response) {
    assert_eq!(resp.status(), 500);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        json["error"], "Internal server error",
        "internals must not leak"
    );
}

async fn app_with(renderer: FakeRenderer) -> TestApp {
    TestApp::builder()
        .renderer(Arc::new(renderer))
        .spawn()
        .await
}

#[tokio::test]
async fn renderer_error_is_a_clean_500() {
    let app = app_with(FakeRenderer::Fails).await;

    assert_generic_500(app.to_bytes(form("<p>x</p>")).await).await;
    assert_generic_500(app.to_s3(form("<p>x</p>")).await).await;
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn renderer_panic_is_contained() {
    let app = app_with(FakeRenderer::Panics).await;

    for _ in 0..5 {
        assert_generic_500(app.to_bytes(form("<p>x</p>")).await).await;
    }
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn garbage_output_cannot_be_encrypted_and_fails_cleanly() {
    let app = app_with(FakeRenderer::Garbage).await;

    let resp = app.to_bytes(form("<p>x</p>").text("password", "pw")).await;

    assert_generic_500(resp).await;
}

#[tokio::test]
async fn slow_render_within_budget_succeeds() {
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Ok(Duration::from_secs(1))))
        .timeout(Duration::from_secs(5))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 200);
}

/// A render can't be killed once it's running. The request still times out on schedule, and
/// its permit stays taken until the work really ends, so runaways can't pile up past the
/// concurrency limit.
#[tokio::test]
async fn hung_render_times_out_and_holds_its_permit_until_done() {
    let finished = Arc::new(AtomicUsize::new(0));
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Slow(
            Duration::from_secs(3),
            finished.clone(),
        )))
        .permits(2)
        .timeout(Duration::from_secs(1))
        .spawn()
        .await;

    let start = Instant::now();
    let resp = app.to_bytes(form("<p>x</p>")).await;

    assert_eq!(resp.status(), 408);
    assert!(start.elapsed() < Duration::from_secs(2));
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().unwrap().contains("timed out"));
    assert_eq!(
        app.available_permits(),
        1,
        "running render keeps its permit"
    );
    assert!(
        eventually(Duration::from_secs(5), || finished.load(Ordering::SeqCst)
            == 1)
        .await
    );
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn hung_renders_cannot_exceed_the_concurrency_limit() {
    let finished = Arc::new(AtomicUsize::new(0));
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Slow(
            Duration::from_secs(2),
            finished.clone(),
        )))
        .permits(2)
        .timeout(Duration::from_millis(300))
        .spawn()
        .await;

    let resps = join_all((0..8).map(|_| app.to_bytes(form("<p>x</p>")))).await;

    assert!(resps.iter().all(|r| r.status() == 408));
    assert!(app.spy.max_active.load(Ordering::SeqCst) <= 2);
    assert!(
        app.renders() <= 2,
        "queued requests timed out before rendering"
    );
}

/// The renderer fails for some requests under load: each request gets the right answer for
/// *its* input, nothing else breaks, and all permits come back.
#[tokio::test(flavor = "multi_thread")]
async fn flaky_renderer_under_load() {
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::FailsOn("FAIL")))
        .permits(3)
        .spawn()
        .await;

    let futs = (0..60).map(|i| {
        let body = if i % 3 == 0 {
            format!("<p>FAIL {i}</p>")
        } else {
            format!("<p>ok {i}</p>")
        };
        let app = &app;
        async move { (i, app.to_bytes(form(&body)).await) }
    });
    for (i, resp) in join_all(futs).await {
        let expected = if i % 3 == 0 { 500 } else { 200 };
        assert_eq!(resp.status(), expected, "request {i}");
        if expected == 200 {
            assert!(is_pdf(&resp.bytes().await.unwrap()));
        }
    }
    app.assert_permits_restored().await;
    assert_healthy(&app).await;
}
