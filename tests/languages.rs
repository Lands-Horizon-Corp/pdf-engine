//! Multi-language text: each script below must be drawn with a font that has it, not with empty
//! boxes.
//!
//! Needs the curated font set (`fonts/install.sh`, used by the Docker images and `make docker-test`).
//! Elsewhere, such as on macOS, the system fonts are used and some scripts are missing, so the test
//! skips itself when `FONTCONFIG_FILE` is not set.

mod common;

use common::*;

/// One line per script.
const LANGUAGES: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
body{font-family:'Noto Sans',sans-serif;font-size:13px}</style></head><body>
<p>Hello, world! ¡Hola, mundo! Grüße Привет, мир! Γειά σου Κόσμε! Xin chào ạ ế ộ</p>
<p>你好，世界！利潤表 · こんにちは世界 · 안녕하세요</p>
<p dir="rtl">مرحبا بالعالم · ہیلو دنیا</p><p dir="rtl">שלום עולם</p>
<p>नमस्ते दुनिया · হ্যালো বিশ্ব · வணக்கம் உலகம் · హలో ప్రపంచం · ಹಲೋ ವಿಶ್ವ · ഹലോ വേൾഡ്</p>
<p>હેલો વિશ્વ · ਸਤ ਸ੍ਰੀ ਅਕਾਲ · හෙලෝ ලෝකය · ନମସ୍କାର ବିଶ୍ୱ</p>
<p>สวัสดีชาวโลก · ສະບາຍດີຊາວໂລກ · សួស្តីពិភពលោក · မင်္ဂလာပါ</p>
<p>ሰላም ዓለም · გამარჯობა მსოფლიო · Բարեւ աշխարհ · ހެލޯ ދުނިޔެ · ܫܠܡܐ ܥܠܡܐ</p>
<p>བཀྲ་ཤིས་བདེ་ལེགས། · ᎣᏏᏲ ᎡᎶᎯ · ᐊᐃ ᓯᓚᕐᔪᐊᖅ · ߊ߬ ߣߌ߫ ߕߎ߬ · ⴰⵣⵓⵍ ⴰⴼⵍⵍⴰ</p>
<p>✗ ✘ ★ ☆ ✓ ✔ → ∑ ∞ ≠ 🚀 ✅ 😀 🇵🇭</p></body></html>"#;

#[tokio::test(flavor = "multi_thread")]
async fn every_script_is_drawn_with_a_font_that_has_it() {
    if std::env::var_os("FONTCONFIG_FILE").is_none() {
        eprintln!("skipped: FONTCONFIG_FILE is not set (curated font set not in use)");
        return;
    }
    let app = TestApp::builder().spawn().await;
    let resp = app.to_bytes(form(LANGUAGES)).await;
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
