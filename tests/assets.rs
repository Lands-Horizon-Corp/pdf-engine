//! `<img src>` loading: data URIs, allowlisted remote images, and every way a fetch is refused.

mod common;

use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::get,
};
use base64::Engine as _;
use common::*;
use pdf_engine::assets::AssetPolicy;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Image server on 127.0.0.1 that counts hits per path.
#[derive(Clone, Default)]
struct ImageServer {
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl ImageServer {
    async fn start() -> (Self, SocketAddr) {
        let server = Self::default();
        let app = Router::new()
            .route("/{name}", get(serve))
            .with_state(server.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (server, addr)
    }

    fn hits(&self, path: &str) -> usize {
        *self.hits.lock().unwrap().get(path).unwrap_or(&0)
    }

    fn total_hits(&self) -> usize {
        self.hits.lock().unwrap().values().sum()
    }
}

async fn serve(State(s): State<ImageServer>, Path(name): Path<String>) -> axum::response::Response {
    *s.hits.lock().unwrap().entry(name.clone()).or_default() += 1;
    let png = [(header::CONTENT_TYPE, "image/png")];
    match name.as_str() {
        "logo.png" | "a.png" | "b.png" | "c.png" => (png, TINY_PNG.to_vec()).into_response(),
        "big.png" => (png, vec![0u8; 4096]).into_response(),
        // Valid PNG signature, corrupt body: the renderer can't decode it.
        "corrupt.png" => (png, [&TINY_PNG[..20], &[0u8; 40][..]].concat()).into_response(),
        "page.png" => ([(header::CONTENT_TYPE, "text/html")], "<html></html>").into_response(),
        "slow.png" => {
            tokio::time::sleep(Duration::from_secs(5)).await;
            (png, TINY_PNG.to_vec()).into_response()
        }
        "redirect.png" => Redirect::temporary("/logo.png").into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Policy that lets the tests reach the local server.
fn local_policy() -> AssetPolicy {
    AssetPolicy {
        allowed_hosts: vec!["127.0.0.1".into()],
        allow_http: true,
        allow_private_ips: true,
        max_bytes: 1024,
        timeout: Duration::from_millis(800),
        ..Default::default()
    }
}

fn img(addr: SocketAddr, path: &str) -> String {
    format!(r#"<img style="width:40px;height:40px" src="http://{addr}/{path}">"#)
}

async fn render(app: &TestApp, html: &str) -> Vec<u8> {
    let resp = app.to_bytes(form(html)).await;
    assert_eq!(
        resp.status(),
        200,
        "rendering must succeed even when images fail"
    );
    resp.bytes().await.unwrap().to_vec()
}

#[tokio::test]
async fn allowlisted_remote_image_is_embedded() {
    let (server, addr) = ImageServer::start().await;
    let app = TestApp::builder().assets(local_policy()).spawn().await;

    let pdf = render(&app, &img(addr, "logo.png")).await;

    assert!(has_image(&load_pdf(&pdf)));
    assert_eq!(server.hits("logo.png"), 1);
    assert_eq!(
        app.calls()[0].images,
        vec![format!("http://{addr}/logo.png")]
    );
}

#[tokio::test]
async fn template_data_urls_survive_autoescaping() {
    let (server, addr) = ImageServer::start().await;
    let app = TestApp::builder().assets(local_policy()).spawn().await;
    let url = format!("http://{addr}/logo.png?size=40&v=2");

    let resp = app
        .to_bytes(
            form(r#"<img style="width:40px;height:40px" src="{{ logo }}">"#)
                .text("data", serde_json::json!({ "logo": url }).to_string()),
        )
        .await;

    assert!(has_image(&load_pdf(&resp.bytes().await.unwrap())));
    assert_eq!(server.hits("logo.png"), 1);
}

#[tokio::test]
async fn remote_images_are_off_by_default() {
    let (server, addr) = ImageServer::start().await;
    let app = TestApp::builder().spawn().await;

    let pdf = render(&app, &img(addr, "logo.png")).await;

    assert!(!has_image(&load_pdf(&pdf)));
    assert_eq!(server.total_hits(), 0);
}

#[tokio::test]
async fn hosts_outside_the_allowlist_are_not_contacted() {
    let (server, addr) = ImageServer::start().await;
    let policy = AssetPolicy {
        allowed_hosts: vec!["cdn.example.com".into()],
        ..local_policy()
    };
    let app = TestApp::builder().assets(policy).spawn().await;

    render(&app, &img(addr, "logo.png")).await;

    assert_eq!(server.total_hits(), 0);
}

#[tokio::test]
async fn private_addresses_are_blocked_even_when_allowlisted() {
    let (server, addr) = ImageServer::start().await;
    let policy = AssetPolicy {
        allow_private_ips: false,
        ..local_policy()
    };
    let app = TestApp::builder().assets(policy).spawn().await;

    let pdf = render(&app, &img(addr, "logo.png")).await;

    assert!(!has_image(&load_pdf(&pdf)));
    assert_eq!(
        server.total_hits(),
        0,
        "SSRF: a private address was contacted"
    );
}

#[tokio::test]
async fn plain_http_needs_explicit_opt_in() {
    let (server, addr) = ImageServer::start().await;
    let policy = AssetPolicy {
        allow_http: false,
        ..local_policy()
    };
    let app = TestApp::builder().assets(policy).spawn().await;

    render(&app, &img(addr, "logo.png")).await;

    assert_eq!(server.total_hits(), 0);
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let (server, addr) = ImageServer::start().await;
    let app = TestApp::builder().assets(local_policy()).spawn().await;

    let pdf = render(&app, &img(addr, "redirect.png")).await;

    assert!(!has_image(&load_pdf(&pdf)));
    assert_eq!(server.hits("redirect.png"), 1);
    assert_eq!(server.hits("logo.png"), 0);
}

#[tokio::test]
async fn oversized_wrong_type_and_missing_images_are_skipped() {
    let (_server, addr) = ImageServer::start().await;
    let app = TestApp::builder().assets(local_policy()).spawn().await;

    for path in ["big.png", "page.png", "missing.png", "corrupt.png"] {
        let pdf = render(&app, &img(addr, path)).await;
        assert!(!has_image(&load_pdf(&pdf)), "{path} was embedded");
    }
}

#[tokio::test]
async fn slow_images_time_out_without_failing_the_pdf() {
    let (_server, addr) = ImageServer::start().await;
    let app = TestApp::builder().assets(local_policy()).spawn().await;

    let started = Instant::now();
    let pdf = render(&app, &img(addr, "slow.png")).await;

    assert!(
        started.elapsed() < Duration::from_secs(3),
        "took {:?}",
        started.elapsed()
    );
    assert!(!has_image(&load_pdf(&pdf)));
}

#[tokio::test]
async fn image_count_is_capped() {
    let (server, addr) = ImageServer::start().await;
    let policy = AssetPolicy {
        max_images: 2,
        ..local_policy()
    };
    let app = TestApp::builder().assets(policy).spawn().await;

    let html = ["a.png", "b.png", "c.png"].map(|p| img(addr, p)).concat();
    render(&app, &html).await;

    assert_eq!(server.total_hits(), 2);
}

#[tokio::test]
async fn data_uri_images_render_without_any_network() {
    let app = TestApp::builder().spawn().await;
    let b64 = base64::engine::general_purpose::STANDARD.encode(TINY_PNG);

    let pdf = render(
        &app,
        &format!(r#"<img style="width:40px;height:40px" src="data:image/png;base64,{b64}">"#),
    )
    .await;

    assert!(has_image(&load_pdf(&pdf)));
}

#[tokio::test]
async fn images_without_a_height_are_still_drawn() {
    let app = TestApp::builder().spawn().await;
    let b64 = base64::engine::general_purpose::STANDARD.encode(TINY_PNG);
    for attrs in [
        "",
        r#"width="40""#,
        r#"style="width:40px""#,
        r#"style="height:40px""#,
    ] {
        let pdf = render(
            &app,
            &format!(r#"<img {attrs} src="data:image/png;base64,{b64}">"#),
        )
        .await;
        assert!(has_image(&load_pdf(&pdf)), "<img {attrs}> was not drawn");
    }
}

#[tokio::test]
async fn local_files_are_never_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let png = write_file(dir.path(), "canary.png", TINY_PNG);
    let policy = AssetPolicy {
        allowed_hosts: vec!["*".into(), "localhost".into()],
        ..local_policy()
    };
    let app = TestApp::builder().assets(policy).spawn().await;

    let pdf = render(
        &app,
        &format!(
            r#"<img src="file://{p}"><img src="{p}">"#,
            p = png.display()
        ),
    )
    .await;

    assert!(!has_image(&load_pdf(&pdf)));
}
