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

/// Which renderer a request asks for (`engine` form field / `PDF_ENGINE` env var).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePref {
    /// Fulgur unless the template uses CSS Fulgur can't render; then Prince.
    Auto,
    Fulgur,
    Prince,
}

impl EnginePref {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Self::Auto),
            "fulgur" => Some(Self::Fulgur),
            "prince" => Some(Self::Prince),
            _ => None,
        }
    }
}

/// The engine that actually rendered a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Fulgur,
    Prince,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fulgur => "fulgur",
            Self::Prince => "prince",
        }
    }
}

pub struct PdfRequest {
    pub template: String,
    pub data: serde_json::Value,
    pub width: String,
    pub height: String,
    pub orientation: String,
    pub filename: String,
    pub password: Option<String>,
    pub engine: EnginePref,
}
