'use strict';

/**
 * Finding printers and sending them bytes. No native modules: the operating system's own tools
 * (CUPS `lp`/`lpstat` on macOS and Linux, PowerShell on Windows) and a TCP socket for network printers.
 *
 * Printer ids:
 *   system://<queue name>     a printer installed in the operating system (USB, network or Bluetooth),
 *                             thermal printers included: they appear as a queue once their driver is added
 *   net://<host>[:<port>]     a network printer that takes raw bytes (port 9100 unless given)
 */

const { execFile } = require('node:child_process');
const net = require('node:net');
const os = require('node:os');
const fs = require('node:fs');
const path = require('node:path');
const usbPrinting = require('./usb.js');

/** Printable width in dots at 203 dpi, the resolution of nearly every receipt and label printer. */
const PAPER_DOTS = { '58mm': 384, '80mm': 576, '112mm': 832 };
const DPI = 203;

const THERMAL = /\b(pos|receipt|thermal|tm[\s_-]?t\d|tm[\s_-]?[mp]\d|tsp\d|rp\d|xp[\s_-]?\d|gp[\s_-]?\d|mp[\s_-]?\d|mtp|sp\d{3}|58\s?mm|80\s?mm|escpos|esc\/pos|bixolon|star\s?micronics|citizen\s?ct)/i;
const LABEL = /\b(zebra|zpl|label|tspl|tsc|gk\d|gx\d|zd\d|zt\d|barcode|sticker|datamax|honeywell\s?pc|brother\s?ql)/i;

/** `'thermal'` (receipt roll), `'label'` (barcode/label) or `'document'` (office laser/inkjet), from a name. */
function classify(name, extra = '') {
  const text = `${name} ${extra}`;
  if (LABEL.test(text)) return 'label';
  if (THERMAL.test(text)) return 'thermal';
  return 'document';
}

/** Parse the output of `lpstat -p`, `lpstat -v` and `lpstat -d` (CUPS: macOS, Linux). */
function parseLpstat({ printers = '', devices = '', defaults = '' }) {
  const uris = new Map();
  for (const line of devices.split('\n')) {
    const m = /^device for (.+?):\s*(.+)$/.exec(line.trim());
    if (m) uris.set(m[1], m[2].trim());
  }
  const def = /destination:\s*(.+)$/.exec(defaults.trim())?.[1]?.trim();
  const out = [];
  for (const line of printers.split('\n')) {
    const m = /^printer\s+(\S+)\s+(.*)$/.exec(line.trim());
    if (!m) continue;
    const [, name, rest] = m;
    const status = /disabled|offline|not accepting/i.test(rest) ? 'offline' : /printing/i.test(rest) ? 'busy' : 'ready';
    const uri = uris.get(name) || '';
    out.push({
      id: `system://${name}`,
      name: name.replace(/_/g, ' '),
      type: classify(name.replace(/_/g, ' '), uri),
      status,
      transport: 'system',
      default: name === def,
      ...(uri ? { uri } : {}),
    });
  }
  return out;
}

/** Parse `Get-Printer | ConvertTo-Json` (Windows): one object or an array. */
function parseWindowsPrinters(json) {
  if (!json || !json.trim()) return [];
  let list;
  try {
    list = JSON.parse(json);
  } catch {
    return [];
  }
  if (!Array.isArray(list)) list = [list];
  return list
    .filter((p) => p && p.Name)
    .map((p) => {
      const status = String(p.PrinterStatus ?? '');
      const offline = /offline|error|notavailable|paperout|^(3|4|5|7|8|9|10|11|12)$/i.test(status) && !/^(0|normal|idle)$/i.test(status);
      return {
        id: `system://${p.Name}`,
        name: p.Name,
        type: classify(p.Name, `${p.DriverName || ''} ${p.PortName || ''}`),
        status: offline ? 'offline' : /printing|^4$/i.test(status) ? 'busy' : 'ready',
        transport: 'system',
        default: Boolean(p.Default),
        ...(p.PortName ? { uri: p.PortName } : {}),
      };
    });
}

