# Vendored parley 0.6.0

Copy of [parley](https://github.com/linebender/parley) 0.6.0 (Apache-2.0 OR MIT), used via `[patch.crates-io]`. Marked
`pdf-engine patch` in the source.

| # | File | Change | Why |
| --- | --- | --- | --- |
| 1 | `src/context.rs` | `LayoutContext::base_direction_rtl: Option<bool>`: the paragraph's base direction, used by the bidi resolver (also for text with no right-to-left letters) | The base direction was always taken from the first strong letter, so CSS `direction: rtl` / `dir="rtl"` had no effect on Latin text, `text-align: start`, or the order of runs |
| 2 | `src/inline_box.rs`, `src/layout/line/greedy.rs`, `src/layout/line/mod.rs` | `InlineBox::baseline`: a box with a text baseline is placed on it (above and below the line's baseline) | An `inline-block` with text sat on its bottom edge, so badges and buttons were lower than in a browser |
