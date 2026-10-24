//! Ported from PhotoCraft crates/algo/src/distort2.rs (`displace`, `shear`, their helpers) and crates/algo/src/distort.rs (`remap`), crates/algo/src/image.rs (`Image::sample`, `get_edge`) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: interleaved straight RGBA f32 buffers (w×h, bounds as `extra::geom::Rect`) instead of `Image`/`Ctx`; no Transparent edge (the dialog offers Wrap and Repeat); the map is an RGBA buffer, so its vertical channel is green as for upstream's >= 3-channel maps; ZigZag dropped; Shear's curve is five dialog sliders (offsets in half widths at 0/25/50/75/100 % of the height), interpolated by a monotone cubic (Fritsch-Carlson tangents) instead of Catmull-Rom, and an offset of 1 reaches half the width, as upstream's `* bw / 2.0` (R27; the curve itself is R22's approximation).

use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;

/// Max displacement at 100 % scale and full map contrast, px.
const DISPLACE_PX: f32 = 128.0;

/// How samples outside the reference bounds are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Wrap,
    Repeat,
}

/// A w×h straight RGBA f32 image.
#[derive(Clone, Copy)]
pub struct Img<'a> {
    pub w: i32,
    pub h: i32,
    pub data: &'a [f32],
}

impl Img<'_> {
    fn get(&self, x: i32, y: i32, c: usize) -> f32 {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.data[(y as usize * self.w as usize + x as usize) * 4 + c]
        } else {
            0.0
        }
    }

    /// A sample with the edge policy relative to `area`.
    fn get_edge(&self, x: i32, y: i32, c: usize, edge: Edge, area: Rect) -> f32 {
        if area.is_empty() {
            return 0.0;
        }
        if area.contains(x, y) {
            return self.get(x, y, c);
        }
        match edge {
            Edge::Repeat => self.get(
                x.clamp(area.x0, area.x1 - 1),
                y.clamp(area.y0, area.y1 - 1),
                c,
            ),
            Edge::Wrap => {
                let (w, h) = (area.width().max(1) as i32, area.height().max(1) as i32);
                self.get(
                    area.x0 + (x - area.x0).rem_euclid(w),
                    area.y0 + (y - area.y0).rem_euclid(h),
                    c,
                )
            }
        }
    }

    /// Bilinear sample of the premultiplied pixel at `(x, y)` (centres at `.5`); straight RGBA.
    fn sample(&self, x: f32, y: f32, edge: Edge, area: Rect) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let mut out = [0.0f32; 4];
        let mut acc_a = 0.0;
        for (dx, dy, wgt) in [
            (0, 0, (1.0 - ax) * (1.0 - ay)),
            (1, 0, ax * (1.0 - ay)),
            (0, 1, (1.0 - ax) * ay),
            (1, 1, ax * ay),
        ] {
            if wgt <= 0.0 {
                continue;
            }
            let a = self.get_edge(x0 + dx, y0 + dy, 3, edge, area);
            for (c, o) in out.iter_mut().enumerate().take(3) {
                *o += self.get_edge(x0 + dx, y0 + dy, c, edge, area) * a * wgt;
            }
            acc_a += a * wgt;
        }
        if acc_a > 0.0 {
            for o in &mut out[..3] {
                *o /= acc_a;
            }
        }
        out[3] = acc_a;
        out
    }
}

/// Polls `int` once per row; a cancelled run keeps the source for the rows it never reached.
fn remap(
    src: Img,
    bounds: Rect,
    edge: Edge,
    int: Interrupt,
    f: impl Fn(f32, f32) -> (f32, f32),
) -> Vec<f32> {
    let mut res = Vec::with_capacity(src.data.len());
    for y in 0..src.h {
        if int.cancelled() {
            res.extend_from_slice(&src.data[res.len()..]);
            break;
        }
        for x in 0..src.w {
            let (sx, sy) = f(x as f32 + 0.5, y as f32 + 0.5);
            res.extend_from_slice(&src.sample(sx, sy, edge, bounds));
        }
    }
    res
}

/// Bilinear read of channel `c` of a map at map-local coordinates (clamped).
fn map_sample(map: Img, u: f32, v: f32, c: usize) -> f32 {
    let (fx, fy) = (u - 0.5, v - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x0, fy - y0);
    let g = |x: i32, y: i32| map.get(x.clamp(0, map.w - 1), y.clamp(0, map.h - 1), c);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let top = g(x0, y0) + (g(x0 + 1, y0) - g(x0, y0)) * ax;
    let bot = g(x0, y0 + 1) + (g(x0 + 1, y0 + 1) - g(x0, y0 + 1)) * ax;
    top + (bot - top) * ay
}

/// Displace: the map's red channel shifts pixels horizontally and its green one vertically;
/// mid-grey = no shift. The map is stretched over the bounds or tiled from their top-left.
pub fn displace(
    src: Img,
    bounds: Rect,
    scale: (f32, f32),
    stretch: bool,
    edge: Edge,
    map: Option<Img>,
    int: Interrupt,
) -> Vec<f32> {
    let Some(map) = map.filter(|m| m.w > 0 && m.h > 0) else {
        return src.data.to_vec();
    };
    let (mw, mh) = (map.w as f32, map.h as f32);
    let (bw, bh) = (bounds.width().max(1) as f32, bounds.height().max(1) as f32);
    let (sx, sy) = (scale.0 / 100.0 * DISPLACE_PX, scale.1 / 100.0 * DISPLACE_PX);
    remap(src, bounds, edge, int, |x, y| {
        let (lx, ly) = (x - bounds.x0 as f32, y - bounds.y0 as f32);
        let (u, v) = if stretch {
            (lx / bw * mw, ly / bh * mh)
        } else {
            (lx.rem_euclid(mw), ly.rem_euclid(mh))
        };
        let dx = (map_sample(map, u, v, 0) - 0.5) * 2.0 * sx;
        let dy = (map_sample(map, u, v, 1) - 0.5) * 2.0 * sy;
        (x + dx, y + dy)
    })
}

