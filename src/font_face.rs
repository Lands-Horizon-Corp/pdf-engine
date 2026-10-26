//! `@font-face` rules: which fonts a document asks for, found in its `<style>` text.
//!
//! The renderer has no network of its own, so the host reads the rules here, loads each rule's font (a `data:` URI,
//! an allowed URL, a file in the allowed folder) and gives the bytes to the renderer under the family name the CSS
//! used. Pure string work, so the browser build uses the same file.

/// One usable `@font-face` rule.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFaceDecl {
    /// `font-family`, without quotes.
    pub family: String,
    /// The `url(...)` to load: the first source in a format the renderer reads (TrueType, OpenType, WOFF2).
    pub src: String,
    /// `font-weight` as a number (`bold` is 700); `None` means 400.
    pub weight: Option<f32>,
    pub italic: bool,
}

/// A font from one `@font-face` rule, loaded.
#[derive(Debug, Clone)]
pub struct LoadedFontFace {
    pub family: String,
    pub data: std::sync::Arc<Vec<u8>>,
    pub weight: Option<f32>,
    pub italic: bool,
}

/// The most rules a document may use; the rest are ignored.
pub const MAX_FONT_FACES: usize = 16;

/// Every usable `@font-face` rule in `html`, in document order, at most [`MAX_FONT_FACES`].
pub fn font_face_decls(html: &str) -> Vec<FontFaceDecl> {
    // Most documents have no such rule: look for one without copying the document.
    if !html
        .as_bytes()
        .windows(10)
        .any(|w| w.eq_ignore_ascii_case(b"@font-face"))
    {
        return Vec::new();
    }
    let lower = html.to_ascii_lowercase();
    let mut out: Vec<FontFaceDecl> = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("@font-face") {
        let start = from + at + "@font-face".len();
        from = start;
        let Some(open) = html[start..].find('{') else {
            break;
        };
        // Only whitespace may sit between the at-rule name and its block.
        if !html[start..start + open].trim().is_empty() {
            continue;
        }
        let body_start = start + open + 1;
        let Some(len) = block_len(&html[body_start..]) else {
            break;
        };
        from = body_start + len;
        if let Some(decl) = parse_block(&html[body_start..body_start + len]) {
            if !out.contains(&decl) {
                out.push(decl);
            }
            if out.len() == MAX_FONT_FACES {
                break;
            }
        }
    }
    out
}

/// Length of the text up to the closing `}` of a block whose `{` was already consumed (quotes and `url()` honoured).
fn block_len(s: &str) -> Option<usize> {
    let (mut quote, mut paren) = (None::<char>, 0usize);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => paren += 1,
            (None, ')') => paren = paren.saturating_sub(1),
            (None, '}') if paren == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Split on `sep` outside quotes and parentheses (a `data:` URI contains both `;` and `,`).
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let (mut quote, mut paren, mut last) = (None::<char>, 0usize, 0usize);
    let mut parts = Vec::new();
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => paren += 1,
            (None, ')') => paren = paren.saturating_sub(1),
            (None, c) if c == sep && paren == 0 => {
                parts.push(&s[last..i]);
                last = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[last..]);
    parts
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.trim();
        }
    }
    s
}

fn parse_block(body: &str) -> Option<FontFaceDecl> {
    let (mut family, mut src_list, mut weight, mut italic) = (None, None, None, false);
    for decl in split_top(body, ';') {
        let Some((name, value)) = decl.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => family = Some(unquote(value).to_string()),
            "src" => src_list = Some(value),
            "font-weight" => {
                // `normal`, `bold`, `700`, or a range (`400 700`): the first number.
                let first = value.split_whitespace().next().unwrap_or("");
                weight = match first.to_ascii_lowercase().as_str() {
                    "normal" => Some(400.0),
                    "bold" => Some(700.0),
                    n => n.parse::<f32>().ok().filter(|w| (1.0..=1000.0).contains(w)),
                };
            }
            "font-style" => italic = value.to_ascii_lowercase().starts_with(['i', 'o']),
            _ => {}
        }
    }
    let family = family.filter(|f| !f.is_empty() && f.len() <= 100)?;
    let src = pick_source(src_list?)?;
    Some(FontFaceDecl {
        family,
        src,
        weight,
        italic,
    })
}

