//! BB spec: effects/blurs-gaussian-blur-plus.md; effects/selection-blur-selection-edge.md; common.md §1–5
use super::blocks::{blur_soft, straight_over};
use super::classic::gaussian_blur;
use super::conventions::{BLUR_SELECTION_EDGE_PRESERVE_OUTSIDE_BAND, GPU_CONTEXT};
use super::manifest::manifest;
use super::raster::{self, Cap, Join, Pen};
use super::ui::{ControlUi, Help, HelpWindow, IndirectUi};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::effect::surface::{Alpha, EffectSurface, premultiply, unpremultiply};
use crate::effect::{
    Control, EffectDef, EffectEnv, EffectKind, RenderInfo, Values, choice, flag, num, range,
};
use crate::pixel::ops::compose;
use std::sync::LazyLock;

/// The same values, labels and order as Layer Properties.
pub static BLEND_CHOICES: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| BlendMode::ALL.map(|m| (m.value(), m.label())).to_vec());

static PLUS_UI: IndirectUi = IndirectUi {
    controls: &[
        ("red", ControlUi::NO_HEADER),
        ("green", ControlUi::NO_HEADER),
        ("blue", ControlUi::NO_HEADER),
        ("alpha", ControlUi::NO_HEADER),
        ("mostOpaque", ControlUi::NO_HEADER),
    ],
    help: Help::Window(HelpWindow::Older),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let plus = manifest("bbGaussianBlurPlus").unwrap();
    let edge = manifest("bbBlurSelectionEdge").unwrap();
    vec![
        EffectDef {
            id: plus.id,
            icon: plus.icon,
            category: plus.category,
            controls: vec![
                range("radius", "Radius", 0.0, 100.0, 2.0),
                Control::Checkbox {
                    key: "red",
                    label: "Red",
                    default: true,
                },
                Control::Checkbox {
                    key: "green",
                    label: "Green",
                    default: true,
                },
                Control::Checkbox {
                    key: "blue",
                    label: "Blue",
                    default: true,
                },
                Control::Checkbox {
                    key: "alpha",
                    label: "Alpha",
                    default: true,
                },
                Control::Checkbox {
                    key: "mostOpaque",
                    label: "Use most opaque alpha",
                    default: false,
                },
                Control::Select {
                    key: "blend",
                    label: "Blending Mode",
                    default: "normal",
                    choices: &BLEND_CHOICES,
                },
            ],
            kind: EffectKind::Cpu {
                render: gaussian_plus,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some("Gaussian Blur+ v4.2"),
            indirect: Some(&PLUS_UI),
        },
        EffectDef {
            id: edge.id,
            icon: edge.icon,
            category: edge.category,
            controls: vec![],
            kind: EffectKind::Cpu {
                render: selection_edge,
                clip_to_selection: false,
            },
            dialog: None,
            dialog_title: None,
            indirect: None,
        },
    ]
}

fn gaussian_plus(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let mut out = gaussian_blur(src, num(v, "radius") as u32);
    let channels = ["red", "green", "blue", "alpha"].map(|k| flag(v, k));
    let most = flag(v, "mostOpaque");
    let mode = BlendMode::from_value(choice(v, "blend"));
    for (q, s) in out
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.data.as_chunks::<4>().0)
    {
        for c in 0..4 {
            if !channels[c] {
                q[c] = s[c];
            }
        }
        if most {
            q[3] = q[3].max(s[3]);
        }
        // quirk(bb): blurs-gaussian-blur-plus.md radius 0 still composites the pixel over itself.
        *q = compose(mode, *s, *q, 255);
    }
    out
}

fn selection_edge(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
    selection_edge_with(src, env, GPU_CONTEXT)
}

