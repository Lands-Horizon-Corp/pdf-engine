# Vendored blitz-dom 0.2.4

Copy of [blitz-dom](https://github.com/DioxusLabs/blitz) 0.2.4 (MIT OR Apache-2.0), used via
`[patch.crates-io]`. Marked `pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/document.rs` | One `SharedRwLock` per thread instead of per document, and the parsed user-agent stylesheet is kept per thread | Parsing the user-agent stylesheet was about half of the time of a small document. Pairs with the stylo patch, which then reuses the style data built from it |
