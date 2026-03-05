use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use std::net::SocketAddr;

mod models;
mod utils;

use crate::models::PdfRequest;

#[tokio::main]
async fn main() {
    let app = Router::new().route("/html-pdf", post(handle_pdf));
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 PDF Server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind port");
    axum::serve(listener, app).await.expect("Server failed");
}

async fn handle_pdf(Json(payload): Json<PdfRequest>) -> impl IntoResponse {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let object_key = format!("pdfs/{}.pdf", timestamp);
    match utils::html_to_pdf_to_storage(
        payload.template,
        payload.data,
        payload.width,
        payload.height,
        object_key,
    )
    .await
    {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            // Return the error message in the response body for easier debugging
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
