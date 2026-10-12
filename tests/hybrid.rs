//! The Fulgur-first / Prince-fallback hybrid: routing, overrides, fallback on failure,
//! timeouts, zero-copy output and response compression.

mod common;

use common::*;
use pdf_engine::models::EnginePref;
use std::{
    io::Read,
    sync::{Arc, atomic::AtomicUsize, atomic::Ordering},
    time::Duration,
};

fn engine_of(resp: &reqwest::Response) -> String {
    resp.headers()["x-pdf-engine"].to_str().unwrap().to_string()
}

async fn auto_app(prince: &FakePrince) -> TestApp {
    TestApp::builder(prince.path())
        .default_engine(EnginePref::Auto)
        .spawn()
        .await
}

// ---- routing ----

#[tokio::test]
async fn simple_document_renders_in_process() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app.to_bytes(form("<h1>Hello</h1><p>world</p>")).await;

    assert_eq!(resp.status(), 200);
    assert_eq!(engine_of(&resp), "fulgur");
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
    assert_eq!(prince.invocations(), 0, "Prince must not be spawned");
}

#[tokio::test]
async fn unsupported_css_routes_to_prince() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app
        .to_bytes(form("<div style=\"float:left\">x</div>"))
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(engine_of(&resp), "prince");
    assert_eq!(prince.invocations(), 1);
}

#[tokio::test]
async fn css_produced_by_template_data_is_routed_too() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app
        .to_bytes(form("<div style=\"{{ s }}\">x</div>").text("data", r#"{"s":"z-index:3"}"#))
        .await;

    assert_eq!(engine_of(&resp), "prince");
}

#[tokio::test]
async fn engine_field_overrides_routing() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let forced_prince = app
        .to_bytes(form("<p>x</p>").text("engine", "prince"))
        .await;
    let forced_fulgur = app
        .to_bytes(form("<div style=\"float:left\">x</div>").text("engine", "FULGUR"))
        .await;

    assert_eq!(engine_of(&forced_prince), "prince");
    assert_eq!(engine_of(&forced_fulgur), "fulgur");
}

#[tokio::test]
async fn invalid_engine_field_is_400() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app
        .to_bytes(form("<p>x</p>").text("engine", "chrome"))
        .await;

    assert_eq!(resp.status(), 400);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert!(json["error"].as_str().unwrap().contains("engine"));
}

#[tokio::test]
async fn page_size_only_prince_understands_routes_to_prince() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let auto = app
        .to_bytes(form("<p>x</p>").text("width", "A4").text("height", ""))
        .await;
    let forced = app
        .to_bytes(
            form("<p>x</p>")
                .text("width", "A4")
                .text("height", "")
                .text("engine", "fulgur"),
        )
        .await;

    assert_eq!(engine_of(&auto), "prince");
    assert_eq!(forced.status(), 400);
}

#[tokio::test]
async fn multi_page_table_with_thead_falls_back_to_prince() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;
    let rows: String = (0..200)
        .map(|i| format!("<tr><td>row {i}</td></tr>"))
        .collect();

    let long = app
        .to_bytes(form(&format!(
            "<table><thead><tr><th>H</th></tr></thead><tbody>{rows}</tbody></table>"
        )))
        .await;
    let short = app
        .to_bytes(form(
            "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>",
        ))
        .await;

    assert_eq!(
        engine_of(&long),
        "prince",
        "Fulgur doesn't repeat <thead> across pages"
    );
    assert_eq!(engine_of(&short), "fulgur");
}

// ---- failure handling ----

#[tokio::test]
async fn fulgur_error_falls_back_to_prince() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .default_engine(EnginePref::Auto)
        .fulgur(Arc::new(FakeFulgur::Fails))
        .spawn()
        .await;

    let auto = app.to_bytes(form("<p>x</p>")).await;
    let forced = app
        .to_bytes(form("<p>x</p>").text("engine", "fulgur"))
        .await;

    assert_eq!(
        (auto.status().as_u16(), engine_of(&auto)),
        (200, "prince".into())
    );
    assert_eq!(forced.status(), 500);
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn fulgur_panic_is_contained_and_falls_back() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .default_engine(EnginePref::Auto)
        .fulgur(Arc::new(FakeFulgur::Panics))
        .spawn()
        .await;

    let auto = app.to_bytes(form("<p>x</p>")).await;
    let forced = app
        .to_bytes(form("<p>x</p>").text("engine", "fulgur"))
        .await;

    assert_eq!(
        (auto.status().as_u16(), engine_of(&auto)),
        (200, "prince".into())
    );
    assert_eq!(forced.status(), 500);
    let json: serde_json::Value = forced.json().await.unwrap();
    assert_eq!(json["error"], "Internal server error");
    app.assert_permits_restored().await;
    assert_healthy(&app).await;
}

