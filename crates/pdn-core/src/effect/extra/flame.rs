//! Photocraft phase spec: S§4.2, S§4.4 (Flame → Render, start/end Pan rows plus a curve amount);
//! research R§B (Render)
//!
//! Flame (`extra::fx::flame`): flames drawn along a path over the layer. paint.rs has no paths, so
//! the path is a quadratic curve from the Start to the End Pan row (fractions of the selection
//! bounds, as everything here anchors on the bounds), bowed by Curve: the control point leaves
//! the chord's midpoint perpendicularly by Curve % of the chord, to the chord's left (upwards for
//! a left-to-right path). The defaults follow upstream's path for a document without one, an
//! arc across the lower part. Upstream's controls are on Basic and Advanced tabs (Photoshop's),
//! the path on a third. Left out: "Adjust Interval for Loops" (the path is never a loop), the path
//! name and "New Layer" (paint.rs effects draw on the active layer). Candle Light burns at Start,
//! so its default position is Start's default (0.15 w, 0.8 h of the bounds); upstream's candle
//! for a document without a path stands at (0.5 w, 0.75 h).

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::pan;
use crate::effect::bb::ui::{IndirectUi, Tab, TabStrip};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, choice, color_value,
    flag, num, range,
};
use crate::extra::fx::flame::{self, FlameShape, FlameSpec, FlameStyle, FlameType};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use serde_json::json;

/// The multiple-flame types: only they space flames by Interval and vary their Length.
const MULTIPLE: [&str; 4] = [
    "multipleFlamesAlongPath",
    "multipleFlamesPathDirections",
    "multipleFlamesVariousLength",
    "multipleFlamesOneDirection",
];

/// Upstream's controls overflow the dialog's height, so they sit on tabs; the Pan rows need the
/// indirect layout anyway (R11).
static FLAME_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: "Basic",
                keys: &[
                    "flameType",
                    "length",
                    "randomizeLength",
                    "width",
                    "angle",
                    "interval",
                    "useCustomColor",
                    "color",
                    "quality",
                ],
            },
            Tab {
                caption: "Advanced",
                keys: &[
                    "turbulent",
                    "jag",
                    "opacity",
                    "flameLines",
                    "flameBottomAlignment",
                    "flameStyle",
                    "flameShape",
                    "randomizeShapes",
                    "seed",
                ],
            },
            Tab {
                caption: "Path",
                keys: &["start", "end", "curve"],
            },
        ],
    }],
    enabled,
    ..IndirectUi::DEFAULT
};

/// Rows that do nothing for these values are greyed: rows the flame type ignores, the colour
/// without "Use Custom Color", the shape when shapes are random, and with Start = End (a
/// zero-length path) the curve and interval. One Flame Along Path then falls back to upstream's
/// candle, which reads Length and is always oval.
fn enabled(v: &Values, key: &str) -> bool {
    let kind = choice(v, "flameType");
    let point = pan(v, "start") == pan(v, "end");
    let candle = kind == "candleLight" || kind == "oneFlameAlongPath" && point;
    match key {
        "length" => kind != "oneFlameAlongPath" || point,
        // Various Length always randomises.
        "randomizeLength" => MULTIPLE.contains(&kind) && kind != "multipleFlamesVariousLength",
        "interval" => MULTIPLE.contains(&kind) && !point,
        "angle" => kind == "multipleFlamesOneDirection",
        "color" => flag(v, "useCustomColor"),
        "flameShape" => !candle && !flag(v, "randomizeShapes"),
        // A candle burns at the path's start only.
        "end" => kind != "candleLight",
        "curve" => kind != "candleLight" && !point,
        _ => true,
    }
}

fn pan_row(key: &'static str, label: &'static str, default: [f64; 2]) -> Control {
    Control::Pan {
        key,
        label,
        min: -1.0,
        max: 1.0,
        default,
        step: 0.01,
        thumb: PanThumb::SelectionCrop,
    }
}

fn select(
    key: &'static str,
    label: &'static str,
    default: &'static str,
    choices: &'static [(&'static str, &'static str)],
) -> Control {
    Control::Select {
        key,
        label,
        default,
        choices,
    }
}

fn checkbox(key: &'static str, label: &'static str) -> Control {
    Control::Checkbox {
        key,
        label,
        default: false,
    }
}

