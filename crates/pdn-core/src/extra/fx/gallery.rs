//! Ported from PhotoCraft crates/algo/src/gallery.rs and the helpers it takes from fxutil.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: the Blur Gallery's shared variable blur, Tilt-Shift, Iris, Field, Spin and Path; kernels work on a whole interleaved straight RGBA f32 layer (`w`, `h`) with the effect's `bounds` as both `out` and `ctx.bounds`; the variable blur always trims its levels (upstream's `trim_levels`; untrimmed output is identical, only slower); Spin and Path sample the whole layer, not only the bounds, and write only the bounds; Path takes polylines, the effect builds them from Start, End and Curve; `Interrupt` is polled per level and per box-blur row; the frozen-reference tests need upstream's document model and are replaced by property tests.
//!
//! Spatially varying blur interpolates between a stack of Gaussian levels (σ = 0 and a geometric
//! series up to the maximum), each computed with a radius-independent box cascade, so cost grows
//! with the number of levels, not the blur size.

use super::util::{gauss_box_radii, smoothstep};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par;

const N: usize = 4;
const LEVELS: usize = 6;

/// Gaussian σ for a gallery blur amount in px.
fn sigma_of(blur: f32) -> f32 {
    blur.clamp(0.0, 500.0) * 0.6
}

/// In-place running-sum box blur of every row of a `w × h × 4` buffer with radius `r` (edges
/// clamped), rows in parallel. f64 sums: rounding must not depend on where a window starts.
/// `false` once cancelled.
fn box_rows(buf: &mut [f32], w: usize, r: usize, ctl: &Interrupt) -> bool {
    if r == 0 || w == 0 {
        return true;
    }
    let norm = 1.0 / (2 * r + 1) as f64;
    par::rows_mut(buf, w * N, |_, row| {
        if ctl.cancelled() {
            return;
        }
        let scratch = row.to_vec();
        let at = |x: isize, c: usize| scratch[(x.clamp(0, w as isize - 1) as usize) * N + c] as f64;
        for c in 0..N {
            let mut acc: f64 = (-(r as isize)..=r as isize).map(|x| at(x, c)).sum();
            for x in 0..w {
                row[x * N + c] = (acc * norm) as f32;
                acc += at(x as isize + r as isize + 1, c) - at(x as isize - r as isize, c);
            }
        }
    });
    !ctl.cancelled()
}

/// [`box_rows`] over columns (no transpose; walks with a stride). Serial: a parallel column pass needs a
/// transpose (or a shared mutable buffer), which the row pass does not.
fn box_cols(
    buf: &mut [f32],
    w: usize,
    h: usize,
    r: usize,
    scratch: &mut Vec<f32>,
    ctl: &Interrupt,
) -> bool {
    if r == 0 || h == 0 {
        return true;
    }
    let norm = 1.0 / (2 * r + 1) as f64;
    for x in 0..w {
        if ctl.cancelled() {
            return false;
        }
        scratch.clear();
        for y in 0..h {
            scratch.extend_from_slice(&buf[(y * w + x) * N..(y * w + x + 1) * N]);
        }
        let at = |y: isize, c: usize| scratch[(y.clamp(0, h as isize - 1) as usize) * N + c] as f64;
        for c in 0..N {
            let mut acc: f64 = (-(r as isize)..=r as isize).map(|y| at(y, c)).sum();
            for y in 0..h {
                buf[(y * w + x) * N + c] = (acc * norm) as f32;
                acc += at(y as isize + r as isize + 1, c) - at(y as isize - r as isize, c);
            }
        }
    }
    true
}

/// Approximate Gaussian blur (three box passes each way) of a `w × h × 4` buffer; cost does not
/// depend on `sigma`. `false` once cancelled.
fn gauss_blur(buf: &mut [f32], w: usize, h: usize, sigma: f32, ctl: &Interrupt) -> bool {
    if sigma < 0.3 {
        return true;
    }
    let mut scratch = Vec::with_capacity(w.max(h) * N);
    let radii = gauss_box_radii(sigma);
    radii.iter().all(|&r| box_rows(buf, w, r, ctl))
        && radii
            .iter()
            .all(|&r| box_cols(buf, w, h, r, &mut scratch, ctl))
}

