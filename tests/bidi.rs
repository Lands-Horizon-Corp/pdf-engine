//! Right-to-left text and bidirectional layout.
//!
//! The order of things on the page is checked by painting each piece in its own flat colour
//! (red, blue, green), rasterising the PDF and comparing where the colours landed. That does not
//! depend on which fonts the machine has: Latin letters inside `dir="rtl"` reorder just like
//! Arabic ones, and the Hebrew and Arabic cases below only need *a* font with the script.
//!
//! **Known gaps.** The paragraph's base direction now follows `dir` and `direction` (a patched parley and blitz-dom,
//! see `vendor/parley/PATCHES.md`), and a flex row flips in an rtl box. The cases that still fail are `#[ignore]`d
//! with their reason, not deleted: `<bdo>` and isolates, grid, table and list mirroring, `inline-start` floats,
//! `padding-inline-*` and `text-indent`. List them with `cargo test --test bidi -- --ignored`.
//!
//! Each case states the rule it checks, from the Unicode Bidirectional Algorithm (UAX #9) or CSS Writing Modes.

mod common;

use common::*;

/// A page with no margin, and three flat colours.
fn page(body: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
@page{{margin:0}}
html,body{{margin:0;padding:0;background:#fff}}
body{{font-family:'Noto Sans',sans-serif;font-size:24px;line-height:1.4}}
.r{{color:#e00}} .b{{color:#00e}} .g{{color:#0a0}}
.box{{padding:10px 20px}}
</style></head><body>{body}</body></html>"#
    )
}

async fn raster(body: &str) -> Raster {
    let pdf = render_pdf(&page(body)).await;
    assert_eq!(page_count(&pdf), 1, "the test page must fit on one page");
    rasterize(&pdf, 0, 1.0)
}

fn x(r: &Raster, ink: Ink) -> f32 {
    r.centroid_x(ink)
        .unwrap_or_else(|| panic!("nothing was drawn in {ink:?}"))
}

/// Assert `a` is drawn to the left of `b`, by at least a few pixels.
fn left_of(r: &Raster, a: Ink, b: Ink, what: &str) {
    let (xa, xb) = (x(r, a), x(r, b));
    assert!(
        xa + 0.005 < xb,
        "{what}: {a:?} ({xa:.3}) should be left of {b:?} ({xb:.3})"
    );
}

// ---- paragraph direction -----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn ltr_paragraph_starts_at_the_left_and_rtl_at_the_right() {
    let ltr = raster(r#"<div class="box" dir="ltr"><span class="r">Start</span></div>"#).await;
    let rtl = raster(r#"<div class="box" dir="rtl"><span class="r">Start</span></div>"#).await;
    let (l0, _) = ltr.x_extent(Ink::Red).unwrap();
    let (_, r1) = rtl.x_extent(Ink::Red).unwrap();
    assert!(
        l0 < 0.1,
        "ltr text should start at the left edge, started at {l0:.3}"
    );
    assert!(
        r1 > 0.9,
        "rtl text should end at the right edge, ended at {r1:.3}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn direction_css_property_matches_the_dir_attribute() {
    let attr = raster(r#"<div class="box" dir="rtl"><span class="r">Start</span></div>"#).await;
    let css =
        raster(r#"<div class="box" style="direction:rtl"><span class="r">Start</span></div>"#)
            .await;
    let (a, c) = (x(&attr, Ink::Red), x(&css, Ink::Red));
    assert!(
        (a - c).abs() < 0.01,
        "dir=rtl ({a:.3}) and direction:rtl ({c:.3}) must agree"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn direction_is_inherited_by_children() {
    let r =
        raster(r#"<div class="box" dir="rtl"><div><p><span class="r">Deep</span></p></div></div>"#)
            .await;
    assert!(x(&r, Ink::Red) > 0.6, "a nested paragraph must inherit rtl");
}

#[tokio::test(flavor = "multi_thread")]
async fn text_align_start_and_end_follow_the_direction() {
    let start = raster(
        r#"<div class="box" dir="rtl" style="text-align:start"><span class="r">x</span></div>"#,
    )
    .await;
    let end = raster(
        r#"<div class="box" dir="rtl" style="text-align:end"><span class="r">x</span></div>"#,
    )
    .await;
    assert!(x(&start, Ink::Red) > 0.8, "start in rtl is the right edge");
    assert!(x(&end, Ink::Red) < 0.2, "end in rtl is the left edge");
}

#[tokio::test(flavor = "multi_thread")]
async fn text_align_left_and_right_stay_physical_in_rtl() {
    let left = raster(
        r#"<div class="box" dir="rtl" style="text-align:left"><span class="r">x</span></div>"#,
    )
    .await;
    let right = raster(
        r#"<div class="box" dir="rtl" style="text-align:right"><span class="r">x</span></div>"#,
    )
    .await;
    assert!(x(&left, Ink::Red) < 0.2, "left is always the left edge");
    assert!(x(&right, Ink::Red) > 0.8, "right is always the right edge");
}

#[tokio::test(flavor = "multi_thread")]
async fn justified_rtl_paragraph_fills_the_line_from_both_sides() {
    let r = raster(
        r#"<div class="box" dir="rtl" style="text-align:justify"><span class="r">one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen</span></div>"#,
    )
    .await;
    let (lo, hi) = r.x_extent(Ink::Red).unwrap();
    assert!(
        hi > 0.9,
        "the first line must reach the right edge, reached {hi:.3}"
    );
    assert!(
        lo < 0.1,
        "a justified line must reach the left edge, reached {lo:.3}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dir_auto_takes_the_direction_of_the_first_strong_letter() {
    // Hebrew first: right to left. Latin first: left to right.
    let he = raster(r#"<div class="box" dir="auto"><span class="r">שלום</span> hello</div>"#).await;
    let en = raster(r#"<div class="box" dir="auto"><span class="r">hello</span> שלום</div>"#).await;
    let (_, he_hi) = he.x_extent(Ink::Red).unwrap();
    let (en_lo, _) = en.x_extent(Ink::Red).unwrap();
    assert!(
        he_hi > 0.9,
        "dir=auto with Hebrew first must end at the right edge ({he_hi:.3})"
    );
    assert!(
        en_lo < 0.1,
        "dir=auto with Latin first must start at the left edge ({en_lo:.3})"
    );
}

// ---- the order of runs --------------------------------------------------------------------------

const THREE: &str =
    r#"<span class="r">AAA</span> <span class="b">BBB</span> <span class="g">CCC</span>"#;
/// Latin words with a Hebrew word between each: three left-to-right runs, so in an rtl paragraph they go right to left.
const THREE_RUNS: &str =
    r#"<span class="r">AAA</span> אב <span class="b">BBB</span> אב <span class="g">CCC</span>"#;

#[tokio::test(flavor = "multi_thread")]
async fn runs_are_in_reading_order_left_to_right() {
    let r = raster(&format!(r#"<div class="box" dir="ltr">{THREE}</div>"#)).await;
    left_of(&r, Ink::Red, Ink::Blue, "ltr");
    left_of(&r, Ink::Blue, Ink::Green, "ltr");
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_are_in_reading_order_right_to_left() {
    // Each Latin word is still read left to right, but the words go from the right edge to the left.
    // Words of one language in a row are a single run and keep their order (UAX #9), so check both cases.
    let one_run = raster(&format!(r#"<div class="box" dir="rtl">{THREE}</div>"#)).await;
    left_of(
        &one_run,
        Ink::Red,
        Ink::Blue,
        "one left-to-right run inside an rtl paragraph",
    );
    let runs = raster(&format!(r#"<div class="box" dir="rtl">{THREE_RUNS}</div>"#)).await;
    left_of(&runs, Ink::Blue, Ink::Red, "rtl");
    left_of(&runs, Ink::Green, Ink::Blue, "rtl");
}

#[tokio::test(flavor = "multi_thread")]
async fn hebrew_words_run_right_to_left_even_in_an_ltr_paragraph() {
    // Hebrew is a strong right-to-left script: no dir attribute is needed.
    let r = raster(
        r#"<div class="box"><span class="r">אחד</span> <span class="b">שנים</span> <span class="g">שלוש</span></div>"#,
    )
    .await;
    left_of(&r, Ink::Blue, Ink::Red, "hebrew in ltr");
    left_of(&r, Ink::Green, Ink::Blue, "hebrew in ltr");
}

#[tokio::test(flavor = "multi_thread")]
async fn arabic_words_run_right_to_left_even_in_an_ltr_paragraph() {
    let r = raster(
        r#"<div class="box"><span class="r">واحد</span> <span class="b">اثنان</span> <span class="g">ثلاثة</span></div>"#,
    )
    .await;
    left_of(&r, Ink::Blue, Ink::Red, "arabic in ltr");
    left_of(&r, Ink::Green, Ink::Blue, "arabic in ltr");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_latin_word_inside_arabic_keeps_its_letters_in_order() {
    // Arabic, then a Latin word (its letters coloured one by one), then Arabic, in an rtl paragraph.
    let r = raster(
        r#"<div class="box" dir="rtl"><span class="g">مرحبا</span> <b class="r">A</b><b class="b">B</b><b class="g">C</b> <span class="g">عالم</span></div>"#,
    )
    .await;
    // The Latin word reads A B C from the left, so A must be left of B (not mirrored).
    left_of(
        &r,
        Ink::Red,
        Ink::Blue,
        "letters of a Latin word inside rtl",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn digits_inside_rtl_text_stay_left_to_right() {
    // UAX #9: European numbers are weak left-to-right, so 1 2 3 stays 1 2 3.
    let r = raster(
        r#"<div class="box" dir="rtl">שלום <b class="r">1</b><b class="b">2</b><b class="g">3</b> שלום</div>"#,
    )
    .await;
    left_of(&r, Ink::Red, Ink::Blue, "digits in rtl");
    left_of(&r, Ink::Blue, Ink::Green, "digits in rtl");
}

#[tokio::test(flavor = "multi_thread")]
async fn arabic_indic_digits_also_stay_in_reading_order() {
    let r = raster(
        r#"<div class="box" dir="rtl">رقم <b class="r">١</b><b class="b">٢</b><b class="g">٣</b></div>"#,
    )
    .await;
    left_of(&r, Ink::Red, Ink::Blue, "arabic-indic digits");
    left_of(&r, Ink::Blue, Ink::Green, "arabic-indic digits");
}

// ---- punctuation --------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn final_punctuation_lands_on_the_left_of_an_rtl_sentence() {
    // "text." in rtl: the full stop ends the sentence, so it is the leftmost mark.
    let r = raster(r#"<div class="box" dir="rtl"><span class="r">שלום עולם</span><span class="b">.</span></div>"#).await;
    left_of(
        &r,
        Ink::Blue,
        Ink::Red,
        "full stop at the end of an rtl sentence",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn brackets_are_mirrored_in_rtl() {
    // In rtl text "(" at the start of a bracketed phrase is drawn as ")" (UAX #9, mirroring), so the
    // bracketed word keeps its brackets on the right sides: the opening one is on the right.
    let r = raster(
        r#"<div class="box" dir="rtl"><b class="b">(</b><span class="r">שלום</span><b class="g">)</b></div>"#,
    )
    .await;
    left_of(
        &r,
        Ink::Red,
        Ink::Blue,
        "the opening bracket is on the right of the word",
    );
    left_of(
        &r,
        Ink::Green,
        Ink::Red,
        "the closing bracket is on the left of the word",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trailing_neutral_after_latin_in_rtl_goes_with_the_paragraph() {
    // "hello!" alone in an rtl paragraph: the "!" is neutral between a Latin letter and the paragraph end,
    // so it takes the paragraph direction and sits on the left of the word.
    let r = raster(
        r#"<div class="box" dir="rtl"><span class="r">hello</span><span class="b">!</span></div>"#,
    )
    .await;
    // It is a bidi corner: "!" is after "hello" in reading order, so it must be on the same side as the end of the line.
    left_of(&r, Ink::Blue, Ink::Red, "neutral after a Latin word in rtl");
}

// ---- controls, isolates and overrides -----------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: <bdo> / unicode-bidi: bidi-override is not applied"]
async fn bdo_rtl_reverses_even_latin_letters() {
    let r = raster(
        r#"<div class="box"><bdo dir="rtl"><b class="r">A</b><b class="b">B</b><b class="g">C</b></bdo></div>"#,
    )
    .await;
    left_of(&r, Ink::Green, Ink::Blue, "bdo rtl");
    left_of(&r, Ink::Blue, Ink::Red, "bdo rtl");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: <bdo> / unicode-bidi: bidi-override is not applied"]
async fn bdo_ltr_stops_hebrew_from_reversing() {
    let r = raster(
        r#"<div class="box"><bdo dir="ltr"><b class="r">א</b><b class="b">ב</b><b class="g">ג</b></bdo></div>"#,
    )
    .await;
    left_of(&r, Ink::Red, Ink::Blue, "bdo ltr over hebrew");
    left_of(&r, Ink::Blue, Ink::Green, "bdo ltr over hebrew");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: <bdo> / unicode-bidi: bidi-override is not applied"]
async fn unicode_bidi_bidi_override_matches_bdo() {
    let r = raster(
        r#"<div class="box"><span style="direction:rtl;unicode-bidi:bidi-override"><b class="r">A</b><b class="b">B</b><b class="g">C</b></span></div>"#,
    )
    .await;
    left_of(&r, Ink::Green, Ink::Blue, "bidi-override");
    left_of(&r, Ink::Blue, Ink::Red, "bidi-override");
}

#[tokio::test(flavor = "multi_thread")]
async fn bdi_isolates_text_of_unknown_direction() {
    // The isolated name is Hebrew, but it must not drag the "after" text to its side: it stays after it.
    let r = raster(
        r#"<div class="box"><span class="r">User</span> <bdi class="b">אברהם</bdi> <span class="g">logged in</span></div>"#,
    )
    .await;
    left_of(&r, Ink::Red, Ink::Blue, "before an isolate");
    left_of(&r, Ink::Blue, Ink::Green, "after an isolate");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: unicode-bidi: isolate is not applied"]
async fn unicode_bidi_isolate_keeps_a_phrase_together() {
    let r = raster(
        r#"<div class="box" dir="rtl"><span class="r">AAA</span> <span style="unicode-bidi:isolate"><span class="b">BBB</span> <span class="g">CCC</span></span></div>"#,
    )
    .await;
    // The isolate counts as a neutral at the end of the line, so it goes to the left of AAA (the first thing in
    // the line); inside it BBB CCC is one left-to-right run.
    left_of(&r, Ink::Blue, Ink::Red, "isolate");
    left_of(&r, Ink::Blue, Ink::Green, "inside the isolate");
}

// ---- layout in a right-to-left context ----------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn flex_row_starts_at_the_right_in_rtl() {
    let r = raster(
        r#"<div class="box" dir="rtl" style="display:flex"><span class="r">One</span><span class="b" style="margin-inline-start:20px">Two</span></div>"#,
    )
    .await;
    left_of(&r, Ink::Blue, Ink::Red, "flex items in rtl");
}

#[tokio::test(flavor = "multi_thread")]
async fn flex_row_reverse_flips_in_rtl() {
    let r = raster(
        r#"<div class="box" dir="rtl" style="display:flex;flex-direction:row-reverse"><span class="r">One</span><span class="b" style="margin-inline-start:20px">Two</span></div>"#,
    )
    .await;
    left_of(&r, Ink::Red, Ink::Blue, "row-reverse in rtl");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: grid columns are not mirrored in an rtl grid (taffy has no direction)"]
async fn grid_columns_are_laid_out_from_the_right_in_rtl() {
    let r = raster(
        r#"<div class="box" dir="rtl" style="display:grid;grid-template-columns:1fr 1fr 1fr"><span class="r">One</span><span class="b">Two</span><span class="g">Three</span></div>"#,
    )
    .await;
    left_of(&r, Ink::Blue, Ink::Red, "grid in rtl");
    left_of(&r, Ink::Green, Ink::Blue, "grid in rtl");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: table columns are not mirrored in an rtl table"]
async fn table_columns_run_right_to_left_in_an_rtl_table() {
    let r = raster(
        r#"<table dir="rtl" style="width:90%;margin:10px 5%;border-collapse:collapse"><tr><td class="r">One</td><td class="b">Two</td><td class="g">Three</td></tr></table>"#,
    )
    .await;
    left_of(&r, Ink::Blue, Ink::Red, "table in rtl");
    left_of(&r, Ink::Green, Ink::Blue, "table in rtl");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: list markers are not moved to the right in an rtl list"]
async fn list_markers_are_on_the_right_in_rtl() {
    let r = raster(r#"<ul dir="rtl" style="margin:10px 20px;padding-inline-start:40px"><li><span class="r">item</span></li></ul>"#).await;
    // The marker is black, the text red: black ink must be right of the red text.
    let marker = r.centroid_x(Ink::Any).unwrap();
    let text = x(&r, Ink::Red);
    assert!(
        marker > text,
        "the bullet ({marker:.3}) should be right of the item text ({text:.3})"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: list markers are not moved to the right in an rtl list"]
async fn ordered_list_numbers_are_on_the_right_in_rtl() {
    let r = raster(r#"<ol dir="rtl" style="margin:10px 20px;padding-inline-start:40px"><li><span class="r">item</span></li></ol>"#).await;
    let all = r.x_extent(Ink::Any).unwrap();
    let text = r.x_extent(Ink::Red).unwrap();
    assert!(
        all.1 > text.1,
        "the number must be drawn to the right of the item text"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: padding-inline-start / margin-inline-start are not supported (even in ltr)"]
async fn logical_margins_and_padding_flip_in_rtl() {
    let ltr = raster(r#"<div class="box" dir="ltr" style="padding-inline-start:100px"><span class="r">x</span></div>"#).await;
    let rtl = raster(r#"<div class="box" dir="rtl" style="padding-inline-start:100px"><span class="r">x</span></div>"#).await;
    // In ltr the text is pushed right of the left edge; in rtl it is pushed left of the right edge.
    let (l, _) = ltr.x_extent(Ink::Red).unwrap();
    let (_, r) = rtl.x_extent(Ink::Red).unwrap();
    assert!(
        l > 0.2,
        "ltr: padding-inline-start pushes the text from the left ({l:.3})"
    );
    assert!(
        r < 0.8,
        "rtl: padding-inline-start pushes the text from the right ({r:.3})"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn border_inline_start_is_on_the_right_in_rtl() {
    let r = raster(r#"<div class="box" dir="rtl" style="border-inline-start:6px solid #e00"><span class="b">x</span></div>"#).await;
    assert!(
        x(&r, Ink::Red) > 0.8,
        "the start border of an rtl box is on its right"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: text-indent is applied at the left in an rtl paragraph"]
async fn text_indent_applies_at_the_start_side() {
    let r = raster(r#"<div class="box" dir="rtl" style="text-indent:80px"><span class="r">indented first line</span></div>"#).await;
    let (_, hi) = r.x_extent(Ink::Red).unwrap();
    assert!(
        hi < 0.9,
        "the first line is indented from the right edge ({hi:.3})"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "known gap: float: inline-start / inline-end are not supported"]
async fn floats_start_and_end_follow_the_direction() {
    let r = raster(
        r#"<div class="box" dir="rtl"><div style="float:inline-start;width:60px;height:30px;background:#e00"></div><span class="b">text</span></div>"#,
    )
    .await;
    assert!(
        x(&r, Ink::Red) > 0.7,
        "float: inline-start in rtl goes to the right"
    );
}

// ---- wrapping and pages --------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn every_wrapped_line_of_an_rtl_paragraph_ends_at_the_right_edge() {
    let words = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon ";
    let r = raster(&format!(
        r#"<div class="box" dir="rtl"><span class="r">{}</span></div>"#,
        words.repeat(2)
    ))
    .await;
    // Check several horizontal bands: each must have its ink ending near the right edge.
    let mut bands_with_ink = 0;
    for band in 0..5 {
        let y0 = 10 + band * 33;
        let mut hi = 0usize;
        let mut any = false;
        for y in y0..y0 + 30 {
            for xx in 0..r.width {
                let p = &r.rgba[(y * r.width + xx) * 4..][..4];
                if p[0] as i32 - p[1] as i32 > 90 {
                    any = true;
                    hi = hi.max(xx);
                }
            }
        }
        if any {
            bands_with_ink += 1;
            assert!(
                hi as f32 / r.width as f32 > 0.8,
                "line {band} does not reach the right edge ({hi})"
            );
        }
    }
    assert!(
        bands_with_ink >= 3,
        "the paragraph should wrap onto several lines"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_rtl_document_paginates_without_losing_text() {
    let para = r#"<p dir="rtl">שלום עולם זהו משפט ארוך שנועד לבדוק שבירת שורות ועימוד של טקסט מימין לשמאל במסמך ארוך מאוד.</p>"#;
    let html = page(&para.repeat(120));
    let pdf = render_pdf(&html).await;
    assert!(
        page_count(&pdf) >= 3,
        "120 paragraphs should need several pages"
    );
    let first = rasterize(&pdf, 0, 1.0);
    let last = rasterize(&pdf, page_count(&pdf) - 1, 1.0);
    assert!(first.count(Ink::Any) > 500, "the first page must have text");
    assert!(last.count(Ink::Any) > 100, "the last page must have text");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtl_page_numbers_and_margin_boxes_render() {
    let html = r#"<!DOCTYPE html><html dir="rtl"><head><meta charset="utf-8"><style>
@page{size:480px 320px;margin:40px;@bottom-center{content:counter(page) " / " counter(pages);color:#00e}}
body{font-size:20px}</style></head><body><p style="color:#e00">שלום עולם</p></body></html>"#;
    let pdf = render_pdf(html).await;
    let r = rasterize(&pdf, 0, 1.0);
    assert!(r.count(Ink::Blue) > 20, "the page number must be drawn");
    assert!(r.count(Ink::Red) > 20, "the body text must be drawn");
}

// ---- fonts and shaping --------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn arabic_letters_are_joined_into_one_connected_word() {
    // Shaped Arabic is a connected stroke; unshaped (isolated forms) leaves gaps. Draw the same word
    // shaped and as separate isolated letters and check the shaped one has far fewer ink columns
    // that are blank between the letters. A cheap, font-independent proxy: connected text is *narrower*
    // than the same letters set one by one (isolated forms are wider and spaced).
    let joined = raster(r#"<div class="box" dir="rtl"><span class="r">العربية</span></div>"#).await;
    let apart = raster(
        r#"<div class="box" dir="rtl"><span class="r">ا&#x200C;ل&#x200C;ع&#x200C;ر&#x200C;ب&#x200C;ي&#x200C;ة</span></div>"#,
    )
    .await;
    let (jl, jr) = joined.x_extent(Ink::Red).unwrap();
    let (al, ar) = apart.x_extent(Ink::Red).unwrap();
    assert!(
        (jr - jl) < (ar - al),
        "joined letters ({:.3}) should take less width than the same letters kept apart ({:.3})",
        jr - jl,
        ar - al
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn arabic_text_is_not_drawn_as_empty_boxes() {
    let pdf = render_pdf(&page(
        r#"<div class="box" dir="rtl"><span class="r">مرحبا بالعالم</span></div>"#,
    ))
    .await;
    let fonts = embedded_fonts(&pdf);
    assert!(
        !fonts.is_empty(),
        "a font must be embedded for the Arabic text"
    );
    let r = rasterize(&pdf, 0, 1.0);
    assert!(
        r.count(Ink::Red) > 200,
        "Arabic text must leave ink on the page, got {}",
        r.count(Ink::Red)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rtl_rendering_is_deterministic() {
    let html = page(
        r#"<div class="box" dir="rtl"><span class="r">שלום עולם</span> hello 123 <span class="g">مرحبا</span></div>"#,
    );
    let a = rasterize(&render_pdf(&html).await, 0, 1.0);
    let b = rasterize(&render_pdf(&html).await, 0, 1.0);
    assert_eq!(
        a.rgba, b.rgba,
        "two renders of the same rtl page must look identical"
    );
}
