use std::fs;
use typst::layout::PagedDocument;
use typst_as_lib::TypstEngine;

fn main() {
    let font_dir = "./src/fonts";
    let mut font_data = Vec::new();

    // 1. Diagnostics: Is the folder even there?
    if let Ok(entries) = fs::read_dir(font_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // Only try to read actual files (ignore directories/.DS_Store)
            if path.is_file() {
                match fs::read(&path) {
                    Ok(data) => {
                        println!(
                            "✅ Found and read font file: {:?}",
                            path.file_name().unwrap()
                        );
                        font_data.push(data);
                    }
                    Err(e) => println!("❌ Failed to read {:?}: {}", path, e),
                }
            }
        }
    } else {
        println!(
            "❌ ERROR: Directory '{}' not found. Is it in the right place?",
            font_dir
        );
        return;
    }

    if font_data.is_empty() {
        println!("❌ ERROR: No font files were loaded. The PDF will be empty!");
        return;
    }

    let content = r#"
        #set page(paper: "a6", margin: 1cm)
        // Use "" as the catch-all. If this still warns, no fonts were loaded.
        #set text(font: ("", "serif"), size: 14pt)

        = Font Test
        If you see this, the bytes were parsed!
    "#;

    // 2. Build the engine
    let engine = TypstEngine::builder()
        .main_file(content)
        .fonts(font_data)
        .build();

    // 3. Compile
    let warned_result = engine.compile::<PagedDocument>();

    for warning in &warned_result.warnings {
        println!("⚠️ Typst Warning: {}", warning.message);
    }

    if let Ok(doc) = warned_result.output {
        let pdf_bytes = typst_pdf::pdf(&doc, &Default::default()).expect("PDF render failed");

        // FIX: Add '&' here to borrow the bytes instead of moving them
        fs::write("output.pdf", &pdf_bytes).expect("Write failed");

        // Now pdf_bytes is still available for this line!
        println!("🚀 PDF written to output.pdf ({} bytes)", pdf_bytes.len());
    }
}
