//! Shared harness for the hermetic test suites.
//!
//! Every suite boots the real `pdf_engine::app` router on an ephemeral port, wired to:
//! - a **fake S3** (an in-process HTTP server that records PUTs and can be told to misbehave), and
//! - a **spy** around the renderer that records every render call and tracks concurrency. The
//!   renderer is the real Fulgur engine unless a test swaps in a [`FakeRenderer`].
//!
//! The server runs on its own runtime in a detached thread, so a panicking handler or a
//! runaway `spawn_blocking` task can't take the test's runtime down with it.

#![allow(dead_code)]

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
};
use lopdf::{Document, LoadOptions, Object, Stream, dictionary};
use opendal::{Operator, services::S3};
use pdf_engine::{
    assets::AssetPolicy,
    config::AppState,
    fulgur_engine::{FulgurEngine, InProcessRenderer, PageMm, RenderJob},
};
use reqwest::{Client, multipart};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub const TOKEN: &str = "test-token";
pub const BUCKET: &str = "test-bucket";

/// Poll `cond` until it's true or `within` elapses.
pub async fn eventually(within: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if cond() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A valid one-page US Letter PDF with a trailer `/ID` (encryption needs one).
pub fn fixture_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
    let media: Vec<Object> = [0, 0, 612, 792].iter().map(|&v: &i64| v.into()).collect();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content, "MediaBox" => media,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let id = Object::string_literal(b"0123456789abcdef".to_vec());
    doc.trailer.set("ID", vec![id.clone(), id]);
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

// ---------------------------------------------------------------------------------------------
// Fake S3
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum S3Mode {
    Ok,
    /// Reply to every request with this status (and an S3-style XML error body).
    Status(u16),
    /// Never reply.
    Hang,
    /// Fail every `n`th PUT with 503 (1-based), succeed otherwise.
    FailEvery(usize),
}

#[derive(Clone, Debug)]
pub struct PutRecord {
    pub path: String,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub body: Bytes,
}

#[derive(Clone)]
pub struct FakeS3 {
    pub addr: SocketAddr,
    pub mode: Arc<Mutex<S3Mode>>,
    pub puts: Arc<Mutex<Vec<PutRecord>>>,
    pub requests: Arc<AtomicUsize>,
}

impl FakeS3 {
    pub fn start(mode: S3Mode) -> Self {
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std_listener.set_nonblocking(true).unwrap();
        let addr = std_listener.local_addr().unwrap();
        let s3 = Self {
            addr,
            mode: Arc::new(Mutex::new(mode)),
            puts: Default::default(),
            requests: Default::default(),
        };
        let router = Router::new()
            .fallback(fake_s3_handler)
            .with_state(s3.clone());
        spawn_detached(async move {
            let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
            axum::serve(listener, router).await.unwrap();
        });
        s3
    }

    pub fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn set_mode(&self, mode: S3Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    pub fn puts(&self) -> Vec<PutRecord> {
        self.puts.lock().unwrap().clone()
    }

    pub fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

async fn fake_s3_handler(
    State(s3): State<FakeS3>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let n = s3.requests.fetch_add(1, Ordering::SeqCst) + 1;
    let mode = s3.mode.lock().unwrap().clone();
    let fail = |code: u16| {
        let xml = format!(
            "<?xml version=\"1.0\"?><Error><Code>Injected{code}</Code><Message>chaos</Message></Error>"
        );
        (StatusCode::from_u16(code).unwrap(), xml).into_response()
    };
    match mode {
        S3Mode::Hang => {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            unreachable!()
        }
        S3Mode::Status(code) => return fail(code),
        S3Mode::FailEvery(k) if n % k == 0 => return fail(503),
        _ => {}
    }
    if method == Method::PUT {
        let h = |k: &str| {
            headers
                .get(k)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
        };
        s3.puts.lock().unwrap().push(PutRecord {
            path: uri.path().to_string(),
            content_type: h("content-type"),
            content_disposition: h("content-disposition"),
            body,
        });
    }
    (StatusCode::OK, [("ETag", "\"fake-etag\"")], "").into_response()
}

/// A TCP endpoint that accepts connections and immediately slams them shut.
pub fn start_connection_dropper() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            drop(stream);
        }
    });
    addr
}

