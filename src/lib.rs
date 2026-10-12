use axum::{Router, extract::DefaultBodyLimit, middleware, routing::post};
use tower_http::compression::{CompressionLayer, CompressionLevel};

pub mod auth;
pub mod config;
pub mod error;
pub mod fulgur_engine;
pub mod handlers;
pub mod models;
pub mod pdf;
pub mod prince;
pub mod router;
pub mod storage;

use config::AppState;

/// Response compression level for gzip / brotli / zstd. Only applied when the client sends
/// `Accept-Encoding`.
pub const COMPRESSION_LEVEL: i32 = 6;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/to-s3", post(handlers::handle_to_s3))
        .route("/api/to-bytes", post(handlers::handle_to_bytes))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(CompressionLayer::new().quality(CompressionLevel::Precise(COMPRESSION_LEVEL)))
        .with_state(state)
}
