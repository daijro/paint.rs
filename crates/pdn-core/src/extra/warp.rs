//! Ported from PhotoCraft crates/algo/src/warp.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT), with `Interp` and `bicubic` from crates/algo/src/transform.rs. Changes: straight RGBA8 `ImageData` in and out instead of `Surface`, each tap premultiplied as it is read (no premultiplied footprint copy per tile); the source is always the whole layer and the result has the layer's size, warped only inside `clip` (the source is copied outside it), so the tiles cover `clip` rather than the warped extent; a triangle with a vertex (destination or source) that is non-finite or past ±2²⁴ is skipped instead of emptying the whole result; the cancel is polled per tile, per row of each triangle's fill and per row of the sampling, and returns `Err(Cancelled)`; no `Nearest`, progress, `place_source*`, `warp_mesh_gray` or `warp_mesh_surface_with`; rayon through `extra::par`.
//!
//! Mesh warps (Layers ▸ Puppet Warp and Perspective Warp): resampling a layer through any
//! forward map.
//!
//! The source rectangle is subdivided into a fine grid whose vertices are mapped forward; each
//! cell's two triangles are rasterized in destination space with the source position
//! interpolated barycentrically, then the source is sampled there (premultiplied, bilinear or
//! Photoshop's Keys bicubic). Cells are a few pixels across in both the source and the
//! destination, so the piecewise-affine inverse is within a small fraction of a pixel of the true
//! one; affine maps (the identity included) are reproduced exactly. Where the warp shrinks the
//! source, each pixel instead averages a grid of bilinear subsamples over its source footprint
//! (about one per source pixel), so thin lines don't alias. Where the warp folds over itself the
//! later (lower-right) cell wins. Work is split per destination tile (parallel on native).

use super::geom::Rect;
use super::interrupt::{Cancelled, Interrupt};
use super::par;
use crate::canvas::ImageData;

/// Grid cell size (source px) used to approximate the map.
const CELL: f64 = 4.0;
/// Most cells along one axis (bounds memory and time on huge layers).
const MAX_CELLS: usize = 512;
/// Most subsamples per axis when a warp shrinks the source; also the longest source step per
/// destination pixel the filter follows.
const MAX_SS: f64 = 8.0;
/// Destination tile side (px): the unit of work and of triangle binning.
const TILE: i32 = 128;
/// The largest coordinate a vertex may have; a triangle past it is skipped, so the pixel
/// arithmetic (`ix - 1 + i`) stays far inside `i32`.
const MAX_COORD: f64 = 16_777_216.0;
/// Probe lines per axis [`mapped_lengths`] follows through the map.
const PROBE: usize = 16;

/// Resampling of the source (upstream's interpolation menu, without Nearest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Bilinear,
    Bicubic,
}

/// Photoshop's Bicubic: the Keys cubic at a = −0.75 (Catmull-Rom is −0.5), the weights of the
/// four source pixels around a sample `t` (0..1) past the second.
pub fn bicubic(t: f64) -> [f64; 4] {
    const A: f64 = -0.75;
    let near = |x: f64| (A + 2.0) * x * x * x - (A + 3.0) * x * x + 1.0;
    let far = |x: f64| A * (x * x * x - 5.0 * x * x + 8.0 * x - 4.0);
    [far(1.0 + t), near(t), near(1.0 - t), far(2.0 - t)]
}

