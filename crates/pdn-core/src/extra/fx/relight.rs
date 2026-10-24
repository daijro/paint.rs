//! Ported from PhotoCraft crates/algo/src/relight.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: interleaved straight RGBA f32 buffers (w×h, alpha last) instead of `Image`/`Ctx`, so the colour-model, depth and `ctx.alpha` branches drop; the shading blur is `fx::tone::gauss_blur` run on the luma and on its validity weights (the two channels upstream packs together blur independently, so the result is the same); reads outside the layer repeat the edge pixel; the lamp pass runs through `extra::par`; `Interrupt` is polled per luma row, per output row and per band; the halo helpers and the engine-level tests drop out, the kernel tests are rewritten on RGBA buffers.
//!
//! Photographic relight: recover the shading with a blur (Retinex) and replace it with a
//! Lambertian lamp whose direction comes from angle / elevation.

use crate::extra::fx::tone::gauss_blur;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par;

/// Softness maps onto this σ range.
const SIGMA_MIN: f32 = 2.0;
const SIGMA_MAX: f32 = 96.0;
/// Finite-difference step is `σ`, but never more than this many pixels.
const STEP_MAX: f32 = 32.0;
/// Luma field allocated for one band; larger `out` rects are split into overlapping bands.
const MAX_LUMA_PIXELS: usize = 8_000_000;

/// Dialog units (degrees / 0–100), matching the command params.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub angle: f32,
    pub elevation: f32,
    pub intensity: f32,
    pub ambient: f32,
    pub warmth: f32,
    pub softness: f32,
}

/// Blur σ from softness and the reference bounds, clamped to `[SIGMA_MIN, SIGMA_MAX]`.
fn sigma(softness: f32, bounds: Rect) -> f32 {
    let side = bounds.width().min(bounds.height()) as f32;
    let s = (softness.clamp(1.0, 100.0) / 100.0) * 0.08 * side.max(1.0);
    s.clamp(SIGMA_MIN, SIGMA_MAX)
}

/// Finite-difference step (pixels) at this σ.
fn step_for_sigma(sig: f32) -> i32 {
    sig.round().clamp(1.0, STEP_MAX) as i32
}

/// Pixels read outside an output rect: `3σ + 1` of the shading blur plus the finite-difference step.
fn read_radius(sig: f32) -> i32 {
    ((sig.max(0.0) * 3.0).ceil() as i32 + 1).saturating_add(step_for_sigma(sig))
}

/// Infinite-light direction: 0° from the right, 90° from above (image space, y down).
fn light_dir(angle: f32, elevation: f32) -> [f32; 3] {
    let (s, co) = angle.to_radians().sin_cos();
    let e = elevation.clamp(0.0, 90.0).to_radians();
    norm3([co * e.cos(), -s * e.cos(), e.sin()])
}

fn norm3(v: [f32; 3]) -> [f32; 3] {
    let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !m.is_finite() || m < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / m, v[1] / m, v[2] / m]
    }
}

fn light_color(warmth: f32) -> [f32; 3] {
    let w = warmth.clamp(-100.0, 100.0) / 100.0;
    if w >= 0.0 {
        [1.0 + 0.15 * w, 1.0 - 0.05 * w, 1.0 - 0.25 * w]
    } else {
        let t = -w;
        [1.0 - 0.20 * t, 1.0 - 0.10 * t, 1.0 + 0.15 * t]
    }
}

fn rec709_y(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// A luma field over `rect`, read with the edge clamped; `NAN` marks pixels with no light to read.
struct Field {
    rect: Rect,
    w: usize,
    data: Vec<f32>,
}

impl Field {
    fn at(&self, x: i32, y: i32) -> f32 {
        let xx = x.clamp(self.rect.x0, self.rect.x1 - 1);
        let yy = y.clamp(self.rect.y0, self.rect.y1 - 1);
        let i = (yy - self.rect.y0) as usize * self.w + (xx - self.rect.x0) as usize;
        self.data
            .get(i)
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(0.04)
    }
}

/// Gaussian of a luma field in which `NAN` samples are held out through a weight channel, so
/// transparent pixels do not drag their neighbours.
fn blur_field(ybuf: &[f32], w: usize, h: usize, sig: f32) -> Vec<f32> {
    let mut value: Vec<f32> = ybuf
        .iter()
        .map(|v| if v.is_finite() { *v } else { 0.0 })
        .collect();
    let mut weight: Vec<f32> = ybuf
        .iter()
        .map(|v| if v.is_finite() { 1.0 } else { 0.0 })
        .collect();
    gauss_blur(&mut value, w, h, sig);
    gauss_blur(&mut weight, w, h, sig);
    value
        .iter()
        .zip(&weight)
        .map(|(&a, &wt)| {
            if wt > 1e-6 && a.is_finite() {
                a / wt
            } else {
                0.5
            }
        })
        .collect()
}

/// The layer to relight: straight RGBA f32, `w × h`.
#[derive(Clone, Copy)]
pub struct Layer<'a> {
    pub data: &'a [f32],
    pub w: usize,
    pub h: usize,
}

