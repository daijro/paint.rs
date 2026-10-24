//! Ported from PhotoCraft crates/algo/src/remove.rs and crates/engine/src/retouch_cmds/remove.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: `Interrupt`/`Cancelled` from `extra::interrupt` (the tile loop passes the caller's interrupt on, with no per-tile progress); `close_loops` works in place and `dilate` takes the mask first (the task's signatures); `window_for` and `MAX_PIXELS` come from the engine glue's `sampling_margin`/`MAX_MARGIN`/`MAX_PIXELS`; the tests for those are new; `remove_with` checks the interrupt after measuring the hole (PC5).
//! The Remove Tool: fill what a brush stroke painted over from its surroundings.
//!
//! * [`close_loops`]: a stroke drawn around an object also removes what it encloses (every pixel
//!   that can't reach the grid's border without crossing the stroke).
//! * [`dilate`]: the hole grows by a few pixels so an object's soft edge and halo go with it.
//! * [`remove_with`]: the hole is completed by [`complete_with`] (patch-based,
//!   with texture features so the fill keeps the grain of its surroundings, thin wires included).
//! * [`hole_radius`]: how thick a hole is, which sets how many scales the completion uses.
//!
//! Buffers are interleaved `w × h × ch` normalised floats (any model/depth) with `w × h` masks;
//! deterministic.

use super::geom::Rect;
use super::interrupt::{Cancelled, Interrupt};
use super::nonlocal::{Params, complete_with};
use super::poisson::membrane_fill_with;

/// Largest stroke, in pixels of its bounding box (upstream's pipeline cap, as Patch's).
pub const MAX_PIXELS: u64 = 16 << 20;

/// Cap on the sampling margin around the stroke (the window grows with the stroke).
const MAX_MARGIN: i32 = 512;

/// The window the fill samples from: `stroke` plus a margin of three quarters of its larger side
/// (at least 32 pixels, at most [`MAX_MARGIN`]), on `canvas`. Computed in u64 so a huge extent
/// can't overflow.
pub fn window_for(stroke: Rect, canvas: Rect) -> Rect {
    // Upstream measures the margin from the stroke already clipped to the canvas.
    let stroke = stroke.intersect(&canvas);
    if stroke.is_empty() {
        return Rect::EMPTY;
    }
    let ext = u64::from(stroke.width().max(stroke.height()));
    let margin = i32::try_from(ext * 3 / 4).unwrap_or(i32::MAX).max(32);
    stroke.inflate(margin.min(MAX_MARGIN)).intersect(&canvas)
}

