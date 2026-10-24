//! BB spec: effects/object-bevel-object.md; effects/selection-bevel-selection.md; common.md §1–5

use super::blocks::{
    Shade, blur_soft, bokeh_with_edge, colour_raw, drop_shadow_with, emboss_surface, lighting,
    quantise8, shadow_offset, straight_over,
};
use super::conventions::*;
use super::manifest::manifest;
use super::raster::{fill_coverage, polygon_path, selection_or_canvas};
use super::ui::{ControlUi, IndirectUi, TRACK_WHITE_BLACK, Tab, TabStrip};
use crate::effect::surface::{Alpha, Edge, EffectSurface, premultiply, unpremultiply};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, color_value, flag, num, range,
};
use serde_json::json;

const fn precision(decimals: usize) -> ControlUi {
    ControlUi {
        decimals: Some(decimals),
        ..ControlUi::NO_HEADER
    }
}
const ANGLE_UI: ControlUi = ControlUi {
    wrap: true,
    ..precision(3)
};
static OBJECT_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: " Shadow ",
                keys: &[
                    "angle",
                    "depth",
                    "strength",
                    "hardEdges",
                    "keepBackground",
                    "dropShadow",
                ],
            },
            Tab {
                caption: " Colors ",
                keys: &["light", "dark"],
            },
        ],
    }],
    controls: &[
        ("angle", ANGLE_UI),
        ("depth", precision(0)),
        ("strength", precision(2)),
        ("hardEdges", ControlUi::NO_HEADER),
        ("keepBackground", ControlUi::NO_HEADER),
        ("dropShadow", ControlUi::NO_HEADER),
        ("light", ControlUi::NO_HEADER),
        ("dark", ControlUi::NO_HEADER),
    ],
    transient_keys: &["tab"],
    enabled: |v, key| key != "dropShadow" || flag(v, "keepBackground"),
    ..IndirectUi::DEFAULT
};
static SELECTION_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: " Shadow ",
                keys: &[
                    "angle",
                    "depth",
                    "strength",
                    "hardEdges",
                    "keepBackground",
                    "keepAlpha",
                ],
            },
            Tab {
                caption: " Colors ",
                keys: &["light", "dark"],
            },
        ],
    }],
    controls: &[
        ("angle", ANGLE_UI),
        ("depth", precision(0)),
        (
            "strength",
            ControlUi {
                track: Some(&TRACK_WHITE_BLACK),
                ..precision(2)
            },
        ),
        ("hardEdges", ControlUi::NO_HEADER),
        ("keepBackground", ControlUi::NO_HEADER),
        ("keepAlpha", ControlUi::NO_HEADER),
        ("light", ControlUi::NO_HEADER),
        ("dark", ControlUi::NO_HEADER),
    ],
    transient_keys: &["tab"],
    enabled: |v, key| key != "keepAlpha" || flag(v, "keepBackground"),
    ..IndirectUi::DEFAULT
};
pub fn effects() -> Vec<EffectDef> {
    [
        ("bbBevelObject", "Bevel Object", false),
        ("bbBevelSelection", "Bevel Selection", true),
    ]
    .into_iter()
    .map(|(id, title, selection)| {
        let m = manifest(id).unwrap();
        EffectDef {
            id,
            icon: m.icon,
            category: m.category,
            controls: vec![
                Control::Value {
                    key: "tab",
                    default: || json!(0),
                },
                // R107: numeric AngleChooser step 1; R14 replacement slider uses small1/large5.
                range("angle", "Angle", 0.0, 360.0, 315.0).slider_keys(3, 1.0, 5.0, false),
                range("depth", "Depth", 1.0, 50.0, 5.0),
                // R120: omitted numeric increment inherits 1, independently of .01/.1 slider steps.
                range("strength", "Strength", 0.0, 2.0, 1.0).slider_keys(2, 0.01, 0.1, false),
                Control::Checkbox {
                    key: "hardEdges",
                    label: "Hard edges",
                    default: false,
                },
                Control::Checkbox {
                    key: "keepBackground",
                    label: "Keep original background image",
                    default: true,
                },
                Control::Checkbox {
                    key: if selection { "keepAlpha" } else { "dropShadow" },
                    label: if selection {
                        "Keep original alpha"
                    } else {
                        "Add drop shadow"
                    },
                    default: selection,
                },
                Control::Color {
                    key: "light",
                    label: "Light Color",
                    default: ColorDefault::Rgb(0xFFFFFF),
                },
                Control::Color {
                    key: "dark",
                    label: "Dark Color",
                    default: ColorDefault::Rgb(0),
                },
            ],
            kind: EffectKind::Gpu(
                GPU_CONTEXT,
                if selection {
                    selection_bevel
                } else {
                    object_bevel
                },
            ),
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(if selection { &SELECTION_UI } else { &OBJECT_UI }),
        }
    })
    .collect()
}

