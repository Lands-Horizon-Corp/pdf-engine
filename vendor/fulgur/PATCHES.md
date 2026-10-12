# Vendored fulgur 0.40.0

Unmodified copy of [fulgur](https://github.com/fulgur-rs/fulgur) 0.40.0 from crates.io
(license: MIT OR Apache-2.0) with one patch, applied through `[patch.crates-io]` in the
workspace `Cargo.toml`.

## Patch: share the system font context across renders

`src/blitz_adapter.rs`, `parse_inner`: when no fonts are bundled and system fonts are enabled,
Blitz builds a fresh `parley::FontContext` for every document, which re-enumerates every font
fontconfig exposes. That dominates render time for small documents. The patch builds the
context once per process (`OnceLock`) and hands each document a clone.

Measured on a 20-line invoice (10-core aarch64, curated 28-font set):

| | Stock | Patched |
| --- | --- | --- |
| Invoice | 9.6 ms | 2.4 ms |
| Generic font families | 8.2 ms | 1.3 ms |
| CJK + Arabic + emoji | 18.1 ms | 11.2 ms |

Trade-off: fonts installed after the first render aren't seen until the process restarts.

## Upgrading fulgur

1. Replace this directory with the new release's crate source (`src/`, `Cargo.toml`, `README.md`).
2. Re-apply the patch above (search for `pdf-engine patch`), or drop the vendored copy and the
   `[patch.crates-io]` entry if upstream offers a way to reuse a font context.
3. Run `make check` and `make stress MODE=matrix`.

## Upstream proposal

Suggested API so this copy can be removed: let callers supply a shared font context, e.g.
`EngineBuilder::font_context(parley::FontContext)` (or an `Arc`-shared handle) that
`parse_inner` clones instead of constructing a new one.
