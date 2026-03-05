use lopdf::Document;
use std::io::Cursor;

pub fn prepend_blank_page_html(html: &str) -> String {
    let blank_page = r#"
    <div style="
        width: 100%;
        height: 100%;
        page-break-after: always;
    "></div>
    "#;

    format!("{}{}", blank_page, html)
}

pub fn remove_first_page_to_doc(
    input_bytes: Vec<u8>,
) -> Result<Document, Box<dyn std::error::Error + Send + Sync>> {
    let mut doc = Document::load_from(Cursor::new(input_bytes))?;
    doc.delete_pages(&[1]);
    doc.prune_objects();
    Ok(doc)
}
