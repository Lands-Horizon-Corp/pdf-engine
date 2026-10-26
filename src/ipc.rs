//! IPC mode: `pdf-engine ipc` serves render requests over stdin/stdout, so a parent process
//! (for example Node.js) can use the engine without HTTP, ports or authentication.
//!
//! # Wire format
//!
//! Both directions send frames:
//!
//! ```text
//! [u32 BE header_len][u32 BE body_len][header: JSON, header_len bytes][body: raw, body_len bytes]
//! ```
//!
//! The PDF travels as the raw body of the response (no base64).
//!
//! Request header (`id` is echoed back; responses may arrive in any order):
//!
//! ```json
//! { "id": 1, "op": "render", "template": "<h1>{{ no }}</h1>", "data": { "no": 42 },
//!   "width": "8.5in", "height": "11in", "orientation": "portrait", "password": "optional" }
//! ```
//!
//! Other ops: `ping` (reports `version` and `protocol`), `load_fonts` (`{ "paths": [...] }`:
//! font files or folders added to the system fonts, replacing any earlier set), `reload_fonts`,
//! `raster` (a receipt or label as ESC/POS or ZPL bytes), `printers` (list printers) and `print`
//! (render and send to a printer). A request's body carries the images it supplies, back to back
//! (`images: [{ "src": "logo", "len": 123 }]` in the header); other ops have none.
//!
//! The Node.js client in `.node/desktop/` speaks this protocol; `PROTOCOL_VERSION` changes whenever it does.
//!
//! Response header, then the PDF as body:
//!
//! ```json
//! { "id": 1, "ok": true, "op": "render", "contentType": "application/pdf", "bytes": 12345 }
//! ```
//!
//! or, with an empty body, the same error object the HTTP API sends:
//!
//! ```json
//! { "id": 1, "ok": false, "error": { "error": "...", "code": "TEMPLATE_ERROR", "status": 400 } }
//! ```
//!
//! Closing stdin finishes the in-flight requests and exits. Only frames go to stdout; logs go
//! to stderr.

use crate::{config::AppState, error::AppError, models::PdfRequest, pdf, printing, raster};
use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Semaphore, mpsc},
    time::timeout,
};

/// Version of the wire format; the Node.js client refuses an engine with a different one.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest accepted request header, in bytes (`IPC_MAX_REQUEST_BYTES`, default 256 MiB).
fn max_request_bytes() -> usize {
    std::env::var("IPC_MAX_REQUEST_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256 * 1024 * 1024)
}

/// Requests accepted but not yet answered (`IPC_MAX_IN_FLIGHT`, default 4 per core). Reading
/// pauses at the limit, so a fast sender can't make the engine hold unbounded requests in memory.
fn max_in_flight(state: &AppState) -> usize {
    std::env::var("IPC_MAX_IN_FLIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(state.render_permit_count * 4)
}

/// A request frame. The body of a `render` request carries its supplied images back to back
/// (see `Request::images`); other ops have none.
struct Frame {
    header: Vec<u8>,
    body: Vec<u8>,
}

/// Read bytes without trusting the announced length: capacity grows as data arrives.
async fn read_exact_vec<R: AsyncRead + Unpin>(r: &mut R, len: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(len.min(16 * 1024 * 1024));
    let read = r.take(len as u64).read_to_end(&mut out).await?;
    if read != len {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(out)
}

/// The next frame, or `None` on a clean end of input.
async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> io::Result<Option<Frame>> {
    let mut prefix = [0u8; 8];
    if r.read(&mut prefix[..1]).await? == 0 {
        return Ok(None);
    }
    r.read_exact(&mut prefix[1..]).await?;
    let header_len = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
    let body_len = u32::from_be_bytes(prefix[4..].try_into().unwrap()) as usize;
    if header_len > max || body_len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {header_len}+{body_len} bytes exceeds the {max}-byte limit"),
        ));
    }
    let header = read_exact_vec(r, header_len).await?;
    let body = read_exact_vec(r, body_len).await?;
    Ok(Some(Frame { header, body }))
}

