//! Photocraft phase spec: S§4.2; research R§B (Stylize)
//!
//! The Extra Stylize effects: Solarize, High Pass, Wind, Trace Contour, Tiles and Extrude
//! (`extra::fx::{stylize, sharpen, stylize2}`).

use crate::canvas::ImageData;
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, choice, color_value,
    flag, num, range,
};
use crate::extra::fx::stylize2::{ExtrudeSpec, ExtrudeType, TileFill, WindMethod};
use crate::extra::geom::Rect;
use crate::extra::{fx, interrupt::Interrupt, shim};

pub fn effects() -> Vec<EffectDef> {
    let mut v = vec![EffectDef {
        id: "pcSolarize",
        icon: "solarize_effect.png",
        category: Category::Effect("stylize"),
        controls: vec![],
        kind: EffectKind::Cpu {
            render: solarize,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: None,
        indirect: None,
    }];
    v.push(EffectDef {
        id: "pcHighPass",
        icon: "high_pass_effect.png",
        category: Category::Effect("stylize"),
        controls: vec![range("radius", "Radius", 0.1, 1000.0, 10.0).slider_keys(1, 0.1, 1.0, true)],
        kind: EffectKind::Cpu {
            render: high_pass,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some("High Pass"),
        indirect: None,
    });
    let stylize = |id: &'static str, controls: Vec<Control>, render: RenderFn| {
        let m = super::manifest::manifest(id).expect("manifest row");
        EffectDef {
            id,
            icon: m.icon,
            category: Category::Effect("stylize"),
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
    v.push(stylize(
        "pcWind",
        vec![
            Control::Select {
                key: "method",
                label: "Method",
                default: "wind",
                choices: &[("wind", "Wind"), ("blast", "Blast"), ("stagger", "Stagger")],
            },
            // Keys keep their first meaning (the way the streaks run); the labels are upstream's
            // and Photoshop's: "From the Right" (default) streaks toward -x.
            Control::Select {
                key: "direction",
                label: "Direction",
                default: "left",
                choices: &[("left", "From the Right"), ("right", "From the Left")],
            },
            seed(),
        ],
        wind,
    ));
    v.push(stylize(
        "pcTraceContour",
        vec![
            range("level", "Level", 0.0, 255.0, 128.0),
            Control::Select {
                key: "edge",
                label: "Edge",
                default: "lower",
                choices: &[("lower", "Lower"), ("upper", "Upper")],
            },
        ],
        trace_contour,
    ));
    v.push(stylize(
        "pcTiles",
        vec![
            range("count", "Number of Tiles", 1.0, 99.0, 10.0),
            range("maxOffset", "Max Offset", 1.0, 99.0, 10.0),
            Control::Select {
                key: "fill",
                label: "Fill",
                default: "background",
                choices: &[
                    ("background", "Background"),
                    ("foreground", "Foreground"),
                    ("inverse", "Inverse"),
                    ("unaltered", "Unaltered"),
                ],
            },
            Control::Color {
                key: "foreground",
                label: "Foreground",
                default: ColorDefault::Primary,
            },
            Control::Color {
                key: "background",
                label: "Background",
                default: ColorDefault::Secondary,
            },
            seed(),
        ],
        tiles,
    ));
    v.push(stylize(
        "pcExtrude",
        vec![
            Control::Select {
                key: "type",
                label: "Type",
                default: "blocks",
                choices: &[("blocks", "Blocks"), ("pyramids", "Pyramids")],
            },
            range("size", "Size", 2.0, 255.0, 30.0),
            range("depth", "Depth", 1.0, 255.0, 30.0),
            Control::Select {
                key: "depthMode",
                label: "Depth Mode",
                default: "random",
                choices: &[("random", "Random"), ("levelBased", "Level-based")],
            },
            Control::Checkbox {
                key: "solidFrontFaces",
                label: "Solid Front Faces",
                default: false,
            },
            Control::Checkbox {
                key: "maskIncompleteBlocks",
                label: "Mask Incomplete Blocks",
                default: false,
            },
            seed(),
        ],
        extrude,
    ));
    v
}

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

/// The seed row folded with the instance seed, as Clouds does.
fn bounds(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

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

fn wind(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let method = match choice(v, "method") {
        "blast" => WindMethod::Blast,
        "stagger" => WindMethod::Stagger,
        _ => WindMethod::Wind,
    };
    let out = fx::stylize2::wind(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        method,
        choice(v, "direction") != "right",
        seed(v, env),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn trace_contour(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = fx::stylize2::trace_contour(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        num(v, "level") as f32,
        choice(v, "edge") == "upper",
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn tiles(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let fill = match choice(v, "fill") {
        "foreground" => TileFill::Foreground,
        "inverse" => TileFill::Inverse,
        "unaltered" => TileFill::Unaltered,
        _ => TileFill::Background,
    };
    let colour = |key| color_value(v, key, env).map(|b| f32::from(b) / 255.0);
    let out = fx::stylize2::tiles(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        num(v, "count") as u32,
        num(v, "maxOffset") as f32,
        fill,
        (colour("foreground"), colour("background")),
        seed(v, env),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn extrude(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let spec = ExtrudeSpec {
        kind: if choice(v, "type") == "pyramids" {
            ExtrudeType::Pyramids
        } else {
            ExtrudeType::Blocks
        },
        size: num(v, "size") as f32,
        depth: num(v, "depth") as f32,
        level_based: choice(v, "depthMode") == "levelBased",
        solid_front: flag(v, "solidFrontFaces"),
        mask_incomplete: flag(v, "maskIncompleteBlocks"),
        seed: seed(v, env),
    };
    let out = fx::stylize2::extrude(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        &spec,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn solarize(src: &ImageData, _: &Values, _: &EffectEnv) -> ImageData {
    let mut buf = shim::to_f32(src);
    fx::stylize::solarize(&mut buf);
    shim::from_f32(&buf, src.width, src.height)
}

fn high_pass(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (w, h) = (src.width as usize, src.height as usize);
    let buf = shim::to_f32(src);
    let ctl = Interrupt(env.cancel);
    match fx::sharpen::high_pass(&buf, w, h, num(v, "radius") as f32, &ctl) {
        Some(out) => shim::from_f32(&out, src.width, src.height),
        None => src.clone(),
    }
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run, solid};
    use serde_json::json;

    /// Flat layers have no detail: mid grey, alpha kept.
    #[test]
    fn high_pass_flat_is_mid_grey() {
        let out = run("pcHighPass", json!({}), &solid(12, 9, [30, 200, 90, 255]));
        for (x, y) in [(0, 0), (5, 4), (11, 8)] {
            let p = px(&out, x, y);
            assert!(
                p[..3].iter().all(|c| c.abs_diff(128) <= 1) && p[3] == 255,
                "{p:?}"
            );
        }
    }

    /// A white dot on black: "From the Left" streaks only to its right; "From the Right" only to
    /// its left.
    #[test]
    fn wind_from_the_right_streaks_left_and_from_the_left_streaks_right() {
        let src = from_fn(80, 9, |x, y| {
            if (x, y) == (40, 4) {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let lit = |out: &crate::canvas::ImageData, mut xs: std::ops::Range<u32>| {
            xs.any(|x| px(out, x, 4)[0] > 0)
        };
        for (dir, right) in [("right", true), ("left", false)] {
            // "right" is the key of "From the Left" (streaks toward +x).
            let out = run("pcWind", json!({"method": "blast", "direction": dir}), &src);
            let (ahead, behind) = if right {
                (41..80, 0..40)
            } else {
                (0..40, 41..80)
            };
            assert!(lit(&out, ahead), "{dir}: no streak downwind");
            assert!(!lit(&out, behind), "{dir}: streak upwind");
            assert!((0..80).all(|x| px(&out, x, 3)[0] == 0 && px(&out, x, 5)[0] == 0));
        }
    }

    /// A left-dark, right-light edge crossing Level 128 is outlined once: only on the lower
    /// (dark) side for Lower, the light side for Upper; flat areas are white paper.
    #[test]
    fn trace_contour_marks_the_level_crossing() {
        let src = from_fn(8, 4, |x, _| {
            if x < 4 {
                [50, 50, 50, 255]
            } else {
                [200, 200, 200, 255]
            }
        });
        let lower = run("pcTraceContour", json!({}), &src);
        let upper = run("pcTraceContour", json!({"edge": "upper"}), &src);
        let col = |o: &crate::canvas::ImageData, x| px(o, x, 2)[0];
        assert_eq!([2, 3, 4, 5].map(|x| col(&lower, x)), [255, 0, 255, 255]);
        assert_eq!([2, 3, 4, 5].map(|x| col(&upper, x)), [255, 255, 0, 255]);
        assert_eq!(px(&lower, 0, 0), [255, 255, 255, 255]);
        // A higher level than every pixel: nothing to trace.
        let none = run("pcTraceContour", json!({"level": 250}), &src);
        assert!((0..8).all(|x| col(&none, x) == 255));
    }

    /// Upstream's minimum Max Offset is 1 %, so identity is reached only at the floor: with
    /// shifts under half a pixel every tile still shows its own pixels, for any Number of Tiles.
    /// At 99 % the shifts differ per tile, so a coarser grid gives fewer distinct shifts.
    #[test]
    fn tiles_count_matches_grid() {
        let src = from_fn(32, 32, |x, y| [(x * 8) as u8, (y * 8) as u8, 7, 255]);
        for count in [1, 4, 10] {
            let out = run("pcTiles", json!({"count": count, "maxOffset": 1}), &src);
            assert_eq!(out.data, src.data, "count {count}");
        }
        let shifts = |count: u32| {
            let out = run(
                "pcTiles",
                json!({"count": count, "maxOffset": 99, "fill": "unaltered"}),
                &src,
            );
            let mut set = std::collections::BTreeSet::new();
            for (x, y) in (0..32).flat_map(|y| (0..32).map(move |x| (x, y))) {
                let p = px(&out, x, y);
                // Interior pixels only: edge samples are clamped to the layer.
                if (2..30).contains(&(i32::from(p[0]) / 8))
                    && (2..30).contains(&(i32::from(p[1]) / 8))
                {
                    set.insert((
                        i32::from(p[0]) / 8 - x as i32,
                        i32::from(p[1]) / 8 - y as i32,
                    ));
                }
            }
            set.len()
        };
        assert!(shifts(2) < shifts(8), "{} !< {}", shifts(2), shifts(8));
    }

    /// The same seed gives the same layer; another seed changes random-height Blocks.
    #[test]
    fn extrude_is_deterministic_with_seed() {
        let src = from_fn(48, 48, |x, y| [(x * 5) as u8, (y * 5) as u8, 90, 255]);
        let go = |seed: u32, ty: &str| {
            run(
                "pcExtrude",
                json!({"size": 8, "depth": 120, "seed": seed, "type": ty}),
                &src,
            )
        };
        for ty in ["blocks", "pyramids"] {
            assert_eq!(go(3, ty).data, go(3, ty).data, "{ty}");
        }
        assert_ne!(go(3, "blocks").data, go(4, "blocks").data);
        assert_ne!(go(3, "blocks").data, src.data);
    }

    /// Tiles' gaps take the secondary colour for Fill Background, the primary for Foreground.
    #[test]
    fn tiles_fill_uses_palette_colours() {
        let src = solid(30, 30, [10, 20, 30, 255]);
        let go = |fill| {
            run(
                "pcTiles",
                json!({"count": 3, "maxOffset": 99, "fill": fill}),
                &src,
            )
        };
        let has = |o: &crate::canvas::ImageData, c: [u8; 4]| o.data.chunks(4).any(|p| p == c);
        let env = crate::effect::EffectEnv::whole(1, 1);
        assert!(has(&go("background"), env.secondary));
        assert!(has(&go("foreground"), env.primary));
    }

    /// Tiles and Extrude anchor on the selection bounds: inside a rectangular selection the
    /// result equals the effect run on the layer cropped to that rectangle (fixed seed, exact).
    #[test]
    fn tiles_and_extrude_anchor_on_selection_bounds() {
        use crate::effect::bb::test_util::{rect_selection, with_selection};
        use crate::effect::test_util::run_env;
        let (w, h, l, t, r, b) = (48u32, 36u32, 8u32, 4u32, 32u32, 20u32);
        let noise = |x: u32, y: u32| {
            let mut st = u64::from(y * 97 + x);
            let v = crate::extra::rng::splitmix64(&mut st).to_le_bytes();
            [v[0], v[1], v[2], 255]
        };
        let full = from_fn(w, h, noise);
        let crop = from_fn(r - l, b - t, |x, y| noise(x + l, y + t));
        let (mask, polys) = rect_selection(w, h, l, t, r, b);
        let env = with_selection(w, h, &mask, &polys);
        let cases = [
            (
                "pcTiles",
                json!({"count": 3, "maxOffset": 60, "fill": "unaltered", "seed": 5}),
            ),
            ("pcExtrude", json!({"size": 8, "depth": 150, "seed": 5})),
            (
                "pcExtrude",
                json!({"size": 8, "depth": 150, "seed": 5, "type": "pyramids"}),
            ),
        ];
        for (id, v) in cases {
            let a = run_env(id, v.clone(), &full, &env);
            let c = run(id, v, &crop);
            for (x, y) in (t..b).flat_map(|y| (l..r).map(move |x| (x, y))) {
                assert_eq!(px(&a, x, y), px(&c, x - l, y - t), "{id} at {x},{y}");
            }
        }
    }
}
