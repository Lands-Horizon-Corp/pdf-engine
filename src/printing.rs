//! Finding printers and sending them bytes: receipt (ESC/POS), label (ZPL) and office (PDF) printers.
//!
//! This is the one implementation behind the Node/Electron client (`printers` and `print` over IPC) and
//! the command line (`pdf-engine printers`, `pdf-engine print`, which a Tauri sidecar can run). It needs
//! no native libraries: the operating system's own tools (CUPS `lp` and `lpstat` on macOS and Linux,
//! PowerShell on Windows), a TCP socket for network printers, and `nusb` (pure Rust) for USB.
//!
//! Printer ids:
//! - `system://<queue name>`: a printer installed in the operating system (USB, network or Bluetooth).
//! - `net://<host>[:<port>]`: a network printer that takes raw bytes (port 9100 unless given).
//! - `usb://<vendor>:<product>`: a USB device talked to directly (hexadecimal ids), for receipt and label
//!   printers.

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpStream, process::Command, time::timeout};

/// Printable width in dots at 203 dpi, the resolution of nearly every receipt and label printer.
pub const DPI: f32 = 203.0;

/// A roll width (`"58mm"`, `"80mm"`, `"112mm"`) or a number of dots as printable dots.
pub fn paper_dots(paper: Option<&Value>) -> Result<usize, AppError> {
    match paper {
        None | Some(Value::Null) => Ok(576),
        Some(Value::Number(n)) => n
            .as_u64()
            .filter(|d| (8..=4096).contains(d))
            .map(|d| d as usize)
            .ok_or_else(|| {
                AppError::InvalidField("paper: a width in dots must be between 8 and 4096".into())
            }),
        Some(Value::String(s)) => match s.as_str() {
            "58mm" => Ok(384),
            "80mm" => Ok(576),
            "112mm" => Ok(832),
            other => Err(AppError::InvalidField(format!(
                "paper: `{other}` is not 58mm, 80mm, 112mm or a width in dots"
            ))),
        },
        Some(_) => Err(AppError::InvalidField(
            "paper must be '58mm', '80mm', '112mm' or a number of dots".into(),
        )),
    }
}

/// A printer the system (or the caller) knows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Printer {
    pub id: String,
    pub name: String,
    /// `thermal` (receipt roll), `label` (barcode and label) or `document` (office printer).
    #[serde(rename = "type")]
    pub kind: String,
    /// `ready`, `busy`, `offline` or `unknown`.
    pub status: String,
    /// `system`, `net` or `usb`.
    pub transport: String,
    pub default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// A printer the caller lists itself (network printers cannot be discovered).
#[derive(Debug, Clone, Deserialize)]
pub struct ExtraPrinter {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

const THERMAL: &str = r"(?i)\b(pos|receipt|thermal|tm[\s_-]?t\d|tm[\s_-]?[mp]\d|tsp\d|rp\d|xp[\s_-]?\d|gp[\s_-]?\d|mp[\s_-]?\d|mtp|sp\d{3}|58\s?mm|80\s?mm|escpos|esc/pos|bixolon|star\s?micronics|citizen\s?ct)";
const LABEL: &str = r"(?i)\b(zebra|zpl|label|tspl|tsc|gk\d|gx\d|zd\d|zt\d|barcode|sticker|datamax|honeywell\s?pc|brother\s?ql)";

/// `thermal`, `label` or `document`, guessed from a printer's name (and driver or port).
pub fn classify(name: &str, extra: &str) -> &'static str {
    use std::sync::LazyLock;
    static THERMAL_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(THERMAL).unwrap());
    static LABEL_RE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(LABEL).unwrap());
    let text = format!("{name} {extra}");
    if LABEL_RE.is_match(&text) {
        "label"
    } else if THERMAL_RE.is_match(&text) {
        "thermal"
    } else {
        "document"
    }
}

