use axum::{Router, extract::DefaultBodyLimit, middleware, routing::post};

pub mod auth;
pub mod config;
pub mod error;
pub mod handlers;
pub mod models;
pub mod pdf;
pub mod storage;

use config::AppState;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/to-s3", post(handlers::handle_to_s3))
        .route("/api/to-bytes", post(handlers::handle_to_bytes))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(state)
}
