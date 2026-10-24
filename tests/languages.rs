//! Multi-language text: each script in `tests/fixtures/languages.html` must be drawn with a font
//! that has it, not with empty boxes.
//!
//! Needs the curated font set (`fonts/install.sh`, used by the Docker images and `make docker-test`).
//! Elsewhere, such as on macOS, the system fonts are used and some scripts are missing, so the test
//! skips itself when `FONTCONFIG_FILE` is not set.

mod common;

use common::*;

#[tokio::test(flavor = "multi_thread")]
async fn every_script_is_drawn_with_a_font_that_has_it() {
    if std::env::var_os("FONTCONFIG_FILE").is_none() {
        eprintln!("skipped: FONTCONFIG_FILE is not set (curated font set not in use)");
        return;
    }
    let app = TestApp::builder().spawn().await;
    let html = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/languages.html"
    ))
    .unwrap();

    let resp = app.to_bytes(form(&html)).await;
    assert_eq!(resp.status(), 200);
    let pdf = resp.bytes().await.unwrap();
    assert_eq!(page_count(&pdf), 1);

    let fonts = embedded_fonts(&pdf);
    for (script, font) in [
        ("Latin / Cyrillic / Greek", "NotoSans-Regular"),
        ("Chinese, Japanese, Korean", "NotoSansCJK"),
        ("Arabic / Urdu", "NotoNaskhArabic"),
        ("Hebrew", "NotoSansHebrew"),
        ("Devanagari (Hindi, Marathi)", "NotoSansDevanagari"),
        ("Bengali", "NotoSansBengali"),
        ("Tamil", "NotoSansTamil"),
        ("Telugu", "NotoSansTelugu"),
        ("Kannada", "NotoSansKannada"),
        ("Malayalam", "NotoSansMalayalam"),
        ("Gujarati", "NotoSansGujarati"),
        ("Gurmukhi (Punjabi)", "NotoSansGurmukhi"),
        ("Sinhala", "NotoSansSinhala"),
        ("Oriya", "NotoSansOriya"),
        ("Thai", "NotoSansThai"),
        ("Lao", "NotoSansLao"),
        ("Khmer", "NotoSansKhmer"),
        ("Myanmar", "NotoSansMyanmar"),
        ("Ethiopic", "NotoSansEthiopic"),
        ("Georgian", "NotoSansGeorgian"),
        ("Armenian", "NotoSansArmenian"),
        ("Thaana", "NotoSansThaana"),
        ("Syriac", "NotoSansSyriac"),
        ("Tibetan", "NotoSerifTibetan"),
        ("Cherokee", "NotoSansCherokee"),
        ("Canadian Aboriginal", "NotoSansCanadianAboriginal"),
        ("N'Ko", "NotoSansNKo"),
        ("Tifinagh", "NotoSansTifinagh"),
        ("Symbols (✗ ✘ and friends)", "NotoSansSymbols2"),
    ] {
        assert!(
            fonts.iter().any(|f| f.starts_with(font)),
            "{script}: no {font} embedded; fonts used: {fonts:?}"
        );
    }
}
