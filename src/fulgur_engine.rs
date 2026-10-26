//! In-process HTML → PDF rendering with Fulgur.
//!
//! Fonts come from fontconfig. The vendored Fulgur (see `vendor/fulgur/PATCHES.md`) builds the
//! font collection once per process; the Docker image points `FONTCONFIG_FILE` at
//! `.public/assets/fonts/fonts.conf`, a curated Noto set, rather than every installed font. Generic families
//! (`sans-serif`, `serif`, `monospace`) resolve through fontconfig's usual aliases.

use fulgur::{AssetBundle, Engine, PageSize};
use std::sync::Arc;

/// Page size in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMm {
    pub width: f32,
    pub height: f32,
}

/// The PDF/A level a request asks for. An unknown level, or a password (PDF/A forbids encryption), is `INVALID_FIELD`.
pub fn parse_pdf_a(
    level: Option<&str>,
    password: Option<&str>,
) -> Result<Option<fulgur::PdfA>, crate::error::AppError> {
    let Some(level) = level.filter(|l| !l.trim().is_empty()) else {
        return Ok(None);
    };
    let parsed = fulgur::PdfA::parse(level).ok_or_else(|| {
        crate::error::AppError::InvalidField(format!(
            "pdfA: `{level}` is not a PDF/A level (use 1b, 2b, 2u, 2a, 3b or 3a)"
        ))
    })?;
    if password.is_some_and(|p| !p.is_empty()) {
        return Err(crate::error::AppError::InvalidField(
            "pdfA: PDF/A forbids encryption, so it cannot be combined with a password".into(),
        ));
    }
    Ok(Some(parsed))
}

/// The current time as `YYYY-MM-DDThh:mm:ss`, UTC. WebAssembly has no clock: its callers pass the date in.
#[cfg(not(target_arch = "wasm32"))]
fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

#[cfg(target_arch = "wasm32")]
fn now_iso() -> String {
    "1970-01-01T00:00:00".to_string()
}

/// Marks a render error that is the document's fault (it cannot meet PDF/A), so callers answer it as `INVALID_FIELD`.
pub const PDF_A_PREFIX: &str = "pdfa: ";

/// What went wrong in a PDF/A validation failure, in words a template author can act on.
fn explain_pdf_a_failure(error: &str) -> String {
    let mut problems = Vec::new();
    if error.contains("ContainsNotDefGlyph") {
        problems.push("the text uses characters that no available font has (add a font that covers them, for example with @font-face)");
    }
    if error.contains("MissingAltText") {
        problems.push(
            "an image has no alt text (levels 2a and 3a need an alt attribute on every <img>)",
        );
    }
    if error.contains("MissingDocumentOutline") {
        problems.push("the document has no headings, so no outline");
    }
    if problems.is_empty() {
        problems.push("the document does not meet the standard");
    }
    format!(
        "{} ({})",
        problems.join("; "),
        error.chars().take(200).collect::<String>()
    )
}

/// The error for a failed render: a PDF/A problem is the caller's, anything else a render failure.
pub fn render_error(message: String) -> crate::error::AppError {
    match message.strip_prefix(PDF_A_PREFIX) {
        Some(detail) => crate::error::AppError::InvalidField(format!("pdfA: {detail}")),
        None => crate::error::AppError::Render(message),
    }
}

impl PageMm {
    /// [`parse`](Self::parse), with the error callers show to users.
    pub fn parse_request(
        width: &str,
        height: &str,
        orientation: &str,
    ) -> Result<Self, crate::error::AppError> {
        Self::parse(width, height, orientation).ok_or_else(|| {
            crate::error::AppError::InvalidField(
                "width/height/orientation: use lengths like 8.5in, 210mm, 21cm, 612pt and \
                 portrait or landscape"
                    .into(),
            )
        })
    }

    /// Parse the request's `width`/`height`/`orientation`. `None` for anything that isn't a
    /// positive length with a unit (`in`, `mm`, `cm`, `pt`, `pc`, `px`) or a known orientation.
    pub fn parse(width: &str, height: &str, orientation: &str) -> Option<Self> {
        let (mut w, mut h) = (length_mm(width)?, length_mm(height)?);
        match orientation.trim().to_ascii_lowercase().as_str() {
            "" | "portrait" => {}
            "landscape" if w < h => std::mem::swap(&mut w, &mut h),
            "landscape" => {}
            _ => return None,
        }
        Some(Self {
            width: w,
            height: h,
        })
    }
}

fn length_mm(s: &str) -> Option<f32> {
    let s = s.trim().to_ascii_lowercase();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(split);
    let n: f32 = num.parse().ok()?;
    let mm = match unit.trim() {
        "mm" => n,
        "cm" => n * 10.0,
        "in" => n * 25.4,
        "pt" => n * 25.4 / 72.0,
        "pc" => n * 25.4 / 6.0,
        "px" => n * 25.4 / 96.0,
        _ => return None,
    };
    (mm.is_finite() && (1.0..=5000.0).contains(&mm)).then_some(mm)
}

