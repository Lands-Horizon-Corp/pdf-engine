# CSS support

Rendering is done by [Fulgur](https://github.com/fulgur-rs/fulgur) 0.40 (patched, see
[vendor/fulgur/PATCHES.md](../vendor/fulgur/PATCHES.md)). Each feature below is covered by a
fixture in `tests/fixtures`.

## Works

Flexbox, grid, block layout (margin, padding, `box-sizing`), positioned elements, transforms,
opacity, borders (all sides and colors, dashed/dotted/double, radius), box shadows, colors
(hex, `rgb()`, `hsl()`, named), linear/radial gradients, tables (`border-collapse`, `colspan`,
cell backgrounds), inline SVG, `<img>` (PNG/JPEG/GIF/SVG via `data:` URIs or allowlisted
URLs), CSS variables, `calc()`, `nth-child`, page breaks, Unicode (CJK, Arabic, emoji).

## Known gaps

| Feature | Result | Use instead |
| --- | --- | --- |
| `<thead>` on multi-page tables | Not repeated on later pages | Accept, or split tables per page |
| `background: url(...)`, `@font-face`, `@import` | Not loaded | `<img>`; add fonts via [fonts](../README.md#fonts) |
| Rows of `inline-block` that wrap | Not drawn | Flexbox with `flex-wrap` |
| `float` | Text doesn't wrap | Flexbox or grid |
| `z-index` | Document order wins | Reorder elements |
| `rowspan` | Wrong layout | Nested tables or grid |
| `@page { size }` | Ignored | `width` / `height` form fields |
| `@page` margin boxes, `position: running()` | Misplaced | Keep headers simple |
| `@media print` | Not applied | Unconditional rules |
| `::before`/`::after` with `display:block`, `counter()` | Wrong / empty | Real elements |
| `text-transform`, `outline`, `::first-letter`, inline-element backgrounds, multi-column with block children, SVG `<text>` | Not rendered | Transform text in the template; `border` instead of `outline` |
