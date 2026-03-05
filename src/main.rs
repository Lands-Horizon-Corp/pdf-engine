use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use std::net::{IpAddr, SocketAddr};
mod models;
mod utils;
use crate::models::PdfRequest;
use std::env;
#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let host: IpAddr = env::var("API_HOST")
        .unwrap_or_else(|_| "127.0.0.1".to_string())
        .parse()
        .expect("Invalid API_HOST format");
    let port: u16 = env::var("API_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("Invalid API_PORT format");
    let addr = SocketAddr::from((host, port));
    let app = Router::new().route("/html-pdf", post(handle_pdf));
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
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