/// Grows `mask` to include everything it encloses: the cells from which no path of unmasked
/// cells (4-connected) reaches the grid's border. A mask of the wrong length is left unchanged.
pub fn close_loops(mask: &mut [bool], w: usize, h: usize) {
    if w == 0 || h == 0 || mask.len() != w.saturating_mul(h) {
        return;
    }
    let mut outside = vec![false; mask.len()];
    let mut stack: Vec<usize> = Vec::new();
    let visit = |i: usize, outside: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if !mask[i] && !outside[i] {
            outside[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        visit(x, &mut outside, &mut stack);
        visit((h - 1) * w + x, &mut outside, &mut stack);
    }
    for y in 0..h {
        visit(y * w, &mut outside, &mut stack);
        visit(y * w + w - 1, &mut outside, &mut stack);
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        if x > 0 {
            visit(i - 1, &mut outside, &mut stack);
        }
        if x + 1 < w {
            visit(i + 1, &mut outside, &mut stack);
        }
        if y > 0 {
            visit(i - w, &mut outside, &mut stack);
        }
        if y + 1 < h {
            visit(i + w, &mut outside, &mut stack);
        }
    }
    for (m, o) in mask.iter_mut().zip(&outside) {
        *m = !*o;
    }
}

/// `mask` grown by `r` cells (a square structuring element), in two separable passes.
pub fn dilate(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    if r == 0 || mask.len() != w.saturating_mul(h) {
        return mask.to_vec();
    }
    // A pass sets each cell when any cell within `r` along one axis is set (a running count).
    let pass = |src: &[bool], horizontal: bool| -> Vec<bool> {
        let (lines, len) = if horizontal { (h, w) } else { (w, h) };
        let at = |line: usize, k: usize| {
            if horizontal {
                line * w + k
            } else {
                k * w + line
            }
        };
        let mut out = vec![false; src.len()];
        for line in 0..lines {
            let mut count = 0usize;
            for k in 0..len.min(r) {
                count += usize::from(src[at(line, k)]);
            }
            for k in 0..len {
                if k + r < len {
                    count += usize::from(src[at(line, k + r)]);
                }
                if k > r {
                    count -= usize::from(src[at(line, k - r - 1)]);
                }
                out[at(line, k)] = count > 0;
            }
        }
        out
    };
    pass(&pass(mask, true), false)
}

/// The largest chessboard distance from a hole cell to the nearest cell outside the hole (cells
/// beyond the grid don't count as outside: a hole at the canvas edge is not thinner for it).
/// `u32::MAX` when no cell lies outside the hole, 0 for a mask of the wrong size.
pub fn hole_radius(w: usize, h: usize, hole: &[bool]) -> u32 {
    if hole.len() != w.saturating_mul(h) {
        return 0;
    }
    let mut d: Vec<u32> = hole.iter().map(|m| if *m { u32::MAX } else { 0 }).collect();
    let relax = |d: &mut Vec<u32>, i: usize, j: usize| {
        let v = d[j].saturating_add(1);
        if v < d[i] {
            d[i] = v;
        }
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if x > 0 {
                relax(&mut d, i, i - 1);
            }
            if y > 0 {
                relax(&mut d, i, i - w);
                if x > 0 {
                    relax(&mut d, i, i - w - 1);
                }
                if x + 1 < w {
                    relax(&mut d, i, i - w + 1);
                }
            }
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if x + 1 < w {
                relax(&mut d, i, i + 1);
            }
            if y + 1 < h {
                relax(&mut d, i, i + w);
                if x + 1 < w {
                    relax(&mut d, i, i + w + 1);
                }
                if x > 0 {
                    relax(&mut d, i, i + w - 1);
                }
            }
        }
    }
    d.iter()
        .zip(hole)
        .filter(|(_, m)| **m)
        .map(|(v, _)| *v)
        .max()
        .unwrap_or(0)
}

/// Holes covering less than this share of their bounding box (a long stroke, scattered marks) are
/// completed tile by tile, so the work follows the hole rather than its bounding box.
const SPARSE: f64 = 0.25;
/// Smallest tile side, in pixels.
const MIN_TILE: usize = 256;

