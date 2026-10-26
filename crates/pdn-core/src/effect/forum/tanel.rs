//! Spec: paintdn-plugin-spec effects/color-color-to-alpha.md, effects/photo-sharpen-plus.md;
//! common.md §1, §2, §5.2, §5.9.
//!
//! Tanel's Color to Alpha (named Color Range to Alpha here, beside PhotoCraft's GIMP-style Color
//! to Alpha) and Sharpen+. Both originals have their own WinForms dialogs; here
//! they use the standard effect dialog with the same controls, labels, ranges and defaults (the
//! colour ranges as sliders on the original bars' gradients, Sharpen+'s pages as tabs). Not
//! carried over: the Review button (hold to see the original) and Color to Alpha's box
//! quantisation (typed range values are kept as typed).

use serde_json::json;

use crate::canvas::ImageData;
use crate::effect::bb::classic::gaussian_blur;
use crate::effect::bb::ui::{ControlUi, Help, HelpWindow, IndirectUi, Tab, TabStrip};
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, flag, num, range};

/// The hue bars' seven stops (red → yellow → lime → cyan → blue → magenta → red).
const TRACK_HUE: [u32; 7] = [
    0xFF0000, 0xFFFF00, 0x00FF00, 0x00FFFF, 0x0000FF, 0xFF00FF, 0xFF0000,
];
const TRACK_SATURATION: [u32; 2] = [0xC0C0C0, 0xFF0000];
const TRACK_BRIGHTNESS: [u32; 2] = [0x000000, 0xFFFFFF];

const fn track(stops: &'static [u32]) -> ControlUi {
    ControlUi {
        track: Some(stops),
        ..ControlUi::DEFAULT
    }
}

static COLOR_TO_ALPHA_UI: IndirectUi = IndirectUi {
    controls: &[
        ("hueFrom", track(&TRACK_HUE)),
        ("hueTo", track(&TRACK_HUE)),
        ("saturationFrom", track(&TRACK_SATURATION)),
        ("saturationTo", track(&TRACK_SATURATION)),
        ("brightnessFrom", track(&TRACK_BRIGHTNESS)),
        ("brightnessTo", track(&TRACK_BRIGHTNESS)),
    ],
    transient_keys: &["seen"],
    defaults: primary_ranges,
    on_change: keep_ordered,
    help: Help::None,
    ..IndirectUi::DEFAULT
};

static SHARPEN_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: "Edge Balance",
                keys: &["bright", "dark"],
            },
            Tab {
                caption: "Shadows",
                keys: &["shadowStrength", "shadowRange"],
            },
            Tab {
                caption: "Highlights",
                keys: &["highlightStrength", "highlightRange"],
            },
            Tab {
                caption: "Color Mask",
                keys: &["red", "green", "blue"],
            },
        ],
    }],
    above_tabs: &["amount", "radius", "threshold"],
    transient_keys: &["tab"],
    help: Help::Window(HelpWindow::Standard),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let make = |id, controls, render, title, ui| {
        let m = super::manifest(id).unwrap();
        EffectDef {
            id: m.id,
            icon: m.icon,
            category: m.category,
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(ui),
        }
    };
    let check = |key, label, default| Control::Checkbox {
        key,
        label,
        default,
    };
    vec![
        make(
            "tanelColorToAlpha",
            vec![
                check("hue", "Hue", true),
                range("hueFrom", "From:", 0.0, 360.0, 0.0),
                range("hueTo", "To:", 0.0, 360.0, 60.0),
                check("saturation", "Saturation", true),
                range("saturationFrom", "From:", 0.0, 100.0, 40.0),
                range("saturationTo", "To:", 0.0, 100.0, 80.0),
                check("brightness", "Brightness", true),
                range("brightnessFrom", "From:", 0.0, 255.0, 0.0),
                range("brightnessTo", "To:", 0.0, 255.0, 200.0),
                range("tolerance", "Tolerance", 0.0, 254.0, 0.0),
                range("feather", "Feather", 0.0, 100.0, 0.0),
                range("baseOpacity", "Base Opacity", 0.0, 255.0, 0.0),
                check("invert", "Invert Alpha", false),
                check("override", "Override Original Alpha", false),
                Control::Value {
                    key: "seen",
                    default: || json!(null),
                },
            ],
            color_to_alpha,
            "Color Range to Alpha",
            &COLOR_TO_ALPHA_UI,
        ),
        make(
            "tanelSharpenPlus",
            vec![
                range("amount", "Amount", 0.0, 500.0, 100.0),
                range("radius", "Radius", 1.0, 100.0, 1.0),
                range("threshold", "Threshold", 0.0, 100.0, 0.0),
                Control::Value {
                    key: "tab",
                    default: || json!(0),
                },
                range("bright", "Bright Borders", 0.0, 100.0, 100.0),
                range("dark", "Dark Borders", 0.0, 100.0, 100.0),
                range("shadowStrength", "Strength", 0.0, 100.0, 100.0),
                range("shadowRange", "Tonal Range", 0.0, 100.0, 50.0),
                range("highlightStrength", "Strength", 0.0, 100.0, 100.0),
                range("highlightRange", "Tonal Range", 0.0, 100.0, 30.0),
                range("red", "Red", 0.0, 200.0, 100.0),
                range("green", "Green", 0.0, 200.0, 100.0),
                range("blue", "Blue", 0.0, 200.0, 100.0),
            ],
            sharpen_plus,
            "Sharpen+",
            &SHARPEN_UI,
        ),
    ]
}

