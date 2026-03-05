use axum::{Router, extract::DefaultBodyLimit, routing::post};
use std::net::SocketAddr;

mod config;
mod error;
mod handlers;
mod models;
mod pdf;
mod storage;

use config::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let state = AppState::new();

    // Warm up the PDF engine
    if let Err(e) = pdf::warm_up_engine(state.prince_concurrency.clone()).await {
        tracing::error!("Warmup failed: {}", e);
        std::process::exit(1);
    }

    let app = Router::new()
        .route("/api/to-s3", post(handlers::handle_to_s3))
        .route("/api/to-bytes", post(handlers::handle_to_bytes))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(state); // Inject state into our app

    let addr: SocketAddr = "0.0.0.0:6767".parse().expect("Invalid address");
    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
