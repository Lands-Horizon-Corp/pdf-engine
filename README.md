# 📄 PDF Engine: High-Speed Rust + PrinceXML Microservice

A blazingly fast, standalone microservice that converts HTML/CSS to production-ready PDFs. Built with **Rust**, powered by **PrinceXML**, and wrapped in a secure **Axum** web server.

## ⚡️ Purpose: Why this over Gotenberg?

While tools like Gotenberg use Headless Chrome (which is built for screens), **this engine uses PrinceXML (which is built for print).** If you are generating invoices, books, reports, or tickets, this is the tool for you.

* **Print-Perfect CSS:** Full support for `@page` rules, CMYK colors, footnotes, and physical margins.
* **Ultra-Lightweight:** Compiles HTML directly to a PDF binary. No massive V8 JavaScript engine or browser bloat.
* **Built-in Templating:** Send raw JSON and a Minijinja (Jinja2) template in one request.
* **Direct-to-S3:** Bypasses your main app. Generates the PDF and uploads it straight to AWS/MinIO, handing you back a signed URL.

*(Note: Because this is optimized for print, it will not execute client-side JavaScript like React or Chart.js).*

---

## 🛠️ Quickstart (Docker)

The easiest way to run the engine is via Docker. It comes pre-packaged with PrinceXML 16.2 and standard fonts.

**1. Create a `.env` file in your directory:**

```dotenv
API_PORT=6767
API_BEARER_TOKEN=my_super_secret_token_123

# Storage Configuration (For the /api/to-s3 endpoint)
STORAGE_API_PORT=9000
STORAGE_DRIVER=minio
STORAGE_ACCESS_KEY=your_access_key
STORAGE_SECRET_KEY=your_secret_key
STORAGE_BUCKET=your-bucket-name
STORAGE_URL=http://127.0.0.1:${STORAGE_API_PORT}
STORAGE_REGION=us-east-1

```

**2. Spin it up:**

```bash
docker-compose up -d

```

*(To run locally without Docker, install PrinceXML v16.2 on your machine and run `cargo run --release`)*.

---

## 📡 API Usage

All endpoints require a `multipart/form-data` payload and your Bearer token in the headers.

### Form Fields

Both endpoints accept the following form fields:

| Field | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `template` | String | **Yes** | - | The Minijinja/HTML template. |
| `data` | JSON | No | `null` | Data to inject into the template. |
| `width` | String | No | `8.5in` | Page width. |
| `height` | String | No | `11in` | Page height. |
| `filename` | String | No | `document.pdf` | Download name (or S3 key suffix). |
| `password` | String | No | None | Encrypts the PDF with this password. |

### Endpoint 1: Direct to Bytes

Renders the PDF and streams the raw binary file directly back to you.

```bash
curl -X POST http://localhost:6767/api/to-bytes \
  -H "Authorization: Bearer my_super_secret_token_123" \
  -F "template=<h1>Invoice</h1><p>Billed to: {{ name }}</p>" \
  -F "data={\"name\": \"Zalven\"}" \
  -F "filename=invoice.pdf" \
  --output my_invoice.pdf

```

### Endpoint 2: Direct to S3

Renders the PDF, uploads it to your configured S3 bucket, and returns a JSON payload with a presigned URL.

```bash
curl -X POST http://localhost:6767/api/to-s3 \
  -H "Authorization: Bearer my_super_secret_token_123" \
  -F "template=<h1>Welcome {{ name }}</h1>" \
  -F "data={\"name\": \"Zalven\"}"

```

**Response (200 OK):**

```json
{
  "file_name": "1709483829000.pdf",
  "file_size": 42056,
  "file_type": "application/pdf",
  "storage_key": "pdfs/1709483829000.pdf",
  "url": "https://your-bucket.s3.amazonaws.com/pdfs/1709483829000.pdf...",
  "status": "success"
}

```

---

## ⚖️ License & Acknowledgements

The Rust application code is open-source under the **MIT License**.

**⚠️ Important Note regarding PrinceXML:**
This project utilizes [PrinceXML](https://www.princexml.com/) as its core rendering engine. The free version inserts a small watermark on the first page of generated PDFs. Using PrinceXML in a commercial production environment requires purchasing a valid license from YesLogic Pty. Ltd. Please review their [End User License Agreement](https://www.princexml.com/license/) before deploying this to production.
