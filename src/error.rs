//! Errors shared by the HTTP server and the IPC mode.
//!
//! Every error has a stable machine-readable `code` (for callers to branch on), a readable
//! message, and where it helps a `hint` and the template `line`. Internal details (storage
//! endpoints, renderer internals) are never included; they go to the log instead.
//!
//! ```json
//! { "error": "Template rendering failed: syntax error: unexpected end of input (in <string>:3)",
//!   "code": "TEMPLATE_ERROR", "status": 400, "line": 3,
//!   "hint": "Check the template syntax: {{ value }}, {% for x in list %}...{% endfor %}." }
//! ```

//!
//! This file is compiled for the native server and, with the server-only variants left out
//! (`cfg(not(target_arch = "wasm32"))`), for the WebAssembly build.

#[cfg(not(target_arch = "wasm32"))]
use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid field: {0}")]
    InvalidField(String),
    #[error("IPC protocol error: {0}")]
    Protocol(String),
    #[error("Font error: {0}")]
    Font(String),
    #[error("Request body exceeds the {0}-byte limit")]
    PayloadTooLarge(usize),
    #[error("Template rendering failed: {0}")]
    Template(#[from] minijinja::Error),
    #[error("The template produced more than {0} bytes of output")]
    TemplateOutputTooLarge(usize),
    #[error("The template is too expensive to render (execution budget exceeded)")]
    TemplateTooExpensive,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(not(target_arch = "wasm32"))]
    #[error("Storage error: {0}")]
    Storage(#[from] opendal::Error),
    #[error("Storage is not configured")]
    StorageNotConfigured,
    #[error("Render failed: {0}")]
    Render(String),
    #[error("PDF post-processing failed: {0}")]
    PdfProcessing(String),
    #[error("Operation timed out after {0:?}")]
    Timeout(Duration),
    #[cfg(not(target_arch = "wasm32"))]
    #[error("Internal Task Error: {0}")]
    Join(#[from] tokio::task::JoinError),
}

impl AppError {
    /// Every code the engine can send. `.node/desktop/index.d.ts` must list exactly these (a test checks).
    pub const CODES: &'static [&'static str] = &[
        "UNAUTHORIZED",
        "MISSING_FIELD",
        "INVALID_FIELD",
        "IPC_PROTOCOL_ERROR",
        "FONT_ERROR",
        "PAYLOAD_TOO_LARGE",
        "TEMPLATE_ERROR",
        "TEMPLATE_OUTPUT_TOO_LARGE",
        "TEMPLATE_TOO_EXPENSIVE",
        "IO_ERROR",
        "STORAGE_ERROR",
        "STORAGE_NOT_CONFIGURED",
        "RENDER_FAILED",
        "PDF_PROCESSING_FAILED",
        "TIMEOUT",
        "INTERNAL_ERROR",
    ];

    /// Stable identifier callers can branch on. New codes may be added; existing ones never change.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Unauthorized => "UNAUTHORIZED",
            AppError::MissingField(_) => "MISSING_FIELD",
            AppError::InvalidField(_) => "INVALID_FIELD",
            AppError::Protocol(_) => "IPC_PROTOCOL_ERROR",
            AppError::Font(_) => "FONT_ERROR",
            AppError::PayloadTooLarge(_) => "PAYLOAD_TOO_LARGE",
            AppError::Template(_) => "TEMPLATE_ERROR",
            AppError::TemplateOutputTooLarge(_) => "TEMPLATE_OUTPUT_TOO_LARGE",
            AppError::TemplateTooExpensive => "TEMPLATE_TOO_EXPENSIVE",
            AppError::Io(_) => "IO_ERROR",
            #[cfg(not(target_arch = "wasm32"))]
            AppError::Storage(_) => "STORAGE_ERROR",
            AppError::StorageNotConfigured => "STORAGE_NOT_CONFIGURED",
            AppError::Render(_) => "RENDER_FAILED",
            AppError::PdfProcessing(_) => "PDF_PROCESSING_FAILED",
            AppError::Timeout(_) => "TIMEOUT",
            #[cfg(not(target_arch = "wasm32"))]
            AppError::Join(_) => "INTERNAL_ERROR",
        }
    }

    /// HTTP-style status. The IPC mode reports it too, so one mapping serves both.
    pub fn status(&self) -> u16 {
        match self {
            AppError::Unauthorized => 401,
            AppError::MissingField(_)
            | AppError::InvalidField(_)
            | AppError::Protocol(_)
            | AppError::Font(_)
            | AppError::Template(_)
            | AppError::TemplateTooExpensive => 400,
            AppError::PayloadTooLarge(_) | AppError::TemplateOutputTooLarge(_) => 413,
            AppError::Timeout(_) => 408,
            AppError::StorageNotConfigured => 501,
            _ => 500,
        }
    }

    /// Message that is safe to show to a caller. Server-side failures stay generic.
    pub fn public_message(&self) -> String {
        match self {
            AppError::Unauthorized => "Invalid or missing Bearer token".into(),
            e if e.status() == 500 => "Internal server error".into(),
            e => e.to_string(),
        }
    }

    /// What the caller can do about it.
    pub fn hint(&self) -> Option<&'static str> {
        Some(match self {
            AppError::Unauthorized => "Send the header `Authorization: Bearer <API_BEARER_TOKEN>`.",
            AppError::MissingField(_) => {
                "Send every required field; `template` is always required."
            }
            AppError::InvalidField(_) => {
                "Use lengths like 8.5in, 210mm, 21cm or 612pt, and portrait or landscape."
            }
            AppError::Font(_) => {
                "Pass paths to .ttf, .otf or .ttc files, or to folders that contain them."
            }
            AppError::Protocol(_) => {
                "Frames are [u32 BE header_len][u32 BE body_len][JSON header][body]; see src/ipc.rs."
            }
            AppError::PayloadTooLarge(_) => "Send less data, or raise the request size limit.",
            AppError::Template(_) => {
                "Check the template syntax: {{ value }}, {% for x in list %}...{% endfor %}."
            }
            AppError::TemplateOutputTooLarge(_) => {
                "A loop is producing far more HTML than expected; check nested loops over large lists."
            }
            AppError::TemplateTooExpensive => {
                "A loop runs too long; check nested loops over large lists, or raise TEMPLATE_FUEL."
            }
            AppError::StorageNotConfigured => {
                "Set STORAGE_BUCKET, STORAGE_URL, STORAGE_ACCESS_KEY and STORAGE_SECRET_KEY, or render to bytes instead."
            }
            AppError::Render(_) => {
                "The HTML could not be turned into a PDF. Check the template's HTML and CSS (docs/css.md)."
            }
            AppError::PdfProcessing(_) => {
                "The PDF could not be finished (for example, encrypted). Retry without `password`."
            }
            AppError::Timeout(_) => {
                "The render took too long. Simplify the document or raise RENDER_TIMEOUT_SECS."
            }
            AppError::Io(_) => return None,
            #[cfg(not(target_arch = "wasm32"))]
            AppError::Storage(_) | AppError::Join(_) => return None,
        })
    }

    /// 1-based template line for template errors.
    pub fn line(&self) -> Option<usize> {
        match self {
            AppError::Template(e) => e.line(),
            _ => None,
        }
    }

    /// The error as sent to callers, over HTTP and IPC alike.
    pub fn to_json(&self) -> Value {
        let mut body = json!({
            "error": self.public_message(),
            "code": self.code(),
            "status": self.status(),
        });
        if let Some(hint) = self.hint() {
            body["hint"] = hint.into();
        }
        if let Some(line) = self.line() {
            body["line"] = line.into();
        }
        body
    }

    /// Log server-side failures, whose details are not shown to the caller.
    pub fn log_if_internal(&self) {
        if self.status() >= 500 {
            tracing::error!(code = self.code(), "{self:?}");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ResponseError for AppError {
    fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    fn error_response(&self) -> HttpResponse {
        self.log_if_internal();
        HttpResponse::build(self.status_code()).json(self.to_json())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_a_code_and_a_readable_message() {
        let cases = [
            (AppError::Unauthorized, "UNAUTHORIZED", 401),
            (
                AppError::MissingField("template".into()),
                "MISSING_FIELD",
                400,
            ),
            (AppError::InvalidField("width".into()), "INVALID_FIELD", 400),
            (
                AppError::Protocol("bad frame".into()),
                "IPC_PROTOCOL_ERROR",
                400,
            ),
            (AppError::Font("nope".into()), "FONT_ERROR", 400),
            (AppError::PayloadTooLarge(10), "PAYLOAD_TOO_LARGE", 413),
            (
                AppError::TemplateOutputTooLarge(10),
                "TEMPLATE_OUTPUT_TOO_LARGE",
                413,
            ),
            (
                AppError::TemplateTooExpensive,
                "TEMPLATE_TOO_EXPENSIVE",
                400,
            ),
            (
                AppError::StorageNotConfigured,
                "STORAGE_NOT_CONFIGURED",
                501,
            ),
            (AppError::Render("boom".into()), "RENDER_FAILED", 500),
            (
                AppError::PdfProcessing("boom".into()),
                "PDF_PROCESSING_FAILED",
                500,
            ),
            (AppError::Timeout(Duration::from_secs(1)), "TIMEOUT", 408),
        ];
        for (err, code, status) in cases {
            assert!(AppError::CODES.contains(&code), "{code} missing from CODES");
            let json = err.to_json();
            assert_eq!(json["code"], code);
            assert_eq!(json["status"], status);
            assert!(!json["error"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn server_side_details_never_reach_the_caller() {
        let json = AppError::Render("secret path /etc/x".into()).to_json();
        assert_eq!(json["error"], "Internal server error");
        assert!(!json.to_string().contains("secret"));
    }

    #[test]
    fn template_errors_carry_the_line_number() {
        let env = minijinja::Environment::new();
        let err = env.render_str("line one\n{% if %}\n", ()).unwrap_err();
        let json = AppError::Template(err).to_json();
        assert_eq!(json["code"], "TEMPLATE_ERROR");
        assert_eq!(json["line"], 2);
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .starts_with("Template rendering failed")
        );
    }
}
