//! Ported from PhotoCraft crates/algo/src/other2.rs (`custom`, `hsb_hsl` and the colour helpers) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: interleaved straight RGBA f32 buffers (w×h, alpha last) instead of `Image`/`Rect`/`Ctx`; samples outside the layer repeat the nearest edge pixel (upstream reads the image clamped to the canvas); the CMYK and non-RGB colour-model branches dropped; the `custom_*` and `hsb_hsl_round_trip` tests rewritten on RGBA buffers (the depth and CMYK cases drop out with 8-bit RGBA only).

use crate::extra::interrupt::Interrupt;

/// Custom: `Σ kernel·pixel / scale + offset/255` per colour channel (alpha kept). Kernels
/// shorter than 25 entries are zero-padded; entries past 25 are ignored.
pub fn custom(
    src: &[f32],
    w: usize,
    h: usize,
    kernel: &[f32],
    scale: f32,
    offset: f32,
    int: Interrupt,
) -> Vec<f32> {
    let k: Vec<(i32, i32, f32)> = (0..25)
        .filter_map(|i| {
            kernel
                .get(i)
                .copied()
                .filter(|v| *v != 0.0)
                .map(|v| (i as i32 % 5 - 2, i as i32 / 5 - 2, v))
        })
        .collect();
    let s = if scale == 0.0 { 1.0 } else { scale };
    let off = offset / 255.0;
    let mut res = src.to_vec();
    if k.is_empty() || w == 0 || h == 0 {
        return res;
    }
    let at = |x: i32, y: i32, c: usize| {
        let (x, y) = (
            x.clamp(0, w as i32 - 1) as usize,
            y.clamp(0, h as i32 - 1) as usize,
        );
        src[(y * w + x) * 4 + c]
    };
    for y in 0..h {
        // A cancelled run keeps the source for the unvisited rows.
        if int.cancelled() {
            break;
        }
        for x in 0..w {
            for c in 0..3 {
                let sum: f32 = k
                    .iter()
                    .map(|&(dx, dy, wt)| at(x as i32 + dx, y as i32 + dy, c) * wt)
                    .sum();
                res[(y * w + x) * 4 + c] = sum / s + off;
            }
        }
    }
    res
}

/// A colour space the three colour channels are read or written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HsbModel {
    Rgb,
    Hsb,
    Hsl,
}

/// RGB to HSB (hue, saturation, brightness), all 0-1.
pub fn rgb_to_hsb(c: [f32; 3]) -> [f32; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let d = mx - mn;
    [hue(c, mx, d), if mx > 0.0 { d / mx } else { 0.0 }, mx]
}

/// RGB to HSL, all 0-1.
pub fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let (mx, mn) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let d = mx - mn;
    let l = (mx + mn) / 2.0;
    let s = if d <= 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-7)
    };
    [hue(c, mx, d), s.clamp(0.0, 1.0), l]
}

fn hue(c: [f32; 3], mx: f32, d: f32) -> f32 {
    if d <= 0.0 {
        return 0.0;
    }
    let h = if mx == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    h / 6.0
}

/// RGB from hue, chroma and the value to add.
fn from_hcm(h: f32, chroma: f32, m: f32) -> [f32; 3] {
    let hp = h.rem_euclid(1.0) * 6.0;
    let x = chroma * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    [r + m, g + m, b + m]
}

pub fn hsb_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let chroma = c[2] * c[1];
    from_hcm(c[0], chroma, c[2] - chroma)
}

pub fn hsl_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let chroma = (1.0 - (2.0 * c[2] - 1.0).abs()) * c[1];
    from_hcm(c[0], chroma, c[2] - chroma / 2.0)
}

