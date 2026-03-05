use crate::models::MediaPayload;
use dashmap::DashMap;
use futures_util::AsyncWriteExt as _;
use minijinja::{Environment, Template, context};
use opendal::{Operator, services::S3};
use serde::Serialize;
use std::env;
use std::process::Stdio;
use std::sync::LazyLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::{Duration, timeout};

// --- GLOBAL STATE & CACHING ---

static PRINCE_CONCURRENCY: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(8));

static ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env
});

// Cache compiled template ASTs to avoid re-parsing HTML on every request
static TEMPLATE_CACHE: LazyLock<DashMap<String, Template<'static, 'static>>> =
    LazyLock::new(DashMap::new);

static STORAGE_BUCKET: LazyLock<String> =
    LazyLock::new(|| env::var("STORAGE_BUCKET").expect("STORAGE_BUCKET must be set"));

static S3_BUCKET: LazyLock<String> =
    LazyLock::new(|| env::var("S3_BUCKET").unwrap_or_else(|_| "unknown".to_string()));

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
        .bucket(&*STORAGE_BUCKET)
        .region(&env::var("STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_string()));
    Operator::new(builder)
        .expect("Storage init failed")
        .finish()
});

// --- ERROR HANDLING ---

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

// --- CORE LOGIC ---

pub async fn html_to_pdf_to_storage<T: Serialize + Send + Sync + 'static>(
    template_str: String,
    data: T,
    width: String,
    height: String,
    object_name: String,
) -> Result<MediaPayload, PdfError> {
    let exec_timeout = Duration::from_secs(30);
    let limit_20mb = 20 * 1024 * 1024;

    let result = timeout(exec_timeout, async move {
        let html_content = tokio::task::spawn_blocking(move || {
            let value = minijinja::Value::from_serialize(&data);
            if let Some(tmpl) = TEMPLATE_CACHE.get(&template_str) {
                return tmpl.render(context! { ..value });
            }
            let static_template_ptr: &'static str =
                Box::leak(template_str.clone().into_boxed_str());

            let tmpl = ENV.template_from_str(static_template_ptr)?;
            let rendered = tmpl.render(context! { ..value })?;
            TEMPLATE_CACHE.insert(template_str, tmpl);
            Ok(rendered)
        })
        .await??;

        let mut final_size = 0u64;
        let mut upload_started = false;
        let process_result = {
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

            let mut stdin = child.stdin.take().unwrap();
            let mut stdout = child.stdout.take().unwrap();
            let transfer_task = async {
                let mut buffer = Vec::with_capacity(1024 * 1024);
                let mut chunk = [0u8; 64 * 1024];
                let mut streamed_to_s3 = false;
                let mut writer = None;
                loop {
                    let n = stdout.read(&mut chunk).await?;
                    if n == 0 {
                        break;
                    }
                    if !streamed_to_s3 {
                        buffer.extend_from_slice(&chunk[..n]);
                        if buffer.len() > limit_20mb {
                            streamed_to_s3 = true;
                            upload_started = true;
                            let mut w = OP.writer(&object_name).await?.into_futures_async_write();
                            w.write_all(&buffer)
                                .await
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                            writer = Some(w);
                            buffer.clear();
                            buffer.shrink_to_fit(); // Active memory release
                        }
                    } else if let Some(ref mut w) = writer {
                        w.write_all(&chunk[..n])
                            .await
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                    }
                    final_size += n as u64;
                }

                if let Some(mut w) = writer {
                    w.close()
                        .await
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                } else {
                    upload_started = true;
                    OP.write(&object_name, buffer).await?;
                }
                Ok::<(), PdfError>(())
            };
            let input_task = async {
                stdin.write_all(html_content.as_bytes()).await?;
                stdin.flush().await?;
                drop(stdin);
                Ok::<(), PdfError>(())
            };

            tokio::try_join!(transfer_task, input_task)?;
            child.wait().await
        };
        match process_result {
            Ok(status) if status.success() => {}
            Ok(status) => {
                if upload_started {
                    let _ = OP.delete(&object_name).await;
                }
                return Err(PdfError::PrinceStatus(status));
            }
            Err(e) => {
                if upload_started {
                    let _ = OP.delete(&object_name).await;
                }
                return Err(PdfError::Io(e));
            }
        }
        let signed_req = OP
            .presign_read(&object_name, Duration::from_secs(3600))
            .await?;
        let file_name =
            object_name[object_name.rfind('/').map(|i| i + 1).unwrap_or(0)..].to_string();
        Ok(MediaPayload {
            file_name,
            file_size: final_size as i64,
            file_type: "application/pdf".to_string(),
            storage_key: object_name,
            url: signed_req.uri().to_string(),
            bucket_name: S3_BUCKET.clone(),
            status: "success".to_string(),
            progress: 100,
        })
    })
    .await;
    result.map_err(|_| PdfError::Timeout(exec_timeout))?
}
