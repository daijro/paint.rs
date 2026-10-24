//! Ported from PhotoCraft crates/algo/src/camera_raw.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only the stages the dialog shows (white balance and exposure, dehaze, tone, presence, Color Mixer, detail): the tone curves, Color Grading, vignette, grain, curve validation and serde dropped with their fields; 8-bit sRGB only (the `float` flag, `encoding_gamma` with `Encoding`, and `pixel_scale` dropped: results always clamp to 0–1, scale is 1); `develop` takes an `Interrupt`, polled between stages and per reference value of the local Laplacian filter, and returns `Err(Cancelled)` when it fires; rows through `extra::par`, and `box_mean`'s column pass and the sharpening mask map over `extra::par::map` as upstream; `luminance_points` and `detail` poll the `Interrupt` too; the sRGB curves are `pixel::gamma::*_f32` (upstream's `photo_util`/`photocraft_color` ones, same formulas); `bilinear` reads one channel; `gauss_box_radii` is `fx::util`'s (upstream's `fxutil` one), and so is `smoothstep` (upstream's own unguarded copy gives the same for the one rising ramp it draws); tests: `defaults_are_identity` and `basic_panel_moves_the_right_way` unchanged, `curves_hsl_grading_detail_effects` keeps its HSL and detail checks as `hsl_and_detail` (its noise from `extra::rng::splitmix64`), the curve, float and validation tests dropped.
//!
//! Camera Raw: a float develop pipeline over RGB pixels.
//!
//! Stages, in order (each skipped when its controls are neutral, so defaults are an identity):
//!
//! 1. **White balance and exposure** in linear light (sRGB decoded, encoded again after).
//! 2. **Dehaze**: K. He, J. Sun, X. Tang, *Single Image Haze Removal Using Dark Channel Prior*,
//!    CVPR 2009 (dark channel, atmospheric light from its brightest 0.1 %, transmission refined
//!    with the guided filter); negative amounts add haze toward the atmospheric light.
//! 3. **Tone** on luminance with colour ratios kept: Highlights / Shadows as a local Laplacian
//!    filter (`local_tone`); Contrast is an S-curve about middle grey; Clarity (large radius,
//!    midtone weighted, guided filter: K. He, J. Sun, X. Tang, *Guided Image Filtering*, ECCV
//!    2010) and Texture (small radius) add band-pass local contrast. Whites / Blacks follow as
//!    Photoshop's Camera Raw applies them: image-adaptive global curves measured on Photoshop 25.4
//!    (see `whites_blacks`).
//! 4. **Presence**: Vibrance (weighted toward less saturated colours) and Saturation.
//! 5. **Color Mixer (HSL)**: eight hue bands (red, orange, yellow, green, aqua, blue, purple,
//!    magenta) with hat-function weights on the hue circle.
//! 6. **Detail**: sharpening on luminance (unsharp mask with Detail as halo control and Masking as
//!    an edge mask) and noise reduction (luminance by guided filter, colour by chroma blur).

#![allow(clippy::needless_range_loop)] // index loops mirror the maths (pixels × channels)

use crate::extra::fx::util::{gauss_box_radii, smoothstep};
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::par;
use crate::pixel::gamma::{
    linear_to_srgb_f32 as linear_to_srgb, srgb_to_linear_f32 as srgb_to_linear,
};

mod local_tone;
mod whites_blacks;

/// Camera Raw settings in Adobe Camera Raw's slider units.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraRaw {
    // Basic
    pub temperature: f32,
    pub tint: f32,
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub texture: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub vibrance: f32,
    pub saturation: f32,
    // Color mixer: red, orange, yellow, green, aqua, blue, purple, magenta.
    pub hsl_hue: [f32; 8],
    pub hsl_sat: [f32; 8],
    pub hsl_lum: [f32; 8],
    // Detail
    pub sharpen_amount: f32,
    pub sharpen_radius: f32,
    pub sharpen_detail: f32,
    pub sharpen_masking: f32,
    pub noise_luminance: f32,
    pub noise_luminance_detail: f32,
    pub noise_color: f32,
    pub noise_color_detail: f32,
}

impl Default for CameraRaw {
    fn default() -> Self {
        CameraRaw {
            temperature: 0.0,
            tint: 0.0,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            hsl_hue: [0.0; 8],
            hsl_sat: [0.0; 8],
            hsl_lum: [0.0; 8],
            sharpen_amount: 0.0,
            sharpen_radius: 1.0,
            sharpen_detail: 25.0,
            sharpen_masking: 0.0,
            noise_luminance: 0.0,
            noise_luminance_detail: 50.0,
            noise_color: 0.0,
            noise_color_detail: 50.0,
        }
    }
}