/// Blurs the `w × h` layer over `out` (inside the layer) with a per-pixel σ (`sigma(x, y)`, at
/// most `smax`), levels trimmed to what `out` reads. The whole layer back with `out` replaced;
/// `None` once cancelled.
fn variable_blur(
    buf: &[f32],
    (w, h): (usize, usize),
    out: Rect,
    smax: f32,
    sigma: impl Fn(f32, f32) -> f32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let win = Rect::new(0, 0, w as i32, h as i32);
    let out = out.intersect(&win);
    if out.is_empty() || smax < 0.3 {
        return Some(buf.to_vec());
    }
    let (ow, oh) = (out.width() as usize, out.height() as usize);
    let pixels = ow * oh;
    let xy = |i: usize| (out.x0 + (i % ow) as i32, out.y0 + (i / ow) as i32);
    // Level σs: 0, then smax / 2^(LEVELS-1) … smax.
    let lv: Vec<f32> = std::iter::once(0.0)
        .chain((0..LEVELS).map(|k| smax / 2f32.powi((LEVELS - 1 - k) as i32)))
        .collect();
    let mut sig = Vec::with_capacity(pixels);
    for i in 0..pixels {
        if i % ow == 0 && ctl.cancelled() {
            return None;
        }
        let (x, y) = xy(i);
        sig.push(sigma(x as f32 + 0.5, y as f32 + 0.5).clamp(0.0, smax));
    }
    // Hat-function weight of level k for σ s.
    let weight = |k: usize, s: f32| -> f32 {
        let c = lv[k];
        if k > 0 && s >= lv[k - 1] && s <= c {
            (s - lv[k - 1]) / (c - lv[k - 1]).max(1e-6)
        } else if k + 1 < lv.len() && s >= c && s <= lv[k + 1] {
            (lv[k + 1] - s) / (lv[k + 1] - c).max(1e-6)
        } else if k + 1 == lv.len() && s >= c {
            1.0
        } else {
            0.0
        }
    };
    let mut p = buf.to_vec();
    for px in p.as_chunks_mut::<4>().0 {
        let a = px[3];
        px[..3].iter_mut().for_each(|v| *v *= a);
    }
    let mut acc = vec![0.0f32; pixels * N];
    let mut level_buf: Vec<f32> = Vec::new();
    for (k, &lvk) in lv.iter().enumerate() {
        if ctl.cancelled() {
            return None;
        }
        if !sig.iter().any(|&s| weight(k, s) > 0.0) {
            continue;
        }
        let (level, lw): (&[f32], usize) = if k == 0 {
            (&p, w)
        } else {
            // Each level needs its own box-cascade reach, not the maximum blur's trailing halo.
            // The original top/left is kept: moving the running sums' start changes
            // floating-point cancellation.
            let reach = if lvk < 0.3 {
                0
            } else {
                gauss_box_radii(lvk).iter().sum::<usize>()
            };
            let reach = i32::try_from(reach).unwrap_or(i32::MAX);
            let lw = out.x1.saturating_add(reach).min(win.x1) as usize;
            let lh = out.y1.saturating_add(reach).min(win.y1) as usize;
            level_buf.clear();
            for row in p.chunks_exact(w * N).take(lh) {
                level_buf.extend_from_slice(&row[..lw * N]);
            }
            if !gauss_blur(&mut level_buf, lw, lh, lvk, ctl) {
                return None;
            }
            (&level_buf, lw)
        };
        for (i, &s) in sig.iter().enumerate() {
            let wk = weight(k, s);
            if wk <= 0.0 {
                continue;
            }
            let (x, y) = xy(i);
            let o = (y as usize * lw + x as usize) * N;
            for (dst, px) in acc[i * N..(i + 1) * N].iter_mut().zip(&level[o..o + N]) {
                *dst += px * wk;
            }
        }
    }
    let mut res = buf.to_vec();
    for (i, px) in acc.as_chunks_mut::<N>().0.iter_mut().enumerate() {
        let a = px[3];
        for v in &mut px[..3] {
            *v = if a > 1e-7 { *v / a } else { 0.0 };
        }
        let (x, y) = xy(i);
        let at = (y as usize * w + x as usize) * N;
        res[at..at + N].copy_from_slice(px);
    }
    Some(res)
}

fn short_side(b: Rect) -> f32 {
    (b.width().min(b.height()) as f32).max(1.0)
}

/// Tilt-Shift's σ at distance `d` from the band's centre line (fractions of the shorter side):
/// 0 within `focus`, `smax` beyond `focus + transition`, smooth between.
fn tilt_sigma(d: f32, smax: f32, focus: f32, transition: f32) -> f32 {
    let f0 = focus.max(0.0);
    smax * smoothstep(f0, f0 + transition.max(1e-3), d)
}

/// Tilt-Shift over `b`: a sharp band through the centre (fractions of `b`) at `angle`, blurring
/// with distance beyond `focus` over `transition` (both fractions of the shorter side).
#[allow(clippy::too_many_arguments)]
pub fn tilt_shift(
    buf: &[f32],
    dims: (usize, usize),
    b: Rect,
    blur: f32,
    centre: (f32, f32),
    angle: f32,
    focus: f32,
    transition: f32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let ss = short_side(b);
    let (cx, cy) = (
        b.x0 as f32 + centre.0 * b.width() as f32,
        b.y0 as f32 + centre.1 * b.height() as f32,
    );
    // Band normal (the band runs along `angle`).
    let (s, c) = angle.to_radians().sin_cos();
    let (nx, ny) = (s, c);
    // A NaN amount must not become the upper bound of f32::clamp below.
    let smax = sigma_of(blur).max(0.0);
    variable_blur(
        buf,
        dims,
        b,
        smax,
        |x, y| {
            tilt_sigma(
                ((x - cx) * nx + (y - cy) * ny).abs() / ss,
                smax,
                focus,
                transition,
            )
        },
        ctl,
    )
}

/// One Iris Blur pin: centre and radii as fractions of the bounds (radii of the shorter side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IrisPin {
    pub x: f32,
    pub y: f32,
    pub radius_x: f32,
    pub radius_y: f32,
    pub angle: f32,
    pub feather: f32,
    pub roundness: f32,
    pub blur: f32,
}

impl Default for IrisPin {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            radius_x: 0.35,
            radius_y: 0.25,
            angle: 0.0,
            feather: 0.5,
            roundness: 0.0,
            blur: 15.0,
        }
    }
}

