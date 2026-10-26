//! `float: left | right` is approximated: floats and the content beside them share a row.

mod common;

use common::*;
use reqwest::multipart;

async fn pages(app: &TestApp, template: &str) -> usize {
    let resp = app
        .to_bytes(multipart::Form::new().text("template", template.to_string()))
        .await;
    assert_eq!(resp.status(), 200);
    load_pdf(&resp.bytes().await.unwrap()).get_pages().len()
}

const BOXES: &str = r#"<style>body{margin:0}.b{width:200px;height:600px;background:#cde}</style>
    <div class="b" style="FLOAT">A</div><div class="b" style="FLOAT2">B</div>"#;

#[tokio::test]
async fn floated_boxes_sit_side_by_side() {
    let app = TestApp::builder().spawn().await;
    // Two 600px boxes do not fit under each other on a page, but do side by side.
    let stacked = pages(&app, &BOXES.replace("FLOAT2", "").replace("FLOAT", "")).await;
    let floated = pages(
        &app,
        &BOXES
            .replace("FLOAT2", "float:right")
            .replace("FLOAT", "float:left"),
    )
    .await;
    assert_eq!(stacked, 2);
    assert_eq!(floated, 1);
}

#[tokio::test]
async fn clear_ends_the_row() {
    let app = TestApp::builder().spawn().await;
    let html = r#"<style>body{margin:0}.b{width:200px;height:600px;background:#cde;float:left}</style>
        <div class="b">A</div><div style="clear:both;height:600px">B</div>"#;
    assert_eq!(pages(&app, html).await, 2);
}

#[tokio::test]
async fn text_beside_a_float_still_paginates() {
    let app = TestApp::builder().spawn().await;
    let paras: String = (0..80)
        .map(|i| format!("<p>Paragraph {i} lorem ipsum dolor sit amet.</p>"))
        .collect();
    let html = format!(
        r#"<div style="float:left;width:100px;height:80px;background:#cde">IMG</div>{paras}"#
    );
    assert!(pages(&app, &html).await >= 2);
}
