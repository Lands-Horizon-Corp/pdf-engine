# Vendored blitz-dom 0.2.4

Copy of [blitz-dom](https://github.com/DioxusLabs/blitz) 0.2.4 (MIT OR Apache-2.0), used via
`[patch.crates-io]`. Marked `pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/document.rs` | One `SharedRwLock` per thread instead of per document, and the parsed user-agent stylesheet is kept per thread | Parsing the user-agent stylesheet was about half of the time of a small document. Pairs with the stylo patch, which then reuses the style data built from it |
| 2 | `src/stylo_to_parley.rs` | A `font-family` list with no generic family gets `sans-serif` added at the end | A list of names that are not installed (`font-family: Comic Sans MS`) resolved to no font, so no text was drawn; browsers fall back to their default font |
| 3 | `src/document.rs` | The media type of the document is `print`, not `screen` | The output is a PDF, which is print media: `@media print` rules were ignored and `@media screen` ones were applied. One stylesheet can now serve the browser's Print and the PDF |
| 4 | `src/layout/list.rs`, `assets/default.css` | List bullets use `Noto Sans`, then `Noto Sans Math`, before the generic families | In the browser build there is no system font, so `▸` had no glyph and `<summary>` showed an empty box. The `<details>` triangle is now a `summary::before` (normal font stack) because an inside list marker looks its font up from the asset bundle only |
| 5 | `src/layout/construct.rs`, `layout/damage.rs`, `node/node.rs` | `float: left`/`right` is approximated: a block with floats gets an anonymous wrapping flex row (left floats, the in-flow content as one item with a 150px basis, right floats), from the first float up to an element with `clear` | The layout engine has no float. Text does not wrap under a float: it sits beside it, or below it when less than 150px is left |
