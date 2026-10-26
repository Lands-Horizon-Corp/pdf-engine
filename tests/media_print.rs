//! A PDF is print media: `@media print` rules apply, `@media screen` rules do not.

mod common;

use common::*;
use lopdf::content::Content;
use reqwest::multipart;

/// The first three numbers of every fill-colour operation on the first page, as 0 to 255 integers.
fn fill_colours(pdf: &[u8]) -> Vec<[u8; 3]> {
    let doc = load_pdf(pdf);
    let id = *doc.get_pages().values().next().unwrap();
    let content = Content::decode(&doc.get_page_content(id)).unwrap();
    content
        .operations
        .iter()
        .filter(|o| matches!(o.operator.as_str(), "rg" | "sc" | "scn"))
        .filter_map(|o| {
            let n: Vec<f32> = o
                .operands
                .iter()
                .filter_map(|x| x.as_float().ok())
                .collect();
            (n.len() >= 3).then(|| {
                [
                    (n[0] * 255.0).round() as u8,
                    (n[1] * 255.0).round() as u8,
                    (n[2] * 255.0).round() as u8,
                ]
            })
        })
        .collect()
}

async fn pdf_of(app: &TestApp, template: &str) -> Vec<u8> {
    let resp = app
        .to_bytes(multipart::Form::new().text("template", template.to_string()))
        .await;
    assert_eq!(resp.status(), 200);
    resp.bytes().await.unwrap().to_vec()
}

#[tokio::test]
async fn print_rules_apply_and_screen_rules_do_not() {
    let app = TestApp::builder().spawn().await;
    let pdf = pdf_of(
        &app,
        "<style>.a { width: 100px; height: 50px; background: #00ff00 }\
         @media print { .a { background: #ff0000 } }\
         @media screen { .a { background: #0000ff } }</style><div class=\"a\"></div>",
    )
    .await;
    let colours = fill_colours(&pdf);
    assert!(
        colours.contains(&[255, 0, 0]),
        "the print rule wins: {colours:?}"
    );
    assert!(
        !colours.contains(&[0, 0, 255]),
        "the screen rule is not applied: {colours:?}"
    );
    assert!(
        !colours.contains(&[0, 255, 0]),
        "the print rule overrides the plain one: {colours:?}"
    );
}

#[tokio::test]
async fn a_screen_only_element_can_be_hidden_for_the_pdf() {
    let app = TestApp::builder().spawn().await;
    // The usual pattern: buttons and navigation are hidden in print.
    let with = pdf_of(
        &app,
        "<style>.no-print { color: #123456 }</style><p class=\"no-print\">Export</p><p>Report</p>",
    )
    .await;
    let hidden = pdf_of(&app, "<style>@media print { .no-print { display: none } }</style><p class=\"no-print\">Export</p><p>Report</p>").await;
    let ops = |b: &[u8]| {
        let doc = load_pdf(b);
        let id = *doc.get_pages().values().next().unwrap();
        Content::decode(&doc.get_page_content(id))
            .unwrap()
            .operations
            .iter()
            .filter(|o| matches!(o.operator.as_str(), "Tj" | "TJ"))
            .count()
    };
    assert!(
        ops(&hidden) < ops(&with),
        "the print rule hides the element ({} vs {})",
        ops(&hidden),
        ops(&with)
    );
}
