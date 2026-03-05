use dashmap::DashMap;
use handlebars::{Context, Handlebars, RenderContext, Renderable, StringOutput, Template};
use once_cell::sync::Lazy;
use serde::Serialize;
use std::process::Stdio;
use tokio::io::{AsyncWriteExt, copy};
use tokio::process::Command;

static HB: Lazy<Handlebars> = Lazy::new(Handlebars::new);
static TEMPLATE_CACHE: Lazy<DashMap<[u8; 16], Template>> = Lazy::new(DashMap::new);

pub async fn html_to_pdf_stream<T: Serialize>(
    template_str: &str,
    data: &T,
    width: &str,
    height: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let hash = *md5::compute(template_str);

    // 1. High-Performance Rendering
    let html_content = {
        let template_ref = TEMPLATE_CACHE
            .entry(hash)
            .or_insert_with(|| Template::compile(template_str).expect("Template syntax error"));

        let ctx = Context::wraps(data)?;
        let mut rc = RenderContext::new(None);

        let mut output = StringOutput::new();
        template_ref.render(&*HB, &ctx, &mut rc, &mut output)?;

        // FIX: Add '?' to resolve the Result into a String
        output.into_string()?
    };

    let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);

    // 2. Prince Command setup
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
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let mut stdout = child.stdout.take().expect("Failed to open stdout");
    let mut file = tokio::fs::File::create(output_path).await?;

    // 3. Concurrent IO: Stream HTML into Prince stdin
    let write_task = tokio::spawn(async move {
        let res = stdin.write_all(html_content.as_bytes()).await;
        drop(stdin); // Signal EOF so Prince knows to finish
        res
    });

    // 4. Stream Prince stdout directly to the output file
    copy(&mut stdout, &mut file).await?;

    // Wait for the background write task and the process to finish
    let _ = write_task.await??;
    let status = child.wait().await?;

    if !status.success() {
        return Err("Prince PDF generation failed".into());
    }

    Ok(())
}
