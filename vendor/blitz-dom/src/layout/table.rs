use std::{ops::Range, sync::Arc};

use markup5ever::local_name;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{Atom, computed_values::table_layout::T as TableLayout};
use taffy::{
    Dimension, LayoutPartialTree as _, ResolveOrZero, TrackSizingFunction, compute_leaf_layout,
    style_helpers,
};

use crate::BaseDocument;

use super::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC};
use super::resolve_calc_value;

pub struct TableTreeWrapper<'doc> {
    pub(crate) doc: &'doc mut BaseDocument,
    pub(crate) ctx: Arc<TableContext>,
}

#[derive(Debug, Clone)]
pub struct TableContext {
    style: taffy::Style<Atom>,
    items: Vec<TableItem>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TableItemKind {
    Row,
    Cell,
}

#[derive(Debug, Clone)]
pub struct TableItem {
    kind: TableItemKind,
    node_id: usize,
    /// Where the item sits in the table's grid. pdf-engine patch: the rest of its style is its node's own
    /// (`TableItemStyle` reads it from there), not a copy of about 470 bytes for every row and cell.
    grid_row: taffy::Line<taffy::GridPlacement<Atom>>,
    grid_column: taffy::Line<taffy::GridPlacement<Atom>>,
    /// A cell's `width` is taken by the column, so the cell itself is `auto`.
    width_auto: bool,
    /// The 1-based grid row a cell starts in, and how many rows it spans (`rowspan`).
    row_start: u16,
    row_span: u16,
    /// pdf-engine patch: half of `border-spacing` (horizontal, vertical), added to the cell's margin.
    half_spacing: (f32, f32),
    /// pdf-engine patch: the 0-based column a cell starts in, and how many columns it spans (for sizing the columns).
    col_start: u16,
    colspan: u16,
}

/// One side of a border as the collapsed-borders model sees it.
#[derive(Clone, Copy)]
struct BorderEdge {
    width: f32,
    style: ::style::values::specified::BorderStyle,
    color: ::style::color::AbsoluteColor,
}

impl BorderEdge {
    fn none() -> Self {
        BorderEdge {
            width: 0.0,
            style: ::style::values::specified::BorderStyle::None,
            color: ::style::color::AbsoluteColor::TRANSPARENT_BLACK,
        }
    }
    fn hidden(&self) -> bool {
        self.style == ::style::values::specified::BorderStyle::Hidden
    }
    /// How strong the style is when two borders are equally wide (CSS 2.1 17.6.2.1: double, solid, dashed, dotted, ridge,
    /// outset, groove, inset).
    fn style_rank(&self) -> u8 {
        use ::style::values::specified::BorderStyle as B;
        match self.style {
            B::Double => 8,
            B::Solid => 7,
            B::Dashed => 6,
            B::Dotted => 5,
            B::Ridge => 4,
            B::Outset => 3,
            B::Groove => 2,
            B::Inset => 1,
            B::None | B::Hidden => 0,
        }
    }
}

/// Of the two borders on one edge, the one that is drawn. `a` is the one that comes first (above or to the left, or a cell
/// against the table), which wins a tie.
fn collapse_winner(a: BorderEdge, b: BorderEdge) -> BorderEdge {
    if a.hidden() {
        return a;
    }
    if b.hidden() {
        return b;
    }
    if (a.width - b.width).abs() > 0.001 {
        return if b.width > a.width { b } else { a };
    }
    if b.style_rank() > a.style_rank() { b } else { a }
}

fn computed_border_edges(st: &::style::properties::ComputedValues) -> [BorderEdge; 4] {
    let current = st.clone_color();
    let b = st.get_border();
    let color = |c: ::style::values::computed::Color| c.resolve_to_absolute(&current);
    [
        BorderEdge { width: b.border_top_width.to_f32_px(), style: st.clone_border_top_style(), color: color(st.clone_border_top_color()) },
        BorderEdge { width: b.border_right_width.to_f32_px(), style: st.clone_border_right_style(), color: color(st.clone_border_right_color()) },
        BorderEdge { width: b.border_bottom_width.to_f32_px(), style: st.clone_border_bottom_style(), color: color(st.clone_border_bottom_color()) },
        BorderEdge { width: b.border_left_width.to_f32_px(), style: st.clone_border_left_style(), color: color(st.clone_border_left_color()) },
    ]
}

pub(crate) fn build_table_context(
    doc: &mut BaseDocument,
    table_root_node_id: usize,
) -> (TableContext, Vec<usize>) {
    let mut items: Vec<TableItem> = Vec::new();
    let mut row = 0u16;
    let mut col = 0u16;
    // For each column, the last row (1-based) occupied by a cell from an earlier row that spans down into it.
    let mut occupied: Vec<u16> = Vec::new();
    let mut max_col = 0u16;

    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let mut style = stylo_taffy::to_taffy_style(&stylo_styles);
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    let mut half_spacing = (0.0f32, 0.0f32);
    // pdf-engine patch: `border-spacing` in the separate-borders model (the default; the user-agent sheet sets 2px) is the
    // gap between cells and also the space between the cells and the table's edge. It was parsed and never used.
    if stylo_styles.clone_border_collapse()
        == ::style::computed_values::border_collapse::T::Separate
    {
        let spacing = stylo_styles.clone_border_spacing();
        let (sw, sh) = (spacing.0.width.0.px(), spacing.0.height.0.px());
        half_spacing = (sw / 2.0, sh / 2.0);
        let pad = style
            .padding
            .resolve_or_zero(taffy::Size::NONE, |_, _| 0.0);
        style.padding = taffy::Rect {
            left: style_helpers::length(pad.left + sw / 2.0),
            right: style_helpers::length(pad.right + sw / 2.0),
            top: style_helpers::length(pad.top + sh / 2.0),
            bottom: style_helpers::length(pad.bottom + sh / 2.0),
        };
    }

    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => true,
        TableLayout::Auto => false,
    };

