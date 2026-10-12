use pdf_engine::{app, config::AppState, pdf};

/// Rendering is allocation-heavy (DOM, style, layout); mimalloc is ~10% faster than glibc's.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use std::net::SocketAddr;

/// Blitz (inside Fulgur) `println!`s every HTML parse warning. Logs go to stderr, and stdout
/// is pointed at /dev/null once, at startup, before any rendering happens.
fn silence_stdout() {
    use std::os::fd::AsRawFd;
    if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
        // SAFETY: dup2 on valid descriptors; nothing else is using stdout yet.
        unsafe { libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO) };
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    silence_stdout();
    dotenvy::dotenv().ok();

    let state = AppState::new();

    if let Err(e) = pdf::warm_up(&state).await {
        tracing::error!("Warmup failed: {}", e);
        std::process::exit(1);
    }

    let app = app(state);

    let port = std::env::var("API_PORT").unwrap_or_else(|_| "6767".to_string());
    let addr_str = format!("0.0.0.0:{}", port);
    let addr: SocketAddr = addr_str.parse().expect("Invalid address");
    tracing::info!(
        "pdf-engine v{} listening on http://{}",
        env!("CARGO_PKG_VERSION"),
        addr
    );

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
