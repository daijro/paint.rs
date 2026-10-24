//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (boxBlur, gaussianBoxRadii,
//! gaussianBlur's CPU path)
//! Reference: paintdotjs @ b4e6fc5
//!
//! Two Gaussian approximations:
//! - `skia_blur_premul`: Skia's raster `GaussPass`, which is what Chromium runs for canvas
//!   `filter: blur()` and shadows (pinned exactly by the `filter-blur*` canvas cases);
//! - `paintdotjs_cpu_box_blur_premul`: the byte-exact port of paintdotjs's CPU fallback (engine
//!   case `blur-gaussian-cpu`), for the effects port.
use crate::js::{clamped_u8, math_round};

/// ponytail: σ above old Skia's `MAX_SIGMA` (532) is clamped to bound scratch layers; Skia's
/// `TentPass` for windows >= 255 (σ >= ~136) is not ported, `GaussPass` math runs instead.
pub const MAX_SIGMA: f64 = 532.0;

/// Skia `BoxBlurWindow`: ⌊σ·3·√(2π)/4 + 0.5⌋, at least 1 (1 = no blur).
fn skia_window(sigma: f64) -> usize {
    let sigma = sigma.clamp(0.0, MAX_SIGMA);
    ((sigma * 3.0 * (2.0 * std::f64::consts::PI).sqrt() / 4.0 + 0.5).floor() as usize).max(1)
}

/// How far `skia_blur_premul` spreads a pixel: Skia's `GaussPass` border.
pub fn skia_blur_extent(sigma: f64) -> usize {
    match skia_window(sigma) {
        1 => 0,
        w if w % 2 == 1 => 3 * (w - 1) / 2,
        w => 3 * (w / 2) - 1,
    }
}

/// Skia's raster `GaussPass` on premultiplied RGBA: three box filters of window d (two of d and
/// one of d + 1 when d is even), summed in integers, transparent outside the buffer, rounded
/// through `ScaledDividerU32`; the x pass, stored as bytes, then the y pass.
pub fn skia_blur_premul(data: &mut [u8], w: u32, h: u32, sigma: f64) {
    let window = skia_window(sigma);
    if window <= 1 {
        return;
    }
    let (w, h) = (w as usize, h as usize);
    let d = window as i64;
    // (left, right) reach of each box.
    let boxes = if window % 2 == 1 {
        [((d - 1) / 2, (d - 1) / 2); 3]
    } else {
        [(d / 2, d / 2 - 1), (d / 2 - 1, d / 2), (d / 2, d / 2)]
    };
    let divisor = if window % 2 == 1 {
        d * d * d
    } else {
        d * d * d + d * d
    } as u64;
    let factor = ((1.0 / divisor as f64) * 4294967296.0).round() as u64;
    let half = (divisor + 1) >> 1;
    let border = skia_blur_extent(sigma);
    let mut line = vec![];
    let mut pass = |data: &mut [u8], n: usize, lines: usize, step: usize, line_step: usize| {
        for l in 0..lines {
            for c in 0..4 {
                // The line, zero-padded by `border` on both sides, through the three boxes.
                line.clear();
                line.resize(n + 2 * border, 0i64);
                for i in 0..n {
                    line[border + i] = data[l * line_step + i * step + c] as i64;
                }
                for &(left, right) in &boxes {
                    let mut prefix = Vec::with_capacity(line.len() + 1);
                    prefix.push(0i64);
                    for v in &line {
                        prefix.push(prefix.last().unwrap() + v);
                    }
                    let len = line.len() as i64;
                    for (i, v) in line.iter_mut().enumerate() {
                        let i = i as i64;
                        *v = prefix[(i + right + 1).min(len) as usize]
                            - prefix[(i - left).max(0) as usize];
                    }
                }
                for i in 0..n {
                    let sum = line[border + i] as u64;
                    data[l * line_step + i * step + c] = (((sum + half) * factor) >> 32) as u8;
                }
            }
        }
    };
    pass(data, w, h, 4, 4 * w);
    pass(data, h, w, 4 * w, 4);
}