    let collapsed = stylo_styles.clone_border_collapse() == ::style::computed_values::border_collapse::T::Collapse;
    let table_edges = computed_border_edges(&stylo_styles);

    drop(stylo_styles);

    let mut column_sizes: Vec<taffy::Dimension> = Vec::new();
    for child_id in children.iter().copied() {
        collect_table_cells(
            doc,
            child_id,
            is_fixed,
            &mut row,
            &mut col,
            &mut items,
            &mut column_sizes,
            &mut occupied,
            &mut max_col,
        );
    }
    column_sizes.resize(max_col.max(col) as usize, style_helpers::auto());

    // A cell cannot span past the last row.
    for item in items.iter_mut().filter(|i| i.kind == TableItemKind::Cell) {
        let span = item.row_span.min(row.saturating_sub(item.row_start) + 1).max(1);
        if span != item.row_span {
            item.row_span = span;
            item.grid_row = taffy::Line {
                start: style_helpers::line(item.row_start as i16),
                end: style_helpers::span(span),
            };
        }
    }

    // pdf-engine patch: the collapsed-borders model. Every edge between two cells (or a cell and the table's edge) has one border:
    // the widest, then the strongest style, then the one nearest the top-left; `hidden` removes it. The layout of each cell holds
    // half of that width on every side (so two cells together take the whole width, and the table's box takes half of its widest
    // outer border), and the cell is told the border to paint. Without this every cell drew its own full borders, so a shared
    // edge came out twice as thick, and the two cells' styles and colours were painted over each other.
    let cell_count = items.iter().filter(|i| i.kind == TableItemKind::Cell).count();
    if collapsed && cell_count > 0 {
        let columns = column_sizes.len();
        let rows_n = row as usize;
        let cells: Vec<usize> = (0..items.len()).filter(|&i| items[i].kind == TableItemKind::Cell).collect();
        // grid[row][col] = which cell (index into `cells`) covers it
        let mut grid: Vec<Vec<Option<usize>>> = vec![vec![None; columns]; rows_n];
        for (ci, &ii) in cells.iter().enumerate() {
            let it = &items[ii];
            for r in (it.row_start as usize - 1)..((it.row_start + it.row_span) as usize - 1).min(rows_n) {
                for c in (it.col_start as usize)..((it.col_start + it.colspan) as usize).min(columns) {
                    grid[r][c] = Some(ci);
                }
            }
        }
        let own: Vec<[BorderEdge; 4]> = cells
            .iter()
            .map(|&ii| {
                doc.nodes[items[ii].node_id]
                    .primary_styles()
                    .map(|st| computed_border_edges(&st))
                    .unwrap_or([BorderEdge::none(); 4])
            })
            .collect();
        // sides: 0 top, 1 right, 2 bottom, 3 left
        let mut resolved: Vec<([BorderEdge; 4], [bool; 4])> = Vec::with_capacity(cells.len());
        let mut table_outer = [0.0f32; 4];
        for (ci, &ii) in cells.iter().enumerate() {
            let it = &items[ii];
            let (r0, r1) = (it.row_start as usize - 1, ((it.row_start + it.row_span) as usize - 1).min(rows_n));
            let (c0, c1) = (it.col_start as usize, ((it.col_start + it.colspan) as usize).min(columns));
            let mut edges = own[ci];
            let mut outer = [false; 4];
            // neighbours along each side, and whether they come first (win ties)
            for side in 0..4 {
                let mut neighbours: Vec<usize> = Vec::new();
                match side {
                    0 if r0 > 0 => neighbours.extend((c0..c1).filter_map(|c| grid[r0 - 1][c])),
                    2 if r1 < rows_n => neighbours.extend((c0..c1).filter_map(|c| grid[r1][c])),
                    1 if c1 < columns => neighbours.extend((r0..r1).filter_map(|r| grid[r][c1])),
                    3 if c0 > 0 => neighbours.extend((r0..r1).filter_map(|r| grid[r][c0 - 1])),
                    _ => {}
                }
                neighbours.sort_unstable();
                neighbours.dedup();
                neighbours.retain(|&n| n != ci);
                if neighbours.is_empty() {
                    // the edge of the table: the cell against the table's own border (a cell wins a tie)
                    outer[side] = true;
                    edges[side] = collapse_winner(edges[side], table_edges[side]);
                    continue;
                }
                // the neighbour on the opposite side of the shared edge
                let facing = (side + 2) % 4;
                let mut result: Option<BorderEdge> = None;
                for n in neighbours {
                    let theirs = own[n][facing];
                    // above or to the left comes first
                    let first_is_neighbour = side == 0 || side == 3;
                    let w = if first_is_neighbour { collapse_winner(theirs, edges[side]) } else { collapse_winner(edges[side], theirs) };
                    result = Some(match result {
                        None => w,
                        Some(prev) => collapse_winner(prev, w),
                    });
                }
                if let Some(w) = result {
                    edges[side] = w;
                }
            }
            for side in 0..4 {
                if outer[side] {
                    table_outer[side] = table_outer[side].max(edges[side].width);
                }
            }
            resolved.push((edges, outer));
        }
        for (ci, &ii) in cells.iter().enumerate() {
            let (edges, outer) = resolved[ci];
            let half = |e: &BorderEdge| style_helpers::length(if e.hidden() { 0.0 } else { e.width / 2.0 });
            let id = items[ii].node_id;
            let node = &mut doc.nodes[id];
            node.style.border = taffy::Rect {
                top: half(&edges[0]),
                right: half(&edges[1]),
                bottom: half(&edges[2]),
                left: half(&edges[3]),
            };
            node.cache.clear();
            if let Some(el) = node.data.downcast_element_mut() {
                el.collapsed_border = Some(Box::new(crate::node::CollapsedBorder {
                    widths: [edges[0].width, edges[1].width, edges[2].width, edges[3].width].map(|w| w),
                    styles: [edges[0].style, edges[1].style, edges[2].style, edges[3].style],
                    colors: [edges[0].color, edges[1].color, edges[2].color, edges[3].color],
                    outer,
                }));
                if edges.iter().any(|e| e.hidden()) {
                    if let Some(cb) = el.collapsed_border.as_mut() {
                        for (k, e) in edges.iter().enumerate() {
                            if e.hidden() {
                                cb.widths[k] = 0.0;
                            }
                        }
                    }
                }
            }
        }
        // the table's box holds half of its widest outer border on each side (its own `border` is part of the resolution)
        style.border = taffy::Rect {
            top: style_helpers::length(table_outer[0] / 2.0),
            right: style_helpers::length(table_outer[1] / 2.0),
            bottom: style_helpers::length(table_outer[2] / 2.0),
            left: style_helpers::length(table_outer[3] / 2.0),
        };
    } else {
        // not a collapsed-borders table: any state left by an earlier layout goes
        for it in items.iter().filter(|i| i.kind == TableItemKind::Cell) {
            if let Some(el) = doc.nodes[it.node_id].data.downcast_element_mut() {
                el.collapsed_border = None;
            }
        }
    }

