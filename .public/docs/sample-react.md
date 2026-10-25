# pdf-engine in React

HTML to PDF in the browser with `@lands-horizon-corp/pdf-engine-wasm`. No server: the document is made in
the page and never leaves it.

> **Tested:** the package runs in a React app in Chromium, Firefox and WebKit as part of the repository's
> browser tests (`.node/wasm/e2e`). The component below is the same pattern, with a download button added.

## 1. Install

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```

## 2. Vite config

Keep the package out of Vite's dependency pre-bundling, so it can find its `.wasm` file:

```ts
// vite.config.ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  optimizeDeps: { exclude: ['@lands-horizon-corp/pdf-engine-wasm'] },
});
```

webpack-based setups (Create React App, Next.js) load the `.wasm` file the same way, through `new URL(..., import.meta.url)`, which webpack 5 understands; see [Next.js](sample-nextjs.md).

## 3. One engine for the whole app

Create the engine once, outside your components, and reuse it. It loads the WebAssembly module on first use
(about 100 ms) and then renders a receipt in about a millisecond.

```ts
// src/pdf.ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

export const pdf = new PDFEngine();
```

## 4. A download button

```tsx
// src/InvoiceButton.tsx
import { useState } from 'react';
import { PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';
import { pdf } from './pdf';

interface Invoice {
  no: string;
  customer: string;
  items: { name: string; price: number }[];
}

const template = `
  <style>
    body { font-family: 'Noto Sans', sans-serif; font-size: 12px; padding: 32px; }
    table { width: 100%; border-collapse: collapse; }
    td { padding: 4px 8px; border-bottom: 1px solid #ddd; }
    .r { text-align: right; }
  </style>
  <h1>Invoice {{ no }}</h1>
  <p>Billed to <b>{{ customer }}</b></p>
  <table>
    {% for item in items %}
      <tr><td>{{ item.name }}</td><td class="r">₱{{ item.price }}</td></tr>
    {% endfor %}
  </table>
`;

export function InvoiceButton({ invoice }: { invoice: Invoice }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setBusy(true);
    setError(null);
    try {
      // generate<Invoice>() checks that `data` has the shape you declared.
      await pdf.download<Invoice>({ template, data: invoice }, `invoice-${invoice.no}.pdf`);
    } catch (e) {
      // A bad template says which line: e.code === 'TEMPLATE_ERROR', e.line, e.hint.
      setError(e instanceof PDFEngineError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <button onClick={save} disabled={busy}>{busy ? 'Making PDF…' : 'Download PDF'}</button>
      {error && <p role="alert">{error}</p>}
    </>
  );
}
```

Other ways to use the bytes: `pdf.open(options)` shows the PDF in a new tab, `pdf.print(options)` opens the
browser's print dialog, and `pdf.generate(options)` gives you the `Uint8Array` to put in an `<iframe>`:

```tsx
const bytes = await pdf.generate({ template, data });
setUrl(URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' })));   // <iframe src={url} />
```

## 5. Other languages, images, your own server

Other scripts: add the font (a browser has none). Latin, Cyrillic and Greek are built in.

```ts
export const pdf = new PDFEngine({ fonts: ['/fonts/NotoSansSC-Regular.otf'] });
```

Images: a URL, a `File` from `<input type="file">`, bytes, or `data:` URIs.

```ts
await pdf.download({
  template: '<img src="logo" width="120"><p>{{ name }}</p>',
  data: { name: 'Acme' },
  images: { logo: fileInput.files[0] },
});
```

Or render on your own pdf-engine server (the Docker image), and fall back to the page when it is down:

```ts
export const pdf = new PDFEngine({ url: 'https://pdf.example.com', token: '…', pick: 'auto' });
```

## The hook

`usePDFEngine` keeps one engine per component and gives you `busy` and `error` (it closes the engine when the component
unmounts, StrictMode included):

```tsx
import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/react';

export function InvoiceButton({ invoice }: { invoice: Invoice }) {
  const { download, busy, error } = usePDFEngine();
  return (
    <>
      <button disabled={busy} onClick={() => download({ template, data: invoice }, `invoice-${invoice.no}.pdf`)}>
        {busy ? 'Making PDF…' : 'Download PDF'}
      </button>
      {error && <p role="alert">{error.code}: {error.message}</p>}
    </>
  );
}
```

For many documents use workers: `usePDFEngine({ workers: 'auto' })`. While you design a template, `preview({ template, data }, { target: '#preview' })`
shows it live with outlines around every box ([reference](browser-api.md#preview-while-you-build-a-template)).

## Things to know

- **React StrictMode** renders effects twice in development; that is why the engine is created at module level
  and not in a component.
- **Server components / SSR:** the engine is for the browser. Use it in client code (`'use client'`, `useEffect`,
  event handlers). See [Next.js](sample-nextjs.md).
- **Big documents** (hundreds of pages) run on the page's thread; call `generate()` from a Web Worker to keep
  the UI smooth.
- **Template language:** `{{ value }}`, `{% for x in list %}` and `{% if %}`. Values are HTML-escaped.
- More options and errors: [`.node/wasm/README.md`](../../.node/wasm/README.md).