/// Skia `SkBlurMask::BlurRect` (normal style): the analytic blurred-rectangle mask behind
/// Chromium's canvas shadows of rectangles (pinned exactly by the `shadow-blur` case),
/// evaluated only where it meets `clip` (`[x, y, w, h]`). Returns that area and its 8-bit
/// coverage.
pub fn skia_blur_rect_mask(
    sigma: f32,
    [l, t, r, b]: [f32; 4],
    clip: [i32; 4],
) -> Option<([i32; 4], Vec<u8>)> {
    let profile_size = (6.0 * sigma).ceil();
    if !(profile_size > 0.0 && profile_size <= 6.0 * MAX_SIGMA as f32 + 1.0) {
        return None;
    }
    let size = profile_size as i64;
    let pad = (size / 2) as f64;
    // SkScalarRoundToInt, in i64 so huge rects cannot overflow.
    let round = |v: f64| (v + 0.5).floor() as i64;
    let (x0, y0) = (round(l as f64 - pad), round(t as f64 - pad));
    let (w, h) = (round(r as f64 + pad) - x0, round(b as f64 + pad) - y0);
    let [cx, cy, cw, ch] = clip.map(|v| v as i64);
    let (ax0, ay0) = (x0.max(cx), y0.max(cy));
    let (ax1, ay1) = ((x0 + w).min(cx + cw), (y0 + h).min(cy + ch));
    if w <= 0 || h <= 0 || ax1 <= ax0 || ay1 <= ay0 {
        return None;
    }
    let profile = blur_profile(size as i32, sigma);
    let row: Vec<u8> = (ax0..ax1)
        .map(|x| blurred_scanline(&profile, w, sigma, x - x0))
        .collect();
    let col: Vec<u8> = (ay0..ay1)
        .map(|y| blurred_scanline(&profile, h, sigma, y - y0))
        .collect();
    let mut mask = Vec::with_capacity(row.len() * col.len());
    for v in &col {
        for u in &row {
            // SkMulDiv255Round
            let prod = *u as u32 * *v as u32 + 128;
            mask.push(((prod + (prod >> 8)) >> 8) as u8);
        }
    }
    let area = [ax0, ay0, ax1 - ax0, ay1 - ay0].map(|v| v as i32);
    Some((area, mask))
}

/// Whether Skia's `SkBlurMaskFilterImpl::filterRectsToNine` takes the analytic nine-patch
/// (`BlurRect`) for this rectangle. If the rect is too small relative to its blur, Skia falls back
/// to the general mask blur (`SkMaskBlurFilter`, the GaussPass math); `shadow-opaque-sigma3`
/// pins that fallback.
pub fn skia_blur_rect_fits(sigma: f32, [l, t, r, b]: [f32; 4]) -> bool {
    let profile_size = (6.0 * sigma).ceil();
    if !(profile_size > 0.0 && profile_size <= 6.0 * MAX_SIGMA as f32 + 1.0) {
        return false;
    }
    let pad = ((profile_size as i64) / 2) as f64;
    let round = |v: f64| (v + 0.5).floor() as i64;
    let fits = |lo: f32, hi: f32| {
        let src = hi.ceil() as i64 - lo.floor() as i64;
        let dst = round(hi as f64 + pad) - round(lo as f64 - pad);
        // +2 for fractional edges, +1 for a stretchable center row/column.
        src - (dst - src + 3) >= 0
    };
    fits(l, r) && fits(t, b)
}

/// Skia's `gaussianIntegral`: a piecewise-cubic approximation of the Gaussian's integral.
fn gaussian_integral(x: f32) -> f32 {
    if x > 1.5 {
        return 0.0;
    }
    if x < -1.5 {
        return 1.0;
    }
    let x2 = x * x;
    let x3 = x2 * x;
    if x > 0.5 {
        0.5625 - (x3 / 6.0 - 3.0 * x2 * 0.25 + 1.125 * x)
    } else if x > -0.5 {
        0.5 - (0.75 * x - x3 / 3.0)
    } else {
        0.4375 + (-x3 / 6.0 - 3.0 * x2 * 0.25 - 1.125 * x)
    }
}

/// `SkBlurMask::ComputeBlurProfile`: a blurred half-plane, pre-inverted (255 − coverage).
fn blur_profile(size: i32, sigma: f32) -> Vec<u8> {
    let center = size >> 1;
    let invr = 1.0 / (2.0 * sigma);
    let mut profile = vec![255u8; size as usize];
    for x in 1..size {
        let scaled_x = ((center - x) as f32 - 0.5) * invr;
        profile[x as usize] = 255 - (255.0 * gaussian_integral(scaled_x)) as u8;
    }
    profile
}

