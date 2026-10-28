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
    // A cell with a `rowspan` is as tall as all the rows it covers, so it says nothing about this one row: the row is
    // measured by its other cells. A row made only of such cells falls back to all of them.
    let spans = |cell: usize| {
        doc.get_node(cell)
            .and_then(|n| n.element_data())
            .and_then(|e| crate::blitz_adapter::get_attr(e, "rowspan"))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .is_some_and(|n| n > 1)
    };
    let cells = element_children(doc, row);
    let single: Vec<usize> = cells.iter().copied().filter(|&c| !spans(c)).collect();
    let measured = if single.is_empty() { cells } else { single };
    let mut found: Option<(u32, f32, f32)> = None;
    for cell in measured {
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
fn simulate(rows: &[RowInfo], header_height: f32, header_rows: usize, page_height: f32) -> Vec<usize> {
    let mut starts = Vec::new();
    let Some(first) = rows.first() else { return starts };
    // The space between two rows on the same page (`border-spacing`, margins), which a row's own height leaves out. Without
    // it every page was predicted one row too long and the loop below corrected one page per pass.
    let gap = rows
        .windows(2)
        .find(|w| w[0].page == w[1].page)
        .map_or(0.0, |w| (w[1].y - (w[0].y + w[0].height)).max(0.0));
    let header_with_gaps = header_height + gap * (header_rows as f32 + 1.0);
    let mut cursor = first.y + first.height;
    for (i, row) in rows.iter().enumerate().skip(1) {
        if cursor + gap + row.height > page_height + 0.5 {
            starts.push(i);
            cursor = header_with_gaps + row.height;
        } else {
            cursor += gap + row.height;
        }
    }
    starts
}

fn plan(doc: &BaseDocument, geometry: &PaginationGeometryTable, page_height: f32) -> (Vec<Insertion>, Vec<usize>) {
    let mut out = Vec::new();
    // body rows whose header copy no longer sits at the start of a page (see below)
    let mut stale: Vec<usize> = Vec::new();
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
        // A page does not start in the middle of a small group of rows tied together by a `rowspan` cell (up to 4 rows): the break goes
        // before the group's first row, so the group moves to the next page whole. A larger group is cut where the page ends (its
        // spanning cell is split, see `plan_span_splits`).
        let reach: Vec<usize> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let span = element_children(doc, r.id)
                    .into_iter()
                    .filter_map(|c| {
                        doc.get_node(c)
                            .and_then(|n| n.element_data())
                            .and_then(|e| crate::blitz_adapter::get_attr(e, "rowspan"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .max()
                    .unwrap_or(1)
                    .clamp(1, 1000);
                if span <= 4 { i + span - 1 } else { i }
            })
            .collect();
        // A copy is stale when the row after it would have fitted on the page before: the copies put in front of earlier pages pushed the
        // rows down, so the break that was predicted (before this row) now falls elsewhere. Left in place it would break the page for
        // nothing, so it is taken out again.
        // (Left alone in a table with `rowspan` cells, where a row's height is not the room it needs.)
        let any_rowspan = rows.iter().any(|r| {
            element_children(doc, r.id).into_iter().any(|c| {
                doc.get_node(c)
                    .and_then(|n| n.element_data())
                    .and_then(|e| crate::blitz_adapter::get_attr(e, "rowspan"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .is_some_and(|n| n > 1)
            })
        });
        if has_copies && !any_rowspan {
            let gap = rows
                .windows(2)
                .find(|w| w[0].page == w[1].page)
                .map_or(0.0, |w| (w[1].y - (w[0].y + w[0].height)).max(0.0));
            for i in 1..rows.len() {
                if copy_before.contains(&rows[i].id) {
                    let (prev, row) = (&rows[i - 1], &rows[i]);
                    // the row and the rest of its small `rowspan` group have to fit together
                    let group_end = reach[i].min(rows.len() - 1).max(i);
                    let needed: f32 = rows[i..=group_end].iter().map(|r| r.height).sum::<f32>() + gap * (group_end - i) as f32;
                    if prev.page != row.page && prev.y + prev.height + gap + needed <= page_height + 0.5 {
                        stale.push(rows[i].id);
                    }
                }
            }
        }
        let starts: Vec<usize> = if !has_copies && rows.last().map(|r| r.page) != rows.first().map(|r| r.page) {
            // First look at this table: predict every continuation page at once. The header height is the tallest cell
            // of each header row, added up; if the header was not laid out, fall back to the page-by-page rule below.
            let header_height: f32 = header_rows
                .iter()
                .filter_map(|&r| row_box(doc, geometry, r).map(|(_, _, h)| h))
                .sum();
            if header_height > 0.0 && header_height < page_height / 2.0 {
                simulate(&rows, header_height, header_rows.len(), page_height)
            } else {
                react(&rows)
            }
        } else {
            react(&rows)
        };
        let group_start = |mut i: usize| {
            while i > 0 && (0..i).any(|j| reach[j] >= i) {
                i = (0..i).find(|&j| reach[j] >= i).unwrap_or(i);
            }
            i
        };
        let mut starts: Vec<usize> = starts.into_iter().map(group_start).collect();
        starts.sort_unstable();
        starts.dedup();
        for i in starts {
            if !copy_before.contains(&rows[i].id) && !stale.contains(&rows[i].id) {
                out.push(Insertion { row: rows[i].id, header_rows: header_rows.clone() });
            }
        }
    }
    (out, stale)
}

/// The rows that start a page after the first (as the layout stands now).
fn react(rows: &[RowInfo]) -> Vec<usize> {
    (1..rows.len()).filter(|&i| rows[i].page > rows[i - 1].page).collect()
}

/// A `rowspan` cell that reaches over the row a header copy is about to be put in front of. The copy is a row of its own, so
/// the cell cannot span it (its first column would be taken, and the header copy's cells pushed out of their columns): it is
/// cut in two, as a browser cuts it at a page break. The first part ends before the copy; a second, empty part with the same
/// style carries on after it.
struct SpanSplit {
    cell: usize,
    /// The first part's `rowspan`, and the second part's.
    first_span: usize,
    rest_span: usize,
    /// The column the cell starts in, so the second part is put back in the same one.
    col: usize,
}

/// The cells of `row`'s section that must be cut before `row`, and the column each of `row`'s own cells starts in.
fn plan_span_splits(doc: &BaseDocument, container: usize, row: usize) -> (Vec<SpanSplit>, Vec<(usize, usize)>) {
    let rows: Vec<usize> = element_children(doc, container).into_iter().filter(|&r| tag(doc, r) == Some("tr")).collect();
    let Some(target) = rows.iter().position(|&r| r == row) else {
        return (Vec::new(), Vec::new());
    };
    let attr_usize = |cell: usize, name: &str| -> usize {
        doc.get_node(cell)
            .and_then(|n| n.element_data())
            .and_then(|e| crate::blitz_adapter::get_attr(e, name))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(1)
    };
    let mut occupied: Vec<usize> = Vec::new(); // occupied[col] = the first row index that column is free again
    let mut splits = Vec::new();
    let mut row_cols = Vec::new();
    for (ri, &r) in rows.iter().enumerate().take(target + 1) {
        let mut col = 0;
        for cell in element_children(doc, r) {
            if !matches!(tag(doc, cell), Some("td") | Some("th")) {
                continue;
            }
            while occupied.get(col).is_some_and(|&free_at| free_at > ri) {
                col += 1;
            }
            let rowspan = attr_usize(cell, "rowspan").clamp(1, 1000);
            let colspan = attr_usize(cell, "colspan").clamp(1, 1000);
            if ri < target && ri + rowspan > target {
                splits.push(SpanSplit { cell, first_span: target - ri, rest_span: ri + rowspan - target, col });
            }
            if ri == target {
                row_cols.push((cell, col));
            }
            if occupied.len() < col + colspan {
                occupied.resize(col + colspan, 0);
            }
            for c in col..col + colspan {
                occupied[c] = ri + rowspan;
            }
            col += colspan;
        }
    }
    (splits, row_cols)
}

/// The look a header cell gets from rules that name the `<thead>` (`thead th { background: ...; color: ... }`): a copy is put among the
/// body rows, where those rules no longer match, so its background, text colour and font are written onto the copy as they computed
/// on the original.
fn header_look(doc: &BaseDocument, cell: usize) -> String {
    let Some(st) = doc.get_node(cell).and_then(|n| n.primary_styles()) else {
        return String::new();
    };
    let current = st.clone_color();
    let rgba = |c: ::style::values::computed::Color| {
        let c = c.resolve_to_absolute(&current).into_srgb_legacy();
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("rgba({},{},{},{:.3})", q(c.components.0), q(c.components.1), q(c.components.2), c.alpha.clamp(0.0, 1.0))
    };
    let mut out = String::new();
    let bg = st.clone_background_color().resolve_to_absolute(&current).into_srgb_legacy();
    if bg.alpha > 0.0 {
        out.push_str(&format!("background-color:{};", rgba(st.clone_background_color())));
    }
    out.push_str(&format!("color:{};", rgba(::style::values::computed::Color::currentcolor())));
    out.push_str(&format!("font-weight:{};", st.clone_font_weight().value()));
    out.push_str(&format!("font-size:{}px;", st.clone_font_size().computed_size().px()));
    out
}

/// Put header copies in front of every body row that starts a continuation page and has none yet. Returns how many
/// rows were added (0 means the layout is final).
pub(crate) fn repeat_table_headers(
    doc: &mut blitz_dom::BaseDocument,
    geometry: &PaginationGeometryTable,
    page_height_px: f32,
) -> usize {
    let (insertions, stale) = plan(doc, geometry, page_height_px);
    let mut added = 0;
    // Take out the copies that no longer start a page: the copy rows directly in front of the row.
    for row in stale {
        let Some(parent) = doc.get_node(row).and_then(|n| n.parent) else { continue };
        let siblings = element_children(doc, parent);
        let Some(pos) = siblings.iter().position(|&r| r == row) else { continue };
        let copies: Vec<usize> = siblings[..pos].iter().rev().take_while(|&&r| is_copy(doc, r)).copied().collect();
        if copies.is_empty() {
            continue;
        }
        let mut m = doc.mutate();
        for c in &copies {
            m.remove_and_drop_node(*c);
        }
        added += copies.len();
    }
    for ins in insertions {
        let container = doc.get_node(ins.row).and_then(|n| n.parent);
        let (splits, row_cols) = match container {
            Some(c) => plan_span_splits(doc, c, ins.row),
            None => (Vec::new(), Vec::new()),
        };
        let mut m = doc.mutate();
        // Cut the `rowspan` cells that reach over the row, and put the second part of each back in its column.
        let rowspan_attr = || blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from("rowspan"));
        for split in &splits {
            let rest = m.deep_clone_node(split.cell);
            m.remove_and_drop_all_children(rest);
            m.set_attribute(rest, rowspan_attr(), &split.rest_span.to_string());
            m.set_attribute(split.cell, rowspan_attr(), &split.first_span.to_string());
            // before the first cell of the row that starts in a later column, or at the end of the row
            let before = row_cols.iter().find(|&&(_, c)| c > split.col).map(|&(cell, _)| cell);
            match before {
                Some(anchor) => m.insert_nodes_before(anchor, &[rest]),
                None => m.append_children(ins.row, &[rest]),
            }
        }
        let mut copies = Vec::with_capacity(ins.header_rows.len());
        for (k, &h) in ins.header_rows.iter().enumerate() {
            let copy = m.deep_clone_node(h);
            m.set_attribute(
                copy,
                blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from(COPY_ATTR)),
                "1",
            );
            // The cells of the copy, in order, and the cells of the row it was cloned from.
            let copy_cells: Vec<usize> = m
                .doc
                .get_node(copy)
                .map(|n| n.children.iter().copied().filter(|&c| m.doc.get_node(c).is_some_and(|c| c.element_data().is_some())).collect())
                .unwrap_or_default();
            let original_cells = element_children(m.doc, h);
            let style_name = || blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from("style"));
            for (i, &cell) in copy_cells.iter().enumerate() {
                let existing = m
                    .doc
                    .get_node(cell)
                    .and_then(|n| n.element_data())
                    .and_then(|e| crate::blitz_adapter::get_attr(e, "style"))
                    .unwrap_or("")
                    .trim_end_matches(|c: char| c == ';' || c.is_whitespace())
                    .to_string();
                let mut style_attr = String::new();
                // the original's computed look goes first, so the cell's own inline style still wins
                if let Some(&orig) = original_cells.get(i) {
                    style_attr.push_str(&header_look(m.doc, orig));
                }
                if !existing.is_empty() {
                    style_attr.push_str(&existing);
                    style_attr.push(';');
                }
                // Opens the page, so the copy is never left alone at the bottom of the one before. The break rules are read from the
                // text of the `style` attribute (not the parsed style), only the first cell of the row asks for it.
                if k == 0 && i == 0 {
                    style_attr.push_str("break-before:page;");
                }
                if !style_attr.is_empty() {
                    m.set_attribute(cell, style_name(), &style_attr);
                }
            }
            copies.push(copy);
        }
        added += copies.len();
        m.insert_nodes_before(ins.row, &copies);
    }
    added
}
