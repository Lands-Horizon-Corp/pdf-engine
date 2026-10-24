# Vendored stylo 0.8.0

Copy of [stylo](https://github.com/servo/stylo) 0.8.0 (MPL-2.0) with the patch below, used via
`[patch.crates-io]` in the root `Cargo.toml`. The change is marked `pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `stylist.rs` | Build the user-agent cascade data per document instead of through the global `UA_CASCADE_DATA_CACHE` mutex | Blitz parses its user-agent stylesheet for every document, so the cache never hit; it only serialized all render threads behind one lock (about 40% of busy time under load) |

## Upgrading stylo

Replace this directory with the new release (keep `PATCHES.md`), re-apply the patch (search
`pdf-engine patch`), then run `make check` and `make stress`.
