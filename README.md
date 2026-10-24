# pdf-engine

HTML + [Minijinja](https://github.com/mitsuhiko/minijinja) templates → PDF. Rust, rendered
in-process by [Fulgur](https://github.com/fulgur-rs/fulgur) (no browser, no external binaries).
Returns the PDF or uploads it to S3.

~1,000 invoices/s on 10 cores · ~2.5 ms per render · amd64 + arm64 image.

## Run

```bash
docker run -d -p 6767:6767 \
  -e API_BEARER_TOKEN=secret \
  -e STORAGE_URL=http://minio:9000 -e STORAGE_BUCKET=pdfs \
  -e STORAGE_ACCESS_KEY=... -e STORAGE_SECRET_KEY=... \
  zalven88/pdf-engine:1.1.0
```

`STORAGE_*` is required even if you only use `/api/to-bytes`. Inside Docker Compose, use the
service name (e.g. `http://minio:9000`), not `localhost`.

### Docker Compose (no setup needed)

Save as `compose.yaml` and run `docker compose up -d`. It starts pdf-engine with its own
S3-compatible storage ([RustFS](https://github.com/rustfs/rustfs)) and creates the bucket.

```yaml
services:
  pdf-engine:
    image: zalven88/pdf-engine:1.1.0
    ports:
      - "6767:6767"
    environment:
      API_BEARER_TOKEN: change-me
      STORAGE_URL: http://storage:9000
      STORAGE_BUCKET: pdfs
      STORAGE_ACCESS_KEY: pdf-engine
      STORAGE_SECRET_KEY: pdf-engine-secret
    depends_on:
      storage-setup:
        condition: service_completed_successfully

  storage:
    image: rustfs/rustfs
    environment:
      RUSTFS_ACCESS_KEY: pdf-engine
      RUSTFS_SECRET_KEY: pdf-engine-secret
    volumes:
      - storage-data:/data

  storage-setup: # creates the bucket, then exits
    image: amazon/aws-cli
    depends_on:
      - storage
    environment:
      AWS_ACCESS_KEY_ID: pdf-engine
      AWS_SECRET_ACCESS_KEY: pdf-engine-secret
      AWS_DEFAULT_REGION: us-east-1
    entrypoint: >
      sh -c "until aws --endpoint-url http://storage:9000 s3api head-bucket --bucket pdfs 2>/dev/null ||
                   aws --endpoint-url http://storage:9000 s3 mb s3://pdfs; do sleep 1; done"

volumes:
  storage-data:
```

Change `API_BEARER_TOKEN` and the storage keys before exposing it. To use AWS S3 or your own
storage instead, drop `storage`/`storage-setup` and point `STORAGE_*` at it. Presigned URLs
from `/api/to-s3` use `STORAGE_URL`, so they only resolve inside the Compose network unless
storage is reachable at that address.

## API

| Endpoint | Auth | Does |
| --- | --- | --- |
| `POST /api/to-bytes` | Bearer | Returns the PDF |
| `POST /api/to-s3` | Bearer | Uploads, returns JSON with a presigned URL |
| `POST /api/fonts/reload` | Bearer | Reloads fonts without a restart |
| `GET /health` | — | `{"status":"ok","version":"…"}` |
| `GET /metrics` | `METRICS_TOKEN` if set | Prometheus metrics (only when enabled) |

Form fields (`multipart/form-data`):

| Field | Default | |
| --- | --- | --- |
| `template` | required | HTML/Minijinja template |
| `data` | `null` | JSON for the template |
| `width` / `height` | `8.5in` / `11in` | Length in `in`, `mm`, `cm`, `pt`, `pc`, `px` |
| `orientation` | `portrait` | or `landscape` |
| `filename` | `document.pdf` | `/api/to-bytes` download name |
| `password` | — | Encrypts the PDF |

```bash
curl -H "Authorization: Bearer secret" \
  --form-string 'template=<h1>Invoice {{ no }}</h1>' -F 'data={"no": 42}' \
  http://localhost:6767/api/to-bytes -o invoice.pdf
```

Responses are gzip/brotli/zstd compressed (level 6) when the client asks.

## Configuration

| Variable | Default | |
| --- | --- | --- |
| `API_BEARER_TOKEN` | required | API token |
| `STORAGE_URL`, `STORAGE_BUCKET`, `STORAGE_ACCESS_KEY`, `STORAGE_SECRET_KEY` | required | S3 / MinIO |
| `STORAGE_REGION` | `us-east-1` | |
| `API_PORT` | `6767` | |
| `RENDER_TIMEOUT_SECS` | `45` | Per request; `408` when exceeded |
| `ASSET_ALLOWED_HOSTS` | off | Hosts `<img>` may load from, e.g. `cdn.example.com,*.assets.io` |
| `ASSET_MAX_BYTES` / `ASSET_TIMEOUT_MS` / `ASSET_MAX_IMAGES` | `5242880` / `5000` / `20` | Per-image limits |
| `ASSET_CACHE_BYTES` / `ASSET_CACHE_TTL_SECS` | `268435456` / `300` | Shared cache of fetched remote images (`0` bytes disables) |
| `TEMPLATE_MAX_OUTPUT_BYTES` / `TEMPLATE_FUEL` | `5368709120` / `50000000` | Cap on rendered HTML size and template execution budget |
| `ASSET_ALLOW_HTTP` / `ASSET_ALLOW_PRIVATE_IPS` | `false` | Only for trusted internal networks |
| `FONT_WATCH` / `FONT_WATCH_DIR` | `false` / `/usr/share/fonts/pdf-engine` | Reload fonts when the directory changes |
| `METRICS_ENABLED` / `METRICS_TOKEN` | `false` / — | Enable `/metrics`, optionally protected |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | off | Export traces over OTLP/HTTP (`OTEL_SERVICE_NAME`, other `OTEL_*` honoured) |
| `RUST_LOG` | `info` | Log level (stderr) |

Everything optional is off by default; set it in `.env` or the container environment.

## Images

`<img src="data:image/...">` always works. Remote `https://` images load only from
`ASSET_ALLOWED_HOSTS`: no redirects, no private/loopback addresses (checked after DNS), images
only, size and time limits. Blocked or broken images are skipped; the PDF still renders.

## Metrics

Set `METRICS_ENABLED=true` to serve Prometheus metrics at `/metrics` (protect with `METRICS_TOKEN`).

| Metric | Type | Labels |
| --- | --- | --- |
| `pdf_renders_total` | counter | `outcome` = `ok`/`error` |
| `pdf_render_seconds` | summary | — |
| `pdf_template_seconds` | summary | — |
| `pdf_render_permits_in_use` / `pdf_render_permits_total` | gauge | — |
| `pdf_s3_uploads_total` | counter | `outcome` = `ok`/`error` |
| `pdf_s3_upload_seconds` | summary | — |
| `pdf_images_total` | counter | `source`, `outcome` = `loaded`/`ignored`/`blocked`/`failed` |
| `pdf_render_image_fallbacks_total` | counter | — |
| `pdf_font_reloads_total` | counter | — |
| `http_requests_total` | counter | `route`, `method`, `status` |
| `http_request_duration_seconds` | summary | `route`, `method` |

Durations are in seconds and exported as summaries (quantiles 0.5, 0.9, 0.99, plus `_sum` and
`_count`), not histograms: there are no `_bucket` series, so `histogram_quantile()` won't work and
quantiles can't be aggregated across replicas. No process CPU/memory metrics are emitted.

## Fonts and languages

The image ships about 150 Noto fonts via [fonts/fonts.conf](fonts/fonts.conf), so text in
practically any living language renders instead of showing empty boxes:

* **Latin, Greek, Cyrillic** (including Vietnamese and all European accents), plus a serif and a
  monospace design
* **Chinese (simplified and traditional), Japanese, Korean**
* **Arabic, Urdu, Hebrew, Thaana, Syriac**, all right-to-left
* **Indic:** Devanagari (Hindi, Marathi, Nepali), Bengali, Tamil, Telugu, Kannada, Malayalam,
  Gujarati, Gurmukhi (Punjabi), Sinhala, Oriya, and more
* **South-East Asian:** Thai, Lao, Khmer, Myanmar, Javanese, Balinese, Sundanese, and more
* **Others:** Ethiopic, Georgian, Armenian, Tibetan, Mongolian, Cherokee, Canadian Aboriginal,
  N'Ko, Tifinagh, Vai, Adlam, Yi, and more
* **Color emoji, flags, symbols and math**

`sans-serif`/Arial → Noto Sans, `serif` → Noto Serif, `monospace` → Noto Sans Mono. Characters
a font lacks fall back to a font that has them. [tests/fixtures/languages.html](tests/fixtures/languages.html)
shows about 30 scripts at once. To add a font: copy it into `/usr/share/fonts/pdf-engine`, then
`POST /api/fonts/reload` (or run with `FONT_WATCH=true`).

**Running outside Docker** (for example `cargo run` on macOS) uses that machine's own fonts,
which can lack some scripts (on the Mac we tested, Chinese ideographs, ★ and ✓ showed as boxes).
The Docker image is what production renders with, so judge language output there
(`make docker-image`, or the published image).

## CSS

Most document CSS works. Known gaps (e.g. repeating `<thead>`, `float`, `z-index`) and
workarounds: [docs/css.md](docs/css.md).

## Performance

| | |
| --- | --- |
| 20-line invoices, sustained | ~970 PDFs/s (100k in ~1.7 min) |
| Invoice render | ~2.5 ms |
| Memory | ~450 MB steady; plan ~1.5 GB for large CJK documents |

Full results and sizing: [docs/performance.md](docs/performance.md).

## Development

```bash
make test       # functional test suites
make check      # fmt + tests (CI)
make bench      # micro-benchmarks
make stress     # load test (MODE=matrix|sweep|soak)
make help       # everything else
```

No Rust locally? Use the `docker-` variants (`make docker-test`, ...).

## Release

Bump `version` in `Cargo.toml`, add a [CHANGELOG.md](CHANGELOG.md) entry, commit, then
`make release`. It tags `vX.Y.Z` and pushes `zalven88/pdf-engine:X.Y.Z`, `X.Y`, `X` and
`latest` for amd64 and arm64 (built one after the other; also `X.Y.Z-amd64` / `-arm64`).

## License

MIT. Rendering by Fulgur (MIT/Apache-2.0); fonts by [Noto](https://fonts.google.com/noto) (OFL).
