//! Photocraft phase spec: S§4.2; research R§B (Distort)
//!
//! The Extra Distort effects: Offset, Polar Coordinates, Wave, Ripple and ZigZag
//! (`extra::fx::{other, distort, distort2}`). All anchor on the selection bounds, as upstream
//! anchors on `ctx.bounds`, and read the whole layer.

use crate::canvas::ImageData;
use crate::effect::{
    Category, Control, EffectDef, EffectEnv, EffectKind, Values, choice, num, range,
};
use crate::extra::fx::distort::{Edge, PolarMode, RippleSize, WaveSpec, WaveType};
use crate::extra::fx::distort2::ZigZagStyle;
use crate::extra::geom::Rect;
use crate::extra::{fx, interrupt::Interrupt, shim};

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

pub fn effects() -> Vec<EffectDef> {
    let distort = |id: &'static str, controls: Vec<Control>, render: RenderFn| {
        let m = super::manifest::manifest(id).expect("manifest row");
        EffectDef {
            id,
            icon: m.icon,
            category: Category::Effect("distort"),
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(m.name),
            indirect: None,
        }
    };
    let select = |key, label, default, choices| Control::Select {
        key,
        label,
        default,
        choices,
    };
    const WRAP_REPEAT: &[(&str, &str)] = &[("wrap", "Wrap"), ("repeat", "Repeat Edge")];
    vec![
        distort(
            "pcOffset",
            vec![
                range("horizontal", "Horizontal", -65535.0, 65535.0, 0.0),
                range("vertical", "Vertical", -65535.0, 65535.0, 0.0),
                select(
                    "undefined",
                    "Undefined Areas",
                    "wrap",
                    &[
                        ("wrap", "Wrap"),
                        ("repeat", "Repeat Edge"),
                        ("transparent", "Transparent"),
                    ],
                ),
            ],
            offset,
        ),
        distort(
            "pcPolarCoordinates",
            vec![select(
                "mode",
                "Conversion",
                "rectangularToPolar",
                &[
                    ("rectangularToPolar", "Rectangular to Polar"),
                    ("polarToRectangular", "Polar to Rectangular"),
                ],
            )],
            polar,
        ),
        distort(
            "pcWave",
            vec![
                range("generators", "Number of Generators", 1.0, 999.0, 5.0),
                range("wavelengthMin", "Minimum Wavelength", 1.0, 998.0, 10.0),
                range("wavelengthMax", "Maximum Wavelength", 2.0, 999.0, 120.0),
                range("amplitudeMin", "Minimum Amplitude", 1.0, 998.0, 5.0),
                range("amplitudeMax", "Maximum Amplitude", 1.0, 999.0, 35.0),
                select(
                    "type",
                    "Type",
                    "sine",
                    &[
                        ("sine", "Sine"),
                        ("triangle", "Triangle"),
                        ("square", "Square"),
                    ],
                ),
                select("undefined", "Undefined Areas", "wrap", WRAP_REPEAT),
                // BB's reseed row: 0-255, folded with the instance seed.
                range("seed", "Randomize", 0.0, 255.0, 0.0),
            ],
            wave,
        ),
        distort(
            "pcRipple",
            vec![
                range("amount", "Amount", -999.0, 999.0, 100.0),
                select(
                    "size",
                    "Size",
                    "medium",
                    &[("small", "Small"), ("medium", "Medium"), ("large", "Large")],
                ),
            ],
            ripple,
        ),
        distort(
            "pcZigZag",
            vec![
                range("amount", "Amount", -100.0, 100.0, 10.0),
                range("ridges", "Ridges", 0.0, 20.0, 5.0),
                select(
                    "style",
                    "Style",
                    "pondRipples",
                    &[
                        ("pondRipples", "Pond Ripples"),
                        ("outFromCenter", "Out from Center"),
                        ("aroundCenter", "Around Center"),
                    ],
                ),
            ],
            zigzag,
        ),
    ]
}

