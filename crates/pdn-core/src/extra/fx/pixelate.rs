//! Ported from PhotoCraft crates/algo/src/pixelate.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only Color Halftone, Facet, Mezzotint and Pointillize (Crystallize and Fragment dropped); kernels take a whole interleaved straight RGBA f32 layer (`w`, `h`) and a `bounds` rectangle instead of `Image`/`Ctx`/`ColorMode`, so the CMYK, Lab and channel-count branches are gone; the grid and screen origin is `bounds`' corner as upstream's `ctx.bounds`, and the layer comes back with only `bounds` (clipped to it) replaced; reads past the layer repeat its edge pixels; Facet is the direct Kuwahara (upstream's `facet::run` reuse path is dropped) and Color Halftone memoizes each cell's radius in a read-only grid built up front (upstream's `screen::Screen` cache is lazy and `&mut`, which cannot be shared by parallel rows); rows run through `extra::par` and `Interrupt` is polled per row; `hash01` is `extra::fx::util`'s (upstream noise.rs) and `cell_point` is copied from fxutil.rs; upstream's tests compare the dropped caches with its scalar reference, so the tests here are new properties.

use super::util::hash01;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par::rows_mut;

const N: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MezzotintType {
    FineDots,
    MediumDots,
    GrainyDots,
    CoarseDots,
    ShortLines,
    MediumLines,
    LongLines,
    ShortStrokes,
    MediumStrokes,
    LongStrokes,
}

/// Jittered feature point of grid cell `(cx, cy)` (cells of `cell` px anchored at `origin`).
#[inline]
fn cell_point(cx: i32, cy: i32, cell: f32, seed: u32, origin: (f32, f32)) -> (f32, f32) {
    (
        origin.0 + (cx as f32 + 0.1 + 0.8 * hash01(cx, cy, 11, seed)) * cell,
        origin.1 + (cy as f32 + 0.1 + 0.8 * hash01(cx, cy, 12, seed)) * cell,
    )
}

/// The layer with its edge pixels replicated outward.
struct Src<'a> {
    buf: &'a [f32],
    w: i32,
    h: i32,
}

impl Src<'_> {
    fn get(&self, x: i32, y: i32, c: usize) -> f32 {
        let (x, y) = (x.clamp(0, self.w - 1), y.clamp(0, self.h - 1));
        self.buf[(y as usize * self.w as usize + x as usize) * N + c]
    }
}

/// `f(x, y, px)` over every pixel of `out` (a rectangle inside the layer), a parallel row at a
/// time; the whole layer back with those pixels replaced, `None` once cancelled.
fn per_pixel(
    buf: &[f32],
    w: usize,
    out: Rect,
    ctl: &Interrupt,
    f: impl Fn(i32, i32, &mut [f32]) + Sync + Send,
) -> Option<Vec<f32>> {
    let mut res = buf.to_vec();
    rows_mut(&mut res, (w * N).max(1), |y, row| {
        let y = y as i32;
        if y < out.y0 || y >= out.y1 || ctl.cancelled() {
            return;
        }
        for x in out.x0..out.x1 {
            f(x, y, &mut row[x as usize * N..(x as usize + 1) * N]);
        }
    });
    (!ctl.cancelled()).then_some(res)
}

/// `bounds` clipped to the layer.
fn clip(bounds: Rect, w: usize, h: usize) -> Rect {
    bounds.intersect(&Rect::new(0, 0, w as i32, h as i32))
}

/// Radii of one screen's dots, by cell, over the cells the output can touch.
struct Grid {
    x0: i32,
    y0: i32,
    width: usize,
    height: usize,
    radii: Vec<f32>,
}

/// Colour Halftone: each of R, G and B is screened at its own angle into dots whose area follows
/// the channel's ink (1 − value); alpha is untouched.
#[allow(clippy::too_many_arguments)]
pub fn color_halftone(
    buf: &[f32],
    w: usize,
    h: usize,
    bounds: Rect,
    max_radius: f32,
    angles: [f32; 3],
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    halftone_with_cap(buf, w, h, bounds, max_radius, angles, ctl, GRID_CAP)
}

/// Most cells of one channel's radius grid; past it every radius is computed directly.
const GRID_CAP: usize = 4 * 1024 * 1024;

