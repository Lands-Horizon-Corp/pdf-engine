use pdf_engine::{app, config::AppState, pdf};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let state = AppState::new();

    if let Err(e) = pdf::warm_up_engine(&state.prince_bin, state.prince_concurrency.clone()).await {
        tracing::error!("Warmup failed: {}", e);
        std::process::exit(1);
    }

    let app = app(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "6767".to_string());
    let addr_str = format!("0.0.0.0:{}", port);
    let addr: SocketAddr = addr_str.parse().expect("Invalid address");
    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
