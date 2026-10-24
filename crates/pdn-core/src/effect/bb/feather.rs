//! BB spec: effects/object-feather-object.md; effects/selection-feather-selection.md; common.md §1–5

use super::blocks::morph_min;
use super::conventions::{
    AA_BYTE_CONVERSION, FEATHER_OBJECT_EROSION_EDGE, FEATHER_POINT_SAMPLED_GAUSSIAN,
    FEATHER_SELECTION_MASK_GREY_AS_IS, FEATHER_SELECTION_ZERO_WIDTH_BAND, RADIUS_TO_SIGMA,
};
use super::manifest::manifest;
use super::raster::{self, Cap, Join, Pen};
use super::staircase::{Neighbour, staircase};
use super::ui::{ControlUi, IndirectUi};
use crate::canvas::ImageData;
use crate::effect::blur::gaussian;
use crate::effect::surface::{Edge, EffectSurface};
use crate::effect::{EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::pixel::gamma::{SRGB8_TO_LINEAR, quantize};

static UI: IndirectUi = IndirectUi {
    controls: &[("radius", ControlUi::NO_HEADER)],
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    [
        (
            "bbFeatherObject",
            "Feather Object",
            feather_object as crate::effect::CpuFn,
        ),
        (
            "bbFeatherSelection",
            "Feather Selection",
            feather_selection as crate::effect::CpuFn,
        ),
    ]
    .into_iter()
    .map(|(id, title, render)| {
        let m = manifest(id).unwrap();
        EffectDef {
            id: m.id,
            icon: m.icon,
            category: m.category,
            controls: vec![range("radius", "Radius", 0.0, 20.0, 1.0)],
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(&UI),
        }
    })
    .collect()
}

fn object_mask(src: &ImageData, radius: i32) -> EffectSurface {
    let mut mask = EffectSurface::from_fn(src.width, src.height, |x, y| {
        let v = if src.data[((y * src.width + x) * 4 + 3) as usize] > 0 {
            1.0
        } else {
            0.0
        };
        [v; 4]
    });
    if radius > 0 {
        // spec-question: Q33 Feather Object erosion boundary.
        mask = morph_min(&mask, radius, FEATHER_OBJECT_EROSION_EDGE);
    }
    // Erosion preserves the binary mask. Flattening makes alpha opaque; contrast 1 preserves both endpoints.
    for p in &mut mask.px {
        p[3] = 1.0;
    }
    mask
}

fn mask_bytes(mask: EffectSurface, sigma: f64) -> Vec<u8> {
    let mask = if sigma <= 0.0 {
        mask
    } else if FEATHER_POINT_SAMPLED_GAUSSIAN {
        // spec-question: Q32 Feather Gaussian kernel/support/border, controller ruling R121.
        // approximation: integer-point Gaussian matching supplied profiles, not a native-exact D2D kernel (both Feather specs Notes).
        let radius = (6.0 * sigma).ceil() as i32;
        let weights: Vec<f64> = (-radius..=radius)
            .map(|x| (-f64::from(x).powi(2) / (2.0 * sigma * sigma)).exp())
            .collect();
        let sum: f64 = weights.iter().sum();
        let kernel: Vec<f32> = weights.into_iter().map(|w| (w / sum) as f32).collect();
        mask.convolve_hv(&kernel, Edge::Transparent)
    } else {
        gaussian(&mask, sigma, Edge::Transparent)
    };
    mask.px.iter().map(|p| quantize(p[0])).collect()
}

fn feather_object(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let radius = num(v, "radius") as i32;
    let sigma = if radius >= 2 {
        2.0 * f64::from((radius - 2).max(1)) * RADIUS_TO_SIGMA
    } else {
        0.0
    };
    let mask = mask_bytes(object_mask(src, radius), sigma);
    // quirk(bb): object-feather-object.md Notes (raw bytes, Q1). Keep stored RGB, including at alpha zero.
    let mut out = src.clone();
    for (p, m) in out.data.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        p[3] = p[3].min(m);
    }
    if radius <= 1 {
        let alpha: Vec<u8> = out.data.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        let (coverage, from) = staircase(&alpha, src.width, src.height, AA_BYTE_CONVERSION);
        for (i, ((p, a), n)) in out
            .data
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(coverage)
            .zip(from)
            .enumerate()
        {
            p[3] = a;
            // quirk(bb): object-feather-object.md Staircase Colour. Q22 controls zero-coverage recording.
            let neighbour = match n {
                Neighbour::None => continue,
                Neighbour::Left => i - 1,
                Neighbour::Right => i + 1,
                Neighbour::Up => i - src.width as usize,
                Neighbour::Down => i + src.width as usize,
            };
            p[..3].copy_from_slice(&src.data[neighbour * 4..neighbour * 4 + 3]);
        }
    }
    out
}

