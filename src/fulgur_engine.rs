//! In-process HTML → PDF rendering with Fulgur.
//!
//! Fonts: Fulgur rescans every system font on each render when `system_fonts` is on (~35 ms
//! with the ~2,300 Noto files in our image). We instead bundle a curated Noto set (~8 ms)
//! and only fall back to system fonts when the document contains a character those fonts
//! don't cover (CJK, emoji, Arabic, ...), which we know from their `cmap` tables.

use fulgur::{AssetBundle, Engine, Margin, PageSize};
use std::path::Path;

/// Page size in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMm {
    pub width: f32,
    pub height: f32,
}

impl PageMm {
    /// Parse the request's `width`/`height`/`orientation` the way Prince's `@page size`
    /// understands them. `None` when Fulgur can't represent it (Prince will be used).
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

/// Same default font stack the Prince path injects, at zero specificity so any template
/// `body { font-family }` wins.
const BASE_CSS: &str = ":where(body) { font-family: 'Noto Sans', 'Noto Sans CJK SC', 'Noto Color Emoji', sans-serif; }";

/// Bundled for the fast path. Missing files are skipped.
const BUNDLED_FONTS: &[&str] = &[
    "NotoSans-Regular.ttf",
    "NotoSans-Italic.ttf",
    "NotoSans-Light.ttf",
    "NotoSans-LightItalic.ttf",
    "NotoSans-Medium.ttf",
    "NotoSans-SemiBold.ttf",
    "NotoSans-Bold.ttf",
    "NotoSans-BoldItalic.ttf",
    "NotoSans-ExtraBold.ttf",
    "NotoSans-Black.ttf",
    "NotoSerif-Regular.ttf",
    "NotoSerif-Italic.ttf",
    "NotoSerif-Bold.ttf",
    "NotoSerif-BoldItalic.ttf",
    "NotoSansMono-Medium.ttf",
    "NotoSansMono-ExtraBold.ttf",
];

pub struct FulgurEngine {
    bundled: AssetBundle,
    system: AssetBundle,
    /// Bitset of codepoints the bundled fonts cover.
    coverage: Vec<u64>,
}

impl FulgurEngine {
    /// Load the bundled fonts from `font_dir`. With no usable fonts every render uses
    /// system fonts (correct, just slower).
    pub fn from_font_dir(font_dir: &Path) -> Self {
        let mut bundled = AssetBundle::new();
        let mut system = AssetBundle::new();
        bundled.add_css(BASE_CSS);
        system.add_css(BASE_CSS);
        let mut coverage = vec![0u64; 0x110000 / 64];
        let mut loaded = 0;
        for name in BUNDLED_FONTS {
            let Ok(bytes) = std::fs::read(font_dir.join(name)) else {
                continue;
            };
            let Ok(face) = ttf_parser::Face::parse(&bytes, 0) else {
                tracing::warn!("Skipping unparseable font {name}");
                continue;
            };
            if let Some(cmap) = face.tables().cmap {
                for sub in cmap.subtables.into_iter().filter(|s| s.is_unicode()) {
                    sub.codepoints(|cp| {
                        if let Some(word) = coverage.get_mut(cp as usize / 64) {
                            *word |= 1 << (cp % 64);
                        }
                    });
                }
            }
            if bundled.add_font_bytes(bytes).is_ok() {
                loaded += 1;
            }
        }
        if loaded == 0 {
            tracing::warn!(
                "No bundled fonts found in {}; Fulgur will use system fonts for every render",
                font_dir.display()
            );
            coverage.fill(0);
        } else {
            tracing::info!("Fulgur: bundled {loaded} fonts from {}", font_dir.display());
        }
        Self {
            bundled,
            system,
            coverage,
        }
    }

    fn covered(&self, c: char) -> bool {
        let cp = c as usize;
        self.coverage[cp / 64] & (1 << (cp % 64)) != 0
    }

    /// Can the bundled fonts draw every character in `html`?
    pub fn bundled_fonts_cover(&self, html: &str) -> bool {
        html.chars()
            .all(|c| matches!(c, '\n' | '\r' | '\t') || self.covered(c))
    }
}

impl InProcessRenderer for FulgurEngine {
    fn render(&self, html: &str, page: PageMm, title: Option<&str>) -> Result<Vec<u8>, String> {
        let use_bundle = self.bundled_fonts_cover(html);
        let mut builder = Engine::builder()
            .page_size(PageSize::custom(page.width, page.height))
            .margin(Margin::uniform(0.0))
            .assets(if use_bundle {
                self.bundled.clone()
            } else {
                self.system.clone()
            })
            .system_fonts(!use_bundle);
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
    fn rejects_what_only_prince_understands() {
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
