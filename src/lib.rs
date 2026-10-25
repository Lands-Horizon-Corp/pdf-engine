use actix_cors::Cors;
use actix_web::{App, HttpServer, http::header, middleware::Compress, web};

pub mod assets;
pub mod auth;
pub mod config;
pub mod error;
pub mod fonts;
pub mod fulgur_engine;
pub mod handlers;
pub mod image_util;
pub mod ipc;
pub mod models;
pub mod pdf;
pub mod pdf_encrypt;
pub mod storage;
pub mod template;

use config::AppState;

/// Bind `listener` and serve the HTTP API until the returned server is stopped or dropped.
///
/// Responses are compressed (gzip / brotli / zstd) only when the client sends
/// `Accept-Encoding`.
pub fn serve(
    listener: std::net::TcpListener,
    state: AppState,
) -> std::io::Result<actix_web::dev::Server> {
    let cors_origins = state.cors_origins.clone();
    let state = web::Data::new(state);
    Ok(HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(routes)
            .wrap(Compress::default())
            // Outermost, so a browser's preflight (OPTIONS, no token) is answered before auth.
            .wrap(cors(&cors_origins))
    })
    .listen(listener)?
    .run())
}

/// CORS for browsers calling the API from another origin (see `CORS_ALLOWED_ORIGINS`).
/// With no origins configured nothing is added to responses.
fn cors(origins: &[String]) -> Cors {
    let mut cors = Cors::default()
        .allowed_methods(["GET", "POST", "OPTIONS"])
        .allowed_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
        .expose_headers([header::CONTENT_DISPOSITION])
        .max_age(3600);
    for origin in origins {
        cors = if origin == "*" {
            cors.allow_any_origin()
        } else {
            cors.allowed_origin(origin)
        };
    }
    cors
}

fn routes(cfg: &mut web::ServiceConfig) {
    // `resource` (not `route`) so a wrong method on a known path answers 405, not 404.
    cfg.service(web::resource("/health").route(web::get().to(handlers::handle_health)))
        .service(
            web::scope("/api")
                .wrap(actix_web::middleware::from_fn(auth::auth_middleware))
                .service(web::resource("/to-s3").route(web::post().to(handlers::handle_to_s3)))
                .service(
                    web::resource("/to-bytes").route(web::post().to(handlers::handle_to_bytes)),
                )
                .service(
                    web::resource("/fonts/reload")
                        .route(web::post().to(handlers::handle_fonts_reload)),
                ),
        );
}
