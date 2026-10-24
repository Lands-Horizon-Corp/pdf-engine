use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde_json::json;
use tokio::time::Duration;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid field: {0}")]
    InvalidField(String),
    #[error("Request body exceeds the {0}-byte limit")]
    PayloadTooLarge(usize),
    #[error("Template rendering failed: {0}")]
    Template(#[from] minijinja::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] opendal::Error),
    #[error("Render failed: {0}")]
    Render(String),
    #[error("PDF post-processing failed: {0}")]
    PdfProcessing(String),
    #[error("Operation timed out after {0:?}")]
    Timeout(Duration),
    #[error("Internal Task Error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::MissingField(_) | AppError::InvalidField(_) | AppError::Template(_) => {
                StatusCode::BAD_REQUEST
            }
            AppError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::Timeout(_) => StatusCode::REQUEST_TIMEOUT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();
        let error_message = match self {
            AppError::Unauthorized => "Invalid or missing Bearer token".to_string(),
            _ if status == StatusCode::INTERNAL_SERVER_ERROR => {
                tracing::error!("Internal server error: {:?}", self);
                "Internal server error".to_string()
            }
            _ => self.to_string(),
        };
        HttpResponse::build(status).json(json!({ "error": error_message }))
    }
}