function run(exec, cmd, args, opts = {}) {
  return new Promise((resolve, reject) => {
    exec(cmd, args, { timeout: 20_000, windowsHide: true, maxBuffer: 16 * 1024 * 1024, ...opts }, (error, stdout, stderr) => {
      if (error) {
        error.stderr = String(stderr || '');
        return reject(error);
      }
      resolve(String(stdout));
    });
  });
}

/**
 * The printers the operating system knows, plus `extra` (printers you list yourself, such as network ones).
 * @param {{ platform?: string, exec?: Function, extra?: Array<{id: string, name?: string, type?: string}> }} [opts]
 */
async function listPrinters({ platform = process.platform, exec = execFile, extra = [], usb: usbOption, usbModule } = {}) {
  let found = [];
  try {
    if (platform === 'win32') {
      const json = await run(exec, 'powershell.exe', [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        "Get-Printer | Select-Object Name,DriverName,PortName,PrinterStatus,@{n='Default';e={$false}} | ConvertTo-Json -Compress",
      ]);
      found = parseWindowsPrinters(json);
      const def = await run(exec, 'powershell.exe', [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        "(Get-CimInstance Win32_Printer | Where-Object Default).Name",
      ]).catch(() => '');
      for (const p of found) p.default = def.trim() !== '' && p.name === def.trim();
    } else {
      const [printers, devices, defaults] = await Promise.all([
        run(exec, 'lpstat', ['-p']).catch((e) => (e.code === 'ENOENT' ? '' : e.stdout || '')),
        run(exec, 'lpstat', ['-v']).catch(() => ''),
        run(exec, 'lpstat', ['-d']).catch(() => ''),
      ]);
      found = parseLpstat({ printers, devices, defaults });
    }
  } catch {
    found = []; // no spooler, or no permission: the printers you listed yourself still work
  }
  const listed = extra.map((p) => normalizeExtra(p));
  const ids = new Set(listed.map((p) => p.id));
  // USB devices seen directly, when the optional `usb` package is installed (never an error when it is not).
  let direct = [];
  if (usbOption !== false) {
    try {
      direct = (await usbPrinting.listUsbDevices({ usbModule })).map((d) => ({
        id: `usb://${usbPrinting.hex4(d.vendorId)}:${usbPrinting.hex4(d.productId)}`,
        name: d.name || `USB printer ${usbPrinting.hex4(d.vendorId)}:${usbPrinting.hex4(d.productId)}`,
        // Raw commands only make sense for receipt and label printers; an office printer goes through the system.
        type: classify(`${d.manufacturer} ${d.name}`) === 'label' ? 'label' : 'thermal',
        status: 'unknown',
        transport: 'usb',
        default: false,
      }));
    } catch {
      direct = [];
    }
  }
  return [...listed, ...found.filter((p) => !ids.has(p.id)), ...direct.filter((p) => !ids.has(p.id))];
}

function normalizeExtra(p) {
  const { scheme, host, port } = parsePrinterId(p.id);
  return {
    id: p.id,
    name: p.name || (scheme === 'net' ? `${host}:${port}` : p.id),
    type: p.type || (scheme === 'net' ? 'thermal' : classify(p.name || p.id)),
    status: p.status || 'unknown',
    transport: scheme,
    default: false,
  };
}

/** `system://Name` or `net://host:9100` to its parts. Throws a readable error for anything else. */
function parsePrinterId(id) {
  const m = /^(system|net|usb):\/\/(.+)$/.exec(String(id ?? ''));
  if (!m) {
    throw Object.assign(new Error(`printerId must look like system://<printer name>, net://<host>[:<port>] or usb://<vendor>:<product>, not ${JSON.stringify(id)}. Use printers() to list them.`), { code: 'INVALID_FIELD' });
  }
  if (m[1] === 'system') return { scheme: 'system', name: m[2] };
  if (m[1] === 'usb') {
    const ids = /^([0-9a-f]{4}):([0-9a-f]{4})$/i.exec(m[2]);
    if (!ids) throw Object.assign(new Error(`usb printer "${m[2]}" must be <vendor id>:<product id> in hexadecimal, like usb://0483:5740.`), { code: 'INVALID_FIELD' });
    return { scheme: 'usb', vendorId: parseInt(ids[1], 16), productId: parseInt(ids[2], 16) };
  }
  const hp = /^(\[[^\]]+\]|[^:]+)(?::(\d+))?$/.exec(m[2]);
  if (!hp) throw Object.assign(new Error(`net printer address "${m[2]}" is not host[:port].`), { code: 'INVALID_FIELD' });
  return { scheme: 'net', host: hp[1].replace(/^\[|\]$/g, ''), port: hp[2] ? Number(hp[2]) : 9100 };
}

