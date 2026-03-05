use crate::models::MediaPayload;
use futures_util::AsyncWriteExt as FuturesAsyncWriteExt;
use handlebars::Handlebars;
use once_cell::sync::Lazy;
use opendal::{Operator, services::S3};
use serde::Serialize;
use std::env;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio_util::compat::TokioAsyncReadCompatExt;

static PRINCE_CONCURRENCY: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(8));

static HB: Lazy<Handlebars<'static>> = Lazy::new(|| {
    let mut hb = Handlebars::new();
    hb.set_strict_mode(true);
    hb
});

static OP: Lazy<Operator> = Lazy::new(|| {
    let builder = S3::default()
        .endpoint(&env::var("S3_ENDPOINT").expect("S3_ENDPOINT must be set"))
        .access_key_id(&env::var("S3_ACCESS_KEY").expect("S3_ACCESS_KEY must be set"))
        .secret_access_key(&env::var("S3_SECRET_KEY").expect("S3_SECRET_KEY must be set"))
        .bucket(&env::var("S3_BUCKET").expect("S3_BUCKET must be set"))
        .region(&env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()));

    Operator::new(builder)
        .expect("Storage init failed")
        .finish()
});

pub async fn html_to_pdf_to_storage<T: Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
    width: String,
    height: String,
    object_name: String,
) -> Result<MediaPayload, Box<dyn std::error::Error + Send + Sync>> {
    let html_content =
        tokio::task::spawn_blocking(move || HB.render_template(&template_str, &data)).await??;
    let _permit = PRINCE_CONCURRENCY.acquire().await?;
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
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open stdout")?;

    // Step 3: Feed HTML to Prince in background
    tokio::spawn(async move {
        let _ = stdin.write_all(html_content.as_bytes()).await;
        let _ = stdin.flush().await;
        drop(stdin);
    });

    // Step 4: Stream Prince STDOUT directly to OpenDAL S3
    // We convert OpenDAL writer to a Futures-compatible AsyncWrite
    // and Tokio stdout to a Futures-compatible AsyncRead
    let writer = OP.writer(&object_name).await?;
    let mut remote_writer = writer.into_futures_async_write();
    let mut reader = tokio::io::BufReader::new(stdout).compat();

    // This transfers bytes directly from Prince to S3 without a large intermediate buffer
    let file_size = futures_util::io::copy(&mut reader, &mut remote_writer).await?;
    remote_writer.close().await?; // Finalize the S3 upload

    // Step 5: Cleanup Process
    let status = child.wait().await?;
    if !status.success() {
        return Err("Prince PDF generation failed".into());
    }

    // Step 6: Generate Metadata & URL
    let signed_req = OP
        .presign_read(&object_name, std::time::Duration::from_secs(3600))
        .await?;

    Ok(MediaPayload {
        file_name: object_name
            .split('/')
            .last()
            .unwrap_or(&object_name)
            .to_string(),
        file_size: file_size as i64,
        file_type: "application/pdf".to_string(),
        storage_key: object_name,
        url: signed_req.uri().to_string(),
        // Pull bucket name from env instead of hardcoding
        bucket_name: env::var("S3_BUCKET").unwrap_or_else(|_| "unknown".to_string()),
        status: "success".to_string(),
        progress: 100,
    })
}
