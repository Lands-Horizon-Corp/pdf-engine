//! Printing end to end: `pdf-engine print` and `pdf-engine printers` against a fake network printer
//! (a TCP listener), so the bytes a real printer would receive are checked exactly.

use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

/// A listener that hands back everything the first connection sends.
fn fake_printer() -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let mut received = Vec::new();
            let _ = socket.read_to_end(&mut received);
            let _ = tx.send(received);
        }
    });
    (port, rx)
}

/// Run `pdf-engine <command>` with the request on standard input; the answer and the exit status.
fn cli(command: &str, request: &Value) -> (Value, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pdf-engine"))
        .args([command, "--request", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let line = text.lines().last().unwrap_or("");
    (
        serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON ({e}): {text}")),
        out.status.success(),
    )
}

const TEMPLATE: &str =
    "<b>STORE</b>{% for i in items %}<div>{{ i }} ₱10</div>{% endfor %}<b>TOTAL ₱20</b>";

#[test]
fn a_receipt_reaches_a_network_printer_as_escpos() {
    let (port, received) = fake_printer();
    let (answer, ok) = cli(
        "print",
        &json!({ "printerId": format!("net://127.0.0.1:{port}"), "paper": "58mm", "template": TEMPLATE, "data": { "items": ["Sardines", "Coffee"] } }),
    );
    assert!(ok, "{answer}");
    assert_eq!(
        (answer["type"].as_str(), answer["language"].as_str()),
        (Some("thermal"), Some("escpos"))
    );

    let bytes = received
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        bytes.len() as u64,
        answer["bytes"].as_u64().unwrap(),
        "every byte arrived"
    );
    assert_eq!(&bytes[..2], &[0x1B, 0x40], "ESC @");
    assert_eq!(&bytes[2..6], &[0x1D, 0x76, 0x30, 0x00], "a raster image");
    assert_eq!(
        u16::from_le_bytes([bytes[6], bytes[7]]),
        48,
        "58 mm is 384 dots, 48 bytes per row"
    );
    assert_eq!(
        &bytes[bytes.len() - 4..],
        &[0x1D, 0x56, 0x42, 0x00],
        "ends with the cut"
    );
    // There is ink: not a blank roll.
    assert!(bytes[10..bytes.len() - 7].iter().any(|&b| b != 0));
}

#[test]
fn labels_get_zpl_and_documents_get_a_pdf() {
    let (port, received) = fake_printer();
    let id = format!("net://127.0.0.1:{port}");
    let (answer, ok) = cli(
        "print",
        &json!({ "printerId": id, "type": "label", "paper": "58mm", "template": "<b>SKU 123</b>" }),
    );
    assert!(ok, "{answer}");
    let zpl = String::from_utf8(
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap(),
    )
    .unwrap();
    assert!(
        zpl.starts_with("^XA^PW384^LL") && zpl.contains("^GFA,") && zpl.ends_with("^FS^XZ"),
        "{}",
        &zpl[..60]
    );

    let (port, received) = fake_printer();
    let (answer, ok) = cli(
        "print",
        &json!({ "printerId": format!("net://127.0.0.1:{port}"), "type": "document", "template": "<h1>Invoice</h1>" }),
    );
    assert!(ok, "{answer}");
    assert_eq!(answer["language"], "pdf");
    assert!(
        received
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .starts_with(b"%PDF-")
    );
}

#[test]
fn copies_are_printed_one_after_another() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let counter = thread::spawn(move || {
        let mut jobs = 0;
        for stream in listener.incoming().take(3) {
            let mut sink = Vec::new();
            stream.unwrap().read_to_end(&mut sink).unwrap();
            jobs += 1;
        }
        jobs
    });
    let (answer, ok) = cli(
        "print",
        &json!({ "printerId": format!("net://127.0.0.1:{port}"), "copies": 3, "template": "<b>x</b>" }),
    );
    assert!(ok, "{answer}");
    assert_eq!(counter.join().unwrap(), 3);
}

#[test]
fn problems_are_reported_with_codes_and_a_failing_exit_status() {
    // Nothing listens on port 1.
    let (answer, ok) = cli(
        "print",
        &json!({ "printerId": "net://127.0.0.1:1", "template": "<b>x</b>" }),
    );
    assert!(!ok);
    assert_eq!(answer["error"]["code"], "PRINTER_UNREACHABLE", "{answer}");
    assert!(answer["error"]["hint"].is_string());

    for (request, code) in [
        (json!({ "template": "x" }), "MISSING_FIELD"),
        (
            json!({ "printerId": "lpt1", "template": "x" }),
            "INVALID_FIELD",
        ),
        (
            json!({ "printerId": "net://h", "paper": "A4", "template": "x" }),
            "INVALID_FIELD",
        ),
        (
            json!({ "printerId": "net://h", "type": "toaster", "template": "x" }),
            "INVALID_FIELD",
        ),
        (
            json!({ "printerId": "usb://0483:5740", "type": "document", "template": "x" }),
            "INVALID_FIELD",
        ),
        (json!({ "printerId": "net://h" }), "MISSING_FIELD"),
    ] {
        let (answer, ok) = cli("print", &request);
        assert!(!ok, "{request}");
        assert_eq!(answer["error"]["code"], code, "{request}: {answer}");
    }
    let (answer, ok) = cli("print", &json!([1, 2]));
    assert!(!ok);
    assert_eq!(answer["error"]["code"], "INVALID_FIELD");
}

#[test]
fn printers_lists_the_ones_you_name() {
    let (answer, ok) = cli(
        "printers",
        &json!({ "printers": [{ "id": "net://192.168.1.150:9100", "name": "Kitchen Epson TM-T82" }] }),
    );
    assert!(ok, "{answer}");
    let first = &answer["printers"][0];
    assert_eq!(first["id"], "net://192.168.1.150:9100");
    assert_eq!(
        (first["type"].as_str(), first["transport"].as_str()),
        (Some("thermal"), Some("net"))
    );
    // Without any request: the system's printers, whatever they are, as a list.
    let output = Command::new(env!("CARGO_BIN_EXE_pdf-engine"))
        .arg("printers")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(answer["printers"].is_array());
}

#[test]
fn the_request_can_be_given_on_the_command_line() {
    let (port, received) = fake_printer();
    let request = json!({ "printerId": format!("net://127.0.0.1:{port}"), "template": "<b>x</b>" })
        .to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_pdf-engine"))
        .args(["print", "--json", &request])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        &received
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()[..2],
        &[0x1B, 0x40]
    );
}
