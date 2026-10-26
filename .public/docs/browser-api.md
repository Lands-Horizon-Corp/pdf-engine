# Browser package reference

`@lands-horizon-corp/pdf-engine-wasm`: the engine compiled to WebAssembly. It runs in the page: no server, no
network round trip, and the document never leaves the browser. It also runs under Node (tests, server routes) and can
send documents to your own pdf-engine servers.

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```

```ts
import { PDFEngine, PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';
```

Types ship with the package (`index.d.ts`). Guides: [React](sample-react.md), [Vue](sample-vue.md),
[Svelte](sample-svelte.md), [Next.js](sample-nextjs.md).

## `new PDFEngine(options?)`

Create one engine for the page and reuse it. It loads the WebAssembly module on the first call (about 100 ms), then
renders a receipt in about a millisecond. An array is shorthand for `{ fonts: [...] }`.

| Option | Type | Meaning |
| --- | --- | --- |
| `fonts` | `(string \| URL \| Uint8Array \| ArrayBuffer \| Blob \| File)[]` | Extra fonts, for other scripts. URLs are fetched once. [Fonts](fonts-and-languages.md) |
| `defaultFonts` | `boolean` (`true`) | Load the bundled Noto Sans Regular and Bold (Latin, Cyrillic, Greek, Vietnamese). |
| `wasm` | `string \| URL \| bytes` | Where the `.wasm` file is, if your bundler does not find it. |
| `fetch` | `typeof fetch` | A custom `fetch` for fonts, the `.wasm` file, images and server requests. |
| `workers` | `number \| 'auto'` | Make documents in this many Web Workers instead of on the page's thread. [Workers](#workers) |
| `compression` | `'fast' \| 'balanced' \| 'small'` | `fast` (default) stores streams: quickest, about 40 KB for a receipt. `small` is about 40% smaller and about 20% slower. |
| `url`, `token` | `string \| string[]` | Your pdf-engine server(s) and bearer token(s). [Routing](#choosing-where-documents-are-made) |
| `pick`, `autoMaxBytes`, `fallback` | | Where documents are made. [Routing](#choosing-where-documents-are-made) |
| `partials` | `Record<string, string>` | Partial templates every document can include. |
| `pruneData` | `boolean` (`true`) | Send only the `data` keys the template names. |
| `onImageError` | `(src, reason) => void` | Called for an image that was skipped. Default: `console.warn`. |
| `allowLocalFiles` | `boolean` (`false`) | Node only: let `<img src>` read local files. |

All of the per-document options below can also be set here as defaults.

## `generate(options): Promise<Uint8Array>`

Make a PDF and return its bytes.

| Option | Type | Meaning |
| --- | --- | --- |
| `template` | `string` (required) | HTML with `{{ }}` and `{% %}`. [Templates](templates.md) |
| `data` | any JSON value | Values for the template. `generate<MyData>()` checks its shape. |
| `password` | `string` | Encrypts the PDF (open and owner password). Empty or missing: not encrypted. |
| `accessible` | `boolean` | A tagged, accessible PDF (PDF/UA): a structure tree, the language and the title. [Accessible PDFs](templates.md#accessible-pdfs) |
| `pdfA` | `'1b' \| '2b' \| '2u' \| '2a' \| '3b' \| '3a'` | Archival PDF/A output, checked against the standard. Not with `password`. [Archival PDF/A](templates.md#archival-pdfa) |
| `partials` | `Record<string, string>` | Named templates for `{% include %}`, `{% extends %}`, `{% import %}`; merged over the constructor's. [Partials](templates.md#partials-headers-footers-rows-and-layouts) |
| `width`, `height` | `string` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px`. Default Letter. |
| `orientation` | `'portrait' \| 'landscape'` | Landscape swaps width and height. |
| `images` | `Record<string, bytes \| Blob \| File \| URL \| string>` | Images by the `src` value used in the template. [Images](templates.md#images) |
| `compression`, `pruneData`, `onImageError`, `allowLocalFiles` | | Override the constructor's value for this document. |
| `url`, `token`, `pick`, `autoMaxBytes`, `fallback` | | Override the routing for this document. |

```ts
const bytes = await pdf.generate({
  template: '<h1>Invoice {{ no }}</h1><img src="logo" width="120">',
  data: { no: '12345' },
  images: { logo: fileInput.files[0] },
  width: '210mm', height: '297mm',
  password: 'secret',
});
```

## Using the bytes

| Method | Does |
| --- | --- |
| `download(options, filename = 'document.pdf')` | Makes the PDF and saves it through the browser's download. |
| `open(options)` | Shows the PDF in a new tab. Resolves with the window (or `null` if the browser blocked it). |
| `print(options)` | Opens the browser's print dialog for the PDF. The user picks the printer. |

They take the same options as `generate()` and need a browser page (`NOT_IN_BROWSER` otherwise). Direct printing to
receipt and label printers is in the [desktop package](printing.md); a web page cannot see printers.

Or build the URL yourself:

```ts
const url = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));   // <iframe src={url}>, <a href download>
```

## Preview while you build a template

```ts
const view = await pdf.preview({ template, data }, { target: '#preview' });   // an iframe in #preview (or a floating panel)

textarea.oninput = () => view.update({ template: textarea.value, data });     // redraw after every edit
view.close();
```

`preview()` makes the PDF and shows it in an `<iframe>`. By default every element gets a thin red outline (`outlines:
false` turns it off, a string sets the colour), so you can see where boxes, flex rows, table cells and margins really
are; the outlines never move anything. `update()` calls are queued, only the latest renders, and errors reject the
promise (show them next to your editor: `e.line`, `e.hint`). The PDF viewer shows where pages end.

## Workers

```ts
const pdf = new PDFEngine({ workers: 2 });          // or 'auto': the cores minus one, at most 4
const files = await Promise.all(invoices.map((data) => pdf.generate({ template, data })));
```

With `workers`, documents are made in Web Workers (worker threads under Node): several at once, and the page stays smooth
while a large document is made. Each worker loads its own copy of the engine (about 10 MB) and its fonts, so use 1 or 2 for
a page and `'auto'` for batches. Errors keep their `code` and `line`; `images`, `partials` and `onImageError` work as
usual (Blobs and `File`s are handed to the worker; functions cannot be, so URL images use the worker's own `fetch`).
Documents sent to a server (`url`) do not use workers. `close()` stops them. One document is not made faster by
workers: a single render still runs on one thread.

## React, Vue and Svelte helpers

Small wrappers keep one engine per component and give you `busy` and `error`:

```tsx
import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/react';

function InvoiceButton({ invoice }) {
  const { download, busy, error } = usePDFEngine();
  return <button disabled={busy} onClick={() => download({ template, data: invoice }, 'invoice.pdf')}>{busy ? 'Making PDF…' : 'Download'}</button>;
}
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
<button disabled={$pdf.busy} on:click={() => pdf.download({ template, data }, 'invoice.pdf')}>Download</button>
```

Each returns `generate`, `download`, `open`, `print`, `preview` and `ping` (the same as `PDFEngine`), and the methods still reject, so
you can `try`/`catch` too. The engine is closed when the component goes away. React and Vue are optional peer
dependencies of the package; the Svelte store needs nothing. The hooks are tested in the browser with real React and
Vue apps.

## Other methods

| Method | Does |
| --- | --- |
| `ping(): Promise<{ version }>` | Loads the engine now (instead of on the first `generate()`) and returns its version. Use it to hide the first-call delay. |
| `loadFonts(fonts)` | Use these fonts (besides the default ones) from now on. |
| `close()` | Release the engine's memory. Calls afterwards throw `ENGINE_CLOSED`. |

## `fromElement(element, options?)`: export what is on screen

Makes the options for a PDF of an element as it looks right now, without writing a template. Pass the result to
`generate()`, `download()`, `open()` or `preview()`.

```js
await pdf.download(await pdf.fromElement('#report'), 'report.pdf');
await pdf.download(await pdf.fromElement(el, { margin: 24, password: 'secret' }), 'report.pdf');
```

| Option | |
| --- | --- |
| `margin` | Page margin in px (default 16). |
| `images` | `'inline'` (default) turns images into data, `'link'` keeps their addresses. |
| `fonts` | `false` leaves the page's `@font-face` fonts out. |
| anything else | Goes to `generate()` (`password`, `accessible`, `orientation`, ...). |

What it keeps: computed styles (copied inline), images and `<canvas>` as data, inline SVG, the `@font-face` fonts the
element uses, simple `::before` and `::after` text, form controls as their values, closed `<details>` as their
`<summary>`, and tables, even where a page restyled them to `display: block`. The page is as wide as the element, plus
the margins.

What it does not: scripts, animations, `position: fixed` tricks, `filter`, `float`, and anything the engine does not draw
(see [Templates and CSS](templates.md)). For a report that matters, a template of its own is still the cleaner answer.
Browser pages only (`NOT_IN_BROWSER` elsewhere).

## Choosing where documents are made

By default everything is rendered in the page. Give `url` (one server or an array) and documents go to your
pdf-engine server(s) instead; no WebAssembly is loaded for those.

```ts
const pdf = new PDFEngine({
  url: ['https://pdf-1.example.com', 'https://pdf-2.example.com'],   // used in turn
  token: ['token-1', 'token-2'],                                     // or one token for all
});

await pdf.generate({
  template, data,
  pick: 'auto',          // 'server' | 'browser' | 'auto'
  autoMaxBytes: '2mb',   // auto: over this size (template + data) goes to a server, the rest renders in the page
});
```

| `pick` | Behaviour |
| --- | --- |
| `'server'` (default when there is a `url`) | Always a server. |
| `'browser'` (default without a `url`) | Always the page. |
| `'auto'` | Documents over `autoMaxBytes` go to a server, smaller ones render in the page. If the browser is offline or every server fails, the page renders it. |

- **Several servers:** used in turn (round robin). A server that is unreachable, or answers 429, 502, 503 or 504, is
  skipped for 30 seconds and the next one is tried. Template errors and other 4xx answers are not retried.
- **Tokens:** a single `token` applies to every server; an array lines up with `url`.
- **`fallback: true`** gives the offline/failure fallback to `pick: 'server'` too. It is on by default only for `'auto'`.
- **`autoMaxBytes`:** bytes or text like `'512kb'`, `'2mb'`, `'1.5 GB'`.
- **Security:** a token in browser code is visible to users. Prefer a same-origin proxy or a short-lived token, and set
  `CORS_ALLOWED_ORIGINS` on the server. Images for server-rendered documents follow the server's rules
  (`ASSET_ALLOWED_HOSTS`); inline them as `data:` URIs to be sure.

## Sending only the data a template uses

`pruneData` (on by default) removes top-level `data` keys the template never names, before rendering or sending, so a
large object you did not use costs nothing and secrets you did not mean to use stay out of the request. Every
identifier inside `{{ }}`, `{% %}` and `{# #}` counts as used, so `item.price` keeps `item`. Templates that look keys up
dynamically (`self`, `include`) keep everything. Turn it off with `pruneData: false`.

## Errors

Every failure is a `PDFEngineError` with a stable `code`, a readable `message`, and where it helps `status`, `hint`
and (for templates) the `line`:

```ts
try {
  await pdf.generate({ template: '<p>\n{% if %}' });
} catch (e) {
  if (e instanceof PDFEngineError && e.code === 'TEMPLATE_ERROR') console.log(e.line, e.hint);   // 2 "Check the template syntax..."
}
```

Browser-only codes: `WASM_LOAD_FAILED`, `FONT_LOAD_FAILED`, `ENGINE_CLOSED`, `SERVER_UNREACHABLE`,
`NOT_IN_BROWSER`, `BROWSER_PRINT_FAILED`. The rest are the engine's: [Errors](errors.md).

## Bundlers and the CDN

The package loads its `.wasm` file and fonts with `new URL('./file', import.meta.url)`, which Vite, webpack 5,
Rollup, Parcel and esbuild understand. With **Vite**, keep it out of dependency pre-bundling:

```ts
// vite.config.ts
export default defineConfig({ optimizeDeps: { exclude: ['@lands-horizon-corp/pdf-engine-wasm'] } });
```

Without a bundler, import it from a CDN that serves npm files as they are:
`https://cdn.jsdelivr.net/npm/@lands-horizon-corp/pdf-engine-wasm@1/index.js` (pin a version; serve the page over
`http(s)`). Serve `.wasm` as `application/wasm`; no special headers (SharedArrayBuffer, COOP/COEP) are needed.

## Good to know

- **Speed:** about 1 ms for a receipt and 2 ms for a 30-line invoice after warm-up; the first few hundred documents run a
  little slower while the browser optimises the module. `generate()` runs on the calling thread: for hundreds of pages
  use a Web Worker.
- **Size:** about 3.2 MB of WebAssembly (brotli), loaded once and cached, plus 1 MB of fonts.
- **Same engine as the server:** templates, CSS and error codes match.
