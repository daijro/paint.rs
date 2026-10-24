//! BB spec: effects/flip-horizontal.md; effects/flip-vertical.md;
//! effects/photo-remove-dust.md; common.md §1, §2, §3, §4

use super::conventions::GPU_CONTEXT;
use super::manifest::manifest;
use super::ui::{ControlUi, IndirectUi};
use crate::canvas::ImageData;
use crate::effect::mix::{MixMode, mix};
use crate::effect::surface::{Alpha, Edge, EffectSurface, premultiply, unpremultiply};
use crate::effect::{Category, Control, CpuFn, EffectDef, EffectEnv, EffectKind, Values, choice};
use crate::pixel::IntRect;

static DUST_UI: IndirectUi = IndirectUi {
    controls: &[("mode", ControlUi::NO_HEADER)],
    ..IndirectUi::DEFAULT
};

/// The pixel-moving effects, in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let flip = |id, render: CpuFn| EffectDef {
        id,
        icon: manifest(id).expect("registered flip effect").icon,
        category: Category::Effect(""),
        controls: vec![],
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: None,
        indirect: None,
    };
    vec![
        flip("bbFlipHorizontal", |src, _, env| {
            flip_pixels(src, env, true)
        }),
        flip("bbFlipVertical", |src, _, env| flip_pixels(src, env, false)),
        EffectDef {
            id: "bbRemoveDust",
            icon: manifest("bbRemoveDust")
                .expect("registered dust effect")
                .icon,
            category: Category::Effect("photo"),
            controls: vec![Control::Select {
                key: "mode",
                label: "Remove dust by deleting",
                default: "white",
                choices: &[("white", "White"), ("black", "Black")],
            }],
            kind: EffectKind::Gpu(GPU_CONTEXT, remove_dust),
            dialog: None,
            dialog_title: Some("Remove Dust"),
            indirect: Some(&DUST_UI),
        },
    ]
}

fn flip_pixels(src: &ImageData, env: &EffectEnv, horizontal: bool) -> ImageData {
    let b = env
        .bounds
        .intersect(&IntRect::new(0, 0, src.width, src.height));
    let mut out = src.clone();
    for y in b.y..b.bottom() {
        for x in b.x..b.right() {
            let (sx, sy) = if horizontal {
                (b.right() - 1 - (x - b.x), y)
            } else {
                (x, b.bottom() - 1 - (y - b.y))
            };
            let index = |x: i32, y: i32| (y as usize * src.width as usize + x as usize) * 4;
            let (dst, source) = (index(x, y), index(sx, sy));
            // quirk(bb): flip-horizontal.md / flip-vertical.md Behaviour: source may be unselected.
            out.data[dst..dst + 4].copy_from_slice(&src.data[source..source + 4]);
        }
    }
    out
}

