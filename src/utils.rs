use crate::models::{MediaPayload, PdfError};
use lopdf::Document;
use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
use minijinja::Environment;
use opendal::Operator;
use opendal::services::S3;
use std::convert::TryInto;
use std::env;
use std::io::Cursor;
use std::process::Stdio;
use std::sync::LazyLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::{Duration, timeout};

static PRINCE_CONCURRENCY: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    Semaphore::new(cores)
});

static STORAGE_BUCKET: LazyLock<String> =
    LazyLock::new(|| env::var("STORAGE_BUCKET").expect("STORAGE_BUCKET must be set"));

static OP: LazyLock<Operator> = LazyLock::new(|| {
    let mut endpoint = env::var("STORAGE_URL").expect("STORAGE_URL must be set");
    if !endpoint.starts_with("http") {
        endpoint = format!("http://{}", endpoint);
    }

    let mut builder = S3::default();
    builder = builder
        .endpoint(&endpoint)
        .access_key_id(&env::var("STORAGE_ACCESS_KEY").expect("KEY set"))
        .secret_access_key(&env::var("STORAGE_SECRET_KEY").expect("SECRET set"))
        .bucket(&*STORAGE_BUCKET)
        .region(&env::var("STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_string()));

    if endpoint.contains("amazonaws.com") || endpoint.contains("googleapis.com") {
        builder = builder.enable_virtual_host_style();
    }
    Operator::new(builder)
        .expect("Storage init failed")
        .finish()
});

/// Forces a blank first page using pure CSS to "catch" the Prince watermark
fn prepend_blank_page(html: &str) -> String {
    format!(
        r#"<div style="page-break-after: always; visibility: hidden;"></div>{}"#,
        html
    )
}

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, PdfError> {
    tokio::task::spawn_blocking(move || {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
        let template = env
            .template_from_str(&template_str)
            .map_err(PdfError::Template)?;
        template
            .render(minijinja::Value::from_serialize(&data))
            .map_err(PdfError::Template)
    })
    .await?
}
async fn run_prince_and_process(
    html: String,
    w: String,
    h: String,
    password: Option<String>,
) -> Result<Vec<u8>, PdfError> {
    let _permit = PRINCE_CONCURRENCY.acquire().await.unwrap();
    let html_with_gap = prepend_blank_page(&html);

    let mut child = Command::new("prince")
        .kill_on_drop(true)
        .args([
            "--no-network",
            "--no-javascript",
            "--silent",
            // Use a faster compression level if Prince supports it in your version
            "--style",
            &format!("data:text/css,@page {{ size: {} {}; margin: 0; }}", w, h),
            "-",
            "-o",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // Move the heavy HTML write to a background task
    tokio::spawn(async move {
        let _ = stdin.write_all(html_with_gap.as_bytes()).await;
        let _ = stdin.flush().await; // Ensure it's pushed through
    });

    // Pre-allocate buffer to avoid reallocations during streaming
    let mut raw_buffer = Vec::with_capacity(1024 * 1024); // 1MB starting point
    stdout.read_to_end(&mut raw_buffer).await?;

    // Check status immediately
    let status = child.wait().await?;
    if !status.success() {
        return Err(PdfError::PrinceStatus("Prince failed to render".into()));
    }

    tokio::task::spawn_blocking(move || {
        let mut doc = Document::load_from(Cursor::new(raw_buffer))
            .map_err(|e| PdfError::PrinceStatus(e.to_string()))?;

        doc.delete_pages(&[1]);

        // Only prune if necessary; it can be slow on very large docs
        doc.prune_objects();

        if let Some(ref p) = password {
            let version = EncryptionVersion::V2 {
                document: &doc,
                owner_password: p,
                user_password: p,
                permissions: Permissions::all(),
                key_length: 128,
            };
            let state: EncryptionState = version
                .try_into()
                .map_err(|e: lopdf::Error| PdfError::PrinceStatus(e.to_string()))?;
            doc.encrypt(&state)
                .map_err(|e: lopdf::Error| PdfError::PrinceStatus(e.to_string()))?;
        }

        // Optimization: Save directly to a pre-allocated Vec
        let mut out = Vec::with_capacity(1024 * 1024);
        doc.save_to(&mut out)
            .map_err(|e| PdfError::PrinceStatus(e.to_string()))?;
        Ok(out)
    })
    .await?
}

pub async fn html_to_pdf_bytes(
    template: String,
    data: serde_json::Value,
    width: String,
    height: String,
    password: Option<String>,
) -> Result<Vec<u8>, PdfError> {
    let html = render_template(template, data).await?;
    run_prince_and_process(html, width, height, password).await
}

pub async fn html_to_pdf_to_storage(
    template: String,
    data: serde_json::Value,
    width: String,
    height: String,
    object_name: String,
    password: Option<String>,
) -> Result<MediaPayload, PdfError> {
    let work = async {
        let html = render_template(template, data).await?;
        let pdf_bytes = run_prince_and_process(html, width, height, password).await?;
        let file_size = pdf_bytes.len() as i64;

        OP.write(&object_name, pdf_bytes)
            .await
            .map_err(PdfError::Storage)?;

        let signed = OP
            .presign_read(&object_name, Duration::from_secs(3600))
            .await?;

        Ok(MediaPayload {
            file_name: object_name
                .split('/')
                .last()
                .unwrap_or("doc.pdf")
                .to_string(),
            file_size,
            file_type: "application/pdf".into(),
            storage_key: object_name,
            url: signed.uri().to_string(),
            bucket_name: STORAGE_BUCKET.clone(),
            status: "success".into(),
            progress: 100,
        })
    };

    timeout(Duration::from_secs(45), work)
        .await
        .map_err(|_| PdfError::Timeout(Duration::from_secs(45)))?
}

pub async fn warm_up_engine() -> Result<(), String> {
    let _ = run_prince_and_process("<html></html>".into(), "1in".into(), "1in".into(), None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