/// Iris Blur over `b`: everything outside each pin's ellipse is blurred; the sharp core is
/// `feather` of the ellipse. Sharpness from any pin wins.
pub fn iris(
    buf: &[f32],
    dims: (usize, usize),
    b: Rect,
    pins: &[IrisPin],
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let smax = pins.iter().map(|p| sigma_of(p.blur)).fold(0.0, f32::max);
    if pins.is_empty() || smax < 0.3 {
        return Some(buf.to_vec());
    }
    let prepared: Vec<_> = pins.iter().map(|p| PreparedIris::new(p, b)).collect();
    variable_blur(
        buf,
        dims,
        b,
        smax,
        |x, y| {
            prepared
                .iter()
                .map(|p| p.sigma(x, y))
                .fold(f32::MAX, f32::min)
        },
        ctl,
    )
}

/// A finite value, else `default` (a NaN pin must not poison every pixel).
fn fin(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}

/// Field Blur pin: a blur amount at a point (fractions of the bounds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldPin {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
}

impl Default for FieldPin {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            blur: 15.0,
        }
    }
}

/// Field Blur over `b`: blur amounts at pins, interpolated by inverse squared distance. Levels
/// are trimmed to what the bounds read.
pub fn field(
    buf: &[f32],
    dims: (usize, usize),
    b: Rect,
    pins: &[FieldPin],
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let sigma = |p: &FieldPin| sigma_of(fin(p.blur, 0.0)).max(0.0);
    let smax = pins.iter().map(sigma).fold(0.0, f32::max);
    if pins.is_empty() || smax < 0.3 {
        return Some(buf.to_vec());
    }
    let pts: Vec<(f32, f32, f32)> = pins
        .iter()
        .map(|p| {
            (
                b.x0 as f32 + fin(p.x, 0.5) * b.width() as f32,
                b.y0 as f32 + fin(p.y, 0.5) * b.height() as f32,
                sigma(p),
            )
        })
        .collect();
    variable_blur(
        buf,
        dims,
        b,
        smax,
        |x, y| {
            let (mut ws, mut s) = (0.0f32, 0.0f32);
            for &(px, py, sg) in &pts {
                let w = 1.0 / ((x - px).powi(2) + (y - py).powi(2) + 1.0);
                ws += w;
                s += w * sg;
            }
            s / ws
        },
        ctl,
    )
}

/// Spin Blur pin: an ellipse whose content is rotated about its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinPin {
    pub x: f32,
    pub y: f32,
    pub radius_x: f32,
    pub radius_y: f32,
    pub angle: f32,
    /// Blur arc in degrees.
    pub blur_angle: f32,
}

impl Default for SpinPin {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            radius_x: 0.3,
            radius_y: 0.3,
            angle: 0.0,
            blur_angle: 15.0,
        }
    }
}

/// Normalized elliptical distance of a point from a spin pin, with the normalized offset
/// `(u, v)` in the ellipse's frame; infinite when not finite.
fn pin_distance(x: f32, y: f32, b: Rect, p: &SpinPin) -> (f32, f32, f32) {
    let ss = short_side(b);
    let (cx, cy) = (
        b.x0 as f32 + fin(p.x, 0.5) * b.width() as f32,
        b.y0 as f32 + fin(p.y, 0.5) * b.height() as f32,
    );
    let (s, c) = fin(p.angle, 0.0).to_radians().sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    let (u, v) = (dx * c + dy * s, -dx * s + dy * c);
    let (u, v) = (
        u / (fin(p.radius_x, 0.3).max(1e-3) * ss),
        v / (fin(p.radius_y, 0.3).max(1e-3) * ss),
    );
    let e = (u * u + v * v).sqrt();
    if e.is_finite() {
        (e, u, v)
    } else {
        (f32::INFINITY, 0.0, 0.0)
    }
}

/// Bilinear sample of the straight RGBA layer at `(x, y)` (pixel centres at +0.5, edges
/// clamped), added to `acc` as premultiplied colour and alpha.
fn add_sample_premul(buf: &[f32], (w, h): (usize, usize), x: f32, y: f32, acc: &mut [f32; N]) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    let fx = (x - 0.5).clamp(0.0, (w - 1) as f32);
    let fy = (y - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
    for (xx, yy, k) in [
        (x0, y0, (1.0 - ax) * (1.0 - ay)),
        (x1, y0, ax * (1.0 - ay)),
        (x0, y1, (1.0 - ax) * ay),
        (x1, y1, ax * ay),
    ] {
        if k <= 0.0 {
            continue;
        }
        let px = &buf[(yy * w + xx) * N..][..N];
        let a = px[3] * k;
        for c in 0..3 {
            acc[c] += px[c] * a;
        }
        acc[3] += a;
    }
}

/// Straight colour from a premultiplied sum.
fn unpremul(acc: &mut [f32; N]) {
    let a = acc[3];
    for v in &mut acc[..3] {
        *v = if a > 1e-7 { *v / a } else { 0.0 };
    }
}