pub fn effects() -> Vec<EffectDef> {
    let m = manifest("pcFlame").expect("manifest row");
    vec![EffectDef {
        id: m.id,
        icon: m.icon,
        category: m.category,
        controls: vec![
            Control::Value {
                key: "tab",
                default: || json!(0),
            },
            select(
                "flameType",
                "Flame Type",
                "oneFlameAlongPath",
                &[
                    ("oneFlameAlongPath", "One Flame Along Path"),
                    ("multipleFlamesAlongPath", "Multiple Flames Along Path"),
                    (
                        "multipleFlamesPathDirections",
                        "Multiple Flames Path Directions",
                    ),
                    (
                        "multipleFlamesVariousLength",
                        "Multiple Flames Various Length",
                    ),
                    ("candleLight", "Candle Light"),
                    (
                        "multipleFlamesOneDirection",
                        "Multiple Flames One Direction",
                    ),
                ],
            ),
            range("length", "Length", 1.0, 1000.0, 150.0),
            checkbox("randomizeLength", "Randomize Length"),
            range("width", "Width", 1.0, 1000.0, 40.0),
            range("angle", "Angle", -180.0, 180.0, 0.0),
            range("interval", "Interval", 1.0, 1000.0, 60.0),
            checkbox("useCustomColor", "Use Custom Color"),
            Control::Color {
                key: "color",
                label: "Color",
                default: ColorDefault::Primary,
            },
            select(
                "quality",
                "Quality",
                "medium",
                &[
                    ("draft", "Draft"),
                    ("low", "Low"),
                    ("medium", "Medium"),
                    ("high", "High"),
                    ("fine", "Fine"),
                ],
            ),
            range("turbulent", "Turbulent", 0.0, 100.0, 25.0),
            range("jag", "Jag", 0.0, 100.0, 25.0),
            range("opacity", "Opacity", 0.0, 100.0, 75.0),
            range("flameLines", "Flame Lines", 1.0, 100.0, 20.0),
            range(
                "flameBottomAlignment",
                "Flame Bottom Alignment",
                0.0,
                100.0,
                20.0,
            ),
            select(
                "flameStyle",
                "Flame Style",
                "normal",
                &[
                    ("normal", "Normal"),
                    ("violent", "Violent"),
                    ("flat", "Flat"),
                ],
            ),
            select(
                "flameShape",
                "Flame Shape",
                "parallel",
                &[
                    ("parallel", "Parallel"),
                    ("toCenter", "To Center"),
                    ("spread", "Spread"),
                    ("oval", "Oval"),
                    ("pointed", "Pointed"),
                ],
            ),
            checkbox("randomizeShapes", "Randomize Shapes"),
            // BB's reseed row: 0-255, folded with the instance seed.
            range("seed", "Randomize", 0.0, 255.0, 0.0),
            pan_row("start", "Start", [-0.7, 0.6]),
            pan_row("end", "End", [0.7, 0.6]),
            range("curve", "Curve", -100.0, 100.0, 20.0),
        ],
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: Some(&FLAME_UI),
    }]
}

/// Segments of the flattened path curve (well under half a pixel off the curve at 24 MP).
const CURVE_SEGMENTS: usize = 64;

