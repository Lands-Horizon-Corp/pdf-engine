use axum::{
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use std::time::Instant;
use tower_http::compression::{CompressionLayer, CompressionLevel};

pub mod assets;
pub mod auth;
pub mod config;
pub mod error;
pub mod fonts;
pub mod fulgur_engine;
pub mod handlers;
pub mod models;
pub mod pdf;
pub mod storage;
pub mod telemetry;

use config::AppState;

/// Response compression level for gzip / brotli / zstd. Only applied when the client sends
/// `Accept-Encoding`.
pub const COMPRESSION_LEVEL: i32 = 6;

pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/to-s3", post(handlers::handle_to_s3))
        .route("/api/to-bytes", post(handlers::handle_to_bytes))
        .route("/api/fonts/reload", post(handlers::handle_fonts_reload))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ));
    let mut router = Router::new()
        .route("/health", get(handlers::handle_health))
        .merge(api);
    if state.metrics.is_some() {
        router = router.route("/metrics", get(handlers::handle_metrics));
    }
    router
        .layer(middleware::from_fn(track_http))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(CompressionLayer::new().quality(CompressionLevel::Precise(COMPRESSION_LEVEL)))
        .with_state(state)
}

/// Request count and latency per route and status (no-ops unless metrics are enabled).
async fn track_http(req: Request, next: Next) -> Response {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_string(), |p| p.as_str().to_string());
    let method = req.method().to_string();
    let started = Instant::now();
    let response = next.run(req).await;
    let status = response.status().as_u16().to_string();
    let labels = [("route", route), ("method", method), ("status", status)];
    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_request_duration_seconds", &labels[..2])
        .record(started.elapsed().as_secs_f64());
    response
}