/// Runs `pixel(x, y, px)` for every pixel of `b` ∩ the layer, rows in parallel, on a copy of
/// the layer whose other pixels stay. `None` once cancelled.
fn per_pixel(
    buf: &[f32],
    (w, h): (usize, usize),
    b: Rect,
    ctl: &Interrupt,
    pixel: impl Fn(i32, i32, &mut [f32]) + Sync + Send,
) -> Option<Vec<f32>> {
    let out = b.intersect(&Rect::new(0, 0, w as i32, h as i32));
    let mut res = buf.to_vec();
    if out.is_empty() {
        return Some(res);
    }
    par::rows_mut(&mut res, w * N, |y, row| {
        if (y as i32) < out.y0 || y as i32 >= out.y1 || ctl.cancelled() {
            return;
        }
        for x in out.x0..out.x1 {
            pixel(x, y as i32, &mut row[x as usize * N..][..N]);
        }
    });
    (!ctl.cancelled()).then_some(res)
}

/// Spin Blur over `b`: content inside each ellipse is blurred along arcs about its centre
/// (`blur_angle` degrees of rotation), feathered at the rim. The first pin containing a pixel
/// wins. Samples come from the whole layer.
pub fn spin(
    buf: &[f32],
    dims: (usize, usize),
    b: Rect,
    pins: &[SpinPin],
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let ss = short_side(b);
    per_pixel(buf, dims, b, ctl, |x, y, px| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        for p in pins {
            let (e, u, v) = pin_distance(fx, fy, b, p);
            if !e.is_finite() || e >= 1.0 {
                continue;
            }
            let theta = fin(p.blur_angle, 0.0).clamp(0.0, 360.0).to_radians();
            let (rx, ry) = (
                fin(p.radius_x, 0.3).max(1e-3),
                fin(p.radius_y, 0.3).max(1e-3),
            );
            let rpx = (e * rx.max(ry) * ss).max(0.0);
            // ~2 px between samples along the arc is smooth enough for a blur and halves the
            // cost.
            let m = ((theta * rpx / 2.0).ceil() as usize).clamp(2, 64);
            let (cx, cy) = (
                b.x0 as f32 + fin(p.x, 0.5) * b.width() as f32,
                b.y0 as f32 + fin(p.y, 0.5) * b.height() as f32,
            );
            let (sa, ca) = fin(p.angle, 0.0).to_radians().sin_cos();
            let mut acc = [0.0f32; N];
            for j in 0..m {
                let phi = theta * (j as f32 / (m - 1) as f32 - 0.5);
                let (sp, cp) = phi.sin_cos();
                // Rotate in the ellipse's normalized space, then map back.
                let (ru, rv) = (u * cp - v * sp, u * sp + v * cp);
                let (lu, lv) = (ru * rx * ss, rv * ry * ss);
                let (sx, sy) = (cx + lu * ca - lv * sa, cy + lu * sa + lv * ca);
                add_sample_premul(buf, dims, sx, sy, &mut acc);
            }
            acc.iter_mut().for_each(|v| *v /= m as f32);
            unpremul(&mut acc);
            let k = 1.0 - smoothstep(0.85, 1.0, e);
            for c in 0..N {
                px[c] += (acc[c] - px[c]) * k;
            }
            break;
        }
    })
}

/// A blur path: a polyline (fractions of the bounds), its blur length in px and taper.
#[derive(Clone, Debug, PartialEq)]
pub struct BlurPath {
    pub points: Vec<[f32; 2]>,
    pub speed: f32,
    /// 0-100: how much the blur fades toward the path's end.
    pub taper: f32,
}

impl Default for BlurPath {
    fn default() -> Self {
        Self {
            points: vec![[0.2, 0.5], [0.8, 0.5]],
            speed: 50.0,
            taper: 0.0,
        }
    }
}

/// One path segment in layer space: from, to, path index, and the path-length fractions at its
/// ends.
struct Seg {
    a: (f32, f32),
    b: (f32, f32),
    path: usize,
    t0: f32,
    t1: f32,
}

/// Path Blur over `b`: motion blur whose direction follows the nearest paths (inverse-distance
/// weighted tangents) and whose length is the path speed, optionally tapering toward each path's
/// end. Samples come from the whole layer.
pub fn path(
    buf: &[f32],
    dims: (usize, usize),
    b: Rect,
    paths: &[BlurPath],
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let (bw, bh) = (b.width() as f32, b.height() as f32);
    let mut segs = Vec::new();
    for (pi, p) in paths.iter().enumerate() {
        let pts: Vec<(f32, f32)> = p
            .points
            .iter()
            .map(|q| {
                (
                    b.x0 as f32 + fin(q[0], 0.5) * bw,
                    b.y0 as f32 + fin(q[1], 0.5) * bh,
                )
            })
            .collect();
        let total: f32 = pts
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
            .sum::<f32>()
            .max(1e-3);
        let mut run = 0.0;
        for w in pts.windows(2) {
            let l = (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1);
            segs.push(Seg {
                a: w[0],
                b: w[1],
                path: pi,
                t0: run / total,
                t1: (run + l) / total,
            });
            run += l;
        }
    }
    if segs.is_empty() {
        return Some(buf.to_vec());
    }
    per_pixel(buf, dims, b, ctl, |x, y, px| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (mut dx, mut dy, mut len, mut ws) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for s in &segs {
            let (sx, sy) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let l2 = (sx * sx + sy * sy).max(1e-6);
            let t = (((fx - s.a.0) * sx + (fy - s.a.1) * sy) / l2).clamp(0.0, 1.0);
            let d2 = (fx - s.a.0 - sx * t).powi(2) + (fy - s.a.1 - sy * t).powi(2);
            let w = 1.0 / (d2 + 1.0);
            let l = l2.sqrt();
            let p = &paths[s.path];
            let along = s.t0 + (s.t1 - s.t0) * t;
            dx += w * sx / l;
            dy += w * sy / l;
            len += w
                * fin(p.speed, 0.0).abs().min(500.0)
                * (1.0 - fin(p.taper, 0.0).clamp(0.0, 100.0) / 100.0 * along);
            ws += w;
        }
        let m = (dx * dx + dy * dy).sqrt();
        let len = len / ws;
        if m < 1e-6 || len < 0.5 {
            return;
        }
        let (ux, uy) = (dx / m, dy / m);
        // Samples ~1.5 px apart (bilinear fills the gaps), capped for very long blurs.
        let steps = ((len / 1.5).ceil() as usize + 1).clamp(2, 64);
        let mut acc = [0.0f32; N];
        for j in 0..steps {
            let o = len * (j as f32 / (steps - 1) as f32 - 0.5);
            add_sample_premul(buf, dims, fx + ux * o, fy + uy * o, &mut acc);
        }
        acc.iter_mut().for_each(|v| *v /= steps as f32);
        unpremul(&mut acc);
        px.copy_from_slice(&acc);
    })
}

