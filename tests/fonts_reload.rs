//! Fonts added at runtime become available after `POST /api/fonts/reload` or, with the
//! watcher, automatically. Runs against a private fontconfig setup (one test, its own process).

mod common;

use common::*;
use std::{path::Path, time::Duration};

const NOTO: &str = "/usr/share/fonts/truetype/noto";

async fn fonts_used(app: &TestApp, family: &str) -> Vec<String> {
    let html = format!("<p style=\"font-family: '{family}'\">Hello</p>");
    let resp = app.to_bytes(form(&html)).await;
    assert_eq!(resp.status(), 200);
    embedded_fonts(&resp.bytes().await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn fonts_added_at_runtime_are_picked_up() {
    let sources = [
        "NotoSans-Regular.ttf",
        "NotoSerif-Regular.ttf",
        "NotoSansMono-Regular.ttf",
    ];
    if sources.iter().any(|f| !Path::new(NOTO).join(f).exists()) {
        eprintln!("SKIPPED: Noto fonts not installed in {NOTO}");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let fonts = dir.path().join("fonts");
    std::fs::create_dir(&fonts).unwrap();
    std::fs::copy(Path::new(NOTO).join(sources[0]), fonts.join(sources[0])).unwrap();
    let conf = write_file(
        dir.path(),
        "fonts.conf",
        format!(
            r#"<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig><dir>{}</dir><include ignore_missing="yes">/etc/fonts/conf.d</include></fontconfig>"#,
            fonts.display()
        )
        .as_bytes(),
    );
    // SAFETY: set before this process renders anything; this binary has a single test.
    unsafe { std::env::set_var("FONTCONFIG_FILE", &conf) };

    let app = TestApp::builder().spawn().await;
    assert!(
        !fonts_used(&app, "Noto Serif")
            .await
            .iter()
            .any(|f| f.starts_with("NotoSerif"))
    );

    // 1. Explicit reload endpoint.
    std::fs::copy(Path::new(NOTO).join(sources[1]), fonts.join(sources[1])).unwrap();
    assert!(
        !fonts_used(&app, "Noto Serif")
            .await
            .iter()
            .any(|f| f.starts_with("NotoSerif")),
        "fonts must not change before a reload"
    );
    let resp = app
        .client
        .post(app.url("/api/fonts/reload"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let used = fonts_used(&app, "Noto Serif").await;
    assert!(used.iter().any(|f| f.starts_with("NotoSerif")), "{used:?}");

    // 2. File watcher.
    pdf_engine::fonts::watch(
        fonts.clone(),
        app.state.renderer.clone(),
        Duration::from_millis(300),
    )
    .unwrap();
    std::fs::copy(Path::new(NOTO).join(sources[2]), fonts.join(sources[2])).unwrap();
    let mut picked_up = false;
    for _ in 0..40 {
        if fonts_used(&app, "Noto Sans Mono")
            .await
            .iter()
            .any(|f| f.starts_with("NotoSansMono"))
        {
            picked_up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(picked_up, "watcher did not reload fonts");
}
