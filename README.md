

# 📄 PDF Engine: High-Speed Rust + PrinceXML Microservice

A blazingly fast, standalone microservice that converts HTML/CSS to production-ready PDFs. Built with **Rust**, powered by **PrinceXML**, and wrapped in a secure **Axum** web server.

## ⚡️ Purpose: Why this over Gotenberg?

While tools like Gotenberg use Headless Chrome (built for screens), **this engine uses PrinceXML (built for print).** If you are generating invoices, books, reports, or tickets, this is the tool for you.

* **Print-Perfect CSS:** Full support for `@page` rules, CMYK colors, footnotes, and physical margins.
* **Ultra-Lightweight:** Compiles HTML directly to a PDF binary. No massive browser bloat.
* **Built-in Templating:** Send raw JSON and a Minijinja (Jinja2) template in one request.
* **Direct-to-S3:** Bypasses your main app by uploading directly to S3/MinIO and returning a signed URL.

---

## 🛠️ Installation & Quickstart

The engine is available as a pre-built Docker image containing all necessary fonts and dependencies.

### 1. Using Docker CLI

Run this command for quick testing:

```bash
docker run -d \
  --name pdf-engine \
  -p 6767:6767 \
  -e API_BEARER_TOKEN="your_secret_token_here" \
  zalven88/pdf-engine:latest

```

### 2. Using Docker Compose (Recommended)

Add this to your `docker-compose.yml`. Note that inside a Docker network, you must use the service name (e.g., `minio`) for the `STORAGE_URL`.

```yaml
services:
  pdf-engine:
    image: zalven88/pdf-engine:latest
    container_name: pdf-engine
    restart: always
    ports:
      - "6767:6767"
    environment:
      - API_PORT=6767
      - API_BEARER_TOKEN=${PDF_ENGINE_TOKEN}
      - STORAGE_DRIVER=minio
      - STORAGE_ACCESS_KEY=${S3_KEY}
      - STORAGE_SECRET_KEY=${S3_SECRET}
      - STORAGE_BUCKET=my-bucket
      - STORAGE_URL=http://minio:9000  # Internal Docker URL
      - STORAGE_REGION=us-east-1
    depends_on:
      - minio
    networks:
      - app-network

networks:
  app-network:
    driver: bridge

```

---

## 📡 API Usage

All endpoints require a `multipart/form-data` payload and your Bearer token in the headers.

### Form Fields

| Field | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `template` | String | **Yes** | - | The Minijinja/HTML template. |
| `data` | JSON | No | `null` | Data to inject into the template. |
| `width` | String | No | `8.5in` | Page width. |
| `height` | String | No | `11in` | Page height. |
| `filename` | String | No | `document.pdf` | Download name (or S3 key suffix). |
| `password` | String | No | None | Encrypts the PDF with this password. |
| `orientation` | String | No | `portrait` | `portrait` or `landscape`. |
| `engine` | String | No | `auto` | `auto`, `fulgur` or `prince` (see below). |

### Rendering Engines

Documents render with **Fulgur** (pure Rust, in-process) by default, and with **PrinceXML** when the template uses CSS Fulgur can't render faithfully yet. Every response says which engine ran in the `X-Pdf-Engine` header.

| Sent to Prince when the template uses | Why |
| --- | --- |
| `data:` images, `url()`, `@font-face`, `@import` | Fulgur doesn't load them |
| `hsl()` / `hsla()` | Element isn't painted |
| `border-top/right/bottom/left` with a colour, or per-side `border-color` | Drawn black / single colour |
| `display: inline-block` (and `inline-flex`/`inline-grid`) | Wrapping rows aren't drawn at all |
| `float`, `z-index`, `position: fixed`, `rowspan` | Wrong layout or stacking |
| `@page`, `@media`, running headers, `::before`/`::after`, counters, `::first-letter` | Ignored or misplaced |
| `text-transform`, `outline`, multi-column, backgrounds on inline elements, SVG `<text>` | Not rendered |
| Page sizes other than plain lengths (e.g. `A4` keyword) | Fulgur needs lengths |

A table with `<thead>` that spans more than one page is re-rendered with Prince, because Fulgur doesn't repeat headers on continuation pages. If Fulgur fails or panics, the request falls back to Prince. `engine=fulgur` / `engine=prince` skips the routing.

