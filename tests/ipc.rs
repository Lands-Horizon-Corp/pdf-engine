//! IPC mode end to end: spawns the real `pdf-engine ipc` binary and talks to it the way the Node.js
//! client does. (The protocol itself is also unit-tested in src/ipc.rs.)

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

struct Engine {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
}

impl Engine {
    fn start() -> Self {
        Self::start_with(&[])
    }

    fn start_with(env: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pdf-engine"));
        cmd.arg("ipc")
            .env_remove("API_BEARER_TOKEN")
            .env_remove("STORAGE_BUCKET")
            .env_remove("STORAGE_URL")
            .env_remove("STORAGE_ACCESS_KEY")
            .env_remove("STORAGE_SECRET_KEY")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("spawn pdf-engine ipc");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        Engine {
            child,
            stdin,
            stdout,
        }
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    fn send(&mut self, header: Value) {
        let header = header.to_string();
        let mut frame = Vec::new();
        frame.extend_from_slice(&(header.len() as u32).to_be_bytes());
        frame.extend_from_slice(&0u32.to_be_bytes());
        frame.extend_from_slice(header.as_bytes());
        self.send_raw(&frame);
    }

    fn send_with_body(&mut self, header: Value, body: &[u8]) {
        let header = header.to_string();
        let mut frame = Vec::new();
        frame.extend_from_slice(&(header.len() as u32).to_be_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(header.as_bytes());
        frame.extend_from_slice(body);
        self.send_raw(&frame);
    }

    fn recv(&mut self) -> (Value, Vec<u8>) {
        let mut prefix = [0u8; 8];
        self.stdout.read_exact(&mut prefix).expect("a reply frame");
        let header_len = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
        let body_len = u32::from_be_bytes(prefix[4..].try_into().unwrap()) as usize;
        let mut header = vec![0; header_len];
        self.stdout.read_exact(&mut header).unwrap();
        let mut body = vec![0; body_len];
        self.stdout.read_exact(&mut body).unwrap();
        (
            serde_json::from_slice(&header).expect("header is JSON"),
            body,
        )
    }

    /// Close stdin and wait for the process to exit.
    fn finish(mut self) -> std::process::ExitStatus {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "engine did not exit after stdin closed"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn renders_a_pdf_without_any_server_settings() {
    // No API_BEARER_TOKEN and no STORAGE_*: IPC needs neither.
    let mut engine = Engine::start();

    engine.send(json!({ "id": 1, "op": "ping" }));
    let (h, _) = engine.recv();
    assert_eq!((h["id"].clone(), h["ok"].clone()), (json!(1), json!(true)));
    assert_eq!(h["version"], env!("CARGO_PKG_VERSION"));

    engine.send(json!({
        "id": 2, "template": "<h1>Invoice {{ no }}</h1>", "data": { "no": 42 },
        "width": "80mm", "height": "120mm"
    }));
    let (h, pdf) = engine.recv();
    assert_eq!(h["ok"], true, "{h}");
    assert_eq!(h["contentType"], "application/pdf");
    assert_eq!(h["bytes"], pdf.len());
    assert!(pdf.starts_with(b"%PDF") && pdf.len() > 500);

    assert!(
        engine.finish().success(),
        "closing stdin must be a clean exit"
    );
}

#[test]
fn errors_use_the_same_codes_as_http() {
    let mut engine = Engine::start();
    let cases = [
        (json!({ "id": 1 }), "MISSING_FIELD", 400),
        (
            json!({ "id": 2, "template": "<p>\n{% if %}" }),
            "TEMPLATE_ERROR",
            400,
        ),
        (
            json!({ "id": 3, "template": "x", "orientation": "sideways" }),
            "INVALID_FIELD",
            400,
        ),
        (json!({ "id": 4, "op": "nope" }), "INVALID_FIELD", 400),
    ];
    for (req, code, status) in cases {
        engine.send(req);
        let (h, body) = engine.recv();
        assert_eq!(h["ok"], false, "{h}");
        assert_eq!(h["error"]["code"], code, "{h}");
        assert_eq!(h["error"]["status"], status);
        assert!(h["error"]["error"].as_str().unwrap().len() > 10);
        assert!(h["error"]["hint"].is_string());
        assert!(body.is_empty());
    }
    // The template error names the line.
    engine.send(json!({ "id": 5, "template": "<p>\n{% if %}" }));
    assert_eq!(engine.recv().0["error"]["line"], 2);
    engine.finish();
}

#[test]
fn many_requests_in_flight_all_get_their_own_answer() {
    let mut engine = Engine::start();
    for i in 0..60 {
        engine.send(json!({ "id": i, "template": "<p>{{ i }}</p>", "data": { "i": i } }));
    }
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..60 {
        let (h, pdf) = engine.recv();
        assert_eq!(h["ok"], true, "{h}");
        assert!(pdf.starts_with(b"%PDF"));
        ids.insert(h["id"].as_u64().unwrap());
    }
    assert_eq!(ids.len(), 60);
    assert!(engine.finish().success());
}

#[test]
fn nothing_but_frames_reaches_stdout() {
    // The HTML parser prints warnings with `println!`; they must not corrupt the stream.
    let mut engine = Engine::start();
    engine.send(
        json!({ "id": 1, "template": "<p>unclosed <b>tags <div><span>and &bogus; entities" }),
    );
    let (h, pdf) = engine.recv();
    assert_eq!(h["ok"], true, "{h}");
    assert!(pdf.starts_with(b"%PDF"));
    engine.send(json!({ "id": 2, "op": "ping" }));
    assert_eq!(engine.recv().0["id"], 2);
    engine.finish();
}

#[test]
fn ipc_mode_does_not_open_an_http_port() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut engine = Engine::start_with(&[("API_PORT", &port.to_string())]);
    engine.send(json!({ "id": 1, "op": "ping" }));
    engine.recv();

    assert!(
        TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(500)
        )
        .is_err(),
        "ipc mode must not listen on API_PORT"
    );
    engine.finish();
}

#[test]
fn a_broken_stream_is_reported_and_exits_with_an_error() {
    let mut engine = Engine::start();
    // Announces a 3 GiB header.
    engine.send_raw(&[0xC0, 0, 0, 0, 0, 0, 0, 0]);
    let (h, _) = engine.recv();
    assert_eq!(h["error"]["code"], "IPC_PROTOCOL_ERROR");
    let status = engine.finish();
    assert_eq!(status.code(), Some(2));
}

fn png() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC")
        .unwrap()
}

fn has_image(pdf: &[u8]) -> bool {
    lopdf::Document::load_mem(pdf)
        .unwrap()
        .objects
        .values()
        .any(|o| match o {
            lopdf::Object::Stream(s) => s
                .dict
                .get(b"Subtype")
                .and_then(lopdf::Object::as_name)
                .is_ok_and(|n| n == b"Image"),
            _ => false,
        })
}

#[test]
fn supplied_images_travel_in_the_request_body() {
    let mut engine = Engine::start();
    let png = png();
    let template = r#"<p>hi</p><img src="logo"><img src="other">"#;
    engine.send_with_body(
        json!({ "id": 1, "template": template, "images": [{ "src": "logo", "len": png.len() }] }),
        &png,
    );
    let (h, pdf) = engine.recv();
    assert_eq!(h["ok"], true, "{h}");
    assert!(has_image(&pdf));

    // Lengths that do not add up to the body are a protocol error, not a hang or a panic.
    engine.send_with_body(
        json!({ "id": 2, "template": template, "images": [{ "src": "logo", "len": 5 }] }),
        &png,
    );
    let (h, _) = engine.recv();
    assert_eq!(h["error"]["code"], "IPC_PROTOCOL_ERROR", "{h}");
    engine.finish();
}

#[test]
fn local_images_load_only_from_the_image_folder() {
    let dir = std::env::temp_dir().join(format!("pdf-engine-ipc-img-{}", std::process::id()));
    let inside = dir.join("images");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(inside.join("logo.png"), png()).unwrap();
    std::fs::write(dir.join("secret.png"), png()).unwrap();

    let folder = inside.to_str().unwrap();
    let mut engine = Engine::start_with(&[("ASSET_LOCAL_DIR", folder)]);
    let file_url = format!("file://{}", inside.join("logo.png").display());
    for (n, src, expected) in [
        (1, "logo.png".to_string(), true),
        (2, file_url, true),
        (3, "../secret.png".to_string(), false),
        (4, dir.join("secret.png").display().to_string(), false),
    ] {
        engine.send(json!({ "id": n, "template": format!(r#"<img src="{src}">"#) }));
        let (h, pdf) = engine.recv();
        assert_eq!(h["ok"], true, "{h}");
        assert_eq!(has_image(&pdf), expected, "src {src}");
    }
    engine.finish();

    // Without the setting nothing local is read.
    let mut engine = Engine::start();
    engine.send(json!({ "id": 1, "template": r#"<img src="logo.png">"# }));
    assert!(!has_image(&engine.recv().1));
    engine.finish();
    std::fs::remove_dir_all(&dir).ok();
}
