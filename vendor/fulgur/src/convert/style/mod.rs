use crate::asset::AssetBundle;
use crate::blitz_adapter::Node;
use crate::draw_primitives::BlockStyle;
use crate::units::F32Units;

mod background;
mod border;
mod box_metrics;
mod opacity;
mod overflow;
mod shadow;

pub(super) use opacity::extract_opacity_visible;

/// Bundle of references threaded through the per-property style extractors.
///
/// `extract_block_style` constructs this once per call and forwards it to
/// helper modules (`overflow`, `border`, `shadow`, `background`). Keeping
/// the four references on a single struct avoids long parameter lists.
pub(super) struct StyleContext<'a> {
    pub(super) styles: &'a style::properties::ComputedValues,
    pub(super) current_color: &'a style::color::AbsoluteColor,
    pub(super) layout: &'a taffy::Layout,
    pub(super) assets: Option<&'a AssetBundle>,
}

/// Extract visual style (background, borders, padding, background-image) from a node.
use ::style::values::specified::box_::DisplayInside;

pub(super) fn extract_block_style(node: &Node, assets: Option<&AssetBundle>) -> BlockStyle {
    let layout = node.final_layout;
    let mut style = BlockStyle::default();
    box_metrics::apply_to(&mut style, &layout);

    // Extract colors from computed styles
    if let Some(styles) = node.primary_styles() {
        let current_color = styles.clone_color();
        let ctx = StyleContext {
            styles: &styles,
            current_color: &current_color,
            layout: &layout,
            assets,
        };

        // Borders (color, radii, styles)
        border::apply_to(&mut style, &ctx);

        // Box shadows
        shadow::apply_to(&mut style, &ctx);

        // Overflow (CSS3 axis-independent interpretation)
        // PDF has no scroll concept: hidden/clip/scroll/auto all collapse to Clip.
        overflow::apply_to(&mut style, &ctx);

        // Background color + background-image layers — kept last to preserve
        // the original temporal order in which `style.background_layers` is
        // populated. VRT golden bytes depend on this ordering.
        background::apply_to(&mut style, &ctx);
    }

    // pdf-engine patch: a cell of a `border-collapse: collapse` table paints the border its edges resolved to (the layout
    // already holds half of each width): the winner's style and colour, not its own, and on the table's outer edge the
    // border reaches half its width outside the cell.
    if let Some(cb) = node.element_data().and_then(|e| e.collapsed_border.as_deref()) {
        style.border_styles = cb.styles.map(border::map_border_style);
        let colors = cb.colors.map(absolute_to_rgba);
        style.border_color = colors[0];
        style.border_side_colors = (!colors.iter().all(|c| *c == colors[0])).then_some(colors);
        style.collapsed_outer = cb.outer;
        if cb.outer.iter().any(|&o| o) {
            // The whole border of an outer side is drawn from the table's own edge inward, as a browser does. The table's edge is
            // as far outside the cell as the cell sits inside the table (the layout rounds both to whole pixels).
            let mut ext = [0.0f32; 4];
            let mut up = node.parent.map(|p| node.with(p));
            for _ in 0..4 {
                let Some(a) = up else { break };
                if a.primary_styles().is_some_and(|st| matches!(st.clone_display().inside(), DisplayInside::Table)) {
                    let (loc, size, table) = (node.final_layout.location, node.final_layout.size, a.final_layout.size);
                    ext = [
                        loc.y,
                        table.width - (loc.x + size.width),
                        table.height - (loc.y + size.height),
                        loc.x,
                    ]
                    .map(|v| v.max(0.0));
                    break;
                }
                up = a.parent.map(|p| a.with(p));
            }
            for side in 0..4 {
                if cb.outer[side] {
                    style.border_widths[side] = cb.widths[side].as_px().in_pt();
                    style.collapsed_outer_extent[side] = ext[side].as_px().in_pt();
                }
            }
        }
    }

    // pdf-engine patch: a table row has no box of its own in the PDF (its cells are the boxes), so its `background` was never
    // painted: striped tables (`tr:nth-child(even)`) and highlighted rows (`<tr style="background:...">`) came out plain.
    // A cell with no background of its own takes its row's (or row group's) colour.
    if style.background_color.is_none()
        && style.background_layers.is_empty()
        && node.primary_styles().is_some_and(|st| {
            matches!(st.clone_display().inside(), DisplayInside::TableCell)
        })
    {
        let mut ancestor = node.parent.map(|p| node.with(p));
        for _ in 0..2 {
            let Some(a) = ancestor else { break };
            let is_row_like = a.primary_styles().is_some_and(|st| {
                matches!(
                    st.clone_display().inside(),
                    DisplayInside::TableRow
                        | DisplayInside::TableRowGroup
                        | DisplayInside::TableHeaderGroup
                        | DisplayInside::TableFooterGroup
                )
            });
            if !is_row_like {
                break;
            }
            let color = extract_block_style(a, assets).background_color;
            if color.is_some() {
                style.background_color = color;
                break;
            }
            ancestor = a.parent.map(|p| a.with(p));
        }
    }

    style
}

pub(super) fn absolute_to_rgba(c: style::color::AbsoluteColor) -> [u8; 4] {
    // `.round()` (not `as u8` truncation) so e.g. `rgb(127.5,…)` lands on 128
    // instead of 127. Truncation introduces a half-channel down-bias for
    // every fractional component, which is most visible in gradient stops.
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    // pdf-engine patch (see PATCHES.md): `hsl()`, `hwb()`, `lab()` etc. keep their own
    // color space; convert to sRGB before reading the components as r/g/b.
    let c = c.into_srgb_legacy();
    [
        q(c.components.0),
        q(c.components.1),
        q(c.components.2),
        q(c.alpha),
    ]
}
