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

pub fn remove_first_page(
    input_bytes: Vec<u8>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let mut doc = Document::load_from(Cursor::new(input_bytes))?;
    doc.delete_pages(&[1]);
    doc.prune_objects();
    let mut out_buffer = Vec::with_capacity(doc.objects.len() * 128);
    doc.save_to(&mut out_buffer)?;
    Ok(out_buffer)
}
