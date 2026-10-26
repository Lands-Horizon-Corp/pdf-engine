//! `@font-face`: a font the document brings along is used under the name its CSS gave it, and only for that document.

mod common;

use base64::Engine as _;
use common::*;
use reqwest::multipart;

fn noto_bold_uri() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".public/assets/fonts/NotoSans-Bold.ttf");
    let bytes = std::fs::read(path).unwrap();
    format!(
        "data:font/ttf;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

async fn pdf_of(app: &TestApp, template: &str) -> Vec<u8> {
    let resp = app
        .to_bytes(multipart::Form::new().text("template", template.to_string()))
        .await;
    assert_eq!(resp.status(), 200);
    resp.bytes().await.unwrap().to_vec()
}

fn has(pdf: &[u8], needle: &str) -> bool {
    pdf.windows(needle.len()).any(|w| w == needle.as_bytes())
}

const TEXT: &str =
    "<p style=\"font-family: 'Zzz Display', serif; font-size: 24px\">Hello World</p>";

#[tokio::test]
async fn a_font_face_font_is_used_under_its_css_name() {
    let app = TestApp::builder().spawn().await;
    let with = pdf_of(
        &app,
        &format!("<style>@font-face {{ font-family: 'Zzz Display'; src: url({}) format('truetype'); }}</style>{TEXT}", noto_bold_uri()),
    )
    .await;
    assert!(has(&with, "NotoSans-Bold"), "the embedded font is used");
    assert!(!has(&with, "Times"), "and not the serif fallback");

    // The same text without the rule falls back, so it really is the rule that made the difference.
    let without = pdf_of(&app, TEXT).await;
    assert!(!has(&without, "NotoSans-Bold"));
}

#[tokio::test]
async fn a_font_face_font_does_not_leak_into_the_next_document() {
    let app = TestApp::builder().spawn().await;
    let rule = format!(
        "<style>@font-face {{ font-family: 'Zzz Display'; src: url({}) }}</style>",
        noto_bold_uri()
    );
    let first = pdf_of(&app, &format!("{rule}{TEXT}")).await;
    assert!(has(&first, "NotoSans-Bold"));
    for _ in 0..3 {
        let later = pdf_of(&app, TEXT).await;
        assert!(
            !has(&later, "NotoSans-Bold"),
            "the family exists only for the document that declared it"
        );
    }
}

#[tokio::test]
async fn weight_and_style_pick_the_right_face() {
    let app = TestApp::builder().spawn().await;
    // Only a bold face is declared, as weight 700: bold text finds it.
    let pdf = pdf_of(
        &app,
        &format!(
            "<style>@font-face {{ font-family: 'Zzz Display'; src: url({}); font-weight: 700 }}</style>\
             <p style=\"font-family: 'Zzz Display'; font-weight: 700\">Bold</p>",
            noto_bold_uri()
        ),
    )
    .await;
    assert!(has(&pdf, "NotoSans-Bold"));
}

#[tokio::test]
async fn an_unusable_font_never_costs_the_document() {
    let app = TestApp::builder().spawn().await;
    let not_a_font = "data:font/ttf;base64,aGVsbG8gd29ybGQ=";
    let pdf = pdf_of(
        &app,
        &format!("<style>@font-face {{ font-family: 'Zzz Display'; src: url({not_a_font}) }}</style>{TEXT}"),
    )
    .await;
    assert_eq!(page_count(&pdf), 1);
    assert!(!has(&pdf, "NotoSans-Bold"));

    // A remote font from a host nobody allowed is skipped too.
    let remote = pdf_of(
        &app,
        &format!("<style>@font-face {{ font-family: 'Zzz Display'; src: url(https://fonts.example.com/a.ttf) format('truetype') }}</style>{TEXT}"),
    )
    .await;
    assert_eq!(page_count(&remote), 1);
}
