//! Response compression (gzip / brotli / zstd at level 6, negotiated via `Accept-Encoding`).

mod common;

use common::*;
use std::io::Read;

async fn request_with_encoding(app: &TestApp, encoding: Option<&str>) -> reqwest::Response {
    let mut req = app
        .client
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .multipart(form("<h1>Compress me</h1><p>hello</p>"));
    if let Some(e) = encoding {
        req = req.header("accept-encoding", e);
    }
    req.send().await.unwrap()
}

#[tokio::test]
async fn gzip_response_decodes_to_the_identical_pdf() {
    let app = TestApp::builder().spawn().await;

    let plain = request_with_encoding(&app, None).await;
    assert!(plain.headers().get("content-encoding").is_none());
    let plain = plain.bytes().await.unwrap();

    let gz = request_with_encoding(&app, Some("gzip")).await;
    assert_eq!(gz.headers()["content-encoding"], "gzip");
    assert_eq!(gz.headers()["content-type"], "application/pdf");
    let compressed = gz.bytes().await.unwrap();
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(&compressed[..])
        .read_to_end(&mut decoded)
        .unwrap();

    assert_eq!(decoded, plain);
    assert!(
        compressed.len() < plain.len(),
        "{} >= {}",
        compressed.len(),
        plain.len()
    );
}

#[tokio::test]
async fn brotli_and_zstd_are_negotiated() {
    let app = TestApp::builder().spawn().await;

    for enc in ["br", "zstd"] {
        let resp = request_with_encoding(&app, Some(enc)).await;
        assert_eq!(resp.headers()["content-encoding"], enc);
    }
}

#[tokio::test]
async fn s3_json_response_is_compressed_when_asked() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .client
        .post(app.url("/api/to-s3"))
        .bearer_auth(TOKEN)
        .header("accept-encoding", "gzip")
        .multipart(form("<p>x</p>"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-encoding"], "gzip");
    let mut json = String::new();
    flate2::read::GzDecoder::new(&resp.bytes().await.unwrap()[..])
        .read_to_string(&mut json)
        .unwrap();
    assert!(json.contains("\"status\":\"success\""));
}
