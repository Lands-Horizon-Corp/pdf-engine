//! Happy path: well-formed, authorized requests produce correct PDFs.

mod common;

use common::*;
use pdf_engine::fulgur_engine::PageMm;

#[tokio::test]
async fn to_bytes_returns_pdf_with_headers() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form("<h1>Hello</h1>").text("filename", "invoice-42.pdf"))
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "application/pdf");
    assert_eq!(
        resp.headers()["content-disposition"],
        "inline; filename=\"invoice-42.pdf\""
    );
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

#[tokio::test]
async fn template_is_rendered_with_data_and_escaped() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(
            form("<h1>Hi {{ name }}</h1>{% for i in items %}<li>{{ i }}</li>{% endfor %}")
                .text("data", r#"{"name":"<Zé>","items":["a","b"]}"#),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let html = &app.calls()[0].html;
    assert!(html.contains("<h1>Hi &lt;Zé&gt;</h1>"), "{html}");
    assert!(html.contains("<li>a</li><li>b</li>"), "{html}");
}

#[tokio::test]
async fn default_page_is_us_letter() {
    let app = TestApp::builder().spawn().await;

    let body = app.to_bytes(form("<p>x</p>")).await.bytes().await.unwrap();

    assert_eq!(media_boxes(&load_pdf(&body)), vec![[0, 0, 612, 792]]);
    assert_eq!(
        app.calls()[0].page,
        PageMm::parse("8.5in", "11in", "portrait").unwrap()
    );
}

#[tokio::test]
async fn custom_page_size_and_orientation() {
    let app = TestApp::builder().spawn().await;

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

    assert_eq!(media_boxes(&load_pdf(&body))[0][2..], [842, 595]);
}

#[tokio::test]
async fn page_breaks_produce_multiple_pages() {
    let app = TestApp::builder().spawn().await;

    let t = "<p>1</p><div style='break-before:page'>2</div><div style='page-break-before:always'>3</div>";
    let body = app.to_bytes(form(t)).await.bytes().await.unwrap();

    assert_eq!(page_count(&body), 3);
}

#[tokio::test]
async fn long_content_flows_onto_more_pages() {
    let app = TestApp::builder().spawn().await;
    let rows: String = (0..200).map(|i| format!("<p>row {i}</p>")).collect();

    let body = app.to_bytes(form(&rows)).await.bytes().await.unwrap();

    assert!(page_count(&body) > 1);
}

#[tokio::test]
async fn full_html_document_with_body_attributes() {
    let app = TestApp::builder().spawn().await;

    let t = "<!DOCTYPE html><html><head><title>T</title></head><body class='a' style='color:red'><p>x</p></body></html>";
    let resp = app.to_bytes(form(t)).await;

    assert_eq!(resp.status(), 200);
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

/// Generic and common web font families must resolve to real fonts; text that silently
/// renders with no font at all is the failure this guards against.
#[tokio::test]
async fn generic_and_web_font_families_draw_text() {
    let app = TestApp::builder().spawn().await;

    for family in [
        "sans-serif",
        "serif",
        "monospace",
        "Arial",
        "Helvetica",
        "'Times New Roman'",
        "'Courier New'",
        "Roboto",
        "system-ui",
    ] {
        let t =
            format!("<html><body style=\"font-family: {family}\"><p>Hello world</p></body></html>");
        let body = app.to_bytes(form(&t)).await.bytes().await.unwrap();
        assert!(
            !embedded_fonts(&body).is_empty(),
            "{family}: no font embedded, text is missing"
        );
    }
}

#[tokio::test]
async fn unicode_cjk_emoji_render() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form("<p>{{ s }}</p>").text(
            "data",
            r#"{"s":"₱ € ¥ | 你好 | 🚀 ✅ | © ® | Ñandú | مرحبا"}"#,
        ))
        .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(page_count(&resp.bytes().await.unwrap()), 1);
}

#[tokio::test]
async fn document_title_is_set() {
    let app = TestApp::builder().spawn().await;

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
async fn password_encrypts_output() {
    let app = TestApp::builder().spawn().await;

    let body = app
        .to_bytes(form("<p>secret</p>").text("password", "hunter2"))
        .await
        .bytes()
        .await
        .unwrap();

    assert!(String::from_utf8_lossy(&body).contains("/Encrypt"));
    let doc = load_pdf_with_password(&body, "hunter2").expect("correct password must open it");
    assert_eq!(doc.get_pages().len(), 1);
    assert!(load_pdf_with_password(&body, "wrong").is_err());
}

#[tokio::test]
async fn output_is_deterministic() {
    let app = TestApp::builder().spawn().await;
    let t = "<h1>Same</h1><p>input, same bytes</p>";

    let a = app.to_bytes(form(t)).await.bytes().await.unwrap();
    let b = app.to_bytes(form(t)).await.bytes().await.unwrap();

    assert_eq!(a, b);
}

#[tokio::test]
async fn to_s3_uploads_and_returns_presigned_payload() {
    let app = TestApp::builder().spawn().await;

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
    assert!(
        url.contains(key) && url.contains("X-Amz-Signature="),
        "{url}"
    );

    let puts = app.s3.puts();
    assert_eq!(puts.len(), 1);
    let put = &puts[0];
    assert_eq!(put.path, format!("/{BUCKET}/{key}"));
    assert_eq!(put.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(put.content_disposition.as_deref(), Some("inline"));
    assert_eq!(put.body.len() as i64, json["file_size"].as_i64().unwrap());
    assert_eq!(page_count(&put.body), 1);
}
