# pdf-engine documentation

HTML in, PDF out, from one Rust engine. It runs in the browser (WebAssembly), in Electron and Node (as a child
process), in Tauri (as a sidecar), on a server (HTTP, Docker) and from the command line.

## Start here

| I want to... | Read |
| --- | --- |
| Make my first PDF in 5 minutes | [Getting started](getting-started.md) |
| Write templates: values, loops, filters, CSS | [Templates and CSS](templates.md) |
| Use it in a web page or a React/Vue/Svelte/Next.js app | [Browser package reference](browser-api.md), then the [React](sample-react.md), [Vue](sample-vue.md), [Svelte](sample-svelte.md) or [Next.js](sample-nextjs.md) guide |
| Use it in Electron or Node | [Node and Electron package reference](desktop-api.md), then the [Electron guide](sample-electron.md) |
| Use it in Tauri | [Tauri guide](sample-tauri.md) |
| Print receipts, labels and documents | [Printing](printing.md) |
| Run it as a server (Docker, HTTP API, S3) | [HTTP server](http-api.md) |
| Call it from another language or a script | [Command line and IPC protocol](cli-and-ipc.md) |
| Use other scripts and languages (Chinese, Arabic, ...) | [Fonts and languages](fonts-and-languages.md) |
| Understand an error code | [Errors](errors.md) |
| Understand an error code | Every test suite, where it runs, and what is not tested |
| See the problems people have with HTML to PDF, and where pdf-engine stands on each | [Real-world concerns](user-concerns.md) |
| Compare speed with other packages, or measure it myself | [Benchmarks](../benchmark/README.md) |

## The pieces

| Package | For | Install |
| --- | --- | --- |
| `@lands-horizon-corp/pdf-engine-wasm` | Web pages, React, Vue, Svelte, Next.js, Tauri frontends, plain HTML | `npm install @lands-horizon-corp/pdf-engine-wasm` |
| `@lands-horizon-corp/pdf-engine` | Electron, Node, desktop apps: PDFs and printing | `npm install @lands-horizon-corp/pdf-engine` (not published yet) |
| `zalven88/pdf-engine` (Docker) | A server your apps call over HTTP | `docker run -p 6767:6767 -e API_BEARER_TOKEN=secret zalven88/pdf-engine` |
| `pdf-engine` (binary) | Command line, Tauri sidecar, other languages | `cargo build --release` |

All of them take the same thing: a **template** (HTML with `{{ placeholders }}`), some **data** (JSON), and
options such as page size and password; they return the **PDF bytes**.

## What it can do

- **Templates:** values, loops, conditions, filters, macros; output is HTML-escaped. [Templates](templates.md)
- **Layout and CSS:** flexbox, grid, tables, gradients, transforms, SVG, CSS variables, `calc()`, page breaks,
  multi-column, Unicode and right-to-left text. [Templates](templates.md#css)
- **Page setup:** any size (`8.5in`, `210mm`, `80mm` receipts), portrait or landscape, password encryption.
- **Images:** PNG, JPEG and GIF (and inline `<svg>`) from URLs, files, data URIs or bytes you pass in. [Images](templates.md#images)
- **Languages:** practically every script when fonts are available. [Fonts](fonts-and-languages.md)
- **Where it runs:** in the page, on your server, or both with automatic fallback. [Browser reference](browser-api.md#choosing-where-documents-are-made)
- **Printing:** receipt (ESC/POS), label (ZPL) and office printers; USB, network and system queues. [Printing](printing.md)
- **Errors you can act on:** a stable `code`, the template `line` and a `hint`. [Errors](errors.md)
