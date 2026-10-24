//! BB spec: effects/blurs-gaussian-blur-plus.md "Building block"; effects/artistic-oil-painting-plus.md
//! step 1, "Building blocks"; effects/artistic-pastel.md; effects/photo-sharpen-classic.md steps 1–2;
//! effects/photo-sharpen-landscape.md steps 2, 4; effects/stylize-floyd-steinberg-dithering.md step 1;
//! common.md §5.2, §5.9
//!
//! Paint.NET 4.2's classic 8-bit CPU built-ins, as the classic plugins call them. paint.rs's own
//! built-ins are the Paint.NET 5 GPU versions and differ (common.md §5.2). Straight RGBA8 in and
//! out, integer maths with the spec's truncation.
// approximation: Paint.NET 4.2's classic code paths, assumed unchanged in 5.x (artistic-oil-painting-plus.md Notes)

use super::conventions::{
    CLASSIC_BLUR_CLAMP_EDGES, CLASSIC_HSV_ROUND_TRIP, MEDIAN_RANK_BASE, PASTEL_HSV_FROM_BYTES,
    PASTEL_VALUE_OFFSET,
};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::pixel::Px;
use crate::pixel::ops::compose;
use crate::render::display::for_bands;
use crate::util::color::{HsvColor, hsv_to_unit_rgb};

/// common.md §5.9 "Intensity": (19595·R + 38470·G + 7471·B) >> 16, fixed-point BT.601 truncated.
pub fn intensity(p: Px) -> u8 {
    ((19595 * p[0] as u32 + 38470 * p[1] as u32 + 7471 * p[2] as u32) >> 16) as u8
}

fn pixels(img: &ImageData) -> &[Px] {
    img.data.as_chunks::<4>().0
}

fn map(img: &ImageData, f: impl Fn(Px) -> Px) -> ImageData {
    ImageData {
        width: img.width,
        height: img.height,
        data: pixels(img).iter().flat_map(|&p| f(p)).collect(),
    }
}

/// A w×h image whose rows `f(y, row)` fills, in parallel on native targets.
fn by_rows(w: u32, h: u32, f: impl Fn(usize, &mut [Px]) + Sync + Send) -> ImageData {
    let mut out = ImageData::new(w, h);
    for_bands(&mut out.data, w as usize * 4, 0..h as usize, 1, |y, row| {
        f(y, row.as_chunks_mut::<4>().0)
    });
    out
}

/// stylize-floyd-steinberg-dithering.md step 1 (Paint.NET's Brightness / Contrast): one shift for
/// R, G and B that moves the intensity I to 127 + (I − 127)·k plus the brightness (added before the
/// scaling for positive contrast), integer maths truncating, each channel clamped; contrast 100
/// thresholds R = G = B at I + B ≥ 128. Alpha kept.
// approximation: Paint.NET 4.2's Brightness / Contrast, assumed identical in 5 (stylize-floyd-steinberg-dithering.md)
pub fn brightness_contrast(img: &ImageData, brightness: i32, contrast: i32) -> ImageData {
    let (b, c) = (brightness, contrast);
    map(img, |p| {
        let i = intensity(p) as i32;
        if c >= 100 {
            let v = if i + b >= 128 { 255 } else { 0 };
            return [v, v, v, p[3]];
        }
        let shift = match c {
            0 => b,
            ..0 => (i - 127) * (100 + c) / 100 + 127 - i + b,
            _ => (i - 127 + b) * 100 / (100 - c) + 127 - i,
        };
        let ch = |k: usize| (p[k] as i32 + shift).clamp(0, 255) as u8;
        [ch(0), ch(1), ch(2), p[3]]
    })
}

/// blurs-gaussian-blur-plus.md "Building block" (common.md §5.2): classic integer tent blur.
/// Supported effect radii are 0..=100. Radius 0 copies, including hidden transparent colour.
/// Keep horizontal Σwac, Σwa and Σw unnormalized, then sum wy times those values vertically.
/// Distributivity gives the prescribed product-weight 2-D sums, with just one final floor division.
/// At r=100: Σw ≤ 256·101⁴ = 26,639,462,656, Σwac ≤ 1,732,231,059,206,400 < u64::MAX.
// approximation: inferred classic native alpha weighting and final floor (blurs-gaussian-blur-plus.md Notes)
// spec-question: Q20 Classic Gaussian blur edges
pub fn gaussian_blur(img: &ImageData, r: u32) -> ImageData {
    tent_blur(img, r, CLASSIC_BLUR_CLAMP_EDGES)
}