async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    header: &[u8],
    body: &[u8],
) -> io::Result<()> {
    let mut head = Vec::with_capacity(8 + header.len());
    head.extend_from_slice(&(header.len() as u32).to_be_bytes());
    head.extend_from_slice(&(body.len() as u32).to_be_bytes());
    head.extend_from_slice(header);
    w.write_all(&head).await?;
    if !body.is_empty() {
        w.write_all(body).await?;
    }
    w.flush().await
}

#[derive(Deserialize, Clone)]
pub struct Request {
    #[serde(default)]
    id: Value,
    #[serde(default)]
    op: Option<String>,
    template: Option<String>,
    #[serde(default)]
    data: Value,
    width: Option<String>,
    height: Option<String>,
    orientation: Option<String>,
    password: Option<String>,
    /// `render` op: make a tagged (accessible, PDF/UA) PDF.
    accessible: Option<bool>,
    /// Named templates the template can `{% include %}` or `{% extends %}`.
    partials: Option<std::collections::BTreeMap<String, String>>,
    paths: Option<Vec<String>>,
    /// Images the caller supplies, in body order: the `<img src>` value and the byte length.
    #[serde(default)]
    images: Vec<SuppliedImage>,
    /// `raster` op: `escpos` (receipt printers) or `zpl` (label printers).
    format: Option<String>,
    /// `raster` op: printable width in dots (default 576, an 80 mm roll at 203 dpi).
    dots: Option<usize>,
    dpi: Option<f32>,
    /// `raster` op, ESC/POS: cut after printing (default true) and blank lines to feed first (default 3).
    cut: Option<bool>,
    feed: Option<u8>,
    /// `raster` op: spread grey into dots (photos); off by default, text is sharper without it.
    dither: Option<bool>,
    /// `print` op: where to print (`system://...`, `net://host:9100`, `usb://vendor:product`).
    #[serde(rename = "printerId")]
    printer_id: Option<String>,
    /// `print` op: `thermal`, `label` or `document`; guessed from the printer's name when absent.
    #[serde(rename = "type")]
    kind: Option<String>,
    /// `print` op: `"58mm"`, `"80mm"`, `"112mm"` or a width in dots.
    paper: Option<Value>,
    /// `print` op: `escpos` or `zpl` (default by type).
    language: Option<String>,
    copies: Option<u32>,
    /// `printers` and `print` ops: printers to offer besides the system's (network printers).
    printers: Option<Vec<printing::ExtraPrinter>>,
}

#[derive(Deserialize, Clone)]
struct SuppliedImage {
    src: String,
    len: usize,
}

type Reply = (Vec<u8>, Bytes);

fn error_reply(id: Value, err: &AppError) -> Reply {
    err.log_if_internal();
    let header = json!({ "id": id, "ok": false, "error": err.to_json() });
    (header.to_string().into_bytes(), Bytes::new())
}

fn ok_reply(id: Value, op: &str, extra: Value, body: Bytes) -> Reply {
    let mut header = json!({ "id": id, "ok": true, "op": op });
    if let (Some(h), Some(e)) = (header.as_object_mut(), extra.as_object()) {
        h.extend(e.clone());
    }
    (header.to_string().into_bytes(), body)
}

/// Answer one request given as a JSON header with no body (the command line's `print` and `printers`).
pub async fn handle_one(state: &AppState, header: Vec<u8>) -> (Vec<u8>, Bytes) {
    handle(
        state,
        Frame {
            header,
            body: Vec::new(),
        },
    )
    .await
}

/// Answer one request. Never fails: problems become error replies.
async fn handle(state: &AppState, frame: Frame) -> Reply {
    let mut header = frame.header;
    // Large requests (big `data`) are parsed off the async threads.
    let parsed = if header.len() > 64 * 1024 {
        tokio::task::spawn_blocking(move || simd_json::serde::from_slice::<Request>(&mut header))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
    } else {
        simd_json::serde::from_slice::<Request>(&mut header).map_err(|e| e.to_string())
    };
    let req = match parsed {
        Ok(r) => r,
        Err(e) => {
            let err = AppError::Protocol(format!("request header is not a valid request: {e}"));
            return error_reply(Value::Null, &err);
        }
    };
    let id = req.id.clone();
    match run_op(state, req, frame.body).await {
        Ok(reply) => reply,
        Err(err) => error_reply(id, &err),
    }
}

