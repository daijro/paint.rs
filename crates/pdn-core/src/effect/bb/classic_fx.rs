//! BB spec: effects/photo-sharpen-classic.md; effects/photo-sharpen-landscape.md;
//! effects/stylize-edge-detect-classic.md; common.md §1–5.

use super::classic::{gaussian_blur, hue_saturation, median};
use super::manifest::manifest;
use super::pan;
use super::ui::{ControlUi, Help, IndirectUi, TRACK_SATURATION, TRACK_WHITE_BLACK};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::effect::slider::SliderKeys;
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, num, range};
use crate::pixel::ops::compose;
use crate::render::display::for_bands;

static SHARPEN_UI: IndirectUi = IndirectUi {
    controls: &[("amount", ControlUi::NO_HEADER)],
    help: Help::Text(&[
        "Sharpen Classic v1.0",
        "Sharpen algorithm Copyright ©2013 by Rick Brewster",
        "All rights reserved.",
    ]),
    ..IndirectUi::DEFAULT
};
static LANDSCAPE_UI: IndirectUi = IndirectUi {
    controls: &[
        (
            "horizon",
            ControlUi {
                decimals: Some(3),
                pan_sliders: Some(SliderKeys {
                    decimals: 3,
                    small: 0.05,
                    large: 0.25,
                    exponential: false,
                }),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "sky",
            ControlUi {
                track: Some(&[0xFFFFFF, 0x0000FF]),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "grass",
            ControlUi {
                track: Some(&[0xFFFFFF, 0x008000]),
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "saturation",
            ControlUi {
                track: Some(&TRACK_SATURATION),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "dynamicRange",
            ControlUi {
                track: Some(&TRACK_WHITE_BLACK),
                ..ControlUi::DEFAULT
            },
        ),
    ],
    ..IndirectUi::DEFAULT
};
static EDGE_UI: IndirectUi = IndirectUi {
    controls: &[(
        "angle",
        ControlUi {
            decimals: Some(3),
            wrap: true,
            ..ControlUi::NO_HEADER
        },
    )],
    help: Help::Text(&[
        "Edge Detect Classic v1.0",
        "Algorithm ©2004 by Chris Crosetto",
        "All rights reserved.",
    ]),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let make = |id, controls, render, title, ui| {
        let m = manifest(id).unwrap();
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
    vec![
        make(
            "bbSharpenClassic",
            vec![range("amount", "Sharpen Amount", 1.0, 20.0, 2.0)],
            sharpen,
            "Sharpen Classic",
            &SHARPEN_UI,
        ),
        make(
            "bbSharpenLandscape",
            vec![
                Control::Pan {
                    key: "horizon",
                    label: "Position of Horizon",
                    min: -1.0,
                    max: 1.0,
                    default: [0.0, -0.25],
                    step: 0.01,
                    thumb: PanThumb::SelectionCrop,
                },
                range("sky", "Sky / Grass", 0.0, 100.0, 50.0),
                range("grass", "", 0.0, 100.0, 50.0),
                range("sharpen", "Sharpen", 0.0, 200.0, 100.0),
                range("saturation", "Saturation", 0.0, 100.0, 50.0),
                range("dynamicRange", "Dynamic Range", 0.0, 100.0, 50.0),
            ],
            landscape,
            "Sharpen Landscape",
            &LANDSCAPE_UI,
        ),
        // R107 native numeric increment1 is independent of p3; R14 slider keys are port policy.
        make(
            "bbEdgeDetectClassic",
            vec![
                range("angle", "Edge Detect Angle", -180.0, 180.0, 45.0)
                    .slider_keys(3, 1.0, 5.0, false),
            ],
            edge_detect,
            "Edge Detect Classic",
            &EDGE_UI,
        ),
    ]
}

fn sharpen(src: &ImageData, values: &Values, _: &EffectEnv) -> ImageData {
    // spec-question: Q11 Legacy Sharpen's median uses the shared +1 convention.
    let mut out = median(src, num(values, "amount").clamp(1.0, 20.0) as u32, 50);
    for (m, &p) in out.data.iter_mut().zip(&src.data) {
        // approximation: inferred clamp then truncate, including alpha (photo-sharpen-classic.md step 3).
        *m = (p as f64 + 0.5 * (p as f64 - *m as f64)).clamp(0.0, 255.0) as u8;
    }
    out
}

fn landscape(src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    // quirk(bb): photo-sharpen-landscape.md step 1 compares h to absolute y, without bounds.y.
    let h = ((pan(values, "horizon")[1].clamp(-1.0, 1.0) + 1.0) / 2.0 * env.bounds.h as f64)
        .round_ties_even();
    let bottom = env.bounds.bottom() as f64;
    let sky = num(values, "sky").clamp(0.0, 100.0);
    let grass = num(values, "grass").clamp(0.0, 100.0);
    let amount = num(values, "sharpen").clamp(0.0, 200.0);
    let saturation = 100 + 3 * num(values, "saturation").clamp(0.0, 100.0) as i32;
    let dynamic = (num(values, "dynamicRange").clamp(0.0, 100.0) * 255.0 / 100.0) as u8;
    let mut out = gaussian_blur(src, 1);
    for_bands(
        &mut out.data,
        src.width as usize * 4,
        0..src.height as usize,
        1,
        |y, row| {
            let y = y as f64;
            let sky = if y < h {
                ((255.0 - 255.0 * y / h) * sky / 100.0) as u8
            } else {
                0
            };
            let grass = if y > h && bottom > h {
                (255.0 * (y - h) / (bottom - h) * grass / 100.0) as u8
            } else {
                0
            };
            let first = y as usize * src.width as usize * 4;
            for (blur, source) in row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.data[first..].as_chunks::<4>().0)
            {
                let rgb = std::array::from_fn::<_, 3, _>(|c| {
                    (source[c] as f64 + (source[c] as f64 - blur[c] as f64) * amount / 100.0)
                        .clamp(0.0, 255.0) as u8
                });
                // quirk(bb): photo-sharpen-landscape.md step 3 retains blurred alpha through unsharp.
                let p = hue_saturation([rgb[0], rgb[1], rgb[2], blur[3]], saturation);
                // quirk(bb): photo-sharpen-landscape.md Transparent pixels gain overlay colour and alpha.
                let p = compose(BlendMode::Multiply, p, [0, 128, 255, sky], 255);
                let p = compose(BlendMode::ColorDodge, p, [0, 128, 0, grass], 255);
                *blur = compose(BlendMode::Multiply, p, [p[0], p[1], p[2], dynamic], 255);
            }
        },
    );
    out
}

/// stylize-edge-detect-classic.md sample-table layout: top-to-bottom rows, left-to-right columns.
pub fn edge_weights(angle_deg: f64) -> [[f64; 3]; 3] {
    // Equivalent endpoints must reach identical cosine operands before byte truncation.
    let d = (angle_deg + 180.0).rem_euclid(360.0);
    let mut weights = [
        [135.0_f64, 90.0, 45.0],
        [180.0, 0.0, 0.0],
        [225.0, 270.0, 315.0],
    ]
    .map(|row| row.map(|direction| (direction - d).to_radians().cos()));
    weights[1][1] = 0.0;
    weights
}

fn edge_detect(src: &ImageData, values: &Values, _: &EffectEnv) -> ImageData {
    let weights = edge_weights(num(values, "angle"));
    let mut out = ImageData::new(src.width, src.height);
    for_bands(
        &mut out.data,
        src.width as usize * 4,
        0..src.height as usize,
        1,
        |y, row| {
            for (x, out) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let mut sums = [0.0; 3];
                for (dy, weights) in weights.iter().enumerate() {
                    let yy = y as i64 + dy as i64 - 1;
                    if !(0..src.height as i64).contains(&yy) {
                        continue;
                    }
                    for (dx, &weight) in weights.iter().enumerate() {
                        let xx = x as i64 + dx as i64 - 1;
                        // quirk(bb): stylize-edge-detect-classic.md Canvas edges drop taps without renormalisation.
                        if !(0..src.width as i64).contains(&xx) {
                            continue;
                        }
                        let index = (yy as usize * src.width as usize + xx as usize) * 4;
                        for (sum, &channel) in sums.iter_mut().zip(&src.data[index..index + 3]) {
                            *sum += weight * channel as f64;
                        }
                    }
                }
                // quirk(bb): stylize-edge-detect-classic.md Output ignores source alpha, including stored hidden RGB.
                *out = [
                    sums[0].clamp(0.0, 255.0) as u8,
                    sums[1].clamp(0.0, 255.0) as u8,
                    sums[2].clamp(0.0, 255.0) as u8,
                    255,
                ];
            }
        },
    );
    out
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::test_util::*;
    use serde_json::{Value, json};
    fn grey(v: u8) -> [u8; 4] {
        [v, v, v, 255]
    }
    fn neutral() -> Value {
        json!({"horizon": [0.0, 0.0], "sky": 0, "grass": 0, "sharpen": 0, "saturation": 0, "dynamicRange": 0})
    }
    fn setting(key: &str, value: Value) -> Value {
        let mut v = neutral();
        v[key] = value;
        v
    }
    /// photo-sharpen-classic.md Expected results 1.
    #[test]
    fn sharpen_flat() {
        for amount in 1..=20 {
            let out = bb(
                "bbSharpenClassic",
                json!({"amount":amount}),
                &solid(10, 10, grey(128)),
            );
            for y in 0..10 {
                for x in 0..10 {
                    assert_px(&out, x, y, grey(128));
                }
            }
        }
    }
    /// photo-sharpen-classic.md Expected results 2.
    #[test]
    fn sharpen_hard_step() {
        let src = from_fn(10, 10, |x, _| grey(if x < 5 { 0 } else { 255 }));
        let out = bb("bbSharpenClassic", json!({"amount":1}), &src);
        for y in 0..10 {
            for x in 0..10 {
                assert_px(&out, x, y, px(&src, x, y));
            }
        }
    }
    /// photo-sharpen-classic.md Expected results 3.
    #[test]
    fn sharpen_dot() {
        let src = from_fn(9, 9, |x, y| grey(if (x, y) == (4, 4) { 160 } else { 128 }));
        let out = bb("bbSharpenClassic", json!({"amount":2}), &src);
        for y in 0..9 {
            for x in 0..9 {
                assert_px(&out, x, y, grey(if (x, y) == (4, 4) { 176 } else { 128 }));
            }
        }
    }
    /// photo-sharpen-classic.md Expected results 4.
    #[test]
    fn sharpen_clamp() {
        let src = from_fn(9, 9, |x, y| grey(if (x, y) == (4, 4) { 240 } else { 128 }));
        assert_px(
            &bb("bbSharpenClassic", json!({"amount":2}), &src),
            4,
            4,
            grey(255),
        );
    }
    /// photo-sharpen-landscape.md Expected results 1.
    #[test]
    fn landscape_neutral() {
        let src = from_fn(6, 4, |x, y| {
            [[0, 51, 0, 255], [255; 4], grey(51)][((x + y) % 3) as usize]
        });
        assert_eq!(bb("bbSharpenLandscape", neutral(), &src).data, src.data);
    }
    /// photo-sharpen-landscape.md Expected results 2.
    #[test]
    fn landscape_unsharp() {
        let src = from_fn(9, 9, |x, y| grey(if (x, y) == (4, 4) { 200 } else { 100 }));
        let out = bb("bbSharpenLandscape", setting("sharpen", json!(100)), &src);
        for y in 0u32..9 {
            for x in 0u32..9 {
                let dx = x.abs_diff(4);
                let dy = y.abs_diff(4);
                let v = match (dx, dy) {
                    (0, 0) => 255,
                    (0, 1) | (1, 0) => 88,
                    (1, 1) => 94,
                    _ => 100,
                };
                assert_px_tol(&out, x, y, grey(v), 3);
            }
        }
    }
    /// photo-sharpen-landscape.md Expected results 3.
    #[test]
    fn landscape_sky() {
        let out = bb(
            "bbSharpenLandscape",
            setting("sky", json!(100)),
            &solid(10, 100, [255; 4]),
        );
        for x in 0..10 {
            assert_px(&out, x, 0, [0, 128, 255, 255]);
            assert_px(&out, x, 25, [128, 192, 255, 255]);
            for y in 50..100 {
                assert_px(&out, x, y, [255; 4]);
            }
        }
    }
    /// photo-sharpen-landscape.md Expected results 4.
    #[test]
    fn landscape_grass() {
        let out = bb(
            "bbSharpenLandscape",
            setting("grass", json!(100)),
            &solid(10, 100, [0, 51, 0, 255]),
        );
        for x in 0..10 {
            assert_px(&out, x, 99, [0, 101, 0, 255]);
            for y in 0..=50 {
                assert_px(&out, x, y, [0, 51, 0, 255]);
            }
        }
    }
    /// photo-sharpen-landscape.md Expected results 5.
    #[test]
    fn landscape_dynamic() {
        for (amount, want) in [(50, 31), (100, 10)] {
            let out = bb(
                "bbSharpenLandscape",
                setting("dynamicRange", json!(amount)),
                &solid(4, 4, grey(51)),
            );
            for y in 0..4 {
                for x in 0..4 {
                    assert_px(&out, x, y, grey(want));
                }
            }
        }
    }
    /// stylize-edge-detect-classic.md Expected results 1.
    #[test]
    fn edge_flat_border() {
        let out = bb("bbEdgeDetectClassic", json!({}), &solid(6, 5, grey(100)));
        let want = [
            [0, 170, 170, 170, 170, 241],
            [0, 0, 0, 0, 0, 170],
            [0, 0, 0, 0, 0, 170],
            [0, 0, 0, 0, 0, 170],
            [0; 6],
        ];
        for (y, row) in want.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                assert_px(&out, x as u32, y as u32, grey(v));
            }
        }
    }
    /// stylize-edge-detect-classic.md Expected results 2.
    #[test]
    fn edge_bright_left() {
        let src = from_fn(8, 4, |x, _| grey(if x < 4 { 255 } else { 0 }));
        let out = bb("bbEdgeDetectClassic", json!({"angle":0}), &src);
        for y in 0..4 {
            for x in 0..8 {
                assert_px(&out, x, y, grey(if x == 3 || x == 4 { 255 } else { 0 }));
            }
        }
    }
    /// stylize-edge-detect-classic.md Expected results 3.
    #[test]
    fn edge_bright_right_and_reverse() {
        let src = from_fn(8, 4, |x, _| grey(if x < 4 { 0 } else { 255 }));
        for a in [0, 180, -180] {
            let out = bb("bbEdgeDetectClassic", json!({"angle":a}), &src);
            for y in 0..4 {
                for x in 0..8 {
                    let lit = if a == 0 { x == 7 } else { x == 3 || x == 4 };
                    assert_px(&out, x, y, grey(if lit { 255 } else { 0 }));
                }
            }
        }
    }
    /// stylize-edge-detect-classic.md Expected results 4.
    #[test]
    fn edge_alpha_ignored() {
        assert!(
            bb("bbEdgeDetectClassic", json!({}), &solid(5, 5, [0; 4]))
                .data
                .chunks(4)
                .all(|q| q == [0, 0, 0, 255])
        );
        let opaque = from_fn(8, 7, |x, y| {
            [(x * 31) as u8, (y * 37) as u8, ((x + y) * 17) as u8, 255]
        });
        let alpha = from_fn(8, 7, |x, y| {
            let mut p = px(&opaque, x, y);
            p[3] = if (x + y) % 3 == 0 {
                0
            } else {
                ((x + y) * 17) as u8
            };
            p
        });
        assert_eq!(
            bb("bbEdgeDetectClassic", json!({}), &opaque).data,
            bb("bbEdgeDetectClassic", json!({}), &alpha).data
        );
    }
    /// common.md §2: full-source reads and exactly one fractional clipping pass.
    #[test]
    fn selections_and_outside_reads() {
        use crate::effect::{EffectEnv, apply, find};
        use crate::pixel::ops::blend;
        let src = from_fn(7, 7, |x, y| {
            [
                (x * 31) as u8,
                (y * 33) as u8,
                ((x + y) * 17) as u8,
                ((x * 5 + y * 7) * 3) as u8,
            ]
        });
        let (mut mask, polys) = rect_selection(7, 7, 2, 2, 5, 5);
        mask.data[3 * 7 + 2] = 128;
        let env = with_selection(7, 7, &mask, &polys);
        for (id, v) in [
            ("bbSharpenClassic", json!({})),
            ("bbSharpenLandscape", setting("sharpen", json!(100))),
            ("bbEdgeDetectClassic", json!({})),
        ] {
            let def = find(id).unwrap();
            let mut values = crate::effect::bb::indirect_defaults(&def, &env);
            values.extend(v.as_object().unwrap().clone());
            let selected = apply(&src, &def, &values, &env);
            let unmasked = apply(&src, &def, &values, &EffectEnv { mask: None, ..env });
            for y in 0..7 {
                for x in 0..7 {
                    assert_eq!(
                        px(&selected, x, y),
                        blend(
                            px(&src, x, y),
                            px(&unmasked, x, y),
                            mask.data[(y * 7 + x) as usize]
                        )
                    );
                }
            }
            // Changing only off-selection pixels must affect a selected boundary pixel.
            let changed = from_fn(7, 7, |x, y| {
                if (2..5).contains(&x) && (2..5).contains(&y) {
                    px(&src, x, y)
                } else {
                    [255, 0, 255, 255]
                }
            });
            let other = apply(&changed, &def, &values, &env);
            assert_ne!(px(&other, 2, 3), px(&selected, 2, 3), "{id}");
        }
    }
    /// photo-sharpen-landscape.md steps 1, 5–6: absolute rows and exclusive bounds bottom.
    #[test]
    fn offset_horizon_ties_endpoints_and_ignored_x() {
        let src = solid(6, 12, [255; 4]);
        let (mask, polys) = rect_selection(6, 12, 1, 4, 5, 10);
        let env = with_selection(6, 12, &mask, &polys);
        let mut sky = setting("sky", json!(100));
        sky["horizon"] = json!([0.0, 0.0]);
        assert_eq!(
            bb_env("bbSharpenLandscape", sky.clone(), &src, &env).data,
            src.data,
            "h=3 is above selection"
        );
        let grass = setting("grass", json!(100));
        let green = solid(6, 12, [0, 51, 0, 255]);
        assert_px(
            &bb_env("bbSharpenLandscape", grass, &green, &env),
            2,
            9,
            [0, 94, 0, 255],
        );
        for (height, horizon) in [(5, 2), (7, 4)] {
            let src = solid(3, height, [255; 4]);
            let out = bb("bbSharpenLandscape", sky.clone(), &src);
            assert_eq!(px(&out, 1, horizon), [255; 4]);
            assert!(px(&out, 1, horizon - 1)[0] < 255);
        }
        for y in [-1.0, 1.0] {
            let mut v = sky.clone();
            v["horizon"] = json!([-1.0, y]);
            let a = bb("bbSharpenLandscape", v.clone(), &src);
            v["horizon"] = json!([1.0, y]);
            assert_eq!(a.data, bb("bbSharpenLandscape", v, &src).data);
            if y == -1.0 {
                assert_eq!(a.data, src.data);
            } else {
                assert_eq!(px(&a, 0, 0), [0, 128, 255, 255]);
            }
        }
    }
    /// Sharpen's independent RGBA median, inferred truncation and Q11 +1 behavior.
    #[test]
    fn sharpen_alpha_and_truncation() {
        let src = from_fn(5, 5, |x, y| {
            if (x, y) == (2, 2) {
                [161, 97, 33, 161]
            } else {
                [128, 64, 0, 128]
            }
        });
        assert_eq!(
            px(&bb("bbSharpenClassic", json!({"amount":1}), &src), 2, 2),
            [177, 113, 49, 177]
        );
        assert_eq!(
            px(
                &bb(
                    "bbSharpenClassic",
                    json!({"amount":1}),
                    &solid(3, 3, [128, 64, 0, 128])
                ),
                1,
                1
            ),
            [127, 63, 0, 127]
        );
    }
    /// photo-sharpen-landscape.md steps 2–7: blurred alpha, saturation and ordered source-over.
    #[test]
    fn landscape_alpha_saturation_and_blend_order() {
        let src = from_fn(3, 3, |x, y| {
            if (x, y) == (1, 1) {
                [204, 102, 51, 255]
            } else {
                [0; 4]
            }
        });
        let out = bb("bbSharpenLandscape", neutral(), &src);
        assert_eq!(px(&out, 1, 1), [204, 102, 51, 63]);
        assert_eq!(px(&out, 0, 0)[3], 28);
        let transparent = solid(3, 4, [51, 102, 153, 0]);
        assert_eq!(
            px(
                &bb(
                    "bbSharpenLandscape",
                    setting("sky", json!(100)),
                    &transparent
                ),
                1,
                0
            ),
            [0, 128, 255, 255]
        );
        assert_eq!(
            px(
                &bb(
                    "bbSharpenLandscape",
                    setting("grass", json!(100)),
                    &transparent
                ),
                1,
                3
            ),
            [0, 128, 0, 127]
        );
        assert_px(
            &bb(
                "bbSharpenLandscape",
                setting("dynamicRange", json!(50)),
                &transparent,
            ),
            1,
            1,
            [51, 102, 153, 127],
        );
        let colorful = solid(3, 4, [102, 153, 204, 128]);
        // Fixed-point intensity143, 250% saturation gives [40,168,255] before integer HSV.
        assert_px(
            &bb(
                "bbSharpenLandscape",
                setting("saturation", json!(50)),
                &colorful,
            ),
            1,
            1,
            [40, 169, 255, 128],
        );
        let mut v = neutral();
        for key in ["sky", "grass", "dynamicRange"] {
            v[key] = json!(100);
        }
        // White → sky Multiply [0,128,255] → self Multiply [0,64,255].
        assert_eq!(
            px(&bb("bbSharpenLandscape", v, &solid(3, 4, [255; 4])), 1, 0),
            [0, 64, 255, 255]
        );
    }
    /// CPU stored-byte arithmetic ignores caller GPU RenderInfo; deterministic on tiny layers.
    #[test]
    fn encoded_cpu_and_tiny_layers() {
        use crate::effect::surface::{Alpha, Space};
        use crate::effect::{EffectEnv, RenderInfo};
        for id in [
            "bbSharpenClassic",
            "bbSharpenLandscape",
            "bbEdgeDetectClassic",
        ] {
            for (w, h) in [(0, 0), (0, 3), (3, 0), (1, 1), (1, 4), (4, 1), (2, 2)] {
                let src = from_fn(w, h, |x, y| {
                    [(x * 71 + 51) as u8, (y * 37 + 102) as u8, 153, 128]
                });
                let a = bb(id, json!({}), &src);
                assert_eq!(a.data.len(), (w * h * 4) as usize);
                for (space, alpha) in [
                    (Space::Srgb, Alpha::Straight),
                    (Space::Linear, Alpha::Premultiplied),
                ] {
                    let env = EffectEnv {
                        render_info: Some(RenderInfo::new(space, alpha)),
                        ..EffectEnv::whole(w, h)
                    };
                    assert_eq!(bb_env(id, json!({}), &src, &env).data, a.data);
                }
            }
        }
    }
    /// All three spec control tables: exact metadata, CPU clipping, menu manifest and help.
    #[test]
    fn presentation() {
        use crate::effect::bb::ui::{Help, HelpWindow, TRACK_SATURATION, TRACK_WHITE_BLACK};
        use crate::effect::{Category, Control, EffectKind, PanThumb, find};
        for (id, title, category, icon) in [
            (
                "bbSharpenClassic",
                "Sharpen Classic",
                "photo",
                "sharpen_classic.png",
            ),
            (
                "bbSharpenLandscape",
                "Sharpen Landscape",
                "photo",
                "landscape.png",
            ),
            (
                "bbEdgeDetectClassic",
                "Edge Detect Classic",
                "stylize",
                "edge_detect_classic.png",
            ),
        ] {
            let def = find(id).unwrap();
            assert_eq!(def.dialog_title, Some(title));
            assert_eq!(def.category, Category::Effect(category));
            assert_eq!(def.icon, icon);
            assert!(matches!(
                def.kind,
                EffectKind::Cpu {
                    clip_to_selection: true,
                    ..
                }
            ));
            assert!(def.dialog.is_none());
            let ui = def.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(ui.transient_keys.is_empty());
            for c in &def.controls {
                assert!((ui.enabled)(&crate::effect::defaults(&def), c.key()));
                assert!(ui.control(c.key()).reset);
            }
        }
        let sharpen = find("bbSharpenClassic").unwrap();
        let landscape = find("bbSharpenLandscape").unwrap();
        let edge = find("bbEdgeDetectClassic").unwrap();
        assert!(matches!(
            sharpen.indirect.unwrap().help,
            Help::Text([
                "Sharpen Classic v1.0",
                "Sharpen algorithm Copyright ©2013 by Rick Brewster",
                "All rights reserved."
            ])
        ));
        assert!(matches!(
            edge.indirect.unwrap().help,
            Help::Text([
                "Edge Detect Classic v1.0",
                "Algorithm ©2004 by Chris Crosetto",
                "All rights reserved."
            ])
        ));
        assert!(matches!(
            landscape.indirect.unwrap().help,
            Help::Window(HelpWindow::Standard)
        ));
        let ranges = |def: &crate::effect::EffectDef,
                      expected: &[(&str, &str, f64, f64, f64, u32)]| {
            for (c, &(key, label, min, max, default, precision)) in def
                .controls
                .iter()
                .filter(|c| matches!(c, Control::Range { .. }))
                .zip(expected)
            {
                let Control::Range {
                    key: k,
                    label: l,
                    min: a,
                    max: b,
                    default: d,
                    step,
                    keyboard,
                    ..
                } = c
                else {
                    unreachable!()
                };
                assert_eq!(
                    (*k, *l, *a, *b, *d, *step),
                    (key, label, min, max, default, 1.0)
                );
                assert_eq!(
                    (
                        keyboard.decimals,
                        keyboard.small,
                        keyboard.large,
                        keyboard.exponential
                    ),
                    (precision, 1.0, 5.0, false)
                );
            }
        };
        assert_eq!(sharpen.controls.len(), 1);
        assert_eq!(edge.controls.len(), 1);
        assert_eq!(landscape.controls.len(), 6);
        ranges(&sharpen, &[("amount", "Sharpen Amount", 1., 20., 2., 0)]);
        ranges(
            &edge,
            &[("angle", "Edge Detect Angle", -180., 180., 45., 3)],
        );
        ranges(
            &landscape,
            &[
                ("sky", "Sky / Grass", 0., 100., 50., 0),
                ("grass", "", 0., 100., 50., 0),
                ("sharpen", "Sharpen", 0., 200., 100., 0),
                ("saturation", "Saturation", 0., 100., 50., 0),
                ("dynamicRange", "Dynamic Range", 0., 100., 50., 0),
            ],
        );
        assert!(!sharpen.indirect.unwrap().control("amount").header_line);
        let a = edge.indirect.unwrap().control("angle");
        assert!(!a.header_line && a.wrap);
        assert_eq!(a.decimals, Some(3));
        assert!(matches!(
            landscape.controls[0],
            Control::Pan {
                key: "horizon",
                label: "Position of Horizon",
                min: -1.0,
                max: 1.0,
                default: [0.0, -0.25],
                step: 0.01,
                thumb: PanThumb::SelectionCrop
            }
        ));
        let ui = landscape.indirect.unwrap();
        let pan = ui.control("horizon");
        assert_eq!(pan.decimals, Some(3));
        let keys = pan.pan_sliders.unwrap();
        assert_eq!(
            (keys.decimals, keys.small, keys.large, keys.exponential),
            (3, 0.05, 0.25, false)
        );
        for key in ["horizon", "sky", "sharpen", "saturation", "dynamicRange"] {
            assert!(ui.control(key).header_line);
        }
        assert!(!ui.control("grass").header_line);
        assert_eq!(ui.control("sky").track, Some(&[0xFFFFFF, 0x0000FF][..]));
        assert_eq!(ui.control("grass").track, Some(&[0xFFFFFF, 0x008000][..]));
        assert_eq!(ui.control("saturation").track, Some(&TRACK_SATURATION[..]));
        assert_eq!(
            ui.control("dynamicRange").track,
            Some(&TRACK_WHITE_BLACK[..])
        );
    }

    /// stylize-edge-detect-classic.md sample weights, all three rows and ±180 equivalence.
    #[test]
    #[allow(
        clippy::approx_constant,
        reason = "literal four-decimal sample table from the spec"
    )]
    fn weight_tables() {
        let r = |a| super::edge_weights(a).map(|row| row.map(|v| (v * 1e4).round() / 1e4));
        assert_eq!(
            r(0.0),
            [
                [0.7071, 0.0, -0.7071],
                [1.0, 0.0, -1.0],
                [0.7071, 0.0, -0.7071]
            ]
        );
        assert_eq!(
            r(45.0),
            [
                [0.0, -0.7071, -1.0],
                [0.7071, 0.0, -0.7071],
                [1.0, 0.7071, 0.0]
            ]
        );
        assert_eq!(
            r(90.0),
            [
                [-0.7071, -1.0, -0.7071],
                [0.0, 0.0, 0.0],
                [0.7071, 1.0, 0.7071]
            ]
        );
        assert_eq!(r(180.0), r(-180.0));
    }
    /// photo-sharpen-landscape.md step 3: multiply the integer percent before division and truncation.
    #[test]
    fn landscape_unsharp_byte_operation_order() {
        let src = from_fn(3, 1, |x, _| grey(if x == 1 { 10 } else { 60 }));
        // Tent blur at center35; 10+(10-35)*28/100=3, then integer HSV yields2.
        assert_eq!(
            px(
                &bb("bbSharpenLandscape", setting("sharpen", json!(28)), &src),
                1,
                0
            ),
            grey(2)
        );
    }
    /// stylize-edge-detect-classic.md: angles ±180 give exactly the same rendered result.
    #[test]
    fn edge_endpoints_are_identical() {
        let src = from_fn(3, 3, |x, y| {
            grey(u8::from((x, y) == (2, 1) || (x, y) == (1, 2)))
        });
        let positive = bb("bbEdgeDetectClassic", json!({"angle": 180}), &src);
        let negative = bb("bbEdgeDetectClassic", json!({"angle": -180}), &src);
        assert_eq!(px(&positive, 1, 1), px(&negative, 1, 1));
        assert_eq!(positive.data, negative.data);
    }
}
