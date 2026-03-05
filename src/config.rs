use opendal::{Operator, services::S3};
use std::{env, sync::Arc};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub storage: Operator,
    pub storage_bucket: String,
    pub prince_concurrency: Arc<Semaphore>,
    pub api_token: String,
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
        let storage = Operator::new(builder)
            .expect("Storage init failed")
            .finish();
        Self {
            storage,
            storage_bucket,
            prince_concurrency,
            api_token,
        }
    }
}
