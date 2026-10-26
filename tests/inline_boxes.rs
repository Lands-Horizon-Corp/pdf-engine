//! `inline-block` and `inline-flex` boxes inside a paragraph that wraps onto a second line are drawn.

mod common;

use common::*;
use lopdf::content::Content;
use reqwest::multipart;

/// The largest font size (in points) used on the first page.
fn largest_font_size(pdf: &[u8]) -> f32 {
    let doc = load_pdf(pdf);
    let id = *doc.get_pages().values().next().unwrap();
    let content = Content::decode(&doc.get_page_content(id)).unwrap();
    content
        .operations
        .iter()
        .filter(|o| o.operator == "Tf")
        .filter_map(|o| o.operands.get(1).and_then(|n| n.as_float().ok()))
        .fold(0.0, f32::max)
}

async fn pdf_of(app: &TestApp, template: &str) -> Vec<u8> {
    let resp = app
        .to_bytes(multipart::Form::new().text("template", template.to_string()))
        .await;
    assert_eq!(resp.status(), 200);
    resp.bytes().await.unwrap().to_vec()
}

#[tokio::test]
async fn an_inline_block_in_a_wrapped_paragraph_is_drawn() {
    let app = TestApp::builder().spawn().await;
    let long =
        "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor ".repeat(4);
    // The box's text is in a much larger size than the paragraph's, so drawing it shows as a bigger font in the page.
    for style in [
        "display:inline-block",
        "display:inline-flex",
        "display:inline-block;padding:.15em 0;white-space:nowrap",
    ] {
        let with = pdf_of(
            &app,
            &format!("<p>{long} a <span style=\"{style};font-size:40px\">MARK</span> b {long}</p>"),
        )
        .await;
        let without = pdf_of(
            &app,
            &format!("<p>{long} a <span style=\"{style};font-size:40px\"></span> b {long}</p>"),
        )
        .await;
        assert!(
            largest_font_size(&without) < 15.0,
            "{style}: the paragraph itself is small text"
        );
        assert!(
            largest_font_size(&with) > 25.0,
            "{style}: the box's text is drawn in a paragraph of several lines"
        );
    }
}

#[tokio::test]
async fn a_formula_inside_a_wrapped_paragraph_is_drawn() {
    let app = TestApp::builder().spawn().await;
    let long =
        "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor ".repeat(4);
    let with = pdf_of(&app, &format!("<p>{long} <math style=\"\">E=mc^2</math> <span style=\"display:inline-block;font-size:40px\">X</span> {long}</p>")).await;
    assert!(largest_font_size(&with) > 25.0);
}