// --- Color to Alpha -------------------------------------------------------------------------

/// Paint.NET 3.x's classic HSV: hue in whole degrees and saturation in whole percent, both
/// truncated; 0 for black, white and greys.
fn hue_saturation(r: u8, g: u8, b: u8) -> (i32, i32) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if max == 0.0 || d == 0.0 {
        return (0, 0);
    }
    let h = if r == max {
        (g - b) / d
    } else if g == max {
        2.0 + (b - r) / d
    } else {
        4.0 + (r - g) / d
    };
    let mut h = h * 60.0;
    if h < 0.0 {
        h += 360.0;
    }
    (h.trunc() as i32, (d / max * 100.0).trunc() as i32)
}

/// common.md §5.9: the fixed-point BT.601 intensity.
fn intensity(r: u8, g: u8, b: u8) -> i32 {
    ((19595 * r as u32 + 38470 * g as u32 + 7471 * b as u32) >> 16) as i32
}

/// The six range values from a colour, through the bars' 0…255 units (Initial values).
fn ranges_from(c: [u8; 4]) -> [f64; 6] {
    let (h, s) = hue_saturation(c[0], c[1], c[2]);
    let i = intensity(c[0], c[1], c[2]);
    let hue_from = if h < 30 { h + 330 } else { h - 30 } * 255 / 360;
    let hue_to = if h > 330 { h - 330 } else { h + 30 } * 255 / 360;
    let sat = |v: i32| (255 * v / 100).clamp(0, 255);
    let bar_hue = |bar: i32| (bar * 360 / 255) as f64;
    let bar_sat = |bar: i32| (bar * 100 / 255) as f64;
    [
        bar_hue(hue_from),
        bar_hue(hue_to),
        bar_sat(sat(s - 20)),
        bar_sat(sat(s + 20)),
        (i - 50).clamp(0, 255) as f64,
        (i + 50).clamp(0, 255) as f64,
    ]
}

const RANGE_KEYS: [&str; 6] = [
    "hueFrom",
    "hueTo",
    "saturationFrom",
    "saturationTo",
    "brightnessFrom",
    "brightnessTo",
];

/// Initial values: a first opening in the session takes the ranges from the primary colour.
fn primary_ranges(v: &mut Values, env: &EffectEnv<'_>) {
    for (key, value) in RANGE_KEYS.iter().zip(ranges_from(env.primary)) {
        v.insert((*key).into(), json!(value));
    }
}

/// Range bars "Order rule": a Saturation or Brightness From moved past its To takes the To along,
/// and the other way round. `seen` remembers the values the last call saw, to tell which moved.
fn keep_ordered(v: &mut Values, _: &EffectEnv<'_>) {
    let seen = v.get("seen").cloned().unwrap_or_default();
    for (from, to) in [
        ("saturationFrom", "saturationTo"),
        ("brightnessFrom", "brightnessTo"),
    ] {
        let (f, t) = (num(v, from), num(v, to));
        if f > t {
            let to_moved = seen[to].as_f64().is_some_and(|old| old != t);
            let (key, value) = if to_moved { (from, t) } else { (to, f) };
            v.insert(key.into(), json!(value));
        }
    }
    let now: serde_json::Map<_, _> = RANGE_KEYS
        .iter()
        .map(|k| ((*k).to_string(), json!(num(v, k))))
        .collect();
    v.insert("seen".into(), serde_json::Value::Object(now));
}