#[allow(clippy::too_many_arguments)]
fn halftone_with_cap(
    buf: &[f32],
    w: usize,
    h: usize,
    bounds: Rect,
    max_radius: f32,
    angles: [f32; 3],
    ctl: &Interrupt,
    grid_cap: usize,
) -> Option<Vec<f32>> {
    let out = clip(bounds, w, h);
    if out.is_empty() {
        return Some(buf.to_vec());
    }
    let src = Src {
        buf,
        w: w as i32,
        h: h as i32,
    };
    let rmax = max_radius.max(1.0);
    // Cell side such that a full-size dot just covers the cell's corners.
    let cell = rmax * std::f32::consts::SQRT_2;
    let (ox, oy) = (bounds.x0 as f32, bounds.y0 as f32);
    let rot: [(f32, f32); 3] = std::array::from_fn(|c| angles[c].to_radians().sin_cos());
    // Channel value at a point averaged over five taps, clamped to the bounds so cells straddling
    // the edge read real pixels.
    let value = |x: f32, y: f32, c: usize| -> f32 {
        let d = cell * 0.25;
        let mut acc = 0.0;
        for (dx, dy) in [(0.0, 0.0), (-d, -d), (d, -d), (-d, d), (d, d)] {
            let ix = ((x + dx).floor() as i32).clamp(out.x0, out.x1 - 1);
            let iy = ((y + dy).floor() as i32).clamp(out.y0, out.y1 - 1);
            acc += src.get(ix, iy, c);
        }
        acc / 5.0
    };
    // The dot radius of screen cell `(i, j)`; it depends only on the cell, not the pixel.
    let radius = |c: usize, i: i32, j: i32| -> f32 {
        let (s, co) = rot[c];
        let (mu, mv) = ((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
        let (dx, dy) = (mu * co - mv * s + ox, mu * s + mv * co + oy);
        let ink = 1.0 - value(dx, dy, c).clamp(0.0, 1.0);
        let touch = std::f32::consts::FRAC_PI_4;
        if ink <= touch {
            cell * (ink / std::f32::consts::PI).sqrt()
        } else {
            cell * (0.5 + (ink - touch) / (1.0 - touch) * (std::f32::consts::FRAC_1_SQRT_2 - 0.5))
        }
    };
    // ponytail: a cell grid that would pass 4 M cells per channel (a huge selection with a tiny
    // radius) falls back to computing every radius directly; raise the cap if that is too slow.
    let grids: [Option<Grid>; 3] = std::array::from_fn(|c| {
        let (s, co) = rot[c];
        let (mut lo, mut hi) = (
            (f32::INFINITY, f32::INFINITY),
            (f32::NEG_INFINITY, f32::NEG_INFINITY),
        );
        for y in [out.y0, out.y1 - 1] {
            for x in [out.x0, out.x1 - 1] {
                let (fx, fy) = (x as f32 + 0.5 - ox, y as f32 + 0.5 - oy);
                let (u, v) = ((fx * co + fy * s) / cell, (-fx * s + fy * co) / cell);
                lo = (lo.0.min(u), lo.1.min(v));
                hi = (hi.0.max(u), hi.1.max(v));
            }
        }
        if !(lo.0.is_finite() && lo.1.is_finite() && hi.0.is_finite() && hi.1.is_finite()) {
            return None;
        }
        let (x0, y0) = (lo.0.floor() as i32 - 2, lo.1.floor() as i32 - 2);
        let width = usize::try_from(hi.0.floor() as i32 + 3 - x0).ok()?;
        let height = usize::try_from(hi.1.floor() as i32 + 3 - y0).ok()?;
        if width.checked_mul(height)? > grid_cap {
            return None;
        }
        let mut radii = vec![0.0f32; width * height];
        rows_mut(&mut radii, width, |j, row| {
            if ctl.cancelled() {
                return;
            }
            for (i, r) in row.iter_mut().enumerate() {
                *r = radius(c, x0 + i as i32, y0 + j as i32);
            }
        });
        Some(Grid {
            x0,
            y0,
            width,
            height,
            radii,
        })
    });
    if ctl.cancelled() {
        return None;
    }
    let cell_radius = |c: usize, i: i32, j: i32| -> f32 {
        grids[c]
            .as_ref()
            .and_then(|g| {
                let (gi, gj) = (
                    usize::try_from(i - g.x0).ok()?,
                    usize::try_from(j - g.y0).ok()?,
                );
                (gi < g.width && gj < g.height).then(|| g.radii[gj * g.width + gi])
            })
            .unwrap_or_else(|| radius(c, i, j))
    };
    per_pixel(buf, w, out, ctl, |x, y, px| {
        let (fx, fy) = (x as f32 + 0.5 - ox, y as f32 + 0.5 - oy);
        for c in 0..3 {
            let (s, co) = rot[c];
            // Rotate into screen space.
            let (u, v) = (fx * co + fy * s, -fx * s + fy * co);
            let (cu, cv) = ((u / cell).floor() as i32, (v / cell).floor() as i32);
            let mut cov: f32 = 0.0;
            for dj in -1..=1 {
                for di in -1..=1 {
                    let (i, j) = (cu + di, cv + dj);
                    let (mu, mv) = ((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
                    let d = ((u - mu).powi(2) + (v - mv).powi(2)).sqrt();
                    if d >= rmax + 0.5 {
                        continue;
                    }
                    let r = cell_radius(c, i, j);
                    // Tiny dots cannot cover more than their own area.
                    cov = cov.max(
                        (r - d + 0.5)
                            .clamp(0.0, 1.0)
                            .min(std::f32::consts::PI * r * r),
                    );
                }
            }
            px[c] = 1.0 - cov;
        }
    })
}

/// Facet: a Kuwahara filter (radius 2): each pixel takes the mean of the least-varied of its four
/// overlapping quadrants, clumping similar colours into flat facets while keeping edges.
pub fn facet(buf: &[f32], w: usize, h: usize, bounds: Rect, ctl: &Interrupt) -> Option<Vec<f32>> {
    let out = clip(bounds, w, h);
    if out.is_empty() {
        return Some(buf.to_vec());
    }
    let src = Src {
        buf,
        w: w as i32,
        h: h as i32,
    };
    let r = 2i32;
    per_pixel(buf, w, out, ctl, |x, y, px| {
        let mut best = (f32::MAX, [0.0f32; N]);
        for (qx, qy) in [(-r, -r), (0, -r), (-r, 0), (0, 0)] {
            let mut sum = [0.0f32; N];
            let (mut s, mut s2) = (0.0f32, 0.0f32);
            for yy in y + qy..=y + qy + r {
                for xx in x + qx..=x + qx + r {
                    let mut t = [0.0f32; N];
                    for (c, v) in t.iter_mut().enumerate() {
                        *v = src.get(xx, yy, c);
                        sum[c] += *v;
                    }
                    let l = 0.299 * t[0] + 0.587 * t[1] + 0.114 * t[2];
                    s += l;
                    s2 += l * l;
                }
            }
            let cnt = ((r + 1) * (r + 1)) as f32;
            let var = s2 / cnt - (s / cnt).powi(2);
            if var < best.0 {
                sum.iter_mut().for_each(|v| *v /= cnt);
                best = (var, sum);
            }
        }
        px.copy_from_slice(&best.1);
    })
}

/// Random threshold for Mezzotint pattern `kind` at a pixel.
fn mezzo_threshold(kind: MezzotintType, x: i32, y: i32, c: u32, seed: u32) -> f32 {
    let lines = |len: i32| {
        let phase = (hash01(0, y, c + 40, seed) * len as f32) as i32;
        hash01((x + phase).div_euclid(len), y, c, seed)
    };
    let strokes = |len: i32| {
        let d = x - y;
        let phase = (hash01(d, 0, c + 50, seed) * len as f32) as i32;
        hash01((x + y + phase).div_euclid(len), d, c + 60, seed)
    };
    match kind {
        MezzotintType::FineDots => hash01(x, y, c, seed),
        MezzotintType::MediumDots => hash01(x.div_euclid(2), y.div_euclid(2), c, seed),
        MezzotintType::GrainyDots => {
            0.5 * hash01(x, y, c, seed)
                + 0.5 * hash01(x.div_euclid(3), y.div_euclid(3), c + 7, seed)
        }
        MezzotintType::CoarseDots => hash01(x.div_euclid(3), y.div_euclid(3), c, seed),
        MezzotintType::ShortLines => lines(6),
        MezzotintType::MediumLines => lines(14),
        MezzotintType::LongLines => lines(32),
        MezzotintType::ShortStrokes => strokes(5),
        MezzotintType::MediumStrokes => strokes(10),
        MezzotintType::LongStrokes => strokes(20),
    }
}

/// Mezzotint: every colour channel is thresholded against a random pattern, giving pure channel
/// values (fully saturated dots/lines in colour images). Alpha is untouched.
pub fn mezzotint(
    buf: &[f32],
    w: usize,
    h: usize,
    bounds: Rect,
    kind: MezzotintType,
    seed: u32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let out = clip(bounds, w, h);
    per_pixel(buf, w, out, ctl, |x, y, px| {
        for (k, v) in px.iter_mut().enumerate().take(3) {
            *v = if *v > mezzo_threshold(kind, x, y, k as u32, seed) {
                1.0
            } else {
                0.0
            };
        }
    })
}

/// Pointillize: randomly sized dots, coloured from the layer at their centre, on a canvas of the
/// `background` colour (straight RGBA in 0-1).
#[allow(clippy::too_many_arguments)]
pub fn pointillize(
    buf: &[f32],
    w: usize,
    h: usize,
    bounds: Rect,
    cell_size: f32,
    seed: u32,
    background: [f32; 4],
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let out = clip(bounds, w, h);
    let src = Src {
        buf,
        w: w as i32,
        h: h as i32,
    };
    let cell = cell_size.max(2.0);
    let origin = (bounds.x0 as f32, bounds.y0 as f32);
    per_pixel(buf, w, out, ctl, |x, y, px| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (cx, cy) = (
            ((fx - origin.0) / cell).floor() as i32,
            ((fy - origin.1) / cell).floor() as i32,
        );
        // The dot whose edge the pixel is deepest inside wins.
        let mut best: Option<(f32, i32, i32, f32, f32)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (gx, gy) = (cx + dx, cy + dy);
                let (px0, py0) = cell_point(gx, gy, cell, seed, origin);
                let rad = cell * (0.45 + 0.35 * hash01(gx, gy, 13, seed));
                let depth = rad - ((px0 - fx).powi(2) + (py0 - fy).powi(2)).sqrt();
                if depth > -0.5 && best.is_none_or(|b| depth > b.0) {
                    best = Some((depth, gx, gy, px0, py0));
                }
            }
        }
        // Canvas first, then the dot over it with an anti-aliased edge.
        let mut canvas = background;
        canvas[3] = px[3].max(background[3].min(1.0));
        if let Some((depth, gx, gy, px0, py0)) = best {
            let (sx, sy) = (px0.floor() as i32, py0.floor() as i32);
            let mut dot: [f32; N] = std::array::from_fn(|c| src.get(sx, sy, c));
            // Slight per-dot brightness jitter, as in pointillist strokes.
            let j = (hash01(gx, gy, 17, seed) - 0.5) * 0.08;
            for d in dot.iter_mut().take(3) {
                *d = (*d + j).clamp(0.0, 1.0);
            }
            let k = (depth + 0.5).clamp(0.0, 1.0);
            for c in 0..N {
                canvas[c] += (dot[c] - canvas[c]) * k;
            }
        }
        px.copy_from_slice(&canvas);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    const NONE: Interrupt<'static> = Interrupt::NONE;

    fn flat(w: usize, h: usize, c: [f32; 4]) -> Vec<f32> {
        (0..w * h).flat_map(|_| c).collect()
    }

    fn noisy(w: usize, h: usize) -> Vec<f32> {
        (0..w * h * N).map(|i| hash01(i as i32, 3, 5, 9)).collect()
    }

    fn all(w: usize, h: usize) -> Rect {
        Rect::new(0, 0, w as i32, h as i32)
    }

    #[test]
    fn color_halftone_white_stays_white() {
        let white = flat(40, 40, [1.0; 4]);
        let out = color_halftone(
            &white,
            40,
            40,
            all(40, 40),
            8.0,
            [108.0, 162.0, 90.0],
            &NONE,
        )
        .unwrap();
        assert_eq!(out, white);
    }

    #[test]
    fn color_halftone_mid_grey_makes_dots_and_keeps_alpha() {
        let img = flat(40, 40, [0.5, 0.5, 0.5, 0.5]);
        let out =
            color_halftone(&img, 40, 40, all(40, 40), 8.0, [108.0, 162.0, 90.0], &NONE).unwrap();
        assert!(out.as_chunks::<4>().0.iter().all(|p| p[3] == 0.5));
        let dark = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] < 0.01)
            .count();
        let light = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 0.99)
            .count();
        assert!(dark > 200 && light > 20, "{dark} {light}");
    }

    #[test]
    fn color_halftone_only_touches_bounds() {
        let img = noisy(40, 40);
        let b = Rect::new(10, 10, 30, 30);
        let out = color_halftone(&img, 40, 40, b, 4.0, [108.0, 162.0, 90.0], &NONE).unwrap();
        for (i, (a, b0)) in out
            .as_chunks::<4>()
            .0
            .iter()
            .zip(img.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let (x, y) = ((i % 40) as i32, (i / 40) as i32);
            if !b.contains(x, y) {
                assert_eq!(a, b0);
            }
        }
        assert_ne!(out, img);
    }

    #[test]
    fn pointillize_is_deterministic_with_seed() {
        let img = noisy(48, 48);
        let run =
            |seed| pointillize(&img, 48, 48, all(48, 48), 6.0, seed, [1.0; 4], &NONE).unwrap();
        assert_eq!(run(3), run(3));
        assert_ne!(run(3), run(4));
    }

    #[test]
    fn pointillize_canvas_is_the_background() {
        let img = flat(48, 48, [0.0, 0.0, 0.0, 1.0]);
        let out = pointillize(
            &img,
            48,
            48,
            all(48, 48),
            8.0,
            1,
            [1.0, 0.0, 0.0, 1.0],
            &NONE,
        )
        .unwrap();
        assert!(
            out.as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 0.99 && p[1] < 0.01)
        );
    }

    #[test]
    fn mezzotint_outputs_channel_extremes() {
        let img = noisy(32, 32);
        for kind in [
            MezzotintType::FineDots,
            MezzotintType::MediumDots,
            MezzotintType::GrainyDots,
            MezzotintType::CoarseDots,
            MezzotintType::ShortLines,
            MezzotintType::MediumLines,
            MezzotintType::LongLines,
            MezzotintType::ShortStrokes,
            MezzotintType::MediumStrokes,
            MezzotintType::LongStrokes,
        ] {
            let out = mezzotint(&img, 32, 32, all(32, 32), kind, 2, &NONE).unwrap();
            for (p, q) in out
                .as_chunks::<4>()
                .0
                .iter()
                .zip(img.as_chunks::<4>().0.iter())
            {
                assert!(p[..3].iter().all(|&v| v == 0.0 || v == 1.0), "{kind:?}");
                assert_eq!(p[3], q[3]);
            }
        }
    }

    #[test]
    fn facet_flat_is_identity() {
        let img = flat(16, 16, [0.4, 0.6, 0.2, 1.0]);
        let out = facet(&img, 16, 16, all(16, 16), &NONE).unwrap();
        for (a, b) in out.iter().zip(&img) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn facet_keeps_a_hard_edge() {
        let img: Vec<f32> = (0..16 * 16)
            .flat_map(|i| {
                if i % 16 < 8 {
                    [0.0, 0.0, 0.0, 1.0]
                } else {
                    [1.0; 4]
                }
            })
            .collect();
        let out = facet(&img, 16, 16, all(16, 16), &NONE).unwrap();
        for (a, b) in out.iter().zip(&img) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn cancelled_kernels_return_none() {
        let flag = AtomicBool::new(true);
        let ctl = Interrupt(Some(&flag));
        assert!(flag.load(Ordering::Relaxed));
        let img = noisy(16, 16);
        assert!(facet(&img, 16, 16, all(16, 16), &ctl).is_none());
        assert!(mezzotint(&img, 16, 16, all(16, 16), MezzotintType::FineDots, 0, &ctl).is_none());
        assert!(pointillize(&img, 16, 16, all(16, 16), 5.0, 0, [1.0; 4], &ctl).is_none());
        assert!(color_halftone(&img, 16, 16, all(16, 16), 8.0, [0.0; 3], &ctl).is_none());
    }

    const ANGLES: [f32; 3] = [108.0, 162.0, 90.0];

    /// The third angle screens blue only.
    #[test]
    fn color_halftone_third_angle_changes_only_blue() {
        let img = flat(40, 40, [0.5, 0.5, 0.5, 1.0]);
        let run = |a| color_halftone(&img, 40, 40, all(40, 40), 8.0, a, &NONE).unwrap();
        let (a, b) = (run(ANGLES), run([108.0, 162.0, 20.0]));
        for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0) {
            assert_eq!((p[0], p[1], p[3]), (q[0], q[1], q[3]));
        }
        assert_ne!(a, b);
    }

    /// Pure white and pure black thresholds stay put in every pattern.
    #[test]
    fn mezzotint_keeps_white_white_and_black_black() {
        for (c, want) in [(1.0, 1.0), (0.0, 0.0)] {
            let img = flat(24, 24, [c, c, c, 1.0]);
            for kind in [
                MezzotintType::FineDots,
                MezzotintType::LongStrokes,
                MezzotintType::ShortLines,
            ] {
                let out = mezzotint(&img, 24, 24, all(24, 24), kind, 5, &NONE).unwrap();
                assert!(
                    out.as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[..3].iter().all(|&v| v == want))
                );
            }
        }
    }

    /// The radius grid is only a memo: forcing the direct fallback gives identical pixels.
    #[test]
    fn halftone_grid_matches_direct_fallback() {
        let img = noisy(37, 29);
        let b = Rect::new(3, 2, 35, 27);
        let grid = halftone_with_cap(&img, 37, 29, b, 5.0, ANGLES, &NONE, GRID_CAP).unwrap();
        let direct = halftone_with_cap(&img, 37, 29, b, 5.0, ANGLES, &NONE, 0).unwrap();
        assert_eq!(grid, direct);
    }

    /// Anchoring: a grid anchored on `bounds` makes a sub-rect render equal the render of the
    /// cropped layer (origin 0,0). Color Halftone reads only inside the bounds, so it matches
    /// everywhere; Pointillize reads the whole layer for dots centred just past the bounds, so
    /// only pixels a cell inside the edge are compared (to float rounding).
    #[test]
    fn grids_anchor_on_bounds() {
        let (w, h) = (50, 44);
        let img = noisy(w, h);
        let b = Rect::new(7, 5, 43, 37);
        let (cw, ch) = (b.width() as usize, b.height() as usize);
        let mut crop = Vec::new();
        for y in b.y0..b.y1 {
            let at = (y as usize * w + b.x0 as usize) * N;
            crop.extend_from_slice(&img[at..at + cw * N]);
        }
        let sub = |full: &[f32], x: usize, y: usize, c: usize, ox: usize, oy: usize, fw: usize| {
            full[((y + oy) * fw + x + ox) * N + c]
        };
        let a = color_halftone(&img, w, h, b, 4.0, ANGLES, &NONE).unwrap();
        let k = color_halftone(&crop, cw, ch, all(cw, ch), 4.0, ANGLES, &NONE).unwrap();
        let pa = pointillize(&img, w, h, b, 6.0, 3, [1.0; 4], &NONE).unwrap();
        let pk = pointillize(&crop, cw, ch, all(cw, ch), 6.0, 3, [1.0; 4], &NONE).unwrap();
        for y in 0..ch {
            for x in 0..cw {
                for c in 0..N {
                    let (bx, by) = (b.x0 as usize, b.y0 as usize);
                    assert_eq!(sub(&a, x, y, c, bx, by, w), sub(&k, x, y, c, 0, 0, cw));
                    if (6..cw - 6).contains(&x) && (6..ch - 6).contains(&y) {
                        let d = (sub(&pa, x, y, c, bx, by, w) - sub(&pk, x, y, c, 0, 0, cw)).abs();
                        assert!(d < 1e-3, "({x}, {y}) {c}");
                    }
                }
            }
        }
    }

    #[test]
    fn empty_bounds_return_the_layer() {
        let img = noisy(8, 8);
        let none = Rect::new(20, 20, 30, 30);
        assert_eq!(facet(&img, 8, 8, none, &NONE).unwrap(), img);
        assert_eq!(
            color_halftone(&img, 8, 8, none, 8.0, [0.0; 3], &NONE).unwrap(),
            img
        );
    }
}
