

# 📄 PDF Engine: High-Speed Rust + PrinceXML Microservice

A blazingly fast, standalone microservice that converts HTML/CSS to production-ready PDFs. Built with **Rust**, powered by **PrinceXML**, and wrapped in a secure **Axum** web server.

## ⚡️ Purpose: Why this over Gotenberg?

While tools like Gotenberg use Headless Chrome (built for screens), **this engine uses PrinceXML (built for print).** If you are generating invoices, books, reports, or tickets, this is the tool for you.

* **Print-Perfect CSS:** Full support for `@page` rules, CMYK colors, footnotes, and physical margins.
* **Ultra-Lightweight:** Compiles HTML directly to a PDF binary. No massive browser bloat.
* **Built-in Templating:** Send raw JSON and a Minijinja (Jinja2) template in one request.
* **Direct-to-S3:** Bypasses your main app by uploading directly to S3/MinIO and returning a signed URL.

---

## 🛠️ Installation & Quickstart

The engine is available as a pre-built Docker image containing all necessary fonts and dependencies.

### 1. Using Docker CLI

Run this command for quick testing:

```bash
docker run -d \
  --name pdf-engine \
  -p 6767:6767 \
  -e API_BEARER_TOKEN="your_secret_token_here" \
  zalven88/pdf-engine:latest

```

### 2. Using Docker Compose (Recommended)

Add this to your `docker-compose.yml`. Note that inside a Docker network, you must use the service name (e.g., `minio`) for the `STORAGE_URL`.

```yaml
services:
  pdf-engine:
    image: zalven88/pdf-engine:latest
    container_name: pdf-engine
    restart: always
    ports:
      - "6767:6767"
    environment:
      - API_PORT=6767
      - API_BEARER_TOKEN=${PDF_ENGINE_TOKEN}
      - STORAGE_DRIVER=minio
      - STORAGE_ACCESS_KEY=${S3_KEY}
      - STORAGE_SECRET_KEY=${S3_SECRET}
      - STORAGE_BUCKET=my-bucket
      - STORAGE_URL=http://minio:9000  # Internal Docker URL
      - STORAGE_REGION=us-east-1
    depends_on:
      - minio
    networks:
      - app-network

networks:
  app-network:
    driver: bridge

```

---

## 📡 API Usage

All endpoints require a `multipart/form-data` payload and your Bearer token in the headers.

### Form Fields

| Field | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `template` | String | **Yes** | - | The Minijinja/HTML template. |
| `data` | JSON | No | `null` | Data to inject into the template. |
| `width` | String | No | `8.5in` | Page width. |
| `height` | String | No | `11in` | Page height. |
| `filename` | String | No | `document.pdf` | Download name (or S3 key suffix). |
| `password` | String | No | None | Encrypts the PDF with this password. |

### Endpoint 1: Direct to Bytes

Returns the raw binary PDF file.

```bash
curl -X POST http://localhost:6767/api/to-bytes \
  -H "Authorization: Bearer my_secret_token" \
  --form-string "template=<h1>Invoice</h1><p>Billed to: {{ name }}</p>" \
  -F "data={\"name\": \"Zalven\"}" \
  --output my_invoice.pdf

```

### Endpoint 2: Direct to S3

Uploads to S3/MinIO and returns a JSON URL.

```bash
curl -X POST http://localhost:6767/api/to-s3 \
  -H "Authorization: Bearer my_secret_token" \
  --form-string "template=<h1>Welcome {{ name }}</h1>" \
  -F "data={\"name\": \"Zalven\"}"

```

---

## 🔍 Troubleshooting

### Docker Networking (`Connection Refused`)

If your logs show `Connection Refused` when trying to upload to S3/MinIO:

* **Don't use** `127.0.0.1` or `localhost` in your `STORAGE_URL` if the storage is in another container.
* **Do use** the service name defined in your compose file (e.g., `http://minio:9000`).

### PrinceXML Watermark

If you see a "Prince" watermark on your PDFs, you are using the free version. To remove it:

1. Purchase a license from [PrinceXML](https://www.princexml.com/).
2. Follow the license installation instructions (usually placing a `license.dat` file in the engine's path).

---

## ⚖️ License & Acknowledgements

The Rust application code is open-source under the **MIT License**.

**⚠️ Important Note regarding PrinceXML:**
This project utilizes [PrinceXML](https://www.princexml.com/) as its core rendering engine. PrinceXML is a commercial product. Please review their [End User License Agreement](https://www.princexml.com/license/) before deploying this to production.