    // pdf-engine patch: an auto-layout table gives the width it has to spare to its columns in proportion to their
    // max-content widths (CSS Tables 3 §3.2), not equally as a grid's `auto` tracks do. Each auto column becomes
    // `minmax(min-content, <max-content width>fr)`, so a `width: 100%` table lays its columns out as a browser does.
    // Only for a table with a width of its own (`width: 100%`, `width: 600px`): an auto-width table shrinks to its content,
    // and flexible columns would collapse to their minimum there.
    let table_has_width = style.size.width.tag() != taffy::CompactLength::AUTO_TAG;
    let mut column_max_content = vec![0.0f32; column_sizes.len()];
    if !is_fixed && table_has_width {
        let measured: Vec<(usize, u16)> = items
            .iter()
            .filter(|i| i.kind == TableItemKind::Cell && i.colspan == 1)
            .map(|i| (i.node_id, i.col_start))
            .collect();
        for (node_id, col_index) in measured {
            let auto_column = column_sizes
                .get(col_index as usize)
                .is_some_and(|d| d.tag() == taffy::CompactLength::AUTO_TAG);
            if !auto_column {
                continue;
            }
            let width = cell_max_content_width(doc, node_id) + 2.0 * half_spacing.0;
            if let Some(m) = column_max_content.get_mut(col_index as usize) {
                *m = m.max(width);
            }
        }
    }
    // A cell that spans several columns adds its width to them: what the columns it covers do not already supply is
    // shared equally by the ones that nothing else sizes (or by all of them).
    if !is_fixed && table_has_width {
        let spanning: Vec<(usize, u16, u16)> = items
            .iter()
            .filter(|i| i.kind == TableItemKind::Cell && i.colspan > 1)
            .map(|i| (i.node_id, i.col_start, i.colspan))
            .collect();
        for (node_id, start, span) in spanning {
            let cols: Vec<usize> = (start as usize..(start + span) as usize)
                .filter(|&c| {
                    column_sizes
                        .get(c)
                        .is_some_and(|d| d.tag() == taffy::CompactLength::AUTO_TAG)
                })
                .collect();
            if cols.is_empty() {
                continue;
            }
            let width = cell_max_content_width(doc, node_id) + 2.0 * half_spacing.0 * span as f32;
            let have: f32 = cols.iter().map(|&c| column_max_content[c]).sum();
            if width > have {
                let empty_cols: Vec<usize> = cols.iter().copied().filter(|&c| column_max_content[c] == 0.0).collect();
                let targets = if empty_cols.is_empty() { &cols } else { &empty_cols };
                let share = (width - have) / targets.len() as f32;
                for &c in targets {
                    column_max_content[c] += share;
                }
            }
        }
    }
    style.grid_template_columns = column_sizes
        .into_iter()
        .zip(column_max_content)
        .map(|(dim, max_content)| {
            if dim.tag() == taffy::CompactLength::AUTO_TAG && max_content > 0.0 {
                style_helpers::minmax(style_helpers::min_content(), style_helpers::fr(max_content))
            } else {
                TrackSizingFunction::from(dim).into()
            }
        })
        .collect();
    style.grid_template_rows = vec![style_helpers::auto(); row as usize];

