use crate::{config::AppState, error::AppError, models::MediaPayload, pdf};
use tokio::time::{Duration, timeout};

pub async fn html_to_pdf_to_storage(
    req: crate::models::PdfRequest,
    object_name: String,
    state: &AppState,
) -> Result<MediaPayload, AppError> {
    let work = async {
        let html = pdf::render_template(req.template, req.data).await?;
        let pdf_bytes = pdf::run_prince_and_process(
            html,
            req.width,
            req.height,
            req.password,
            state.prince_concurrency.clone(),
        )
        .await?;

        let file_size = pdf_bytes.len() as i64;

        state
            .storage
            .write_with(&object_name, pdf_bytes)
            .content_disposition("inline")
            .content_type("application/pdf")
            .await?;

        let signed = state
            .storage
            .presign_read(&object_name, Duration::from_secs(3600))
            .await?;

        Ok(MediaPayload {
            file_name: object_name
                .split('/')
                .last()
                .unwrap_or("doc.pdf")
                .to_string(),
            file_size,
            file_type: "application/pdf".into(),
            storage_key: object_name.clone(),
            url: signed.uri().to_string(),
            bucket_name: state.storage_bucket.clone(),
            status: "success".into(),
            progress: 100,
        })
    };

    timeout(Duration::from_secs(45), work)
        .await
        .map_err(|_| AppError::Timeout(Duration::from_secs(45)))?
}
