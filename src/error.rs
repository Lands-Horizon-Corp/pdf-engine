use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tokio::time::Duration;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Template rendering failed: {0}")]
    Template(#[from] minijinja::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] opendal::Error),
    #[error("Prince failed: {0}")]
    PrinceStatus(String),
    #[error("Operation timed out after {0:?}")]
    Timeout(Duration),
    #[error("Internal Task Error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            AppError::MissingField(_) | AppError::Template(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            AppError::Timeout(_) => (StatusCode::REQUEST_TIMEOUT, self.to_string()),
            _ => {
                tracing::error!("Internal server error: {:?}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };
        let body = Json(json!({ "error": error_message }));
        (status, body).into_response()
    }
}