    for item in items.iter_mut().filter(|i| i.kind == TableItemKind::Cell) {
        item.half_spacing = half_spacing;
    }

    let layout_children = items
        .iter()
        .filter(|item| item.kind == TableItemKind::Cell)
        .map(|cell| cell.node_id)
        .collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (TableContext { style, items }, layout_children)
}

/// The width of a cell (border box) when it takes all the room it wants.
fn cell_max_content_width(doc: &mut BaseDocument, node_id: usize) -> f32 {
    use taffy::{AvailableSpace, Line, RequestedAxis, RunMode, Size, SizingMode, tree::LayoutInput};
    doc.compute_child_layout(
        taffy::NodeId::from(node_id),
        LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::ContentSize,
            axis: RequestedAxis::Horizontal,
            known_dimensions: Size::NONE,
            parent_size: Size::NONE,
            available_space: Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        },
    )
    .size
    .width
}

pub(crate) fn collect_table_cells(
    doc: &mut BaseDocument,
    node_id: usize,
    is_fixed: bool,
    row: &mut u16,
    col: &mut u16,
    cells: &mut Vec<TableItem>,
    columns: &mut Vec<Dimension>,
    occupied: &mut Vec<u16>,
    max_col: &mut u16,
) {
    let node = &doc.nodes[node_id];

    if !node.is_element() {
        return;
    }

    let Some(display) = node.primary_styles().map(|s| s.clone_display()) else {
        println!("Ignoring table descendent because it has no styles");
        return;
    };

    if display.outside() == DisplayOutside::None {
        node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
        return;
    }

    match display.inside() {
        DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => {
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                doc.nodes[child_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                collect_table_cells(doc, child_id, is_fixed, row, col, cells, columns, occupied, max_col);
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableRow => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            *row += 1;
            *col = 0;

            cells.push(TableItem {
                kind: TableItemKind::Row,
                node_id,
                grid_column: taffy::Line {
                    start: style_helpers::line(0),
                    end: style_helpers::line(-1),
                },
                grid_row: taffy::Line {
                    start: style_helpers::line(*row as i16),
                    end: style_helpers::span(1),
                },
                width_auto: false,
                half_spacing: (0.0, 0.0),
                col_start: 0,
                colspan: 1,
                row_start: *row,
                row_span: 1,
            });

            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(doc, child_id, is_fixed, row, col, cells, columns, occupied, max_col);
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableCell => {
            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            let stylo_style = &node.primary_styles().unwrap();
            let colspan: u16 = node
                .attr(local_name!("colspan"))
                .and_then(|val| val.parse().ok())
                .unwrap_or(1);
            // `rowspan="0"` means "to the end of the row group"; the cell is clamped to the last row afterwards.
            let rowspan: u16 = node
                .attr(local_name!("rowspan"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|n| if n == 0 { u16::MAX / 2 } else { n.min(1000) })
                .unwrap_or(1);
            // Skip the columns taken by a cell from an earlier row that spans down into this one.
            while occupied.get(*col as usize).is_some_and(|&through| through >= *row) {
                *col += 1;
            }
            let style = stylo_taffy::to_taffy_style(stylo_style);

            // TODO: account for padding/border/margin
            if *row == 1 {
                let column = match style.size.width.tag() {
                    taffy::CompactLength::LENGTH_TAG => {
                        let len = style.size.width.value();
                        let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                        style_helpers::length(len + padding.left + padding.right)
                    }
                    taffy::CompactLength::PERCENT_TAG => {
                        style_helpers::percent(style.size.width.value())
                    }
                    taffy::CompactLength::AUTO_TAG => style_helpers::auto(),
                    _ => unreachable!(),
                };
                columns.push(column);
            } else if !is_fixed
                && (*col as usize) < columns.len()
                && taffy::CompactLength::LENGTH_TAG == style.size.width.tag()
            {
                let new_len = style.size.width.value();
                let tag = columns[*col as usize].tag();
                let value = columns[*col as usize].value();
                columns[*col as usize] = match tag {
                    taffy::CompactLength::LENGTH_TAG => style_helpers::length(value.max(new_len)),
                    taffy::CompactLength::AUTO_TAG => style_helpers::length(new_len),
                    taffy::CompactLength::PERCENT_TAG => style_helpers::percent(value),
                    _ => unreachable!(),
                }
            }

            cells.push(TableItem {
                kind: TableItemKind::Cell,
                node_id,
                grid_column: taffy::Line {
                    start: style_helpers::line((*col + 1) as i16),
                    end: style_helpers::span(colspan),
                },
                grid_row: taffy::Line {
                    start: style_helpers::line(*row as i16),
                    end: style_helpers::span(rowspan),
                },
                width_auto: true,
                half_spacing: (0.0, 0.0),
                col_start: *col,
                colspan,
                row_start: *row,
                row_span: rowspan,
            });

            let end_col = *col + colspan;
            if occupied.len() < end_col as usize {
                occupied.resize(end_col as usize, 0);
            }
            for c in *col..end_col {
                occupied[c as usize] = (*row).saturating_add(rowspan - 1);
            }
            *max_col = (*max_col).max(end_col);
            *col += colspan;
        }
        DisplayInside::Flow
        | DisplayInside::FlowRoot
        | DisplayInside::Flex
        | DisplayInside::Grid => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Probably a table caption: ignore
            // println!(
            //     "Warning: ignoring non-table typed descendent of table ({:?})",
            //     display.inside()
            // );
        }
        DisplayInside::TableColumnGroup | DisplayInside::TableColumn | DisplayInside::Table => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            //Ignore
        }
        DisplayInside::None => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Ignore
        }
    }
}