/// Hue band centres of the Color Mixer (degrees).
pub const HSL_BANDS: [f32; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0];

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Upstream's `par_rows` over pixels: `f(y, row)` for each `w`-pixel row, through `extra::par`.
fn pixel_rows(px: &mut [[f32; 4]], w: usize, f: impl Fn(usize, &mut [[f32; 4]]) + Sync + Send) {
    par::rows_mut(px.as_flattened_mut(), w * 4, |y, row| {
        f(y, row.as_chunks_mut::<4>().0)
    });
}

/// Bilinear sample of a `w × h` single-channel buffer, clamped to the edges.
fn bilinear(buf: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let p = |xx: usize, yy: usize| buf[yy * w + xx];
    let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * tx;
    let bot = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * tx;
    top + (bot - top) * ty
}

/// Box mean of a single-channel buffer (radius `r`, edge clamped): rows in parallel, then
/// columns with running column sums over whole rows (cache-friendly, no transposes).
fn box_mean(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    if r == 0 || w == 0 || h == 0 {
        return v.to_vec();
    }
    let n = (2 * r + 1) as f64;
    let mut tmp = vec![0.0f32; w * h];
    par::rows_mut(&mut tmp, w, |y, row| {
        let src = &v[y * w..(y + 1) * w];
        let at = |k: i64| src[k.clamp(0, w as i64 - 1) as usize] as f64;
        let mut acc: f64 = (-(r as i64)..=r as i64).map(at).sum();
        for (x, o) in row.iter_mut().enumerate() {
            *o = (acc / n) as f32;
            acc += at(x as i64 + r as i64 + 1) - at(x as i64 - r as i64);
        }
    });
    // Column bands in parallel (as upstream): each band keeps its own running column sums.
    const BAND: usize = 256;
    let bands = par::map(w.div_ceil(BAND), true, |b| {
        let (x0, x1) = (b * BAND, ((b + 1) * BAND).min(w));
        let bw = x1 - x0;
        let row = |y: i64| {
            let y = y.clamp(0, h as i64 - 1) as usize;
            &tmp[y * w + x0..y * w + x1]
        };
        let mut acc = vec![0.0f64; bw];
        for y in -(r as i64)..=r as i64 {
            for (a, v) in acc.iter_mut().zip(row(y)) {
                *a += *v as f64;
            }
        }
        let mut out = vec![0.0f32; bw * h];
        for y in 0..h {
            for (o, a) in out[y * bw..(y + 1) * bw].iter_mut().zip(&acc) {
                *o = (*a / n) as f32;
            }
            let (add, sub) = (row(y as i64 + r as i64 + 1), row(y as i64 - r as i64));
            for ((a, p), m) in acc.iter_mut().zip(add).zip(sub) {
                *a += (*p - *m) as f64;
            }
        }
        out
    });
    let mut out = vec![0.0f32; w * h];
    for (b, band) in bands.iter().enumerate() {
        let (x0, x1) = (b * BAND, ((b + 1) * BAND).min(w));
        let bw = x1 - x0;
        for y in 0..h {
            out[y * w + x0..y * w + x1].copy_from_slice(&band[y * bw..(y + 1) * bw]);
        }
    }
    out
}

/// Approximate Gaussian (three box passes).
fn gauss(v: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    if sigma < 0.3 {
        return v.to_vec();
    }
    let mut out = v.to_vec();
    for r in gauss_box_radii(sigma) {
        out = box_mean(&out, w, h, r);
    }
    out
}

/// Fast guided filter (K. He, J. Sun, *Fast Guided Filter*, arXiv 2015): the linear
/// coefficients are computed on a `s×` subsampled copy and upsampled bilinearly, which is
/// visually equivalent for the large radii used by tone controls at a fraction of the cost.
fn guided_fast(i: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let s = (r / 8).clamp(1, 8);
    if s == 1 {
        return guided(i, p, w, h, r, eps);
    }
    let (sw, sh) = (w.div_ceil(s), h.div_ceil(s));
    let down = |v: &[f32]| -> Vec<f32> {
        let mut o = vec![0.0f32; sw * sh];
        let mut c = vec![0.0f32; sw * sh];
        for y in 0..h {
            for x in 0..w {
                let k = (y / s) * sw + x / s;
                o[k] += v[y * w + x];
                c[k] += 1.0;
            }
        }
        o.iter().zip(&c).map(|(a, b)| a / b.max(1.0)).collect()
    };
    let (si, sp) = (down(i), down(p));
    let rs = (r / s).max(1);
    let mi = box_mean(&si, sw, sh, rs);
    let mp = box_mean(&sp, sw, sh, rs);
    let ip: Vec<f32> = si.iter().zip(&sp).map(|(a, b)| a * b).collect();
    let ii: Vec<f32> = si.iter().map(|a| a * a).collect();
    let mip = box_mean(&ip, sw, sh, rs);
    let mii = box_mean(&ii, sw, sh, rs);
    let a: Vec<f32> = (0..sw * sh)
        .map(|k| (mip[k] - mi[k] * mp[k]) / (mii[k] - mi[k] * mi[k] + eps))
        .collect();
    let b: Vec<f32> = (0..sw * sh).map(|k| mp[k] - a[k] * mi[k]).collect();
    let (ma, mb) = (box_mean(&a, sw, sh, rs), box_mean(&b, sw, sh, rs));
    let mut out = vec![0.0f32; w * h];
    let sf = s as f32;
    par::rows_mut(&mut out, w, |y, row| {
        let fy = (y as f32 + 0.5) / sf - 0.5;
        for (x, o) in row.iter_mut().enumerate() {
            let fx = (x as f32 + 0.5) / sf - 0.5;
            let av = bilinear(&ma, sw, sh, fx, fy);
            let bv = bilinear(&mb, sw, sh, fx, fy);
            *o = av * i[y * w + x] + bv;
        }
    });
    out
}

