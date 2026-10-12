use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MediaPayload {
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub storage_key: String,
    pub url: String,
    pub bucket_name: String,
    pub status: String,
    pub progress: i32,
}

pub struct PdfRequest {
    pub template: String,
    pub data: serde_json::Value,
    pub width: String,
    pub height: String,
    pub orientation: String,
    pub filename: String,
    pub password: Option<String>,
}