/// Steps 2–4 on a premultiplied height image; output RGB is straight relief, alpha is retained.
/// Gaussian consumes premultiplied height. Bokeh consumes straight height and owns its internal
/// premultiply/unpremultiply pair. Quantisation is on straight values before Emboss's RGB/alpha.
pub(crate) fn relief(height: &EffectSurface, depth: f64, hard: bool, angle: f64) -> EffectSurface {
    let soft = if hard {
        // approximation: custom-exponent option interpretation (object-bevel-object.md Behaviour 2)
        // spec-question: Q34 Hard-edge custom exponent order
        let power = if HARD_EDGE_POWER_FIRST {
            3.0
        } else {
            1.0 / 3.0
        };
        let pre = EffectSurface::from_fn(height.w, height.h, |x, y| {
            let p = unpremultiply(height.at(x, y));
            [p[0].powf(power), p[1].powf(power), p[2].powf(power), p[3]]
        });
        let blurred = bokeh_with_edge(&pre, depth, 10, 0.0, Edge::Transparent);
        EffectSurface::from_fn(height.w, height.h, |x, y| {
            let p = blurred.at(x, y);
            let f = |c: f32| {
                if HARD_EDGE_POWER_FIRST {
                    c.cbrt()
                } else {
                    c.powi(3)
                }
            };
            [f(p[0]), f(p[1]), f(p[2]), p[3]]
        })
    } else {
        // quirk(bb): object-bevel-object.md Behaviour 2 supplies sigma directly, not a radius.
        let blurred = blur_soft(height, depth / 3.0);
        EffectSurface::from_fn(height.w, height.h, |x, y| unpremultiply(blurred.at(x, y)))
    };
    relief_from_soft(&soft, angle)
}

