//! A rendered PDF as a black-and-white bitmap, and that bitmap as printer commands.
//!
//! Receipt and label printers print dots, not PDF. The document is rendered as usual (so every CSS feature,
//! font and script works), rasterised at the printer's resolution, trimmed of the blank paper below the
//! content, and encoded as ESC/POS (receipt printers) or ZPL (Zebra-style label printers).

use crate::error::AppError;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;

/// One byte per dot, 1 = black. `width` is a multiple of 8 so rows pack into whole bytes.
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub dots: Vec<u8>,
}

impl Bitmap {
    /// Rows packed 8 dots to a byte, most significant bit first, 1 = black.
    fn packed_row(&self, row: usize) -> impl Iterator<Item = u8> + '_ {
        self.dots[row * self.width..(row + 1) * self.width]
            .chunks_exact(8)
            .map(|c| c.iter().fold(0u8, |acc, &d| (acc << 1) | d))
    }

    pub fn bytes_per_row(&self) -> usize {
        self.width / 8
    }
}

/// Printable width in dots for a width in millimetres at `dpi`, rounded down to whole bytes.
pub fn dots_for_mm(mm: f32, dpi: f32) -> usize {
    ((mm / 25.4 * dpi) as usize / 8 * 8).max(8)
}

/// Rasterise a PDF to `dots` wide, all its pages stacked, and trim the blank paper at the bottom.
/// `dither` spreads grey (photos) into dots; text and lines look best without it.
pub fn pdf_to_bitmap(pdf: &[u8], dots: usize, dither: bool) -> Result<Bitmap, AppError> {
    let dots = dots.max(8) / 8 * 8;
    let document = Pdf::new(pdf.to_vec())
        .map_err(|e| AppError::Render(format!("cannot read the PDF: {e:?}")))?;
    let first = document
        .pages()
        .iter()
        .next()
        .ok_or_else(|| AppError::Render("the document has no pages".into()))?;
    let (page_width, _) = first.render_dimensions();
    if page_width <= 0.0 {
        return Err(AppError::Render("the page has no width".into()));
    }
    let scale = dots as f32 / page_width;
    let pages = hayro::render_pdf(&document, scale, InterpreterSettings::default(), None)
        .ok_or_else(|| AppError::Render("cannot rasterise the PDF".into()))?;

    // Grey levels, white = 255, pages stacked, `dots` wide (cropped or padded with white).
    let mut grey: Vec<u8> = Vec::new();
    let mut height = 0usize;
    for pixmap in &pages {
        let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
        let rgba = pixmap.data_as_u8_slice();
        for y in 0..h {
            for x in 0..dots {
                grey.push(if x < w {
                    let p = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                    // The page is drawn on white, so the premultiplied colour is the colour.
                    ((299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32) / 1000) as u8
                } else {
                    255
                });
            }
        }
        height += h;
    }

    let height = trim_blank_rows(&grey, dots, height);
    grey.truncate(height * dots);
    let dots_out = if dither {
        floyd_steinberg(&mut grey, dots, height)
    } else {
        threshold(&grey)
    };
    Ok(Bitmap {
        width: dots,
        height,
        dots: dots_out,
    })
}

/// Rows to keep: everything down to the last row with ink, plus a little paper below it.
fn trim_blank_rows(grey: &[u8], width: usize, height: usize) -> usize {
    let last_ink = (0..height)
        .rev()
        .find(|&y| grey[y * width..(y + 1) * width].iter().any(|&g| g < 200));
    last_ink.map_or(1, |y| (y + 1 + 8).min(height))
}

fn threshold(grey: &[u8]) -> Vec<u8> {
    grey.iter().map(|&g| u8::from(g < 160)).collect()
}

fn floyd_steinberg(grey: &mut [u8], width: usize, height: usize) -> Vec<u8> {
    let mut work: Vec<i32> = grey.iter().map(|&g| g as i32).collect();
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let old = work[i].clamp(0, 255);
            let new = if old < 128 { 0 } else { 255 };
            out[i] = u8::from(new == 0);
            let err = old - new;
            if x + 1 < width {
                work[i + 1] += err * 7 / 16;
            }
            if y + 1 < height {
                if x > 0 {
                    work[i + width - 1] += err * 3 / 16;
                }
                work[i + width] += err * 5 / 16;
                if x + 1 < width {
                    work[i + width + 1] += err / 16;
                }
            }
        }
    }
    out
}

