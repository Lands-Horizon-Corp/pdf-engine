<p align="center"><img src="https://raw.githubusercontent.com/Lands-Horizon-Corp/pdf-engine/master/.public/assets/logo.svg" width="420" alt="pdf-engine: HTML in, PDF out, Rust inside" /></p>

<p align="center">
  <a href="https://www.npmjs.com/package/@lands-horizon-corp/pdf-engine-wasm"><img src="https://img.shields.io/npm/v/@lands-horizon-corp/pdf-engine-wasm.svg?style=flat-square&color=2563EB&label=npm" alt="npm version" /></a>
  <a href="https://github.com/Lands-Horizon-Corp/pdf-engine/actions"><img src="https://img.shields.io/github/actions/workflow/status/Lands-Horizon-Corp/pdf-engine/ci.yml?branch=master&style=flat-square&label=CI" alt="CI status" /></a>
  <img src="https://img.shields.io/badge/browsers-Chromium%20%7C%20Firefox%20%7C%20WebKit-orange?style=flat-square" alt="browsers" />
  <img src="https://img.shields.io/badge/engine-Rust%20%2B%20WebAssembly-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust and WebAssembly" />
  <img src="https://img.shields.io/badge/TypeScript-types%20included-blue?style=flat-square&logo=typescript&logoColor=white" alt="TypeScript types" />
  <img src="https://img.shields.io/badge/no%20Chromium-needed-brightgreen?style=flat-square" alt="no Chromium needed" />
  <a href="https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT license" /></a>
</p>

<p align="center"><strong>Turn HTML templates into real PDFs, in the browser.</strong></p>
<p align="center">The pdf-engine renderer compiled to WebAssembly: no server, no headless Chrome, no network round trip.<br/>
Vector text, not screenshots. The document never leaves the page.</p>

---

## Quick start

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```

```tsx
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

const pdf = new PDFEngine();                       

const doc = {
  template: '<h1>Invoice {{ no }}</h1>',          
  data: { no: '12345' },
  password: '',                                  
};