/** Send bytes to a network printer over TCP and wait until they are written. */
function sendNet(host, port, bytes, { timeoutMs = 15_000, connect = net.createConnection } = {}) {
  return new Promise((resolve, reject) => {
    const socket = connect({ host, port });
    const fail = (e) => {
      socket.destroy();
      reject(Object.assign(new Error(`Could not print to ${host}:${port}: ${e.message}`), { code: 'PRINTER_UNREACHABLE' }));
    };
    socket.setTimeout(timeoutMs, () => fail(new Error('timed out')));
    socket.once('error', fail);
    socket.once('connect', () => {
      socket.end(Buffer.from(bytes), () => resolve({ bytes: bytes.length }));
    });
    socket.once('close', () => resolve({ bytes: bytes.length }));
  });
}

async function withTempFile(bytes, ext, use) {
  const dir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pdf-engine-print-'));
  const file = path.join(dir, `job${ext}`);
  try {
    await fs.promises.writeFile(file, bytes);
    return await use(file);
  } finally {
    fs.promises.rm(dir, { recursive: true, force: true }).catch(() => {});
  }
}

/** PowerShell that writes a file to a printer queue as raw bytes (winspool.drv). */
const WINDOWS_RAW = `
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
`;

/** Send raw printer commands (ESC/POS, ZPL) to a system queue. */
async function sendSystemRaw(name, bytes, { platform = process.platform, exec = execFile } = {}) {
  if (platform === 'win32') {
    return withTempFile(bytes, '.bin', async (file) => {
      const script = path.join(path.dirname(file), 'raw.ps1');
      await fs.promises.writeFile(script, WINDOWS_RAW);
      await run(exec, 'powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, name, file], { timeout: 60_000 });
      return { bytes: bytes.length };
    });
  }
  return withTempFile(bytes, '.bin', async (file) => {
    await run(exec, 'lp', ['-d', name, '-o', 'raw', file], { timeout: 60_000 });
    return { bytes: bytes.length };
  });
}

/** Print a PDF on an office printer through the operating system. */
async function sendSystemPdf(name, pdf, { copies = 1, platform = process.platform, exec = execFile } = {}) {
  return withTempFile(pdf, '.pdf', async (file) => {
    if (platform === 'win32') {
      // SumatraPDF prints silently when it is installed; otherwise the default PDF app's "print to" verb.
      const sumatra = await run(exec, 'where.exe', ['SumatraPDF.exe']).then((s) => s.split(/\r?\n/)[0].trim(), () => '');
      for (let i = 0; i < copies; i++) {
        if (sumatra) await run(exec, sumatra, ['-print-to', name, '-silent', file], { timeout: 120_000 });
        else await run(exec, 'powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `Start-Process -FilePath '${file.replace(/'/g, "''")}' -Verb PrintTo -ArgumentList '"${name.replace(/'/g, "''")}"' -WindowStyle Hidden -Wait`], { timeout: 120_000 });
      }
    } else {
      await run(exec, 'lp', ['-d', name, '-n', String(copies), file], { timeout: 60_000 });
    }
    return { bytes: pdf.length };
  });
}

module.exports = {
  DPI,
  PAPER_DOTS,
  classify,
  listPrinters,
  parseLpstat,
  parsePrinterId,
  parseWindowsPrinters,
  sendNet,
  sendSystemPdf,
  sendUsb: usbPrinting.sendUsb,
  sendSystemRaw,
};
