use crate::error::AppError;
use lopdf::{
    Document,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};
use minijinja::Environment;
use std::{io::Cursor, process::Stdio, sync::Arc};
use tokio::fs::File;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Semaphore,
};

fn wrap_and_prepend_blank_page(html: &str) -> String {
    let blank_page =
        r#"<div style="page-break-after: always; visibility: hidden; height: 0;"></div>"#;

    if html.contains("<body") {
        html.replace("<body>", &format!("<body>{}", blank_page))
    } else {
        format!("{}<div class=\"pdf-root\">{}</div>", blank_page, html)
    }
}

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
        let template = env.template_from_str(&template_str)?;
        let rendered = template.render(minijinja::Value::from_serialize(&data))?;
        let xml_safe_rendered: String = rendered
            .chars()
            .map(|c| {
                if c.is_ascii() {
                    c.to_string()
                } else {
                    format!("&#{};", c as u32)
                }
            })
            .collect();

        Ok(xml_safe_rendered)
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
    let xhtml_ready_content = wrap_and_prepend_blank_page(&html);

    let mut child = Command::new("prince")
        .kill_on_drop(true)
        .args([
            "--no-network",
            "--no-javascript",
            "--silent",
            "--style",
            &format!(
                "data:text/css,@page {{ size: {} {}; margin: 0; }} \
                body {{ font-family: 'Noto Sans', 'Noto Sans CJK SC', 'Noto Color Emoji', sans-serif; }}",
                w, h
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
        let _ = stdin.write_all(xhtml_ready_content.as_bytes()).await;
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

        // Delete the blank page prepended earlier
        if doc.get_pages().len() > 1 {
            doc.delete_pages(&[1]);
        }
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
    tracing::info!("Warming up Prince engine and generating test PDF...");

    let test_content = "<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>".into();
    let pdf_bytes =
        run_prince_and_process(test_content, "5in".into(), "5in".into(), None, semaphore).await?;

    let mut file = File::create("sample.pdf")
        .await
        .map_err(|e| AppError::PrinceStatus(format!("Failed to create sample.pdf: {}", e)))?;

    file.write_all(&pdf_bytes)
        .await
        .map_err(|e| AppError::PrinceStatus(format!("Failed to write to sample.pdf: {}", e)))?;

    tracing::info!("Prince engine warmed up. 'sample.pdf' has been saved to the root directory.");
    Ok(())
}
