use pdf_engine::{app, config::AppState, fonts, pdf, telemetry};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

/// Rendering is allocation-heavy (DOM, style, layout); mimalloc is ~10% faster than glibc's.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Blitz (inside Fulgur) `println!`s every HTML parse warning. Logs go to stderr, and stdout
/// is pointed at /dev/null once, at startup, before any rendering happens.
fn silence_stdout() {
    use std::os::fd::AsRawFd;
    if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
        // SAFETY: dup2 on valid descriptors; nothing else is using stdout yet.
        unsafe { libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO) };
    }
}

fn port() -> String {
    std::env::var("API_PORT").unwrap_or_else(|_| "6767".to_string())
}

/// `pdf-engine healthcheck`: exit 0 if `GET /health` on this container answers 200. Used by
/// the Docker `HEALTHCHECK` (the image has no curl).
fn healthcheck() -> ! {
    let ok = (|| -> std::io::Result<bool> {
        let addr: SocketAddr = format!("127.0.0.1:{}", port()).parse().expect("API_PORT");
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response.starts_with("HTTP/1.0 200") || response.starts_with("HTTP/1.1 200"))
    })()
    .unwrap_or(false);
    std::process::exit(if ok { 0 } else { 1 });
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        healthcheck();
    }

    let _telemetry = telemetry::init_tracing();
    silence_stdout();
    let metrics = telemetry::init_metrics_from_env();

    let state = AppState::new(metrics);

    if let Err(e) = pdf::warm_up(&state).await {
        tracing::error!("Warmup failed: {}", e);
        std::process::exit(1);
    }
    fonts::watch_from_env(state.renderer.clone());

    let app = app(state);

    let addr: SocketAddr = format!("0.0.0.0:{}", port())
        .parse()
        .expect("Invalid address");
    tracing::info!(
        "pdf-engine v{} listening on http://{}",
        env!("CARGO_PKG_VERSION"),
        addr
    );

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
