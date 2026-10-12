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
