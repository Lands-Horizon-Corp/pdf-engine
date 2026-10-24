# Vendored taffy 0.9.2

Copy of [taffy](https://github.com/DioxusLabs/taffy) 0.9.2 (MIT) with the patch below, used via
`[patch.crates-io]` in the root `Cargo.toml`. The change is marked `pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/compute/block.rs` | Table children of a block get their explicit width resolved against the container's content box | A `<table style="width:100%">` directly inside an element with horizontal padding (very common: `body { padding: 30px }`) resolved `100%` against the padded outer width and overflowed the right edge by the padding |

## Upgrading taffy

Replace this directory with the new release (keep `PATCHES.md`), re-apply the patch (search
`pdf-engine patch`) unless upstream fixed it, then run `make check`.
