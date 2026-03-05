use crate::models::MediaPayload;
use futures_util::{AsyncWriteExt as _, io::copy};
use minijinja::{Environment, context};
use opendal::{Operator, services::S3};
use serde::Serialize;
use std::env;
use std::process::Stdio;
use std::sync::LazyLock;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::{Duration, timeout};
use tokio_util::compat::TokioAsyncReadCompatExt;

static PRINCE_CONCURRENCY: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(8));

static ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env
});

static OP: LazyLock<Operator> = LazyLock::new(|| {
    let endpoint = env::var("STORAGE_URL").expect("STORAGE_URL must be set");
    let endpoint = if endpoint.starts_with("http") {
        endpoint
    } else {
        format!("http://{}", endpoint)
    };
    let builder = S3::default()
        .endpoint(&endpoint)
        .access_key_id(&env::var("STORAGE_ACCESS_KEY").expect("STORAGE_ACCESS_KEY must be set"))
        .secret_access_key(&env::var("STORAGE_SECRET_KEY").expect("STORAGE_SECRET_KEY must be set"))
        .bucket(&env::var("STORAGE_BUCKET").expect("STORAGE_BUCKET must be set"))
        .region(&env::var("STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_string()));
    Operator::new(builder)
        .expect("Storage init failed")
        .finish()
});

#[derive(thiserror::Error, Debug)]
pub enum PdfError {
    #[error("Template rendering failed: {0}")]
    Template(#[from] minijinja::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] opendal::Error),
    #[error("Prince process failed with status: {0}")]
    PrinceStatus(std::process::ExitStatus),
    #[error("Operation timed out after {0:?}")]
    Timeout(Duration),
    #[error("Internal Task Error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("Generic error: {0}")]
    Other(String),
}

pub async fn html_to_pdf_to_storage<T: Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
    width: String,
    height: String,
    object_name: String,
) -> Result<MediaPayload, PdfError> {
    let exec_timeout = Duration::from_secs(30);
    let result = timeout(exec_timeout, async {
        let html_content = tokio::task::spawn_blocking(move || {
            let value = minijinja::Value::from_serialize(&data);
            ENV.render_str(&template_str, context! { ..value })
        })
        .await??;
        let _permit = PRINCE_CONCURRENCY
            .acquire()
            .await
            .map_err(|e| PdfError::Other(e.to_string()))?;
        let size_css = format!("@page {{ size: {} {}; margin: 0; }}", width, height);
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
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| PdfError::Other("Stdin missing".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PdfError::Other("Stdout missing".into()))?;
        let writer = OP.writer(&object_name).await?;
        let mut remote_writer = writer.into_futures_async_write();
        let (file_size, _) = tokio::try_join!(
            async {
                let mut reader = BufReader::with_capacity(128 * 1024, stdout).compat();
                let bytes = copy(&mut reader, &mut remote_writer).await?;
                remote_writer.close().await?;
                Ok::<u64, PdfError>(bytes)
            },
            async {
                stdin.write_all(html_content.as_bytes()).await?;
                stdin.flush().await?;
                drop(stdin);
                Ok(())
            }
        )?;
        let status = child.wait().await?;
        if !status.success() {
            return Err(PdfError::PrinceStatus(status));
        }
        let signed_req = OP
            .presign_read(&object_name, Duration::from_secs(3600))
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
    })
    .await;
    result.map_err(|_| PdfError::Timeout(exec_timeout))?
}
