# pdf-engine in Tauri

Two ways to make PDFs in a Tauri app. Pick the first unless you need the native engine's speed on big batches.

| | A. WebAssembly in the webview | B. Native engine as a sidecar |
| --- | --- | --- |
| How | `@lands-horizon-corp/pdf-engine-wasm` in your frontend | the `pdf-engine` binary, started by Tauri, called over `127.0.0.1` |
| Setup | `npm install` | add the binary as an `externalBin`, start it from Rust |
| Speed | about 1 ms per receipt, one at a time | about 1 ms per receipt, and thousands per second in parallel (uses all cores) |
| Fonts | bundled Latin/Cyrillic/Greek, add others | all the system's fonts |

> **Status:** written from the packages' APIs and Tauri v2's documented sidecar and shell plugin. The repository
> tests the browser package in Chromium, Firefox and WebKit; Tauri's webviews (WebView2, WKWebView, WebKitGTK)
> are the same engines as Edge/Chromium, Safari and the GTK WebKit, but a Tauri app is not part of the tests yet.
> Run each step in your app before relying on it.
>
> **Why not the Node client?** `@lands-horizon-corp/pdf-engine` starts the engine with Node's `child_process`,
> which a Tauri webview does not have. Use option A or B below for PDFs, and the sidecar's command line for
> [printing](#printing-receipt-label-and-office-printers). Printing has not been tried on physical printers yet.

## A. WebAssembly in the webview

Exactly the browser package; use the guide for your frontend ([React](sample-react.md), [Vue](sample-vue.md),
[Svelte](sample-svelte.md)). In short:

```ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

const pdf = new PDFEngine();
const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: '12345' } });
```

Save or open the result with Tauri's plugins instead of a browser download (a webview may not show the download):

```ts
import { save } from '@tauri-apps/plugin-dialog';
import { writeFile } from '@tauri-apps/plugin-fs';

const path = await save({ defaultPath: 'invoice.pdf', filters: [{ name: 'PDF', extensions: ['pdf'] }] });
if (path) await writeFile(path, bytes);
```

(Add the `dialog` and `fs` plugins and their permissions in `src-tauri/capabilities/default.json`.)

The webview loads the `.wasm` file from your app's own files, so no extra network permission is needed. If your
Content-Security-Policy is strict, allow WebAssembly: `script-src 'self' 'wasm-unsafe-eval'`.

## B. Native engine as a sidecar

The engine runs as a separate program that Tauri starts, listening only on this computer, and your frontend
renders through the browser package's `url` option.

### 1. Put the binary where Tauri expects it

Tauri sidecars are named with the Rust target triple. Build the engine and copy it:

```sh
cargo build --release                       # in the pdf-engine repository
# src-tauri/binaries/pdf-engine-aarch64-apple-darwin          (Apple Silicon)
# src-tauri/binaries/pdf-engine-x86_64-apple-darwin
# src-tauri/binaries/pdf-engine-x86_64-pc-windows-msvc.exe
# src-tauri/binaries/pdf-engine-x86_64-unknown-linux-gnu
```

```jsonc
// src-tauri/tauri.conf.json
{
  "bundle": { "externalBin": ["binaries/pdf-engine"] },
  "app": {
    "security": {
      "csp": "default-src 'self'; connect-src 'self' http://127.0.0.1:*; img-src 'self' data: blob:"
    }
  }
}
```

### 2. Start it from Rust, on a free port, with a random token

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-shell = "2"
rand = "0.8"
```

```rust
// src-tauri/src/lib.rs
use std::net::TcpListener;
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

struct PdfServer {
    url: String,
    token: String,
}

#[tauri::command]
fn pdf_server(state: tauri::State<PdfServer>) -> (String, String) {
    (state.url.clone(), state.token.clone())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // A free port on this computer only.
            let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
            let token: String = (0..32).map(|_| format!("{:x}", rand::random::<u8>() % 16)).collect();

            let (_events, _child) = app
                .shell()
                .sidecar("pdf-engine")?
                .env("API_HOST", "127.0.0.1")                // only this computer can connect
                .env("API_PORT", port.to_string())
                .env("API_BEARER_TOKEN", &token)
                // The webview's origin: tauri://localhost on macOS and Linux, http://tauri.localhost on
                // Windows, and your dev server while developing.
                .env("CORS_ALLOWED_ORIGINS", "tauri://localhost,http://tauri.localhost,http://localhost:1420")
                .spawn()?;
            // Keep `_child` in app state and call `.kill()` when the app exits, so the engine never outlives it.

            app.manage(PdfServer { url: format!("http://127.0.0.1:{port}"), token });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![pdf_server])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
```

```jsonc
// src-tauri/capabilities/default.json: allow the app to start the sidecar
{
  "permissions": [
    "core:default",
    { "identifier": "shell:allow-spawn", "allow": [{ "name": "binaries/pdf-engine", "sidecar": true }] }
  ]
}
```

### 3. Use it from the frontend

```ts
import { invoke } from '@tauri-apps/api/core';
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

const [url, token] = await invoke<[string, string]>('pdf_server');

// pick: 'auto' renders on the sidecar, and in the webview (WebAssembly) if the sidecar is not reachable.
export const pdf = new PDFEngine({ url, token, pick: 'auto' });

const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: '12345' } });
```

The engine can take a few hundred milliseconds to start. Call `await pdf.generate(...)` after the app window is
ready; with `pick: 'auto'` an early call falls back to the page instead of failing.

## Printing: receipt, label and office printers

Printing is done by the engine itself, so Tauri gets the same printing as Electron by running the sidecar's
command line (no Node, no extra package, and no HTTP server needed for this). Any printer works: receipt
printers get ESC/POS, label printers ZPL, office printers the PDF, through the system's print queue
(`system://`), a network socket (`net://host:9100`) or a USB device directly (`usb://0483:5740`).