async fn run_op(state: &AppState, req: Request, body: Vec<u8>) -> Result<Reply, AppError> {
    let id = req.id.clone();
    match req.op.as_deref().unwrap_or("render") {
        "render" => {
            let pdf_req = PdfRequest {
                template: req
                    .template
                    .ok_or_else(|| AppError::MissingField("template".into()))?,
                data: req.data,
                width: req.width.unwrap_or_else(|| "8.5in".into()),
                height: req.height.unwrap_or_else(|| "11in".into()),
                orientation: req.orientation.unwrap_or_else(|| "portrait".into()),
                filename: String::new(),
                password: req.password.filter(|p| !p.is_empty()),
                accessible: req.accessible.unwrap_or(false),
                partials: req.partials.unwrap_or_default().into_iter().collect(),
            };
            let supplied = split_images(&req.images, body)?;
            let pdf = timeout(
                state.render_timeout,
                pdf::render_with_images(pdf_req, supplied, state),
            )
            .await
            .map_err(|_| AppError::Timeout(state.render_timeout))??;
            Ok(ok_reply(
                id,
                "render",
                json!({ "contentType": "application/pdf", "bytes": pdf.len() }),
                pdf,
            ))
        }
        "raster" => {
            let spec = RasterSpec::from_request(
                &req,
                req.dots,
                req.format.as_deref().unwrap_or("escpos"),
            )?;
            let template = req
                .template
                .ok_or_else(|| AppError::MissingField("template".into()))?;
            let supplied = split_images(&req.images, body)?;
            let (commands, width, height) = render_raster(
                state,
                template,
                req.data,
                req.height,
                req.partials.unwrap_or_default().into_iter().collect(),
                &spec,
                supplied,
            )
            .await?;
            Ok(ok_reply(
                id,
                "raster",
                json!({ "contentType": "application/octet-stream", "format": spec.format, "bytes": commands.len(), "width": width, "height": height }),
                Bytes::from(commands),
            ))
        }
        "printers" => {
            let printers = printing::list(&req.printers.unwrap_or_default()).await;
            Ok(ok_reply(
                id,
                "printers",
                json!({ "printers": printers }),
                Bytes::new(),
            ))
        }
        "print" => {
            let result = print_job(state, req, body).await?;
            Ok(ok_reply(id, "print", result, Bytes::new()))
        }
        "ping" => Ok(ok_reply(
            id,
            "ping",
            json!({ "version": env!("CARGO_PKG_VERSION"), "protocol": PROTOCOL_VERSION }),
            Bytes::new(),
        )),
        "load_fonts" => {
            let paths = req
                .paths
                .ok_or_else(|| AppError::MissingField("paths".into()))?;
            let fonts = tokio::task::spawn_blocking(move || read_font_files(&paths)).await??;
            let loaded = fonts.len();
            let renderer = state.renderer.clone();
            tokio::task::spawn_blocking(move || renderer.set_extra_fonts(fonts)).await?;
            Ok(ok_reply(
                id,
                "load_fonts",
                json!({ "loaded": loaded }),
                Bytes::new(),
            ))
        }
        "reload_fonts" => {
            let renderer = state.renderer.clone();
            tokio::task::spawn_blocking(move || renderer.reload_fonts()).await?;
            Ok(ok_reply(id, "reload_fonts", json!({}), Bytes::new()))
        }
        other => Err(AppError::InvalidField(format!(
            "unknown op `{other}` (expected render, raster, printers, print, ping, load_fonts or reload_fonts)"
        ))),
    }
}

/// How a receipt or label is rasterised and encoded.
struct RasterSpec {
    /// `escpos` or `zpl`.
    format: String,
    dots: usize,
    dpi: f32,
    dither: bool,
    cut: bool,
    feed: u8,
}