/// Guided filter of `p` guided by `i` (He et al. 2010).
fn guided(i: &[f32], p: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let mi = box_mean(i, w, h, r);
    let mp = box_mean(p, w, h, r);
    let ip: Vec<f32> = i.iter().zip(p).map(|(a, b)| a * b).collect();
    let ii: Vec<f32> = i.iter().map(|a| a * a).collect();
    let mip = box_mean(&ip, w, h, r);
    let mii = box_mean(&ii, w, h, r);
    let a: Vec<f32> = (0..w * h)
        .map(|k| (mip[k] - mi[k] * mp[k]) / (mii[k] - mi[k] * mi[k] + eps))
        .collect();
    let b: Vec<f32> = (0..w * h).map(|k| mp[k] - a[k] * mi[k]).collect();
    let ma = box_mean(&a, w, h, r);
    let mb = box_mean(&b, w, h, r);
    (0..w * h).map(|k| ma[k] * i[k] + mb[k]).collect()
}

fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let d = mx - mn;
    let h = if d <= 1e-9 {
        0.0
    } else if mx == c[0] {
        60.0 * (((c[1] - c[2]) / d).rem_euclid(6.0))
    } else if mx == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    [h, if mx > 1e-9 { d / mx } else { 0.0 }, mx]
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// Weights of the eight HSL bands at hue `h` (hat functions between neighbouring centres).
pub fn band_weights(h: f32) -> [f32; 8] {
    let mut w = [0.0f32; 8];
    for k in 0..8 {
        let (c, prev, next) = (HSL_BANDS[k], HSL_BANDS[(k + 7) % 8], HSL_BANDS[(k + 1) % 8]);
        let diff = ((h - c + 540.0) % 360.0) - 180.0;
        let span = if diff >= 0.0 {
            (next - c).rem_euclid(360.0)
        } else {
            (c - prev).rem_euclid(360.0)
        };
        w[k] = (1.0 - diff.abs() / span.max(1e-3)).max(0.0);
    }
    w
}

impl CameraRaw {
    fn wb_neutral(&self) -> bool {
        self.temperature == 0.0 && self.tint == 0.0 && self.exposure == 0.0
    }
    fn tone_neutral(&self) -> bool {
        self.local_tone_neutral() && self.whites == 0.0 && self.blacks == 0.0
    }
    /// Contrast, Highlights, Shadows, Texture and Clarity (the luminance stage).
    fn local_tone_neutral(&self) -> bool {
        [
            self.contrast,
            self.highlights,
            self.shadows,
            self.texture,
            self.clarity,
        ]
        .iter()
        .all(|v| *v == 0.0)
    }
    fn hsl_neutral(&self) -> bool {
        self.hsl_hue
            .iter()
            .chain(&self.hsl_sat)
            .chain(&self.hsl_lum)
            .all(|v| *v == 0.0)
    }
    /// True when every control is neutral (the filter is an identity).
    pub fn is_identity(&self) -> bool {
        self.wb_neutral()
            && self.tone_neutral()
            && self.dehaze == 0.0
            && self.vibrance == 0.0
            && self.saturation == 0.0
            && self.hsl_neutral()
            && self.sharpen_amount == 0.0
            && self.noise_luminance == 0.0
            && self.noise_color == 0.0
    }
}

/// Camera Raw's white-balance channel gains for Temperature / Tint (−100…100), relative to the
/// as-shot balance: warmer raises red and lowers blue (±35 % at the ends); a negative tint
/// raises green, a positive one lowers it toward magenta (±25 %). An observed approximation of
/// Adobe Camera Raw's sliders, not a colour-temperature model.
pub fn white_balance_gains(temperature: f32, tint: f32) -> [f32; 3] {
    let t = temperature / 100.0;
    let tn = tint / 100.0;
    [1.0 + 0.35 * t, 1.0 - 0.25 * tn, 1.0 - 0.35 * t]
}

