//! The 13 adjustments on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`DEFINITIONS` lines 19–153 and
//! `levelsDefinition`: ids, icons, dialog layouts and control keys only).
//! Paint.NET's inventory, working spaces and algorithms; histograms over selected pixels;
//! approximations for the Direct2D-backed four.

use super::sample::gaussian_kernel;
use super::surface::{Alpha, Edge, EffectSurface, Space, luma, premultiply, unpremultiply};
use super::{
    Category, Control, DialogLayout, EffectDef, EffectEnv, EffectKind, GpuFn, RenderInfo, Values,
    choice, flag, num, range,
};
use crate::canvas::ImageData;
use crate::pixel::gamma::{linear_to_srgb_f32, srgb_to_linear_f32};
use crate::pixel::mask::Mask;
use serde_json::{Value, json};

const SRGB: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Straight);
const SRGB_PM: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
const LINEAR_PM: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);
const IDENTITY: &[[f64; 2]] = &[[0.0, 0.0], [255.0, 255.0]];

fn adj(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    render: GpuFn,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Adjustment,
        controls,
        kind: EffectKind::Gpu(info, render),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

fn map_px(src: &EffectSurface, f: impl Fn([f32; 4]) -> [f32; 4] + Sync) -> EffectSurface {
    EffectSurface::from_fn(src.w, src.h, |x, y| f(src.at(x, y)))
}

fn checkbox(key: &'static str, label: &'static str) -> Control {
    Control::Checkbox {
        key,
        label,
        default: true,
    }
}

/// §2.1 inventory in Paint.NET's menu order (paintdotjs ids, icons and keys; R19 ranges).
pub fn definitions() -> Vec<EffectDef> {
    let curve = |key| Control::Curve {
        key,
        default: IDENTITY,
    };
    vec![
        adj("autoLevel", "auto_level.png", vec![], SRGB, |s, _, env| {
            let img = s.to_rgba8(Space::Srgb, Alpha::Straight);
            levels_render(s, &auto_level(&histogram(&img, env.mask)))
        }),
        adj(
            "desaturateEffect",
            "desaturate_effect.png",
            vec![],
            SRGB,
            |s, _, _| {
                map_px(s, |p| {
                    let y = luma(p[0], p[1], p[2]);
                    [y, y, y, p[3]]
                })
            },
        ),
        adj(
            "brightnessAndContrastAdjustment",
            "brightness_and_contrast_adjustment.png",
            vec![
                range("brightness", "Brightness", -100.0, 100.0, 0.0),
                range("contrast", "Contrast", -100.0, 100.0, 0.0),
            ],
            SRGB,
            |s, v, _| {
                let (b, c) = (num(v, "brightness") as f32, num(v, "contrast") as f32);
                map_px(s, |p| {
                    let [r, g, b] = bc_shift([p[0], p[1], p[2]], b, c);
                    [r, g, b, p[3]]
                })
            },
        ),
        EffectDef {
            dialog: Some(DialogLayout::Curves),
            ..adj(
                "curvesEffect",
                "curves_effect.png",
                vec![
                    Control::Select {
                        key: "channel",
                        label: "Transfer Map",
                        default: "luminosity",
                        choices: &[("luminosity", "Luminosity"), ("rgb", "RGB")],
                    },
                    curve("points"),
                    curve("redPoints"),
                    curve("greenPoints"),
                    curve("bluePoints"),
                ],
                SRGB,
                curves,
            )
        },
        adj(
            "exposureEffect",
            "exposure_effect.png",
            vec![range("exposure", "Exposure", -200.0, 200.0, 0.0)],
            LINEAR_PM,
            // approximation: D2D Exposure (§6 row 2, B-A2)
            |s, v, _| {
                let g = (num(v, "exposure") as f32 / 100.0).exp2();
                map_px(s, |p| [p[0] * g, p[1] * g, p[2] * g, p[3]])
            },
        ),
        adj(
            "highlightsAndShadowsEffect",
            "highlights_and_shadows_effect.png",
            vec![
                range("highlights", "Highlights", -100.0, 100.0, 0.0),
                range("shadows", "Shadows", -100.0, 100.0, 0.0),
                range("clarity", "Clarity", -100.0, 100.0, 0.0),
                range("radius", "Radius", 0.0, 10.0, 1.25).step(0.25),
            ],
            SRGB_PM,
            highlights_shadows,
        ),
        EffectDef {
            dialog: Some(DialogLayout::HueSaturation),
            ..adj(
                "hueAndSaturationAdjustment",
                "hue_and_saturation_adjustment.png",
                vec![
                    range("hue", "Hue", -180.0, 180.0, 0.0),
                    range("saturation", "Saturation", 0.0, 200.0, 100.0),
                    range("lightness", "Lightness", -100.0, 100.0, 0.0),
                ],
                SRGB_PM,
                hue_saturation,
            )
        },
        adj(
            "invertAlphaEffect",
            "invert_alpha_effect.png",
            vec![],
            SRGB,
            |s, _, _| map_px(s, |p| [p[0], p[1], p[2], 1.0 - p[3]]),
        ),
        adj(
            "invertColorsEffect",
            "invert_colors_effect.png",
            vec![],
            SRGB,
            |s, _, _| map_px(s, |p| [1.0 - p[0], 1.0 - p[1], 1.0 - p[2], p[3]]),
        ),
        EffectDef {
            dialog: Some(DialogLayout::Levels),
            dialog_title: Some("Levels Adjustment"),
            ..adj(
                "levelsEffect",
                "levels_effect.png",
                levels_controls(),
                SRGB,
                |s, v, _| levels_render(s, &LevelsParams::from_values(v)),
            )
        },
        adj(
            "posterizeAdjustment",
            "posterize_effect_icon.png",
            vec![
                range("red", "Red", 2.0, 64.0, 16.0),
                range("green", "Green", 2.0, 64.0, 16.0),
                range("blue", "Blue", 2.0, 64.0, 16.0),
                range("alpha", "Alpha", 2.0, 64.0, 16.0),
                // Labels pending B-E13.
                checkbox("redEnabled", "Enable red"),
                checkbox("greenEnabled", "Enable green"),
                checkbox("blueEnabled", "Enable blue"),
                checkbox("alphaEnabled", "Enable alpha"),
                checkbox("linked", "Linked"),
            ],
            SRGB,
            posterize,
        ),
        adj(
            "sepiaEffect",
            "sepia_effect.png",
            vec![range("intensity", "Intensity", 0.0, 100.0, 50.0)],
            SRGB,
            |s, v, _| {
                let i = num(v, "intensity") as f32 / 100.0;
                let lerp = |b: f32| 1.0 + i * (b - 1.0);
                let (er, eb) = (lerp(0.6), lerp(1.4));
                map_px(s, |p| {
                    let y = luma(p[0], p[1], p[2]);
                    let pow = |e: f32| y.abs().powf(e) * y.signum();
                    [pow(er), y, pow(eb), p[3]]
                })
            },
        ),
        adj(
            "temperatureAndTintEffect",
            "temperature_and_tint_effect.png",
            vec![
                range("temperature", "Temperature", -100.0, 100.0, 0.0),
                range("tint", "Tint", -100.0, 100.0, 0.0),
            ],
            SRGB_PM,
            temperature_tint,
        ),
    ]
}

/// Levels: byte triples, gamma as the exponent (R19), and the R/G/B edit-mask checkboxes.
fn levels_controls() -> Vec<Control> {
    let triple = |key, default| Control::Value { key, default };
    vec![
        triple("inputLow", || json!([0.0, 0.0, 0.0])),
        triple("inputHigh", || json!([255.0, 255.0, 255.0])),
        triple("gamma", || json!([1.0, 1.0, 1.0])),
        triple("outputLow", || json!([0.0, 0.0, 0.0])),
        triple("outputHigh", || json!([255.0, 255.0, 255.0])),
        checkbox("red", "R"),
        checkbox("green", "G"),
        checkbox("blue", "B"),
    ]
}

/// §2.9 Brightness / Contrast steps 1–6; B and C on the ±100 scale.
pub fn bc_shift(rgb: [f32; 3], brightness: f32, contrast: f32) -> [f32; 3] {
    let y = luma(rgb[0], rgb[1], rgb[2]);
    // Step 1: the effect property carries B/100 and multiplies it back by 100 in f32.
    let beta = brightness / 100.0 * 100.0 / 255.0;
    let (mu, delta) = if contrast < 0.0 {
        (contrast + 100.0, 100.0)
    } else if contrast > 0.0 {
        (100.0, 100.0 - contrast)
    } else {
        (1.0, 1.0)
    };
    if delta == 0.0 {
        return [if y + beta < 0.5 { 0.0 } else { 1.0 }; 3];
    }
    let (p, t) = if delta == 100.0 {
        (0.0, beta)
    } else {
        (beta, 0.0)
    };
    let s = ((y - 0.5 + p) * mu) * (1.0 / delta) + ((0.5 - y) + t);
    rgb.map(|c| (c + s).clamp(0.0, 1.0))
}

/// §2.9 Levels parameters: per-channel input black/white, gamma (the exponent), output black/white.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelsParams {
    pub lo: [f32; 3],
    pub hi: [f32; 3],
    pub gamma: [f32; 3],
    pub out_lo: [f32; 3],
    pub out_hi: [f32; 3],
}

