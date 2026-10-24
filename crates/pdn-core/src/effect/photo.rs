//! The 6 Photo effects on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` entries glowEffect,
//! redEyeRemoveEffect, sharpenEffect, softenPortraitEffect, straightenEffect and vignetteEffect:
//! ids, icons and control keys only).

use super::artistic::brightness_contrast;
use super::blur::gaussian;
use super::distort::{anchor, catmull_rom, filter4, offset};
use super::mix::{W3cBlend, blend_w3c};
use super::surface::{Alpha, Edge, EffectSurface, Space, luma, premultiply, unpremultiply};
use super::{
    Category, Control, EffectDef, EffectEnv, EffectKind, RenderInfo, Values, choice, num, range,
    underwrite,
};
use crate::canvas::ImageData;
use crate::pixel::IntRect;
use crate::pixel::mask::Mask;
use std::f64::consts::PI;

const SRGB: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Straight);
const SRGB_PM: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
const LINEAR: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Straight);
const LINEAR_PM: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);

fn photo(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    kind: EffectKind,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("photo"),
        controls,
        kind,
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Photo submenu (§1): paintdotjs ids, icons, keys, labels and steps; Paint.NET's ranges and
/// the new Strength, Sampling and Center controls (R19, §2.2–2.7). Paint.NET's exponential sliders
/// (Sharpen threshold, Vignette radius and strength) are marked.
pub fn effects() -> Vec<EffectDef> {
    vec![
        photo(
            "glowEffect",
            "glow_effect.png",
            vec![
                range("radius", "Radius", 1.0, 20.0, 6.0),
                range("brightness", "Brightness", -100.0, 100.0, 10.0),
                range("contrast", "Contrast", -100.0, 100.0, 10.0),
            ],
            EffectKind::Gpu(SRGB, glow),
        ),
        photo(
            "redEyeRemoveEffect",
            "red_eye_remove_effect.png",
            vec![range("strength", "Strength", 1.0, 5.0, 3.0)],
            EffectKind::Cpu {
                render: red_eye,
                clip_to_selection: false,
            },
        ),
        photo(
            "sharpenEffect",
            "sharpen_effect.png",
            vec![
                range("amount", "Amount", 0.0, 10.0, 2.0).step(0.1),
                range("threshold", "Threshold", 0.0, 1.0, 0.0)
                    .step(0.01)
                    .exponential(),
            ],
            EffectKind::Gpu(SRGB_PM, sharpen),
        ),
        photo(
            "softenPortraitEffect",
            "soften_portrait_effect_icon.png",
            vec![
                range("softness", "Softness", 0.0, 10.0, 5.0),
                range("lighting", "Lighting", -20.0, 20.0, 0.0),
                range("warmth", "Warmth", 0.0, 20.0, 10.0),
            ],
            EffectKind::Gpu(SRGB, soften_portrait),
        ),
        photo(
            "straightenEffect",
            "straighten_effect.png",
            vec![
                range("angle", "Angle", -45.0, 45.0, 0.0).step(0.1),
                Control::Select {
                    key: "sampling",
                    label: "Sampling",
                    default: "bicubic",
                    choices: &[
                        ("bicubic", "Bicubic"),
                        ("bilinear", "Bilinear"),
                        ("nearestNeighbor", "Nearest Neighbor"),
                    ],
                },
            ],
            EffectKind::Gpu(LINEAR_PM, straighten),
        ),
        photo(
            "vignetteEffect",
            "vignette_effect_icon.png",
            vec![
                range("centerX", "Center X", -1.0, 1.0, 0.0).step(0.01),
                range("centerY", "Center Y", -1.0, 1.0, 0.0).step(0.01),
                range("radius", "Radius", 0.1, 4.0, 0.5)
                    .step(0.01)
                    .exponential(),
                range("strength", "Strength", 0.0, 1.0, 1.0)
                    .step(0.01)
                    .exponential(),
            ],
            EffectKind::Gpu(LINEAR, vignette),
        ),
    ]
}

fn premultiplied(src: &EffectSurface) -> EffectSurface {
    EffectSurface {
        px: src.px.iter().copied().map(premultiply).collect(),
        ..*src
    }
}

/// §2.2 on sRGB straight colour: P = premultiply(src); out = unpremultiply(Screen(P,
/// F11(Gaussian(P, 1.25·radius/3, Hard)))).
fn glow(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D GaussianBlur "Quality" (effects-2.md §6 item 7; B-E14)
    let radius = num(v, "radius").clamp(1.0, 20.0);
    let [b, c] = ["brightness", "contrast"].map(|k| num(v, k).clamp(-100.0, 100.0) as f32);
    let pre = premultiplied(src);
    let blurred = gaussian(&pre, 1.25 * radius / 3.0, Edge::Mirror);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let glow = brightness_contrast(blurred.at(x, y), b, c);
        unpremultiply(blend_w3c(W3cBlend::Screen, pre.at(x, y), glow))
    })
}