pub struct RangeIter(Range<usize>);

impl Iterator for RangeIter {
    type Item = taffy::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(taffy::NodeId::from)
    }
}

impl taffy::TraversePartialTree for TableTreeWrapper<'_> {
    type ChildIter<'a>
        = RangeIter
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, _node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        RangeIter(0..self.ctx.items.len())
    }

    #[inline(always)]
    fn child_count(&self, node_id: taffy::NodeId) -> usize {
        self.doc.child_count(node_id)
    }

    #[inline(always)]
    fn get_child_id(&self, _node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        index.into()
    }
}
impl taffy::TraverseTree for TableTreeWrapper<'_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style<Atom> {
        &self.ctx.style
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &taffy::Layout) {
        let node_id = taffy::NodeId::from(self.ctx.items[usize::from(node_id)].node_id);
        self.doc.set_unrounded_layout(node_id, layout)
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let cell = &self.ctx.items[usize::from(node_id)];
        match cell.kind {
            TableItemKind::Row => {
                let style = TableItemStyle::of(cell, &self.doc.nodes[cell.node_id].style);
                compute_leaf_layout(inputs, &style, resolve_calc_value, |_, _| {
                    taffy::Size::ZERO
                })
            }
            TableItemKind::Cell => {
                let node_id = taffy::NodeId::from(cell.node_id);
                self.doc.compute_child_layout(node_id, inputs)
            }
        }
    }
}

