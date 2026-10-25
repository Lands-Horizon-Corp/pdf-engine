# pdf-engine in Electron

Make PDFs and print on receipt, label and office printers from an Electron app, with
`@lands-horizon-corp/pdf-engine`. The Rust engine runs as a child process of your **main process** and talks to
it over stdin and stdout: no HTTP server, no open port, no network, and it works offline.

> **Status:** the Node client this uses is tested against the real engine on Linux and macOS in CI (and on
> Windows, in the job that also prints to a virtual printer). It has not been run inside an Electron app in this
> repository's tests, and printing has not been tried on physical printers. Treat the packaging steps as a guide
> and check them in your own build. The package is not on npm yet; see [`PUBLISHING.md`](../../PUBLISHING.md).

## 1. Install

```sh
npm install @lands-horizon-corp/pdf-engine
```

The package needs the engine binary for each platform you ship. It looks for it, in order, at:

1. `new PDFEngine({ binary: '/path/to/pdf-engine' })`
2. the `PDF_ENGINE_BIN` environment variable
3. `bin/<platform>-<arch>/pdf-engine` inside the package (`bin/win32-x64/pdf-engine.exe` on Windows), using the
   unpacked copy when the app is packed into an `.asar`
4. `<resources>/pdf-engine/<platform>-<arch>/pdf-engine` and `<resources>/bin/pdf-engine` (electron-builder
   `extraResources`)

To build it for the machine you are on: `npm run build:native` in the package (it runs `cargo build --release`
and copies the binary into `bin/`). Build on each target operating system, or cross-compile.

## 2. The main process owns the engine

Never create the engine in a renderer: it starts a process. Keep one in the main process and expose a small API.

```ts
// src/main/pdf.ts
import { app, ipcMain } from 'electron';
import { PDFEngine, PDFEngineError } from '@lands-horizon-corp/pdf-engine';

const pdf = new PDFEngine({
  // Optional extra fonts (the system fonts are always used). Ship them as extraResources.
  // fonts: [path.join(process.resourcesPath, 'fonts')],
  // Network receipt printers cannot be discovered; list them here.
  printers: [{ id: 'net://192.168.1.150:9100', name: 'Kitchen Epson TM-T82', type: 'thermal' }],
});

const fail = (e: unknown) => {
  // Errors cross IPC as plain data: keep the code, the message and the hint.
  if (e instanceof PDFEngineError) return { code: e.code, message: e.message, hint: e.hint, line: e.line };
  return { code: 'INTERNAL_ERROR', message: String(e) };
};

export function registerPdfHandlers() {
  ipcMain.handle('pdf:generate', async (_event, options) => {
    try {
      return { ok: true as const, bytes: await pdf.generate(options) };   // a Buffer arrives as a Uint8Array
    } catch (e) {
      return { ok: false as const, error: fail(e) };
    }
  });

  ipcMain.handle('pdf:printers', async () => pdf.printers());

  ipcMain.handle('pdf:print', async (_event, options) => {
    try {
      return { ok: true as const, result: await pdf.print(options) };
    } catch (e) {
      return { ok: false as const, error: fail(e) };
    }
  });

  app.on('before-quit', () => void pdf.close());
}
```

```ts
// src/main/index.ts
import { app, BrowserWindow } from 'electron';
import path from 'node:path';
import { registerPdfHandlers } from './pdf';

app.whenReady().then(() => {
  registerPdfHandlers();
  new BrowserWindow({
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,   // keep these two as they are
      nodeIntegration: false,
    },
  }).loadFile('index.html');
});
```

## 3. Expose it to the page with a preload script

```ts
// src/main/preload.ts
import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('pdf', {
  generate: (options: unknown) => ipcRenderer.invoke('pdf:generate', options),
  printers: () => ipcRenderer.invoke('pdf:printers'),
  print: (options: unknown) => ipcRenderer.invoke('pdf:print', options),
});
```