/// One document to render.
#[derive(Debug, Clone)]
pub struct RenderJob {
    pub html: String,
    pub page: PageMm,
    pub title: Option<String>,
    /// Image bytes keyed by the exact `<img src>` value they replace.
    pub images: Vec<(String, Arc<Vec<u8>>)>,
    /// Fonts from the document's `@font-face` rules, loaded.
    pub font_faces: Vec<crate::font_face::LoadedFontFace>,
    /// A tagged, accessible (PDF/UA) PDF: a structure tree, the document language and the title.
    pub accessible: bool,
    /// Archival PDF/A conformance level, if asked for.
    pub pdf_a: Option<fulgur::PdfA>,
    /// The creation date (ISO 8601) PDF/A requires. The browser build has no clock of its own and is given one;
    /// elsewhere `None` means now.
    pub date: Option<String>,
}

/// Something that turns HTML into PDF bytes synchronously, in-process.
/// Implemented by [`FulgurEngine`]; tests substitute misbehaving fakes.
pub trait InProcessRenderer: Send + Sync {
    fn render(&self, job: &RenderJob) -> Result<Vec<u8>, String>;

    /// Rediscover fonts (after fonts were added or removed). Renders already running keep
    /// the fonts they started with.
    fn reload_fonts(&self) {}

    /// Use these font files in addition to the system fonts, replacing any earlier set.
    fn set_extra_fonts(&self, _fonts: Vec<Vec<u8>>) {}
}

/// Defaults a template can override: the font stack (at zero specificity, so any `body { font-family }` wins) and no
/// page margin. The margin is set here, not on the engine, because a margin set on the engine would also switch off
/// the template's own `@page` margins and margin boxes (page numbers, headers and footers).
const BASE_CSS: &str = "@page { margin: 0; } :where(body) { font-family: 'Noto Sans', sans-serif, 'Noto Sans Math', 'Noto Sans CJK SC', 'Noto Color Emoji'; }";

/// Noto Sans Math (SIL Open Font License, see `.public/assets/fonts/OFL.txt`), cut down to the symbols the `math`
/// converter and everyday text use: operators, relations, arrows, big operators, brackets, blackboard and script letters.
/// It is always available, in the server, the Node client and the browser build alike.
const MATH_FONT: &[u8] = include_bytes!("NotoSansMath-Subset.ttf");

/// Noto Sans Regular and Bold (SIL Open Font License, `.public/assets/fonts/OFL.txt`): Latin, Cyrillic, Greek and
/// Vietnamese, the fonts of the browser package. Not in the WebAssembly build, which gets them from its page.
#[cfg(not(target_arch = "wasm32"))]
const BUNDLED_FONTS: [&[u8]; 2] = [
    include_bytes!("NotoSans-Regular.ttf"),
    include_bytes!("NotoSans-Bold.ttf"),
];

/// Lay out documents that need only Latin, Cyrillic, Greek, Vietnamese, currency and math text with fonts built into the
/// program, and look at the system's fonts only for a document that needs them. For the Node, Electron and Tauri
/// engine, where the system's fonts are many and enumerating them costs a lot of memory and time. The HTTP server does
/// not use it: its fonts are a curated set, and its output must not depend on this.
#[cfg(not(target_arch = "wasm32"))]
pub fn use_light_fonts() {
    fulgur::set_static_extra_fonts(&[MATH_FONT]);
    fulgur::set_static_fonts(&BUNDLED_FONTS);
}

pub struct FulgurEngine {
    assets: AssetBundle,
}

impl Default for FulgurEngine {
    fn default() -> Self {
        let mut assets = AssetBundle::new();
        assets.add_css(BASE_CSS);
        let engine = Self { assets };
        // The math font is there from the start (shared, not copied); fonts the host adds later join it.
        fulgur::set_static_extra_fonts(&[MATH_FONT]);
        engine
    }
}

