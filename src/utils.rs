use crate::models::{MediaPayload, PdfError};
use lopdf::Document;
// StateFactory is required to use .create_state() on the version enum
use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
use minijinja::Environment;
use opendal::{Operator, services::S3};
use std::env;
use std::io::Cursor;
use std::process::Stdio;
use std::sync::LazyLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::{Duration, timeout};

static BLOCKING_OP: LazyLock<opendal::blocking::Operator> = LazyLock::new(|| {
    opendal::blocking::Operator::new((&*OP).clone())
        .expect("Failed to initialize blocking operator")
});

static PRINCE_CONCURRENCY: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let permit_count = if cores > 1 { cores - 1 } else { 1 };
    Semaphore::new(permit_count)
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

fn prepend_blank_page(html: &str) -> String {
    format!(
        r#"<div style="width: 100%; height: 100%; page-break-after: always;"></div>{}"#,
        html
    )
}

pub async fn render_template<T: serde::Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
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

async fn run_prince_to_bytes(html: String, w: String, h: String) -> Result<Vec<u8>, PdfError> {
    let _permit = PRINCE_CONCURRENCY.acquire().await.unwrap();
    let html_with_blank = prepend_blank_page(&html);
    let mut child = Command::new("prince")
        .kill_on_drop(true)
        .args([
            "--no-network",
            "--no-javascript",
            "--silent",
            "--style",
            &format!(
                "data:text/css,{}",
                format!("@page {{ size: {} {}; margin: 0; }}", w, h)
            ),
            "-",
            "-o",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    tokio::spawn(async move {
        let _ = stdin.write_all(html_with_blank.as_bytes()).await;
        drop(stdin);
    });
    let mut buffer = Vec::new();
    stdout.read_to_end(&mut buffer).await?;
    let _ = child.wait().await?;
    Ok(buffer)
}

fn process_and_upload(
    raw_bytes: Vec<u8>,
    object_name: &str,
    password: Option<String>,
) -> Result<i64, PdfError> {
    let mut doc = Document::load_from(Cursor::new(raw_bytes))
        .map_err(|e| PdfError::PrinceStatus(format!("PDF Load Error: {}", e)))?;

    doc.delete_pages(&[1]);
    doc.prune_objects();

    if let Some(ref p) = password {
        let version = EncryptionVersion::V2 {
            document: &doc,
            owner_password: p,
            user_password: p,
            permissions: Permissions::all(),
            key_length: 128,
        };

        // 2. Use try_into() to turn the version into a state
        let state: EncryptionState = version.try_into().map_err(|e: lopdf::Error| {
            PdfError::PrinceStatus(format!("Encryption State: {}", e))
        })?;

        // 3. Pass by reference as requested by the compiler
        doc.encrypt(&state)
            .map_err(|e: lopdf::Error| PdfError::PrinceStatus(format!("Encrypt Apply: {}", e)))?;
    }

    let mut cleaned_buffer = Vec::new();
    doc.save_to(&mut cleaned_buffer)
        .map_err(|e| PdfError::PrinceStatus(e.to_string()))?;

    let final_size = cleaned_buffer.len() as i64;
    let mut writer = (&*BLOCKING_OP)
        .writer(object_name)
        .map_err(PdfError::Storage)?;
    writer
        .write(opendal::Buffer::from(cleaned_buffer))
        .map_err(PdfError::Storage)?;
    writer.close().map_err(PdfError::Storage)?;

    Ok(final_size)
}

pub async fn html_to_pdf_to_storage<T: serde::Serialize + Send + Sync + 'static>(
    template: String,
    data: T,
    width: String,
    height: String,
    object_name: String,
    password: Option<String>,
) -> Result<MediaPayload, PdfError> {
    let obj_name_clone = object_name.clone();
    let work = async move {
        let html = render_template(template, data).await?;
        let raw_pdf = run_prince_to_bytes(html, width, height).await?;
        let final_size = tokio::task::spawn_blocking(move || {
            process_and_upload(raw_pdf, &obj_name_clone, password)
        })
        .await??;
        let signed = OP
            .presign_read(&object_name, Duration::from_secs(3600))
            .await?;
        Ok(MediaPayload {
            file_name: object_name.split('/').last().unwrap().to_string(),
            file_size: final_size,
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
        .map_err(|_| PdfError::Timeout(Duration::from_secs(7200)))?
}

pub async fn html_to_pdf_bytes(
    template: String,
    data: serde_json::Value,
    width: String,
    height: String,
    password: Option<String>,
) -> Result<Vec<u8>, PdfError> {
    // Required to use .try_into() for EncryptionState
    use std::convert::TryInto;

    let html = render_template(template, data).await?;
    let raw_pdf = run_prince_to_bytes(html, width, height).await?;

    tokio::task::spawn_blocking(move || {
        let mut doc = Document::load_from(Cursor::new(raw_pdf))
            .map_err(|e| PdfError::PrinceStatus(format!("PDF Load: {}", e)))?;
        doc.delete_pages(&[1]);
        doc.prune_objects();

        if let Some(ref p) = password {
            let version = EncryptionVersion::V2 {
                document: &doc,
                owner_password: p,
                user_password: p,
                permissions: Permissions::all(),
                key_length: 128,
            };

            // Convert version to state via TryInto trait
            let state: EncryptionState = version
                .try_into()
                .map_err(|e: lopdf::Error| PdfError::PrinceStatus(e.to_string()))?;

            doc.encrypt(&state)
                .map_err(|e: lopdf::Error| PdfError::PrinceStatus(e.to_string()))?;
        }

        let mut out = Vec::new();
        doc.save_to(&mut out)
            .map_err(|e| PdfError::PrinceStatus(e.to_string()))?;
        Ok(out)
    })
    .await?
}

pub async fn warm_up_engine() -> Result<(), String> {
    let dummy_html = "<html><body>Warmup</body></html>".to_string();
    let _ = html_to_pdf_bytes(
        dummy_html,
        serde_json::json!({}),
        "1in".into(),
        "1in".into(),
        None,
    )
    .await
    .map_err(|e| format!("Prince failed: {}", e))?;
    Ok(())
}