impl LevelsParams {
    /// The byte triples divided by 255 in f32; gamma as f32. A missing key takes the control's
    /// default.
    pub fn from_values(values: &Values) -> Self {
        let defaults: Values = levels_controls()
            .iter()
            .map(|c| (c.key().to_string(), c.default_value()))
            .collect();
        let t = |key: &str, scale: f32| -> [f32; 3] {
            let a = values
                .get(key)
                .or(defaults.get(key))
                .and_then(Value::as_array);
            std::array::from_fn(|i| {
                a.and_then(|a| a.get(i))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0) as f32
                    / scale
            })
        };
        Self {
            lo: t("inputLow", 255.0),
            hi: t("inputHigh", 255.0),
            gamma: t("gamma", 1.0),
            out_lo: t("outputLow", 255.0),
            out_hi: t("outputHigh", 255.0),
        }
    }

    pub fn lut(&self, c: usize) -> [f32; 256] {
        levels_lut(
            self.lo[c],
            self.hi[c],
            self.gamma[c],
            self.out_lo[c],
            self.out_hi[c],
        )
    }
}

/// §2.9 Levels LUT.
pub fn levels_lut(b: f32, w: f32, gamma: f32, o: f32, out_w: f32) -> [f32; 256] {
    std::array::from_fn(|i| {
        let x = (i as f64 / 255.0) as f32;
        let d = x - b;
        if d < 0.0 {
            o
        } else if d + b >= w {
            out_w
        } else {
            let n = d as f64 / (w as f64 - b as f64);
            ((o as f64 + (out_w as f64 - o as f64) * n.powf(gamma as f64)) as f32).clamp(0.0, 1.0)
        }
    })
}

