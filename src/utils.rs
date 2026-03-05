use handlebars::Handlebars;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

static HB: Lazy<Handlebars> = Lazy::new(|| {
    let mut hb = Handlebars::new();
    // Pre-registering a template is faster than rendering from string every time
    hb.register_template_string(
        "invoice",
        "<html><body><h1>Invoice for {{customer_name}}</h1></body></html>",
    )
    .unwrap();
    hb
});

pub async fn html_to_pdf_stream<T: Serialize>(
    template_name: &str,
    data: &T,
    width: &str,
    height: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let html_content = HB.render(template_name, data)?;
    let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);
    let mut child = Command::new("prince")
        .arg("-")
        .arg("-o")
        .arg("-") // Output to stdout
        .arg("--style")
        .arg(format!("data:text/css,{}", size_css))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut file = tokio::fs::File::create(output_path).await?;
    let write_task = tokio::spawn(async move { stdin.write_all(html_content.as_bytes()).await });
    let mut buffer = [0u8; 8192];
    while let Ok(n) = stdout.read(&mut buffer).await {
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n]).await?;
    }
    let _ = write_task.await?;
    let status = child.wait().await?;
    if !status.success() {
        return Err("Prince conversion failed".into());
    }
    Ok(())
}
