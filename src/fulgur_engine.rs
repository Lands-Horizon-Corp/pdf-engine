//! In-process HTML → PDF rendering with Fulgur.
//!
//! Fonts come from fontconfig. The vendored Fulgur (see `vendor/fulgur/PATCHES.md`) builds the
//! font collection once per process; the Docker image points `FONTCONFIG_FILE` at
//! `fonts/fonts.conf`, a curated Noto set, rather than every installed font. Generic families
//! (`sans-serif`, `serif`, `monospace`) resolve through fontconfig's usual aliases.

use fulgur::{AssetBundle, Engine, Margin, PageSize};

/// Page size in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMm {
    pub width: f32,
    pub height: f32,
}

impl PageMm {
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

/// Something that turns HTML into PDF bytes synchronously, in-process.
/// Implemented by [`FulgurEngine`]; tests substitute misbehaving fakes.
pub trait InProcessRenderer: Send + Sync {
    fn render(&self, html: &str, page: PageMm, title: Option<&str>) -> Result<Vec<u8>, String>;
}

/// Default font stack, at zero specificity so any template `body { font-family }` wins.
const BASE_CSS: &str = ":where(body) { font-family: 'Noto Sans', 'Noto Sans CJK SC', 'Noto Color Emoji', sans-serif; }";

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
    fn render(&self, html: &str, page: PageMm, title: Option<&str>) -> Result<Vec<u8>, String> {
        let mut builder = Engine::builder()
            .page_size(PageSize::custom(page.width, page.height))
            .margin(Margin::uniform(0.0))
            .assets(self.assets.clone())
            .system_fonts(true);
        if let Some(t) = title {
            builder = builder.title(t);
        }
        builder.build().render(html).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