fn remove_dust(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let mode = MixMode::from_value(if choice(v, "mode") == "black" {
        "lighten"
    } else {
        "darken"
    });
    let input = |p| match info.input {
        Alpha::Straight => premultiply(p),
        Alpha::Premultiplied => p,
    };
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        // quirk(bb): photo-remove-dust.md Behaviour: one left tap, also outside the selection.
        let shifted = src.fetch(x as i32 - 1, y as i32, Edge::Transparent);
        // approximation: prescribed Direct2D source-over alpha uses the existing W3C mix
        // (photo-remove-dust.md Notes / uncertainties).
        let p = mix(mode, input(src.at(x, y)), input(shifted));
        match info.output {
            Alpha::Straight => unpremultiply(p),
            Alpha::Premultiplied => p,
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::{Help, Note};
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::{Category, Control, EffectEnv, EffectKind, RenderInfo, apply, find};
    use crate::pixel::{IntRect, mask::Mask};
    use serde_json::json;

    const R: [u8; 4] = [255, 0, 0, 255];
    const G: [u8; 4] = [0, 255, 0, 255];
    const B: [u8; 4] = [0, 0, 255, 255];
    const T: [u8; 4] = [0; 4];
    const K: [u8; 4] = [0, 0, 0, 255];
    const W: [u8; 4] = [255; 4];
    fn row(p: &[[u8; 4]]) -> ImageData {
        from_fn(p.len() as u32, 1, |x, _| p[x as usize])
    }
    fn col(p: &[[u8; 4]]) -> ImageData {
        from_fn(1, p.len() as u32, |_, y| p[y as usize])
    }
    fn pixels(img: &ImageData) -> Vec<[u8; 4]> {
        img.data.as_chunks::<4>().0.to_vec()
    }
    fn p(x: u32, y: u32) -> [u8; 4] {
        [(x * 80 + 10) as u8, (y * 90 + 20) as u8, 7, 255]
    }
    fn dust(mode: &str, src: &ImageData, want: &[[u8; 4]]) {
        for info in [GPU_CONTEXT, ALT_GPU_CONTEXT] {
            let out = bb_ctx("bbRemoveDust", info, json!({"mode": mode}), src);
            assert_eq!(pixels(&out).len(), want.len());
            for (got, want) in pixels(&out).into_iter().zip(want) {
                assert!(
                    got.into_iter()
                        .zip(want)
                        .all(|(a, b)| a.abs_diff(*b) <= TOL),
                    "{got:?} != {want:?}, {info:?}"
                );
            }
        }
    }

    /// flip-horizontal.md Expected results 1.
    #[test]
    fn horizontal_moves_whole_pixels() {
        assert_eq!(
            pixels(&bb("bbFlipHorizontal", json!({}), &row(&[R, G, B, T]))),
            [T, B, G, R]
        );
    }

    /// flip-horizontal.md Expected results 2.
    #[test]
    fn horizontal_uses_selection_bounds() {
        let (m, p) = rect_selection(4, 1, 1, 0, 3, 1);
        assert_eq!(
            pixels(&bb_env(
                "bbFlipHorizontal",
                json!({}),
                &row(&[R, G, B, T]),
                &with_selection(4, 1, &m, &p)
            )),
            [R, B, G, T]
        );
    }

    /// flip-horizontal.md Expected results 3.
    #[test]
    fn horizontal_reads_unselected_source() {
        let img = from_fn(3, 2, p);
        let mask = Mask {
            rect: IntRect::new(0, 0, 3, 2),
            data: vec![255, 255, 0, 255, 0, 0],
        };
        let env = EffectEnv {
            mask: Some(&mask),
            bounds: IntRect::new(0, 0, 2, 2),
            ..EffectEnv::whole(3, 2)
        };
        assert_eq!(
            pixels(&bb_env("bbFlipHorizontal", json!({}), &img, &env)),
            [p(1, 0), p(0, 0), p(2, 0), p(1, 1), p(1, 1), p(2, 1)]
        );
    }

    /// flip-horizontal.md Expected results 4.
    #[test]
    fn horizontal_keeps_odd_center_and_alpha() {
        let hidden = [9, 8, 7, 0];
        let middle = [17, 21, 31, 99];
        assert_eq!(
            pixels(&bb(
                "bbFlipHorizontal",
                json!({}),
                &row(&[R, G, middle, B, hidden])
            )),
            [hidden, B, middle, G, R]
        );
    }

    /// flip-horizontal.md Expected results 5.
    #[test]
    fn horizontal_twice_restores_exactly() {
        let img = from_fn(7, 5, |x, y| {
            [(x * 30) as u8, (y * 50) as u8, 3, (x * 20 + y) as u8]
        });
        assert_eq!(
            bb(
                "bbFlipHorizontal",
                json!({}),
                &bb("bbFlipHorizontal", json!({}), &img)
            )
            .data,
            img.data
        );
    }

    /// flip-vertical.md Expected results 1.
    #[test]
    fn vertical_moves_whole_pixels() {
        assert_eq!(
            pixels(&bb("bbFlipVertical", json!({}), &col(&[R, G, B, T]))),
            [T, B, G, R]
        );
    }

    /// flip-vertical.md Expected results 2.
    #[test]
    fn vertical_uses_selection_bounds() {
        let (m, p) = rect_selection(1, 4, 0, 1, 1, 3);
        assert_eq!(
            pixels(&bb_env(
                "bbFlipVertical",
                json!({}),
                &col(&[R, G, B, T]),
                &with_selection(1, 4, &m, &p)
            )),
            [R, B, G, T]
        );
    }

    /// flip-vertical.md Expected results 3.
    #[test]
    fn vertical_reads_unselected_source() {
        let img = from_fn(2, 3, p);
        let mask = Mask {
            rect: IntRect::new(0, 0, 2, 3),
            data: vec![255, 255, 255, 0, 0, 0],
        };
        let env = EffectEnv {
            mask: Some(&mask),
            bounds: IntRect::new(0, 0, 2, 2),
            ..EffectEnv::whole(2, 3)
        };
        assert_eq!(
            pixels(&bb_env("bbFlipVertical", json!({}), &img, &env)),
            [p(0, 1), p(1, 1), p(0, 0), p(1, 1), p(0, 2), p(1, 2)]
        );
    }

    /// flip-vertical.md Expected results 4.
    #[test]
    fn vertical_twice_restores_exactly() {
        let img = from_fn(5, 7, |x, y| {
            [(x * 30) as u8, (y * 35) as u8, 3, (x * 20 + y) as u8]
        });
        assert_eq!(
            bb(
                "bbFlipVertical",
                json!({}),
                &bb("bbFlipVertical", json!({}), &img)
            )
            .data,
            img.data
        );
    }

    /// flip-vertical.md Behaviour: odd heights and stored RGB travel exactly with alpha.
    #[test]
    fn vertical_keeps_odd_center_and_hidden_rgb() {
        let hidden = [9, 8, 7, 0];
        let middle = [17, 21, 31, 99];
        assert_eq!(
            pixels(&bb(
                "bbFlipVertical",
                json!({}),
                &col(&[R, G, middle, B, hidden])
            )),
            [hidden, B, middle, G, R]
        );
    }

    /// Both flip specs Behaviour: antialiased coverage blends output, never the source lookup.
    #[test]
    fn flips_blend_only_the_output_mask() {
        for (id, img, rect) in [
            ("bbFlipHorizontal", row(&[R, B]), IntRect::new(0, 0, 2, 1)),
            ("bbFlipVertical", col(&[R, B]), IntRect::new(0, 0, 1, 2)),
        ] {
            let mask = Mask {
                rect,
                data: vec![128, 255],
            };
            let env = EffectEnv {
                mask: Some(&mask),
                ..EffectEnv::whole(img.width, img.height)
            };
            assert_eq!(
                pixels(&bb_env(id, json!({}), &img, &env)),
                [[127, 0, 128, 255], R]
            );
        }
    }

    /// Both flip specs Behaviour: empty bounds are no-ops and 1px axes do not underflow.
    #[test]
    fn flips_handle_degenerate_and_empty_bounds() {
        for id in ["bbFlipHorizontal", "bbFlipVertical"] {
            for (w, h) in [(0, 0), (0, 3), (3, 0), (1, 1), (1, 5), (5, 1)] {
                let img = from_fn(w, h, |x, y| [x as u8, y as u8, 3, 0]);
                let out = bb(id, json!({}), &img);
                assert_eq!(bb(id, json!({}), &out).data, img.data);
                let env = EffectEnv {
                    bounds: IntRect::new(0, 0, 0, h),
                    ..EffectEnv::whole(w, h)
                };
                assert_eq!(bb_env(id, json!({}), &img, &env).data, img.data);
                let env = EffectEnv {
                    bounds: IntRect::new(0, 0, w, 0),
                    ..env
                };
                assert_eq!(bb_env(id, json!({}), &img, &env).data, img.data);
            }
        }
    }

    /// photo-remove-dust.md Expected results 1.
    #[test]
    fn white_speck_disappears() {
        dust("white", &row(&[K, K, W, K, K]), &[K; 5]);
    }
    /// photo-remove-dust.md Expected results 2.
    #[test]
    fn black_speck_disappears() {
        dust("black", &row(&[W, W, K, W, W]), &[W; 5]);
    }
    /// photo-remove-dust.md Expected results 3.
    #[test]
    fn wider_speck_loses_leftmost_pixel() {
        dust("white", &row(&[K, W, W, K]), &[K, K, W, K]);
    }
    /// photo-remove-dust.md Expected results 4.
    #[test]
    fn dust_takes_minimum_per_channel() {
        dust(
            "white",
            &row(&[[100, 150, 100, 255], [200, 50, 100, 255]]),
            &[[100, 150, 100, 255], [100, 50, 100, 255]],
        );
    }
    /// photo-remove-dust.md Expected results 5.
    #[test]
    fn dust_has_only_a_horizontal_component() {
        let vertical = from_fn(5, 5, |x, _| if x == 2 { W } else { K });
        dust("white", &vertical, &[K; 25]);
        let horizontal = from_fn(5, 5, |_, y| if y == 2 { W } else { K });
        dust("white", &horizontal, &pixels(&horizontal));
        let segment = from_fn(
            5,
            5,
            |x, y| if y == 2 && (1..=3).contains(&x) { W } else { K },
        );
        let want = from_fn(
            5,
            5,
            |x, y| if y == 2 && (2..=3).contains(&x) { W } else { K },
        );
        dust("white", &segment, &pixels(&want));
    }
    /// photo-remove-dust.md Expected results 6.
    #[test]
    fn dust_keeps_left_column() {
        dust("black", &row(&[K, W]), &[K, W]);
    }

    /// photo-remove-dust.md Behaviour: full source supplies the left neighbour outside a selection.
    #[test]
    fn dust_reads_outside_selection_and_clips_output() {
        let img = row(&[K, W, W]);
        let (mask, p) = rect_selection(3, 1, 1, 0, 2, 1);
        assert_eq!(
            pixels(&bb_env(
                "bbRemoveDust",
                json!({}),
                &img,
                &with_selection(3, 1, &mask, &p)
            )),
            [K, K, W]
        );
        let mask = Mask {
            rect: IntRect::new(0, 0, 3, 1),
            data: vec![0, 128, 0],
        };
        let env = EffectEnv {
            mask: Some(&mask),
            bounds: IntRect::new(1, 0, 1, 1),
            ..EffectEnv::whole(3, 1)
        };
        assert_eq!(
            pixels(&bb_env("bbRemoveDust", json!({}), &img, &env)),
            [K, [127, 127, 127, 255], W]
        );
    }

    /// photo-remove-dust.md Behaviour Alpha: opaque pixels grow right into transparency.
    #[test]
    fn dust_alpha_uses_source_over() {
        let hidden = [31, 42, 53, 0];
        for mode in ["white", "black"] {
            dust(mode, &row(&[hidden, R, hidden]), &[T, R, R]);
            dust(
                mode,
                &row(&[[200, 30, 10, 128], hidden]),
                &[[200, 30, 10, 128]; 2],
            );
        }
    }

    /// photo-remove-dust.md Behaviour Alpha formula, in both colour spaces and effective alpha modes (R88).
    #[test]
    fn dust_partial_alpha_honors_effective_context() {
        let img = row(&[[255, 0, 0, 128], [0, 0, 255, 128]]);
        for (space, white, black) in [
            (Space::Srgb, [85, 0, 85, 192], [170, 0, 170, 192]),
            (Space::Linear, [156, 0, 156, 192], [213, 0, 213, 192]),
        ] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = RenderInfo {
                        space,
                        input,
                        output,
                    };
                    for (mode, want) in [("white", white), ("black", black)] {
                        let out = bb_ctx("bbRemoveDust", info, json!({"mode":mode}), &img);
                        assert_px(&out, 0, 0, [255, 0, 0, 128]);
                        assert_px(&out, 1, 0, want);
                    }
                }
            }
        }
    }

    /// photo-remove-dust.md Controls and Behaviour: White default, one Select row, no header or note.
    #[test]
    fn definitions_match_dialog_and_cpu_contracts() {
        for id in ["bbFlipHorizontal", "bbFlipVertical"] {
            let def = find(id).unwrap();
            assert_eq!(def.category, Category::Effect(""));
            assert!(
                def.controls.is_empty()
                    && def.indirect.is_none()
                    && def.dialog_title.is_none()
                    && def.dialog.is_none()
            );
            assert!(matches!(
                def.kind,
                EffectKind::Cpu {
                    clip_to_selection: true,
                    ..
                }
            ));
        }
        let def = find("bbRemoveDust").unwrap();
        assert_eq!(def.category, Category::Effect("photo"));
        assert_eq!(def.dialog_title, Some("Remove Dust"));
        assert!(matches!(def.kind,EffectKind::Gpu(info,_) if info==GPU_CONTEXT));
        assert!(
            matches!(&def.controls[..],[Control::Select {key:"mode",label:"Remove dust by deleting",default:"white",choices}] if *choices==[("white","White"),("black","Black")])
        );
        let ui = def.indirect.unwrap();
        assert_eq!(ui.width, 1.0);
        assert!(matches!(ui.help, Help::Window(_)));
        assert!(!ui.control("mode").header_line);
        assert!(matches!(ui.control("mode").note, Note::None));
        assert_eq!(
            bb("bbRemoveDust", json!({}), &row(&[K, W])).data,
            bb("bbRemoveDust", json!({"mode":"white"}), &row(&[K, W])).data
        );
        let env = EffectEnv::whole(2, 1);
        let values = crate::effect::defaults(&def);
        let img = row(&[K, W]);
        let out = apply(&img, &def, &values, &env);
        assert_eq!(pixels(&out), [K, K]);
    }
}