/// R, G and B through their Levels LUTs (a Direct2D table transfer); alpha unchanged.
fn levels_render(src: &EffectSurface, p: &LevelsParams) -> EffectSurface {
    let luts = [p.lut(0), p.lut(1), p.lut(2)];
    map_px(src, |px| {
        let f = |c: usize| sample_table(&luts[c], px[c]);
        [f(0), f(1), f(2), px[3]]
    })
}

/// 256 bins per channel (R, G, B, A) over the pixels whose mask byte is > 0, transparent ones
/// included (§2.5 step 3, §2.9 Auto-Level step 1).
pub fn histogram(src: &ImageData, mask: Option<&Mask>) -> [[u32; 256]; 4] {
    let mut h = [[0u32; 256]; 4];
    let w = src.width.max(1) as usize;
    for (i, p) in src.data.as_chunks::<4>().0.iter().enumerate() {
        if mask.is_none_or(|m| m.get((i % w) as i32, (i / w) as i32) > 0) {
            for c in 0..4 {
                h[c][p[c] as usize] += 1;
            }
        }
    }
    h
}

/// §2.9 Auto-Level steps 2–4 per R, G, B.
pub fn auto_level(hist: &[[u32; 256]; 4]) -> LevelsParams {
    let k = |j: usize| (j as f64 / 255.0) as f32;
    let mut p = LevelsParams {
        lo: [0.0; 3],
        hi: [1.0; 3],
        gamma: [1.0; 3],
        out_lo: [0.0; 3],
        out_hi: [1.0; 3],
    };
    for (c, bins) in hist.iter().take(3).enumerate() {
        let n: u64 = bins.iter().map(|&b| b as u64).sum();
        let pct = |q: f32| {
            let goal = q as f64 * n as f64;
            let mut cum = 0u64;
            k(bins
                .iter()
                .position(|&b| {
                    cum += b as u64;
                    cum as f64 >= goal
                })
                .unwrap_or(255))
        };
        let (black, white) = (pct(0.005), pct(0.995));
        let sum: f64 = bins
            .iter()
            .enumerate()
            .map(|(j, &b)| j as f64 / 255.0 * b as f64)
            .sum();
        let mean = (sum / n.max(1) as f64) as f32;
        p.lo[c] = black;
        p.hi[c] = white;
        if black < mean && mean < white {
            let r = (mean as f64 - black as f64) / (white as f64 - black as f64);
            p.gamma[c] = (0.5f64.ln() / r.ln()).clamp(0.1, 10.0) as f32;
        }
    }
    p
}

/// §2.9 Curves steps 2–3: natural cubic spline in f64 through (x/255, y/255), sampled at 1024
/// points i/1023 as f32, unclamped. Fewer than 2 points give the identity.
pub fn curve_lut(points: &[[f64; 2]]) -> Vec<f32> {
    curve_samples(points, 1024)
        .into_iter()
        .map(|v| v as f32)
        .collect()
}

/// §2.9 Curves step 2: the spline at the `n` points i/(n − 1), in f64 (the editor draws n = 256).
pub fn curve_samples(points: &[[f64; 2]], n_out: usize) -> Vec<f64> {
    if points.len() < 2 {
        return curve_samples(IDENTITY, n_out);
    }
    let x: Vec<f64> = points.iter().map(|p| p[0] / 255.0).collect();
    let y: Vec<f64> = points.iter().map(|p| p[1] / 255.0).collect();
    let n = x.len();
    let (mut y2, mut u) = (vec![0.0; n], vec![0.0; n]);
    for i in 1..n - 1 {
        let sig = (x[i] - x[i - 1]) / (x[i + 1] - x[i - 1]);
        let p = sig * y2[i - 1] + 2.0;
        y2[i] = (sig - 1.0) / p;
        let d = (y[i + 1] - y[i]) / (x[i + 1] - x[i]) - (y[i] - y[i - 1]) / (x[i] - x[i - 1]);
        u[i] = (6.0 * d / (x[i + 1] - x[i - 1]) - sig * u[i - 1]) / p;
    }
    for k in (0..n - 1).rev() {
        y2[k] = y2[k] * y2[k + 1] + u[k];
    }
    let last = n_out.saturating_sub(1).max(1) as f64;
    (0..n_out)
        .map(|i| {
            let t = i as f64 / last;
            let (mut lo, mut hi) = (0, n - 1);
            while hi - lo > 1 {
                let k = (hi + lo) / 2;
                if x[k] > t { hi = k } else { lo = k }
            }
            let h = x[hi] - x[lo];
            let (a, b) = ((x[hi] - t) / h, (t - x[lo]) / h);
            a * y[lo]
                + b * y[hi]
                + ((a * a * a - a) * y2[lo] + (b * b * b - b) * y2[hi]) * (h * h) / 6.0
        })
        .collect()
}