/// Parse `lpstat -p`, `lpstat -v` and `lpstat -d` (CUPS: macOS and Linux).
pub fn parse_lpstat(printers: &str, devices: &str, defaults: &str) -> Vec<Printer> {
    let uris: std::collections::HashMap<&str, &str> = devices
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("device for ")?;
            let (name, uri) = rest.split_once(':')?;
            Some((name.trim(), uri.trim()))
        })
        .collect();
    let default = defaults
        .trim()
        .rsplit_once("destination:")
        .map(|(_, d)| d.trim());
    printers
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("printer ")?;
            let (name, state) = rest.split_once(char::is_whitespace)?;
            let lower = state.to_ascii_lowercase();
            let status = if lower.contains("disabled")
                || lower.contains("offline")
                || lower.contains("not accepting")
            {
                "offline"
            } else if lower.contains("printing") {
                "busy"
            } else {
                "ready"
            };
            let uri = uris.get(name).copied().unwrap_or("");
            let shown = name.replace('_', " ");
            Some(Printer {
                id: format!("system://{name}"),
                kind: classify(&shown, uri).into(),
                name: shown,
                status: status.into(),
                transport: "system".into(),
                default: Some(name) == default,
                uri: (!uri.is_empty()).then(|| uri.to_string()),
            })
        })
        .collect()
}

/// Parse `Get-Printer | ConvertTo-Json` (Windows): one object or an array.
pub fn parse_windows_printers(json: &str) -> Vec<Printer> {
    let Ok(value) = serde_json::from_str::<Value>(json.trim()) else {
        return Vec::new();
    };
    let list = match value {
        Value::Array(a) => a,
        v @ Value::Object(_) => vec![v],
        _ => return Vec::new(),
    };
    list.iter()
        .filter_map(|p| {
            let name = p.get("Name")?.as_str()?.to_string();
            let driver = p.get("DriverName").and_then(Value::as_str).unwrap_or("");
            let port = p.get("PortName").and_then(Value::as_str).unwrap_or("");
            let state = match p.get("PrinterStatus") {
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::String(s)) => s.to_ascii_lowercase(),
                _ => String::new(),
            };
            // 0 = Normal, 1 = Paused, 2 = Error, 3 = Pending deletion, 4 = Paper jam, 5 = Paper out, 6 = Manual feed,
            // 7 = Paper problem, 8 = Offline ... (a number or its name).
            let status = match state.as_str() {
                "" | "0" | "normal" | "idle" => "ready",
                "printing" | "4" => "busy",
                _ => "offline",
            };
            Some(Printer {
                id: format!("system://{name}"),
                kind: classify(&name, &format!("{driver} {port}")).into(),
                name,
                status: status.into(),
                transport: "system".into(),
                default: p.get("Default").and_then(Value::as_bool).unwrap_or(false),
                uri: (!port.is_empty()).then(|| port.to_string()),
            })
        })
        .collect()
}

/// Where a printer id points.
#[derive(Debug, PartialEq)]
pub enum Target {
    System(String),
    Net { host: String, port: u16 },
    Usb { vendor: u16, product: u16 },
}

pub fn parse_id(id: &str) -> Result<Target, AppError> {
    let bad = || {
        AppError::InvalidField(format!(
            "printerId `{id}` must look like system://<printer name>, net://<host>[:<port>] or usb://<vendor>:<product>"
        ))
    };
    let (scheme, rest) = id.split_once("://").ok_or_else(bad)?;
    if rest.is_empty() {
        return Err(bad());
    }
    match scheme {
        "system" => Ok(Target::System(rest.to_string())),
        "net" => {
            let (host, port) = match rest.rsplit_once(':') {
                Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, Some(p)),
                _ => (rest, None),
            };
            let port = match port {
                Some(p) => p.parse::<u16>().map_err(|_| bad())?,
                None => 9100,
            };
            Ok(Target::Net {
                host: host.trim_matches(['[', ']']).to_string(),
                port,
            })
        }
        "usb" => {
            let (v, p) = rest.split_once(':').ok_or_else(bad)?;
            let hex = |s: &str| {
                (s.len() == 4)
                    .then(|| u16::from_str_radix(s, 16).ok())
                    .flatten()
            };
            Ok(Target::Usb {
                vendor: hex(v).ok_or_else(bad)?,
                product: hex(p).ok_or_else(bad)?,
            })
        }
        _ => Err(bad()),
    }
}

async fn run(program: &str, args: &[&str], limit: Duration) -> Result<String, std::io::Error> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // no console window
    let child = cmd.spawn()?;
    let out = timeout(limit, child.wait_with_output())
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("`{program}` did not finish in {limit:?}"),
            )
        })??;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let why = String::from_utf8_lossy(&out.stderr);
        let why = if why.trim().is_empty() {
            String::from_utf8_lossy(&out.stdout)
        } else {
            why
        };
        Err(std::io::Error::other(format!(
            "`{program}` failed: {}",
            why.trim()
        )))
    }
}

