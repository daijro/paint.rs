//! Per-pixel sources that Chromium's software canvas (Skia) computes differently from tiny-skia.
//!
//! Dithered gradients (Skia dithers gradient shaders; pinned exactly by `linear-gradient` and
//! `radial-gradient-two-point`).
//!
//! drawImage samplers, pinned exactly by the `draw-image-smooth-*` canvas cases:
//! - "low": Skia's legacy `SkBitmapProcState` bilinear (4-bit weights, integer floor);
//! - "medium", and "high" when downscaling: bilinear with linear mipmaps (`SkMipmap` levels,
//!   `SkMipmap::ComputeLevel`), in float;
//! - "high" when upscaling: Mitchell bicubic, which tiny-skia's `Bicubic` already is.
use tiny_skia::{IntRect, Pixmap};

/// A 2D affine map in f64, `[a, b, c, d, e, f]` like the CTM.
pub(super) type Matrix = [f64; 6];

pub(super) fn invert(m: &Matrix) -> Option<Matrix> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    Some([
        m[3] / det,
        -m[1] / det,
        -m[2] / det,
        m[0] / det,
        (m[2] * m[5] - m[3] * m[4]) / det,
        (m[1] * m[4] - m[0] * m[5]) / det,
    ])
}

/// Premultiplied color (0..=1) at gradient position `t` (padded): stops interpolate
/// unpremultiplied, like Skia without `kInterpolateColorsInPremul`.
pub(super) fn gradient_color(stops: &[(f64, [u8; 4])], t: f64) -> [f64; 4] {
    let t = t.clamp(0.0, 1.0);
    let c = |s: &[u8; 4]| s.map(|v| v as f64 / 255.0);
    let i = stops.partition_point(|s| s.0 < t);
    let rgba = match (i.checked_sub(1).map(|j| &stops[j]), stops.get(i)) {
        (Some(a), Some(b)) if b.0 > a.0 => {
            let u = (t - a.0) / (b.0 - a.0);
            let (ca, cb) = (c(&a.1), c(&b.1));
            std::array::from_fn(|k| ca[k] + (cb[k] - ca[k]) * u)
        }
        (_, Some(b)) => c(&b.1),
        (Some(a), None) => c(&a.1),
        (None, None) => [0.0; 4],
    };
    [
        rgba[0] * rgba[3],
        rgba[1] * rgba[3],
        rgba[2] * rgba[3],
        rgba[3],
    ]
}

/// Skia's 8×8 ordered `dither` stage (rate 1/255) on a premultiplied color at device (x, y),
/// clamped to alpha, then rounded to bytes.
pub(super) fn dither(px: [f64; 4], x: i32, y: i32) -> [u8; 4] {
    let (x, y) = (x as u32, (y ^ x) as u32);
    let m = (y & 1) << 5 | (x & 1) << 4 | (y & 2) << 2 | (x & 2) << 1 | (y & 4) >> 1 | (x & 4) >> 2;
    let d = (m as f64 * 2.0 / 128.0 - 63.0 / 128.0) / 255.0;
    let a = px[3];
    std::array::from_fn(|k| {
        let v = if k == 3 { a } else { (px[k] + d).clamp(0.0, a) };
        (v * 255.0).round() as u8
    })
}

fn texel(img: &Pixmap, x: f64, y: f64) -> [u8; 4] {
    let x = x.clamp(0.0, img.width() as f64 - 1.0) as usize;
    let y = y.clamp(0.0, img.height() as f64 - 1.0) as usize;
    let i = (y * img.width() as usize + x) * 4;
    img.data()[i..i + 4].try_into().unwrap()
}

/// `SkBitmapProcState` bilinear with clamp tiling: 4-bit subpixel weights, `>> 8`.
pub(super) fn legacy_bilinear(img: &Pixmap, u: f64, v: f64) -> [u8; 4] {
    let (u, v) = (u - 0.5, v - 0.5);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (((u - x0) * 16.0) as u32, ((v - y0) * 16.0) as u32);
    let taps = [
        (texel(img, x0, y0), (16 - fx) * (16 - fy)),
        (texel(img, x0 + 1.0, y0), fx * (16 - fy)),
        (texel(img, x0, y0 + 1.0), (16 - fx) * fy),
        (texel(img, x0 + 1.0, y0 + 1.0), fx * fy),
    ];
    std::array::from_fn(|c| (taps.iter().map(|(p, w)| p[c] as u32 * w).sum::<u32>() >> 8) as u8)
}