/// Fill `hole` in `img` from the pixels around it. Returns the full buffer (unchanged outside the
/// hole); a buffer or mask of the wrong size comes back unchanged. When nothing around the hole is
/// fully known (a hole about as big as `img`), it is diffused from its edges instead.
///
/// A sparse hole is filled in tiles eight times its thickness (at least [`MIN_TILE`] pixels), row
/// by row, each with a margin of six times its thickness (at least 48 pixels): each tile sees the
/// ones before it as filled, so the fill runs on across tile edges.
pub fn remove_with(
    w: usize,
    h: usize,
    ch: usize,
    img: &[f32],
    hole: &[bool],
    seed: u64,
    ctl: &Interrupt,
) -> Result<Vec<f32>, Cancelled> {
    let n = w.saturating_mul(h);
    if ch == 0 || hole.len() != n || img.len() != n.saturating_mul(ch) {
        return Ok(img.to_vec());
    }
    let (mut bx0, mut by0, mut bx1, mut by1, mut count) = (w, h, 0, 0, 0usize);
    for (i, _) in hole.iter().enumerate().filter(|(_, m)| **m) {
        let (x, y) = (i % w, i / w);
        (bx0, by0, bx1, by1, count) = (
            bx0.min(x),
            by0.min(y),
            bx1.max(x + 1),
            by1.max(y + 1),
            count + 1,
        );
    }
    if count == 0 {
        return Ok(img.to_vec());
    }
    let thickness = hole_radius(w, h, hole).min(1 << 20) as usize;
    ctl.check()?;
    let tile = (8 * thickness).max(MIN_TILE);
    let area = (bx1 - bx0) * (by1 - by0);
    if count as f64 >= SPARSE * area as f64 || (bx1 - bx0).max(by1 - by0) <= 2 * tile {
        return fill(w, h, ch, img, hole, seed, ctl);
    }
    let margin = (6 * thickness).max(48);
    let mut out = img.to_vec();
    let mut left = hole.to_vec();
    let origins: Vec<(usize, usize)> = (by0..by1)
        .step_by(tile)
        .flat_map(|y| (bx0..bx1).step_by(tile).map(move |x| (x, y)))
        .collect();
    for (k, &(tx0, ty0)) in origins.iter().enumerate() {
        ctl.check()?;
        let (tx1, ty1) = ((tx0 + tile).min(bx1), (ty0 + tile).min(by1));
        if !(ty0..ty1).any(|y| left[y * w + tx0..y * w + tx1].iter().any(|m| *m)) {
            continue;
        }
        let (x0, y0, x1, y1) = (
            tx0.saturating_sub(margin),
            ty0.saturating_sub(margin),
            (tx1 + margin).min(w),
            (ty1 + margin).min(h),
        );
        let ww = x1 - x0;
        let sub_img: Vec<f32> = (y0..y1)
            .flat_map(|y| out[(y * w + x0) * ch..(y * w + x1) * ch].iter().copied())
            .collect();
        let sub_hole: Vec<bool> = (y0..y1)
            .flat_map(|y| left[y * w + x0..y * w + x1].iter().copied())
            .collect();
        let filled = fill(
            ww,
            y1 - y0,
            ch,
            &sub_img,
            &sub_hole,
            seed.wrapping_add(k as u64),
            ctl,
        )?;
        for y in ty0..ty1 {
            for x in tx0..tx1 {
                let i = y * w + x;
                if left[i] {
                    let j = (y - y0) * ww + (x - x0);
                    out[i * ch..(i + 1) * ch].copy_from_slice(&filled[j * ch..(j + 1) * ch]);
                    left[i] = false;
                }
            }
        }
    }
    Ok(out)
}