impl RasterSpec {
    fn from_request(req: &Request, dots: Option<usize>, format: &str) -> Result<Self, AppError> {
        let format = format.to_ascii_lowercase();
        if format != "escpos" && format != "zpl" {
            return Err(AppError::InvalidField(format!(
                "format: `{format}` is not supported (expected escpos or zpl)"
            )));
        }
        Ok(Self {
            format,
            dots: dots.filter(|d| (8..=4096).contains(d)).unwrap_or(576),
            dpi: req
                .dpi
                .filter(|d| (72.0..=1200.0).contains(d))
                .unwrap_or(printing::DPI),
            dither: req.dither.unwrap_or(false),
            cut: req.cut.unwrap_or(true),
            feed: req.feed.unwrap_or(3),
        })
    }
}

/// Render a template at the printer's width and encode it as printer commands, with its size in dots.
async fn render_raster(
    state: &AppState,
    template: String,
    data: Value,
    height: Option<String>,
    partials: Vec<(String, String)>,
    spec: &RasterSpec,
    supplied: Images,
) -> Result<(Vec<u8>, usize, usize), AppError> {
    let width_mm = spec.dots as f32 / spec.dpi * 25.4;
    let pdf_req = PdfRequest {
        template,
        data,
        width: format!("{width_mm:.3}mm"),
        // Tall enough for a long receipt; the blank paper below the content is trimmed.
        height: height.unwrap_or_else(|| "2000mm".into()),
        orientation: "portrait".into(),
        filename: String::new(),
        password: None,
        accessible: false,
        partials,
    };
    let pdf = timeout(
        state.render_timeout,
        pdf::render_with_images(pdf_req, supplied, state),
    )
    .await
    .map_err(|_| AppError::Timeout(state.render_timeout))??;
    let (dots, dither, cut, feed, zpl) = (
        spec.dots,
        spec.dither,
        spec.cut,
        spec.feed,
        spec.format == "zpl",
    );
    let bitmap =
        tokio::task::spawn_blocking(move || raster::pdf_to_bitmap(&pdf, dots, dither)).await??;
    let (width, height) = (bitmap.width, bitmap.height);
    let commands = if zpl {
        raster::encode_zpl(&bitmap)
    } else {
        raster::encode_escpos(&bitmap, feed, cut)
    };
    Ok((commands, width, height))
}

