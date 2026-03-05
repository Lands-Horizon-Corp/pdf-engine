use handlebars::Handlebars;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::process::Stdio;
use std::sync::RwLock;
use tokio::io::AsyncWriteExt;
use tokio::io::copy;
use tokio::process::Command;

static HB: Lazy<RwLock<Handlebars>> = Lazy::new(|| RwLock::new(Handlebars::new()));
pub async fn html_to_pdf_stream<T: Serialize>(
    template_str: &str,
    data: &T,
    width: &str,
    height: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 2. Fast Path Rendering
    // Pre-calculate the HTML to avoid holding any locks during the heavy IO phase
    let html_content = {
        let hb = HB.read().unwrap();
        hb.render_template(template_str, data)? // Skip the hash-check/register dance if possible
    };

    let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);

    let mut child = Command::new("prince")
        .args([
            "-",
            "-o",
            "-",
            "--style",
            &format!("data:text/css,{}", size_css),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // Don't capture stderr unless you need it; reduces overhead
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut file = tokio::fs::File::create(output_path).await?;

    // 3. Optimized Pipe: use tokio::io::copy for zero-effort efficiency
    let write_task = tokio::spawn(async move { stdin.write_all(html_content.as_bytes()).await });

    // Use copy to stream stdout directly to the file
    copy(&mut stdout, &mut file).await?;

    let _ = write_task.await?;
    let status = child.wait().await?;

    if !status.success() {
        return Err("Prince failed".into());
    }
    Ok(())
}
