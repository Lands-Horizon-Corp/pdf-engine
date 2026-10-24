# pdf-engine

**HTML to real PDFs, from one Rust renderer.** Use it in the browser (WebAssembly), in Node, Electron
and Tauri, or as an HTTP service. No headless Chrome.

```ts
const pdf = new PDFEngine();
const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: '12345' } });
```

## Why pdf-engine

- **Real PDFs, not screenshots.** Vector text you can select, search and copy, in tens of KB. Client-side
  libraries built on html2canvas turn the page into an image first.
- **No browser to ship.** No Puppeteer or Chromium (hundreds of MB), no wkhtmltopdf binary.
- **One engine everywhere.** Browser, desktop app and server give the same output, with the same API and
  the same error codes.
- **Templates built in.** `{{ value }}`, loops and HTML-escaping; no second library.
- **Fast and small.** About 1–2 ms per document natively and about 4 ms in the browser; 2.4 MB of
  WebAssembly (brotli).<sup>1</sup>
- **Every language.** About 150 Noto fonts on the server cover practically every living script; in the
  browser you add the font you need.
- **Errors you can act on.** A stable `code`, the template `line` and a `hint`.

<sub>1. Measured on an Apple Silicon Mac, warm, for receipt- and invoice-sized documents.</sub>

## Install