/// Value `x` of `SkBlurMask::ComputeBlurredScanline` (with `ProfileLookup`) for `width`.
fn blurred_scanline(profile: &[u8], width: i64, sigma: f32, x: i64) -> u8 {
    let size = profile.len() as i64;
    let sw = width - size;
    if size <= sw {
        // The nearest odd number below the profile size is the 2×-scaled profile's center.
        let sharp = sw - ((size & !1) - 1);
        let dx = (((x << 1) + 1) - width).abs() - sharp;
        profile[(dx >> 1).max(0) as usize]
    } else {
        let span = sw as f32 / (2.0 * sigma);
        let gi = 1.5 - (x as f32 + 0.5) / (2.0 * sigma);
        (255.0 * (gaussian_integral(gi) - gaussian_integral(gi + span))) as u8
    }
}

/// `BitmapEffectEngine.gaussianBlur` CPU path at `quality = 2` (3 box passes), taking σ directly
/// (the JS derives `sigma = radius / 3`). Edges clamp, like the JS.
pub fn paintdotjs_cpu_box_blur_premul(data: &mut [u8], w: u32, h: u32, sigma: f64) {
    // `if (sigma < .01) return copy`
    if sigma.is_nan() || sigma < 0.01 {
        return;
    }
    // passes = Utility.clamp(Math.round(quality) || 2, 1, 4) + 1 with quality 2
    for radius in gaussian_box_radii(sigma, 3) {
        box_blur(data, w as usize, h as usize, radius);
    }
}

/// `BitmapEffectEngine.gaussianBoxRadii`: box radii whose combined variance best matches σ.
pub fn gaussian_box_radii(sigma: f64, passes: u32) -> Vec<f64> {
    let p = passes as f64;
    let ideal_width = (12.0 * sigma * sigma / p + 1.0).sqrt();
    let mut lower_width = ideal_width.floor();
    if lower_width % 2.0 == 0.0 {
        lower_width -= 1.0;
    }
    lower_width = lower_width.max(1.0);
    let upper_width = lower_width + 2.0;
    let lower_passes = math_round(
        (12.0 * sigma * sigma - p * lower_width * lower_width - 4.0 * p * lower_width - 3.0 * p)
            / (-4.0 * lower_width - 4.0),
    )
    .max(0.0)
    .min(p);
    (0..passes)
        .map(|i| {
            let width = if (i as f64) < lower_passes {
                lower_width
            } else {
                upper_width
            };
            (width - 1.0) / 2.0
        })
        .collect()
}

/// `BitmapEffectEngine.boxBlur`: horizontal pass into an f32 buffer (the JS `Float32Array`),
/// vertical pass stored through `Uint8ClampedArray` rounding. Edge samples clamp.
pub fn box_blur(data: &mut [u8], width: usize, height: usize, radius: f64) {
    let radius = math_round(radius).max(0.0) as i64;
    if radius == 0 || width == 0 || height == 0 {
        return;
    }
    let (w, h) = (width as i64, height as i64);
    let at = |x: i64, y: i64| ((y * w + x) * 4) as usize;
    let mut horizontal = vec![0f32; data.len()];
    let diameter = (radius * 2 + 1) as f64;
    for y in 0..h {
        let mut sums = [0f64; 4];
        for x in -radius..=radius {
            let offset = at(x.clamp(0, w - 1), y);
            for c in 0..4 {
                sums[c] += data[offset + c] as f64;
            }
        }
        for x in 0..w {
            let offset = at(x, y);
            for c in 0..4 {
                horizontal[offset + c] = (sums[c] / diameter) as f32;
            }
            let (remove, add) = (
                at((x - radius).max(0), y),
                at((x + radius + 1).min(w - 1), y),
            );
            for c in 0..4 {
                sums[c] += data[add + c] as f64 - data[remove + c] as f64;
            }
        }
    }
    for x in 0..w {
        let mut sums = [0f64; 4];
        for y in -radius..=radius {
            let offset = at(x, y.clamp(0, h - 1));
            for c in 0..4 {
                sums[c] += horizontal[offset + c] as f64;
            }
        }
        for y in 0..h {
            let offset = at(x, y);
            for c in 0..4 {
                data[offset + c] = clamped_u8(sums[c] / diameter);
            }
            let (remove, add) = (
                at(x, (y - radius).max(0)),
                at(x, (y + radius + 1).min(h - 1)),
            );
            for c in 0..4 {
                sums[c] += horizontal[add + c] as f64 - horizontal[remove + c] as f64;
            }
        }
    }
}
