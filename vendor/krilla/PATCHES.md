# Vendored krilla 0.7.0

Copy of [krilla](https://github.com/LaurenzV/krilla) 0.7.0 (MIT OR Apache-2.0), used via
`[patch.crates-io]`. Marked `pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/stream.rs`, `src/lib.rs` | `set_compression_level(0-9)`; the default stays 6 | The browser build lowers it: zlib is about a fifth of a small document's time |
