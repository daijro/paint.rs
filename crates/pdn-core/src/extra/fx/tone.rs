//! Ported from PhotoCraft crates/algo/src/tone.rs, crates/compose/src/adjust.rs (`black_white_gray`, the Black & White tint), crates/algo/src/fxutil.rs (`gauss_blur_n`) and crates/color/src/{convert,blend}.rs (Lab, `set_lum`) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: only HDR Toning, Match Color (with `lab_stats`) and the Black & White grey/tint; interleaved straight RGBA f32 buffers (`as_chunks_mut`) instead of `[[f32; 4]]`/`Image`; the blur is the one-channel box triple, serial; rayon through `extra::par`; Shadows/Highlights, Replace Color, serde derives and the toning curve dropped; 8-bit only (no 32-bit tint branch).

use super::util::gauss_box_radii;

/// Rec. 601 luma.
fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

const D50: [f32; 3] = [0.964_22, 1.0, 0.825_21];
const SRGB_TO_XYZ_D50: [[f32; 3]; 3] = [
    [0.436_074, 0.385_064, 0.143_080],
    [0.222_504, 0.716_878, 0.060_618],
    [0.013_932, 0.097_104, 0.714_173],
];
const XYZ_D50_TO_SRGB: [[f32; 3]; 3] = [
    [3.133_856, -1.616_867, -0.490_615],
    [-0.978_768, 1.916_142, 0.033_454],
    [0.071_945, -0.228_991, 1.405_243],
];

fn mat3_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn lab_to_srgb([l, a, b]: [f32; 3]) -> [f32; 3] {
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let finv = |t: f32| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    let xyz = [finv(fx) * D50[0], finv(fy) * D50[1], finv(fz) * D50[2]];
    mat3_mul(&XYZ_D50_TO_SRGB, xyz).map(|lin| linear_to_srgb(lin.clamp(0.0, 1.0)))
}

