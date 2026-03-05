use serde::Serialize;
use tokio::time::Duration;
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaPayload {
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub storage_key: String,
    pub url: String,
    pub bucket_name: String,
    pub status: String,
    pub progress: i64,
}

#[derive(thiserror::Error, Debug)]
pub enum PdfError {
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