const bytes = await pdf.generate(doc);           
await pdf.download(doc, 'invoice.pdf');          
await pdf.open(doc);                             
```

Every option, method and error is in the [reference](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/browser-api.md). No bundler? See [Without a build step (CDN)](#without-a-build-step-cdn).
Using React, Vue or Svelte? See [Framework helpers](#react-vue-and-svelte-helpers).

The Node.js/Electron package ([`@lands-horizon-corp/pdf-engine`](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.node/desktop/README.md)) has the same API, so
code moves between them.

---

## Why pdf-engine?

Turning HTML into a PDF on the web has always meant picking your trade-off:

| The usual way | What you give up |
| --- | --- |
| **A server with headless Chrome** (Puppeteer, Playwright) | A Node backend just to make PDFs, and 200 to 370 MB of Chromium to install, deploy and keep running |
| **A screenshot in the browser** (html2pdf.js, jspdf-html2canvas: html2canvas + jsPDF) | The PDF is a picture of the page: text cannot be selected or searched, it blurs when zoomed, and it is slow (70 to 100 ms per page here) |
| **Older engines** (html-pdf on PhantomJS, wkhtmltopdf) | Abandoned: PhantomJS development ended in 2018, `html-pdf` is deprecated on npm, the npm wrapper for `wkhtmltopdf` has not been updated since 2022 and the project is archived |

pdf-engine is a real layout engine (Blitz, Stylo, Taffy, Parley and Krilla, in Rust) compiled to WebAssembly. It runs
**inside the page**: no server, no Chromium, no PhantomJS, no round trip, works offline. The PDF has real vector
text, in 1 to 2 ms. The same engine also runs natively in Electron, Tauri and Docker when you want a server.

| HTML to PDF | pdf-engine | Puppeteer | html2pdf.js | jspdf-html2canvas | html-pdf | wkhtmltopdf |
| --- | :-: | :-: | :-: | :-: | :-: | :-: |
| Takes HTML and CSS (flexbox, grid, tables) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Text stays text (selectable, searchable) | ✅ | ✅ | ❌ picture | ❌ picture | ✅ | ✅ |
| Runs in the browser, no server | ✅ | ❌ | ✅ | ✅ | ❌ | ❌ |
| Nothing heavy to install (no Chromium, PhantomJS or binary) | ✅ | ❌ ~370 MB | ✅ | ✅ | ❌ PhantomJS | ❌ binary |
| Maintained | ✅ | ✅ | ✅ | ✅ | ❌ deprecated | ❌ since 2022 |
| Time for a receipt, in the browser<sup>1</sup> | **0.7 ms** | n/a | 87 ms | 73 ms | n/a | n/a |
| Prints to receipt and label printers (Node package) | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |

<sub>1. Chromium on an Apple M4, median after warm-up. Puppeteer, html-pdf and wkhtmltopdf need a server, so a
browser time does not apply: on a server a receipt takes 4.7 ms with Puppeteer (fastest mode), 356 ms with wkhtmltopdf and
413 ms with html-pdf (both x86 builds under Rosetta on that Mac, so slower than they would be natively), against 0.9 ms
here. Puppeteer renders everything Chrome can, including JavaScript, which pdf-engine does not run; see the
[CSS gaps](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/templates.md#css). pdf-engine is new and has no track record yet.
[Full benchmarks and how to run them](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/README.md#benchmarks).</sub>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/Lands-Horizon-Corp/pdf-engine/master/.public/benchmark/charts/browser-time-dark.svg">
  <img alt="In the browser: time to make one invoice PDF" src="https://raw.githubusercontent.com/Lands-Horizon-Corp/pdf-engine/master/.public/benchmark/charts/browser-time-light.svg">
</picture>

**What you get**

- 📄 **Real PDFs.** Vector text in tens of KB. Flexbox, grid, tables, gradients, transforms, SVG, CSS variables,
  `calc()`, page breaks, multi-column, and `@page` margin boxes.
- ⚡ **Fast.** About 1 ms per receipt and 2 ms per 30-line invoice in the browser.
- 🌍 **Every language.** Bundled Noto Sans covers Latin, Cyrillic, Greek and Vietnamese; add the font you need for
  anything else.
- 🧩 **Templates built in.** `{{ value }}`, loops, conditions, filters, macros, partials; HTML-escaped by default.
- 🧾 **Page numbers, page breaks, QR codes and barcodes**, drawn as vectors.
- 🖼️ **Images.** PNG, JPEG and GIF from URLs, files, `data:` URIs or bytes (and inline `<svg>` elements). A bad image is skipped, never an error.
- 🔀 **Browser or server, your choice.** Several servers with failover, `pick: 'auto'` by size, and a fallback to
  WebAssembly when offline.
- 🔐 **Password encryption.** Set `password` and the PDF is encrypted.
- 🧭 **Errors you can act on.** A stable `code`, the template `line` and a `hint`.
- 🔒 **Safe by default.** Output and execution limits stop runaway templates; images load only from hosts you allow.

---

## Templates

The template language is [minijinja](https://github.com/mitsuhiko/minijinja): `{{ value }}`, `{% for %}`, `{% if %}`,
filters and macros, HTML-escaped by default. Full guide: [Templates and CSS](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/templates.md).

**Partials** (headers, footers, rows, layouts), given to the constructor or to one call:

```js
const pdf = new PDFEngine({
  partials: {
    header: '<h1>{{ store }}</h1><hr>',
    row: '<tr><td>{{ item.name }}</td><td>₱{{ item.price }}</td></tr>',
  },
});
await pdf.generate({
  template: '{% include "header" %}<table>{% for item in items %}{% include "row" %}{% endfor %}</table>',
  data: { store: 'Manila Central', items },
});
```

`{% extends %}` / `{% block %}` make page layouts and `{% import %}` brings in macros.

**Page numbers, headers and footers** with `@page` margin boxes:

```html
<style>
  @page { margin: 20mm;
    @top-center   { content: "{{ company }}"; }
    @bottom-right { content: "Page " counter(page) " of " counter(pages); } }
