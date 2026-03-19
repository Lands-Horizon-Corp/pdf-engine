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

/// Wraps everything in a single root <div> to satisfy XHTML requirements
fn wrap_and_prepend_blank_page(html: &str) -> String {
    format!(
        r#"<div class="pdf-root">
            <div style="page-break-after: always; visibility: hidden;"></div>
            {}
        </div>"#,
        html
    )
}

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        let mut env = Environment::new();
        // Use Html escaping for the data inside the template
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);

        let template = env.template_from_str(&template_str)?;
        let rendered = template.render(minijinja::Value::from_serialize(&data))?;

        // Convert ONLY non-ASCII characters to numeric entities.
        // This keeps <html> tags intact while turning ₱ into &#8369;
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

    // Crucial: Use the new wrapper to prevent "Extra content" errors
    let xhtml_ready_content = wrap_and_prepend_blank_page(&html);

    let mut child = Command::new("prince")
        .kill_on_drop(true)
        .args([
            "-i", "xhtml", // Stricter XML parsing for entities
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
    tracing::info!("Warming up Prince engine...");
    // Warm up with a simple span to keep the XML tree clean
    run_prince_and_process(
        "<span>warmup</span>".into(),
        "1in".into(),
        "1in".into(),
        None,
        semaphore,
    )
    .await?;
    Ok(())
}