impl TableTreeWrapper<'_> {
    /// pdf-engine patch: `vertical-align` of a table cell (`middle` is the default in a browser, `top` and `bottom` the other
    /// usual values): the cell's content sits at the top, the middle or the bottom of the room the row gives it. The grid has
    /// laid every cell out at the top, so the cell is laid out again with its spare height added above its content (as extra
    /// top padding), which moves its text and its children together.
    pub(crate) fn align_cells_vertically(&mut self) {
        use ::style::values::{computed::VerticalAlign, generics::box_::VerticalAlignKeyword};
        use taffy::{AvailableSpace, Line, RequestedAxis, RunMode, Size, SizingMode, tree::LayoutInput};
        let ids: Vec<usize> = self
            .ctx
            .items
            .iter()
            .filter(|i| i.kind == TableItemKind::Cell)
            .map(|i| i.node_id)
            .collect();
        for id in ids {
            let factor = match self.doc.nodes[id]
                .primary_styles()
                .map(|st| st.clone_vertical_align())
            {
                Some(VerticalAlign::Keyword(VerticalAlignKeyword::Middle)) => 0.5,
                Some(VerticalAlign::Keyword(VerticalAlignKeyword::Bottom))
                | Some(VerticalAlign::Keyword(VerticalAlignKeyword::TextBottom)) => 1.0,
                _ => continue,
            };
            let layout = self.doc.nodes[id].unrounded_layout;
            let (w, h) = (layout.size.width, layout.size.height);
            if h <= 0.0 {
                continue;
            }
            // the height the content needs, measured without the cell's own fixed height
            let saved = {
                let st = &mut self.doc.nodes[id].style;
                let saved = (st.size.height, st.min_size.height);
                st.size.height = taffy::Dimension::auto();
                st.min_size.height = taffy::Dimension::auto();
                saved
            };
            self.doc.nodes[id].cache.clear();
            let content_h = self
                .doc
                .compute_child_layout(
                    taffy::NodeId::from(id),
                    LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        sizing_mode: SizingMode::InherentSize,
                        axis: RequestedAxis::Vertical,
                        known_dimensions: Size { width: Some(w), height: None },
                        parent_size: Size::NONE,
                        available_space: Size {
                            width: AvailableSpace::Definite(w),
                            height: AvailableSpace::MaxContent,
                        },
                        vertical_margins_are_collapsible: Line::FALSE,
                    },
                )
                .size
                .height;
            {
                let st = &mut self.doc.nodes[id].style;
                st.size.height = saved.0;
                st.min_size.height = saved.1;
            }
            self.doc.nodes[id].cache.clear();
            let shift = ((h - content_h) * factor).max(0.0);
            if shift < 0.5 {
                continue;
            }
            // lay the cell out again at its final size with the shift as extra top padding
            let original_top = self.doc.nodes[id].style.padding.top;
            let top_px = {
                let raw = original_top.into_raw();
                if raw.tag() == taffy::CompactLength::LENGTH_TAG { raw.value() } else { 0.0 }
            };
            self.doc.nodes[id].style.padding.top = style_helpers::length(top_px + shift);
            self.doc.nodes[id].cache.clear();
            self.doc.compute_child_layout(
                taffy::NodeId::from(id),
                LayoutInput {
                    run_mode: RunMode::PerformLayout,
                    sizing_mode: SizingMode::InherentSize,
                    axis: RequestedAxis::Both,
                    known_dimensions: Size { width: Some(w), height: Some(h) },
                    parent_size: Size { width: Some(w), height: Some(h) },
                    available_space: Size {
                        width: AvailableSpace::Definite(w),
                        height: AvailableSpace::Definite(h),
                    },
                    vertical_margins_are_collapsible: Line::FALSE,
                },
            );
            self.doc.nodes[id].style.padding.top = original_top;
            self.doc.nodes[id].unrounded_layout.padding.top += shift;
        }
    }
}