/// Runs the Camera Raw pipeline on straight sRGB RGBA pixels (`w × h`); alpha is untouched and
/// the colour ends clamped to 0–1. `Err(Cancelled)` (pixels part-way) once `stop` fires.
pub fn develop(
    px: &mut [[f32; 4]],
    w: usize,
    h: usize,
    p: &CameraRaw,
    stop: Interrupt,
) -> Result<(), Cancelled> {
    if px.is_empty() || p.is_identity() {
        return Ok(());
    }
    let long = w.max(h) as f32;
    // 1. White balance + exposure in linear light.
    if !p.wb_neutral() {
        let mut g = white_balance_gains(p.temperature, p.tint);
        let norm = luma(g);
        g = g.map(|v| v / norm * 2f32.powf(p.exposure));
        pixel_rows(px, w, |_, row| {
            for q in row.iter_mut() {
                for c in 0..3 {
                    q[c] = linear_to_srgb((srgb_to_linear(q[c]) * g[c]).max(0.0));
                }
            }
        });
    }
    // 2. Dehaze.
    if p.dehaze != 0.0 {
        stop.check()?;
        dehaze(px, w, h, p.dehaze / 100.0);
    }
    // 3. Tone. Highlights and Shadows: a local Laplacian filter (see `local_tone`).
    if p.highlights != 0.0 || p.shadows != 0.0 {
        stop.check()?;
        local_tone::apply(px, w, h, p.shadows, p.highlights, stop)?;
    }
    if !p.local_tone_neutral() && [p.contrast, p.texture, p.clarity].iter().any(|v| *v != 0.0) {
        stop.check()?;
        let l: Vec<f32> = px
            .iter()
            .map(|q| luma([q[0], q[1], q[2]]).max(0.0))
            .collect();
        let lc: Vec<f32> = l.iter().map(|v| v.min(1.0)).collect();
        let clar = (p.clarity != 0.0)
            .then(|| guided_fast(&lc, &lc, w, h, ((long * 0.02) as usize).max(3), 0.005));
        let tex = (p.texture != 0.0).then(|| gauss(&lc, w, h, (long * 0.002).max(1.5)));
        let k_con = p.contrast / 100.0 * 0.6;
        let tau = std::f32::consts::TAU;
        pixel_rows(px, w, |y, row| {
            for (x, q) in row.iter_mut().enumerate() {
                let i = y * w + x;
                let l0 = l[i];
                let mut lv = lc[i];
                if let Some(c) = &clar {
                    let mw = 1.0 - (2.0 * lv.clamp(0.0, 1.0) - 1.0).powi(2);
                    lv += p.clarity / 100.0 * 0.8 * (lc[i] - c[i]) * mw;
                }
                if let Some(t) = &tex {
                    lv += p.texture / 100.0 * 0.9 * (lc[i] - t[i]).clamp(-0.1, 0.1);
                }
                if k_con != 0.0 {
                    let xx = lv.clamp(0.0, 1.0);
                    lv += -k_con * (tau * xx).sin() / tau;
                }
                let lv = lv.max(0.0) + (l0 - lc[i]);
                let ratio = if l0 > 1e-5 { lv / l0 } else { 1.0 };
                for c in 0..3 {
                    q[c] = if l0 > 1e-5 {
                        q[c] * ratio
                    } else {
                        q[c] + (lv - l0)
                    };
                }
            }
        });
    }
    // 3b. Whites and Blacks: global curves adapted to the image's black and white points.
    if p.whites != 0.0 || p.blacks != 0.0 {
        stop.check()?;
        let (bp, wp) = luminance_points(px, w, h, stop)?;
        let (xw, xb) = whites_blacks::amounts(
            p.whites.clamp(-100.0, 100.0),
            p.blacks.clamp(-100.0, 100.0),
            bp,
            wp,
        );
        whites_blacks::apply(px, w, xw, xb);
    }
    // 4–5: per-pixel colour stages.
    if p.vibrance != 0.0 || p.saturation != 0.0 || !p.hsl_neutral() {
        stop.check()?;
        let hsl = !p.hsl_neutral();
        pixel_rows(px, w, |_, row| {
            for q in row.iter_mut() {
                let mut c = [q[0], q[1], q[2]];
                if p.vibrance != 0.0 || p.saturation != 0.0 {
                    let l = luma(c);
                    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
                    let sat = if mx > 1e-6 { (mx - mn) / mx } else { 0.0 };
                    // Vibrance protects skin-ish hues (oranges) a little, like ACR.
                    let hue = rgb_to_hsv(c)[0];
                    let skin = (1.0 - ((hue - 25.0).abs() / 25.0)).clamp(0.0, 1.0) * 0.5;
                    let k = 1.0
                        + p.saturation / 100.0
                        + p.vibrance / 100.0 * (1.0 - sat) * (1.0 - skin);
                    c = c.map(|v| l + (v - l) * k.max(0.0));
                }
                if hsl {
                    let [hh, s, v] = rgb_to_hsv(c.map(|x| x.max(0.0)));
                    if s > 1e-4 {
                        let wts = band_weights(hh);
                        let (mut dh, mut ds, mut dl) = (0.0f32, 0.0f32, 0.0f32);
                        for k in 0..8 {
                            dh += wts[k] * p.hsl_hue[k];
                            ds += wts[k] * p.hsl_sat[k];
                            dl += wts[k] * p.hsl_lum[k];
                        }
                        let nh = hh + dh / 100.0 * 30.0;
                        let ns = (s * (1.0 + ds / 100.0)).clamp(0.0, 1.0);
                        let mut out = hsv_to_rgb(nh, ns, v);
                        // Keep the luminance, then apply the luminance shift weighted by saturation.
                        let (l0, l1) = (luma(c), luma(out));
                        if l1 > 1e-6 {
                            out = out.map(|x| x * l0 / l1);
                        }
                        let k = 2f32.powf(dl / 100.0 * 1.0 * s);
                        c = out.map(|x| x * k);
                    }
                }
                q[0] = c[0];
                q[1] = c[1];
                q[2] = c[2];
            }
        });
    }
    // 6. Detail.
    if p.noise_luminance > 0.0 || p.noise_color > 0.0 || p.sharpen_amount > 0.0 {
        stop.check()?;
        detail(px, w, h, p, stop)?;
    }
    for q in px.iter_mut() {
        for c in 0..3 {
            q[c] = q[c].clamp(0.0, 1.0);
        }
    }
    Ok(())
}