/// The selection bounds ∩ the layer, as upstream's `ctx.bounds`.
fn bounds(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

/// The seed row folded with the instance seed, as Clouds does.
fn seed(v: &Values, env: &EffectEnv) -> u32 {
    (num(v, "seed").clamp(0.0, 255.0) as u32 ^ env.instance_seed) & 255
}

fn edge(v: &Values) -> Edge {
    match choice(v, "undefined") {
        "repeat" => Edge::Repeat,
        "transparent" => Edge::Transparent,
        _ => Edge::Wrap,
    }
}

/// A kernel's result as a layer; a cancelled render leaves the source.
fn finish(src: &ImageData, out: Option<Vec<f32>>) -> ImageData {
    out.map_or_else(
        || src.clone(),
        |buf| shim::from_f32(&buf, src.width, src.height),
    )
}

fn offset(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = fx::other::offset(
        &shim::to_f32(src),
        src.width as usize,
        bounds(env),
        num(v, "horizontal") as i32,
        num(v, "vertical") as i32,
        edge(v),
    );
    shim::from_f32(&out, src.width, src.height)
}

fn polar(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mode = if choice(v, "mode") == "polarToRectangular" {
        PolarMode::PolarToRectangular
    } else {
        PolarMode::RectangularToPolar
    };
    let out = fx::distort::polar(
        &shim::to_f32(src),
        src.width as usize,
        bounds(env),
        mode,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn wave(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let spec = WaveSpec {
        generators: num(v, "generators").clamp(1.0, 999.0) as u32,
        wavelength: (
            num(v, "wavelengthMin") as f32,
            num(v, "wavelengthMax") as f32,
        ),
        amplitude: (num(v, "amplitudeMin") as f32, num(v, "amplitudeMax") as f32),
        wave_type: match choice(v, "type") {
            "triangle" => WaveType::Triangle,
            "square" => WaveType::Square,
            _ => WaveType::Sine,
        },
        edge: edge(v),
        seed: seed(v, env),
    };
    let out = fx::distort::wave(
        &shim::to_f32(src),
        src.width as usize,
        bounds(env),
        spec,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn ripple(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let size = match choice(v, "size") {
        "small" => RippleSize::Small,
        "large" => RippleSize::Large,
        _ => RippleSize::Medium,
    };
    let out = fx::distort::ripple(
        &shim::to_f32(src),
        src.width as usize,
        bounds(env),
        num(v, "amount") as f32,
        size,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn zigzag(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let style = match choice(v, "style") {
        "outFromCenter" => ZigZagStyle::OutFromCenter,
        "aroundCenter" => ZigZagStyle::AroundCenter,
        _ => ZigZagStyle::PondRipples,
    };
    let out = fx::distort2::zigzag(
        &shim::to_f32(src),
        src.width as usize,
        bounds(env),
        num(v, "amount") as f32,
        num(v, "ridges") as f32,
        style,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use serde_json::json;

    fn noise(x: u32, y: u32) -> [u8; 4] {
        let mut st = u64::from(y * 97 + x);
        let v = crate::extra::rng::splitmix64(&mut st).to_le_bytes();
        [v[0], v[1], v[2], 255]
    }

    /// (+3, 0) with Wrap moves pixel x to (x + 3) mod w, bit for bit.
    #[test]
    fn offset_wraps_exactly() {
        let src = from_fn(10, 6, noise);
        let out = run("pcOffset", json!({"horizontal": 3}), &src);
        for (x, y) in (0..6).flat_map(|y| (0..10).map(move |x| (x, y))) {
            assert_eq!(px(&out, (x + 3) % 10, y), px(&src, x, y), "{x},{y}");
        }
        // Repeat Edge and Transparent fill the vacated columns instead.
        let rep = run(
            "pcOffset",
            json!({"horizontal": 3, "undefined": "repeat"}),
            &src,
        );
        assert_eq!(px(&rep, 0, 2), px(&src, 0, 2));
        let clear = run(
            "pcOffset",
            json!({"horizontal": 3, "undefined": "transparent"}),
            &src,
        );
        assert_eq!(px(&clear, 1, 2), [0, 0, 0, 0]);
        assert_eq!(px(&clear, 5, 2), px(&src, 2, 2));
    }

    /// R46: the ranges reach Paint.NET's maximum image dimension, and a full-width Wrap on a
    /// wide layer is the identity.
    #[test]
    fn offset_ranges_reach_65535_and_a_full_wrap_is_identity() {
        let def = crate::effect::find("pcOffset").unwrap();
        for key in ["horizontal", "vertical"] {
            let c = def.controls.iter().find(|c| c.key() == key).unwrap();
            let crate::effect::Control::Range { min, max, .. } = c else {
                panic!("{key} is not a range")
            };
            assert_eq!((*min, *max), (-65535.0, 65535.0), "{key}");
        }
        let src = from_fn(300, 2, noise);
        for dx in [300, -300, 65535 - 65535 % 300] {
            let out = run("pcOffset", json!({"horizontal": dx}), &src);
            assert_eq!(out.data, src.data, "dx {dx}");
        }
        let out = run("pcOffset", json!({"vertical": 65535}), &src);
        assert_eq!(px(&out, 0, 0), px(&src, 0, (65535 % 2) as u32));
    }

    /// Rectangular to Polar and back on a smooth gradient stays within 6 levels inside the
    /// inscribed circle, bar its outermost ring: the last 4 px lose up to 24 levels where the
    /// map runs out of source.
    #[test]
    fn polar_round_trip_is_close() {
        let n = 64u32;
        let src = from_fn(n, n, |x, y| [(x * 4) as u8, (y * 4) as u8, 128, 255]);
        let there = run("pcPolarCoordinates", json!({}), &src);
        assert_ne!(there.data, src.data);
        let back = run(
            "pcPolarCoordinates",
            json!({"mode": "polarToRectangular"}),
            &there,
        );
        let mut checked = 0;
        for (x, y) in (0..n).flat_map(|y| (0..n).map(move |x| (x, y))) {
            let (dx, dy) = (x as f32 + 0.5 - 32.0, y as f32 + 0.5 - 32.0);
            if (dx * dx + dy * dy).sqrt() < 28.0 {
                checked += 1;
                for c in 0..3 {
                    let (a, b) = (px(&back, x, y)[c], px(&src, x, y)[c]);
                    assert!(a.abs_diff(b) <= 6, "{x},{y} c{c}: {a} vs {b}");
                }
            }
        }
        assert!(checked > 2000);
    }

    /// The default Wave moves pixels; the same seed reproduces it, another seed does not.
    #[test]
    fn wave_is_seeded() {
        let src = from_fn(40, 40, noise);
        let go = |seed: u32| run("pcWave", json!({"seed": seed}), &src);
        assert_ne!(go(1).data, src.data);
        assert_eq!(go(1).data, go(1).data);
        assert_ne!(go(1).data, go(2).data);
    }

    #[test]
    fn ripple_zero_is_identity() {
        let src = from_fn(24, 24, noise);
        for size in ["small", "medium", "large"] {
            let out = run("pcRipple", json!({"amount": 0, "size": size}), &src);
            assert_eq!(out.data, src.data, "{size}");
        }
        assert_ne!(run("pcRipple", json!({}), &src).data, src.data);
    }

    #[test]
    fn zigzag_zero_is_identity() {
        let src = from_fn(24, 24, noise);
        for style in ["pondRipples", "outFromCenter", "aroundCenter"] {
            let out = run("pcZigZag", json!({"amount": 0, "style": style}), &src);
            assert_eq!(out.data, src.data, "{style}");
            let moved = run("pcZigZag", json!({"amount": 80, "style": style}), &src);
            assert_ne!(moved.data, src.data, "{style}");
        }
    }

    /// Centres, wrap and ripple phase anchor on the selection bounds: inside a rectangular
    /// selection the result equals the effect run on the layer cropped to that rectangle.
    #[test]
    fn distort_effects_anchor_on_selection_bounds() {
        use crate::effect::bb::test_util::{rect_selection, with_selection};
        let (w, h, l, t, r, b) = (48u32, 36u32, 8u32, 4u32, 32u32, 20u32);
        let full = from_fn(w, h, noise);
        let crop = from_fn(r - l, b - t, |x, y| noise(x + l, y + t));
        let (mask, polys) = rect_selection(w, h, l, t, r, b);
        let env = with_selection(w, h, &mask, &polys);
        let cases = [
            ("pcOffset", json!({"horizontal": 5, "vertical": -3})),
            ("pcPolarCoordinates", json!({})),
            ("pcPolarCoordinates", json!({"mode": "polarToRectangular"})),
            ("pcWave", json!({"seed": 9})),
            ("pcRipple", json!({})),
            ("pcZigZag", json!({"amount": 60})),
        ];
        for (id, v) in cases {
            // Polar and ZigZag go through trigonometry, so the cropped layer's different
            // coordinates round one level apart at most.
            let tol = if id == "pcPolarCoordinates" || id == "pcZigZag" {
                1
            } else {
                0
            };
            let a = run_env(id, v.clone(), &full, &env);
            let c = run(id, v.clone(), &crop);
            for (x, y) in (t..b).flat_map(|y| (l..r).map(move |x| (x, y))) {
                let (p, q) = (px(&a, x, y), px(&c, x - l, y - t));
                assert!(
                    (0..4).all(|i| p[i].abs_diff(q[i]) <= tol),
                    "{id} {v} at {x},{y}: {p:?} {q:?}"
                );
            }
        }
    }
}
