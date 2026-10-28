# Changelog

All notable changes to this project are documented here. Versions follow
[Semantic Versioning](https://semver.org/); each release is a git tag `vX.Y.Z` and the Docker
image `zalven88/pdf-engine:X.Y.Z` (also `X.Y`, `X` and `latest`).

## [Unreleased]

Pagination, right-to-left text and a Chrome parity score. Every page of a growing corpus (now 188 pages: whole documents,
one-feature probes, right-to-left pages, 84 pagination pages and 16 complex tables) is made by headless Chrome and by pdf-engine,
both drawn by the same rasteriser, and scored. The score is on the [demo page](https://lands-horizon-corp.github.io/pdf-engine/)
and described in [Parity with Chrome](.public/docs/parity.md). The first run scored 80.3; it is now 93.81 over all 188 pages (about 95 without the complex tables, the newest and
hardest group: see Known gaps).

### Added

- **Chrome parity score and health** (`node .public/parity/parity.mjs`, `--check` for CI), with a per-category breakdown and a
  Chrome | pdf-engine | difference picture for every page on the demo.
- **Right-to-left tests** (`tests/bidi.rs`, 38 cases; 27 pass, 11 known gaps are ignored with a reason).
- **Pagination parity pages** in seven categories, and a **complex tables** category (spans, colours, border styles, outlines,
  rounded cells, nested tables, heatmaps, long tables).

### Fixed

- **A page filled exactly lost its last row**, so every long document needed one page too many (279.4 mm was 959.9999 px).
- **Content lost at a page break.** A body with only text and `<br>` lines was clipped to the first page; a block inside a wrapper
  that did not fit the rest of the page, a flex row of images, and a flex item or grid cell taller than a page were clipped at
  the bottom of the page and the rest was dropped.
- **A box that straddles a page break is cut at the break** when its first line of text fits above it (as browsers do), and moves
  whole otherwise. Images, tables, multi-column boxes and `break-inside: avoid` are never cut. The `gap` after a cut item is kept.
- **A gradient or background image continues across the pages** a box is cut over, instead of restarting on each.
- **`dir="rtl"` and `direction: rtl` set the direction of a paragraph** (they had no effect on Latin text, `text-align: start`
  or the order of runs), and a flex row flips in a right-to-left box.
- **An `inline-block` with text sits on its text baseline** (it was about 70 px too low) and its `margin` is honoured.
- **SVG images are drawn:** `<img>` of an SVG (a file or a data URI) and CSS `url()` backgrounds, which were not collected at all.
- **`@page { size }` is honoured** when the request leaves the page size at its default (Letter).
- **Margin boxes** (`@top-center`, `@bottom-right`, ...) are aligned left, centre or right and centred vertically in the margin.

### Known gaps (found by the parity pages, not fixed here)

- Complex tables: `border-collapse: separate` with rounded cells, nested tables, `outline`, and vertical alignment inside `rowspan`
  cells differ from Chrome.
- Right-to-left: `<bdo>` and `unicode-bidi: isolate`, grid, table and list mirroring in an rtl box, `float: inline-start`,
  `padding-inline-*`, and `text-indent` at the start side.
- `outline` is not drawn; `::before`/`::after` with `display: block` lay out side by side; an inline `<span>` with only a space
  paints no background; `column-rule` on a box that must split across columns.
- Long documents drift from Chrome's page breaks because Chrome draws `sans-serif` as Helvetica and the engine as Noto Sans,
  which has looser line spacing.

### Engine internals

The layout engine's vendored copies carry the fixes: `vendor/parley` (new: a paragraph's base direction, the baseline of an inline
box), `vendor/blitz-dom` and `vendor/fulgur` (pagination, backgrounds, margin boxes, SVG images). Each change is in that crate's
`PATCHES.md`. The Node, Electron and Tauri package and the browser package take them from this source when they are built.

## [1.4.1] - 2026-10-27

Memory. One PDF, each library in a fresh process ([the benchmark](.public/benchmark/README.md)), peak memory of the whole
process tree, including the Node process that runs it (about 55 MB):

| | 1.4.0 | 1.4.1 |
| --- | ---: | ---: |
| Idle engine process (Node, Electron, Tauri) | 399 MB | 22 MB |
| A 1-page PDF | 140 MB | 89 MB |
| A 100-page PDF | 167 MB | 162 MB |
| A 1000-page PDF | 1,311 MB (engine alone) | 803 MB |

At 1 page pdf-engine now uses less than wkhtmltopdf (97 MB) and html-pdf (119 MB). From 100 pages on they still use less
(115 and 141 MB at 100 pages, 282 and 323 MB at 1000): pdf-engine builds the whole document in memory, about 0.75 MB a page.

### Changed

- **System fonts are looked at only for a document that needs them** (Node, Electron and Tauri engine). Noto Sans (regular
  and bold) and the math font are built in and used for a document they cover: Latin, Cyrillic, Greek and Vietnamese text,
  currency and math symbols, and `font-family` lists that start with `Noto Sans` or `sans-serif`. Chinese, Japanese,
  Arabic, emoji, another family first in the list, `serif`, `<code>`, `<pre>`, form controls and the `font` shorthand
  read the computer's fonts as before. On macOS that read costs about 200 MB and 100 ms, which every document paid.
  So `sans-serif` is Noto Sans in a plain document on every computer (it was the system's sans-serif), and italic is
  slanted regular. The HTTP server and Docker image are unchanged.
- Fonts are shared, never copied: one copy for every context and document, built-in fonts are not copied at all.

### Fixed (memory)

- A table row or cell kept a second copy of its style (about 470 bytes each).
- Each node's layout cache allocated nine measurement slots (360 bytes) whether or not it was ever measured.
- The node array doubled as the document grew, copying itself with both copies in memory; it is sized once.
- Every container kept a second copy of its child list that nothing read.
- The benchmark measured memory until the program exited, including the PDF checker it loads afterwards.

## [1.4.0] - 2026-10-27

### Added

- **Archival PDF/A:** `pdfA: '2b'` (also `1b`, `2u`, `2a`, `3b`, `3a`; the `pdf_a` form field on the server) makes a PDF/A file, checked
  against the standard as it is made. A document that cannot conform is `INVALID_FIELD` with the reason.
- **`float: left | right` (approximation):** floats sit side by side with the content beside them; no text wrapping under them yet.
- **`pdf.fromElement(element)`** (browser): options for a PDF of an element as it looks on screen, from computed styles,
  images, canvases, inline SVG and the page's `@font-face` fonts.
- **Repeating table headers:** a table's `<thead>` is drawn again at the top of every page the table continues onto.
- **`accessible: true`**: a tagged PDF (structure tree, document language, title), declared PDF/UA when the document has
  headings. In the browser package, the Node package, the HTTP form field `accessible` and the IPC `render` op.

### Changed

- `<details>` triangles and list bullets draw in the browser build (built-in symbol font).
- **A PDF is now print media:** `@media print` rules apply and `@media screen` rules do not.
- The default font stack tries the system's sans-serif before the built-in math font, so on a computer without Noto Sans
  (the Node and Electron package on a Mac or Windows PC) body text and bold use the system font, not the math font.

### Fixed

- **Browser package: a crash no longer leaves the engine unusable.** After a trap (for example, running out of memory)
  every later call failed with "memory access out of bounds" and the promised restart never happened. The WebAssembly
  instance is now replaced, and every other `PDFEngine` on the page rebuilds itself on its next call.
- **Node and Electron package: a field of the wrong type** (`template: 5`, `password: 12345`) was never answered, so the call
  waited until its timeout. It is now `INVALID_FIELD`.
- **Node and Electron package: `loadFonts()` with a path that does not exist** made every later call, and every restart,
  fail with `FONT_ERROR`. The failed list is no longer kept.
- **Node and Electron package: an engine process that failed to start properly** (for example `loadFonts()` with a bad path)
  was left running and lost; the next call started another. It is now stopped.
- **HTML nested more than 512 levels deep** (a template that would overflow the renderer's stack) is a `TEMPLATE_ERROR`
  instead of a crash.
- **Browser package: a template that would produce more than 256 MiB of HTML** is `TEMPLATE_OUTPUT_TOO_LARGE` instead of
  running WebAssembly out of memory (the server's limit is unchanged, 5 GiB, and `TEMPLATE_MAX_OUTPUT_BYTES` still sets it).

### Tests

- New functional suites for the browser package and the Node package: smoke, happy path, sad path, limits and memory
  (out of memory, leaks, concurrency, restarts), and the Tauri sidecar's command line and wire protocol.

## [1.3.0] - 2026-10-26

### Added

- **`rowspan`** in tables (with `colspan`, across pages, and clamped at the last row).
- **`@font-face`**: a document brings its own fonts (TrueType, OpenType, WOFF2) from a `data:` URI, an allowed URL
  or file, under the family name its CSS uses, with `font-weight` and `font-style`. Only that document sees them. In
  the browser package the page fetches URL fonts (`onImageError` reports failures). A font that cannot be loaded is
  skipped; the document is still made.

- **Math** in templates: LaTeX in `<math>...</math>` (inline) and `<math display>...</math>`, or `{{ f|math }}` and
  `{{ f|math(display=true) }}`. Fractions, roots, scripts, sums and integrals with limits, matrices, `cases`, accents,
  Greek and about 150 symbols, drawn as text with a built-in Noto Sans Math (about 50 KB compressed).
- **Markdown** in templates: `{{ text|markdown }}` and `{% filter markdown %}...{% endfilter %}` (CommonMark, tables,
  strikethrough, task lists). Raw HTML in the text is shown as text and unsafe link schemes are removed.

### Fixed

- **`inline-block`, `inline-flex` and similar boxes in a paragraph that wraps onto a second line were not drawn** (a
  badge, a formula, a small box in running text). They are now.

### Changed

- **Node package:** the `printers` constructor option is gone. Printers cannot be known when the engine is created:
  `printers()` lists what the system and USB report, and a network printer is printed to by its address
  (`printerId: 'net://host:9100'`).

## [1.2.1] - 2026-10-26

### Added

- **Live demo** (GitHub Pages): edit a template, change the page size, compression, workers and password, and run a
  speed test in the browser. The package READMEs and docs are rewritten (comparison table, benchmarks, framework
  helpers) and now say which images and SVG features are not drawn yet.
- **Partials** (`partials` option, `partials` form field): named templates for `{% include %}`, `{% extends %}` and
  `{% import %}`. **`{% pagebreak %}`** starts a new page. **`<barcode>`** draws QR codes and Code 128, Code 39, EAN-13
  and EAN-8 barcodes as vector shapes (the tests decode a QR code back from the PDF).

- **Browser package:** `pdf.preview()` (a live preview with outlines around every box), `workers: N` (documents made in
  Web Workers, or worker threads under Node), and `usePDFEngine` for React and Vue and `createPDFEngine` for Svelte
  (`/react`, `/vue`, `/svelte`), all tested in Chromium and WebKit with real apps.

### Fixed

- **Browser package:** `workers` now start when the package is loaded from a CDN (a page cannot start a worker from
  another origin; a same-origin wrapper imports it).
- **Page numbers, headers and footers** (`@page` margin boxes) were never drawn: the engine forced a zero page margin
  that switched them off. The default margin is now a zero-specificity CSS rule the template can override.
- **No text in the browser** with `font-family: Arial, sans-serif`, `sans-serif`, `serif`, `monospace`, `system-ui`
  or any list of names that are not installed: the browser build has only the fonts it is given, so those resolved to
  no font. Generic families now use the supplied font, and a list with no generic family falls back to `sans-serif`.

## [1.2.0] - 2026-10-25

### Documentation

- A full documentation set in `.public/docs/`: getting started, templates and CSS (with the filters that exist and the ones
  that do not), the browser and Node/Electron references, printing, the HTTP server, the command line and IPC protocol,
  fonts and languages, errors, and guides for React, Vue, Svelte, Next.js, Electron and Tauri.

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
