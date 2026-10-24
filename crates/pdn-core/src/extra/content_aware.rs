//! Ported from PhotoCraft crates/algo/src/content_aware.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: `Interrupt`/`Cancelled` from PC1's `extra::interrupt` (no progress, so `fill_with` reports none and its test checks the cancel alone); only `FillOptions` (colour adaptation and seed: the rotation, scale and mirror variants and their mosaic dropped, with `rotation_level`, `variant` and `sample`), `color_level`, `sampling_window` (moved here from `tool::spot_healing`, with engine/src/fill_cmds.rs's clip of the hole to the canvas), `fill_with`, `color_amount` and `level_transform`; `fill`, `fill_owned`, `auto_sampling` and the Sampling Brush helpers dropped (the tests' `fill` is the uncancelled `fill_with`); the kept tests are upstream's, minus those of the dropped items.
//!
//! Edit › Content-Aware Fill: patch-based completion ([`super::inpaint::complete_masked`]) with
//! Photoshop's sampling area and colour adaptation, as the Patch tool's Content-Aware mode uses it.
//!
//! Photoshop 25.4 was measured as a black box (synthetic and public-domain images, results read
//! back exactly; none of its code or assets were used):
//!
//! * **Sampling window** ([`sampling_window`]): the fill copies only from a square centred on the
//!   selection's bounding box, of side `4·√(max(w, 50)·max(h, 50))`: four times a square
//!   selection's side, 200 px at least, and for a long thin selection four times the geometric
//!   mean of its sides. At the canvas edge the square slides inwards rather than being cut. Pixels
//!   outside it never change the result (Edit › Fill and the workspace's Rectangular area alike).
//! * **The fill** copies pieces of the sampling area pixel for pixel (15–40 px across), joined by
//!   seams a few pixels wide: see [`super::inpaint`].
//! * **Colour adaptation** None / Default / High / Very High: with None the copies are exact; the
//!   others let each copied patch take a gain and a bias towards its surroundings, more at each
//!   level ([`color_level`]). The patch transform is that of S. Darabi et al., *Image Melding*,
//!   SIGGRAPH 2012.
//!
//! Buffers are interleaved `w × h × ch` normalised floats (any model/depth); deterministic.

use super::geom::Rect;
use super::inpaint::{Adapt, CompleteParams, Px, complete_masked};
use super::interrupt::{Cancelled, Interrupt};
use super::par;
use super::poisson::membrane_fill_with;

/// Options for [`fill_with`].
#[derive(Clone, Debug, PartialEq)]
pub struct FillOptions {
    /// Colour adaptation of the copies ([`color_level`]).
    pub color_adaptation: Adapt,
    pub seed: u64,
}

impl Default for FillOptions {
    fn default() -> Self {
        Self {
            color_adaptation: color_level("default"),
            seed: 1,
        }
    }
}

/// Colour adaptation for Photoshop's levels: none, default, high, veryHigh. Measured on a texture
/// under a brightness ramp: with None the copies are exact; Default shifts them by up to about
/// 10/255, High about 20, Very High about 35, scaling their contrast a little more at each level.
pub fn color_level(level: &str) -> Adapt {
    match level {
        "none" => Adapt::NONE,
        "high" => Adapt {
            gain: (0.8, 1.25),
            bias: 0.08,
        },
        "veryHigh" => Adapt {
            gain: (0.67, 1.5),
            bias: 0.14,
        },
        _ => Adapt {
            gain: (0.9, 1.1),
            bias: 0.04,
        },
    }
}

