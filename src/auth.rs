use crate::{config::AppState, error::AppError};
use axum::{
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::Response,
};

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    match auth_header {
        Some(h) if h == &*state.expected_auth => Ok(next.run(req).await),
        _ => Err(AppError::Unauthorized),
    }
}
