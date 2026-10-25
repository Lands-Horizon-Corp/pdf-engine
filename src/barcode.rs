//! `<barcode>` tags: QR codes and barcodes drawn as vector shapes (inline SVG), so they stay sharp at any size and
//! scan from paper or screen.
//!
//! ```html
//! <barcode value="https://example.com/pay/123" type="qr" width="120" />
//! <barcode value="INV-2026-0042" type="code128" width="220" height="60" />
//! ```
//!
//! Attributes: `value` (required), `type` (`qr` default, `code128`, `code39`, `ean13`, `ean8`), `width` and `height`
//! (pixels), `color`, `background`, `margin` (quiet zone in modules), `ec` (QR error correction: `L`, `M`, `Q`, `H`),
//! and `class` / `style`, which are copied to the `<svg>`. The tag is replaced after the template has run, so
//! `value="{{ invoice.no }}"` works.

use crate::{error::AppError, image_util};
use barcoders::sym::{code39::Code39, code128::Code128, ean8::EAN8, ean13::EAN13};
use qrcode::{Color, EcLevel, QrCode};
use std::{borrow::Cow, fmt::Write};

/// Replace every `<barcode .../>` in `html` with an SVG. Returns the input untouched when there is none.
pub fn expand(html: &str) -> Result<Cow<'_, str>, AppError> {
    let bytes = html.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut pos = 0;
    while let Some(start) = image_util::find_ci(bytes, b"<barcode", pos) {
        // `<barcode` must be a whole tag name, not the start of `<barcodes>`.
        let after = bytes.get(start + 8).copied().unwrap_or(b'>');
        if !(after.is_ascii_whitespace() || after == b'/' || after == b'>') {
            pos = start + 8;
            continue;
        }
        let Some(end) = html[start..].find('>').map(|i| start + i + 1) else {
            return Err(AppError::InvalidField(
                "barcode: the tag is not closed".into(),
            ));
        };
        let tag = &html[start..end];
        let svg = render_tag(tag)?;
        // `<barcode ...></barcode>` is accepted too.
        let mut resume = end;
        if !tag.trim_end_matches('>').ends_with('/') {
            let rest = &html[end..];
            if let Some(close) = rest.trim_start().strip_prefix("</") {
                if close.to_ascii_lowercase().starts_with("barcode>") {
                    resume = end + (rest.len() - rest.trim_start().len()) + 2 + "barcode>".len();
                }
            }
        }
        let buf = out.get_or_insert_with(|| String::with_capacity(html.len() + svg.len()));
        buf.push_str(&html[copied..start]);
        buf.push_str(&svg);
        copied = resume;
        pos = resume;
    }
    Ok(match out {
        Some(mut buf) => {
            buf.push_str(&html[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(html),
    })
}

fn attr(tag: &str, name: &str) -> Option<String> {
    image_util::attr_value(tag, name).map(image_util::decode_entities)
}

fn number(tag: &str, name: &str) -> Result<Option<f64>, AppError> {
    match attr(tag, name) {
        None => Ok(None),
        Some(v) => v
            .trim()
            .trim_end_matches("px")
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n > 0.0 && *n <= 5000.0)
            .map(Some)
            .ok_or_else(|| {
                AppError::InvalidField(format!(
                    "barcode: {name}=\"{v}\" must be a number of pixels between 0 and 5000"
                ))
            }),
    }
}

/// A CSS colour is copied into the SVG, so keep it to characters a colour can contain.
fn colour(tag: &str, name: &str, default: &str) -> Result<String, AppError> {
    match attr(tag, name) {
        None => Ok(default.to_string()),
        Some(c)
            if !c.is_empty()
                && c.len() <= 40
                && c.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "#(),. %-".contains(ch)) =>
        {
            Ok(c)
        }
        Some(c) => Err(AppError::InvalidField(format!(
            "barcode: {name}=\"{c}\" is not a colour"
        ))),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_tag(tag: &str) -> Result<String, AppError> {
    let value = attr(tag, "value")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::InvalidField("barcode: `value` is required".into()))?;
    let kind = attr(tag, "type")
        .unwrap_or_else(|| "qr".into())
        .to_ascii_lowercase();
    let (width, height) = (number(tag, "width")?, number(tag, "height")?);
    let fill = colour(tag, "color", "#000000")?;
    let background = match attr(tag, "background") {
        Some(_) => Some(colour(tag, "background", "#ffffff")?),
        None => None,
    };
    let margin = number(tag, "margin")?;

    // (modules per row of dark bars, rows) as filled rectangles in module units.
    let (view_w, view_h, path, default_w, default_h, aspect) = if kind == "qr" {
        let level = match attr(tag, "ec").map(|s| s.to_ascii_uppercase()).as_deref() {
            None | Some("M") => EcLevel::M,
            Some("L") => EcLevel::L,
            Some("Q") => EcLevel::Q,
            Some("H") => EcLevel::H,
            Some(other) => {
                return Err(AppError::InvalidField(format!(
                    "barcode: ec=\"{other}\" must be L, M, Q or H"
                )));
            }
        };
        let code = QrCode::with_error_correction_level(value.as_bytes(), level).map_err(|e| {
            AppError::InvalidField(format!("barcode: the value does not fit a QR code ({e})"))
        })?;
        let n = code.width();
        let m = margin.unwrap_or(4.0);
        let colors = code.to_colors();
        let mut d = String::new();
        for y in 0..n {
            let mut x = 0;
            while x < n {
                if colors[y * n + x] == Color::Dark {
                    let from = x;
                    while x < n && colors[y * n + x] == Color::Dark {
                        x += 1;
                    }
                    let _ = write!(
                        d,
                        "M{} {}h{}v1h-{}z",
                        from as f64 + m,
                        y as f64 + m,
                        x - from,
                        x - from
                    );
                } else {
                    x += 1;
                }
            }
        }
        let side = n as f64 + 2.0 * m;
        (side, side, d, 128.0, 128.0, "xMidYMid meet")
    } else {
        let modules: Vec<u8> = match kind.as_str() {
            "code128" => {
                // Digits only, in pairs: character set C packs them twice as tight; everything else: set B.
                let set = if value.len() % 2 == 0 && value.bytes().all(|b| b.is_ascii_digit()) { '\u{0106}' } else { '\u{0181}' };
                Code128::new(format!("{set}{value}")).map(|c| c.encode())
            }
            "code39" => Code39::new(value.to_ascii_uppercase()).map(|c| c.encode()),
            "ean13" => EAN13::new(&value).map(|c| c.encode()),
            "ean8" => EAN8::new(&value).map(|c| c.encode()),
            other => {
                return Err(AppError::InvalidField(format!(
                    "barcode: type=\"{other}\" is not supported (use qr, code128, code39, ean13 or ean8)"
                )));
            }
        }
        .map_err(|e| AppError::InvalidField(format!("barcode: `{value}` is not valid for {kind} ({e})")))?;
        let m = margin.unwrap_or(10.0);
        let mut d = String::new();
        let mut x = 0;
        while x < modules.len() {
            if modules[x] == 1 {
                let from = x;
                while x < modules.len() && modules[x] == 1 {
                    x += 1;
                }
                let _ = write!(d, "M{} 0h{}v1h-{}z", from as f64 + m, x - from, x - from);
            } else {
                x += 1;
            }
        }
        let total = modules.len() as f64 + 2.0 * m;
        (total, 1.0, d, (total * 2.0).round(), 60.0, "none")
    };

    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, if kind == "qr" { w } else { default_h }),
        (None, Some(h)) => (if kind == "qr" { h } else { default_w }, h),
        (None, None) => (default_w, default_h),
    };

    let mut svg = String::with_capacity(path.len() + 300);
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {view_w} {view_h}" preserveAspectRatio="{aspect}" shape-rendering="crispEdges" role="img" aria-label="{} {}""#,
        escape(&kind),
        if kind == "qr" { "code" } else { "barcode" }
    );
    for name in ["class", "style"] {
        if let Some(v) = attr(tag, name) {
            let _ = write!(svg, r#" {name}="{}""#, escape(&v));
        }
    }
    svg.push('>');
    if let Some(bg) = background {
        let _ = write!(
            svg,
            r#"<rect width="{view_w}" height="{view_h}" fill="{bg}"/>"#
        );
    }
    let _ = write!(svg, r#"<path d="{path}" fill="{fill}"/></svg>"#);
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(tag: &str) -> String {
        expand(tag).unwrap().into_owned()
    }

    #[test]
    fn text_without_a_barcode_is_returned_as_it_is() {
        assert!(matches!(
            expand("<p>hi</p><barcodes></barcodes>").unwrap(),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn a_qr_code_is_an_svg_of_squares() {
        let out = svg(r#"before <barcode value="HELLO" width="100" /> after"#);
        assert!(
            out.starts_with("before <svg ") && out.ends_with("</svg> after"),
            "{out}"
        );
        assert!(out.contains(r#"width="100" height="100""#));
        // Version 1 is 21 modules; with the 4-module quiet zone the view box is 29.
        assert!(out.contains(r#"viewBox="0 0 29 29""#), "{out}");
        assert!(out.contains("<path d=\"M") && out.contains(r##"fill="#000000""##));
        // The finder pattern's top-left module starts at the margin.
        assert!(out.contains("M4 4h7v1h-7z"), "{out}");
    }

    #[test]
    fn linear_barcodes_stretch_to_the_height() {
        let out = svg(r#"<barcode value="INV-2026-0042" type="code128" width="220" height="60"/>"#);
        assert!(
            out.contains(r#"width="220" height="60""#)
                && out.contains(r#"preserveAspectRatio="none""#),
            "{out}"
        );
        assert!(out.contains(r#"aria-label="code128 barcode""#));
        // 12 digits as set C pairs are shorter than the same as set B.
        let paired = svg(r#"<barcode value="123456789012" type="code128"/>"#);
        let single = svg(r#"<barcode value="12345678901A" type="code128"/>"#);
        let width = |s: &str| {
            s.split("viewBox=\"0 0 ")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        };
        assert!(
            width(&paired) < width(&single),
            "{} < {}",
            width(&paired),
            width(&single)
        );
        for kind in ["code39", "ean13", "ean8"] {
            let value = match kind {
                "code39" => "ABC-123",
                "ean13" => "750103131130",
                _ => "9638507",
            };
            assert!(
                svg(&format!(r#"<barcode value="{value}" type="{kind}"/>"#)).contains("<path"),
                "{kind}"
            );
        }
    }

    #[test]
    fn options_and_the_closing_tag() {
        let out = svg(
            r##"<barcode value="x" color="#112233" background="white" margin="2" ec="H" class="qr" style="float:right"></barcode>!"##,
        );
        assert!(out.ends_with("</svg>!"), "{out}");
        assert!(
            out.contains(r##"fill="#112233""##)
                && out.contains(r#"<rect width="#)
                && out.contains(r#"class="qr""#)
                && out.contains(r#"style="float:right""#)
        );
        assert!(svg(r#"<BARCODE VALUE="a&amp;b"/>"#).contains("<svg"));
    }

    #[test]
    fn mistakes_are_named() {
        for (tag, needle) in [
            (r#"<barcode />"#, "`value` is required"),
            (r#"<barcode value="x" type="aztec"/>"#, "not supported"),
            (
                r#"<barcode value="abc" type="ean13"/>"#,
                "not valid for ean13",
            ),
            (r#"<barcode value="x" width="-4"/>"#, "width"),
            (r#"<barcode value="x" color="red;x:y"/>"#, "not a colour"),
            (r#"<barcode value="x" ec="Z"/>"#, "L, M, Q or H"),
            (r#"<barcode value="x""#, "not closed"),
        ] {
            let err = expand(tag).unwrap_err();
            assert_eq!(err.code(), "INVALID_FIELD", "{tag}");
            assert!(err.to_string().contains(needle), "{tag}: {err}");
        }
    }
}