fn srgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let xyz = mat3_mul(&SRGB_TO_XYZ_D50, rgb.map(srgb_to_linear));
    let f = |t: f32| {
        if t > (6.0f32 / 29.0).powi(3) {
            t.cbrt()
        } else {
            t / (3.0 * (6.0f32 / 29.0).powi(2)) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Black & White grey value: each colour is a grey part (its smallest channel), a secondary part
/// (yellow, cyan or magenta: middle − smallest) and a primary part (red, green or blue: largest −
/// middle); `weights` (percent; reds, yellows, greens, cyans, blues, magentas) weight the two.
pub fn black_white_gray(c: [f32; 3], weights: &[f32; 6]) -> f32 {
    let (r, g, b) = (c[0], c[1], c[2]);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let mid = r + g + b - max - min;
    let primary = if r >= g && r >= b {
        0
    } else if g >= b {
        2
    } else {
        4
    };
    let secondary = if b <= r && b <= g {
        1
    } else if r <= g {
        3
    } else {
        5
    };
    (min + (mid - min) * weights[secondary] / 100.0 + (max - mid) * weights[primary] / 100.0)
        .clamp(0.0, 1.0)
}

/// The Color blend mode's tint (`set_lum` + `clip_color`): `tint`'s hue and saturation at grey
/// level `g` (Photoshop's Black & White Tint).
pub fn tint_gray(tint: [f32; 3], g: f32) -> [f32; 3] {
    let lum = |c: [f32; 3]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
    let d = g - lum(tint);
    let mut out = tint.map(|v| v + d);
    let l = lum(out);
    let (n, x) = (
        out[0].min(out[1]).min(out[2]),
        out[0].max(out[1]).max(out[2]),
    );
    if n < 0.0 {
        let d = l - n;
        out = out.map(|v| {
            if l > 0.0 && d > 0.0 {
                l + (v - l) * l / d
            } else {
                l
            }
        });
    }
    if x > 1.0 {
        let d = x - l;
        out = out.map(|v| {
            if l < 1.0 && d > 0.0 {
                l + (v - l) * (1.0 - l) / d
            } else {
                l
            }
        });
    }
    out.map(|v| v.clamp(0.0, 1.0))
}

/// Image › Adjustments › Black & White on every pixel (alpha untouched).
pub fn black_white(buf: &mut [f32], weights: &[f32; 6], tint: Option<[f32; 3]>) {
    for p in buf.as_chunks_mut::<4>().0 {
        let g = black_white_gray([p[0], p[1], p[2]], weights);
        let o = tint.map_or([g; 3], |t| tint_gray(t, g));
        p[..3].copy_from_slice(&o);
    }
}

/// Lab mean and standard deviation of the opaque pixels, `[L, a, b]` each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabStats {
    pub mean: [f32; 3],
    pub std: [f32; 3],
}

/// Statistics for Match Color (pixels weighted by alpha); `None` when nothing is opaque.
pub fn lab_stats(buf: &[f32]) -> Option<LabStats> {
    let (mut sum, mut sq, mut wsum) = ([0.0f64; 3], [0.0f64; 3], 0.0f64);
    for q in buf.as_chunks::<4>().0 {
        let w = f64::from(q[3]);
        if w <= 0.0 {
            continue;
        }
        let lab = srgb_to_lab([q[0], q[1], q[2]]);
        for k in 0..3 {
            sum[k] += w * f64::from(lab[k]);
            sq[k] += w * f64::from(lab[k]) * f64::from(lab[k]);
        }
        wsum += w;
    }
    if wsum <= 0.0 {
        return None;
    }
    let mean = std::array::from_fn(|k| (sum[k] / wsum) as f32);
    let std =
        std::array::from_fn(|k| (sq[k] / wsum - (sum[k] / wsum).powi(2)).max(0.0).sqrt() as f32);
    Some(LabStats { mean, std })
}

/// Match Color options (Photoshop ranges).
#[derive(Clone, Debug, PartialEq)]
pub struct MatchColor {
    /// 1..=200 (100 = matched luminance).
    pub luminance: f32,
    /// Color intensity 1..=200 (100 = matched chroma).
    pub intensity: f32,
    /// 0..=100: blend back toward the original.
    pub fade: f32,
    /// Remove the colour cast (target a/b means → 0).
    pub neutralize: bool,
}

/// Reinhard-style colour transfer in Lab: shifts and scales each channel of the image (statistics
/// `target`) toward `source`'s; with no source only Neutralize / intensity / luminance act.
pub fn match_color(buf: &mut [f32], target: &LabStats, source: Option<&LabStats>, o: &MatchColor) {
    let src = source.copied().unwrap_or(*target);
    let (lum, inten) = (o.luminance / 100.0, o.intensity / 100.0);
    let fade = (o.fade / 100.0).clamp(0.0, 1.0);
    let scale = |k: usize| {
        if target.std[k] > 1e-4 {
            src.std[k] / target.std[k]
        } else {
            1.0
        }
    };
    let mut dst_mean = src.mean;
    if o.neutralize {
        dst_mean[1] = 0.0;
        dst_mean[2] = 0.0;
    }
    for q in buf.as_chunks_mut::<4>().0.iter_mut().filter(|q| q[3] > 0.0) {
        let c = [q[0], q[1], q[2]];
        let lab = srgb_to_lab(c);
        let l = ((lab[0] - target.mean[0]) * scale(0) + dst_mean[0]) * lum;
        let a = ((lab[1] - target.mean[1]) * scale(1) + dst_mean[1]) * inten;
        let b = ((lab[2] - target.mean[2]) * scale(2) + dst_mean[2]) * inten;
        let out = lab_to_srgb([l.clamp(0.0, 100.0), a, b]);
        for i in 0..3 {
            q[i] = (out[i] + (c[i] - out[i]) * fade).clamp(0.0, 1.0);
        }
    }
}

/// HDR Toning ("Local Adaptation") parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct HdrToning {
    /// Edge glow radius in px (1..=500).
    pub radius: f32,
    /// Edge glow strength 0.1..=4.
    pub strength: f32,
    /// 0.1..=2 (1 = neutral; lower = more contrast).
    pub gamma: f32,
    /// -5..=5 stops.
    pub exposure: f32,
    /// -100..=300 %.
    pub detail: f32,
    pub shadow: f32,
    pub highlight: f32,
    /// -100..=100.
    pub vibrance: f32,
    pub saturation: f32,
}

impl Default for HdrToning {
    fn default() -> Self {
        HdrToning {
            radius: 30.0,
            strength: 0.5,
            gamma: 1.0,
            exposure: 0.0,
            detail: 30.0,
            shadow: 0.0,
            highlight: 0.0,
            vibrance: 0.0,
            saturation: 20.0,
        }
    }
}

