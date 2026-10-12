# Performance


| Command | What it measures | Takes |
| --- | --- | --- |
| `make stress` | **Load test:** a release server under concurrent HTTP traffic. Throughput (PDFs/s), latency percentiles and peak memory per scenario, a concurrency sweep, and a 20,000-PDF soak. Use it for capacity planning. | ~10 min |
| `make bench` | **Micro-benchmarks:** single-threaded cost of each step (render per document type, templating, encryption, gzip), measured with Criterion. Use it to compare code or Fulgur versions. | ~3 min |

Results: one machine, 10 CPUs (Linux aarch64, Docker Desktop), via `make docker-stress` / `make docker-bench`. Rerun on your own hardware before capacity planning.

## Load test: what 100,000 PDFs takes on one 10-core server

| Workload | PDFs/s | 100k PDFs |
| --- | --- | --- |
| 20-line invoices, sustained over 20,000 | ~970 | **~1.7 min** |
| Production-like mix (60% invoices / 30% two-page invoices / 10% CJK) | ~640 | ~2.6 min |
| 10-page reports | ~295 | ~5.7 min |
| 200-line (multi-page) invoices | ~200 | ~8.3 min |

## Load test: scenario matrix (concurrency 32)

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

## Load test: concurrency sweep (20-line invoice)

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

## Load test: soak, 20,000 invoices on one server

| PDFs so far | PDFs/s | p99 | Server memory |
| --- | --- | --- | --- |
| 0 (idle) | | | 73 MB |
| 2,000 | 1,007 | 78 ms | 428 MB |
| 6,000 | 852 | 99 ms | 463 MB |
| 10,000 | 938 | 111 ms | 444 MB |
| 14,000 | 866 | 71 ms | 453 MB |
| 20,000 | 957 | 68 ms | 427 MB |

Memory plateaus around 450 MB with no sustained growth (mimalloc keeps freed memory for reuse), and every request succeeded.

## Micro-benchmarks (single thread)

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

## What makes it fast

* **Shared font context:** Fulgur normally rebuilds its font collection from fontconfig for every document. A small patch (`vendor/fulgur`, see [PATCHES.md](vendor/fulgur/PATCHES.md)) builds it once per process, which made renders 4–6× faster. Fonts added after startup need a restart.
* **Curated font set:** every font fontconfig exposes is enumerated, so the image exposes 29 files instead of ~2,300.
* **mimalloc:** rendering allocates heavily; mimalloc is ~10% faster than the system allocator.
* **Zero-copy output:** the PDF is one buffer from the renderer to the socket or S3.

## Production sizing

* **Memory:** budget ~600 MB per instance for invoice-style documents and ~1.5 GB if you render large CJK/emoji-heavy documents. Set a container memory limit with that headroom.
* **Concurrency:** the server caps concurrent renders at one per core; scale out with more instances rather than more concurrency per instance.
* **Timeouts:** keep `RENDER_TIMEOUT_SECS` above your p99 for the largest documents you generate. A render can't be interrupted once started; a timed-out render keeps its concurrency slot until it finishes.
* **Fonts:** keep `FONTCONFIG_FILE` pointed at the curated set; every extra font file adds per-render cost.
* **Compression:** gzip at level 6 saves ~20% on PDFs; clients that don't send `Accept-Encoding` get the raw PDF.

## Running the benchmarks

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
