//! BB spec: effects/object-apply-mask.md; effects/object-switch-alpha-to-gray.md;
//! effects/object-switch-gray-to-alpha.md; common.md §1, §2, §3, §4, §5.11

use super::conventions::{APPLY_MASK_STORED_TRANSPARENT_COLOUR, GPU_CONTEXT};
use super::index;
use super::manifest::manifest;
use super::ui::{ControlUi, IndirectUi};
use crate::canvas::ImageData;
use crate::effect::surface::{Alpha, EffectSurface, Space, luma, premultiply, unpremultiply};
use crate::effect::{
    Category, Control, EffectDef, EffectEnv, EffectKind, GpuFn, LabelText, RenderInfo, Values,
    choice, flag,
};
use crate::pixel::gamma::linear_to_srgb_f32;
use std::sync::LazyLock;

static APPLY_CURRENT: LazyLock<String> =
    LazyLock::new(|| format!("Current layer{}", " ".repeat(4)));
static ALPHA_CURRENT: LazyLock<String> =
    LazyLock::new(|| format!("Current layer{}", " ".repeat(5)));
static CAPTIONS: LazyLock<[String; 4]> = LazyLock::new(|| {
    [
        ("Mask layer", 5),
        ("Mask type", 6),
        ("Method", 10),
        ("Mask of layer", 2),
    ]
    .map(|(label, spaces)| format!("{label}{}", " ".repeat(spaces)))
});
static APPLY_ROWS: LazyLock<[(&str, ControlUi); 5]> = LazyLock::new(|| {
    [
        ("current", ControlUi::NO_HEADER),
        (
            "mask",
            ControlUi {
                caption: &CAPTIONS[0],
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "maskType",
            ControlUi {
                caption: &CAPTIONS[1],
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "method",
            ControlUi {
                caption: &CAPTIONS[2],
                ..ControlUi::NO_HEADER
            },
        ),
        ("invert", ControlUi::NO_HEADER),
    ]
});
static APPLY_UI: LazyLock<IndirectUi> = LazyLock::new(|| IndirectUi {
    controls: &*APPLY_ROWS,
    defaults: |v, env| {
        v.insert(
            "mask".into(),
            env.layer_index
                .saturating_add(1)
                .min(env.layers.len().saturating_sub(1))
                .into(),
        );
    },
    ..IndirectUi::DEFAULT
});
static ALPHA_ROWS: LazyLock<[(&str, ControlUi); 3]> = LazyLock::new(|| {
    [
        ("info", ControlUi::NO_HEADER),
        (
            "layer",
            ControlUi {
                caption: &CAPTIONS[3],
                ..ControlUi::NO_HEADER
            },
        ),
        ("invert", ControlUi::NO_HEADER),
    ]
});
static ALPHA_UI: LazyLock<IndirectUi> = LazyLock::new(|| IndirectUi {
    controls: &*ALPHA_ROWS,
    defaults: |v, env| {
        v.insert("layer".into(), env.layer_index.into());
    },
    ..IndirectUi::DEFAULT
});
static GRAY_UI: IndirectUi = IndirectUi {
    controls: &[("mode", ControlUi::NO_HEADER)],
    ..IndirectUi::DEFAULT
};

fn channel(
    id: &'static str,
    title: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    render: GpuFn,
    ui: &'static IndirectUi,
) -> EffectDef {
    EffectDef {
        id,
        icon: manifest(id).expect("registered channel effect").icon,
        category: Category::Effect("object"),
        controls,
        kind: EffectKind::Gpu(info, render),
        dialog: None,
        dialog_title: Some(title),
        indirect: Some(ui),
    }
}

/// The Object channel effects, in manifest order.
pub fn effects() -> Vec<EffectDef> {
    vec![
        channel(
            "bbApplyMask",
            "Apply Alpha Mask",
            vec![
                Control::Label {
                    key: "current",
                    label: "",
                    text: LabelText::CurrentLayer(&APPLY_CURRENT),
                },
                Control::LayerSelect {
                    key: "mask",
                    label: "",
                },
                Control::Select {
                    key: "maskType",
                    label: "",
                    default: "gray",
                    choices: &[("gray", "Shades of gray"), ("alpha", "Alpha")],
                },
                Control::Select {
                    key: "method",
                    label: "",
                    default: "replace",
                    choices: &[
                        ("replace", "Replace"),
                        ("minimum", "Minimum"),
                        ("multiply", "Multiply"),
                    ],
                },
                Control::Checkbox {
                    key: "invert",
                    label: "Invert mask",
                    default: false,
                },
            ],
            RenderInfo::new(Space::Srgb, Alpha::Straight),
            apply_mask,
            &APPLY_UI,
        ),
        channel(
            "bbSwitchAlphaToGray",
            "Switch Alpha to Gray",
            vec![
                Control::Label {
                    key: "info",
                    label: "Create an alpha mask by converting alpha to shades of gray.\n ",
                    text: LabelText::CurrentLayer(&ALPHA_CURRENT),
                },
                Control::LayerSelect {
                    key: "layer",
                    label: "",
                },
                Control::Checkbox {
                    key: "invert",
                    label: "Invert mask",
                    default: false,
                },
            ],
            GPU_CONTEXT,
            alpha_to_gray,
            &ALPHA_UI,
        ),
        channel(
            "bbSwitchGrayToAlpha",
            "Switch Gray to Alpha",
            vec![Control::Select {
                key: "mode",
                label: "Isolate line art by deleting",
                default: "white",
                choices: &[("white", "White"), ("black", "Black")],
            }],
            GPU_CONTEXT,
            gray_to_alpha,
            &GRAY_UI,
        ),
    ]
}

/// Raw pixels at a stored bottom-first layer index, clamped for Repeat with fewer layers.
/// An empty layer list asks the caller to use its own input instead.
pub fn layer_pixels<'a>(env: &'a EffectEnv, stored: usize) -> Option<&'a ImageData> {
    env.layers
        .get(stored.min(env.layers.len().saturating_sub(1)))
        .map(|layer| layer.pixels.as_ref())
}

