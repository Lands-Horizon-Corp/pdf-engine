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
    let expected_header = format!("Bearer {}", state.api_token);
    if let Some(auth_header) = auth_header {
        if auth_header == expected_header {
            return Ok(next.run(req).await);
        }
    }
    Err(AppError::Unauthorized)
}
