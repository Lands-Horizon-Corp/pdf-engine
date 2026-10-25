# Getting started

A PDF is made from three things: a **template** (HTML with placeholders), **data** (JSON the placeholders read)
and optional **settings** (page size, password).

```html
<h1>Invoice {{ no }}</h1>
<ul>{% for item in items %}<li>{{ item.name }}: ₱{{ item.price }}</li>{% endfor %}</ul>
```
```json
{ "no": "12345", "items": [{ "name": "Hosting", "price": 1500 }, { "name": "Domain", "price": 450 }] }
```

Pick where it runs. The call is almost the same everywhere.

## In a web page (no build step)

```html
<button id="save">Download PDF</button>
<script type="module">
  import { PDFEngine } from 'https://cdn.jsdelivr.net/npm/@lands-horizon-corp/pdf-engine-wasm@1/index.js';

  const pdf = new PDFEngine();
  document.querySelector('#save').onclick = () =>
    pdf.download({
      template: '<h1>Invoice {{ no }}</h1>',
      data: { no: '12345' },
    }, 'invoice.pdf');
</script>
```

Serve the page over `http://` or `https://` (browsers do not load modules from `file://`).

## In React, Vue, Svelte or Next.js

```sh
npm install @lands-horizon-corp/pdf-engine-wasm
```
```ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';

const pdf = new PDFEngine();                       // create once, reuse
const bytes = await pdf.generate({ template, data });   // a Uint8Array: the PDF
```

Vite needs one line of config. See the guide for your framework: [React](sample-react.md), [Vue](sample-vue.md),
[Svelte](sample-svelte.md), [Next.js](sample-nextjs.md).

## In Electron or Node

```ts
import { PDFEngine } from '@lands-horizon-corp/pdf-engine';
import fs from 'node:fs/promises';

const pdf = new PDFEngine();
await fs.writeFile('invoice.pdf', await pdf.generate({ template, data }));
await pdf.close();
```

See the [Electron guide](sample-electron.md) for the main-process setup and packaging.

## On a server

```sh
docker run -p 6767:6767 -e API_BEARER_TOKEN=secret zalven88/pdf-engine

curl -H "Authorization: Bearer secret" \
  --form-string 'template=<h1>Invoice {{ no }}</h1>' -F 'data={"no":"12345"}' \
  http://localhost:6767/api/to-bytes -o invoice.pdf
```

Details: [HTTP server](http-api.md). Call it from a browser or Electron with the `url` option:
[routing](browser-api.md#choosing-where-documents-are-made).

## From the command line

```sh
pdf-engine print --json '{"printerId":"net://192.168.1.150:9100","template":"<b>Hello</b>"}'
```

Printing and the other commands: [Printing](printing.md), [Command line and IPC](cli-and-ipc.md).

## Next

- Make the document look right: [Templates and CSS](templates.md).
- Something failed: [Errors](errors.md) (every error has a `code`, a readable message and a `hint`).
- Non-Latin text: [Fonts and languages](fonts-and-languages.md).
