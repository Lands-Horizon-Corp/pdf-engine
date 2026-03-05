use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use std::net::SocketAddr;

mod helpers;
mod models;
mod utils;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let addr: SocketAddr = "0.0.0.0:6767".parse().expect("Invalid address");

    let app = Router::new()
        .route("/api/to-s3", post(handle_to_s3))
        .route("/api/to-bytes", post(handle_to_bytes))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024));

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_to_s3(mut multipart: Multipart) -> impl IntoResponse {
    let mut template = String::new();
    let mut data = serde_json::Value::Null;
    let mut width = "8.5in".to_string();
    let mut height = "11in".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("template") => template = field.text().await.unwrap_or_default(),
            Some("data") => {
                data = field
                    .text()
                    .await
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(data)
            }
            Some("width") => width = field.text().await.unwrap_or(width),
            Some("height") => height = field.text().await.unwrap_or(height),
            _ => {}
        }
    }

    if template.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing template").into_response();
    }

    let key = format!("pdfs/{}.pdf", chrono::Utc::now().timestamp_millis());
    match utils::html_to_pdf_to_storage(template, data, width, height, key).await {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn handle_to_bytes(mut multipart: Multipart) -> impl IntoResponse {
    let mut template = String::new();
    let mut data = None;
    let mut width = "8.5in".to_string();
    let mut height = "11in".to_string();
    let mut filename = "document.pdf".to_string();

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("template") => template = field.text().await.unwrap_or_default(),
            Some("data") => {
                data = field
                    .text()
                    .await
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
            }
            Some("width") => width = field.text().await.unwrap_or(width),
            Some("height") => height = field.text().await.unwrap_or(height),
            Some("filename") => filename = field.text().await.unwrap_or(filename),
            _ => {}
        }
    }
    match utils::html_to_pdf_bytes(template, data, width, height).await {
        Ok(bytes) => {
            let process_result = tokio::task::spawn_blocking(move || {
                let mut doc =
                    helpers::remove_first_page_to_doc(bytes).map_err(|e| e.to_string())?;
                let mut out_buffer = Vec::with_capacity(128 * 1024);
                doc.save_to(&mut out_buffer).map_err(|e| e.to_string())?;
                Ok::<Vec<u8>, String>(out_buffer)
            })
            .await;
            match process_result {
                Ok(Ok(cleaned_bytes)) => {
                    let mut headers = HeaderMap::new();
                    headers.insert(header::CONTENT_TYPE, "application/pdf".parse().unwrap());
                    headers.insert(
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", filename)
                            .parse()
                            .unwrap(),
                    );
                    (StatusCode::OK, headers, cleaned_bytes).into_response()
                }
                _ => (StatusCode::INTERNAL_SERVER_ERROR, "PDF processing failed").into_response(),
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