impl taffy::LayoutGridContainer for TableTreeWrapper<'_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = TableItemStyle<'a>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        let item = &self.ctx.items[usize::from(child_node_id)];
        TableItemStyle::of(item, &self.doc.nodes[item.node_id].style)
    }
}

/// The style of a row or cell as the table's grid sees it: its node's own style, with the placement (and the
/// cell's `auto` width) the table gave it. A view, so no style is copied per item.
pub struct TableItemStyle<'a> {
    base: &'a taffy::Style<Atom>,
    grid_row: taffy::Line<taffy::GridPlacement<Atom>>,
    grid_column: taffy::Line<taffy::GridPlacement<Atom>>,
    width_auto: bool,
    half_spacing: (f32, f32),
}

impl<'a> TableItemStyle<'a> {
    fn of(item: &TableItem, base: &'a taffy::Style<Atom>) -> Self {
        Self {
            base,
            grid_row: item.grid_row.clone(),
            grid_column: item.grid_column.clone(),
            width_auto: item.width_auto,
            half_spacing: item.half_spacing,
        }
    }
}

impl taffy::CoreStyle for TableItemStyle<'_> {
    type CustomIdent = Atom;

    fn box_generation_mode(&self) -> taffy::BoxGenerationMode {
        taffy::CoreStyle::box_generation_mode(self.base)
    }
    fn is_block(&self) -> bool {
        taffy::CoreStyle::is_block(self.base)
    }
    fn is_compressible_replaced(&self) -> bool {
        taffy::CoreStyle::is_compressible_replaced(self.base)
    }
    fn box_sizing(&self) -> taffy::BoxSizing {
        taffy::CoreStyle::box_sizing(self.base)
    }
    fn overflow(&self) -> taffy::Point<taffy::Overflow> {
        taffy::CoreStyle::overflow(self.base)
    }
    fn scrollbar_width(&self) -> f32 {
        taffy::CoreStyle::scrollbar_width(self.base)
    }
    fn position(&self) -> taffy::Position {
        taffy::CoreStyle::position(self.base)
    }
    fn inset(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        taffy::CoreStyle::inset(self.base)
    }
    fn size(&self) -> taffy::Size<Dimension> {
        let mut size = taffy::CoreStyle::size(self.base);
        if self.width_auto {
            size.width = style_helpers::auto();
        }
        // pdf-engine patch: `height` on a table row or cell is a minimum (CSS Tables 3 §3) for the row it sits in:
        // the grid sees it as a minimum height, so a spanning cell's `height` is spread over the rows it spans.
        if size.height.tag() == taffy::CompactLength::LENGTH_TAG {
            size.height = style_helpers::auto();
        }
        size
    }
    fn min_size(&self) -> taffy::Size<Dimension> {
        let mut min = taffy::CoreStyle::min_size(self.base);
        let height = taffy::CoreStyle::size(self.base).height;
        if height.tag() == taffy::CompactLength::LENGTH_TAG && min.height.tag() == taffy::CompactLength::AUTO_TAG {
            min.height = height;
        }
        min
    }
    fn max_size(&self) -> taffy::Size<Dimension> {
        taffy::CoreStyle::max_size(self.base)
    }
    fn aspect_ratio(&self) -> Option<f32> {
        taffy::CoreStyle::aspect_ratio(self.base)
    }
    fn margin(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        let mut m = taffy::CoreStyle::margin(self.base);
        let (hx, hy) = self.half_spacing;
        if hx > 0.0 || hy > 0.0 {
            // pdf-engine patch: half the table's `border-spacing` on every side of a cell, so two cells are a whole
            // spacing apart and the first and last are half a spacing in (the table adds the other half as padding).
            // A grid `gap` did the same but made a table with thousands of rows quadratically slow.
            let add = |side: taffy::LengthPercentageAuto, by: f32| {
                if side.is_auto() {
                    side
                } else {
                    let raw = side.into_raw();
                    let base = if raw.tag() == taffy::CompactLength::LENGTH_TAG { raw.value() } else { 0.0 };
                    style_helpers::length(base + by)
                }
            };
            m.left = add(m.left, hx);
            m.right = add(m.right, hx);
            m.top = add(m.top, hy);
            m.bottom = add(m.bottom, hy);
        }
        m
    }
    fn padding(&self) -> taffy::Rect<taffy::LengthPercentage> {
        taffy::CoreStyle::padding(self.base)
    }
    fn border(&self) -> taffy::Rect<taffy::LengthPercentage> {
        taffy::CoreStyle::border(self.base)
    }
}

impl taffy::GridItemStyle for TableItemStyle<'_> {
    fn grid_row(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        self.grid_row.clone()
    }
    fn grid_column(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        self.grid_column.clone()
    }
    fn align_self(&self) -> Option<taffy::AlignSelf> {
        taffy::GridItemStyle::align_self(self.base)
    }
    fn justify_self(&self) -> Option<taffy::AlignSelf> {
        taffy::GridItemStyle::justify_self(self.base)
    }
}
