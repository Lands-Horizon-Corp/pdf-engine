# Vendored fulgur 0.40.0

Copy of [fulgur](https://github.com/fulgur-rs/fulgur) 0.40.0 (MIT OR Apache-2.0) with the
patches below, used via `[patch.crates-io]` in the root `Cargo.toml`. Each is marked
`pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/blitz_adapter.rs` | Build the system font context once per process; `reload_system_fonts()` swaps it | Renders were rebuilding fonts from fontconfig every time (4–6× slower) |
| 2 | `src/convert/style/mod.rs` | Convert colors to sRGB before reading components | `hsl()`/`hsla()` were drawn as garbage colors |
| 3 | `src/convert/style/border.rs`, `src/draw_primitives.rs` | Per-side border colors | Every side used `border-top-color` |
| 4 | `src/blitz_adapter.rs` | The shared font context starts with every font file already loaded | Each render re-opened and re-mapped the font files |
| 5 | `src/convert/mod.rs` | Font file bytes are copied once per process, keyed by the file's content (length and a hash of its first, middle and last 4 KB) | Each render copied every font it used (many MB for CJK fonts). Keying on the address leaked a full copy of any font opened on demand on every render (about 7 MB per document) |
| 6 | `src/blitz_adapter.rs`, `src/lib.rs` | `set_extra_fonts`: host-supplied font files join the shared font context | Lets an embedding app (the Node.js client's `fonts` option) bring its own fonts |
| 7 | `src/blitz_adapter.rs` | A generic family (`sans-serif`, `serif`, `monospace`, `system-ui`, ...) with no installed font uses the first supplied font (Noto Sans when present) | In the browser build the only fonts are the ones supplied, so `font-family: Arial, sans-serif` drew no text at all |
| 8 | `src/blitz_adapter.rs`, `src/lib.rs` | `with_doc_font_faces`: fonts from a document's `@font-face` rules join that one render's font context, under the CSS family name | Blitz resolves `@font-face` through a network provider that the engine does not have, so such rules were ignored |
| 9 | `src/pagination_layout.rs` | A multi-line paragraph records geometry for the boxes inside it on every page it covers | `inline-block`, `inline-flex` and similar boxes inside a paragraph that wrapped onto a second line were never drawn |
| 10 | `src/table_header.rs`, `src/engine.rs` | A table's `<thead>` rows are copied in front of every body row that starts a page after the first, then the document is laid out again | Long tables showed their header only on the first page. The first pass predicts all the page starts from the row heights, so a table costs about one extra layout; documents without a `<thead>` pay nothing |
| 11 | `src/config.rs`, `engine.rs`, `render.rs`, `lib.rs` | A `PdfA` conformance level (`1b`, `2b`, `2u`, `2a`, `3b`, `3a`) on the config and builder; the PDF writer validates against it (the `a` levels imply tagging) | PDF/A archival output |
| 12 | `src/lib.rs`, `render.rs`, `image.rs` | `with_image_interpolation(on, f)`: images are drawn with the `/Interpolate` flag unless turned off for the thread | PDF/A forbids the flag, so the renderer turns it off for PDF/A documents |
| 13 | `src/blitz_adapter.rs`, `lib.rs` | A "light" font context (fonts the host builds in, `set_static_fonts`) is used for a document its fonts cover, and the system fonts are looked at only for a document that needs them (`document_needs_system_fonts`); system fonts are not all opened up front on Apple platforms | Enumerating the system fonts cost about 200 MB resident and 100 ms on macOS, for every process, even for a receipt |
| 14 | `src/blitz_adapter.rs` | Font bytes are shared as blobs, never copied: the host's fonts keep one identity in every context, `@font-face` fonts are shared with the document, and built-in fonts are `&'static` | A copy of every font for every document and every context |
| 15 | `src/blitz_adapter.rs` | `expected_nodes` is passed from the number of `<` in the HTML | See blitz-dom patch 6 |

## Upgrading fulgur

1. Replace `src/`, `Cargo.toml` and `README.md` with the new release.
2. Re-apply the patches that upstream hasn't fixed (search `pdf-engine patch` in this copy).
3. Run `make check`.

Upstream proposals: a way to supply a shared/reloadable font context; patches 2 and 3 as bug fixes.
