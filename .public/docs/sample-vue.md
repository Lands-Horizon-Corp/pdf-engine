# pdf-engine in Vue 3

HTML to PDF in the browser with `@lands-horizon-corp/pdf-engine-wasm`. No server: the document is made in
the page and never leaves it.

> **Tested:** the package runs in a Vue 3 app in Chromium, Firefox and WebKit as part of the repository's
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
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  plugins: [vue()],
  optimizeDeps: { exclude: ['@lands-horizon-corp/pdf-engine-wasm'] },
});
```

## 3. One engine for the whole app

```ts
// src/pdf.ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

export const pdf = new PDFEngine();   // loads the WebAssembly module on first use
```

## 4. A download button

```vue
<!-- src/InvoiceButton.vue -->
<script setup lang="ts">
import { ref } from 'vue';
import { PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';
import { pdf } from './pdf';

interface Invoice {
  no: string;
  customer: string;
  items: { name: string; price: number }[];
}

const props = defineProps<{ invoice: Invoice }>();

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

const busy = ref(false);
const error = ref<string | null>(null);

async function save() {
  busy.value = true;
  error.value = null;
  try {
    await pdf.download<Invoice>({ template, data: props.invoice }, `invoice-${props.invoice.no}.pdf`);
  } catch (e) {
    error.value = e instanceof PDFEngineError ? `${e.code}: ${e.message}` : String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <button :disabled="busy" @click="save">{{ busy ? 'Making PDF…' : 'Download PDF' }}</button>
  <p v-if="error" role="alert">{{ error }}</p>
</template>
```

> `data: props.invoice` is a reactive proxy. That is fine: it is turned into plain JSON when sent to the renderer.
> If you pass a very large reactive object, use `toRaw(props.invoice)` to skip the proxy overhead.

Other ways to use the bytes: `pdf.open(options)` (new tab), `pdf.print(options)` (the browser's print dialog),
or `pdf.generate(options)` for the `Uint8Array`:

```ts
const bytes = await pdf.generate({ template, data });
pdfUrl.value = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));   // <iframe :src="pdfUrl" />
```

## 5. Other languages, images, your own server

Other scripts: add the font (a browser has none):

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

- **Nuxt:** render PDFs in client-only code (`onMounted`, event handlers, a `.client.vue` component or
  `<ClientOnly>`). The engine is for the browser.
- **Big documents** run on the page's thread; call `generate()` from a Web Worker to keep the UI smooth.
- **Template language:** `{{ value }}`, `{% for x in list %}` and `{% if %}`. This is not Vue's template syntax:
  the string is rendered by pdf-engine, not by Vue.
- More options and errors: [`.node/wasm/README.md`](../../.node/wasm/README.md).
