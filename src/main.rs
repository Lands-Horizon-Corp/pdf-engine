use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use std::net::SocketAddr;

mod models; // Ensure your MediaPayload and PdfError are defined here
mod utils;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // 1. ENGINE WARM-UP
    // Ensures Prince, Jinja, and S3 are ready before accepting traffic.
    if let Err(e) = utils::warm_up_engine().await {
        eprintln!("FATAL: Engine warm-up failed! {}", e);
        std::process::exit(1);
    }

    let addr: SocketAddr = "0.0.0.0:6767".parse().expect("Invalid address");

    let app = Router::new()
        .route("/api/to-s3", post(handle_to_s3))
        .route("/api/to-bytes", post(handle_to_bytes))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024));

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("🚀 PDF Engine listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

// --- HANDLERS ---

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
    let mut data = serde_json::Value::Null;
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
                    .unwrap_or(data)
            }
            Some("width") => width = field.text().await.unwrap_or(width),
            Some("height") => height = field.text().await.unwrap_or(height),
            Some("filename") => filename = field.text().await.unwrap_or(filename),
            _ => {}
        }
    }

    match utils::html_to_pdf_bytes(template, data, width, height).await {
        Ok(bytes) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "application/pdf".parse().unwrap());
            headers.insert(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename)
                    .parse()
                    .unwrap(),
            );
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