</style>
```

**Page breaks** with `{% pagebreak %}`, and **vector QR codes and barcodes** with `<barcode>`:

```html
<h1>Invoice</h1> ... {% pagebreak %} <h1>Terms</h1>
<barcode value="https://example.com/pay/{{ no }}" type="qr" width="120" />
<barcode value="{{ no }}" type="code128" width="220" height="60" />   <!-- also code39, ean13, ean8 -->
```

They are drawn as shapes, so they stay sharp at any size and scan from paper or screen.

## Preview while you build a template

```js
const view = await pdf.preview({ template, data }, { target: '#preview' });   // an iframe in #preview
textarea.oninput = () => view.update({ template: textarea.value, data });     // redraw after every edit
view.close();
```

Every element gets a thin outline (`outlines: false` turns it off), so you can see where boxes, flex rows, table
cells and margins really are. The outlines never move anything. Errors reject the promise: show `e.line` and
`e.hint` next to your editor.

## Workers

```js
const pdf = new PDFEngine({ workers: 2 });          // or 'auto': the cores minus one, at most 4
const files = await Promise.all(invoices.map((data) => pdf.generate({ template, data })));
```

Documents are made in Web Workers (worker threads under Node), several at once, and the page stays smooth while a
large document is made. Each worker loads its own copy of the engine (about 10 MB) and its fonts, so use 1 or 2 for a
page and `'auto'` for batches. A single document is not made faster: it still renders on one thread.

## React, Vue and Svelte helpers

One engine per component, with `busy` and `error`. React and Vue are optional peer dependencies; the Svelte store needs nothing.

```tsx
import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/react';

