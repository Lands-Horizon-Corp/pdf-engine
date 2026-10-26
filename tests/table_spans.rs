//! `rowspan` (and `colspan` together with it) in tables, checked by where the text lands on the page.

mod common;

use common::*;
use lopdf::content::Content;
use reqwest::multipart;

/// The x position of every text-showing operation of the first page, in drawing order.
fn text_x_positions(pdf: &[u8]) -> Vec<f32> {
    let doc = load_pdf(pdf);
    let id = *doc.get_pages().values().next().unwrap();
    let content = Content::decode(&doc.get_page_content(id)).unwrap();
    let (mut x, mut out) = (0.0f32, Vec::new());
    for op in &content.operations {
        let num = |i: usize| op.operands.get(i).and_then(|o| o.as_float().ok());
        match op.operator.as_str() {
            "Tm" => x = num(4).unwrap_or(x),
            "Td" | "TD" => x += num(0).unwrap_or(0.0),
            "Tj" | "TJ" => out.push(x),
            _ => {}
        }
    }
    out
}

async fn pdf_of(app: &TestApp, template: &str) -> Vec<u8> {
    let form = multipart::Form::new().text("template", template.to_string());
    let resp = app.to_bytes(form).await;
    assert_eq!(resp.status(), 200);
    resp.bytes().await.unwrap().to_vec()
}

#[tokio::test]
async fn a_rowspan_cell_keeps_the_next_rows_in_their_columns() {
    let app = TestApp::builder().spawn().await;
    // R spans three rows. Without rowspan, B and C would be pushed into R's column.
    let pdf = pdf_of(
        &app,
        "<table><tr><td rowspan=\"3\">R</td><td>A</td><td>1</td></tr>\
         <tr><td>B</td><td>2</td></tr><tr><td>C</td><td>3</td></tr></table>",
    )
    .await;
    let x = text_x_positions(&pdf);
    assert_eq!(x.len(), 7, "R A 1 B 2 C 3: {x:?}");
    let (r, a, one, b, two, c, three) = (x[0], x[1], x[2], x[3], x[4], x[5], x[6]);
    assert!(a > r + 5.0, "A is right of R: {x:?}");
    assert!(one > a + 5.0, "1 is right of A: {x:?}");
    assert!(
        (b - a).abs() < 1.0 && (c - a).abs() < 1.0,
        "A, B, C share a column: {x:?}"
    );
    assert!(
        (two - one).abs() < 1.0 && (three - one).abs() < 1.0,
        "1, 2, 3 share a column: {x:?}"
    );
}

#[tokio::test]
async fn rowspan_and_colspan_work_together() {
    let app = TestApp::builder().spawn().await;
    // A is 2 columns by 2 rows; the second row starts after it, so D sits in A's third column.
    let pdf = pdf_of(
        &app,
        "<table><tr><td rowspan=\"2\" colspan=\"2\">A</td><td>B</td></tr><tr><td>D</td></tr>\
         <tr><td>E</td><td>F</td><td>G</td></tr></table>",
    )
    .await;
    let x = text_x_positions(&pdf);
    assert_eq!(x.len(), 6, "A B D E F G: {x:?}");
    let (a, b, d, e, f, g) = (x[0], x[1], x[2], x[3], x[4], x[5]);
    assert!((b - d).abs() < 1.0, "B and D share the third column: {x:?}");
    assert!((g - b).abs() < 1.0, "G is in the third column too: {x:?}");
    assert!((e - a).abs() < 1.0, "E is in the first column: {x:?}");
    assert!(f > e + 5.0 && f < g, "F is in the second column: {x:?}");
}

#[tokio::test]
async fn a_rowspan_past_the_last_row_does_not_break_the_table() {
    let app = TestApp::builder().spawn().await;
    let pdf = pdf_of(
        &app,
        "<table><tr><td>1</td><td rowspan=\"9\">2</td></tr><tr><td>3</td></tr></table>",
    )
    .await;
    assert_eq!(page_count(&pdf), 1);
    assert_eq!(text_x_positions(&pdf).len(), 3);
}