/// §2.9 Curves step 4 (a Direct2D table transfer): t = v·(N − 1), lerp entries ⌊t⌋ and the next
/// (clamped at both ends).
pub fn sample_table(lut: &[f32], v: f32) -> f32 {
    let last = lut.len() - 1;
    let t = (v * last as f32).clamp(0.0, last as f32);
    let i = t.floor() as usize;
    let (a, b) = (lut[i], lut[(i + 1).min(last)]);
    a + (t - i as f32) * (b - a)
}

/// The token's points, sorted by x (§2.9 Curves step 1).
fn curve_points(v: &Values, key: &str) -> Vec<[f64; 2]> {
    let mut pts = v
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
        .collect::<Vec<_>>();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    pts
}

/// §2.9 Curves steps 5–6.
fn curves(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    if choice(v, "channel") == "rgb" {
        let t = ["redPoints", "greenPoints", "bluePoints"].map(|k| curve_lut(&curve_points(v, k)));
        // Clamped only at quantization.
        return map_px(src, |p| {
            [
                sample_table(&t[0], p[0]),
                sample_table(&t[1], p[1]),
                sample_table(&t[2], p[2]),
                p[3],
            ]
        });
    }
    let t = curve_lut(&curve_points(v, "points"));
    map_px(src, |p| {
        let l = luma(p[0], p[1], p[2]);
        let d = sample_table(&t, l).clamp(0.0, 1.0) - l;
        let c = [p[0] + d, p[1] + d, p[2] + d];
        let l2 = luma(c[0], c[1], c[2]);
        let (lo, hi) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
        let c = if lo < 0.0 {
            c.map(|x| l2 + (x - l2) * l2 / (l2 - lo))
        } else if hi > 1.0 {
            c.map(|x| l2 + (x - l2) * (1.0 - l2) / (hi - l2))
        } else {
            c
        };
        // saturate(c″) is q()'s clamp.
        [c[0], c[1], c[2], p[3]]
    })
}

/// §2.9 Posterize LUT: entry v = ⌊255·j_v/(n − 1)⌋/255, j₀ = 0, j_v = ⌊(v·n − 1)/255⌋; n is
/// clamped to the control's 2..=64.
pub fn posterize_lut(levels: u32) -> [f32; 256] {
    let n = levels.clamp(2, 64);
    std::array::from_fn(|v| {
        let j = if v == 0 { 0 } else { (v as u32 * n - 1) / 255 };
        (255 * j / (n - 1)) as f32 / 255.0
    })
}

/// §2.9 Posterize: every enabled channel, alpha included; `linked` uses `red`'s level throughout.
fn posterize(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let linked = flag(v, "linked");
    let luts = [
        ("red", "redEnabled"),
        ("green", "greenEnabled"),
        ("blue", "blueEnabled"),
        ("alpha", "alphaEnabled"),
    ]
    .map(|(k, on)| {
        flag(v, on).then(|| posterize_lut(num(v, if linked { "red" } else { k }) as u32))
    });
    map_px(src, |p| {
        std::array::from_fn(|c| luts[c].as_ref().map_or(p[c], |t| sample_table(t, p[c])))
    })
}

pub fn rgb_to_hsl(r: f32, g: f32, b: f32) -> [f32; 3] {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (l, d) = ((max + min) / 2.0, max - min);
    if d == 0.0 {
        return [0.0, 0.0, l];
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        (g - b) / d
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [(h / 6.0).rem_euclid(1.0), s, l]
}

/// Unclamped: S > 1 overshoots and q() clips.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let [r, g, b] = match hp as u32 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    [r + m, g + m, b + m]
}

fn hue_saturation(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let [hue, sat, light] = ["hue", "saturation", "lightness"].map(|k| num(v, k) as f32);
    map_px(src, |p| {
        let u = unpremultiply(p);
        let [r, g, b] = hsl_adjust([u[0], u[1], u[2]], hue, sat, light);
        premultiply([r, g, b, u[3]])
    })
}

/// §2.9 Hue / Saturation on straight colour, with standard f32 HSL for D2D RgbToHue/HueToRgb:
/// hue in degrees, saturation and lightness on the ±100 scales (100 = unchanged saturation).
// approximation: D2D RgbToHue/HueToRgb (§6 row 1, B-A1)
pub fn hsl_adjust(
    rgb: [f32; 3],
    hue_deg: f32,
    saturation_pct: f32,
    lightness_pct: f32,
) -> [f32; 3] {
    let hue = hue_deg / 360.0;
    let s = saturation_pct / 100.0;
    let k = if s <= 1.0 { s } else { (s - 1.0) * 3.0 + 1.0 };
    let l = lightness_pct;
    let (target, amount) = (if l < 0.0 { 0.0 } else { 1.0 }, l.abs() / 100.0);
    let [h, sat, lum] = rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
    hsl_to_rgb((h + hue).rem_euclid(1.0), sat * k, lum).map(|c| c + (target - c) * amount)
}

/// Unpremultiplied, linear-light RGB plus straight alpha.
fn to_linear(p: [f32; 4]) -> [f32; 4] {
    let u = unpremultiply(p);
    [
        srgb_to_linear_f32(u[0]),
        srgb_to_linear_f32(u[1]),
        srgb_to_linear_f32(u[2]),
        u[3],
    ]
}

