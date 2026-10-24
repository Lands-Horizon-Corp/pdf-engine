# Changelog

All notable changes to this project are documented here. Versions follow
[Semantic Versioning](https://semver.org/); each release is a git tag `vX.Y.Z` and the Docker
image `zalven88/pdf-engine:X.Y.Z` (also `X.Y`, `X` and `latest`).

## [Unreleased]

### Added

- **IPC mode** for Node.js and Electron: `pdf-engine ipc` serves render requests over
  stdin/stdout (no HTTP server, no port, no token, no storage settings). PDFs come back as raw
  bytes. Protocol: `src/ipc.rs`.
- **Browser package** in `wasm/` (`@lands-horizon-corp/pdf-engine-wasm`): the same renderer compiled to
  WebAssembly for React, Vue, Svelte and plain JS. No server; bundled Latin fonts, add others with `fonts`.
- **Node.js client** in `node/`: `new PDFEngine({ fonts })` and `generate({ template, data,
  password, ... })`, with TypeScript types. See `node/README.md`. Published as
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
