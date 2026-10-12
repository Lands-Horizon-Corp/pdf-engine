//! CSS coverage: proves every design the router sends to Fulgur looks the same as Prince.
//!
//! `tests/fixtures/probes/*.html` each isolate one CSS feature on plain coloured boxes (no
//! text, since the engines' font rasterization legitimately differs). For every probe:
//! - the router's decision must match the evidence recorded below, and
//! - if the router picks Fulgur, Fulgur's raster must match Prince's raster, both in pixel
//!   positions and in colours (a black border where Prince draws blue fails the colour check
//!   even though it covers few pixels).
//!
//! `tests/fixtures/css/*.html` are realistic documents rendered end to end through the app.
//!
//! Parity tests need `prince` and `pdftoppm` (poppler-utils) and are skipped without them
//! unless `REQUIRE_PRINCE=1`.

mod common;

use common::*;
use pdf_engine::{fulgur_engine::PageMm, models::EnginePref, pdf, prince, router};
use std::{collections::HashMap, path::Path, process::Command};

/// Probes Fulgur renders like Prince; the router must keep them on Fulgur.
const FULGUR_PROBES: &[&str] = &[
    "bg_hex",
    "bg_linear_gradient",
    "bg_radial_gradient",
    "bg_rgba",
    "block_margin_padding",
    "border_black",
    "border_dashed",
    "border_dotted",
    "border_double",
    "border_longhand_color",
    "border_radius",
    "border_radius_bordered",
    "border_shorthand_hex",
    "border_shorthand_named",
    "border_shorthand_rgb",
    "border_side_black",
    "border_top_color_longhand",
    "box_shadow",
    "box_sizing",
    "color_named",
    "color_rgb_modern",
    "css_variables_calc",
    "flex_grow",
    "flex_row",
    "grid_cols",
    "inline_svg",
    "nth_child",
    "opacity_block",
    "page_break",
    "position_absolute",
    "position_relative",
    "table_cell_bg",
    "table_colspan",
    "table_collapse_black",
    "table_collapse_colored",
    "transform_block",
    "transform_translate",
];

/// Probes Fulgur 0.40 renders wrongly (or that we route away conservatively); the router
/// must send them to Prince.
const PRINCE_PROBES: &[&str] = &[
    // missing or wrong output
    "bg_hsl",
    "color_hsla",
    "bg_image_data_uri",
    "img_data_png",
    "img_data_svg",
    "border_side_hex",
    "border_left_color",
    "border_four_sides_colors",
    "table_row_bottom_border",
    "span_background",
    "outline",
    "z_index",
    "float_left",
    "table_rowspan",
    "before_after_content",
    "media_print",
    "multicolumn_rule",
    // wrapping rows of inline-blocks vanish entirely, so every inline-block goes to Prince
    "inline_block_many_wrapping",
    "inline_block_div",
    "inline_block_span",
    "inline_block_padding",
    "inline_block_valign_top",
    "inline_block_margin",
    "inline_block_with_text",
    "inline_block_bordered",
    "inline_block_class_style",
    "inline_block_class_nostyle_text",
];

fn probe_dir() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/probes"
    ))
}

fn read_probe(name: &str) -> String {
    std::fs::read_to_string(probe_dir().join(format!("{name}.html"))).unwrap()
}

