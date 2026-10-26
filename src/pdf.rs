//! Template rendering and PDF generation.
//!
//! `render` produces the final PDF as a single `Bytes` buffer that is handed to the HTTP
//! response or the S3 upload without further copies. It is only re-serialized to encrypt.

use crate::{
    assets,
    config::AppState,
    error::AppError,
    fulgur_engine::{PageMm, RenderJob, extract_title},
    models::PdfRequest,
};
use bytes::Bytes;
use std::sync::Arc;

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
    partials: Vec<(String, String)>,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        crate::template::render_with(&template_str, data, &partials)
    })
    .await?
}

/// Render a request to its final PDF bytes.
pub async fn render(req: PdfRequest, state: &AppState) -> Result<Bytes, AppError> {
    render_with_images(req, Vec::new(), state).await
}

/// Like `render`, with images the caller already holds, keyed by the exact `<img src>` value.
/// They win over anything the asset policy would load for the same `src`.
pub async fn render_with_images(
    req: PdfRequest,
    supplied: Vec<(String, Arc<Vec<u8>>)>,
    state: &AppState,
) -> Result<Bytes, AppError> {
    let page = PageMm::parse_request(&req.width, &req.height, &req.orientation)?;
    let pdf_a = crate::fulgur_engine::parse_pdf_a(req.pdf_a.as_deref(), req.password.as_deref())?;
    let html = render_template(req.template, req.data, req.partials).await?;

    let mut images = if supplied.is_empty() {
        assets::resolve_images(&state.assets, &html).await
    } else {
        // Fetch only what the caller did not bring.
        let missing: Vec<String> = crate::image_util::image_sources(&html)
            .into_iter()
            .filter(|s| !supplied.iter().any(|(k, _)| k == s))
            .collect();
        let mut loaded = assets::resolve_sources(&state.assets, missing).await;
        loaded.extend(supplied);
        loaded
    };
    images.retain(|(_, b)| crate::image_util::is_supported_image(b));
    let html = crate::image_util::size_images(&html, &images).unwrap_or(html);

    let font_faces = assets::resolve_font_faces(&state.assets, &html).await;

    let title = extract_title(&html);
    let job = Arc::new(RenderJob {
        html,
        page,
        title,
        images,
        font_faces,
        accessible: req.accessible,
        pdf_a,
        date: None,
    });
    // A corrupt image fails the whole render; retry once without images so a bad remote
    // image never costs the document. The job is shared, not cloned up front, so the common
    // path never copies the HTML.
    let has_extras = !job.images.is_empty() || !job.font_faces.is_empty();
    let pdf = match render_shared(Arc::clone(&job), state).await {
        Err(AppError::Render(e)) if has_extras => {
            tracing::warn!(error = %e, "render failed with images or fonts; retrying without them");
            // The render thread has dropped its handle by now, so this moves the job out.
            let mut job = Arc::try_unwrap(job).unwrap_or_else(|shared| (*shared).clone());
            job.images.clear();
            job.font_faces.clear();
            render_html(job, state).await?
        }
        other => other?,
    };
    finalize(pdf, req.password).await
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
        let result = renderer.render(&job);
        drop(job);
        result
    })
    .await;
    match joined {
        Ok(Ok(pdf)) => Ok(pdf),
        Ok(Err(e)) => Err(crate::fulgur_engine::render_error(e)),
        Err(e) if e.is_panic() => Err(AppError::Render("renderer panicked".into())),
        Err(e) => Err(e.into()),
    }
}

/// Encrypt if a password was given; otherwise hand the buffer through untouched.
pub async fn finalize(pdf: Vec<u8>, password: Option<String>) -> Result<Bytes, AppError> {
    let Some(password) = password else {
        return Ok(Bytes::from(pdf));
    };
    tokio::task::spawn_blocking(move || {
        crate::pdf_encrypt::encrypt(&pdf, &password)
            .map(Bytes::from)
            .map_err(AppError::PdfProcessing)
    })
    .await?
}

/// Render a small document so fonts and caches are ready before the first real request.
///
/// `wide` includes scripts and emoji, so the system's fonts are looked at and cached too (the server does this); the
/// Node, Electron and Tauri engine warms up on plain text and leaves the system's fonts alone until a document needs them.
pub async fn warm_up_in_memory(state: &AppState, wide: bool) -> Result<Vec<u8>, AppError> {
    let html = if wide {
        "<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>"
    } else {
        "<h1>Warm-up</h1><p>Warmup Test: &euro; | Invoice 1,234.50</p><table><tr><td>a</td><td>b</td></tr></table>"
    };
    let job = RenderJob {
        html: html.to_string(),
        page: PageMm::parse("8.5in", "11in", "portrait").unwrap(),
        title: None,
        images: Vec::new(),
        font_faces: Vec::new(),
        accessible: false,
        pdf_a: None,
        date: None,
    };
    render_html(job, state).await
}

/// Warm up and save the result as `sample.pdf` in the working directory (HTTP server only).
pub async fn warm_up(state: &AppState) -> Result<(), AppError> {
    tracing::info!("Warming up renderer...");
    let pdf = warm_up_in_memory(state, true).await?;
    tokio::fs::write("sample.pdf", &pdf).await?;
    tracing::info!("Renderer warmed up. 'sample.pdf' has been saved to the working directory.");
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
            Vec::new(),
        )
        .await
        .unwrap();
        // minijinja 3 also escapes `/` as `&#x2f;`.
        assert_eq!(out, "<p>&lt;b&gt;你&lt;&#x2f;b&gt;</p>");
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
