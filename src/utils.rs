use crate::models::{MediaPayload, PdfError};
use lopdf::Document;
use minijinja::{Environment, context};
use opendal::{Operator, services::S3};
use std::env;
use std::io::Cursor;
use std::process::Stdio;
use std::sync::LazyLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::{Duration, timeout};
use tokio_util::bytes::Bytes;

static PRINCE_CONCURRENCY: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(8));

static ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env
});

static STORAGE_BUCKET: LazyLock<String> =
    LazyLock::new(|| env::var("STORAGE_BUCKET").expect("STORAGE_BUCKET must be set"));

static OP: LazyLock<Operator> = LazyLock::new(|| {
    let mut endpoint = env::var("STORAGE_URL").expect("STORAGE_URL must be set");
    if !endpoint.starts_with("http") {
        if endpoint.contains("127.0.0.1") || endpoint.contains("localhost") {
            endpoint = format!("http://{}", endpoint);
        } else {
            endpoint = format!("https://{}", endpoint);
        }
    }
    let mut builder = S3::default();
    builder = builder
        .endpoint(&endpoint)
        .access_key_id(&env::var("STORAGE_ACCESS_KEY").expect("STORAGE_ACCESS_KEY set"))
        .secret_access_key(&env::var("STORAGE_SECRET_KEY").expect("STORAGE_SECRET_KEY set"))
        .bucket(&*STORAGE_BUCKET)
        .region(&env::var("STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_string()));
    // In OpenDAL, path-style is the default IF virtual_host_style is NOT enabled.
    if endpoint.contains("amazonaws.com") || endpoint.contains("googleapis.com") {
        builder = builder.enable_virtual_host_style();
    }
    Operator::new(builder)
        .expect("Storage init failed")
        .finish()
});

/// 1. Prepend the blank page so Prince watermarks THIS page
fn prepend_blank_page(html: &str) -> String {
    let blank_page = r#"<div style="width: 100%; height: 100%; page-break-after: always;"></div>"#;
    format!("{}{}", blank_page, html)
}

pub async fn render_template<T: serde::Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
) -> Result<String, PdfError> {
    tokio::task::spawn_blocking(move || {
        let value = minijinja::Value::from_serialize(&data);
        ENV.render_str(&template_str, context! { ..value })
            .map_err(PdfError::Template)
    })
    .await?
}

async fn run_prince_to_bytes(
    html_content: String,
    width: String,
    height: String,
) -> Result<Vec<u8>, PdfError> {
    let _permit = PRINCE_CONCURRENCY.acquire().await.unwrap();

    let html_with_blank = prepend_blank_page(&html_content);
    let combined_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);

    let mut child = Command::new("prince")
        .args([
            "-",
            "-o",
            "-",
            "--no-network",
            "--silent",
            "--style",
            &format!("data:text/css,{}", combined_css),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().expect("Stdin failed");
    let mut stdout = child.stdout.take().expect("Stdout failed");

    // Feed HTML in background
    tokio::spawn(async move {
        let _ = stdin.write_all(html_with_blank.as_bytes()).await;
        let _ = stdin.flush().await;
        drop(stdin);
    });

    let mut buffer = Vec::new();
    stdout.read_to_end(&mut buffer).await?;
    let _ = child.wait().await?;

    Ok(buffer)
}

/// 3. Remove the watermarked page (CPU intensive, so use spawn_blocking)
fn remove_first_page_logic(input: Vec<u8>) -> Result<Vec<u8>, PdfError> {
    let mut doc = Document::load_from(Cursor::new(input))
        .map_err(|e| PdfError::PrinceStatus(format!("PDF Load: {}", e)))?;

    // Delete page 1 (The Prince watermark page)
    doc.delete_pages(&[1]);
    doc.prune_objects();

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| PdfError::PrinceStatus(format!("PDF Save: {}", e)))?;

    Ok(out)
}

// --- PUBLIC FUNCTIONS ---

pub async fn html_to_pdf_to_storage<T: serde::Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
    width: String,
    height: String,
    object_name: String,
) -> Result<MediaPayload, PdfError> {
    let exec_timeout = Duration::from_secs(45);

    let work = async {
        let html = render_template(template_str, data).await?;
        let raw_pdf = run_prince_to_bytes(html, width, height).await?;

        // Process in blocking thread
        let cleaned_pdf =
            tokio::task::spawn_blocking(move || remove_first_page_logic(raw_pdf)).await??;

        let final_size = cleaned_pdf.len() as i64;

        // Upload cleaned PDF
        let mut writer = OP.writer(&object_name).await?;
        writer.write(Bytes::from(cleaned_pdf)).await?;
        writer.close().await?;

        let signed_req = OP
            .presign_read(&object_name, Duration::from_secs(3600))
            .await?;

        Ok(MediaPayload {
            file_name: object_name
                .split('/')
                .last()
                .unwrap_or(&object_name)
                .to_string(),
            file_size: final_size,
            file_type: "application/pdf".into(),
            storage_key: object_name,
            url: signed_req.uri().to_string(),
            bucket_name: STORAGE_BUCKET.clone(),
            status: "success".into(),
            progress: 100,
        })
    };

    timeout(exec_timeout, work)
        .await
        .map_err(|_| PdfError::Timeout(exec_timeout))?
}

pub async fn html_to_pdf_bytes(
    template_str: String,
    data: serde_json::Value,
    width: String,
    height: String,
) -> Result<Vec<u8>, PdfError> {
    let html = render_template(template_str, data).await?;
    let raw_pdf = run_prince_to_bytes(html, width, height).await?;

    tokio::task::spawn_blocking(move || remove_first_page_logic(raw_pdf)).await?
}