impl FulgurEngine {
    /// One render. `pdf_ua` asks for a PDF/UA conforming file (which needs a document outline, so a page with no
    /// headings cannot claim it); `job.accessible` without it still gives a tagged PDF with a language and a title.
    fn render_once(&self, job: &RenderJob, pdf_ua: bool) -> Result<Vec<u8>, String> {
        let mut assets = self.assets.clone();
        for (src, data) in &job.images {
            // Same key normalisation as `AssetBundle::get_image`.
            let key = src.strip_prefix("./").unwrap_or(src);
            assets.images.insert(key.to_string(), data.clone());
        }
        let mut builder = Engine::builder()
            .page_size(PageSize::custom(job.page.width, job.page.height))
            .assets(assets)
            .system_fonts(true);
        if let Some(t) = &job.title {
            builder = builder.title(t.clone());
        }
        if job.accessible {
            // A tagged PDF needs a title and a language; a document that gives neither gets "Document" and English.
            if job.title.is_none() {
                builder = builder.title("Document");
            }
            builder = builder
                .tagged(true)
                .pdf_ua(pdf_ua)
                .lang(extract_lang(&job.html).unwrap_or_else(|| "en".to_string()));
        }
        if let Some(level) = job.pdf_a {
            if job.title.is_none() && !job.accessible {
                builder = builder.title("Document");
            }
            let date = job.date.clone().unwrap_or_else(now_iso);
            builder = builder
                .creation_date(date)
                .pdf_a(Some(level))
                .lang(extract_lang(&job.html).unwrap_or_else(|| "en".to_string()));
        }
        let faces: Vec<fulgur::DocFontFace> = job
            .font_faces
            .iter()
            .map(|f| fulgur::DocFontFace {
                data: Arc::clone(&f.data),
                family: f.family.clone(),
                weight: f.weight,
                italic: f.italic,
            })
            .collect();
        let interpolate = job.pdf_a.is_none();
        fulgur::with_image_interpolation(interpolate, || {
            fulgur::with_doc_font_faces(faces, || builder.build().render(&job.html))
        })
        .map_err(|e| e.to_string())
    }
}

impl InProcessRenderer for FulgurEngine {
    fn render(&self, job: &RenderJob) -> Result<Vec<u8>, String> {
        match self.render_once(job, job.accessible) {
            Err(e) if job.pdf_a.is_some() && e.contains("Validation") => {
                Err(format!("{PDF_A_PREFIX}{}", explain_pdf_a_failure(&e)))
            }
            // The PDF/UA check failed (typically: no headings, so no outline). Keep the tags and the language, drop the claim.
            Err(e) if job.accessible && e.contains("Validation") => self.render_once(job, false),
            other => other,
        }
    }

    fn reload_fonts(&self) {
        fulgur::reload_system_fonts();
    }

    fn set_extra_fonts(&self, fonts: Vec<Vec<u8>>) {
        fulgur::set_extra_fonts(fonts);
    }
}

/// The `lang` of the `<html>` element (`<html lang="fil">`), for the document language of an accessible PDF.
pub fn extract_lang(html: &str) -> Option<String> {
    let start = find_ascii_ci(html, "<html", 0)?;
    let tag_end = start + html[start..].find('>')?;
    let tag = &html[start..tag_end];
    let at = find_ascii_ci(tag, "lang=", 0)?;
    let rest = &tag[at + 5..];
    let (quote, rest) = match rest.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &rest[1..]),
        _ => (None, rest),
    };
    let end = rest
        .find(|c: char| quote.map_or(c.is_whitespace(), |q| c == q))
        .unwrap_or(rest.len());
    let lang = rest[..end].trim();
    (!lang.is_empty() && lang.len() <= 35).then(|| lang.to_string())
}

/// `<title>` text, found without allocating a lowercased copy of the document.
pub fn extract_title(html: &str) -> Option<String> {
    let start = find_ascii_ci(html, "<title", 0)?;
    let open_end = start + html[start..].find('>')? + 1;
    let end = find_ascii_ci(html, "</title", open_end)?;
    let t = html[open_end..end].trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Byte offset of the ASCII `needle` in `hay[from..]`, ignoring ASCII case.
fn find_ascii_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    hay.as_bytes()[from..]
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
        .map(|i| i + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_extraction() {
        assert_eq!(
            extract_title("<html><head><TITLE> Inv 1 </TITLE>"),
            Some("Inv 1".into())
        );
        assert_eq!(extract_title("<title></title>"), None);
        assert_eq!(extract_title("<p>no title</p>"), None);
    }

    #[test]
    fn parses_page_lengths() {
        let letter = PageMm::parse("8.5in", "11in", "portrait").unwrap();
        assert!((letter.width - 215.9).abs() < 0.01 && (letter.height - 279.4).abs() < 0.01);
        assert_eq!(
            PageMm::parse("210mm", "297mm", "landscape"),
            Some(PageMm {
                width: 297.0,
                height: 210.0
            })
        );
        assert_eq!(
            PageMm::parse("21cm", " 29.7CM ", ""),
            Some(PageMm {
                width: 210.0,
                height: 297.0
            })
        );
        assert!(PageMm::parse("612pt", "792pt", "portrait").is_some());
        assert!(PageMm::parse("816px", "1056px", "portrait").is_some());
    }

    #[test]
    fn rejects_non_lengths() {
        for (w, h, o) in [
            ("A4", "", ""),
            ("8.5", "11", ""),
            ("", "", ""),
            ("8.5in", "11in", "sideways"),
            ("0in", "11in", ""),
            ("-1in", "11in", ""),
            ("99999in", "11in", ""),
            ("calc(1in)", "11in", ""),
        ] {
            assert_eq!(PageMm::parse(w, h, o), None, "{w:?} {h:?} {o:?}");
        }
    }
}