/// Running-sum box blur (radius `r`, edges clamped) of `line` in place, through `scratch`.
fn box_line(line: &mut [f32], r: usize, scratch: &mut Vec<f32>) {
    let n = line.len();
    if r == 0 || n == 0 {
        return;
    }
    scratch.clear();
    scratch.extend_from_slice(line);
    let norm = 1.0 / (2 * r + 1) as f64;
    // f64 sums: rounding must not depend on where a window starts.
    let at = |x: isize| f64::from(scratch[x.clamp(0, n as isize - 1) as usize]);
    let mut acc: f64 = (-(r as isize)..=r as isize).map(at).sum();
    for (x, v) in line.iter_mut().enumerate() {
        *v = (acc * norm) as f32;
        acc += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
    }
}

/// Approximate Gaussian blur (three box passes each way) of a one-channel `w × h` buffer.
pub(crate) fn gauss_blur(buf: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 || w == 0 || h == 0 {
        return;
    }
    let mut scratch = Vec::with_capacity(w.max(h));
    let mut col = vec![0.0; h];
    for r in gauss_box_radii(sigma) {
        for row in buf.chunks_exact_mut(w) {
            box_line(row, r, &mut scratch);
        }
    }
    for r in gauss_box_radii(sigma) {
        for x in 0..w {
            for (y, v) in col.iter_mut().enumerate() {
                *v = buf[y * w + x];
            }
            box_line(&mut col, r, &mut scratch);
            for (y, v) in col.iter().enumerate() {
                buf[y * w + x] = *v;
            }
        }
    }
}

