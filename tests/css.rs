//! CSS coverage for the Fulgur renderer.
//!
//! - `tests/fixtures/css/*.html`: 20 realistic documents (typography, tables, flex/grid,
//!   invoices, page breaks, margin boxes, images, ...). Each must render, with the page count
//!   a browser-grade print engine (PrinceXML) produced for it.
//! - `tests/fixtures/probes/*.html`: one CSS feature each. Each must render without error or
//!   panic. Which of them Fulgur draws differently from a browser is listed in the README.

mod common;

use common::*;
use std::path::Path;

/// Expected page counts for the realistic fixtures.
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

fn fixture_dir(sub: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(sub)
}

#[tokio::test(flavor = "multi_thread")]
async fn realistic_fixtures_render_with_expected_page_counts() {
    let app = TestApp::builder().spawn().await;

    for (name, pages) in FIXTURES {
        let html =
            std::fs::read_to_string(fixture_dir("css").join(format!("{name}.html"))).unwrap();
        let resp = app.to_bytes(form(&html)).await;
        assert_eq!(resp.status(), 200, "{name}");
        assert_eq!(
            page_count(&resp.bytes().await.unwrap()),
            *pages,
            "{name}: page count"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_css_probe_renders() {
    let app = TestApp::builder().spawn().await;
    let mut probes: Vec<_> = std::fs::read_dir(fixture_dir("probes"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    probes.sort();
    assert!(probes.len() >= 50, "probe fixtures missing");

    for path in probes {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let html = std::fs::read_to_string(&path).unwrap();
        let resp = app.to_bytes(form(&html)).await;
        assert_eq!(resp.status(), 200, "{name}");
        let expected = if name == "page_break" { 2 } else { 1 };
        assert_eq!(page_count(&resp.bytes().await.unwrap()), expected, "{name}");
    }
}

// ---- patched Fulgur gaps (vendor/fulgur/PATCHES.md) ----

async fn colors_of(app: &TestApp, html: &str) -> Vec<String> {
    let resp = app.to_bytes(form(html)).await;
    assert_eq!(resp.status(), 200);
    color_ops(&resp.bytes().await.unwrap())
}

#[tokio::test]
async fn hsl_and_hsla_colors_are_painted() {
    let app = TestApp::builder().spawn().await;

    let ops = colors_of(
        &app,
        r#"<div style="height:20px;background:hsl(0, 100%, 50%)"></div>
           <div style="height:20px;background:hsla(240, 100%, 50%, 0.5)"></div>
           <div style="height:20px;background:hsl(120deg 100% 25%)"></div>"#,
    )
    .await;

    for expected in ["1 0 0 rg", "0 0 1 rg", "0 0.5019608 0 rg"] {
        assert!(
            ops.iter().any(|o| o == expected),
            "missing {expected} in {ops:?}"
        );
    }
}

#[tokio::test]
async fn each_border_side_keeps_its_own_color() {
    let app = TestApp::builder().spawn().await;

    let ops = colors_of(
        &app,
        r#"<div style="width:100px;height:40px;border-style:solid;border-width:4px;
                       border-color:#ff0000 #00ff00 #0000ff #ffff00"></div>"#,
    )
    .await;

    for expected in ["1 0 0 RG", "0 1 0 RG", "0 0 1 RG", "1 1 0 RG"] {
        assert!(
            ops.iter().any(|o| o == expected),
            "missing {expected} in {ops:?}"
        );
    }
}

#[tokio::test]
async fn single_side_border_color_is_not_black() {
    let app = TestApp::builder().spawn().await;

    let ops = colors_of(
        &app,
        r#"<table style="border-collapse:collapse"><tr><td style="border-bottom:2px solid #00ff00">a</td></tr></table>
           <div style="height:20px;border-left:6px solid rgb(255, 0, 0)"></div>"#,
    )
    .await;

    assert!(ops.iter().any(|o| o == "0 1 0 RG"), "{ops:?}");
    assert!(ops.iter().any(|o| o == "1 0 0 RG"), "{ops:?}");
    assert!(
        !ops.iter().any(|o| o == "0 0 0 RG"),
        "a colored border was drawn black: {ops:?}"
    );
}
