use crate::{config::AppState, error::AppError, models::PdfRequest, pdf, storage};
use actix_multipart::{Field, Multipart};
use actix_web::{HttpResponse, http::header, web};
use futures_util::TryStreamExt;
use tokio::time::timeout;

/// Largest accepted multipart body (all fields together).
pub const MAX_BODY_BYTES: usize = 25 * 1024 * 1024;

/// All bytes of one field, counted against the shared body budget.
async fn field_bytes(field: &mut Field, budget: &mut usize) -> Result<Vec<u8>, AppError> {
    let mut out = Vec::new();
    while let Some(chunk) = field
        .try_next()
        .await
        .map_err(|_| AppError::InvalidField("malformed multipart body".into()))?
    {
        *budget = budget
            .checked_sub(chunk.len())
            .ok_or(AppError::PayloadTooLarge(MAX_BODY_BYTES))?;
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn field_text(field: &mut Field, budget: &mut usize) -> Result<Option<String>, AppError> {
    Ok(String::from_utf8(field_bytes(field, budget).await?).ok())
}

/// JSON `data` field, parsed in place with SIMD. Invalid JSON becomes `null`, as before.
fn parse_data(mut bytes: Vec<u8>) -> serde_json::Value {
    simd_json::serde::from_slice(&mut bytes).unwrap_or(serde_json::Value::Null)
}

async fn parse_pdf_multipart(mut multipart: Multipart) -> Result<PdfRequest, AppError> {
    let mut template = None;
    let mut data = serde_json::Value::Null;
    let mut width = "8.5in".to_string();
    let mut height = "11in".to_string();
    let mut orientation = "portrait".to_string();
    let mut filename = "document.pdf".to_string();
    let mut password = None;
    let mut partials = Vec::new();
    let mut budget = MAX_BODY_BYTES;

    while let Ok(Some(mut field)) = multipart.try_next().await {
        match field.name().unwrap_or_default() {
            "template" => {
                template = Some(
                    field_text(&mut field, &mut budget)
                        .await?
                        .ok_or_else(|| AppError::MissingField("template".into()))?,
                )
            }
            "data" => data = parse_data(field_bytes(&mut field, &mut budget).await?),
            "partials" => {
                let bytes = field_bytes(&mut field, &mut budget).await?;
                let map: std::collections::BTreeMap<String, String> =
                    serde_json::from_slice(&bytes).map_err(|_| {
                        AppError::InvalidField(
                            "partials must be a JSON object of name: template".into(),
                        )
                    })?;
                partials = map.into_iter().collect();
            }
            "width" => width = field_text(&mut field, &mut budget).await?.unwrap_or(width),
            "orientation" => {
                orientation = field_text(&mut field, &mut budget)
                    .await?
                    .unwrap_or(orientation)
            }
            "height" => height = field_text(&mut field, &mut budget).await?.unwrap_or(height),
            "filename" => {
                filename = field_text(&mut field, &mut budget)
                    .await?
                    .unwrap_or(filename)
            }
            "password" => {
                let p = field_text(&mut field, &mut budget)
                    .await?
                    .unwrap_or_default();
                if !p.is_empty() {
                    password = Some(p);
                }
            }
            _ => {
                field_bytes(&mut field, &mut budget).await?;
            }
        }
    }

    Ok(PdfRequest {
        template: template.ok_or_else(|| AppError::MissingField("template".into()))?,
        data,
        width,
        height,
        orientation,
        filename,
        password,
        partials,
    })
}

/// `inline; filename="..."` with quotes and control characters removed, so a hostile
/// filename can neither inject headers nor make header construction fail.
fn content_disposition(filename: &str) -> header::HeaderValue {
    let safe: String = filename
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect();
    header::HeaderValue::from_str(&format!("inline; filename=\"{safe}\""))
        .unwrap_or_else(|_| header::HeaderValue::from_static("inline; filename=\"document.pdf\""))
}

pub async fn handle_to_s3(
    state: web::Data<AppState>,
    multipart: Multipart,
) -> Result<HttpResponse, AppError> {
    let req = parse_pdf_multipart(multipart).await?;
    // The timestamp alone collides under concurrency, silently overwriting another PDF.
    let key = format!(
        "pdfs/{}-{}.pdf",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        uuid::Uuid::new_v4().simple()
    );
    let payload = storage::html_to_pdf_to_storage(req, key, &state).await?;
    Ok(HttpResponse::Ok().json(payload))
}

pub async fn handle_to_bytes(
    state: web::Data<AppState>,
    multipart: Multipart,
) -> Result<HttpResponse, AppError> {
    let req = parse_pdf_multipart(multipart).await?;
    let disposition = content_disposition(&req.filename);
    let pdf_bytes = timeout(state.render_timeout, pdf::render(req, &state))
        .await
        .map_err(|_| AppError::Timeout(state.render_timeout))??;

    Ok(HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/pdf"))
        .insert_header((header::CONTENT_DISPOSITION, disposition))
        .body(pdf_bytes))
}

/// `GET /health`: liveness for load balancers and Docker. No authentication.
pub async fn handle_health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `POST /api/fonts/reload`: rediscover fonts without restarting.
pub async fn handle_fonts_reload(state: web::Data<AppState>) -> Result<HttpResponse, AppError> {
    let started = std::time::Instant::now();
    let renderer = state.renderer.clone();
    tokio::task::spawn_blocking(move || renderer.reload_fonts()).await?;
    let took_ms = started.elapsed().as_millis() as u64;
    tracing::info!(took_ms, "fonts reloaded");
    Ok(HttpResponse::Ok().json(serde_json::json!({ "status": "reloaded", "took_ms": took_ms })))
}

#[cfg(test)]
mod tests {
    use super::content_disposition;

    #[test]
    fn hostile_filenames_never_break_the_header() {
        assert_eq!(content_disposition("a.pdf"), "inline; filename=\"a.pdf\"");
        assert_eq!(
            content_disposition("evil\r\nSet-Cookie: x=1.pdf"),
            "inline; filename=\"evilSet-Cookie: x=1.pdf\""
        );
        assert_eq!(
            content_disposition("q\"uo\0te.pdf"),
            "inline; filename=\"quote.pdf\""
        );
        assert_eq!(
            content_disposition("报告.pdf").as_bytes(),
            "inline; filename=\"报告.pdf\"".as_bytes()
        );
    }
}
