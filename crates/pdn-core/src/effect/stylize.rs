//! The 4 Stylize effects on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` entries
//! edgeDetectEffect, embossEffect, outlineEffect and reliefEffect: ids, icons and control keys
//! only).
//! Outline's percentile sampler is Median Blur's.

use super::blur::{gaussian, madd, percentile_filter};
use super::surface::{Alpha, Edge, EffectSurface, Space, luma};
use super::{
    Category, Control, EffectDef, EffectEnv, EffectKind, GpuFn, RenderInfo, Values, choice, flag,
    num, range,
};
use std::f64::consts::PI;

const SRGB: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Straight);
const SRGB_PM: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);

fn stylize(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    f: GpuFn,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("stylize"),
        controls,
        kind: EffectKind::Gpu(info, f),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Stylize submenu (§1): paintdotjs ids, icons, keys, labels and steps; Paint.NET's ranges
/// and the new Edge Detect, Emboss and Relief controls (R19, §2.12–2.15).
pub fn effects() -> Vec<EffectDef> {
    vec![
        stylize(
            "edgeDetectEffect",
            "edge_detect_effect.png",
            vec![
                range("strength", "Strength", 0.0, 1.0, 0.5).step(0.01),
                range("blurring", "Blurring", 0.0, 10.0, 0.0)
                    .step(0.01)
                    .exponential(),
                Control::Select {
                    key: "algorithm",
                    label: "Algorithm",
                    default: "sobel",
                    choices: &[("sobel", "Sobel"), ("prewitt", "Prewitt")],
                },
                Control::Checkbox {
                    key: "overlay",
                    label: "Overlay edges",
                    default: false,
                },
            ],
            SRGB_PM,
            edge_detect,
        ),
        stylize(
            "embossEffect",
            "emboss_effect.png",
            vec![range("angle", "Angle", 0.0, 360.0, 0.0)],
            SRGB,
            emboss,
        ),
        stylize(
            "outlineEffect",
            "outline_effect_icon.png",
            vec![
                range("thickness", "Thickness", 1.0, 70.0, 3.0),
                range("intensity", "Intensity", 0.0, 100.0, 50.0),
                range("quality", "Quality", 1.0, 9.0, 8.0),
            ],
            SRGB,
            outline,
        ),
        stylize(
            "reliefEffect",
            "relief_effect.png",
            vec![range("angle", "Angle", -180.0, 180.0, 45.0)],
            SRGB,
            relief,
        ),
    ]
}

type Kernel = [[f32; 3]; 3];

/// Σ k[fy][fx]·src(x − 1 + fx, y − 1 + fy) over Mirror-extended taps (D2D BorderMode Hard,
/// §2.0 item 7).
fn correlate(src: &EffectSurface, k: &Kernel, x: u32, y: u32) -> [f32; 4] {
    let mut acc = [0.0; 4];
    for (fy, row) in k.iter().enumerate() {
        for (fx, &w) in row.iter().enumerate() {
            let (sx, sy) = (x as i32 - 1 + fx as i32, y as i32 - 1 + fy as i32);
            madd(&mut acc, w, src.fetch(sx, sy, Edge::Mirror));
        }
    }
    acc
}

/// The §2.13/§2.15 rows at r with δ = π/4: [cos(r+δ), cos(r+2δ), cos(r+3δ)], [cos r, centre,
/// cos(r+4δ)], [cos(r−δ), cos(r−2δ), cos(r−3δ)].
fn angle_kernel(r: f64, centre: f32) -> Kernel {
    let c = |k: f64| (r + k * PI / 4.0).cos() as f32;
    [
        [c(1.0), c(2.0), c(3.0)],
        [c(0.0), centre, c(4.0)],
        [c(-1.0), c(-2.0), c(-3.0)],
    ]
}

/// §6 item 4 proposal on premultiplied sRGB: g = Gaussian(σ = blurring/3, Mirror); per colour
/// channel |(gx, gy)| of the Sobel or Prewitt kernels × strength, added to the source with
/// Overlay; alpha unchanged.
fn edge_detect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D EdgeDetection (effects-2.md §6 item 4; B-E10)
    let strength = num(v, "strength").clamp(0.0, 1.0) as f32;
    let g = gaussian(src, num(v, "blurring").clamp(0.0, 10.0) / 3.0, Edge::Mirror);
    let s = if choice(v, "algorithm") == "prewitt" {
        1.0
    } else {
        2.0
    };
    let kx = [[-1.0, 0.0, 1.0], [-s, 0.0, s], [-1.0, 0.0, 1.0]];
    let ky = [[-1.0, -s, -1.0], [0.0; 3], [1.0, s, 1.0]];
    let overlay = flag(v, "overlay");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let (gx, gy) = (correlate(&g, &kx, x, y), correlate(&g, &ky, x, y));
        let p = src.at(x, y);
        let e = |c: usize| {
            (gx[c] * gx[c] + gy[c] * gy[c]).sqrt() * strength + if overlay { p[c] } else { 0.0 }
        };
        [e(0), e(1), e(2), p[3]]
    })
}

