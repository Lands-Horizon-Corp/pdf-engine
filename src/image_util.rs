//! Pure helpers for `<img>` handling, shared by the native engine (`assets.rs`) and the
//! WebAssembly build: find `<img src>` values, decode `data:` URIs, recognise supported formats.
//! No networking and no operating-system calls.

use base64::Engine as _;

#[derive(Clone)]
pub(crate) enum Skip {
    /// Not something we load (relative path, `file://`, non-image data URI).
    Ignored,
    /// Refused by policy.
    Blocked(&'static str),
    Failed(String),
}

#[allow(dead_code)] // the WebAssembly build and the tests use it
pub(crate) fn decode_data_uri(rest: &str, max_bytes: usize) -> Result<Vec<u8>, Skip> {
    decode_data_uri_as(rest, max_bytes, true)
}

/// As `decode_data_uri`; with `images_only` false any media type is accepted (fonts, for `@font-face`).
pub(crate) fn decode_data_uri_as(
    rest: &str,
    max_bytes: usize,
    images_only: bool,
) -> Result<Vec<u8>, Skip> {
    let (meta, payload) = rest.split_once(',').ok_or(Skip::Ignored)?;
    if images_only && !meta.to_ascii_lowercase().starts_with("image/") {
        return Err(Skip::Ignored);
    }
    if payload.len() > max_bytes.saturating_mul(4) / 3 + 4 {
        return Err(Skip::Blocked("data URI too large"));
    }
    let bytes = if meta.to_ascii_lowercase().ends_with(";base64") {
        let decode = |input: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(input)
                .map_err(|e| Skip::Failed(format!("bad base64: {e}")))
        };
        // Whitespace is rare; only then pay for a cleaned copy.
        if payload.bytes().any(|c| c.is_ascii_whitespace()) {
            let compact: String = payload
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            decode(&compact)?
        } else {
            decode(payload)?
        }
    } else {
        percent_decode(payload)
    };
    if bytes.len() > max_bytes {
        return Err(Skip::Blocked("data URI too large"));
    }
    Ok(bytes)
}

/// Formats the renderer draws: PNG, JPEG, GIF and SVG.
pub(crate) fn is_supported_image(b: &[u8]) -> bool {
    if b.starts_with(b"\x89PNG") || b.starts_with(b"\xFF\xD8\xFF") || b.starts_with(b"GIF") {
        return true;
    }
    let text = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let start = text
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(text.len());
    let text = &text[start..];
    text.starts_with(b"<svg") || text.starts_with(b"<?xml")
}

/// Unique `src` values of `<img>` tags, HTML entities decoded (as the renderer sees them).
pub fn image_sources(html: &str) -> Vec<String> {
    let bytes = html.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut pos = 0;
    while let Some(start) = find_ci(bytes, b"<img", pos) {
        let tag_end = html[start..].find('>').map_or(html.len(), |i| start + i);
        let tag = &html[start..tag_end];
        if let Some(src) = attr_value(tag, "src") {
            let src = decode_entities(src);
            if !src.is_empty() && !out.contains(&src) {
                out.push(src);
            }
        }
        pos = tag_end.max(start + 4);
    }
    out
}

/// Value of attribute `name` inside an opening tag (`<img ... name="value" ...`).
pub(crate) fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut pos = 0;
    while let Some(at) = find_ci(bytes, name.as_bytes(), pos) {
        pos = at + name.len();
        let preceded_by_space = at > 0 && bytes[at - 1].is_ascii_whitespace();
        let rest = tag[pos..].trim_start();
        if !preceded_by_space || !rest.starts_with('=') {
            continue;
        }
        let rest = rest[1..].trim_start();
        return Some(match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest
                .split(|c: char| c.is_ascii_whitespace() || c == '>')
                .next()
                .unwrap_or(""),
        });
    }
    None
}

pub(crate) fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
        .map(|i| i + from)
}

