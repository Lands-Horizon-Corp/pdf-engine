# pdf-engine in Next.js

Two ways, depending on where the PDF should be made:

| | In the browser (WebAssembly) | On your Next.js server |
| --- | --- | --- |
| Package | `@lands-horizon-corp/pdf-engine-wasm` | the same package under Node, or your pdf-engine Docker server |
| Good for | "Download PDF" buttons, receipts, no server cost | emailing PDFs, background jobs, API routes |

> **Status:** written from the packages' APIs and their webpack/Vite behaviour. The repository tests the browser
> package with Vite (React, Vue, plain JS) and under Node, but has no Next.js app in its tests yet. Run it once in
> your project before relying on it; the notes below say where Next.js needs care.

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```

## A. In the browser (client component)

The engine must run in the browser, so put it in a client component.

```tsx
// app/components/InvoiceButton.tsx
'use client';

import { useState } from 'react';
import { PDFEngine, PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';

// Created once per browser tab. The module loads on the first call, not at import time.
const pdf = new PDFEngine();

interface Invoice {
  no: string;
  customer: string;
  items: { name: string; price: number }[];
}

const template = `
  <style>body { font-family: 'Noto Sans', sans-serif; font-size: 12px; padding: 32px; } td { padding: 4px 8px; }</style>
  <h1>Invoice {{ no }}</h1>
  <p>Billed to <b>{{ customer }}</b></p>
  <table>{% for item in items %}<tr><td>{{ item.name }}</td><td>₱{{ item.price }}</td></tr>{% endfor %}</table>
`;

export function InvoiceButton({ invoice }: { invoice: Invoice }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setBusy(true);
    setError(null);
    try {
      await pdf.download<Invoice>({ template, data: invoice }, `invoice-${invoice.no}.pdf`);
    } catch (e) {
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

Use it from a server component by passing plain data:

```tsx
// app/invoices/[id]/page.tsx
import { InvoiceButton } from '@/app/components/InvoiceButton';

export default async function Page({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const invoice = await loadInvoice(id);          // your data access
  return <InvoiceButton invoice={invoice} />;
}
```

The package loads its `.wasm` file and fonts with `new URL('./file', import.meta.url)`, which webpack 5 (the Next.js
default bundler) and Turbopack handle as static assets. If the `.wasm` file is not found in your setup, copy it to
`public/` and tell the engine where it is:

```ts
// next.config.js: nothing needed. In the component:
const pdf = new PDFEngine({ wasm: '/pdf_engine_wasm_bg.wasm' });   // after copying it from
// node_modules/@lands-horizon-corp/pdf-engine-wasm/dist/pdf_engine_wasm_bg.wasm to public/
```

## B. On the server (API route or server action)

Under Node the package reads its `.wasm` and fonts from `node_modules` itself. Tell Next.js not to bundle it, so
those files are found where they are:

```js
// next.config.js (Next.js 15; in 13 and 14 the key is experimental.serverComponentsExternalPackages)
module.exports = {
  serverExternalPackages: ['@lands-horizon-corp/pdf-engine-wasm'],
};
```

```ts
// app/api/invoice/[id]/route.ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

export const runtime = 'nodejs';                 // not the Edge runtime
const pdf = new PDFEngine();

export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const invoice = await loadInvoice(id);
  const bytes = await pdf.generate({
    template: '<h1>Invoice {{ no }}</h1>',
    data: invoice,
  });
  return new Response(bytes, {
    headers: { 'content-type': 'application/pdf', 'content-disposition': `inline; filename="invoice-${id}.pdf"` },
  });
}
```

For heavy traffic, run the native engine instead: deploy the pdf-engine Docker image and call it with
`new PDFEngine({ url: process.env.PDF_ENGINE_URL, token: process.env.PDF_ENGINE_TOKEN })`, or use the Node
package (`@lands-horizon-corp/pdf-engine`) where you can ship its binary. On an Apple M4 the server handled about 5,000
receipts per second in the benchmarks; WebAssembly in one process, about 1,300.

## Things to know

- **Edge runtime** (`export const runtime = 'edge'`) cannot load WebAssembly from files; use the `nodejs` runtime.
- **Serverless platforms** with small size limits: the package is about 5 MB (3.2 MB of WebAssembly, compressed).
- **Tokens in the browser** are visible to users. If a client component talks to your pdf-engine server, use a
  short-lived token or put a route of your own in front.
- **Template language:** `{{ value }}`, `{% for x in list %}` and `{% if %}`. Not JSX: the string is rendered by
  pdf-engine.
- More options and errors: [`.node/wasm/README.md`](../../.node/wasm/README.md).