/// §2.3 on the 8-bit layer: copy, `strength` passes over the island bounds, then the effect's own
/// mask blend (selection clipping is off).
fn red_eye(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    // approximation: GDI+ RedEyeCorrection (effects-2.md §6 item 5; B-E11)
    let mut out = src.clone();
    let passes = num(v, "strength").clamp(1.0, 5.0) as u32;
    let w = src.width as usize;
    let mut done = vec![false; w * src.height as usize];
    // ruling R50: no mask is no selection or Select All, both a whole-canvas selection
    // (adjustments-and-effect-framework.md §2.5 step 1), so one island equal to the layer.
    let islands = env.mask.map_or_else(
        || vec![IntRect::new(0, 0, src.width, src.height)],
        red_eye_islands,
    );
    for island in islands {
        let r = island.intersect(&IntRect::new(0, 0, src.width, src.height));
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let i = y as usize * w + x as usize;
                // Each pass corrects a pixel once, however many island bounds hold it.
                if std::mem::replace(&mut done[i], true) {
                    continue;
                }
                let p = &mut out.data[4 * i..4 * i + 4];
                let gb = (p[1] as u32 + p[2] as u32) / 2;
                for _ in 0..passes {
                    if p[0] as u32 > gb + 8 {
                        p[0] = ((p[0] as u32 + gb) / 2) as u8;
                    }
                }
            }
        }
    }
    if let Some(mask) = env.mask {
        underwrite(src, &mut out, mask);
    }
    out
}

/// §2.3 step 3: the 8-connected components of mask > 0 (a scan joins an island when it touches a
/// member rect inflated by 1 px), each as the bounds of its pixels in layer coordinates, ordered by
/// their first pixel in scan order (merged islands keep the first island's place).
pub fn red_eye_islands(mask: &Mask) -> Vec<IntRect> {
    // ponytail: push-on-discover DFS peaks near N/4 stack entries plus 4 B/px for `seen` and
    // `done` (~400 MB transient at 100 MP); under the per-pixel approximation the islands can't
    // change the output. Row-run labelling is the upgrade if B-E11 keeps the islands.
    let (w, h) = (mask.rect.w as usize, mask.rect.h as usize);
    let mut seen = vec![false; w * h];
    let mut islands = vec![];
    for start in 0..w * h {
        if mask.data[start] == 0 || seen[start] {
            continue;
        }
        seen[start] = true;
        let (mut lo, mut hi) = ([usize::MAX; 2], [0; 2]);
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            lo = [lo[0].min(x), lo[1].min(y)];
            hi = [hi[0].max(x), hi[1].max(y)];
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let j = ny * w + nx;
                    if mask.data[j] > 0 && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        islands.push(IntRect::new(
            mask.rect.x + lo[0] as i32,
            mask.rect.y + lo[1] as i32,
            (hi[0] - lo[0] + 1) as u32,
            (hi[1] - lo[1] + 1) as u32,
        ));
    }
    islands
}

/// §2.4 on sRGB premultiplied colour, per channel: out = in + k·(in − box₃ₓ₃(in)) where
/// |in − box| > threshold, k = amount/2, Mirror (Hard) edges.
fn sharpen(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D Sharpen (effects-2.md §6 item 4; B-E10)
    let k = (num(v, "amount").clamp(0.0, 10.0) / 2.0) as f32;
    if k == 0.0 {
        return src.clone();
    }
    let threshold = num(v, "threshold").clamp(0.0, 1.0) as f32;
    let mean = src.convolve_hv(&[1.0 / 3.0; 3], Edge::Mirror);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let (c, m) = (src.at(x, y), mean.at(x, y));
        std::array::from_fn(|i| {
            let d = c[i] - m[i];
            if d.abs() > threshold {
                c[i] + k * d
            } else {
                c[i]
            }
        })
    })
}

