use crate::{config::AppState, error::AppError, models::PdfRequest, pdf, storage};
use axum::{
    Json,
    extract::{Multipart, State},
    http::{HeaderMap, header},
    response::IntoResponse,
};

async fn parse_pdf_multipart(mut multipart: Multipart) -> Result<PdfRequest, AppError> {
    let mut template = None;
    let mut data = serde_json::Value::Null;
    let mut width = "8.5in".to_string();
    let mut height = "11in".to_string();
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
        filename,
        password,
    })
}

pub async fn handle_to_s3(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<crate::models::MediaPayload>, AppError> {
    let req = parse_pdf_multipart(multipart).await?;
    let key = format!(
        "pdfs/{}.pdf",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let res = storage::html_to_pdf_to_storage(req, key, &state).await?;
    Ok(Json(res))
}

pub async fn handle_to_bytes(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let req = parse_pdf_multipart(multipart).await?;
    let safe_filename = req.filename.replace('"', "");
    let html = pdf::render_template(req.template, req.data).await?;
    let bytes = pdf::run_prince_and_process(
        html,
        req.width,
        req.height,
        req.password,
        state.prince_concurrency.clone(),
    )
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/pdf".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", safe_filename)
            .parse()
            .unwrap(),
    );

    Ok((headers, bytes))
}
