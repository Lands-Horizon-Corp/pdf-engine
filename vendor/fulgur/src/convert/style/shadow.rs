//! box-shadow extraction.
//!
//! Iterates the computed `box-shadow` list and pushes every shadow (inset
//! and outer alike) onto `BlockStyle::box_shadows`. Outer shadows paint
//! outside the box (`background::draw_single_box_shadow`); inset shadows
//! paint inside it (`background::draw_single_inset_box_shadow`). Non-zero
//! blur is rendered via a gradient 9-slice mask in `background.rs`.

use super::{StyleContext, absolute_to_rgba};
use crate::draw_primitives::BlockStyle;
use crate::units::F32Units;

pub(super) fn apply_to(style: &mut BlockStyle, ctx: &StyleContext<'_>) {
    let shadow_list = ctx.styles.clone_box_shadow();
    for shadow in shadow_list.0.iter() {
        let blur_px = shadow.base.blur.px();
        let rgba = absolute_to_rgba(shadow.base.color.resolve_to_absolute(ctx.current_color));
        if rgba[3] == 0 {
            continue; // fully transparent — skip
        }
        style.box_shadows.push(crate::draw_primitives::BoxShadow {
            offset_x: shadow.base.horizontal.px().as_px().in_pt(),
            offset_y: shadow.base.vertical.px().as_px().in_pt(),
            blur: blur_px.as_px().in_pt(),
            spread: shadow.spread.px().as_px().in_pt(),
            color: rgba,
            inset: shadow.inset,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;

    fn render_with_shadow(shadow_css: &str) -> Vec<u8> {
        let html = format!(
            r#"<html><body><div style="width:100px;height:100px;box-shadow:{shadow_css}"></div></body></html>"#
        );
        Engine::builder()
            .build()
            .render(&html)
            .expect("render should succeed")
    }

    /// Non-inset shadow with partial alpha exercises the main push path.
    /// The shadow is actually drawn, so the PDF must differ from the no-shadow baseline.
    #[test]
    fn non_inset_shadow_is_pushed() {
        let baseline = render_with_shadow("none");
        let pdf = render_with_shadow("5px 5px 10px rgba(0,0,0,0.5)");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
        assert_ne!(
            pdf.len(),
            baseline.len(),
            "non-inset shadow should add content to PDF"
        );
    }

    /// `inset` keyword is pushed through to `BlockStyle::box_shadows` and
    /// painted by `background::draw_single_inset_box_shadow` (solid, no blur).
    /// It must add content to the PDF, same as an outer shadow.
    #[test]
    fn inset_shadow_is_drawn() {
        let baseline = render_with_shadow("none");
        let pdf = render_with_shadow("inset 5px 5px 10px red");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
        assert_ne!(
            pdf.len(),
            baseline.len(),
            "inset shadow should add content to PDF"
        );
    }

    /// `inset` with `blur > 0` takes the luminosity-mask path in
    /// `draw_inset_blur_box_shadow`, which differs from the unblurred path.
    #[test]
    fn inset_shadow_with_blur_is_drawn() {
        let baseline = render_with_shadow("none");
        let solid = render_with_shadow("inset 0px 0px 0px 2px red");
        let blurred = render_with_shadow("inset 0px 0px 10px 2px red");
        assert!(blurred.starts_with(b"%PDF"), "expected PDF header");
        assert_ne!(
            blurred.len(),
            baseline.len(),
            "blurred inset shadow should add content to PDF"
        );
        assert_ne!(
            blurred.len(),
            solid.len(),
            "blurred inset shadow should render differently from the unblurred one"
        );
    }

    /// A rounded box with an inset shadow must still render without panicking,
    /// exercising the rounded-rect paths in both the solid and blurred cases.
    #[test]
    fn inset_shadow_with_border_radius_is_drawn() {
        let html = r#"<html><body><div style="width:100px;height:100px;border-radius:12px;box-shadow:inset 0px 0px 6px 1px rgba(0,0,0,0.5)"></div></body></html>"#;
        let pdf = Engine::builder()
            .build()
            .render(html)
            .expect("render should succeed");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
    }

    /// Spread large enough to consume the whole box must not panic, and
    /// should still paint the box fully as shadow (no inner hole left).
    #[test]
    fn inset_shadow_with_spread_exceeding_box_is_drawn() {
        let baseline = render_with_shadow("none");
        let pdf = render_with_shadow("inset 0px 0px 0px 80px black");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
        assert_ne!(
            pdf.len(),
            baseline.len(),
            "inset shadow should still paint when spread exceeds the box"
        );
    }

    /// Fully transparent color exercises the `rgba[3] == 0` early-continue branch.
    /// Skipped shadows leave no trace — output must match the no-shadow baseline.
    #[test]
    fn transparent_shadow_is_skipped() {
        let baseline = render_with_shadow("none");
        let pdf = render_with_shadow("5px 5px 10px transparent");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
        assert_eq!(
            pdf.len(),
            baseline.len(),
            "transparent shadow should not affect output"
        );
    }

    /// Multiple shadows: inset + transparent + outer in one list. The fully
    /// transparent one is still skipped; the inset and outer ones are both
    /// drawn, so the output must differ from either shadow alone.
    #[test]
    fn mixed_shadow_list_skips_only_transparent() {
        let outer_only = render_with_shadow("3px 3px 0px black");
        let pdf = render_with_shadow("inset 2px 2px red, 5px 5px transparent, 3px 3px 0px black");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
        assert_ne!(
            pdf.len(),
            outer_only.len(),
            "the inset shadow should add content beyond the outer shadow alone"
        );
    }

    /// Non-zero spread radius is stored via `shadow.spread.px().as_px().in_pt()`.
    /// The shadow is actually drawn, so the PDF must differ from the no-shadow baseline.
    #[test]
    fn shadow_with_spread_radius() {
        let baseline = render_with_shadow("none");
        let pdf = render_with_shadow("2px 2px 5px 8px rgba(255,0,0,0.8)");
        assert!(pdf.starts_with(b"%PDF"), "expected PDF header");
        assert_ne!(
            pdf.len(),
            baseline.len(),
            "shadow with spread should add content to PDF"
        );
    }
}