#[test]
fn every_probe_is_classified() {
    let mut on_disk: Vec<String> = std::fs::read_dir(probe_dir())
        .unwrap()
        .map(|e| {
            e.unwrap()
                .path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    on_disk.sort();
    let mut listed: Vec<String> = FULGUR_PROBES
        .iter()
        .chain(PRINCE_PROBES)
        .map(|s| s.to_string())
        .collect();
    listed.sort();
    assert_eq!(
        on_disk, listed,
        "add new probes to FULGUR_PROBES or PRINCE_PROBES"
    );
}

#[test]
fn router_matches_probe_evidence() {
    for name in FULGUR_PROBES {
        assert_eq!(
            router::prince_reason(&read_probe(name)),
            None,
            "{name} should stay on Fulgur"
        );
    }
    for name in PRINCE_PROBES {
        assert!(
            router::prince_reason(&read_probe(name)).is_some(),
            "{name} should go to Prince"
        );
    }
}

// ---- raster comparison ----

struct Raster {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

fn tools_available() -> bool {
    real_prince_available()
        && Command::new("pdftoppm")
            .arg("-v")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

macro_rules! require_tools {
    () => {
        if !tools_available() {
            if std::env::var("REQUIRE_PRINCE").as_deref() == Ok("1") {
                panic!("REQUIRE_PRINCE=1 but prince/pdftoppm are not on PATH");
            }
            eprintln!("SKIPPED: needs prince and pdftoppm");
            return;
        }
    };
}

/// Rasterize page 1 at 40 dpi into RGB.
fn rasterize(pdf: &[u8]) -> Raster {
    let dir = tempfile::tempdir().unwrap();
    let input = write_file(dir.path(), "in.pdf", pdf);
    let out = dir.path().join("out");
    let st = Command::new("pdftoppm")
        .args(["-r", "40", "-f", "1", "-l", "1", "-singlefile"])
        .arg(&input)
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success());
    parse_ppm(&std::fs::read(dir.path().join("out.ppm")).unwrap())
}

fn parse_ppm(b: &[u8]) -> Raster {
    // "P6\n<w> <h>\n255\n" followed by raw RGB.
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&b[start..i]).unwrap().to_string());
    }
    assert_eq!(fields[0], "P6");
    let (w, h) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    Raster {
        w,
        h,
        rgb: b[i + 1..].to_vec(),
    }
}

struct Parity {
    /// Fraction of pixels whose colour differs noticeably.
    pixel_diff: f64,
    /// Worst count ratio (0..1) over significant non-white colour bins.
    palette_ratio: f64,
    worst_bin: String,
}

fn compare(a: &Raster, b: &Raster) -> Parity {
    assert_eq!((a.w, a.h), (b.w, b.h), "page sizes differ");
    let n = a.w * a.h;
    let differing = (0..n)
        .filter(|&p| (0..3).any(|c| a.rgb[p * 3 + c].abs_diff(b.rgb[p * 3 + c]) > 64))
        .count();

    // Colour check by hue family: anti-aliasing turns black into grey and shifts shades,
    // but a border Prince draws blue must not come out black (or missing) in Fulgur.
    let hist = |r: &Raster| {
        let mut m: HashMap<usize, usize> = HashMap::new();
        for px in r.rgb.chunks_exact(3) {
            if let Some(h) = hue_bucket(px[0], px[1], px[2]) {
                *m.entry(h).or_default() += 1;
            }
        }
        m
    };
    let (ha, hb) = (hist(a), hist(b));
    let mut worst = (1.0, String::new());
    for bucket in ha.keys().chain(hb.keys()) {
        let (x, y) = (*ha.get(bucket).unwrap_or(&0), *hb.get(bucket).unwrap_or(&0));
        if x.max(y) < 40 {
            continue; // anti-aliasing noise
        }
        let ratio = x.min(y) as f64 / x.max(y) as f64;
        if ratio < worst.0 {
            worst = (ratio, format!("hue {}° prince={x} fulgur={y}", bucket * 30));
        }
    }
    Parity {
        pixel_diff: differing as f64 / n as f64,
        palette_ratio: worst.0,
        worst_bin: worst.1,
    }
}

/// Hue in 30° buckets for saturated pixels; `None` for white, grey and black.
fn hue_bucket(r: u8, g: u8, b: u8) -> Option<usize> {
    let (r, g, b) = (r as f64, g as f64, b as f64);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max < 40.0 || (max - min) / max < 0.3 {
        return None;
    }
    let d = max - min;
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Some((h / 30.0) as usize % 12)
}

/// Fulgur-safe probes measure <= 0.0037; a flipped z-order already measures 0.0059.
const MAX_PIXEL_DIFF: f64 = 0.004;
const MIN_PALETTE_RATIO: f64 = 0.5;

