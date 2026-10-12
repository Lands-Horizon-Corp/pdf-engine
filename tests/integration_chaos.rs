//! Integration chaos: the full render → upload → presign pipeline with S3 failing in every way
//! we can produce. Errors must be clean, bounded in time, and must never report success for an
//! object that wasn't stored.

mod common;

use common::*;
use std::{
    collections::HashSet,
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

#[tokio::test]
async fn s3_returns_500() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::Status(500))
        .spawn()
        .await;

    assert_generic_500(app.to_s3(form("<p>x</p>")).await).await;
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn s3_access_denied() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::Status(403))
        .spawn()
        .await;

    assert_generic_500(app.to_s3(form("<p>x</p>")).await).await;
}

#[tokio::test]
async fn s3_unreachable_fails_fast() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_endpoint(format!("http://{}", closed_port()))
        .spawn()
        .await;

    let start = Instant::now();
    assert_generic_500(app.to_s3(form("<p>x</p>")).await).await;
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "took {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn s3_drops_connections() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_endpoint(format!("http://{}", start_connection_dropper()))
        .spawn()
        .await;

    assert_generic_500(app.to_s3(form("<p>x</p>")).await).await;
    assert_healthy(&app).await;
}

#[tokio::test]
async fn s3_hangs_hits_timeout() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::Hang)
        .timeout(Duration::from_secs(2))
        .spawn()
        .await;

    let start = Instant::now();
    let resp = app.to_s3(form("<p>x</p>")).await;

    assert_eq!(resp.status(), 408);
    assert!(start.elapsed() < Duration::from_secs(5));
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn s3_outage_does_not_affect_to_bytes() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::Status(503))
        .spawn()
        .await;

    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 500);
    assert_eq!(app.to_bytes(form("<p>x</p>")).await.status(), 200);
}

#[tokio::test]
async fn s3_recovers_after_outage() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::Status(503))
        .spawn()
        .await;

    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 500);
    app.s3.set_mode(S3Mode::Ok);
    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 200);
}

#[tokio::test]
async fn prince_failure_never_touches_s3() {
    let prince = FakePrince::new("cat > /dev/null; exit 1");
    let app = TestApp::builder(prince.path()).spawn().await;

    assert_eq!(app.to_s3(form("<p>x</p>")).await.status(), 500);
    assert_eq!(app.s3.request_count(), 0);
}

/// Half the uploads fail. Every 200 must point at an object that really landed in S3, and every
/// stored object must belong to exactly one successful response.
#[tokio::test(flavor = "multi_thread")]
async fn flaky_s3_never_reports_phantom_success() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::FailEvery(2))
        .permits(8)
        .spawn()
        .await;

    let futs = (0..40).map(|_| app.to_s3(form("<p>x</p>")));
    let mut ok_keys = HashSet::new();
    for resp in futures_util::future::join_all(futs).await {
        match resp.status().as_u16() {
            200 => {
                let json: serde_json::Value = resp.json().await.unwrap();
                assert!(ok_keys.insert(json["storage_key"].as_str().unwrap().to_string()));
            }
            500 => {}
            s => panic!("unexpected status {s}"),
        }
    }
    let stored: HashSet<String> = app
        .s3
        .puts()
        .into_iter()
        .map(|p| {
            p.path
                .trim_start_matches(&format!("/{BUCKET}/"))
                .to_string()
        })
        .collect();

    assert!(!ok_keys.is_empty(), "nothing succeeded");
    assert_eq!(ok_keys, stored);
    app.assert_permits_restored().await;
}

/// Prince and S3 both flaky at the same time.
#[tokio::test(flavor = "multi_thread")]
async fn prince_and_s3_both_flaky() {
    let prince = FakePrince::new(
        r#"input=$(cat)
case "$input" in *FAIL*) exit 1;; esac
cat "$D/fixture.pdf""#,
    );
    let app = TestApp::builder(prince.path())
        .s3_mode(S3Mode::FailEvery(3))
        .permits(4)
        .spawn()
        .await;

    let futs = (0..45).map(|i| {
        let t = if i % 5 == 0 {
            "<p>FAIL</p>"
        } else {
            "<p>ok</p>"
        };
        let app = &app;
        async move { (i, app.to_s3(form(t)).await.status().as_u16()) }
    });
    for (i, status) in futures_util::future::join_all(futs).await {
        if i % 5 == 0 {
            assert_eq!(status, 500, "prince failure {i}");
        } else {
            assert!(status == 200 || status == 500, "request {i} -> {status}");
        }
    }
    app.assert_permits_restored().await;
    app.s3.set_mode(S3Mode::Ok);
    assert_eq!(app.to_s3(form("<p>ok</p>")).await.status(), 200);
}

#[tokio::test]
async fn real_prince_with_flaky_s3() {
    require_prince!();
    let app = TestApp::builder("prince")
        .s3_mode(S3Mode::FailEvery(2))
        .spawn()
        .await;

    let a = app.to_s3(form("<h1>a</h1>")).await.status();
    let b = app.to_s3(form("<h1>b</h1>")).await.status();

    assert_eq!((a.as_u16(), b.as_u16()), (200, 500));
    assert_eq!(page_count(&app.s3.puts()[0].body), 1);
}
