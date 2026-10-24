//! BB spec: effects/artistic-oil-painting-plus.md; effects/artistic-pastel.md; common.md §1–5.
use super::classic::{median, oil_painting, pastel_recolour, pencil_sketch};
use super::manifest::manifest;
use super::ui::{ControlUi, Help, HelpWindow, IndirectUi, TRACK_WHITE_BLACK};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::effect::{EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::pixel::ops::compose;

static OIL_UI: IndirectUi = IndirectUi {
    // The plugin supplies text but no help type, so the default is no button.
    help: Help::None,
    ..IndirectUi::DEFAULT
};
static PASTEL_UI: IndirectUi = IndirectUi {
    controls: &[
        (
            "pencil",
            ControlUi {
                track: Some(&TRACK_WHITE_BLACK),
                decimals: Some(3),
                ..ControlUi::DEFAULT
            },
        ),
        ("tip", ControlUi::NO_HEADER),
    ],
    enabled: |v, key| key != "tip" || num(v, "pencil") > 0.0,
    help: Help::Window(HelpWindow::Standard),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let oil = manifest("bbOilPaintingPlus").unwrap();
    let pastel = manifest("bbPastel").unwrap();
    vec![
        EffectDef {
            id: oil.id,
            icon: oil.icon,
            category: oil.category,
            controls: vec![
                range("shadowAmount", "Shadow Amount", 1.0, 10.0, 5.0),
                range("brushSize", "Brush Size", 1.0, 10.0, 5.0),
                range("shadowDetail", "Shadow Detail", 1.0, 10.0, 3.0),
            ],
            kind: EffectKind::Cpu {
                render: oil_plus,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some("Oil Painting+"),
            indirect: Some(&OIL_UI),
        },
        EffectDef {
            id: pastel.id,
            icon: pastel.icon,
            category: pastel.category,
            controls: vec![
                range("size", "Pastel Size", 1.0, 8.0, 3.0),
                range("roughness", "Roughness", 3.0, 255.0, 50.0),
                range("pencil", "Pencil Amount", 0.0, 1.0, 0.5)
                    .step(0.01)
                    .slider_keys(3, 0.05, 0.25, false),
                range("tip", "Pencil Tip Size", 1.0, 20.0, 2.0),
            ],
            kind: EffectKind::Cpu {
                render: pastel_render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some("Pastel Effect"),
            indirect: Some(&PASTEL_UI),
        },
    ]
}

fn recolour(img: ImageData) -> ImageData {
    ImageData {
        width: img.width,
        height: img.height,
        data: img
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&p| pastel_recolour(p))
            .collect(),
    }
}

fn oil_painted(src: &ImageData, brush: u32) -> ImageData {
    // quirk(bb): artistic-oil-painting-plus.md step 1 clamps this stage to 8;
    // the later median radii still use the requested brush size.
    recolour(oil_painting(src, brush.min(8), 50))
}

fn oil_plus(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let b = num(v, "brushSize").clamp(1.0, 10.0) as u32;
    let d = num(v, "shadowDetail").clamp(1.0, 10.0) as u32;
    let n = num(v, "shadowAmount").clamp(1.0, 10.0) as u32;
    let painted = oil_painted(src, b);
    let smoothed = median(src, b, 50);
    let sketch = pencil_sketch(&smoothed, d, (2 * d) as i32);
    let mut out = median(&painted, 2 * b, 50);
    // ponytail: rebuilt per render; cache painted/smoothed by brush size if previews are slow.
    for _ in 0..n {
        for (p, &s) in out
            .data
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(sketch.data.as_chunks::<4>().0)
        {
            *p = compose(BlendMode::Multiply, *p, s, 255);
        }
    }
    out
}

fn pastel_render(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let size = num(v, "size").clamp(1.0, 8.0) as u32;
    let roughness = num(v, "roughness").clamp(3.0, 255.0) as u32;
    let pencil = num(v, "pencil").clamp(0.0, 1.0);
    let mut paint = recolour(oil_painting(src, size, roughness));
    if pencil == 0.0 {
        return paint;
    }
    let tip = num(v, "tip").clamp(1.0, 20.0) as u32;
    let sketch = pencil_sketch(src, tip, 0);
    for (p, &s) in paint
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(sketch.data.as_chunks::<4>().0)
    {
        // The supplied transparent-black case leaves a black alpha-127 haze at
        // the default amount; do not multiply by source or sketch alpha again.
        let alpha = ((255 - s[0]) as f64 * pencil) as u8;
        *p = compose(BlendMode::Normal, *p, [0, 0, 0, alpha], 255);
    }
    paint
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::classic::{oil_painting, pastel_recolour};
    use crate::effect::bb::test_util::*;
    use serde_json::json;

    fn photo(w: u32, h: u32) -> crate::canvas::ImageData {
        let mut data = crate::testing::fixture_rgba(w, h, 7);
        for p in data.chunks_mut(4) {
            p[3] = 255;
        }
        crate::canvas::ImageData {
            width: w,
            height: h,
            data,
        }
    }

    /// artistic-oil-painting-plus.md Expected results 1–5.
    #[test]
    fn oil_painting_plus() {
        let flat = bb(
            "bbOilPaintingPlus",
            json!({}),
            &solid(64, 64, [128, 128, 128, 255]),
        );
        let c = px(&flat, 32, 32);
        assert!(
            c[0] == c[1] && c[1] == c[2] && c[0].abs_diff(202) <= 6,
            "{c:?}"
        );
        assert!(flat.data.chunks(4).all(|q| q == c));
        let img = photo(32, 32);
        for n in 1..10 {
            let a = bb("bbOilPaintingPlus", json!({"shadowAmount": n}), &img);
            let b = bb("bbOilPaintingPlus", json!({"shadowAmount": n + 1}), &img);
            assert!(
                a.data
                    .chunks(4)
                    .zip(b.data.chunks(4))
                    .all(|(p, q)| (0..3).all(|i| q[i] <= p[i])),
                "N {n}"
            );
        }
        let o = bb(
            "bbOilPaintingPlus",
            json!({}),
            &solid(64, 64, [255, 64, 0, 255]),
        );
        let q = px(&o, 32, 32);
        assert!(o.data.as_chunks::<4>().0.iter().all(|p| p == &q));
        let ratio = q[1] as f64 / q[0] as f64;
        assert!((ratio - 1.0 / 3.0).abs() < 0.02 && q[2] <= 2, "{q:?}");
        assert_eq!(
            super::oil_painted(&img, 8).data,
            super::oil_painted(&img, 9).data
        );
        assert_eq!(
            super::oil_painted(&img, 9).data,
            super::oil_painted(&img, 10).data
        );
        assert_ne!(
            bb("bbOilPaintingPlus", json!({"brushSize": 8}), &img).data,
            bb("bbOilPaintingPlus", json!({"brushSize": 9}), &img).data
        );
        let (mask, polygons) = rect_selection(32, 32, 8, 8, 24, 20);
        let selected = bb_env(
            "bbOilPaintingPlus",
            json!({}),
            &img,
            &with_selection(32, 32, &mask, &polygons),
        );
        let whole = bb("bbOilPaintingPlus", json!({}), &img);
        for y in 0..32 {
            for x in 0..32 {
                let want = if (8..24).contains(&x) && (8..20).contains(&y) {
                    px(&whole, x, y)
                } else {
                    px(&img, x, y)
                };
                assert_eq!(px(&selected, x, y), want);
            }
        }
    }

    /// Both classic effects use whole-image intermediates, final selection clipping,
    /// and the dialog metadata in the supplied effect files.
    #[test]
    fn presentation_and_pastel_selection() {
        use crate::effect::bb::indirect_defaults;
        use crate::effect::bb::ui::{Help, HelpWindow, TRACK_WHITE_BLACK};
        use crate::effect::{Control, EffectEnv, EffectKind, find};
        let oil = find("bbOilPaintingPlus").unwrap();
        let pastel = find("bbPastel").unwrap();
        assert!(matches!(
            oil.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        assert!(matches!(
            pastel.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        assert!(matches!(oil.indirect.unwrap().help, Help::None));
        assert!(matches!(
            pastel.indirect.unwrap().help,
            Help::Window(HelpWindow::Standard)
        ));
        assert_eq!(oil.dialog_title, Some("Oil Painting+"));
        assert_eq!(pastel.dialog_title, Some("Pastel Effect"));
        let ui = pastel.indirect.unwrap();
        assert_eq!(ui.control("pencil").track, Some(&TRACK_WHITE_BLACK[..]));
        assert_eq!(ui.control("pencil").decimals, Some(3));
        assert!(!ui.control("tip").header_line);
        let mut values = indirect_defaults(&pastel, &EffectEnv::whole(24, 24));
        assert!((ui.enabled)(&values, "tip"));
        values.insert("pencil".into(), json!(0));
        assert!(!(ui.enabled)(&values, "tip"));
        assert!(matches!(pastel.controls[2], Control::Range { step, .. } if step == 0.01));
        let Control::Range { keyboard, .. } = pastel.controls[2] else {
            panic!("pencil slider")
        };
        assert_eq!(keyboard.decimals, 3);
        assert_eq!(keyboard.stepped(0.5, 0.0, 1.0, true, false), 0.55);
        assert_eq!(keyboard.stepped(0.5, 0.0, 1.0, true, true), 0.75);
        assert_eq!(keyboard.stepped(0.5, 0.0, 1.0, false, false), 0.45);
        let verify = |def: &crate::effect::EffectDef,
                      expected: &[(&str, &str, f64, f64, f64, bool)]| {
            assert_eq!(def.controls.len(), expected.len());
            for (control, &(key, label, min, max, default, header)) in
                def.controls.iter().zip(expected)
            {
                let Control::Range {
                    key: got_key,
                    label: got_label,
                    min: got_min,
                    max: got_max,
                    default: got_default,
                    ..
                } = control
                else {
                    panic!("expected range")
                };
                assert_eq!(
                    (*got_key, *got_label, *got_min, *got_max, *got_default),
                    (key, label, min, max, default)
                );
                assert_eq!(def.indirect.unwrap().control(key).header_line, header);
            }
        };
        verify(
            &oil,
            &[
                ("shadowAmount", "Shadow Amount", 1., 10., 5., true),
                ("brushSize", "Brush Size", 1., 10., 5., true),
                ("shadowDetail", "Shadow Detail", 1., 10., 3., true),
            ],
        );
        verify(
            &pastel,
            &[
                ("size", "Pastel Size", 1., 8., 3., true),
                ("roughness", "Roughness", 3., 255., 50., true),
                ("pencil", "Pencil Amount", 0., 1., 0.5, true),
                ("tip", "Pencil Tip Size", 1., 20., 2., false),
            ],
        );

        let img = photo(24, 24);
        let (mask, polygons) = rect_selection(24, 24, 4, 4, 20, 20);
        let selected = bb_env(
            "bbPastel",
            json!({"pencil": 0}),
            &img,
            &with_selection(24, 24, &mask, &polygons),
        );
        let whole = bb("bbPastel", json!({"pencil": 0}), &img);
        for y in 0..24 {
            for x in 0..24 {
                let want = if (4..20).contains(&x) && (4..20).contains(&y) {
                    px(&whole, x, y)
                } else {
                    px(&img, x, y)
                };
                assert_eq!(px(&selected, x, y), want);
            }
        }
    }

    /// artistic-pastel.md Expected results 1–5.
    #[test]
    fn pastel() {
        assert!(
            bb(
                "bbPastel",
                json!({"size": 5, "roughness": 200}),
                &solid(32, 32, [0, 0, 0, 255])
            )
            .data
            .chunks(4)
            .all(|q| q == [203, 203, 203, 255])
        );
        let gray = bb("bbPastel", json!({}), &solid(32, 32, [128, 128, 128, 255]));
        assert!(
            gray.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p == &[246, 246, 246, 255])
        );
        for (i, o) in [
            ([255, 64, 0], [255, 84, 0]),
            ([200, 100, 50], [255, 127, 63]),
            ([0, 128, 255], [0, 127, 255]),
            ([255, 255, 255], [255, 255, 255]),
        ] {
            let src = solid(32, 32, [i[0], i[1], i[2], 255]);
            assert_eq!(
                px(&bb("bbPastel", json!({"pencil": 0}), &src), 16, 16),
                [o[0], o[1], o[2], 255]
            );
            assert_px_tol(
                &bb("bbPastel", json!({}), &src),
                16,
                16,
                [o[0], o[1], o[2], 255],
                1,
            );
        }
        let img = photo(24, 24);
        let off = bb("bbPastel", json!({"pencil": 0}), &img);
        let want: Vec<u8> = oil_painting(&img, 3, 50)
            .data
            .chunks(4)
            .flat_map(|p| pastel_recolour(p.try_into().unwrap()))
            .collect();
        assert_eq!(off.data, want);
        assert_eq!(
            bb("bbPastel", json!({"pencil": 0, "tip": 9}), &img).data,
            off.data
        );
        let half = from_fn(
            64,
            64,
            |x, _| if x < 32 { [0, 0, 0, 255] } else { [255; 4] },
        );
        let out = bb("bbPastel", json!({}), &half);
        assert_px(&out, 20, 32, [203, 203, 203, 255]);
        assert_px(&out, 45, 32, [255; 4]);
        assert!((26..32).any(|x| px(&out, x, 32)[0] < 203));
        let at = |a: f64| px(&bb("bbPastel", json!({"pencil": a}), &half), 31, 32)[0];
        assert!(at(0.0) >= at(0.25) && at(0.25) >= at(0.5) && at(0.5) >= at(1.0));
        assert!(
            bb("bbPastel", json!({}), &solid(16, 16, [0; 4]))
                .data
                .chunks(4)
                .all(|q| q == [0, 0, 0, 127])
        );
        assert!(
            bb("bbPastel", json!({"pencil": 0}), &solid(16, 16, [0; 4]))
                .data
                .chunks(4)
                .all(|q| q[3] == 0)
        );
    }
}