/// §2.5 on sRGB straight colour, l = lighting, w = warmth/100: Overlay(A, B) with backdrop
/// A = premultiply(Y·(1 + w, 1, 1 − w)) unclamped and source B = F11(Gaussian(P, 1.25·softness,
/// Hard), B = l, C = −l/2).
fn soften_portrait(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D GaussianBlur "Quality" (effects-2.md §6 item 7; B-E14)
    let softness = num(v, "softness").clamp(0.0, 10.0);
    let l = num(v, "lighting").clamp(-20.0, 20.0) as f32;
    let w = (num(v, "warmth").clamp(0.0, 20.0) / 100.0) as f32;
    let blurred = gaussian(&premultiplied(src), 1.25 * softness, Edge::Mirror);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let [r, g, b, a] = src.at(x, y);
        let grey = luma(r, g, b);
        let tinted = premultiply([grey * (1.0 + w), grey, grey * (1.0 - w), a]);
        let lit = brightness_contrast(blurred.at(x, y), l, -l / 2.0);
        unpremultiply(blend_w3c(W3cBlend::Overlay, tinted, lit))
    })
}

/// §2.6: the cover scale cos|θ| + max(W/H, H/W)·sin|θ| for θ = clamp(−angle, −45°, 45°).
pub fn straighten_scale(w: f64, h: f64, angle_deg: f64) -> f64 {
    let t = angle_deg.clamp(-45.0, 45.0).abs() * PI / 180.0;
    t.cos() + (w / h).max(h / w) * t.sin()
}

/// §2.6 in linear premultiplied light: sample c + R(θ)(p − c)/s with Catmull-Rom (a = −½),
/// bilinear or nearest taps and Clamp edges; R(θ)(x, y) = (x cos θ − y sin θ, x sin θ + y cos θ).
fn straighten(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D Straighten MaintainSize (effects-2.md §6 item 3; B-E9)
    let angle = num(v, "angle");
    let theta = (-angle).clamp(-45.0, 45.0) * PI / 180.0;
    if theta == 0.0 || src.px.is_empty() {
        return src.clone();
    }
    let s = straighten_scale(src.w as f64, src.h as f64, angle);
    let (cos, sin) = ((theta.cos() / s) as f32, (theta.sin() / s) as f32);
    let c = [src.w as f32 / 2.0, src.h as f32 / 2.0];
    let sampling = choice(v, "sampling");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let d = [x as f32 + 0.5 - c[0], y as f32 + 0.5 - c[1]];
        let q = [
            c[0] + (d[0] * cos - d[1] * sin),
            c[1] + (d[0] * sin + d[1] * cos),
        ];
        match sampling {
            "bilinear" => src.bilinear(q[0], q[1], Edge::Clamp),
            "nearestNeighbor" => src.point(q[0], q[1], Edge::Clamp),
            _ => filter4(q, catmull_rom, |i, j| src.fetch(i, j, Edge::Clamp)),
        }
    })
}

/// §2.7: 1 − σ when cos d ≤ 0 or d ≥ π, else (1 − σ) + σ·((cos d)²)².
pub fn vignette_factor(d2k: f64, strength: f64) -> f64 {
    let c = d2k.cos();
    if c <= 0.0 || d2k >= PI {
        1.0 - strength
    } else {
        let c2 = c * c;
        (1.0 - strength) + strength * (c2 * c2)
    }
}