/// Render a template and print it: the PDF on an office printer, ESC/POS on a receipt printer, ZPL on a
/// label printer.
pub async fn print_job(state: &AppState, req: Request, body: Vec<u8>) -> Result<Value, AppError> {
    let printer_id = req
        .printer_id
        .clone()
        .ok_or_else(|| AppError::MissingField("printerId".into()))?;
    let target = printing::parse_id(&printer_id)?;
    let template = req
        .template
        .clone()
        .ok_or_else(|| AppError::MissingField("template".into()))?;
    let extra = req.printers.clone().unwrap_or_default();

    let kind = match req.kind.clone() {
        Some(k) => k,
        None => printing::list(&extra)
            .await
            .into_iter()
            .find(|p| p.id == printer_id)
            .map(|p| p.kind)
            .unwrap_or_else(|| match target {
                printing::Target::System(_) => "document".into(),
                _ => "thermal".into(),
            }),
    };
    if !["thermal", "label", "document"].contains(&kind.as_str()) {
        return Err(AppError::InvalidField(format!(
            "type: `{kind}` is not thermal, label or document"
        )));
    }
    if kind == "document" && matches!(target, printing::Target::Usb { .. }) {
        return Err(AppError::InvalidField(
            "a usb:// printer takes receipt or label commands, not PDFs; print documents through system://<printer name>"
                .into(),
        ));
    }
    let copies = req.copies.unwrap_or(1).clamp(1, 100);
    let supplied = split_images(&req.images, body)?;

    let (bytes, language) = if kind == "document" {
        let pdf_req = PdfRequest {
            template,
            data: req.data.clone(),
            width: req.width.clone().unwrap_or_else(|| "8.5in".into()),
            height: req.height.clone().unwrap_or_else(|| "11in".into()),
            orientation: req.orientation.clone().unwrap_or_else(|| "portrait".into()),
            filename: String::new(),
            password: None,
            accessible: false,
            partials: req
                .partials
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        };
        let pdf = timeout(
            state.render_timeout,
            pdf::render_with_images(pdf_req, supplied, state),
        )
        .await
        .map_err(|_| AppError::Timeout(state.render_timeout))??;
        (pdf.to_vec(), "pdf".to_string())
    } else {
        let language = req.language.clone().unwrap_or_else(|| {
            if kind == "label" {
                "zpl".into()
            } else {
                "escpos".into()
            }
        });
        let dots = printing::paper_dots(req.paper.as_ref())?;
        let spec = RasterSpec::from_request(&req, Some(dots), &language)?;
        let (commands, _, _) = render_raster(
            state,
            template,
            req.data.clone(),
            req.height.clone(),
            req.partials
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect(),
            &spec,
            supplied,
        )
        .await?;
        (commands, spec.format)
    };

    match &target {
        printing::Target::System(name) if language == "pdf" => {
            printing::send_system_pdf(name, &bytes, copies).await?;
        }
        _ => {
            for _ in 0..copies {
                match &target {
                    printing::Target::Net { host, port } => {
                        printing::send_net(host, *port, &bytes).await?
                    }
                    printing::Target::Usb { vendor, product } => {
                        printing::send_usb(*vendor, *product, bytes.clone()).await?
                    }
                    printing::Target::System(name) if language == "pdf" => {
                        printing::send_system_pdf(name, &bytes, 1).await?
                    }
                    printing::Target::System(name) => {
                        printing::send_system_raw(name, &bytes).await?
                    }
                }
            }
        }
    }
    Ok(
        json!({ "printerId": printer_id, "type": kind, "language": language, "bytes": bytes.len(), "copies": copies }),
    )
}

type Images = Vec<(String, std::sync::Arc<Vec<u8>>)>;

/// Cut the request body into the images the header announced.
fn split_images(list: &[SuppliedImage], body: Vec<u8>) -> Result<Images, AppError> {
    if list.is_empty() {
        return Ok(Vec::new());
    }
    let total = list.iter().try_fold(0usize, |t, i| t.checked_add(i.len));
    if total != Some(body.len()) {
        return Err(AppError::Protocol(
            "the images announced in the header do not add up to the request body".into(),
        ));
    }
    let mut at = 0;
    Ok(list
        .iter()
        .map(|i| {
            let bytes = body[at..at + i.len].to_vec();
            at += i.len;
            (i.src.clone(), std::sync::Arc::new(bytes))
        })
        .collect())
}

/// Contents of every font file named by `paths`; a folder contributes the fonts inside it.
fn read_font_files(paths: &[String]) -> Result<Vec<Vec<u8>>, AppError> {
    fn is_font(path: &std::path::Path) -> bool {
        path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
    }
    fn visit(path: &std::path::Path, depth: u8, out: &mut Vec<Vec<u8>>) -> Result<(), AppError> {
        let font_err = |e: std::io::Error| AppError::Font(format!("{}: {e}", path.display()));
        if path.is_dir() {
            if depth > 4 {
                return Ok(());
            }
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .map_err(font_err)?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .collect();
            entries.sort();
            for entry in entries {
                if entry.is_dir() || is_font(&entry) {
                    visit(&entry, depth + 1, out)?;
                }
            }
            Ok(())
        } else if path.is_file() {
            out.push(std::fs::read(path).map_err(font_err)?);
            Ok(())
        } else {
            Err(AppError::Font(format!(
                "font path not found: {}",
                path.display()
            )))
        }
    }
    let mut out = Vec::new();
    for p in paths {
        visit(std::path::Path::new(p), 0, &mut out)?;
    }
    if out.is_empty() && !paths.is_empty() {
        return Err(AppError::Font(
            "no .ttf, .otf or .ttc files found in the given paths".into(),
        ));
    }
    Ok(out)
}