fn emboss(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    emboss_surface(src, num(v, "angle").clamp(0.0, 360.0))
}

/// §2.13 on straight sRGB: Y of rgb/α, correlated with K[2 − fy][2 − fx] (K at r = (angle −
/// 180)·π/180; D2D's 180° kernel rotation), + 128/255, unclamped; alpha kept. Any angle.
pub fn emboss_surface(src: &EffectSurface, angle_deg: f64) -> EffectSurface {
    let k = angle_kernel((angle_deg - 180.0) * PI / 180.0, 0.0);
    let flipped: Kernel = std::array::from_fn(|fy| std::array::from_fn(|fx| k[2 - fy][2 - fx]));
    let grey = EffectSurface::from_fn(src.w, src.h, |x, y| {
        let [mut r, mut g, mut b, a] = src.at(x, y);
        // quirk(pdn): effects-2.md §2.13 step 1 — Straight selects UnPremultiply on straight data.
        if a > 0.0 {
            (r, g, b) = (r / a, g / a, b / a);
        }
        let l = luma(r, g, b);
        [l, l, l, a]
    });
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let c = correlate(&grey, &flipped, x, y);
        let o = c[0] + 128.0 / 255.0;
        [o, o, o, src.at(x, y)[3]]
    })
}

/// §2.14: the (1 ∓ I)/2 percentiles over radius min(thickness, 50), Clamp; out = (1 − (hi − lo),
/// hi.α).
fn outline(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let r = num(v, "thickness").clamp(1.0, 70.0).min(50.0).ceil() as u32;
    let i = num(v, "intensity").clamp(0.0, 100.0) / 100.0;
    let bits = num(v, "quality").clamp(1.0, 9.0) as u32;
    let [lo, hi] =
        [(1.0 - i) / 2.0, (1.0 + i) / 2.0].map(|p| percentile_filter(src, r, p, bits, Edge::Clamp));
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let (l, h) = (lo.at(x, y), hi.at(x, y));
        [
            1.0 - (h[0] - l[0]),
            1.0 - (h[1] - l[1]),
            1.0 - (h[2] - l[2]),
            h[3],
        ]
    })
}

