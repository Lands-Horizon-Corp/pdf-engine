//! Poison pills: hostile or pathological inputs. A single bad request must never crash the
//! process, hang a worker forever, leak a render permit, or escape the sandbox.
//!
//! Note: release builds use `panic = 'abort'`, so any handler panic there kills the whole
//! server. A panic here shows up as a transport error on the client.

mod common;

use common::*;
use reqwest::multipart;
use std::time::{Duration, Instant};

#[tokio::test]
async fn crlf_in_filename_does_not_crash_the_server() {
    let app = TestApp::builder().spawn().await;

    for name in [
        "evil\r\nSet-Cookie: pwned=1.pdf",
        "nul\0byte.pdf",
        "bell\x07.pdf",
    ] {
        let resp = app
            .client
            .post(app.url("/api/to-bytes"))
            .bearer_auth(TOKEN)
            .multipart(form("<p>x</p>").text("filename", name.to_string()))
            .send()
            .await;
        let resp = resp.unwrap_or_else(|e| {
            panic!("server dropped the connection for filename {name:?} (handler panic): {e}")
        });
        assert!(
            resp.status() == 200 || resp.status() == 400,
            "{name:?} -> {}",
            resp.status()
        );
        assert!(
            resp.headers().get("set-cookie").is_none(),
            "header injection"
        );
    }
    assert_healthy(&app).await;
}

#[tokio::test]
async fn quotes_in_filename_cannot_break_out_of_content_disposition() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form("<p>x</p>").text("filename", r#"a"; filename*=UTF-8''evil.exe; x=".pdf"#))
        .await;

    assert_eq!(resp.status(), 200);
    let cd = resp.headers()["content-disposition"].to_str().unwrap();
    assert_eq!(cd.matches('"').count(), 2, "{cd}");
}

#[tokio::test]
async fn unicode_filename_is_accepted() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form("<p>x</p>").text("filename", "报告-é.pdf"))
        .await;

    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn template_recursion_bomb_is_400() {
    let app = TestApp::builder().spawn().await;

    let resp = app
        .to_bytes(form("{% macro f() %}{{ f() }}{% endmacro %}{{ f() }}"))
        .await;

    assert_eq!(resp.status(), 400);
    assert_eq!(app.renders(), 0);
}

#[tokio::test]
async fn template_runtime_errors_are_400() {
    let app = TestApp::builder().spawn().await;

    for t in [
        "{{ 1 / 0 }}",
        "{{ range(1000000) }}",
        "{{ [1,2][\"x\"] + {} }}",
    ] {
        let resp = app.to_bytes(form(t)).await;
        assert_eq!(resp.status(), 400, "{t}");
    }
    assert_eq!(app.renders(), 0);
}

/// A template that would spin for hours must be cut off by the render timeout.
///
/// Known gap: the timeout only abandons the `spawn_blocking` render; that thread keeps burning
/// a CPU until it finishes. Enabling minijinja's `fuel` feature would stop it for real.
#[tokio::test]
async fn cpu_bomb_template_times_out() {
    let app = TestApp::builder()
        .timeout(Duration::from_secs(2))
        .spawn()
        .await;

    let start = Instant::now();
    let resp = app
        .to_bytes(form(
            "{% for a in range(100000) %}{% for b in range(100000) %}{% endfor %}{% endfor %}",
        ))
        .await;

    assert_eq!(resp.status(), 408);
    assert!(start.elapsed() < Duration::from_secs(6));
    assert_eq!(app.renders(), 0);
    assert_healthy(&app).await;
}

#[tokio::test]
async fn oversized_body_is_rejected_without_5xx() {
    let app = TestApp::builder().spawn().await;

    let huge = "x".repeat(26 * 1024 * 1024);
    let resp = app
        .client
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .multipart(form(&huge))
        .send()
        .await;

    // The server may reject and close before the client finishes uploading.
    if let Ok(resp) = resp {
        assert!(resp.status().is_client_error(), "got {}", resp.status());
    }
    assert_eq!(app.renders(), 0);
    assert_healthy(&app).await;
}

