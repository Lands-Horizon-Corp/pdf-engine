# @lands-horizon-corp/pdf-engine-wasm

HTML to PDF **in the browser**, for React, Vue, Svelte and anything else that bundles JavaScript. The
[pdf-engine](https://github.com/Lands-Horizon-Corp/pdf-engine) renderer compiled to WebAssembly: no
server, no network round trip, and the document never leaves the page.

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```

```tsx
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

const pdf = new PDFEngine();                       // create once, reuse

const bytes = await pdf.generate({
  template: '<h1>Invoice {{ no }}</h1>',           // HTML with {{ placeholders }}
  data: { no: '12345' },
  password: '',                                    // set it to encrypt
});

const url = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));
window.open(url);                                  // or <iframe src={url}> or <a href={url} download>
```

The Node.js/Electron package ([`@lands-horizon-corp/pdf-engine`](../node/README.md)) has the same API, so
code moves between them.

## Options

| `new PDFEngine({ ... })` | |
| --- | --- |
| `fonts` | Extra fonts: URLs (fetched once), `Uint8Array`/`ArrayBuffer`, or `File`/`Blob`. |
| `defaultFonts` | Load the bundled Noto Sans Regular and Bold (default `true`). |
| `wasm` | Where the `.wasm` file is, if your bundler does not find it (see below). |
| `fetch` | A custom `fetch` for fonts and the wasm file. |

| `generate({ ... })` | |
| --- | --- |
| `template` | HTML with `{{ value }}` and `{% for x in list %}...{% endfor %}` (HTML-escaped). |
| `data` | Values for the template. |
| `password` | Encrypts the PDF. |
| `width`, `height` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default US Letter). |
| `orientation` | `portrait` (default) or `landscape`. |

Also `ping()`, `loadFonts(fonts)` and `close()`.

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

- **Size:** about 2.4 MB compressed (brotli), loaded once and cached by the browser.
- **Speed:** the first call also loads the module (about 100 ms); after that a receipt takes a few
  milliseconds and a two-page table about 20 ms. `generate()` runs on the calling thread, so for large
  documents call it from a Web Worker.
- **Images:** `data:` URIs only (an `<img src="https://...">` cannot be fetched by the engine). Fetch the
  image yourself and inline it as a data URI.
- **Same engine as the server:** templates, CSS support and error codes match the native engine.