/// Photoshop's sampling window for a selection with bounding box `hole` (clipped to `canvas`):
/// the square of side `4·√(max(w, 50)·max(h, 50))` centred on it, slid inside `canvas` (and
/// clipped to it only where the canvas is smaller than the square). A selection longer than that
/// square (more than 16 times as long as it is wide, which was not measured) keeps its whole
/// length.
pub fn sampling_window(hole: Rect, canvas: Rect) -> Rect {
    let hole = hole.intersect(&canvas);
    if hole.is_empty() || canvas.is_empty() {
        return Rect::EMPTY;
    }
    let (w, h) = (
        f64::from(hole.width().max(50)),
        f64::from(hole.height().max(50)),
    );
    let side = (4.0 * (w * h).sqrt()).round().min(i32::MAX as f64) as i64;
    // Twice the centre, so that odd sizes stay exact.
    let span = |c2: i64, extent: u32, lo: i32, hi: i32| -> (i32, i32) {
        let (lo, hi) = (i64::from(lo), i64::from(hi));
        let side = side.max(i64::from(extent));
        if side >= hi - lo {
            return (lo as i32, hi as i32);
        }
        let a = ((c2 - side) / 2).clamp(lo, hi - side);
        (a as i32, (a + side) as i32)
    };
    let c = |a: i32, b: i32| i64::from(a) + i64::from(b);
    let (x0, x1) = span(c(hole.x0, hole.x1), hole.width(), canvas.x0, canvas.x1);
    let (y0, y1) = span(c(hole.y0, hole.y1), hole.height(), canvas.y0, canvas.y1);
    Rect::new(x0, y0, x1, y1)
}

/// Fill `hole` in `img`, copying only from pixels where `source` is true (and not in the hole).
/// Returns the full buffer (unchanged outside the hole). Falls back to the membrane fill when
/// nothing can be sampled. Can be cancelled (see [`super::inpaint::complete_with`]).
#[allow(clippy::too_many_arguments)]
pub fn fill_with(
    w: usize,
    h: usize,
    ch: usize,
    img: &[f32],
    hole: &[bool],
    source: &[bool],
    opts: &FillOptions,
    ctl: &Interrupt,
) -> Result<Vec<f32>, Cancelled> {
    assert_eq!(img.len(), w * h * ch);
    assert_eq!(hole.len(), w * h);
    assert_eq!(source.len(), w * h);
    if !hole.iter().any(|h| *h) {
        return Ok(img.to_vec());
    }
    let params = CompleteParams {
        seed: opts.seed,
        adapt: opts.color_adaptation,
        ..Default::default()
    };
    let roles = par::map(w * h, true, |s| {
        if hole[s] {
            Px::Hole
        } else if source[s] {
            Px::Source
        } else {
            Px::Known
        }
    });
    ctl.check()?;
    // The completion is nearly all the work.
    let out = match complete_masked(w, h, ch, img, &roles, &params, ctl)? {
        Some(f) => f,
        None => membrane_fill_with(w, h, ch, img, hole, ctl)?,
    };
    ctl.check()?;
    Ok(out)
}

/// The Patch Tool's and the Content-Aware Move Tool's Color option (0–10) as the colour adaptation
/// of the copied content. Measured on Photoshop 25.4 with content brighter than its new place: 0
/// keeps it as it is; 3 moves it about 0.12 (32 of 42 levels, and 31 of 102: a limit, not a
/// fraction), 6 about 80 % of 42 levels, 10 most of the way (35 of 42, 95 of 102). So the shift is
/// limited to about `0.04 · color`, the contrast change likewise widening.
pub fn color_amount(color: u8) -> Adapt {
    if color == 0 {
        return Adapt::NONE;
    }
    let c = f32::from(color.min(10));
    Adapt {
        gain: (1.0 - 0.033 * c, 1.0 + 0.05 * c),
        bias: 0.04 * c,
    }
}

