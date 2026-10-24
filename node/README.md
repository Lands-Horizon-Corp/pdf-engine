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
| `env` | Extra environment for the engine, e.g. `ASSET_ALLOWED_HOSTS` for remote images. |
| `onLog` | Receives the engine's log output. |

| `generate({ ... })` | |
| --- | --- |
| `template` | HTML with `{{ value }}` and `{% for x in list %}...{% endfor %}` (minijinja, HTML-escaped). |
| `data` | Values for the template. |
| `password` | Encrypts the PDF. |
| `width`, `height` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default US Letter). |
| `orientation` | `portrait` (default) or `landscape`. |

Also: `ping()`, `loadFonts(paths)`, `reloadFonts()`, `close()`.

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