/// Decode the entities template auto-escaping produces (`&amp;`, `&#x2f;`, ...).
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Pixel size of a PNG, GIF, JPEG or SVG, read from its header.
pub(crate) fn image_dimensions(b: &[u8]) -> Option<(f64, f64)> {
    let be32 = |i: usize| {
        b.get(i..i + 4)
            .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64)
    };
    let le16 = |i: usize| {
        b.get(i..i + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]) as f64)
    };
    let (w, h) = if b.starts_with(b"\x89PNG") {
        (be32(16)?, be32(20)?)
    } else if b.starts_with(b"GIF") {
        (le16(6)?, le16(8)?)
    } else if b.starts_with(b"\xFF\xD8") {
        let mut i = 2;
        loop {
            while *b.get(i)? != 0xFF {
                i += 1;
            }
            let marker = *b.get(i + 1)?;
            if marker == 0xFF {
                i += 1;
                continue;
            }
            let len = u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]) as usize;
            // SOF0-SOF15 except DHT (C4), JPG (C8) and DAC (CC) carry the frame size.
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([*b.get(i + 5)?, *b.get(i + 6)?]) as f64;
                let w = u16::from_be_bytes([*b.get(i + 7)?, *b.get(i + 8)?]) as f64;
                break (w, h);
            }
            if marker == 0xD9 || len < 2 {
                return None;
            }
            i += 2 + len;
        }
    } else {
        let text = std::str::from_utf8(b.get(..b.len().min(2048))?)
            .ok()
            .or_else(|| {
                // A multi-byte character may be cut at the 2 KB mark.
                std::str::from_utf8(&b[..b.len().min(2048)])
                    .err()
                    .and_then(|e| std::str::from_utf8(&b[..e.valid_up_to()]).ok())
            })?;
        let tag = &text[find_ci(text.as_bytes(), b"<svg", 0)?..];
        let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
        let number = |v: &str| v.trim().trim_end_matches("px").parse::<f64>().ok();
        match (
            attr_value(tag, "width").and_then(number),
            attr_value(tag, "height").and_then(number),
        ) {
            (Some(w), Some(h)) => (w, h),
            _ => {
                let mut v = attr_value(tag, "viewBox")?
                    .split(|c: char| c.is_ascii_whitespace() || c == ',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.parse::<f64>());
                let (_, _, w, h) = (
                    v.next()?.ok()?,
                    v.next()?.ok()?,
                    v.next()?.ok()?,
                    v.next()?.ok()?,
                );
                (w, h)
            }
        }
    };
    (w > 0.0 && h > 0.0).then_some((w, h))
}

/// Does `style` (or the tag) set `prop` itself (`width`, not `max-width`)?
fn sets_property(style: &str, prop: &str) -> bool {
    let b = style.as_bytes();
    let mut from = 0;
    while let Some(at) = find_ci(b, prop.as_bytes(), from) {
        from = at + prop.len();
        let before_ok = at == 0 || !(b[at - 1] == b'-' || b[at - 1].is_ascii_alphanumeric());
        if before_ok && style[from..].trim_start().starts_with(':') {
            return true;
        }
    }
    false
}

