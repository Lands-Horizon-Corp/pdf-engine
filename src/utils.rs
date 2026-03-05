use crate::models::MediaPayload;
use futures_util::AsyncWriteExt as _;
use minijinja::{Environment, context};
use once_cell::sync::Lazy;
use opendal::{Operator, services::S3};
use serde::Serialize;
use std::env;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio_util::compat::TokioAsyncReadCompatExt;

// Restrict Prince to 8 concurrent processes to protect CPU/Memory
static PRINCE_CONCURRENCY: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(8));

// MiniJinja Environment: Faster than Handlebars
static ENV: Lazy<Environment<'static>> = Lazy::new(|| {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    // env.add_template("report", include_str!("../templates/report.html")).unwrap();
    env
});
// OpenDAL Operator for S3 Streaming
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
    // 1. High-speed Template Rendering
    let html_content = tokio::task::spawn_blocking(move || {
        let value = minijinja::Value::from_serialize(&data);
        ENV.render_str(&template_str, context! { ..value })
    })
    .await??;

    // 2. Resource management
    let _permit = PRINCE_CONCURRENCY.acquire().await?;
    let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);

    // 3. Setup Prince Process
    let mut child = Command::new("prince")
        .args([
            "-",
            "-o",
            "-",
            "--no-network",
            "--silent",
            "--style",
            &format!("data:text/css,{}", size_css),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // Usually better to ignore or log separately
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open stdout")?;

    // 4. Pipe HTML to Prince in background
    tokio::spawn(async move {
        let _ = stdin.write_all(html_content.as_bytes()).await;
        let _ = stdin.flush().await;
        drop(stdin); // Important: tells Prince we're done sending data
    });

    // 5. Stream from Prince STDOUT directly to S3
    let writer = OP.writer(&object_name).await?;
    let mut remote_writer = writer.into_futures_async_write();

    // Use a large 128KB buffer to minimize context switching overhead
    let mut reader = tokio::io::BufReader::with_capacity(128 * 1024, stdout).compat();

    // The core streaming transfer
    let file_size = match futures_util::io::copy(&mut reader, &mut remote_writer).await {
        Ok(size) => {
            remote_writer.close().await?;
            size
        }
        Err(e) => {
            let _ = child.kill().await; // Kill process if upload fails
            return Err(e.into());
        }
    };

    // 6. Ensure Prince finished successfully
    let status = child.wait().await?;
    if !status.success() {
        return Err("Prince PDF generation failed".into());
    }

    // 7. Metadata & URL Generation
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
        bucket_name: env::var("S3_BUCKET").unwrap_or_else(|_| "unknown".to_string()),
        status: "success".to_string(),
        progress: 100,
    })
}
