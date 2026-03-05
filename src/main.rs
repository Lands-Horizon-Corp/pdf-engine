use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use serde::Deserialize;
use std::net::SocketAddr;

mod utils;

#[derive(Deserialize)]
struct PdfRequest {
    template: String,
    data: serde_json::Value,
    width: String,
    height: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/html-pdf", post(handle_pdf));
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 PDF Server running on http://{}", addr);
    println!("📦 Storage Bucket: lands-horizon");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_pdf(Json(payload): Json<PdfRequest>) -> impl IntoResponse {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let object_key = format!("pdfs/{}.pdf", timestamp);
    match utils::html_to_pdf_to_storage(
        &payload.template,
        &payload.data,
        &payload.width,
        &payload.height,
        &object_key,
    )
    .await
    {
        Ok(media_payload) => (StatusCode::OK, Json(media_payload)).into_response(),
        Err(e) => {
            eprintln!("PDF Error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error generating/uploading PDF: {}", e),
            )
                .into_response()
        }
    }
}