/// The image's black and white points as sRGB-encoded grey levels: the 3rd percentile of the
/// luminance and the 99.5th of the brightest channel, on a copy at most 256 pixels across (so a
/// preview proxy and the full image agree).
fn luminance_points(
    px: &[[f32; 4]],
    w: usize,
    h: usize,
    stop: Interrupt,
) -> Result<(f32, f32), Cancelled> {
    let f = w.max(h).div_ceil(256).max(1);
    let (sw, sh) = (w.div_ceil(f), h.div_ceil(f));
    let mut sum = vec![[0.0f64; 4]; sw * sh];
    for y in 0..h {
        stop.check()?;
        for x in 0..w {
            let q = px[y * w + x];
            let s = &mut sum[(y / f) * sw + x / f];
            for c in 0..3 {
                s[c] += f64::from(srgb_to_linear(q[c]).clamp(0.0, 1.0));
            }
            s[3] += 1.0;
        }
    }
    let cells: Vec<[f32; 3]> = sum
        .iter()
        .filter(|s| s[3] > 0.0)
        .map(|s| [0, 1, 2].map(|c| (s[c] / s[3]) as f32))
        .collect();
    if cells.is_empty() {
        return Ok((0.0, 1.0));
    }
    let pct = |mut v: Vec<f32>, q: f32| {
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * q).round() as usize]
    };
    let bp = pct(cells.iter().map(|c| luma(*c)).collect(), 0.03);
    let wp = pct(
        cells.iter().map(|c| c[0].max(c[1]).max(c[2])).collect(),
        0.995,
    );
    Ok((linear_to_srgb(bp), linear_to_srgb(wp)))
}

