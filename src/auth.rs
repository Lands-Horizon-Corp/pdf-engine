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
    // Look for the "Authorization" header
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    // Format what we expect it to look like based on our AppState
    let expected_header = format!("Bearer {}", state.api_token);

    // If the header exists and matches, let the request through
    if let Some(auth_header) = auth_header {
        if auth_header == expected_header {
            return Ok(next.run(req).await);
        }
    }

    // Otherwise, bounce them
    Err(AppError::Unauthorized)
}
