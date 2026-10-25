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
const BASE_CSS: &str = "@page { margin: 0; } :where(body) { font-family: 'Noto Sans', 'Noto Sans CJK SC', 'Noto Color Emoji', sans-serif; }";

pub struct FulgurEngine {
    assets: AssetBundle,
}

impl Default for FulgurEngine {
    fn default() -> Self {
        let mut assets = AssetBundle::new();
        assets.add_css(BASE_CSS);
        Self { assets }
    }
}

impl InProcessRenderer for FulgurEngine {
    fn render(&self, job: &RenderJob) -> Result<Vec<u8>, String> {
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
        builder.build().render(&job.html).map_err(|e| e.to_string())
    }

    fn reload_fonts(&self) {
        fulgur::reload_system_fonts();
    }

    fn set_extra_fonts(&self, fonts: Vec<Vec<u8>>) {
        fulgur::set_extra_fonts(fonts);
    }
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
