//! `accessible`: a tagged PDF with a structure tree, the document language and a title (PDF/UA).

mod common;

use common::*;
use reqwest::multipart;

async fn pdf_of(app: &TestApp, template: &str, accessible: Option<&str>) -> Vec<u8> {
    let mut form = multipart::Form::new().text("template", template.to_string());
    if let Some(v) = accessible {
        form = form.text("accessible", v.to_string());
    }
    let resp = app.to_bytes(form).await;
    let status = resp.status();
    let body = resp.bytes().await.unwrap().to_vec();
    assert_eq!(
        status,
        200,
        "{}",
        String::from_utf8_lossy(&body[..body.len().min(400)])
    );
    body
}

fn has(pdf: &[u8], needle: &str) -> bool {
    pdf.windows(needle.len()).any(|w| w == needle.as_bytes())
}

const DOC: &str = "<html lang=\"fil\"><head><title>Invoice 42</title></head><body>\
    <h1>Invoice</h1><p>Hello <a href=\"https://example.com\">link</a>.</p>\
    <table><tr><th>Item</th><th>Qty</th></tr><tr><td>Hosting</td><td>1</td></tr></table></body></html>";

#[tokio::test]
async fn an_accessible_pdf_has_a_structure_tree_a_language_and_a_title() {
    let app = TestApp::builder().spawn().await;
    let plain = pdf_of(&app, DOC, None).await;
    assert!(
        !has(&plain, "/StructTreeRoot"),
        "an ordinary PDF is not tagged"
    );

    let tagged = pdf_of(&app, DOC, Some("true")).await;
    assert!(has(&tagged, "/StructTreeRoot"), "a structure tree");
    assert!(has(&tagged, "/MarkInfo"), "marked as tagged");
    assert!(
        has(&tagged, "/Lang") && has(&tagged, "fil"),
        "the document language from <html lang>"
    );
    assert!(has(&tagged, "Invoice 42"), "the title");
    assert!(has(&tagged, "pdfuaid"), "declared as PDF/UA");
}

#[tokio::test]
async fn the_language_defaults_to_english_and_odd_values_do_not_break_it() {
    let app = TestApp::builder().spawn().await;
    let tagged = pdf_of(&app, "<h1>Hello</h1><p>text</p>", Some("1")).await;
    assert!(has(&tagged, "/Lang") && has(&tagged, "en"));
    // A page with no headings has no outline, which PDF/UA requires: it is still tagged, with a language and a title,
    // but is not declared as PDF/UA.
    let no_headings = pdf_of(&app, "<p>just a paragraph</p>", Some("true")).await;
    assert!(has(&no_headings, "/StructTreeRoot") && !has(&no_headings, "pdfuaid"));
    // `accessible` is false for anything that is not a yes.
    let off = pdf_of(&app, DOC, Some("no")).await;
    assert!(!has(&off, "/StructTreeRoot"));
    assert_eq!(
        page_count(
            &pdf_of(
                &app,
                "<html lang=\"\"><body><p>x</p></body></html>",
                Some("true")
            )
            .await
        ),
        1
    );
}

#[tokio::test]
async fn an_accessible_document_with_images_forms_and_page_breaks_still_renders() {
    let app = TestApp::builder().spawn().await;
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC";
    let html = format!(
        "<html lang=\"en\"><head><title>Report</title></head><body><h1>One</h1>\
         <img src=\"data:image/png;base64,{png}\" alt=\"a small square\" width=\"40\">\
         <ul><li>a</li><li>b</li></ul>{{% pagebreak %}}<h2>Two</h2><p>more</p></body></html>"
    );
    let pdf = pdf_of(&app, &html, Some("true")).await;
    assert_eq!(page_count(&pdf), 2);
    assert!(has(&pdf, "/StructTreeRoot"));
}
