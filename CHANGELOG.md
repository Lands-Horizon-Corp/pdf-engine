# Changelog

All notable changes to this project are documented here. Versions follow
[Semantic Versioning](https://semver.org/); each release is a git tag `vX.Y.Z` and the Docker
image `zalven88/pdf-engine:X.Y.Z` (also `X.Y`, `X` and `latest`).

## [Unreleased]

### Performance

- **Memory leak fixed.** A font file that reached a render through a fresh handle (for example a font
  named in `font-family` that the system opens on demand) was copied and kept for every document, about
  7 MB each. Ten thousand multi-table statements peaked at 37 GB; they now peak at 0.8 GB and finish 3×
  faster (52 s to 17 s). Ten thousand 7-page invoices went from 10.2 GB to 0.85 GB.
- **Browser package about 1.7× faster, native small documents faster too.** The parsed user-agent stylesheet
  and the style data built from it are now reused for every document on a thread (patched blitz-dom and
  stylo; it was about half of a small document's time). The WebAssembly build is optimised for speed
  (opt-level 3, SIMD), and PDF streams are stored uncompressed by default (`compression: 'fast'`;
  `'small'` gives about 40% smaller files and takes about 20% longer). A receipt takes about 0.8 ms and a
  30-line invoice about 1.7 ms in Chromium. The WebAssembly module grew from 2.4 to 3.2 MB (brotli).

### Added

- **Printing** (Electron, Tauri, Node, any program): the engine lists printers and prints a receipt as
  ESC/POS, a label as ZPL or a document as PDF, through the system queue (`system://`), a network socket
  (`net://host:9100`) or a USB device directly (`usb://vendor:product`, pure Rust, no libusb). Use it from the
  Node client (`printers()`, `print()` over IPC) or from the command line: `pdf-engine printers` and
  `pdf-engine print --request <file>` (one JSON answer on standard output), which a Tauri sidecar can run.
  New IPC ops `raster`, `printers` and `print`; error codes `PRINT_FAILED` and `PRINTER_UNREACHABLE`.
  New setting `API_HOST`. Not yet tried on physical printers; see `PUBLISHING.md`.
- **Browser package: several servers and smart routing.** `url` and `token` accept arrays (round robin; a
  server that is down or overloaded is skipped), `pick: 'server' | 'browser' | 'auto'` with `autoMaxBytes`,
  and a fallback to WebAssembly when offline. Only the `data` keys a template names are sent (`pruneData`).
- `download()`, `open()` and `print()` (browser print dialog) on the browser package, and a documented
  no-build-step CDN setup for plain HTML, Vanilla JS and jQuery. `generate<T>()` types the data.
- **Images everywhere.** Browser package: `<img src>` can be a URL, `blob:`, `data:` or a name from the new
  `images` option (Blob, File, bytes), loaded before rendering; failures are skipped and reported to
  `onImageError`. Node client: `images` (bytes, `data:` URI or file path, sent in the request frame),
  `imageDir` and `imageHosts`. Server: `ASSET_LOCAL_DIR`.
- Images with no height, or only a width, are drawn at their own proportions (they used to vanish).
- **IPC mode** for Node.js and Electron: `pdf-engine ipc` serves render requests over
  stdin/stdout (no HTTP server, no port, no token, no storage settings). PDFs come back as raw
  bytes. Protocol: `src/ipc.rs`.
- **Browser package** in `.node/wasm/` (`@lands-horizon-corp/pdf-engine-wasm`): the same renderer compiled to
  WebAssembly for React, Vue, Svelte and plain JS. No server; bundled Latin fonts, add others with `fonts`.
- `url` option in the browser package: render on a deployed server instead of in the page. The HTTP
  server answers browsers with `CORS_ALLOWED_ORIGINS` (default off).
- `.public/benchmark/`: benchmarks against html2pdf.js, jspdf-html2canvas and headless Chromium; browser matrix
  tests (Chromium, WebKit, Firefox in CI) with plain JS, React and Vue in `.node/wasm/e2e/`.
- **Node.js client** in `.node/desktop/`: `new PDFEngine({ fonts })` and `generate({ template, data,
  password, ... })`, with TypeScript types. See `.node/desktop/README.md`. Published as
  `@lands-horizon-corp/pdf-engine`.
- Every error now has a stable `code` (`TEMPLATE_ERROR`, `MISSING_FIELD`, ...), plus `status`,
  `hint` and, for templates, the `line`. The HTTP `error` field keeps its message.
- Custom fonts: the `fonts` option (IPC `load_fonts`).
- About 150 Noto fonts in the image: practically every living script, plus symbols and emoji.
- Template output cap and execution budget (`TEMPLATE_MAX_OUTPUT_BYTES`, `TEMPLATE_FUEL`);
  remote image cache (`ASSET_CACHE_BYTES`, `ASSET_CACHE_TTL_SECS`).

### Changed

- Faster rendering: fonts opened once per process, per-document style data without a global
  lock, no per-render copies of font files. Patched copies of fulgur, stylo, fontique and taffy
  live in `vendor/` (see each `PATCHES.md`).
- HTTP server moved from axum to actix-web; `data` is parsed with simd-json.
- `width: 100%` tables no longer overflow a padded parent.
- S3 storage is optional; `POST /api/to-s3` answers `STORAGE_NOT_CONFIGURED` without it.

### Removed

- **All observability:** Prometheus `/metrics` (`METRICS_ENABLED`, `METRICS_TOKEN`),
  OpenTelemetry (`OTEL_*`), request metrics and tracing spans. Logs still go to stderr.
- Benchmarks, the load-test script, the `#[ignore]` stress and live-server tests, `docs/`
  and the sample templates.

## [1.1.1] - 2026-10-13

### Fixed

- Docker build works on Railway (cache mounts carry the required service-scoped ids).

### Security

- Runtime image moved to Debian 13 (`trixie-slim`) and installs pending security updates,
  clearing most Docker Scout findings from the bookworm base.

## [1.1.0] - 2026-10-12

### Added

- `GET /health` (no auth) and a Docker `HEALTHCHECK` (`pdf-engine healthcheck`).
- Images: `<img src="data:image/...">` renders; remote images load from `ASSET_ALLOWED_HOSTS`
  (no redirects, no private addresses, size/time limits). Bad images are skipped.
- Fonts reload without restart: `POST /api/fonts/reload`, or `FONT_WATCH=true`.
- Optional observability: Prometheus `/metrics` (`METRICS_ENABLED`, `METRICS_TOKEN`) and
  OpenTelemetry traces (`OTEL_EXPORTER_OTLP_ENDPOINT`).

### Fixed

- `hsl()` / `hsla()` colors render correctly.
- Border colors per side (`border-bottom: 1px solid #ddd` was drawn black).

### Changed

- README shortened; details moved to `docs/`.

## [1.0.0] - 2026-10-12

First versioned release.

### Changed

- Rendering is done in-process by [Fulgur](https://github.com/fulgur-rs/fulgur) (pure Rust).
  PrinceXML is no longer used or installed.
- Page `width`/`height` must be lengths with a unit (`in`, `mm`, `cm`, `pt`, `pc`, `px`);
  anything else returns `400` instead of being passed to the renderer.
- S3 object keys are `pdfs/<timestamp>-<uuid>.pdf`; the timestamp alone could collide under
  concurrent uploads and overwrite another PDF.
- Logs go to stderr.

### Added

- Response compression (gzip / brotli / zstd at level 6) when the client sends `Accept-Encoding`.
- `RENDER_TIMEOUT_SECS` setting (default 45 s; exceeding it returns `408`).
- Multi-arch Docker image (linux/amd64, linux/arm64) with a curated Noto font set
  (Latin weights, Serif, Mono, CJK, Arabic, Hebrew, Thai, color emoji).
- Test suites (functional, chaos, race, poison-pill, CSS), Criterion benchmarks, a load-test
  script and a Makefile (`make test`, `make bench`, `make stress`, `make release`).

### Fixed

- A filename containing control characters (e.g. a newline) crashed the server.
- Templates could read local files into the PDF.

### Performance

- ~970 invoices/s sustained on 10 cores (~1.7 min per 100,000), ~2.5 ms per invoice render;
  the PDF is handed to the response or S3 without copies.

[1.1.0]: https://github.com/Lands-Horizon-Corp/pdf-engine/releases/tag/v1.1.0
[1.0.0]: https://github.com/Lands-Horizon-Corp/pdf-engine/releases/tag/v1.0.0
