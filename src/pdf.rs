//! Template rendering and PDF generation.
//!
//! `render` produces the final PDF as a single `Bytes` buffer that is handed to the HTTP
//! response or the S3 upload without further copies. It is only re-serialized to encrypt.

use crate::{config::AppState, error::AppError, fulgur_engine::PageMm, models::PdfRequest};
use bytes::Bytes;
use lopdf::{
    Document, Object,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};
use minijinja::Environment;
use std::sync::LazyLock;

static TEMPLATE_ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
    env
});

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        Ok(TEMPLATE_ENV.render_str(
            &template_str,
            minijinja::Value::from(minijinja::value::Serde(&data)),
        )?)
    })
    .await?
}

/// Render a request to its final PDF bytes.
pub async fn render(req: PdfRequest, state: &AppState) -> Result<Bytes, AppError> {
    let page = PageMm::parse(&req.width, &req.height, &req.orientation).ok_or_else(|| {
        AppError::InvalidField(
            "width/height/orientation: use lengths like 8.5in, 210mm, 21cm, 612pt and \
             portrait or landscape"
                .into(),
        )
    })?;
    let html = render_template(req.template, req.data).await?;
    let pdf = render_html(html, page, state).await?;
    finalize(pdf, req.password).await
}

/// Render HTML on a blocking thread. The permit moves into that thread, so a render the
/// request has already timed out on keeps counting against concurrency until it really ends.
/// A panic inside the renderer is contained to this request (requires `panic = "unwind"`).
pub async fn render_html(
    html: String,
    page: PageMm,
    state: &AppState,
) -> Result<Vec<u8>, AppError> {
    let permit = state
        .render_permits
        .clone()
        .acquire_owned()
        .await
        .expect("semaphore closed");
    let renderer = state.renderer.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let title = extract_title(&html);
        renderer.render(&html, page, title.as_deref())
    })
    .await;
    match joined {
        Ok(Ok(pdf)) => Ok(pdf),
        Ok(Err(e)) => Err(AppError::Render(e)),
        Err(e) if e.is_panic() => Err(AppError::Render("renderer panicked".into())),
        Err(e) => Err(e.into()),
    }
}

/// `<title>` text, found without allocating a lowercased copy of the document.
fn extract_title(html: &str) -> Option<String> {
    let start = find_ascii_ci(html, "<title", 0)?;
    let open_end = start + html[start..].find('>')? + 1;
    let end = find_ascii_ci(html, "</title", open_end)?;
    let t = html[open_end..end].trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Byte offset of the ASCII `needle` in `hay[from..]`, ignoring ASCII case.
fn find_ascii_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.as_bytes()[from..]
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
        .map(|i| i + from)
}

/// Encrypt if a password was given; otherwise hand the buffer through untouched.
pub async fn finalize(pdf: Vec<u8>, password: Option<String>) -> Result<Bytes, AppError> {
    let Some(password) = password else {
        return Ok(Bytes::from(pdf));
    };
    tokio::task::spawn_blocking(move || {
        let err = |e: lopdf::Error| AppError::PdfProcessing(e.to_string());
        let mut doc = Document::load_mem(&pdf).map_err(err)?;
        drop(pdf);
        ensure_file_id(&mut doc);
        let version = EncryptionVersion::V2 {
            document: &doc,
            owner_password: &password,
            user_password: &password,
            permissions: Permissions::all(),
            key_length: 128,
        };
        let state: EncryptionState = version.try_into().map_err(err)?;
        doc.encrypt(&state).map_err(err)?;
        let mut out = Vec::with_capacity(1024 * 1024);
        doc.save_to(&mut out)
            .map_err(|e| AppError::PdfProcessing(e.to_string()))?;
        Ok(Bytes::from(out))
    })
    .await?
}

/// Encryption keys are derived from the trailer `/ID`; add one if the producer didn't.
fn ensure_file_id(doc: &mut Document) {
    if doc.trailer.get(b"ID").is_err() {
        let id: Vec<u8> = uuid::Uuid::new_v4().as_bytes().to_vec();
        let id = Object::String(id, lopdf::StringFormat::Hexadecimal);
        doc.trailer.set("ID", vec![id.clone(), id]);
    }
}

pub async fn warm_up(state: &AppState) -> Result<(), AppError> {
    tracing::info!("Warming up renderer...");
    let content = "<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>".to_string();
    let letter = PageMm::parse("8.5in", "11in", "portrait").unwrap();
    let pdf = render_html(content, letter, state).await?;
    tokio::fs::write("sample.pdf", &pdf).await?;
    tracing::info!("Renderer warmed up. 'sample.pdf' has been saved to the root directory.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn renders_with_html_autoescape() {
        let out = render_template(
            "<p>{{ v }}</p>".into(),
            serde_json::json!({ "v": "<b>你</b>" }),
        )
        .await
        .unwrap();
        // minijinja 3 also escapes `/` as `&#x2f;`.
        assert_eq!(out, "<p>&lt;b&gt;你&lt;&#x2f;b&gt;</p>");
    }

    #[test]
    fn title_extraction() {
        assert_eq!(
            extract_title("<html><head><TITLE> Inv 1 </TITLE>"),
            Some("Inv 1".into())
        );
        assert_eq!(extract_title("<title></title>"), None);
        assert_eq!(extract_title("<p>no title</p>"), None);
    }

    #[tokio::test]
    async fn finalize_is_zero_copy_without_password() {
        let input = b"%PDF-not-really".to_vec();
        let ptr = input.as_ptr();
        let out = finalize(input, None).await.unwrap();
        assert_eq!(
            out.as_ptr(),
            ptr,
            "buffer must be handed through, not copied"
        );
    }
}
