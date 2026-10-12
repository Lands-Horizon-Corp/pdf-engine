//! Template rendering and engine dispatch.
//!
//! `render` produces the final PDF as a single `Bytes` buffer that is handed to the HTTP
//! response or the S3 upload without further copies. The only re-serialization happens when
//! it's required: stripping Prince's blank lead page, or encrypting.

use crate::{
    config::AppState,
    error::AppError,
    fulgur_engine::PageMm,
    models::{EngineKind, EnginePref, PdfRequest},
    prince, router,
};
use bytes::Bytes;
use lopdf::{
    Document, Object,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};
use minijinja::Environment;
use std::sync::{Arc, LazyLock};

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

pub struct Rendered {
    pub bytes: Bytes,
    pub engine: EngineKind,
}

/// Render a request to its final PDF bytes.
pub async fn render(req: PdfRequest, state: &AppState) -> Result<Rendered, AppError> {
    let html = Arc::new(render_template(req.template, req.data).await?);
    let page = PageMm::parse(&req.width, &req.height, &req.orientation);
    let pref = match req.engine {
        EnginePref::Auto => state.default_engine,
        explicit => explicit,
    };

    let engine = match (pref, &state.prince_bin) {
        (EnginePref::Prince, None) => {
            return Err(AppError::InvalidField(
                "engine: Prince is not available".into(),
            ));
        }
        (EnginePref::Prince, Some(_)) => EngineKind::Prince,
        (EnginePref::Fulgur, _) | (EnginePref::Auto, None) => {
            if page.is_none() {
                return Err(AppError::InvalidField(
                    "width/height/orientation: use lengths like 8.5in, 210mm, 21cm, 612pt".into(),
                ));
            }
            EngineKind::Fulgur
        }
        (EnginePref::Auto, Some(_)) => {
            let reason = match page {
                None => Some("page size"),
                Some(_) => router::prince_reason(&html),
            };
            match reason {
                Some(r) => {
                    tracing::debug!(reason = r, "routing to Prince");
                    EngineKind::Prince
                }
                None => EngineKind::Fulgur,
            }
        }
    };

    if engine == EngineKind::Fulgur {
        let page = page.expect("page validated above");
        let may_fall_back = pref == EnginePref::Auto && state.prince_bin.is_some();
        match render_fulgur(html.clone(), page, state).await {
            Ok(pdf) if may_fall_back && needs_repeated_headers(&html, &pdf) => {
                tracing::debug!("multi-page table with <thead>; re-rendering with Prince");
            }
            Ok(pdf) => {
                let bytes = finalize(pdf, false, req.password).await?;
                return Ok(Rendered { bytes, engine });
            }
            Err(e) if may_fall_back => {
                tracing::warn!("Fulgur failed, falling back to Prince: {e}");
            }
            Err(e) => return Err(e),
        }
    }

    let prince_bin = state
        .prince_bin
        .as_deref()
        .expect("Prince availability checked above");
    let raw = {
        let _permit = state
            .render_permits
            .acquire()
            .await
            .expect("semaphore closed");
        prince::render(&html, &req.width, &req.height, &req.orientation, prince_bin).await?
    };
    let bytes = finalize(raw, true, req.password).await?;
    Ok(Rendered {
        bytes,
        engine: EngineKind::Prince,
    })
}

/// Run Fulgur on a blocking thread. The permit moves into that thread, so a render the
/// request has already timed out on keeps counting against concurrency until it really ends.
/// A panic inside Fulgur is contained to this request (requires `panic = "unwind"`).
async fn render_fulgur(
    html: Arc<String>,
    page: PageMm,
    state: &AppState,
) -> Result<Vec<u8>, AppError> {
    let permit = state
        .render_permits
        .clone()
        .acquire_owned()
        .await
        .expect("semaphore closed");
    let renderer = state.fulgur.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let title = extract_title(&html);
        renderer.render(&html, page, title.as_deref())
    })
    .await;
    match joined {
        Ok(Ok(pdf)) => Ok(pdf),
        Ok(Err(e)) => Err(AppError::Fulgur(e)),
        Err(e) if e.is_panic() => Err(AppError::Fulgur("renderer panicked".into())),
        Err(e) => Err(e.into()),
    }
}

/// Fulgur doesn't repeat `<thead>` on continuation pages; Prince does.
fn needs_repeated_headers(html: &str, pdf: &[u8]) -> bool {
    html.to_ascii_lowercase().contains("<thead")
        && Document::load_mem(pdf)
            .map(|d| d.get_pages().len() > 1)
            .unwrap_or(false)
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = start + lower[start..].find('>')? + 1;
    let end = open_end + lower[open_end..].find("</title")?;
    let t = html[open_end..end].trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Strip the leading blank page and/or encrypt. Without either, the input is returned as is.
pub async fn finalize(
    pdf: Vec<u8>,
    strip_first_page: bool,
    password: Option<String>,
) -> Result<Bytes, AppError> {
    if !strip_first_page && password.is_none() {
        return Ok(Bytes::from(pdf));
    }
    tokio::task::spawn_blocking(move || {
        let err = |e: lopdf::Error| AppError::PdfProcessing(e.to_string());
        let mut doc = Document::load_mem(&pdf).map_err(err)?;
        drop(pdf);
        if strip_first_page && doc.get_pages().len() > 1 {
            doc.delete_pages(&[1]);
            doc.prune_objects();
        }
        if let Some(p) = password {
            ensure_file_id(&mut doc);
            let version = EncryptionVersion::V2 {
                document: &doc,
                owner_password: &p,
                user_password: &p,
                permissions: Permissions::all(),
                key_length: 128,
            };
            let state: EncryptionState = version.try_into().map_err(err)?;
            doc.encrypt(&state).map_err(err)?;
        }
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

pub async fn warm_up(state: &mut AppState) -> Result<(), AppError> {
    tracing::info!("Warming up engines...");
    let content = "<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>";
    let letter = PageMm::parse("8.5in", "11in", "portrait").unwrap();

    let fulgur_pdf = render_fulgur(Arc::new(content.into()), letter, state).await?;
    tracing::info!("Fulgur ready");

    let mut sample = fulgur_pdf;
    if let Some(bin) = state.prince_bin.clone() {
        match prince::render(content, "8.5in", "11in", "portrait", &bin).await {
            Ok(raw) => {
                sample = finalize(raw, true, None).await?.to_vec();
                tracing::info!("Prince ready");
            }
            Err(e) => {
                tracing::warn!("Prince unavailable ({e}); every document will render with Fulgur");
                state.prince_bin = None;
            }
        }
    }

    tokio::fs::write("sample.pdf", &sample).await?;
    tracing::info!("Engines warmed up. 'sample.pdf' has been saved to the root directory.");
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
    async fn finalize_is_zero_copy_without_work() {
        let input = b"%PDF-not-really".to_vec();
        let ptr = input.as_ptr();
        let out = finalize(input, false, None).await.unwrap();
        assert_eq!(
            out.as_ptr(),
            ptr,
            "buffer must be handed through, not copied"
        );
    }
}