const { download, busy, error } = usePDFEngine();
<button disabled={busy} onClick={() => download({ template, data }, 'invoice.pdf')}>Download</button>
```

```vue
<script setup>
import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/vue';
const { download, busy, error } = usePDFEngine();      // busy and error are refs
</script>
```

```svelte
<script>
  import { onDestroy } from 'svelte';
  import { createPDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/svelte';
  const pdf = createPDFEngine();                       // a store: $pdf.busy, $pdf.error
  onDestroy(pdf.close);
</script>
```

Each returns `generate`, `download`, `open`, `print`, `preview` and `ping`. Guides:
[React](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/sample-react.md), [Vue](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/sample-vue.md),
[Svelte](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/sample-svelte.md), [Next.js](https://github.com/Lands-Horizon-Corp/pdf-engine/blob/master/.public/docs/sample-nextjs.md).

## Images

```js
await pdf.generate({
  template: `<img src="https://cdn.example.com/logo.png" width="120">
             <img src="photo"> <img src="data:image/png;base64,...">`,
  images: { photo: fileInput.files[0] },   // Blob, File, Uint8Array, ArrayBuffer, URL or data: URI
});
```

- `<img src>` can be a `data:` URI, an `https:` or `blob:` URL (fetched by the browser, so the host must send CORS
  headers), or a name you give in `images`.
- An image with no size, or only a width or height, keeps its proportions.
- A missing or broken image is skipped and the document still renders. `onImageError(src, reason)` (on
  `generate()` or the constructor) tells you which; the default is a `console.warn`.
- Up to 200 images of 5 MB each per document.
- Under Node, a bare path or `file:` URL in a template is not read unless you pass `allowLocalFiles: true`, because
  a template is not trusted with your disk. A file you put in `images` is always fine.
- With `url` (rendering on a server), the server loads images by its own rules (`ASSET_ALLOWED_HOSTS`); inline
  them as `data:` URIs to be sure.

## Download, open, print

In a browser page, three helpers take the same options as `generate()`:

```js
await pdf.download({ template, data }, 'invoice.pdf');   // saves the file
await pdf.open({ template, data });                      // shows it in a new tab
await pdf.print({ template, data });                     // opens the browser's print dialog
```

`print()` goes through the browser's own print dialog, so the user picks the printer. Listing printers and
printing straight to thermal or label printers needs the operating system, so it belongs to the Node package
(Electron, Tauri), not to a web page.

## Without a build step (CDN)

A plain HTML page, Vanilla JavaScript or jQuery, needs no Vite, Webpack or Node. Import the package file from
a CDN that serves npm files as they are (jsDelivr or unpkg), from a `<script type="module">`:

```html
<script src="https://code.jquery.com/jquery-3.7.1.min.js"></script>
<button id="pdf">Download PDF</button>

<script type="module">
  import { PDFEngine } from 'https://cdn.jsdelivr.net/npm/@lands-horizon-corp/pdf-engine-wasm@1/index.js';

  const pdf = new PDFEngine();                    // the module and its fonts load next to index.js

  $('#pdf').on('click', async () => {             // jQuery, or document.querySelector(...).addEventListener
    await pdf.download({
      template: `<h1>Invoice {{ no }}</h1>
                 <ul>{% for item in items %}<li>{{ item.name }}: ₱{{ item.price }}</li>{% endfor %}</ul>`,
      data: { no: '12345', items: [{ name: 'Hosting', price: 1500 }, { name: 'Domain', price: 450 }] },
    }, 'invoice.pdf');
  });
</script>
```

- Pin a version (`@1.2.0`) so the files and the module always match. Module scripts run after the page has
  parsed, so there is nothing to wait for; use `await pdf.ping()` to load the engine ahead of the first click.
- The template language is `{{ value }}` and `{% for %}` / `{% if %}` (not Handlebars `{{#each}}`).
- Open the page from a web server (`http://` or `https://`), not as a `file://` file: browsers do not load
  modules or WebAssembly from `file:`.
- The CDN must send `Access-Control-Allow-Origin` (jsDelivr and unpkg do) and `application/wasm` for the `.wasm`.

## Options

| `new PDFEngine({ ... })` | |
| --- | --- |
| `compression` | `'fast'` (default: streams stored, about 40 KB for a receipt), `'balanced'`, or `'small'` (about 40% smaller files, about 20% slower). Also per `generate()`. |
| `fonts` | Extra fonts: URLs (fetched once), `Uint8Array`/`ArrayBuffer`, or `File`/`Blob`. |
| `defaultFonts` | Load the bundled Noto Sans Regular and Bold (default `true`). |
| `wasm` | Where the `.wasm` file is, if your bundler does not find it (see below). |
| `fetch` | A custom `fetch` for fonts, the wasm file and server requests. |
| `url`, `token` | One server, or arrays of servers and tokens, instead of rendering in the page (see below). |
| `pick`, `autoMaxBytes`, `fallback` | Where documents are rendered when there is a `url`: see below. |
| `pruneData` | Send only the `data` keys the template names (default `true`; skipped when the template uses partials). |
| `partials` | Named templates for `{% include %}`, `{% extends %}` and `{% import %}`. Also per `generate()`. |
| `workers` | Make documents in this many workers (a number or `'auto'`). See [Workers](#workers). |
| `onImageError` | `(src, reason)` called for an image that could not be used. |

| `generate({ ... })` | |
| --- | --- |
| `template` | HTML with `{{ value }}` and `{% for x in list %}...{% endfor %}` (HTML-escaped). |
| `data` | Values for the template. |
| `password` | Encrypts the PDF. |
| `width`, `height` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default US Letter). |
| `orientation` | `portrait` (default) or `landscape`. |
| `images` | Images by name for `<img src="name">`. See [Images](#images). |
| `partials` | Partial templates for this document, merged over the constructor's. |

Also `download(options, filename)`, `open(options)`, `print(options)`, `preview(options, view)`, `ping()`,
`loadFonts(fonts)` and `close()`.

## Render on your server instead

Pass the address of a deployed pdf-engine (the Docker image) and documents are rendered there. Nothing is
downloaded to the page, and the server's full font set (every script) is available:

```ts
const pdf = new PDFEngine({ url: 'https://pdf.example.com', token: '…' });
const bytes = await pdf.generate({ template, data });          // same call, same errors
await pdf.generate({ template, data, url: 'https://other' });  // or choose per document
```

### Several servers, and the page as a fallback

```ts
const pdf = new PDFEngine({
  url: ['https://pdf-1.example.com', 'https://pdf-2.example.com'],   // used in turn
  token: ['token-1', 'token-2'],                                     // or one token for all
});

await pdf.generate({
  template, data,
  pick: 'auto',            // 'server' | 'browser' | 'auto'
  autoMaxBytes: '2mb',     // auto: over this size (template + data) goes to a server, the rest renders in the page
});
```

- With a `url` and no `pick`, every document goes to a server. Without a `url`, to the page.
- A server that is unreachable or answers 429, 502, 503 or 504 is skipped for 30 seconds and the next one is
  tried. Template errors and other 4xx answers are not retried: they would fail everywhere.
- With `pick: 'auto'`, a page that is offline or whose servers all fail renders the document itself. Add
  `fallback: true` to get that for `pick: 'server'` too.
- Only the `data` keys the template names are sent (`pruneData: false` turns that off), so a large object you did
  not use stays out of the renderer and off the network.

Set `CORS_ALLOWED_ORIGINS=https://your-app.com` on each server. Anything in browser code is visible to
users, so prefer a same-origin proxy or a short-lived token over the real `API_BEARER_TOKEN`.

## Fonts and languages

A browser has no system fonts, so the engine only knows the fonts you give it. The package bundles
**Noto Sans (Latin, Cyrillic, Greek, Vietnamese)**, about 0.2 MB compressed. For anything else, add a
font that has it:

```ts
const pdf = new PDFEngine({
  fonts: ['/fonts/NotoSansSC-Regular.otf', '/fonts/NotoNaskhArabic-Regular.ttf'],
});
// then use it in the template: <p style="font-family: 'Noto Sans SC'">你好</p>
```

Chinese, Japanese and Korean fonts are large (several MB even compressed); load them only when a
document needs them, for example `await pdf.loadFonts([cjkUrl])` just before rendering.

## Bundlers

The package loads its `.wasm` file and fonts with `new URL('./file', import.meta.url)`, which Vite,
webpack 5, Rollup, Parcel and esbuild all understand. With **Vite**, keep the package out of dependency
pre-bundling and pass the wasm URL explicitly:

```ts
// vite.config.ts
export default defineConfig({
  optimizeDeps: { exclude: ['@lands-horizon-corp/pdf-engine-wasm'] },
});

// app code
import wasmUrl from '@lands-horizon-corp/pdf-engine-wasm/wasm?url';
const pdf = new PDFEngine({ wasm: wasmUrl });
```

Serve `.wasm` files as `application/wasm` (every static host and dev server does by default). No special
headers (SharedArrayBuffer, COOP/COEP) are needed.

## Errors

Every failure is a `PDFEngineError` with a stable `code`, a readable `message`, and where it helps a
`hint`, `status` and (templates) the `line`:

```ts
try {
  await pdf.generate({ template: '<p>\n{% if %}' });
} catch (e) {
  if (e instanceof PDFEngineError && e.code === 'TEMPLATE_ERROR') {
    console.log(e.message, 'line', e.line, e.hint);
  }
}
```

Codes are listed in `index.d.ts`; they match the native engine's (`MISSING_FIELD`, `INVALID_FIELD`,
`TEMPLATE_ERROR`, `TEMPLATE_TOO_EXPENSIVE`, `RENDER_FAILED`, ...), plus `WASM_LOAD_FAILED`,
`FONT_LOAD_FAILED` and `ENGINE_CLOSED` from this package.

## Good to know

- **Size:** about 3.2 MB compressed (brotli), loaded once and cached by the browser.
- **Speed:** the first call also loads the module (about 100 ms); after that a receipt takes about
  1 ms and a 30-line invoice about 2 ms (the first few hundred documents run a little slower while the browser
  optimises the module). `generate()` runs on the calling thread by default; use [`workers`](#workers) to keep a
  page smooth while large documents are made.
- **Images:** PNG, JPEG and GIF, with or without a width and height; inline `<svg>` elements are drawn, but an `<img>` that points at an SVG file is not drawn yet. See [Images](#images).
- **Same engine as the server:** templates, CSS support and error codes match the native engine.