fn tent_blur(img: &ImageData, r: u32, clamp: bool) -> ImageData {
    if r == 0 || img.width == 0 || img.height == 0 {
        return img.clone();
    }
    // Each active worker owns at most 64×(32+2r) five-u64 sums: 593,920 bytes at r=100.
    // Total weighted scratch is bounded by this × min(Rayon workers, ceil(h/32)); wasm uses one worker.
    // Bands retain the existing native parallelism; strips bound scratch independently of canvas size.
    const TILE: usize = 64;
    const BAND: usize = 32;
    let (w, h, r) = (img.width as usize, img.height as usize, r as usize);
    let src = pixels(img);
    let mut out = ImageData::new(img.width, img.height);
    for_bands(&mut out.data, w * 4, 0..h, BAND, |first, bytes| {
        let rows = bytes.len() / (w * 4);
        let top = first.saturating_sub(r);
        let bottom = (first + rows + r).min(h);
        for left in (0..w).step_by(TILE) {
            let columns = (w - left).min(TILE);
            let mut horizontal = vec![[0u64; 5]; columns * (bottom - top)];
            for y in top..bottom {
                let sample = |mut x: i64| -> [u64; 5] {
                    if clamp {
                        x = x.clamp(0, w as i64 - 1);
                    } else if x < 0 || x >= w as i64 {
                        return [0; 5];
                    }
                    let p = src[y * w + x as usize];
                    let a = 16 * p[3] as u64;
                    [a * p[0] as u64, a * p[1] as u64, a * p[2] as u64, a, 16]
                };
                let mut sum = [0u64; 5];
                let mut before = [0u64; 5];
                let mut after = sample(left as i64 + r as i64 + 1);
                for dx in -(r as i64)..=r as i64 {
                    let p = sample(left as i64 + dx);
                    for c in 0..5 {
                        sum[c] += (r as u64 + 1 - dx.unsigned_abs()) * p[c];
                        if dx <= 0 {
                            before[c] += p[c];
                        } else {
                            after[c] += p[c];
                        }
                    }
                }
                for x in left..left + columns {
                    horizontal[(y - top) * columns + x - left] = sum;
                    if x + 1 < left + columns {
                        // Moving the tent right adds its right (r+1)-tap box and removes its left box.
                        // Slide those boxes too. No rounding, and add before subtract keeps sums unsigned.
                        let enter = sample(x as i64 + r as i64 + 2);
                        let middle = sample(x as i64 + 1);
                        let leave = sample(x as i64 - r as i64);
                        for c in 0..5 {
                            sum[c] = sum[c] + after[c] - before[c];
                            before[c] = before[c] + middle[c] - leave[c];
                            after[c] = after[c] + enter[c] - middle[c];
                        }
                    }
                }
            }
            for (row, output) in bytes.chunks_exact_mut(w * 4).enumerate() {
                let y = first + row;
                for x in left..left + columns {
                    let mut sum = [0u64; 5];
                    for dy in -(r as i64)..=r as i64 {
                        let mut yy = y as i64 + dy;
                        if clamp {
                            yy = yy.clamp(0, h as i64 - 1);
                        } else if yy < 0 || yy >= h as i64 {
                            continue;
                        }
                        let s = horizontal[(yy as usize - top) * columns + x - left];
                        let weight = 16 * (r as u64 + 1 - dy.unsigned_abs());
                        for c in 0..5 {
                            sum[c] += weight * s[c];
                        }
                    }
                    let p = match sum[3] {
                        0 => [0; 4],
                        alpha => [
                            (sum[0] / alpha) as u8,
                            (sum[1] / alpha) as u8,
                            (sum[2] / alpha) as u8,
                            (alpha / sum[4]) as u8,
                        ],
                    };
                    output[x * 4..x * 4 + 4].copy_from_slice(&p);
                }
            }
        }
    });
    out
}

/// The classic Median's window: dx² + dy² ≤ r² + r (= ⌊((2r + 1)² + 2)/4⌋), row by row.
pub fn disc_offsets(r: u32) -> Vec<(i32, i32)> {
    let r = r as i32;
    (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| dx * dx + dy * dy <= r * r + r)
        .collect()
}

/// Row `y` of a sliding-window filter: `half[i]` is the window's half-width on row y + i − r
/// (r = half.len()/2). `add(state, p, ±1)` adds or removes an in-canvas pixel as the window moves
/// right; `emit(state, x)` reads it at each x.
fn slide<S>(
    img: &ImageData,
    y: usize,
    half: &[i32],
    s: &mut S,
    add: impl Fn(&mut S, Px, i32),
    mut emit: impl FnMut(&S, usize),
) {
    let (w, h) = (img.width as i32, img.height as i32);
    let r = (half.len() / 2) as i32;
    let src = pixels(img);
    let put = |s: &mut S, x: i32, y: i32, sign: i32| {
        if (0..w).contains(&x) && (0..h).contains(&y) {
            add(s, src[(y * w + x) as usize], sign);
        }
    };
    for x in 0..w {
        for (i, &k) in half.iter().enumerate() {
            let yy = y as i32 + i as i32 - r;
            if x == 0 {
                (-k..=k).for_each(|xx| put(s, xx, yy, 1));
            } else {
                put(s, x - 1 - k, yy, -1);
                put(s, x + k, yy, 1);
            }
        }
        emit(s, x as usize);
    }
}

