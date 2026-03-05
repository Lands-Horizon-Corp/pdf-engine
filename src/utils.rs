use dashmap::DashMap;
use handlebars::{Context, Handlebars, RenderContext, Renderable, StringOutput, Template};
use once_cell::sync::Lazy;
use opendal::{Operator, services::S3};
use serde::Serialize;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

static HB: Lazy<Handlebars> = Lazy::new(Handlebars::new);
static TEMPLATE_CACHE: Lazy<DashMap<[u8; 16], Template>> = Lazy::new(DashMap::new);

static OP: Lazy<Operator> = Lazy::new(|| {
    let builder = S3::default()
        .endpoint("http://127.0.0.1:9000")
        .access_key_id("5pMiSk03Lt7yft5gXwe8L4EMXKXduE")
        .secret_access_key("nimcCJvW7N2L8yChupPiJcEBqxQ8Wc")
        .bucket("lands-horizon")
        .region("us-east-1");
    Operator::new(builder)
        .expect("Failed to create storage operator")
        .finish()
});

pub async fn html_to_pdf_to_storage<T: Serialize>(
    template_str: &str,
    data: &T,
    width: &str,
    height: &str,
    object_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let hash = *md5::compute(template_str);
    let html_content = {
        let template_ref = TEMPLATE_CACHE
            .entry(hash)
            .or_insert_with(|| Template::compile(template_str).expect("Template syntax error"));
        let ctx = Context::wraps(data)?;
        let mut rc = RenderContext::new(None);
        let mut output = StringOutput::new();
        template_ref.render(&*HB, &ctx, &mut rc, &mut output)?;
        output.into_string()?
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
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let mut stdout = child.stdout.take().expect("Failed to open stdout");
    let write_task = tokio::spawn(async move {
        let res = stdin.write_all(html_content.as_bytes()).await;
        drop(stdin);
        res
    });
    let mut pdf_buffer = Vec::new();
    tokio::io::copy(&mut stdout, &mut pdf_buffer).await?;

    let _ = write_task.await??;
    let status = child.wait().await?;

    if !status.success() {
        return Err("Prince PDF generation failed".into());
    }
    OP.write(object_name, pdf_buffer).await?;

    Ok(())
}
