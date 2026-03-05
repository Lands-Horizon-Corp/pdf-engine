use crate::error::AppError;
use lopdf::{
    Document,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};
use minijinja::Environment;
use std::{io::Cursor, process::Stdio, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Semaphore,
};

fn prepend_blank_page(html: &str) -> String {
    format!(
        r#"<div style="page-break-after: always; visibility: hidden;"></div>{}"#,
        html
    )
}

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
        let template = env.template_from_str(&template_str)?;
        let rendered = template.render(minijinja::Value::from_serialize(&data))?;
        Ok(rendered)
    })
    .await?
}

pub async fn run_prince_and_process(
    html: String,
    w: String,
    h: String,
    password: Option<String>,
    semaphore: Arc<Semaphore>,
) -> Result<Vec<u8>, AppError> {
    let _permit = semaphore.acquire().await.unwrap();
    let html_with_gap = prepend_blank_page(&html);

    let mut child = Command::new("prince")
        .kill_on_drop(true)
        .args([
            "--no-network",
            "--no-javascript",
            "--silent",
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

    tokio::spawn(async move {
        let _ = stdin.write_all(html_with_gap.as_bytes()).await;
        let _ = stdin.flush().await;
    });

    let mut raw_buffer = Vec::with_capacity(1024 * 1024);
    stdout.read_to_end(&mut raw_buffer).await?;

    let status = child.wait().await?;
    if !status.success() {
        return Err(AppError::PrinceStatus("Prince failed to render".into()));
    }

    tokio::task::spawn_blocking(move || {
        let mut doc = Document::load_from(Cursor::new(raw_buffer))
            .map_err(|e| AppError::PrinceStatus(e.to_string()))?;
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
            let state: EncryptionState = version
                .try_into()
                .map_err(|e: lopdf::Error| AppError::PrinceStatus(e.to_string()))?;
            doc.encrypt(&state)
                .map_err(|e: lopdf::Error| AppError::PrinceStatus(e.to_string()))?;
        }

        let mut out = Vec::with_capacity(1024 * 1024);
        doc.save_to(&mut out)
            .map_err(|e| AppError::PrinceStatus(e.to_string()))?;
        Ok(out)
    })
    .await?
}

pub async fn warm_up_engine(semaphore: Arc<Semaphore>) -> Result<(), AppError> {
    tracing::info!("Warming up Prince engine...");
    run_prince_and_process(
        "<html></html>".into(),
        "1in".into(),
        "1in".into(),
        None,
        semaphore,
    )
    .await?;
    Ok(())
}
