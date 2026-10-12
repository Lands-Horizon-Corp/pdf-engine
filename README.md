# 📄 PDF Engine: High-Speed Rust HTML → PDF Microservice

A standalone microservice that converts HTML/CSS templates to PDFs. Built with **Rust**, rendered in-process by **[Fulgur](https://github.com/fulgur-rs/fulgur)** (a pure-Rust HTML/CSS layout engine, no browser and no external binaries), and served by **Axum**.

## ⚡️ Why this engine

* **No browser, no subprocess:** rendering is a library call inside the server. No Chromium, no PrinceXML, no child processes to manage.
* **Fast:** ~2.5 ms per invoice; ~1,000 invoices/s sustained on a 10-core box (see [Performance](#-performance)).
* **Built-in templating:** send a Minijinja (Jinja2) template and JSON data in one request.
* **Direct-to-S3:** upload to S3/MinIO and get a presigned URL back.
* **Safe by default:** no network access and no local file access from templates.

---

## 🛠️ Installation & Quickstart

The Docker image ([zalven88/pdf-engine](https://hub.docker.com/r/zalven88/pdf-engine), linux/amd64 and linux/arm64) contains the server, fontconfig and a curated Noto font set. Pin a version in production (`zalven88/pdf-engine:1.0.0`); `latest` follows the newest release.

### 1. Using Docker CLI

```bash
docker run -d \
  --name pdf-engine \
  -p 6767:6767 \
  -e API_BEARER_TOKEN="your_secret_token_here" \
  -e STORAGE_URL="http://minio:9000" \
  -e STORAGE_BUCKET="my-bucket" \
  -e STORAGE_ACCESS_KEY="..." \
  -e STORAGE_SECRET_KEY="..." \
  zalven88/pdf-engine:1.0.0
```

The `STORAGE_*` variables are required at startup even if you only use `/api/to-bytes`.

### 2. Using Docker Compose (Recommended)

Inside a Docker network, use the service name (e.g., `minio`) for `STORAGE_URL`.

```yaml
services:
  pdf-engine:
    image: zalven88/pdf-engine:1.0.0
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

All endpoints take a `multipart/form-data` payload and your Bearer token.

### Form Fields

| Field | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `template` | String | **Yes** | - | The Minijinja/HTML template. |
| `data` | JSON | No | `null` | Data to inject into the template. |
| `width` | String | No | `8.5in` | Page width: a length in `in`, `mm`, `cm`, `pt`, `pc` or `px`. |
| `height` | String | No | `11in` | Page height, same units. |
| `orientation` | String | No | `portrait` | `portrait` or `landscape`. |
| `filename` | String | No | `document.pdf` | Download name for `/api/to-bytes`. |
| `password` | String | No | None | Encrypts the PDF with this password. |

Invalid page sizes return `400`. Responses are compressed with gzip, brotli or zstd at level 6 when the client sends `Accept-Encoding`. The PDF is produced once and handed to the response or the S3 upload without further copies; it's only rewritten when encrypting.

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

Uploads to S3/MinIO and returns a JSON payload with a presigned URL.

```bash
curl -X POST http://localhost:6767/api/to-s3 \
  -H "Authorization: Bearer my_secret_token" \
  --form-string "template=<h1>Welcome {{ name }}</h1>" \
  -F "data={\"name\": \"Zalven\"}"
```

### Optional Settings

| Env var | Default | Description |
| --- | --- | --- |
| `RENDER_TIMEOUT_SECS` | `45` | Per-request budget for render + upload; exceeding it returns `408`. |
| `FONTCONFIG_FILE` | `/etc/pdf-engine/fonts.conf` (in the image) | Font set the renderer can use; see [Fonts](#fonts). |

### Fonts

The renderer only sees the fonts fontconfig exposes, and every exposed font adds per-render work. The image ships [fonts/fonts.conf](fonts/fonts.conf), which exposes a curated set of 29 Noto fonts (Latin weights, Serif, Mono, CJK, Arabic, Hebrew, Thai, color emoji) copied in by [fonts/install.sh](fonts/install.sh). Generic and common web families resolve through fontconfig: `sans-serif`, `Arial` and `Helvetica` → Noto Sans; `serif` and `Times New Roman` → Noto Serif; `monospace` and `Courier New` → Noto Sans Mono.

To add a font, drop it into `/usr/share/fonts/pdf-engine`, run `FONTCONFIG_FILE=/etc/pdf-engine/fonts.conf fc-cache -f`, and restart the server (the font collection is built once at startup). Without `FONTCONFIG_FILE` the renderer sees every installed font: correct, but much slower and heavier.

### CSS Support

Fulgur covers most layout used in documents. These render well (each verified by a fixture in `tests/fixtures`): flexbox, grid, block layout with margins/padding/`box-sizing`, positioned elements, transforms, opacity, borders (full `border` shorthand, dashed/dotted/double, radius), box shadows, solid colors in hex/rgb/rgba/named, linear and radial gradients, tables (`border-collapse`, `colspan`, cell backgrounds), inline SVG, CSS variables and `calc()`, `nth-child` selectors, page breaks, Unicode (CJK, Arabic, emoji).

Known gaps in Fulgur 0.40. These render differently from a browser, so prefer the alternative where one exists:

| Feature | What happens | Alternative |
| --- | --- | --- |
| `<img src="data:...">`, `background: url(...)`, `@font-face`, `@import` | Not loaded | Inline `<svg>`; fonts via [fonts](#fonts) |
| `hsl()` / `hsla()` | Element not painted | Hex or `rgb()` |
| Single-side border with a colour (`border-bottom: 1px solid #ddd`) | Drawn black | Full `border` shorthand, or `border-bottom-color` longhand |
| Per-side `border-color: a b c d` | All sides use the first colour | Per-side longhands |
| A row of `inline-block` elements that wraps onto a second line | Not drawn at all | Flexbox with `flex-wrap` |
| `float` | Doesn't wrap text | Flexbox or grid |
| `z-index` | Ignored (document order wins) | Reorder elements |
| `rowspan` | Wrong layout | Nested tables or grid |
| `<thead>` on multi-page tables | Not repeated on continuation pages | |
| `@page { size: ... }` | Ignored | `width` / `height` form fields |
| `@page` margin boxes / running headers (`@top-center`, `position: running()`) | Rendered, but placed differently than in a browser; running headers can repeat in the body | Check output; keep headers simple |
| `@media print` | Not applied (screen rules are) | Unconditional rules |
| `::before`/`::after` with `display: block`, `counter()` in `content` | Wrong / empty | Real elements |
| `text-transform`, `outline`, `::first-letter`, inline-element backgrounds, multi-column with block children, SVG `<text>` | Not rendered | Transform text in the template; use `border` instead of `outline` |

---

## 🧪 Testing

The suites under `tests/` boot the real router in-process against a fake S3, with a spy around the renderer that records every render call; chaos tests swap in renderers that fail, panic or hang.

| Suite | Covers |
| --- | --- |
| `happy_path` | Correct PDFs, headers, page sizes, fonts, encryption, titles, S3 payload |
| `sad_path` | Auth failures, missing/broken templates, bad requests |
| `poison_pill` | Header injection, template bombs, oversized bodies, SSRF / local-file access |
| `chaos` | Renderer failing, panicking, hanging, emitting garbage, flaky under load |
| `integration_chaos` | S3 erroring, hanging, unreachable, flaky under load |
| `race_conditions` | Unique keys, concurrency cap, isolation, cancellation storms |
| `zero_absence` | Empty/missing fields, renders outliving requests, leaked permits |
| `compression` | gzip / brotli / zstd negotiation |
| `css` | 20 realistic documents render with the expected page counts; 50+ single-feature CSS probes render |
| `smoke` | Quick liveness; `live_*` tests hit a deployed instance (`--ignored`) |

```bash
make test            # all functional suites above (hermetic: no server, no network)
make check           # formatting check + functional suites (use this in CI)
make bench           # criterion micro-benchmarks
make stress          # load test: scenario matrix, concurrency sweep, 20k soak (MODE=matrix|sweep|soak)
make test-live       # smoke + integration tests against a running server (SMOKE_URL, API_BEARER_TOKEN)
```

No Rust toolchain locally? Prefix with `docker-` (`make docker-test`, `make docker-bench`, `make docker-stress`) to run inside `Dockerfile.dev`, which has the toolchain and the production font set. `make help` lists every target. `JOBS` (default 2) limits parallel builds; linking many test binaries at once needs a lot of memory.

---

## 🚀 Performance

There are two kinds of benchmarks:

| Command | What it measures | Takes |
| --- | --- | --- |
| `make stress` | **Load test:** a release server under concurrent HTTP traffic. Throughput (PDFs/s), latency percentiles and peak memory per scenario, a concurrency sweep, and a 20,000-PDF soak. Use it for capacity planning. | ~10 min |
| `make bench` | **Micro-benchmarks:** single-threaded cost of each step (render per document type, templating, encryption, gzip), measured with Criterion. Use it to compare code or Fulgur versions. | ~3 min |

Both have `docker-` variants (`make docker-stress`, `make docker-bench`) that run inside `Dockerfile.dev` with the production font set, so you don't need Rust installed. The results below are from those, on one machine: 10 CPUs (Linux aarch64 under Docker Desktop, shared with other containers). Absolute numbers will differ on your hardware; rerun on your servers before capacity planning.

### Load test: what 100,000 PDFs takes on one 10-core server

| Workload | PDFs/s | 100k PDFs |
| --- | --- | --- |
| 20-line invoices, sustained over 20,000 | ~970 | **~1.7 min** |
| Production-like mix (60% invoices / 30% two-page invoices / 10% CJK) | ~640 | ~2.6 min |
| 10-page reports | ~295 | ~5.7 min |
| 200-line (multi-page) invoices | ~200 | ~8.3 min |

### Load test: scenario matrix (concurrency 32)

| Scenario | PDFs/s | p50 | p99 | Peak memory |
| --- | --- | --- | --- | --- |
| Small document | 2,841 | 8.8 ms | 31 ms | 332 MB |
| 20-line invoice | 1,269 | 22 ms | 47 ms | 423 MB |
| 200-line invoice (multi-page) | 202 | 149 ms | 212 ms | 498 MB |
| 10-page report (grid, page breaks) | 295 | 98 ms | 132 ms | 407 MB |
| Password-encrypted PDF | 997 | 24 ms | 82 ms | 599 MB |
| 500-row table full of CJK + emoji | 55 | 533 ms | 912 ms | 1.1 GB |
| Mixed traffic | 642 | 43 ms | 90 ms | 838 MB |

All 2,100 requests in the matrix returned `200`.

### Load test: concurrency sweep (20-line invoice)

| In-flight requests | PDFs/s | p50 | p99 | Peak memory |
| --- | --- | --- | --- | --- |
| 1 | 223 | 4.4 ms | 6.9 ms | 78 MB |
| 2 | 396 | 4.0 ms | 13.8 ms | 93 MB |
| 5 | 1,094 | 4.4 ms | 7.8 ms | 132 MB |
| 10 | 1,193 | 7.3 ms | 18.7 ms | 231 MB |
| 20 | 1,401 | 13.2 ms | 26.6 ms | 337 MB |
| 40 | 1,424 | 26.1 ms | 45.7 ms | 371 MB |
| 80 | 1,236 | 54.9 ms | 89.4 ms | 540 MB |

Throughput peaks at two to four in-flight requests per core. Beyond that, requests queue on the render semaphore (one permit per core), so latency rises while memory stays bounded.

### Load test: soak, 20,000 invoices on one server

| PDFs so far | PDFs/s | p99 | Server memory |
| --- | --- | --- | --- |
| 0 (idle) | | | 73 MB |
| 2,000 | 1,007 | 78 ms | 428 MB |
| 6,000 | 852 | 99 ms | 463 MB |
| 10,000 | 938 | 111 ms | 444 MB |
| 14,000 | 866 | 71 ms | 453 MB |
| 20,000 | 957 | 68 ms | 427 MB |

Memory plateaus around 450 MB with no sustained growth (mimalloc keeps freed memory for reuse), and every request succeeded.

### Micro-benchmarks (single thread)

| Step | Time | Notes |
| --- | --- | --- |
| Render a tiny document | 1.4 ms | heading + paragraph |
| Render a flexbox layout | 1.8 ms | `tests/fixtures/css/05_flexbox.html` |
| Render a grid layout | 1.7 ms | `06_grid.html` |
| Render an invoice | 3.3 ms | `08_invoice.html` |
| Render Chinese, Japanese and Korean text | 6.9 ms | font fallback across scripts |
| Render a 300-row table | 24.7 ms | multi-page |
| Fill a template with 500 rows | 0.44 ms | Minijinja |
| Encrypt a PDF (`password`) | 0.60 ms | lopdf, RC4 128-bit |
| Hand the PDF to the response without a password | < 1 µs | zero-copy |
| gzip level 1 / 6 / 9 a small PDF | 74 / 160 / 164 µs | 168 / 78 / 76 MiB/s |

Rendering is where the time goes. Templating, encryption and compression together add well under 1 ms. Level 6 compresses almost as well as level 9 at the same cost.

### What makes it fast

* **Shared font context:** Fulgur normally rebuilds its font collection from fontconfig for every document. A small patch (`vendor/fulgur`, see [PATCHES.md](vendor/fulgur/PATCHES.md)) builds it once per process, which made renders 4–6× faster. Fonts added after startup need a restart.
* **Curated font set:** every font fontconfig exposes is enumerated, so the image exposes 29 files instead of ~2,300.
* **mimalloc:** rendering allocates heavily; mimalloc is ~10% faster than the system allocator.
* **Zero-copy output:** the PDF is one buffer from the renderer to the socket or S3.

### Production sizing

* **Memory:** budget ~600 MB per instance for invoice-style documents and ~1.5 GB if you render large CJK/emoji-heavy documents. Set a container memory limit with that headroom.
* **Concurrency:** the server caps concurrent renders at one per core; scale out with more instances rather than more concurrency per instance.
* **Timeouts:** keep `RENDER_TIMEOUT_SECS` above your p99 for the largest documents you generate. A render can't be interrupted once started; a timed-out render keeps its concurrency slot until it finishes.
* **Fonts:** keep `FONTCONFIG_FILE` pointed at the curated set; every extra font file adds per-render cost.
* **Compression:** gzip at level 6 saves ~20% on PDFs; clients that don't send `Accept-Encoding` get the raw PDF.

### Running the benchmarks

```bash
make stress                  # load test: matrix + concurrency sweep + 20k soak
make stress MODE=matrix      # one part only: matrix | sweep | soak
make bench                   # micro-benchmarks (local runs also write target/criterion/report/index.html)

# Load-test a deployed instance (no memory numbers):
PDF_URL=https://pdf.example.com API_BEARER_TOKEN=... make stress MODE=matrix
```

`make stress` runs [scripts/loadtest.sh](scripts/loadtest.sh). Without `PDF_URL` it builds a release binary and starts a fresh server for every scenario, measuring that server's peak memory. Use the production font set (`FONTCONFIG_FILE`, or `make docker-stress`) for representative numbers. Tunables: `REQUESTS`, `CONCURRENCY`, `SWEEP`, `SOAK_ROUNDS`, `SOAK_BATCH`.

Each scenario is an ignored test in [tests/stress_tests.rs](tests/stress_tests.rs) and can run on its own:

```bash
STRESS_URL=http://localhost:6767 API_BEARER_TOKEN=... STRESS_REQUESTS=2000 STRESS_CONCURRENCY=32 \
  cargo test --release --test stress_tests -- --ignored --nocapture stress_to_bytes_invoice
```

Set `STRESS_MAX_P99_MS` to make a scenario fail when its p99 latency exceeds the limit; that works as a performance gate in CI. Micro-benchmark sources are in [benches/render.rs](benches/render.rs).

---

## 🔍 Troubleshooting

### Docker Networking (`Connection Refused`)

If your logs show `Connection Refused` when trying to upload to S3/MinIO:

* **Don't use** `127.0.0.1` or `localhost` in your `STORAGE_URL` if the storage is in another container.
* **Do use** the service name defined in your compose file (e.g., `http://minio:9000`).

### Text missing or in the wrong font

The renderer only sees fonts fontconfig exposes. Check what a family resolves to with `FONTCONFIG_FILE=/etc/pdf-engine/fonts.conf fc-match "<family>"`, and add the font as described in [Fonts](#fonts).

---

## 🏷️ Versioning & Releases

Releases follow [Semantic Versioning](https://semver.org/) and are listed in [CHANGELOG.md](CHANGELOG.md). Each release is a git tag `vX.Y.Z` and a multi-arch image tagged `X.Y.Z`, `X.Y`, `X` and `latest`.

To cut a release:

1. Bump `version` in `Cargo.toml` and add a section to `CHANGELOG.md`.
2. Commit, then run `make release`. This tags `vX.Y.Z`, pushes the tag, cross-compiles the image for linux/amd64 and linux/arm64, and pushes it to Docker Hub (`docker login` first).

`make release-image` pushes the image without tagging git; `make docker-image` builds for the local machine only.

---

## ⚖️ License & Acknowledgements

The Rust application code is open-source under the **MIT License**. Rendering is provided by [Fulgur](https://github.com/fulgur-rs/fulgur) (MIT OR Apache-2.0). Fonts are from the [Noto](https://fonts.google.com/noto) project (SIL Open Font License).
