//! pdf-engine patch (PATCHES.md #10): repeat a table's `<thead>` rows at the top of every page the table continues onto.
//!
//! The layout is paginated first, which shows which body row starts each continuation page. A copy of the header rows
//! is then put before that row (`break-before: page`, so the copy always opens the page and is never left alone at the
//! bottom of the previous one), the document is laid out again, and this repeats until no page is missing a header.
//! Each pass only adds copies in front of rows that start a page, and a page that gains a header may push its last row
//! onto the next page, which then needs a header of its own, so the loop ends after about one pass per page of the table.

use crate::pagination_layout::PaginationGeometryTable;
use blitz_dom::BaseDocument;

/// The most layout passes spent on headers: one per page of the longest table, so a table of up to this many pages repeats
/// its header on every one.
pub(crate) const MAX_PASSES: usize = 40;

/// Marks a row that was copied from a `<thead>` by this module.
const COPY_ATTR: &str = "data-fulgur-thead-copy";

fn tag(doc: &BaseDocument, id: usize) -> Option<&str> {
    doc.get_node(id)?.element_data().map(|e| e.name.local.as_ref())
}

fn is_copy(doc: &BaseDocument, id: usize) -> bool {
    doc.get_node(id)
        .and_then(|n| n.element_data())
        .is_some_and(|e| crate::blitz_adapter::get_attr(e, COPY_ATTR).is_some())
}

fn element_children(doc: &BaseDocument, id: usize) -> Vec<usize> {
    doc.get_node(id)
        .map(|n| n.children.iter().copied().filter(|&c| doc.get_node(c).is_some_and(|c| c.element_data().is_some())).collect())
        .unwrap_or_default()
}


/// Where a copy of the header rows must go: before `row`, the first body row on a page the table continues onto.
struct Insertion {
    row: usize,
    header_rows: Vec<usize>,
}

/// One body row as the first layout placed it.
struct RowInfo {
    id: usize,
    page: u32,
    /// Top of the row on its page, and its height, in px.
    y: f32,
    height: f32,
}

/// The size of a row: its cells' fragments (the row itself has none). The tallest cell is the row.
fn row_box(doc: &BaseDocument, geometry: &PaginationGeometryTable, row: usize) -> Option<(u32, f32, f32)> {
    let mut found: Option<(u32, f32, f32)> = None;
    for cell in element_children(doc, row) {
        if let Some(f) = geometry.get(&cell).and_then(|g| g.fragments.first()) {
            let (y, h) = (f.y.to_f32(), f.height.to_f32());
            found = Some(match found {
                None => (f.page_index, y, h),
                Some((p, y0, h0)) => (p, y0.min(y), h0.max(h)),
            });
        }
    }
    found
}

/// The first layout leaves no room for the headers it is about to add, so every continuation page holds fewer rows than it
/// showed. Walking the rows again with that room taken away tells where every page will start, so all the copies go in at
/// once and the loop only has to correct what the estimate got slightly wrong.
fn simulate(rows: &[RowInfo], header_height: f32, page_height: f32) -> Vec<usize> {
    let mut starts = Vec::new();
    let Some(first) = rows.first() else { return starts };
    let mut cursor = first.y + first.height;
    for (i, row) in rows.iter().enumerate().skip(1) {
        if cursor + row.height > page_height + 0.5 {
            starts.push(i);
            cursor = header_height + row.height;
        } else {
            cursor += row.height;
        }
    }
    starts
}

fn plan(doc: &BaseDocument, geometry: &PaginationGeometryTable, page_height: f32) -> Vec<Insertion> {
    let mut out = Vec::new();
    for (table_id, node) in doc.tree().iter() {
        if node.element_data().is_none() || tag(doc, table_id) != Some("table") {
            continue;
        }
        let parts = element_children(doc, table_id);
        let header_rows: Vec<usize> = parts
            .iter()
            .copied()
            .filter(|&p| tag(doc, p) == Some("thead"))
            .flat_map(|p| element_children(doc, p))
            .filter(|&r| tag(doc, r) == Some("tr"))
            .collect();
        if header_rows.is_empty() {
            continue;
        }
        // The body rows in order, as (container, index among its element children), with the copies left out.
        let containers: Vec<usize> = parts.iter().copied().filter(|&p| tag(doc, p) == Some("tbody")).collect();
        let mut rows: Vec<RowInfo> = Vec::new();
        let mut has_copies = false;
        let mut copy_before: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for &container in &containers {
            let siblings = element_children(doc, container);
            for (i, &row) in siblings.iter().enumerate() {
                if tag(doc, row) != Some("tr") {
                    continue;
                }
                if is_copy(doc, row) {
                    has_copies = true;
                    continue;
                }
                if i > 0 && is_copy(doc, siblings[i - 1]) {
                    copy_before.insert(row);
                }
                if let Some((page, y, height)) = row_box(doc, geometry, row) {
                    rows.push(RowInfo { id: row, page, y, height });
                }
            }
        }
        if rows.len() < 2 {
            continue;
        }
        let starts: Vec<usize> = if !has_copies && rows.last().map(|r| r.page) != rows.first().map(|r| r.page) {
            // First look at this table: predict every continuation page at once. The header height is the tallest cell
            // of each header row, added up; if the header was not laid out, fall back to the page-by-page rule below.
            let header_height: f32 = header_rows
                .iter()
                .filter_map(|&r| row_box(doc, geometry, r).map(|(_, _, h)| h))
                .sum();
            if header_height > 0.0 && header_height < page_height / 2.0 {
                simulate(&rows, header_height, page_height)
            } else {
                react(&rows)
            }
        } else {
            react(&rows)
        };
        for i in starts {
            if !copy_before.contains(&rows[i].id) {
                out.push(Insertion { row: rows[i].id, header_rows: header_rows.clone() });
            }
        }
    }
    out
}

/// The rows that start a page after the first (as the layout stands now).
fn react(rows: &[RowInfo]) -> Vec<usize> {
    (1..rows.len()).filter(|&i| rows[i].page > rows[i - 1].page).collect()
}

/// Put header copies in front of every body row that starts a continuation page and has none yet. Returns how many
/// rows were added (0 means the layout is final).
pub(crate) fn repeat_table_headers(
    doc: &mut blitz_dom::BaseDocument,
    geometry: &PaginationGeometryTable,
    page_height_px: f32,
) -> usize {
    let insertions = plan(doc, geometry, page_height_px);
    let mut added = 0;
    for ins in insertions {
        let mut m = doc.mutate();
        let mut copies = Vec::with_capacity(ins.header_rows.len());
        for (k, &h) in ins.header_rows.iter().enumerate() {
            let copy = m.deep_clone_node(h);
            m.set_attribute(
                copy,
                blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from(COPY_ATTR)),
                "1",
            );
            if k == 0 {
                // Opens the page, so the copy is never left alone at the bottom of the one before. The break is read from
                // a cell (the table's layout children are its cells), and only the first cell of the row asks for it.
                let first_cell = m.doc.get_node(copy).and_then(|n| n.children.iter().copied().find(|&c| m.doc.get_node(c).is_some_and(|c| c.element_data().is_some())));
                if let Some(cell) = first_cell {
                    m.set_style_property(cell, "break-before", "page");
                }
            }
            copies.push(copy);
        }
        added += copies.len();
        m.insert_nodes_before(ins.row, &copies);
    }
    added
}
