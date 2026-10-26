# HTTP server

The same engine as a web service, for apps that cannot run it themselves (mobile apps, other servers, back-office
jobs) or that want one shared, fast renderer. It is a small Docker image (multi-arch: amd64 and arm64) with about 150
Noto fonts, so every script renders.

```sh
docker run -p 6767:6767 -e API_BEARER_TOKEN=secret zalven88/pdf-engine
```

```sh
curl -H "Authorization: Bearer secret" \
  --form-string 'template=<h1>Invoice {{ no }}</h1>' -F 'data={"no": 42}' \
  http://localhost:6767/api/to-bytes -o invoice.pdf
```

Use `--form-string` for the template: with `-F`, curl reads a value that starts with `<` as a file name.

Browsers and Electron can call it with the [`url` option](browser-api.md#choosing-where-documents-are-made) instead of
writing HTTP by hand.

## Endpoints

| Method and path | Auth | Does |
| --- | --- | --- |
| `GET /health` | none | `{ "status": "ok", "version": "1.3.0" }` for load balancers and health checks |
| `POST /api/to-bytes` | Bearer | Renders and answers with the PDF (`application/pdf`) |
| `POST /api/to-s3` | Bearer | Renders, uploads to S3-compatible storage and answers with a link |
| `POST /api/fonts/reload` | Bearer | Rediscovers fonts without restarting: `{ "status": "reloaded", "took_ms": 12 }` |

Authentication is the header `Authorization: Bearer <API_BEARER_TOKEN>`; without it, `401 UNAUTHORIZED`.
Other methods on a known path answer `405`.

### Request: `multipart/form-data`

| Field | Required | Meaning |
| --- | --- | --- |
| `template` | yes | HTML with `{{ }}` and `{% %}` ([Templates](templates.md)) |
| `data` | no | A JSON object (as text) for the placeholders |
| `width`, `height` | no | `8.5in`, `210mm`, ... (default Letter) |
| `orientation` | no | `portrait` or `landscape` |
| `password` | no | Encrypts the PDF |
| `partials` | no | A JSON object of name to template, for `{% include %}`, `{% extends %}` and `{% import %}` ([Partials](templates.md#partials-headers-footers-rows-and-layouts)) |
| `filename` | no | The name in the `Content-Disposition` header (default `document.pdf`) |

The whole request may be up to 25 MB (`413 PAYLOAD_TOO_LARGE` beyond that). Other fields are ignored.

### Responses

- **`/api/to-bytes`**: `200` with the PDF as the body, `Content-Type: application/pdf` and
  `Content-Disposition: inline; filename="..."`. Gzip, brotli and zstd are used when the client asks for them.
- **`/api/to-s3`**: `200` with JSON:
  ```json
  { "file_name": "1761366412345-3f2a....pdf", "file_size": 12345, "file_type": "application/pdf",
    "storage_key": "pdfs/1761366412345-3f2a....pdf", "url": "https://...signed link, valid 1 hour...",
    "bucket_name": "pdfs", "status": "completed", "progress": 100 }
  ```
  Needs the `STORAGE_*` settings, otherwise `501 STORAGE_NOT_CONFIGURED`. A failed render leaves nothing in storage.
- **Errors**: a JSON object with a stable `code`:
  ```json
  { "error": "Template rendering failed: syntax error ... (in <string>:3)", "code": "TEMPLATE_ERROR", "status": 400, "line": 3,
    "hint": "Check the template syntax: {{ value }}, {% for x in list %}...{% endfor %}." }
  ```
  Server-side failures are generic (`Internal server error`); details go to the log. [Errors](errors.md)

## Settings

Everything optional is off by default. Set them as environment variables (or in a `.env` file).

| Variable | Default | Meaning |
| --- | --- | --- |
| `API_BEARER_TOKEN` | required | The API token. Change it before exposing the server. |
| `API_PORT` | `6767` | Port to listen on |
| `API_HOST` | `0.0.0.0` | Address to listen on; `127.0.0.1` for a desktop sidecar that only the app should reach |
| `RENDER_TIMEOUT_SECS` | `45` | Per request; `408 TIMEOUT` when exceeded |
| `CORS_ALLOWED_ORIGINS` | none | Browser origins allowed to call the API, comma separated (`*` = any) |
| `STORAGE_URL`, `STORAGE_BUCKET`, `STORAGE_ACCESS_KEY`, `STORAGE_SECRET_KEY` | optional | S3 or MinIO, for `/api/to-s3`. Set all four or none. |
| `STORAGE_REGION` | `us-east-1` | |
| `ASSET_ALLOWED_HOSTS` | off | Hosts `<img>` may load from, e.g. `cdn.example.com,*.assets.io` |
| `ASSET_MAX_BYTES`, `ASSET_TIMEOUT_MS`, `ASSET_MAX_IMAGES` | `5242880`, `5000`, `20` | Per-image limits |
| `ASSET_CACHE_BYTES`, `ASSET_CACHE_TTL_SECS` | `268435456`, `300` | Shared cache of fetched remote images (`0` bytes disables) |
| `ASSET_ALLOW_HTTP`, `ASSET_ALLOW_PRIVATE_IPS` | `false` | Only for trusted internal networks |
| `ASSET_LOCAL_DIR` | off | A folder templates may read image files from. Leave it off on a server that renders other people's templates. |
| `TEMPLATE_MAX_OUTPUT_BYTES`, `TEMPLATE_FUEL` | 5 GiB, `50000000` | Cap on rendered HTML size, and the template's execution budget |
| `FONT_WATCH`, `FONT_WATCH_DIR` | `false`, `/usr/share/fonts/pdf-engine` | Reload fonts when the directory changes |
| `IPC_MAX_REQUEST_BYTES`, `IPC_MAX_IN_FLIGHT` | 256 MiB, 4 per core | IPC mode only: the largest request, and requests held at once |
| `RUST_LOG` | `info` | Log level (written to stderr) |

## Images from the web

`<img src="data:image/...">` always works. `https://` images load only from `ASSET_ALLOWED_HOSTS`: no redirects, no
private or loopback addresses (checked after DNS), images only, with size and time limits. Blocked or broken images
are skipped; the PDF still renders.

## Run it with storage (Docker Compose)

The repository's `docker-compose.yml` starts the server with an S3-compatible store (RustFS) and creates the bucket:
`docker compose up`. Point `STORAGE_*` at AWS S3 or your own MinIO to use those instead.

## Calling it from code

```ts
const form = new FormData();
form.append('template', '<h1>Invoice {{ no }}</h1>');
form.append('data', JSON.stringify({ no: 42 }));
const res = await fetch('https://pdf.example.com/api/to-bytes', {
  method: 'POST', body: form, headers: { Authorization: 'Bearer secret' },
});
if (!res.ok) {
  const body = await res.json();            // { error, code, status, hint, line }
  throw Object.assign(new Error(body.error), body);
}
const pdf = new Uint8Array(await res.arrayBuffer());
```

In browsers, set `CORS_ALLOWED_ORIGINS=https://your-app.com`. A token in browser code is visible to users: use a
same-origin proxy or a short-lived token.