/// The shared tail of step 3–4 (also Text Fun Factory's, R159) on straight softened RGBA: each
/// channel quantised to 8 bits, premultiplied for Emboss (which divides RGB by alpha), Emboss at
/// `angle`, recentred on `RELIEF_FLAT`. Output RGB is raw, unclamped straight relief; alpha is
/// retained.
pub(crate) fn relief_from_soft(soft: &EffectSurface, angle: f64) -> EffectSurface {
    let quantised = EffectSurface::from_fn(soft.w, soft.h, |x, y| {
        premultiply(soft.at(x, y).map(quantise8))
    });
    let embossed = emboss_surface(&quantised, angle + EMBOSS_ANGLE_OFFSET_DEG);
    EffectSurface::from_fn(soft.w, soft.h, |x, y| {
        let p = embossed.at(x, y);
        // spec-question: Q16 Emboss flat level
        let r = p[0] - 128.0 / 255.0 + RELIEF_FLAT;
        [r, r, r, p[3]]
    })
}
fn object_bevel(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    render(src, v, env, false)
}
fn selection_bevel(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    render(src, v, env, true)
}
fn render(src: &EffectSurface, v: &Values, env: &EffectEnv, selection: bool) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let height = if selection {
        // approximation: device-default antialiasing (selection-bevel-selection.md Behaviour 1)
        // spec-question: Q35 Bevel Selection default fill antialiasing
        let coverage = polygon_path(&selection_or_canvas(env), [0.0; 2], true).map_or_else(
            || vec![0.0; src.px.len()],
            |p| fill_coverage(&p, src.w, src.h, BEVEL_SELECTION_ANTIALIAS),
        );
        EffectSurface::from_fn(src.w, src.h, |x, y| {
            let h = 1.0 - coverage[(y * src.w + x) as usize];
            [h, h, h, 1.0]
        })
    } else {
        EffectSurface::from_fn(src.w, src.h, |x, y| {
            let h = 1.0 - src.at(x, y)[3];
            [h, h, h, 1.0]
        })
    };
    let depth = num(v, "depth").clamp(1.0, 50.0);
    let strength = num(v, "strength").clamp(0.0, 2.0);
    let angle = num(v, "angle").clamp(0.0, 360.0);
    let relief = relief(&height, depth, flag(v, "hardEdges"), angle);
    let light = colour_raw(color_value(v, "light", env));
    let dark = colour_raw(color_value(v, "dark", env));
    let keep = flag(v, "keepBackground");
    let out = EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = match info.input {
            Alpha::Premultiplied => unpremultiply(src.at(x, y)),
            Alpha::Straight => src.at(x, y),
        };
        let layer = lighting(
            relief.at(x, y)[0],
            strength as f32,
            if selection { 0.0 } else { 0.001 },
        )
        .map_or([0.0; 4], |(shade, o)| {
            let c = match shade {
                Shade::Light => light,
                Shade::Dark => dark,
            };
            [c[0], c[1], c[2], o]
        });
        let mut c = if keep { straight_over(p, layer) } else { layer };
        if keep && (!selection || flag(v, "keepAlpha")) {
            c[3] = p[3];
        }
        if !keep && !selection {
            c[3] = p[3].min(layer[3]);
        }
        premultiply(c)
    });
    let out = if !selection && keep && flag(v, "dropShadow") {
        // quirk(bb): object-bevel-object.md Behaviour 6 truncates shadow alpha before filtering.
        let a = ((255.0 * strength / 2.0).floor() / 255.0) as f32;
        drop_shadow_with(
            &out,
            depth * RADIUS_TO_SIGMA,
            shadow_offset(depth, angle + SHADOW_ANGLE_OFFSET_DEG),
            [0.0, 0.0, 0.0, a],
            false,
        )
    } else {
        out
    };
    match info.output {
        Alpha::Premultiplied => out,
        Alpha::Straight => EffectSurface::from_fn(src.w, src.h, |x, y| unpremultiply(out.at(x, y))),
    }
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::test_util::*;
    use serde_json::{Value, json};
    const G: [u8; 4] = [128, 128, 128, 255];
    fn t() -> ImageData {
        from_fn(100, 100, |x, y| {
            if (30..70).contains(&x) && (30..70).contains(&y) {
                G
            } else {
                [0; 4]
            }
        })
    }
    fn bs(src: &ImageData, v: Value) -> ImageData {
        let (m, p) = rect_selection(100, 100, 30, 30, 70, 70);
        bb_env(
            "bbBevelSelection",
            v,
            src,
            &with_selection(100, 100, &m, &p),
        )
    }
    fn outside(out: &ImageData, want: [u8; 4]) {
        for y in 0..100 {
            for x in 0..100 {
                if !(30..70).contains(&x) || !(30..70).contains(&y) {
                    assert_eq!(px(out, x, y), want, "outside ({x},{y})");
                }
            }
        }
    }
    fn edges(out: &ImageData) {
        for (x, y) in [(30, 50), (50, 30)] {
            let p = px(out, x, y);
            assert!(p[..3].iter().all(|&c| c >= 250), "{x},{y}: {p:?}");
            assert_eq!(p[3], 255);
        }
        for (x, y) in [(69, 50), (50, 69)] {
            let p = px(out, x, y);
            assert!(p[..3].iter().all(|&c| c <= 5 + TOL), "{x},{y}: {p:?}");
            assert_eq!(p[3], 255);
        }
        for y in 38..62 {
            for x in 38..62 {
                assert_eq!(px(out, x, y), G);
            }
        }
    }
    fn corners(out: &ImageData) {
        assert_px_tol(out, 69, 30, G, 10);
        assert_px_tol(out, 30, 69, G, 10);
        assert!(px(out, 30, 30)[..3].iter().all(|&c| c >= 230));
        assert!(px(out, 69, 69)[..3].iter().all(|&c| c <= 25));
    }
    fn only(out: &ImageData) {
        assert_px(out, 30, 50, [255, 255, 255, 252]);
        assert_px(out, 69, 50, [0, 0, 0, 252]);
        assert_eq!(px(out, 50, 50)[3], 0);
    }
    /// object-bevel-object.md Expected results 1.
    #[test]
    fn object_opaque_is_flat() {
        let src = solid(50, 50, G);
        assert_eq!(bb("bbBevelObject", json!({}), &src).data, src.data);
        assert!(
            bb("bbBevelObject", json!({"keepBackground":false}), &src)
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 0)
        );
    }
    /// object-bevel-object.md Expected results 2.
    #[test]
    fn object_edges_and_interior() {
        let out = bb("bbBevelObject", json!({}), &t());
        edges(&out);
        outside(&out, [0; 4]);
        for c in &px(&out, 33, 50)[..3] {
            assert!(*c > 140 && *c < 250, "{c}");
        }
    }
    /// object-bevel-object.md Expected results 3.
    #[test]
    fn object_corner_symmetry() {
        corners(&bb("bbBevelObject", json!({}), &t()));
    }
    /// object-bevel-object.md Expected results 4.
    #[test]
    fn object_lighting_only() {
        let out = bb("bbBevelObject", json!({"keepBackground":false}), &t());
        only(&out);
        outside(&out, [0; 4]);
    }
    /// object-bevel-object.md Expected results 5.
    #[test]
    fn object_strength_and_shadow() {
        let src = t();
        assert_eq!(
            bb(
                "bbBevelObject",
                json!({"strength":0,"dropShadow":true}),
                &src
            )
            .data,
            src.data
        );
        let out = bb("bbBevelObject", json!({"dropShadow":true}), &src);
        for (x, y) in [(72, 50), (50, 72)] {
            let p = px(&out, x, y);
            assert!(p[3] > 0 && p[3] <= 127 + TOL);
            assert!(p[..3].iter().all(|&c| c <= TOL));
        }
        assert_eq!(px(&out, 25, 50)[3], 0);
    }
    /// selection-bevel-selection.md Expected results 1.
    #[test]
    fn selection_edges_and_interior() {
        let out = bs(&solid(100, 100, G), json!({}));
        edges(&out);
        outside(&out, G);
    }
    /// selection-bevel-selection.md Expected results 2.
    #[test]
    fn selection_corner_symmetry() {
        corners(&bs(&solid(100, 100, G), json!({})));
    }
    /// selection-bevel-selection.md Expected results 3.
    #[test]
    fn selection_lighting_only() {
        let out = bs(&solid(100, 100, G), json!({"keepBackground":false}));
        only(&out);
        outside(&out, G);
    }
    /// selection-bevel-selection.md Expected results 4.
    #[test]
    fn selection_keep_alpha() {
        let src = solid(100, 100, [0; 4]);
        assert!(
            bs(&src, json!({}))
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 0)
        );
        let out = bs(&src, json!({"keepAlpha":false}));
        only(&out);
        outside(&out, [0; 4]);
    }
    /// selection-bevel-selection.md Expected results 5.
    #[test]
    fn selection_no_ops() {
        let src = solid(100, 100, G);
        assert_eq!(bs(&src, json!({"strength":0})).data, src.data);
        assert_eq!(bb("bbBevelSelection", json!({}), &src).data, src.data);
    }
    fn ctx_selection(info: crate::effect::RenderInfo, v: Value, src: &ImageData) -> ImageData {
        let (m, p) = rect_selection(100, 100, 30, 30, 70, 70);
        let mut def = crate::effect::find("bbBevelSelection").unwrap();
        let crate::effect::EffectKind::Gpu(_, render) = def.kind else {
            unreachable!()
        };
        def.kind = crate::effect::EffectKind::Gpu(info, render);
        crate::effect::apply(
            src,
            &def,
            &crate::effect::with(&def, &v),
            &with_selection(100, 100, &m, &p),
        )
    }
    /// R124/common.md §1: a capped black overlay leaves linear byte7 versus gamma byte1.
    #[test]
    fn capped_dark_edge_in_both_working_spaces() {
        use crate::effect::surface::{Alpha, Space};
        for (space, want) in [(Space::Linear, 7), (Space::Srgb, 1)] {
            let info = crate::effect::RenderInfo::new(space, Alpha::Premultiplied);
            let out = bb_ctx("bbBevelObject", info, json!({}), &t());
            assert_eq!(px(&out, 69, 50), [want, want, want, 255]);
            let out = ctx_selection(info, json!({}), &solid(100, 100, G));
            assert_eq!(px(&out, 69, 50), [want, want, want, 255]);
        }
    }
    /// Behaviour 5/6: raw lighting bytes, straight source-over and restored/free alpha are independent.
    #[test]
    fn effective_contexts_and_partial_alpha() {
        use crate::effect::surface::{Alpha, Space};
        for (space, lit, shade) in [
            (Space::Srgb, [128, 64, 33], [32, 64, 128]),
            (Space::Linear, [187, 137, 100], [99, 137, 188]),
        ] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = crate::effect::RenderInfo {
                        space,
                        input,
                        output,
                    };
                    let src = solid(100, 100, [64, 128, 192, 128]);
                    for (keep_alpha, a) in [(true, 128), (false, 254)] {
                        let out = ctx_selection(
                            info,
                            json!({"light":"#804020","dark":"#204080","keepAlpha":keep_alpha}),
                            &src,
                        );
                        assert_px(&out, 30, 50, [lit[0], lit[1], lit[2], a]);
                        assert_px(&out, 69, 50, [shade[0], shade[1], shade[2], a]);
                        assert_px(&out, 50, 50, [64, 128, 192, 128]);
                        outside(&out, [64, 128, 192, 128]);
                    }
                    for id in ["bbBevelObject", "bbBevelSelection"] {
                        let out = bb_ctx(id, info, json!({"strength":0}), &t());
                        assert_eq!(out.data, t().data);
                    }
                    let out = bb_ctx(
                        "bbBevelObject",
                        info,
                        json!({"keepBackground":false,"light":"#808080","dark":"#808080"}),
                        &t(),
                    );
                    let c = if space == Space::Srgb { 128 } else { 188 };
                    assert_px(&out, 30, 50, [c, c, c, 252]);
                    assert_px(&out, 69, 50, [c, c, c, 252]);
                }
            }
        }
    }
    /// Behaviour 6: Object preserves partial alpha, lighting-only takes min, and dropShadow is ignored off.
    #[test]
    fn object_partial_alpha_and_disabled_shadow() {
        for a in [1, 64, 128, 192, 254] {
            let src = from_fn(100, 100, |x, y| {
                if (30..70).contains(&x) && (30..70).contains(&y) {
                    [70, 120, 180, a]
                } else {
                    [0; 4]
                }
            });
            let out = bb("bbBevelObject", json!({}), &src);
            assert!(
                out.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(src.data.as_chunks::<4>().0)
                    .all(|(p, q)| p[3] == q[3])
            );
            let off = bb("bbBevelObject", json!({"keepBackground":false}), &src);
            assert!(
                off.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(src.data.as_chunks::<4>().0)
                    .all(|(p, q)| p[3] <= q[3])
            );
            assert_eq!(
                off.data,
                bb(
                    "bbBevelObject",
                    json!({"keepBackground":false,"dropShadow":true}),
                    &src
                )
                .data
            );
        }
    }
    /// Steps 2–4: height alpha is normalized exactly once; flat relief is .5 even after soft canvas loss.
    #[test]
    fn relief_representation_and_flat_level() {
        use crate::effect::surface::EffectSurface;
        for hard in [false, true] {
            for size in [(1, 1), (3, 2), (40, 30)] {
                let zero = EffectSurface::from_fn(size.0, size.1, |_, _| [0.0, 0.0, 0.0, 1.0]);
                let out = super::relief(&zero, 5.0, hard, 315.0);
                assert!(out.px.iter().all(|p| p[0] == 0.5));
            }
            let h = |a| {
                EffectSurface::from_fn(40, 30, |x, _| {
                    let v = if x < 20 { 0.1 } else { 0.9 };
                    [v * a, v * a, v * a, a]
                })
            };
            let opaque = super::relief(&h(1.0), 5.0, hard, 315.0);
            let partial = super::relief(&h(0.5), 5.0, hard, 315.0);
            for (a, b) in opaque.px.iter().zip(&partial.px) {
                assert!((a[0] - b[0]).abs() < 1e-5, "{hard}: {a:?} {b:?}");
            }
        }
    }
    /// Q34: power-then-root lifts a small disc overlap at the inner border; reversed order does not.
    #[test]
    fn hard_edges_exponent_order_has_visible_inner_slope() {
        let h = crate::effect::surface::EffectSurface::from_fn(64, 32, |x, _| {
            let v = if x < 32 { 0.0 } else { 0.125 };
            [v, v, v, 1.0]
        });
        let out = super::relief(&h, 5.0, true, 0.0);
        assert!(
            (out.at(28, 16)[0] - 0.5).abs() > 0.005,
            "{:?}",
            out.at(28, 16)
        );
    }
    /// Shared geometry pipeline: identical masks give the same selected pixels for soft and hard edges.
    #[test]
    fn hard_and_soft_object_selection_agree() {
        for hard in [false, true] {
            for depth in [1, 5, 15] {
                let a = bb(
                    "bbBevelObject",
                    json!({"hardEdges":hard,"depth":depth}),
                    &t(),
                );
                let b = bs(&solid(100, 100, G), json!({"hardEdges":hard,"depth":depth}));
                for y in 30..70 {
                    for x in 30..70 {
                        assert_eq!(px(&a, x, y), px(&b, x, y));
                    }
                }
            }
        }
    }
    /// Selection canvas fallback and explicit Select All agree; a curved outline lights upper-left.
    #[test]
    fn select_all_and_curved_outline() {
        for hard in [false, true] {
            let src = from_fn(40, 30, |x, y| [(x * 6) as u8, (y * 8) as u8, 90, 255]);
            let p = vec![vec![[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]]];
            let mut env = crate::effect::EffectEnv::whole(40, 30);
            env.selection = &p;
            assert_eq!(
                bb_env("bbBevelSelection", json!({"hardEdges":hard}), &src, &env).data,
                src.data
            );
            assert_eq!(
                bb("bbBevelSelection", json!({"hardEdges":hard}), &src).data,
                src.data
            );
            assert!(
                bb(
                    "bbBevelSelection",
                    json!({"hardEdges":hard,"keepBackground":false}),
                    &src
                )
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 0)
            );
        }
        let (m, p) = ellipse_selection(80, 80, [40.0, 40.0], [30.0, 20.0]);
        let out = bb_env(
            "bbBevelSelection",
            json!({}),
            &solid(80, 80, G),
            &with_selection(80, 80, &m, &p),
        );
        assert!(px(&out, 14, 35)[0] > 160);
        assert_px(&out, 40, 40, G);
        assert_px(&out, 2, 2, G);
    }
    /// Object geometry ignores the selection, which clips its completed lighting result once.
    #[test]
    fn object_selection_only_clips() {
        let src = t();
        let full = bb("bbBevelObject", json!({}), &src);
        let (m, p) = rect_selection(100, 100, 20, 20, 50, 80);
        let out = bb_env(
            "bbBevelObject",
            json!({}),
            &src,
            &with_selection(100, 100, &m, &p),
        );
        for y in 0..100 {
            for x in 0..100 {
                assert_eq!(
                    px(&out, x, y),
                    px(
                        if (20..50).contains(&x) && (20..80).contains(&y) {
                            &full
                        } else {
                            &src
                        },
                        x,
                        y
                    )
                );
            }
        }
    }
    /// Controls/common.md §3/4 and R107/R120: exact dialog contract and operative slider key metadata.
    #[test]
    fn metadata_and_slider_outcomes() {
        use crate::effect::bb::ui::{Help, HelpWindow};
        use crate::effect::{Control, EffectKind};
        for (id, last, caption, category, icon, title, checked) in [
            (
                "bbBevelObject",
                "dropShadow",
                "Add drop shadow",
                "object",
                "object_tools__bevel_object.png",
                "Bevel Object",
                false,
            ),
            (
                "bbBevelSelection",
                "keepAlpha",
                "Keep original alpha",
                "selection",
                "selection_tools__bevel_selection.png",
                "Bevel Selection",
                true,
            ),
        ] {
            let d = crate::effect::find(id).unwrap();
            assert_eq!(d.icon, icon);
            assert_eq!(d.dialog_title, Some(title));
            assert_eq!(d.category, crate::effect::Category::Effect(category));
            assert!(matches!(d.kind,EffectKind::Gpu(info,_) if info==super::GPU_CONTEXT));
            assert_eq!(
                d.controls.iter().map(Control::key).collect::<Vec<_>>(),
                vec![
                    "tab",
                    "angle",
                    "depth",
                    "strength",
                    "hardEdges",
                    "keepBackground",
                    last,
                    "light",
                    "dark"
                ]
            );
            for (key, label, colour) in [
                ("light", "Light Color", 0xFFFFFF),
                ("dark", "Dark Color", 0),
            ] {
                assert!(
                    matches!(d.controls.iter().find(|c|c.key()==key).unwrap(),Control::Color{label:found,default:crate::effect::ColorDefault::Rgb(c),..} if *found==label && *c==colour)
                );
            }
            let ui = d.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert_eq!(ui.transient_keys, &["tab"]);
            assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
            assert_eq!(ui.tabs[0].tabs[0].caption, " Shadow ");
            assert_eq!(ui.tabs[0].tabs[1].caption, " Colors ");
            assert_eq!(
                ui.tabs[0].tabs[0].keys,
                &[
                    "angle",
                    "depth",
                    "strength",
                    "hardEdges",
                    "keepBackground",
                    last
                ]
            );
            assert_eq!(ui.tabs[0].tabs[1].keys, &["light", "dark"]);
            for c in &d.controls[1..] {
                let u = ui.control(c.key());
                assert!(!u.header_line);
                assert!(u.reset);
                assert!(!u.alpha);
            }
            assert!(ui.control("angle").wrap);
            assert_eq!(ui.control("angle").decimals, Some(3));
            assert_eq!(ui.control("strength").decimals, Some(2));
            assert_eq!(
                ui.control("strength").track,
                if checked {
                    Some(&super::TRACK_WHITE_BLACK[..])
                } else {
                    None
                }
            );
            assert!(
                matches!(d.controls[6],Control::Checkbox{label,default,..} if label==caption&&default==checked)
            );
            let mut v = crate::effect::defaults(&d);
            assert!((ui.enabled)(&v, last));
            v.insert(last.into(), json!(true));
            v.insert("keepBackground".into(), json!(false));
            assert!(!(ui.enabled)(&v, last));
            assert_eq!(v[last], true);
            for (key, min, max, default, decimals, small, large) in [
                ("angle", 0.0, 360.0, 315.0, 3, 1.0, 5.0),
                ("depth", 1.0, 50.0, 5.0, 0, 1.0, 5.0),
                ("strength", 0.0, 2.0, 1.0, 2, 0.01, 0.1),
            ] {
                let Control::Range {
                    min: a,
                    max: b,
                    default: c,
                    step,
                    keyboard,
                    exponential,
                    ..
                } = d.controls.iter().find(|c| c.key() == key).unwrap()
                else {
                    panic!()
                };
                assert_eq!(
                    (*a, *b, *c, *step, *exponential),
                    (min, max, default, 1.0, false)
                );
                assert_eq!(
                    (
                        keyboard.decimals,
                        keyboard.small,
                        keyboard.large,
                        keyboard.exponential
                    ),
                    (decimals, small, large, false)
                );
                assert!(
                    (keyboard.stepped(default, min, max, true, false) - (default + small).min(max))
                        .abs()
                        < 1e-9
                );
                assert!(
                    (keyboard.stepped(default, min, max, false, true) - (default - large).max(min))
                        .abs()
                        < 1e-9
                );
            }
        }
    }

    /// Canvas masks are transparent off-image for both blur paths; a canvas-bound opaque edge is flat.
    #[test]
    fn canvas_boundaries_use_transparent_height() {
        use crate::effect::surface::EffectSurface;
        let h = EffectSurface::from_fn(9, 7, |x, _| {
            let v = x as f32 / 8.0;
            [v, v, v, 1.0]
        });
        for hard in [false, true] {
            let r = super::relief(&h, 3.0, hard, 315.0);
            assert!(
                r.at(0, 0)[3] > 0.0 && r.at(0, 0)[3] < 0.9,
                "{hard}: {:?}",
                r.at(0, 0)
            );
            let src = from_fn(40, 40, |x, _| if x < 20 { G } else { [0; 4] });
            let out = bb("bbBevelObject", json!({"hardEdges":hard}), &src);
            assert_eq!(px(&out, 0, 20), G);
            let out = bb(
                "bbBevelObject",
                json!({"hardEdges":hard,"keepBackground":false}),
                &src,
            );
            assert_eq!(px(&out, 0, 20)[3], 0);
        }
    }
    /// Behaviour 3 precedes Emboss: three close straight heights all quantise to 128/255.
    #[test]
    fn relief_quantisation_precedes_emboss() {
        let h = crate::effect::surface::EffectSurface::from_fn(3, 3, |x, _| {
            let v = [0.5001, 0.501, 0.5018][x as usize];
            [v, v, v, 1.0]
        });
        let r = super::relief(&h, 0.0, false, 0.0);
        assert!(r.px.iter().all(|p| p[0] == 0.5));
    }
    /// Behaviour 6 truncates the original double Strength before narrowing shadow alpha to float.
    #[test]
    fn shadow_byte_uses_full_numeric_precision() {
        use crate::effect::surface::EffectSurface;
        let src =
            EffectSurface::from_rgba8(&t(), super::GPU_CONTEXT.space, super::GPU_CONTEXT.input);
        let mut env = crate::effect::EffectEnv::whole(100, 100);
        env.render_info = Some(super::GPU_CONTEXT);
        let def = crate::effect::find("bbBevelObject").unwrap();
        let render = |s| {
            super::object_bevel(
                &src,
                &crate::effect::with(&def, &json!({"dropShadow":true,"strength":s})),
                &env,
            )
        };
        let full = render(2.0);
        let out = render(0.02352941166470588);
        // floor(255*.02352941166470588/2)=2 in double; float rounds up to3.
        let want = full.at(72, 50)[3] * (2.0 / 255.0);
        assert!(
            (out.at(72, 50)[3] - want).abs() < 1e-6,
            "{} != {want}",
            out.at(72, 50)[3]
        );
    }
    /// selection-bevel-selection.md Behaviour 1/Q35: fractional geometry changes the inner lighting fringe.
    #[test]
    fn selection_antialiasing_changes_fractional_fringe() {
        let (mut mask, _) = rect_selection(40, 40, 8, 0, 40, 40);
        for y in 0..40 {
            mask.data[y * 40 + 8] = 191;
        }
        let outline = vec![vec![[8.25, 0.0], [40.0, 0.0], [40.0, 40.0], [8.25, 40.0]]];
        let out = bb_env(
            "bbBevelSelection",
            json!({"depth":1,"angle":0,"strength":0.1,"keepBackground":false}),
            &solid(40, 40, G),
            &with_selection(40, 40, &mask, &outline),
        );
        // Independent reference-kernel arithmetic: AA gives quantised height8=81/255 and
        // height10=0. Emboss delta=(1+sqrt2)*81/255, giving alpha39.11. Aliased gives15.93.
        assert_px(&out, 9, 20, [255, 255, 255, 39]);
        assert_eq!(px(&out, 7, 20), G);
        assert_eq!(px(&out, 20, 20)[3], 0);
    }
    /// R159: Text Fun Factory shares `relief`'s quantise/Emboss/recentre tail without changing a
    /// Bevel Object or Bevel Selection byte (FNV-1a of each output, pinned at BB31's 3ee30cf).
    #[test]
    fn relief_tail_keeps_bevel_bytes() {
        let fnv = |b: &[u8]| {
            b.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &x| {
                (h ^ x as u64).wrapping_mul(0x100_0000_01b3)
            })
        };
        // An off-centre disc with a 3 px antialiased rim, in a colour.
        let src = from_fn(100, 100, |x, y| {
            let d = ((x as f32 - 48.3).powi(2) + (y as f32 - 51.7).powi(2)).sqrt();
            let a = ((25.0 - d) / 3.0).clamp(0.0, 1.0);
            [40, 90, 200, (a * 255.0).round() as u8]
        });
        let mut got = vec![];
        for hard in [false, true] {
            for (depth, angle) in [(7, 315), (3, 120)] {
                let v = json!({"hardEdges": hard, "depth": depth, "angle": angle});
                got.push(fnv(&bb("bbBevelObject", v.clone(), &src).data));
                got.push(fnv(&bs(&t(), v).data));
            }
        }
        assert_eq!(
            got,
            [
                13805484932370835640,
                4550275184142328524,
                13797266200300323450,
                2942243151833236237,
                6061171430044287377,
                10124050700008143450,
                12515680680014959820,
                3923902761893053611
            ]
        );
    }
}
