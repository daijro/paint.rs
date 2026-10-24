//! BB spec: effects/photo-combined-photo-effects.md; common.md §1–5

use super::blocks::{bc_shift, blur_soft, hsl_adjust, temp_tint_linear};
use super::conventions::{GPU_CONTEXT, RADIUS_TO_SIGMA};
use super::manifest::manifest;
use super::ui::{ControlUi, IndirectUi, TRACK_BLACK_WHITE, TRACK_CONTRAST, TRACK_SATURATION};
use crate::effect::mix::{MixMode, mix};
use crate::effect::surface::{Alpha, EffectSurface, Space, premultiply, unpremultiply};
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, flag, num, range};
use crate::pixel::gamma::{linear_to_srgb_f32, srgb_to_linear_f32};

const fn slider_ui(track: Option<&'static [u32]>) -> ControlUi {
    ControlUi {
        track,
        decimals: Some(0),
        ..ControlUi::NO_HEADER
    }
}
static PHOTO_UI: IndirectUi = IndirectUi {
    controls: &[
        ("shadowRecovery", slider_ui(None)),
        ("brightness", slider_ui(Some(&TRACK_BLACK_WHITE))),
        ("contrast", slider_ui(Some(&TRACK_CONTRAST))),
        // approximation: track endpoint alpha ignored (photo-combined-photo-effects.md control 4)
        (
            "temperature",
            slider_ui(Some(&[0x98BAFF, 0xFFFFFF, 0xFF890E])),
        ),
        ("tint", slider_ui(Some(&[0xFF00FF, 0xFFFFFF, 0x008000]))),
        ("saturation", slider_ui(Some(&TRACK_SATURATION))),
        ("dynamicRange", slider_ui(None)),
        ("previewOriginal", ControlUi::NO_HEADER),
    ],
    enabled: |v, key| key == "previewOriginal" || !flag(v, "previewOriginal"),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let m = manifest("bbCombinedPhotoEffects").unwrap();
    vec![EffectDef {
        id: m.id,
        icon: m.icon,
        category: m.category,
        controls: vec![
            range("shadowRecovery", "Shadow recovery", 0.0, 200.0, 100.0)
                .slider_keys(0, 1.0, 5.0, false),
            range("brightness", "Brightness / Contrast", -100.0, 100.0, 0.0)
                .slider_keys(0, 1.0, 5.0, false),
            range("contrast", "", -100.0, 100.0, 0.0).slider_keys(0, 1.0, 5.0, false),
            range("temperature", "Temperature / Tint", -100.0, 100.0, 0.0)
                .slider_keys(0, 1.0, 5.0, false),
            range("tint", "", -100.0, 100.0, 0.0).slider_keys(0, 1.0, 5.0, false),
            range(
                "saturation",
                "Saturation / Dynamic range",
                -100.0,
                100.0,
                0.0,
            )
            .slider_keys(0, 1.0, 5.0, false),
            range("dynamicRange", "", 0.0, 100.0, 0.0).slider_keys(0, 1.0, 5.0, false),
            Control::Checkbox {
                key: "previewOriginal",
                label: "Preview original image",
                default: false,
            },
        ],
        kind: EffectKind::Gpu(GPU_CONTEXT, render),
        dialog: None,
        dialog_title: Some("Photo Effects"),
        indirect: Some(&PHOTO_UI),
    }]
}