fn straight(p: [f32; 4], input: Alpha) -> [f32; 4] {
    match input {
        Alpha::Straight => p,
        Alpha::Premultiplied => unpremultiply(p),
    }
}

fn output(p: [f32; 4], alpha: Alpha) -> [f32; 4] {
    match alpha {
        Alpha::Straight => p,
        Alpha::Premultiplied => premultiply(p),
    }
}

fn apply_mask(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let mask = layer_pixels(env, index(v, "mask"));
    let alpha_mask = choice(v, "maskType") == "alpha";
    let method = choice(v, "method");
    let invert = flag(v, "invert");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let mut p = straight(src.at(x, y), info.input);
        let q = if let Some(mask) = mask {
            let i = (y as usize * mask.width as usize + x as usize) * 4;
            std::array::from_fn(|c| mask.data[i + c] as f32 / 255.0)
        } else {
            let mut q = p;
            if info.space == Space::Linear {
                for c in &mut q[..3] {
                    *c = linear_to_srgb_f32(*c);
                }
            }
            q
        };
        // spec-question: Q19 transparent mask pixels: stored colour
        // approximation: stored transparent mask colour (object-apply-mask.md Notes / uncertainties)
        let mut m = if alpha_mask {
            q[3]
        } else if q[3] == 0.0 && !APPLY_MASK_STORED_TRANSPARENT_COLOUR {
            0.0
        } else {
            luma(q[0], q[1], q[2])
        };
        if invert {
            m = 1.0 - m;
        }
        p[3] = match method {
            "minimum" => p[3].min(m),
            "multiply" => p[3] * m,
            _ => m,
        };
        output(p, info.output)
    })
}

fn alpha_to_gray(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let layer = layer_pixels(env, index(v, "layer"));
    let invert = flag(v, "invert");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let a = layer.map_or_else(
            || src.at(x, y)[3],
            |layer| {
                layer.data[(y as usize * layer.width as usize + x as usize) * 4 + 3] as f32 / 255.0
            },
        );
        let g = if invert { 1.0 - a } else { a };
        [g, g, g, 1.0]
    })
}