/// ESC/POS: initialise, the bitmap as raster bands (`GS v 0`), feed, and cut.
pub fn encode_escpos(bitmap: &Bitmap, feed_lines: u8, cut: bool) -> Vec<u8> {
    const BAND: usize = 128; // rows per command: small enough for printers with little buffer
    let bytes_per_row = bitmap.bytes_per_row();
    let mut out = vec![0x1B, 0x40]; // ESC @
    let mut row = 0;
    while row < bitmap.height {
        let rows = BAND.min(bitmap.height - row);
        out.extend_from_slice(&[0x1D, 0x76, 0x30, 0x00]); // GS v 0, normal density
        out.extend_from_slice(&(bytes_per_row as u16).to_le_bytes());
        out.extend_from_slice(&(rows as u16).to_le_bytes());
        for r in row..row + rows {
            out.extend(bitmap.packed_row(r));
        }
        row += rows;
    }
    if feed_lines > 0 {
        out.extend_from_slice(&[0x1B, 0x64, feed_lines]); // ESC d n
    }
    if cut {
        out.extend_from_slice(&[0x1D, 0x56, 0x42, 0x00]); // GS V B 0: feed to the cutter and partial cut
    }
    out
}

/// ZPL: one graphic field (`^GFA`, hexadecimal) the size of the label.
pub fn encode_zpl(bitmap: &Bitmap) -> Vec<u8> {
    let bytes_per_row = bitmap.bytes_per_row();
    let total = bytes_per_row * bitmap.height;
    let mut hex = String::with_capacity(total * 2);
    for row in 0..bitmap.height {
        for byte in bitmap.packed_row(row) {
            hex.push_str(&format!("{byte:02X}"));
        }
    }
    format!(
        "^XA^PW{}^LL{}^FO0,0^GFA,{total},{total},{bytes_per_row},{hex}^FS^XZ",
        bitmap.width, bitmap.height
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(rows: &[&str]) -> Bitmap {
        let width = rows[0].len();
        Bitmap {
            width,
            height: rows.len(),
            dots: rows
                .iter()
                .flat_map(|r| r.bytes().map(|c| u8::from(c == '#')))
                .collect(),
        }
    }

    #[test]
    fn dots_are_whole_bytes() {
        assert_eq!(dots_for_mm(72.0, 203.0), 576);
        assert_eq!(dots_for_mm(48.0, 203.0), 376);
        assert_eq!(dots_for_mm(1.0, 203.0), 8);
    }

    #[test]
    fn escpos_frames_the_bitmap() {
        let b = bitmap(&["#.......", "########", "........"]);
        let out = encode_escpos(&b, 3, true);
        assert_eq!(&out[..2], &[0x1B, 0x40], "ESC @");
        assert_eq!(&out[2..6], &[0x1D, 0x76, 0x30, 0x00], "GS v 0");
        assert_eq!(&out[6..10], &[1, 0, 3, 0], "1 byte wide, 3 rows");
        assert_eq!(&out[10..13], &[0b1000_0000, 0xFF, 0x00]);
        assert_eq!(&out[13..16], &[0x1B, 0x64, 3], "feed 3 lines");
        assert_eq!(&out[16..], &[0x1D, 0x56, 0x42, 0x00], "cut");
        // No cut, no feed.
        assert_eq!(encode_escpos(&b, 0, false).len(), 13);
    }

    #[test]
    fn escpos_splits_tall_bitmaps_into_bands() {
        let b = Bitmap {
            width: 8,
            height: 300,
            dots: vec![1; 8 * 300],
        };
        let out = encode_escpos(&b, 0, false);
        // 2 + three bands of (8-byte header + rows): 128, 128 and 44 rows of one byte.
        assert_eq!(out.len(), 2 + 3 * 8 + 300);
        assert_eq!(&out[6..10], &[1, 0, 128, 0]);
    }

    #[test]
    fn zpl_is_one_graphic_field() {
        let b = bitmap(&["#.......", "########"]);
        let zpl = String::from_utf8(encode_zpl(&b)).unwrap();
        assert_eq!(zpl, "^XA^PW8^LL2^FO0,0^GFA,2,2,1,80FF^FS^XZ");
    }

    #[test]
    fn blank_paper_below_the_content_is_trimmed() {
        let (width, height) = (8usize, 100usize);
        let mut grey = vec![255u8; width * height];
        grey[10 * width + 3] = 0; // one dot of ink on row 10
        assert_eq!(trim_blank_rows(&grey, width, height), 19);
        assert_eq!(
            trim_blank_rows(&vec![255u8; width * height], width, height),
            1,
            "an empty page keeps one row"
        );
    }

    #[test]
    fn threshold_and_dither() {
        assert_eq!(threshold(&[0, 100, 159, 160, 255]), vec![1, 1, 1, 0, 0]);
        let mut mid = vec![128u8; 16];
        let dithered = floyd_steinberg(&mut mid, 4, 4);
        let black = dithered.iter().filter(|&&d| d == 1).count();
        assert!(
            (4..=12).contains(&black),
            "mid grey becomes a mix of dots: {black}"
        );
    }
}