/// The settings a render reads.
struct Alpha {
    hue: Option<(f32, f32)>,
    saturation: Option<(f32, f32)>,
    brightness: Option<(f32, f32)>,
    tolerance: f32,
    base: f32,
    invert: bool,
    override_alpha: bool,
}

impl Alpha {
    fn from(v: &Values) -> Alpha {
        let pair = |on: &str, from: &str, to: &str| {
            flag(v, on).then(|| (num(v, from) as f32, num(v, to) as f32))
        };
        Alpha {
            hue: pair("hue", "hueFrom", "hueTo"),
            saturation: pair("saturation", "saturationFrom", "saturationTo"),
            brightness: pair("brightness", "brightnessFrom", "brightnessTo"),
            tolerance: num(v, "tolerance") as f32,
            base: num(v, "baseOpacity") as f32,
            invert: flag(v, "invert"),
            override_alpha: flag(v, "override"),
        }
    }

    /// The match factor of one colour with the effective base `b` ("Soft-edge slope").
    fn matching(&self, px: [u8; 4], b: f32) -> f32 {
        let q = 1.0 - b / 255.0;
        let t = self.tolerance;
        let (h, s) = hue_saturation(px[0], px[1], px[2]);
        let i = intensity(px[0], px[1], px[2]);
        let hue = self
            .hue
            .map_or(1.0, |(h1, h2)| hue_factor(h as f32, h1, h2, t, q));
        let linear = |v: f32, (lo, hi): (f32, f32), scale: f32| {
            let away = if v > hi {
                v - hi
            } else if v < lo {
                lo - v
            } else {
                return 1.0;
            };
            (1.0 - q * ((away - 0.9) * scale) / (t + 0.01)).max(0.0)
        };
        let saturation = self.saturation.map_or(1.0, |r| linear(s as f32, r, 2.55));
        let brightness = self.brightness.map_or(1.0, |r| linear(i as f32, r, 1.0));
        hue * saturation * brightness
    }
}

/// Hue factor: inside the (possibly wrapping) range 1, else the larger of the two bounds'
/// candidates, the seam rule first.
fn hue_factor(h: f32, h1: f32, h2: f32, tolerance: f32, q: f32) -> f32 {
    let h2 = h2.min(359.0);
    let span = if h2 < h1 { h2 - h1 + 360.0 } else { h2 - h1 };
    let th = tolerance.min((360.0 - span) / 2.0 * 255.0 / 360.0);
    let th_deg = th * 360.0 / 255.0;
    let inside = if h1 <= h2 {
        h1 <= h && h <= h2
    } else {
        h >= h1 || h <= h2
    };
    if inside {
        return 1.0;
    }
    let candidate = |x: f32| {
        if x + th_deg > 359.0 {
            1.0 - q * ((360.0 - x) * 255.0 / 360.0) / (th + 0.01)
        } else if x + th_deg < 0.0 {
            0.0
        } else if x <= -1.0 {
            1.0 - q * ((-x - 0.9) * 255.0 / 360.0) / (th + 0.01)
        } else {
            0.0
        }
    };
    candidate(h2 - h).max(candidate(h - h1))
}

fn to_byte(v: f32) -> u8 {
    v.clamp(0.0, 255.0).round_ties_even() as u8
}

