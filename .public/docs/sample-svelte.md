# pdf-engine in Svelte and SvelteKit

HTML to PDF in the browser with `@lands-horizon-corp/pdf-engine-wasm`. No server: the document is made in
the page and never leaves it.

> **Status:** written from the package's API and its Vite setup, which the repository tests with React, Vue and
> plain JavaScript in Chromium, Firefox and WebKit. A Svelte app is not part of those tests yet, so run it once
> in your project before relying on it.

## 1. Install

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```

## 2. Vite config

Svelte and SvelteKit use Vite. Keep the package out of dependency pre-bundling:

```ts
// vite.config.ts
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit()],
  optimizeDeps: { exclude: ['@lands-horizon-corp/pdf-engine-wasm'] },
});
```

## 3. One engine for the whole app

```ts
// src/lib/pdf.ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

export const pdf = new PDFEngine();   // creating it loads nothing; the module loads on the first generate()
```

Importing the package on the server (SvelteKit renders pages there first) is harmless: nothing runs until you
call `generate()`, and you only call it in the browser.

## 4. A download button

```svelte
<!-- src/lib/InvoiceButton.svelte -->
<script lang="ts">
  import { PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';
  import { pdf } from '$lib/pdf';

  interface Invoice {
    no: string;
    customer: string;
    items: { name: string; price: number }[];
  }

  let { invoice }: { invoice: Invoice } = $props();   // Svelte 5. Svelte 4: export let invoice: Invoice;

  const template = `
    <style>
      body { font-family: 'Noto Sans', sans-serif; font-size: 12px; padding: 32px; }
      td { padding: 4px 8px; border-bottom: 1px solid #ddd; }
    </style>
    <h1>Invoice {{ no }}</h1>
    <p>Billed to <b>{{ customer }}</b></p>
    <table>
      {% for item in items %}
        <tr><td>{{ item.name }}</td><td>₱{{ item.price }}</td></tr>
      {% endfor %}
    </table>
  `;

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function save() {
    busy = true;
    error = null;
    try {
      // $state.snapshot gives plain data (no proxies) for the renderer.
      await pdf.download<Invoice>({ template, data: $state.snapshot(invoice) }, `invoice-${invoice.no}.pdf`);
    } catch (e) {
      error = e instanceof PDFEngineError ? `${e.code}: ${e.message}` : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<button onclick={save} disabled={busy}>{busy ? 'Making PDF…' : 'Download PDF'}</button>
{#if error}<p role="alert">{error}</p>{/if}
```

`pdf.open(options)` (new tab), `pdf.print(options)` (the browser's print dialog) and `pdf.generate(options)`
(the `Uint8Array`, for an `<iframe>`) work the same way.

## 5. Other languages, images, your own server

Other scripts: add the font (put it in `static/fonts`):

```ts
export const pdf = new PDFEngine({ fonts: ['/fonts/NotoSansSC-Regular.otf'] });
```

Images (a URL, a `File`, bytes or `data:` URIs):

```ts
await pdf.download({ template: '<img src="logo" width="120">', images: { logo: file } });
```

Your own server, with the page as a fallback:

```ts
export const pdf = new PDFEngine({ url: 'https://pdf.example.com', token: '…', pick: 'auto' });
```

## Things to know

- **SSR:** call the engine from event handlers or `onMount`, not at the top level of a `+page.server.ts` or
  while the server renders a component.
- **Big documents** run on the page's thread; call `generate()` from a Web Worker to keep the UI smooth.
- **Template language:** `{{ value }}`, `{% for x in list %}` and `{% if %}`. Svelte does not touch the string;
  pdf-engine renders it.
- More options and errors: [`.node/wasm/README.md`](../../.node/wasm/README.md).
