use opendal::{Operator, services::S3};
use std::{env, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub storage: Operator,
    pub storage_bucket: Arc<str>,
    pub prince_concurrency: Arc<Semaphore>,
    pub expected_auth: Arc<str>,
    pub prince_bin: Arc<str>,
    pub render_timeout: Duration,
}

impl AppState {
    pub fn new() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let prince_concurrency = Arc::new(Semaphore::new(cores));
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
        Self {
            storage,
            storage_bucket: storage_bucket.into(),
            prince_concurrency,
            expected_auth: format!("Bearer {}", api_token).into(),
            prince_bin: prince_bin.into(),
            render_timeout,
        }
    }
}
