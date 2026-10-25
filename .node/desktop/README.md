# pdf-engine for Node.js and Electron

HTML to PDF. The Rust engine runs as a child process and talks to your code over stdin/stdout, so
there is **no HTTP server, no port and no network call**; the PDF comes back as raw bytes.

```ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine';

const pdf = new PDFEngine({ fonts: ['/path/to/Inter.ttf'] }); // fonts are optional

const bytes = await pdf.generate({
  template: '<h1>Invoice {{ no }}</h1>',
  data: { no: '12345' },
  password: '', // empty = not encrypted
});
await fs.promises.writeFile('invoice.pdf', bytes);

await pdf.close(); // when you are done (the engine also stops if your process exits)
```

The engine starts on the first `generate()` and is reused. Calls can run in parallel.

## Options

| `new PDFEngine({ ... })` | |
| --- | --- |
| `fonts` | Font files (`.ttf`, `.otf`, `.ttc`) or folders, added to the system fonts. |
| `binary` | Path to the `pdf-engine` executable (found automatically in a normal install). |
| `timeoutMs` | Give up waiting for one answer after this long (default 60000). |
| `imageDir` | A folder templates may load images from: `<img src="logo.png">` or a `file:` URL inside it. Nothing outside it (`..`, symlinks) is read. Off by default. |
| `imageHosts` | Hosts `<img src="https://...">` may be fetched from, e.g. `['cdn.example.com', '*.example.com']`. Off by default. |
| `env` | Extra environment for the engine, e.g. `ASSET_ALLOWED_HOSTS` for remote images. |
| `onLog` | Receives the engine's log output. |

| `generate({ ... })` | |
| --- | --- |
| `template` | HTML with `{{ value }}` and `{% for x in list %}...{% endfor %}` (minijinja, HTML-escaped). |
| `data` | Values for the template. |
| `password` | Encrypts the PDF. |
| `width`, `height` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default US Letter). |
| `orientation` | `portrait` (default) or `landscape`. |
| `images` | Images for `<img src="name">`, by name: a Buffer/Uint8Array, a `data:` URI or a file path. Skipped, not an error, if unusable. |

Also: `ping()`, `loadFonts(paths)`, `reloadFonts()`, `close()`.

## Printing (Electron, Tauri, Node)

Find printers and print straight to them: receipt (thermal) printers, label printers and office printers.

```js
const pdf = new PDFEngine({
  printers: [{ id: 'net://192.168.1.150:9100', name: 'Kitchen Epson TM-T82' }],   // network printers: list them
});

const printers = await pdf.printers();
// [{ id: 'system://POS-58', name: 'POS-58', type: 'thermal', status: 'ready', transport: 'system', default: false },
//  { id: 'net://192.168.1.150:9100', name: 'Kitchen Epson TM-T82', type: 'thermal', ... },
//  { id: 'system://HP_LaserJet_P1102', type: 'document', ... }]

await pdf.print({
  printerId: 'system://POS-58',
  paper: '58mm',                                   // '58mm', '80mm' (default), '112mm', or dots
  template: '<b>STORE</b>{% for i in items %}<div>{{ i.name }} ₱{{ i.price }}</div>{% endfor %}<b>TOTAL ₱{{ total }}</b>',
  data: { items: [{ name: 'Sardines', price: 24.5 }], total: 24.5 },
});
```

- **What each printer is sent.** `thermal`: the receipt as a picture in ESC/POS commands (so any CSS, font or
  script prints, including ₱ and non-Latin text), then a feed and a paper cut. `label`: the same picture as a
  ZPL graphic. `document`: the PDF. The type is guessed from the printer's name; set `type` to override it.
- **How it reaches the printer.** `system://<name>` goes through the operating system's queue (CUPS `lp` on
  macOS and Linux, the Windows spooler), so any printer with its driver installed works, USB and Bluetooth
  ones included. `net://host[:port]` writes straight to a network printer on port 9100.
- **Options.** `cut`, `feed` (blank lines before the cut), `dither` (for photos and logos), `copies`,
  `language` (`escpos` or `zpl`), and everything `generate()` takes except `password`.