/// artistic-oil-painting-plus.md "Median" (common.md §5.9): per channel over the `disc_offsets(r)`
/// window cut at the canvas, with n pixels and k = ⌊n·p/100⌋, one more than the k-th smallest
/// value (capped at 255), or the smallest when k = 0. The spec's "alpha is exactly 255 when every
/// window pixel is opaque" follows from the cap. Sliding 256-bin histograms.
// quirk(bb): artistic-oil-painting-plus.md "Median" (+1)
// spec-question: Q10 Classic Median "k-th smallest"
pub fn median(img: &ImageData, r: u32, percentile: u32) -> ImageData {
    rank_filter(img, r, percentile, MEDIAN_RANK_BASE)
}

fn rank_filter(img: &ImageData, r: u32, percentile: u32, base: usize) -> ImageData {
    let disc = disc_offsets(r);
    let half: Vec<i32> = (-(r as i32)..=r as i32)
        .map(|dy| {
            disc.iter()
                .filter(|o| o.1 == dy)
                .map(|o| o.0)
                .max()
                .unwrap_or(0)
        })
        .collect();
    by_rows(img.width, img.height, |y, row| {
        let mut s = ([[0i32; 256]; 4], 0i32);
        let add = |(hist, n): &mut ([[i32; 256]; 4], i32), p: Px, sign| {
            (0..4).for_each(|c| hist[c][p[c] as usize] += sign);
            *n += sign;
        };
        slide(img, y, &half, &mut s, add, |(hist, n), x| {
            let n = *n as usize;
            let k = n * percentile as usize / 100;
            let target = (k + 1).saturating_sub(base).clamp(1, n);
            row[x] = std::array::from_fn(|c| {
                let mut seen = 0;
                let v = hist[c]
                    .iter()
                    .position(|&m| {
                        seen += m as usize;
                        seen >= target
                    })
                    .unwrap_or(255) as u8;
                if k >= 1 { v.saturating_add(1) } else { v }
            });
        });
    })
}

/// artistic-oil-painting-plus.md "Oil Painting": over the (2b + 1)² window cut at the canvas, the
/// pixels fall into c + 1 buckets by (intensity·c + 127)/255; the fullest bucket (the lowest on a
/// tie) gives the truncated mean of its R, G, B and A.
pub fn oil_painting(img: &ImageData, brush: u32, coarseness: u32) -> ImageData {
    let c = coarseness as usize;
    let half = vec![brush as i32; 2 * brush as usize + 1];
    by_rows(img.width, img.height, |y, row| {
        let mut s = (vec![0i32; c + 1], vec![[0i32; 4]; c + 1]);
        let add = |(count, sum): &mut (Vec<i32>, Vec<[i32; 4]>), p: Px, sign| {
            let b = (intensity(p) as usize * c + 127) / 255;
            count[b] += sign;
            (0..4).for_each(|k| sum[b][k] += sign * p[k] as i32);
        };
        slide(img, y, &half, &mut s, add, |(count, sum), x| {
            let b = (0..=c).fold(0, |m, i| if count[i] > count[m] { i } else { m });
            row[x] = sum[b].map(|v| (v / count[b]) as u8);
        });
    })
}

/// artistic-oil-painting-plus.md "Pencil Sketch": `gaussian_blur(t)`, Brightness/Contrast (g, −g),
/// inverted, desaturated to its intensity, then Paint.NET's Color Dodge of that (top) over the
/// intensity-desaturated input (bottom).
// approximation: Paint.NET 4.2 Pencil Sketch alpha propagation is inferred from the
// classic blur and Color Dodge path (artistic-pastel.md Notes / uncertainties).
pub fn pencil_sketch(img: &ImageData, tip: u32, range: i32) -> ImageData {
    let top = brightness_contrast(&gaussian_blur(img, tip), range, -range);
    let data = pixels(img)
        .iter()
        .zip(pixels(&top))
        .flat_map(|(&p, &q)| {
            let i = intensity([255 - q[0], 255 - q[1], 255 - q[2], q[3]]);
            let d = intensity(p);
            compose(BlendMode::ColorDodge, [d, d, d, p[3]], [i, i, i, q[3]], 255)
        })
        .collect();
    ImageData {
        width: img.width,
        height: img.height,
        data,
    }
}