fn selection_mask(w: u32, h: u32, env: &EffectEnv, radius: i32) -> EffectSurface {
    let Some(path) = raster::polygon_path(&raster::selection_or_canvas(env), [0.0, 0.0], true)
    else {
        return EffectSurface::new(w, h);
    };
    let mut fill = raster::fill_coverage(&path, w, h, false);
    // spec-question: Q31 Feather Selection zero-width widen.
    let width = if radius == 0 {
        FEATHER_SELECTION_ZERO_WIDTH_BAND
    } else {
        2.0 * radius as f32
    };
    if width > 0.0 {
        let band = raster::stroke_coverage(
            &path,
            &Pen {
                width,
                join: Join::Round,
                cap: Cap::Flat,
                miter_limit: 10.0,
                dash: None,
            },
            w,
            h,
            false,
        );
        for (f, b) in fill.iter_mut().zip(band) {
            *f *= 1.0 - b;
        }
    }
    EffectSurface::from_fn(w, h, |x, y| {
        let f = fill[(y * w + x) as usize];
        [f, f, f, 1.0]
    })
}

fn feather_selection(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let radius = num(v, "radius") as i32;
    let sigma = if radius >= 2 {
        2.0 * f64::from(radius) * RADIUS_TO_SIGMA
    } else {
        0.0
    };
    let mut mask = mask_bytes(selection_mask(src.width, src.height, env, radius), sigma);
    if radius <= 1 {
        mask = staircase(&mask, src.width, src.height, AA_BYTE_CONVERSION).0;
        // spec-question: Q5 Feather Selection r <= 1 mask grey.
        if !FEATHER_SELECTION_MASK_GREY_AS_IS {
            for m in &mut mask {
                *m = quantize(SRGB8_TO_LINEAR[*m as usize]);
            }
        }
    }
    let mut out = src.clone();
    for (p, m) in out.data.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        p[3] = p[3].min(m);
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::indirect_defaults;
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::{Help, HelpWindow, Note};
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::{Category, Control, EffectEnv, EffectKind, RenderInfo, find};
    use serde_json::json;

    const OBJECT: &str = "bbFeatherObject";
    const SELECTION: &str = "bbFeatherSelection";
    const TOP: [u8; 20] = [
        141, 148, 160, 173, 186, 198, 211, 224, 236, 251, 249, 236, 224, 211, 198, 186, 173, 160,
        148, 141,
    ];
    const SIDE: [u8; 10] = [141, 167, 192, 217, 255, 243, 217, 192, 167, 141];

    fn rect(w: u32, h: u32, bounds: [u32; 4], colour: [u8; 4]) -> ImageData {
        let [l, t, r, b] = bounds;
        from_fn(w, h, |x, y| {
            if (l..r).contains(&x) && (t..b).contains(&y) {
                colour
            } else {
                [0; 4]
            }
        })
    }
    fn square() -> ImageData {
        rect(100, 100, [30, 30, 70, 70], [255; 4])
    }
    fn tent(out: &ImageData) {
        for y in [5, 14] {
            assert_eq!((5..25).map(|x| px(out, x, y)[3]).collect::<Vec<_>>(), TOP);
        }
        for x in [5, 24] {
            assert_eq!((5..15).map(|y| px(out, x, y)[3]).collect::<Vec<_>>(), SIDE);
        }
        for y in 6..14 {
            for x in 6..24 {
                assert_eq!(px(out, x, y)[3], 255);
            }
        }
    }
    fn profile(out: &ImageData, want: &[u8]) {
        for (i, &a) in want.iter().enumerate() {
            let i = i as u32;
            for (x, y) in [(30 + i, 50), (69 - i, 50), (50, 30 + i), (50, 69 - i)] {
                assert!(
                    px(out, x, y)[3].abs_diff(a) <= 4,
                    "({x},{y}) {} want {a}",
                    px(out, x, y)[3]
                );
            }
        }
    }
    fn outside(src: &ImageData, out: &ImageData, bounds: [u32; 4]) {
        let [l, t, r, b] = bounds;
        for y in 0..src.height {
            for x in 0..src.width {
                if !(l..r).contains(&x) || !(t..b).contains(&y) {
                    assert_eq!(px(out, x, y), px(src, x, y), "({x},{y})");
                }
            }
        }
    }
    fn ring(out: &ImageData) {
        for y in 4..16 {
            for x in 4..26 {
                if x == 4 || x == 25 || y == 4 || y == 15 {
                    assert_eq!(px(out, x, y)[3], 0, "({x},{y})");
                }
            }
        }
    }
    fn selected(src: &ImageData, bounds: [u32; 4], radius: u32) -> ImageData {
        let [l, t, r, b] = bounds;
        let (m, p) = rect_selection(src.width, src.height, l, t, r, b);
        bb_env(
            SELECTION,
            json!({"radius":radius}),
            src,
            &with_selection(src.width, src.height, &m, &p),
        )
    }

    /// object-feather-object.md Expected results 1: complete integer-exact tent and unchanged RGB.
    #[test]
    fn object_radius_zero_tent() {
        let src = rect(30, 20, [5, 5, 25, 15], [255, 0, 0, 255]);
        let out = bb(OBJECT, json!({"radius":0}), &src);
        tent(&out);
        outside(&src, &out, [5, 5, 25, 15]);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[3] == 0 || p[..3] == [255, 0, 0])
        );
    }
    /// object-feather-object.md Expected results 2: the complete outer ring disappears.
    #[test]
    fn object_radius_one_ring() {
        let src = rect(30, 20, [4, 4, 26, 16], [255, 0, 0, 255]);
        let out = bb(OBJECT, json!({"radius":1}), &src);
        ring(&out);
        tent(&out);
        outside(&src, &out, [4, 4, 26, 16]);
        for (a, b) in src
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.data.as_chunks::<4>().0.iter())
        {
            assert_eq!(a[..3], b[..3]);
        }
    }
    /// object-feather-object.md Expected results 3: all four profile directions and outside alpha.
    #[test]
    fn object_radius_three_profile() {
        let src = square();
        let out = bb(OBJECT, json!({"radius":3}), &src);
        profile(&out, &[0, 2, 51, 204, 253, 255]);
        outside(&src, &out, [30, 30, 70, 70]);
        for (a, b) in src
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.data.as_chunks::<4>().0.iter())
        {
            assert_eq!(a[..3], b[..3]);
        }
    }
    /// object-feather-object.md Expected results 4: mirrored radius-five profile and opaque centre.
    #[test]
    fn object_radius_five_profile() {
        let src = square();
        let out = bb(OBJECT, json!({"radius":5}), &src);
        profile(&out, &[3, 10, 26, 57, 102, 153, 198, 229, 245, 252, 254]);
        assert_eq!(px(&out, 50, 50), [255; 4]);
        outside(&src, &out, [30, 30, 70, 70]);
    }
    /// object-feather-object.md Expected results 5: a 40px object has no core after 41px erosion.
    #[test]
    fn object_radius_twenty_erases() {
        let out = bb(OBJECT, json!({"radius":20}), &square());
        assert!(out.data.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
        assert_eq!(px(&out, 50, 50), [255, 255, 255, 0]);
    }
    /// selection-feather-selection.md Expected results 1: exact tent, red RGB and every outside byte.
    #[test]
    fn selection_radius_zero_tent() {
        let src = solid(30, 20, [255, 0, 0, 255]);
        let out = selected(&src, [5, 5, 25, 15], 0);
        tent(&out);
        outside(&src, &out, [5, 5, 25, 15]);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [255, 0, 0])
        );
    }
    /// selection-feather-selection.md Expected results 2: all four sides of the erased outer ring.
    #[test]
    fn selection_radius_one_ring() {
        let src = solid(30, 20, [255, 0, 0, 255]);
        let out = selected(&src, [4, 4, 26, 16], 1);
        ring(&out);
        tent(&out);
        outside(&src, &out, [4, 4, 26, 16]);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [255, 0, 0])
        );
    }
    /// selection-feather-selection.md Expected results 3: mirrored profile, opaque centre, full complement.
    #[test]
    fn selection_radius_three_profile() {
        let src = solid(100, 100, [255; 4]);
        let out = selected(&src, [30, 30, 70, 70], 3);
        profile(&out, &[26, 57, 102, 153, 198, 229, 245, 252, 254, 255]);
        assert_eq!(px(&out, 50, 50), [255; 4]);
        outside(&src, &out, [30, 30, 70, 70]);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [255; 3])
        );
    }
    /// selection-feather-selection.md Expected results 4: transparent colours survive every radius.
    #[test]
    fn selection_transparent_stays_transparent() {
        let src = from_fn(40, 40, |x, y| [x as u8, (x + y) as u8, (3 * y) as u8, 0]);
        for radius in 0..=20 {
            assert_eq!(selected(&src, [5, 5, 30, 30], radius).data, src.data);
            assert_eq!(bb(SELECTION, json!({"radius":radius}), &src).data, src.data);
        }
    }
    /// selection-feather-selection.md Expected results 5: all canvas sides fade monotonically inward.
    #[test]
    fn selection_without_selection_feathers_canvas() {
        let out = bb(SELECTION, json!({"radius":3}), &solid(100, 100, [255; 4]));
        for edge in 0..4 {
            let at = |i| {
                let (x, y) = match edge {
                    0 => (i, 50),
                    1 => (99 - i, 50),
                    2 => (50, i),
                    _ => (50, 99 - i),
                };
                px(&out, x, y)[3]
            };
            assert!(at(0) < 60);
            assert!(at(9) >= 250);
            for i in 0..49 {
                assert!(at(i) <= at(i + 1));
            }
        }
        assert_eq!(px(&out, 50, 50), [255; 4]);
    }
    /// selection-feather-selection.md Behaviour 1–5: ellipse geometry and fractional host clipping.
    #[test]
    fn selection_effects_on_an_ellipse() {
        let (m, p) = ellipse_selection(80, 80, [40.0, 40.0], [30.0, 20.0]);
        let src = solid(80, 80, [43, 87, 169, 255]);
        let env = with_selection(80, 80, &m, &p);
        let out = bb_env(SELECTION, json!({"radius":4}), &src, &env);
        assert_eq!(px(&out, 40, 40), [43, 87, 169, 255]);
        assert!((12..30).all(|x| px(&out, x, 40)[3] <= px(&out, x + 1, 40)[3]));
        let mut raw_env = env;
        raw_env.mask = None;
        let raw = bb_env(SELECTION, json!({"radius":4}), &src, &raw_env);
        let mut partial = 0;
        for y in 0..80 {
            for x in 0..80 {
                let coverage = m.get(x, y);
                assert_eq!(
                    px(&out, x as u32, y as u32),
                    crate::pixel::ops::blend(
                        px(&src, x as u32, y as u32),
                        px(&raw, x as u32, y as u32),
                        coverage
                    )
                );
                if coverage == 0 {
                    assert_eq!(px(&out, x as u32, y as u32), px(&src, x as u32, y as u32));
                }
                if coverage > 0 && coverage < 255 {
                    partial += 1;
                }
            }
        }
        assert!(partial > 0);
    }
    /// object-feather-object.md Colour / Q22: original neighbour RGB, including zero-coverage retention.
    #[test]
    fn object_copies_the_recorded_original_neighbour() {
        let src = from_fn(21, 21, |x, y| {
            let dx = x as i32 - 10;
            let dy = y as i32 - 10;
            [
                x as u8 * 7,
                y as u8 * 9,
                (x + y) as u8 * 3,
                if dx * dx + dy * dy <= 9 { 255 } else { 0 },
            ]
        });
        let out = bb(OBJECT, json!({"radius":0}), &src);
        let copies = [
            (7, 9, 8, 9),
            (7, 11, 8, 11),
            (13, 9, 12, 9),
            (13, 11, 12, 11),
            (9, 7, 9, 8),
            (11, 7, 11, 8),
            (9, 13, 9, 12),
            (11, 13, 11, 12),
        ];
        for y in 0..21 {
            for x in 0..21 {
                let colour = if let Some(&(_, _, nx, ny)) =
                    copies.iter().find(|&&(tx, ty, _, _)| tx == x && ty == y)
                {
                    px(&src, nx, ny)
                } else {
                    px(&src, x, y)
                };
                assert_eq!(px(&out, x, y)[..3], colour[..3], "({x},{y})");
            }
        }
        for (x, y) in [(8, 7), (8, 13)] {
            assert_eq!(px(&out, x, y), px(&src, x, y));
            assert_eq!(px(&out, x, y)[3], 0);
        }
        assert_eq!(px(&out, 7, 9)[3], 62);
        assert_eq!(px(&out, 7, 11)[3], 63);
        assert_eq!(px(&out, 9, 7)[3], 63);
        assert_eq!(px(&out, 11, 7)[3], 31);
    }
    /// object-feather-object.md Behaviour 1–5: any nonzero alpha enters the mask, RGB never blurs.
    #[test]
    fn object_uses_any_nonzero_alpha_and_preserves_raw_colour() {
        for a in [1, 2, 127, 254] {
            let src = rect(40, 40, [5, 5, 35, 35], [31, 97, 183, a]);
            let out = bb(OBJECT, json!({"radius":0}), &src);
            assert_eq!(out.data, src.data);
            let blurred = bb(OBJECT, json!({"radius":3}), &src);
            assert_eq!(px(&blurred, 20, 20), [31, 97, 183, a]);
            assert_eq!(px(&blurred, 5, 20), [31, 97, 183, 0]);
            for (s, o) in src
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .zip(blurred.data.as_chunks::<4>().0.iter())
            {
                assert_eq!(s[..3], o[..3]);
                assert!(o[3] <= s[3]);
            }
        }
        let full = bb(
            OBJECT,
            json!({"radius":1}),
            &solid(30, 30, [31, 97, 183, 255]),
        );
        for i in 0..30 {
            for (x, y) in [(i, 0), (i, 29), (0, i), (29, i)] {
                assert_eq!(px(&full, x, y), [31, 97, 183, 0]);
            }
        }
        assert_eq!(px(&full, 15, 15), [31, 97, 183, 255]);
    }
    /// common.md §2 / object-feather-object.md Behaviour: shape from the whole layer, clip once.
    #[test]
    fn object_ignores_selection_shape_and_clips_once() {
        let src = rect(40, 40, [5, 5, 35, 35], [37, 111, 203, 255]);
        let (mut m, p) = rect_selection(40, 40, 5, 5, 30, 30);
        for y in 5..30 {
            m.data[(y * 40 + 5) as usize] = 128;
        }
        let env = with_selection(40, 40, &m, &p);
        for radius in [0, 1, 3, 20] {
            let whole = bb(OBJECT, json!({"radius":radius}), &src);
            let out = bb_env(OBJECT, json!({"radius":radius}), &src, &env);
            for y in 0..40 {
                for x in 0..40 {
                    assert_eq!(
                        px(&out, x, y),
                        crate::pixel::ops::blend(
                            px(&src, x, y),
                            px(&whole, x, y),
                            m.get(x as i32, y as i32)
                        )
                    );
                }
            }
        }
    }
    /// Both specs Behaviour: CPU byte paths ignore stale GPU context and remain deterministic.
    #[test]
    fn cpu_context_and_input_independence() {
        let src = from_fn(30, 20, |x, y| {
            [
                x as u8 * 7,
                y as u8 * 9,
                73,
                if (4..26).contains(&x) && (4..16).contains(&y) {
                    255
                } else {
                    0
                },
            ]
        });
        for id in [OBJECT, SELECTION] {
            for radius in [0, 1, 3, 20] {
                let expected = bb(id, json!({"radius":radius}), &src);
                for space in [Space::Srgb, Space::Linear] {
                    for alpha in [Alpha::Straight, Alpha::Premultiplied] {
                        let info = RenderInfo::new(space, alpha);
                        let mut env = EffectEnv::whole(30, 20);
                        env.render_info = Some(info);
                        assert_eq!(
                            bb_env(id, json!({"radius":radius}), &src, &env).data,
                            expected.data
                        );
                        assert_eq!(env.render_info, Some(info));
                    }
                }
            }
        }
    }
    /// Both specs Controls / common.md §3–4: complete metadata consumed by the real dialog.
    #[test]
    fn registered_dialog_contracts() {
        for (id, name, category, icon) in [
            (
                OBJECT,
                "Object",
                "object",
                "object_tools__feather_object.png",
            ),
            (
                SELECTION,
                "Selection",
                "selection",
                "selection_tools__feather_selection.png",
            ),
        ] {
            let def = find(id).unwrap();
            assert_eq!(def.category, Category::Effect(category));
            assert_eq!(def.icon, icon);
            assert_eq!(def.dialog_title, Some(format!("Feather {name}").as_str()));
            assert!(def.dialog.is_none());
            assert!(def.is_configurable());
            assert!(matches!(
                def.kind,
                EffectKind::Cpu {
                    clip_to_selection: true,
                    ..
                }
            ));
            assert_eq!(def.controls.len(), 1);
            assert!(
                matches!(def.controls[0],Control::Range{key:"radius",label:"Radius",min:0.0,max:20.0,default:1.0,step:1.0,exponential:false,track:"default",keyboard} if keyboard.decimals==0 && keyboard.small==1.0 && keyboard.large==5.0 && !keyboard.exponential)
            );
            let ui = def.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(ui.transient_keys.is_empty());
            assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
            let c = ui.control("radius");
            assert!(!c.header_line);
            assert_eq!(c.caption, "");
            assert!(matches!(c.note, Note::None));
            assert!(c.reset);
            assert!(!c.wrap);
            assert!(c.track.is_none());
            assert!(c.decimals.is_none());
            let values = indirect_defaults(&def, &EffectEnv::whole(30, 20));
            assert_eq!(values.len(), 1);
            assert_eq!(values["radius"], json!(1.0));
            assert!((ui.enabled)(&values, "radius"));
        }
    }
    /// common.md §3: preview/Cancel, remembered values, reopening and Repeat use registered effects.
    #[test]
    fn session_preview_cancel_values_and_repeat() {
        use crate::effect::session::{EffectSession, repeat_last};
        use crate::testing::fresh_doc;
        for id in [OBJECT, SELECTION] {
            let mut app = fresh_doc(30, 20, None);
            let src = rect(30, 20, [4, 4, 26, 16], [31, 97, 183, 255]);
            app.documents[0]
                .document
                .layers
                .get_at_mut(0)
                .unwrap()
                .surface
                .put_image_data(&src, 0, 0);
            let def = find(id).unwrap();
            let session = EffectSession::begin(&mut app, def.clone()).unwrap();
            let source = session.source().clone();
            let mut values = session.defaults();
            values.insert("radius".into(), json!(3));
            let preview = session.render(&values);
            assert_ne!(preview.data, source.data);
            session.show(&mut app, Some(&preview));
            session.cancel(&mut app);
            assert!(!app.effect_tokens.contains_key(id));
            assert!(app.last_effect.is_none());
            let session = EffectSession::begin(&mut app, def.clone()).unwrap();
            assert_eq!(session.source().data, source.data);
            let expected = session.render(&values);
            assert!(session.commit(&mut app, values.clone()));
            assert_eq!(app.effect_tokens[id], values);
            assert_eq!(app.last_effect.as_deref(), Some(id));
            let reopened = EffectSession::begin(&mut app, def).unwrap();
            assert_eq!(reopened.initial_values(&app), values);
            assert_eq!(reopened.source().data, expected.data);
            let repeated = reopened.render(&values);
            reopened.cancel(&mut app);
            assert!(repeat_last(&mut app));
            let next = EffectSession::begin(&mut app, find(id).unwrap()).unwrap();
            assert_eq!(next.source().data, repeated.data);
        }
    }
    /// object-feather-object.md Behaviour 1–3: erosion and flattening admit no greys before blur.
    #[test]
    fn object_preblur_mask_is_binary() {
        let src = from_fn(11, 9, |x, y| {
            [
                17,
                91,
                183,
                if x == 5 && y == 4 {
                    0
                } else {
                    (x + y + 1) as u8
                },
            ]
        });
        for r in [0, 1, 2, 5] {
            let mask = super::object_mask(&src, r);
            for y in 0..9 {
                for x in 0..11 {
                    let keep = (y as i32 - r..=y as i32 + r).all(|j| {
                        (x as i32 - r..=x as i32 + r).all(|i| {
                            (0..11).contains(&i)
                                && (0..9).contains(&j)
                                && px(&src, i as u32, j as u32)[3] > 0
                        })
                    });
                    let v = if keep { 1.0 } else { 0.0 };
                    assert_eq!(mask.at(x, y), [v, v, v, 1.0]);
                }
            }
        }
    }
    /// selection-feather-selection.md Behaviour 1–2: round joins subtract a Euclidean corner band.
    #[test]
    fn selection_mask_round_concave_corner() {
        let p = vec![vec![
            [5.0, 5.0],
            [35.0, 5.0],
            [35.0, 15.0],
            [15.0, 15.0],
            [15.0, 35.0],
            [5.0, 35.0],
        ]];
        let mut env = EffectEnv::whole(40, 40);
        env.selection = &p;
        let mask = super::selection_mask(40, 40, &env, 5);
        assert_eq!(mask.at(10, 10)[0], 1.0);
        assert_eq!(mask.at(11, 11)[0], 0.0);
        assert!(mask.px.iter().all(|p| p[0] == 0.0 || p[0] == 1.0));
    }
    /// selection-feather-selection.md Behaviour 5: minimum, not multiplication, for partial alpha.
    #[test]
    fn selection_alpha_minimum_and_raw_rgb() {
        let src = from_fn(32, 28, |x, y| {
            [
                x as u8 * 7,
                y as u8 * 7,
                143,
                [0, 1, 63, 128, 254, 255][((x + y) % 6) as usize],
            ]
        });
        let opaque = from_fn(32, 28, |x, y| {
            let mut p = px(&src, x, y);
            p[3] = 255;
            p
        });
        for r in 0..=20 {
            let out = selected(&src, [4, 4, 28, 24], r);
            let mask = selected(&opaque, [4, 4, 28, 24], r);
            for y in 0..28 {
                for x in 0..32 {
                    let s = px(&src, x, y);
                    let o = px(&out, x, y);
                    assert_eq!(o[..3], s[..3]);
                    assert_eq!(o[3], s[3].min(px(&mask, x, y)[3]));
                }
            }
        }
    }
    /// Both Feather specs Notes / Q32: transparent finite input, no edge renormalization or reflection.
    #[test]
    fn finite_mask_blur_drops_off_canvas_taps() {
        let mask = crate::effect::surface::EffectSurface::from_fn(3, 2, |_, _| [1.0; 4]);
        assert_eq!(super::mask_bytes(mask, 1.0), [114, 144, 114, 114, 144, 114]);
    }
    /// Both Feather specs Behaviour: empty and narrow canvases stay bounded, hidden colour is retained.
    #[test]
    fn tiny_canvases_and_empty_objects() {
        for (w, h) in [
            (0, 0),
            (0, 3),
            (3, 0),
            (1, 1),
            (1, 5),
            (5, 1),
            (2, 2),
            (2, 3),
        ] {
            for id in [OBJECT, SELECTION] {
                for radius in [0, 1, 2, 3, 20] {
                    let src = solid(w, h, [37, 91, 173, 0]);
                    let out = bb(id, json!({"radius":radius}), &src);
                    assert_eq!((out.width, out.height), (w, h));
                    assert_eq!(out.data, src.data);
                    let opaque = solid(w, h, [37, 91, 173, 255]);
                    let out = bb(id, json!({"radius":radius}), &opaque);
                    assert_eq!((out.width, out.height), (w, h));
                    assert_eq!(out.data.len(), opaque.data.len());
                    assert!(
                        out.data
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|p| p[..3] == [37, 91, 173])
                    );
                    if radius >= 2 {
                        assert!(out.data.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
                    }
                }
            }
        }
    }
}
