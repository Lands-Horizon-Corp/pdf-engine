//! Happy path: well-formed, authorized requests produce correct PDFs.

mod common;

use common::*;
use reqwest::multipart;

// ---- fake Prince: verifies everything the engine does around Prince ----

#[tokio::test]
async fn to_bytes_returns_pdf_with_headers() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app
        .to_bytes(form("<h1>Hello</h1>").text("filename", "invoice-42.pdf"))
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/pdf");
    assert_eq!(
        resp.headers()["content-disposition"],
        "inline; filename=\"invoice-42.pdf\""
    );
    assert!(is_pdf(&resp.bytes().await.unwrap()));
}

#[tokio::test]
async fn blank_lead_page_is_stripped() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let body = app.to_bytes(form("<p>x</p>")).await.bytes().await.unwrap();

    let doc = load_pdf(&body);
    assert_eq!(media_boxes(&doc), vec![CONTENT_PAGE_BOX], "page 1 should be removed");
}

#[tokio::test]
async fn template_is_rendered_with_data_and_escaped_before_prince() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app
        .to_bytes(
            form("<html><body class=\"x\"><h1>Hi {{ name }}</h1>{% for i in items %}<li>{{ i }}</li>{% endfor %}</body></html>")
                .text("data", r#"{"name":"<Zé>","items":["a","b"]}"#),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let stdin = &prince.stdins()[0];
    assert!(
        stdin.starts_with("<html><body class=\"x\"><div style=\"page-break-after: always;"),
        "blank page must follow <body>: {stdin}"
    );
    assert!(stdin.contains("<h1>Hi &lt;Z&#233;&gt;</h1>"), "{stdin}");
    assert!(stdin.contains("<li>a</li><li>b</li>"), "{stdin}");
    assert!(stdin.is_ascii(), "non-ASCII must be entity-encoded");
}

#[tokio::test]
async fn prince_gets_safe_flags_and_default_page_size() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    app.to_bytes(form("<p>x</p>")).await;

    let args = &prince.args()[0];
    for flag in ["--no-network", "--no-javascript", "--silent"] {
        assert!(args.lines().any(|l| l == flag), "missing {flag}: {args}");
    }
    assert!(args.contains("size: 8.5in 11in portrait;"), "{args}");
}

#[tokio::test]
async fn custom_page_size_and_orientation_are_forwarded() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    app.to_bytes(
        form("<p>x</p>")
            .text("width", "210mm")
            .text("height", "297mm")
            .text("orientation", "landscape"),
    )
    .await;

    assert!(prince.args()[0].contains("size: 210mm 297mm landscape;"));
}

#[tokio::test]
async fn password_encrypts_output() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let body = app
        .to_bytes(form("<p>secret</p>").text("password", "hunter2"))
        .await
        .bytes()
        .await
        .unwrap();

    let raw = String::from_utf8_lossy(&body);
    assert!(raw.contains("/Encrypt"), "output should carry an /Encrypt dictionary");
    let doc = load_pdf_with_password(&body, "hunter2").expect("correct password must open it");
    assert_eq!(doc.get_pages().len(), 1);
}

#[tokio::test]
async fn to_s3_uploads_and_returns_presigned_payload() {
    let prince = FakePrince::ok();
    let app = TestApp::builder(prince.path()).spawn().await;

    let resp = app.to_s3(form("<h1>S3</h1>")).await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();

    let key = json["storage_key"].as_str().unwrap();
    assert!(key.starts_with("pdfs/") && key.ends_with(".pdf"), "{key}");
    assert_eq!(json["file_name"], key.trim_start_matches("pdfs/"));
    assert_eq!(json["file_type"], "application/pdf");
    assert_eq!(json["bucket_name"], BUCKET);
    assert_eq!(json["status"], "success");
    assert_eq!(json["progress"], 100);
    let url = json["url"].as_str().unwrap();
    assert!(url.contains(key) && url.contains("X-Amz-Signature="), "{url}");

    let puts = app.s3.puts();
    assert_eq!(puts.len(), 1);
    let put = &puts[0];
    assert_eq!(put.path, format!("/{BUCKET}/{key}"));
    assert_eq!(put.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(put.content_disposition.as_deref(), Some("inline"));
    assert_eq!(put.body.len() as i64, json["file_size"].as_i64().unwrap());
    assert!(is_pdf(&put.body));
}

// ---- real Prince: verifies actual rendering ----

#[tokio::test]
async fn real_single_page_document() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let body = app.to_bytes(form("<h1>One page</h1>")).await.bytes().await.unwrap();

    let doc = load_pdf(&body);
    assert_eq!(doc.get_pages().len(), 1);
    assert_eq!(media_boxes(&doc), vec![[0, 0, 612, 792]], "US Letter by default");
}

#[tokio::test]
async fn real_multi_page_document_keeps_every_content_page() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let t = "<p>1</p><div style='page-break-before:always'>2</div><div style='page-break-before:always'>3</div>";
    let body = app.to_bytes(form(t)).await.bytes().await.unwrap();

    assert_eq!(page_count(&body), 3);
}

#[tokio::test]
async fn real_full_html_document_with_body_attributes() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let t = "<!DOCTYPE html><html><head><title>T</title></head><body class='a' style='color:red'><p>x</p></body></html>";
    let body = app.to_bytes(form(t)).await.bytes().await.unwrap();

    assert_eq!(page_count(&body), 1);
}

#[tokio::test]
async fn real_a4_landscape() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let body = app
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

    let [_, _, w, h] = media_boxes(&load_pdf(&body))[0];
    assert_eq!((w, h), (842, 595), "A4 landscape is 842x595pt");
}

#[tokio::test]
async fn real_unicode_cjk_emoji_render() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let resp = app
        .to_bytes(
            form("<p>{{ s }}</p>").text("data", r#"{"s":"₱ € ¥ | 你好 | 🚀 ✅ | © ® | Ñandú"}"#),
        )
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

#[tokio::test]
async fn real_encrypted_opens_with_password_only() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let body = app
        .to_bytes(form("<p>x</p>").text("password", "s3cret"))
        .await
        .bytes()
        .await
        .unwrap();

    assert!(load_pdf_with_password(&body, "s3cret").is_ok());
    assert!(load_pdf_with_password(&body, "wrong").is_err());
}

#[tokio::test]
async fn real_to_s3_end_to_end() {
    require_prince!();
    let app = TestApp::builder("prince").spawn().await;

    let resp = app
        .to_s3(multipart::Form::new().text("template", "<h1>{{ t }}</h1>").text("data", r#"{"t":"E2E"}"#))
        .await;
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();

    let put = &app.s3.puts()[0];
    assert_eq!(put.body.len() as i64, json["file_size"].as_i64().unwrap());
    assert_eq!(page_count(&put.body), 1);
}