struct PreparedIris {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    sin: f32,
    cos: f32,
    exponent: f32,
    feather: f32,
    blur: f32,
}

impl PreparedIris {
    fn new(p: &IrisPin, b: Rect) -> Self {
        let ss = short_side(b);
        let angle = if p.angle.is_finite() { p.angle } else { 0.0 };
        let (sin, cos) = angle.to_radians().sin_cos();
        Self {
            cx: b.x0 as f32 + p.x * b.width() as f32,
            cy: b.y0 as f32 + p.y * b.height() as f32,
            rx: p.radius_x.max(1e-3) * ss,
            ry: p.radius_y.max(1e-3) * ss,
            sin,
            cos,
            exponent: 2.0 + p.roundness.clamp(0.0, 100.0) / 100.0 * 6.0,
            feather: p.feather.clamp(0.0, 0.999),
            blur: sigma_of(p.blur),
        }
    }

    fn sigma(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let (u, v) = (
            (dx * self.cos + dy * self.sin) / self.rx,
            (-dx * self.sin + dy * self.cos) / self.ry,
        );
        // Outside the normalized bounding square, the superellipse distance is at least one.
        // The falloff is saturated: no sqrt/powf is needed.
        let e = if u.abs() >= 1.0 || v.abs() >= 1.0 {
            f32::INFINITY
        } else if (self.exponent - 2.0).abs() < 1e-3 {
            (u * u + v * v).sqrt()
        } else {
            (u.abs().powf(self.exponent) + v.abs().powf(self.exponent)).powf(1.0 / self.exponent)
        };
        let e = if e.is_finite() { e } else { f32::INFINITY };
        self.blur * smoothstep(self.feather, 1.0, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: &Interrupt<'static> = &Interrupt::NONE;

    /// A hard-edged checker in every channel: any blur changes it.
    fn checker(w: usize, h: usize) -> Vec<f32> {
        (0..w * h)
            .flat_map(|i| {
                let v = ((i % w + i / w) % 2) as f32;
                [v, v, v, 1.0]
            })
            .collect()
    }

    fn px(buf: &[f32], w: usize, x: usize, y: usize) -> &[f32] {
        &buf[(y * w + x) * 4..(y * w + x) * 4 + 4]
    }

    #[test]
    fn zero_blur_is_the_identity() {
        let src = checker(32, 32);
        let b = Rect::new(0, 0, 32, 32);
        assert_eq!(
            tilt_shift(&src, (32, 32), b, 0.0, (0.5, 0.5), 0.0, 0.1, 0.15, NONE).unwrap(),
            src
        );
        let pin = IrisPin {
            blur: 0.0,
            ..IrisPin::default()
        };
        assert_eq!(iris(&src, (32, 32), b, &[pin], NONE).unwrap(), src);
        assert_eq!(iris(&src, (32, 32), b, &[], NONE).unwrap(), src);
    }

    #[test]
    fn tilt_shift_band_is_sharp_and_far_rows_blur() {
        let (w, h) = (48, 64);
        let src = checker(w, h);
        let b = Rect::new(0, 0, w as i32, h as i32);
        let out = tilt_shift(&src, (w, h), b, 30.0, (0.5, 0.5), 0.0, 0.1, 0.2, NONE).unwrap();
        // angle 0: horizontal band across the middle rows, which stay bit-identical.
        for x in 0..w {
            assert_eq!(px(&out, w, x, 32), px(&src, w, x, 32), "x {x}");
        }
        // Far rows are smoothed towards grey.
        let v = px(&out, w, 24, 2)[0];
        assert!((v - 0.5).abs() < 0.2, "{v}");
    }

    #[test]
    fn iris_centre_is_sharp_edges_blurred() {
        let (w, h) = (64, 64);
        let src = checker(w, h);
        let b = Rect::new(0, 0, w as i32, h as i32);
        let out = iris(&src, (w, h), b, &[IrisPin::default()], NONE).unwrap();
        assert_eq!(px(&out, w, 32, 32), px(&src, w, 32, 32));
        let corner = px(&out, w, 1, 1)[0];
        assert!((corner - 0.5).abs() < 0.2, "{corner}");
    }

    #[test]
    fn only_the_bounds_change() {
        let (w, h) = (40, 40);
        let src = checker(w, h);
        let b = Rect::new(10, 12, 30, 28);
        let out = tilt_shift(&src, (w, h), b, 20.0, (0.5, 0.5), 0.0, 0.0, 0.1, NONE).unwrap();
        for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
            if !b.contains(x as i32, y as i32) {
                assert_eq!(px(&out, w, x, y), px(&src, w, x, y), "{x},{y}");
            }
        }
        assert_ne!(out, src);
    }

    #[test]
    fn nonfinite_parameters_do_not_panic() {
        let src = checker(16, 16);
        let b = Rect::new(0, 0, 16, 16);
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            tilt_shift(&src, (16, 16), b, v, (v, v), v, v, v, NONE);
            for field in 0..8 {
                let mut p = IrisPin::default();
                match field {
                    0 => p.x = v,
                    1 => p.y = v,
                    2 => p.radius_x = v,
                    3 => p.radius_y = v,
                    4 => p.angle = v,
                    5 => p.feather = v,
                    6 => p.blur = v,
                    _ => p.roundness = v,
                }
                iris(&src, (16, 16), b, &[p], NONE);
            }
        }
    }