#[tokio::test]
async fn deeply_nested_json_data_does_not_crash() {
    let app = TestApp::builder().spawn().await;

    let depth = 100_000;
    let data = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    let resp = app
        .to_bytes(form("<p>{{ x }}</p>").text("data", data))
        .await;

    assert!(!resp.status().is_server_error(), "got {}", resp.status());
    assert_healthy(&app).await;
}

#[tokio::test]
async fn invalid_utf8_template_does_not_crash() {
    let app = TestApp::builder().spawn().await;

    let part = multipart::Part::bytes(vec![0x3C, 0x70, 0x3E, 0xFF, 0xFE, 0xC3, 0x28, 0x80]);
    let resp = app
        .to_bytes(multipart::Form::new().part("template", part))
        .await;

    assert!(!resp.status().is_server_error(), "got {}", resp.status());
    assert_healthy(&app).await;
}

#[tokio::test]
async fn unicode_password_does_not_5xx() {
    let app = TestApp::builder().spawn().await;

    for pw in ["密码🔑", &"p".repeat(10_000)] {
        let resp = app
            .to_bytes(form("<p>x</p>").text("password", pw.to_string()))
            .await;
        assert!(
            !resp.status().is_server_error(),
            "password {:.20?} -> {}",
            pw,
            resp.status()
        );
    }
}

#[tokio::test]
async fn hostile_page_args_are_rejected_cleanly() {
    let app = TestApp::builder().spawn().await;

    for w in [
        "1in; } html { display:none } @page { size: 1in",
        "8.5in#frag",
        "100%25",
        "9999999in",
    ] {
        let resp = app.to_bytes(form("<p>x</p>").text("width", w)).await;
        assert_eq!(resp.status(), 400, "width {w:?}");
    }
    assert_healthy(&app).await;
}

#[tokio::test]
async fn no_outbound_network_requests() {
    let app = TestApp::builder().spawn().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let a = listener.local_addr().unwrap();

    let t = format!(
        r#"<html><head><link rel="stylesheet" href="http://{a}/s.css"><style>@import url("http://{a}/i.css"); body {{ background: url("http://{a}/bg.png") }}</style></head>
        <body><img src="http://{a}/x.png"><iframe src="http://{a}/f"></iframe><object data="http://{a}/o"></object></body></html>"#
    );
    let resp = app.to_bytes(form(&t)).await;

    assert_eq!(resp.status(), 200);
    assert!(
        listener.accept().is_err(),
        "the renderer made an outbound connection (SSRF)"
    );
}

#[tokio::test]
async fn local_files_cannot_be_embedded() {
    let app = TestApp::builder().spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let png = write_file(dir.path(), "canary.png", TINY_PNG);

    let t = format!(
        r#"<img src="file://{p}"><img src="{p}"><div style="background:url('file://{p}')">x</div>"#,
        p = png.display()
    );
    let body = app.to_bytes(form(&t)).await.bytes().await.unwrap();

    assert!(
        !has_image(&load_pdf(&body)),
        "a local file was read into the PDF"
    );
}

#[tokio::test]
async fn body_tag_edge_cases_render() {
    let app = TestApp::builder().spawn().await;

    for t in [
        "<body",
        "<p><body</p>",
        "<BODY><p>x</p></BODY>",
        "<p>&lt;body&gt;</p>",
        "<bodyguard>x</bodyguard>",
    ] {
        let resp = app.to_bytes(form(t)).await;
        assert_eq!(resp.status(), 200, "{t:?}");
        assert!(is_pdf(&resp.bytes().await.unwrap()));
    }
}

#[tokio::test]
async fn nul_and_control_bytes_in_template() {
    let app = TestApp::builder().spawn().await;

    let resp = app.to_bytes(form("<p>a\0b\x01c\x1bd</p>")).await;

    assert!(!resp.status().is_server_error(), "got {}", resp.status());
    assert_healthy(&app).await;
}