/// §2.7 on linear straight colour over the selection bounds R: c = anchor(R, centre), r =
/// max(R.w, R.h)·ρ/2, k = π/(8r²), out = (rgb·f(|p − c|²·k), a). The centre clamps to ±2 (§2.7).
fn vignette(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let b = env.bounds;
    let c = anchor(b, offset(v, ["centerX", "centerY"], 2.0)).map(f64::from);
    let r = b.w.max(b.h) as f64 * num(v, "radius").clamp(0.1, 4.0) / 2.0;
    let k = PI / (8.0 * r * r);
    let strength = num(v, "strength").clamp(0.0, 1.0);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let [r, g, b, a] = src.at(x, y);
        let (dx, dy) = (x as f64 + 0.5 - c[0], y as f64 + 0.5 - c[1]);
        let f = vignette_factor((dx * dx + dy * dy) * k, strength) as f32;
        [r * f, g * f, b * f, a]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, run, run_env, solid};
    use serde_json::json;

    fn grey_out(id: &str, values: serde_json::Value, k: u8) -> [u8; 4] {
        px(&run(id, values, &solid(9, 9, [k, k, k, 255])), 4, 4)
    }

    /// effects-2.md §5 vectors 1–3.
    #[test]
    fn glow_soften_vignette_vectors() {
        assert_eq!(
            [128u8, 64, 200].map(|k| grey_out("glowEffect", json!({}), k)[0]),
            [197, 115, 247]
        );
        assert_eq!(
            &grey_out("softenPortraitEffect", json!({}), 100)[..3],
            [86, 78, 71]
        );
        assert_eq!(
            &grey_out("softenPortraitEffect", json!({}), 180)[..3],
            [221, 211, 200]
        );
        let v = run(
            "vignetteEffect",
            json!({}),
            &solid(101, 101, [200, 200, 200, 255]),
        );
        let at = |x, y| px(&v, x, y)[0];
        assert_eq!(
            [at(50, 50), at(75, 50), at(60, 60), at(90, 50), at(0, 0)],
            [200, 175, 197, 66, 0]
        );
    }

    /// Delegated effects (Direct2D Sharpen/Straighten, GDI+ Red Eye): neutral settings are identity.
    #[test]
    fn delegated_effects_are_neutral() {
        let img = from_fn(12, 10, |x, y| [(x * 20) as u8, (y * 25) as u8, 90, 255]);
        assert_eq!(
            run("sharpenEffect", json!({"amount": 0}), &img).data,
            img.data
        );
        for s in ["bicubic", "bilinear", "nearestNeighbor"] {
            assert_eq!(
                run("straightenEffect", json!({"angle": 0, "sampling": s}), &img).data,
                img.data,
                "{s}"
            );
        }
        // ruling R50: no selection (or Select All) is one whole-layer island: 220 → 130 → 85 → 62.
        let red = solid(1, 1, [220, 40, 40, 255]);
        assert_eq!(
            px(&run("redEyeRemoveEffect", json!({}), &red), 0, 0),
            [62, 40, 40, 255]
        );
        assert!(
            (straighten_scale(100.0, 50.0, 10.0)
                - (10f64.to_radians().cos() + 2.0 * 10f64.to_radians().sin()))
            .abs()
                < 1e-12
        );
    }

    /// Red Eye: only the selected island loses red; strength = passes (B-E11).
    #[test]
    fn red_eye_inside_islands() {
        use crate::pixel::{IntRect, mask::Mask};
        let img = from_fn(6, 1, |_, _| [220, 40, 40, 255]);
        let mask = Mask {
            rect: IntRect::new(0, 0, 6, 1),
            data: vec![255, 255, 0, 0, 0, 0],
        };
        assert_eq!(red_eye_islands(&mask), [IntRect::new(0, 0, 2, 1)]);
        let mut env = crate::effect::EffectEnv::whole(6, 1);
        env.mask = Some(&mask);
        let out = run_env("redEyeRemoveEffect", json!({}), &img, &env);
        assert!(px(&out, 0, 0)[0] < 220);
        assert_eq!(px(&out, 5, 0), [220, 40, 40, 255]);
        let one = run_env("redEyeRemoveEffect", json!({"strength": 1}), &img, &env);
        assert!(px(&one, 0, 0)[0] > px(&out, 0, 0)[0]);
    }

    #[test]
    fn photo_menu() {
        let ids: Vec<&str> = crate::effect::effects()
            .iter()
            .filter(|e| e.category == crate::effect::Category::Effect("photo"))
            .map(|e| e.id)
            .collect();
        assert_eq!(ids.len(), 6);
        let obj: Vec<&str> = crate::effect::effects()
            .iter()
            .filter(|e| e.category == crate::effect::Category::Effect("object"))
            .map(|e| e.id)
            .collect();
        assert_eq!(obj, ["dropShadowEffect"]);
    }

    // ---- Vectors from an independent f64 Python port of §2.2–2.7 (the brief's approximations for
    // the delegated effects). Each names the plausible bug it rules out. ----

    /// Glow blur σ = 1.25·radius/3 with Mirror (Hard) edges, and translucent input:
    /// - grey 64 with a white column at x = 7: x = 3 → 122.17, x = 6 → 138.14 (σ = radius/3:
    ///   119.53, 142.53; σ = 1.25·radius: 125.56, 125.71);
    /// - grey 32 with a white column at x = 1: x = 0 → 116.98 (Clamp 91.99, Transparent 53.55);
    /// - flat (128, 128, 128, 128) → (154.94, …, 191.75); B −50, C −50 on flat 128 → 166.72;
    ///   B 30, C −20 → 207 (brightness and contrast swapped: 178);
    ///   B −100, C 100 is the d = 0 step → black → Screen(v, 0) = v.
    #[test]
    fn glow_blur_edges_and_alpha() {
        let band = from_fn(
            15,
            3,
            |x, _| if x == 7 { [255; 4] } else { [64, 64, 64, 255] },
        );
        let out = run("glowEffect", json!({}), &band);
        assert_eq!(
            [3, 4, 5, 6].map(|x| px(&out, x, 1)[0]),
            [122, 127, 133, 138]
        );
        let edge = from_fn(
            9,
            3,
            |x, _| if x == 1 { [255; 4] } else { [32, 32, 32, 255] },
        );
        assert_eq!(px(&run("glowEffect", json!({}), &edge), 0, 1)[0], 117);
        assert_eq!(
            px(&run("glowEffect", json!({}), &solid(9, 9, [128; 4])), 4, 4),
            [155, 155, 155, 192]
        );
        assert_eq!(
            grey_out(
                "glowEffect",
                json!({"brightness": -50, "contrast": -50}),
                128
            ),
            [167, 167, 167, 255]
        );
        assert_eq!(
            grey_out(
                "glowEffect",
                json!({"brightness": 30, "contrast": -20}),
                128
            ),
            [207, 207, 207, 255]
        );
        assert_eq!(
            grey_out(
                "glowEffect",
                json!({"brightness": -100, "contrast": 100}),
                128
            ),
            [128, 128, 128, 255]
        );
    }

    /// Soften Portrait: Overlay(A = grey·(1 + w, 1, 1 − w), B = F11(blur, B = l, C = −l/2)):
    /// - lighting ±20 on flat 128 → (159.12, 148.37, 133.68) and (121.39, 106.42, 95.62);
    /// - warmth 20 on flat 128 → (154.00, 128.50 − ε, 102.80);
    /// - pure red: A = 0.299·(1.1, 1, 0.9) ≤ ½ → 2·A·B = 167.74 for red (Overlay with backdrop
    ///   and source swapped gives 255);
    /// - flat 230, warmth 20: A's red 1.08 stays unclamped → 259.1 → 255, green 250.10;
    /// - (100, 100, 100, 128) → (98.73, 92.77, 86.82, 191.75);
    /// - blur σ = 1.25·softness: grey 100 with a white column at x = 7, softness 2, x = 6 →
    ///   (105.76, 96.14, 86.53) (σ = 1.25·softness/3: 118.52; σ = softness: 109.46).
    #[test]
    fn soften_portrait_lighting_warmth_and_blur() {
        assert_eq!(
            grey_out("softenPortraitEffect", json!({"lighting": 20}), 128),
            [159, 148, 134, 255]
        );
        assert_eq!(
            grey_out("softenPortraitEffect", json!({"lighting": -20}), 128),
            [121, 106, 96, 255]
        );
        let red = run(
            "softenPortraitEffect",
            json!({}),
            &solid(9, 9, [255, 0, 0, 255]),
        );
        assert_eq!(px(&red, 4, 4), [168, 0, 0, 255]);
        assert_eq!(
            grey_out("softenPortraitEffect", json!({"warmth": 20}), 230),
            [255, 250, 241, 255]
        );
        let translucent = run(
            "softenPortraitEffect",
            json!({}),
            &solid(9, 9, [100, 100, 100, 128]),
        );
        assert_eq!(px(&translucent, 4, 4), [99, 93, 87, 192]);
        // Branch A stays unclamped: (230, 230, 230, 128), warmth 20 → red 255 (A's red clamped to
        // 1 gives 247).
        let warm = run(
            "softenPortraitEffect",
            json!({"warmth": 20}),
            &solid(9, 9, [230, 230, 230, 128]),
        );
        assert_eq!(px(&warm, 4, 4), [255, 237, 218, 192]);
        let band = from_fn(15, 3, |x, _| {
            if x == 7 {
                [255; 4]
            } else {
                [100, 100, 100, 255]
            }
        });
        let out = run("softenPortraitEffect", json!({"softness": 2}), &band);
        assert_eq!(px(&out, 6, 1), [106, 96, 87, 255]);
    }

    /// Sharpen (B-E10 approximation) on sRGB premultiplied colour, k = amount/2, 3×3 box, Mirror:
    /// - grey 100 with a 160 column at x = 2: box 120 → 200 at x = 2, 80 beside it, x = 0 untouched;
    /// - amount 3 (k 1.5) → 220 and 70; threshold 0.1 keeps x = 1 (|Δ| = 20/255) and sharpens x = 2;
    /// - the column at x = 0: Mirror's box (160, 160, 100) → 180 (Wrap 200, Transparent 233);
    /// - a (200, 200, 200, 128) column: x = 1 sharpens premultiplied colour and alpha → 85.65 at α
    ///   1.17 (straight colour would give 67).
    #[test]
    fn sharpen_vectors() {
        let col = |c: [u8; 4], k: u32| {
            from_fn(
                5,
                3,
                move |x, _| if x == k { c } else { [100, 100, 100, 255] },
            )
        };
        let row = |v: serde_json::Value, img| {
            let out = run("sharpenEffect", v, &img);
            [0, 1, 2, 3].map(|x| px(&out, x, 1)[0])
        };
        assert_eq!(
            row(json!({}), col([160, 160, 160, 255], 2)),
            [100, 80, 200, 80]
        );
        assert_eq!(
            row(json!({"amount": 3}), col([160, 160, 160, 255], 2)),
            [100, 70, 220, 70]
        );
        assert_eq!(
            row(json!({"threshold": 0.1}), col([160, 160, 160, 255], 2)),
            [100, 100, 200, 100]
        );
        assert_eq!(row(json!({}), col([160, 160, 160, 255], 0))[..2], [180, 80]);
        let out = run("sharpenEffect", json!({}), &col([200, 200, 200, 128], 2));
        assert_eq!(
            [px(&out, 1, 1), px(&out, 2, 1)],
            [[86, 86, 86, 255], [255, 255, 255, 43]]
        );
    }

    /// Straighten (B-E9 approximation) on an 8×6 RGB pattern, angle 10: θ = −10°, s = 1.21634,
    /// sample at c + R(θ)(p − c)/s in linear premultiplied light with Clamp edges. Nearest, bilinear
    /// and Catmull-Rom at (1, 4) and (6, 1); angle −10 (the opposite turn) differs in every mode. A
    /// red/transparent split turned 30° stays pure red at partial alpha (premultiplied taps).
    #[test]
    fn straighten_vectors() {
        let img = from_fn(8, 6, |x, y| {
            [
                (x * 32) as u8,
                (y * 50) as u8,
                ((x + y) % 2 * 255) as u8,
                255,
            ]
        });
        let at = |angle: f64, s: &str| {
            let out = run(
                "straightenEffect",
                json!({"angle": angle, "sampling": s}),
                &img,
            );
            [px(&out, 1, 4), px(&out, 6, 1)]
        };
        assert_eq!(
            at(10.0, "nearestNeighbor"),
            [[64, 200, 0, 255], [160, 50, 0, 255]]
        );
        assert_eq!(
            at(-10.0, "nearestNeighbor"),
            [[32, 150, 0, 255], [192, 100, 0, 255]]
        );
        assert_eq!(
            at(10.0, "bilinear"),
            [[56, 204, 157, 255], [171, 48, 157, 255]]
        );
        assert_eq!(
            at(-10.0, "bilinear"),
            [[43, 170, 176, 255], [184, 86, 176, 255]]
        );
        assert_eq!(
            at(10.0, "bicubic"),
            [[54, 204, 133, 255], [170, 46, 133, 255]]
        );
        // The corner's Catmull-Rom taps reach texel −1: Clamp gives (9, 49, 218), Transparent
        // (8, 49, 220).
        let corner = run("straightenEffect", json!({"angle": 10}), &img);
        assert_eq!(px(&corner, 0, 0), [9, 49, 218, 255]);
        let split = from_fn(6, 6, |x, _| {
            if x < 3 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 0]
            }
        });
        let out = run(
            "straightenEffect",
            json!({"angle": 30, "sampling": "bilinear"}),
            &split,
        );
        assert_eq!(
            [px(&out, 2, 2), px(&out, 3, 3), px(&out, 2, 4)],
            [[255, 0, 0, 255], [0; 4], [255, 0, 0, 68]]
        );
        assert!((straighten_scale(8.0, 6.0, 10.0) - 1.2163386565681151).abs() < 1e-12);
        assert_eq!(
            straighten_scale(8.0, 6.0, -90.0),
            straighten_scale(6.0, 8.0, 45.0)
        );
    }

    /// Vignette: f = 1 − σ when cos d ≤ 0 or d ≥ π, else (1 − σ) + σ·cos⁴d; geometry from the
    /// selection bounds; alpha kept (straight):
    /// - bounds (5, 0, 10, 11) of a 21×11 (200, 150, 100, 128) layer: c = (10, 5.5), r = 2.75 →
    ///   x = 12 → (181.70, 135.98, 90.25), x = 13 → (134.89, 100.10, 65.31), x = 15 and x = 0 → 0;
    /// - whole layer, centre (0.5, −0.5), radius 1, strength 0.5 → (0, 10) = (150.55, 112.10, 73.66),
    ///   (10, 5) = (198.62, 148.94, 99.26).
    #[test]
    fn vignette_geometry_and_strength() {
        assert_eq!(vignette_factor(0.0, 0.7), 1.0);
        assert!((vignette_factor(0.5, 0.6) - (0.4 + 0.6 * 0.5f64.cos().powi(4))).abs() < 1e-15);
        assert_eq!(
            [2.0, 3.0 * std::f64::consts::PI, std::f64::consts::TAU]
                .map(|d| vignette_factor(d, 0.25)),
            [0.75; 3]
        );
        let img = solid(21, 11, [200, 150, 100, 128]);
        let mut env = crate::effect::EffectEnv::whole(21, 11);
        env.bounds = crate::pixel::IntRect::new(5, 0, 10, 11);
        let out = run_env("vignetteEffect", json!({}), &img, &env);
        assert_eq!(
            [10, 12, 13, 15, 0].map(|x| px(&out, x, 5)),
            [
                [200, 150, 100, 128],
                [182, 136, 90, 128],
                [135, 100, 65, 128],
                [0, 0, 0, 128],
                [0, 0, 0, 128]
            ]
        );
        let out = run(
            "vignetteEffect",
            json!({"centerX": 0.5, "centerY": -0.5, "radius": 1, "strength": 0.5}),
            &img,
        );
        assert_eq!(
            [px(&out, 0, 10), px(&out, 10, 5)],
            [[151, 112, 74, 128], [199, 149, 99, 128]]
        );
        // The effect clamps the centre to ±2 (§2.7), past the ±1 control: radius 4, centre x 9 →
        // 2 gives 192 at (0, 5) (±1 gives 198, no clamp 0).
        let out = run(
            "vignetteEffect",
            json!({"centerX": 9, "radius": 4}),
            &solid(21, 11, [200, 150, 100, 255]),
        );
        assert_eq!(px(&out, 0, 5)[0], 192);
    }

    /// Islands are the 8-connected components of mask > 0 in scan order (a scan touching two
    /// islands merges them into the first), in layer coordinates; each pass sets
    /// r ← (r + (g + b)/2)/2 where r > (g + b)/2 + 8; the effect blends its own mask (§2.3 step 5).
    #[test]
    fn red_eye_islands_passes_and_mask() {
        use crate::pixel::{IntRect, mask::Mask};
        let m = |w: u32, h: u32, rows: &[&str]| Mask {
            rect: IntRect::new(0, 0, w, h),
            data: rows
                .concat()
                .bytes()
                .map(|b| if b == b'#' { 255 } else { 0 })
                .collect(),
        };
        assert_eq!(
            red_eye_islands(&m(5, 4, &["##...", "..#..", "....#", "#...."])),
            [
                IntRect::new(0, 0, 3, 2),
                IntRect::new(4, 2, 1, 1),
                IntRect::new(0, 3, 1, 1)
            ]
        );
        assert_eq!(
            red_eye_islands(&m(4, 2, &["#..#", ".##."])),
            [IntRect::new(0, 0, 4, 2)]
        );
        assert_eq!(
            red_eye_islands(&m(3, 2, &["..#", "#.."])),
            [IntRect::new(2, 0, 1, 1), IntRect::new(0, 1, 1, 1)]
        );
        let offset = Mask {
            rect: IntRect::new(2, 1, 3, 1),
            data: vec![9, 0, 1],
        };
        assert_eq!(
            red_eye_islands(&offset),
            [IntRect::new(2, 1, 1, 1), IntRect::new(4, 1, 1, 1)]
        );

        let img = from_fn(4, 1, |x, _| {
            [
                [220, 40, 40, 255],
                [50, 40, 40, 255],
                [48, 40, 40, 255],
                [220, 40, 40, 255],
            ][x as usize]
        });
        let mask = Mask {
            rect: IntRect::new(0, 0, 4, 1),
            data: vec![255, 255, 255, 128],
        };
        let mut env = crate::effect::EffectEnv::whole(4, 1);
        env.mask = Some(&mask);
        let red = |s: f64| {
            let out = run_env("redEyeRemoveEffect", json!({"strength": s}), &img, &env);
            (0..4).map(|x| px(&out, x, 0)).collect::<Vec<_>>()
        };
        // 220 → 130 → 85 → 62; 50 → 45 then stops (45 ≤ 48); 48 never changes; the m = 128 pixel
        // is blend(source, 62, 128) = ⌊(220·127 + 62·128)/255⌋ = 140.
        assert_eq!(
            red(3.0),
            [
                [62, 40, 40, 255],
                [45, 40, 40, 255],
                [48, 40, 40, 255],
                [140, 40, 40, 255]
            ]
        );
        assert_eq!(red(1.0)[0], [130, 40, 40, 255]);
        assert_eq!(red(1e9)[0], red(5.0)[0]);
        assert_eq!(red(-9.0)[0], red(1.0)[0]);
    }

    /// R19 controls: paintdotjs's keys, labels and steps where it has the control, Paint.NET's
    /// ranges, defaults and the new Strength, Sampling and Center controls; out-of-range values
    /// are clamped.
    #[test]
    fn photo_controls_and_clamping() {
        let ranges = |id: &str| -> Vec<(&str, &str, f64, f64, f64, f64)> {
            crate::effect::find(id)
                .unwrap()
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Range {
                        key,
                        label,
                        min,
                        max,
                        default,
                        step,
                        ..
                    } => Some((*key, *label, *min, *max, *default, *step)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            ranges("glowEffect"),
            [
                ("radius", "Radius", 1.0, 20.0, 6.0, 1.0),
                ("brightness", "Brightness", -100.0, 100.0, 10.0, 1.0),
                ("contrast", "Contrast", -100.0, 100.0, 10.0, 1.0)
            ]
        );
        assert_eq!(
            ranges("redEyeRemoveEffect"),
            [("strength", "Strength", 1.0, 5.0, 3.0, 1.0)]
        );
        assert_eq!(
            ranges("sharpenEffect"),
            [
                ("amount", "Amount", 0.0, 10.0, 2.0, 0.1),
                ("threshold", "Threshold", 0.0, 1.0, 0.0, 0.01)
            ]
        );
        assert_eq!(
            ranges("softenPortraitEffect"),
            [
                ("softness", "Softness", 0.0, 10.0, 5.0, 1.0),
                ("lighting", "Lighting", -20.0, 20.0, 0.0, 1.0),
                ("warmth", "Warmth", 0.0, 20.0, 10.0, 1.0)
            ]
        );
        assert_eq!(
            ranges("straightenEffect"),
            [("angle", "Angle", -45.0, 45.0, 0.0, 0.1)]
        );
        assert_eq!(
            ranges("vignetteEffect"),
            [
                ("centerX", "Center X", -1.0, 1.0, 0.0, 0.01),
                ("centerY", "Center Y", -1.0, 1.0, 0.0, 0.01),
                ("radius", "Radius", 0.1, 4.0, 0.5, 0.01),
                ("strength", "Strength", 0.0, 1.0, 1.0, 0.01)
            ]
        );
        let straighten = crate::effect::find("straightenEffect").unwrap();
        assert!(matches!(
            &straighten.controls[1],
            Control::Select { key: "sampling", label: "Sampling", default: "bicubic", choices }
                if *choices == [("bicubic", "Bicubic"), ("bilinear", "Bilinear"), ("nearestNeighbor", "Nearest Neighbor")]
        ));
        let red_eye = crate::effect::find("redEyeRemoveEffect").unwrap();
        assert!(matches!(
            red_eye.kind,
            EffectKind::Cpu {
                clip_to_selection: false,
                ..
            }
        ));
        let img = solid(4, 3, [90, 140, 30, 200]);
        for (id, v) in [
            (
                "glowEffect",
                json!({"radius": 1e9, "brightness": -1e9, "contrast": 1e9}),
            ),
            ("glowEffect", json!({"radius": -3})),
            ("sharpenEffect", json!({"amount": 1e9, "threshold": -1})),
            (
                "softenPortraitEffect",
                json!({"softness": 1e9, "lighting": 1e9, "warmth": -1e9}),
            ),
            ("straightenEffect", json!({"angle": 1e9, "sampling": "x"})),
            (
                "vignetteEffect",
                json!({"centerX": 1e9, "centerY": -1e9, "radius": 0, "strength": 9}),
            ),
        ] {
            assert_eq!(run(id, v, &img).data.len(), 48, "{id}");
        }
        assert_eq!(
            run("straightenEffect", json!({"angle": 1e9}), &img).data,
            run("straightenEffect", json!({"angle": 45}), &img).data
        );
        assert_eq!(
            px(&run("vignetteEffect", json!({"strength": -5}), &img), 0, 0),
            [90, 140, 30, 200]
        );
    }
}