/// Warps the whole of `src` through the forward map `f` (source pixels → destination pixels),
/// writing only inside `clip`; pixels of `clip` the warp doesn't cover come out transparent.
pub fn warp_mesh(
    src: &ImageData,
    f: &(dyn Fn(f64, f64) -> (f64, f64) + Sync),
    clip: Rect,
    interp: Interp,
    stop: Interrupt,
) -> Result<ImageData, Cancelled> {
    let src_rect = Rect::new(0, 0, src.width as i32, src.height as i32);
    if src_rect.is_empty() {
        return Ok(src.clone());
    }
    let (sw, sh) = (f64::from(src.width), f64::from(src.height));
    // Cells are a few pixels across on both sides of the map: a thin source stretched far needs
    // many cells, or it comes out straight.
    let (lu, lv) = mapped_lengths(src_rect, f);
    let cells = |s: f64, d: f64| ((s.max(d) / CELL).ceil() as usize).clamp(1, MAX_CELLS);
    let (nx, ny) = (cells(sw, lu), cells(sh, lv));
    let mut verts = Vec::with_capacity((nx + 1) * (ny + 1));
    for j in 0..=ny {
        for i in 0..=nx {
            let sx = sw * i as f64 / nx as f64;
            let sy = sh * j as f64 / ny as f64;
            let (dx, dy) = f(sx, sy);
            verts.push(([dx, dy], [sx, sy]));
        }
    }
    let w1 = nx + 1;
    let mut tris = Vec::with_capacity(nx * ny * 2);
    for j in 0..ny {
        for i in 0..nx {
            let a = j * w1 + i;
            tris.push([a, a + 1, a + w1 + 1]);
            tris.push([a, a + w1 + 1, a + w1]);
        }
    }
    stop.check()?;
    warp_triangles(src, &verts, &tris, clip, interp, stop)
}

/// The longest destination length of a source row and of a source column under `f`, measured
/// along polylines through a [`PROBE`]-step grid. Non-finite steps count as zero.
fn mapped_lengths(src_rect: Rect, f: &(dyn Fn(f64, f64) -> (f64, f64) + Sync)) -> (f64, f64) {
    let (x0, y0) = (f64::from(src_rect.x0), f64::from(src_rect.y0));
    let (sw, sh) = (f64::from(src_rect.width()), f64::from(src_rect.height()));
    let n = PROBE + 1;
    let mut pts = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let (dx, dy) = f(
                x0 + sw * i as f64 / PROBE as f64,
                y0 + sh * j as f64 / PROBE as f64,
            );
            pts.push([dx, dy]);
        }
    }
    let step = |a: [f64; 2], b: [f64; 2]| {
        let d = (b[0] - a[0]).hypot(b[1] - a[1]);
        if d.is_finite() { d } else { 0.0 }
    };
    let (mut lu, mut lv) = (0.0f64, 0.0f64);
    for k in 0..n {
        let (mut row, mut col) = (0.0, 0.0);
        for m in 1..n {
            row += step(pts[k * n + m - 1], pts[k * n + m]);
            col += step(pts[(m - 1) * n + k], pts[m * n + k]);
        }
        lu = lu.max(row);
        lv = lv.max(col);
    }
    (lu, lv)
}

/// Rasterizes textured triangles into `src`'s size: each vertex is `(destination, source)` in
/// layer pixels and each triangle samples the whole of `src` through the barycentric
/// interpolation of its source positions. Later triangles draw over earlier ones. Only `clip` is
/// written (transparent where no triangle covers it); outside it the result is `src`.
pub fn warp_triangles(
    src: &ImageData,
    verts: &[([f64; 2], [f64; 2])],
    tris: &[[usize; 3]],
    clip: Rect,
    interp: Interp,
    stop: Interrupt,
) -> Result<ImageData, Cancelled> {
    triangles(src, verts, tris, clip, interp, &|| stop.cancelled()).ok_or(Cancelled)
}

