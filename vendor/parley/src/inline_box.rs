// Copyright 2024 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

/// A box to be laid out inline with text
#[derive(PartialEq, Debug, Clone)]
pub struct InlineBox {
    /// User-specified identifier for the box, which can be used by the user to determine which box in
    /// parley's output corresponds to which box in its input.
    pub id: u64,
    /// The byte offset into the underlying text string at which the box should be placed.
    /// This must not be within a Unicode code point.
    pub index: usize,
    /// The width of the box in pixels
    pub width: f32,
    /// The height of the box in pixels
    pub height: f32,
    /// pdf-engine patch: distance from the top of the box to the baseline it aligns on the line (the first text
    /// baseline of an `inline-block`). `None` puts the bottom of the box on the baseline, as before.
    pub baseline: Option<f32>,
}
