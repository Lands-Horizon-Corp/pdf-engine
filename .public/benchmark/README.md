# Benchmarks

Speed of pdf-engine against the other HTML-to-PDF packages (Puppeteer, dompdf.js, ironpress, html2pdf.js, jspdf-html2canvas, html-pdf, wkhtmltopdf), measured on your own machine. Plain Node
scripts; the charts and tables in the main README come from them. Only speed is measured here.

| Command | What it measures | Time |
| --- | --- | --- |
| `npm run check` | Renders one document of each kind with every library and prints pages, text and size, so you can see they all made the same document | 1 min |
| `npm run bench` | **Time per document**: first call, warm median and 95th percentile, and throughput. In Chromium and WebKit, and in Node. Then redraws the charts | 10 min |
| `npm run scale` | **10,000 documents** per library and kind of document: time to finish | 30–90 min |
| `npm run stats` | Live weekly downloads and last update of each package from the npm registry | seconds |
| `npm run charts` | Redraws `charts/*.svg` and the README tables from `results/*.json` | seconds |

From the repository root: `make bench-setup` (once), `make bench-check`, `make bench` (per document) and
`make bench-scale` (10,000 documents; `BENCH_COUNT`, `BENCH_BUDGET`, `BENCH_LIBS` narrow it).

## Set up

You need Node 20+ and Rust (to build the engine).

```sh
# 1. The engine (native binary, used by the Node benchmarks) and the browser package
cargo build --release                        # at the repository root
(cd ../wasm && npm run build)                # needs the wasm32 target and wasm-bindgen-cli, see .node/wasm/README.md

# 2. This folder
npm install
npx playwright install chromium webkit firefox      # the browsers for the in-browser runs
npx puppeteer browsers install chrome chrome-headless-shell
npm run setup:tools                          # PhantomJS (for html-pdf) and wkhtmltopdf, into tools/
npm run build                                # bundles the browser test page
```

Set `PDF_ENGINE_BIN=/path/to/pdf-engine` to use another binary. For fair numbers use a release build, plug
in the laptop, and close other heavy programs (Chrome especially).

## Run

```sh
npm run check                                # sanity check first
npm run bench                                # per-document speed + charts
npm run scale                                # 10,000 documents each (long)
```

`scale` options:

```sh
node scale.mjs --count 1000 --budget 60               # a quick pass: 1,000 documents, 60 s per run
node scale.mjs --scenarios small,large                # some kinds of document
node scale.mjs --libs pdf-engine,puppeteer-shell      # some libraries (ids: see libs.mjs and scale.mjs)
node scale.mjs --force                                # start over; without it finished runs are kept
```

A long run can be stopped and resumed: results are saved after every run. On a laptop use
`caffeinate -i npm run scale` (macOS) so it does not sleep.

## What is measured

**Documents** ([`scenarios.js`](scenarios.js)). Every library receives the same HTML, so everyone makes the same document:

| Document | Content |
| --- | --- |
| `small` / `receipt` | A receipt: 8 lines, 1 page |
| `invoice` | An invoice: 30 lines, 1 page |
| `large` | One invoice of 300 lines, 7–9 pages |
| `multitable` | A statement of 5 tables of 35 lines, each with a heading and subtotal, 4–6 pages |

Every document has its own number (`#2026-000123`), so nothing can be cached between documents.

**Per document** (`npm run bench`). The first call (from nothing to the first PDF; the median of 5 launches in
Node), then the median and 95th percentile of warm documents, and documents per second with several in
flight. Browsers compile hot code in stages, so each library gets 300 untimed warm-up documents first; the
timed runs are what a busy page or server sees. Libraries that photograph the page (html2pdf.js,
jspdf-html2canvas) take about 100 ms each and get 8 runs.

**10,000 documents** (`npm run scale`). Wall time for the whole run, each library in its own process. A run
stops at 10,000 documents or after the time budget (default 150 s). A library that does not finish is
reported with how many documents it did finish, and its time for 10,000 is **projected** from that rate.
Projected figures are always labelled.

**Correctness.** Every run renders its first document and counts the pages and the selectable text, so a
library that draws a blank page or a picture is visible (`npm run check` shows them all).