fn dehaze(px: &mut [[f32; 4]], w: usize, h: usize, amount: f32) {
    // Transmission on a ≤ 768 px proxy, upsampled.
    let k = w.max(h).div_ceil(768).max(1);
    let (sw, sh) = (w.div_ceil(k), h.div_ceil(k));
    let mut small = vec![[0.0f32; 3]; sw * sh];
    let mut cnt = vec![0f32; sw * sh];
    for y in 0..h {
        for x in 0..w {
            let i = (y / k) * sw + x / k;
            for c in 0..3 {
                small[i][c] += px[y * w + x][c].clamp(0.0, 1.0);
            }
            cnt[i] += 1.0;
        }
    }
    for (s, c) in small.iter_mut().zip(&cnt) {
        *s = s.map(|v| v / c.max(1.0));
    }
    let r = (sw.max(sh) / 60).max(2);
    let min_filter = |v: &[f32]| -> Vec<f32> {
        let mut a = vec![0.0f32; sw * sh];
        for y in 0..sh {
            for x in 0..sw {
                let (x0, x1) = (x.saturating_sub(r), (x + r).min(sw - 1));
                a[y * sw + x] = (x0..=x1).map(|xx| v[y * sw + xx]).fold(f32::MAX, f32::min);
            }
        }
        let mut b = vec![0.0f32; sw * sh];
        for y in 0..sh {
            for x in 0..sw {
                let (y0, y1) = (y.saturating_sub(r), (y + r).min(sh - 1));
                b[y * sw + x] = (y0..=y1).map(|yy| a[yy * sw + x]).fold(f32::MAX, f32::min);
            }
        }
        b
    };
    let dark = min_filter(
        &small
            .iter()
            .map(|c| c[0].min(c[1]).min(c[2]))
            .collect::<Vec<_>>(),
    );
    // Atmospheric light: mean colour of the brightest 0.1 % of the dark channel.
    let mut idx: Vec<usize> = (0..sw * sh).collect();
    idx.sort_by(|&a, &b| dark[b].total_cmp(&dark[a]));
    let top = (sw * sh / 1000).max(1);
    let mut air = [0.0f32; 3];
    for &i in &idx[..top] {
        for c in 0..3 {
            air[c] += small[i][c] / top as f32;
        }
    }
    let air = air.map(|v| v.max(0.2));
    let norm_dark = min_filter(
        &small
            .iter()
            .map(|c| (c[0] / air[0]).min(c[1] / air[1]).min(c[2] / air[2]))
            .collect::<Vec<_>>(),
    );
    let t_raw: Vec<f32> = norm_dark.iter().map(|d| 1.0 - 0.95 * d).collect();
    let guide: Vec<f32> = small.iter().map(|c| luma(*c)).collect();
    let t = guided(&guide, &t_raw, sw, sh, r * 2, 1e-3);
    let ks = k as f32;
    pixel_rows(px, w, |y, row| {
        for (x, q) in row.iter_mut().enumerate() {
            let tv = bilinear(
                &t,
                sw,
                sh,
                (x as f32 + 0.5) / ks - 0.5,
                (y as f32 + 0.5) / ks - 0.5,
            )
            .clamp(0.1, 1.0);
            for c in 0..3 {
                let i = q[c];
                let j = (i - air[c]) / tv + air[c];
                q[c] = if amount >= 0.0 {
                    i + (j - i) * amount
                } else {
                    i + (air[c] - i) * (-amount) * 0.5 * (1.0 - tv * 0.5)
                };
            }
        }
    });
}

