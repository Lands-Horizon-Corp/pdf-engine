use rayon::prelude::*;
use std::fs;
use std::process::Command;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let num_docs = 100;
    println!(
        "🚀 Orchestrating {} PDF generations via Fullbleed Binary...",
        num_docs
    );

    // Create a data directory for our JSON inputs
    fs::create_dir_all("batch_data")?;
    fs::create_dir_all("output")?;

    let start = Instant::now();

    // Parallel execution across all CPU cores
    (0..num_docs).into_par_iter().for_each(|id| {
        let json_path = format!("batch_data/record_{}.json", id);
        let pdf_path = format!("output/invoice_{}.pdf", id);

        // 1. Create the data record for this PDF
        let data = serde_json::json!({
            "id": id,
            "client": "Zerodha User",
            "amount": 5000.0 + (id as f64)
        });
        fs::write(&json_path, data.to_string()).unwrap();

        // 2. Call the Fullbleed binary directly (Pure Rust performance)
        // Command: fullbleed render <template> <json_data> <output_pdf>
        let status = Command::new("fullbleed")
            .arg("render")
            .arg("templates/invoice.html") // Your HTML template
            .arg(&json_path)
            .arg(&pdf_path)
            .status()
            .expect("Failed to execute fullbleed");

        if !status.success() {
            eprintln!("Error rendering PDF {}", id);
        }
    });

    let duration = start.elapsed();
    println!("✅ Done! Generated {} PDFs in {:?}", num_docs, duration);
    println!("📈 Performance: {:?} per PDF", duration / num_docs as u32);

    Ok(())
}