```ts
// renderer (any framework)
const res = await window.pdf.generate({
  template: '<h1>Invoice {{ no }}</h1>',
  data: { no: '12345' },
});
if (!res.ok) throw new Error(`${res.error.code}: ${res.error.message}`);
const url = URL.createObjectURL(new Blob([res.bytes], { type: 'application/pdf' }));
window.open(url);          // or <iframe src={url}> or an <a download> link
```

## 4. Print

```ts
const printers = await window.pdf.printers();
// [{ id: 'system://POS-58', name: 'POS-58', type: 'thermal', status: 'ready', transport: 'system', ... },
//  { id: 'net://192.168.1.150:9100', ... }, { id: 'system://HP_LaserJet_P1102', type: 'document', ... }]

await window.pdf.print({
  printerId: 'system://POS-58',
  paper: '58mm',                      // '58mm', '80mm' (default) or '112mm'
  template: `<b>STORE</b>{% for i in items %}<div>{{ i.name }} ₱{{ i.price }}</div>{% endfor %}<b>TOTAL ₱{{ total }}</b>`,
  data: { items: [{ name: 'Sardines', price: 24.5 }], total: 24.5 },
});
```

- Receipt printers get the receipt as ESC/POS commands with a paper cut; label printers get ZPL; office printers
  get the PDF. `type` is guessed from the printer's name; pass `type: 'thermal' | 'label' | 'document'` to force it.
- `system://<name>` uses the operating system's queue (works for USB and Bluetooth printers once their driver is
  installed). `net://host:9100` writes straight to a network printer. `usb://0483:5740` talks to a USB device
  directly (see [`.node/desktop/README.md`](../../.node/desktop/README.md#printing-electron-tauri-node)).
- Errors: `PRINTER_UNREACHABLE` (a network printer did not answer), `PRINT_FAILED` (the queue refused the job).

## 5. Package the app

With electron-builder, ship the binary unpacked, outside the `.asar` (an executable cannot run from inside it):

```jsonc
// electron-builder config (package.json "build", or electron-builder.yml)
{
  // Option 1: the package's own bin/ folder, unpacked from the asar
  "asarUnpack": ["node_modules/@lands-horizon-corp/pdf-engine/bin/**"],

  // Option 2: copy each platform's binary into the app's resources (folder names are <platform>-<arch>)
  "win": { "extraResources": [{ "from": "engine/win32-x64", "to": "pdf-engine/win32-x64" }] },
  "mac": { "extraResources": [{ "from": "engine/darwin-arm64", "to": "pdf-engine/darwin-arm64" }] },
  "linux": { "extraResources": [{ "from": "engine/linux-x64", "to": "pdf-engine/linux-x64" }] }
}
```

Check on a **packaged** build (not only `npm start`) that `await pdf.ping()` returns a version. If it fails
with `BINARY_NOT_FOUND`, the error lists every path it tried.

## Windows notes

- Binary name: `pdf-engine.exe`, found as `bin/win32-x64/pdf-engine.exe`. No console window opens.
- Antivirus software and code signing: sign the engine together with your app, or some scanners flag a new
  unsigned `.exe` that is started by another program.
- Printing goes through the Windows spooler. A receipt printer should use a "Generic / Text Only" driver (or the
  maker's raw driver) so the commands are sent unchanged; a PDF for an office printer needs SumatraPDF on the
  `PATH` for silent printing, otherwise the default PDF app's print command is used.
- Direct USB (`usb://`) needs a WinUSB driver for the device (Zadig), which stops Windows listing it as a printer.
  Most Windows apps should use `system://` instead.

## Things to know

- **One engine, many calls:** requests can run in parallel; the engine uses all cores.
- **Closing:** `pdf.close()` finishes work in progress and stops the process; it also stops if the app exits.
- **Template language:** `{{ value }}`, `{% for x in list %}` and `{% if %}`. Values are HTML-escaped.
- More: [`.node/desktop/README.md`](../../.node/desktop/README.md).
