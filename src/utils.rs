use crate::helpers::prepend_blank_page_html;
use crate::models::{MediaPayload, PdfError};
use minijinja::{Environment, context};
use opendal::{Operator, services::S3};
use serde::Serialize;
use std::env;
use std::process::Stdio;
use std::sync::LazyLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::{Duration, timeout};

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
    if (endpoint.starts_with("127.0.0.1") || endpoint.starts_with("localhost"))
        && !endpoint.starts_with("http")
    {
        endpoint = format!("http://{}", endpoint);
    }
    let mut builder = S3::default();
    builder = builder
        .endpoint(&endpoint)
        .access_key_id(&env::var("STORAGE_ACCESS_KEY").expect("STORAGE_ACCESS_KEY must be set"))
        .secret_access_key(&env::var("STORAGE_SECRET_KEY").expect("STORAGE_SECRET_KEY must be set"))
        .bucket(&*STORAGE_BUCKET)
        .region(&env::var("STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_string()));

    if endpoint.contains("amazonaws.com") || endpoint.contains("googleapis.com") {
        builder = builder.enable_virtual_host_style();
    }
    Operator::new(builder)
        .expect("Storage init failed")
        .finish()
});

async fn render_template<T: Serialize + Send + Sync + 'static>(
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

async fn spawn_prince(
    html_content: String,
    width: String,
    height: String,
) -> Result<(tokio::process::ChildStdout, tokio::process::Child), PdfError> {
    let _permit = PRINCE_CONCURRENCY.acquire().await.unwrap();

    let combined_css = format!(
        r#"
    @page {{
        size: {} {};
        margin: 0;
    }}
    "#,
        width, height
    );

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
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    tokio::spawn(async move {
        let _ = stdin.write_all(html_content.as_bytes()).await;
        let _ = stdin.flush().await;
    });

    Ok((stdout, child))
}
pub async fn html_to_pdf_to_storage<T: Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
    width: String,
    height: String,
    object_name: String,
) -> Result<MediaPayload, PdfError> {
    let exec_timeout = Duration::from_secs(45);

    let work = async {
        let html = render_template(template_str, data).await?;
        let html_with_blank = prepend_blank_page_html(&html);
        let (mut stdout, child) = spawn_prince(html_with_blank, width, height).await?;
        let mut raw_bytes = Vec::with_capacity(512 * 1024);
        stdout.read_to_end(&mut raw_bytes).await?;
        child.wait_with_output().await?;
        let mut doc = tokio::task::spawn_blocking(move || {
            crate::helpers::remove_first_page_to_doc(raw_bytes)
                .map_err(|e| PdfError::PrinceStatus(e.to_string()))
        })
        .await??;
        let mut writer = OP.writer(&object_name).await?;
        let mut buffer = Vec::with_capacity(128 * 1024);
        doc.save_to(&mut buffer)
            .map_err(|e| PdfError::PrinceStatus(e.to_string()))?;

        let final_size = buffer.len() as i64;
        writer.write(buffer).await?;
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
            file_type: "application/pdf".to_string(),
            storage_key: object_name,
            url: signed_req.uri().to_string(),
            bucket_name: STORAGE_BUCKET.clone(),
            status: "success".to_string(),
            progress: 100,
        })
    };

    timeout(exec_timeout, work)
        .await
        .map_err(|_| PdfError::Timeout(exec_timeout))?
}

pub async fn html_to_pdf_bytes(
    template_str: String,
    data: Option<serde_json::Value>,
    width: String,
    height: String,
) -> Result<Vec<u8>, PdfError> {
    let data = data.unwrap_or_else(|| serde_json::json!({}));
    let html = render_template(template_str, data).await?;
    let html_with_blank = prepend_blank_page_html(&html);
    let (mut stdout, child) = spawn_prince(html_with_blank, width, height).await?;

    let mut buffer = Vec::new();
    stdout.read_to_end(&mut buffer).await?;

    let output = child.wait_with_output().await?;
    if !output.status.success() {
        return Err(PdfError::PrinceStatus(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    Ok(buffer)
}
