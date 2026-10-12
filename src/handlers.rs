use crate::{config::AppState, error::AppError, models::PdfRequest, pdf, storage};
use axum::{
    Json,
    body::Body,
    extract::{Multipart, State},
    http::{HeaderMap, HeaderValue, header},
    response::IntoResponse,
};
use tokio::time::timeout;

async fn parse_pdf_multipart(mut multipart: Multipart) -> Result<PdfRequest, AppError> {
    let mut template = None;
    let mut data = serde_json::Value::Null;
    let mut width = "8.5in".to_string();
    let mut height = "11in".to_string();
    let mut orientation = "portrait".to_string();
    let mut filename = "document.pdf".to_string();
    let mut password = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "template" => {
                template = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| AppError::MissingField("template".into()))?,
                )
            }
            "data" => {
                if let Ok(text) = field.text().await {
                    data = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
                }
            }
            "width" => width = field.text().await.unwrap_or(width),
            "orientation" => orientation = field.text().await.unwrap_or(orientation),
            "height" => height = field.text().await.unwrap_or(height),
            "filename" => filename = field.text().await.unwrap_or(filename),
            "password" => {
                let p = field.text().await.unwrap_or_default();
                if !p.is_empty() {
                    password = Some(p);
                }
            }
            _ => {}
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
    })
}

/// `inline; filename="..."` with quotes and control characters removed, so a hostile
/// filename can neither inject headers nor make header construction fail.
fn content_disposition(filename: &str) -> HeaderValue {
    let safe: String = filename
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect();
    HeaderValue::from_str(&format!("inline; filename=\"{safe}\""))
        .unwrap_or_else(|_| HeaderValue::from_static("inline; filename=\"document.pdf\""))
}

pub async fn handle_to_s3(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let req = parse_pdf_multipart(multipart).await?;
    // The timestamp alone collides under concurrency, silently overwriting another PDF.
    let key = format!(
        "pdfs/{}-{}.pdf",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        uuid::Uuid::new_v4().simple()
    );
    let payload = storage::html_to_pdf_to_storage(req, key, &state).await?;
    Ok(Json(payload))
}

pub async fn handle_to_bytes(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let req = parse_pdf_multipart(multipart).await?;
    let disposition = content_disposition(&req.filename);
    let pdf_bytes = timeout(state.render_timeout, pdf::render(req, &state))
        .await
        .map_err(|_| AppError::Timeout(state.render_timeout))??;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    headers.insert(header::CONTENT_DISPOSITION, disposition);

    Ok((headers, Body::from(pdf_bytes)))
}

/// `GET /health`: liveness for load balancers and Docker. No authentication.
pub async fn handle_health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `POST /api/fonts/reload`: rediscover fonts without restarting.
pub async fn handle_fonts_reload(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let started = std::time::Instant::now();
    let renderer = state.renderer.clone();
    tokio::task::spawn_blocking(move || renderer.reload_fonts()).await?;
    let took_ms = started.elapsed().as_millis() as u64;
    tracing::info!(took_ms, "fonts reloaded");
    metrics::counter!("pdf_font_reloads_total").increment(1);
    Ok(Json(
        serde_json::json!({ "status": "reloaded", "took_ms": took_ms }),
    ))
}

/// `GET /metrics`: Prometheus text format. Registered only when metrics are enabled.
pub async fn handle_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    if let Some(expected) = &state.metrics_token {
        let given = headers
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok());
        if given != Some(&**expected) {
            return Err(AppError::Unauthorized);
        }
    }
    let in_use = state.render_permit_count
        - state
            .render_permits
            .available_permits()
            .min(state.render_permit_count);
    metrics::gauge!("pdf_render_permits_in_use").set(in_use as f64);
    metrics::gauge!("pdf_render_permits_total").set(state.render_permit_count as f64);
    let body = state
        .metrics
        .as_ref()
        .map(|h| h.render())
        .unwrap_or_default();
    Ok((
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4"),
        )],
        body,
    ))
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