/// Serve requests from `input` and write replies to `output` until `input` ends.
///
/// Requests run concurrently (bounded); replies are written as they finish. Returns an error
/// only for a broken stream, after telling the peer why when it can.
pub async fn run<R, W>(state: AppState, mut input: R, mut output: W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let max = max_request_bytes();
    let limit = max_in_flight(&state);
    let in_flight = Arc::new(Semaphore::new(limit));
    let (tx, mut rx) = mpsc::channel::<Reply>(limit);

    let writer = tokio::spawn(async move {
        while let Some((header, body)) = rx.recv().await {
            if write_frame(&mut output, &header, &body).await.is_err() {
                break;
            }
        }
    });

    let mut outcome = Ok(());
    while !tx.is_closed() {
        let frame = match read_frame(&mut input, max).await {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(e) => {
                // A broken stream can't be resynchronised; say why, then stop.
                let err = AppError::Protocol(format!("unreadable frame: {e}"));
                let _ = tx.send(error_reply(Value::Null, &err)).await;
                outcome = Err(e);
                break;
            }
        };
        let permit = in_flight
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore closed");
        let (state, tx) = (state.clone(), tx.clone());
        tokio::spawn(async move {
            let reply = handle(&state, frame).await;
            let _ = tx.send(reply).await;
            drop(permit);
        });
    }

    // Let the requests already accepted finish and be written.
    let _all = in_flight.acquire_many(limit as u32).await;
    drop(tx);
    let _ = writer.await;
    outcome
}

/// Take over the real stdout for the protocol and point fd 1 at /dev/null, so nothing else
/// (the HTML parser prints warnings with `println!`) can corrupt the frames.
#[cfg(unix)]
pub fn take_stdout() -> io::Result<tokio::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // SAFETY: dup on a valid descriptor; the new descriptor is owned by the returned File.
    let fd = unsafe { libc::dup(libc::STDOUT_FILENO) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let proto = unsafe { std::fs::File::from_raw_fd(fd) };
    let devnull = std::fs::OpenOptions::new().write(true).open("/dev/null")?;
    // SAFETY: dup2 between valid descriptors, before any other thread writes to stdout.
    if unsafe { libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(tokio::fs::File::from_std(proto))
}

/// Windows: keep the real stdout handle for the protocol and point the process's stdout at `NUL`.
/// Rust's `println!` looks the handle up on every write, so after this nothing but the protocol
/// can reach the pipe the parent reads.
#[cfg(windows)]
pub fn take_stdout() -> io::Result<tokio::fs::File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{GetStdHandle, STD_OUTPUT_HANDLE, SetStdHandle},
    };

    // SAFETY: plain Win32 calls on the process's own standard handle.
    let original = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if original.is_null() || original == INVALID_HANDLE_VALUE {
        return Err(io::Error::other(
            "no standard output to write the protocol to",
        ));
    }
    let nul = std::fs::OpenOptions::new().write(true).open("NUL")?;
    // SAFETY: `nul` is a valid open handle; it is leaked below so it stays valid for the process.
    if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, nul.as_raw_handle() as _) } == 0 {
        return Err(io::Error::last_os_error());
    }
    std::mem::forget(nul);
    // SAFETY: `original` was the process's stdout handle; nothing else writes to it now.
    let proto = unsafe { std::fs::File::from_raw_handle(original as _) };
    Ok(tokio::fs::File::from_std(proto))
}