async fn render_both(html: &str) -> (Vec<u8>, Vec<u8>) {
    let letter = PageMm::parse("8.5in", "11in", "portrait").unwrap();
    let f = real_fulgur().render(html, letter, None).unwrap();
    let raw = prince::render(html, "8.5in", "11in", "portrait", "prince")
        .await
        .unwrap();
    let p = pdf::finalize(raw, true, None).await.unwrap().to_vec();
    (f, p)
}

#[tokio::test]
async fn fulgur_routed_probes_match_prince() {
    require_tools!();
    let mut failures = Vec::new();
    for name in FULGUR_PROBES {
        let (f, p) = render_both(&read_probe(name)).await;
        assert_eq!(page_count(&f), page_count(&p), "{name}: page count");
        let m = compare(&rasterize(&p), &rasterize(&f));
        println!(
            "{name:28} pixel_diff={:.4} palette_ratio={:.2} {}",
            m.pixel_diff, m.palette_ratio, m.worst_bin
        );
        if m.pixel_diff > MAX_PIXEL_DIFF || m.palette_ratio < MIN_PALETTE_RATIO {
            failures.push(format!(
                "{name}: {:.4} / {:.2} {}",
                m.pixel_diff, m.palette_ratio, m.worst_bin
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "Fulgur differs from Prince:\n{}",
        failures.join("\n")
    );
}

/// Not an assertion: reports which Prince-routed probes Fulgur now gets right, so the router
/// can be relaxed after a Fulgur upgrade. Run with `--nocapture`.
#[tokio::test]
async fn report_fulgur_gaps() {
    require_tools!();
    for name in PRINCE_PROBES {
        let (f, p) = render_both(&read_probe(name)).await;
        let m = compare(&rasterize(&p), &rasterize(&f));
        let ok = m.pixel_diff <= MAX_PIXEL_DIFF && m.palette_ratio >= MIN_PALETTE_RATIO;
        println!(
            "{} {name:30} pixel_diff={:.4} palette_ratio={:.2}",
            if ok { "NOW MATCHES" } else { "still differs" },
            m.pixel_diff,
            m.palette_ratio
        );
    }
}

// ---- realistic documents ----

/// Expected page counts (Prince's output) for the realistic fixtures.
const FIXTURES: &[(&str, usize)] = &[
    ("01_typography", 1),
    ("02_unicode", 1),
    ("03_colors_backgrounds", 1),
    ("04_box_model", 1),
    ("05_flexbox", 1),
    ("06_grid", 1),
    ("07_tables", 1),
    ("08_invoice", 1),
    ("09_page_breaks", 3),
    ("10_page_margin_boxes", 2),
    ("11_running_headers", 1),
    ("12_positioning", 1),
    ("13_floats_inline", 1),
    ("14_images_svg", 1),
    ("15_lists_counters", 1),
    ("16_transforms_opacity", 1),
    ("17_multicolumn", 1),
    ("18_variables_calc_media", 1),
    ("19_pseudo_selectors", 1),
    ("20_long_table_multipage", 4),
];

#[tokio::test(flavor = "multi_thread")]
async fn realistic_fixtures_render_end_to_end() {
    require_prince!();
    let app = TestApp::builder("prince")
        .default_engine(EnginePref::Auto)
        .spawn()
        .await;
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/css"));

    for (name, pages) in FIXTURES {
        let html = std::fs::read_to_string(dir.join(format!("{name}.html"))).unwrap();
        let resp = app.to_bytes(form(&html)).await;
        assert_eq!(resp.status(), 200, "{name}");
        let engine = resp.headers()["x-pdf-engine"].to_str().unwrap().to_string();
        let expected = if router::prince_reason(&html).is_some() {
            "prince"
        } else {
            "fulgur"
        };
        let body = resp.bytes().await.unwrap();
        println!("{name:26} {engine:7} {:?}", router::prince_reason(&html));

        assert!(
            engine == expected || (engine == "prince" && html.contains("<thead")),
            "{name}"
        );
        assert_eq!(page_count(&body), *pages, "{name}: page count");
    }
}
