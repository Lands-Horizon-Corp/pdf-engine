//! Logs, traces and metrics. Everything beyond stderr logs is opt-in through the environment:
//!
//! - `OTEL_EXPORTER_OTLP_ENDPOINT`: export traces over OTLP/HTTP (e.g. `http://collector:4318`).
//!   `OTEL_SERVICE_NAME` defaults to `pdf-engine`.
//! - `METRICS_ENABLED=true`: serve Prometheus metrics at `GET /metrics`, protected by
//!   `METRICS_TOKEN` when set.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use std::env;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Keep alive for the life of the process; flushes pending spans on drop.
pub struct Telemetry {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        if let Some(p) = self.tracer_provider.take() {
            let _ = p.shutdown();
        }
    }
}

/// Install logging (stderr, `RUST_LOG`, default `info`) and, if configured, OTLP tracing.
pub fn init_tracing() -> Telemetry {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    let tracer_provider = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .and_then(|_| match build_tracer_provider() {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("OpenTelemetry disabled: {e}");
                None
            }
        });
    let otel = tracer_provider
        .as_ref()
        .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer("pdf-engine")));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
        .with(otel)
        .init();
    if tracer_provider.is_some() {
        tracing::info!("OpenTelemetry tracing enabled");
    }
    Telemetry { tracer_provider }
}

fn build_tracer_provider() -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    // Endpoint, headers and timeout come from the standard OTEL_EXPORTER_OTLP_* variables.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()?;
    let service = env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "pdf-engine".into());
    Ok(SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(Resource::builder().with_service_name(service).build())
        .build())
}

/// Prometheus recorder when `METRICS_ENABLED` is truthy. Summaries report p50/p90/p99.
pub fn init_metrics_from_env() -> Option<PrometheusHandle> {
    let enabled =
        env::var("METRICS_ENABLED").is_ok_and(|v| matches!(v.trim(), "1" | "true" | "yes"));
    enabled.then(|| install_metrics_recorder().expect("failed to install metrics recorder"))
}

/// Install the global Prometheus recorder (once per process).
pub fn install_metrics_recorder() -> Result<PrometheusHandle, String> {
    PrometheusBuilder::new()
        .set_quantiles(&[0.5, 0.9, 0.99])
        .map_err(|e| e.to_string())?
        .install_recorder()
        .map_err(|e| e.to_string())
}