Allow the sidecar to run with arguments in `capabilities/default.json`:

```jsonc
{
  "permissions": [
    "core:default",
    {
      "identifier": "shell:allow-execute",
      "allow": [{ "name": "binaries/pdf-engine", "sidecar": true, "args": true }]
    }
  ]
}
```

Then, from the frontend:

```ts
import { Command } from '@tauri-apps/plugin-shell';

interface PrintResult {
  ok: boolean;
  error?: { code: string; error: string; hint?: string };
  [key: string]: unknown;
}

async function run(command: 'printers' | 'print', request: object): Promise<PrintResult> {
  // The answer is one line of JSON on standard output; the exit code is 0 when ok.
  const out = await Command.sidecar('binaries/pdf-engine', [command, '--json', JSON.stringify(request)]).execute();
  const answer = JSON.parse(out.stdout.trim().split('\n').pop() ?? '{}') as PrintResult;
  if (!answer.ok) throw new Error(`${answer.error?.code}: ${answer.error?.error}`);
  return answer;
}

// 1. The printers on this computer, and network printers you name.
const { printers } = (await run('printers', {
  printers: [{ id: 'net://192.168.1.150:9100', name: 'Kitchen Epson TM-T82' }],
})) as unknown as { printers: { id: string; name: string; type: string; status: string }[] };

// 2. Print a receipt.
await run('print', {
  printerId: printers[0].id,
  paper: '58mm',                       // '58mm', '80mm' (default), '112mm' or dots
  template: '<b>STORE</b>{% for i in items %}<div>{{ i.name }} ₱{{ i.price }}</div>{% endfor %}<b>TOTAL ₱{{ total }}</b>',
  data: { items: [{ name: 'Sardines', price: 24.5 }], total: 24.5 },
});
```

- The request has the same fields as `print()` in the [Node package](../../.node/desktop/README.md#printing-electron-tauri-node):
  `printerId`, `template`, `data`, `paper`, `type`, `language`, `cut`, `feed`, `dither`, `copies`, `width`, `height`.
- `--json '<request>'` suits receipts. A Windows command line holds about 32,000 characters; for bigger data write the
  request to a file and use `--request <file>`.
- Images in a template must be `data:` URIs here (or `https://` hosts you allow with the engine's
  `ASSET_ALLOWED_HOSTS`). The sidecar needs no HTTP server for printing; option B's server is only for PDFs.
- Errors come back with a code: `PRINTER_UNREACHABLE` (a network printer did not answer), `PRINT_FAILED` (the
  queue or the USB device refused the job), `INVALID_FIELD`, `MISSING_FIELD`, `TEMPLATE_ERROR`.
- Windows: a receipt printer should use a "Generic / Text Only" driver so the commands are sent unchanged; direct
  `usb://` needs a WinUSB driver (Zadig), which most Windows apps should skip in favour of `system://`.

## Things to know

- **The token never leaves the machine,** but it is in the webview's memory. That is fine for a local sidecar
  bound to `127.0.0.1`; never ship a fixed token.
- **Windows:** the sidecar is `pdf-engine-x86_64-pc-windows-msvc.exe`. Windows Firewall may ask once about a
  program that listens; binding to `127.0.0.1` keeps it local, and the prompt can be declined.
- **Fonts** (sidecar): the engine uses the operating system's fonts. In the webview (option A), add fonts with
  `new PDFEngine({ fonts: [...] })`.
- **Template language:** `{{ value }}`, `{% for x in list %}` and `{% if %}`. Values are HTML-escaped.
- More options: [`.node/wasm/README.md`](../../.node/wasm/README.md). The server's settings are in the
  [main README](../../README.md#http-and-docker).
