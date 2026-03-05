use serde::Serialize;

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