/// The printers the system knows, then the ones the caller lists, then USB receipt and label printers.
pub async fn list(extra: &[ExtraPrinter]) -> Vec<Printer> {
    let limit = Duration::from_secs(20);
    let mut found: Vec<Printer> = if cfg!(windows) {
        let json = run(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-Printer | Select-Object Name,DriverName,PortName,PrinterStatus | ConvertTo-Json -Compress",
            ],
            limit,
        )
        .await
        .unwrap_or_default();
        let mut printers = parse_windows_printers(&json);
        let default = run(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_Printer | Where-Object Default).Name",
            ],
            limit,
        )
        .await
        .unwrap_or_default();
        let default = default.trim();
        for p in &mut printers {
            p.default = !default.is_empty() && p.name == default;
        }
        printers
    } else {
        let (p, v, d) = tokio::join!(
            run("lpstat", &["-p"], limit),
            run("lpstat", &["-v"], limit),
            run("lpstat", &["-d"], limit),
        );
        parse_lpstat(
            &p.unwrap_or_default(),
            &v.unwrap_or_default(),
            &d.unwrap_or_default(),
        )
    };

    let mut listed: Vec<Printer> = extra
        .iter()
        .filter_map(|e| {
            let target = parse_id(&e.id).ok()?;
            let (transport, fallback) = match &target {
                Target::Net { host, port } => ("net", format!("{host}:{port}")),
                Target::Usb { vendor, product } => ("usb", format!("{vendor:04x}:{product:04x}")),
                Target::System(name) => ("system", name.clone()),
            };
            let name = e.name.clone().unwrap_or(fallback);
            let kind = e.kind.clone().unwrap_or_else(|| {
                if transport == "system" {
                    classify(&name, "").into()
                } else {
                    "thermal".into()
                }
            });
            Some(Printer {
                id: e.id.clone(),
                name,
                kind,
                status: "unknown".into(),
                transport: transport.into(),
                default: false,
                uri: None,
            })
        })
        .collect();
    let taken: std::collections::HashSet<String> = listed.iter().map(|p| p.id.clone()).collect();
    found.retain(|p| !taken.contains(&p.id));
    listed.extend(found);

    let usb = tokio::task::spawn_blocking(usb_printers)
        .await
        .unwrap_or_default();
    listed.extend(usb.into_iter().filter(|p| !taken.contains(&p.id)));
    listed
}

/// Vendors of receipt printers that show a vendor-specific interface instead of the printer class.
const THERMAL_VENDORS: [u16; 12] = [
    0x04b8, 0x0519, 0x0483, 0x0416, 0x28e9, 0x0dd4, 0x0fe6, 0x1504, 0x0525, 0x154f, 0x20d1, 0x6868,
];
const PRINTER_CLASS: u8 = 7;

fn usb_printers() -> Vec<Printer> {
    use nusb::MaybeFuture;
    let Ok(devices) = nusb::list_devices().wait() else {
        return Vec::new();
    };
    devices
        .filter(|d| {
            d.interfaces().any(|i| i.class() == PRINTER_CLASS)
                || THERMAL_VENDORS.contains(&d.vendor_id())
        })
        .filter(|d| {
            d.interfaces().any(|i| i.class() == PRINTER_CLASS)
                || d.class() == 0xFF
                || d.class() == 0
        })
        .map(|d| {
            let id = format!("usb://{:04x}:{:04x}", d.vendor_id(), d.product_id());
            let name = d.product_string().map(str::to_string).unwrap_or_else(|| {
                format!("USB printer {:04x}:{:04x}", d.vendor_id(), d.product_id())
            });
            let maker = d.manufacturer_string().unwrap_or("");
            // Raw commands only suit receipt and label printers; an office printer goes through the system.
            let kind = if classify(&format!("{maker} {name}"), "") == "label" {
                "label"
            } else {
                "thermal"
            };
            Printer {
                id,
                name,
                kind: kind.into(),
                status: "unknown".into(),
                transport: "usb".into(),
                default: false,
                uri: None,
            }
        })
        .collect()
}