/// Give every `<img>` whose size the layout cannot know a size from the image itself.
///
/// The layout engine learns an `<img>`'s size only from its attributes and style, not from the
/// file, so `<img src="logo.png">` or `<img src="logo.png" width="120">` gets no height and is
/// not drawn. With neither width nor height, the image's own size is added; with only one, an
/// `aspect-ratio` is added so the other follows. Images the template already sizes both ways,
/// and sources not in `images`, are left alone. Returns `None` when nothing changed.
pub fn size_images(html: &str, images: &[(String, std::sync::Arc<Vec<u8>>)]) -> Option<String> {
    if images.is_empty() {
        return None;
    }
    let bytes = html.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut pos = 0;
    while let Some(start) = find_ci(bytes, b"<img", pos) {
        let tag_end = html[start..].find('>').map_or(html.len(), |i| start + i);
        pos = tag_end.max(start + 4);
        let tag = &html[start..tag_end];
        let Some(src) = attr_value(tag, "src").map(decode_entities) else {
            continue;
        };
        let Some((_, data)) = images.iter().find(|(s, _)| *s == src) else {
            continue;
        };
        let Some((iw, ih)) = image_dimensions(data) else {
            continue;
        };

        let style = attr_value(tag, "style");
        let has = |name: &str| {
            attr_value(tag, name).is_some() || style.is_some_and(|s| sets_property(s, name))
        };
        let (has_w, has_h) = (has("width"), has("height"));
        if has_w && has_h {
            continue;
        }
        let (at, insert) =
            if !has_w && !has_h && !style.is_some_and(|s| sets_property(s, "aspect-ratio")) {
                (
                    tag.trim_end_matches('/').trim_end().len(),
                    format!(" width=\"{iw}\" height=\"{ih}\""),
                )
            } else if let Some(s) = style {
                // Put the ratio inside the existing style (a second `style` attribute would be ignored).
                let quoted = tag
                    .as_bytes()
                    .get((s.as_ptr() as usize - tag.as_ptr() as usize).wrapping_sub(1))
                    .is_some_and(|c| matches!(c, b'"' | b'\''));
                if !quoted {
                    continue;
                }
                (
                    s.as_ptr() as usize - tag.as_ptr() as usize,
                    format!("aspect-ratio:{iw}/{ih};"),
                )
            } else {
                (
                    tag.trim_end_matches('/').trim_end().len(),
                    format!(" style=\"aspect-ratio:{iw}/{ih}\""),
                )
            };
        out.push_str(&html[copied..start + at]);
        out.push_str(&insert);
        copied = start + at;
    }
    if copied == 0 {
        return None;
    }
    out.push_str(&html[copied..]);
    Some(out)
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use std::sync::Arc;

    // A 10 x 10 PNG, a 3 x 2 GIF header and a 40 x 20 SVG.
    fn png() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC")
            .unwrap()
    }
    fn imgs() -> Vec<(String, Arc<Vec<u8>>)> {
        vec![
            ("p".into(), Arc::new(png())),
            (
                "s".into(),
                Arc::new(br#"<svg viewBox="0 0 40 20" xmlns="x"></svg>"#.to_vec()),
            ),
        ]
    }

    #[test]
    fn dimensions_of_each_format() {
        assert_eq!(image_dimensions(&png()), Some((10.0, 10.0)));
        assert_eq!(
            image_dimensions(b"GIF89a\x03\x00\x02\x00"),
            Some((3.0, 2.0))
        );
        assert_eq!(
            image_dimensions(br#"<svg width="40" height="20"></svg>"#),
            Some((40.0, 20.0))
        );
        assert_eq!(
            image_dimensions(br#"<svg viewBox="0 0 40 20"></svg>"#),
            Some((40.0, 20.0))
        );
        // SOF0: 480 high, 640 wide, after an APP0 segment.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0x01, 0xE0, 0x02, 0x80, 1,
        ];
        assert_eq!(image_dimensions(&jpeg), Some((640.0, 480.0)));
        assert_eq!(image_dimensions(b"nonsense"), None);
        assert_eq!(image_dimensions(&[0xFF, 0xD8]), None);
    }

    #[test]
    fn unsized_images_get_their_own_size() {
        let out = size_images(r#"<p>a</p><img src="p">"#, &imgs()).unwrap();
        assert_eq!(out, r#"<p>a</p><img src="p" width="10" height="10">"#);
        let out = size_images(r#"<IMG src='s' />"#, &imgs()).unwrap();
        assert_eq!(out, r#"<IMG src='s' width="40" height="20" />"#);
    }

    #[test]
    fn half_sized_images_get_an_aspect_ratio() {
        let out = size_images(r#"<img src="s" width="100">"#, &imgs()).unwrap();
        assert_eq!(
            out,
            r#"<img src="s" width="100" style="aspect-ratio:40/20">"#
        );
        let out = size_images(r#"<img src="s" style="width:50%">"#, &imgs()).unwrap();
        assert_eq!(out, r#"<img src="s" style="aspect-ratio:40/20;width:50%">"#);
        // max-width does not size the image.
        let out = size_images(r#"<img src="p" style="max-width:5px">"#, &imgs()).unwrap();
        assert!(out.contains(r#"width="10" height="10""#) || out.contains("aspect-ratio"));
    }

    #[test]
    fn sized_or_unknown_images_are_untouched() {
        assert!(size_images(r#"<img src="p" width="5" height="5">"#, &imgs()).is_none());
        assert!(size_images(r#"<img src="p" style="width:5px;height:5px">"#, &imgs()).is_none());
        assert!(size_images(r#"<img src="other">"#, &imgs()).is_none());
        assert!(size_images(r#"<img src="p">"#, &[]).is_none());
    }
}
