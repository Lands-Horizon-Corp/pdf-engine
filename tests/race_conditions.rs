//! Sync & race conditions: concurrent requests stay isolated, shared resources are bounded,
//! and nothing collides.

mod common;

use common::*;
use futures_util::future::join_all;
use std::{
    collections::HashSet,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

/// Storage keys must be unique even when many uploads start in the same instant.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_uploads_get_unique_keys() {
    let app = TestApp::builder().permits(16).spawn().await;
    let n = 150;

    let resps = join_all((0..n).map(|_| app.to_s3(form("<p>x</p>")))).await;
    let mut keys = HashSet::new();
    for r in resps {
        assert_eq!(r.status(), 200);
        let json: serde_json::Value = r.json().await.unwrap();
        keys.insert(json["storage_key"].as_str().unwrap().to_string());
    }
    let stored: HashSet<_> = app.s3.puts().into_iter().map(|p| p.path).collect();

    assert_eq!(
        keys.len(),
        n,
        "two responses share a storage key (object overwritten)"
    );
    assert_eq!(
        stored.len(),
        n,
        "fewer distinct objects stored than successful uploads"
    );
}

/// The semaphore caps concurrent renders.
#[tokio::test(flavor = "multi_thread")]
async fn render_concurrency_never_exceeds_permits() {
    let permits = 3;
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Ok(Duration::from_millis(300))))
        .permits(permits)
        .spawn()
        .await;

    let resps = join_all((0..15).map(|_| app.to_bytes(form("<p>x</p>")))).await;
    assert!(resps.iter().all(|r| r.status() == 200));

    let max = app.spy.max_active.load(Ordering::SeqCst);
    assert!(
        max <= permits,
        "saw {max} concurrent renders with {permits} permits"
    );
    assert!(
        max >= 2,
        "requests were not rendered in parallel at all (max {max})"
    );
    app.assert_permits_restored().await;
}

/// Concurrent encrypted renders with different passwords don't swap or share keys.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_passwords_stay_isolated() {
    let app = TestApp::builder().permits(8).spawn().await;

    let futs = (0..30).map(|i| {
        let app = &app;
        async move {
            let pw = format!("pw-{i}");
            let body = app
                .to_bytes(form("<p>x</p>").text("password", pw.clone()))
                .await
                .bytes()
                .await
                .unwrap();
            (i, body)
        }
    });
    for (i, body) in join_all(futs).await {
        assert!(
            load_pdf_with_password(&body, &format!("pw-{i}")).is_ok(),
            "request {i}"
        );
        let other = format!("pw-{}", (i + 1) % 30);
        assert!(
            load_pdf_with_password(&body, &other).is_err(),
            "request {i} opens with {other}"
        );
    }
}

/// Each response belongs to its own request: filenames and rendered bodies never cross over.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_responses_are_not_cross_wired() {
    let app = TestApp::builder().permits(8).spawn().await;

    let futs = (0..40).map(|i| {
        let app = &app;
        async move {
            let r = app
                .to_bytes(
                    form("<p>req-{{ i }}</p>")
                        .text("data", format!(r#"{{"i":{i}}}"#))
                        .text("filename", format!("f-{i}.pdf")),
                )
                .await;
            (
                i,
                r.headers()["content-disposition"]
                    .to_str()
                    .unwrap()
                    .to_string(),
            )
        }
    });
    for (i, cd) in join_all(futs).await {
        assert_eq!(cd, format!("inline; filename=\"f-{i}.pdf\""));
    }
    let rendered: HashSet<String> = app
        .calls()
        .iter()
        .map(|c| {
            c.html
                .split("req-")
                .nth(1)
                .unwrap()
                .split('<')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(rendered, (0..40).map(|i| i.to_string()).collect());
}

/// Good and bad requests interleaved: each gets its own correct answer.
#[tokio::test(flavor = "multi_thread")]
async fn mixed_valid_and_invalid_requests() {
    let app = TestApp::builder().permits(2).spawn().await;

    let futs = (0..60).map(|i| {
        let app = &app;
        async move {
            let r = match i % 3 {
                0 => app.to_bytes(form("<p>ok</p>")).await,
                1 => app.to_bytes(form("{% broken")).await,
                _ => app
                    .client
                    .post(app.url("/api/to-bytes"))
                    .bearer_auth("nope")
                    .multipart(form("<p>x</p>"))
                    .send()
                    .await
                    .unwrap(),
            };
            (i, r.status().as_u16())
        }
    });
    for (i, status) in join_all(futs).await {
        assert_eq!(status, [200, 400, 401][i % 3], "request {i}");
    }
    assert_eq!(app.renders(), 20);
    app.assert_permits_restored().await;
}

/// A queue of requests behind a single permit drains in full without deadlock.
#[tokio::test(flavor = "multi_thread")]
async fn single_permit_queue_drains() {
    let app = TestApp::builder().permits(1).spawn().await;

    let resps = tokio::time::timeout(
        Duration::from_secs(30),
        join_all((0..25).map(|_| app.to_bytes(form("<p>x</p>")))),
    )
    .await
    .expect("deadlock: queue behind one permit did not drain");

    assert!(resps.iter().all(|r| r.status() == 200));
    app.assert_permits_restored().await;
}

/// Client cancellations in the middle of a burst don't wedge the semaphore.
#[tokio::test(flavor = "multi_thread")]
async fn cancellation_storm_releases_permits() {
    let app = TestApp::builder()
        .renderer(Arc::new(FakeRenderer::Ok(Duration::from_millis(500))))
        .permits(2)
        .timeout(Duration::from_secs(10))
        .spawn()
        .await;
    let impatient = reqwest::Client::builder()
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let futs = (0..20).map(|_| {
        impatient
            .post(app.url("/api/to-bytes"))
            .bearer_auth(TOKEN)
            .multipart(form("<p>x</p>"))
            .send()
    });
    for r in join_all(futs).await {
        assert!(r.is_err(), "client should have given up");
    }

    // Whatever the server was doing for abandoned clients must finish or be dropped.
    let restored = eventually(Duration::from_secs(15), || app.available_permits() == 2).await;
    assert!(restored, "permits stuck at {}", app.available_permits());
    assert_healthy(&app).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn real_concurrent_documents_keep_their_own_content() {
    let app = TestApp::builder().permits(4).spawn().await;

    let futs = (0..16).map(|i| {
        let app = &app;
        async move {
            let t =
                format!("<html><head><title>doc-{i}</title></head><body><p>{i}</p></body></html>");
            (i, app.to_bytes(form(&t)).await.bytes().await.unwrap())
        }
    });
    for (i, body) in join_all(futs).await {
        assert_eq!(
            info_title(&load_pdf(&body)).as_deref(),
            Some(&*format!("doc-{i}"))
        );
    }
}