- **USB.** The simple way is the operating-system queue: add the printer (the "Generic / Text Only" driver on
  Windows, a raw queue on macOS and Linux) and use its `system://` id. To talk to the USB device directly, by
  vendor and product id (`usb://0483:5740`), install the optional [`usb`](https://www.npmjs.com/package/usb)
  package (`npm install usb`); `printers()` then lists receipt and label printers it finds. Windows needs a
  WinUSB driver for that device (Zadig), which stops it appearing as a normal printer; Linux needs permission
  on the device (a udev rule). Direct USB is for receipt and label printers; PDFs go through `system://`.
- **Not a web feature.** A web page cannot see printers; the browser package's `print()` opens the browser's
  print dialog instead.

Printing needs the operating system's tools to be present (`lp` and `lpstat` on macOS and Linux, PowerShell
on Windows). Errors are `PRINTER_UNREACHABLE` (a network printer did not answer) and `PRINT_FAILED` (the
queue refused the job); both carry the system's message.

## Errors

Every failure is a `PDFEngineError` with a stable `code` to branch on, a readable `message`, and
where it helps a `hint`, `status` and (templates) the `line`:

```ts
try {
  await pdf.generate({ template: '<p>\n{% if %}' });
} catch (e) {
  if (e instanceof PDFEngineError && e.code === 'TEMPLATE_ERROR') {
    console.log(e.message, 'line', e.line, e.hint);
  }
}
```

Codes: `MISSING_FIELD`, `INVALID_FIELD`, `TEMPLATE_ERROR`, `TEMPLATE_OUTPUT_TOO_LARGE`,
`TEMPLATE_TOO_EXPENSIVE`, `FONT_ERROR`, `RENDER_FAILED`, `PDF_PROCESSING_FAILED`, `TIMEOUT`,
`INTERNAL_ERROR`, `IPC_PROTOCOL_ERROR`, and from the client `BINARY_NOT_FOUND`, `ENGINE_EXITED`,
`ENGINE_CLOSED`, `CLIENT_TIMEOUT`, `PROTOCOL_MISMATCH`. The full list is typed in `index.d.ts`.

## Install the binary

```sh
npm run build:native            # builds the Rust engine, copies it to bin/<platform>-<arch>/
npm run build:native -- --debug # faster to compile
```

Or point at a binary with `new PDFEngine({ binary })` or `PDF_ENGINE_BIN`. macOS and Linux only for
now (the IPC mode redirects file descriptors; Windows is not supported yet).

## Electron

Run the engine in the **main process** and let the UI (React, Vue, Svelte, anything) ask for PDFs
over Electron's IPC. The UI framework does not matter, and no WebAssembly is involved.

```ts
// main.ts
import { app, ipcMain } from 'electron';
import { PDFEngine } from '@lands-horizon-corp/pdf-engine';

const pdf = new PDFEngine();
ipcMain.handle('pdf:generate', (_event, options) => pdf.generate(options)); // returns a Buffer
app.on('before-quit', () => pdf.close());

// preload.ts
import { contextBridge, ipcRenderer } from 'electron';
contextBridge.exposeInMainWorld('pdf', {
  generate: (options) => ipcRenderer.invoke('pdf:generate', options),
});

// in your React/Vue component
const bytes: Uint8Array = await window.pdf.generate({ template, data });
const url = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));
```

Packaging with electron-builder: the executable cannot run from inside `app.asar`, so unpack it:

```json
{
  "build": {
    "asarUnpack": ["node_modules/@lands-horizon-corp/pdf-engine/bin/**"]
  }
}
```

(or ship it with `extraResources` as `pdf-engine/<platform>-<arch>/pdf-engine`; both places are searched).

## Browsers

A web page cannot start a program, so it cannot use this package directly. Call a backend that
uses `PDFEngine`, or run the engine's HTTP mode (`pdf-engine`, see the root README).

## How the Rust engine and this package stay in sync

`src/ipc.rs` defines the wire protocol, `index.d.ts` mirrors its error codes, and `ping` reports a
protocol version that the client checks at startup. `cargo test --test node_contract` fails if the
codes, protocol version or package version drift apart.
