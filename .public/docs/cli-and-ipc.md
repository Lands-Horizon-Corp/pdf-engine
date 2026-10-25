# Command line and IPC protocol

The `pdf-engine` program has five commands. The first two serve other programs; the next two are one-shot tools for
scripts and Tauri sidecars.

| Command | Does |
| --- | --- |
| `pdf-engine` or `pdf-engine serve` | The [HTTP server](http-api.md) |
| `pdf-engine ipc` | Serves requests over standard input and output (what the Node package uses) |
| `pdf-engine printers` | Lists printers as JSON |
| `pdf-engine print` | Renders a template and prints it |
| `pdf-engine healthcheck` | Exits 0 if the local server answers `GET /health` (used by Docker) |

Build it: `cargo build --release` (the binary is `target/release/pdf-engine`).

## `printers` and `print`

One request, one answer, no server. The request is the JSON of the [print options](printing.md#printoptions), given as:

| Form | |
| --- | --- |
| `--request <file>` | Read from a file (no size limit) |
| `--request -` | Read from standard input |
| `--json '<request>'` | On the command line (about 32,000 characters on Windows) |

```sh
pdf-engine printers
pdf-engine print --json '{"printerId":"system://POS-58","paper":"58mm","template":"<b>Hello</b>"}'
```

Standard output is one line of JSON: `{"ok":true,...}`, or `{"ok":false,"error":{"code":"...","error":"...","status":502,"hint":"..."}}`.
The exit status is 0 for ok and 1 otherwise. Logs go to standard error. [Printing](printing.md)

## IPC protocol (to write your own client)

`pdf-engine ipc` reads requests from standard input and writes answers to standard output. No port, no token. Only
frames go to standard output; logs go to standard error. Closing standard input finishes the requests in progress and
exits.

**Frame**, in both directions:

```text
[u32 big-endian header_len][u32 big-endian body_len][header: JSON, header_len bytes][body: raw bytes, body_len bytes]
```

A request header carries an `id` that is echoed back (answers may arrive in any order, so you can send many at once)
and an `op`:

| `op` | Request fields | Answer |
| --- | --- | --- |
| `render` (default) | `template`, `data`, `width`, `height`, `orientation`, `password`, `images` | Body: the PDF |
| `raster` | `template`, `data`, `height`, `format` (`escpos` or `zpl`), `dots`, `dpi`, `cut`, `feed`, `dither`, `images` | Body: printer commands |
| `printers` | `printers` (extra printers) | Header: `printers` (a list) |
| `print` | `printerId`, `template`, `data`, `type`, `paper`, `language`, `cut`, `feed`, `dither`, `copies`, `width`, `height`, `orientation`, `printers`, `images` | Header: `printerId`, `type`, `language`, `bytes`, `copies` |
| `ping` | | Header: `version`, `protocol` |
| `load_fonts` | `paths` (font files or folders) | Header: `loaded` |
| `reload_fonts` | | `{}` |

**Images** a request supplies travel in its body, back to back, announced in the header:
`"images": [{ "src": "logo", "len": 1234 }, ...]`; the lengths must add up to `body_len` (else `IPC_PROTOCOL_ERROR`).

**Answers:**

```json
{ "id": 1, "ok": true, "op": "render", "contentType": "application/pdf", "bytes": 12345 }
{ "id": 1, "ok": false, "error": { "error": "...", "code": "TEMPLATE_ERROR", "status": 400, "line": 3, "hint": "..." } }
```

A successful `render` or `raster` has the bytes as its body; everything else has an empty body. A broken stream is
reported as a frame with `"id": null` and `"ok": false`, then the process exits with status 2.

**Limits:** a request larger than `IPC_MAX_REQUEST_BYTES` (256 MiB) ends the session with `IPC_PROTOCOL_ERROR`;
`IPC_MAX_IN_FLIGHT` (4 per core) requests are held at once, and reading pauses beyond that, so a fast sender cannot
make the engine hold unbounded memory.

**Version:** `ping` reports `protocol` (currently `1`). It changes whenever the wire format does; check it after starting.

The authoritative description is the comment at the top of `src/ipc.rs`; the Node client in `.node/desktop/index.js` is a
complete example.
