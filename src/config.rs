use crate::{
    fulgur_engine::{FulgurEngine, InProcessRenderer},
    models::EnginePref,
};
use opendal::{Operator, services::S3};
use std::{env, path::Path, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub storage: Operator,
    pub storage_bucket: Arc<str>,
    /// Caps concurrent renders across both engines (one per core by default).
    pub render_permits: Arc<Semaphore>,
    pub expected_auth: Arc<str>,
    /// Prince executable (`PRINCE_BIN`, default `prince`); `None` when it isn't usable.
    pub prince_bin: Option<Arc<str>>,
    /// In-process renderer (Fulgur).
    pub fulgur: Arc<dyn InProcessRenderer>,
    /// Engine used when a request doesn't ask for one (`PDF_ENGINE`, default `auto`).
    pub default_engine: EnginePref,
    pub render_timeout: Duration,
}

impl AppState {
    pub fn new() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let render_permits = Arc::new(Semaphore::new(cores));
        let api_token = env::var("API_BEARER_TOKEN").expect("API_BEARER_TOKEN must be set");
        let storage_bucket = env::var("STORAGE_BUCKET").expect("STORAGE_BUCKET must be set");
        let mut endpoint = env::var("STORAGE_URL").expect("STORAGE_URL must be set");
        if !endpoint.starts_with("http") {
            endpoint = format!("http://{}", endpoint);
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
        let prince_bin = env::var("PRINCE_BIN").unwrap_or_else(|_| "prince".to_string());
        let render_timeout = env::var("RENDER_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(45));
        let font_dir = env::var("FULGUR_FONT_DIR")
            .unwrap_or_else(|_| "/usr/share/fonts/truetype/noto".to_string());
        let default_engine = env::var("PDF_ENGINE")
            .map(|v| EnginePref::parse(&v).expect("PDF_ENGINE must be auto, fulgur or prince"))
            .unwrap_or(EnginePref::Auto);
        Self {
            storage,
            storage_bucket: storage_bucket.into(),
            render_permits,
            expected_auth: format!("Bearer {}", api_token).into(),
            prince_bin: Some(prince_bin.into()),
            fulgur: Arc::new(FulgurEngine::from_font_dir(Path::new(&font_dir))),
            default_engine,
            render_timeout,
        }
    }
}