// CPU dispatch deliberately has no RenderInfo. Both context columns exercise this same pipeline;
// only colour space varies, while the explicit premultiply/unpremultiply topology stays fixed.
fn selection_edge_with(src: &ImageData, env: &EffectEnv, info: RenderInfo) -> ImageData {
    let Some(path) = raster::polygon_path(&raster::selection_or_canvas(env), [0.0, 0.0], true)
    else {
        return src.clone();
    };
    let band = raster::stroke_coverage(
        &path,
        &Pen {
            width: 2.0,
            join: Join::Round,
            cap: Cap::Flat,
            miter_limit: 10.0,
            dash: None,
        },
        src.width,
        src.height,
        true,
    );
    let s = EffectSurface::from_rgba8(src, info.space, Alpha::Premultiplied);
    let mut blurred = blur_soft(&s, 1.0);
    for ((q, p), &coverage) in blurred.px.iter_mut().zip(&s.px).zip(&band) {
        let mut overlay = unpremultiply(*q);
        // quirk(bb): selection-blur-selection-edge.md "transparency is not preserved inside the band".
        overlay[3] = coverage;
        *q = premultiply(straight_over(unpremultiply(*p), overlay));
    }
    let mut out = blurred.to_rgba8(info.space, Alpha::Premultiplied);
    for ((p, s), &coverage) in out
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.data.as_chunks::<4>().0)
        .zip(&band)
    {
        // spec-question: Q29 Blur Selection Edge outside-band stored colour
        if coverage == 0.0 && BLUR_SELECTION_EDGE_PRESERVE_OUTSIDE_BAND {
            *p = *s;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::test_util::*;
    use crate::effect::{Control, EffectKind, find};
    use serde_json::json;
    const PLUS: &str = "bbGaussianBlurPlus";
    const EDGE: &str = "bbBlurSelectionEdge";
    fn checker(w: u32) -> crate::canvas::ImageData {
        from_fn(w, w, |x, y| {
            if (x + y) % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            }
        })
    }
    /// blurs-gaussian-blur-plus.md Expected results 1.
    #[test]
    fn radius_zero_semitransparent() {
        for alpha in [false, true] {
            assert_px_tol(
                &bb(
                    PLUS,
                    json!({"radius":0,"alpha":alpha}),
                    &solid(1, 1, [255, 0, 0, 128]),
                ),
                0,
                0,
                [255, 0, 0, 192],
                1,
            );
        }
    }
    /// blurs-gaussian-blur-plus.md Expected results 2: all checkbox combinations.
    #[test]
    fn radius_zero_opaque() {
        let img = from_fn(6, 4, |x, y| [(x * 40) as u8, (y * 60) as u8, 99, 255]);
        for bits in 0..32 {
            assert_eq!(
                bb(
                    PLUS,
                    json!({"radius":0,"red":bits&1!=0,"green":bits&2!=0,
                "blue":bits&4!=0,"alpha":bits&8!=0,"mostOpaque":bits&16!=0}),
                    &img
                )
                .data,
                img.data
            );
        }
    }
    /// blurs-gaussian-blur-plus.md Expected results 3.
    #[test]
    fn single_channel() {
        let img = from_fn(40, 20, |x, _| {
            if x < 20 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let out = bb(
            PLUS,
            json!({"radius":5,"green":false,"blue":false,"alpha":false}),
            &img,
        );
        for y in 0..20 {
            for x in 0..40 {
                let p = px(&out, x, y);
                assert_eq!(&p[1..], &px(&img, x, y)[1..]);
                if x <= 14 {
                    assert_eq!(p[0], 255);
                }
                if x >= 25 {
                    assert_eq!(p[0], 0);
                }
                if x > 0 {
                    assert!(p[0] <= px(&out, x - 1, y)[0]);
                }
                assert!((p[0] as i32 + px(&out, 39 - x, y)[0] as i32 - 255).abs() <= 1);
            }
        }
    }
    fn square() -> crate::canvas::ImageData {
        from_fn(40, 40, |x, y| {
            if (10..30).contains(&x) && (10..30).contains(&y) {
                [255; 4]
            } else {
                [0; 4]
            }
        })
    }
    /// blurs-gaussian-blur-plus.md Expected results 4.
    #[test]
    fn alpha_only_grows() {
        let img = square();
        let out = bb(PLUS, json!({"radius":5}), &img);
        for y in 0..40 {
            for x in 0..40 {
                let p = px(&out, x, y);
                if (10..30).contains(&x) && (10..30).contains(&y) {
                    assert_eq!(p, [255; 4]);
                }
                if !(5..35).contains(&x) || !(5..35).contains(&y) {
                    assert_eq!(p, [0; 4]);
                }
                if p[3] > 0 {
                    assert_eq!(p[..3], [255; 3]);
                }
                assert!(p[3] >= px(&img, x, y)[3]);
            }
        }
        for k in 5..10 {
            assert!(px(&out, k, 20)[3] > 0);
            assert!(px(&out, k, 20)[3] <= px(&out, k + 1, 20)[3]);
        }
    }
    /// blurs-gaussian-blur-plus.md Expected results 5.
    #[test]
    fn alpha_unchecked() {
        let img = square();
        let out = bb(PLUS, json!({"radius":5,"alpha":false}), &img);
        assert_eq!(out.data, img.data);
    }
    /// selection-blur-selection-edge.md Expected results 1.
    #[test]
    fn uniform_image() {
        let (m, p) = rect_selection(50, 50, 10, 10, 40, 40);
        let img = solid(50, 50, [0x30, 0x80, 0xc0, 255]);
        assert_eq!(
            bb_env(EDGE, json!({}), &img, &with_selection(50, 50, &m, &p)).data,
            img.data
        );
    }
    /// selection-blur-selection-edge.md Expected results 2: all four sides and unchanged complement.
    #[test]
    fn band_position() {
        let (m, p) = rect_selection(50, 50, 10, 10, 40, 40);
        let img = checker(50);
        let out = bb_env(EDGE, json!({}), &img, &with_selection(50, 50, &m, &p));
        for v in 11..39 {
            for e in [9, 10, 39, 40] {
                assert_px_tol(&out, e, v, [188, 188, 188, 255], 4);
                assert_px_tol(&out, v, e, [188, 188, 188, 255], 4);
            }
        }
        for y in 0..50 {
            for x in 0..50 {
                if !(9..=40).contains(&x)
                    || !(9..=40).contains(&y)
                    || ((11..39).contains(&x) && (11..39).contains(&y))
                {
                    assert_eq!(px(&out, x, y), px(&img, x, y));
                }
            }
        }
        for v in 11..39 {
            for e in [8, 11, 38, 41] {
                assert_eq!(px(&out, e, v), px(&img, e, v));
                assert_eq!(px(&out, v, e), px(&img, v, e));
            }
        }
    }
    /// selection-blur-selection-edge.md Expected results 3. The soft-border checker corners follow
    /// effects-1.md §2.3: one-sided even/odd kernel imbalance squared gives 118/137 gamma, 181/194 linear.
    #[test]
    fn canvas_border() {
        let img = checker(50);
        let out = bb(EDGE, json!({}), &img);
        for y in 0..50 {
            for x in 0..50 {
                if x == 0 || y == 0 || x == 49 || y == 49 {
                    let gray = if (x == 0 || x == 49) && (y == 0 || y == 49) {
                        if (x + y) % 2 == 0 { 181 } else { 194 }
                    } else {
                        188
                    };
                    assert_px_tol(&out, x, y, [gray, gray, gray, 255], 6);
                } else {
                    assert_eq!(px(&out, x, y), px(&img, x, y));
                }
            }
        }
    }
    /// selection-blur-selection-edge.md Expected results 4.
    #[test]
    fn transparent_band() {
        let (m, p) = rect_selection(50, 50, 10, 10, 40, 40);
        let out = bb_env(
            EDGE,
            json!({}),
            &solid(50, 50, [0; 4]),
            &with_selection(50, 50, &m, &p),
        );
        for v in 11..39 {
            for e in [9, 10, 39, 40] {
                assert_eq!(px(&out, e, v), [0, 0, 0, 255]);
                assert_eq!(px(&out, v, e), [0, 0, 0, 255]);
            }
        }
        assert_eq!(px(&out, 25, 25), [0; 4]);
        assert_eq!(px(&out, 2, 2), [0; 4]);
    }
    /// Both specs Controls: definitions used by the dialog, clipping and no-dialog registration.
    #[test]
    fn registered_contracts() {
        let d = find(PLUS).unwrap();
        assert_eq!(
            d.controls.iter().map(Control::key).collect::<Vec<_>>(),
            [
                "radius",
                "red",
                "green",
                "blue",
                "alpha",
                "mostOpaque",
                "blend"
            ]
        );
        assert_eq!(d.dialog_title, Some("Gaussian Blur+ v4.2"));
        assert!(matches!(
            d.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        let d = find(EDGE).unwrap();
        assert!(
            d.controls.is_empty()
                && d.dialog_title.is_none()
                && d.dialog.is_none()
                && d.indirect.is_none()
        );
        assert!(matches!(
            d.kind,
            EffectKind::Cpu {
                clip_to_selection: false,
                ..
            }
        ));
    }
    /// blurs-gaussian-blur-plus.md Behaviour: full-opacity blend order and every host mode.
    #[test]
    fn blend_modes_and_most_opaque() {
        let src = from_fn(2, 1, |x, _| {
            if x == 0 {
                [100, 100, 100, 255]
            } else {
                [200, 200, 200, 255]
            }
        });
        // Left pixel's tent blur is floor((2*100+200)/3)=133. Source is bottom.
        for (mode, want) in [
            ("normal", 133),
            ("multiply", 52),
            ("additive", 233),
            ("colorBurn", 0),
            ("colorDodge", 209),
            ("reflect", 81),
            ("glow", 114),
            ("overlay", 104),
            ("difference", 33),
            ("negation", 233),
            ("lighten", 133),
            ("darken", 100),
            ("screen", 181),
            ("xor", 225),
        ] {
            assert_eq!(
                px(&bb(PLUS, json!({"radius":1,"blend":mode}), &src), 0, 0),
                [want, want, want, 255],
                "{mode}"
            );
        }
        let src = from_fn(2, 1, |x, _| if x == 0 { [255, 0, 0, 128] } else { [0; 4] });
        assert_eq!(
            px(&bb(PLUS, json!({"radius":1}), &src), 0, 0),
            [255, 0, 0, 170]
        );
        assert_eq!(
            px(&bb(PLUS, json!({"radius":1,"mostOpaque":true}), &src), 0, 0),
            [255, 0, 0, 192]
        );
        assert_eq!(
            bb(PLUS, json!({"radius":1,"alpha":false}), &src).data,
            bb(
                PLUS,
                json!({"radius":1,"alpha":false,"mostOpaque":true}),
                &src
            )
            .data
        );
    }

    /// blurs-gaussian-blur-plus.md Behaviour/common.md §2: whole-layer reads, clipped output and AA coverage.
    #[test]
    fn plus_selection() {
        let src = from_fn(50, 50, |x, y| [(x * 7) as u8, (y * 5) as u8, 90, 255]);
        let full = bb(PLUS, json!({"radius":5}), &src);
        let (m, p) = ellipse_selection(50, 50, [25.0, 25.0], [15.0, 10.0]);
        let out = bb_env(
            PLUS,
            json!({"radius":5}),
            &src,
            &with_selection(50, 50, &m, &p),
        );
        let mut partial = 0;
        for y in 0..50 {
            for x in 0..50 {
                let coverage = m.get(x as i32, y as i32);
                let want = match coverage {
                    0 => px(&src, x, y),
                    255 => px(&full, x, y),
                    a => {
                        partial += 1;
                        crate::pixel::ops::blend(px(&src, x, y), px(&full, x, y), a)
                    }
                };
                assert_eq!(px(&out, x, y), want);
            }
        }
        assert!(partial > 0);
        // Outside the rectangular selection, a bright source tap still contributes to the selected pixel.
        let (m, p) = rect_selection(3, 1, 1, 0, 2, 1);
        let src = from_fn(3, 1, |x, _| if x == 0 { [255; 4] } else { [0, 0, 0, 255] });
        let out = bb_env(
            PLUS,
            json!({"radius":1}),
            &src,
            &with_selection(3, 1, &m, &p),
        );
        assert_eq!(px(&out, 1, 0), [63, 63, 63, 255]);
        assert_eq!(px(&out, 0, 0), [255; 4]);
    }

    /// selection-blur-selection-edge.md Expected results 1–4: the gamma column uses the same CPU worker.
    #[test]
    fn selection_context_columns_and_cpu_invariant() {
        use super::selection_edge_with;
        use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
        use crate::effect::surface::{Alpha, Space};
        use crate::effect::{EffectEnv, RenderInfo};
        let (m, p) = rect_selection(50, 50, 10, 10, 40, 40);
        let mut env = with_selection(50, 50, &m, &p);
        let img = checker(50);
        let registered = bb_env(EDGE, json!({}), &img, &env);
        assert_eq!(
            registered.data,
            selection_edge_with(&img, &env, GPU_CONTEXT).data
        );
        for (info, gray) in [(GPU_CONTEXT, 188), (ALT_GPU_CONTEXT, 128)] {
            let out = selection_edge_with(&img, &env, info);
            for v in 11..39 {
                for e in [9, 10, 39, 40] {
                    assert_px_tol(&out, e, v, [gray, gray, gray, 255], 4);
                    assert_px_tol(&out, v, e, [gray, gray, gray, 255], 4);
                }
            }
            let flat = solid(50, 50, [48, 128, 192, 255]);
            assert_eq!(selection_edge_with(&flat, &env, info).data, flat.data);
            let clear = selection_edge_with(&solid(50, 50, [0; 4]), &env, info);
            assert_eq!(px(&clear, 9, 20), [0, 0, 0, 255]);
            assert_eq!(px(&clear, 20, 20), [0; 4]);
            let whole = selection_edge_with(&img, &EffectEnv::whole(50, 50), info);
            for y in 0..50 {
                for x in 0..50 {
                    if x == 0 || x == 49 || y == 0 || y == 49 {
                        let g = if (x == 0 || x == 49) && (y == 0 || y == 49) {
                            match (gray, (x + y) % 2) {
                                (188, 0) => 181,
                                (188, _) => 194,
                                (_, 0) => 118,
                                _ => 137,
                            }
                        } else {
                            gray
                        };
                        assert_px_tol(&whole, x, y, [g, g, g, 255], 6);
                    } else {
                        assert_eq!(px(&whole, x, y), px(&img, x, y));
                    }
                }
            }
        }
        let plus = bb_env(PLUS, json!({}), &img, &env);
        for space in [Space::Srgb, Space::Linear] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = RenderInfo {
                        space,
                        input,
                        output,
                    };
                    env.render_info = Some(info);
                    assert_eq!(bb_env(EDGE, json!({}), &img, &env).data, registered.data);
                    assert_eq!(bb_env(PLUS, json!({}), &img, &env).data, plus.data);
                    assert_eq!(env.render_info, Some(info));
                }
            }
        }
    }

    /// selection-blur-selection-edge.md Behaviour: fractional band replaces alpha and mixes in working space.
    #[test]
    fn ellipse_band_and_partial_alpha() {
        use super::selection_edge_with;
        use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
        let (m, p) = ellipse_selection(50, 50, [25.0, 25.0], [15.0, 10.0]);
        assert_eq!(p[0].len(), 64);
        assert!(m.data.iter().any(|&a| a > 0 && a < 255));
        let env = with_selection(50, 50, &m, &p);
        let img = checker(50);
        let out = bb_env(EDGE, json!({}), &img, &env);
        assert_eq!(px(&out, 25, 25), px(&img, 25, 25));
        assert_eq!(px(&out, 2, 2), px(&img, 2, 2));
        assert_px_tol(&out, 40, 25, [188, 188, 188, 255], 6);
        let hidden = solid(50, 50, [37, 91, 153, 0]);
        let out = bb_env(EDGE, json!({}), &hidden, &env);
        assert_eq!(px(&out, 25, 25), [37, 91, 153, 0]);
        assert_eq!(px(&out, 2, 2), [37, 91, 153, 0]);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 0 && p[3] < 255 && p[..3] == [0; 3])
        );
        for info in [GPU_CONTEXT, ALT_GPU_CONTEXT] {
            let hidden_out = selection_edge_with(&hidden, &env, info);
            assert_eq!(px(&hidden_out, 25, 25), [37, 91, 153, 0]);
            assert_eq!(px(&hidden_out, 2, 2), [37, 91, 153, 0]);
            assert!(
                hidden_out
                    .data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 0 && p[3] < 255 && p[..3] == [0; 3])
            );
            let src = solid(50, 50, [200, 100, 50, 64]);
            let out = selection_edge_with(&src, &env, info);
            assert_eq!(px(&out, 25, 25), [200, 100, 50, 64]);
            assert!(
                out.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 64 && p[3] < 255)
            );
            for p in out.data.as_chunks::<4>().0 {
                assert_eq!(p[..3], [200, 100, 50]);
                assert!(p[3] >= 64);
            }
        }
    }

    /// Both specs Controls/common.md §3–4: complete consumed metadata, host blend order and keys.
    #[test]
    fn complete_dialog_metadata() {
        use crate::document::layer::BlendMode;
        use crate::effect::EffectEnv;
        use crate::effect::bb::indirect_defaults;
        use crate::effect::bb::ui::{Help, HelpWindow, Note};
        let d = find(PLUS).unwrap();
        let ui = d.indirect.unwrap();
        assert_eq!(ui.width, 1.0);
        assert!(ui.tabs.is_empty());
        assert!(matches!(ui.help, Help::Window(HelpWindow::Older)));
        assert!(d.dialog.is_none());
        assert!(
            matches!(d.controls[0],Control::Range{key:"radius",label:"Radius",min:0.0,max:100.0,default:2.0,step:1.0,exponential:false,keyboard,track:"default"}
            if keyboard.decimals==0&&keyboard.small==1.0&&keyboard.large==5.0&&!keyboard.exponential
            &&keyboard.stepped(2.0,0.0,100.0,true,false)==3.0&&keyboard.stepped(2.0,0.0,100.0,true,true)==7.0)
        );
        for (i, key, label, default) in [
            (1, "red", "Red", true),
            (2, "green", "Green", true),
            (3, "blue", "Blue", true),
            (4, "alpha", "Alpha", true),
            (5, "mostOpaque", "Use most opaque alpha", false),
        ] {
            assert!(
                matches!(d.controls[i],Control::Checkbox{key:k,label:l,default:v} if (k,l,v)==(key,label,default))
            );
        }
        let Control::Select {
            key: "blend",
            label: "Blending Mode",
            default: "normal",
            choices,
        } = d.controls[6]
        else {
            panic!("blend select")
        };
        assert_eq!(choices, BlendMode::ALL.map(|m| (m.value(), m.label())));
        assert_eq!(choices.len(), 14);
        let mut v = indirect_defaults(&d, &EffectEnv::whole(1, 1));
        for alpha in [false, true] {
            v.insert("alpha".into(), json!(alpha));
            for c in &d.controls {
                assert!((ui.enabled)(&v, c.key()));
                let u = ui.control(c.key());
                assert_eq!(u.header_line, matches!(c.key(), "radius" | "blend"));
                assert!(
                    u.track.is_none()
                        && u.caption.is_empty()
                        && !u.alpha
                        && u.reset
                        && !u.wrap
                        && matches!(u.note, Note::None)
                );
            }
        }
    }
}
