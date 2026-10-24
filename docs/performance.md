# Performance


| Command | What it measures | Takes |
| --- | --- | --- |
| `make stress` | **Load test:** a release server under concurrent HTTP traffic. Throughput (PDFs/s), latency percentiles and peak memory per scenario, a concurrency sweep, and a 20,000-PDF soak. Use it for capacity planning. | ~10 min |
| `make bench` | **Micro-benchmarks:** single-threaded cost of each step (render per document type, templating, encryption, gzip), measured with Criterion. Use it to compare code or Fulgur versions. | ~3 min |

Results: one machine, 10 CPUs (Apple Silicon Mac, macOS, thin-LTO release build), via `make stress` and `make bench`, measured after the changes listed in [What changed](#what-changed). A laptop doing other work gives noisy numbers (single runs vary by 20–40%), and a Linux server will differ. The original code on the same Mac is in [Results of these changes](#results-of-these-changes). Rerun on your own hardware before capacity planning.

## Load test: what 100,000 PDFs takes on one 10-core server

| Workload | PDFs/s | 100k PDFs |
| --- | --- | --- |
| 20-line invoices, sustained over 20,000 | ~1,400 | **~1.2 min** |
| Production-like mix (60% invoices / 30% two-page invoices / 10% CJK) | ~1,520 | ~1.1 min |
| 10-page reports | ~380 | ~4.4 min |
| 200-line (multi-page) invoices | ~310 | ~5.4 min |

## Load test: scenario matrix (concurrency 32)

| Scenario | PDFs/s | p50 | p99 | Peak memory |
| --- | --- | --- | --- | --- |
| Small document | 7,514 | 3.3 ms | 10 ms | 449 MB |
| 20-line invoice | 2,184 | 12 ms | 32 ms | 502 MB |
| 200-line invoice (multi-page) | 237 | 130 ms | 194 ms | 828 MB |
| 10-page report (grid, page breaks) | 459 | 65 ms | 93 ms | 688 MB |
| Password-encrypted PDF | 4,931 | 4.8 ms | 16 ms | 404 MB |
| 500-row table full of CJK + emoji | 74 | 413 ms | 585 ms | 974 MB |
| Mixed traffic | 1,528 | 18 ms | 39 ms | 664 MB |

All 1,900 requests in the matrix returned `200`. A second run gave the same shape: small 8,122, invoice 1,598, 200-line invoice 378, report 298, encrypted 4,640, CJK table 65, mixed 1,520 PDFs/s.

## Load test: concurrency sweep (20-line invoice)

| In-flight requests | PDFs/s | p50 | p99 | Peak memory |
| --- | --- | --- | --- | --- |
| 1 | 343 | 2.8 ms | 3.5 ms | 411 MB |
| 2 | 678 | 2.9 ms | 3.7 ms | 413 MB |
| 5 | 1,368 | 3.4 ms | 7.2 ms | 430 MB |
| 10 | 2,260 | 4.1 ms | 8.7 ms | 447 MB |
| 20 | 1,714 | 10 ms | 37 ms | 472 MB |
| 40 | 1,508 | 23 ms | 60 ms | 514 MB |
| 80 | 1,515 | 45 ms | 80 ms | 564 MB |

Throughput peaks at about one in-flight request per core. Beyond that, requests queue on the render semaphore (one permit per core), so latency rises while memory stays bounded.

## Load test: soak, 20,000 invoices on one server

| PDFs so far | PDFs/s | p99 | Server memory |
| --- | --- | --- | --- |
| 0 (idle) | | | 403 MB |
| 2,000 | 2,063 | 43 ms | 536 MB |
| 6,000 | 1,443 | 56 ms | 591 MB |
| 10,000 | 1,417 | 52 ms | 611 MB |
| 14,000 | 1,396 | 49 ms | 633 MB |
| 20,000 | 1,261 | 125 ms | 645 MB |

Memory plateaus around 640 MB with no sustained growth (mimalloc keeps freed memory for reuse), and every request succeeded. The idle figure is higher than before because the font files are now loaded and mapped at startup.

## Micro-benchmarks (single thread)

| Step | Time | Notes |
| --- | --- | --- |
| Render a tiny document | 0.74 ms | heading + paragraph |
| Render a flexbox layout | 1.2 ms | `tests/fixtures/css/05_flexbox.html` |
| Render a grid layout | 1.1 ms | `06_grid.html` |
| Render an invoice | 1.9 ms | `08_invoice.html` |
| Render Chinese, Japanese and Korean text | 1.5 ms | font fallback across scripts |
| Render a 300-row table | 21 ms | multi-page |
| Fill a template with 500 rows | 0.24 ms | Minijinja |
| Encrypt a PDF (`password`) | 0.30 ms | lopdf, RC4 128-bit |
| Hand the PDF to the response without a password | < 1 µs | zero-copy |
| gzip level 1 / 6 / 9 a small PDF | 68 / 150 / 149 µs | 183 / 83 / 83 MiB/s |

Rendering is where the time goes. Templating, encryption and compression together add well under 1 ms. Level 6 compresses almost as well as level 9 at the same cost. The gzip rows measure the library at fixed levels; the server itself now uses actix-web's default compression level.

## What changed

| Change | Where | Effect |
| --- | --- | --- |
| **Template data is shared, not copied.** The request JSON sits behind an `Arc` and minijinja reads it lazily. | [src/template.rs](../src/template.rs) | ~17% faster at 100,000 rows (72 ms vs 87 ms); no second copy of the data in memory. Level at small sizes. |
| **Template output cap.** Rendering writes into a capped buffer: `TEMPLATE_MAX_OUTPUT_BYTES`, default 5 GiB. | [src/template.rs](../src/template.rs) | A template that explodes (nested loops over a large array) returns a 400 instead of exhausting memory. |
| **Template execution budget.** `TEMPLATE_FUEL` (default 50,000,000) and a recursion limit of 64. | [src/template.rs](../src/template.rs) | A runaway template stops with a 400 instead of holding a render thread. |
| **No HTML clone before rendering.** The job is shared through an `Arc`; it is copied only if a render with images fails and is retried without them. | [src/pdf.rs](../src/pdf.rs) | Removes a full copy of the HTML from every request that contains images. |
| **Remote image cache.** Fetched images are kept in a shared cache (`ASSET_CACHE_BYTES`, default 256 MiB, `0` disables; `ASSET_CACHE_TTL_SECS`, default 300). Concurrent requests for one URL share a single fetch. | [src/assets.rs](../src/assets.rs) | Receipts that repeat a logo skip DNS, the TLS handshake and the download after the first. The scheme, host and private-address checks still run on every call. |
| **Data-URI images skip a copy.** Base64 is decoded straight from the string unless it contains whitespace. | [src/assets.rs](../src/assets.rs) | One less allocation per inline image. |
| **simd-json for the `data` field.** Parsed in place from the raw bytes, with no UTF-8 `String` first. | [src/handlers.rs](../src/handlers.rs) | Faster parsing of large payloads. Not benchmarked. Invalid JSON still becomes `null`. |
| **actix-web instead of axum.** | [src/lib.rs](../src/lib.rs), [src/handlers.rs](../src/handlers.rs), [src/auth.rs](../src/auth.rs), [src/error.rs](../src/error.rs) | Framework overhead is microseconds against milliseconds of rendering, so expect no visible change. Not benchmarked. |
| **Removed `handlebars`, `axum` and `tower-http` from the dependencies.** `axum` remains a dev-dependency for the test servers. | [Cargo.toml](../Cargo.toml) | Smaller build. |
| **Fonts are loaded once per process.** The shared font context starts with every font file already opened and mapped. | [vendor/fulgur](../vendor/fulgur/PATCHES.md) (patch 4) | Renders no longer re-open and re-map the font files. |
| **Font file bytes are copied once per process.** | [vendor/fulgur](../vendor/fulgur/PATCHES.md) (patch 5) | Removes a full copy of every font used (many MB for CJK) from every render. In-process: tiny document 1.0 → 0.74 ms, CJK document 12.8 → 1.5 ms. |
| **No global lock around browser-default style data.** Each document builds its own rules instead of queueing on one process-wide mutex. | [vendor/stylo](../vendor/stylo/PATCHES.md) | The mutex was ~44% of busy time under load. Small documents use ~35% less CPU per request. |
| **Per-render font handles.** Each render gets its own handle over the same mapped font bytes. | [vendor/fontique](../vendor/fontique/PATCHES.md) | Without this, the shared font handle's reference count was hammered by every render thread (10-page reports used ~60% more CPU at 8 concurrent renders). |
| **Dependencies updated.** `cargo update`, plus `lopdf` 0.46 and `lru` 0.19. Rust is 1.99.1. | [Cargo.toml](../Cargo.toml) | Picks up upstream fixes. Not benchmarked. |
| **Three vendored crates:** `fulgur`, `stylo` and `fontique`, each patched and wired in with `[patch.crates-io]`. | [vendor/](../vendor) | See each `PATCHES.md` when upgrading them. |

Behavior differences from the move to actix-web:

* **Compression level:** actix's `Compress` middleware does not allow setting a level, so responses use its defaults instead of level 6. The compression tests still pass.
* **Body limit:** the 25 MB limit is counted across all multipart fields and returns `413` when exceeded.
* **Wrong method:** still `405`; unknown path still `404`.

### Template rendering, measured

Release build, 4 columns per row, `cargo bench --bench template`:

| Rows | Shared view | Deep copy (before) |
| --- | --- | --- |
| 10 | 11.0 µs | 11.7 µs |
| 1,000 | 710 µs | 691 µs |
| 100,000 | 72 ms | 87 ms |

For an invoice, the full render takes about 2.0 ms and a 500-row template about 0.23 ms, so the template is roughly a tenth of the work. Layout and PDF output inside Fulgur dominate, which is why the changes above do not move receipts-per-second much on their own.

### Results of these changes

One Mac (10 CPUs, macOS), thin-LTO release builds, 400 requests per scenario at concurrency 32. The original code is the commit before these changes. Each row shows PDFs/s over two runs of the new code and one fresh run of the original made right after them. This is a laptop doing other work, so single runs vary by 20–40%; read the ratios, not the digits.

| Scenario | Original | New (2 runs) | Peak memory (original → new) |
| --- | --- | --- | --- |
| Small document | 4,931 | 7,514 / 8,122 | 214 → 449 MB |
| 20-line invoice | 1,932 | 2,184 / 1,598 | 403 → 502 MB |
| 200-line invoice (multi-page) | 227 | 237 / 378 | 494 → 828 MB |
| 10-page report | 469 | 459 / 298 | 438 → 688 MB |
| Password-encrypted PDF | 3,306 | 4,931 / 4,640 | 214 → 404 MB |
| 500-row table full of CJK + emoji | 48 | 74 / 65 | 3,702 → 974 MB |
| Mixed traffic | 539 | 1,528 / 1,520 | 3,300 → 664 MB |

More stable than wall-clock throughput is the server CPU time spent per request, which does not depend on what else the machine is doing:

| Document | Original | New |
| --- | --- | --- |
| Small document, 32 in flight | ~1,960 µs | ~1,100–1,200 µs |
| 10-page report, 1 in flight | ~8,000 µs | ~7,800 µs |
| 10-page report, 8 in flight | ~9,500–12,000 µs | ~9,400–9,900 µs |

In plain terms:

* **Receipt-sized documents are the big win:** about 1.5× the throughput and about 40% less CPU per request.
* **Mixed and CJK-heavy traffic improves most** (about 2.8× on the mixed scenario, with p99 latency from ~157 ms to ~39 ms), because it no longer copies and re-maps large fonts on every render.
* **Large single documents are about level.** Their time is in text shaping and layout, which these changes do not touch.
* **Memory goes up for small documents** (about +200 MB per instance): the process now keeps the font files mapped and one copy of each used font's bytes. It goes down a lot for CJK-heavy traffic. Budget accordingly.
* **Soak, 20,000 invoices on one server:** about 1,260–2,060 PDFs/s throughout; memory settles at about 640 MB with no sustained growth, and every request succeeded.

How the picture was found, if you need to repeat it: sample a busy server with `sample <pid>` on a build with symbols (`CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=1`), and compare builds on server CPU per request rather than on wall-clock throughput.

### What was deliberately not done

* **No new `unsafe`.** The only `unsafe` is the pointer into the shared JSON in [src/template.rs](../src/template.rs), which avoids the copy. Our own code has no tight loop where skipping bounds checks or zeroing would help.
* **`panic = "unwind"` is kept.** A panic in the renderer fails one request instead of aborting the server.
* **No Protobuf or Bincode.** It would break every existing caller of the multipart API.

## What makes it fast

* **Shared font context:** Fulgur normally rebuilds its font collection from fontconfig for every document. A small patch (`vendor/fulgur`, see [PATCHES.md](vendor/fulgur/PATCHES.md)) builds it once per process, with every font file already loaded, which made renders 4–6× faster. Fonts added after startup need a restart or a `POST /api/fonts/reload`.
* **Curated font set:** every font fontconfig exposes is enumerated, so the image exposes 29 files instead of ~2,300.
* **mimalloc:** rendering allocates heavily; mimalloc is ~10% faster than the system allocator.
* **Zero-copy output:** the PDF is one buffer from the renderer to the socket or S3.
* **Shared template data and a shared image cache:** see [What changed](#what-changed).

## Production sizing

* **Memory:** budget ~700 MB per instance for invoice-style documents (about 400 MB idle, ~650 MB after a long soak, up to ~830 MB for multi-page invoices) and ~1.2 GB if you render large CJK/emoji-heavy documents. Set a container memory limit with that headroom.
* **Concurrency:** the server caps concurrent renders at one per core; scale out with more instances rather than more concurrency per instance.
* **Timeouts:** keep `RENDER_TIMEOUT_SECS` above your p99 for the largest documents you generate. A render can't be interrupted once started; a timed-out render keeps its concurrency slot until it finishes.
* **Fonts:** keep `FONTCONFIG_FILE` pointed at the curated set; every extra font file adds per-render cost.
* **Compression:** gzip saves ~20% on PDFs (measured at level 6, before the move to actix-web's default level); clients that don't send `Accept-Encoding` get the raw PDF. Compressing `application/pdf` costs CPU for a small saving; if your callers are internal services, consider turning it off.
* **Template limits:** `TEMPLATE_MAX_OUTPUT_BYTES` (default 5 GiB) bounds the HTML a template can produce. Rendering a very large report needs memory for the HTML plus Fulgur's layout, so size the container for your largest report, not just the cap.
* **Image cache:** `ASSET_CACHE_BYTES` adds up to that much memory per instance on top of the figures above.
* **Font memory:** the process keeps the curated font files mapped and one in-memory copy of each font it has used (cleared on a font reload). Measured with the curated set, small-document traffic peaked about 200 MB higher; a larger font set costs proportionally more.

## Running the benchmarks

```bash
make stress                  # load test: matrix + concurrency sweep + 20k soak
make stress MODE=matrix      # one part only: matrix | sweep | soak
make bench                   # micro-benchmarks (local runs also write target/criterion/report/index.html)
cargo bench --bench template # template rendering only: shared view vs deep copy

# Load-test a deployed instance (no memory numbers):
PDF_URL=https://pdf.example.com API_BEARER_TOKEN=... make stress MODE=matrix
```

`make stress` runs [scripts/loadtest.sh](scripts/loadtest.sh). Without `PDF_URL` it builds a release binary and starts a fresh server for every scenario, measuring that server's peak memory. Use the production font set (`FONTCONFIG_FILE`, or `make docker-stress`) for representative numbers. Tunables: `REQUESTS`, `CONCURRENCY`, `SWEEP`, `SOAK_ROUNDS`, `SOAK_BATCH`.

Each scenario is an ignored test in [tests/stress_tests.rs](tests/stress_tests.rs) and can run on its own:

```bash
STRESS_URL=http://localhost:6767 API_BEARER_TOKEN=... STRESS_REQUESTS=2000 STRESS_CONCURRENCY=32 \
  cargo test --release --test stress_tests -- --ignored --nocapture stress_to_bytes_invoice
```

Set `STRESS_MAX_P99_MS` to make a scenario fail when its p99 latency exceeds the limit; that works as a performance gate in CI. Micro-benchmark sources are in [benches/render.rs](benches/render.rs) and [benches/template.rs](benches/template.rs).