/// A runaway in-process render can't be killed. The request still times out on schedule,
/// and its permit stays taken until the work really ends, so runaways can't pile up past
/// the concurrency limit.
#[tokio::test]
async fn slow_fulgur_times_out_and_holds_its_permit_until_done() {
    let prince = FakePrince::ok();
    let finished = Arc::new(AtomicUsize::new(0));
    let app = TestApp::builder(prince.path())
        .default_engine(EnginePref::Fulgur)
        .fulgur(Arc::new(FakeFulgur::Slow(
            Duration::from_secs(3),
            finished.clone(),
        )))
        .permits(2)
        .timeout(Duration::from_secs(1))
        .spawn()
        .await;

    let resp = app.to_bytes(form("<p>x</p>")).await;

    assert_eq!(resp.status(), 408);
    assert_eq!(
        app.available_permits(),
        1,
        "permit must stay held by the running render"
    );
    let done = eventually(Duration::from_secs(5), || {
        finished.load(Ordering::SeqCst) == 1
    })
    .await;
    assert!(done);
    app.assert_permits_restored().await;
}

#[tokio::test]
async fn without_prince_everything_renders_in_process() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path())
        .default_engine(EnginePref::Auto)
        .without_prince()
        .spawn()
        .await;

    let floated = app
        .to_bytes(form("<div style=\"float:left\">x</div>"))
        .await;
    let forced = app
        .to_bytes(form("<p>x</p>").text("engine", "prince"))
        .await;

    assert_eq!(
        (floated.status().as_u16(), engine_of(&floated)),
        (200, "fulgur".into())
    );
    assert_eq!(forced.status(), 400);
}

// ---- Fulgur output ----

#[tokio::test]
async fn fulgur_page_sizes() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let letter = app.to_bytes(form("<p>x</p>")).await.bytes().await.unwrap();
    let a4l = app
        .to_bytes(
            form("<p>x</p>")
                .text("width", "210mm")
                .text("height", "297mm")
                .text("orientation", "landscape"),
        )
        .await
        .bytes()
        .await
        .unwrap();

    assert_eq!(media_boxes(&load_pdf(&letter)), vec![[0, 0, 612, 792]]);
    assert_eq!(media_boxes(&load_pdf(&a4l))[0][2..], [842, 595]);
}

#[tokio::test]
async fn fulgur_password_encryption() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app
        .to_bytes(form("<p>secret</p>").text("password", "pw-1"))
        .await;
    assert_eq!(engine_of(&resp), "fulgur");
    let body = resp.bytes().await.unwrap();

    assert!(load_pdf_with_password(&body, "pw-1").is_ok());
    assert!(load_pdf_with_password(&body, "wrong").is_err());
}

#[tokio::test]
async fn fulgur_sets_document_title() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let body = app
        .to_bytes(form(
            "<html><head><title>Invoice 42</title></head><body>x</body></html>",
        ))
        .await
        .bytes()
        .await
        .unwrap();

    assert_eq!(info_title(&load_pdf(&body)).as_deref(), Some("Invoice 42"));
}

#[tokio::test]
async fn fulgur_renders_scripts_outside_the_bundled_fonts() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app
        .to_bytes(form("<p>你好世界 · こんにちは · مرحبا · 🚀</p>"))
        .await;

    assert_eq!(
        (resp.status().as_u16(), engine_of(&resp)),
        (200, "fulgur".into())
    );
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

#[tokio::test]
async fn fulgur_is_deterministic() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;
    let t = "<h1>Same</h1><p>input, same bytes</p>";

    let a = app.to_bytes(form(t)).await.bytes().await.unwrap();
    let b = app.to_bytes(form(t)).await.bytes().await.unwrap();

    assert_eq!(a, b);
}

#[tokio::test]
async fn fulgur_to_s3() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    let resp = app.to_s3(form("<h1>S3</h1>")).await;
    assert_eq!(engine_of(&resp), "fulgur");
    let json: serde_json::Value = resp.json().await.unwrap();

    let put = &app.s3.puts()[0];
    assert_eq!(put.body.len() as i64, json["file_size"].as_i64().unwrap());
    assert_eq!(page_count(&put.body), 1);
}

#[tokio::test]
async fn fulgur_cannot_read_local_files() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;
    let dir = tempfile::tempdir().unwrap();
    let png = write_file(dir.path(), "canary.png", TINY_PNG);

    let resp = app
        .to_bytes(form(&format!(
            r#"<img src="file://{p}"><img src="{p}">"#,
            p = png.display()
        )))
        .await;

    assert_eq!(engine_of(&resp), "fulgur");
    assert!(!has_image(&load_pdf(&resp.bytes().await.unwrap())));
}

#[tokio::test]
async fn fulgur_makes_no_network_requests() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let a = listener.local_addr().unwrap();

    let t =
        format!(r#"<link rel="stylesheet" href="http://{a}/s.css"><img src="http://{a}/x.png">"#);
    let resp = app.to_bytes(form(&t)).await;

    assert_eq!(engine_of(&resp), "fulgur");
    assert!(
        listener.accept().is_err(),
        "Fulgur made an outbound connection"
    );
}

// ---- compression ----

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
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

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
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

    for enc in ["br", "zstd"] {
        let resp = request_with_encoding(&app, Some(enc)).await;
        assert_eq!(resp.headers()["content-encoding"], enc);
    }
}

#[tokio::test]
async fn s3_json_response_is_compressed_when_asked() {
    let prince = FakePrince::ok();
    let app = auto_app(&prince).await;

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