fn color_to_alpha(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let s = Alpha::from(v);
    let feather = num(v, "feather") as u32;
    let mut out = src.clone();
    let alpha = |px: [u8; 4]| {
        if s.override_alpha {
            255.0
        } else {
            px[3] as f32
        }
    };
    if feather == 0 {
        for p in out.data.as_chunks_mut::<4>().0 {
            let px = [p[0], p[1], p[2], p[3]];
            let m = s.matching(px, s.base).max(0.0);
            let mut a = alpha(px) * m.max(s.base / 255.0);
            if s.base > 0.0 && a < s.base {
                a = s.base;
            }
            if s.invert {
                a = 255.0 - a;
            }
            p[3] = to_byte(a);
        }
        return out;
    }
    // Feather: a grey mask over the selection (base + 1 as the floor, no Invert), blurred with
    // the classic Gaussian Blur; outside the selection it is transparent black.
    let floor = (s.base + 1.0) / 255.0;
    let mut mask = ImageData::new(src.width, src.height);
    for (i, (m, px)) in mask
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.data.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (
            (i % src.width as usize) as i32,
            (i / src.width as usize) as i32,
        );
        if env.mask.is_some_and(|sel| sel.get(x, y) == 0) {
            continue;
        }
        let px = [px[0], px[1], px[2], px[3]];
        let f = s.matching(px, s.base + 1.0).max(0.0).max(floor);
        let g = to_byte(alpha(px) * f);
        m.copy_from_slice(&[g, g, g, 255]);
    }
    let blurred = gaussian_blur(&mask, feather);
    for (p, m) in out
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(blurred.data.as_chunks::<4>().0)
    {
        let mut a = m[0] as f32;
        if s.base > 0.0 && a < s.base {
            a = s.base;
        }
        if s.invert {
            a = 255.0 - a;
        }
        p[3] = to_byte(a);
    }
    out
}

// --- Sharpen+ -------------------------------------------------------------------------------

/// Step 1: the separable tent blur of `radius` (weights 1 … r + 1 … 1), horizontal pass first,
/// each pass rounded to whole levels, edge pixels repeated beyond the canvas. R, G and B.
fn tent_blur(src: &ImageData, r: usize) -> Vec<[u8; 3]> {
    let (w, h) = (src.width as usize, src.height as usize);
    let total = ((r + 1) * (r + 1)) as u32;
    let pass = |get: &dyn Fn(isize) -> [u8; 3], n: usize| -> Vec<[u8; 3]> {
        (0..n as isize)
            .map(|i| {
                let mut sum = [0u32; 3];
                for j in -(r as isize)..=r as isize {
                    let wgt = (r + 1 - j.unsigned_abs()) as u32;
                    let c = get((i + j).clamp(0, n as isize - 1));
                    for k in 0..3 {
                        sum[k] += wgt * c[k] as u32;
                    }
                }
                sum.map(|s| ((s + total / 2) / total) as u8)
            })
            .collect()
    };
    let mut rows = Vec::with_capacity(w * h);
    for y in 0..h {
        let at = |x: isize| {
            let p = (y * w + x as usize) * 4;
            [src.data[p], src.data[p + 1], src.data[p + 2]]
        };
        rows.extend(pass(&at, w));
    }
    let mut out = vec![[0u8; 3]; w * h];
    for x in 0..w {
        let at = |y: isize| rows[y as usize * w + x];
        for (y, c) in pass(&at, h).into_iter().enumerate() {
            out[y * w + x] = c;
        }
    }
    out
}

