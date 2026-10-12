use crate::{
    config::AppState,
    error::AppError,
    models::{MediaPayload, PdfRequest},
    pdf,
};
use std::future::IntoFuture;
use tokio::time::{Duration, timeout};
use tracing::Instrument;

pub async fn html_to_pdf_to_storage(
    req: PdfRequest,
    object_name: String,
    state: &AppState,
) -> Result<MediaPayload, AppError> {
    let work = async {
        let pdf_bytes = pdf::render(req, state).await?;
        let file_size = pdf_bytes.len() as i64;

        // `Bytes` converts into opendal's `Buffer` without copying.
        let started = std::time::Instant::now();
        let upload = state
            .storage
            .write_with(&object_name, pdf_bytes)
            .content_disposition("inline")
            .content_type("application/pdf")
            .into_future()
            .instrument(tracing::info_span!("s3_upload"))
            .await;
        metrics::histogram!("pdf_s3_upload_seconds").record(started.elapsed().as_secs_f64());
        let outcome = if upload.is_ok() { "ok" } else { "error" };
        metrics::counter!("pdf_s3_uploads_total", "outcome" => outcome).increment(1);
        upload?;

        let signed = state
            .storage
            .presign_read(&object_name, Duration::from_secs(3600))
            .await?;

        let payload = MediaPayload {
            file_name: object_name
                .split('/')
                .last()
                .unwrap_or("doc.pdf")
                .to_string(),
            file_size,
            file_type: "application/pdf".into(),
            storage_key: object_name.clone(),
            url: signed.uri().to_string(),
            bucket_name: state.storage_bucket.to_string(),
            status: "success".into(),
            progress: 100,
        };
        Ok(payload)
    };

    timeout(state.render_timeout, work)
        .await
        .map_err(|_| AppError::Timeout(state.render_timeout))?
}
