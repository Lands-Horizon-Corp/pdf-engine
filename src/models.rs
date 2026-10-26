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
    /// Make a tagged (accessible, PDF/UA) PDF: a structure tree, the document language and a title.
    pub accessible: bool,
    /// Archival PDF/A output at this level (`1b`, `2b`, `2u`, `2a`, `3b`, `3a`); `None` for an ordinary PDF.
    pub pdf_a: Option<String>,
    /// Named templates the main template can `{% include %}` or `{% extends %}`.
    pub partials: Vec<(String, String)>,
}
