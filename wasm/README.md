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

## Options

| `new PDFEngine({ ... })` | |
| --- | --- |
| `compression` | `'fast'` (default: streams stored, about 40 KB for a receipt), `'balanced'`, or `'small'` (about 40% smaller files, about 20% slower). Also per `generate()`. |
| `fonts` | Extra fonts: URLs (fetched once), `Uint8Array`/`ArrayBuffer`, or `File`/`Blob`. |
| `defaultFonts` | Load the bundled Noto Sans Regular and Bold (default `true`). |
| `wasm` | Where the `.wasm` file is, if your bundler does not find it (see below). |
| `fetch` | A custom `fetch` for fonts, the wasm file and server requests. |
| `url`, `token` | Render on a deployed pdf-engine server instead of in the page (see below). |

| `generate({ ... })` | |
| --- | --- |
| `template` | HTML with `{{ value }}` and `{% for x in list %}...{% endfor %}` (HTML-escaped). |
| `data` | Values for the template. |
| `password` | Encrypts the PDF. |
| `width`, `height` | `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default US Letter). |
| `orientation` | `portrait` (default) or `landscape`. |

Also `ping()`, `loadFonts(fonts)` and `close()`.

## Render on your server instead

Pass the address of a deployed pdf-engine (the Docker image) and documents are rendered there. Nothing is
downloaded to the page, and the server's full font set (every script) is available:

```ts
const pdf = new PDFEngine({ url: 'https://pdf.example.com', token: '…' });
const bytes = await pdf.generate({ template, data });          // same call, same errors
await pdf.generate({ template, data, url: 'https://other' });  // or choose per document
```

Set `CORS_ALLOWED_ORIGINS=https://your-app.com` on the server. Anything in browser code is visible to
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
  optimises the module). `generate()` runs on the calling thread, so for large
  documents call it from a Web Worker.
- **Images:** PNG, JPEG, GIF and SVG, with or without a width and height. See [Images](#images).
- **Same engine as the server:** templates, CSS support and error codes match the native engine.
