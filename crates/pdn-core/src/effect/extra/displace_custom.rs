//! Photocraft phase spec: S§4.2; research R§B (Distort, Stylize, Color and Photo groups)
//!
//! Displace, Shear, Custom, HSB/HSL, NTSC Colors and De-Interlace (`extra::fx::{distort2_pc30,
//! other2, video}`).

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::channels::layer_pixels;
use crate::effect::bb::index;
use crate::effect::bb::ui::IndirectUi;
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, choice, num, range};
use crate::extra::fx::distort2_pc30::{self as distort, Edge, Img};
use crate::extra::fx::other2::{self, HsbModel};
use crate::extra::fx::video;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::shim;

/// The reference bounds: the selection's bounds, else the layer.
fn bounds(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

/// Displace's dialog draws its Map row (the generic layout skips `LayerSelect`) and starts on the
/// layer below the active one, or above it at the bottom.
static DISPLACE_UI: IndirectUi = IndirectUi {
    defaults: |v, env| {
        let map = if env.layer_index > 0 {
            env.layer_index - 1
        } else {
            1.min(env.layers.len().saturating_sub(1))
        };
        v.insert("map".into(), map.into());
    },
    ..IndirectUi::DEFAULT
};

fn def(id: &'static str, controls: Vec<Control>, render: crate::effect::CpuFn) -> EffectDef {
    let m = manifest(id).expect("effect has a manifest row");
    EffectDef {
        id,
        icon: m.icon,
        category: m.category,
        controls,
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: None,
    }
}

const UNDEFINED: &[(&str, &str)] = &[("wrap", "Wrap Around"), ("repeat", "Repeat Edge Pixels")];
const MODES: &[(&str, &str)] = &[("rgb", "RGB"), ("hsb", "HSB"), ("hsl", "HSL")];
const KERNEL_KEYS: [&str; 25] = [
    "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13", "k14",
    "k15", "k16", "k17", "k18", "k19", "k20", "k21", "k22", "k23", "k24",
];
const KERNEL_LABELS: [&str; 25] = [
    "Row 1, Column 1",
    "Row 1, Column 2",
    "Row 1, Column 3",
    "Row 1, Column 4",
    "Row 1, Column 5",
    "Row 2, Column 1",
    "Row 2, Column 2",
    "Row 2, Column 3",
    "Row 2, Column 4",
    "Row 2, Column 5",
    "Row 3, Column 1",
    "Row 3, Column 2",
    "Row 3, Column 3",
    "Row 3, Column 4",
    "Row 3, Column 5",
    "Row 4, Column 1",
    "Row 4, Column 2",
    "Row 4, Column 3",
    "Row 4, Column 4",
    "Row 4, Column 5",
    "Row 5, Column 1",
    "Row 5, Column 2",
    "Row 5, Column 3",
    "Row 5, Column 4",
    "Row 5, Column 5",
];
/// Shear's curve (approximation: R22): offsets at 0/25/50/75/100 % of the height.
const SHEAR_KEYS: [&str; 5] = ["s0", "s25", "s50", "s75", "s100"];
const SHEAR_LABELS: [&str; 5] = [
    "Offset at 0% of height",
    "Offset at 25% of height",
    "Offset at 50% of height",
    "Offset at 75% of height",
    "Offset at 100% of height",
];

pub fn effects() -> Vec<EffectDef> {
    let mut kernel: Vec<Control> = (0..25)
        .map(|i| {
            range(
                KERNEL_KEYS[i],
                KERNEL_LABELS[i],
                -999.0,
                999.0,
                if i == 12 { 1.0 } else { 0.0 },
            )
        })
        .collect();
    kernel.push(range("scale", "Scale", 1.0, 9999.0, 1.0));
    kernel.push(range("offset", "Offset", -9999.0, 9999.0, 0.0));
    // approximation: (R22) — the generic dialog cannot draw a Control::Curve.
    let mut shear: Vec<Control> = (0..5)
        .map(|i| range(SHEAR_KEYS[i], SHEAR_LABELS[i], -100.0, 100.0, 0.0))
        .collect();
    shear.push(Control::Select {
        key: "undefined",
        label: "Undefined Areas",
        default: "wrap",
        choices: UNDEFINED,
    });
    vec![
        EffectDef {
            indirect: Some(&DISPLACE_UI),
            ..def(
                "pcDisplace",
                vec![
                    range("horizontal", "Horizontal Scale", -999.0, 999.0, 10.0),
                    range("vertical", "Vertical Scale", -999.0, 999.0, 10.0),
                    Control::LayerSelect {
                        key: "map",
                        label: "Displacement Map",
                    },
                    Control::Select {
                        key: "fit",
                        label: "Map Fit",
                        default: "stretch",
                        choices: &[("stretch", "Stretch To Fit"), ("tile", "Tile")],
                    },
                    Control::Select {
                        key: "undefined",
                        label: "Undefined Areas",
                        default: "repeat",
                        choices: UNDEFINED,
                    },
                ],
                displace,
            )
        },
        def("pcShear", shear, shear_render),
        def("pcCustom", kernel, custom),
        def(
            "pcHsbHsl",
            vec![
                Control::Select {
                    key: "inputMode",
                    label: "Input Mode",
                    default: "rgb",
                    choices: MODES,
                },
                Control::Select {
                    key: "rowOrder",
                    label: "Row Order",
                    default: "hsb",
                    choices: MODES,
                },
            ],
            hsb_hsl,
        ),
        def("pcNtscColors", vec![], ntsc),
        def(
            "pcDeInterlace",
            vec![
                Control::Select {
                    key: "eliminate",
                    label: "Eliminate",
                    default: "oddFields",
                    choices: &[("oddFields", "Odd Fields"), ("evenFields", "Even Fields")],
                },
                Control::Select {
                    key: "createBy",
                    label: "New Field Created By",
                    default: "interpolation",
                    choices: &[
                        ("interpolation", "Interpolation"),
                        ("duplication", "Duplication"),
                    ],
                },
            ],
            de_interlace,
        ),
    ]
}

fn edge(v: &Values) -> Edge {
    if choice(v, "undefined") == "repeat" {
        Edge::Repeat
    } else {
        Edge::Wrap
    }
}

fn displace(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let buf = shim::to_f32(src);
    // No layer list (or a missing map layer): nothing to displace by.
    let map_buf = layer_pixels(env, index(v, "map")).map(shim::to_f32);
    let img = Img {
        w: src.width as i32,
        h: src.height as i32,
        data: &buf,
    };
    let map = layer_pixels(env, index(v, "map"))
        .zip(map_buf.as_deref())
        .map(|(l, data)| Img {
            w: l.width as i32,
            h: l.height as i32,
            data,
        });
    let out = distort::displace(
        img,
        bounds(env),
        (num(v, "horizontal") as f32, num(v, "vertical") as f32),
        choice(v, "fit") != "tile",
        edge(v),
        map,
        Interrupt(env.cancel),
    );
    shim::from_f32(&out, src.width, src.height)
}

fn shear_render(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let buf = shim::to_f32(src);
    let points: Vec<[f32; 2]> = (0..5)
        .map(|i| [i as f32 / 4.0, num(v, SHEAR_KEYS[i]) as f32 / 100.0])
        .collect();
    let img = Img {
        w: src.width as i32,
        h: src.height as i32,
        data: &buf,
    };
    let out = distort::shear(img, bounds(env), &points, edge(v), Interrupt(env.cancel));
    shim::from_f32(&out, src.width, src.height)
}

fn custom(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let k: Vec<f32> = KERNEL_KEYS.iter().map(|k| num(v, k) as f32).collect();
    let out = other2::custom(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        &k,
        num(v, "scale") as f32,
        num(v, "offset") as f32,
        Interrupt(env.cancel),
    );
    shim::from_f32(&out, src.width, src.height)
}

fn hsb_hsl(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let m = |key| match choice(v, key) {
        "hsb" => HsbModel::Hsb,
        "hsl" => HsbModel::Hsl,
        _ => HsbModel::Rgb,
    };
    let mut buf = shim::to_f32(src);
    other2::hsb_hsl(&mut buf, m("inputMode"), m("rowOrder"));
    shim::from_f32(&buf, src.width, src.height)
}

fn ntsc(src: &ImageData, _: &Values, _: &EffectEnv) -> ImageData {
    let mut buf = shim::to_f32(src);
    video::ntsc(&mut buf);
    shim::from_f32(&buf, src.width, src.height)
}

fn de_interlace(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = video::deinterlace(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        bounds(env),
        choice(v, "eliminate") == "evenFields",
        choice(v, "createBy") != "duplication",
    );
    shim::from_f32(&out, src.width, src.height)
}

#[cfg(test)]
mod tests {
    use crate::effect::LayerInfo;
    use crate::effect::bb::indirect_defaults;
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use crate::effect::{EffectEnv, find};
    use serde_json::json;
    use std::sync::Arc;

    /// R91: Stylize's convolution is named for what it does; its id stays `pcCustom`.
    #[test]
    fn custom_is_named_custom_kernel() {
        let m = crate::effect::extra::manifest::manifest("pcCustom").unwrap();
        assert_eq!(m.name, "Custom Kernel");
        assert_eq!(
            find("pcCustom").unwrap().dialog_title,
            Some("Custom Kernel")
        );
    }

    fn gradient() -> crate::canvas::ImageData {
        from_fn(16, 12, |x, y| [(x * 15) as u8, (y * 20) as u8, 90, 255])
    }

    fn within(a: &crate::canvas::ImageData, b: &crate::canvas::ImageData, tol: u8) -> bool {
        a.data
            .iter()
            .zip(&b.data)
            .all(|(p, q)| p.abs_diff(*q) <= tol)
    }

    /// A mid-grey map moves nothing (128/255 is half a level off the 0.5 centre).
    #[test]
    fn displace_with_mid_grey_map_is_identity() {
        let src = gradient();
        let ls = [LayerInfo {
            name: "Map".into(),
            pixels: Arc::new(from_fn(4, 4, |_, _| [128, 128, 128, 255])),
        }];
        let mut env = EffectEnv::whole(16, 12);
        env.layers = &ls;
        for fit in ["stretch", "tile"] {
            let out = run_env("pcDisplace", json!({"map": 0, "fit": fit}), &src, &env);
            assert!(within(&out, &src, 2), "{fit}");
        }
        // A black map moves the picture (by the full scale to the left/up).
        let ls = [LayerInfo {
            name: "Map".into(),
            pixels: Arc::new(from_fn(4, 4, |_, _| [0, 0, 0, 255])),
        }];
        env.layers = &ls;
        let moved = run_env("pcDisplace", json!({"map": 0, "horizontal": 5}), &src, &env);
        assert_ne!(moved.data, src.data);
    }

    #[test]
    fn displace_without_layers_is_identity() {
        let src = gradient();
        assert_eq!(run("pcDisplace", json!({}), &src).data, src.data);
    }

    #[test]
    fn shear_straight_curve_is_identity() {
        let src = gradient();
        assert!(within(&run("pcShear", json!({}), &src), &src, 0));
        let bent = run("pcShear", json!({"s50": 50}), &src);
        // Row 0 sits on a zero offset; the middle rows move.
        assert_eq!(px(&bent, 3, 0), px(&src, 3, 0));
        assert_ne!(px(&bent, 3, 6), px(&src, 3, 6));
    }

    #[test]
    fn custom_default_kernel_is_identity() {
        let src = gradient();
        assert_eq!(run("pcCustom", json!({}), &src).data, src.data);
        let out = run("pcCustom", json!({"k12": 2, "scale": 2}), &src);
        assert!(within(&out, &src, 1));
        let off = run("pcCustom", json!({"offset": 51}), &src);
        assert_eq!(px(&off, 0, 0)[0], 51);
        assert_eq!(px(&off, 0, 0)[3], 255);
    }

    #[test]
    fn hsb_effect_encodes_and_decodes() {
        let src = from_fn(1, 1, |_, _| [255, 0, 0, 200]);
        let hsb = run("pcHsbHsl", json!({}), &src);
        assert_eq!(px(&hsb, 0, 0), [0, 255, 255, 200]);
        let back = run(
            "pcHsbHsl",
            json!({"inputMode": "hsb", "rowOrder": "rgb"}),
            &hsb,
        );
        assert_eq!(px(&back, 0, 0), [255, 0, 0, 200]);
    }

    #[test]
    fn ntsc_tames_saturated_red_only() {
        let src = from_fn(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 255]
            } else {
                [100, 110, 120, 255]
            }
        });
        let out = run("pcNtscColors", json!({}), &src);
        assert_ne!(px(&out, 0, 0), px(&src, 0, 0));
        assert_eq!(px(&out, 1, 0), px(&src, 1, 0));
    }

    #[test]
    fn de_interlace_duplicates_the_kept_field() {
        let src = from_fn(2, 4, |_, y| [(y * 60) as u8, 0, 0, 255]);
        let out = run(
            "pcDeInterlace",
            json!({"eliminate": "evenFields", "createBy": "duplication"}),
            &src,
        );
        // Even fields keep offsets 0 and 2; offsets 1 and 3 copy the line above.
        let col: Vec<u8> = (0..4).map(|y| px(&out, 0, y)[0]).collect();
        assert_eq!(col, [0, 0, 120, 120]);
    }

    /// Displace's Map row is drawn only on the indirect path; the default map is another layer.
    #[test]
    fn displace_defaults_to_another_layer_and_has_an_indirect_ui() {
        let def = find("pcDisplace").unwrap();
        assert!(def.indirect.is_some());
        let img = from_fn(1, 1, |_, _| [1, 2, 3, 255]);
        let ls: Vec<LayerInfo> = (0..3)
            .map(|i| LayerInfo {
                name: format!("L{i}"),
                pixels: Arc::new(img.clone()),
            })
            .collect();
        for active in 0..3 {
            let mut env = EffectEnv::whole(1, 1);
            env.layers = &ls;
            env.layer_index = active;
            let map = crate::effect::num(&indirect_defaults(&def, &env), "map") as usize;
            assert_ne!(map, active);
            assert!(map < 3);
        }
    }
}
