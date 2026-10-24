//! The canvas shim's chrome path only: Chromium's CPU compositing of an image at an integer
//! translation (`drawImage(img, x, y)`), for the paintdotjs-look chrome the Canvas-2D shim still
//! emulates (R11). Document pixels never come here: layers composite with Paint.NET's kernels
//! (`pixel::ops`, Task 17c). tiny-skia always runs highp for pattern shaders, so its results
//! differ from Skia by 1..3 premultiplied. Skia's integer paths, modelled against Chromium:
//! - source-over: the sprite blitter (`SkBlendARGB32` for globalAlpha < 1, `SkPMSrcOver` otherwise);
//! - lighter, multiply, screen, darken, lighten, difference, overlay: the raster-pipeline lowp
//!   stages (u16 fixed point, `div255(v) = (v + 255) >> 8`, globalAlpha applied by
//!   `scale_1_float`). Darken, lighten, difference and overlay take their alpha from source-over;
//!   screen uses an exactly rounded div255.
//!
//! Reference: Skia SkColorData.h (`SkBlendARGB32`), SkRasterPipeline_opts.h (lowp blend stages).

use super::CompositeOp;
use tiny_skia::{Mask, Pixmap};

fn div255(v: i64) -> i64 {
    (v + 255) >> 8
}

fn inv(v: i64) -> i64 {
    255 - v
}

fn lerp(from: i64, to: i64, t: i64) -> i64 {
    div255(from * inv(t) + to * t)
}

/// Exactly rounded `v / 255` (screen's colour stage).
fn div255_round(v: i64) -> i64 {
    (v + 127) / 255
}

/// `SkBlendARGB32` for one pixel; `aa` is the 8-bit global alpha.
fn blend_argb32(s: [i64; 4], d: [i64; 4], aa: i64) -> [i64; 4] {
    let src_scale = aa + 1;
    let prod = 0xFFFF - s[3] * src_scale;
    let dst_scale = (prod + (prod >> 8)) >> 8;
    std::array::from_fn(|k| (s[k] * src_scale + d[k] * dst_scale) >> 8)
}

/// `SkPMSrcOver`.
fn pm_src_over(s: [i64; 4], d: [i64; 4]) -> [i64; 4] {
    std::array::from_fn(|k| s[k] + ((d[k] * (256 - s[3])) >> 8))
}

/// Overlay's colour channel (hardlight with source and destination swapped).
fn overlay(s: i64, d: i64, sa: i64, da: i64) -> i64 {
    let two = |v: i64| 2 * v;
    let mix = if two(d) <= da {
        two(s * d)
    } else {
        sa * da - two((da - d) * (sa - s))
    };
    div255(s * inv(da) + d * inv(sa) + mix)
}

fn pipeline(op: CompositeOp, s: [i64; 4], d: [i64; 4]) -> [i64; 4] {
    use CompositeOp::*;
    let (sa, da) = (s[3], d[3]);
    let src_over_alpha = sa + div255(da * inv(sa));
    let colors = |f: &dyn Fn(i64, i64) -> i64| -> [i64; 4] {
        [f(s[0], d[0]), f(s[1], d[1]), f(s[2], d[2]), 0]
    };
    let (mut out, alpha) = match op {
        Lighter => (colors(&|s, d| (s + d).min(255)), (sa + da).min(255)),
        Multiply => {
            let m = |s, d, sa, da| div255(s * inv(da) + d * inv(sa) + s * d);
            (colors(&|s, d| m(s, d, sa, da)), m(sa, da, sa, da))
        }
        Screen => {
            let f = |s, d| s + d - div255_round(s * d);
            (colors(&f), f(sa, da))
        }
        Darken => (
            colors(&|s, d| s + d - div255((s * da).max(d * sa))),
            src_over_alpha,
        ),
        Lighten => (
            colors(&|s, d| s + d - div255((s * da).min(d * sa))),
            src_over_alpha,
        ),
        Difference => (
            colors(&|s, d| s + d - 2 * div255((s * da).min(d * sa))),
            src_over_alpha,
        ),
        Overlay => (colors(&|s, d| overlay(s, d, sa, da)), src_over_alpha),
        _ => unreachable!(),
    };
    out[3] = alpha;
    out
}

pub(super) fn supports(op: CompositeOp) -> bool {
    use CompositeOp::*;
    matches!(
        op,
        SourceOver | Lighter | Multiply | Screen | Overlay | Darken | Lighten | Difference
    )
}

/// Draws `src`'s pixels `(rx, ry, rw, rh)` onto `dst` with the region's origin at `(ox, oy)`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_image(
    dst: &mut Pixmap,
    src: &Pixmap,
    (rx, ry, rw, rh): (i64, i64, i64, i64),
    (ox, oy): (i64, i64),
    global_alpha: f32,
    op: CompositeOp,
    mask: Option<&Mask>,
) {
    let (dw, dh) = (dst.width() as i64, dst.height() as i64);
    let sw = src.width() as usize;
    let aa = (global_alpha * 255.0 + 0.5) as i64;
    // A zero paint alpha draws nothing for every supported op (and `SkBlendARGB32` must never
    // see a zero scale: it would darken the destination).
    if aa == 0 {
        return;
    }
    let (x0, y0) = (ox.max(0), oy.max(0));
    let (x1, y1) = ((ox + rw).min(dw), (oy + rh).min(dh));
    let dst_w = dst.width() as usize;
    let sdata = src.data();
    let ddata = dst.data_mut();
    for y in y0..y1 {
        for x in x0..x1 {
            let si = (((ry + y - oy) as usize) * sw + (rx + x - ox) as usize) * 4;
            let di = (y as usize * dst_w + x as usize) * 4;
            let raw: [i64; 4] = std::array::from_fn(|k| sdata[si + k] as i64);
            let d: [i64; 4] = std::array::from_fn(|k| ddata[di + k] as i64);
            let out = if op == CompositeOp::SourceOver {
                if aa == 255 {
                    pm_src_over(raw, d)
                } else {
                    blend_argb32(raw, d, aa)
                }
            } else {
                let s = if aa == 255 {
                    raw
                } else {
                    raw.map(|v| div255(v * aa))
                };
                pipeline(op, s, d)
            };
            // Partial clip coverage lerps like lowp's `lerp_u8` (unpinned: no AA-clipped layer case yet).
            let cov = mask.map_or(255, |m| m.data()[y as usize * dst_w + x as usize] as i64);
            for k in 0..4 {
                let v = if cov == 255 {
                    out[k]
                } else {
                    lerp(d[k], out[k], cov)
                };
                ddata[di + k] = v.clamp(0, 255) as u8;
            }
        }
    }
}
