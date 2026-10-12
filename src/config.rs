use crate::{
    assets::AssetPolicy,
    fulgur_engine::{FulgurEngine, InProcessRenderer},
};
use metrics_exporter_prometheus::PrometheusHandle;
use opendal::{Operator, services::S3};
use std::{env, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub storage: Operator,
    pub storage_bucket: Arc<str>,
    /// Caps concurrent renders (one per core by default).
    pub render_permits: Arc<Semaphore>,
    /// Total number of render permits (for the in-use gauge).
    pub render_permit_count: usize,
    pub expected_auth: Arc<str>,
    /// HTML → PDF renderer (Fulgur, in-process).
    pub renderer: Arc<dyn InProcessRenderer>,
    pub render_timeout: Duration,
    /// Rules for `<img src>` loading (`ASSET_*`).
    pub assets: Arc<AssetPolicy>,
    /// Prometheus handle when `METRICS_ENABLED`; `/metrics` exists only then.
    pub metrics: Option<PrometheusHandle>,
    /// Bearer token required by `/metrics` (`METRICS_TOKEN`), if any.
    pub metrics_token: Option<Arc<str>>,
}

impl AppState {
    /// Build from the environment. `metrics` comes from `telemetry::init_metrics_from_env`.
    pub fn new(metrics: Option<PrometheusHandle>) -> Self {
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
        let render_timeout = env::var("RENDER_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(45));
        Self {
            storage,
            storage_bucket: storage_bucket.into(),
            render_permits,
            render_permit_count: cores,
            expected_auth: format!("Bearer {}", api_token).into(),
            renderer: Arc::new(FulgurEngine::default()),
            render_timeout,
            assets: Arc::new(AssetPolicy::from_env()),
            metrics,
            metrics_token: env::var("METRICS_TOKEN")
                .ok()
                .filter(|t| !t.is_empty())
                .map(|t| format!("Bearer {t}").into()),
        }
    }
}