/// artistic-oil-painting-plus.md step 1 (artistic-pastel.md step 3): f64 HSV (V = max/255,
/// S = (max − min)/max), the hue snapped to 10° (ties to even, 360 → 0), V′ = min(V/3 + 0.8, 1),
/// back through the sector HSV → RGB with each channel truncated. Alpha kept.
// quirk(bb): artistic-oil-painting-plus.md step 1 (0.8 one ulp low)
// spec-question: Q17 Pastel recolour table vs formula
// spec-question: Q21 Pastel recolour hue arithmetic
pub fn pastel_recolour(p: Px) -> Px {
    let f = |c: u8| {
        if PASTEL_HSV_FROM_BYTES {
            c as f64
        } else {
            c as f64 / 255.0
        }
    };
    let (r, g, b) = (f(p[0]), f(p[1]), f(p[2]));
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let (h, s) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let h = 60.0
            * if r == mx {
                (g - b) / d
            } else if g == mx {
                2.0 + (b - r) / d
            } else {
                4.0 + (r - g) / d
            };
        (if h < 0.0 { h + 360.0 } else { h }, d / mx)
    };
    let h = 10.0 * (h / 10.0).round_ties_even() % 360.0;
    let v = (p[0].max(p[1]).max(p[2]) as f64 / 255.0 / 3.0 + PASTEL_VALUE_OFFSET).min(1.0);
    let [r, g, b] = hsv_to_unit_rgb(h, s, v).map(|x| (x * 255.0) as u8);
    [r, g, b, p[3]]
}

/// photo-sharpen-landscape.md step 4: Paint.NET's classic Hue/Saturation op at hue 0 and lightness
/// 0. Each channel moves away from the intensity I by ⌊s·1024/100⌋/1024 (floored, clamped), then
/// through the integer HSV round trip. Alpha kept.
// spec-question: Q15 HSV round trip (photo-sharpen-landscape.md Notes)
pub fn hue_saturation(p: Px, saturation_pct: i32) -> Px {
    let [r, g, b] = saturate(p, saturation_pct);
    let [r, g, b] = if CLASSIC_HSV_ROUND_TRIP {
        HsvColor::from_rgb(r, g, b).to_rgb()
    } else {
        [r, g, b]
    };
    [r, g, b, p[3]]
}

