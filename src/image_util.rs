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

pub(crate) fn decode_data_uri(rest: &str, max_bytes: usize) -> Result<Vec<u8>, Skip> {
    let (meta, payload) = rest.split_once(',').ok_or(Skip::Ignored)?;
    if !meta.to_ascii_lowercase().starts_with("image/") {
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
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}