/// Complete one hole, or diffuse it when there is no fully-known patch to copy from.
fn fill(
    w: usize,
    h: usize,
    ch: usize,
    img: &[f32],
    hole: &[bool],
    seed: u64,
    ctl: &Interrupt,
) -> Result<Vec<f32>, Cancelled> {
    match complete_with(
        w,
        h,
        ch,
        img,
        hole,
        &Params {
            seed,
            ..Params::default()
        },
        ctl,
    )? {
        Some(out) => Ok(out),
        None => membrane_fill_with(w, h, ch, img, hole, ctl),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: usize, h: usize, f: impl Fn(usize, usize) -> bool) -> Vec<bool> {
        (0..w * h).map(|i| f(i % w, i / w)).collect()
    }

    #[test]
    fn a_ring_closes_into_a_disc_and_an_open_line_does_not() {
        let (w, h) = (40, 40);
        let ring = grid(w, h, |x, y| {
            let d = (x as f32 - 20.0).hypot(y as f32 - 20.0);
            (10.0..12.0).contains(&d)
        });
        let mut closed = ring.clone();
        close_loops(&mut closed, w, h);
        assert!(closed[20 * w + 20], "the centre is enclosed");
        assert!(!closed[2 * w + 2], "the outside stays outside");
        let line = grid(w, h, |x, y| y == 20 && (5..35).contains(&x));
        let mut open = line.clone();
        close_loops(&mut open, w, h);
        assert_eq!(open, line, "an open stroke encloses nothing");
    }

    #[test]
    fn a_loop_open_at_one_pixel_encloses_nothing() {
        let (w, h) = (20, 20);
        let mut sq = grid(w, h, |x, y| {
            (x == 5 || x == 14 || y == 5 || y == 14)
                && (5..=14).contains(&x)
                && (5..=14).contains(&y)
        });
        sq[5 * w + 9] = false;
        close_loops(&mut sq, w, h);
        assert!(!sq[10 * w + 10]);
    }

    #[test]
    fn dilate_grows_by_the_radius() {
        let (w, h) = (11, 11);
        let dot = grid(w, h, |x, y| (x, y) == (5, 5));
        let d = dilate(&dot, w, h, 2);
        assert_eq!(d.iter().filter(|m| **m).count(), 25);
        assert!(d[3 * w + 3] && d[7 * w + 7] && !d[2 * w + 5]);
        assert_eq!(dilate(&dot, w, h, 0), dot);
    }

    #[test]
    fn radius_measures_the_thickest_part() {
        let (w, h) = (30, 30);
        let line = grid(w, h, |x, y| y == 10 && (5..25).contains(&x));
        assert_eq!(hole_radius(w, h, &line), 1);
        let block = grid(w, h, |x, y| (5..16).contains(&x) && (5..16).contains(&y));
        assert_eq!(hole_radius(w, h, &block), 6);
        assert_eq!(hole_radius(w, h, &vec![true; w * h]), u32::MAX);
        assert_eq!(hole_radius(w, h, &[true]), 0, "a mask of the wrong size");
    }

    /// A hole in a horizontal gradient is filled close to the gradient, both thin and wide.
    #[test]
    fn removes_thin_and_wide_holes_from_a_gradient() {
        let (w, h, ch) = (64, 48, 2);
        let truth: Vec<f32> = (0..w * h)
            .flat_map(|i| [(i % w) as f32 / w as f32, 1.0])
            .collect();
        for hole in [
            grid(w, h, |x, y| y == 24 && (10..54).contains(&x)),
            grid(w, h, |x, y| (24..40).contains(&x) && (16..32).contains(&y)),
        ] {
            let mut img = truth.clone();
            for (i, m) in hole.iter().enumerate() {
                if *m {
                    img[i * ch] = 1.0;
                }
            }
            let out = remove_with(w, h, ch, &img, &hole, 7, &Interrupt::NONE).unwrap();
            for (i, m) in hole.iter().enumerate() {
                if *m {
                    assert!(
                        (out[i * ch] - truth[i * ch]).abs() < 0.12,
                        "pixel {i}: {} vs {}",
                        out[i * ch],
                        truth[i * ch]
                    );
                } else {
                    assert_eq!(
                        out[i * ch..(i + 1) * ch],
                        img[i * ch..(i + 1) * ch],
                        "outside the hole is untouched"
                    );
                }
            }
        }
    }

    /// A long, thin diagonal hole goes tile by tile; every cell is filled, the rest is untouched.
    #[test]
    fn a_long_thin_hole_is_filled_in_tiles() {
        let (w, h, ch) = (900, 700, 1);
        let img: Vec<f32> = (0..w * h)
            .map(|i| 0.3 + 0.4 * (((i % w) / 7 + (i / w) / 5) % 2) as f32)
            .collect();
        let hole = grid(w, h, |x, y| {
            (x as f32 - y as f32 * 1.2 - 20.0).abs() < 3.0 && (40..680).contains(&y)
        });
        let mut holed = img.clone();
        hole.iter()
            .enumerate()
            .filter(|(_, m)| **m)
            .for_each(|(i, _)| holed[i] = 1.0);
        let out = remove_with(w, h, ch, &holed, &hole, 3, &Interrupt::NONE).unwrap();
        for (i, m) in hole.iter().enumerate() {
            if *m {
                assert!(
                    (0.29..=0.71).contains(&out[i]),
                    "cell {i} not filled from the stripes: {}",
                    out[i]
                );
            } else {
                assert_eq!(out[i], holed[i]);
            }
        }
    }

    #[test]
    fn close_loops_fills_an_encircled_area() {
        let (w, h) = (30, 30);
        let mut m = grid(w, h, |x, y| {
            (x == 8 || x == 20 || y == 8 || y == 20)
                && (8..=20).contains(&x)
                && (8..=20).contains(&y)
        });
        close_loops(&mut m, w, h);
        assert!(m[14 * w + 14] && m[9 * w + 9] && !m[3 * w + 3]);
        assert_eq!(m.iter().filter(|v| **v).count(), 13 * 13);
    }

    #[test]
    fn window_is_the_stroke_plus_a_clamped_margin_on_the_canvas() {
        let canvas = Rect::new(0, 0, 4000, 3000);
        // 100 px stroke: margin 75.
        assert_eq!(
            window_for(Rect::new(1000, 1000, 1100, 1050), canvas),
            Rect::new(925, 925, 1175, 1125)
        );
        // Tiny stroke: at least 32. Huge stroke: at most MAX_MARGIN. Clipped at the canvas.
        assert_eq!(
            window_for(Rect::new(1000, 1000, 1004, 1004), canvas),
            Rect::new(968, 968, 1036, 1036)
        );
        assert_eq!(
            window_for(Rect::new(1000, 1000, 3000, 2000), canvas),
            Rect::new(488, 488, 3512, 2512)
        );
        assert_eq!(
            window_for(Rect::new(0, 0, 10, 10), canvas),
            Rect::new(0, 0, 42, 42)
        );
        // A stroke half off the canvas: the margin follows the on-canvas 40 px part (margin 32),
        // not the raw 400 px one.
        assert_eq!(
            window_for(Rect::new(1000, -360, 1040, 40), canvas),
            Rect::new(968, 0, 1072, 72)
        );
        assert_eq!(
            window_for(Rect::new(-500, -500, -400, -400), canvas),
            Rect::EMPTY
        );
        assert_eq!(MAX_PIXELS, 16 * 1024 * 1024);
    }

    #[test]
    fn remove_fills_a_hole_in_a_flat_image_with_its_colour() {
        let (w, h, ch) = (64, 64, 4);
        let px = [0.2f32, 0.4, 0.6, 1.0];
        let img: Vec<f32> = (0..w * h).flat_map(|_| px).collect();
        let hole = grid(w, h, |x, y| (27..37).contains(&x) && (27..37).contains(&y));
        let mut holed = img.clone();
        for (i, _) in hole.iter().enumerate().filter(|(_, m)| **m) {
            holed[i * ch..(i + 1) * ch].copy_from_slice(&[1.0, 0.0, 0.0, 1.0]);
        }
        let out = remove_with(w, h, ch, &holed, &hole, 1, &Interrupt::NONE).unwrap();
        for (o, t) in out.iter().zip(&img) {
            assert!((o - t).abs() <= 1.0 / 255.0, "{o} vs {t}");
        }
    }

    /// Same seed, same bytes, whatever the rayon pool size.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn remove_is_deterministic_and_thread_independent() {
        let (w, h, ch) = (160, 140, 3);
        let img: Vec<f32> = (0..w * h * ch)
            .map(|i| ((i as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32)
            .collect();
        let hole = grid(w, h, |x, y| (50..100).contains(&x) && (40..95).contains(&y));
        let run = |n| {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(n)
                .build()
                .unwrap();
            pool.install(|| remove_with(w, h, ch, &img, &hole, 9, &Interrupt::NONE).unwrap())
        };
        let (one, four) = (run(1), run(4));
        assert_eq!(one, four);
        assert_eq!(one, run(1));
    }

    #[test]
    fn remove_cancel_returns_cancelled() {
        let (w, h) = (64, 64);
        let img = vec![0.5f32; w * h];
        let hole = grid(w, h, |x, y| (20..40).contains(&x) && (20..40).contains(&y));
        let flag = std::sync::atomic::AtomicBool::new(true);
        assert_eq!(
            remove_with(w, h, 1, &img, &hole, 1, &Interrupt(Some(&flag))),
            Err(Cancelled)
        );
    }

    #[test]
    fn hostile_sizes_return_the_input() {
        let img = vec![0.5f32; 12];
        assert_eq!(
            remove_with(2, 2, 3, &img, &[true], 1, &Interrupt::NONE).unwrap(),
            img
        );
        assert_eq!(
            remove_with(2, 2, 0, &img, &[true; 4], 1, &Interrupt::NONE).unwrap(),
            img
        );
        assert_eq!(
            remove_with(3, 2, 2, &img, &[false; 6], 1, &Interrupt::NONE).unwrap(),
            img
        );
        close_loops(&mut [], 0, 0);
        assert_eq!(dilate(&[true], 3, 3, 1), vec![true]);
    }
}
