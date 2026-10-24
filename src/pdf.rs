//! Template rendering and PDF generation.
//!
//! `render` produces the final PDF as a single `Bytes` buffer that is handed to the HTTP
//! response or the S3 upload without further copies. It is only re-serialized to encrypt.

use crate::{
    assets,
    config::AppState,
    error::AppError,
    fulgur_engine::{PageMm, RenderJob},
    models::PdfRequest,
};
use bytes::Bytes;
use lopdf::{
    Document, Object,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};
use std::{sync::Arc, time::Instant};
use tracing::Instrument;

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || crate::template::render(&template_str, data)).await?
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
    let started = Instant::now();
    let html = render_template(req.template, req.data)
        .instrument(tracing::info_span!("template"))
        .await?;
    metrics::histogram!("pdf_template_seconds").record(started.elapsed().as_secs_f64());

    let images = assets::resolve_images(&state.assets, &html)
        .instrument(tracing::info_span!("images"))
        .await;

    let title = extract_title(&html);
    let job = Arc::new(RenderJob {
        html,
        page,
        title,
        images,
    });
    // A corrupt image fails the whole render; retry once without images so a bad remote
    // image never costs the document. The job is shared, not cloned up front, so the common
    // path never copies the HTML.
    let has_images = !job.images.is_empty();
    let pdf = match render_shared(Arc::clone(&job), state)
        .instrument(tracing::info_span!("render"))
        .await
    {
        Err(AppError::Render(e)) if has_images => {
            tracing::warn!(error = %e, "render failed with images; retrying without them");
            metrics::counter!("pdf_render_image_fallbacks_total").increment(1);
            // The render thread has dropped its handle by now, so this moves the job out.
            let mut job = Arc::try_unwrap(job).unwrap_or_else(|shared| (*shared).clone());
            job.images.clear();
            render_html(job, state)
                .instrument(tracing::info_span!("render_without_images"))
                .await?
        }
        other => other?,
    };
    finalize(pdf, req.password)
        .instrument(tracing::info_span!("finalize"))
        .await
}

/// Render HTML on a blocking thread. The permit moves into that thread, so a render the
/// request has already timed out on keeps counting against concurrency until it really ends.
/// A panic inside the renderer is contained to this request (requires `panic = "unwind"`).
pub async fn render_html(job: RenderJob, state: &AppState) -> Result<Vec<u8>, AppError> {
    render_shared(Arc::new(job), state).await
}

async fn render_shared(job: Arc<RenderJob>, state: &AppState) -> Result<Vec<u8>, AppError> {
    let permit = state
        .render_permits
        .clone()
        .acquire_owned()
        .await
        .expect("semaphore closed");
    let renderer = state.renderer.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let started = Instant::now();
        let result = renderer.render(&job);
        drop(job);
        metrics::histogram!("pdf_render_seconds").record(started.elapsed().as_secs_f64());
        result
    })
    .await;
    let outcome = match joined {
        Ok(Ok(pdf)) => Ok(pdf),
        Ok(Err(e)) => Err(AppError::Render(e)),
        Err(e) if e.is_panic() => Err(AppError::Render("renderer panicked".into())),
        Err(e) => Err(e.into()),
    };
    let label = if outcome.is_ok() { "ok" } else { "error" };
    metrics::counter!("pdf_renders_total", "outcome" => label).increment(1);
    outcome
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
    let job = RenderJob {
        html: "<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>".to_string(),
        page: PageMm::parse("8.5in", "11in", "portrait").unwrap(),
        title: None,
        images: Vec::new(),
    };
    let pdf = render_html(job, state).await?;
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