fn detail(
    px: &mut [[f32; 4]],
    w: usize,
    h: usize,
    p: &CameraRaw,
    stop: Interrupt,
) -> Result<(), Cancelled> {
    // Work in Y + chroma (BT.709 luma, B−Y, R−Y).
    let mut y: Vec<f32> = px.iter().map(|q| luma([q[0], q[1], q[2]])).collect();
    let mut cb: Vec<f32> = px.iter().zip(&y).map(|(q, l)| q[2] - l).collect();
    let mut cr: Vec<f32> = px.iter().zip(&y).map(|(q, l)| q[0] - l).collect();
    stop.check()?;
    if p.noise_luminance > 0.0 {
        let n = p.noise_luminance / 100.0;
        let r = (1.0 + 3.0 * n).round().max(1.0) as usize;
        let smooth = guided(&y, &y, w, h, r, (n * 0.06).powi(2) + 1e-6);
        let keep = p.noise_luminance_detail / 100.0 * 0.5;
        for i in 0..w * h {
            y[i] = smooth[i] + (y[i] - smooth[i]) * keep;
        }
    }
    stop.check()?;
    if p.noise_color > 0.0 {
        let sigma = (p.noise_color / 100.0 * 6.0).max(0.3);
        let (bcb, bcr) = (gauss(&cb, w, h, sigma), gauss(&cr, w, h, sigma));
        let keep = p.noise_color_detail / 100.0 * 0.3;
        for i in 0..w * h {
            cb[i] = bcb[i] + (cb[i] - bcb[i]) * keep;
            cr[i] = bcr[i] + (cr[i] - bcr[i]) * keep;
        }
    }
    stop.check()?;
    if p.sharpen_amount > 0.0 {
        let sigma = p.sharpen_radius.clamp(0.5, 3.0).max(0.3);
        let blur = gauss(&y, w, h, sigma);
        // Masking: an edge mask from the gradient of a smoothed luminance.
        let mask: Option<Vec<f32>> = (p.sharpen_masking > 0.0).then(|| {
            let s = gauss(&y, w, h, 1.0);
            let t = p.sharpen_masking / 100.0 * 0.08;
            par::map(w * h, true, |i| {
                let (x, yy) = (i % w, i / w);
                let gx = s[yy * w + (x + 1).min(w - 1)] - s[yy * w + x.saturating_sub(1)];
                let gy = s[(yy + 1).min(h - 1) * w + x] - s[yy.saturating_sub(1) * w + x];
                smoothstep(t * 0.5, t + 1e-4, gx.hypot(gy))
            })
        });
        let amt = p.sharpen_amount / 100.0 * 1.2;
        let det = p.sharpen_detail / 100.0;
        for i in 0..w * h {
            let hp = y[i] - blur[i];
            // Low Detail damps large (halo-producing) differences.
            let limit = 0.03 + det * 0.3;
            let hp = hp.clamp(-limit, limit) * (0.5 + 0.5 * det);
            let m = mask.as_ref().map_or(1.0, |m| m[i]);
            y[i] += amt * hp * m;
        }
    }
    for i in 0..w * h {
        let (l, b, r) = (y[i], cb[i], cr[i]);
        let (rr, bb) = (r + l, b + l);
        let g = (l - 0.2126 * rr - 0.0722 * bb) / 0.7152;
        px[i][0] = rr;
        px[i][1] = g;
        px[i][2] = bb;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: usize, h: usize) -> Vec<[f32; 4]> {
        (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
                [0.2 + 0.6 * x, 0.3 + 0.4 * y, 0.5 - 0.3 * x * y, 1.0]
            })
            .collect()
    }

    fn mean(px: &[[f32; 4]], c: usize) -> f32 {
        px.iter().map(|q| q[c]).sum::<f32>() / px.len() as f32
    }

    fn develop(px: &mut [[f32; 4]], w: usize, h: usize, p: &CameraRaw) {
        super::develop(px, w, h, p, Interrupt::NONE).unwrap();
    }

    /// The banded column pass gives the serial one's bits, across several 256-wide bands.
    #[test]
    fn box_mean_columns_match_a_serial_run() {
        let (w, h, r) = (600usize, 9usize, 3usize);
        let v: Vec<f32> = (0..w * h)
            .map(|i| ((i * 7919) % 1013) as f32 / 1013.0)
            .collect();
        let got = box_mean(&v, w, h, r);
        // Serial reference: the rows pass, then whole-row running column sums.
        let n = (2 * r + 1) as f64;
        let mut tmp = vec![0.0f32; w * h];
        for y in 0..h {
            let src = &v[y * w..(y + 1) * w];
            let at = |k: i64| src[k.clamp(0, w as i64 - 1) as usize] as f64;
            let mut acc: f64 = (-(r as i64)..=r as i64).map(at).sum();
            for x in 0..w {
                tmp[y * w + x] = (acc / n) as f32;
                acc += at(x as i64 + r as i64 + 1) - at(x as i64 - r as i64);
            }
        }
        let row = |y: i64| &tmp[y.clamp(0, h as i64 - 1) as usize * w..][..w];
        let mut acc = vec![0.0f64; w];
        for y in -(r as i64)..=r as i64 {
            row(y)
                .iter()
                .zip(&mut acc)
                .for_each(|(v, a)| *a += *v as f64);
        }
        let mut want = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                want[y * w + x] = (acc[x] / n) as f32;
            }
            let (add, sub) = (row(y as i64 + r as i64 + 1), row(y as i64 - r as i64));
            for x in 0..w {
                acc[x] += (add[x] - sub[x]) as f64;
            }
        }
        assert_eq!(got, want);
    }

    #[test]
    fn detail_returns_early_when_cancelled() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut a = img(8, 8);
        let b = a.clone();
        let p = CameraRaw {
            noise_luminance: 50.0,
            sharpen_amount: 50.0,
            ..CameraRaw::default()
        };
        assert_eq!(
            detail(&mut a, 8, 8, &p, Interrupt(Some(&flag))),
            Err(Cancelled)
        );
        assert_eq!(a, b);
    }

    #[test]
    fn defaults_are_identity() {
        let mut a = img(40, 30);
        let b = a.clone();
        develop(&mut a, 40, 30, &CameraRaw::default());
        assert_eq!(a, b);
    }

    #[test]
    fn basic_panel_moves_the_right_way() {
        let (w, h) = (64, 48);
        let base = img(w, h);
        let run = |p: CameraRaw| {
            let mut a = base.clone();
            develop(&mut a, w, h, &p);
            a
        };
        let warm = run(CameraRaw {
            temperature: 50.0,
            ..Default::default()
        });
        assert!(mean(&warm, 0) > mean(&base, 0) && mean(&warm, 2) < mean(&base, 2));
        let bright = run(CameraRaw {
            exposure: 1.0,
            ..Default::default()
        });
        assert!(mean(&bright, 1) > mean(&base, 1) + 0.1);
        let shadows = run(CameraRaw {
            shadows: 100.0,
            ..Default::default()
        });
        let dark_px = |a: &[[f32; 4]]| a[0][1];
        assert!(dark_px(&shadows) > dark_px(&base));
        let hl = run(CameraRaw {
            highlights: -100.0,
            ..Default::default()
        });
        assert!(hl[w * h - 1][0] < base[w * h - 1][0]);
        let con = run(CameraRaw {
            contrast: 100.0,
            ..Default::default()
        });
        // More contrast: darker darks, brighter brights.
        assert!(con[0][1] < base[0][1] && con[w * h - 1][0] > base[w * h - 1][0]);
        let desat = run(CameraRaw {
            saturation: -100.0,
            ..Default::default()
        });
        assert!(
            desat
                .iter()
                .all(|q| (q[0] - q[1]).abs() < 1e-4 && (q[1] - q[2]).abs() < 1e-4)
        );
        let vib = run(CameraRaw {
            vibrance: 100.0,
            ..Default::default()
        });
        let spread = |q: [f32; 4]| q[0].max(q[1]).max(q[2]) - q[0].min(q[1]).min(q[2]);
        assert!(spread(vib[w * h / 2]) > spread(base[w * h / 2]));
        for p in [
            CameraRaw {
                clarity: 80.0,
                ..Default::default()
            },
            CameraRaw {
                texture: 80.0,
                ..Default::default()
            },
            CameraRaw {
                dehaze: 60.0,
                ..Default::default()
            },
            CameraRaw {
                dehaze: -60.0,
                ..Default::default()
            },
            CameraRaw {
                whites: 50.0,
                blacks: -50.0,
                ..Default::default()
            },
        ] {
            let out = run(p.clone());
            assert!(
                out.iter().all(|q| q[..3]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))),
                "{p:?}"
            );
            assert_ne!(out, base);
        }
    }

    #[test]
    fn hsl_and_detail() {
        // Band weights sum to one everywhere.
        for hh in (0..360).step_by(7) {
            let s: f32 = band_weights(hh as f32).iter().sum();
            assert!((s - 1.0).abs() < 1e-4, "{hh}: {s}");
        }
        let (w, h) = (48, 32);
        let reds: Vec<[f32; 4]> = vec![[0.8, 0.2, 0.2, 1.0]; w * h];
        let mut a = reds.clone();
        let mut hs = [0.0; 8];
        hs[0] = -100.0;
        develop(
            &mut a,
            w,
            h,
            &CameraRaw {
                hsl_sat: hs,
                ..Default::default()
            },
        );
        assert!(a[0][0] - a[0][1] < 0.1, "reds desaturated: {:?}", a[0]);
        let blues: Vec<[f32; 4]> = vec![[0.2, 0.2, 0.8, 1.0]; w * h];
        let mut b = blues.clone();
        develop(
            &mut b,
            w,
            h,
            &CameraRaw {
                hsl_sat: hs,
                ..Default::default()
            },
        );
        assert!((b[0][2] - 0.8).abs() < 1e-4, "blues untouched");
        // Noise reduction reduces variance; sharpening increases it.
        let noisy: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let mut s = i as u64 * 3 + 1;
                let n = (crate::extra::rng::splitmix64(&mut s) >> 40) as f32 / (1u64 << 24) as f32
                    * 0.2;
                [0.4 + n, 0.4 + n, 0.4 + n, 1.0]
            })
            .collect();
        let var = |a: &[[f32; 4]]| {
            let m = mean(a, 1);
            a.iter().map(|q| (q[1] - m).powi(2)).sum::<f32>() / a.len() as f32
        };
        let mut nr = noisy.clone();
        develop(
            &mut nr,
            w,
            h,
            &CameraRaw {
                noise_luminance: 80.0,
                ..Default::default()
            },
        );
        assert!(var(&nr) < var(&noisy) * 0.5);
        let mut sh = noisy.clone();
        develop(
            &mut sh,
            w,
            h,
            &CameraRaw {
                sharpen_amount: 100.0,
                ..Default::default()
            },
        );
        assert!(var(&sh) > var(&noisy));
    }

    /// Each stage stops at its next poll once cancelled.
    #[test]
    fn develop_stops_when_cancelled() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut a = img(32, 32);
        for p in [
            CameraRaw {
                shadows: 50.0,
                ..Default::default()
            },
            CameraRaw {
                dehaze: 50.0,
                ..Default::default()
            },
            CameraRaw {
                sharpen_amount: 50.0,
                ..Default::default()
            },
        ] {
            assert_eq!(
                super::develop(&mut a, 32, 32, &p, Interrupt(Some(&flag))),
                Err(Cancelled)
            );
        }
    }
}