fn from_linear(p: [f32; 4]) -> [f32; 4] {
    premultiply([
        linear_to_srgb_f32(p[0]),
        linear_to_srgb_f32(p[1]),
        linear_to_srgb_f32(p[2]),
        p[3],
    ])
}

// approximation: D2D HighlightsShadows (B-A3)
fn highlights_shadows(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let [h, s, c] = ["highlights", "shadows", "clarity"].map(|k| num(v, k) as f32 / 100.0);
    if h == 0.0 && s == 0.0 && c == 0.0 {
        return src.clone();
    }
    let lin = map_px(src, to_linear);
    let y = map_px(&lin, |p| [luma(p[0], p[1], p[2]), 0.0, 0.0, 0.0]);
    let m = y.convolve_hv(&gaussian_kernel(num(v, "radius")), Edge::Mirror);
    EffectSurface::from_fn(src.w, src.h, |px, py| {
        let (p, yv, mv) = (
            lin.at(px, py),
            y.at(px, py)[0],
            m.at(px, py)[0].clamp(0.0, 1.0),
        );
        let g = (1.5 * (s * (1.0 - mv) * (1.0 - mv) + h * mv * mv)).exp2();
        let f = |l: f32| l * g + c * (yv - mv) * 0.5;
        from_linear([f(p[0]), f(p[1]), f(p[2]), p[3]])
    })
}

fn temperature_tint(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let [t, n] = ["temperature", "tint"].map(|k| num(v, k) as f32 / 100.0);
    if t == 0.0 && n == 0.0 {
        return src.clone();
    }
    map_px(src, |p| {
        let l = to_linear(p);
        let [r, g, b] = temp_tint_linear([l[0], l[1], l[2]], t, n);
        from_linear([r, g, b, l[3]])
    })
}

