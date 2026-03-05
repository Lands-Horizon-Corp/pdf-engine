use lopdf::Document;

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

pub fn remove_first_page(pdf_bytes: Vec<u8>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut doc = Document::load_mem(&pdf_bytes)?;
    let pages = doc.get_pages();
    if pages.len() <= 1 {
        return Ok(pdf_bytes);
    }
    let first_page = *pages.keys().next().unwrap();
    doc.delete_pages(&[first_page]);
    let mut output = Vec::new();
    doc.save_to(&mut output)?;
    Ok(output)
}
