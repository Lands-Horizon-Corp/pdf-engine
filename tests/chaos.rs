//! Chaos: the Prince process misbehaves in every way we can think of. The engine must answer
//! with a clean error (never hang, never leak internals), give its permit back, and keep serving.

mod common;

use common::*;
use std::time::{Duration, Instant};

async fn assert_generic_500(resp: reqwest::Response) {
    assert_eq!(resp.status(), 500);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        json["error"], "Internal server error",
        "internals must not leak"
    );
}

async fn run_broken_prince(body: &str) {
    let prince = FakePrince::new(body);
    let app = TestApp::builder(prince.path()).spawn().await;
    let resp = app.to_bytes(form("<p>x</p>")).await;
    assert_generic_500(resp).await;
    app.assert_permits_restored().await;
    // Prince is still broken, so a second request must fail the same way rather than hang.
    assert_generic_500(app.to_bytes(form("<p>again</p>")).await).await;
}

#[tokio::test]
async fn prince_exits_nonzero() {
    run_broken_prince("cat > /dev/null; echo 'boom' >&2; exit 3").await;
}

#[tokio::test]
async fn prince_exits_zero_with_no_output() {
    run_broken_prince("cat > /dev/null; exit 0").await;
}

#[tokio::test]
async fn prince_emits_garbage() {
    run_broken_prince("cat > /dev/null; head -c 4096 /dev/urandom").await;
}

#[tokio::test]
async fn prince_emits_truncated_pdf() {
    run_broken_prince(r#"cat > /dev/null; head -c 300 "$D/fixture.pdf""#).await;
}

#[tokio::test]
async fn prince_emits_html_instead_of_pdf() {
    run_broken_prince("cat").await;
}

#[tokio::test]
async fn prince_is_killed_by_signal() {
    run_broken_prince("kill -9 $$").await;
}

#[tokio::test]
async fn prince_segfaults_midway_through_output() {
    run_broken_prince(r#"cat > /dev/null; head -c 200 "$D/fixture.pdf"; kill -SEGV $$"#).await;
}

#[tokio::test]
async fn prince_binary_missing() {
    let app = TestApp::builder("/nonexistent/prince").spawn().await;
    assert_generic_500(app.to_bytes(form("<p>x</p>")).await).await;
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn prince_binary_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    let bin = write_file(dir.path(), "prince", b"#!/bin/sh\nexit 0\n");
    let app = TestApp::builder(bin.to_str().unwrap()).spawn().await;
    assert_generic_500(app.to_bytes(form("<p>x</p>")).await).await;
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn prince_ignores_stdin_and_exits_early() {
    // Writing to a closed pipe must not panic or SIGPIPE the server; the output is still valid.
    let prince = FakePrince::new(r#"cat "$D/fixture.pdf""#);
    let app = TestApp::builder(prince.path()).spawn().await;

    let big = "<p>filler</p>".repeat(500_000);
    let resp = app.to_bytes(form(&big)).await;

    assert_eq!(resp.status(), 200);
    app.assert_permits_restored().await;
    assert_healthy(&app).await;
}

#[tokio::test]
async fn prince_slow_but_within_budget() {
    let prince = FakePrince::new(r#"cat > /dev/null; sleep 1; cat "$D/fixture.pdf""#);
    let app = TestApp::builder(prince.path())
        .timeout(Duration::from_secs(5))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 200);
}

#[tokio::test]
async fn prince_hangs_forever_hits_timeout() {
    let prince = FakePrince::new("exec sleep 3600");
    let app = TestApp::builder(prince.path())
        .timeout(Duration::from_secs(2))
        .spawn()
        .await;

    let start = Instant::now();
    let resp = app.to_bytes(form("<p>x</p>")).await;

    assert_eq!(resp.status(), 408);
    assert!(start.elapsed() < Duration::from_secs(5));
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().unwrap().contains("timed out"));
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn prince_stalls_after_partial_output_hits_timeout() {
    let prince = FakePrince::new(r#"head -c 100 "$D/fixture.pdf"; exec sleep 3600"#);
    let app = TestApp::builder(prince.path())
        .timeout(Duration::from_secs(2))
        .spawn()
        .await;

    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 408);
    app.assert_permits_restored().await;
}

/// Prince fails for some requests under load: each request gets the right answer for *its*
/// input, nothing else breaks, and all permits come back.
#[tokio::test(flavor = "multi_thread")]
async fn flaky_prince_under_load() {
    let prince = FakePrince::new(
        r#"input=$(cat)
case "$input" in *FAIL*) exit 1;; esac
cat "$D/fixture.pdf""#,
    );
    let app = TestApp::builder(prince.path()).permits(3).spawn().await;

    let futs = (0..60).map(|i| {
        let body = if i % 3 == 0 {
            format!("<p>FAIL {i}</p>")
        } else {
            format!("<p>ok {i}</p>")
        };
        let app = &app;
        async move { (i, app.to_bytes(form(&body)).await) }
    });
    for (i, resp) in futures_util::future::join_all(futs).await {
        let expected = if i % 3 == 0 { 500 } else { 200 };
        assert_eq!(resp.status(), expected, "request {i}");
        if expected == 200 {
            assert!(is_pdf(&resp.bytes().await.unwrap()));
        }
    }
    app.assert_permits_restored().await;
    assert_healthy(&app).await;
}
