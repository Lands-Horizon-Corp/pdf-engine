mod utils; // This looks for utils.rs

use serde::Serialize;
use std::fs::File;
use std::io::Write;

#[derive(Serialize)]
struct Invoice {
    customer_name: String,
    items: Vec<Item>,
    total: f64,
}

#[derive(Serialize)]
struct Item {
    name: String,
    price: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let template = "<html><body><h1>Invoice for {{customer_name}}</h1></body></html>";
    let data = Invoice {
        customer_name: "Alice Liddell".to_string(),
        items: vec![],
        total: 0.0,
    };

    println!("Starting conversion...");

    // Call it using the module prefix
    let pdf_bytes = utils::html_to_pdf(template, &data, "210mm", "297mm", |p| {
        println!("Progress: {}%", p)
    })?;

    let mut file = File::create("invoice.pdf")?;
    file.write_all(&pdf_bytes)?;

    println!("Success! Created invoice.pdf");
    Ok(())
}