    /// R58: σ is 0 at e <= feather, blur/2 at the feather-1 midpoint, blur at e >= 1.
    #[test]
    fn iris_sigma_follows_the_feather_ramp() {
        let b = Rect::new(0, 0, 100, 100);
        let pin = IrisPin {
            x: 0.5,
            y: 0.5,
            radius_x: 0.4,
            radius_y: 0.4,
            feather: 0.5,
            blur: 50.0,
            ..IrisPin::default()
        };
        let p = PreparedIris::new(&pin, b);
        let smax = sigma_of(50.0);
        // Round pin, radius 40 px about (50, 50): e = distance / 40.
        let at = |e: f32| p.sigma(50.0 + e * 40.0, 50.0);
        assert_eq!(at(0.0), 0.0);
        assert_eq!(at(0.5), 0.0);
        assert!((at(0.75) - smax / 2.0).abs() < 1e-3, "{}", at(0.75));
        assert!((at(1.0) - smax).abs() < 1e-4);
        assert_eq!(at(1.5), smax);
        assert!(at(0.6) < at(0.7) && at(0.7) < at(0.9));
    }

    #[test]
    fn tilt_sigma_is_zero_in_focus_and_full_beyond_the_transition() {
        let smax = 30.0;
        assert_eq!(tilt_sigma(0.0, smax, 0.1, 0.2), 0.0);
        assert_eq!(tilt_sigma(0.1, smax, 0.1, 0.2), 0.0);
        assert!((tilt_sigma(0.2, smax, 0.1, 0.2) - smax / 2.0).abs() < 1e-4);
        assert_eq!(tilt_sigma(0.3, smax, 0.1, 0.2), smax);
        assert_eq!(tilt_sigma(5.0, smax, 0.1, 0.2), smax);
    }

    fn unchanged(a: &[f32], b: &[f32]) -> usize {
        let (a, b) = (a.as_chunks::<4>().0, b.as_chunks::<4>().0);
        a.iter().zip(b).filter(|(p, q)| p == q).count()
    }

    /// A larger feather or focus leaves monotonically more pixels unchanged.
    #[test]
    fn wider_sharp_zone_keeps_more_pixels() {
        let (w, h) = (48, 48);
        let src = checker(w, h);
        let b = Rect::new(0, 0, w as i32, h as i32);
        let counts: Vec<_> = [0.0, 0.3, 0.6, 0.9]
            .into_iter()
            .map(|feather| {
                let pin = IrisPin {
                    feather,
                    ..IrisPin::default()
                };
                unchanged(&iris(&src, (w, h), b, &[pin], NONE).unwrap(), &src)
            })
            .collect();
        assert!(
            counts.windows(2).all(|c| c[0] <= c[1]) && counts[0] < counts[3],
            "{counts:?}"
        );
        let counts: Vec<_> = [0.0, 0.1, 0.2, 0.4]
            .into_iter()
            .map(|focus| {
                let out = tilt_shift(&src, (w, h), b, 20.0, (0.5, 0.5), 0.0, focus, 0.1, NONE);
                unchanged(&out.unwrap(), &src)
            })
            .collect();
        assert!(
            counts.windows(2).all(|c| c[0] <= c[1]) && counts[0] < counts[3],
            "{counts:?}"
        );
    }

    /// Angle 90 turns the band vertical: the middle columns stay sharp, the left edge blurs.
    #[test]
    fn angle_90_gives_a_vertical_band() {
        let (w, h) = (48, 48);
        let src = checker(w, h);
        let b = Rect::new(0, 0, w as i32, h as i32);
        let out = tilt_shift(&src, (w, h), b, 30.0, (0.5, 0.5), 90.0, 0.1, 0.2, NONE).unwrap();
        for y in 0..h {
            assert_eq!(px(&out, w, 24, y), px(&src, w, 24, y), "y {y}");
        }
        assert!((px(&out, w, 1, 24)[0] - 0.5).abs() < 0.2);
        assert_ne!(px(&out, w, 1, 24), px(&src, w, 1, 24));
    }

