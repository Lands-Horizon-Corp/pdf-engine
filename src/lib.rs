use actix_web::{App, HttpServer, middleware::Compress, web};

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
    let state = web::Data::new(state);
    Ok(HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .configure(routes)
            .wrap(Compress::default())
    })
    .listen(listener)?
    .run())
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