/// §2.15 on straight sRGB: all four channels correlated with the rows at r = angle·π/180, centre
/// 1 (the stored 180° pre-rotation cancels D2D's flip), unclamped.
fn relief(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let k = angle_kernel(num(v, "angle").clamp(-180.0, 180.0) * PI / 180.0, 1.0);
    EffectSurface::from_fn(src.w, src.h, |x, y| correlate(src, &k, x, y))
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run, solid};
    use serde_json::json;

    fn columns() -> crate::canvas::ImageData {
        from_fn(3, 3, |x, _| {
            let v = [115u8, 128, 140][x as usize];
            [v, v, v, 255]
        })
    }

    /// effects-2.md §5 vectors 8–9 (they settle the convolution orientation).
    #[test]
    fn emboss_and_relief_orientation() {
        assert_eq!(
            px(&run("embossEffect", json!({"angle": 0}), &columns()), 1, 1)[..3],
            [68, 68, 68]
        );
        assert_eq!(
            px(&run("reliefEffect", json!({"angle": 45}), &columns()), 1, 1)[..3],
            [85, 85, 85]
        );
    }

    /// Vector 10: a flat image outlines to white with the source alpha; thickness clamps at 50.
    #[test]
    fn outline_flat_and_clamp() {
        let flat = solid(12, 12, [40, 90, 200, 180]);
        assert_eq!(
            px(&run("outlineEffect", json!({}), &flat), 6, 6),
            [255, 255, 255, 180]
        );
        let img = from_fn(30, 30, |x, y| [(x * 8) as u8, (y * 8) as u8, 77, 255]);
        assert_eq!(
            run("outlineEffect", json!({"thickness": 70}), &img).data,
            run("outlineEffect", json!({"thickness": 50}), &img).data
        );
    }

    /// Edge Detect (approximation, B-E10): a flat image has no edges.
    #[test]
    fn edge_detect_flat_is_black() {
        let out = run(
            "edgeDetectEffect",
            json!({}),
            &solid(9, 9, [120, 60, 30, 255]),
        );
        assert_eq!(px(&out, 4, 4), [0, 0, 0, 255]);
    }

    // ---- Vectors from an independent f64 Python port of §2.12–2.15. ----

    fn rows() -> crate::canvas::ImageData {
        from_fn(3, 3, |_, y| {
            let v = [115u8, 128, 140][y as usize];
            [v, v, v, 255]
        })
    }

    /// §2.13: angle 90 lights the rows (188; unflipped 68) and leaves the columns flat (128);
    /// angle 30 at the Mirror-extended corner and edge; the straight-alpha quirk divides by α
    /// again (α 128: 8 with the quirk, 68 without), alpha kept.
    #[test]
    fn emboss_angle_edges_and_alpha_quirk() {
        let at = |angle: f64, img: &crate::canvas::ImageData, x, y| {
            px(&run("embossEffect", json!({"angle": angle}), img), x, y)
        };
        assert_eq!(at(90.0, &rows(), 1, 1), [188, 188, 188, 255]);
        assert_eq!(at(90.0, &columns(), 1, 1), [128, 128, 128, 255]);
        assert_eq!(at(30.0, &columns(), 0, 0), [101, 101, 101, 255]);
        assert_eq!(at(30.0, &columns(), 2, 1), [103, 103, 103, 255]);
        let translucent = from_fn(3, 3, |x, _| {
            let v = [115u8, 128, 140][x as usize];
            [v, v, v, 128]
        });
        assert_eq!(at(0.0, &translucent, 1, 1), [8, 8, 8, 128]);
    }

    /// §2.15: angle −90 on rows and 0 on columns (68); all four channels are convolved (alpha
    /// columns 255/128/64: angle 0 → α 255, angle 180 → α 0 with the hidden colour kept).
    #[test]
    fn relief_angles_and_alpha() {
        let at = |angle: f64, img: &crate::canvas::ImageData, x, y| {
            px(&run("reliefEffect", json!({"angle": angle}), img), x, y)
        };
        assert_eq!(at(-90.0, &rows(), 1, 1), [68, 68, 68, 255]);
        assert_eq!(at(0.0, &columns(), 1, 1), [68, 68, 68, 255]);
        let alpha = from_fn(3, 3, |x, _| [200, 100, 50, [255u8, 128, 64][x as usize]]);
        assert_eq!(at(0.0, &alpha, 1, 1), [200, 100, 50, 255]);
        assert_eq!(at(180.0, &alpha, 1, 1), [200, 100, 50, 0]);
        assert_eq!(at(0.0, &alpha, 0, 0), [200, 100, 50, 255]);
    }

    /// §2.14 at intensity 100: lo/hi are the exact min/max over the F6 disc with Clamp edges,
    /// out = 1 − (hi − lo).
    #[test]
    fn outline_full_intensity_is_disc_range() {
        let img = from_fn(30, 30, |x, y| [(x * 8) as u8, (y * 8) as u8, 77, 255]);
        let out = run("outlineEffect", json!({"intensity": 100}), &img);
        assert_eq!(px(&out, 10, 12), [207, 207, 255, 255]);
        assert_eq!(px(&out, 0, 0), [231, 231, 255, 255]);
        let thin = run(
            "outlineEffect",
            json!({"intensity": 100, "thickness": 1}),
            &img,
        );
        assert_eq!(px(&thin, 29, 5), [247, 239, 255, 255]);
        // α is hi's (the disc max 143 around a 116 pixel), not lo's.
        let alpha = from_fn(30, 30, |x, y| {
            [100, 150, 200, ((x * 8 + y * 3) % 256) as u8]
        });
        let out = run("outlineEffect", json!({"intensity": 100}), &alpha);
        assert_eq!(px(&out, 10, 12), [255, 255, 255, 143]);
    }

    /// §2.14 with the bisection (Python port of effects-1.md §2.3's sampler): the gradient at the
    /// defaults (p = 0.25/0.75), intensity 20 at thickness 5, and 4 bits; a red disc with 7 zeros
    /// of 37 stays white at τ = 9.25 (p 0.125 would give 1), 12 zeros give 1.
    #[test]
    fn outline_percentiles() {
        let img = from_fn(30, 30, |x, y| [(x * 8) as u8, (y * 8) as u8, 77, 255]);
        let at = |v| px(&run("outlineEffect", v, &img), 10, 12);
        assert_eq!(at(json!({})), [239, 239, 255, 255]);
        assert_eq!(
            at(json!({"intensity": 20, "thickness": 5})),
            [239, 239, 255, 255]
        );
        assert_eq!(at(json!({"quality": 4})), [240, 240, 255, 255]);
        // Clamp edge at the defaults, pixel (0, 12): the left taps replicate column 0, so x's hi
        // percentile is ~8/255 (range .0314 → 247); Mirror reflects to ~16/255 (239).
        // Independent f64 port of §2.14: x lo .00018 hi .03143; y 239; b flat.
        assert_eq!(
            px(&run("outlineEffect", json!({}), &img), 0, 12),
            [247, 239, 255, 255]
        );
        let disc = crate::effect::sample::circle_offsets(3);
        for (zeros, red) in [(7, 255), (12, 1)] {
            let field = from_fn(9, 9, |x, y| {
                let z = disc[..zeros].contains(&(x as i32 - 4, y as i32 - 4));
                [if z { 0 } else { 255 }, 255, 120, 255]
            });
            let out = run("outlineEffect", json!({}), &field);
            assert_eq!(px(&out, 4, 4), [red, 255, 255, 255], "{zeros} zeros");
        }
    }

    /// §6 item 4 proposal: per channel |(gx, gy)| × strength on premultiplied colour (Mirror),
    /// Sobel vs Prewitt, Overlay adds the source.
    #[test]
    fn edge_detect_kernels() {
        let diag = from_fn(7, 6, |x, y| {
            [
                if x + y > 3 {
                    (40 * x + 10 * y).min(255) as u8
                } else {
                    0
                },
                (30 * y) as u8,
                if x == y { 255 } else { 0 },
                255,
            ]
        });
        let at = |v, x, y| px(&run("edgeDetectEffect", v, &diag), x, y);
        assert_eq!(at(json!({}), 3, 2), [222, 120, 255, 255]);
        assert_eq!(
            at(json!({"algorithm": "prewitt"}), 3, 2),
            [181, 90, 180, 255]
        );
        assert_eq!(at(json!({}), 0, 0), [0, 60, 255, 255]);
        assert_eq!(at(json!({"strength": 0.3}), 3, 2), [133, 72, 216, 255]);
        assert_eq!(
            at(json!({"strength": 0.3, "algorithm": "prewitt"}), 3, 2),
            [109, 54, 108, 255]
        );
        let step = from_fn(6, 5, |x, _| {
            if x < 3 {
                [0, 0, 50, 255]
            } else {
                [200, 100, 50, 255]
            }
        });
        let at = |v, x, y| px(&run("edgeDetectEffect", v, &step), x, y);
        assert_eq!(at(json!({}), 2, 2), [255, 200, 0, 255]);
        assert_eq!(
            at(json!({"algorithm": "prewitt"}), 2, 2),
            [255, 150, 0, 255]
        );
        assert_eq!(at(json!({"overlay": true}), 3, 2), [255, 255, 50, 255]);
        assert_ne!(at(json!({"blurring": 3}), 2, 2), at(json!({}), 2, 2));
    }
}
