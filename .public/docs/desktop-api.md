# Node and Electron package reference

`@lands-horizon-corp/pdf-engine`: the native Rust engine, run as a child process of your program. It talks to it over
stdin and stdout, so there is no HTTP server, no port, no network and no token, and it works offline. It makes PDFs and
prints them. Use it in the **main process** of Electron, in Node scripts and servers, and in any Node-based tool.

> Not on npm yet: it needs the engine binary for every platform first ([PUBLISHING.md](../../PUBLISHING.md)).
> In a Tauri app use the [sidecar](sample-tauri.md) instead; a web page uses the [browser package](browser-api.md).

```ts
import { PDFEngine, PDFEngineError } from '@lands-horizon-corp/pdf-engine';

const pdf = new PDFEngine();
const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: '12345' } });   // a Buffer
await pdf.close();
```

The engine starts on the first call and is reused; calls can run in parallel and use every core. It is restarted
automatically if it dies. The process also stops when your program exits.

## `new PDFEngine(options?)`

An array is shorthand for `{ fonts: [...] }`.

| Option | Type | Meaning |
| --- | --- | --- |
| `fonts` | `string[]` | Font files (`.ttf`, `.otf`, `.ttc`) or folders, added to the system fonts. [Fonts](fonts-and-languages.md) |
| `binary` | `string` | Path to the `pdf-engine` executable. Found automatically in a normal install. |
| `timeoutMs` | `number` (`60000`) | Give up waiting for one answer after this long (`0` = never). |
| `imageDir` | `string` | A folder templates may load images from: `src="logo.png"` or a `file:` URL inside it. Nothing outside it is read. Off by default. |
| `imageHosts` | `string[]` | Hosts `<img src="https://...">` may be fetched from (`cdn.example.com`, `*.example.com`). Off by default. |
| `printers` | `{ id, name?, type? }[]` | Printers to offer besides the system's, such as network printers. [Printing](printing.md) |
| `env` | `Record<string, string>` | Extra environment for the engine (`RENDER_TIMEOUT_SECS`, `ASSET_ALLOWED_HOSTS`, ...). [Settings](http-api.md#settings) |
| `onLog` | `(text) => void` | Receives the engine's log output (stderr). |

### Where the binary is looked for

In order: the `binary` option, the `PDF_ENGINE_BIN` environment variable, `bin/<platform>-<arch>/pdf-engine` inside the
package (the copy outside the `.asar` in a packed Electron app), then `<resources>/pdf-engine/<platform>-<arch>/` and
`<resources>/bin/` (electron-builder `extraResources`), then `PATH`. Folder names are `darwin-arm64`, `darwin-x64`,
`linux-x64`, `win32-x64`, ... If none exists you get `BINARY_NOT_FOUND`, listing every path tried. To build it for this
machine: `npm run build:native` in the package. [Packaging for Electron](sample-electron.md#5-package-the-app).

## `generate(options): Promise<Buffer>`

| Option | Type | Meaning |
| --- | --- | --- |
| `template` | `string` (required) | HTML with `{{ }}` and `{% %}`. [Templates](templates.md) |
| `data` | any JSON value | Values for the template. `generate<MyData>()` checks its shape. |
| `password` | `string` | Encrypts the PDF. |
| `width`, `height` | `string` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px`. Default Letter. |
| `orientation` | `'portrait' \| 'landscape'` | |
| `images` | `Record<string, Buffer \| Uint8Array \| ArrayBuffer \| string>` | Images by their `src` name: bytes, a `data:` URI or a **file path** (read by your program, so `imageDir` does not limit it). Sent to the engine with the request. |

```ts
await fs.writeFile('invoice.pdf', await pdf.generate({
  template: '<img src="logo" width="120"><h1>{{ no }}</h1>',
  data: { no: '12345' },
  images: { logo: await fs.readFile('logo.png') },
  width: '210mm', height: '297mm',
}));
```

An image that cannot be used is skipped; the document still renders.

## Printing

| Method | Does |
| --- | --- |
| `printers(): Promise<Printer[]>` | Lists the printers: the system's, the `printers` you passed, and USB receipt/label printers seen directly. |
| `print(options): Promise<PrintResult>` | Renders a template and prints it: ESC/POS on a receipt printer, ZPL on a label printer, the PDF on an office printer. |

Everything about ids, paper sizes, options, USB and Windows is in [Printing](printing.md).

## Other methods and properties

| | Does |
| --- | --- |
| `ping(): Promise<{ version, protocol }>` | Starts the engine now and reports its version and wire protocol. |
| `loadFonts(paths)` | Use these font files or folders (besides the system fonts) from now on. |
| `reloadFonts()` | Pick up fonts installed on or removed from the system while running. |
| `close(): Promise<void>` | Finishes requests in progress, then stops the engine. New calls then throw `ENGINE_CLOSED`. |
| `pid: number \| null` | The engine's process id while it runs. |

Exports: `PDFEngine`, `PDFEngineError`, `PROTOCOL_VERSION`.

## Errors

```ts
try {
  await pdf.generate({ template: '<p>\n{% if %}' });
} catch (e) {
  if (e instanceof PDFEngineError) console.log(e.code, e.status, e.line, e.hint, e.message);
}
```

Engine errors keep the engine's `code`, `status`, `hint` and `line` ([Errors](errors.md)). Client-side codes:

| Code | Meaning |
| --- | --- |
| `BINARY_NOT_FOUND` | No engine executable at any of the places above. |
| `ENGINE_START_FAILED` | The process could not be started (permissions, blocked by security software). |
| `ENGINE_EXITED` | The engine stopped while a request was in progress; the message has its exit code and last log lines. The next call starts a new one. |
| `ENGINE_CLOSED` | You called a method after `close()`. |
| `CLIENT_TIMEOUT` | No answer within `timeoutMs`. |
| `PROTOCOL_MISMATCH` | The binary and this package speak different protocol versions: use matching versions. |

## In Electron

Keep the engine in the main process, expose a few functions with `contextBridge`, and never create it in a renderer.
The [Electron guide](sample-electron.md) has the complete main, preload and renderer code, packaging and Windows notes.