#[cfg(not(any(unix, windows)))]
pub fn take_stdout() -> io::Result<tokio::fs::File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "IPC mode is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn send(w: &mut (impl AsyncWrite + Unpin), header: Value) {
        write_frame(w, header.to_string().as_bytes(), &[])
            .await
            .unwrap();
    }

    async fn recv(r: &mut (impl AsyncRead + Unpin)) -> (Value, Vec<u8>) {
        let mut prefix = [0u8; 8];
        r.read_exact(&mut prefix).await.expect("a reply");
        let header_len = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
        let body_len = u32::from_be_bytes(prefix[4..].try_into().unwrap()) as usize;
        let header = read_exact_vec(r, header_len).await.unwrap();
        let body = read_exact_vec(r, body_len).await.unwrap();
        (serde_json::from_slice(&header).unwrap(), body)
    }

    /// Run the server on one end of a pipe; returns the client's reader and writer.
    fn start() -> (
        impl AsyncRead + Unpin,
        impl AsyncWrite + Unpin,
        tokio::task::JoinHandle<io::Result<()>>,
    ) {
        let (client_w, server_r) = duplex(1 << 20);
        let (server_w, client_r) = duplex(1 << 20);
        let task = tokio::spawn(run(AppState::for_ipc(), server_r, server_w));
        (client_r, client_w, task)
    }

    #[tokio::test]
    async fn ping_and_render_round_trip() {
        let (mut r, mut w, task) = start();
        send(&mut w, json!({ "id": 7, "op": "ping" })).await;
        let (h, body) = recv(&mut r).await;
        assert_eq!((h["id"].clone(), h["ok"].clone()), (json!(7), json!(true)));
        assert!(body.is_empty());

        send(
            &mut w,
            json!({ "id": "a", "template": "<p>{{ n }}</p>", "data": { "n": 42 } }),
        )
        .await;
        let (h, pdf) = recv(&mut r).await;
        assert_eq!(h["id"], "a");
        assert_eq!(h["contentType"], "application/pdf");
        assert_eq!(h["bytes"], pdf.len());
        assert!(pdf.starts_with(b"%PDF"));

        drop(w);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn errors_have_codes_and_keep_the_stream_usable() {
        let (mut r, mut w, task) = start();
        for (req, code) in [
            (json!({ "id": 1 }), "MISSING_FIELD"),
            (json!({ "id": 2, "template": "{% if %}" }), "TEMPLATE_ERROR"),
            (
                json!({ "id": 3, "template": "x", "width": "wide" }),
                "INVALID_FIELD",
            ),
            (json!({ "id": 4, "op": "explode" }), "INVALID_FIELD"),
        ] {
            send(&mut w, req).await;
            let (h, body) = recv(&mut r).await;
            assert_eq!(h["ok"], false, "{h}");
            assert_eq!(h["error"]["code"], code, "{h}");
            assert!(h["error"]["error"].as_str().unwrap().len() > 5);
            assert!(body.is_empty());
        }
        send(&mut w, json!({ "id": 5, "op": "ping" })).await;
        assert_eq!(recv(&mut r).await.0["ok"], true);
        drop(w);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn replies_match_their_request_when_sent_concurrently() {
        let (mut r, mut w, task) = start();
        for i in 0..24 {
            send(
                &mut w,
                json!({ "id": i, "template": "<p>{{ i }}</p>", "data": { "i": i } }),
            )
            .await;
        }
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..24 {
            let (h, pdf) = recv(&mut r).await;
            assert_eq!(h["ok"], true, "{h}");
            assert!(pdf.starts_with(b"%PDF"));
            seen.insert(h["id"].as_u64().unwrap());
        }
        assert_eq!(seen.len(), 24);
        drop(w);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn garbage_header_is_an_error_not_a_crash() {
        let (mut r, mut w, task) = start();
        write_frame(&mut w, b"this is not json", &[]).await.unwrap();
        let (h, _) = recv(&mut r).await;
        assert_eq!(h["ok"], false);
        assert_eq!(h["error"]["code"], "IPC_PROTOCOL_ERROR");
        send(&mut w, json!({ "id": 1, "op": "ping" })).await;
        assert_eq!(recv(&mut r).await.0["ok"], true);
        drop(w);
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn oversized_frames_end_the_session_with_a_protocol_error() {
        let (mut r, mut w, task) = start();
        // Announces a 3 GiB header; nothing near that is ever allocated.
        w.write_all(&[0xC0, 0, 0, 0, 0, 0, 0, 0]).await.unwrap();
        let (h, _) = recv(&mut r).await;
        assert_eq!(h["error"]["code"], "IPC_PROTOCOL_ERROR");
        assert!(task.await.unwrap().is_err());
    }
}