/// Local adaptation tone mapping (Durand–Dorsey base/detail split in log luminance): the blurred
/// log luminance (radius) is the base layer, compressed toward its mean by `strength`; detail is
/// boosted; then exposure, gamma, shadow/highlight lift and vibrance/saturation.
pub fn hdr_toning(buf: &mut [f32], w: usize, h: usize, p: &HdrToning) {
    if buf.is_empty() {
        return;
    }
    const EPS: f32 = 1e-4;
    let px = buf.as_chunks::<4>().0;
    let logl: Vec<f32> = px
        .iter()
        .map(|q| (luma([q[0], q[1], q[2]]).max(0.0) + EPS).ln())
        .collect();
    let mut base = logl.clone();
    gauss_blur(&mut base, w, h, p.radius.max(1.0) / 2.0);
    let n = px.iter().filter(|q| q[3] > 0.0).count().max(1) as f32;
    let mean = base
        .iter()
        .zip(px)
        .filter(|(_, q)| q[3] > 0.0)
        .map(|(b, _)| *b)
        .sum::<f32>()
        / n;
    let compress = 1.0 / (1.0 + p.strength.max(0.0));
    let detail = 1.0 + p.detail / 100.0;
    let exposure = 2f32.powf(p.exposure);
    let gamma = p.gamma.clamp(0.1, 4.0);
    crate::extra::par::rows_mut(buf, w * 4, |y, row| {
        for (x, q) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let i = y * w + x;
            if q[3] <= 0.0 {
                continue;
            }
            let c = [q[0], q[1], q[2]];
            let l0 = luma(c).max(0.0) + EPS;
            let new_log = mean + (base[i] - mean) * compress + (logl[i] - base[i]) * detail;
            // "Lower = more contrast": gamma divides the exponent.
            let mut l1 = (new_log.exp() * exposure).max(0.0).powf(1.0 / gamma);
            l1 += p.shadow / 100.0 * (1.0 - l1).powi(3) * 0.5
                - p.highlight / 100.0 * l1.powi(3) * 0.5;
            let ratio = l1.clamp(0.0, 1.0) / l0;
            let mut out = c.map(|v| v * ratio);
            // Saturation then vibrance (less saturated colours move more), around the new luma.
            let ll = luma(out);
            let sat_now = {
                let (mx, mn) = (
                    out[0].max(out[1]).max(out[2]),
                    out[0].min(out[1]).min(out[2]),
                );
                if mx > 0.0 { (mx - mn) / mx } else { 0.0 }
            };
            let s = 1.0 + p.saturation / 100.0 + p.vibrance / 100.0 * (1.0 - sat_now);
            out = out.map(|v| ll + (v - ll) * s.max(0.0));
            for k in 0..3 {
                q[k] = out[k].clamp(0.0, 1.0);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(w: usize, h: usize) -> Vec<f32> {
        (0..w * h)
            .flat_map(|i| {
                let x = (i % w) as f32 / (w - 1) as f32;
                [x, x * 0.8, x * 0.6 + 0.1, 1.0]
            })
            .collect()
    }

    fn px_luma(buf: &[f32], i: usize) -> f32 {
        luma([buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2]])
    }

    #[test]
    fn black_white_weights() {
        let d = [40.0, 60.0, 40.0, 60.0, 20.0, 80.0];
        assert!((black_white_gray([0.5; 3], &d) - 0.5).abs() < 1e-6);
        assert!((black_white_gray([1.0, 0.0, 0.0], &d) - 0.4).abs() < 1e-6);
        assert!((black_white_gray([1.0, 1.0, 0.0], &d) - 0.6).abs() < 1e-6);
        let mut w = d;
        w[0] = 100.0;
        assert!(black_white_gray([1.0, 0.0, 0.0], &w) > black_white_gray([1.0, 0.0, 0.0], &d));
    }

    /// Upstream's measured Photoshop tint values (±2.5 levels).
    #[test]
    fn black_white_tint_matches_photoshop() {
        let measured: [([u8; 3], u8, [u8; 3]); 6] = [
            ([255, 0, 0], 24, [81, 1, 1]),
            ([255, 0, 0], 120, [255, 64, 64]),
            ([255, 0, 0], 240, [255, 235, 235]),
            ([0xe1, 0xd3, 0xb3], 120, [133, 119, 87]),
            ([0xe1, 0xd3, 0xb3], 24, [33, 24, 1]),
            ([0x40, 0x20, 0x10], 120, [144, 112, 96]),
        ];
        for (tint, grey, want) in measured {
            let got = tint_gray(tint.map(|v| f32::from(v) / 255.0), f32::from(grey) / 255.0);
            for (g, w) in got.iter().zip(want) {
                assert!((g * 255.0 - f32::from(w)).abs() <= 2.5, "{tint:?} {grey}");
            }
        }
    }

    #[test]
    fn match_color_moves_statistics_toward_source() {
        let src: Vec<f32> = (0..64)
            .flat_map(|i| [0.8, 0.4 + (i % 8) as f32 * 0.02, 0.2, 1.0])
            .collect();
        let mut tgt: Vec<f32> = (0..64)
            .flat_map(|i| [0.2, 0.3, 0.6 + (i % 8) as f32 * 0.03, 1.0])
            .collect();
        let s = lab_stats(&src).unwrap();
        let t = lab_stats(&tgt).unwrap();
        let o = MatchColor {
            luminance: 100.0,
            intensity: 100.0,
            fade: 0.0,
            neutralize: false,
        };
        match_color(&mut tgt, &t, Some(&s), &o);
        let after = lab_stats(&tgt).unwrap();
        for k in 0..3 {
            assert!(
                (after.mean[k] - s.mean[k]).abs() < (t.mean[k] - s.mean[k]).abs() * 0.2 + 1.0,
                "{k}: {after:?} vs {s:?}"
            );
        }
        // Neutralize with no source: the mean colour goes gray.
        let mut tint: Vec<f32> = (0..16)
            .flat_map(|i| [0.7, 0.5, 0.3 + i as f32 * 0.01, 1.0])
            .collect();
        let t = lab_stats(&tint).unwrap();
        match_color(
            &mut tint,
            &t,
            None,
            &MatchColor {
                neutralize: true,
                ..o
            },
        );
        let n = lab_stats(&tint).unwrap();
        assert!(n.mean[1].abs() < 2.0 && n.mean[2].abs() < 2.0, "{n:?}");
    }

    #[test]
    fn hdr_toning_compresses_range_and_keeps_bounds() {
        let mut px = ramp(40, 10);
        let p = HdrToning {
            strength: 2.0,
            detail: 0.0,
            saturation: 0.0,
            ..Default::default()
        };
        hdr_toning(&mut px, 40, 10, &p);
        assert!(px_luma(&px, 38) > px_luma(&px, 2));
        assert!(px.iter().all(|v| (0.0..=1.0).contains(v)));
    }
}
