# Vendored stylo 0.8.0

Copy of [stylo](https://github.com/servo/stylo) 0.8.0 (MPL-2.0) with the patch below, used via
`[patch.crates-io]` in the root `Cargo.toml`. The change is marked `pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `stylist.rs` | The user-agent cascade data cache is per thread (`thread_local`) instead of one global mutex | The cache is useful only if documents share a parsed user-agent stylesheet, which the patched blitz-dom does per thread. A global mutex made all render threads queue (about 40% of busy time under load) |

## Upgrading stylo

Replace this directory with the new release (keep `PATCHES.md`), re-apply the patch (search
`pdf-engine patch`), then run `make check` and `make stress`.
