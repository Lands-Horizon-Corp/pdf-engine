use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use serde::Deserialize;
use std::net::SocketAddr;

mod utils;

#[derive(Deserialize)]
struct PdfRequest {
    template: String,
    data: serde_json::Value, // Accepts any JSON object
    width: String,
    height: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/html-pdf", post(handle_pdf));
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_pdf(Json(payload): Json<PdfRequest>) -> impl IntoResponse {
    let filename = format!("output_{}.pdf", chrono::Utc::now().timestamp_millis());

    match utils::html_to_pdf_stream(
        &payload.template,
        &payload.data,
        &payload.width,
        &payload.height,
        &filename,
    )
    .await
    {
        Ok(_) => (StatusCode::OK, format!("Success! Saved to {}", filename)),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Error: {}", e)),
    }
}
