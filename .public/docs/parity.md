# Parity with Chrome

How close does pdf-engine look to headless Chrome, page by page? The score is on the [demo page](https://lands-horizon-corp.github.io/pdf-engine/)
and is made by one command:

```sh
node .public/parity/parity.mjs                   # run, write the report and the demo's parity.json
node .public/parity/parity.mjs --check           # exit 1 unless HEALTHY (for CI)
node .public/parity/parity.mjs --update-baseline # accept today's scores as the new baseline
```

It needs the puppeteer install in `.public/benchmark` (`npm ci` there). The report, with a Chrome | pdf-engine | difference
image for every case, is `.public/parity/out/report.html`.

## What is compared

Every HTML file in `tests/fixtures/css` (20 documents), `tests/fixtures/probes` (64 one-feature pages),
`.public/parity/cases` (right-to-left and mixed-direction pages) and `.public/parity/cases/<category>/` (84 pagination pages,
12 in each of seven categories, and 16 complex tables: page count, forced breaks, `break-inside: avoid`, tables, images at a page break, flex and
grid across pages, and fixed headers and footers; made by `gen-cases.py`) is made into a PDF by Chrome (Letter, no margin,
backgrounds on, `@page` size respected) and by pdf-engine. **Both PDFs are drawn by the same rasteriser** (hayro, at 96 dpi),
so the difference is in the PDFs, not in the viewer.

## The score

- **Layout score** (the headline, 0 to 100): the pages are cut into 6 mm squares and the mean colour of each square is
  compared. A square counts only if it has ink in either page. A few pixels of drift in a line of text average out; a missing
  image, a wrong background or text on the wrong side does not. **A different page count scores 0** for that case.
- **Pixel score** (shown beside it, stricter): pixels that still differ after forgiving a one-pixel shift. Text that sits
  3 px lower than Chrome's loses points here and not in the layout score.
- A case **passes** at 90 or more, is **close** at 70 or more, and **fails** below. The overall score is the mean of the cases.
- **Health**: `HEALTHY` is 85 or more, no errors and no case more than 2 points below `baseline.json`. `DEGRADED` is 60 to 85
  or a regression. `BROKEN` is below 60.

No tuning per case, no alignment of the two pages before comparing, and the thresholds above are fixed.

## What it does not tell you

- The reference is Chrome, not the CSS specification. Where Chrome is wrong, matching it is still a loss here.
- Fonts differ: Chrome uses the system fonts, pdf-engine its bundled ones, so text-heavy pages never reach 100.
- A mirrored right-to-left line can score high on the layout score because the filled area is the same. The exact order of
  runs is checked by `tests/bidi.rs`, not here.
- 88 cases are a sample, not the whole of CSS.

## Where it stands (28 October 2026, pdf-engine 1.4.1 plus the patches below, Chrome 156)

Score **96.46, HEALTHY**: 187 pass, 19 close, 6 fail, of 212 cases. The first run, on 88 cases, scored 80.3.

What moved it (every engine change is in the `PATCHES.md` of `vendor/fulgur`, `vendor/parley` and `vendor/blitz-dom`):

- **Both sides use standards mode.** Several fixtures have no `<!doctype>`, which Chrome renders in quirks mode. The driver adds a
  doctype to both renderers: a change to the comparison, not to the engine. pdf-engine has no quirks mode.
- **Repeated table headers** open every page, keep their styling and are removed again when a layout shift makes them stray.
- **Tables:** `border-collapse: collapse` is a real collapsed-borders model (one border per shared edge, the widest and strongest
  wins), `vertical-align` works in cells, row backgrounds are painted, columns of a `width: 100%` table share the spare width in proportion to their content,
  a `rowspan` cell's `height` is spread over the rows it spans, `border-spacing` is applied, a cell with a nested table
  grows its row, and `outline` is drawn.
- **`line-height: normal`** is 1.15 (was 1.2), the browsers' value for their default fonts.
- **Right to left:** `dir` and `direction: rtl` set a paragraph's direction; a flex row flips in an rtl box.
- **Inline boxes:** an `inline-block` with text sits on its text baseline and its margin is honoured.
- **Images:** `<img>` of an SVG (a file or a data URI) and CSS `url()` backgrounds are drawn.
- **Pagination** (the largest group):
  - a page filled exactly no longer loses its last row (279.4 mm was 959.9999 px, so every long document needed one page too many);
  - a block, flex item or grid cell that straddles a page break is cut at the break when its first line of text fits above it,
    and moves whole otherwise (as browsers do); images, tables and `break-inside: avoid` are never cut;
  - a body that holds only text and `<br>` lines is split across pages (it used to be clipped to the first page);
  - a gradient or background image continues across the slices of a cut box;
  - `@page { size }` is honoured when the request leaves the page size at its default;
  - margin boxes (`@top-center`, `@bottom-right`, ...) are aligned left, centre or right and centred vertically.

| Category | Score | Pass |
| --- | ---: | ---: |
| columns | 34.4 | 0 of 1 |
| backgrounds | 85.7 | 6 of 7 |
| positioning | 91.8 | 2 of 3 |
| box model and selectors | 91.9 | 7 of 8 |
| complex tables | 93.2 | 11 of 16 |
| whole documents | 93.6 | 15 of 20 |
| pagination: headers and footers | 96.2 | 11 of 12 |
| pagination: break-inside avoid | 96.6 | 11 of 12 |
| right-to-left | 96.8 | 4 of 4 |
| pagination: images | 97.2 | 11 of 12 |
| inline and floats | 97.3 | 11 of 12 |
| pagination: flex and grid | 97.7 | 11 of 12 |
| borders | 99.2 | 15 of 16 |
| pagination: forced breaks | 99.3 | 12 of 12 |
| pagination: tables | 99.6 | 12 of 12 |
| colour and opacity | 100.0 | 4 of 4 |
| flexbox and grid | 100.0 | 3 of 3 |
| images | 100.0 | 2 of 2 |
| tables | 100.0 | 6 of 6 |
| transforms | 100.0 | 2 of 2 |
| pagination: page count | 100.0 | 12 of 12 |

Categories under 90, for these reasons:

- **complex tables** (16 cases, the newest group, now over 90): spans, merged headers, financial statements, invoices, timetables, rounded cells,
  nested tables and outlines match Chrome (87 to 100). A `rowspan` group that crosses page breaks under a repeating header now matches too. Still apart: thick collapsed
  borders and some border styles, and column widths that depend on glyph widths.

- **columns** (one case): `column-rule` on a box that must split across the columns when its only child is one empty tall block.
- **backgrounds** (one case in seven): an inline `<span>` with only a space and some padding paints no background.
- **whole documents** (90.1): mostly **font metrics**. Chrome draws `sans-serif` as Helvetica and the engine as Noto Sans, which has
  looser line spacing, so a long table fits 45 rows a page against Chrome's 47 and every later row sits a little lower.
  Floats are approximated (the right float sits at the top), which costs a few points on the floats page.

Other failures: `::before`/`::after` with `display: block` lay out side by side, and an `inline-block`
with text in some layouts.
