//! BB spec: effects/artistic-dream.md; effects/photo-vignette-plus.md; common.md §1–5

use super::blocks::{blur_soft, colour_exact, d2d_vignette};
use super::conventions::GPU_CONTEXT;
use super::manifest::manifest;
use super::ui::{ControlUi, IndirectUi};
use crate::effect::mix::{MixMode, mix};
use crate::effect::surface::{Alpha, EffectSurface, premultiply, unpremultiply};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, color_value, flag, num, range,
};
use serde_json::json;

static DREAM_UI: IndirectUi = IndirectUi {
    controls: &[
        (
            "radius",
            ControlUi {
                decimals: Some(0),
                ..ControlUi::NO_HEADER
            },
        ),
        ("vignette", ControlUi::NO_HEADER),
    ],
    defaults: |v, env| {
        v.insert(
            "radius".into(),
            json!((env.width.max(env.height) / 80).clamp(10, 200)),
        );
    },
    ..IndirectUi::DEFAULT
};
static VIGNETTE_UI: IndirectUi = IndirectUi {
    controls: &[
        ("color", ControlUi::NO_HEADER),
        (
            "radius",
            ControlUi {
                decimals: Some(2),
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "strength",
            ControlUi {
                decimals: Some(2),
                ..ControlUi::NO_HEADER
            },
        ),
    ],
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    [
        ("bbDream", "Dream", true),
        ("bbVignettePlus", "Vignette+", false),
    ]
    .into_iter()
    .map(|(id, title, dream)| {
        let m = manifest(id).unwrap();
        EffectDef {
            id,
            icon: m.icon,
            category: m.category,
            controls: if dream {
                vec![
                    range("radius", "Radius", 0.0, 200.0, 10.0).slider_keys(0, 1.0, 5.0, false),
                    Control::Checkbox {
                        key: "vignette",
                        label: "Include vignette",
                        default: true,
                    },
                ]
            } else {
                vec![
                    Control::Color {
                        key: "color",
                        label: "",
                        default: ColorDefault::Rgb(0),
                    },
                    range("radius", "Radius", 0.0, 1.0, 0.5).slider_keys(2, 1.0, 5.0, false),
                    range("strength", "Strength", 0.0, 1.0, 0.5).slider_keys(2, 1.0, 5.0, false),
                ]
            },
            kind: EffectKind::Gpu(
                GPU_CONTEXT,
                if dream { dream_render } else { vignette_render },
            ),
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(if dream { &DREAM_UI } else { &VIGNETTE_UI }),
        }
    })
    .collect()
}
fn dream_render(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    render(src, v, env, true)
}
fn vignette_render(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    render(src, v, env, false)
}
fn render(src: &EffectSurface, v: &Values, env: &EffectEnv, dream: bool) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    // Shared Gaussian, Darken and vignette blocks consume premultiplied pixels.
    let src = EffectSurface::from_fn(src.w, src.h, |x, y| match info.input {
        Alpha::Premultiplied => src.at(x, y),
        Alpha::Straight => premultiply(src.at(x, y)),
    });
    let out = if dream {
        // Dream supplies sigma directly; the shared radius convention does not apply.
        let blurred = blur_soft(&src, num(v, "radius").clamp(0.0, 200.0) / 3.0);
        let darkened = EffectSurface::from_fn(src.w, src.h, |x, y| {
            mix(MixMode::Darken, src.at(x, y), blurred.at(x, y))
        });
        if flag(v, "vignette") {
            d2d_vignette(&darkened, env.bounds, [0.0, 0.0, 0.0, 1.0], 0.8, 0.3)
        } else {
            darkened
        }
    } else {
        let mut colour = color_value(v, "color", env);
        colour[3] = 255;
        d2d_vignette(
            &src,
            env.bounds,
            colour_exact(colour, info),
            num(v, "radius").clamp(0.0, 1.0) as f32,
            num(v, "strength").clamp(0.0, 1.0) as f32,
        )
    };
    match info.output {
        Alpha::Premultiplied => out,
        Alpha::Straight => EffectSurface::from_fn(out.w, out.h, |x, y| unpremultiply(out.at(x, y))),
    }
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
    use crate::effect::bb::ui::{Help, HelpWindow};
    use crate::effect::bb::{indirect_defaults, test_util::*};
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::{Control, EffectEnv, EffectKind, RenderInfo};
    use serde_json::json;

    fn contexts() -> [RenderInfo; 2] {
        [GPU_CONTEXT, ALT_GPU_CONTEXT]
    }
    fn equal_pixels(a: &crate::canvas::ImageData, b: &crate::canvas::ImageData) {
        assert_eq!((a.width, a.height), (b.width, b.height));
        for y in 0..a.height {
            for x in 0..a.width {
                assert_px(a, x, y, px(b, x, y));
            }
        }
    }
    fn symmetric(img: &crate::canvas::ImageData, x0: u32, y0: u32, w: u32, h: u32) {
        for y in 0..h {
            for x in 0..w {
                assert_px(img, x0 + w - 1 - x, y0 + y, px(img, x0 + x, y0 + y));
                assert_px(img, x0 + x, y0 + h - 1 - y, px(img, x0 + x, y0 + y));
            }
        }
    }

    /// artistic-dream.md Expected results 1: includes transparent-border Gaussian taps at every edge.
    #[test]
    fn dream_uniform_unchanged() {
        let src = solid(200, 200, [128, 176, 255, 255]);
        for info in contexts() {
            equal_pixels(
                &bb_ctx("bbDream", info, json!({"radius":30,"vignette":false}), &src),
                &src,
            );
        }
    }
    /// artistic-dream.md Expected results 2: radius zero bypasses blur, without vignette.
    #[test]
    fn dream_zero_radius_identity() {
        let src = from_fn(30, 20, |x, y| [(x * 8) as u8, (y * 12) as u8, 99, 255]);
        for info in contexts() {
            equal_pixels(
                &bb_ctx("bbDream", info, json!({"radius":0,"vignette":false}), &src),
                &src,
            );
        }
    }
    /// artistic-dream.md Expected results 3: sigma10 edge and both prescribed working-space values.
    #[test]
    fn dream_darken_edge() {
        let src = from_fn(
            200,
            100,
            |x, _| if x < 100 { [0, 0, 0, 255] } else { [255; 4] },
        );
        for (info, want) in [(GPU_CONTEXT, 191), (ALT_GPU_CONTEXT, 133)] {
            let out = bb_ctx("bbDream", info, json!({"radius":30,"vignette":false}), &src);
            for y in 0..100 {
                for x in 0..100 {
                    assert_eq!(px(&out, x, y), [0, 0, 0, 255]);
                }
            }
            assert!(out.data.iter().zip(&src.data).all(|(o, i)| o <= i));
            assert_px(&out, 100, 50, [want, want, want, 255]);
            assert!(px(&out, 130, 50)[0] >= 253);
            assert!((100..130).all(|x| px(&out, x, 50)[0] <= px(&out, x + 1, 50)[0]));
            assert!(px(&out, 101, 50)[0] < 250);
        }
    }
    /// artistic-dream.md Expected results 4: all corners, all edge midpoints and both mirror axes.
    #[test]
    fn dream_vignette_geometry() {
        let src = solid(300, 200, [255; 4]);
        for info in contexts() {
            let out = bb_ctx("bbDream", info, json!({"radius":10}), &src);
            assert_px(&out, 150, 100, [255; 4]);
            symmetric(&out, 0, 0, 300, 200);
            for (x, y) in [(0, 0), (299, 0), (0, 199), (299, 199)] {
                for (mx, my) in [(150, 0), (150, 199), (0, 100), (299, 100)] {
                    assert!(px(&out, x, y)[0] < px(&out, mx, my)[0]);
                    assert!(px(&out, mx, my)[0] < px(&out, 150, 100)[0]);
                }
            }
        }
    }
    /// artistic-dream.md Expected results 5: bounds-centred vignette and exhaustive untouched exterior.
    #[test]
    fn dream_selection_fitting() {
        let src = solid(300, 200, [255; 4]);
        let (m, p) = rect_selection(300, 200, 0, 0, 100, 100);
        let env = with_selection(300, 200, &m, &p);
        let out = bb_env("bbDream", json!({"radius":10}), &src, &env);
        assert_px(&out, 50, 50, [255; 4]);
        symmetric(&out, 0, 0, 100, 100);
        let darkest = px(&out, 0, 0)[0];
        for y in 0..200 {
            for x in 0..300 {
                if x >= 100 || y >= 100 {
                    assert_eq!(px(&out, x, y), [255; 4]);
                } else {
                    assert!(px(&out, x, y)[0] >= darkest);
                }
            }
        }
        assert!(darkest < px(&out, 50, 50)[0]);
    }
    /// photo-vignette-plus.md Expected results 1: exhaustive left/right and top/bottom symmetry.
    #[test]
    fn vignette_symmetry() {
        for info in contexts() {
            symmetric(
                &bb_ctx(
                    "bbVignettePlus",
                    info,
                    json!({}),
                    &solid(401, 301, [255; 4]),
                ),
                0,
                0,
                401,
                301,
            );
        }
    }
    /// photo-vignette-plus.md Expected results 2: global corner minimum and all four centre/corner rays.
    #[test]
    fn vignette_centre_corners_monotonic() {
        for info in contexts() {
            let out = bb_ctx(
                "bbVignettePlus",
                info,
                json!({}),
                &solid(401, 301, [255; 4]),
            );
            assert_px(&out, 200, 150, [255; 4]);
            let darkest = px(&out, 0, 0)[0];
            assert!(out.data.as_chunks::<4>().0.iter().all(|p| p[0] >= darkest));
            for (right, down) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut prev = 255;
                for k in 0..=200 {
                    let dx = k;
                    let dy = 3 * k / 4;
                    let x = if right { 200 + dx } else { 200 - dx };
                    let y = if down { 150 + dy } else { 150 - dy };
                    let value = px(&out, x, y)[0];
                    assert!(value <= prev);
                    prev = value;
                }
            }
        }
    }
    /// photo-vignette-plus.md Expected results 3: opaque chosen red at each corner in either context.
    #[test]
    fn vignette_colour() {
        for info in contexts() {
            let out = bb_ctx(
                "bbVignettePlus",
                info,
                json!({"color":"#ff0000","strength":1,"radius":1}),
                &solid(401, 301, [255; 4]),
            );
            for (x, y) in [(0, 0), (400, 0), (0, 300), (400, 300)] {
                let c = px(&out, x, y);
                assert!(
                    c[0] >= 253 && c[1] < 128 && c[2] < 128 && c[3] == 255,
                    "{c:?}"
                );
            }
        }
    }
    /// photo-vignette-plus.md Expected results 4: whole-image strength-zero identity, including alpha.
    #[test]
    fn vignette_zero_strength_identity() {
        let src = from_fn(30, 20, |x, y| [(x * 8) as u8, (y * 12) as u8, 99, 128]);
        for info in contexts() {
            equal_pixels(
                &bb_ctx("bbVignettePlus", info, json!({"strength":0}), &src),
                &src,
            );
        }
    }
    /// photo-vignette-plus.md Expected results 5: fitted half-width ellipse and untouched right half.
    #[test]
    fn vignette_selection_fitting() {
        let src = solid(600, 400, [255; 4]);
        let (m, p) = rect_selection(600, 400, 0, 0, 300, 400);
        let out = bb_env(
            "bbVignettePlus",
            json!({}),
            &src,
            &with_selection(600, 400, &m, &p),
        );
        assert_px(&out, 150, 200, [255; 4]);
        symmetric(&out, 0, 0, 300, 400);
        assert!(px(&out, 0, 0)[0] < px(&out, 150, 200)[0]);
        for y in 0..400 {
            for x in 300..600 {
                assert_eq!(px(&out, x, y), [255; 4]);
            }
        }
    }
    /// artistic-dream.md Default Radius: full document dimensions, integer division and both clamps.
    #[test]
    fn dream_document_defaults() {
        let def = crate::effect::find("bbDream").unwrap();
        for ((w, h), r) in [
            ((800, 600), 10),
            ((400, 300), 10),
            ((1920, 1080), 24),
            ((4000, 3000), 50),
            ((20000, 500), 200),
            ((500, 4000), 50),
            ((1999, 10), 24),
        ] {
            let mut env = EffectEnv::whole(w, h);
            env.bounds = crate::pixel::IntRect::new(0, 0, 10, 10);
            assert_eq!(indirect_defaults(&def, &env)["radius"], json!(r));
        }
    }
    /// common.md §1, Q18: all effective alpha combinations, alpha union and source-over vignette.
    #[test]
    fn alpha_modes_and_effective_colour_context() {
        for space in [Space::Linear, Space::Srgb] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = RenderInfo {
                        space,
                        input,
                        output,
                    };
                    let src = solid(5, 5, [128, 64, 32, 128]);
                    let dream = bb_ctx("bbDream", info, json!({"radius":0,"vignette":false}), &src);
                    assert_px(&dream, 2, 2, [128, 64, 32, 192]);
                    let vignette = bb_ctx(
                        "bbVignettePlus",
                        info,
                        json!({"radius":1,"strength":1,"color":"#804020"}),
                        &solid(5, 5, [0; 4]),
                    );
                    assert_px(&vignette, 0, 0, [128, 64, 32, 231]);
                    assert_eq!(px(&vignette, 2, 2), [0; 4]);
                    let half = bb_ctx(
                        "bbVignettePlus",
                        info,
                        json!({"radius":1,"strength":1,"color":"#804020"}),
                        &src,
                    );
                    assert_px(&half, 0, 0, [128, 64, 32, 243]);
                    let fixed = bb_ctx(
                        "bbDream",
                        info,
                        json!({"radius":0}),
                        &solid(5, 5, [255, 255, 255, 128]),
                    );
                    assert_px(
                        &fixed,
                        0,
                        0,
                        if space == Space::Srgb {
                            [175, 175, 175, 208]
                        } else {
                            [216, 216, 216, 208]
                        },
                    );
                }
            }
        }
    }
    /// Full controls tables and common.md §3: metadata consumed by the actual generic dialog.
    #[test]
    fn dialog_metadata() {
        for (id, title, keys) in [
            ("bbDream", "Dream", vec!["radius", "vignette"]),
            (
                "bbVignettePlus",
                "Vignette+",
                vec!["color", "radius", "strength"],
            ),
        ] {
            let d = crate::effect::find(id).unwrap();
            assert_eq!(d.dialog_title, Some(title));
            assert_eq!(
                d.controls.iter().map(Control::key).collect::<Vec<_>>(),
                keys
            );
            assert!(matches!(d.kind,EffectKind::Gpu(info,_) if info==GPU_CONTEXT));
            let ui = d.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
            for c in &d.controls {
                let u = ui.control(c.key());
                assert!(!u.header_line && u.reset && !u.alpha && !u.wrap);
            }
            for c in &d.controls {
                if let Control::Range {
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
                {
                    let dream = id == "bbDream";
                    assert_eq!(
                        (*min, *max, *default, *step, *exponential),
                        (
                            0.0,
                            if dream { 200.0 } else { 1.0 },
                            if dream { 10.0 } else { 0.5 },
                            1.0,
                            false
                        )
                    );
                    assert_eq!(
                        *label,
                        if *key == "radius" {
                            "Radius"
                        } else {
                            "Strength"
                        }
                    );
                    assert_eq!(
                        (
                            keyboard.decimals,
                            keyboard.small,
                            keyboard.large,
                            keyboard.exponential
                        ),
                        (if dream { 0 } else { 2 }, 1.0, 5.0, false)
                    );
                    assert_eq!(ui.control(key).decimals, Some(if dream { 0 } else { 2 }));
                }
            }
            if id == "bbDream" {
                assert!(matches!(
                    d.controls[1],
                    Control::Checkbox {
                        label: "Include vignette",
                        default: true,
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    d.controls[0],
                    Control::Color {
                        label: "",
                        default: crate::effect::ColorDefault::Rgb(0),
                        ..
                    }
                ));
            }
        }
    }
}