/// The first `url(...)` whose `format()` is one the renderer reads (or that has no `format()` and is not a WOFF 1 file).
fn pick_source(list: &str) -> Option<String> {
    for entry in split_top(list, ',') {
        let entry = entry.trim();
        let Some(rest) = entry.strip_prefix("url(").or_else(|| {
            entry
                .get(..4)
                .filter(|p| p.eq_ignore_ascii_case("url("))
                .map(|_| &entry[4..])
        }) else {
            continue;
        };
        let end = {
            let (mut quote, mut found) = (None::<char>, None);
            for (i, c) in rest.char_indices() {
                match (quote, c) {
                    (Some(q), c) if c == q => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(c),
                    (None, ')') => {
                        found = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            found?
        };
        let url = unquote(&rest[..end]).to_string();
        let after = rest[end + 1..].to_ascii_lowercase();
        let format = after
            .find("format(")
            .and_then(|i| after[i + 7..].split(')').next())
            .map(|f| unquote(f).to_string());
        let ok = match format.as_deref() {
            Some(
                "truetype"
                | "opentype"
                | "woff2"
                | "collection"
                | "truetype-variations"
                | "opentype-variations"
                | "woff2-variations",
            ) => true,
            Some(_) => false,
            None => url_looks_readable(&url),
        };
        if ok && !url.is_empty() {
            return Some(url);
        }
    }
    None
}

/// A source with no `format()`: judged by its extension (or, for a `data:` URI, its media type).
fn url_looks_readable(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    if let Some(rest) = path.strip_prefix("data:") {
        let media = rest.split([';', ',']).next().unwrap_or("");
        return !matches!(
            media,
            "font/woff"
                | "application/font-woff"
                | "application/vnd.ms-fontobject"
                | "image/svg+xml"
        );
    }
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some(ext) if ext.len() <= 5 && !ext.contains('/') => {
            matches!(ext, "ttf" | "otf" | "ttc" | "woff2")
        }
        _ => true, // no extension: let the first bytes decide
    }
}

/// True when `bytes` start like a font file the renderer reads: TrueType, OpenType, a collection, or WOFF2.
pub fn is_font(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true" | b"ttcf" | b"wOF2")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(css: &str) -> FontFaceDecl {
        let all = font_face_decls(&format!("<style>{css}</style>"));
        assert_eq!(all.len(), 1, "{css}: {all:?}");
        all.into_iter().next().unwrap()
    }

    #[test]
    fn reads_family_source_weight_and_style() {
        let d = one(
            "@font-face { font-family: 'Brand Sans'; src: url(\"fonts/brand.ttf\") format('truetype'); font-weight: bold; font-style: italic }",
        );
        assert_eq!(d.family, "Brand Sans");
        assert_eq!(d.src, "fonts/brand.ttf");
        assert_eq!(d.weight, Some(700.0));
        assert!(d.italic);
    }

    #[test]
    fn a_data_uri_with_semicolons_and_commas_survives() {
        let d = one(
            "@font-face{font-family:Zzz;src:url(data:font/ttf;base64,AAEAAAAA,BBB==) format('truetype');}",
        );
        assert_eq!(d.src, "data:font/ttf;base64,AAEAAAAA,BBB==");
        assert_eq!(d.weight, None);
    }

    #[test]
    fn picks_the_first_format_it_can_read() {
        let d = one(
            "@font-face{font-family:A;src:url(a.eot),url(a.woff) format('woff'),url(a.woff2) format('woff2'),url(a.ttf) format('truetype')}",
        );
        assert_eq!(d.src, "a.woff2");
        let d = one("@font-face{font-family:A;src:local('Arial'),url(a.woff),url(a.otf)}");
        assert_eq!(d.src, "a.otf");
    }

    #[test]
    fn rules_it_cannot_use_are_skipped() {
        assert!(
            font_face_decls(
                "<style>@font-face{font-family:A;src:url(a.woff) format('woff')}</style>"
            )
            .is_empty()
        );
        assert!(font_face_decls("<style>@font-face{src:url(a.ttf)}</style>").is_empty());
        assert!(font_face_decls("<style>@font-face{font-family:A}</style>").is_empty());
        assert!(font_face_decls("<p>no rules here</p>").is_empty());
    }

    #[test]
    fn weight_ranges_and_keywords() {
        assert_eq!(
            one("@font-face{font-family:A;src:url(a.ttf);font-weight:300 900}").weight,
            Some(300.0)
        );
        assert_eq!(
            one("@font-face{font-family:A;src:url(a.ttf);font-weight:normal}").weight,
            Some(400.0)
        );
        assert_eq!(
            one("@font-face{font-family:A;src:url(a.ttf);font-weight:9999}").weight,
            None
        );
    }

    #[test]
    fn several_rules_and_a_limit() {
        let css: String = (0..30)
            .map(|i| format!("@font-face{{font-family:F{i};src:url(f{i}.ttf)}}"))
            .collect();
        let all = font_face_decls(&css);
        assert_eq!(all.len(), MAX_FONT_FACES);
        assert_eq!(all[0].family, "F0");
    }

    #[test]
    fn font_magic() {
        assert!(is_font(&[0, 1, 0, 0, 9]));
        assert!(is_font(b"OTTOxxxx") && is_font(b"wOF2xxxx") && is_font(b"ttcfxxxx"));
        assert!(!is_font(b"wOFFxxxx") && !is_font(b"\x89PNG") && !is_font(b""));
    }
}
