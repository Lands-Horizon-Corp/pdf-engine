# What developers struggle with, and where pdf-engine stands

HTML to PDF has a reputation for being painful. This page collects the concerns developers actually raise when
they have to build it (taken from a long public discussion, "HTML to PDF is such a pain", on r/webdev, and from the
problems we hit ourselves), and says for each one **what pdf-engine does today, how, and what is still missing**.

It is meant to be kept honest: a gap is listed as a gap, with the plan, and a claim links to the doc or the test that
backs it. If you find a concern that is not here, or one that is marked wrongly, please open an issue.

**Status**

| | Meaning |
| --- | --- |
| ✅ | Solved today |
| 🟡 | Partly: it works, with a limit you should know about |
| 🔴 | Not yet. It is on the plan below |
| ⚪ | Out of scope on purpose, with what to do instead |

Contents: [The "export this page" button](#1-the-export-this-page-button) ·
[Headless browsers: cost, weight, privacy](#2-headless-browsers-cost-weight-privacy) ·
[Layout: pages, breaks, headers, fonts](#3-layout-pages-breaks-headers-fonts) ·
[Quality of the output](#4-quality-of-the-output) · [Speed and scale](#5-speed-and-scale) ·
[Security](#6-security) · [Accessibility and archiving](#7-accessibility-and-archiving) ·
[Do you need a PDF at all?](#8-do-you-need-a-pdf-at-all) · [The plan](#9-the-plan-in-order)

---

## 1. The "export this page" button

The most common story: *an admin dashboard needs an "export as PDF" button*, and the browser extension that does
it ("GoFullPage") makes it look easy while doing it from JavaScript is "practically impossible".

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **"Turn the page the user is looking at into a PDF, like a browser extension does."** | 🔴 | pdf-engine renders an HTML **template plus data**, not a live page. For a report the usual answer is a template of its own (clean, paginated, tested). For "the page as it is", a helper is planned: `pdf.fromElement(element)`, which copies the element with its computed styles, turns images and canvases into data, and renders that. It does not exist yet. |
| **Content drawn by JavaScript: charts on a canvas, framework-rendered widgets.** | ⚪ | No JavaScript runs inside a template, on purpose (it keeps rendering fast and safe). Draw charts as inline `<svg>` (the [demo](https://lands-horizon-corp.github.io/pdf-engine/) "Stress test" does bars, a line and a donut that way), or turn a canvas into a PNG in your page and pass it in `images`. |
| **"Just use `@media print` and the browser's own Print to PDF."** | ✅ | Good advice for a plain "print this page" button, and `pdf.print()` opens exactly that dialog. See [Do you need a PDF at all?](#8-do-you-need-a-pdf-at-all). pdf-engine itself now treats a PDF as print media: `@media print` rules apply and `@media screen` ones do not, so one stylesheet can serve the browser's Print and the PDF. |
| **Pages behind a login: the server cannot fetch them without cookies.** | ✅ | Nothing is fetched by URL. You give the HTML and the data, so there is no session to forward. Images that need a login are passed as bytes in `images`. |
| **Browser extension quality ("it just works").** | 🟡 | The live [playground](https://lands-horizon-corp.github.io/pdf-engine/) lets you check what your template becomes before you write code. |

## 2. Headless browsers: cost, weight, privacy

"Headless is the only way to do it properly, but you have to pay an API for that and expose sensitive data to third
parties", and the self-hosted answers (Puppeteer, Playwright, Gotenberg, `chromium --headless --print-to-pdf`) are
heavy.

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **Paying a hosted API, and sending private data to a third party.** | ✅ | Free and MIT licensed. The browser package makes the PDF **on the user's device**, with nothing uploaded and no network once the page has loaded. The server is a Docker image you run yourself. |
| **Chromium is huge to install, deploy and keep running.** | ✅ | No browser. The browser package is about 3.3 MB compressed; the native engine is one 29 MB binary. A Chromium install is 200 to 370 MB (the [install size chart](../benchmark/README.md) has the numbers). |
| **Starting Chrome for each PDF is slow; keeping it warm is complex.** | ✅ | From nothing to the first PDF takes about 50 to 110 ms natively, against about 110 to 190 ms for Puppeteer's headless shell and 360 to 600 ms for default Chrome (two runs, one on an idle machine and one on a busy one). In a browser page the first PDF takes about 100 to 190 ms, once, and later ones about a millisecond. There is no browser process to pool or restart. |
| **Self-hosting looks complicated (Gotenberg in Docker, Cloud Run, Lambda).** | 🟡 | `docker run -p 6767:6767 zalven88/pdf-engine` is the whole server ([HTTP guide](http-api.md)). The WebAssembly package also runs under Node.js, so it fits a serverless function; we have not yet tested it on a specific platform. |
| **Memory use and scaling a headless browser.** | ✅ | One process, no browser tabs; a render is bounded by a concurrency limit and a time limit ([settings](http-api.md#settings)). |
| **Different languages: PHP (Browsershot, mPDF), Python (WeasyPrint), .NET.** | 🟡 | Any language can call the HTTP server or the CLI ([CLI and IPC](cli-and-ipc.md)). There are official clients only for JavaScript. Short examples for PHP, Python, Go and C# are planned. |

## 3. Layout: pages, breaks, headers, fonts

"Good luck if you need exact control of page breaks but have dynamic content", "running headers and footers weren't
really working", "fonts shifting", "gaps at the end of the page with a big paragraph".

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **Page breaks with dynamic content.** | ✅ | `page-break-before/after/inside`, `break-*`, and `{% pagebreak %}` in templates. Table rows and paragraphs split at line and row edges, never in the middle of a line ([Templates](templates.md#page-breaks)). Orphan and widow control is fixed at two lines (the CSS properties are not read yet). |
| **Gaps at the bottom of a page when something big will not fit.** | 🟡 | A paragraph splits across pages, so it does not leave a gap. A box that cannot split (a picture, `break-inside: avoid`) moves to the next page and leaves the space, as in any browser. |
| **Page numbers, headers and footers, "page X of Y".** | ✅ | `@page` margin boxes with `counter(page)` and `counter(pages)`, with text from your data. The demo's report example shows it. [Templates](templates.md#page-numbers-headers-and-footers) |
| **A table header that repeats on every page.** | 🔴 | Not repeated yet (checked: the header row appears only on the first page). A long table has to repeat the header by hand. This is the first item on the plan. |
| **Floats (`float: left`), text wrapping around an image.** | 🔴 | Not supported: a floated box is laid out as a block. The layout engine has no float support at all, so this is a large change. Plan: first an approximation (side-by-side boxes), then real text wrapping. |
| **Modern CSS, because old engines (wkhtmltopdf, PhantomJS) lack it.** | ✅ | Flexbox, grid, tables (with `rowspan` and `colspan`), CSS variables, `calc()`, multi-column, transforms, opacity, rounded corners, `box-shadow`, gradients, `@font-face`, `@page`. [CSS](templates.md#css) |
| **CSS the engine ignores.** | 🔴 / 🟡 | Checked: `outline` and `filter` (blur) are not drawn; `position: sticky` is ignored. The list is in [Known gaps](templates.md#css). |
| **Fonts look different on Linux and Windows; emoji and symbols break.** | ✅ / 🟡 | The same engine and the same fonts everywhere, so a page breaks in the same place on every machine and in every browser. The browser package bundles Noto Sans (Latin, Cyrillic, Greek, Vietnamese) and Noto Sans Math; the server image carries about 150 Noto fonts. Your own fonts come with `@font-face`. 🟡 Chinese, Japanese, Korean, Arabic, Thai and colour emoji are not bundled in the browser: load the font you need ([Fonts](fonts-and-languages.md)). Optional font packs fetched on demand are planned. |
| **Pixel and sub-pixel differences between machines.** | ✅ | The layout is computed by the engine, not by a platform's text stack, so there is nothing to differ. |
| **Mixing text and mathematics, Markdown content.** | ✅ | `<math>` for LaTeX and `{{ text|markdown }}`, built in ([Math](templates.md#math), [Markdown](templates.md#markdown)). Spacing of formulas is an approximation, not TeX. |

## 4. Quality of the output

"html2canvas + jsPDF ... it's basically just screenshotting and cramming it into a PDF", "scale: 2 or the text looks
like garbage", "the PDFs were 15 MB", "blank spaces where the images should be (CORS)".

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **Blurry text from a screenshot.** | ✅ | Text is drawn as vector text, sharp at any zoom. |
| **Text cannot be selected, searched or copied.** | ✅ | It can. (Libraries that photograph the page cannot do this.) |
| **Huge files.** | ✅ | A receipt is about 25 to 40 KB and a 30-line invoice 25 to 65 KB, depending on the `compression` option (the smaller setting makes files about 40% smaller, a little more slowly). |
| **Images missing because of CORS.** | 🟡 | `data:` URIs and bytes you pass in always work. A URL is fetched by the page, so the host must allow it (CORS); on the server, only hosts you allow are fetched. A broken image is skipped, never an error. |
| **Images the engine does not draw.** | 🔴 | An `<img>` that points at an **SVG file** is not drawn (an inline `<svg>` element is). An image with a percentage CSS width and no height is drawn at size zero (use a `width` attribute). An inline SVG even 1 px wider than its space is not drawn (give it `max-width: 100%`). All three are in the [known gaps](templates.md#css) and on the plan. |
| **Different results between the preview and the PDF.** | ✅ | `pdf.preview()` shows the real PDF with outlines around each box, live as you type. |

## 5. Speed and scale

"Doing thousands of these a day", "1.5 million PDFs in 25 minutes", "slow (WeasyPrint)".

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **Throughput.** | ✅ | In our 10,000-document runs, receipts took about 4 seconds natively and about 13 seconds as WebAssembly, against about 30 seconds for Puppeteer's fastest mode. Large invoices of seven pages took about a minute natively against several minutes. See the [benchmarks](../benchmark/README.md) for every library and the method. These figures were taken with other programs open on the machine; compare the ratios. |
| **Many PDFs at once without exhausting the machine.** | ✅ | A concurrency limit and a per-render time limit protect the server; in the browser, `workers` makes documents in parallel threads. |
| **Comparison with the tools people actually run: Gotenberg, WeasyPrint, Prince, DocRaptor.** | 🔴 | Not measured. They are not npm packages and need other runtimes, so they are not in the benchmark yet. Adding Gotenberg (a Docker service) is planned. We make no claim about them. |

## 6. Security

"wkhtmltopdf has a very old code base: safe for internal pages, not for public ones."

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **Untrusted HTML in an old, unpatched engine.** | ✅ | No JavaScript runs. Output size and run time are capped. Remote images load only from hosts you allow, with no private addresses by default. Values are HTML-escaped; Markdown from a value shows raw HTML as text. The engine is young, so it has no long security history yet. |
| **A third party seeing your data.** | ✅ | See section 2: nothing leaves the page or your own server. |

## 7. Accessibility and archiving

"No one calls out accessibility. PrinceXML can generate accessible PDFs from HTML."

| Concern | Today | How it is handled, and what is missing |
| --- | --- | --- |
| **Tagged (accessible) PDFs, PDF/UA.** | 🟡 | `accessible: true` makes a tagged PDF with a structure tree, the document language and the title, and declares PDF/UA when the document has headings ([Accessible PDFs](templates.md#accessible-pdfs)). We have not run an external checker (veraPDF) or a screen reader over it yet, so we do not claim certification. |
| **Archival PDF/A.** | 🔴 | Not supported yet. |
| **Password protection.** | ✅ | The `password` option encrypts the PDF. |
| **Digital signatures.** | ⚪ | Not in scope; sign the finished PDF with a signing tool. |

## 8. Do you need a PDF at all?

Many answers in the discussion say: *push back*. That is often right, and pdf-engine is not the answer to every export.

| If you need... | Use |
| --- | --- |
| "Print this page" for a person at a screen | The browser's print dialog with `@media print` CSS (or `pdf.print()`, which opens it). |
| A **document** produced from data: invoices, receipts, statements, tickets, labels, certificates, reports | **pdf-engine**: a template and your data, the same result every time, no browser. |
| A snapshot of a page that depends on scripts, complex animations or every CSS feature | A headless browser (Puppeteer, Playwright, Gotenberg). |
| Accessible, print-grade typography for a book or a magazine | PrinceXML (paid) today; see section 7. |
| Numbers someone will process | CSV, Excel or JSON, with the PDF as a copy for people. |

## 9. The plan, in order

Ordered by how often the concern comes up and how much it blocks real documents.

1. **Repeat table headers on every page** (`<thead>`). Common in invoices and reports.
2. **`pdf.fromElement(element)`**: export what is on screen without writing a template.
3. ~~Apply `@media print`~~ **Done:** a PDF is print media.
4. **`float`**, starting with side-by-side boxes, then text wrapping.
5. **Image fixes:** `<img>` of an SVG file, percentage widths without a height, inline SVG wider than its box.
6. **`outline` and `filter`**, and a way to read the CSS `orphans` and `widows` properties.
7. **Tagged (accessible) PDFs**: the option exists; next, check it with veraPDF and a screen reader, then PDF/A.
8. **Optional font packs** for the browser (CJK, Arabic, Thai, emoji), fetched on demand.
9. **Short client examples** for PHP, Python, Go and C#, and a Gotenberg entry in the benchmark.

Items 1 to 7 are the ones that decide whether a given report can move to pdf-engine; items 8 and 9 are about reach.
The release notes ([CHANGELOG](../../CHANGELOG.md)) say when one lands.
