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

## Upgrading fulgur

1. Replace `src/`, `Cargo.toml` and `README.md` with the new release.
2. Re-apply the patches that upstream hasn't fixed (search `pdf-engine patch` in this copy).
3. Run `make check`.

Upstream proposals: a way to supply a shared/reloadable font context; patches 2 and 3 as bug fixes.
