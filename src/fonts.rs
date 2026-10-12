//! Reload fonts automatically when the font directory changes (`FONT_WATCH=true`).

use crate::fulgur_engine::InProcessRenderer;
use notify::{
    EventKind, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode, ModifyKind},
};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

/// Default directory the curated font set lives in (see `fonts/install.sh`).
pub const DEFAULT_FONT_DIR: &str = "/usr/share/fonts/pdf-engine";

/// Watch `dir` and call `reload_fonts` after changes settle for `debounce`. Runs on its own
/// thread for the life of the process.
pub fn watch(
    dir: PathBuf,
    renderer: Arc<dyn InProcessRenderer>,
    debounce: Duration,
) -> notify::Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;
    tracing::info!(dir = %dir.display(), "watching fonts for changes");
    std::thread::Builder::new()
        .name("font-watcher".into())
        .spawn(move || {
            let _watcher = watcher; // keep alive
            while let Ok(event) = rx.recv() {
                if !is_change(&event) {
                    continue;
                }
                // Wait until changes stop for `debounce`, then reload once. Reads (renders
                // opening font files) don't count, or a busy server would never settle.
                let mut last_change = Instant::now();
                loop {
                    let remaining = debounce.saturating_sub(last_change.elapsed());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok(ev) if is_change(&ev) => last_change = Instant::now(),
                        Ok(_) => {}
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                let started = Instant::now();
                renderer.reload_fonts();
                metrics::counter!("pdf_font_reloads_total").increment(1);
                tracing::info!(
                    took_ms = started.elapsed().as_millis() as u64,
                    "fonts reloaded"
                );
            }
        })?;
    Ok(())
}

/// Files added, removed, renamed or written; not reads or metadata touches.
fn is_change(event: &notify::Result<notify::Event>) -> bool {
    matches!(
        event.as_ref().map(|e| e.kind),
        Ok(EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_))
            | EventKind::Access(AccessKind::Close(AccessMode::Write)))
    )
}

/// Start the watcher if `FONT_WATCH` is truthy. Directory: `FONT_WATCH_DIR` or the default.
pub fn watch_from_env(renderer: Arc<dyn InProcessRenderer>) {
    let enabled =
        std::env::var("FONT_WATCH").is_ok_and(|v| matches!(v.trim(), "1" | "true" | "yes"));
    if !enabled {
        return;
    }
    let dir = std::env::var("FONT_WATCH_DIR").unwrap_or_else(|_| DEFAULT_FONT_DIR.into());
    if let Err(e) = watch(PathBuf::from(dir), renderer, Duration::from_secs(1)) {
        tracing::error!("font watcher disabled: {e}");
    }
}
