use handlebars::Handlebars;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};

// 1. Define your data structure
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

// --- Your function stays here ---
pub fn html_to_pdf<T: Serialize>(
    template_str: &str,
    data: &T,
    width: &str,
    height: &str,
    progress: impl Fn(f32),
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    progress(10.0);
    let hb = Handlebars::new(); // Note: removed mut as it's not needed for one-off render
    let html_content = hb.render_template(template_str, data)?;
    progress(30.0);

    let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);

    let mut child = Command::new("prince")
        .arg("-")
        .arg("-o")
        .arg("-")
        .arg("--style")
        .arg(format!("data:text/css,{}", size_css))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    progress(50.0);

    let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
    stdin.write_all(html_content.as_bytes())?;
    drop(stdin);

    progress(80.0);

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!("PrinceXML Error: {}", err_msg).into());
    }

    progress(100.0);
    Ok(output.stdout)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 2. Prepare Template
    let template = r#"
        <html>
        <head><style>body { font-family: sans-serif; padding: 20px; }</style></head>
        <body>
            <h1>Invoice for {{customer_name}}</h1>
            <ul>
                {{#each items}}
                <li>{{this.name}}: ${{this.price}}</li>
                {{/each}}
            </ul>
            <hr/>
            <h2>Total: ${{total}}</h2>
        </body>
        </html>
    "#;

    // 3. Prepare Data
    let data = Invoice {
        customer_name: "Alice Liddell".to_string(),
        items: vec![
            Item {
                name: "White Rabbit Watch".to_string(),
                price: 45.0,
            },
            Item {
                name: "Tea Set".to_string(),
                price: 120.50,
            },
        ],
        total: 165.50,
    };

    // 4. Call the function
    println!("Starting conversion...");
    let pdf_bytes = html_to_pdf(
        template,
        &data,
        "210mm", // A4 Width
        "297mm", // A4 Height
        |p| println!("Progress: {}%", p),
    )?;

    // 5. Save the result to a file
    let mut file = File::create("invoice.pdf")?;
    file.write_all(&pdf_bytes)?;

    println!("Success! Created invoice.pdf");
    Ok(())
}
