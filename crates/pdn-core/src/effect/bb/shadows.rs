//! BB spec: effects/object-inner-shadow.md; effects/selection-inner-shadow.md;
//! effects/object-drop-shadow-plus.md; common.md §1–5
use super::blocks::{
    blur_soft, colour_exact, drop_shadow_with, motion_blur, quantise8, rotate_zoom_with,
    shadow_offset, source_over, swap_alpha, with_alpha,
};
use super::conventions::*;
use super::manifest::manifest;
use super::raster::{fill_coverage, polygon_path, selection_or_canvas};
use super::staircase::staircase;
use super::ui::{ControlUi, Help, IndirectUi, TRACK_WHITE_BLACK, Tab, TabStrip};
use crate::effect::surface::{Alpha, Edge, EffectSurface, Space};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, LabelText, PanThumb, RenderInfo,
    Values, choice, color_value, flag, num, range,
};
use serde_json::json;

const fn precision(decimals: usize) -> ControlUi {
    ControlUi {
        decimals: Some(decimals),
        ..ControlUi::NO_HEADER
    }
}
const fn strength_ui(decimals: usize) -> ControlUi {
    ControlUi {
        track: Some(&TRACK_WHITE_BLACK),
        ..precision(decimals)
    }
}
const ANGLE_UI: ControlUi = ControlUi {
    wrap: true,
    ..precision(3)
};
static INNER_UI: IndirectUi = IndirectUi {
    transient_keys: &["tab"],
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: " Shadow ",
                keys: &["angle", "length", "strength"],
            },
            Tab {
                caption: " Options ",
                keys: &["color", "keepBackground"],
            },
        ],
    }],
    controls: &[
        ("angle", ANGLE_UI),
        ("length", ControlUi::NO_HEADER),
        ("strength", strength_ui(2)),
        ("color", ControlUi::NO_HEADER),
        ("keepBackground", ControlUi::NO_HEADER),
    ],
    ..IndirectUi::DEFAULT
};
static DROP_UI: IndirectUi = IndirectUi {
    width: 1.1,
    tabs: &[TabStrip {
        key: "shadowType",
        tabs: &[
            Tab {
                caption: "Drop Shadow",
                keys: &[
                    "distance",
                    "angle",
                    "strength",
                    "color",
                    "shadowOnly",
                    "note",
                ],
            },
            Tab {
                caption: "Long Shadow",
                keys: &[
                    "longDistance",
                    "angle",
                    "strength",
                    "color",
                    "edges",
                    "shadowOnly",
                ],
            },
            Tab {
                caption: "3D Shadow",
                keys: &[
                    "distance",
                    "strength",
                    "tiltAngle",
                    "tiltDirection",
                    "tiltAmount",
                    "location",
                    "zoom",
                    "shadowOnly",
                ],
            },
        ],
    }],
    controls: &[
        ("distance", ControlUi::NO_HEADER),
        ("angle", ANGLE_UI),
        ("strength", strength_ui(3)),
        ("color", ControlUi::NO_HEADER),
        ("shadowOnly", ControlUi::NO_HEADER),
        ("longDistance", ControlUi::NO_HEADER),
        (
            "edges",
            ControlUi {
                caption: "Shadow edges: ",
                ..ControlUi::NO_HEADER
            },
        ),
        ("tiltAngle", precision(3)),
        ("tiltDirection", precision(3)),
        ("tiltAmount", precision(3)),
        (
            "location",
            ControlUi {
                pan_sliders: Some(crate::effect::slider::SliderKeys {
                    decimals: 3,
                    small: 0.05,
                    large: 0.25,
                    exponential: false,
                }),
                ..precision(3)
            },
        ),
        ("zoom", precision(3)),
        ("note", ControlUi::NO_HEADER),
    ],
    help: Help::Text(&["Drop Shadow+ v1.0"]),
    ..IndirectUi::DEFAULT
};
fn colour() -> Control {
    Control::Color {
        key: "color",
        label: "Color",
        default: ColorDefault::Rgb(0),
    }
}
fn checkbox(key: &'static str, label: &'static str, default: bool) -> Control {
    Control::Checkbox {
        key,
        label,
        default,
    }
}
fn tab(key: &'static str) -> Control {
    Control::Value {
        key,
        default: || json!(0),
    }
}
fn angle(label: &'static str) -> Control {
    // R14 replacement slider: explicit port key policy; R107 numeric AngleChooser increment is 1.
    range("angle", label, -180.0, 180.0, -45.0).slider_keys(3, 1.0, 5.0, false)
}
fn def(
    id: &'static str,
    title: &'static str,
    controls: Vec<Control>,
    kind: EffectKind,
    ui: &'static IndirectUi,
) -> EffectDef {
    let m = manifest(id).unwrap();
    EffectDef {
        id,
        icon: m.icon,
        category: m.category,
        controls,
        kind,
        dialog: None,
        dialog_title: Some(title),
        indirect: Some(ui),
    }
}
pub fn effects() -> Vec<EffectDef> {
    let inner = || {
        vec![
            tab("tab"),
            angle("Angle"),
            range("length", "Length", 1.0, 100.0, 10.0),
            range("strength", "Strength", 0.0, 1.0, 0.75).slider_keys(2, 0.01, 0.1, false),
            colour(),
            checkbox("keepBackground", "Keep background", true),
        ]
    };
    vec![
        def(
            "bbInnerShadowObject",
            "Object Inner Shadow",
            inner(),
            EffectKind::Gpu(GPU_CONTEXT, inner_object),
            &INNER_UI,
        ),
        def(
            "bbInnerShadowSelection",
            "Selection Inner Shadow",
            inner(),
            EffectKind::Gpu(GPU_CONTEXT, inner_selection),
            &INNER_UI,
        ),
        def(
            "bbDropShadowPlus",
            "Drop Shadow+",
            vec![
                tab("shadowType"),
                range("distance", "Distance", 0.0, 100.0, 4.0),
                angle("Light Angle"),
                range("strength", "Strength", 0.0, 1.0, 0.75)
                    .step(0.01)
                    .slider_keys(3, 0.05, 0.25, false),
                colour(),
                checkbox("shadowOnly", "Only draw shadow", false),
                range("longDistance", "Distance", 0.0, 500.0, 100.0),
                Control::Select {
                    key: "edges",
                    label: "",
                    default: "blurred",
                    choices: &[
                        ("blurred", "Blurred"),
                        ("antiAliased", "Anti-Aliased"),
                        ("aliased", "Aliased"),
                    ],
                },
                range("tiltAngle", "Tilt", -180.0, 180.0, 0.0)
                    .step(0.01)
                    .slider_keys(3, 1.0, 5.0, false),
                range("tiltDirection", "", -180.0, 180.0, -90.0)
                    .step(0.01)
                    .slider_keys(3, 1.0, 5.0, false),
                range("tiltAmount", "", 0.0, 90.0, 60.0)
                    .step(0.01)
                    .slider_keys(3, 1.0, 5.0, false),
                Control::Pan {
                    key: "location",
                    label: "Location",
                    min: -1.0,
                    max: 1.0,
                    default: [0.0; 2],
                    step: 0.01,
                    thumb: PanThumb::SelectionCrop,
                },
                range("zoom", "Zoom", 0.0, 2.0, 1.0)
                    .step(0.01)
                    .slider_keys(3, 0.05, 0.25, false),
                Control::Label {
                    key: "note",
                    label: "",
                    text: LabelText::Fixed(
                        "\nThis effect requires a layer with transparency. Make sure that the object you want a drop shadow for is on its own layer.",
                    ),
                },
            ],
            EffectKind::Gpu(
                RenderInfo::new(Space::Linear, Alpha::Premultiplied),
                drop_plus,
            ),
            &DROP_UI,
        ),
    ]
}
fn strength(v: &Values) -> f32 {
    num(v, "strength").clamp(0.0, 1.0) as f32
}
fn truncated_strength(v: &Values) -> f32 {
    (num(v, "strength").clamp(0.0, 1.0) * 255.0).trunc() as f32 / 255.0
}
fn chosen(v: &Values, env: &EffectEnv) -> [f32; 4] {
    let mut c = color_value(v, "color", env);
    c[3] = 255;
    colour_exact(c, env.render_info.unwrap_or(GPU_CONTEXT))
}
fn inner_object(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    inner(src, v, env, false)
}
fn inner_selection(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    inner(src, v, env, true)
}
fn inner(src: &EffectSurface, v: &Values, env: &EffectEnv, selection: bool) -> EffectSurface {
    let rgb = chosen(v, env);
    // quirk(bb): object-inner-shadow.md / selection-inner-shadow.md Behaviour 1, truncate alpha.
    let c = rgb.map(|c| c * truncated_strength(v));
    let s = if selection {
        let coverage = polygon_path(&selection_or_canvas(env), [0.0; 2], true).map_or_else(
            || vec![0.0; src.px.len()],
            |p| fill_coverage(&p, src.w, src.h, true),
        );
        EffectSurface::from_fn(src.w, src.h, |x, y| {
            c.map(|c| c * (1.0 - coverage[(y * src.w + x) as usize]))
        })
    } else {
        // quirk(bb): object-inner-shadow.md Behaviour 2, even alpha 1/255 counts as object.
        EffectSurface::from_fn(src.w, src.h, |x, y| {
            if src.at(x, y)[3] == 0.0 { c } else { [0.0; 4] }
        })
    };
    let smear = motion_blur(
        &s,
        num(v, "length").clamp(1.0, 100.0),
        num(v, "angle") + SMEAR_ANGLE_OFFSET_DEG,
        false,
        Edge::Transparent,
    );
    let q = blur_soft(&smear, 1.0);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = src.at(x, y);
        let q = q.at(x, y);
        if flag(v, "keepBackground") {
            let over = source_over(p, q);
            if selection {
                over
            } else {
                // spec-question: Q30 Inner Shadow object colour/alpha swizzle
                if INNER_SHADOW_STRAIGHT_COLOR_SWIZZLE {
                    with_alpha(over, p[3])
                } else {
                    swap_alpha(over, p[3])
                }
            }
        } else if selection {
            q
        } else {
            let tinted = rgb.map(|c| c * p[3]);
            std::array::from_fn(|i| tinted[i].min(q[i]))
        }
    })
}
fn drop_plus(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let offset = |distance| shadow_offset(distance, num(v, "angle") + SHADOW_ANGLE_OFFSET_DEG);
    let distance = num(v, "distance").clamp(0.0, 100.0);
    let only = flag(v, "shadowOnly");
    match super::index(v, "shadowType") {
        1 => {
            let sil = drop_shadow_with(src, 0.0, offset(1.5), [0.0, 0.0, 0.0, 1.0], true);
            // spec-question: Q12 Long Shadow at large distances, retain exact kernel/threshold.
            // shortcut: O(area · taps); use a directional running window if photo-sized 500px sweeps are slow.
            let sweep = motion_blur(
                &sil,
                num(v, "longDistance").clamp(0.0, 500.0),
                num(v, "angle") + SMEAR_ANGLE_OFFSET_DEG,
                false,
                Edge::Transparent,
            );
            // quirk(bb): object-drop-shadow-plus.md Long Shadow 3, two UNORM quantisations.
            let mask: Vec<u8> = sweep
                .px
                .iter()
                .map(|p| {
                    if quantise8(quantise8(p[3] * 255.0) * 255.0) == 1.0 {
                        255
                    } else {
                        0
                    }
                })
                .collect();
            let mask = if choice(v, "edges") == "aliased" {
                mask
            } else {
                staircase(&mask, src.w, src.h, AA_BYTE_CONVERSION).0
            };
            let c = chosen(v, env).map(|c| c * strength(v));
            let shadow = EffectSurface::from_fn(src.w, src.h, |x, y| {
                c.map(|c| c * mask[(y * src.w + x) as usize] as f32 / 255.0)
            });
            let shadow = if choice(v, "edges") == "blurred" {
                blur_soft(&shadow, 2.0 * RADIUS_TO_SIGMA)
            } else {
                shadow
            };
            if only {
                shadow
            } else {
                EffectSurface::from_fn(src.w, src.h, |x, y| {
                    source_over(shadow.at(x, y), src.at(x, y))
                })
            }
        }
        2 => {
            // quirk(bb): object-drop-shadow-plus.md 3D Shadow 1, chosen Color is ignored.
            let shadow = drop_shadow_with(
                src,
                distance * RADIUS_TO_SIGMA,
                offset(distance),
                [0.0, 0.0, 0.0, truncated_strength(v)],
                true,
            );
            let location = super::pan(v, "location");
            let rz = Values::from_iter([
                (
                    ROTATE_ZOOM_TILT_KEYS[0].to_string(),
                    json!(num(v, "tiltAngle")),
                ),
                (
                    ROTATE_ZOOM_TILT_KEYS[1].to_string(),
                    json!(num(v, "tiltDirection")),
                ),
                (
                    ROTATE_ZOOM_TILT_KEYS[2].to_string(),
                    json!(num(v, "tiltAmount")),
                ),
                ("panX".into(), json!(location[0] * ROTATE_ZOOM_PAN_SCALE)),
                ("panY".into(), json!(location[1] * ROTATE_ZOOM_PAN_SCALE)),
                ("zoom".into(), json!(num(v, "zoom"))),
                ("quality".into(), json!(ROTATE_ZOOM_QUALITY)),
                ("tiling".into(), json!(ROTATE_ZOOM_EDGE)),
                ("sampling".into(), json!("linear")),
            ]);
            let shadow = rotate_zoom_with(&shadow, &rz);
            if only {
                shadow
            } else {
                EffectSurface::from_fn(src.w, src.h, |x, y| {
                    source_over(shadow.at(x, y), src.at(x, y))
                })
            }
        }
        _ => drop_shadow_with(
            src,
            distance * RADIUS_TO_SIGMA,
            offset(distance),
            chosen(v, env).map(|c| c * truncated_strength(v)),
            only,
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::test_util::*;
    use serde_json::json;

    fn square(w: u32, l: u32, r: u32) -> crate::canvas::ImageData {
        from_fn(w, w, |x, y| {
            if (l..r).contains(&x) && (l..r).contains(&y) {
                [255; 4]
            } else {
                [0; 4]
            }
        })
    }

    /// object-inner-shadow.md Expected results 1–5.
    #[test]
    fn inner_shadow_object() {
        let white = solid(50, 50, [255; 4]);
        assert_eq!(
            bb("bbInnerShadowObject", json!({}), &white).data,
            white.data
        );
        assert!(
            bb(
                "bbInnerShadowObject",
                json!({"keepBackground": false}),
                &white
            )
            .data
            .chunks(4)
            .all(|q| q[3] == 0)
        );
        let t = square(100, 30, 70);
        let out = bb("bbInnerShadowObject", json!({}), &t);
        for (x, y) in [(10, 10), (29, 50), (70, 50), (50, 75)] {
            assert_eq!(px(&out, x, y)[3], 0);
        }
        for (x, y) in [(50, 30), (30, 50)] {
            let q = px(&out, x, y);
            assert!(q[0] < 200 && q[3] == 255, "{q:?}");
        }
        assert!(px(&out, 50, 42)[0] >= 253 && px(&out, 42, 50)[0] >= 253);
        for (x, y) in [(50, 69), (69, 50)] {
            assert!(px(&out, x, y)[0] >= 230);
        }
        assert_px(&out, 50, 50, [255; 4]);
        assert_eq!(
            bb("bbInnerShadowObject", json!({"strength": 0}), &t).data,
            t.data
        );
        assert!(
            bb(
                "bbInnerShadowObject",
                json!({"strength": 0, "keepBackground": false}),
                &t
            )
            .data
            .chunks(4)
            .all(|q| q[3] == 0)
        );
    }

    /// selection-inner-shadow.md Expected results 1–4.
    #[test]
    fn inner_shadow_selection() {
        let white = solid(100, 100, [255; 4]);
        assert_eq!(
            bb(
                "bbInnerShadowSelection",
                json!({}),
                &solid(50, 50, [255; 4])
            )
            .data,
            solid(50, 50, [255; 4]).data
        );
        assert!(
            bb(
                "bbInnerShadowSelection",
                json!({"keepBackground": false}),
                &solid(50, 50, [255; 4])
            )
            .data
            .chunks(4)
            .all(|q| q[3] == 0)
        );
        let (m, p) = rect_selection(100, 100, 30, 30, 70, 70);
        let env = with_selection(100, 100, &m, &p);
        let out = bb_env("bbInnerShadowSelection", json!({}), &white, &env);
        for (x, y) in [(50, 30), (30, 50)] {
            let q = px(&out, x, y);
            assert!(q[0] < 200 && q[3] == 255);
        }
        assert!(px(&out, 50, 42)[0] >= 253);
        assert_px(&out, 29, 50, [255; 4]);
        let off = bb_env(
            "bbInnerShadowSelection",
            json!({"keepBackground": false}),
            &white,
            &env,
        );
        assert_eq!(px(&off, 50, 50)[3], 0);
        let s = px(&off, 30, 50);
        assert!(s[0] <= TOL && s[3] > 0 && s[3] <= 191 + TOL, "{s:?}");
        assert_px(&off, 10, 10, [255; 4]);
        let clear = bb_env(
            "bbInnerShadowSelection",
            json!({}),
            &solid(100, 100, [0; 4]),
            &env,
        );
        assert!(px(&clear, 30, 50)[3] > 0 && px(&clear, 30, 50)[0] <= TOL);
        assert_eq!(px(&clear, 50, 50)[3], 0);
    }

    fn ds(v: serde_json::Value) -> crate::canvas::ImageData {
        bb("bbDropShadowPlus", v, &square(60, 20, 40))
    }

    /// object-drop-shadow-plus.md Expected results 1, 2, 6.
    #[test]
    fn drop_shadow_tab() {
        let out = ds(json!({}));
        for y in 20..40 {
            for x in 20..40 {
                assert_px(&out, x, y, [255; 4]);
            }
        }
        assert!(px(&out, 44, 44)[3] > px(&out, 15, 15)[3]);
        assert!(px(&out, 44, 44)[0] <= TOL && px(&out, 44, 44)[3] <= 191 + TOL);
        assert_eq!(px(&out, 2, 2), [0, 0, 0, 0]);
        let only = ds(json!({"shadowOnly": true}));
        assert!(
            only.data
                .chunks(4)
                .all(|q| q[..3].iter().all(|c| *c <= TOL))
        );
        assert!(px(&only, 30, 30)[3] <= 191 + TOL);
        let opaque = solid(30, 30, [90, 30, 200, 255]);
        assert_eq!(bb("bbDropShadowPlus", json!({}), &opaque).data, opaque.data);
    }

    /// object-drop-shadow-plus.md Expected results 3–4 (Long Shadow).
    #[test]
    fn long_shadow() {
        let base = json!({"shadowType": 1, "longDistance": 20, "edges": "aliased", "strength": 1.0, "color": "#ff0000"});
        let out = ds(base.clone());
        assert!(
            out.data
                .chunks(4)
                .all(|q| q == [255, 255, 255, 255] || q == [255, 0, 0, 255] || q == [0; 4]),
            "no partial alpha"
        );
        assert_eq!(
            (px(&out, 50, 50), px(&out, 45, 30)),
            ([255, 0, 0, 255], [255, 0, 0, 255])
        );
        assert_eq!((px(&out, 10, 30)[3], px(&out, 58, 58)[3]), (0, 0));
        let mut v = base.clone();
        v["strength"] = json!(0.75);
        let soft = ds(v.clone());
        assert!(px(&soft, 48, 48)[3].abs_diff(191) <= 1);
        v["edges"] = json!("antiAliased");
        let aa = ds(v);
        assert!(px(&aa, 48, 48)[3].abs_diff(191) <= 1);
        for y in 1..59 {
            for x in 1..59 {
                let a = px(&aa, x, y)[3];
                if a != 0 && a.abs_diff(191) > 1 && px(&aa, x, y)[0] == 255 && px(&aa, x, y)[1] == 0
                {
                    let edge = (-1i32..=1).any(|dy| {
                        (-1i32..=1).any(|dx| {
                            px(&soft, (x as i32 + dx) as u32, (y as i32 + dy) as u32)[3] == 0
                        })
                    });
                    assert!(
                        edge,
                        "partial alpha at ({x},{y}) is not within 1 px of the hard boundary"
                    );
                }
            }
        }
    }

    /// object-drop-shadow-plus.md Expected results 5: 3D Shadow ignores Color.
    #[test]
    fn three_d_shadow_is_black() {
        let out = ds(json!({"shadowType": 2, "color": "#ff0000"}));
        assert!(
            out.data
                .chunks(4)
                .filter(|q| q[3] > 0 && q != &[255, 255, 255, 255])
                .all(|q| q[..3].iter().all(|c| *c <= TOL)),
            "every shadow pixel is black"
        );
        assert!(
            out.data.chunks(4).any(|q| q[3] > 0 && q[0] <= TOL),
            "a shadow is drawn"
        );
    }

    /// object-inner-shadow.md Expected results 3–4: all exterior pixels, monotone fade and fringe extent.
    #[test]
    fn object_whole_exterior_and_fade() {
        let out = bb("bbInnerShadowObject", json!({}), &square(100, 30, 70));
        check_side(&out);
        for y in 0..100 {
            for x in 0..100 {
                if !(30..70).contains(&x) || !(30..70).contains(&y) {
                    assert_eq!(px(&out, x, y), [0; 4]);
                }
            }
        }
    }
    fn check_side(out: &crate::canvas::ImageData) {
        for vertical in [false, true] {
            let at = |depth: u32| {
                if vertical {
                    px(out, 50, 30 + depth)
                } else {
                    px(out, 30 + depth, 50)
                }
            };
            let fade: Vec<_> = (0..=12).map(|d| at(d)[0]).collect();
            assert!(fade.windows(2).all(|a| a[0] <= a[1]), "{fade:?}");
            assert!(fade[0] < 200 && fade[12] >= 253);
        }
        assert_px(out, 50, 67, [255; 4]);
        assert_px(out, 67, 50, [255; 4]);
        assert!(px(out, 50, 69)[0] > px(out, 50, 30)[0]);
        assert!(px(out, 69, 50)[0] > px(out, 30, 50)[0]);
    }
    /// selection-inner-shadow.md Expected results 2–4: full exterior and both inner-edge fades.
    #[test]
    fn selection_whole_exterior_and_fade() {
        let (m, p) = rect_selection(100, 100, 30, 30, 70, 70);
        let env = with_selection(100, 100, &m, &p);
        let white = solid(100, 100, [255; 4]);
        let out = bb_env("bbInnerShadowSelection", json!({}), &white, &env);
        check_side(&out);
        for keep in [true, false] {
            let out = bb_env(
                "bbInnerShadowSelection",
                json!({"keepBackground":keep}),
                &white,
                &env,
            );
            for y in 0..100 {
                for x in 0..100 {
                    if !(30..70).contains(&x) || !(30..70).contains(&y) {
                        assert_eq!(px(&out, x, y), [255; 4]);
                    }
                }
            }
        }
    }
    /// object-inner-shadow.md Behaviour 2 and Notes: alpha 1/255 is object, not empty canvas.
    #[test]
    fn any_positive_alpha_is_object() {
        let src = solid(30, 30, [100, 130, 170, 1]);
        for context in [super::GPU_CONTEXT, super::ALT_GPU_CONTEXT] {
            assert_eq!(
                bb_ctx("bbInnerShadowObject", context, json!({}), &src).data,
                src.data
            );
            assert!(
                bb_ctx(
                    "bbInnerShadowObject",
                    context,
                    json!({"keepBackground":false}),
                    &src
                )
                .data
                .iter()
                .all(|b| *b == 0)
            );
        }
    }
    /// object-inner-shadow.md Notes and Behaviour 5: straight-colour swizzle, original alpha, unscaled chosen RGB.
    #[test]
    fn coloured_partial_alpha_and_effective_context() {
        use crate::effect::surface::Space;
        let src = from_fn(100, 100, |x, y| {
            if (30..70).contains(&x) && (30..70).contains(&y) {
                [255, 0, 0, 128]
            } else {
                [0; 4]
            }
        });
        for context in [super::GPU_CONTEXT, super::ALT_GPU_CONTEXT] {
            let off = bb_ctx(
                "bbInnerShadowObject",
                context,
                json!({"color":"#0080ff","keepBackground":false}),
                &src,
            );
            let on = bb_ctx(
                "bbInnerShadowObject",
                context,
                json!({"color":"#0080ff"}),
                &src,
            );
            let q = px(&off, 50, 33);
            assert!(q[3] > 0 && q[3] < 128);
            assert_px(&off, 50, 33, [0, 128, 255, q[3]]);
            let alpha = 128.0 / 255.0;
            let shadow = q[3] as f64 / 255.0;
            let share = shadow / (shadow + alpha * (1.0 - shadow));
            let linear = |x: f64| {
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            };
            let encode = |x: f64| {
                if x <= 0.0031308 {
                    x * 12.92
                } else {
                    1.055 * x.powf(1.0 / 2.4) - 0.055
                }
            };
            let picked = [0.0, 128.0 / 255.0, 1.0];
            let want: [u8; 4] = std::array::from_fn(|i| {
                if i == 3 {
                    128
                } else {
                    let picked = if context.space == Space::Linear {
                        linear(picked[i])
                    } else {
                        picked[i]
                    };
                    let value = (if i == 0 { 1.0 - share } else { 0.0 }) + picked * share;
                    ((if context.space == Space::Linear {
                        encode(value)
                    } else {
                        value
                    }) * 255.0)
                        .round() as u8
                }
            });
            assert_px(&on, 50, 33, want);
            for y in 30..70 {
                for x in 30..70 {
                    assert_eq!(px(&on, x, y)[3], 128);
                }
            }
        }
    }
    /// object-drop-shadow-plus.md Behaviour Drop Shadow: exact chosen colour, truncated strength and source alpha.
    #[test]
    fn drop_distance_zero_and_fractional_source() {
        let src = solid(10, 10, [10, 20, 30, 128]);
        for context in [super::GPU_CONTEXT, super::ALT_GPU_CONTEXT] {
            let out = bb_ctx(
                "bbDropShadowPlus",
                context,
                json!({"distance":0,"color":"#4080c0","shadowOnly":true}),
                &src,
            );
            for y in 0..10 {
                for x in 0..10 {
                    assert_px(&out, x, y, [64, 128, 192, 96]);
                }
            }
        }
    }
    /// object-drop-shadow-plus.md Long Shadow steps 5–6: exact strength and optional Gaussian softening.
    #[test]
    fn long_exact_strength_blur_and_zero_distance() {
        let out = ds(
            json!({"shadowType":1,"longDistance":20,"edges":"aliased","strength":0.5019,"shadowOnly":true,"color":"#4080c0"}),
        );
        assert_px_tol(&out, 48, 48, [64, 128, 192, 128], 1);
        let hard =
            ds(json!({"shadowType":1,"longDistance":20,"edges":"antiAliased","shadowOnly":true}));
        let blurred =
            ds(json!({"shadowType":1,"longDistance":20,"edges":"blurred","shadowOnly":true}));
        assert_ne!(hard.data, blurred.data);
        let zero = ds(
            json!({"shadowType":1,"longDistance":0,"angle":0,"edges":"aliased","shadowOnly":true,"strength":1}),
        );
        assert_eq!(px(&zero, 20, 30), [0; 4]);
        assert_eq!(px(&zero, 21, 30), [0, 0, 0, 255]);
        assert_eq!(px(&zero, 41, 30), [0, 0, 0, 255]);
        assert_eq!(px(&zero, 42, 30), [0; 4]);
    }
    /// object-drop-shadow-plus.md Behaviour: shape ignores selection, host clipping restores its exterior.
    #[test]
    fn object_effects_clip_without_using_selection_shape() {
        let src = square(60, 20, 40);
        let (m, p) = rect_selection(60, 60, 25, 25, 50, 50);
        let env = with_selection(60, 60, &m, &p);
        for (id, values) in [
            ("bbInnerShadowObject", json!({})),
            ("bbDropShadowPlus", json!({})),
            (
                "bbDropShadowPlus",
                json!({"shadowType":1,"longDistance":20}),
            ),
            ("bbDropShadowPlus", json!({"shadowType":2})),
        ] {
            let whole = bb(id, values.clone(), &src);
            let selected = bb_env(id, values, &src, &env);
            for y in 0..60 {
                for x in 0..60 {
                    assert_eq!(
                        px(&selected, x, y),
                        if (25..50).contains(&x) && (25..50).contains(&y) {
                            px(&whole, x, y)
                        } else {
                            px(&src, x, y)
                        }
                    );
                }
            }
        }
    }
    /// object-inner-shadow.md / selection-inner-shadow.md Controls; object-drop-shadow-plus.md Controls.
    #[test]
    fn shadow_metadata_and_tab_persistence() {
        use crate::effect::{
            bb::ui::Help,
            find,
            session::{EffectSession, repeat_last},
        };
        use crate::testing::fresh_doc;
        for (id, title) in [
            ("bbInnerShadowObject", "Object Inner Shadow"),
            ("bbInnerShadowSelection", "Selection Inner Shadow"),
            ("bbDropShadowPlus", "Drop Shadow+"),
        ] {
            let def = find(id).unwrap();
            assert_eq!(def.dialog_title, Some(title));
            let ui = def.indirect.unwrap();
            assert!(
                def.controls
                    .iter()
                    .filter(|c| !matches!(c, crate::effect::Control::Value { .. }))
                    .all(|c| !ui.control(c.key()).header_line)
            );
            assert!(!ui.control("color").alpha && ui.control("color").reset);
            if id != "bbDropShadowPlus" {
                assert_eq!(
                    ui.tabs[0]
                        .tabs
                        .iter()
                        .map(|t| t.caption)
                        .collect::<Vec<_>>(),
                    [" Shadow ", " Options "]
                );
            } else {
                assert_eq!(ui.width, 1.1);
                assert!(matches!(ui.help, Help::Text(["Drop Shadow+ v1.0"])));
            }
            let mut app = fresh_doc(8, 8, None);
            let session = EffectSession::begin(&mut app, def.clone()).unwrap();
            let mut values = session.defaults();
            let tab = if id == "bbDropShadowPlus" {
                "shadowType"
            } else {
                "tab"
            };
            values.insert(tab.into(), json!(1));
            values.insert("strength".into(), json!(0.42));
            session.cancel(&mut app);
            assert!(!app.effect_tokens.contains_key(id));
            let session = EffectSession::begin(&mut app, def.clone()).unwrap();
            session.commit(&mut app, values);
            let token = &app.effect_tokens[id];
            assert_eq!(token["strength"], json!(0.42));
            if tab == "tab" {
                assert!(!token.contains_key(tab));
                assert!(!app.last_effect_values.as_ref().unwrap().contains_key(tab));
            } else {
                assert_eq!(token[tab], json!(1));
            }
            let reopened = EffectSession::begin(&mut app, def).unwrap();
            let initial = reopened.initial_values(&app);
            assert_eq!(
                initial.get(tab).cloned().unwrap_or(json!(0)),
                json!(if tab == "tab" { 0 } else { 1 })
            );
            assert_eq!(initial["strength"], json!(0.42));
            assert!(repeat_last(&mut app));
            assert_eq!(app.effect_tokens[id]["strength"], json!(0.42));
        }
    }

    /// selection-inner-shadow.md Behaviour 2,6: antialiased shape and host clip are separate coverages.
    #[test]
    fn fractional_selection_coverage_and_transparent_source() {
        use crate::pixel::{
            IntRect,
            mask::{SelectionQuality, rasterize},
        };
        let p = vec![vec![[20.5, 20.5], [80.5, 20.5], [80.5, 80.5], [20.5, 80.5]]];
        let m = rasterize(
            &p,
            SelectionQuality::HighQuality,
            IntRect::new(0, 0, 100, 100),
        );
        let env = with_selection(100, 100, &m, &p);
        let src = solid(100, 100, [0; 4]);
        let out = bb_env("bbInnerShadowSelection", json!({}), &src, &env);
        assert_eq!(px(&out, 19, 50), [0; 4]);
        assert!(px(&out, 20, 50)[3] > 0);
        assert!(px(&out, 20, 50)[3] < px(&out, 21, 50)[3]);
        assert_eq!(px(&out, 50, 50), [0; 4]);
    }
    /// object-drop-shadow-plus.md 3D Shadow: hidden Light Angle and transform settings still affect pixels.
    #[test]
    fn three_d_hidden_angle_and_transform_controls() {
        let base = ds(json!({"shadowType":2,"shadowOnly":true,"color":"#ff0000"}));
        assert_eq!(
            base.data,
            ds(json!({"shadowType":2,"shadowOnly":true,"color":"#00ff00"})).data
        );
        for setting in [
            json!({"angle":45}),
            json!({"tiltAmount":0}),
            json!({"location":[0.5,-0.2]}),
            json!({"zoom":0.5}),
        ] {
            let mut values = json!({"shadowType":2,"shadowOnly":true});
            values
                .as_object_mut()
                .unwrap()
                .extend(setting.as_object().unwrap().clone());
            assert_ne!(ds(values).data, base.data);
        }
    }
}
