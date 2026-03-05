use axum::{Json, Router, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

mod utils;

#[derive(Deserialize, Serialize)]
struct Invoice {
    customer_name: String,
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/html-pdf", post(handle_pdf));
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_pdf(Json(payload): Json<Invoice>) -> impl IntoResponse {
    let filename = format!("invoice_{}.pdf", chrono::Utc::now().timestamp_millis());
    // Using your stream utility
    match utils::html_to_pdf_stream("invoice", &payload, "210mm", "297mm", &filename).await {
        Ok(_) => format!("Success! Saved to {}", filename),
        Err(e) => format!("Error: {}", e),
    }
}