/// [`warp_triangles`] with the cancel as a poll (`None` once it says stop).
fn triangles(
    src: &ImageData,
    verts: &[([f64; 2], [f64; 2])],
    tris: &[[usize; 3]],
    clip: Rect,
    interp: Interp,
    poll: &(dyn Fn() -> bool + Sync),
) -> Option<ImageData> {
    let (sw, sh) = (src.width as i32, src.height as i32);
    let clip = clip.intersect(&Rect::new(0, 0, sw, sh));
    let mut out = src.clone();
    if clip.is_empty() {
        return (!poll()).then_some(out);
    }
    let ok = |p: [f64; 2]| p.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD);
    let tris: Vec<[usize; 3]> = tris
        .iter()
        .copied()
        .filter(|t| {
            t.iter()
                .all(|&v| verts.get(v).is_some_and(|(d, s)| ok(*d) && ok(*s)))
        })
        .collect();
    // Triangles binned per destination tile of `clip`, in draw order.
    let cols = clip.width().div_ceil(TILE as u32) as usize;
    let rows = clip.height().div_ceil(TILE as u32) as usize;
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); cols * rows];
    for (k, t) in tris.iter().enumerate() {
        let ps = t.map(|v| verts[v].0);
        let lo = |c: usize| ps.iter().map(|p| p[c]).fold(f64::MAX, f64::min);
        let hi = |c: usize| ps.iter().map(|p| p[c]).fold(f64::MIN, f64::max);
        let x0 = ((lo(0) - 0.5).floor() as i32).max(clip.x0);
        let y0 = ((lo(1) - 0.5).floor() as i32).max(clip.y0);
        let x1 = ((hi(0) + 0.5).ceil() as i32).min(clip.x1);
        let y1 = ((hi(1) + 0.5).ceil() as i32).min(clip.y1);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        for cy in (y0 - clip.y0) / TILE..=(y1 - 1 - clip.y0) / TILE {
            for cx in (x0 - clip.x0) / TILE..=(x1 - 1 - clip.x0) / TILE {
                bins[cy as usize * cols + cx as usize].push(k as u32);
            }
        }
    }
    let (w, a) = (src.width as usize, 3);
    // A source pixel, premultiplied, 0–1; transparent outside the layer.
    let at = |x: i32, y: i32| -> [f64; 4] {
        if x < 0 || y < 0 || x >= sw || y >= sh {
            return [0.0; 4];
        }
        let p = &src.data[(y as usize * w + x as usize) * 4..][..4];
        let al = f64::from(p[3]) / 255.0;
        [
            f64::from(p[0]) / 255.0 * al,
            f64::from(p[1]) / 255.0 * al,
            f64::from(p[2]) / 255.0 * al,
            al,
        ]
    };
    // One premultiplied sample at source position (u, v) (pixel centres at integers).
    let point = |interp: Interp, u: f64, v: f64| -> [f64; 4] {
        let mut acc = [0.0f64; 4];
        match interp {
            Interp::Bilinear => {
                let (ix, iy) = (u.floor() as i32, v.floor() as i32);
                let (fx, fy) = (u - f64::from(ix), v - f64::from(iy));
                for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
                    for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
                        let p = at(ix + dx, iy + dy);
                        for (s, v) in acc.iter_mut().zip(p) {
                            *s += v * wx * wy;
                        }
                    }
                }
            }
            Interp::Bicubic => {
                // Snap to the pixel grid when within float noise, so identity is exact.
                let snap = |t: f64| {
                    if (t - t.round()).abs() < 1e-7 {
                        t.round()
                    } else {
                        t
                    }
                };
                let (u, v) = (snap(u), snap(v));
                let (ix, iy) = (u.floor() as i32, v.floor() as i32);
                let (wx, wy) = (bicubic(u - f64::from(ix)), bicubic(v - f64::from(iy)));
                for (j, wyj) in wy.iter().enumerate() {
                    for (i, wxi) in wx.iter().enumerate() {
                        let k = wxi * wyj;
                        if k == 0.0 {
                            continue;
                        }
                        let p = at(ix - 1 + i as i32, iy - 1 + j as i32);
                        for (s, v) in acc.iter_mut().zip(p) {
                            *s += v * k;
                        }
                    }
                }
            }
        }
        acc
    };
    let work = |i: usize| -> Option<(Rect, Vec<u8>)> {
        if poll() {
            return None;
        }
        let (cx, cy) = ((i % cols) as i32, (i / cols) as i32);
        let t = Rect::new(
            clip.x0 + cx * TILE,
            clip.y0 + cy * TILE,
            (clip.x0 + (cx + 1) * TILE).min(clip.x1),
            (clip.y0 + (cy + 1) * TILE).min(clip.y1),
        );
        let (tw, th) = (t.width() as usize, t.height() as usize);
        let mut px = vec![0u8; tw * th * 4];
        let bin = &bins[i];
        if bin.is_empty() {
            return Some((t, px));
        }
        // Subsamples along one destination axis, from the source step `j` it makes: one where
        // the warp keeps or enlarges the source, else about one per source pixel the output
        // pixel spans (an area average). The subsamples are bilinear: the box does the filtering.
        let count = |j: [f64; 2]| j[0].hypot(j[1]).round().clamp(1.0, MAX_SS) as usize;
        // Source position per destination pixel (NaN = not covered) and the triangle it came
        // from. Per triangle: its source step per destination pixel along x and y (`[dx_u, dx_v,
        // dy_u, dy_v]`, constant over a triangle; zero when point-sampled) and the subsamples.
        let mut uv = vec![[f64::NAN; 2]; tw * th];
        let mut owner = vec![0u32; tw * th];
        let mut jac: Vec<([f64; 4], usize, usize)> = Vec::with_capacity(bin.len());
        for (k, &ti) in bin.iter().enumerate() {
            let [(p0, s0), (p1, s1), (p2, s2)] = tris[ti as usize].map(|v| verts[v]);
            let det = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p2[0] - p0[0]) * (p1[1] - p0[1]);
            if det.abs() < 1e-12 {
                jac.push(([0.0; 4], 1, 1));
                continue;
            }
            let (d1, d2) = (
                [s1[0] - s0[0], s1[1] - s0[1]],
                [s2[0] - s0[0], s2[1] - s0[1]],
            );
            let (l1x, l1y, l2x, l2y) = (
                (p2[1] - p0[1]) / det,
                (p0[0] - p2[0]) / det,
                (p0[1] - p1[1]) / det,
                (p1[0] - p0[0]) / det,
            );
            // Capped, so a sliver triangle can't ask for an unbounded footprint.
            let cap = |j: [f64; 2]| {
                let l = j[0].hypot(j[1]);
                if l > MAX_SS {
                    [j[0] * MAX_SS / l, j[1] * MAX_SS / l]
                } else {
                    j
                }
            };
            let jx = cap([l1x * d1[0] + l2x * d2[0], l1x * d1[1] + l2x * d2[1]]);
            let jy = cap([l1y * d1[0] + l2y * d2[0], l1y * d1[1] + l2y * d2[1]]);
            let (nx, ny) = (count(jx), count(jy));
            jac.push(if nx * ny == 1 {
                ([0.0; 4], 1, 1)
            } else {
                ([jx[0], jx[1], jy[0], jy[1]], nx, ny)
            });
            let bx0 = (p0[0].min(p1[0]).min(p2[0]) - 0.5)
                .floor()
                .max(f64::from(t.x0)) as i32;
            let by0 = (p0[1].min(p1[1]).min(p2[1]) - 0.5)
                .floor()
                .max(f64::from(t.y0)) as i32;
            let bx1 = (p0[0].max(p1[0]).max(p2[0]) + 0.5)
                .ceil()
                .min(f64::from(t.x1)) as i32;
            let by1 = (p0[1].max(p1[1]).max(p2[1]) + 0.5)
                .ceil()
                .min(f64::from(t.y1)) as i32;
            let eps = -1e-9;
            for y in by0..by1 {
                // R71: per row, so one huge triangle can't hold up a cancel.
                if poll() {
                    return None;
                }
                let py = f64::from(y) + 0.5;
                for x in bx0..bx1 {
                    let px = f64::from(x) + 0.5;
                    let l1 =
                        ((px - p0[0]) * (p2[1] - p0[1]) - (p2[0] - p0[0]) * (py - p0[1])) / det;
                    let l2 =
                        ((p1[0] - p0[0]) * (py - p0[1]) - (px - p0[0]) * (p1[1] - p0[1])) / det;
                    let l0 = 1.0 - l1 - l2;
                    if l0 < eps || l1 < eps || l2 < eps {
                        continue;
                    }
                    let o = (y - t.y0) as usize * tw + (x - t.x0) as usize;
                    uv[o] = [
                        l0 * s0[0] + l1 * s1[0] + l2 * s2[0],
                        l0 * s0[1] + l1 * s1[1] + l2 * s2[1],
                    ];
                    owner[o] = k as u32;
                }
            }
        }
        for y in 0..th {
            if poll() {
                return None;
            }
            for x in 0..tw {
                let o = y * tw + x;
                let q = uv[o];
                if q[0].is_nan() {
                    continue;
                }
                let (j, nx, ny) = jac[owner[o] as usize];
                let acc = if nx * ny == 1 {
                    point(interp, q[0] - 0.5, q[1] - 0.5)
                } else {
                    let mut acc = [0.0f64; 4];
                    for sy in 0..ny {
                        let t = (sy as f64 + 0.5) / ny as f64 - 0.5;
                        for sx in 0..nx {
                            let s = (sx as f64 + 0.5) / nx as f64 - 0.5;
                            let one = point(
                                Interp::Bilinear,
                                q[0] + s * j[0] + t * j[2] - 0.5,
                                q[1] + s * j[1] + t * j[3] - 0.5,
                            );
                            for (c, v) in acc.iter_mut().zip(one) {
                                *c += v;
                            }
                        }
                    }
                    acc.map(|c| c / (nx * ny) as f64)
                };
                let al = acc[a].clamp(0.0, 1.0);
                // Float noise (a vertex a hair off the pixel grid) must not leave invisible
                // colour behind in transparent pixels.
                if al <= 1e-6 {
                    continue;
                }
                let b = o * 4;
                for c in 0..a {
                    // The unclamped alpha, so a bicubic overshoot past 1 doesn't lighten the
                    // colour (#6203).
                    px[b + c] = ((acc[c] / acc[a]).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                px[b + a] = (al * 255.0 + 0.5) as u8;
            }
        }
        Some((t, px))
    };
    let done = par::map(cols * rows, true, work);
    if poll() {
        return None;
    }
    for (t, px) in done.into_iter().collect::<Option<Vec<_>>>()? {
        let tw = t.width() as usize * 4;
        for (k, row) in px.chunks_exact(tw).enumerate() {
            let o = ((t.y0 as usize + k) * w + t.x0 as usize) * 4;
            out.data[o..o + tw].copy_from_slice(row);
        }
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px};

    pub(crate) fn whole(img: &ImageData) -> Rect {
        Rect::new(0, 0, img.width as i32, img.height as i32)
    }

    /// The bounding box of the pixels with any alpha (upstream's `content_bounds`).
    pub(crate) fn content(img: &ImageData) -> Rect {
        let mut r = Rect::EMPTY;
        for (i, p) in img.data.as_chunks::<4>().0.iter().enumerate() {
            if p[3] > 0 {
                let (x, y) = (
                    (i % img.width as usize) as i32,
                    (i / img.width as usize) as i32,
                );
                r = r.union(&Rect::new(x, y, x + 1, y + 1));
            }
        }
        r
    }

    /// Upstream's `sample` surface at 8 bits: a 40×24 pattern at (5, 3) in a 60×40 layer.
    fn sample() -> ImageData {
        from_fn(60, 40, |x, y| {
            if !(5..45).contains(&x) || !(3..27).contains(&y) {
                return [0; 4];
            }
            let (x, y) = (x - 5, y - 3);
            let v = ((x * 7 + y * 13) % 17) as f32 / 16.0;
            let q = |f: f32| (f * 255.0 + 0.5) as u8;
            [q(v), q(1.0 - v), q(x as f32 / 40.0), 255]
        })
    }

    #[test]
    fn a_thin_layer_stretched_into_a_curve_keeps_the_curve() {
        // #5793: a 3 px wide column stretched 100x and bent into an arch. The grid was sized in
        // source pixels (one cell across), so the arch came out as a straight band.
        let src = from_fn(310, 100, |x, y| {
            if x < 3 && y < 40 {
                [255, 0, 0, 255]
            } else {
                [0; 4]
            }
        });
        let arch = |x: f64, y: f64| {
            let dx = x * 100.0;
            let t = (dx - 150.0) / 150.0;
            (dx, y + 40.0 * (1.0 - t * t))
        };
        let out = warp_mesh(&src, &arch, whole(&src), Interp::Bilinear, Interrupt::NONE).unwrap();
        let alpha = |x: u32, y: u32| px(&out, x, y)[3];
        // The band follows the arch: 40 px lower in the middle, about 36 px a third of the way in.
        assert!(
            alpha(150, 60) > 252 && alpha(150, 20) < 3,
            "mid {} {}",
            alpha(150, 60),
            alpha(150, 20)
        );
        assert!(
            alpha(100, 55) > 252 && alpha(100, 20) < 3,
            "third {} {}",
            alpha(100, 55),
            alpha(100, 20)
        );
    }

    #[test]
    fn the_interruptible_mesh_warp_stops_when_cancelled() {
        let s = from_fn(300, 200, |_, _| [51, 102, 153, 255]);
        let map = |x: f64, y: f64| (x + 0.3 * (y / 40.0).sin(), y + 0.2 * x / 300.0);
        let never = std::sync::atomic::AtomicBool::new(false);
        let out = warp_mesh(
            &s,
            &map,
            whole(&s),
            Interp::Bicubic,
            Interrupt(Some(&never)),
        );
        let want = warp_mesh(&s, &map, whole(&s), Interp::Bicubic, Interrupt::NONE);
        assert_eq!(out.unwrap().data, want.unwrap().data);
        let always = std::sync::atomic::AtomicBool::new(true);
        let stopped = warp_mesh(
            &s,
            &map,
            whole(&s),
            Interp::Bicubic,
            Interrupt(Some(&always)),
        );
        assert!(matches!(stopped, Err(Cancelled)));
    }

    /// R71: the cancel is polled per row inside one triangle's fill and the sampling, not once
    /// per triangle or tile; a poll that says stop partway through ends the warp.
    #[test]
    fn a_cancel_is_polled_per_row_inside_one_triangle() {
        let src = from_fn(100, 100, |x, y| [(x * 2) as u8, (y * 2) as u8, 7, 255]);
        // One triangle over the whole 100×100 clip (a single tile).
        let verts = [
            ([-10.0, -10.0], [-10.0, -10.0]),
            ([250.0, -10.0], [250.0, -10.0]),
            ([-10.0, 250.0], [-10.0, 250.0]),
        ];
        let polls = std::sync::atomic::AtomicUsize::new(0);
        let count = || {
            polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            false
        };
        let clip = whole(&src);
        let full = triangles(&src, &verts, &[[0, 1, 2]], clip, Interp::Bicubic, &count).unwrap();
        assert_eq!(full.data, src.data, "an identity triangle copies the layer");
        // 100 rows of fill and 100 rows of sampling, besides the per-tile polls.
        assert!(polls.into_inner() >= 200);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let stop_at_30 = || calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 30;
        let stopped = triangles(
            &src,
            &verts,
            &[[0, 1, 2]],
            clip,
            Interp::Bicubic,
            &stop_at_30,
        );
        assert!(stopped.is_none());
        assert!(
            calls.into_inner() <= 32,
            "it stopped at the first poll that said so"
        );
    }

    /// R71: a triangle with a non-finite or far vertex is skipped on its own (its pixel
    /// arithmetic would overflow); the rest still draw.
    #[test]
    fn a_hostile_vertex_skips_only_its_triangle() {
        let src = from_fn(20, 20, |_, _| [200, 100, 50, 255]);
        let good = [
            ([0.0, 0.0], [0.0, 0.0]),
            ([20.0, 0.0], [20.0, 0.0]),
            ([0.0, 20.0], [0.0, 20.0]),
        ];
        for bad in [f64::NAN, f64::INFINITY, 1e30, -3e9] {
            let mut verts = good.to_vec();
            verts.push(([20.0, 20.0], [bad, 5.0]));
            verts.push(([bad, 20.0], [20.0, 20.0]));
            let tris = [[0, 1, 2], [1, 3, 2], [1, 4, 2]];
            let out = warp_triangles(
                &src,
                &verts,
                &tris,
                whole(&src),
                Interp::Bicubic,
                Interrupt::NONE,
            )
            .unwrap();
            assert_eq!(px(&out, 2, 2), [200, 100, 50, 255], "{bad}: the good one");
            assert_eq!(px(&out, 18, 18), [0; 4], "{bad}: the bad ones are skipped");
        }
    }

    /// R71: Keys' cubic at a = −0.75 (Catmull-Rom's −0.5 gives −0.0625 and 0.5625 here).
    #[test]
    fn bicubic_is_keys_at_minus_three_quarters() {
        assert_eq!(bicubic(0.5), [-0.09375, 0.59375, 0.59375, -0.09375]);
        assert_eq!(bicubic(0.0), [0.0, 1.0, 0.0, 0.0]);
    }

    /// Upstream's `identity_warp_is_a_no_op_at_every_depth` at paint.rs's one depth, exact.
    #[test]
    fn identity_warp_is_a_no_op() {
        let src = sample();
        for interp in [Interp::Bilinear, Interp::Bicubic] {
            let out = warp_mesh(&src, &|x, y| (x, y), whole(&src), interp, Interrupt::NONE);
            assert_eq!(out.unwrap().data, src.data, "{interp:?}");
        }
    }

    #[test]
    fn a_rotated_soft_edged_sprite_keeps_its_colour() {
        // #6203, as in Free Transform: the bicubic alpha overshoots past 1 next to the half
        // transparent edge, and must not lighten the colour.
        let src = from_fn(40, 40, |x, y| {
            if (11..29).contains(&x) && (11..29).contains(&y) {
                [204, 153, 102, 255]
            } else if (10..30).contains(&x) && (10..30).contains(&y) {
                [204, 153, 102, 128]
            } else {
                [0; 4]
            }
        });
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let rotate = |x: f64, y: f64| {
            (
                20.0 + (x - 20.0) * k - (y - 20.0) * k,
                20.0 + (x - 20.0) * k + (y - 20.0) * k,
            )
        };
        let out = warp_mesh(&src, &rotate, whole(&src), Interp::Bicubic, Interrupt::NONE).unwrap();
        let mut seen = 0;
        for p in out.data.as_chunks::<4>().0.iter().filter(|p| p[3] > 0) {
            seen += 1;
            let off = (0..3)
                .map(|i| p[i].abs_diff([204, 153, 102][i]))
                .max()
                .unwrap();
            assert!(off <= 1, "{p:?}");
        }
        assert!(seen > 400, "{seen}");
    }

    /// Upstream's `translation_and_known_arc_points` without the Arc preset (no Bezier warps).
    #[test]
    fn translation_moves_every_pixel() {
        let src = sample();
        let out = warp_mesh(
            &src,
            &|x, y| (x + 10.0, y - 2.0),
            whole(&src),
            Interp::Bilinear,
            Interrupt::NONE,
        )
        .unwrap();
        assert_eq!(content(&out), content(&src).translate(10, -2));
        assert_eq!(px(&out, 20, 10), px(&src, 10, 12));
    }

    #[test]
    fn a_shrinking_mesh_warp_is_prefiltered() {
        // A warp to ~1/5 size: a grid of 1 px lines every 4 px must come out its area average
        // (9/16 white) everywhere, not point-sampled moiré (crinkled, doubled or missing lines).
        // Upstream bends it with a 15 % Arc preset; a gentle sine stands in (no Bezier warps).
        let (w, h) = (400, 320);
        let src = from_fn(w, h, |x, y| {
            let v = if x % 4 == 0 || y % 4 == 0 { 0 } else { 255 };
            [v, v, v, 255]
        });
        let f = |x: f64, y: f64| {
            let bend = 4.0 * (x / 400.0 * std::f64::consts::PI).sin();
            (0.2 * x + 7.0, 0.2 * y + 5.0 + bend)
        };
        let out = warp_mesh(&src, &f, whole(&src), Interp::Bicubic, Interrupt::NONE).unwrap();
        let opaque: Vec<f32> = out
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .map(|p| f32::from(p[0]) / 255.0)
            .collect();
        assert!(opaque.len() > 2000, "{} opaque pixels", opaque.len());
        let worst = opaque
            .iter()
            .map(|v| (v - 0.5625).abs())
            .fold(0.0f32, f32::max);
        // An area average over the ~5 px footprint still covers one or two lines per axis (up
        // to 0.2 off); point sampling lands wholly on or off a line (up to 0.56 off).
        assert!(
            worst < 0.25,
            "worst deviation from the area average {worst}"
        );
    }

    /// The source is the whole layer; only `clip` is written, the rest is the layer as it was.
    #[test]
    fn only_the_clip_is_written() {
        let src = sample();
        let clip = Rect::new(10, 5, 30, 20);
        let out = warp_mesh(
            &src,
            &|x, y| (x + 3.0, y),
            clip,
            Interp::Bicubic,
            Interrupt::NONE,
        )
        .unwrap();
        for y in 0..40 {
            for x in 0..60 {
                let want = if clip.contains(x as i32, y as i32) {
                    px(&src, x - 3, y)
                } else {
                    px(&src, x, y)
                };
                assert_eq!(px(&out, x, y), want, "({x}, {y})");
            }
        }
    }
}
