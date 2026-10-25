use crate::{
    assets::AssetPolicy,
    fulgur_engine::{FulgurEngine, InProcessRenderer},
};
use opendal::{Operator, services::S3};
use std::{env, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    /// S3 storage for `POST /api/to-s3`. `None` in IPC mode, and when the server is started
    /// without storage settings.
    pub storage: Option<Operator>,
    pub storage_bucket: Arc<str>,
    /// Caps concurrent renders (one per core by default).
    pub render_permits: Arc<Semaphore>,
    /// Total number of render permits.
    pub render_permit_count: usize,
    /// `Authorization` header value the HTTP API requires. Unused by IPC.
    pub expected_auth: Arc<str>,
    /// HTML → PDF renderer (Fulgur, in-process).
    pub renderer: Arc<dyn InProcessRenderer>,
    pub render_timeout: Duration,
    /// Rules for `<img src>` loading (`ASSET_*`).
    pub assets: Arc<AssetPolicy>,
    /// Browser origins allowed to call the HTTP API (`CORS_ALLOWED_ORIGINS`, comma separated;
    /// `*` allows any). Empty = no CORS headers, which is right for server-to-server callers.
    pub cors_origins: Vec<String>,
}

fn cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn render_timeout() -> Duration {
    env::var("RENDER_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(45))
}

impl AppState {
    /// State for the HTTP server. Needs `API_BEARER_TOKEN`; S3 is optional and `to-s3`
    /// answers `STORAGE_NOT_CONFIGURED` without it. Panics at startup on a bad configuration.
    pub fn from_env() -> Self {
        let api_token = env::var("API_BEARER_TOKEN").expect("API_BEARER_TOKEN must be set");
        let (storage, storage_bucket) = storage_from_env();
        Self {
            expected_auth: format!("Bearer {api_token}").into(),
            storage,
            storage_bucket,
            ..Self::for_ipc()
        }
    }

    /// State for IPC mode: no HTTP, no token, no storage.
    pub fn for_ipc() -> Self {
        let cores = cores();
        Self {
            storage: None,
            storage_bucket: "".into(),
            render_permits: Arc::new(Semaphore::new(cores)),
            render_permit_count: cores,
            expected_auth: "".into(),
            renderer: Arc::new(FulgurEngine::default()),
            render_timeout: render_timeout(),
            assets: Arc::new(AssetPolicy::from_env()),
            cors_origins: env::var("CORS_ALLOWED_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(|o| o.trim().trim_end_matches('/').to_string())
                .filter(|o| !o.is_empty())
                .collect(),
        }
    }
}

/// S3 settings from `STORAGE_*`. All four of bucket, URL and the two keys are needed; with
/// none set there is no storage, with only some set the server refuses to start.
fn storage_from_env() -> (Option<Operator>, Arc<str>) {
    let any = [
        "STORAGE_BUCKET",
        "STORAGE_URL",
        "STORAGE_ACCESS_KEY",
        "STORAGE_SECRET_KEY",
    ]
    .iter()
    .any(|k| env::var(k).is_ok_and(|v| !v.is_empty()));
    if !any {
        return (None, "".into());
    }
    let storage_bucket = env::var("STORAGE_BUCKET").expect("STORAGE_BUCKET must be set");
    let mut endpoint = env::var("STORAGE_URL").expect("STORAGE_URL must be set");
    if !endpoint.starts_with("http") {
        endpoint = format!("http://{endpoint}");
    }
    let mut builder = S3::default()
        .endpoint(&endpoint)
        .access_key_id(&env::var("STORAGE_ACCESS_KEY").expect("STORAGE_ACCESS_KEY set"))
        .secret_access_key(&env::var("STORAGE_SECRET_KEY").expect("STORAGE_SECRET_KEY set"))
        .bucket(&storage_bucket)
        .region(&env::var("STORAGE_REGION").unwrap_or_else(|_| "us-east-1".to_string()));
    if endpoint.contains("amazonaws.com") || endpoint.contains("googleapis.com") {
        builder = builder.enable_virtual_host_style();
    }
    let storage = Operator::new(builder).expect("Storage init failed");
    (Some(storage), storage_bucket.into())
}