/// Monotone cubic (Fritsch-Carlson) through sorted `(t, value)` points, clamped at the ends.
pub fn curve_at(pts: &[[f32; 2]], t: f32) -> f32 {
    match pts.len() {
        0 => 0.0,
        1 => pts[0][1],
        n => {
            if t <= pts[0][0] {
                return pts[0][1];
            }
            if t >= pts[n - 1][0] {
                return pts[n - 1][1];
            }
            let i = pts
                .windows(2)
                .position(|w| t >= w[0][0] && t <= w[1][0])
                .unwrap_or(0);
            let slope =
                |k: usize| (pts[k + 1][1] - pts[k][1]) / (pts[k + 1][0] - pts[k][0]).max(1e-6);
            // Tangent at point k: zero at a local extreme, else the harmonic mean of the secants.
            let tangent = |k: usize| {
                if k == 0 {
                    slope(0)
                } else if k == n - 1 {
                    slope(n - 2)
                } else {
                    let (a, b) = (slope(k - 1), slope(k));
                    if a * b <= 0.0 {
                        0.0
                    } else {
                        2.0 * a * b / (a + b)
                    }
                }
            };
            let (p1, p2) = (pts[i], pts[i + 1]);
            let span = (p2[0] - p1[0]).max(1e-6);
            let s = (t - p1[0]) / span;
            let (m1, m2) = (tangent(i) * span, tangent(i + 1) * span);
            let (s2, s3) = (s * s, s * s * s);
            (2.0 * s3 - 3.0 * s2 + 1.0) * p1[1]
                + (s3 - 2.0 * s2 + s) * m1
                + (-2.0 * s3 + 3.0 * s2) * p2[1]
                + (s3 - s2) * m2
        }
    }
}

/// Shear: each row shifts horizontally by the curve through `points` (`[t, offset]`, t top to
/// bottom of the bounds, offset in half widths of the bounds, as upstream).
pub fn shear(src: Img, bounds: Rect, points: &[[f32; 2]], edge: Edge, int: Interrupt) -> Vec<f32> {
    let mut pts: Vec<[f32; 2]> = points
        .iter()
        .map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(-1.0, 1.0)])
        .collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6);
    let (bw, bh) = (bounds.width().max(1) as f32, bounds.height().max(1) as f32);
    // Per-row offsets are shared by every pixel of the row.
    let rows: Vec<f32> = (0..src.h)
        .map(|y| curve_at(&pts, (y as f32 + 0.5 - bounds.y0 as f32) / bh) * bw / 2.0)
        .collect();
    remap(src, bounds, edge, int, |x, y| {
        let i = ((y - 0.5) as i32).clamp(0, rows.len() as i32 - 1) as usize;
        (x - rows[i], y)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cancelled remap returns at once and keeps the source for the unvisited rows.
    #[test]
    fn displace_and_shear_stop_when_cancelled() {
        use std::sync::atomic::AtomicBool;
        let data: Vec<f32> = (0..4 * 4 * 4).map(|i| i as f32 / 64.0).collect();
        let img = Img {
            w: 4,
            h: 4,
            data: &data,
        };
        let flag = AtomicBool::new(true);
        let int = Interrupt(Some(&flag));
        let b = Rect::new(0, 0, 4, 4);
        let out = shear(img, b, &[[0.0, 0.5], [1.0, 0.5]], Edge::Wrap, int);
        assert_eq!(out, data);
        let map = Img {
            w: 1,
            h: 1,
            data: &[0.0, 0.0, 0.0, 1.0],
        };
        let out = displace(img, b, (50.0, 50.0), true, Edge::Wrap, Some(map), int);
        assert_eq!(out, data);
    }

    /// Upstream's reach: an offset of 1 shifts a row by half the width.
    #[test]
    fn shear_offset_reaches_half_a_width() {
        let data: Vec<f32> = (0..8)
            .flat_map(|x| [x as f32 / 8.0, 0.0, 0.0, 1.0])
            .collect();
        let img = Img {
            w: 8,
            h: 1,
            data: &data,
        };
        let out = shear(
            img,
            Rect::new(0, 0, 8, 1),
            &[[0.0, 1.0], [1.0, 1.0]],
            Edge::Wrap,
            Interrupt::NONE,
        );
        // Pixel 0 reads source x - 4 (wrapped) = 4.
        assert!((out[0] - 4.0 / 8.0).abs() < 1e-6);
    }

    #[test]
    fn curve_passes_through_points_and_clamps() {
        let pts = [[0.0, 0.0], [0.5, 0.4], [1.0, -0.2]];
        assert!((curve_at(&pts, 0.5) - 0.4).abs() < 1e-6);
        assert!((curve_at(&pts, 1.0) + 0.2).abs() < 1e-6);
        assert_eq!(curve_at(&pts, -1.0), 0.0);
        assert_eq!(curve_at(&[[0.0, 0.3], [1.0, 0.3]], 0.42), 0.3);
    }

    /// The five-slider curve never overshoots its points (monotone between monotone points).
    #[test]
    fn curve_is_monotone_between_monotone_points() {
        let pts = [[0.0, 0.0], [0.25, 0.0], [0.5, 0.1], [0.75, 0.9], [1.0, 1.0]];
        let mut last = f32::MIN;
        for k in 0..=100 {
            let v = curve_at(&pts, k as f32 / 100.0);
            assert!(v >= last - 1e-6 && (0.0..=1.0).contains(&v), "{k}: {v}");
            last = v;
        }
    }
}
