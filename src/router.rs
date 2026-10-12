//! Decides which engine renders a document.
//!
//! Fulgur (in-process, fast) is the default. Templates that use CSS Fulgur 0.40 is known to
//! render wrongly are sent to Prince instead. Every rule below comes from a probe in
//! `tests/fixtures/probes` that renders visibly differently in Fulgur than in Prince; the
//! `css_parity` suite keeps this list honest.
//!
//! Detection is a conservative text scan of the rendered HTML: it reads `<style>` blocks and
//! inline `style` attributes, so a false positive only costs speed (Prince renders it), never
//! correctness. Styles applied to inline elements through classes are not detected.

/// Why a document must go to Prince, or `None` if Fulgur can render it.
pub fn prince_reason(html: &str) -> Option<&'static str> {
    let lower = html.to_ascii_lowercase();

    for (needle, reason) in HTML_RULES {
        if lower.contains(needle) {
            return Some(reason);
        }
    }
    if lower.contains("<svg") && lower.contains("<text") {
        return Some("svg <text>");
    }

    for block in style_blocks(&lower) {
        for (needle, reason) in STYLESHEET_RULES {
            if block.contains(needle) {
                return Some(reason);
            }
        }
        if let Some(r) = declarations(block).find_map(|(p, v)| declaration_reason(p, v)) {
            return Some(r);
        }
    }

    for (tag, style) in style_attributes(&lower) {
        for (prop, value) in declarations(style) {
            if let Some(r) = declaration_reason(prop, value) {
                return Some(r);
            }
            if INLINE_TAGS.contains(&tag) && prop.starts_with("background") {
                return Some("background on inline element");
            }
        }
    }
    None
}

/// Substrings of the whole (lowercased) document that force Prince.
const HTML_RULES: &[(&str, &str)] = &[
    ("rowspan", "table rowspan"),
    ("<mark", "<mark> background"),
    ("src=\"data:", "data: URI image"),
    ("src='data:", "data: URI image"),
    ("src=data:", "data: URI image"),
];

/// Substrings of a `<style>` block that force Prince.
const STYLESHEET_RULES: &[(&str, &str)] = &[
    ("@page", "@page rules"),
    ("@media", "@media queries"),
    ("@font-face", "@font-face"),
    ("@import", "@import"),
    (":before", "::before/::after"),
    (":after", "::before/::after"),
    (":first-letter", "::first-letter/line"),
    (":first-line", "::first-letter/line"),
];

const INLINE_TAGS: &[&str] = &[
    "span", "a", "b", "strong", "em", "i", "u", "s", "small", "code", "label", "sub", "sup",
    "abbr", "cite", "q", "time", "var", "kbd", "samp",
];

const SIDE_BORDERS: &[&str] = &["border-top", "border-right", "border-bottom", "border-left"];

const NON_COLOR_WORDS: &[&str] = &[
    "none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset",
    "thin", "medium", "thick", "inherit", "initial", "unset", "revert",
];

fn declaration_reason(prop: &str, value: &str) -> Option<&'static str> {
    let is_none = matches!(value, "none" | "0" | "initial" | "unset");
    if value.contains("hsl(") || value.contains("hsla(") {
        return Some("hsl() colors");
    }
    if value.contains("url(") {
        return Some("url() resources");
    }
    if value.contains("counter(") || value.contains("counters(") {
        return Some("CSS counters");
    }
    if value.contains("running(") || value.contains("element(") {
        return Some("running elements");
    }
    match prop {
        "float" if value != "none" => Some("floats"),
        "z-index" => Some("z-index"),
        "display" if value.starts_with("inline-") => Some("inline-block/flex/grid"),
        "text-transform" if value != "none" => Some("text-transform"),
        "position" if value == "fixed" => Some("position: fixed"),
        "counter-reset" | "counter-increment" | "counter-set" => Some("CSS counters"),
        "columns" | "column-count" | "column-width" | "column-span" | "column-fill" => {
            Some("multi-column")
        }
        p if p.starts_with("column-rule") => Some("multi-column"),
        p if p.starts_with("outline") && !is_none => Some("outline"),
        "border-color" if value.split_whitespace().count() > 1 => Some("per-side border colors"),
        p if SIDE_BORDERS.contains(&p) && has_non_black_color(value) => {
            Some("colored single-side border")
        }
        _ => None,
    }
}

fn has_non_black_color(value: &str) -> bool {
    value.split_whitespace().any(|tok| {
        let is_length = tok.starts_with(|c: char| c.is_ascii_digit() || c == '.');
        let is_black = matches!(tok, "black" | "#000" | "#000000" | "currentcolor");
        !is_length && !is_black && !NON_COLOR_WORDS.contains(&tok)
    })
}

/// Contents of every `<style>...</style>` block.
fn style_blocks(lower: &str) -> impl Iterator<Item = &str> {
    let mut rest = lower;
    std::iter::from_fn(move || {
        let start = rest.find("<style")?;
        let open_end = start + rest[start..].find('>')? + 1;
        let close = rest[open_end..]
            .find("</style")
            .map_or(rest.len(), |i| open_end + i);
        let block = &rest[open_end..close];
        rest = &rest[close..];
        Some(block)
    })
}

