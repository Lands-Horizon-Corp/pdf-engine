//! Rendering through the external Prince binary.
//!
//! Prince stamps its free-license logo on page 1, so we prepend a blank page and the caller
//! strips it afterwards (`pdf::finalize`).

use crate::error::AppError;
use std::{fmt::Write as _, process::Stdio};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

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

/// Replace every non-ASCII char with a numeric entity, without per-char allocations.
fn escape_non_ascii(s: &str) -> String {
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

/// Run Prince and return its raw output, which still contains the leading blank page.
/// The caller holds the render permit.
pub async fn render(
    html: &str,
    w: &str,
    h: &str,
    o: &str,
    prince_bin: &str,
) -> Result<Vec<u8>, AppError> {
    let input = if html.is_ascii() {
        wrap_and_prepend_blank_page(html)
    } else {
        wrap_and_prepend_blank_page(&escape_non_ascii(html))
    };

    let mut child = Command::new(prince_bin)
        .kill_on_drop(true)
        .args([
            "--no-network",
            "--no-local-files",
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
        let _ = stdin.write_all(input.as_bytes()).await;
        let _ = stdin.flush().await;
    });

    let mut raw_buffer = Vec::with_capacity(1024 * 1024);
    stdout.read_to_end(&mut raw_buffer).await?;

    let status = child.wait().await?;
    if !status.success() {
        return Err(AppError::PrinceStatus("Prince failed to render".into()));
    }
    Ok(raw_buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_page_after_plain_body() {
        let out = wrap_and_prepend_blank_page("<html><body><p>x</p></body></html>");
        assert_eq!(
            out,
            format!("<html><body>{BLANK_PAGE}<p>x</p></body></html>")
        );
    }

    #[test]
    fn blank_page_after_body_with_attributes() {
        let out = wrap_and_prepend_blank_page(r#"<body class="a"><p>x</p></body>"#);
        assert_eq!(
            out,
            format!(r#"<body class="a">{BLANK_PAGE}<p>x</p></body>"#)
        );
    }

    #[test]
    fn fragment_is_wrapped() {
        let out = wrap_and_prepend_blank_page("<p>x</p>");
        assert_eq!(
            out,
            format!(r#"{BLANK_PAGE}<div class="pdf-root"><p>x</p></div>"#)
        );
    }

    #[test]
    fn escapes_non_ascii_only() {
        assert_eq!(escape_non_ascii("abc"), "abc");
        assert_eq!(escape_non_ascii("a₱é🚀"), "a&#8369;&#233;&#128640;");
    }
}
