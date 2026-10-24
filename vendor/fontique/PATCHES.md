# Vendored fontique 0.6.0

Copy of [fontique](https://github.com/linebender/parley) 0.6.0 (Apache-2.0 OR MIT) with the patch
below, used via `[patch.crates-io]` in the root `Cargo.toml`. The change is marked
`pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/source_cache.rs` | Cloning a `SourceCache` gives each font its own `Blob` handle over the same mapped bytes | The system font context is shared by all renders. With one shared handle, the shaper's per-cluster clone/drop made every render thread hammer one atomic refcount (report documents used ~60% more CPU at 8 concurrent renders) |

## Upgrading fontique

Replace this directory with the new release (keep `PATCHES.md`), re-apply the patch (search
`pdf-engine patch`), then run `make check` and `make stress`.
