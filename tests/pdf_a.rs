//! Archival PDF/A output: the level is declared, fonts are embedded, and a document that cannot meet the standard
//! is refused with a reason (never a silently non-conforming file).

mod common;

use common::*;
use reqwest::multipart;

async fn render(
    app: &TestApp,
    template: &str,
    level: &str,
    password: Option<&str>,
) -> reqwest::Response {
    let mut form = multipart::Form::new()
        .text("template", template.to_string())
        .text("pdf_a", level.to_string());
    if let Some(p) = password {
        form = form.text("password", p.to_string());
    }
    app.to_bytes(form).await
}

fn text(pdf: &[u8]) -> String {
    String::from_utf8_lossy(pdf).into_owned()
}

#[tokio::test]
async fn each_level_declares_itself() {
    let app = TestApp::builder().spawn().await;
    for (level, part, conformance) in [
        ("1b", "1", "B"),
        ("2b", "2", "B"),
        ("2u", "2", "U"),
        ("3b", "3", "B"),
        ("2a", "2", "A"),
        ("PDF/A-2b", "2", "B"),
    ] {
        let resp = render(&app, "<h1>Invoice</h1><p>₱100 total</p>", level, None).await;
        assert_eq!(resp.status(), 200, "level {level}");
        let pdf = resp.bytes().await.unwrap();
        let t = text(&pdf);
        assert!(
            t.contains(&format!("<pdfaid:part>{part}</pdfaid:part>")),
            "{level}"
        );
        assert!(
            t.contains(&format!(
                "<pdfaid:conformance>{conformance}</pdfaid:conformance>"
            )),
            "{level}"
        );
        assert!(
            t.contains("/OutputIntent") && t.contains("GTS_PDFA"),
            "{level}: output intent"
        );
        assert!(!t.contains("/Encrypt"), "{level}: PDF/A is never encrypted");
    }
}

#[tokio::test]
async fn the_a_levels_are_tagged() {
    let app = TestApp::builder().spawn().await;
    let pdf = render(&app, "<h1>T</h1><p>x</p>", "2a", None)
        .await
        .bytes()
        .await
        .unwrap();
    assert!(text(&pdf).contains("/StructTreeRoot"));
    let pdf = render(&app, "<h1>T</h1><p>x</p>", "2b", None)
        .await
        .bytes()
        .await
        .unwrap();
    assert!(!text(&pdf).contains("/StructTreeRoot"));
}

#[tokio::test]
async fn an_ordinary_pdf_is_not_pdf_a() {
    let app = TestApp::builder().spawn().await;
    let resp = app
        .to_bytes(multipart::Form::new().text("template", "<p>x</p>"))
        .await;
    assert!(!text(&resp.bytes().await.unwrap()).contains("pdfaid"));
}

#[tokio::test]
async fn real_documents_meet_the_standard() {
    let app = TestApp::builder().spawn().await;
    for template in [
        "<table border=1><tr><td>a</td><td>b</td></tr></table>",
        "<div style=\"opacity:.5;background:red;height:20px\">x</div>",
        "<div style=\"background:linear-gradient(red,blue);height:50px\"></div>",
        "<div style=\"box-shadow:3px 3px 5px #000;width:50px;height:50px\"></div>",
        "<a href=\"https://example.com\">link</a><math>\\frac{a}{b}</math>",
        "<img width=40 alt=\"dot\" src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==\">",
    ] {
        let resp = render(&app, template, "2b", None).await;
        assert_eq!(
            resp.status(),
            200,
            "{template}: {}",
            resp.text().await.unwrap()
        );
    }
}

#[tokio::test]
async fn a_document_that_cannot_conform_is_refused_with_a_reason() {
    let app = TestApp::builder().spawn().await;
    // Level 2a needs alt text on images.
    let resp = render(&app, "<img width=40 src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==\">", "2a", None).await;
    assert_eq!(resp.status(), 400);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("INVALID_FIELD") && body.contains("alt"),
        "{body}"
    );
}

#[tokio::test]
async fn bad_levels_and_a_password_are_invalid_fields() {
    let app = TestApp::builder().spawn().await;
    for level in ["9z", "pdfa", "2c", "A2"] {
        assert_eq!(
            render(&app, "<p>x</p>", level, None).await.status(),
            400,
            "{level}"
        );
    }
    let resp = render(&app, "<p>x</p>", "2b", Some("secret")).await;
    assert_eq!(resp.status(), 400);
    assert!(resp.text().await.unwrap().contains("encryption"));
}
