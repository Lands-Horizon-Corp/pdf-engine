# Testing: what is covered, and what is not

Every layer of pdf-engine has tests, and the same kinds of test (smoke, happy path, sad path, limits and memory) exist for the
browser package and the Node package. This page lists them, says where each runs, and says what is **not** tested.

## Run them

```sh
cargo test                                   # the engine: unit and integration tests (244)
cd .node/wasm && npm run build && npm test   # the browser package, under Node (90)
cd .node/desktop && node --test              # the Node, Electron and Tauri package, against the debug binary (79)
cd .node/wasm/e2e && npx vite build && npx playwright test   # the browser package in real browsers (6 per browser set)
make check                                   # formatting + all Rust tests (what CI runs)
```

The Node package tests need the engine built first (`cargo build`). The browser package tests need `npm run build`.

## At a glance

| Layer | Tests | What it proves |
| --- | ---: | --- |
| **Engine, unit** (`src/`) | 73 | Templates and filters, Markdown, math, barcodes, image handling, font-face parsing, raster/printing helpers, error mapping |
| **Engine, integration** (`tests/`) | 171 | The HTTP server, the IPC protocol, the command line and printing, end to end, with real PDFs |
| **Browser package, under Node** (`.node/wasm/test/`) | 90 | The WebAssembly build: every feature, every error, memory, workers, routing between servers |
| **Node, Electron and Tauri package** (`.node/desktop/test/`) | 79 | The native engine through the Node client, and the Tauri sidecar's command line and wire protocol |
| **Browser package, in browsers** (`.node/wasm/e2e/`) | 6 per set | Plain JavaScript, React and Vue apps in Chromium, WebKit (and Firefox on Linux CI) |

## The four kinds of test

The browser package and the Node package each have these files, named the same way:

| File | Kind | What it covers |
| --- | --- | --- |
| `smoke.test.js` | **Smoke** | The package loads and exports its API, the engine starts and reports a version, the smallest document works, `close()` is safe twice (and in Node the process really stops) |
| `happy-path.test.js` | **Happy path** | Every feature with real documents: values, loops, filters, macros, HTML escaping, page sizes and units, orientation, 60-page documents, passwords, tables (colspan, rowspan, repeating header, nested), `float`, flex, grid, columns, gradients, transforms, SVG, `@page` margin boxes, images (bytes, `data:` URI, file), languages (Latin, Cyrillic, Greek, CJK, Arabic, Hebrew, emoji), QR codes and barcodes, Markdown, math, `@font-face`, accessible PDFs, PDF/A (every level), `@media print`, partials |
| `sad-path.test.js` | **Sad path** | Missing and wrongly typed fields, bad page sizes (negative, zero, huge, junk), template syntax errors with `line` and `hint`, runaway templates (loops, recursion, output size, deep nesting), bad barcodes, broken images, fonts, SVG, math and Markdown, hostile CSS (huge sizes, 100,000-column grids, 300 floats), odd data shapes, a missing or wrong binary, the engine killed mid-request, a request that times out, bad font paths, a closed engine, printing without a printer, PDF/A refusals. Every failure is a `PDFEngineError` with a stable `code`, and **the engine still works afterwards** |
| `limits.test.js` | **Limits and memory (OOM)** | A 5,000-row table, 500 pages, a repeating header over 20,000 rows, a very large result and request, the output cap (`TEMPLATE_OUTPUT_TOO_LARGE`) instead of running out of memory, a render that times out, 200 documents at once, failures mixed in with successes, **no leak** over 1,500 documents and over 2,000 errors, many engines started and closed, a crash in one engine not affecting another, restart storms (killing the engine 10 times), and memory ceilings (an idle engine under 100 MB, a 100-page document under 200 MB) |

Two more files exist only for the Node package:

| File | What it covers |
| --- | --- |
| `sidecar.test.js` | What a **Tauri sidecar** (or a client in another language) sees: `pdf-engine ipc` frames (ping, render, errors by id, images in the body, bad lengths, a wrong-typed field answered with the request id, unknown ops, invalid JSON, a stream cut in half, garbage bytes, absurd frame lengths, closing standard input, ESC/POS and ZPL rasters) and the command line (`printers`, `print` to an unreachable printer, a request from standard input or a file, bad JSON, `healthcheck`, an unknown command) |
| `printing.test.js` | Printing to a **fake network printer** (port 9100): the ESC/POS bytes, the cut, `PRINTER_UNREACHABLE`, invalid ids |

And for the browser package: `routing.test.js` and `hybrid.test.js` (several servers, failover, `pick: 'auto'`, falling back to WebAssembly when offline), and `engine.test.js` (the original feature tests: workers, compression, partials, `@font-face`, Markdown, math, accessible PDFs).

## The engine's integration tests (`tests/`)

| Area | Files |
| --- | --- |
| Basics | `smoke.rs`, `happy_path.rs`, `sad_path.rs`, `error_codes.rs`, `node_contract.rs` (versions, error codes and protocol must match between Rust and the Node package) |
| Rendering features | `css.rs`, `table_spans.rs`, `table_header.rs`, `float.rs`, `font_face.rs`, `media_print.rs`, `accessible.rs`, `pdf_a.rs`, `inline_boxes.rs`, `template_tags.rs`, `languages.rs` (needs the production fonts: it runs in the Docker job) |
| Fonts | `light_fonts.rs` (which documents use the built-in fonts and which need the system's), `fonts_reload.rs` |
| Server | `assets.rs` (images and hosts), `compression.rs`, `cors.rs` |
| IPC and printing | `ipc.rs`, `printing.rs` |
| Abuse and robustness | `chaos.rs`, `integration_chaos.rs`, `poison_pill.rs` (a bad document must not poison the engine), `race_conditions.rs`, `zero_absence.rs` |

## Where each runs (CI, on every push)

| Job | Runs |
| --- | --- |
| **Rust** (Ubuntu, macOS) | `cargo fmt --check`, `cargo test` |
| **Linux with the production fonts** (Docker) | The same, inside the image with the curated Noto font set, so every script is checked |
| **Windows** | Build, `cargo test`, the Node package tests, the browser package under Node, and printing to a **virtual receipt printer through the real Windows spooler** |
| **Node client** (Ubuntu and macOS, Node 20 and 22) | `node --test` for the Node package |
| **Browsers** | The browser package in Chromium, Firefox and WebKit on Linux, Chromium and WebKit on macOS, from plain JavaScript, React and Vue |

## What is **not** tested

Be careful to read this before relying on a feature:

- **Physical printers.** Nothing has been printed on a real receipt, label or office printer. Printing is checked against a
  fake network printer (all systems) and a virtual printer (Windows).
- **macOS and Linux `system://` printing** (the CUPS `lp` path), and **USB** direct printing: no test creates a CUPS queue or
  talks to a USB device.
- **Electron and Tauri apps.** The Node package and the Tauri sidecar's protocol are tested; a real Electron or Tauri
  application using them is not (the guides are written, not run).
- **PDF/A and PDF/UA conformance by an external validator.** The files are validated by the PDF writer as they are made, and
  the tests check the markers they carry; veraPDF has not been run on them, and no screen reader has been tried.
- **Firefox on macOS and real Safari or phones.** Firefox runs on Linux only; Edge and Brave are Chromium.
- **Out of memory in the browser at the browser's own limit.** The output cap and recovery after a crash are tested; the tab
  running out of memory on a phone is not.
- **Windows memory and leak tests.** They need `ps` and are skipped on Windows.
- **Speed and memory against other libraries** are benchmarks, not tests: see [the benchmark](../benchmark/README.md).
