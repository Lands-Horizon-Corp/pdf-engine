use crate::error::AppError;
use lopdf::{
    Document,
    encryption::{EncryptionState, EncryptionVersion, Permissions},
};
use minijinja::Environment;
use std::{
    fmt::Write as _,
    io::Cursor,
    process::Stdio,
    sync::{Arc, LazyLock},
};
use tokio::fs::File;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Semaphore,
};

static TEMPLATE_ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
    env
});

const BLANK_PAGE: &str =
    r#"<div style="page-break-after: always; visibility: hidden; height: 0;"></div>"#;

fn wrap_and_prepend_blank_page(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + BLANK_PAGE.len() + 32);
    if let Some(start) = html.find("<body") {
        if let Some(rel_end) = html[start..].find('>') {
            let at = start + rel_end + 1;
            out.push_str(&html[..at]);
            out.push_str(BLANK_PAGE);
            out.push_str(&html[at..]);
            return out;
        }
    }

    out.push_str(BLANK_PAGE);
    out.push_str("<div class=\"pdf-root\">");
    out.push_str(html);
    out.push_str("</div>");
    out
}

fn escape_non_ascii(s: String) -> String {
    if s.is_ascii() {
        return s;
    }
    let mut out = String::with_capacity(s.len() + s.len() / 2);
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let _ = write!(out, "&#{};", c as u32);
        }
    }
    out
}

pub async fn render_template(
    template_str: String,
    data: serde_json::Value,
) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || {
        let rendered = TEMPLATE_ENV
            .render_str(&template_str, minijinja::Value::from(minijinja::value::Serde(&data)))?;
        Ok(escape_non_ascii(rendered))
    })
    .await?
}

pub async fn run_prince_and_process(
    html: String,
    w: String,
    h: String,
    o: String,
    password: Option<String>,
    prince_bin: &str,
    semaphore: Arc<Semaphore>,
) -> Result<Vec<u8>, AppError> {
    let xhtml_ready_content = wrap_and_prepend_blank_page(&html);
    drop(html);
    let _permit = semaphore.acquire().await.unwrap();

    let mut child = Command::new(prince_bin)
        .kill_on_drop(true)
        .args([
            "--no-network",
            "--no-javascript",
            "--silent",
            "--style",
            &format!(
                "data:text/css,@page {{ size: {} {} {}; margin: 0; }} \
                body {{ font-family: 'Noto Sans', 'Noto Sans CJK SC', 'Noto Color Emoji', sans-serif; }}",
                w, h, o
            ),
            "-",
            "-o",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    tokio::spawn(async move {
        let _ = stdin.write_all(xhtml_ready_content.as_bytes()).await;
        let _ = stdin.flush().await;
    });

    let mut raw_buffer = Vec::with_capacity(1024 * 1024);
    stdout.read_to_end(&mut raw_buffer).await?;

    let status = child.wait().await?;
    if !status.success() {
        return Err(AppError::PrinceStatus("Prince failed to render".into()));
    }

    tokio::task::spawn_blocking(move || {
        let mut doc = Document::load_from(Cursor::new(raw_buffer))
            .map_err(|e| AppError::PrinceStatus(e.to_string()))?;
        if doc.get_pages().len() > 1 {
            doc.delete_pages(&[1]);
        }
        doc.prune_objects();
        if let Some(ref p) = password {
            let version = EncryptionVersion::V2 {
                document: &doc,
                owner_password: p,
                user_password: p,
                permissions: Permissions::all(),
                key_length: 128,
            };
            let state: EncryptionState = version
                .try_into()
                .map_err(|e: lopdf::Error| AppError::PrinceStatus(e.to_string()))?;
            doc.encrypt(&state)
                .map_err(|e: lopdf::Error| AppError::PrinceStatus(e.to_string()))?;
        }
        let mut out = Vec::with_capacity(1024 * 1024);
        doc.save_to(&mut out)
            .map_err(|e| AppError::PrinceStatus(e.to_string()))?;
        Ok(out)
    })
    .await?
}

pub async fn warm_up_engine(prince_bin: &str, semaphore: Arc<Semaphore>) -> Result<(), AppError> {
    tracing::info!("Warming up Prince engine and generating test PDF...");

    let test_content = "<span>Warmup Test: ₱ € ¥ | 你好 | 🚀 ✅ | © ®</span>".into();
    let pdf_bytes = run_prince_and_process(
        test_content,
        "8.5in".into(),
        "11in".into(),
        "portrait".into(),
        None,
        prince_bin,
        semaphore,
    )
    .await?;

    let mut file = File::create("sample.pdf")
        .await
        .map_err(|e| AppError::PrinceStatus(format!("Failed to create sample.pdf: {}", e)))?;

    file.write_all(&pdf_bytes)
        .await
        .map_err(|e| AppError::PrinceStatus(format!("Failed to write to sample.pdf: {}", e)))?;

    tracing::info!("Prince engine warmed up. 'sample.pdf' has been saved to the root directory.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_page_after_plain_body() {
        let out = wrap_and_prepend_blank_page("<html><body><p>x</p></body></html>");
        assert_eq!(out, format!("<html><body>{BLANK_PAGE}<p>x</p></body></html>"));
    }

    #[test]
    fn blank_page_after_body_with_attributes() {
        let out = wrap_and_prepend_blank_page(r#"<body class="a"><p>x</p></body>"#);
        assert_eq!(out, format!(r#"<body class="a">{BLANK_PAGE}<p>x</p></body>"#));
    }

    #[test]
    fn fragment_is_wrapped() {
        let out = wrap_and_prepend_blank_page("<p>x</p>");
        assert_eq!(out, format!(r#"{BLANK_PAGE}<div class="pdf-root"><p>x</p></div>"#));
    }

    #[test]
    fn escapes_non_ascii_only() {
        assert_eq!(escape_non_ascii("abc".into()), "abc");
        assert_eq!(escape_non_ascii("a₱é🚀".into()), "a&#8369;&#233;&#128640;");
    }

    #[tokio::test]
    async fn renders_with_html_autoescape() {
        let out = render_template(
            "<p>{{ v }}</p>".into(),
            serde_json::json!({ "v": "<b>你</b>" }),
        )
        .await
        .unwrap();
        // minijinja 3 also escapes `/` as `&#x2f;`.
        assert_eq!(out, "<p>&lt;b&gt;&#20320;&lt;&#x2f;b&gt;</p>");
    }
}
