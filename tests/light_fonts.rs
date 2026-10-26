//! Which documents are laid out with the built-in fonts alone (no system fonts enumerated), and which need the system's.

use pdf_engine::fulgur_engine::use_light_fonts;

fn needs(html: &str) -> bool {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(use_light_fonts);
    fulgur::document_needs_system_fonts(html)
}

#[test]
fn plain_documents_stay_light() {
    assert!(!needs(
        "<h1>Invoice</h1><p>Total 1,234.50 caf\u{e9} &amp; &euro;5 &nbsp; &#8364; \u{20b1}100 \u{410}\u{43e}</p>"
    ));
    assert!(!needs(
        "<style>body{font-family:'Noto Sans',sans-serif;font-size:12px}h1{font-weight:700}</style><p>x</p>"
    ));
    assert!(!needs("<p style=\"font-family: sans-serif\">x</p>"));
    assert!(
        !needs(
            "<style>body{font-family:'Noto Sans', Arial, Helvetica, sans-serif}</style><p>x</p>"
        ),
        "the first family wins"
    );
    assert!(!needs(
        "<p style=\"font-family:'Noto Sans',Arial,sans-serif;color:red\">x</p>"
    ));
    assert!(!needs(
        "<style>.md\\:flex{display:flex}</style><div class=\"md:flex\">x</div>"
    ));
    assert!(!needs(
        "<ul><li>a</li></ul><table><tr><td>b</td></tr></table><img src=x><br>"
    ));
    assert!(
        !needs("<p>\u{2211} \u{221a} \u{2264} \u{2192}</p>"),
        "math symbols are in the built-in math font"
    );
}

#[test]
fn characters_the_built_in_fonts_lack_need_the_system() {
    assert!(needs("<p>\u{4f60}\u{597d}</p>"));
    assert!(needs("<p>\u{1F600}</p>"));
    assert!(needs("<p>&#x4F60;</p>"));
    assert!(needs("<p>&#128512;</p>"));
    assert!(needs("<p>&hearts;</p>"));
    assert!(needs("<style>p::before{content:'\\2022'}</style><p>x</p>"));
    assert!(needs("<p>\u{645}\u{631}\u{62d}\u{628}\u{627}</p>"));
}

#[test]
fn other_families_and_elements_need_the_system() {
    assert!(needs("<p style=\"font-family:Arial\">x</p>"));
    assert!(
        needs("<p style=\"font-family:Arial, 'Noto Sans'\">x</p>"),
        "Arial comes first, and may be installed"
    );
    assert!(needs(
        "<style>p{font-family:'Inter', sans-serif}</style><p>x</p>"
    ));
    assert!(needs("<p style=\"font-family:serif\">x</p>"));
    assert!(needs("<p style=\"font-family:var(--f)\">x</p>"));
    assert!(needs("<p style=\"font: 12px Georgia\">x</p>"));
    for tag in [
        "code", "pre", "kbd", "samp", "tt", "textarea", "input", "button", "select", "font",
    ] {
        assert!(needs(&format!("<{tag}>x</{tag}>")), "{tag}");
    }
}