/// Raster-pipeline bilinear with clamp tiling, unrounded.
fn bilinear(img: &Pixmap, u: f64, v: f64) -> [f64; 4] {
    let (u, v) = (u - 0.5, v - 0.5);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let taps = [
        (texel(img, x0, y0), (1.0 - fx) * (1.0 - fy)),
        (texel(img, x0 + 1.0, y0), fx * (1.0 - fy)),
        (texel(img, x0, y0 + 1.0), (1.0 - fx) * fy),
        (texel(img, x0 + 1.0, y0 + 1.0), fx * fy),
    ];
    std::array::from_fn(|c| taps.iter().map(|(p, w)| p[c] as f64 * w).sum())
}

/// `SkMipmap::ComputeLevel` for the smaller axis scale; the −0.5 bias emulates the GPU's
/// sharpen-mipmap option. 0 when not downscaling.
pub(super) fn mip_level(scale: f64) -> f64 {
    let scale = scale as f32;
    if !(scale > 0.0 && scale < 1.0) {
        return 0.0;
    }
    (-scale.log2() - 0.5).max(0.0) as f64
}

/// `SkMipmap` levels 0..=`count`: each halves (floored, min 1) with Skia's downsamplers:
/// a 2-tap box per even axis, 1-2-1 per odd axis, shifts truncating.
pub(super) fn mip_chain(base: &Pixmap, count: usize) -> Vec<Pixmap> {
    let mut levels = vec![base.clone()];
    while levels.len() <= count {
        let src = levels.last().unwrap();
        if src.width() == 1 && src.height() == 1 {
            break;
        }
        let taps = |n: u32| -> &'static [(usize, u32)] {
            match n {
                1 => &[(0, 1)],
                n if n % 2 == 0 => &[(0, 1), (1, 1)],
                _ => &[(0, 1), (1, 2), (2, 1)],
            }
        };
        let (tx, ty) = (taps(src.width()), taps(src.height()));
        let shift = (tx.iter().map(|t| t.1).sum::<u32>() * ty.iter().map(|t| t.1).sum::<u32>())
            .trailing_zeros();
        let (w, h) = ((src.width() / 2).max(1), (src.height() / 2).max(1));
        let mut out = Pixmap::new(w, h).unwrap();
        let (sw, s) = (src.width() as usize, src.data());
        let d = out.data_mut();
        for y in 0..h as usize {
            for x in 0..w as usize {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for &(oy, wy) in ty {
                        for &(ox, wx) in tx {
                            sum += s[((2 * y + oy) * sw + 2 * x + ox) * 4 + c] as u32 * wx * wy;
                        }
                    }
                    d[(y * w as usize + x) * 4 + c] = (sum >> shift) as u8;
                }
            }
        }
        levels.push(out);
    }
    levels
}

/// Bilinear on the two levels around `level`, blended linearly; `(u, v)` are level-0 coordinates.
pub(super) fn mip_linear(levels: &[Pixmap], u: f64, v: f64, level: f64) -> [u8; 4] {
    let at = |i: usize| {
        let l = &levels[i.min(levels.len() - 1)];
        let (kx, ky) = (
            l.width() as f64 / levels[0].width() as f64,
            l.height() as f64 / levels[0].height() as f64,
        );
        bilinear(l, u * kx, v * ky)
    };
    let (lo, t) = (level.floor() as usize, level.fract());
    let a = at(lo);
    let b = if t > 0.0 { at(lo + 1) } else { a };
    std::array::from_fn(|c| (a[c] + (b[c] - a[c]) * t).round().clamp(0.0, 255.0) as u8)
}

/// A premultiplied pixmap covering device `bbox`: pixel (x, y) is `sample(x, y, u, v)`, (u, v)
/// being the pixel center mapped through `inv` (device → image or user space).
pub(super) fn resample(
    inv: &Matrix,
    bbox: IntRect,
    sample: impl Fn(i32, i32, f64, f64) -> [u8; 4],
) -> Pixmap {
    let mut out = Pixmap::new(bbox.width(), bbox.height()).unwrap();
    let d = out.data_mut();
    let mut i = 0;
    for y in bbox.top()..bbox.bottom() {
        for x in bbox.left()..bbox.right() {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let (u, v) = (
                inv[0] * px + inv[2] * py + inv[4],
                inv[1] * px + inv[3] * py + inv[5],
            );
            d[i..i + 4].copy_from_slice(&sample(x, y, u, v));
            i += 4;
        }
    }
    out
}