    #[test]
    fn empty_bounds_return_the_layer() {
        let src = checker(16, 16);
        for b in [Rect::EMPTY, Rect::new(30, 30, 40, 40)] {
            assert_eq!(
                tilt_shift(&src, (16, 16), b, 30.0, (0.5, 0.5), 0.0, 0.1, 0.2, NONE).unwrap(),
                src
            );
            assert_eq!(
                iris(&src, (16, 16), b, &[IrisPin::default()], NONE).unwrap(),
                src
            );
        }
    }

    #[test]
    fn cancelled_returns_none() {
        let src = checker(32, 32);
        let flag = std::sync::atomic::AtomicBool::new(true);
        let b = Rect::new(0, 0, 32, 32);
        let ctl = Interrupt(Some(&flag));
        assert!(tilt_shift(&src, (32, 32), b, 30.0, (0.5, 0.5), 0.0, 0.1, 0.15, &ctl).is_none());
        assert!(iris(&src, (32, 32), b, &[IrisPin::default()], &ctl).is_none());
    }

    fn assert_close(a: &[f32], b: &[f32], tol: f32, what: &str) {
        assert_eq!(a.len(), b.len());
        for (i, (p, q)) in a.iter().zip(b).enumerate() {
            assert!((p - q).abs() <= tol, "{what}: sample {i}: {p} vs {q}");
        }
    }

