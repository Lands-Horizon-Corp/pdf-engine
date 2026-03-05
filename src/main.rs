use axum::{Router, extract::DefaultBodyLimit, middleware, routing::post};
use std::net::SocketAddr;

mod auth;
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

    if let Err(e) = pdf::warm_up_engine(state.prince_concurrency.clone()).await {
        tracing::error!("Warmup failed: {}", e);
        std::process::exit(1);
    }

    let app = Router::new()
        .route("/api/to-s3", post(handlers::handle_to_s3))
        .route("/api/to-bytes", post(handlers::handle_to_bytes))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "6767".to_string());
    let addr_str = format!("0.0.0.0:{}", port);
    let addr: SocketAddr = addr_str.parse().expect("Invalid address");
    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