/// The op's fixed-point saturation step: c′ = clamp(I + ⌊(c − I)·⌊s·1024/100⌋ / 1024⌋, 0, 255).
fn saturate(p: Px, saturation_pct: i32) -> [u8; 3] {
    let i = intensity(p) as i32;
    let factor = saturation_pct * 1024 / 100;
    let ch = |c: u8| (i + (((c as i32 - i) * factor) >> 10)).clamp(0, 255) as u8;
    [ch(p[0]), ch(p[1]), ch(p[2])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, solid};

    /// common.md §5.9 "Intensity"; object-paste-alpha.md Expected results 2 (149, not 150).
    #[test]
    fn fixed_point_intensity() {
        assert_eq!(
            [
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [128, 128, 128, 255]
            ]
            .map(intensity),
            [76, 149, 29, 128]
        );
    }

    /// blurs-gaussian-blur-plus.md Building block: normalize once after both axes.
    #[test]
    fn gaussian_exact_opaque_counterexample() {
        let gray = from_fn(2, 2, |x, y| {
            let v = [[0, 2], [0, 8]][y as usize][x as usize];
            [v, v, v, 255]
        });
        assert_eq!(px(&gaussian_blur(&gray, 1), 0, 0), [1, 1, 1, 255]);
    }

    /// blurs-gaussian-blur-plus.md Building block: retain weighted alpha before the final division.
    #[test]
    fn gaussian_exact_alpha_counterexample() {
        let alpha = from_fn(2, 2, |x, y| match (x, y) {
            (0, 0) => [255, 0, 0, 2],
            (0, 1) => [0, 0, 255, 5],
            _ => [0; 4],
        });
        assert_eq!(px(&gaussian_blur(&alpha, 1), 0, 0), [113, 0, 141, 2]);
    }

    // Independent reference: direct 2-D taps with the common factor 256 cancelled.
    fn gaussian_oracle(img: &ImageData, radius: u32, clamp: bool) -> ImageData {
        if radius == 0 {
            return img.clone();
        }
        let r = radius as i32;
        from_fn(img.width, img.height, |x, y| {
            let mut sum = [0u64; 5];
            for dy in -r..=r {
                for dx in -r..=r {
                    let (mut xx, mut yy) = (x as i32 + dx, y as i32 + dy);
                    if clamp {
                        xx = xx.clamp(0, img.width as i32 - 1);
                        yy = yy.clamp(0, img.height as i32 - 1);
                    } else if xx < 0 || yy < 0 || xx >= img.width as i32 || yy >= img.height as i32
                    {
                        continue;
                    }
                    let p = px(img, xx as u32, yy as u32);
                    let w = ((r + 1 - dx.abs()) * (r + 1 - dy.abs())) as u64;
                    sum[4] += w;
                    sum[3] += w * p[3] as u64;
                    for c in 0..3 {
                        sum[c] += w * p[3] as u64 * p[c] as u64;
                    }
                }
            }
            match sum[3] {
                0 => [0; 4],
                alpha => [
                    (sum[0] / alpha) as u8,
                    (sum[1] / alpha) as u8,
                    (sum[2] / alpha) as u8,
                    (alpha / sum[4]) as u8,
                ],
            }
        })
    }

    /// blurs-gaussian-blur-plus.md Building block; Q20: both edge choices, low alpha, strip boundaries.
    #[test]
    fn gaussian_matches_direct_2d() {
        for (w, h, r) in [
            (1, 1, 100),
            (1, 7, 2),
            (7, 1, 2),
            (5, 7, 1),
            (9, 8, 3),
            (67, 69, 2),
            (111, 137, 7),
            (3, 2, 100),
        ] {
            let img = from_fn(w, h, |x, y| {
                [
                    (x * 73 + y * 29) as u8,
                    (x * 19 + y * 37) as u8,
                    (x * 41 + y * 13) as u8,
                    [0, 1, 2, 8, 127, 255][((x + 2 * y) % 6) as usize],
                ]
            });
            for clamp in [false, true] {
                assert_eq!(
                    tent_blur(&img, r, clamp).data,
                    gaussian_oracle(&img, r, clamp).data,
                    "{w}x{h} radius {r}, clamp {clamp}"
                );
            }
        }
        // An interior radius-100 window reaches the documented worst-case accumulator bound.
        let saturated = solid(203, 203, [255; 4]);
        assert_eq!(gaussian_blur(&saturated, 100).data, saturated.data);
        let hidden = solid(7, 5, [77, 99, 123, 0]);
        assert!(gaussian_blur(&hidden, 3).data.iter().all(|&c| c == 0));
        assert_eq!(gaussian_blur(&hidden, 0).data, hidden.data);
    }

    /// blurs-gaussian-blur-plus.md "Building block": tent weights, alpha-weighted colour, dropped off-canvas taps.
    #[test]
    fn classic_gaussian() {
        let dot = from_fn(9, 9, |x, y| {
            if (x, y) == (4, 4) {
                [200, 200, 200, 255]
            } else {
                [100, 100, 100, 255]
            }
        });
        let b = gaussian_blur(&dot, 1);
        assert_eq!(
            [
                px(&b, 4, 4)[0],
                px(&b, 4, 3)[0],
                px(&b, 3, 3)[0],
                px(&b, 0, 0)[0]
            ],
            [125, 112, 106, 100]
        );
        assert_eq!(gaussian_blur(&dot, 0).data, dot.data);
        let edge = from_fn(3, 1, |x, _| [if x == 2 { 255 } else { 0 }, 0, 0, 255]);
        assert_eq!(px(&gaussian_blur(&edge, 1), 2, 0)[0], 170);
        let half = from_fn(2, 1, |x, _| {
            if x == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        assert_eq!(px(&gaussian_blur(&half, 1), 1, 0), [255, 255, 255, 85]);
        assert_eq!(
            px(&gaussian_blur(&solid(3, 3, [0, 0, 0, 0]), 2), 1, 1),
            [0, 0, 0, 0]
        );
    }

    /// Review Focus 1: radius 100 is separable (two 201-tap passes), not a 201² window.
    #[test]
    fn classic_gaussian_radius_100_is_separable() {
        let img = from_fn(300, 300, |x, y| [(x % 256) as u8, (y % 256) as u8, 7, 255]);
        let t = std::time::Instant::now();
        let _ = gaussian_blur(&img, 100);
        assert!(t.elapsed().as_secs_f64() < 20.0);
    }

    /// artistic-oil-painting-plus.md "Median": a flat v comes out v + 1; transparent areas get alpha 1; an
    /// all-opaque window keeps alpha 255.
    #[test]
    fn classic_median_quirks() {
        assert_eq!(
            px(&median(&solid(9, 9, [100, 50, 254, 255]), 2, 50), 4, 4),
            [101, 51, 255, 255]
        );
        assert_eq!(
            px(&median(&solid(9, 9, [0, 0, 0, 0]), 2, 50), 4, 4),
            [1, 1, 1, 1]
        );
        assert_eq!(disc_offsets(1).len(), 9);
        assert_eq!(disc_offsets(2).len(), 21);
    }

    /// artistic-oil-painting-plus.md step 1 tables (pastel recolour).
    #[test]
    fn pastel_recolour_tables() {
        // spec-question Q17: the descriptive table's 150 and 153 rows contradict the stated formula; not pinned.
        for (inp, out) in [
            (0, 203),
            (1, 204),
            (3, 204),
            (30, 213),
            (51, 220),
            (64, 225),
            (102, 237),
            (128, 246),
            (151, 254),
            (152, 254),
            (255, 255),
        ] {
            assert_eq!(
                pastel_recolour([inp, inp, inp, 255]),
                [out, out, out, 255],
                "grey {inp}"
            );
        }
        for (i, o) in [
            ([255, 64, 0], [255, 84, 0]),
            ([200, 100, 50], [255, 127, 63]),
            ([0, 128, 255], [0, 127, 255]),
            ([10, 200, 30], [12, 255, 53]),
            ([255, 0, 128], [255, 0, 127]),
        ] {
            assert_eq!(
                pastel_recolour([i[0], i[1], i[2], 9]),
                [o[0], o[1], o[2], 9]
            );
        }
    }

    /// artistic-oil-painting-plus.md "Oil Painting" and "Pencil Sketch": flat areas.
    #[test]
    fn oil_painting_and_pencil_sketch_on_flat_areas() {
        assert_eq!(
            px(&oil_painting(&solid(9, 9, [10, 200, 30, 255]), 3, 50), 4, 4),
            [10, 200, 30, 255]
        );
        let two = from_fn(3, 1, |x, _| {
            if x < 2 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        assert_eq!(px(&oil_painting(&two, 1, 50), 1, 0), [0, 0, 0, 255]);
        assert_eq!(
            px(
                &pencil_sketch(&solid(9, 9, [128, 128, 128, 255]), 2, 0),
                4,
                4
            ),
            [255, 255, 255, 255]
        );
        assert_eq!(
            px(&pencil_sketch(&solid(9, 9, [0, 0, 0, 0]), 2, 0), 4, 4),
            [0, 0, 0, 0]
        );
    }

    /// stylize-floyd-steinberg-dithering.md step 1: integer B/C; contrast 100 thresholds at I + b ≥ 128.
    #[test]
    fn classic_brightness_contrast() {
        let g = solid(1, 1, [128, 128, 128, 255]);
        assert_eq!(brightness_contrast(&g, 0, 0).data, g.data);
        assert_eq!(
            px(&brightness_contrast(&g, 100, 0), 0, 0),
            [228, 228, 228, 255]
        );
        assert_eq!(
            px(
                &brightness_contrast(&solid(1, 1, [127, 127, 127, 255]), 0, 100),
                0,
                0
            ),
            [0, 0, 0, 255]
        );
        assert_eq!(
            px(&brightness_contrast(&g, 0, 100), 0, 0),
            [255, 255, 255, 255]
        );
    }

    /// photo-sharpen-landscape.md step 4: 100 % changes nothing; greys stay grey at any saturation.
    #[test]
    fn classic_hue_saturation_op() {
        assert_eq!(hue_saturation([0, 51, 255, 255], 100), [0, 51, 255, 255]);
        assert_eq!(hue_saturation([204, 204, 204, 7], 400), [204, 204, 204, 7]);
    }

    /// photo-sharpen-landscape.md step 4: the fixed-point step floors (130 %: ⌊−31.2⌋ = −32) and
    /// clamps; the integer HSV round trip then quantises (64 → 63).
    #[test]
    fn classic_hue_saturation_steps() {
        assert_eq!(saturate([200, 100, 50, 255], 130), [222, 92, 27]);
        assert_eq!(saturate([200, 100, 50, 255], 250), [255, 64, 0]);
        assert_eq!(hue_saturation([200, 100, 50, 9], 250), [255, 63, 0, 9]);
    }

    /// stylize-floyd-steinberg-dithering.md step 1: negative contrast truncates toward zero, positive
    /// contrast adds the brightness before scaling, one shift moves all three channels, and the
    /// threshold counts the brightness.
    #[test]
    fn classic_brightness_contrast_branches() {
        let bc = |p: Px, b, c| px(&brightness_contrast(&solid(1, 1, p), b, c), 0, 0);
        let g = |v: u8| [v, v, v, 255];
        assert_eq!(bc(g(100), 0, -50), g(114));
        assert_eq!(bc(g(200), 10, -50), g(173));
        assert_eq!(bc(g(100), 0, 50), g(73));
        assert_eq!(bc(g(100), 10, 50), g(93));
        assert_eq!(bc(g(100), 0, 30), g(89));
        assert_eq!(bc([250, 10, 100, 7], 20, 0), [255, 30, 120, 7]);
        assert_eq!(bc(g(127), 1, 100), g(255));
    }

    /// Q20's other reading (`CLASSIC_BLUR_CLAMP_EDGES`): clamped taps repeat the edge pixel, in x and
    /// in y.
    #[test]
    fn classic_gaussian_edge_conventions() {
        let corner = from_fn(3, 3, |x, y| {
            [if (x, y) == (2, 2) { 255 } else { 0 }, 0, 0, 255]
        });
        assert_eq!(px(&tent_blur(&corner, 1, true), 2, 2)[0], 143);
        assert_eq!(px(&tent_blur(&corner, 1, false), 2, 2)[0], 113);
    }

    /// blurs-gaussian-blur-plus.md Expected results 4–5 at block level: transparent neighbours lend
    /// no colour, alpha falls off over r px outside the square, and pixels further out stay
    /// #00000000.
    #[test]
    fn classic_gaussian_square_on_transparent() {
        let inside = |v: u32| (10..30).contains(&v);
        let sq = from_fn(40, 40, |x, y| {
            if inside(x) && inside(y) {
                [255; 4]
            } else {
                [0; 4]
            }
        });
        let b = gaussian_blur(&sq, 5);
        assert!(b.data.chunks(4).all(|p| p[3] == 0 || p[..3] == [255; 3]));
        let row: Vec<u8> = (3..=15).map(|x| px(&b, x, 20)[3]).collect();
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "{row:?}");
        assert_eq!((row[1], row[12]), (0, 255));
        assert!(row[2] > 0);
        assert_eq!(px(&b, 4, 20), [0; 4]);
        // Radius 0 copies, even a transparent pixel's colour.
        assert_eq!(
            gaussian_blur(&solid(1, 1, [9, 8, 7, 0]), 0).data,
            [9, 8, 7, 0]
        );
    }

    fn noisy(w: u32, h: u32) -> ImageData {
        from_fn(w, h, |x, y| {
            let b = (x + 29 * y + 3).wrapping_mul(2_654_435_761).to_be_bytes();
            [b[0] & 0xF0, b[1], b[2], [0, 255, b[3]][(b[3] % 3) as usize]]
        })
    }

    /// The sliding windows against a direct pass over each window: the median at several radii
    /// and percentiles (0 gives the minimum, no + 1), the oil painting at two coarsenesses, with
    /// the windows cut at every edge.
    #[test]
    fn sliding_windows_match_direct_windows() {
        let (w, h) = (23, 17);
        let img = noisy(w, h);
        let window = |x: u32, y: u32, offs: &[(i32, i32)]| -> Vec<Px> {
            let at = |(dx, dy): (i32, i32)| {
                let (u, v) = (x as i32 + dx, y as i32 + dy);
                ((0..w as i32).contains(&u) && (0..h as i32).contains(&v))
                    .then(|| px(&img, u as u32, v as u32))
            };
            offs.iter().filter_map(|&o| at(o)).collect()
        };
        for (r, p) in [(0, 50), (1, 0), (1, 25), (2, 50), (3, 25), (4, 100)] {
            let m = median(&img, r, p);
            for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
                let win = window(x, y, &disc_offsets(r));
                let k = win.len() * p as usize / 100;
                let want: Px = std::array::from_fn(|c| {
                    let mut v: Vec<u8> = win.iter().map(|q| q[c]).collect();
                    v.sort();
                    if k == 0 {
                        v[0]
                    } else {
                        v[k - 1].saturating_add(1)
                    }
                });
                assert_eq!(px(&m, x, y), want, "median r {r} p {p} at ({x}, {y})");
            }
        }
        for (b, c) in [(2, 7), (1, 50)] {
            let o = oil_painting(&img, b, c);
            let b = b as i32;
            let square: Vec<_> = (-b..=b)
                .flat_map(|dy| (-b..=b).map(move |dx| (dx, dy)))
                .collect();
            for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
                let win = window(x, y, &square);
                let bucket = |q: &Px| (intensity(*q) as u32 * c + 127) / 255;
                let count = |k| win.iter().filter(|q| bucket(q) == k).count();
                let best = (0..=c)
                    .max_by_key(|&k| (count(k), std::cmp::Reverse(k)))
                    .unwrap();
                let sel: Vec<_> = win.iter().filter(|q| bucket(q) == best).collect();
                let want: Px = std::array::from_fn(|i| {
                    (sel.iter().map(|q| q[i] as usize).sum::<usize>() / sel.len()) as u8
                });
                assert_eq!(px(&o, x, y), want, "oil b {b} c {c} at ({x}, {y})");
            }
        }
    }

    /// Q10's other reading (`MEDIAN_RANK_BASE` 0) takes v[k] + 1, and k = n then clamps to the
    /// largest value.
    #[test]
    fn classic_median_rank_conventions() {
        let ramp = from_fn(3, 3, |x, y| {
            let v = (10 * (3 * y + x)) as u8;
            [v, v, v, 255]
        });
        assert_eq!(px(&rank_filter(&ramp, 1, 50, 1), 1, 1)[0], 31);
        assert_eq!(px(&rank_filter(&ramp, 1, 50, 0), 1, 1)[0], 41);
        assert_eq!(px(&rank_filter(&ramp, 1, 100, 0), 1, 1)[0], 81);
        assert_eq!(disc_offsets(0), [(0, 0)]);
    }

    /// Ties in the oil painting's buckets go to the lowest bucket; the bucket rounds I·c/255 to
    /// nearest (I 91 at c 7 is 2.498 → bucket 2, joining grey 60 against grey 100).
    #[test]
    fn oil_painting_buckets() {
        let bw = from_fn(2, 1, |x, _| if x == 0 { [0, 0, 0, 255] } else { [255; 4] });
        let o = oil_painting(&bw, 1, 50);
        assert_eq!(
            (px(&o, 0, 0), px(&o, 1, 0)),
            ([0, 0, 0, 255], [0, 0, 0, 255])
        );
        let row = from_fn(3, 1, |x, _| {
            let v = [60, 91, 100][x as usize];
            [v, v, v, 255]
        });
        assert_eq!(px(&oil_painting(&row, 1, 7), 1, 0), [75, 75, 75, 255]);
    }

    /// artistic-oil-painting-plus.md Expected results 1's derivation at block level (the sketch of
    /// the smoothed grey 129 at tip 3, range 6 is 245), and artistic-pastel.md Expected results 1
    /// (black gives an all-white sketch, so no pencil).
    #[test]
    fn pencil_sketch_of_flat_areas() {
        let s = pencil_sketch(&solid(9, 9, [129, 129, 129, 255]), 3, 6);
        assert_eq!(px(&s, 4, 4), [245, 245, 245, 255]);
        let s = pencil_sketch(&solid(9, 9, [0, 0, 0, 255]), 2, 0);
        assert!(s.data.chunks(4).all(|p| p == [255; 4]));
    }

    /// artistic-pastel.md Expected results 4 at block level: a black | white edge gives a black line
    /// on the dark side, as wide as the blur reaches (tip 2: x = 6, 7), white elsewhere.
    #[test]
    fn pencil_sketch_draws_edges() {
        let half = from_fn(16, 4, |x, _| if x < 8 { [0, 0, 0, 255] } else { [255; 4] });
        let s = pencil_sketch(&half, 2, 0);
        let row: Vec<u8> = (0..16).map(|x| px(&s, x, 2)[0]).collect();
        let want: Vec<u8> = (0..16)
            .map(|x| if x == 6 || x == 7 { 0 } else { 255 })
            .collect();
        assert_eq!(row, want);
    }

    /// The recolour's edges: an exact 25° hue goes to 20° (ties to even), 357.6° wraps to 0°, −20°
    /// is taken as 340°, and a Q21 pin (205° exactly from the bytes → 200°; 205.000…03 from c/255 would give 210°).
    #[test]
    fn pastel_recolour_ties_and_wrap() {
        assert_eq!(pastel_recolour([252, 105, 0, 255]), [255, 84, 0, 255]);
        assert_eq!(pastel_recolour([255, 0, 10, 255]), [255, 0, 0, 255]);
        assert_eq!(pastel_recolour([0, 77, 132, 255]), [0, 165, 248, 255]);
        // A negative hue is wrapped into 0…360 before the sector maths (#080506: −20° → 340°;
        // sector −1 instead would give blue 155).
        assert_eq!(pastel_recolour([8, 5, 6, 255]), [206, 129, 154, 255]);
    }

    /// Review Focus 2: empty, single-pixel and one-row layers with radii past their size.
    #[test]
    fn degenerate_layers() {
        for (w, h) in [(0, 0), (1, 1), (1, 5), (5, 1)] {
            let img = noisy(w, h);
            for out in [
                gaussian_blur(&img, 7),
                median(&img, 7, 50),
                oil_painting(&img, 7, 50),
                pencil_sketch(&img, 7, 20),
                brightness_contrast(&img, -100, 100),
            ] {
                assert_eq!(
                    (out.width, out.height, out.data.len()),
                    (w, h, img.data.len())
                );
            }
        }
    }
}