    /// Blurring only `out` gives, bit for bit, what blurring the whole layer gives there:
    /// trimming the levels to what `out` reads saves work and changes nothing.
    #[test]
    fn trimmed_variable_blur_matches_the_whole_layer() {
        let (w, h) = (40, 40);
        let src = checker(w, h);
        let sigma = |x: f32, _: f32| x * 0.2;
        let whole = variable_blur(&src, (w, h), Rect::new(0, 0, 40, 40), 8.0, sigma, NONE).unwrap();
        let out = Rect::new(8, 6, 30, 26);
        let part = variable_blur(&src, (w, h), out, 8.0, sigma, NONE).unwrap();
        for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
            let want = if out.contains(x as i32, y as i32) {
                &whole
            } else {
                &src
            };
            assert_eq!(px(&part, w, x, y), px(want, w, x, y), "{x},{y}");
        }
        assert_ne!(part, src);
    }

    /// One pin is the same blur everywhere: a plain Gaussian of the whole layer, wherever the
    /// pin sits.
    #[test]
    fn field_single_pin_is_uniform_gaussian() {
        let (w, h) = (40, 32);
        let src = checker(w, h);
        let b = Rect::new(0, 0, w as i32, h as i32);
        let mut want = src.clone();
        for px in want.as_chunks_mut::<4>().0 {
            let a = px[3];
            px[..3].iter_mut().for_each(|v| *v *= a);
        }
        assert!(gauss_blur(&mut want, w, h, sigma_of(10.0), NONE));
        for (x, y) in [(0.5, 0.5), (0.0, 0.0), (1.0, 0.3)] {
            let pin = FieldPin { x, y, blur: 10.0 };
            let out = field(&src, (w, h), b, &[pin], NONE).unwrap();
            assert_close(&out, &want, 1e-4, "gaussian");
        }
        let none = FieldPin {
            blur: 0.0,
            ..FieldPin::default()
        };
        assert_eq!(field(&src, (w, h), b, &[none], NONE).unwrap(), src);
        assert_eq!(field(&src, (w, h), b, &[], NONE).unwrap(), src);
    }

    /// Inverse-distance weights: next to the sharp pin the layer stays sharp, next to the
    /// blurred one it blurs.
    #[test]
    fn field_blur_follows_the_nearest_pin() {
        let (w, h) = (64, 32);
        let src = checker(w, h);
        let b = Rect::new(0, 0, w as i32, h as i32);
        let pins = [
            FieldPin {
                x: 0.0,
                y: 0.5,
                blur: 0.0,
            },
            FieldPin {
                x: 1.0,
                y: 0.5,
                blur: 30.0,
            },
        ];
        let out = field(&src, (w, h), b, &pins, NONE).unwrap();
        let dev = |x: usize| (px(&out, w, x, 16)[0] - px(&src, w, x, 16)[0]).abs();
        assert!(dev(1) < 0.1, "{}", dev(1));
        assert!(dev(62) > 0.3, "{}", dev(62));
    }

    /// Only the bounds change, and the layer outside them is still read: blanking everything
    /// outside the bounds changes the Field, Spin and Path result inside them.
    #[test]
    fn gallery_blurs_write_only_the_bounds_and_read_outside() {
        let (w, h) = (48, 32);
        let src: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = if i % w < 16 { 1.0 } else { 0.5 };
                [v, v, v, 1.0]
            })
            .collect();
        let b = Rect::new(16, 8, 40, 24);
        let blank: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                if b.contains(x, y) {
                    src[i * 4..i * 4 + 4].to_vec()
                } else {
                    vec![0.0; 4]
                }
            })
            .collect();
        // Radii of 1.0 let the ellipse reach past the bounds.
        let spin_pin = SpinPin {
            radius_x: 1.0,
            radius_y: 1.0,
            blur_angle: 90.0,
            ..SpinPin::default()
        };
        let path = BlurPath {
            speed: 20.0,
            ..BlurPath::default()
        };
        let field_pin = [FieldPin {
            blur: 20.0,
            ..FieldPin::default()
        }];
        let run = |layer: &[f32], n: usize| match n {
            0 => field(layer, (w, h), b, &field_pin, NONE),
            1 => spin(layer, (w, h), b, &[spin_pin], NONE),
            _ => path_of(layer, (w, h), b, &path),
        };
        for n in 0..3 {
            let (out, other) = (run(&src, n).unwrap(), run(&blank, n).unwrap());
            for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
                if !b.contains(x as i32, y as i32) {
                    assert_eq!(px(&out, w, x, y), px(&src, w, x, y), "{n}: {x},{y}");
                }
            }
            assert_ne!(out, src, "{n}");
            let differs = (b.y0..b.y1)
                .flat_map(|y| (b.x0..b.x1).map(move |x| (x as usize, y as usize)))
                .any(|(x, y)| px(&out, w, x, y) != px(&other, w, x, y));
            assert!(differs, "{n} did not read outside the bounds");
        }
    }

    fn path_of(src: &[f32], dims: (usize, usize), b: Rect, p: &BlurPath) -> Option<Vec<f32>> {
        path(src, dims, b, std::slice::from_ref(p), NONE)
    }

    #[test]
    fn spin_centre_is_sharp_and_rim_blurs() {
        let (w, h) = (64, 64);
        let src: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = (((i % w + 4) / 8 + (i / w + 4) / 8) % 2) as f32;
                [v, v, v, 1.0]
            })
            .collect();
        let b = Rect::new(0, 0, w as i32, h as i32);
        let pin = SpinPin {
            radius_x: 0.45,
            radius_y: 0.45,
            blur_angle: 90.0,
            ..SpinPin::default()
        };
        let out = spin(&src, (w, h), b, &[pin], NONE).unwrap();
        let dev = |x: usize, y: usize| (px(&out, w, x, y)[0] - px(&src, w, x, y)[0]).abs();
        assert!(dev(32, 32) < dev(44, 33), "{} {}", dev(32, 32), dev(44, 33));
        // Outside the ellipse the layer is untouched.
        assert_eq!(px(&out, w, 1, 1), px(&src, w, 1, 1));
        // A zero arc is the identity.
        let flat = SpinPin {
            blur_angle: 0.0,
            ..pin
        };
        assert_eq!(spin(&src, (w, h), b, &[flat], NONE).unwrap(), src);
    }

    #[test]
    fn path_zero_speed_is_identity_and_taper_fades_the_blur() {
        let (w, h) = (64, 16);
        let src: Vec<f32> = (0..w * h)
            .flat_map(|i| {
                let v = (i % w % 2) as f32;
                [v, v, v, 1.0]
            })
            .collect();
        let b = Rect::new(0, 0, w as i32, h as i32);
        let still = BlurPath {
            speed: 0.0,
            ..BlurPath::default()
        };
        assert_eq!(path_of(&src, (w, h), b, &still).unwrap(), src);
        let none = BlurPath {
            points: vec![],
            ..BlurPath::default()
        };
        assert_eq!(path_of(&src, (w, h), b, &none).unwrap(), src);
        // Full taper: the blur is longest at the path's start and gone at its end.
        let p = BlurPath {
            points: vec![[0.0, 0.5], [1.0, 0.5]],
            speed: 10.0,
            taper: 100.0,
        };
        let out = path_of(&src, (w, h), b, &p).unwrap();
        let dev = |x: usize| (px(&out, w, x, 8)[0] - px(&src, w, x, 8)[0]).abs();
        assert!(dev(6) > 0.3, "{}", dev(6));
        assert!(dev(62) < 0.05, "{}", dev(62));
    }

    #[test]
    fn nonfinite_gallery_parameters_do_not_panic() {
        let src = checker(16, 16);
        let b = Rect::new(0, 0, 16, 16);
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let f = FieldPin {
                x: v,
                y: v,
                blur: v,
            };
            let out = field(&src, (16, 16), b, &[f], NONE).unwrap();
            assert!(out.iter().all(|c| c.is_finite()));
            let s = SpinPin {
                x: v,
                y: v,
                radius_x: v,
                radius_y: v,
                angle: v,
                blur_angle: v,
            };
            assert!(spin(&src, (16, 16), b, &[s], NONE).is_some());
            let p = BlurPath {
                points: vec![[v, v], [v, 0.5]],
                speed: v,
                taper: v,
            };
            assert!(path_of(&src, (16, 16), b, &p).is_some());
        }
    }

    #[test]
    fn gallery_blurs_cancel_to_none() {
        let src = checker(32, 32);
        let flag = std::sync::atomic::AtomicBool::new(true);
        let ctl = Interrupt(Some(&flag));
        let b = Rect::new(0, 0, 32, 32);
        assert!(field(&src, (32, 32), b, &[FieldPin::default()], &ctl).is_none());
        assert!(spin(&src, (32, 32), b, &[SpinPin::default()], &ctl).is_none());
        assert!(path(&src, (32, 32), b, &[BlurPath::default()], &ctl).is_none());
    }
}