/// `(tag_name, style_attribute_value)` for every element with an inline style.
fn style_attributes(lower: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut rest = lower;
    std::iter::from_fn(move || {
        loop {
            let at = rest.find("style=")?;
            let tag_start = rest[..at].rfind('<').unwrap_or(0);
            let tag = rest[tag_start..]
                .trim_start_matches('<')
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or("");
            let after = &rest[at + "style=".len()..];
            let (value, consumed) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let end = after[1..].find(q).map_or(after.len(), |i| i + 1);
                    (&after[1..end], end)
                }
                _ => {
                    let end = after.find([' ', '>']).unwrap_or(after.len());
                    (&after[..end], end)
                }
            };
            rest = &after[consumed..];
            if !value.is_empty() {
                return Some((tag, value));
            }
        }
    })
}

/// `(property, value)` pairs from CSS text. Selectors and at-rule preludes produce junk pairs
/// that simply never match a rule.
fn declarations(css: &str) -> impl Iterator<Item = (&str, &str)> {
    css.split([';', '{', '}'])
        .filter_map(|decl| decl.split_once(':'))
        .map(|(p, v)| {
            let v = v.trim().trim_end_matches("!important").trim();
            (p.trim(), v)
        })
}

#[cfg(test)]
mod tests {
    use super::prince_reason;

    #[test]
    fn plain_documents_use_fulgur() {
        for html in [
            "<p>hello</p>",
            "<div style=\"display:flex;gap:10px\"><div style=\"flex:1;background:#123\">x</div></div>",
            "<style>table{border-collapse:collapse} td{border:1px solid #ccc}</style><table><tr><td>a</td></tr></table>",
            "<div style=\"border-bottom:2px solid #000;border-top: 1px solid black\">x</div>",
            "<div style=\"position:absolute;top:0;transform:rotate(3deg);opacity:.5\">x</div>",
            "<style>.g{display:grid;grid-template-columns:1fr 2fr;column-gap:8px}</style>",
            "<div style=\"float:none;outline:none;text-transform:none\">x</div>",
            "<p>Metadata: about float: left and z-index in prose</p>",
        ] {
            assert_eq!(prince_reason(html), None, "{html}");
        }
    }

    #[test]
    fn unsupported_features_use_prince() {
        for (html, reason) in [
            (
                "<img src=\"data:image/png;base64,AAAA\">",
                "data: URI image",
            ),
            (
                "<div style=\"background:url('data:image/svg+xml,x')\"></div>",
                "url() resources",
            ),
            ("<div style=\"float: left\"></div>", "floats"),
            ("<div style=\"z-index:2\"></div>", "z-index"),
            (
                "<div style=\"display: inline-block\"></div>",
                "inline-block/flex/grid",
            ),
            (
                "<style>.b{display:inline-flex}</style>",
                "inline-block/flex/grid",
            ),
            ("<style>@page { size: A4 }</style>", "@page rules"),
            (
                "<style>@media print { .x { color: red } }</style>",
                "@media queries",
            ),
            ("<style>.q::before{content:'x'}</style>", "::before/::after"),
            ("<style>li:after{content:''}</style>", "::before/::after"),
            (
                "<style>h1::first-letter{color:red}</style>",
                "::first-letter/line",
            ),
            (
                "<div style=\"color:hsl(10,50%,50%)\"></div>",
                "hsl() colors",
            ),
            (
                "<div style=\"border-bottom:1px solid #e5e7eb\"></div>",
                "colored single-side border",
            ),
            (
                "<style>td{border-left: 4px solid red}</style>",
                "colored single-side border",
            ),
            (
                "<div style=\"border-color:red blue\"></div>",
                "per-side border colors",
            ),
            ("<div style=\"outline:2px solid #000\"></div>", "outline"),
            (
                "<div style=\"text-transform:uppercase\"></div>",
                "text-transform",
            ),
            ("<div style=\"column-count:3\"></div>", "multi-column"),
            ("<div style=\"position:fixed\"></div>", "position: fixed"),
            ("<style>ol{counter-reset:s}</style>", "CSS counters"),
            (
                "<table><tr><td rowspan=\"2\"></td></tr></table>",
                "table rowspan",
            ),
            ("<p><mark>hi</mark></p>", "<mark> background"),
            (
                "<p><span style=\"background:yellow\">hi</span></p>",
                "background on inline element",
            ),
            ("<svg><text x=\"1\">hi</text></svg>", "svg <text>"),
        ] {
            assert_eq!(prince_reason(html), Some(reason), "{html}");
        }
    }

    #[test]
    fn detection_is_case_and_whitespace_insensitive() {
        assert!(prince_reason("<DIV STYLE='Float : Left'></DIV>").is_some());
        assert!(prince_reason("<div style=\"z-index : 1 !important\"></div>").is_some());
        assert!(prince_reason("<STYLE>@PAGE{}</STYLE>").is_some());
    }
}
