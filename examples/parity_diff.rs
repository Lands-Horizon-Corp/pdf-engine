//! Compare two PDFs the way a person would: draw both with the same rasteriser and count how many
//! pixels still differ after forgiving a one-pixel shift (anti-aliasing, sub-pixel text positions).
//!
//! `parity_diff <reference.pdf> <candidate.pdf> <side-by-side.png>` prints one JSON object. Used by
//! `.public/parity/parity.mjs`; see `.public/docs/parity.md` for what the numbers mean.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;

const SCALE: f32 = 96.0 / 72.0;
/// A channel difference above this counts as "a different colour" (out of 255).
const TOLERANCE: i32 = 48;

struct Page {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

fn rasterize(path: &str) -> Result<Vec<Page>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let pdf = Pdf::new(bytes).map_err(|e| format!("{path}: cannot read the PDF: {e:?}"))?;
    let pages = hayro::render_pdf(&pdf, SCALE, InterpreterSettings::default(), None)
        .ok_or_else(|| format!("{path}: cannot rasterise"))?;
    Ok(pages
        .iter()
        .map(|p| Page {
            w: p.width() as usize,
            h: p.height() as usize,
            rgba: p.data_as_u8_slice().to_vec(),
        })
        .collect())
}

fn px(p: &Page, x: i64, y: i64) -> [i32; 3] {
    if x < 0 || y < 0 || x as usize >= p.w || y as usize >= p.h {
        return [255, 255, 255]; // outside the page is paper
    }
    let i = (y as usize * p.w + x as usize) * 4;
    [p.rgba[i] as i32, p.rgba[i + 1] as i32, p.rgba[i + 2] as i32]
}

fn close(a: [i32; 3], b: [i32; 3]) -> bool {
    (0..3).all(|c| (a[c] - b[c]).abs() <= TOLERANCE)
}

fn is_ink(c: [i32; 3]) -> bool {
    c.iter().any(|&v| v < 235)
}

/// Side of the squares the layout score compares (px at 96 dpi, about 6 mm).
const BLOCK: usize = 24;
/// Mean colours of two blocks that differ by more than this (per channel) are a different look.
const BLOCK_TOLERANCE: f64 = 22.0;

/// Compare the two pages block by block by mean colour: a few pixels of drift in a line of text still
/// averages to the same grey, but a missing image, a wrong background or text on the wrong side does not.
/// Returns (blocks with ink in either page, blocks that differ).
fn block_score(a: &Page, b: &Page, w: usize, h: usize) -> (u64, u64) {
    let (mut ink, mut wrong) = (0u64, 0u64);
    for by in (0..h).step_by(BLOCK) {
        for bx in (0..w).step_by(BLOCK) {
            let mut ma = [0f64; 3];
            let mut mb = [0f64; 3];
            let mut n = 0f64;
            for y in by..(by + BLOCK).min(h) {
                for x in bx..(bx + BLOCK).min(w) {
                    let (ca, cb) = (px(a, x as i64, y as i64), px(b, x as i64, y as i64));
                    for c in 0..3 {
                        ma[c] += ca[c] as f64;
                        mb[c] += cb[c] as f64;
                    }
                    n += 1.0;
                }
            }
            let has = |m: &[f64; 3]| m.iter().any(|v| v / n < 250.0);
            if !has(&ma) && !has(&mb) {
                continue;
            }
            ink += 1;
            if (0..3).any(|c| (ma[c] - mb[c]).abs() / n > BLOCK_TOLERANCE) {
                wrong += 1;
            }
        }
    }
    (ink, wrong)
}