/// HSB/HSL: reinterprets the three colour channels, encoding RGB as H, S, B/L in the R, G, B
/// channels, or decoding such channels back to RGB. Alpha is kept.
pub fn hsb_hsl(buf: &mut [f32], input: HsbModel, output: HsbModel) {
    if input == output {
        return;
    }
    for px in buf.as_chunks_mut::<4>().0 {
        let c = [
            px[0].clamp(0.0, 1.0),
            px[1].clamp(0.0, 1.0),
            px[2].clamp(0.0, 1.0),
        ];
        let rgb = match input {
            HsbModel::Rgb => c,
            HsbModel::Hsb => hsb_to_rgb(c),
            HsbModel::Hsl => hsl_to_rgb(c),
        };
        let o = match output {
            HsbModel::Rgb => rgb,
            HsbModel::Hsb => rgb_to_hsb(rgb),
            HsbModel::Hsl => rgb_to_hsl(rgb),
        };
        px[..3].copy_from_slice(&o);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 5;
    const H: usize = 5;

    /// Upstream's fixture (a 5×5 ramp, per-channel offset) as straight RGBA.
    fn fixture() -> Vec<f32> {
        let mut v = Vec::new();
        for y in 0..H {
            for x in 0..W {
                for c in 0..4 {
                    v.push((x + 5 * y + 3 * c) as f32 / 40.0);
                }
            }
        }
        v
    }

    fn px(buf: &[f32], x: usize, y: usize) -> [f32; 4] {
        let i = (y * W + x) * 4;
        buf[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn custom_all_25_taps_use_row_major_offsets_and_repeat_canvas_edges() {
        let src = fixture();
        for tap in 0..25 {
            let mut kernel = vec![0.0; 25];
            kernel[tap] = 1.0;
            let out = custom(&src, W, H, &kernel, 1.0, 0.0, Interrupt::NONE);
            let (dx, dy) = (tap as i32 % 5 - 2, tap as i32 / 5 - 2);
            for y in 0..H {
                for x in 0..W {
                    let (sx, sy) = (
                        (x as i32 + dx).clamp(0, 4) as usize,
                        (y as i32 + dy).clamp(0, 4) as usize,
                    );
                    let (e, a) = (px(&src, sx, sy), px(&out, x, y));
                    assert_eq!(a[..3], e[..3], "tap {tap}");
                    assert_eq!(a[3], px(&src, x, y)[3], "tap={tap}, alpha must not shift");
                }
            }
        }
    }

    #[test]
    fn custom_short_kernel_zero_pads_and_ignores_entries_after_25() {
        let src = fixture();
        let mut short = vec![0.0; 13];
        short[0] = 0.25;
        short[12] = 0.75;
        let mut padded = short.clone();
        padded.resize(25, 0.0);
        let mut long = padded.clone();
        long.extend([1e6, -1e6, f32::NAN]);
        let out = custom(&src, W, H, &short, 1.0, 0.0, Interrupt::NONE);
        assert_eq!(out, custom(&src, W, H, &padded, 1.0, 0.0, Interrupt::NONE));
        assert_eq!(out, custom(&src, W, H, &long, 1.0, 0.0, Interrupt::NONE));
        for y in 0..H {
            for x in 0..W {
                let corner = px(&src, x.saturating_sub(2), y.saturating_sub(2));
                let centre = px(&src, x, y);
                let a = px(&out, x, y);
                for c in 0..3 {
                    assert!((a[c] - (0.25 * corner[c] + 0.75 * centre[c])).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn custom_scale_zero_is_one_and_offset_is_in_255ths() {
        let src = fixture();
        let before = src.clone();
        let mut kernel = vec![0.0; 25];
        kernel[12] = 2.0;
        let out = custom(&src, W, H, &kernel, 0.0, -51.0, Interrupt::NONE);
        assert_eq!(
            out,
            custom(&src, W, H, &kernel, 1.0, -51.0, Interrupt::NONE)
        );
        for y in 0..H {
            for x in 0..W {
                let (o, a) = (px(&src, x, y), px(&out, x, y));
                for c in 0..3 {
                    assert!((a[c] - (2.0 * o[c] - 0.2)).abs() < 1e-6);
                }
                assert_eq!(a[3], o[3]);
            }
        }
        assert_eq!(src, before);
    }

    #[test]
    fn custom_neighbourhood_convolution_preserves_each_pixels_alpha() {
        let src = fixture();
        let out = custom(&src, W, H, &[1.0; 25], 25.0, 0.0, Interrupt::NONE);
        assert_ne!(
            px(&out, 0, 0)[0],
            px(&src, 0, 0)[0],
            "fixture must change colour"
        );
        for y in 0..H {
            for x in 0..W {
                assert_eq!(px(&out, x, y)[3], px(&src, x, y)[3]);
            }
        }
    }

    /// A cancelled convolution leaves the source untouched.
    #[test]
    fn custom_stops_when_cancelled() {
        let src = fixture();
        let flag = std::sync::atomic::AtomicBool::new(true);
        let out = custom(&src, W, H, &[1.0; 25], 25.0, 0.0, Interrupt(Some(&flag)));
        assert_eq!(out, src);
    }

    #[test]
    fn hsb_hsl_round_trip() {
        for c in [
            [0.2, 0.7, 0.4],
            [1.0, 0.0, 0.0],
            [0.5, 0.5, 0.5],
            [0.1, 0.2, 0.9],
            [0.9, 0.8, 0.05],
        ] {
            let a = hsb_to_rgb(rgb_to_hsb(c));
            let b = hsl_to_rgb(rgb_to_hsl(c));
            for k in 0..3 {
                assert!(
                    (a[k] - c[k]).abs() < 1e-5 && (b[k] - c[k]).abs() < 1e-5,
                    "{c:?} {a:?} {b:?}"
                );
            }
        }
        assert_eq!(rgb_to_hsb([1.0, 0.0, 0.0]), [0.0, 1.0, 1.0]);
        let l = rgb_to_hsl([0.0, 0.0, 1.0]);
        assert!((l[0] - 2.0 / 3.0).abs() < 1e-6 && (l[2] - 0.5).abs() < 1e-6);
    }

    /// The buffer form encodes and decodes, and keeps alpha.
    #[test]
    fn hsb_hsl_buffer_round_trips() {
        let src = [0.2, 0.7, 0.4, 0.5, 0.9, 0.1, 0.3, 1.0];
        for m in [HsbModel::Hsb, HsbModel::Hsl] {
            let mut b = src;
            hsb_hsl(&mut b, HsbModel::Rgb, m);
            assert_ne!(b[..3], src[..3]);
            hsb_hsl(&mut b, m, HsbModel::Rgb);
            assert!(b.iter().zip(&src).all(|(a, e)| (a - e).abs() < 1e-5));
        }
    }
}
