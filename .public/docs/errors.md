# Errors

Every failure has a stable **`code`** to branch on, a readable **message**, and where it helps a **`hint`** (what to do),
an HTTP-style **`status`** and, for template errors, the **`line`**. The shape is the same everywhere: the HTTP server,
the IPC protocol, the command line and both JavaScript packages.

```json
{ "error": "Template rendering failed: syntax error: unexpected end of input (in <string>:3)",
  "code": "TEMPLATE_ERROR", "status": 400, "line": 3,
  "hint": "Check the template syntax: {{ value }}, {% for x in list %}...{% endfor %}." }
```

```ts
try {
  await pdf.generate({ template, data });
} catch (e) {
  if (e instanceof PDFEngineError) {
    switch (e.code) {
      case 'TEMPLATE_ERROR': showEditorError(e.line, e.message); break;
      case 'TIMEOUT': retryLater(); break;
      default: report(e.code, e.hint);
    }
  }
}
```

New codes may be added; existing ones never change meaning. Codes the engine sends are listed in `index.d.ts` of both
packages (a test keeps the lists identical to the Rust engine).

## Engine errors

| Code | Status | Meaning | What to do |
| --- | --- | --- | --- |
| `UNAUTHORIZED` | 401 | The bearer token is missing or wrong (HTTP server) | Send `Authorization: Bearer <API_BEARER_TOKEN>` |
| `MISSING_FIELD` | 400 | A required field is missing, usually `template` (or `printerId` when printing) | Send every required field |
| `INVALID_FIELD` | 400 | A field has a value the engine cannot use: a page size like `wide`, an unknown `orientation`, a bad `printerId`, `paper`, `type` or `format` | Use lengths like `8.5in`, `210mm`, `612pt` and `portrait` or `landscape`; the message names the field |
| `TEMPLATE_ERROR` | 400 | The template has a syntax or runtime error; `line` is the 1-based line | Check `{{ value }}` and `{% for x in list %}...{% endfor %}`; unknown filters and functions also land here (see [Templates](templates.md#filters)) |
| `TEMPLATE_TOO_EXPENSIVE` | 400 | The template ran past its execution budget (`TEMPLATE_FUEL`) | A loop runs too long; check nested loops over large lists |
| `TEMPLATE_OUTPUT_TOO_LARGE` | 413 | The template produced more HTML than `TEMPLATE_MAX_OUTPUT_BYTES` | A loop produces far more than expected |
| `PAYLOAD_TOO_LARGE` | 413 | The request is bigger than the limit (25 MB on the HTTP server) | Send less data |
| `FONT_ERROR` | 400 | A font file or folder could not be used (IPC `load_fonts`) | Pass paths to `.ttf`, `.otf` or `.ttc` files, or to folders that contain them |
| `IPC_PROTOCOL_ERROR` | 400 | A malformed frame or request over IPC | See [IPC protocol](cli-and-ipc.md#ipc-protocol-to-write-your-own-client) |
| `RENDER_FAILED` | 500 | The HTML could not be turned into a PDF | Check the template's HTML and CSS against the [known gaps](templates.md#css) |
| `PDF_PROCESSING_FAILED` | 500 | The PDF could not be finished, for example encrypted | Retry without `password`; report it if it persists |
| `TIMEOUT` | 408 | The render took longer than `RENDER_TIMEOUT_SECS` (default 45) | Simplify the document or raise the limit |
| `PRINTER_UNREACHABLE` | 502 | A network printer did not answer, or a USB device is not connected | Check the address, cable and power; network printers listen on port 9100 |
| `PRINT_FAILED` | 502 | The system print queue or the USB device refused the job | The message has the system's reason; check paper, driver and permissions |
| `STORAGE_NOT_CONFIGURED` | 501 | `/api/to-s3` was called without `STORAGE_*` settings | Set all four `STORAGE_*` variables, or use `/api/to-bytes` |
| `STORAGE_ERROR` | 500 | The object store failed | Check the endpoint and credentials in the server log |
| `IO_ERROR` | 500 | A file or system operation failed | See the server log |
| `INTERNAL_ERROR` | 500 | Something unexpected; the details stay in the log | Report it with the request that caused it |

Messages for `500` errors are deliberately generic ("Internal server error"): storage endpoints and renderer internals
are never sent to callers.

A bad image never causes an error: it is skipped and the document renders without it.

## Errors only the JavaScript packages produce

**Browser package** (`@lands-horizon-corp/pdf-engine-wasm`)

| Code | Meaning | What to do |
| --- | --- | --- |
| `WASM_LOAD_FAILED` | The `.wasm` file could not be fetched or compiled | Serve it as `application/wasm`, or pass `{ wasm: url }` |
| `FONT_LOAD_FAILED` | A font in `fonts` could not be fetched | Check the URLs, or pass the font bytes |
| `SERVER_UNREACHABLE` | Your pdf-engine server did not answer | Check the URL, and `CORS_ALLOWED_ORIGINS` on the server |
| `ENGINE_CLOSED` | A call after `close()` | Create a new `PDFEngine` |
| `NOT_IN_BROWSER` | `download()`, `open()` or `print()` outside a page | Use `generate()` and save the bytes yourself |
| `BROWSER_PRINT_FAILED` | The browser could not load the PDF for printing | Try `open()` and print from the viewer |

**Node and Electron package** (`@lands-horizon-corp/pdf-engine`): `BINARY_NOT_FOUND`, `ENGINE_START_FAILED`,
`ENGINE_EXITED`, `ENGINE_CLOSED`, `CLIENT_TIMEOUT`, `PROTOCOL_MISMATCH`: see the
[reference](desktop-api.md#errors).