/// True if `c` is matched by a pixel of `other` within one pixel of (x, y).
fn matched(c: [i32; 3], other: &Page, x: i64, y: i64) -> bool {
    (-1..=1).any(|dy| (-1..=1).any(|dx| close(c, px(other, x + dx, y + dy))))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: parity_diff <reference.pdf> <candidate.pdf> <out.png>");
        std::process::exit(2);
    }
    let (reference, candidate) = match (rasterize(&args[1]), rasterize(&args[2])) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            let why = a.err().or(b.err()).unwrap_or_default();
            println!("{}", serde_json::json!({ "error": why }));
            return;
        }
    };

    let paired = reference.len().min(candidate.len());
    let mut ink_union = 0u64;
    let mut wrong = 0u64;
    let mut total = 0u64;
    let (mut blocks, mut blocks_wrong) = (0u64, 0u64);
    let mut size_mismatch = false;
    let mut pages = Vec::new();
    let mut panels: Vec<(usize, usize, Vec<u8>)> = Vec::new();

    for i in 0..paired {
        let (a, b) = (&reference[i], &candidate[i]);
        if (a.w as i64 - b.w as i64).abs() > 2 || (a.h as i64 - b.h as i64).abs() > 2 {
            size_mismatch = true;
        }
        let (w, h) = (a.w.max(b.w), a.h.max(b.h));
        let mut page_wrong = 0u64;
        let mut page_ink = 0u64;
        // reference | candidate | difference (red = only in the reference, blue = only in the candidate)
        let mut sheet = vec![255u8; w * 3 * h * 4];
        for y in 0..h {
            for x in 0..w {
                let (ca, cb) = (px(a, x as i64, y as i64), px(b, x as i64, y as i64));
                let (ia, ib) = (is_ink(ca), is_ink(cb));
                let only_a = ia && !matched(ca, b, x as i64, y as i64);
                let only_b = ib && !matched(cb, a, x as i64, y as i64);
                if ia || ib {
                    page_ink += 1;
                }
                if only_a || only_b {
                    page_wrong += 1;
                }
                let mut write = |col: usize, c: [i32; 3]| {
                    let o = (y * w * 3 + col * w + x) * 4;
                    sheet[o..o + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
                };
                write(0, ca);
                write(1, cb);
                write(
                    2,
                    if only_a {
                        [230, 30, 30]
                    } else if only_b {
                        [30, 30, 230]
                    } else if ia || ib {
                        [200, 200, 200]
                    } else {
                        [255, 255, 255]
                    },
                );
            }
        }
        let (bi, bw) = block_score(a, b, w, h);
        blocks += bi;
        blocks_wrong += bw;
        total += (w * h) as u64;
        ink_union += page_ink;
        wrong += page_wrong;
        pages.push(serde_json::json!({
            "ink": page_ink,
            "wrong": page_wrong,
            "similarity": if page_ink == 0 { 100.0 } else { 100.0 * (1.0 - page_wrong as f64 / page_ink as f64) },
        }));
        // Keep the picture sharp: no downscaling, but cut the empty paper below the last ink of either page.
        let last_ink = (0..h)
            .rev()
            .find(|&y| {
                (0..w)
                    .any(|x| is_ink(px(a, x as i64, y as i64)) || is_ink(px(b, x as i64, y as i64)))
            })
            .map_or(0, |y| y + 1);
        let keep = (last_ink + 24).min(h).max(1);
        sheet.truncate(w * 3 * keep * 4);
        panels.push((w * 3, keep, sheet));
    }

    // Stack the pages into one image, scaled down so a long document stays a reasonable file.
    if let Some(width) = panels.iter().map(|p| p.0).max() {
        let height: usize = panels.iter().map(|p| p.1).sum();
        let mut image = vec![255u8; width * height * 4];
        let mut top = 0;
        for (w, h, data) in &panels {
            for y in 0..*h {
                image[((top + y) * width) * 4..((top + y) * width + w) * 4]
                    .copy_from_slice(&data[y * w * 4..(y + 1) * w * 4]);
            }
            top += h;
        }
        let (iw, ih, img) = (width, height, image);
        if let Ok(file) = std::fs::File::create(&args[3]) {
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), iw as u32, ih as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if let Ok(mut w) = enc.write_header() {
                let _ = w.write_image_data(&img);
            }
        }
    }

    println!(
        "{}",
        serde_json::json!({
            "pagesReference": reference.len(),
            "pagesCandidate": candidate.len(),
            "sizeMismatch": size_mismatch,
            "ink": ink_union,
            "wrong": wrong,
            "total": total,
            "layout": if blocks == 0 { 100.0 } else { 100.0 * (1.0 - blocks_wrong as f64 / blocks as f64) },
            "similarity": if ink_union == 0 { 100.0 } else { 100.0 * (1.0 - wrong as f64 / ink_union as f64) },
            "pages": pages,
        })
    );
}