impl Layer<'_> {
    fn px(&self, x: i32, y: i32) -> [f32; 4] {
        let x = x.clamp(0, self.w as i32 - 1) as usize;
        let y = y.clamp(0, self.h as i32 - 1) as usize;
        let i = (y * self.w + x) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

/// Relight `out` of `src`, `bounds` being the selection bounds that size the blur. Identity
/// when `intensity` is 0. A cancelled run returns what it has: untouched source elsewhere.
pub fn relight(src: Layer, out: Rect, bounds: Rect, p: Params, int: Interrupt) -> Vec<f32> {
    let mut res = src.data.to_vec();
    if out.is_empty() || !p.intensity.is_finite() || p.intensity == 0.0 {
        return res;
    }
    let sig = sigma(p.softness, bounds);
    let reach = read_radius(sig);
    let band_h = luma_band_rows(out, reach, MAX_LUMA_PIXELS).unwrap_or(out.height() as i32);
    let mut y = out.y0;
    while y < out.y1 {
        let y1 = y.saturating_add(band_h).min(out.y1);
        if !relight_rect(src, Rect::new(out.x0, y, out.x1, y1), p, sig, int, &mut res) {
            break;
        }
        y = y1;
    }
    res
}

/// Horizontal strip height so the inflated luma field stays within `cap` pixels; `None` when
/// `out` fits in one piece.
fn luma_band_rows(out: Rect, reach: i32, cap: usize) -> Option<i32> {
    let margin = 2usize.saturating_mul(reach.max(0) as usize);
    let yw = (out.width() as usize).saturating_add(margin);
    let yh = (out.height() as usize).saturating_add(margin);
    if yw == 0 || yw.saturating_mul(yh) <= cap {
        return None;
    }
    let max_yh = (cap / yw).max(1);
    let inner = max_yh.saturating_sub(margin).max(1).min(i32::MAX as usize) as i32;
    (inner < out.height() as i32).then_some(inner)
}

/// Relights one band into `res`; `false` when cancelled.
fn relight_rect(
    src: Layer,
    out: Rect,
    p: Params,
    sig: f32,
    int: Interrupt,
    res: &mut [f32],
) -> bool {
    // Finite differences at the shading scale (`σ`), not 1 px: a 1 px step vanishes on large
    // documents and the lamp collapses to a flat multiply.
    const SCALE: f32 = 2.0;
    let step = step_for_sigma(sig);
    let y_rect = out.inflate(read_radius(sig));
    let yw = y_rect.width() as usize;
    let yh = y_rect.height() as usize;
    let mut ybuf = vec![0.5f32; yw * yh];
    for (row, line) in ybuf.chunks_exact_mut(yw).enumerate() {
        if int.cancelled() {
            return false;
        }
        let y = y_rect.y0 + row as i32;
        for (col, slot) in line.iter_mut().enumerate() {
            let c = src.px(y_rect.x0 + col as i32, y);
            *slot = if c[3] > 0.0 {
                rec709_y([c[0], c[1], c[2]])
            } else {
                f32::NAN
            };
        }
    }
    let sbuf = Field {
        rect: y_rect,
        w: yw,
        data: blur_field(&ybuf, yw, yh, sig),
    };
    let ldir = light_dir(p.angle, p.elevation);
    let lcol = light_color(p.warmth);
    let amb = p.ambient.clamp(0.0, 100.0) / 100.0;
    let inten = p.intensity.clamp(0.0, 100.0) / 100.0;
    let (x0, x1) = (out.x0 as usize, out.x1 as usize);
    let rows = &mut res[out.y0 as usize * src.w * 4..out.y1 as usize * src.w * 4];
    par::rows_mut(rows, src.w * 4, |ry, row| {
        if int.cancelled() {
            return;
        }
        let y = out.y0 + ry as i32;
        for x in x0..x1 {
            let px = &mut row[x * 4..x * 4 + 4];
            if px[3] <= 0.0 {
                continue;
            }
            let (xi, yi) = (x as i32, y);
            let s0 = sbuf.at(xi, yi).max(0.04);
            let nx = (sbuf.at(xi - step, yi) - sbuf.at(xi + step, yi)) * SCALE;
            let ny = (sbuf.at(xi, yi - step) - sbuf.at(xi, yi + step)) * SCALE;
            let nrm = norm3([nx, ny, 1.0]);
            let ndl = (nrm[0] * ldir[0] + nrm[1] * ldir[1] + nrm[2] * ldir[2]).max(0.0);
            let shade = amb + inten * ndl;
            for (ch, lc) in px[..3].iter_mut().zip(lcol) {
                *ch = (*ch / s0) * shade * lc;
            }
        }
    });
    !int.cancelled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn luma(px: &[f32]) -> f32 {
        0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2]
    }

    fn params(intensity: f32, angle: f32) -> Params {
        Params {
            angle,
            elevation: 40.0,
            intensity,
            ambient: 55.0,
            warmth: 0.0,
            softness: 25.0,
        }
    }

    /// A lit sphere on a dark ground, as upstream's `shaded_sphere`.
    fn sphere(size: usize) -> Vec<f32> {
        let cx = size as f32 / 2.0;
        let rad = size as f32 * 0.42;
        let mut v = Vec::new();
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cx);
                let d2 = dx * dx + dy * dy;
                if d2 <= rad * rad {
                    let sh = 0.22 + 0.78 * (rad * rad - d2).max(0.0).sqrt() / rad;
                    v.extend_from_slice(&[0.55 * sh, 0.55 * sh, 0.55 * sh, 1.0]);
                } else {
                    v.extend_from_slice(&[0.18, 0.18, 0.18, 1.0]);
                }
            }
        }
        v
    }

    fn run(data: &[f32], size: usize, p: Params) -> Vec<f32> {
        let layer = Layer {
            data,
            w: size,
            h: size,
        };
        let r = Rect::new(0, 0, size as i32, size as i32);
        relight(layer, r, r, p, Interrupt::NONE)
    }

    fn half_mean(buf: &[f32], size: usize, left: bool) -> f32 {
        let cx = size as f32 / 2.0;
        let rad = size as f32 * 0.42;
        let (mut sum, mut n) = (0.0, 0.0f32);
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cx);
                let off = (x as f32) >= cx - 4.0;
                let skip = if left { off } else { (x as f32) < cx + 4.0 };
                if dx * dx + dy * dy > rad * rad || skip {
                    continue;
                }
                sum += luma(&buf[(y * size + x) * 4..]);
                n += 1.0;
            }
        }
        sum / n.max(1.0)
    }

    #[test]
    fn zero_intensity_is_identity() {
        let src = sphere(32);
        assert_eq!(run(&src, 32, params(0.0, 45.0)), src);
    }

    /// The lamp comes from the angle: a light from the right brightens the right half of the
    /// sphere relative to the left, and a light from the left does the opposite.
    #[test]
    fn light_direction_favours_its_side() {
        let src = sphere(64);
        let from_right = run(&src, 64, params(80.0, 0.0));
        let from_left = run(&src, 64, params(80.0, 180.0));
        let bias = |b: &[f32]| half_mean(b, 64, false) - half_mean(b, 64, true);
        assert!(
            bias(&from_right) > bias(&from_left) + 0.02,
            "{} {}",
            bias(&from_right),
            bias(&from_left)
        );
    }

    /// Transparent pixels stay untouched and do not leak into the shading of opaque ones.
    #[test]
    fn transparent_pixels_are_left_alone() {
        let mut src = sphere(16);
        src[3] = 0.0;
        let out = run(&src, 16, params(60.0, 45.0));
        assert_eq!(out[..4], src[..4]);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    /// Splitting the work into bands changes nothing visible (the halo covers the blur).
    #[test]
    fn bands_match_a_single_pass() {
        let size = 48;
        let src = sphere(size);
        let layer = Layer {
            data: &src,
            w: size,
            h: size,
        };
        let r = Rect::new(0, 0, size as i32, size as i32);
        let p = params(70.0, 30.0);
        let sig = sigma(p.softness, r);
        let whole = run(&src, size, p);
        let mut banded = src.clone();
        for y in (0..size as i32).step_by(10) {
            let band = Rect::new(0, y, size as i32, (y + 10).min(size as i32));
            assert!(relight_rect(
                layer,
                band,
                p,
                sig,
                Interrupt::NONE,
                &mut banded
            ));
        }
        let worst = whole
            .iter()
            .zip(&banded)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(worst < 1e-3, "{worst}");
    }

    #[test]
    fn luma_bands_only_when_the_field_is_over_the_cap() {
        let out = Rect::new(0, 0, 100, 100);
        assert_eq!(luma_band_rows(out, 5, MAX_LUMA_PIXELS), None);
        let inner = luma_band_rows(out, 5, 4000).expect("over the cap");
        assert!((1..100).contains(&inner));
    }

    #[test]
    fn cancelled_relight_keeps_the_source() {
        let src = sphere(16);
        let flag = AtomicBool::new(true);
        let layer = Layer {
            data: &src,
            w: 16,
            h: 16,
        };
        let r = Rect::new(0, 0, 16, 16);
        assert_eq!(
            relight(layer, r, r, params(60.0, 45.0), Interrupt(Some(&flag))),
            src
        );
    }
}