fn render(src: &EffectSurface, values: &Values, env: &EffectEnv) -> EffectSurface {
    if flag(values, "previewOriginal") {
        return src.clone();
    }
    let info = env.render_info.expect("GPU render context");
    // The blur and Blend blocks consume premultiplied colour in the effective working space.
    let src = EffectSurface::from_fn(src.w, src.h, |x, y| match info.input {
        Alpha::Straight => premultiply(src.at(x, y)),
        Alpha::Premultiplied => src.at(x, y),
    });
    // approximation: raw optimisation 3 uses the shared highest-quality blur (photo-combined-photo-effects.md step 1)
    let blurred = blur_soft(&src, 12.0 * RADIUS_TO_SIGMA);
    let [
        recovery,
        brightness,
        contrast,
        temperature,
        tint,
        saturation,
        dynamic,
    ] = [
        "shadowRecovery",
        "brightness",
        "contrast",
        "temperature",
        "tint",
        "saturation",
        "dynamicRange",
    ]
    .map(|key| num(values, key) as f32);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = unpremultiply(src.at(x, y));
        if p[3] == 0.0 {
            return [0.0; 4];
        }
        let q = unpremultiply(blurred.at(x, y));
        let mean = (q[0] + q[1] + q[2]) / 3.0;
        let rgb = [p[0], p[1], p[2]];
        // The spec explicitly treats a zero blurred mean as unchanged black at this stage.
        let [r, g, b] = if mean == 0.0 {
            rgb
        } else {
            shadow_highlight(rgb.map(|c| c * 255.0), mean * 255.0, recovery).map(|c| c / 255.0)
        };
        let u = unpremultiply(premultiply([r, g, b, p[3]]));
        let mut rgb = bc_shift([u[0], u[1], u[2]], brightness, contrast);
        if temperature != 0.0 || tint != 0.0 {
            let linear = if info.space == Space::Srgb {
                rgb.map(srgb_to_linear_f32)
            } else {
                rgb
            };
            rgb = temp_tint_linear(linear, temperature / 100.0, tint / 100.0);
            if info.space == Space::Srgb {
                rgb = rgb.map(linear_to_srgb_f32);
            }
        }
        let [r, g, b] = hsl_adjust(rgb, 0.0, 100.0 + saturation, 0.0);
        let p = premultiply([r, g, b, p[3]]);
        let p = if dynamic > 0.0 {
            mix(MixMode::Multiply, p, p.map(|c| c * (dynamic / 100.0)))
        } else {
            p
        };
        match info.output {
            Alpha::Straight => unpremultiply(p),
            Alpha::Premultiplied => p,
        }
    })
}