## The libraries

| Id | Library | How it is run |
| --- | --- | --- |
| `pdf-engine` | pdf-engine, native, over IPC | 8 documents in flight |
| `wasm` | pdf-engine, WebAssembly | in Chromium, one at a time |
| `puppeteer`, `puppeteer-shell` | Puppeteer with its default headless mode, and with `chrome-headless-shell` | 8 pages in flight |
| `ironpress`, `dompdf` | ironpress (WebAssembly, one reused converter, generic font family), dompdf.js | in Chromium, one at a time |
| `html2pdf`, `jspdf-html2canvas` | html2pdf.js, jspdf-html2canvas | in Chromium, one at a time |
| `html-pdf` | html-pdf (PhantomJS) | 8 in flight |
| `wkhtmltopdf` | wkhtmltopdf 0.12.6 (one process per document) | 8 in flight |

**Notes on PhantomJS and wkhtmltopdf.** Neither has an Apple Silicon build, so on an M-series Mac they run
as x86_64 under Rosetta 2, which slows them down; on x86 Linux or Windows they would be faster. Their numbers
here are therefore an upper bound on their time. `npm run setup:tools` installs both into `tools/`
(or set `WKHTMLTOPDF=/path/to/wkhtmltopdf`). PhantomJS ignores `zoomFactor`, so html-pdf makes 9 pages where
Chrome makes 7 for the 300-line invoice; wkhtmltopdf is run with `--disable-smart-shrinking --zoom 1.3`
to get Chrome's page counts. Both engines are old (PhantomJS is archived since 2018, wkhtmltopdf since 2023)
and do not support modern CSS such as grid; `npm run check` shows what each draws.

**Not measured here:**

- **@adobe/pdfservices-node-sdk** calls Adobe's hosted service (account, network and per-call cost), so
  there is nothing to run locally.
- **puppeteer-core** is the same code as Puppeteer without the browser download.

## Fairness rules

- Only HTML-to-PDF packages are measured: each one receives the same HTML.
- Each competitor gets its best configuration: Puppeteer is measured in both of its headless modes, and
  wkhtmltopdf is tuned until its page count matches Chrome's.
- The in-browser libraries run on the page's one thread; pdf-engine's server and Puppeteer use several cores.
- Numbers from one machine show the ranking and the size of the gaps, not what you will get. Run it on
  your hardware, and report losses as well as wins.

## Adding a library

1. In [`libs.mjs`](libs.mjs), add a driver: `{ name, kind, concurrency, make, render, close }`, where
   `render(handle, doc)` returns the PDF bytes from `doc.html`.
2. Add its id to the `all` list in [`scale.mjs`](scale.mjs) (and the list in `node.mjs`). For the browser,
   add an adapter to `libraries` in [`browser/src/page.js`](browser/src/page.js).
3. `npm run check` shows what it draws; fix the layout until the pages and text match the others.
4. `npm run bench` and `npm run scale`, then `npm run charts`.

## Files

| | |
| --- | --- |
| `scenarios.js`, `libs.mjs`, `tools.mjs`, `setup-tools.mjs` | The documents, the library drivers, and where PhantomJS/wkhtmltopdf are found and installed |
| `node.mjs`, `browser.mjs`, `browser/` | Per-document benchmark (Node, browsers) and the page the browsers load |
| `scale.mjs`, `scale-worker.mjs` | The 10,000-document run and its one-library-per-process worker |
| `sizes.mjs` | What a browser downloads for each library |
| `charts.mjs`, `charts/` | Draws and holds the README charts (light and dark SVG) |
| `results/` | The measured data (JSON) with the machine it came from, committed so charts can be redrawn |
| `out/` | The PDFs the libraries produced (not committed) |

## Troubleshooting

- **Firefox does not start**: some Playwright Firefox builds do not run on the newest macOS. The run skips
  it and says so. It runs in CI on Linux.
- **A library is "not run"**: its reason is printed and saved in `results/scale.json` under `skipped`.
- **Numbers jump around**: close other programs, plug in, and use a release build.