| Where | Package | |
| --- | --- | --- |
| **Browser**: React, Vue, Svelte, plain JS | [`@lands-horizon-corp/pdf-engine-wasm`](wasm/README.md) | In-page WebAssembly, no server |
| **Node, Electron, Tauri** | [`@lands-horizon-corp/pdf-engine`](node/README.md) | Rust engine over stdin/stdout, no port |
| **HTTP / Docker** | `zalven88/pdf-engine` | `POST /api/to-bytes` ([details](#http-and-docker)) |

```tsx
// Browser
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';
const pdf = new PDFEngine({ fonts: ['/fonts/NotoSansSC-Regular.otf'] }); // fonts are optional
const bytes = await pdf.generate({ template, data, password: '' });
```

```ts
// Node / Electron main process
import { PDFEngine } from '@lands-horizon-corp/pdf-engine';
const pdf = new PDFEngine({ fonts: ['/path/to/Inter.ttf'] });
await fs.promises.writeFile('invoice.pdf', await pdf.generate({ template, data }));
```

Both packages have the same `generate({ template, data, password, width, height, orientation })`.

## How it compares

| Package | Weekly downloads | Last release | Runs in | How it makes the PDF | License |
| --- | ---: | --- | --- | --- | --- |
| [html2pdf.js](https://www.npmjs.com/package/html2pdf.js) | 4.8M | 9 months ago | Browser | Draws the page to an image (html2canvas), then jsPDF | MIT |
| [@adobe/pdfservices-node-sdk](https://www.npmjs.com/package/@adobe/pdfservices-node-sdk) | 211K | 2 years ago | Node | Adobe's hosted service (needs an account and network) | Adobe terms |
| [jspdf-html2canvas](https://www.npmjs.com/package/jspdf-html2canvas) | 116K | 3 months ago | Browser | jsPDF + html2canvas | ISC |
| [pdf-creator-node](https://www.npmjs.com/package/pdf-creator-node) | 54K | 7 months ago | Node | Node library | MIT |
| [vue-html2pdf](https://www.npmjs.com/package/vue-html2pdf) / [vue3-html2pdf](https://www.npmjs.com/package/vue3-html2pdf) | 44K / 11K | 6 / 5 years ago | Vue | Wrappers around html2pdf.js | MIT |
| [html2pdf](https://www.npmjs.com/package/html2pdf) | 42K | 13 years ago | Node | Needs the wkhtmltopdf binary | n/a |
| [html-export-pdf-cli](https://www.npmjs.com/package/html-export-pdf-cli) | 12K | 2 years ago | Node CLI | Puppeteer (headless Chromium) | MIT |
| [html3pdf](https://www.npmjs.com/package/html3pdf) | 9K | 3 years ago | Browser | Fork of html2pdf.js | MIT |
| [convert-html-to-pdf](https://www.npmjs.com/package/convert-html-to-pdf) | 5K | 7 years ago | Node | Node library | MIT |
| [pdf-node](https://www.npmjs.com/package/pdf-node) | 3K | a year ago | Node | Handlebars templates to PDF | MIT |
| **pdf-engine** | new | n/a | **Browser, Node, Electron, Tauri, HTTP** | **Rust renderer: vector text, same output everywhere** | MIT |

<sub>Figures from an npm search on 2026-10-24; "how" is taken from each package's npm description.
Downloads show adoption, not quality: html2pdf.js is the established choice for in-browser snapshots.
pdf-engine is new and has no track record yet.</sub>

## Compatibility

| | Status |
| --- | --- |
| macOS: native engine, Node client, browser package (Chrome) | Tested |
| Linux: native engine and fonts (Docker) | Tested |
| Electron and Tauri apps on macOS, Linux and Windows | Planned |
| Windows: native engine (IPC mode) | Not supported yet |
| Browsers: Safari, Firefox, Brave, Edge, mobile | Planned |

## CSS

Flexbox, grid, tables, gradients, transforms, shadows, SVG, CSS variables, `calc()`, page breaks and
Unicode work. Known gaps:

| Not supported | Use instead |
| --- | --- |
| `<thead>` repeating on later pages | Split long tables per page |
| `background: url()`, `@font-face`, `@import` | `<img>`; the `fonts` option |
| `float`, `z-index`, `rowspan` | Flexbox or grid; reorder; nested tables |
| `@page { size }`, `@media print` | `width` / `height`; unconditional rules |
| `-webkit-background-clip: text` | A solid text color |
| Vertical padding on `display: inline-block` | `display: block` or flexbox |
| JavaScript in templates | Compute values before calling `generate()` |

## Errors

Every failure has a stable `code`, a readable message, and where useful a `line` and `hint`
(`MISSING_FIELD`, `INVALID_FIELD`, `TEMPLATE_ERROR`, `TEMPLATE_TOO_EXPENSIVE`, `FONT_ERROR`, `TIMEOUT`,
`RENDER_FAILED`, ...). The full typed list is in [`node/index.d.ts`](node/index.d.ts).

```json
{ "error": "Template rendering failed: syntax error ... (in <string>:2)", "code": "TEMPLATE_ERROR",
  "status": 400, "line": 2, "hint": "Check the template syntax: {{ value }}, {% for x in list %}...{% endfor %}." }
```

## HTTP and Docker

<details>
<summary>Run, API and configuration</summary>

### Run

```bash
docker run -d -p 6767:6767 \
  -e API_BEARER_TOKEN=secret \
  -e STORAGE_URL=http://minio:9000 -e STORAGE_BUCKET=pdfs \
  -e STORAGE_ACCESS_KEY=... -e STORAGE_SECRET_KEY=... \
  zalven88/pdf-engine:1.1.0
```

`STORAGE_*` is only needed for `/api/to-s3`; without it that endpoint answers
`STORAGE_NOT_CONFIGURED`. Inside Docker Compose, use the service name (e.g. `http://minio:9000`),
not `localhost`.

#### Docker Compose (no setup needed)

Save as `compose.yaml` and run `docker compose up -d`. It starts pdf-engine with its own
S3-compatible storage ([RustFS](https://github.com/rustfs/rustfs)) and creates the bucket.

```yaml
services:
  pdf-engine:
    image: zalven88/pdf-engine:1.1.0
    ports:
      - "6767:6767"
    environment:
      API_BEARER_TOKEN: change-me
      STORAGE_URL: http://storage:9000
      STORAGE_BUCKET: pdfs
      STORAGE_ACCESS_KEY: pdf-engine
      STORAGE_SECRET_KEY: pdf-engine-secret
    depends_on:
      storage-setup:
        condition: service_completed_successfully

  storage:
    image: rustfs/rustfs
    environment:
      RUSTFS_ACCESS_KEY: pdf-engine
      RUSTFS_SECRET_KEY: pdf-engine-secret
    volumes:
      - storage-data:/data

  storage-setup: # creates the bucket, then exits
    image: amazon/aws-cli
    depends_on:
      - storage
    environment:
      AWS_ACCESS_KEY_ID: pdf-engine
      AWS_SECRET_ACCESS_KEY: pdf-engine-secret
      AWS_DEFAULT_REGION: us-east-1
    entrypoint: >
      sh -c "until aws --endpoint-url http://storage:9000 s3api head-bucket --bucket pdfs 2>/dev/null ||
                   aws --endpoint-url http://storage:9000 s3 mb s3://pdfs; do sleep 1; done"

volumes:
  storage-data:
```

Change `API_BEARER_TOKEN` and the storage keys before exposing it. To use AWS S3 or your own
storage instead, drop `storage`/`storage-setup` and point `STORAGE_*` at it. Presigned URLs
from `/api/to-s3` use `STORAGE_URL`, so they only resolve inside the Compose network unless
storage is reachable at that address.

### API

| Endpoint | Auth | Does |
| --- | --- | --- |
| `POST /api/to-bytes` | Bearer | Returns the PDF |
| `POST /api/to-s3` | Bearer | Uploads, returns JSON with a presigned URL |
| `POST /api/fonts/reload` | Bearer | Reloads fonts without a restart |
| `GET /health` | — | `{"status":"ok","version":"…"}` |

Form fields (`multipart/form-data`):

| Field | Default | |
| --- | --- | --- |
| `template` | required | HTML/Minijinja template |
| `data` | `null` | JSON for the template |
| `width` / `height` | `8.5in` / `11in` | Length in `in`, `mm`, `cm`, `pt`, `pc`, `px` |
| `orientation` | `portrait` | or `landscape` |
| `filename` | `document.pdf` | `/api/to-bytes` download name |
| `password` | — | Encrypts the PDF |

```bash
curl -H "Authorization: Bearer secret" \
  --form-string 'template=<h1>Invoice {{ no }}</h1>' -F 'data={"no": 42}' \
  http://localhost:6767/api/to-bytes -o invoice.pdf
```

Responses are gzip/brotli/zstd compressed when the client asks.

### Configuration

| Variable | Default | |
| --- | --- | --- |
| `API_BEARER_TOKEN` | required (HTTP) | API token |
| `STORAGE_URL`, `STORAGE_BUCKET`, `STORAGE_ACCESS_KEY`, `STORAGE_SECRET_KEY` | optional | S3 / MinIO, for `/api/to-s3` |
| `STORAGE_REGION` | `us-east-1` | |
| `API_PORT` | `6767` | |
| `RENDER_TIMEOUT_SECS` | `45` | Per request; `408` when exceeded |
| `ASSET_ALLOWED_HOSTS` | off | Hosts `<img>` may load from, e.g. `cdn.example.com,*.assets.io` |
| `ASSET_MAX_BYTES` / `ASSET_TIMEOUT_MS` / `ASSET_MAX_IMAGES` | `5242880` / `5000` / `20` | Per-image limits |
| `ASSET_CACHE_BYTES` / `ASSET_CACHE_TTL_SECS` | `268435456` / `300` | Shared cache of fetched remote images (`0` bytes disables) |
| `TEMPLATE_MAX_OUTPUT_BYTES` / `TEMPLATE_FUEL` | `5368709120` / `50000000` | Cap on rendered HTML size and template execution budget |
| `ASSET_ALLOW_HTTP` / `ASSET_ALLOW_PRIVATE_IPS` | `false` | Only for trusted internal networks |
| `FONT_WATCH` / `FONT_WATCH_DIR` | `false` / `/usr/share/fonts/pdf-engine` | Reload fonts when the directory changes |
| `IPC_MAX_REQUEST_BYTES` / `IPC_MAX_IN_FLIGHT` | `268435456` / 4 per core | IPC mode: largest request, and requests held at once |
| `RUST_LOG` | `info` | Log level (stderr) |

Everything optional is off by default; set it in `.env` or the container environment.

### Images

`<img src="data:image/...">` always works. Remote `https://` images load only from
`ASSET_ALLOWED_HOSTS`: no redirects, no private/loopback addresses (checked after DNS), images
only, size and time limits. Blocked or broken images are skipped; the PDF still renders.

### Fonts and languages

The image ships about 150 Noto fonts via [fonts/fonts.conf](fonts/fonts.conf), so text in
practically any living language renders instead of showing empty boxes:

- **Latin, Greek, Cyrillic** (including Vietnamese and all European accents), plus a serif and a
  monospace design
- **Chinese (simplified and traditional), Japanese, Korean**
- **Arabic, Urdu, Hebrew, Thaana, Syriac**, all right-to-left
- **Indic:** Devanagari (Hindi, Marathi, Nepali), Bengali, Tamil, Telugu, Kannada, Malayalam,
  Gujarati, Gurmukhi (Punjabi), Sinhala, Oriya, and more
- **South-East Asian:** Thai, Lao, Khmer, Myanmar, Javanese, Balinese, Sundanese, and more
- **Others:** Ethiopic, Georgian, Armenian, Tibetan, Mongolian, Cherokee, Canadian Aboriginal,
  N'Ko, Tifinagh, Vai, Adlam, Yi, and more
- **Color emoji, flags, symbols and math**

`sans-serif`/Arial → Noto Sans, `serif` → Noto Serif, `monospace` → Noto Sans Mono. Characters
a font lacks fall back to a font that has them; `tests/languages.rs` covers about 30 scripts. To add a font: copy it into `/usr/share/fonts/pdf-engine`, then
`POST /api/fonts/reload` (or run with `FONT_WATCH=true`).

**Running outside Docker** (for example `cargo run` on macOS) uses that machine's own fonts,
which can lack some scripts (on the Mac we tested, Chinese ideographs, ★ and ✓ showed as boxes).
The Docker image is what production renders with, so judge language output there
(`make docker-image`, or the published image).

</details>

## Development

<details>
<summary>Build, test and release</summary>

```bash
make test       # functional test suites
make check      # fmt + tests (CI)
make node       # Node.js client tests (cargo build first)
make help       # everything else
```

No Rust locally? Use the `docker-` variants (`make docker-test`, ...).

### Release

Bump `version` in `Cargo.toml`, add a [CHANGELOG.md](CHANGELOG.md) entry, commit, then
`make release`. It tags `vX.Y.Z` and pushes `zalven88/pdf-engine:X.Y.Z`, `X.Y`, `X` and
`latest` for amd64 and arm64 (built one after the other; also `X.Y.Z-amd64` / `-arm64`).

</details>

## License

MIT. Rendering by [Fulgur](https://github.com/fulgur-rs/fulgur) (MIT/Apache-2.0); fonts by
[Noto](https://fonts.google.com/noto) (OFL).