/// Shadow/highlight shader on straight 0–255 working-space RGB and blurred mean.
pub fn shadow_highlight(c: [f32; 3], blur_mean: f32, recovery: f32) -> [f32; 3] {
    let m = (c[0] + c[1] + c[2]) / 3.0;
    let v = (m + blur_mean) / 2.0;
    let shadow = (50.0 / (v + 10.0) - 0.24).clamp(0.0, 17.0 / 6.0);
    let highlight = (50.0 / (265.0 - v) - 0.9).clamp(0.0, 17.0 / 6.0);
    c.map(|channel| {
        let s = if recovery > 0.0 {
            channel * (1.0 + shadow * recovery / 100.0)
        } else {
            // quirk(bb): photo-combined-photo-effects.md "Shadow recovery = 0"
            channel + shadow * (channel - m)
        };
        let d = 0.2 * highlight * (m - s);
        if m - d == 0.0 {
            s
        } else {
            s * (m - 2.0 * d) / (m - d)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::shadow_highlight;
    use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
    use crate::effect::bb::indirect_defaults;
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::{
        Help, HelpWindow, Note, TRACK_BLACK_WHITE, TRACK_CONTRAST, TRACK_SATURATION,
    };
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::{Category, Control, EffectEnv, EffectKind, RenderInfo};
    use serde_json::json;

    const ID: &str = "bbCombinedPhotoEffects";
    fn contexts() -> [RenderInfo; 2] {
        [GPU_CONTEXT, ALT_GPU_CONTEXT]
    }
    #[track_caller]
    fn close3(got: [f32; 3], want: [f32; 3]) {
        assert!(
            got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 0.15),
            "{got:?} != {want:?}"
        );
    }

    /// photo-combined-photo-effects.md Sample values: all three columns and the >=199 tail.
    #[test]
    fn shadow_highlight_complete_table() {
        for (g, r0, r100, r200) in [
            (5., 5., 19.2, 33.3),
            (10., 10., 32.6, 55.2),
            (20., 20., 48.5, 77.1),
            (32., 32., 62.4, 92.8),
            (64., 64., 91.9, 119.8),
            (100., 100., 121.5, 142.9),
            (128., 128., 143.7, 159.3),
            (160., 160., 168.7, 177.3),
            (198., 198., 198.1, 198.2),
        ] {
            for (a, want) in [(0., r0), (100., r100), (200., r200)] {
                close3(shadow_highlight([g; 3], g, a), [want; 3]);
            }
        }
        for g in 199..=255 {
            for a in [0., 100., 200.] {
                close3(shadow_highlight([g as f32; 3], g as f32, a), [g as f32; 3]);
            }
        }
    }
    /// photo-combined-photo-effects.md Sample values: Recovery=0 colour quirk and slider-independent highlights.
    #[test]
    fn shadow_highlight_colour_samples() {
        close3(
            shadow_highlight([200., 100., 50.], 116.67, 100.),
            [230.9, 115.5, 57.7],
        );
        close3(
            shadow_highlight([200., 100., 50.], 116.67, 0.),
            [212.9, 97.4, 39.7],
        );
        for a in [0., 1., 100., 200.] {
            close3(
                shadow_highlight([240., 230., 200.], 223.33, a),
                [241.1, 230.4, 198.7],
            );
        }
    }
    /// photo-combined-photo-effects.md Behaviour 2: capped gain, preserved ratios, local tone and unclamped output.
    #[test]
    fn shadow_weight_and_unclamped_properties() {
        for recovery in [1., 100., 200.] {
            let out = shadow_highlight([1., 2., 3.], 2., recovery);
            assert!((out[1] / out[0] - 2.).abs() < 1e-5 && (out[2] / out[0] - 3.).abs() < 1e-5);
            let gain = if recovery == 100. {
                23. / 6.
            } else if recovery == 200. {
                20. / 3.
            } else {
                1. + 17. / 600.
            };
            assert!((out[0] - gain).abs() < 1e-5);
        }
        for g in 1..199 {
            let g = g as f32;
            let a = shadow_highlight([g; 3], g, 1.)[0];
            let b = shadow_highlight([g; 3], g, 100.)[0];
            let c = shadow_highlight([g; 3], g, 200.)[0];
            assert!(g <= a && a <= b && b <= c);
        }
        assert!(
            shadow_highlight([32.; 3], 0., 100.)[0] > shadow_highlight([32.; 3], 200., 100.)[0]
        );
        assert!(shadow_highlight([0., 0., 30.], 10., 0.)[0] < 0.);
        assert!(shadow_highlight([255., 0., 0.], 85., 200.)[0] > 255.);
        assert_eq!(shadow_highlight([0.; 3], 0., 100.), [0.; 3]);
    }
    /// photo-combined-photo-effects.md Expected results 1; common.md §1 and native §2.4 conversions stay active.
    #[test]
    fn preview_bypass_all_effective_contexts() {
        let img = from_fn(2, 1, |x, _| [64, 128, 192, if x == 0 { 128 } else { 0 }]);
        for space in [Space::Srgb, Space::Linear] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = RenderInfo {
                        space,
                        input,
                        output,
                    };
                    let out = bb_ctx(
                        ID,
                        info,
                        json!({"previewOriginal":true,"shadowRecovery":200,"brightness":100,"contrast":100,"temperature":100,"tint":100,"saturation":-100,"dynamicRange":100}),
                        &img,
                    );
                    let rgb = match (input, output, space) {
                        (Alpha::Straight, Alpha::Premultiplied, Space::Srgb) => [128, 255, 255],
                        (Alpha::Straight, Alpha::Premultiplied, Space::Linear) => [90, 175, 255],
                        (Alpha::Premultiplied, Alpha::Straight, Space::Srgb) => [32, 64, 96],
                        (Alpha::Premultiplied, Alpha::Straight, Space::Linear) => [45, 93, 141],
                        _ => [64, 128, 192],
                    };
                    assert_px(&out, 0, 0, [rgb[0], rgb[1], rgb[2], 128]);
                    assert_eq!(
                        px(&out, 1, 0),
                        if input == Alpha::Straight && output == Alpha::Straight {
                            [64, 128, 192, 0]
                        } else {
                            [0; 4]
                        }
                    );
                }
            }
        }
    }
    /// photo-combined-photo-effects.md Expected results 2: complete gamma/linear columns and canvas edges.
    #[test]
    fn default_shadow_lift() {
        for (info, want) in [(GPU_CONTEXT, 156), (ALT_GPU_CONTEXT, 144)] {
            let out = bb_ctx(ID, info, json!({}), &solid(16, 16, [128, 128, 128, 255]));
            for y in 0..16 {
                for x in 0..16 {
                    assert_px(&out, x, y, [want, want, want, 255]);
                }
            }
        }
    }
    /// photo-combined-photo-effects.md Expected results 3: grey identity in both spaces, coloured gamma saturation quirk.
    #[test]
    fn zero_recovery_greys_and_colours() {
        for info in contexts() {
            assert_px(
                &bb_ctx(
                    ID,
                    info,
                    json!({"shadowRecovery":0}),
                    &solid(16, 16, [32, 32, 32, 255]),
                ),
                8,
                8,
                [32, 32, 32, 255],
            );
        }
        assert_px(
            &bb_ctx(
                ID,
                ALT_GPU_CONTEXT,
                json!({"shadowRecovery":0}),
                &solid(16, 16, [200, 100, 50, 255]),
            ),
            8,
            8,
            [213, 97, 40, 255],
        );
    }
    /// photo-combined-photo-effects.md Expected results 4: every dynamic-range context-table entry.
    #[test]
    fn dynamic_range_squares_and_half_mix() {
        for (info, k, want) in [
            (GPU_CONTEXT, 100, 61),
            (ALT_GPU_CONTEXT, 100, 64),
            (GPU_CONTEXT, 50, 101),
            (ALT_GPU_CONTEXT, 50, 96),
        ] {
            assert_px(
                &bb_ctx(
                    ID,
                    info,
                    json!({"shadowRecovery":0,"dynamicRange":k}),
                    &solid(16, 16, [128, 128, 128, 255]),
                ),
                8,
                8,
                [want, want, want, 255],
            );
        }
    }
    /// photo-combined-photo-effects.md Expected results 5 and Behaviour 2 edge case: black/white fixed, transparent stays clear.
    #[test]
    fn endpoints_and_zero_blurred_mean() {
        for info in contexts() {
            for p in [[255; 4], [0, 0, 0, 255], [91, 35, 208, 0]] {
                let out = bb_ctx(ID, info, json!({}), &solid(16, 16, p));
                assert!(
                    out.data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|q| *q == if p[3] == 0 { [0; 4] } else { p })
                );
            }
            let want = if info.space == Space::Srgb { 50 } else { 122 };
            assert_px(
                &bb_ctx(
                    ID,
                    info,
                    json!({"brightness":50}),
                    &solid(16, 16, [0, 0, 0, 255]),
                ),
                8,
                8,
                [want, want, want, 255],
            );
            assert!(
                bb_ctx(
                    ID,
                    info,
                    json!({"brightness":100,"dynamicRange":100}),
                    &solid(16, 16, [91, 35, 208, 0])
                )
                .data
                .iter()
                .all(|c| *c == 0)
            );
        }
    }
    /// photo-combined-photo-effects.md Behaviour 7: scale every premultiplied channel and source-over alpha union.
    #[test]
    fn dynamic_range_alpha_and_colour_table() {
        for (space, rows) in [
            (
                Space::Srgb,
                [
                    (0, [0, 0, 0]),
                    (64, [128, 122, 119]),
                    (128, [128, 115, 107]),
                    (255, [128, 96, 64]),
                ],
            ),
            (
                Space::Linear,
                [
                    (0, [0, 0, 0]),
                    (64, [128, 124, 121]),
                    (128, [128, 118, 111]),
                    (255, [128, 101, 61]),
                ],
            ),
        ] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    for (alpha, values) in rows {
                        let want_alphas = match alpha {
                            0 => [0, 0, 0],
                            64 => [64, 88, 112],
                            128 => [128, 160, 192],
                            _ => [255; 3],
                        };
                        for (i, k) in [0, 50, 100].into_iter().enumerate() {
                            let out = bb_ctx(
                                ID,
                                RenderInfo {
                                    space,
                                    input,
                                    output,
                                },
                                json!({"shadowRecovery":0,"dynamicRange":k}),
                                &solid(5, 5, [128, 128, 128, alpha]),
                            );
                            assert_px(
                                &out,
                                2,
                                2,
                                [values[i], values[i], values[i], want_alphas[i]],
                            );
                        }
                    }
                }
            }
        }
    }
    /// photo-combined-photo-effects.md Behaviour 4–7: B/C precedes Temperature/Tint, saturation follows it, Multiply is last.
    #[test]
    fn noncommuting_pipeline_order() {
        for (space, opaque, half) in [(Space::Srgb, 133, 150), (Space::Linear, 111, 128)] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    for (alpha, want, want_alpha) in [(255, opaque, 255), (128, half, 160)] {
                        let out = bb_ctx(
                            ID,
                            RenderInfo {
                                space,
                                input,
                                output,
                            },
                            json!({"shadowRecovery":0,"brightness":20,"contrast":25,"temperature":75,"tint":-50,"saturation":-100,"dynamicRange":50}),
                            &solid(5, 5, [128, 128, 128, alpha]),
                        );
                        assert_px(&out, 2, 2, [want, want, want, want_alpha]);
                    }
                }
            }
        }
    }
    /// photo-combined-photo-effects.md Behaviour 2–7: shadow recovery precedes brightness and final Multiply.
    #[test]
    fn shadow_precedes_brightness() {
        for (info, want) in [(GPU_CONTEXT, 146), (ALT_GPU_CONTEXT, 134)] {
            assert_px(
                &bb_ctx(
                    ID,
                    info,
                    json!({"brightness":20,"dynamicRange":50}),
                    &solid(9, 9, [128, 128, 128, 255]),
                ),
                4,
                4,
                [want, want, want, 255],
            );
        }
    }
    /// photo-combined-photo-effects.md Behaviour 1–2: unpremultiplying blur cancels colour fading at canvas/alpha edges.
    #[test]
    fn blur_uses_alpha_weighted_local_colour() {
        for info in contexts() {
            let reference = bb_ctx(ID, info, json!({}), &solid(17, 9, [80, 40, 20, 255]));
            let want = px(&reference, 8, 4);
            let src = from_fn(17, 9, |x, y| {
                [
                    80,
                    40,
                    20,
                    if x < 8 && y < 5 {
                        128
                    } else if x == 12 {
                        64
                    } else {
                        0
                    },
                ]
            });
            let out = bb_ctx(ID, info, json!({}), &src);
            for y in 0..9 {
                for x in 0..17 {
                    let a = px(&src, x, y)[3];
                    assert_px(
                        &out,
                        x,
                        y,
                        if a == 0 {
                            [0; 4]
                        } else {
                            [want[0], want[1], want[2], a]
                        },
                    );
                }
            }
        }
    }
    /// photo-combined-photo-effects.md Behaviour selection: unselected neighbours influence local tone; raw outside pixels remain unchanged.
    #[test]
    fn selection_reads_unselected_neighbours_and_clips() {
        let src = from_fn(33, 9, |x, _| {
            if x < 16 {
                [32, 32, 32, 255]
            } else if x == 32 {
                [77, 123, 91, 0]
            } else {
                [255; 4]
            }
        });
        let (mask, polys) = rect_selection(33, 9, 15, 0, 16, 9);
        let env = with_selection(33, 9, &mask, &polys);
        let out = bb_env(ID, json!({}), &src, &env);
        let full = bb(ID, json!({}), &src);
        let dark = bb(ID, json!({}), &solid(33, 9, [32, 32, 32, 255]));
        assert!(px(&out, 15, 4)[0] < px(&dark, 15, 4)[0]);
        for y in 0..9 {
            for x in 0..33 {
                assert_eq!(
                    px(&out, x, y),
                    if x == 15 {
                        px(&full, x, y)
                    } else {
                        px(&src, x, y)
                    }
                );
            }
        }
    }
    /// photo-combined-photo-effects.md Controls table and common.md §3: complete dialog contract.
    #[test]
    fn dialog_metadata() {
        let d = crate::effect::find(ID).unwrap();
        assert_eq!(d.display_name(), "Combined Photo Effects");
        assert_eq!(d.category, Category::Effect("photo"));
        assert_eq!(d.icon, "photo_adjustments.png");
        assert!(d.is_configurable());
        assert_eq!(d.dialog_title, Some("Photo Effects"));
        assert!(d.dialog.is_none());
        assert!(matches!(d.kind,EffectKind::Gpu(info,_) if info==GPU_CONTEXT));
        let ui = d.indirect.unwrap();
        assert!(ui.tabs.is_empty());
        assert_eq!(ui.width, 1.);
        assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
        let expected = [
            ("shadowRecovery", "Shadow recovery", 0., 200., 100., None),
            (
                "brightness",
                "Brightness / Contrast",
                -100.,
                100.,
                0.,
                Some(TRACK_BLACK_WHITE.as_slice()),
            ),
            (
                "contrast",
                "",
                -100.,
                100.,
                0.,
                Some(TRACK_CONTRAST.as_slice()),
            ),
            (
                "temperature",
                "Temperature / Tint",
                -100.,
                100.,
                0.,
                Some([0x98BAFF, 0xFFFFFF, 0xFF890E].as_slice()),
            ),
            (
                "tint",
                "",
                -100.,
                100.,
                0.,
                Some([0xFF00FF, 0xFFFFFF, 0x008000].as_slice()),
            ),
            (
                "saturation",
                "Saturation / Dynamic range",
                -100.,
                100.,
                0.,
                Some(TRACK_SATURATION.as_slice()),
            ),
            ("dynamicRange", "", 0., 100., 0., None),
        ];
        assert_eq!(d.controls.len(), 8);
        for (c, (want_key, want_label, want_min, want_max, want_default, want_track)) in
            d.controls.iter().zip(expected)
        {
            let Control::Range {
                key,
                label,
                min,
                max,
                default,
                step,
                keyboard,
                exponential,
                ..
            } = c
            else {
                panic!("expected integer slider")
            };
            assert_eq!(
                (*key, *label, *min, *max, *default),
                (want_key, want_label, want_min, want_max, want_default)
            );
            assert_eq!(
                (
                    *step,
                    *exponential,
                    keyboard.decimals,
                    keyboard.small,
                    keyboard.large,
                    keyboard.exponential
                ),
                (1., false, 0, 1., 5., false)
            );
            assert_eq!(ui.control(key).track, want_track);
            assert_eq!(ui.control(key).decimals, Some(0));
        }
        assert!(matches!(
            d.controls[7],
            Control::Checkbox {
                key: "previewOriginal",
                label: "Preview original image",
                default: false
            }
        ));
        for preview in [false, true] {
            let mut v = indirect_defaults(&d, &EffectEnv::whole(20, 10));
            v.insert("previewOriginal".into(), json!(preview));
            for c in &d.controls {
                let u = ui.control(c.key());
                assert!(!u.header_line && u.reset && !u.alpha && !u.wrap);
                assert!(matches!(u.note, Note::None));
                assert!(u.caption.is_empty());
                assert_eq!(
                    (ui.enabled)(&v, c.key()),
                    !preview || c.key() == "previewOriginal"
                );
            }
        }
    }
}
