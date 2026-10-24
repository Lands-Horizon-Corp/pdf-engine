use actix_web::{
    App, HttpServer,
    body::{BoxBody, MessageBody},
    dev::{Server, ServiceRequest, ServiceResponse},
    middleware::{Compress, Next, from_fn},
    web,
};
use std::time::Instant;

pub mod assets;
pub mod auth;
pub mod config;
pub mod error;
pub mod fonts;
pub mod fulgur_engine;
pub mod handlers;
pub mod models;
pub mod pdf;
pub mod storage;
pub mod telemetry;
pub mod template;

use config::AppState;

/// Bind `listener` and serve the API until the returned server is stopped or dropped.
///
/// Responses are compressed (gzip / brotli / zstd) only when the client sends
/// `Accept-Encoding`.
pub fn serve(listener: std::net::TcpListener, state: AppState) -> std::io::Result<Server> {
    let metrics_enabled = state.metrics.is_some();
    let state = web::Data::new(state);
    Ok(HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(|cfg| routes(cfg, metrics_enabled))
            .wrap(from_fn(track_http))
            .wrap(Compress::default())
    })
    .listen(listener)?
    .run())
}

fn routes(cfg: &mut web::ServiceConfig, metrics_enabled: bool) {
    // `resource` (not `route`) so a wrong method on a known path answers 405, not 404.
    cfg.service(web::resource("/health").route(web::get().to(handlers::handle_health)))
        .service(
            web::scope("/api")
                .wrap(from_fn(auth::auth_middleware))
                .service(web::resource("/to-s3").route(web::post().to(handlers::handle_to_s3)))
                .service(
                    web::resource("/to-bytes").route(web::post().to(handlers::handle_to_bytes)),
                )
                .service(
                    web::resource("/fonts/reload")
                        .route(web::post().to(handlers::handle_fonts_reload)),
                ),
        );
    if metrics_enabled {
        cfg.service(web::resource("/metrics").route(web::get().to(handlers::handle_metrics)));
    }
}

/// Request count and latency per route and status (no-ops unless metrics are enabled).
async fn track_http(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let route = req
        .match_pattern()
        .unwrap_or_else(|| "unmatched".to_string());
    let method = req.method().to_string();
    let started = Instant::now();
    let response = next.call(req).await?;
    let status = response.status().as_u16().to_string();
    let labels = [("route", route), ("method", method), ("status", status)];
    metrics::counter!("http_requests_total", &labels).increment(1);
    metrics::histogram!("http_request_duration_seconds", &labels[..2])
        .record(started.elapsed().as_secs_f64());
    Ok(response.map_into_boxed_body())
}