/// Gain and bias per channel taking the samples `from` to the level and contrast of the samples
/// `to` (both interleaved, `ch` per sample), within `a`: how copied content is fitted to its new
/// surroundings.
pub fn level_transform(ch: usize, from: &[f32], to: &[f32], a: Adapt) -> Vec<(f32, f32)> {
    let moments = |v: &[f32], c: usize| {
        let (mut s, mut s2, mut n) = (0.0f64, 0.0f64, 0.0f64);
        for x in v.chunks_exact(ch.max(1)) {
            s += f64::from(x[c]);
            s2 += f64::from(x[c]) * f64::from(x[c]);
            n += 1.0;
        }
        (s, s2, n)
    };
    (0..ch)
        .map(|c| {
            let (fs, fs2, fnn) = moments(from, c);
            let (ts, ts2, tn) = moments(to, c);
            if a.is_none() || fnn == 0.0 || tn == 0.0 {
                return (1.0, 0.0);
            }
            // Both sets scaled to the same count for the fit.
            let k = fnn / tn;
            super::inpaint::fit(
                a,
                fnn as f32,
                (ts * k) as f32,
                (ts2 * k) as f32,
                fs as f32,
                fs2 as f32,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// Upstream's `fill`: [`fill_with`], never cancelled.
    fn fill(
        w: usize,
        h: usize,
        ch: usize,
        img: &[f32],
        hole: &[bool],
        source: &[bool],
        opts: &FillOptions,
    ) -> Vec<f32> {
        fill_with(w, h, ch, img, hole, source, opts, &Interrupt::NONE).unwrap()
    }

    /// Vertical stripes (period 6) in grey, with a hole in the middle.
    fn stripes(w: usize, h: usize) -> (Vec<f32>, Vec<bool>) {
        let img: Vec<f32> = (0..w * h)
            .map(|i| if (i % w) % 6 < 3 { 0.2 } else { 0.8 })
            .collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (i % w).abs_diff(w / 2) < 4 && (i / w).abs_diff(h / 2) < 4)
            .collect();
        (img, hole)
    }

    /// Upstream's `cancelled_fill_stops_and_progress_reaches_one`, without the progress (paint.rs
    /// reports none): a cancelled fill stops with `Cancelled`; an uncancelled one is the plain
    /// fill.
    #[test]
    fn cancelled_fill_stops_and_progress_reaches_one() {
        let (w, h) = (60, 40);
        let (img, hole) = stripes(w, h);
        let source = vec![true; w * h];
        let yes = AtomicBool::new(true);
        let cancelled = Interrupt(Some(&yes));
        let opts = FillOptions::default();
        assert!(fill_with(w, h, 1, &img, &hole, &source, &opts, &cancelled).is_err());
        let no = AtomicBool::new(false);
        let out = fill_with(w, h, 1, &img, &hole, &source, &opts, &Interrupt(Some(&no))).unwrap();
        assert_eq!(
            out,
            fill(w, h, 1, &img, &hole, &source, &opts),
            "same result as the plain fill"
        );
    }

    #[test]
    fn fills_only_the_hole_with_known_texture() {
        let (w, h) = (48, 32);
        let (img, hole) = stripes(w, h);
        let src = vec![true; w * h];
        let opts = FillOptions {
            color_adaptation: Adapt::NONE,
            ..Default::default()
        };
        let out = fill(w, h, 1, &img, &hole, &src, &opts);
        for i in 0..w * h {
            if !hole[i] {
                assert_eq!(out[i], img[i]);
            }
        }
        // The hole is filled with stripe values, not a flat average.
        let vals: Vec<f32> = (0..w * h).filter(|i| hole[*i]).map(|i| out[i]).collect();
        assert!(
            vals.iter().any(|v| *v < 0.35) && vals.iter().any(|v| *v > 0.65),
            "{vals:?}"
        );
    }

    #[test]
    fn sampling_area_restricts_sources() {
        // Left half dark, right half bright; sampling only the right half fills bright.
        let (w, h) = (40, 20);
        let img: Vec<f32> = (0..w * h)
            .map(|i| if i % w < 20 { 0.1 } else { 0.9 })
            .collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (16..24).contains(&(i % w)) && (6..14).contains(&(i / w)))
            .collect();
        let right: Vec<bool> = (0..w * h).map(|i| i % w >= 24).collect();
        let opts = FillOptions {
            color_adaptation: Adapt::NONE,
            ..Default::default()
        };
        let out = fill(w, h, 1, &img, &hole, &right, &opts);
        let mean: f32 = (0..w * h).filter(|i| hole[*i]).map(|i| out[i]).sum::<f32>() / 64.0;
        assert!(mean > 0.8, "mean {mean}");
    }

    #[test]
    fn color_adaptation_follows_a_gradient() {
        // A horizontal brightness ramp with a hole: adaptation keeps the fill near the ramp.
        let (w, h) = (40, 20);
        let img: Vec<f32> = (0..w * h).map(|i| (i % w) as f32 / w as f32).collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (14..26).contains(&(i % w)) && (5..15).contains(&(i / w)))
            .collect();
        let src = vec![true; w * h];
        let opts = FillOptions {
            color_adaptation: color_level("veryHigh"),
            ..Default::default()
        };
        let out = fill(w, h, 1, &img, &hole, &src, &opts);
        let err: f32 = (0..w * h)
            .filter(|i| hole[*i])
            .map(|i| (out[i] - img[i]).abs())
            .sum::<f32>()
            / 120.0;
        assert!(err < 0.12, "mean error {err}");
    }

    /// A vertical sky gradient (each row one colour) filled at every adaptation level stays the
    /// gradient: no tinted blotch where the copied rows' colours were shifted per channel.
    #[test]
    fn color_adaptation_leaves_a_smooth_gradient_untinted() {
        let (w, h) = (120, 60);
        let row = |y: usize| {
            let t = y as f32 / (h - 1) as f32;
            [0.27 + 0.45 * t, 0.53 + 0.32 * t, 0.86 + 0.1 * t]
        };
        let img: Vec<f32> = (0..w * h).flat_map(|i| row(i / w)).collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (50..70).contains(&(i % w)) && (20..40).contains(&(i / w)))
            .collect();
        let src = vec![true; w * h];
        for level in ["none", "default", "high", "veryHigh"] {
            let opts = FillOptions {
                color_adaptation: color_level(level),
                ..Default::default()
            };
            let out = fill(w, h, 3, &img, &hole, &src, &opts);
            let (out, img) = (&out, &img);
            let worst = (0..w * h)
                .filter(|i| hole[*i])
                .flat_map(|i| (0..3).map(move |c| (out[i * 3 + c] - img[i * 3 + c]).abs()))
                .fold(0.0f32, f32::max);
            assert!(worst < 3.0 / 255.0, "{level}: off by {}", worst * 255.0);
        }
    }

    /// I.i.d. noise: every value occurs once, so a filled pixel equal to an image pixel was copied.
    fn noise(w: usize, h: usize, seed: u64) -> Vec<f32> {
        let mut z = seed;
        (0..w * h * 3)
            .map(|_| {
                z = z
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                0.2 + 0.6 * ((z >> 40) as f32 / (1u64 << 24) as f32)
            })
            .collect()
    }

    /// Upstream's test, with `fill_cmds`'s clip of the hole to the canvas (moved here from
    /// `tool::spot_healing`, PC12b).
    #[test]
    fn sampling_window_is_photoshops() {
        let canvas = Rect::new(0, 0, 800, 800);
        let sq = |x0: i32, y0: i32, w: i32, h: i32| Rect::new(x0, y0, x0 + w, y0 + h);
        // Photoshop 25.4's sampling overlay: a square of side 4·√(max(w,50)·max(h,50)) centred on
        // the selection's bounds.
        assert_eq!(
            sampling_window(sq(375, 375, 50, 50), canvas),
            sq(300, 300, 200, 200)
        );
        assert_eq!(
            sampling_window(sq(350, 350, 100, 100), canvas),
            sq(200, 200, 400, 400)
        );
        assert_eq!(
            sampling_window(sq(395, 395, 10, 10), canvas),
            sq(300, 300, 200, 200)
        );
        assert_eq!(
            sampling_window(sq(370, 370, 60, 60), canvas),
            sq(280, 280, 240, 240)
        );
        // Long selections (Photoshop's overlay, read off the screen to a pixel: 281, 399, 691).
        assert_eq!(
            sampling_window(sq(350, 385, 100, 30), canvas),
            sq(258, 258, 283, 283)
        );
        assert_eq!(
            sampling_window(sq(300, 375, 200, 50), canvas),
            sq(200, 200, 400, 400)
        );
        assert_eq!(
            sampling_window(sq(250, 350, 300, 100), canvas),
            sq(53, 53, 693, 693)
        );
        // At the canvas edge the square slides inwards rather than being cut.
        assert_eq!(
            sampling_window(sq(10, 375, 50, 50), canvas),
            sq(0, 300, 200, 200)
        );
        assert_eq!(
            sampling_window(sq(5, 5, 60, 60), canvas),
            sq(0, 0, 240, 240)
        );
        // A canvas smaller than the square clips it; a very long selection keeps its length.
        let small = Rect::new(0, 0, 150, 120);
        assert_eq!(sampling_window(sq(40, 40, 50, 50), small), small);
        let tall = Rect::new(0, 0, 2000, 2000);
        let long = sq(100, 995, 1800, 10);
        assert_eq!(sampling_window(long, tall), sq(100, 400, 1800, 1200));
        assert_eq!(sampling_window(Rect::EMPTY, canvas), Rect::EMPTY);
    }

    #[test]
    fn without_colour_adaptation_the_fill_copies_pixels_exactly() {
        // A smooth texture with fine noise, so that every value is unique: a filled pixel equal to
        // an image pixel was copied from it. Photoshop's fill (colour adaptation None) is such
        // copies, joined by narrow seams.
        let (w, h) = (120, 120);
        let fine = noise(w, h, 9);
        let img: Vec<f32> = (0..w * h * 3)
            .map(|i| {
                let (x, y, c) = (((i / 3) % w) as f32, ((i / 3) / w) as f32, i % 3);
                0.5 + 0.2 * (x * 0.21 + c as f32).sin() * (y * 0.17).cos()
                    + 0.1 * (x * 0.05 - y * 0.08).sin()
                    + (fine[i] - 0.5) * 0.08
            })
            .collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (40..80).contains(&(i % w)) && (40..80).contains(&(i / w)))
            .collect();
        // Votes of identical copies average to the copy up to float rounding.
        let key = |v: &[f32], i: usize| [0, 1, 2].map(|c| (v[i * 3 + c] * 1e5).round() as i64);
        let known: std::collections::HashSet<[i64; 3]> = (0..w * h)
            .filter(|i| !hole[*i])
            .map(|i| key(&img, i))
            .collect();
        let opts = FillOptions {
            color_adaptation: Adapt::NONE,
            ..Default::default()
        };
        let out = fill(w, h, 3, &img, &hole, &vec![true; w * h], &opts);
        let exact = (0..w * h)
            .filter(|i| hole[*i] && known.contains(&key(&out, *i)))
            .count();
        assert!(
            exact as f32 > 0.2 * 1600.0,
            "{exact} of 1600 filled pixels are exact copies"
        );
        // And the fill is as busy as the noise around it, not a blur of it.
        let lap = |v: &[f32], x: usize, y: usize| {
            let p = |x: usize, y: usize| v[(y * w + x) * 3];
            (4.0 * p(x, y) - p(x - 1, y) - p(x + 1, y) - p(x, y - 1) - p(x, y + 1)).abs()
        };
        let mean = |inside: bool| {
            let pts: Vec<(usize, usize)> = (1..h - 1)
                .flat_map(|y| (1..w - 1).map(move |x| (x, y)))
                .filter(|&(x, y)| hole[y * w + x] == inside)
                .collect();
            pts.iter().map(|&(x, y)| lap(&out, x, y)).sum::<f32>() / pts.len() as f32
        };
        assert!(
            mean(true) > 0.6 * mean(false),
            "fill detail {} against {}",
            mean(true),
            mean(false)
        );
    }

    #[test]
    fn patch_color_shifts_up_to_about_four_hundredths_per_step() {
        // Content 0.4 brighter than where it lands (Photoshop's Patch, Color 3 and 10).
        let from: Vec<f32> = (0..300)
            .map(|i| 0.7 + 0.05 * ((i % 7) as f32 / 7.0))
            .collect();
        let to: Vec<f32> = from.iter().map(|v| v - 0.4).collect();
        let level = from.iter().sum::<f32>() / 300.0;
        let shift = |c: u8| {
            let gb = level_transform(1, &from, &to, color_amount(c));
            level - from.iter().map(|v| gb[0].0 * v + gb[0].1).sum::<f32>() / 300.0
        };
        assert_eq!(shift(0), 0.0);
        assert!((shift(3) - 0.12).abs() < 0.01, "{}", shift(3));
        assert!((shift(10) - 0.4).abs() < 0.01, "{}", shift(10));
    }
}