/// The path in layer pixels: Start → End through the bowed control point (see the module doc).
fn path(v: &Values, b: Rect) -> Vec<(f32, f32)> {
    let at = |key| {
        let [x, y] = pan(v, key).map(|c| 0.5 + c.clamp(-1.0, 1.0) / 2.0);
        (
            b.x0 as f32 + (x * f64::from(b.width())) as f32,
            b.y0 as f32 + (y * f64::from(b.height())) as f32,
        )
    };
    let (s, e) = (at("start"), at("end"));
    let k = (num(v, "curve").clamp(-100.0, 100.0) / 100.0) as f32;
    let (dx, dy) = (e.0 - s.0, e.1 - s.1);
    let ctl = ((s.0 + e.0) / 2.0 + dy * k, (s.1 + e.1) / 2.0 - dx * k);
    (0..=CURVE_SEGMENTS)
        .map(|i| {
            let t = i as f32 / CURVE_SEGMENTS as f32;
            let (a, m, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            (a * s.0 + m * ctl.0 + c * e.0, a * s.1 + m * ctl.1 + c * e.1)
        })
        .collect()
}

fn spec(v: &Values, env: &EffectEnv) -> FlameSpec {
    let c = color_value(v, "color", env);
    FlameSpec {
        flame_type: match choice(v, "flameType") {
            "multipleFlamesAlongPath" => FlameType::MultipleFlamesAlongPath,
            "multipleFlamesPathDirections" => FlameType::MultipleFlamesPathDirections,
            "multipleFlamesVariousLength" => FlameType::MultipleFlamesVariousLength,
            "candleLight" => FlameType::CandleLight,
            "multipleFlamesOneDirection" => FlameType::MultipleFlamesOneDirection,
            _ => FlameType::OneFlameAlongPath,
        },
        length: num(v, "length") as f32,
        randomize_length: flag(v, "randomizeLength"),
        width: num(v, "width") as f32,
        angle: num(v, "angle").clamp(-180.0, 180.0) as f32,
        interval: num(v, "interval") as f32,
        color: flag(v, "useCustomColor")
            .then(|| [c[0], c[1], c[2], 255].map(|b| f32::from(b) / 255.0)),
        turbulent: num(v, "turbulent") as f32,
        jag: num(v, "jag") as f32,
        opacity: num(v, "opacity") as f32,
        flame_lines: num(v, "flameLines") as f32,
        flame_bottom_alignment: num(v, "flameBottomAlignment") as f32,
        flame_style: match choice(v, "flameStyle") {
            "violent" => FlameStyle::Violent,
            "flat" => FlameStyle::Flat,
            _ => FlameStyle::Normal,
        },
        flame_shape: match choice(v, "flameShape") {
            "toCenter" => FlameShape::ToCenter,
            "spread" => FlameShape::Spread,
            "oval" => FlameShape::Oval,
            "pointed" => FlameShape::Pointed,
            _ => FlameShape::Parallel,
        },
        randomize_shapes: flag(v, "randomizeShapes"),
        // The seed row folded with the instance seed, as Clouds does.
        seed: (num(v, "seed").clamp(0.0, 255.0) as u32 ^ env.instance_seed) & 255,
        quality: ["draft", "low", "medium", "high", "fine"]
            .iter()
            .position(|q| *q == choice(v, "quality"))
            .unwrap_or(2) as u32,
    }
}

fn render(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let b = env.bounds;
    let clip = Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32).intersect(&Rect::new(
        0,
        0,
        src.width as i32,
        src.height as i32,
    ));
    let prims = flame::flame(&spec(v, env), &[path(v, clip)]);
    let mut out = src.clone();
    match flame::composite(
        &mut out.data,
        src.width as usize,
        clip,
        &prims,
        Interrupt(env.cancel),
    ) {
        Some(()) => out,
        None => src.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::FLAME_UI;
    use crate::canvas::ImageData;
    use crate::effect::bb::test_util::{rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px, run, run_env, solid};
    use crate::effect::{EffectEnv, defaults, find};
    use serde_json::{Value, json};
    use std::sync::atomic::AtomicBool;

    fn noise(x: u32, y: u32) -> [u8; 4] {
        let mut s = u64::from(y * 97 + x);
        let b = crate::extra::rng::splitmix64(&mut s).to_le_bytes();
        [b[0], b[1], b[2], 255]
    }

    /// A layer pixel in Pan coordinates (-1..1 across a `w × h` layer with no selection).
    fn at(x: f64, y: f64, w: f64, h: f64) -> Value {
        json!([x / w * 2.0 - 1.0, y / h * 2.0 - 1.0])
    }

    /// The pixels drawn on a transparent layer (alpha over upstream's 0.05).
    fn drawn(img: &ImageData) -> Vec<(u32, u32)> {
        (0..img.height)
            .flat_map(|y| (0..img.width).map(move |x| (x, y)))
            .filter(|&(x, y)| f32::from(px(img, x, y)[3]) / 255.0 > 0.05)
            .collect()
    }

    /// Upstream engine `each_renderer_draws_undoes_and_is_seeded_at_every_depth` (Flame, 8-bit):
    /// the defaults draw over 50 pixels of a 160 × 120 transparent layer, the same values draw the
    /// same, and another seed draws differently. The Randomize row folds with the instance seed:
    /// row 0 under instance seed 7 is row 7 under instance seed 0.
    #[test]
    fn flame_is_deterministic_with_seed() {
        let src = solid(160, 120, [0, 0, 0, 0]);
        let first = run("pcFlame", json!({}), &src);
        let n = drawn(&first).len();
        assert!(n > 50, "drew {n} px");
        assert_eq!(run("pcFlame", json!({}), &src).data, first.data);
        assert_ne!(run("pcFlame", json!({"seed": 5}), &src).data, first.data);
        let mut env = EffectEnv::whole(160, 120);
        env.instance_seed = 7;
        assert_eq!(
            run_env("pcFlame", json!({"seed": 0}), &src, &env).data,
            run("pcFlame", json!({"seed": 7}), &src).data
        );
        assert_ne!(
            run_env("pcFlame", json!({"seed": 0}), &src, &env).data,
            first.data
        );
    }

    /// Upstream engine `flame_follows_the_work_path`: a vertical path at x = 20 of a 160 × 120
    /// layer (width 16, length 60) draws only near it, from its base to well up its length.
    /// Curve bows the path to the chord's left (up for a left-to-right path), and a moved path
    /// moves the flame.
    #[test]
    fn flame_draws_along_the_path() {
        let src = solid(160, 120, [0, 0, 0, 0]);
        let (w, h) = (160.0, 120.0);
        let v = json!({"start": at(20.0, 110.0, w, h), "end": at(20.0, 20.0, w, h), "curve": 0,
            "width": 16, "length": 60});
        let pts = drawn(&run("pcFlame", v, &src));
        let x0 = pts.iter().map(|p| p.0).min().unwrap();
        let x1 = pts.iter().map(|p| p.0).max().unwrap() + 1;
        assert!(
            x0 < 20 && x1 < 60,
            "drawn x {x0}..{x1} hugs the path at x=20"
        );
        assert!(pts.iter().any(|p| p.1 > 100) && pts.iter().any(|p| p.1 < 50));
        // A horizontal path across the middle, bowed either way: the drawing in the middle
        // column sits above the chord for a positive curve and below it for a negative one.
        let mean_y = |curve: i32| {
            let v = json!({"start": at(20.0, 60.0, w, h), "end": at(140.0, 60.0, w, h),
                "curve": curve, "width": 16});
            let pts = drawn(&run("pcFlame", v, &src));
            let col: Vec<u32> = pts.iter().filter(|p| p.0 == 80).map(|p| p.1).collect();
            assert!(!col.is_empty(), "curve {curve}");
            col.iter().sum::<u32>() as f32 / col.len() as f32
        };
        let (up, flat, down) = (mean_y(60), mean_y(0), mean_y(-60));
        assert!(
            up < 40.0 && (50.0..70.0).contains(&flat) && down > 80.0,
            "{up} {flat} {down}"
        );
    }

    /// Everything anchors on the selection bounds: inside a selection moved diagonally from the
    /// origin, the result equals Flame run on the layer cropped to it, so a path anchored on the
    /// layer origin fails in x and in y. (±1: the moved path's f32 coordinates round differently.)
    #[test]
    fn flame_anchors_on_the_selection_bounds() {
        let (w, h, l, t, r, b) = (96u32, 80u32, 24u32, 16u32, 88u32, 72u32);
        let full = from_fn(w, h, noise);
        let crop = from_fn(r - l, b - t, |x, y| noise(x + l, y + t));
        let (mask, polys) = rect_selection(w, h, l, t, r, b);
        let env = with_selection(w, h, &mask, &polys);
        for v in [
            json!({}),
            json!({"flameType": "multipleFlamesAlongPath", "length": 30, "width": 12,
                "interval": 10, "start": [-0.8, 0.5], "end": [0.6, 0.2], "curve": -40}),
            json!({"flameType": "candleLight", "length": 40, "start": [0.2, 0.7]}),
        ] {
            let a = run_env("pcFlame", v.clone(), &full, &env);
            let c = run("pcFlame", v.clone(), &crop);
            assert_ne!(c.data, crop.data, "{v}");
            for (x, y) in (t..b).flat_map(|y| (l..r).map(move |x| (x, y))) {
                let (p, q) = (px(&a, x, y), px(&c, x - l, y - t));
                // ±1: the path moved by (l, t) has f32 coordinates that round differently from
                // the crop's, so coverage can differ in the last bit and the byte by one.
                assert!(
                    (0..4).all(|i| p[i].abs_diff(q[i]) <= 1),
                    "{v} at {x},{y}: {p:?} {q:?}"
                );
            }
            // Outside the selection the layer is untouched.
            for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
                if !(l..r).contains(&x) || !(t..b).contains(&y) {
                    assert_eq!(px(&a, x, y), px(&full, x, y), "{v} at {x},{y}");
                }
            }
        }
    }

    /// Flame draws source-over: an opaque layer stays opaque everywhere, its edges included,
    /// and the natural colours are warm (red over blue).
    #[test]
    fn flame_keeps_an_opaque_layer_opaque() {
        let src = solid(64, 48, [0, 0, 0, 255]);
        let v = json!({"start": [-1.0, 0.8], "end": [1.0, 0.8], "width": 30});
        let out = run("pcFlame", v, &src);
        assert!(out.data.chunks(4).all(|p| p[3] == 255));
        assert!(out.data.chunks(4).any(|p| p[0] > 100));
        assert!(out.data.chunks(4).all(|p| p[0] >= p[2]));
    }

    /// The custom colour defaults to the primary colour (not black), and an explicit colour
    /// wins; without "Use Custom Color" the natural colours are drawn whatever the colour row.
    #[test]
    fn flame_custom_color_defaults_to_primary() {
        let src = solid(64, 48, [0, 0, 0, 0]);
        let mut env = EffectEnv::whole(64, 48);
        env.primary = [0, 0, 255, 255];
        env.secondary = [255, 255, 0, 255];
        let blue = run_env("pcFlame", json!({"useCustomColor": true}), &src, &env);
        // Above alpha 64 the strands dominate; a glow's faint rim alone is redder (upstream).
        let lit: Vec<&[u8]> = blue.data.chunks(4).filter(|p| p[3] > 64).collect();
        assert!(!lit.is_empty() && lit.iter().all(|p| p[2] > p[0] && p[2] > p[1]));
        let green = json!({"useCustomColor": true, "color": "#00ff00"});
        let green = run_env("pcFlame", green, &src, &env);
        assert!(
            green
                .data
                .chunks(4)
                .filter(|p| p[3] > 64)
                .all(|p| p[1] > p[0] && p[1] > p[2])
        );
        let natural = run_env("pcFlame", json!({}), &src, &env);
        assert_eq!(natural.data, run("pcFlame", json!({}), &src).data);
        assert!(
            natural
                .data
                .chunks(4)
                .filter(|p| p[3] > 12)
                .all(|p| p[0] >= p[2])
        );
    }

    /// No dead controls: every row changes the picture whenever it is enabled, and a row the
    /// enable rules grey out changes nothing.
    #[test]
    fn flame_controls_matter_exactly_when_enabled() {
        let src = solid(96, 72, [0, 0, 0, 0]);
        let multi = json!({"flameType": "multipleFlamesAlongPath", "length": 40, "width": 20});
        let one_dir = json!({"flameType": "multipleFlamesOneDirection", "length": 40});
        let candle = json!({"flameType": "candleLight", "length": 40});
        let various = json!({"flameType": "multipleFlamesVariousLength", "length": 40});
        let custom = json!({"useCustomColor": true});
        // Start = End: a zero-length path.
        let point = json!({"start": [0.0, 0.5], "end": [0.0, 0.5]});
        let point_dir = json!({"flameType": "multipleFlamesOneDirection", "length": 40,
            "start": [0.0, 0.5], "end": [0.0, 0.5]});
        let cases = [
            (json!({}), "flameType", json!("multipleFlamesAlongPath")),
            (json!({}), "flameType", json!("candleLight")),
            (
                multi.clone(),
                "flameType",
                json!("multipleFlamesPathDirections"),
            ),
            (
                multi.clone(),
                "flameType",
                json!("multipleFlamesVariousLength"),
            ),
            (
                multi.clone(),
                "flameType",
                json!("multipleFlamesOneDirection"),
            ),
            (multi.clone(), "length", json!(25)),
            (candle.clone(), "length", json!(25)),
            (multi.clone(), "randomizeLength", json!(true)),
            (json!({}), "width", json!(10)),
            (one_dir.clone(), "angle", json!(60)),
            (multi.clone(), "interval", json!(25)),
            (json!({}), "useCustomColor", json!(true)),
            (custom.clone(), "color", json!("#00ff00")),
            (json!({}), "quality", json!("fine")),
            (json!({}), "turbulent", json!(90)),
            (json!({}), "jag", json!(90)),
            (json!({}), "opacity", json!(30)),
            (json!({}), "flameLines", json!(60)),
            (json!({}), "flameBottomAlignment", json!(90)),
            (json!({}), "flameStyle", json!("violent")),
            (json!({}), "flameStyle", json!("flat")),
            (json!({}), "flameShape", json!("pointed")),
            (json!({}), "randomizeShapes", json!(true)),
            (json!({}), "seed", json!(9)),
            (json!({}), "start", json!([-0.2, 0.2])),
            (json!({}), "end", json!([0.2, 0.0])),
            (json!({}), "curve", json!(-50)),
            (candle.clone(), "start", json!([0.3, 0.5])),
            (various.clone(), "length", json!(25)),
            (
                json!({"randomizeShapes": true}),
                "randomizeShapes",
                json!(false),
            ),
            // One Flame Along Path's candle fallback reads Length.
            (point.clone(), "length", json!(25)),
            (point.clone(), "width", json!(10)),
            (point.clone(), "end", json!([0.3, 0.5])),
            (point_dir.clone(), "angle", json!(60)),
            (point_dir.clone(), "randomizeLength", json!(true)),
            // Greyed rows.
            (json!({}), "length", json!(25)),
            (json!({}), "randomizeLength", json!(true)),
            (json!({}), "interval", json!(25)),
            (json!({}), "angle", json!(60)),
            (multi.clone(), "angle", json!(60)),
            (json!({}), "color", json!("#00ff00")),
            (candle.clone(), "end", json!([0.2, 0.0])),
            (candle.clone(), "curve", json!(-50)),
            (candle.clone(), "interval", json!(25)),
            (various.clone(), "randomizeLength", json!(true)),
            (
                json!({"randomizeShapes": true}),
                "flameShape",
                json!("pointed"),
            ),
            (candle.clone(), "flameShape", json!("pointed")),
            (point.clone(), "curve", json!(-50)),
            (point.clone(), "flameShape", json!("pointed")),
            (point_dir.clone(), "interval", json!(25)),
            (point_dir.clone(), "curve", json!(-50)),
        ];
        let def = find("pcFlame").unwrap();
        for (base, key, value) in cases {
            let mut v = defaults(&def);
            v.extend(base.as_object().unwrap().clone());
            let before = crate::effect::apply(&src, &def, &v, &EffectEnv::whole(96, 72));
            let on = (FLAME_UI.enabled)(&v, key);
            v.insert(key.into(), value.clone());
            let after = crate::effect::apply(&src, &def, &v, &EffectEnv::whole(96, 72));
            assert_eq!(
                after.data != before.data,
                on,
                "{base} {key}={value} (enabled {on})"
            );
        }
    }

    /// A cancelled render leaves the layer untouched.
    #[test]
    fn flame_stops_when_cancelled() {
        let src = from_fn(32, 32, noise);
        let flag = AtomicBool::new(true);
        let mut env = EffectEnv::whole(32, 32);
        env.cancel = Some(&flag);
        assert_eq!(run_env("pcFlame", json!({}), &src, &env).data, src.data);
    }

    /// Upstream `bench_24mp` (Flame): `cargo test -p pdn-core --release flame::tests::bench_24mp
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_24mp() {
        let src = solid(6000, 4000, [0, 0, 0, 0]);
        for v in [
            json!({"flameType": "multipleFlamesAlongPath", "length": 600, "width": 160,
                "interval": 120}),
            json!({"length": 1500, "width": 400, "flameLines": 100, "quality": "fine"}),
            json!({"length": 1000, "width": 1000, "flameLines": 100, "quality": "fine",
                "start": [-1.0, 1.0], "end": [1.0, -1.0], "curve": 100}),
        ] {
            let t = std::time::Instant::now();
            let out = run("pcFlame", v.clone(), &src);
            println!("{v}: {:?} ({} px)", t.elapsed(), drawn(&out).len());
        }
    }
}