/// The per-channel gain on linear-light RGB, temperature and tint on −1…1 (0, 0 = identity).
// approximation: D2D TemperatureTint (B-A4)
pub fn temp_tint_linear(rgb_linear: [f32; 3], temperature: f32, tint: f32) -> [f32; 3] {
    let (t, n) = (temperature, tint);
    let gain = [0.5 * t - 0.25 * n, 0.5 * n, -0.5 * t - 0.25 * n].map(f32::exp2);
    std::array::from_fn(|c| rgb_linear[c] * gain[c])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::test_util::{px, run, run_env, solid};
    use serde_json::json;

    fn one(id: &str, values: serde_json::Value, p: [u8; 4]) -> [u8; 4] {
        px(&run(id, values, &solid(1, 1, p)), 0, 0)
    }
    fn rgb(id: &str, values: serde_json::Value, c: [u8; 3]) -> [u8; 3] {
        let o = one(id, values, [c[0], c[1], c[2], 255]);
        [o[0], o[1], o[2]]
    }
    fn gray_map(id: &str, values: serde_json::Value, ks: &[u8]) -> Vec<u8> {
        ks.iter()
            .map(|&k| rgb(id, values.clone(), [k, k, k])[0])
            .collect()
    }
    fn within(got: [u8; 3], want: [u8; 3], tol: u8) -> bool {
        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= tol)
    }

    /// §5 Task 38 vectors, Exact.
    #[test]
    fn black_and_white_and_inverts() {
        let bw = |c| rgb("desaturateEffect", json!({}), c)[0];
        assert_eq!(
            [
                bw([255, 0, 0]),
                bw([0, 255, 0]),
                bw([0, 0, 255]),
                bw([200, 100, 50])
            ],
            [76, 150, 29, 124]
        );
        assert_eq!(
            one("invertColorsEffect", json!({}), [10, 20, 30, 0]),
            [245, 235, 225, 0]
        );
        assert_eq!(
            one("invertAlphaEffect", json!({}), [10, 20, 30, 77]),
            [10, 20, 30, 178]
        );
    }

    #[test]
    fn brightness_contrast() {
        let bc = |b: i32, c: i32, p| {
            rgb(
                "brightnessAndContrastAdjustment",
                json!({"brightness": b, "contrast": c}),
                p,
            )
        };
        assert_eq!(bc(0, 50, [125, 125, 124]), [122, 122, 121]);
        assert_eq!(bc(20, -50, [200, 100, 50]), [222, 122, 72]);
        assert_eq!(bc(40, 0, [10, 10, 10]), [50, 50, 50]);
        assert_eq!(bc(0, 100, [127, 127, 127]), [0, 0, 0]);
        assert_eq!(bc(1, 100, [127, 127, 127]), [255, 255, 255]);
    }

    /// Sepia default 50 (R19); GPU pow: Max(1).
    #[test]
    fn sepia() {
        let s = |i: i32, p| rgb("sepiaEffect", json!({"intensity": i}), p);
        assert_eq!(
            crate::effect::defaults(&crate::effect::find("sepiaEffect").unwrap())["intensity"],
            json!(50.0)
        );
        for (i, want) in [
            (50, [143, 124, 108]),
            (100, [166, 124, 93]),
            (0, [124, 124, 124]),
        ] {
            assert!(within(s(i, [200, 100, 50]), want, 1), "{i}");
        }
        for (i, want) in [
            (50, [136, 116, 99]),
            (100, [159, 116, 85]),
            (0, [116, 116, 116]),
        ] {
            assert!(within(s(i, [64, 128, 192]), want, 1), "{i}");
        }
    }

    #[test]
    fn posterize_floors_and_posterizes_alpha() {
        let v16 = json!({"red": 16, "green": 16, "blue": 16, "alpha": 16});
        assert_eq!(
            gray_map(
                "posterizeAdjustment",
                v16.clone(),
                &[9, 15, 16, 31, 32, 127, 128, 239, 240, 255]
            ),
            [0, 0, 17, 17, 34, 119, 136, 238, 255, 255]
        );
        let v3 = json!({"red": 3, "green": 3, "blue": 3, "alpha": 3});
        assert_eq!(
            gray_map("posterizeAdjustment", v3, &[85, 86, 170, 171]),
            [0, 127, 127, 255]
        );
        assert_eq!(one("posterizeAdjustment", v16, [0, 0, 0, 128])[3], 136);
        let off = json!({"red": 3, "green": 3, "blue": 3, "alpha": 3, "redEnabled": false, "linked": false});
        assert_eq!(
            rgb("posterizeAdjustment", off, [86, 86, 86]),
            [86, 127, 127]
        );
    }

    #[test]
    fn levels_gamma_is_the_exponent() {
        let lv = |v: serde_json::Value, ks: &[u8]| gray_map("levelsEffect", v, ks);
        let a = json!({"inputLow": [51, 51, 51], "inputHigh": [204, 204, 204], "gamma": [2.0, 2.0, 2.0]});
        assert_eq!(
            lv(a, &[51, 52, 100, 128, 180, 203, 204]),
            [0, 0, 26, 65, 181, 252, 255]
        );
        assert_eq!(
            lv(json!({"gamma": [0.5, 0.5, 0.5]}), &[1, 64, 128, 192]),
            [16, 128, 181, 221]
        );
        assert_eq!(
            lv(
                json!({"outputLow": [32, 32, 32], "outputHigh": [223, 223, 223]}),
                &[0, 1, 128, 254, 255]
            ),
            [32, 33, 128, 222, 223]
        );
    }

    /// The Levels quirk is dropped (§2.5, §4): Levels honours a partial mask.
    #[test]
    fn levels_honours_the_selection() {
        use crate::pixel::{IntRect, mask::Mask};
        let src = solid(2, 1, [100, 100, 100, 255]);
        let mask = Mask {
            rect: IntRect::new(0, 0, 2, 1),
            data: vec![255, 128],
        };
        let mut env = crate::effect::EffectEnv::whole(2, 1);
        env.mask = Some(&mask);
        let out = run_env(
            "levelsEffect",
            json!({"gamma": [2.0, 2.0, 2.0]}),
            &src,
            &env,
        );
        let full = px(&out, 0, 0);
        assert_eq!(
            px(&out, 1, 0),
            crate::pixel::ops::blend([100, 100, 100, 255], full, 128)
        );
    }

    #[test]
    fn auto_level_vectors() {
        let ramp = ImageData {
            width: 161,
            height: 1,
            data: (40u8..=200).flat_map(|k| [k, k, k, 255]).collect(),
        };
        let out = run("autoLevel", json!({}), &ramp);
        let at = |k: u8| px(&out, (k - 40) as u32, 0)[0];
        assert_eq!(
            [at(40), at(41), at(100), at(160), at(199), at(200)],
            [0, 2, 96, 191, 253, 255]
        );
        assert!(at(120).abs_diff(128) <= 1); // exact tie at 127.5 (B-A5)
        let mut data = vec![];
        for (k, n) in [(10u8, 50), (100, 900), (240, 50)] {
            data.extend(std::iter::repeat_n([k, k, k, 255], n).flatten());
        }
        let img = ImageData {
            width: 1000,
            height: 1,
            data,
        };
        let out = run("autoLevel", json!({}), &img);
        assert_eq!(
            [px(&out, 0, 0)[0], px(&out, 50, 0)[0], px(&out, 999, 0)[0]],
            [0, 125, 255]
        );
        let mut data = vec![];
        for (k, n) in [(0u8, 4), (100, 992), (255, 4)] {
            data.extend(std::iter::repeat_n([k, k, k, 255], n).flatten());
        }
        let img = ImageData {
            width: 1000,
            height: 1,
            data,
        };
        let out = run("autoLevel", json!({}), &img);
        assert_eq!(
            [px(&out, 0, 0)[0], px(&out, 4, 0)[0], px(&out, 999, 0)[0]],
            [0, 255, 255]
        );
    }

    /// Histograms count selected pixels only (mask > 0), transparent ones included (§2.5 step 3).
    #[test]
    fn histogram_counts_selected_pixels() {
        use crate::pixel::{IntRect, mask::Mask};
        let img = ImageData {
            width: 3,
            height: 1,
            data: vec![10, 0, 0, 0, 20, 0, 0, 255, 30, 0, 0, 255],
        };
        let mask = Mask {
            rect: IntRect::new(0, 0, 3, 1),
            data: vec![1, 255, 0],
        };
        let h = histogram(&img, Some(&mask));
        assert_eq!((h[0][10], h[0][20], h[0][30], h[3][0]), (1, 1, 0, 1));
    }

    /// Curves: 1024-entry spline LUT + table sampling; Max(1) (§6 row 7).
    #[test]
    fn curves_vectors() {
        let lum =
            json!({"channel": "luminosity", "points": [[0, 0], [64, 100], [190, 140], [255, 255]]});
        for (p, want) in [
            ([64, 64, 64], [100, 100, 100]),
            ([128, 128, 128], [118, 118, 118]),
            ([200, 100, 50], [194, 94, 44]),
            ([20, 200, 240], [0, 164, 200]),
            ([250, 250, 10], [213, 213, 0]),
        ] {
            assert!(
                within(rgb("curvesEffect", lum.clone(), p), want, 1),
                "{p:?}"
            );
        }
        let rgbc = json!({"channel": "rgb", "redPoints": [[0, 30], [255, 220]], "greenPoints": [[0, 0], [128, 200], [255, 255]], "bluePoints": [[0, 255], [255, 0]]});
        for (p, want) in [
            ([0, 0, 0], [30, 0, 255]),
            ([64, 64, 64], [78, 114, 191]),
            ([128, 128, 128], [125, 200, 127]),
            ([200, 100, 50], [179, 167, 205]),
            ([255, 255, 255], [220, 255, 0]),
        ] {
            assert!(
                within(rgb("curvesEffect", rgbc.clone(), p), want, 1),
                "{p:?}"
            );
        }
        assert_eq!(curve_lut(&[[0.0, 0.0], [255.0, 255.0]]).len(), 1024);
    }

    /// Hue/Saturation: standard f32 HSL standing in for D2D RgbToHue/HueToRgb (§6 row 1); Max(1) until B-A1.
    #[test]
    fn hue_saturation_approximation() {
        let hs = |h: i32, s: i32, l: i32, p| {
            rgb(
                "hueAndSaturationAdjustment",
                json!({"hue": h, "saturation": s, "lightness": l}),
                p,
            )
        };
        for (h, s, l, p, want) in [
            (0, 200, 0, [200, 100, 100], [255, 0, 0]),
            (180, 100, 0, [255, 0, 0], [0, 255, 255]),
            (-180, 100, 0, [255, 0, 0], [0, 255, 255]),
            (30, 100, 0, [200, 100, 50], [200, 175, 50]),
            (0, 100, -50, [200, 100, 50], [100, 50, 25]),
            (0, 100, 50, [200, 100, 50], [227, 177, 152]),
            (0, 0, 0, [200, 100, 50], [125, 125, 125]),
        ] {
            assert!(within(hs(h, s, l, p), want, 1), "{h} {s} {l} {p:?}");
        }
        // Premultiplied path: hidden RGB becomes 0 (§5 Task 38).
        assert_eq!(
            one(
                "hueAndSaturationAdjustment",
                json!({"hue": 30}),
                [90, 180, 250, 0]
            ),
            [0, 0, 0, 0]
        );
    }

    /// Exposure: linear-light ×2^(E/100) (§6 row 2); Max(1) until B-A2.
    #[test]
    fn exposure_approximation() {
        let e = |k: u8, v: i32| rgb("exposureEffect", json!({"exposure": v}), [k, k, k])[0];
        for (k, v, want) in [
            (128u8, 100, 176u8),
            (255, -100, 188),
            (64, 200, 125),
            (200, -200, 106),
            (77, 0, 77),
        ] {
            assert!(e(k, v).abs_diff(want) <= 1, "{k} {v}");
        }
    }

    /// Highlights & Shadows and Temperature & Tint: identity at 0, direction tests only (§6 rows 3–4; B-A3, B-A4).
    #[test]
    fn d2d_placeholders_are_neutral_at_zero() {
        let img = crate::effect::test_util::from_fn(9, 9, |x, y| {
            [(x * 28) as u8, (y * 28) as u8, 128, 255]
        });
        assert_eq!(
            run("highlightsAndShadowsEffect", json!({}), &img).data,
            img.data
        );
        assert_eq!(
            run("temperatureAndTintEffect", json!({}), &img).data,
            img.data
        );
        let dark = rgb(
            "highlightsAndShadowsEffect",
            json!({"shadows": 100}),
            [40, 40, 40],
        );
        assert!(dark[0] > 40, "{dark:?}");
        let warm = rgb(
            "temperatureAndTintEffect",
            json!({"temperature": 100}),
            [128, 128, 128],
        );
        assert!(warm[0] > 128 && warm[2] < 128, "{warm:?}");
        let green = rgb(
            "temperatureAndTintEffect",
            json!({"tint": 100}),
            [128, 128, 128],
        );
        assert!(green[1] > 128, "{green:?}");
    }

    /// §2.9 Levels: each channel uses its own triple element.
    #[test]
    fn levels_triples_are_per_channel() {
        let v =
            json!({"inputLow": [0, 51, 0], "inputHigh": [255, 204, 255], "gamma": [1.0, 2.0, 0.5]});
        assert_eq!(rgb("levelsEffect", v, [128, 128, 128]), [128, 65, 181]);
    }

    /// §2.9 Auto-Level steps 2–4 per channel, exact (doc γ 0.76097214 = 0x3f42cf12). The ramp's
    /// ratio (mean − black)/(white − black) of the f32 statistics is 0.49999998812 in f64, not ½,
    /// so its γ is 1 − 2⁻²⁴ (an f32 ratio would give 0x3f7ffffd and 0x3f42cf11).
    #[test]
    fn auto_level_is_per_channel() {
        let mut h = [[0u32; 256]; 4];
        h[0][40..=200].fill(1);
        for (k, n) in [(10, 50), (100, 900), (240, 50)] {
            h[1][k] = n;
        }
        for (k, n) in [(0, 4), (100, 992), (255, 4)] {
            h[2][k] = n;
        }
        let p = auto_level(&h);
        let b = |k: u32| (k as f64 / 255.0) as f32;
        assert_eq!(p.lo, [b(40), b(10), b(100)]);
        assert_eq!(p.hi, [b(200), b(240), b(100)]);
        assert_eq!(
            p.gamma.map(f32::to_bits),
            [0.99999994f32, 0.76097214, 1.0].map(f32::to_bits)
        );
        assert_eq!((p.out_lo, p.out_hi), ([0.0; 3], [1.0; 3]));
    }

    /// §2.9 Auto-Level step 2: p = f64(0.995f32), so with N = 200 the white point needs all 200
    /// pixels (199.00000095), not 199.
    #[test]
    fn auto_level_white_uses_the_f32_percentile() {
        let mut data = vec![];
        for (k, n) in [(50u8, 100), (150, 99), (250, 1)] {
            data.extend(std::iter::repeat_n([k, k, k, 255], n).flatten());
        }
        let img = ImageData {
            width: 200,
            height: 1,
            data,
        };
        let out = run("autoLevel", json!({}), &img);
        assert_eq!(
            [px(&out, 0, 0)[0], px(&out, 100, 0)[0], px(&out, 199, 0)[0]],
            [0, 180, 255]
        );
    }

    /// Hue/Sat lightness lerps toward T·a in premultiplied terms (§2.9), i.e. toward T in straight
    /// terms; Curves (straight) keeps alpha.
    #[test]
    fn alpha_paths() {
        let o = one(
            "hueAndSaturationAdjustment",
            json!({"lightness": 40}),
            [200, 100, 50, 128],
        );
        assert!(
            within([o[0], o[1], o[2]], [222, 162, 132], 1) && o[3] == 128,
            "{o:?}"
        );
        let rgbc = json!({"channel": "rgb", "redPoints": [[0, 30], [255, 220]], "greenPoints": [[0, 0], [128, 200], [255, 255]], "bluePoints": [[0, 255], [255, 0]]});
        let o = one("curvesEffect", rgbc, [64, 64, 64, 77]);
        assert!(
            within([o[0], o[1], o[2]], [78, 114, 191], 1) && o[3] == 77,
            "{o:?}"
        );
    }

    /// `linked` (default on) applies `red`'s level to every channel, alpha included.
    #[test]
    fn posterize_linked_uses_red() {
        let v = json!({"red": 3, "green": 16, "blue": 64, "alpha": 2});
        assert_eq!(
            one("posterizeAdjustment", v.clone(), [86, 86, 86, 100]),
            [127, 127, 127, 127]
        );
        let mut v = v;
        v["linked"] = json!(false);
        assert_eq!(
            one("posterizeAdjustment", v, [86, 86, 86, 100]),
            [127, 85, 85, 0]
        );
    }

    /// Token values from JSON: posterize levels clamp to the 2..=64 range, missing Levels keys
    /// fall back to the control defaults, and curve points are sorted by x.
    #[test]
    fn value_boundary() {
        assert_eq!(
            (posterize_lut(0), posterize_lut(100)),
            (posterize_lut(2), posterize_lut(64))
        );
        let d = LevelsParams {
            lo: [0.0; 3],
            hi: [1.0; 3],
            gamma: [1.0; 3],
            out_lo: [0.0; 3],
            out_hi: [1.0; 3],
        };
        assert_eq!(LevelsParams::from_values(&Values::new()), d);
        let v = json!({"points": [[255, 255], [0, 0], [128, 200]]});
        let pts = curve_points(v.as_object().unwrap(), "points");
        assert_eq!(pts, [[0.0, 0.0], [128.0, 200.0], [255.0, 255.0]]);
    }

    #[test]
    fn inventory_and_r19_controls() {
        let ids: Vec<&str> = crate::effect::definitions().iter().map(|d| d.id).collect();
        assert_eq!(
            ids,
            [
                "autoLevel",
                "desaturateEffect",
                "brightnessAndContrastAdjustment",
                "curvesEffect",
                "exposureEffect",
                "highlightsAndShadowsEffect",
                "hueAndSaturationAdjustment",
                "invertAlphaEffect",
                "invertColorsEffect",
                "levelsEffect",
                "posterizeAdjustment",
                "sepiaEffect",
                "temperatureAndTintEffect",
            ]
        );
        let hs = crate::effect::find("highlightsAndShadowsEffect").unwrap();
        let keys: Vec<&str> = hs.controls.iter().map(|c| c.key()).collect();
        assert_eq!(keys, ["highlights", "shadows", "clarity", "radius"]);
        let d = crate::effect::defaults(&hs);
        assert_eq!(d["radius"], json!(1.25));
        let ex = crate::effect::find("exposureEffect").unwrap();
        let crate::effect::Control::Range {
            min, max, default, ..
        } = &ex.controls[0]
        else {
            panic!("range")
        };
        assert_eq!((*min, *max, *default), (-200.0, 200.0, 0.0));
        assert!(
            crate::effect::definitions()
                .iter()
                .all(|d| d.category == crate::effect::Category::Adjustment)
        );
    }
}