/// An address nothing is listening on.
pub fn closed_port() -> SocketAddr {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap()
}

fn s3_operator(endpoint: &str) -> Operator {
    let builder = S3::default()
        .endpoint(endpoint)
        .bucket(BUCKET)
        .region("us-east-1")
        .access_key_id("test-access")
        .secret_access_key("test-secret")
        .disable_config_load()
        .disable_ec2_metadata();
    Operator::new(builder).unwrap()
}

// ---------------------------------------------------------------------------------------------
// The app under test
// ---------------------------------------------------------------------------------------------

/// Run a future on its own runtime in a detached thread that is never joined.
fn spawn_detached<F: Future<Output = ()> + Send + 'static>(fut: F) {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap()
            .block_on(fut)
    });
}

pub struct TestApp {
    pub base: String,
    pub state: AppState,
    pub s3: FakeS3,
    pub client: Client,
    pub permits: usize,
    pub spy: Arc<Spy>,
}

pub struct TestAppBuilder {
    renderer: Option<Arc<dyn InProcessRenderer>>,
    assets: AssetPolicy,
    permits: usize,
    timeout: Duration,
    s3_mode: S3Mode,
    s3_endpoint: Option<String>,
}

impl TestApp {
    /// App with the real Fulgur renderer; swap it with `.renderer(...)`.
    pub fn builder() -> TestAppBuilder {
        TestAppBuilder {
            renderer: None,
            assets: AssetPolicy::default(),
            permits: 4,
            timeout: Duration::from_secs(20),
            s3_mode: S3Mode::Ok,
            s3_endpoint: None,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub async fn to_bytes(&self, form: multipart::Form) -> reqwest::Response {
        self.client
            .post(self.url("/api/to-bytes"))
            .bearer_auth(TOKEN)
            .multipart(form)
            .send()
            .await
            .expect("request to /api/to-bytes failed at the transport level")
    }

    pub async fn to_s3(&self, form: multipart::Form) -> reqwest::Response {
        self.client
            .post(self.url("/api/to-s3"))
            .bearer_auth(TOKEN)
            .multipart(form)
            .send()
            .await
            .expect("request to /api/to-s3 failed at the transport level")
    }

    pub fn available_permits(&self) -> usize {
        self.state.render_permits.available_permits()
    }

    /// How many times the renderer was invoked.
    pub fn renders(&self) -> usize {
        self.spy.calls.lock().unwrap().len()
    }

    /// Every render call so far.
    pub fn calls(&self) -> Vec<Call> {
        self.spy.calls.lock().unwrap().clone()
    }

    /// Every render permit eventually comes back (nothing leaked / dangling).
    pub async fn assert_permits_restored(&self) {
        let ok = eventually(Duration::from_secs(5), || {
            self.available_permits() == self.permits
        })
        .await;
        assert!(
            ok,
            "semaphore permits leaked: {} of {} available",
            self.available_permits(),
            self.permits
        );
    }
}

impl TestAppBuilder {
    pub fn permits(mut self, n: usize) -> Self {
        self.permits = n;
        self
    }

    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn s3_mode(mut self, m: S3Mode) -> Self {
        self.s3_mode = m;
        self
    }

    pub fn assets(mut self, policy: AssetPolicy) -> Self {
        self.assets = policy;
        self
    }

    /// Replace the renderer (default: the real Fulgur engine). It is still wrapped in a spy.
    pub fn renderer(mut self, r: Arc<dyn InProcessRenderer>) -> Self {
        self.renderer = Some(r);
        self
    }

    /// Point storage somewhere other than the fake S3 (e.g. a closed port).
    pub fn s3_endpoint(mut self, e: String) -> Self {
        self.s3_endpoint = Some(e);
        self
    }

    pub async fn spawn(self) -> TestApp {
        let s3 = FakeS3::start(self.s3_mode);
        let endpoint = self.s3_endpoint.unwrap_or_else(|| s3.endpoint());
        let spy = Arc::new(Spy::new(self.renderer.unwrap_or_else(real_fulgur)));
        let state = AppState {
            storage: Some(s3_operator(&endpoint)),
            storage_bucket: BUCKET.into(),
            render_permits: Arc::new(Semaphore::new(self.permits)),
            render_permit_count: self.permits,
            expected_auth: format!("Bearer {TOKEN}").into(),
            renderer: spy.clone(),
            render_timeout: self.timeout,
            assets: Arc::new(self.assets),
        };

        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std_listener.set_nonblocking(true).unwrap();
        let addr = std_listener.local_addr().unwrap();
        let server_state = state.clone();
        std::thread::spawn(move || {
            actix_web::rt::System::new()
                .block_on(async move {
                    std_listener.set_nonblocking(false).unwrap();
                    pdf_engine::serve(std_listener, server_state).unwrap().await
                })
                .unwrap();
        });

        TestApp {
            base: format!("http://{addr}"),
            state,
            s3,
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap(),
            permits: self.permits,
            spy,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// In-process renderers
// ---------------------------------------------------------------------------------------------

/// The production Fulgur engine.
pub fn real_fulgur() -> Arc<dyn InProcessRenderer> {
    static ENGINE: OnceLock<Arc<FulgurEngine>> = OnceLock::new();
    ENGINE
        .get_or_init(|| Arc::new(FulgurEngine::default()))
        .clone()
}

#[derive(Clone, Debug)]
pub struct Call {
    pub html: String,
    pub page: PageMm,
    pub title: Option<String>,
    /// `src` keys of the images handed to the renderer.
    pub images: Vec<String>,
}

/// Wraps a renderer, recording calls and the peak number of concurrent renders.
pub struct Spy {
    inner: Arc<dyn InProcessRenderer>,
    pub calls: Mutex<Vec<Call>>,
    active: AtomicUsize,
    pub max_active: AtomicUsize,
}

impl Spy {
    fn new(inner: Arc<dyn InProcessRenderer>) -> Self {
        Self {
            inner,
            calls: Mutex::default(),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
        }
    }
}

impl InProcessRenderer for Spy {
    fn render(&self, job: &RenderJob) -> Result<Vec<u8>, String> {
        self.calls.lock().unwrap().push(Call {
            html: job.html.clone(),
            page: job.page,
            title: job.title.clone(),
            images: job.images.iter().map(|(k, _)| k.clone()).collect(),
        });
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now, Ordering::SeqCst);
        struct Done<'a>(&'a AtomicUsize);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _done = Done(&self.active);
        self.inner.render(job)
    }

    fn reload_fonts(&self) {
        self.inner.reload_fonts()
    }
}

/// A renderer that misbehaves on demand.
pub enum FakeRenderer {
    /// Returns `fixture_pdf()` after an optional delay.
    Ok(Duration),
    Fails,
    Panics,
    /// Fails when the HTML contains this marker, otherwise renders for real.
    FailsOn(&'static str),
    /// Returns bytes that aren't a PDF.
    Garbage,
    /// Sleeps, then reports when it actually finished.
    Slow(Duration, Arc<AtomicUsize>),
}

impl InProcessRenderer for FakeRenderer {
    fn render(&self, job: &RenderJob) -> Result<Vec<u8>, String> {
        match self {
            FakeRenderer::Ok(d) => {
                std::thread::sleep(*d);
                Ok(fixture_pdf())
            }
            FakeRenderer::Fails => Err("injected failure".into()),
            FakeRenderer::Panics => panic!("injected panic"),
            FakeRenderer::FailsOn(m) if job.html.contains(m) => Err("injected failure".into()),
            FakeRenderer::FailsOn(_) => real_fulgur().render(job),
            FakeRenderer::Garbage => Ok(b"definitely not a pdf".to_vec()),
            FakeRenderer::Slow(d, finished) => {
                std::thread::sleep(*d);
                finished.fetch_add(1, Ordering::SeqCst);
                Ok(fixture_pdf())
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Request / PDF helpers
// ---------------------------------------------------------------------------------------------

pub fn form(template: &str) -> multipart::Form {
    multipart::Form::new().text("template", template.to_string())
}

pub fn is_pdf(b: &[u8]) -> bool {
    b.starts_with(b"%PDF")
}

pub fn load_pdf(b: &[u8]) -> Document {
    Document::load_mem(b).expect("response is not a loadable PDF")
}

pub fn load_pdf_with_password(b: &[u8], pw: &str) -> lopdf::Result<Document> {
    Document::load_mem_with_options(b, LoadOptions::with_password(pw))
}

pub fn page_count(b: &[u8]) -> usize {
    load_pdf(b).get_pages().len()
}

/// MediaBox of every page (walking up to /Pages when inherited), as rounded points.
pub fn media_boxes(doc: &Document) -> Vec<[i64; 4]> {
    doc.get_pages()
        .values()
        .map(|&id| {
            let mut node = doc.get_dictionary(id).unwrap();
            loop {
                if let Ok(arr) = node.get(b"MediaBox").and_then(Object::as_array) {
                    let v: Vec<i64> = arr
                        .iter()
                        .map(|o| o.as_float().map(|f| f.round() as i64).unwrap())
                        .collect();
                    return [v[0], v[1], v[2], v[3]];
                }
                let parent = node.get(b"Parent").and_then(Object::as_reference).unwrap();
                node = doc.get_dictionary(parent).unwrap();
            }
        })
        .collect()
}

/// `/Title` from the document info dictionary, if any.
pub fn info_title(doc: &Document) -> Option<String> {
    let info = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    let raw = doc
        .get_dictionary(info)
        .ok()?
        .get(b"Title")
        .ok()?
        .as_str()
        .ok()?;
    if raw.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).ok()
    } else {
        Some(String::from_utf8_lossy(raw).into_owned())
    }
}

/// Does any page reference an image XObject?
pub fn has_image(doc: &Document) -> bool {
    doc.objects.values().any(|o| match o {
        Object::Stream(s) => s
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n == b"Image")
            .unwrap_or(false),
        _ => false,
    })
}

/// Fill/stroke color operators (`r g b rg` / `r g b RG`) from every page's content stream.
pub fn color_ops(pdf: &[u8]) -> Vec<String> {
    let doc = load_pdf(pdf);
    doc.get_pages()
        .values()
        .flat_map(|&id| {
            String::from_utf8_lossy(&doc.get_page_content(id))
                .lines()
                .filter(|l| l.ends_with(" rg") || l.ends_with(" RG"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Font names embedded in the PDF (subset prefixes stripped).
pub fn embedded_fonts(pdf: &[u8]) -> Vec<String> {
    let doc = load_pdf(pdf);
    let mut names: Vec<String> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Type").and_then(|t| t.as_name()).ok() == Some(b"Font".as_slice()))
        .filter_map(|d| d.get(b"BaseFont").and_then(|b| b.as_name()).ok())
        .map(|n| {
            let n = String::from_utf8_lossy(n).into_owned();
            n.split_once('+')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(n)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The server still serves a normal request correctly. Uses a fresh connection so a socket the
/// server is closing (e.g. after rejecting an oversized upload) can't be reused.
pub async fn assert_healthy(app: &TestApp) {
    let resp = Client::new()
        .post(app.url("/api/to-bytes"))
        .bearer_auth(TOKEN)
        .multipart(form("<p>health</p>"))
        .send()
        .await
        .expect("server unreachable");
    assert_eq!(resp.status(), 200, "server unhealthy");
    assert!(is_pdf(&resp.bytes().await.unwrap()));
}

/// A valid 1x1 red PNG.
pub const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

pub fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}
