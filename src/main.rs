use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use std::net::SocketAddr;

mod models;
mod utils;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    if let Err(e) = utils::warm_up_engine().await {
        eprintln!("Warmup failed: {}", e);
        std::process::exit(1);
    }

    let addr: SocketAddr = "0.0.0.0:6767".parse().expect("Invalid address");

    let app = Router::new()
        .route("/api/to-s3", post(handle_to_s3))
        .route("/api/to-bytes", post(handle_to_bytes))
        // 2. Increase limit for large templates/data payloads
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024));

    println!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Helper to parse multipart fields into a clean struct
struct PdfRequest {
    template: String,
    data: serde_json::Value,
    width: String,
    height: String,
    filename: String,
    password: Option<String>,
}

async fn parse_pdf_multipart(mut multipart: Multipart) -> Result<PdfRequest, StatusCode> {
    let mut template = None;
    let mut data = serde_json::Value::Null;
    let mut width = "8.5in".to_string();
    let mut height = "11in".to_string();
    let mut filename = "document.pdf".to_string();
    let mut password = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "template" => template = Some(field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?),
            "data" => {
                let text = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                data = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            }
            "width" => width = field.text().await.unwrap_or(width),
            "height" => height = field.text().await.unwrap_or(height),
            "filename" => filename = field.text().await.unwrap_or(filename),
            "password" => {
                let p = field.text().await.unwrap_or_default();
                if !p.is_empty() {
                    password = Some(p);
                }
            }
            _ => {}
        }
    }

    Ok(PdfRequest {
        template: template.ok_or(StatusCode::BAD_REQUEST)?,
        data,
        width,
        height,
        filename,
        password,
    })
}

async fn handle_to_s3(multipart: Multipart) -> impl IntoResponse {
    let req = match parse_pdf_multipart(multipart).await {
        Ok(r) => r,
        Err(status) => return (status, "Missing required fields").into_response(),
    };

    // Generate a unique key using a faster timestamp approach
    let key = format!(
        "pdfs/{}.pdf",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );

    match utils::html_to_pdf_to_storage(
        req.template,
        req.data,
        req.width,
        req.height,
        key,
        req.password,
    )
    .await
    {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn handle_to_bytes(multipart: Multipart) -> impl IntoResponse {
    let req = match parse_pdf_multipart(multipart).await {
        Ok(r) => r,
        Err(status) => return (status, "Missing required fields").into_response(),
    };

    match utils::html_to_pdf_bytes(req.template, req.data, req.width, req.height, req.password)
        .await
    {
        Ok(bytes) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "application/pdf".parse().unwrap());
            // Ensure filename is sanitized to avoid header injection
            let safe_filename = req.filename.replace('"', "");
            headers.insert(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", safe_filename)
                    .parse()
                    .unwrap(),
            );
            (StatusCode::OK, headers, bytes).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
