//! A table's `<thead>` is repeated at the top of every page the table continues onto.

mod common;

use common::*;
use lopdf::content::Content;
use reqwest::multipart;

/// For every page, the largest font size drawn on it (the header uses a font much larger than the body text).
fn largest_font_per_page(pdf: &[u8]) -> Vec<f32> {
    let doc = load_pdf(pdf);
    doc.get_pages()
        .values()
        .map(|id| {
            let content = Content::decode(&doc.get_page_content(*id)).unwrap();
            content
                .operations
                .iter()
                .filter(|o| o.operator == "Tf")
                .filter_map(|o| o.operands.get(1).and_then(|n| n.as_float().ok()))
                .fold(0.0, f32::max)
        })
        .collect()
}

async fn pdf_of(app: &TestApp, template: &str) -> Vec<u8> {
    let resp = app
        .to_bytes(multipart::Form::new().text("template", template.to_string()))
        .await;
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

fn rows(n: usize) -> String {
    (1..=n)
        .map(|i| format!("<tr><td>{i}</td><td>Item {i}</td></tr>"))
        .collect()
}

const STYLE: &str = "<style>body{font-family:sans-serif;font-size:12px} table{width:100%;border-collapse:collapse} \
    th{font-size:30px;text-align:left} td{padding:6px 8px;border-bottom:1px solid #ddd} tr{page-break-inside:avoid}</style>";

#[tokio::test]
async fn the_header_is_on_every_page_of_a_long_table() {
    let app = TestApp::builder().spawn().await;
    let pdf = pdf_of(&app, &format!("{STYLE}<table><thead><tr><th>#</th><th>Item</th></tr></thead><tbody>{}</tbody></table>", rows(300))).await;
    let pages = largest_font_per_page(&pdf);
    assert!(pages.len() >= 4, "a long table: {} pages", pages.len());
    for (i, size) in pages.iter().enumerate() {
        assert!(
            *size >= 20.0,
            "page {} has the header (largest font {size})",
            i + 1
        );
    }
    // Without a thead, only the first page has the large text.
    let plain = pdf_of(
        &app,
        &format!(
            "{STYLE}<table><tbody><tr><th>#</th><th>Item</th></tr>{}</tbody></table>",
            rows(300)
        ),
    )
    .await;
    let plain_pages = largest_font_per_page(&plain);
    assert!(plain_pages[1..].iter().all(|s| *s < 20.0));
}

#[tokio::test]
async fn several_header_rows_and_a_table_that_starts_mid_page() {
    let app = TestApp::builder().spawn().await;
    let html = format!(
        "{STYLE}<h1 style=\"font-size:14px\">Some text before the table</h1><p>more</p>\
         <table><thead><tr><th>A</th><th>B</th></tr><tr><th>a</th><th>b</th></tr></thead><tbody>{}</tbody></table>",
        rows(200)
    );
    let pdf = pdf_of(&app, &html).await;
    let pages = largest_font_per_page(&pdf);
    assert!(pages.len() >= 3);
    assert!(
        pages.iter().all(|s| *s >= 20.0),
        "every page has the header: {pages:?}"
    );
}

#[tokio::test]
async fn short_tables_and_tables_without_a_header_are_untouched() {
    let app = TestApp::builder().spawn().await;
    // Fits on one page: nothing is repeated, and the page count is the same as without the feature's help.
    let one = pdf_of(&app, &format!("{STYLE}<table><thead><tr><th>#</th><th>Item</th></tr></thead><tbody>{}</tbody></table>", rows(10))).await;
    assert_eq!(page_count(&one), 1);
    let none = pdf_of(
        &app,
        &format!("{STYLE}<table><tbody>{}</tbody></table>", rows(300)),
    )
    .await;
    assert!(page_count(&none) >= 4);
    // An empty body, and a header only.
    assert_eq!(
        page_count(
            &pdf_of(
                &app,
                &format!("{STYLE}<table><thead><tr><th>x</th></tr></thead></table>")
            )
            .await
        ),
        1
    );
    assert_eq!(
        page_count(
            &pdf_of(
                &app,
                &format!("{STYLE}<table><thead><tr><th>x</th></tr></thead><tbody></tbody></table>")
            )
            .await
        ),
        1
    );
}

#[tokio::test]
async fn two_long_tables_each_repeat_their_own_header() {
    let app = TestApp::builder().spawn().await;
    let html = format!(
        "{STYLE}<table><thead><tr><th>First</th></tr></thead><tbody>{}</tbody></table>\
         <table><thead><tr><th>Second</th></tr></thead><tbody>{}</tbody></table>",
        rows(120),
        rows(120)
    );
    let pages = largest_font_per_page(&pdf_of(&app, &html).await);
    assert!(pages.len() >= 4);
    assert!(pages.iter().all(|s| *s >= 20.0), "{pages:?}");
}