/// Send bytes to a network printer.
pub async fn send_net(host: &str, port: u16, bytes: &[u8]) -> Result<(), AppError> {
    let unreachable = |why: String| AppError::PrinterUnreachable(format!("{host}:{port}: {why}"));
    let mut stream = timeout(Duration::from_secs(10), TcpStream::connect((host, port)))
        .await
        .map_err(|_| unreachable("timed out".into()))?
        .map_err(|e| unreachable(e.to_string()))?;
    timeout(Duration::from_secs(30), async {
        stream.write_all(bytes).await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| unreachable("timed out while sending".into()))?
    .map_err(|e| unreachable(e.to_string()))
}

/// A temporary file that is removed when it goes out of scope.
struct TempFile(PathBuf);

impl TempFile {
    async fn create(ext: &str, bytes: &[u8]) -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!("pdf-engine-{}{ext}", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, bytes).await?;
        Ok(Self(path))
    }
    fn path(&self) -> &str {
        self.0.to_str().unwrap_or_default()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// PowerShell that writes a file to a printer queue as raw bytes (winspool.drv).
const WINDOWS_RAW: &str = r#"
param([string]$Printer, [string]$File)
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
public class RawPrint {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public class DOCINFO { public string pDocName; public string pOutputFile; public string pDataType; }
  [DllImport("winspool.drv", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool OpenPrinter(string n, out IntPtr h, IntPtr d);
  [DllImport("winspool.drv", SetLastError=true)] public static extern bool ClosePrinter(IntPtr h);
  [DllImport("winspool.drv", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool StartDocPrinter(IntPtr h, int l, [In] DOCINFO d);
  [DllImport("winspool.drv", SetLastError=true)] public static extern bool EndDocPrinter(IntPtr h);
  [DllImport("winspool.drv", SetLastError=true)] public static extern bool StartPagePrinter(IntPtr h);
  [DllImport("winspool.drv", SetLastError=true)] public static extern bool EndPagePrinter(IntPtr h);
  [DllImport("winspool.drv", SetLastError=true)] public static extern bool WritePrinter(IntPtr h, byte[] b, int c, out int w);
}
"@
$bytes = [System.IO.File]::ReadAllBytes($File)
$h = [IntPtr]::Zero
if (-not [RawPrint]::OpenPrinter($Printer, [ref]$h, [IntPtr]::Zero)) { throw "Cannot open printer $Printer" }
$d = New-Object RawPrint+DOCINFO; $d.pDocName = 'pdf-engine'; $d.pDataType = 'RAW'
[void][RawPrint]::StartDocPrinter($h, 1, $d); [void][RawPrint]::StartPagePrinter($h)
$w = 0
if (-not [RawPrint]::WritePrinter($h, $bytes, $bytes.Length, [ref]$w)) { throw "Cannot write to printer $Printer" }
[void][RawPrint]::EndPagePrinter($h); [void][RawPrint]::EndDocPrinter($h); [void][RawPrint]::ClosePrinter($h)
"#;

fn printer_error(e: std::io::Error) -> AppError {
    AppError::Printer(e.to_string())
}

/// Send raw printer commands (ESC/POS, ZPL) to a system queue.
pub async fn send_system_raw(name: &str, bytes: &[u8]) -> Result<(), AppError> {
    let job = TempFile::create(".bin", bytes).await?;
    if cfg!(windows) {
        let script = TempFile::create(".ps1", WINDOWS_RAW.as_bytes()).await?;
        run(
            "powershell.exe",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.path(),
                name,
                job.path(),
            ],
            Duration::from_secs(60),
        )
        .await
        .map(drop)
        .map_err(printer_error)
    } else {
        run(
            "lp",
            &["-d", name, "-o", "raw", job.path()],
            Duration::from_secs(60),
        )
        .await
        .map(drop)
        .map_err(printer_error)
    }
}

/// Print a PDF on an office printer through the operating system.
pub async fn send_system_pdf(name: &str, pdf: &[u8], copies: u32) -> Result<(), AppError> {
    let job = TempFile::create(".pdf", pdf).await?;
    if cfg!(windows) {
        // SumatraPDF prints silently when it is installed; otherwise the default PDF app's "print to" verb.
        let sumatra = run("where.exe", &["SumatraPDF.exe"], Duration::from_secs(10))
            .await
            .ok()
            .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
            .filter(|s| !s.is_empty());
        for _ in 0..copies {
            let result = match &sumatra {
                Some(exe) => {
                    run(
                        exe,
                        &["-print-to", name, "-silent", job.path()],
                        Duration::from_secs(120),
                    )
                    .await
                }
                None => {
                    let script = format!(
                        "Start-Process -FilePath '{}' -Verb PrintTo -ArgumentList '\"{}\"' -WindowStyle Hidden -Wait",
                        job.path().replace('\'', "''"),
                        name.replace('\'', "''")
                    );
                    run(
                        "powershell.exe",
                        &["-NoProfile", "-NonInteractive", "-Command", &script],
                        Duration::from_secs(120),
                    )
                    .await
                }
            };
            result.map_err(printer_error)?;
        }
        Ok(())
    } else {
        run(
            "lp",
            &["-d", name, "-n", &copies.to_string(), job.path()],
            Duration::from_secs(60),
        )
        .await
        .map(drop)
        .map_err(printer_error)
    }
}

/// Send bytes straight to a USB printer (receipt and label printers).
pub async fn send_usb(vendor: u16, product: u16, bytes: Vec<u8>) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || send_usb_blocking(vendor, product, &bytes)).await?
}

fn send_usb_blocking(vendor: u16, product: u16, bytes: &[u8]) -> Result<(), AppError> {
    use nusb::{
        MaybeFuture,
        descriptors::TransferType,
        transfer::{Bulk, Direction, Out},
    };
    use std::io::Write;
    let id = format!("{vendor:04x}:{product:04x}");
    let info = nusb::list_devices()
        .wait()
        .map_err(|e| AppError::Printer(format!("USB: {e}")))?
        .find(|d| d.vendor_id() == vendor && d.product_id() == product)
        .ok_or_else(|| AppError::PrinterUnreachable(format!("no USB device {id} is connected")))?;
    let hint = " (on Windows the device needs a WinUSB driver; on Linux the user needs permission on the device)";
    let device = info
        .open()
        .wait()
        .map_err(|e| AppError::Printer(format!("USB {id}: {e}{hint}")))?;

    // The printer interface with a bulk OUT endpoint, preferring the printer class.
    let config = device
        .active_configuration()
        .map_err(|e| AppError::Printer(format!("USB {id}: {e}")))?;
    let mut found: Option<(u8, u8, bool)> = None;
    for alt in config.interface_alt_settings() {
        if let Some(ep) = alt
            .endpoints()
            .find(|e| e.direction() == Direction::Out && e.transfer_type() == TransferType::Bulk)
        {
            let printer_class = alt.class() == PRINTER_CLASS;
            if found.is_none() || (printer_class && !found.is_some_and(|f| f.2)) {
                found = Some((alt.interface_number(), ep.address(), printer_class));
            }
        }
    }
    let (interface_number, endpoint, _) =
        found.ok_or_else(|| AppError::Printer(format!("USB {id} has no printer endpoint")))?;

    // Linux: the kernel's usblp driver may hold the printer; take it for this job.
    let interface = device
        .detach_and_claim_interface(interface_number)
        .wait()
        .map_err(|e| AppError::Printer(format!("USB {id}: {e}{hint}")))?;
    let out = interface
        .endpoint::<Bulk, Out>(endpoint)
        .map_err(|e| AppError::Printer(format!("USB {id}: {e}")))?;
    let mut writer = out.writer(16 * 1024);
    writer
        .write_all(bytes)
        .map_err(|e| AppError::Printer(format!("USB {id}: {e}")))?;
    writer
        .flush()
        .map_err(|e| AppError::Printer(format!("USB {id}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_label_and_office_printers_are_told_apart() {
        for name in [
            "POS-58 Thermal Receipt Printer",
            "EPSON TM-T82",
            "Star TSP143",
            "Rongta RP326",
            "XP-80C",
            "GP-58130",
        ] {
            assert_eq!(classify(name, ""), "thermal", "{name}");
        }
        for name in [
            "Zebra ZD420",
            "TSC TTP-244",
            "Label Printer",
            "Brother QL-800",
        ] {
            assert_eq!(classify(name, ""), "label", "{name}");
        }
        for name in [
            "HP LaserJet P1102",
            "Canon PIXMA",
            "Office A4 Invoice Printer",
        ] {
            assert_eq!(classify(name, ""), "document", "{name}");
        }
    }

    #[test]
    fn paper_widths() {
        assert_eq!(paper_dots(None).unwrap(), 576);
        assert_eq!(paper_dots(Some(&serde_json::json!("58mm"))).unwrap(), 384);
        assert_eq!(paper_dots(Some(&serde_json::json!("112mm"))).unwrap(), 832);
        assert_eq!(paper_dots(Some(&serde_json::json!(640))).unwrap(), 640);
        for bad in [
            serde_json::json!("A4"),
            serde_json::json!(4),
            serde_json::json!(true),
            serde_json::json!(99999),
        ] {
            assert_eq!(
                paper_dots(Some(&bad)).unwrap_err().code(),
                "INVALID_FIELD",
                "{bad}"
            );
        }
    }

    #[test]
    fn cups_output_is_parsed() {
        let printers = parse_lpstat(
            "printer POS58_USB is idle.  enabled since Sat Oct 25 09:00:00 2026\n\
             printer HP_LaserJet_P1102 disabled since Sat Oct 25 08:00:00 2026 -\n\
             printer Kitchen_TM_T82 now printing Kitchen_TM_T82-12.  enabled since Sat",
            "device for POS58_USB: usb://POS/POS58?serial=1\n\
             device for HP_LaserJet_P1102: ipp://printer.local/ipp/print\n\
             device for Kitchen_TM_T82: socket://192.168.1.150:9100",
            "system default destination: HP_LaserJet_P1102",
        );
        let summary: Vec<_> = printers
            .iter()
            .map(|p| (p.id.as_str(), p.kind.as_str(), p.status.as_str(), p.default))
            .collect();
        assert_eq!(
            summary,
            [
                ("system://POS58_USB", "thermal", "ready", false),
                ("system://HP_LaserJet_P1102", "document", "offline", true),
                ("system://Kitchen_TM_T82", "thermal", "busy", false),
            ]
        );
        assert_eq!(printers[0].uri.as_deref(), Some("usb://POS/POS58?serial=1"));
        assert!(parse_lpstat("", "", "").is_empty());
    }

    #[test]
    fn windows_output_is_parsed() {
        let many = parse_windows_printers(
            r#"[{"Name":"POS-58","DriverName":"Generic / Text Only","PortName":"USB001","PrinterStatus":0},
                {"Name":"HP LaserJet","DriverName":"HP","PortName":"IP_1","PrinterStatus":3}]"#,
        );
        let summary: Vec<_> = many
            .iter()
            .map(|p| (p.id.as_str(), p.kind.as_str(), p.status.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("system://POS-58", "thermal", "ready"),
                ("system://HP LaserJet", "document", "offline")
            ]
        );
        assert_eq!(
            parse_windows_printers(r#"{"Name":"Zebra ZD420","PrinterStatus":"Normal"}"#)[0].kind,
            "label"
        );
        assert!(parse_windows_printers("").is_empty());
        assert!(parse_windows_printers("not json").is_empty());
    }

    #[test]
    fn printer_ids() {
        assert_eq!(
            parse_id("system://My Printer").unwrap(),
            Target::System("My Printer".into())
        );
        assert_eq!(
            parse_id("net://192.168.1.150:9100").unwrap(),
            Target::Net {
                host: "192.168.1.150".into(),
                port: 9100
            }
        );
        assert_eq!(
            parse_id("net://kitchen.local").unwrap(),
            Target::Net {
                host: "kitchen.local".into(),
                port: 9100
            }
        );
        assert_eq!(
            parse_id("usb://0483:5740").unwrap(),
            Target::Usb {
                vendor: 0x0483,
                product: 0x5740
            }
        );
        for bad in [
            "http://printer",
            "printer",
            "",
            "net://",
            "usb://0483",
            "usb://zzzz:5740",
            "net://h:notaport",
        ] {
            assert_eq!(parse_id(bad).unwrap_err().code(), "INVALID_FIELD", "{bad}");
        }
    }

    #[tokio::test]
    async fn network_printers_receive_the_bytes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let reader = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            socket.read_to_end(&mut received).await.unwrap();
            received
        });
        let bytes = vec![0x1b, 0x40, 1, 2, 3, 0x1d, 0x56, 0x42, 0];
        send_net("127.0.0.1", port, &bytes).await.unwrap();
        assert_eq!(reader.await.unwrap(), bytes);
        // Nothing listening there.
        let err = send_net("127.0.0.1", 1, &bytes).await.unwrap_err();
        assert_eq!(err.code(), "PRINTER_UNREACHABLE");
    }

    #[tokio::test]
    async fn listing_adds_the_printers_you_name() {
        let extra = [ExtraPrinter {
            id: "net://192.168.1.150:9100".into(),
            name: Some("Kitchen".into()),
            kind: None,
        }];
        let printers = list(&extra).await;
        let kitchen = printers
            .iter()
            .find(|p| p.id == "net://192.168.1.150:9100")
            .unwrap();
        assert_eq!(
            (
                kitchen.kind.as_str(),
                kitchen.transport.as_str(),
                kitchen.name.as_str()
            ),
            ("thermal", "net", "Kitchen")
        );
        assert_eq!(printers[0].id, kitchen.id, "yours come first");
    }
}
