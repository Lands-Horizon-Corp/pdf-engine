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

    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => true,
        TableLayout::Auto => false,
    };

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

    style.grid_template_columns = column_sizes
        .into_iter()
        .map(|dim| TrackSizingFunction::from(dim).into())
        .collect();
    style.grid_template_rows = vec![style_helpers::auto(); row as usize];

    let layout_children = items
        .iter()
        .filter(|item| item.kind == TableItemKind::Cell)
        .map(|cell| cell.node_id)
        .collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (TableContext { style, items }, layout_children)
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
}

impl<'a> TableItemStyle<'a> {
    fn of(item: &TableItem, base: &'a taffy::Style<Atom>) -> Self {
        Self {
            base,
            grid_row: item.grid_row.clone(),
            grid_column: item.grid_column.clone(),
            width_auto: item.width_auto,
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
        size
    }
    fn min_size(&self) -> taffy::Size<Dimension> {
        taffy::CoreStyle::min_size(self.base)
    }
    fn max_size(&self) -> taffy::Size<Dimension> {
        taffy::CoreStyle::max_size(self.base)
    }
    fn aspect_ratio(&self) -> Option<f32> {
        taffy::CoreStyle::aspect_ratio(self.base)
    }
    fn margin(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        taffy::CoreStyle::margin(self.base)
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