fn gray_to_alpha(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let black = choice(v, "mode") == "black";
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = src.at(x, y);
        // quirk(bb): object-switch-gray-to-alpha.md Behaviour step 3
        let u = if p[3] == 0.0 {
            [0.0; 4]
        } else {
            straight(p, info.input)
        };
        let y = luma(u[0], u[1], u[2]);
        output(
            if black {
                [1.0, 1.0, 1.0, y]
            } else {
                [0.0, 0.0, 0.0, 1.0 - y]
            },
            info.output,
        )
    })
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
    use crate::effect::bb::indirect_defaults;
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::{Help, HelpWindow};
    use crate::effect::session::EffectSession;
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::{
        Category, Control, EffectEnv, EffectKind, LabelText, LayerInfo, RenderInfo, find,
    };
    use serde_json::{Value, json};
    use std::sync::Arc;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const LINEAR: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);

    fn layers(imgs: &[ImageData]) -> Vec<LayerInfo> {
        imgs.iter()
            .enumerate()
            .map(|(i, p)| LayerInfo {
                name: format!("Layer {}", i + 1),
                pixels: Arc::new(p.clone()),
            })
            .collect()
    }

    fn pair(a: [u8; 4], b: [u8; 4]) -> ImageData {
        from_fn(2, 1, |x, _| if x == 0 { a } else { b })
    }

    fn masked(src: &ImageData, mask: ImageData, values: Value) -> ImageData {
        let ls = layers(&[src.clone(), mask]);
        let out = bb_env("bbApplyMask", values, src, &layers_env(&ls, 0));
        // Straight colour must survive for all pixels, including alpha zero.
        for (s, o) in src
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.data.as_chunks::<4>().0)
        {
            assert_eq!(&o[..3], &s[..3]);
        }
        out
    }

    /// object-apply-mask.md Expected results 1.
    #[test]
    fn apply_defaults_white_black() {
        let out = masked(&pair(RED, RED), pair([255; 4], [0, 0, 0, 255]), json!({}));
        assert_px(&out, 0, 0, RED);
        assert_px(&out, 1, 0, [255, 0, 0, 0]);
    }

    /// object-apply-mask.md Expected results 2.
    #[test]
    fn apply_luma_weights() {
        let out = masked(&pair(RED, RED), pair(RED, [128, 128, 128, 255]), json!({}));
        assert_px(&out, 0, 0, [255, 0, 0, 76]);
        assert_px(&out, 1, 0, [255, 0, 0, 128]);
    }

    /// object-apply-mask.md Expected results 3.
    #[test]
    fn apply_methods_and_inversion() {
        for (method, plain, inverse) in [
            ("replace", 100, 155),
            ("minimum", 100, 155),
            ("multiply", 78, 122),
        ] {
            for (invert, alpha) in [(false, plain), (true, inverse)] {
                let out = masked(
                    &pair([255, 0, 0, 200], [255, 0, 0, 200]),
                    solid(2, 1, [100, 100, 100, 255]),
                    json!({"method":method,"invert":invert}),
                );
                for x in 0..2 {
                    assert_px(&out, x, 0, [255, 0, 0, alpha]);
                }
            }
        }
    }

    /// object-apply-mask.md Expected results 4.
    #[test]
    fn apply_alpha_mask() {
        for (invert, a, b) in [(false, 64, 0), (true, 191, 255)] {
            let out = masked(
                &pair(RED, RED),
                pair([0x12, 0x34, 0x56, 0x40], [0; 4]),
                json!({"maskType":"alpha","invert":invert}),
            );
            assert_px(&out, 0, 0, [255, 0, 0, a]);
            assert_px(&out, 1, 0, [255, 0, 0, b]);
        }
    }

    /// object-apply-mask.md Expected results 5.
    #[test]
    fn apply_default_layer_and_self_identity() {
        let ls = layers(&[
            pair(RED, RED),
            pair(RED, RED),
            pair([1, 2, 3, 40], [4, 5, 6, 90]),
        ]);
        let def = find("bbApplyMask").unwrap();
        for i in [1, 2] {
            assert_eq!(
                indirect_defaults(&def, &layers_env(&ls, i))["mask"],
                json!(2)
            );
        }
        let top = ls[2].pixels.as_ref();
        assert_eq!(
            bb_env(
                "bbApplyMask",
                json!({"maskType":"alpha"}),
                top,
                &layers_env(&ls, 2)
            )
            .data,
            top.data
        );
    }

    /// object-switch-alpha-to-gray.md Expected results 1.
    #[test]
    fn alpha_to_gray_opaque_transparent() {
        let src = pair(RED, [0; 4]);
        let ls = layers(std::slice::from_ref(&src));
        let out = bb_env("bbSwitchAlphaToGray", json!({}), &src, &layers_env(&ls, 0));
        assert_px(&out, 0, 0, [255; 4]);
        assert_px(&out, 1, 0, [0, 0, 0, 255]);
    }

    /// object-switch-alpha-to-gray.md Expected results 2.
    #[test]
    fn alpha_to_gray_inverted() {
        let out = bb(
            "bbSwitchAlphaToGray",
            json!({"invert":true}),
            &pair(RED, [0; 4]),
        );
        assert_px(&out, 0, 0, [0, 0, 0, 255]);
        assert_px(&out, 1, 0, [255; 4]);
    }

    /// object-switch-alpha-to-gray.md Expected results 3.
    #[test]
    fn alpha_to_gray_other_layer_unchanged() {
        let src = solid(3, 1, [0, 0, 255, 255]);
        let mask = from_fn(3, 1, |x, _| if x == 0 { [0, 255, 0, 255] } else { [0; 4] });
        let ls = layers(&[src.clone(), mask.clone()]);
        let out = bb_env(
            "bbSwitchAlphaToGray",
            json!({"layer":1}),
            &src,
            &layers_env(&ls, 0),
        );
        assert_px(&out, 0, 0, [255; 4]);
        for x in 1..3 {
            assert_px(&out, x, 0, [0, 0, 0, 255]);
        }
        assert_eq!(ls[1].pixels.data, mask.data);
        assert_eq!(ls[0].pixels.data, src.data);
    }

    /// object-switch-alpha-to-gray.md Expected results 4; Behaviour step 2 table, all columns.
    #[test]
    fn alpha_to_gray_context_table() {
        for (a, lin, lin_inv) in [
            (0, 0, 255),
            (32, 99, 240),
            (64, 137, 224),
            (128, 188, 187),
            (191, 224, 137),
            (255, 255, 0),
        ] {
            let src = solid(1, 1, [255, 0, 0, a]);
            for (context, g, gi) in [(LINEAR, lin, lin_inv), (ALT_GPU_CONTEXT, a, 255 - a)] {
                for (invert, want) in [(false, g), (true, gi)] {
                    assert_px(
                        &bb_ctx(
                            "bbSwitchAlphaToGray",
                            context,
                            json!({"invert":invert}),
                            &src,
                        ),
                        0,
                        0,
                        [want, want, want, 255],
                    );
                }
            }
        }
        assert_px(
            &bb(
                "bbSwitchAlphaToGray",
                json!({}),
                &solid(1, 1, [255, 0, 0, 128]),
            ),
            0,
            0,
            [188, 188, 188, 255],
        );
    }

    /// object-switch-alpha-to-gray.md Expected results 5; common.md §2.
    #[test]
    fn alpha_to_gray_selection_becomes_opaque() {
        let src = from_fn(256, 2, |x, _| [42, 100, 205, x as u8]);
        let (mask, polys) = rect_selection(256, 2, 16, 0, 240, 1);
        let env = with_selection(256, 2, &mask, &polys);
        for invert in [false, true] {
            let out = bb_env("bbSwitchAlphaToGray", json!({"invert":invert}), &src, &env);
            for y in 0..2 {
                for x in 0..256 {
                    let p = px(&out, x, y);
                    if y == 0 && (16..240).contains(&x) {
                        assert_eq!(p[3], 255);
                        assert_eq!(p[0], p[1]);
                        assert_eq!(p[1], p[2]);
                    } else {
                        assert_eq!(p, px(&src, x, y));
                    }
                }
            }
        }
    }

    /// object-switch-gray-to-alpha.md Expected results 1.
    #[test]
    fn gray_to_alpha_white_extremes() {
        let out = bb(
            "bbSwitchGrayToAlpha",
            json!({}),
            &pair([255; 4], [0, 0, 0, 255]),
        );
        assert_eq!(px(&out, 0, 0)[3], 0);
        assert_px(&out, 1, 0, [0, 0, 0, 255]);
    }

    /// object-switch-gray-to-alpha.md Expected results 2.
    #[test]
    fn gray_to_alpha_white_primaries() {
        for context in [LINEAR, ALT_GPU_CONTEXT] {
            for (src, alpha) in [(RED, 179), ([0, 255, 0, 255], 105), ([0, 0, 255, 255], 226)] {
                assert_px(
                    &bb_ctx("bbSwitchGrayToAlpha", context, json!({}), &solid(1, 1, src)),
                    0,
                    0,
                    [0, 0, 0, alpha],
                );
            }
        }
    }

    /// object-switch-gray-to-alpha.md Expected results 3.
    #[test]
    fn gray_to_alpha_black_mode() {
        let out = bb(
            "bbSwitchGrayToAlpha",
            json!({"mode":"black"}),
            &pair([255; 4], [0, 0, 0, 255]),
        );
        assert_px(&out, 0, 0, [255; 4]);
        assert_eq!(px(&out, 1, 0)[3], 0);
        assert_px(
            &bb(
                "bbSwitchGrayToAlpha",
                json!({"mode":"black"}),
                &solid(1, 1, RED),
            ),
            0,
            0,
            [255, 255, 255, 76],
        );
    }

    /// object-switch-gray-to-alpha.md Expected results 4; Behaviour step 3.
    #[test]
    fn gray_to_alpha_discards_alpha_and_zero_hidden_colour() {
        for context in [LINEAR, ALT_GPU_CONTEXT] {
            let out = bb_ctx(
                "bbSwitchGrayToAlpha",
                context,
                json!({}),
                &pair([255, 255, 255, 128], [0; 4]),
            );
            assert_eq!(px(&out, 0, 0)[3], 0);
            assert_px(&out, 1, 0, [0, 0, 0, 255]);
            for p in [[0; 4], [255, 255, 255, 0], [99, 241, 37, 0]] {
                let src = solid(1, 1, p);
                assert_px(
                    &bb_ctx("bbSwitchGrayToAlpha", context, json!({}), &src),
                    0,
                    0,
                    [0, 0, 0, 255],
                );
                assert_eq!(
                    px(
                        &bb_ctx(
                            "bbSwitchGrayToAlpha",
                            context,
                            json!({"mode":"black"}),
                            &src
                        ),
                        0,
                        0
                    )[3],
                    0
                );
            }
            let src = from_fn(255, 1, |x, _| [97, 180, 233, (x + 1) as u8]);
            for mode in ["white", "black"] {
                let out = bb_ctx("bbSwitchGrayToAlpha", context, json!({"mode":mode}), &src);
                let first = px(&out, 0, 0);
                for x in 1..255 {
                    assert_eq!(px(&out, x, 0), first);
                }
            }
        }
    }

    /// object-switch-gray-to-alpha.md Expected results 5; Behaviour grey table, all columns.
    #[test]
    fn gray_to_alpha_context_table() {
        for (v, white, black) in [
            (0, 255, 0),
            (32, 251, 4),
            (64, 242, 13),
            (128, 200, 55),
            (191, 122, 133),
            (224, 65, 190),
            (255, 0, 255),
        ] {
            let src = solid(1, 1, [v, v, v, 255]);
            for (context, w, b) in [(LINEAR, white, black), (ALT_GPU_CONTEXT, 255 - v, v)] {
                let a = bb_ctx("bbSwitchGrayToAlpha", context, json!({}), &src);
                assert_px(&a, 0, 0, [0, 0, 0, w]);
                let a = bb_ctx(
                    "bbSwitchGrayToAlpha",
                    context,
                    json!({"mode":"black"}),
                    &src,
                );
                assert!(px(&a, 0, 0)[3].abs_diff(b) <= TOL);
                if b > 0 {
                    assert_px(&a, 0, 0, [255, 255, 255, b]);
                }
            }
        }
        assert_px(
            &bb(
                "bbSwitchGrayToAlpha",
                json!({}),
                &solid(1, 1, [128, 128, 128, 255]),
            ),
            0,
            0,
            [0, 0, 0, 200],
        );
    }

    /// object-apply-mask.md Notes / uncertainties: Q19 stored straight mask colour, no alpha weighting.
    #[test]
    fn mask_reads_raw_transparent_colour() {
        let out = masked(
            &solid(4, 1, [43, 128, 230, 0]),
            from_fn(4, 1, |x, _| [100, 100, 100, [0, 1, 128, 255][x as usize]]),
            json!({}),
        );
        for x in 0..4 {
            assert_px(&out, x, 0, [43, 128, 230, 100]);
        }
    }

    /// object-apply-mask.md Behaviour and object-switch-alpha-to-gray.md Notes: stale indices clamp.
    #[test]
    fn layer_indices_clamp_and_empty_lists_use_source() {
        let ls = layers(&[solid(2, 1, RED), pair([255, 255, 255, 32], [0, 0, 0, 191])]);
        let src = ls[0].pixels.as_ref();
        for stored in [json!(1), json!(7), json!(u64::MAX)] {
            let out = bb_env(
                "bbApplyMask",
                json!({"mask":stored}),
                src,
                &layers_env(&ls, 0),
            );
            assert_px(&out, 0, 0, RED);
            assert_px(&out, 1, 0, [255, 0, 0, 0]);
            let out = bb_env(
                "bbSwitchAlphaToGray",
                json!({"layer":stored}),
                src,
                &layers_env(&ls, 0),
            );
            assert_px(&out, 0, 0, [99, 99, 99, 255]);
            assert_px(&out, 1, 0, [224, 224, 224, 255]);
        }
        let own = pair([200, 100, 50, 0], [11, 22, 33, 73]);
        assert_eq!(
            bb("bbApplyMask", json!({"mask":99,"maskType":"alpha"}), &own).data,
            own.data
        );
        let out = bb("bbApplyMask", json!({}), &own);
        assert_px(&out, 0, 0, [200, 100, 50, 124]);
        let def = find("bbApplyMask").unwrap();
        let mut env = EffectEnv::whole(2, 1);
        env.layer_index = usize::MAX;
        assert_eq!(indirect_defaults(&def, &env)["mask"], json!(0));
        env.layers = &ls;
        assert_eq!(indirect_defaults(&def, &env)["mask"], json!(1));
        for id in ["bbApplyMask", "bbSwitchAlphaToGray"] {
            let key = if id == "bbApplyMask" { "mask" } else { "layer" };
            let negative = json!({key:-99});
            let zero = json!({key:0});
            assert_eq!(
                bb_env(id, negative, src, &layers_env(&ls, 0)).data,
                bb_env(id, zero, src, &layers_env(&ls, 0)).data
            );
        }
    }

    /// object-apply-mask.md Behaviour steps 2–4: all source/mask byte boundaries and straight RGB.
    #[test]
    fn apply_method_properties_all_alpha_pairs() {
        let src = from_fn(256, 256, |x, y| [x as u8, 37, 219, y as u8]);
        let mask = from_fn(256, 256, |x, _| [5, 12, 201, x as u8]);
        for invert in [false, true] {
            let results = ["replace", "minimum", "multiply"].map(|method| {
                masked(
                    &src,
                    mask.clone(),
                    json!({"maskType":"alpha","method":method,"invert":invert}),
                )
            });
            for y in 0..256 {
                for x in 0..256 {
                    let [replace, minimum, multiply] = results.each_ref().map(|r| px(r, x, y)[3]);
                    let expected = if invert { 255 - x as u8 } else { x as u8 };
                    assert_eq!(replace, expected);
                    assert_eq!(minimum, (y as u8).min(expected));
                    assert!(multiply <= minimum);
                    if y == 0 || expected == 0 {
                        assert_eq!(multiply, 0);
                    }
                    if y == 255 {
                        assert_eq!(multiply, expected);
                    }
                    if x > 0 {
                        let before = px(&results[2], x - 1, y)[3];
                        assert!(if invert {
                            multiply <= before
                        } else {
                            multiply >= before
                        });
                    }
                }
            }
        }
    }

    /// object-switch-gray-to-alpha.md Behaviour: monotonic coverage and complementary modes.
    #[test]
    fn gray_to_alpha_monotonic_complementary() {
        let src = from_fn(256, 1, |x, _| [x as u8, x as u8, x as u8, 255]);
        for context in [LINEAR, ALT_GPU_CONTEXT] {
            let white = bb_ctx("bbSwitchGrayToAlpha", context, json!({}), &src);
            let black = bb_ctx(
                "bbSwitchGrayToAlpha",
                context,
                json!({"mode":"black"}),
                &src,
            );
            for x in 0..256 {
                let (w, b) = (px(&white, x, 0)[3], px(&black, x, 0)[3]);
                assert!((w as i16 + b as i16 - 255).abs() <= 1);
                if x > 0 {
                    assert!(w <= px(&white, x - 1, 0)[3]);
                    assert!(b >= px(&black, x - 1, 0)[3]);
                }
            }
        }
    }

    /// common.md §2: Apply Mask and Gray to Alpha preserve pixels outside the selection.
    #[test]
    fn selection_clips_other_channel_effects() {
        let src = solid(3, 2, [120, 99, 201, 81]);
        let (mask, polys) = rect_selection(3, 2, 1, 0, 2, 1);
        let mut env = with_selection(3, 2, &mask, &polys);
        let ls = layers(&[src.clone(), solid(3, 2, [255; 4])]);
        env.layers = &ls;
        for id in ["bbApplyMask", "bbSwitchGrayToAlpha"] {
            let out = bb_env(id, json!({}), &src, &env);
            assert_ne!(px(&out, 1, 0), px(&src, 1, 0));
            for y in 0..2 {
                for x in 0..3 {
                    if (x, y) != (1, 0) {
                        assert_eq!(px(&out, x, y), px(&src, x, y));
                    }
                }
            }
        }
    }

    /// R88: Gray to Alpha normalizes actual input/output modes, including alpha-zero straight input.
    #[test]
    fn effective_straight_and_mixed_contexts() {
        let src = pair([128, 128, 128, 64], [255, 255, 255, 0]);
        for input in [Alpha::Straight, Alpha::Premultiplied] {
            for output in [Alpha::Straight, Alpha::Premultiplied] {
                let info = RenderInfo {
                    space: Space::Srgb,
                    input,
                    output,
                };
                let out = bb_ctx("bbSwitchGrayToAlpha", info, json!({}), &src);
                assert_px(&out, 0, 0, [0, 0, 0, 127]);
                assert_px(&out, 1, 0, [0, 0, 0, 255]);
                let out = bb_ctx("bbSwitchGrayToAlpha", info, json!({"mode":"black"}), &src);
                assert_px(&out, 0, 0, [255, 255, 255, 128]);
                assert_eq!(px(&out, 1, 0)[3], 0);
                let out = bb_ctx("bbSwitchAlphaToGray", info, json!({}), &src);
                assert_px(&out, 0, 0, [64, 64, 64, 255]);
                assert_px(&out, 1, 0, [0, 0, 0, 255]);
                let out = bb_ctx("bbApplyMask", info, json!({"maskType":"alpha"}), &src);
                assert_px(&out, 0, 0, [128, 128, 128, 64]);
            }
        }
    }

    /// All three specs Controls, metadata and registration; common.md §3, §4.
    #[test]
    fn registered_dialog_contracts() {
        for (id, name, title, icon, keys, context) in [
            (
                "bbApplyMask",
                "Apply Mask",
                "Apply Alpha Mask",
                "apply_mask.png",
                vec!["current", "mask", "maskType", "method", "invert"],
                RenderInfo::new(Space::Srgb, Alpha::Straight),
            ),
            (
                "bbSwitchAlphaToGray",
                "Switch Alpha to Gray",
                "Switch Alpha to Gray",
                "alpha2_gray.png",
                vec!["info", "layer", "invert"],
                GPU_CONTEXT,
            ),
            (
                "bbSwitchGrayToAlpha",
                "Switch Gray to Alpha",
                "Switch Gray to Alpha",
                "gray2_alpha.png",
                vec!["mode"],
                GPU_CONTEXT,
            ),
        ] {
            let def = find(id).unwrap();
            assert_eq!(def.display_name(), name);
            assert_eq!(def.category, Category::Effect("object"));
            assert_eq!(def.dialog_title, Some(title));
            assert_eq!(def.icon, icon);
            assert!(def.dialog.is_none());
            let EffectKind::Gpu(info, _) = def.kind else {
                panic!("GPU effect expected")
            };
            assert_eq!(info, context);
            assert_eq!(
                def.controls.iter().map(Control::key).collect::<Vec<_>>(),
                keys
            );
            let ui = def.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
            let defaults = indirect_defaults(&def, &EffectEnv::whole(1, 1));
            for key in keys {
                assert!(!ui.control(key).header_line);
                assert!((ui.enabled)(&defaults, key));
            }
        }
        let apply = find("bbApplyMask").unwrap();
        assert!(matches!(
            apply.controls[0],
            Control::Label {
                label: "",
                text: LabelText::CurrentLayer("Current layer    "),
                ..
            }
        ));
        assert!(matches!(
            apply.controls[1],
            Control::LayerSelect { label: "", .. }
        ));
        let ui = apply.indirect.unwrap();
        for (key, caption) in [
            ("mask", format!("Mask layer{}", " ".repeat(5))),
            ("maskType", format!("Mask type{}", " ".repeat(6))),
            ("method", format!("Method{}", " ".repeat(10))),
        ] {
            assert_eq!(ui.control(key).caption, caption);
        }
        assert!(
            matches!(&apply.controls[2],Control::Select {label:"",default:"gray",choices,..} if *choices==[("gray","Shades of gray"),("alpha","Alpha")])
        );
        assert!(
            matches!(&apply.controls[3],Control::Select {label:"",default:"replace",choices,..} if *choices==[("replace","Replace"),("minimum","Minimum"),("multiply","Multiply")])
        );
        assert!(matches!(
            apply.controls[4],
            Control::Checkbox {
                label: "Invert mask",
                default: false,
                ..
            }
        ));
        let gray = find("bbSwitchAlphaToGray").unwrap();
        assert!(matches!(
            gray.controls[0],
            Control::Label {
                label: "Create an alpha mask by converting alpha to shades of gray.\n ",
                text: LabelText::CurrentLayer("Current layer     "),
                ..
            }
        ));
        assert!(matches!(
            gray.controls[1],
            Control::LayerSelect { label: "", .. }
        ));
        assert!(matches!(
            gray.controls[2],
            Control::Checkbox {
                label: "Invert mask",
                default: false,
                ..
            }
        ));
        assert_eq!(
            gray.indirect.unwrap().control("layer").caption,
            format!("Mask of layer{}", " ".repeat(2))
        );
        let alpha = find("bbSwitchGrayToAlpha").unwrap();
        assert!(
            matches!(&alpha.controls[0],Control::Select {label:"Isolate line art by deleting",default:"white",choices,..} if *choices==[("white","White"),("black","Black")])
        );
        let defaults = indirect_defaults(&apply, &EffectEnv::whole(1, 1));
        assert_eq!(defaults["mask"], json!(0));
        assert_eq!(defaults["maskType"], json!("gray"));
        assert_eq!(defaults["method"], json!("replace"));
        assert_eq!(defaults["invert"], json!(false));
    }

    /// Layer-reading specs: actual sessions keep raw bytes, duplicate names and bottom-first indices.
    #[test]
    fn actual_session_keeps_raw_layers_and_dynamic_defaults() {
        let mut app = crate::testing::fresh_doc(2, 1, Some(4));
        crate::action::perform(&mut app, "menu.layers.add.new.layer");
        let ws = app.active_ws_mut().unwrap();
        for i in 0..2 {
            ws.document.layers.get_at_mut(i).unwrap().properties.name = "Same".into();
        }
        // Populate the mask's raw alpha independently of display properties.
        let mask = ws.document.layers.get_at_mut(1).unwrap();
        mask.surface
            .data_mut()
            .copy_from_slice(&[100, 100, 100, 32, 100, 100, 100, 191]);
        mask.properties.visible = false;
        mask.properties.opacity = 0;
        mask.properties.blend_mode = crate::document::layer::BlendMode::Multiply;
        ws.active_layer = 0;
        for id in ["bbApplyMask", "bbSwitchAlphaToGray"] {
            let session = EffectSession::begin(&mut app, find(id).unwrap()).unwrap();
            assert_eq!(session.layer_names(), vec!["Same", "Same"]);
            assert_eq!(
                session.env().layers[1].pixels.data,
                [100, 100, 100, 32, 100, 100, 100, 191]
            );
            let defaults = session.defaults();
            let key = if id == "bbApplyMask" { "mask" } else { "layer" };
            assert_eq!(
                defaults[key],
                json!(if id == "bbApplyMask" { 1 } else { 0 })
            );
            let mut values = defaults;
            values.insert(key.into(), json!(1));
            let out = session.render(&values);
            if id == "bbApplyMask" {
                for x in 0..2 {
                    assert_eq!(px(&out, x, 0)[3], 100);
                }
            } else {
                assert_px(&out, 0, 0, [99, 99, 99, 255]);
                assert_px(&out, 1, 0, [224, 224, 224, 255]);
            }
        }
        app.active_ws_mut().unwrap().active_layer = 1;
        let session = EffectSession::begin(&mut app, find("bbSwitchAlphaToGray").unwrap()).unwrap();
        assert_eq!(session.defaults()["layer"], json!(1));
    }
}