Responses are compressed with gzip, brotli or zstd at level 6 when the client sends `Accept-Encoding`. The PDF is produced once and handed to the response or the S3 upload without further copies; it's only rewritten to strip Prince's blank lead page or to encrypt.

### Endpoint 1: Direct to Bytes

Returns the raw binary PDF file.

```bash
curl -X POST http://localhost:6767/api/to-bytes \
  -H "Authorization: Bearer my_secret_token" \
  --form-string "template=<h1>Invoice</h1><p>Billed to: {{ name }}</p>" \
  -F "data={\"name\": \"Zalven\"}" \
  --output my_invoice.pdf

```

### Endpoint 2: Direct to S3

Uploads to S3/MinIO and returns a JSON URL.

```bash
curl -X POST http://localhost:6767/api/to-s3 \
  -H "Authorization: Bearer my_secret_token" \
  --form-string "template=<h1>Welcome {{ name }}</h1>" \
  -F "data={\"name\": \"Zalven\"}"

```

### Optional Settings

| Env var | Default | Description |
| --- | --- | --- |
| `PRINCE_BIN` | `prince` | Prince executable to run. |
| `RENDER_TIMEOUT_SECS` | `45` | Per-request budget for render + upload; exceeding it returns `408`. |
| `PDF_ENGINE` | `auto` | Default engine when a request doesn't send `engine`. |
| `FULGUR_FONT_DIR` | `/usr/share/fonts/truetype/noto` | Where Fulgur's bundled Noto fonts are loaded from. Documents with characters those fonts don't cover use system fonts (slower). |

If Prince fails its startup warm-up, the server keeps running with Fulgur only.

---

## 🧪 Testing

The suites under `tests/` boot the real router in-process against a fake S3 and either a scripted fake Prince or the real `prince` binary:

| Suite | Covers |
| --- | --- |
| `happy_path` | Correct PDFs, headers, page sizes, encryption, S3 payload |
| `sad_path` | Auth failures, missing/broken templates, bad requests |
| `poison_pill` | Header injection, template bombs, oversized bodies, SSRF / local-file access |
| `chaos` | Prince crashing, hanging, emitting garbage, missing binary |
| `integration_chaos` | S3 erroring, hanging, unreachable, flaky under load |
| `race_conditions` | Unique keys, concurrency cap, isolation, cancellation storms |
| `zero_absence` | Empty/missing fields, orphaned Prince processes, leaked permits |
| `smoke` | Quick liveness; `live_*` tests hit a deployed instance (`--ignored`) |
| `hybrid` | Engine routing, overrides, Fulgur failure/panic fallback, timeouts, compression |
| `css_parity` | Every CSS probe the router sends to Fulgur must match Prince pixel-for-pixel and by colour; 20 realistic documents render end to end |

```bash
cargo test --lib --test happy_path --test sad_path --test poison_pill --test chaos \
  --test integration_chaos --test race_conditions --test zero_absence --test smoke \
  --test hybrid --test css_parity
```

Tests that need the real Prince (and `pdftoppm` from poppler-utils, for `css_parity`) are skipped when they aren't on `PATH`; set `REQUIRE_PRINCE=1` (e.g. in CI) to make that a failure instead.

To check whether a newer Fulgur fixes any of the routed-away features, run `cargo test --test css_parity report_fulgur_gaps -- --nocapture`.

Benchmarks: `cargo bench --bench render` (Fulgur vs Prince per document type, routing, templating, encryption, gzip levels).

---

## 🔍 Troubleshooting

### Docker Networking (`Connection Refused`)

If your logs show `Connection Refused` when trying to upload to S3/MinIO:

* **Don't use** `127.0.0.1` or `localhost` in your `STORAGE_URL` if the storage is in another container.
* **Do use** the service name defined in your compose file (e.g., `http://minio:9000`).

### PrinceXML Watermark

If you see a "Prince" watermark on your PDFs, you are using the free version. To remove it:

1. Purchase a license from [PrinceXML](https://www.princexml.com/).
2. Follow the license installation instructions (usually placing a `license.dat` file in the engine's path).

---

## ⚖️ License & Acknowledgements

The Rust application code is open-source under the **MIT License**.

**⚠️ Important Note regarding PrinceXML:**
This project utilizes [PrinceXML](https://www.princexml.com/) as its core rendering engine. PrinceXML is a commercial product. Please review their [End User License Agreement](https://www.princexml.com/license/) before deploying this to production.
