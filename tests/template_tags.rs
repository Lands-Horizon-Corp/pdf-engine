//! Partials, `{% pagebreak %}` and `<barcode>`: through the real renderer, down to the pixels of a QR code.

mod common;

use common::*;
use pdf_engine::raster;
use reqwest::multipart;

fn form(template: &str, partials: Option<&str>) -> multipart::Form {
    let form = multipart::Form::new()
        .text("template", template.to_string())
        .text("data", r#"{"title":"Invoice 42","no":"INV-2026-0042"}"#);
    match partials {
        Some(p) => form.text("partials", p.to_string()),
        None => form,
    }
}

async fn pdf_of(app: &TestApp, template: &str, partials: Option<&str>) -> Vec<u8> {
    let resp = app.to_bytes(form(template, partials)).await;
    let status = resp.status();
    let body = resp.bytes().await.unwrap().to_vec();
    assert_eq!(
        status,
        200,
        "{}",
        String::from_utf8_lossy(&body[..body.len().min(300)])
    );
    body
}

#[tokio::test]
async fn pagebreak_starts_a_new_page() {
    let app = TestApp::builder().spawn().await;
    let one = pdf_of(&app, "<p>first</p><p>second</p>", None).await;
    assert_eq!(page_count(&one), 1);
    let two = pdf_of(&app, "<p>first</p>{% pagebreak %}<p>second</p>", None).await;
    assert_eq!(page_count(&two), 2);
    let three = pdf_of(
        &app,
        "<p>a</p>{%- pagebreak -%}<p>b</p>{% pagebreak %}<p>c</p>",
        None,
    )
    .await;
    assert_eq!(page_count(&three), 3);
}

#[tokio::test]
async fn partials_can_be_included_extended_and_imported() {
    let app = TestApp::builder().spawn().await;
    let partials = r#"{
        "header": "<h1>{{ title }}</h1>",
        "layout": "<header>{% block head %}{% endblock %}</header>{% block body %}{% endblock %}{% pagebreak %}<footer>end</footer>",
        "macros": "{% macro row(x) %}<tr><td>{{ x }}</td></tr>{% endmacro %}"
    }"#;
    let included = pdf_of(&app, r#"{% include "header" %}<p>body</p>"#, Some(partials)).await;
    assert_eq!(page_count(&included), 1);
    let extended = pdf_of(&app, r#"{% extends "layout" %}{% block head %}{{ title }}{% endblock %}{% block body %}<p>hi</p>{% endblock %}"#, Some(partials)).await;
    assert_eq!(
        page_count(&extended),
        2,
        "the pagebreak inside the partial counts"
    );
    let imported = pdf_of(
        &app,
        r#"{% import "macros" as m %}<table>{{ m.row("a") }}{{ m.row("b") }}</table>"#,
        Some(partials),
    )
    .await;
    assert_eq!(page_count(&imported), 1);
}

#[tokio::test]
async fn partial_mistakes_are_reported() {
    let app = TestApp::builder().spawn().await;
    for (partials, needle) in [
        (r#"{"../etc/passwd":"x"}"#, "not a valid name"),
        (r#"{"":"x"}"#, "not a valid name"),
        (r#"["not","an","object"]"#, "JSON object"),
        (r#"{"broken":"{% if %}"}"#, "syntax"),
    ] {
        let resp = app
            .to_bytes(form(r#"{% include "broken" %}"#, Some(partials)))
            .await;
        assert_eq!(resp.status(), 400, "{partials}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(
            body["error"].as_str().unwrap().contains(needle),
            "{partials}: {body}"
        );
    }
    // A name that was never given.
    let resp = app.to_bytes(form(r#"{% include "nope" %}"#, None)).await;
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap()["code"],
        "TEMPLATE_ERROR"
    );
}

/// Render a template to a bitmap the way a receipt printer would see it, and read the QR code back.
async fn scan(app: &TestApp, template: &str) -> Vec<String> {
    let pdf = pdf_of(app, template, None).await;
    let bitmap =
        tokio::task::spawn_blocking(move || raster::pdf_to_bitmap(&pdf, 816, false).unwrap())
            .await
            .unwrap();
    let mut image =
        rqrr::PreparedImage::prepare_from_greyscale(bitmap.width, bitmap.height, |x, y| {
            if bitmap.dots[y * bitmap.width + x] == 1 {
                0
            } else {
                255
            }
        });
    image
        .detect_grids()
        .into_iter()
        .filter_map(|g| g.decode().ok().map(|(_, text)| text))
        .collect()
}

#[tokio::test]
async fn a_qr_code_in_the_pdf_scans_back_to_its_value() {
    let app = TestApp::builder().spawn().await;
    let found = scan(&app, r#"<p>Pay here</p><barcode value="https://example.com/pay/{{ no }}?a=1&amp;b=2" type="qr" width="240"/>"#).await;
    assert_eq!(found, ["https://example.com/pay/INV-2026-0042?a=1&b=2"]);
    // Dark on white also with a colour and a background.
    let styled = scan(&app, r##"<barcode value="HELLO ₱ 你好" width="300" color="#111827" background="#ffffff" ec="H"/>"##).await;
    assert_eq!(styled, ["HELLO ₱ 你好"]);
}

#[tokio::test]
async fn linear_barcodes_make_a_page_with_bars_and_bad_values_a_400() {
    let app = TestApp::builder().spawn().await;
    let pdf = pdf_of(
        &app,
        r#"<barcode value="{{ no }}" type="code128" width="320" height="80"/>"#,
        None,
    )
    .await;
    let bitmap =
        tokio::task::spawn_blocking(move || raster::pdf_to_bitmap(&pdf, 816, false).unwrap())
            .await
            .unwrap();
    let ink = bitmap.dots.iter().filter(|&&d| d == 1).count();
    assert!(ink > 2_000, "bars were drawn: {ink} dots");

    let resp = app
        .to_bytes(form(r#"<barcode value="12AB" type="ean13"/>"#, None))
        .await;
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap()["code"],
        "INVALID_FIELD"
    );
}

/// Text-showing operators on each page of a PDF.
fn text_ops_per_page(pdf: &[u8]) -> Vec<usize> {
    let doc = load_pdf(pdf);
    doc.get_pages()
        .values()
        .map(|id| {
            let content = String::from_utf8_lossy(&doc.get_page_content(*id)).into_owned();
            content.matches("Tj").count() + content.matches("TJ").count()
        })
        .collect()
}

#[tokio::test]
async fn page_headers_and_numbers_are_drawn() {
    let app = TestApp::builder().spawn().await;
    let plain = pdf_of(&app, "<h1>One</h1>{% pagebreak %}<h1>Two</h1>", None).await;
    assert_eq!(text_ops_per_page(&plain), [1, 1]);

    let numbered = pdf_of(
        &app,
        r#"<style>@page { margin: 20mm; @top-center { content: "{{ title }}"; } @bottom-right { content: "Page " counter(page) " of " counter(pages); } }</style>
           <h1>One</h1>{% pagebreak %}<h1>Two</h1>"#,
        None,
    )
    .await;
    // Header and footer add text to each of the two pages. How many operators the text is split into
    // depends on the platform's fonts, so only compare with the page that has the body alone.
    let counts = text_ops_per_page(&numbered);
    assert_eq!(counts.len(), 2);
    assert!(
        counts.iter().all(|&n| n >= 3),
        "header and footer missing: {counts:?}"
    );
}
