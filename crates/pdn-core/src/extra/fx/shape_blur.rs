//! Ported from PhotoCraft crates/algo/src/blur2.rs and shape_blur.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only Shape Blur; one whole interleaved straight RGBA f32 layer in and out, the effect's `out` rect computed over a halo window of the layer, outside the layer reading as the nearest edge pixel (upstream's `Image::read_clamped`, R57); rows run through `extra::par` and poll `Interrupt`; the tiling, 16-bit and scalar-oracle tests are replaced by property tests.
//!
//! Shaped kernels are applied as horizontal spans over per-row prefix sums (f64), so a kernel of
//! radius r costs O(r) per pixel instead of O(r²).

use std::f32::consts::{PI, TAU};

use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par;

const N: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlurShape {
    Circle,
    Ring,
    Square,
    Diamond,
    Triangle,
    Hexagon,
    Star,
    Heart,
    Cross,
}

/// Point-in-polygon (even-odd) for `pts`.
fn in_poly(u: f32, v: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > v) != (yj > v) && u < (xj - xi) * (v - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Radius of a regular `n`-gon (circumradius 1) in direction `theta`.
fn polygon_radius(theta: f32, n: u32, rot: f32) -> f32 {
    let seg = TAU / n as f32;
    let a = (theta - rot).rem_euclid(seg) - seg / 2.0;
    (PI / n as f32).cos() / a.cos()
}

/// Whether `(u, v)` (each in -1..1) is inside the shape.
fn inside(shape: BlurShape, star: &[(f32, f32)], u: f32, v: f32) -> bool {
    match shape {
        BlurShape::Circle => u * u + v * v <= 1.0,
        BlurShape::Ring => (0.36..=1.0).contains(&(u * u + v * v)),
        BlurShape::Square => true,
        BlurShape::Diamond => u.abs() + v.abs() <= 1.0,
        BlurShape::Triangle => v <= 0.75 && u.abs() <= (v + 1.0) / 1.75,
        BlurShape::Hexagon => (u * u + v * v).sqrt() <= polygon_radius(v.atan2(u), 6, 0.0),
        BlurShape::Star => in_poly(u, v, star),
        BlurShape::Heart => {
            // (x² + y² − 1)³ − x²y³ ≤ 0, scaled into the unit square and flipped (y up).
            let (x, y) = (u * 1.2, -v * 1.2 + 0.15);
            (x * x + y * y - 1.0).powi(3) - x * x * y * y * y <= 0.0
        }
        BlurShape::Cross => u.abs() <= 0.3 || v.abs() <= 0.3,
    }
}

/// A binary kernel as horizontal runs `(dy, dx0, dx1)` (inclusive).
struct Spans {
    rows: Vec<(i32, i32, i32)>,
    count: f64,
}

impl Spans {
    /// Rasterizes `shape` over a disc of radius `r`.
    fn new(r: f32, shape: BlurShape) -> Spans {
        let star: Vec<(f32, f32)> = (0..10)
            .map(|i| {
                let r = if i % 2 == 0 { 1.0 } else { 0.42 };
                let a = -PI / 2.0 + i as f32 * PI / 5.0;
                (r * a.cos(), r * a.sin())
            })
            .collect();
        let ri = r.max(0.0).ceil() as i32;
        let mut rows = Vec::new();
        let mut count = 0.0;
        if r < 0.5 {
            return Spans {
                rows: vec![(0, 0, 0)],
                count: 1.0,
            };
        }
        for dy in -ri..=ri {
            let mut run: Option<i32> = None;
            for dx in -ri..=ri + 1 {
                let hit = dx <= ri && inside(shape, &star, dx as f32 / r, dy as f32 / r);
                match (hit, run) {
                    (true, None) => run = Some(dx),
                    (false, Some(s)) => {
                        rows.push((dy, s, dx - 1));
                        count += f64::from(dx - s);
                        run = None;
                    }
                    _ => {}
                }
            }
        }
        if rows.is_empty() {
            rows.push((0, 0, 0));
            count = 1.0;
        }
        Spans { rows, count }
    }
}

/// Shape Blur of the `w × h` layer over `out`: each pixel the mean of the shape of `radius` px
/// (clamped to 0..1000) about it, colour weighted by alpha; the layer's edge pixels repeat
/// outward. The whole layer back with `out` replaced; `None` once cancelled.
pub fn shape_blur(
    buf: &[f32],
    (w, h): (usize, usize),
    out: Rect,
    radius: f32,
    shape: BlurShape,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let r = if radius.is_finite() {
        radius.clamp(0.0, 1000.0)
    } else {
        0.0
    };
    let out = out.intersect(&Rect::new(0, 0, w as i32, h as i32));
    if out.is_empty() || r < 0.5 {
        return Some(buf.to_vec());
    }
    let spans = Spans::new(r, shape);
    // The window the kernel reads: `out` plus the radius, inside the layer. Where the kernel
    // overhangs the layer, rows clamp into it and columns add `overhang × edge pixel`.
    let ri = r.ceil() as i32;
    let src = Rect::new(out.x0 - ri, out.y0 - ri, out.x1 + ri, out.y1 + ri)
        .intersect(&Rect::new(0, 0, w as i32, h as i32));
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let ow = out.width() as usize;
    let (wi, sx0) = (w as i64, i64::from(src.x0));
    let stride = (sw + 1) * N;
    // Per-row prefix sums of the premultiplied window.
    let mut prefix = vec![0.0f64; stride * sh];
    par::rows_mut(&mut prefix, stride, |y, row| {
        if ctl.cancelled() {
            return;
        }
        let at = ((src.y0 as usize + y) * w + src.x0 as usize) * N;
        let mut sum = [0.0f64; N];
        for (x, px) in buf[at..at + sw * N].as_chunks::<N>().0.iter().enumerate() {
            for c in 0..N {
                // Multiplication stays f32, exactly as upstream's premultiply.
                let v = if c < 3 { px[c] * px[3] } else { px[c] };
                sum[c] += f64::from(v);
                row[(x + 1) * N + c] = sum[c];
            }
        }
    });
    let mut res = buf.to_vec();
    par::rows_mut(&mut res, w * N, |y, row| {
        if (y as i32) < out.y0 || y as i32 >= out.y1 || ctl.cancelled() {
            return;
        }
        let mut acc = vec![0.0f64; ow * N];
        for &(dy, a, b) in &spans.rows {
            let ay = (y as i64 + i64::from(dy)).clamp(0, h as i64 - 1);
            let yy = (ay - i64::from(src.y0)) as usize;
            let prow = &prefix[yy * stride..(yy + 1) * stride];
            let first = i64::from(out.x0) + i64::from(a);
            let last_end = i64::from(out.x0) + (ow as i64 - 1) + i64::from(b) + 1;
            if first >= 0 && last_end <= wi {
                // Contiguous interior: no clamps, so the loop vectorizes.
                let lo = (first - sx0) as usize * N;
                let hi = (first + i64::from(b - a) + 1 - sx0) as usize * N;
                let (low, high) = (&prow[lo..lo + ow * N], &prow[hi..hi + ow * N]);
                for ((acc, hi), lo) in acc.iter_mut().zip(high).zip(low) {
                    *acc += hi - lo;
                }
                continue;
            }
            // Edge pixels of the row (read only where the kernel overhangs that side).
            let edge0: [f64; N] = std::array::from_fn(|c| prow[N + c] - prow[c]);
            let edge1: [f64; N] =
                std::array::from_fn(|c| prow[sw * N + c] - prow[(sw - 1) * N + c]);
            for x in 0..ow {
                let l = first + x as i64;
                let r = l + i64::from(b - a) + 1;
                let lo = (l.clamp(0, wi) - sx0) as usize * N;
                let hi = (r.clamp(0, wi) - sx0) as usize * N;
                let left = (r.min(0) - l).max(0) as f64;
                let right = (r - l.max(wi)).max(0) as f64;
                for c in 0..N {
                    acc[x * N + c] +=
                        prow[hi + c] - prow[lo + c] + left * edge0[c] + right * edge1[c];
                }
            }
        }
        let dst = &mut row[out.x0 as usize * N..(out.x0 as usize + ow) * N];
        for (px, acc) in dst
            .as_chunks_mut::<N>()
            .0
            .iter_mut()
            .zip(acc.as_chunks::<N>().0)
        {
            for c in 0..N {
                px[c] = (acc[c] / spans.count) as f32;
            }
            let a = px[3];
            for v in &mut px[..3] {
                *v = if a > 1e-7 { *v / a } else { 0.0 };
            }
        }
    });
    (!ctl.cancelled()).then_some(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: &Interrupt<'static> = &Interrupt::NONE;
    const SHAPES: [BlurShape; 9] = [
        BlurShape::Circle,
        BlurShape::Ring,
        BlurShape::Square,
        BlurShape::Diamond,
        BlurShape::Triangle,
        BlurShape::Hexagon,
        BlurShape::Star,
        BlurShape::Heart,
        BlurShape::Cross,
    ];

    /// A deterministic non-trivial layer with partial alpha. Every value is a multiple of 1/128
    /// (products of two of 1/16384), so f64 sums are exact in any order and the oracle below can
    /// be compared bit for bit.
    fn layer(w: usize, h: usize) -> Vec<f32> {
        (0..w * h)
            .flat_map(|i| {
                let f = |k: usize| ((i * 31 + k * 13) % 97) as f32 / 128.0;
                [f(0), f(1), f(2), 0.25 + f(3) * 0.5]
            })
            .collect()
    }

    #[test]
    fn binary_kernel_sampling_including_fractional_square_is_preserved() {
        for (shape, radius, count) in [
            (BlurShape::Circle, 3.0, 29.0),
            (BlurShape::Ring, 3.0, 20.0),
            (BlurShape::Square, 3.0, 49.0),
            (BlurShape::Square, 2.5, 49.0),
            (BlurShape::Diamond, 3.0, 25.0),
            (BlurShape::Triangle, 3.0, 18.0),
            (BlurShape::Cross, 3.0, 13.0),
        ] {
            assert_eq!(Spans::new(radius, shape).count, count);
        }
    }

    /// A point-by-point mean over the same kernel (alpha-weighted, edge pixels repeated outward)
    /// matches the span/prefix result bit for bit, for every shape, at the layer's edges and
    /// corners and in the interior fast path.
    #[test]
    fn point_convolution_independent_of_prefix_and_span_accumulation() {
        let (w, h) = (13, 11);
        let src = layer(w, h);
        let outs = [
            Rect::new(0, 0, w as i32, h as i32),
            Rect::new(5, 4, 8, 7),
            Rect::new(0, 3, 6, 9),
            Rect::new(7, 0, 13, 5),
        ];
        for shape in SHAPES {
            for radius in [0.5, 1.0, 2.5, 4.0, 20.0] {
                let spans = Spans::new(radius, shape);
                for out in outs {
                    let got = shape_blur(&src, (w, h), out, radius, shape, NONE).unwrap();
                    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
                        let p = &got[(y * w + x) * 4..][..4];
                        if !out.contains(x as i32, y as i32) {
                            assert_eq!(p, &src[(y * w + x) * 4..][..4]);
                            continue;
                        }
                        let mut acc = [0.0f64; 4];
                        for &(dy, a, b) in &spans.rows {
                            for dx in a..=b {
                                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                                let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                                let q = &src[(sy * w + sx) * 4..][..4];
                                for c in 0..3 {
                                    acc[c] += f64::from(q[c] * q[3]);
                                }
                                acc[3] += f64::from(q[3]);
                            }
                        }
                        let m: [f32; 4] = std::array::from_fn(|c| (acc[c] / spans.count) as f32);
                        for c in 0..4 {
                            let want = if c < 3 {
                                if m[3] > 1e-7 { m[c] / m[3] } else { 0.0 }
                            } else {
                                m[3]
                            };
                            assert_eq!(p[c], want, "{shape:?} {radius} {out:?} {x},{y} c{c}");
                        }
                    }
                }
            }
        }
    }

    /// An opaque layer stays opaque at its edges and corners: the layer repeats outward instead
    /// of fading to transparent.
    #[test]
    fn opaque_layer_keeps_alpha_at_edges_and_corners() {
        let (w, h) = (12, 9);
        let mut src = layer(w, h);
        src.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|p| p[3] = 1.0);
        let all = Rect::new(0, 0, w as i32, h as i32);
        for shape in SHAPES {
            let out = shape_blur(&src, (w, h), all, 6.0, shape, NONE).unwrap();
            assert!(
                out.as_chunks::<4>().0.iter().all(|p| p[3] == 1.0),
                "{shape:?}"
            );
        }
    }

    #[test]
    fn radius_below_half_a_pixel_and_nonfinite_are_the_identity() {
        let src = layer(9, 9);
        let all = Rect::new(0, 0, 9, 9);
        for r in [0.0, 0.49, f32::NAN, f32::NEG_INFINITY, -3.0] {
            assert_eq!(
                shape_blur(&src, (9, 9), all, r, BlurShape::Star, NONE).unwrap(),
                src
            );
        }
        // Infinite radius clamps to 1000 and still finishes on a small layer.
        let out = shape_blur(&src, (9, 9), all, f32::INFINITY, BlurShape::Cross, NONE).unwrap();
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn only_the_out_rect_changes() {
        let (w, h) = (24, 20);
        let src = layer(w, h);
        let out = Rect::new(6, 5, 18, 14);
        let res = shape_blur(&src, (w, h), out, 3.0, BlurShape::Hexagon, NONE).unwrap();
        for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
            let i = (y * w + x) * 4;
            if !out.contains(x as i32, y as i32) {
                assert_eq!(res[i..i + 4], src[i..i + 4], "{x},{y}");
            }
        }
        assert_ne!(res, src);
        // An out rect off the layer changes nothing.
        let far = Rect::new(40, 40, 50, 50);
        assert_eq!(
            shape_blur(&src, (w, h), far, 3.0, BlurShape::Circle, NONE).unwrap(),
            src
        );
    }

    #[test]
    fn cancelled_returns_none() {
        let src = layer(16, 16);
        let flag = std::sync::atomic::AtomicBool::new(true);
        let ctl = Interrupt(Some(&flag));
        let all = Rect::new(0, 0, 16, 16);
        assert!(shape_blur(&src, (16, 16), all, 3.0, BlurShape::Ring, &ctl).is_none());
    }
}
