//! Photocraft phase spec: S§4.2; research R§B (Pixelate)
//!
//! The Extra Distort effects Color Halftone, Pointillize and Mezzotint, and Artistic's Facet
//! (`extra::fx::pixelate`). There is no Halftone submenu (user decision). The grid and screen
//! origin is the selection bounds; the kernels read the whole layer.

use crate::canvas::ImageData;
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, choice, color_value,
    num, range,
};
use crate::extra::fx::pixelate::{self, MezzotintType};
use crate::extra::geom::Rect;
use crate::extra::{interrupt::Interrupt, shim};

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

pub fn effects() -> Vec<EffectDef> {
    let def =
        |id: &'static str, category: &'static str, controls: Vec<Control>, render: RenderFn| {
            let m = super::manifest::manifest(id).expect("manifest row");
            EffectDef {
                id,
                icon: m.icon,
                category: Category::Effect(category),
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
    // BB's reseed row: 0-255, folded with the instance seed.
    let seed = || range("seed", "Randomize", 0.0, 255.0, 0.0);
    let angle = |key, label, default| range(key, label, -360.0, 360.0, default);
    // Keys keep upstream's channel numbers; Paint.NET documents are RGB, so there are three.
    let halftone_angles = vec![
        angle("channel1", "Red angle", 108.0),
        angle("channel2", "Green angle", 162.0),
        angle("channel3", "Blue angle", 90.0),
    ];
    let mut halftone_controls = vec![range("maxRadius", "Max Radius", 4.0, 127.0, 8.0)];
    halftone_controls.extend(halftone_angles);
    vec![
        def(
            "pcColorHalftone",
            "distort",
            halftone_controls,
            color_halftone,
        ),
        def(
            "pcPointillize",
            "distort",
            vec![
                range("cellSize", "Cell Size", 3.0, 300.0, 5.0),
                Control::Color {
                    key: "background",
                    label: "Background",
                    default: ColorDefault::Secondary,
                },
                seed(),
            ],
            pointillize,
        ),
        def(
            "pcMezzotint",
            "distort",
            vec![
                Control::Select {
                    key: "type",
                    label: "Type",
                    default: "fineDots",
                    choices: &[
                        ("fineDots", "Fine Dots"),
                        ("mediumDots", "Medium Dots"),
                        ("grainyDots", "Grainy Dots"),
                        ("coarseDots", "Coarse Dots"),
                        ("shortLines", "Short Lines"),
                        ("mediumLines", "Medium Lines"),
                        ("longLines", "Long Lines"),
                        ("shortStrokes", "Short Strokes"),
                        ("mediumStrokes", "Medium Strokes"),
                        ("longStrokes", "Long Strokes"),
                    ],
                },
                seed(),
            ],
            mezzotint,
        ),
        def("pcFacet", "artistic", vec![], facet),
    ]
}

fn bounds(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

/// The seed row folded with the instance seed, as Clouds does.
fn seed(v: &Values, env: &EffectEnv) -> u32 {
    (num(v, "seed").clamp(0.0, 255.0) as u32 ^ env.instance_seed) & 255
}

/// A kernel's result as a layer; a cancelled render leaves the source.
fn finish(src: &ImageData, out: Option<Vec<f32>>) -> ImageData {
    out.map_or_else(
        || src.clone(),
        |buf| shim::from_f32(&buf, src.width, src.height),
    )
}

fn color_halftone(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = pixelate::color_halftone(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        num(v, "maxRadius").clamp(4.0, 127.0) as f32,
        ["channel1", "channel2", "channel3"].map(|k| num(v, k) as f32),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn pointillize(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let background = color_value(v, "background", env).map(|b| f32::from(b) / 255.0);
    let out = pixelate::pointillize(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        num(v, "cellSize").clamp(3.0, 300.0) as f32,
        seed(v, env),
        background,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn mezzotint(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let kind = match choice(v, "type") {
        "mediumDots" => MezzotintType::MediumDots,
        "grainyDots" => MezzotintType::GrainyDots,
        "coarseDots" => MezzotintType::CoarseDots,
        "shortLines" => MezzotintType::ShortLines,
        "mediumLines" => MezzotintType::MediumLines,
        "longLines" => MezzotintType::LongLines,
        "shortStrokes" => MezzotintType::ShortStrokes,
        "mediumStrokes" => MezzotintType::MediumStrokes,
        "longStrokes" => MezzotintType::LongStrokes,
        _ => MezzotintType::FineDots,
    };
    let out = pixelate::mezzotint(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        kind,
        seed(v, env),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn facet(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
    let out = pixelate::facet(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run};
    use crate::effect::{Category, find, menu_categories};
    use serde_json::json;

    /// The three halftone-family items are Distort members; no Halftone submenu exists.
    #[test]
    fn halftone_items_are_listed_under_distort() {
        let cats = menu_categories(true, true);
        assert!(cats.iter().all(|(k, _)| *k != "halftone"));
        let (_, distort) = cats.iter().find(|(k, _)| *k == "distort").unwrap();
        for id in ["pcColorHalftone", "pcPointillize", "pcMezzotint"] {
            assert!(distort.contains(&id), "{id}");
            assert_eq!(find(id).unwrap().category, Category::Effect("distort"));
        }
        let (_, artistic) = cats.iter().find(|(k, _)| *k == "artistic").unwrap();
        assert!(artistic.contains(&"pcFacet"));
    }

    #[test]
    fn color_halftone_white_stays_white() {
        let src = from_fn(24, 24, |_, _| [255, 255, 255, 255]);
        assert_eq!(run("pcColorHalftone", json!({}), &src).data, src.data);
    }

    /// Pointillize paints the Secondary colour (white in `EffectEnv::whole`) between dots, and
    /// the same seed gives the same picture.
    #[test]
    fn pointillize_is_deterministic_with_seed() {
        let src = from_fn(32, 32, |x, y| [(x * 8) as u8, (y * 8) as u8, 40, 255]);
        let a = run("pcPointillize", json!({"seed": 7}), &src);
        assert_eq!(a.data, run("pcPointillize", json!({"seed": 7}), &src).data);
        assert_ne!(a.data, run("pcPointillize", json!({"seed": 8}), &src).data);
        let black = from_fn(32, 32, |_, _| [0, 0, 0, 255]);
        let out = run("pcPointillize", json!({"cellSize": 8}), &black);
        assert!((0..32 * 32).any(|i| px(&out, i % 32, i / 32) == [255, 255, 255, 255]));
    }

    /// The background row defaults to Secondary: with a non-white Secondary the canvas between
    /// dots takes that colour, and an explicit colour overrides it.
    #[test]
    fn pointillize_background_is_secondary() {
        let src = from_fn(32, 32, |_, _| [0, 0, 0, 255]);
        let mut env = crate::effect::EffectEnv::whole(32, 32);
        env.secondary = [200, 10, 30, 255];
        let out =
            crate::effect::test_util::run_env("pcPointillize", json!({"cellSize": 8}), &src, &env);
        assert!((0..32 * 32).any(|i| px(&out, i % 32, i / 32) == [200, 10, 30, 255]));
        let out = crate::effect::test_util::run_env(
            "pcPointillize",
            json!({"cellSize": 8, "background": "#00ff00"}),
            &src,
            &env,
        );
        assert!((0..32 * 32).any(|i| px(&out, i % 32, i / 32) == [0, 255, 0, 255]));
    }

    #[test]
    fn mezzotint_outputs_channel_extremes() {
        let src = from_fn(24, 24, |x, y| [(x * 10) as u8, (y * 10) as u8, 128, 200]);
        for kind in ["fineDots", "mediumDots", "grainyDots", "coarseDots"] {
            let out = run("pcMezzotint", json!({"type": kind}), &src);
            for p in out.data.as_chunks::<4>().0.iter() {
                assert!(p[..3].iter().all(|&c| c == 0 || c == 255), "{kind}");
                assert_eq!(p[3], 200);
            }
        }
    }

    #[test]
    fn facet_flat_is_identity() {
        let src = from_fn(16, 16, |_, _| [100, 150, 200, 255]);
        assert_eq!(run("pcFacet", json!({}), &src).data, src.data);
    }
}
