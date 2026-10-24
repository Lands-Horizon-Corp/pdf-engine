use pdf_engine::{config::AppState, fonts, ipc, pdf, serve};
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
#[cfg(unix)]
fn silence_stdout() {
    use std::os::fd::AsRawFd;
    if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
        // SAFETY: dup2 on valid descriptors; nothing else is using stdout yet.
        unsafe { libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO) };
    }
}

#[cfg(not(unix))]
fn silence_stdout() {}

/// Warnings and errors to stderr; `RUST_LOG` changes the level (default `info`).
fn init_logging() {
    use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
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

/// `pdf-engine ipc`: serve render requests over stdin/stdout (see `src/ipc.rs`). No HTTP
/// server, no port, no token and no storage settings are needed.
fn run_ipc() -> ! {
    init_logging();
    let output = match ipc::take_stdout() {
        Ok(out) => out,
        Err(e) => {
            eprintln!("pdf-engine ipc: cannot take over stdout: {e}");
            std::process::exit(1);
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = runtime.block_on(async {
        let state = AppState::for_ipc();
        if let Err(e) = pdf::warm_up_in_memory(&state).await {
            tracing::error!("Warmup failed: {e}");
            return 1;
        }
        fonts::watch_from_env(state.renderer.clone());
        tracing::info!("pdf-engine v{} ready (ipc)", env!("CARGO_PKG_VERSION"));
        match ipc::run(state, tokio::io::stdin(), output).await {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!("ipc stream ended with an error: {e}");
                2
            }
        }
    });
    std::process::exit(code);
}

/// `pdf-engine` (default): the HTTP API.
fn run_http() {
    init_logging();
    silence_stdout();
    actix_web::rt::System::new().block_on(async {
        let state = AppState::from_env();

        if let Err(e) = pdf::warm_up(&state).await {
            tracing::error!("Warmup failed: {}", e);
            std::process::exit(1);
        }
        fonts::watch_from_env(state.renderer.clone());

        let addr: SocketAddr = format!("0.0.0.0:{}", port())
            .parse()
            .expect("Invalid address");
        tracing::info!(
            "pdf-engine v{} listening on http://{}",
            env!("CARGO_PKG_VERSION"),
            addr
        );

        let listener = std::net::TcpListener::bind(addr).unwrap();
        serve(listener, state).unwrap().await.unwrap();
    });
}

fn main() {
    dotenvy::dotenv().ok();
    match std::env::args().nth(1).as_deref() {
        Some("healthcheck") => healthcheck(),
        Some("ipc") => run_ipc(),
        Some("serve") | None => run_http(),
        Some(other) => {
            eprintln!("unknown command `{other}`; use `ipc`, `serve` (default) or `healthcheck`");
            std::process::exit(64);
        }
    }
}