fn sharpen_plus(src: &ImageData, v: &Values, _env: &EffectEnv) -> ImageData {
    let f = |k: &str| num(v, k) as f32;
    let blurred = tent_blur(src, num(v, "radius").max(1.0) as usize);
    let t = f("threshold") / 10.0;
    let amount = f("amount") / 100.0;
    let (bright, dark) = (f("bright") / 100.0, f("dark") / 100.0);
    let (th, hs) = (255.0 - f("highlightRange"), f("highlightStrength") / 100.0);
    let (st, ss) = (f("shadowRange"), f("shadowStrength") / 100.0);
    let cm = (f("red") + f("green") + f("blue")) / 300.0;
    let mut out = src.clone();
    for (p, b) in out.data.as_chunks_mut::<4>().0.iter_mut().zip(&blurred) {
        let c = [p[0] as f32, p[1] as f32, p[2] as f32];
        let l = (c[0] + c[1] + c[2]) / 3.0;
        let lb = (b[0] as f32 + b[1] as f32 + b[2] as f32) / 3.0;
        let diff = l - lb;
        let gate = if t == 0.0 || diff.abs() > t + 1.0 {
            1.0
        } else {
            ((diff.abs() - t) / 0.999).max(0.0)
        };
        let gain = if diff > 0.0 { bright } else { dark };
        let highlights = if lb < th - 50.0 {
            1.0
        } else if lb < th {
            1.0 - ((lb - (th - 50.0)) / 50.0) * (1.0 - hs)
        } else if lb > th {
            hs
        } else {
            0.0
        };
        let shadows = if lb < st {
            ss
        } else if lb > st + 50.0 {
            1.0
        } else if lb > st {
            1.0 - ((st + 50.0 - lb) / 50.0) * (1.0 - ss)
        } else {
            0.0
        };
        let delta = diff * gate * amount * gain * highlights * shadows * cm;
        let k = (l + delta) / if l == 0.0 { 0.001 } else { l };
        let max = c[0].max(c[1]).max(c[2]);
        let min = c[0].min(c[1]).min(c[2]);
        let rgb = if k > 255.0 / max && max > min {
            // Overflow: up to 255 on the brightest channel, then the rest of the brightness the
            // mask asked for spills toward white.
            let scaled = c.map(|x| x * 255.0 / max);
            let target: f32 = (c[0] + c[1] + c[2]) * k;
            let sum: f32 = scaled.iter().sum();
            let p = (target - sum) / (765.0 - sum);
            scaled.map(|x| x + (255.0 - x) * p)
        } else {
            c.map(|x| x * k)
        };
        for i in 0..3 {
            p[i] = to_byte(rgb[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, run};
    use serde_json::json;

    fn grey(v: u8) -> [u8; 4] {
        [v, v, v, 255]
    }

    /// One row: `a` ×5 then `b` ×5.
    fn step(a: [u8; 4], b: [u8; 4]) -> ImageData {
        from_fn(10, 1, |x, _| if x < 5 { a } else { b })
    }

    fn row(img: &ImageData) -> Vec<u8> {
        (0..img.width).map(|x| px(img, x, 0)[0]).collect()
    }

    fn sharpen(values: serde_json::Value, img: &ImageData) -> ImageData {
        run("tanelSharpenPlus", values, img)
    }

    /// photo-sharpen-plus.md Expected results 1–5.
    #[test]
    fn sharpen_plus_flat_and_step() {
        let flat = from_fn(5, 5, |_, _| grey(128));
        assert_eq!(sharpen(json!({"amount": 500}), &flat).data, flat.data);
        let edge = step(grey(100), grey(150));
        let out = |v| row(&sharpen(v, &edge));
        assert_eq!(
            out(json!({})),
            [100, 100, 100, 100, 87, 162, 150, 150, 150, 150]
        );
        assert_eq!(
            out(json!({"radius": 2})),
            [100, 100, 100, 94, 83, 167, 156, 150, 150, 150]
        );
        assert_eq!(out(json!({"amount": 0})), row(&edge));
        assert_eq!(out(json!({"amount": 500}))[4..6], [35, 210]);
        assert_eq!(out(json!({"dark": 0}))[4..6], [100, 162]);
        assert_eq!(out(json!({"bright": 0}))[4..6], [87, 150]);
    }

    /// Expected results 6–8, 11, 12: threshold, tonal protection, the equality quirk, black.
    #[test]
    fn sharpen_plus_threshold_and_tones() {
        let out = |v, a, b| row(&sharpen(v, &step(grey(a), grey(b))));
        assert_eq!(out(json!({}), 100, 108)[4..6], [98, 110]);
        assert_eq!(out(json!({"threshold": 15}), 100, 108)[4..6], [99, 109]);
        assert_eq!(out(json!({"threshold": 30}), 100, 108)[4..6], [100, 108]);
        assert_eq!(out(json!({}), 60, 90)[4..6], [52, 97]);
        assert_eq!(out(json!({"shadowStrength": 0}), 60, 90)[4..6], [57, 95]);
        assert_eq!(out(json!({}), 180, 220)[4..6], [170, 230]);
        assert_eq!(
            out(json!({"highlightStrength": 0}), 180, 220)[4..6],
            [173, 223]
        );
        assert_eq!(out(json!({}), 220, 240)[4..6], [220, 245]);
        assert_eq!(out(json!({"highlightRange": 31}), 220, 240)[4], 215);
        assert_eq!(out(json!({}), 0, 255), row(&step(grey(0), grey(255))));
    }

    /// Expected results 9, 10, 13, 14: colour scaling with overflow, the Color Mask average,
    /// alpha left alone.
    #[test]
    fn sharpen_plus_colour_and_mask() {
        let img = from_fn(9, 9, |x, y| {
            if (x, y) == (4, 4) {
                [200, 100, 50, 255]
            } else {
                [100, 50, 25, 255]
            }
        });
        let out = sharpen(json!({}), &img);
        assert_eq!(px(&out, 4, 4), [255, 141, 84, 255]);
        assert_eq!(px(&out, 3, 4), [87, 43, 22, 255]);
        assert_eq!(px(&out, 4, 3), [87, 44, 22, 255]);
        assert_eq!(px(&out, 3, 3), [94, 47, 23, 255]);
        assert_eq!(px(&out, 0, 0), [100, 50, 25, 255]);
        let dot = |c| {
            from_fn(9, 9, move |x, y| {
                grey(if (x, y) == (4, 4) { c } else { 100 })
            })
        };
        let g = |v, img: &ImageData| {
            let o = sharpen(v, img);
            [px(&o, 4, 4)[0], px(&o, 3, 4)[0], px(&o, 3, 3)[0]]
        };
        assert_eq!(g(json!({}), &dot(200)), [255, 87, 94]);
        assert_eq!(g(json!({}), &dot(140)), [170, 95, 97]);
        assert_eq!(
            g(json!({"red": 200, "green": 100, "blue": 0}), &dot(140)),
            [170, 95, 97]
        );
        assert_eq!(
            g(json!({"red": 50, "green": 50, "blue": 50}), &dot(140)),
            [155, 98, 98]
        );
        let hidden = step([100, 100, 100, 0], grey(150));
        let o = sharpen(json!({}), &hidden);
        assert_eq!(px(&o, 4, 0), [87, 87, 87, 0]);
        assert_eq!(px(&o, 5, 0)[3], 255);
    }

    fn c2a(values: serde_json::Value, colour: [u8; 4]) -> [u8; 4] {
        px(
            &run("tanelColorToAlpha", values, &from_fn(1, 1, |_, _| colour)),
            0,
            0,
        )
    }

    const RED: [f64; 6] = [328.0, 29.0, 80.0, 100.0, 26.0, 126.0];

    fn ranges(r: [f64; 6]) -> serde_json::Value {
        json!({
            "hueFrom": r[0], "hueTo": r[1], "saturationFrom": r[2], "saturationTo": r[3],
            "brightnessFrom": r[4], "brightnessTo": r[5],
        })
    }

    fn with(mut base: serde_json::Value, more: serde_json::Value) -> serde_json::Value {
        base.as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        base
    }

    /// color-color-to-alpha.md Expected results 1, 8 and 10 (the primary-colour ranges).
    #[test]
    fn color_to_alpha_ranges() {
        assert_eq!(ranges_from([255, 0, 0, 255]), RED);
        assert_eq!(
            ranges_from([0, 0, 0, 255]),
            [328.0, 29.0, 0.0, 20.0, 0.0, 50.0]
        );
        let red = ranges(RED);
        let inv = with(red.clone(), json!({"invert": true}));
        assert_eq!(c2a(red.clone(), [255, 0, 0, 255]), [255, 0, 0, 255]);
        assert_eq!(c2a(inv.clone(), [255, 0, 0, 255]), [255, 0, 0, 0]);
        assert_eq!(c2a(red.clone(), [0, 0, 255, 255]), [0, 0, 255, 0]);
        assert_eq!(c2a(inv.clone(), [0, 0, 255, 255]), [0, 0, 255, 255]);
        assert_eq!(c2a(red.clone(), [255, 255, 255, 255])[3], 0);
        assert_eq!(c2a(red.clone(), [255, 40, 40, 255])[3], 255);
        assert_eq!(c2a(red.clone(), [200, 0, 0, 255])[3], 255);
        assert_eq!(c2a(red.clone(), [255, 0, 0, 128])[3], 128);
        assert_eq!(c2a(inv.clone(), [255, 0, 0, 128])[3], 127);
        assert_eq!(c2a(red, [0, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(c2a(inv, [0, 0, 0, 0]), [0, 0, 0, 255]);
        let black = ranges([328.0, 29.0, 0.0, 20.0, 0.0, 50.0]);
        assert_eq!(c2a(black.clone(), grey(0))[3], 255);
        assert_eq!(c2a(black.clone(), grey(40))[3], 255);
        assert_eq!(c2a(black.clone(), grey(60))[3], 0);
        assert_eq!(c2a(black, [30, 0, 0, 255])[3], 0);
    }

    /// Expected results 2–4: brightness with tolerance, Base Opacity, Override.
    #[test]
    fn color_to_alpha_tolerance_and_base() {
        let b = json!({"hue": false, "saturation": false, "brightnessFrom": 100,
            "brightnessTo": 150, "tolerance": 20});
        let alphas = |v: serde_json::Value, levels: &[u8]| -> Vec<u8> {
            levels.iter().map(|&g| c2a(v.clone(), grey(g))[3]).collect()
        };
        assert_eq!(
            alphas(b.clone(), &[90, 100, 150, 151, 160, 170, 171, 200]),
            [139, 255, 255, 254, 139, 12, 0, 0]
        );
        let base = with(b.clone(), json!({"baseOpacity": 64}));
        assert_eq!(
            alphas(base.clone(), &[90, 160, 170, 171, 200]),
            [168, 168, 73, 64, 64]
        );
        assert_eq!(c2a(base, [200, 200, 200, 0])[3], 64);
        let plain = json!({"hue": false, "saturation": false, "brightnessFrom": 100,
            "brightnessTo": 150});
        assert_eq!(c2a(plain.clone(), [128, 128, 128, 0])[3], 0);
        assert_eq!(
            c2a(with(plain, json!({"override": true})), [128, 128, 128, 0])[3],
            255
        );
    }

    /// Expected results 5–7: wrapping hue with tolerance, the seam rule, saturation tolerance.
    #[test]
    fn color_to_alpha_hue_and_saturation() {
        let hue = json!({"saturation": false, "brightness": false, "hueFrom": 350,
            "hueTo": 10, "tolerance": 30});
        let a = |v: &serde_json::Value, c| c2a(v.clone(), c)[3];
        assert_eq!(a(&hue, [255, 86, 0, 255]), 200);
        assert_eq!(a(&hue, [255, 0, 84, 255]), 200);
        assert_eq!(a(&hue, [255, 0, 0, 255]), 255);
        assert_eq!(a(&hue, [255, 171, 0, 255]), 80);
        assert_eq!(a(&hue, [0, 255, 0, 255]), 0);
        let seam = json!({"saturation": false, "brightness": false, "hueFrom": 300,
            "hueTo": 350, "tolerance": 20});
        assert_eq!(a(&seam, [255, 0, 0, 255]), 165);
        assert_eq!(a(&seam, [255, 21, 0, 255]), 129);
        assert_eq!(a(&seam, [255, 0, 43, 255]), 255);
        let sat = json!({"hue": false, "brightness": false, "saturationFrom": 40,
            "saturationTo": 60, "tolerance": 51});
        assert_eq!(a(&sat, [255, 100, 100, 255]), 255);
        assert_eq!(a(&sat, [255, 50, 50, 255]), 12);
        assert_eq!(a(&sat, [255, 25, 25, 255]), 0);
        assert_eq!(a(&sat, [255, 155, 155, 255]), 254);
        assert_eq!(a(&sat, [255, 150, 150, 255]), 255);
    }

    /// Expected results 9: the feather mask's floor of 1, through the radius-1 classic blur.
    #[test]
    fn color_to_alpha_feather() {
        let img = from_fn(5, 1, |x, _| grey(if x == 2 { 255 } else { 0 }));
        let out = run(
            "tanelColorToAlpha",
            json!({"hue": false, "saturation": false, "brightnessFrom": 200,
                "brightnessTo": 255, "feather": 1}),
            &img,
        );
        let alpha: Vec<u8> = out.data.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alpha, [1, 64, 128, 64, 1]);
        assert_eq!(
            out.data.chunks(4).map(|p| p[0]).collect::<Vec<_>>(),
            [0, 0, 255, 0, 0]
        );
    }

    /// Range bars "Order rule": the moved bound drags the other along.
    #[test]
    fn color_to_alpha_keeps_saturation_ordered() {
        let def = crate::effect::find("tanelColorToAlpha").unwrap();
        let env = EffectEnv::whole(1, 1);
        let mut v = crate::effect::defaults(&def);
        keep_ordered(&mut v, &env);
        v.insert("saturationFrom".into(), json!(90));
        keep_ordered(&mut v, &env);
        assert_eq!(
            (num(&v, "saturationFrom"), num(&v, "saturationTo")),
            (90.0, 90.0)
        );
        v.insert("saturationTo".into(), json!(10));
        keep_ordered(&mut v, &env);
        assert_eq!(
            (num(&v, "saturationFrom"), num(&v, "saturationTo")),
            (10.0, 10.0)
        );
    }
}
