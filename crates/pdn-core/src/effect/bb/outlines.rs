//! BB spec: effects/object-outline-object.md; effects/selection-outline-selection.md; common.md §1–5

use super::blocks::{colour_exact, source_over, straight_over};
use super::color_value_alpha;
use super::conventions::{AA_BYTE_CONVERSION, GPU_CONTEXT, RADIUS_TO_SIGMA};
use super::manifest::manifest;
use super::raster::{self, Cap, Join, Pen};
use super::staircase::{disc_dilate, staircase};
use super::ui::{ControlUi, IndirectUi};
use crate::canvas::ImageData;
use crate::effect::blur::gaussian;
use crate::effect::surface::{Alpha, Edge, EffectSurface, premultiply, unpremultiply};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, RenderInfo, Values, choice, flag, num,
    range,
};
use std::sync::LazyLock;

const COLOUR_UI: ControlUi = ControlUi {
    alpha: true,
    reset: false,
    ..ControlUi::NO_HEADER
};
static OBJECT_UI: IndirectUi = IndirectUi {
    controls: &[
        ("width", ControlUi::NO_HEADER),
        ("color", COLOUR_UI),
        ("antiAlias", ControlUi::NO_HEADER),
        ("blur", ControlUi::NO_HEADER),
    ],
    ..IndirectUi::DEFAULT
};
static LOCATION_CAPTION: LazyLock<String> = LazyLock::new(|| format!("Location{}", " ".repeat(5)));
static STYLE_CAPTION: LazyLock<String> = LazyLock::new(|| format!("Line style{}", " ".repeat(4)));
static SELECTION_ROWS: LazyLock<[(&str, ControlUi); 6]> = LazyLock::new(|| {
    [
        ("width", ControlUi::NO_HEADER),
        ("color", COLOUR_UI),
        (
            "location",
            ControlUi {
                caption: &LOCATION_CAPTION,
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "corner",
            ControlUi {
                caption: "Corner style",
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "style",
            ControlUi {
                caption: &STYLE_CAPTION,
                ..ControlUi::NO_HEADER
            },
        ),
        ("antiAlias", ControlUi::NO_HEADER),
    ]
});
static SELECTION_UI: LazyLock<IndirectUi> = LazyLock::new(|| IndirectUi {
    controls: &*SELECTION_ROWS,
    ..IndirectUi::DEFAULT
});

pub fn effects() -> Vec<EffectDef> {
    let object = manifest("bbOutlineObject").unwrap();
    let selection = manifest("bbOutlineSelection").unwrap();
    vec![
        EffectDef {
            id: object.id,
            icon: object.icon,
            category: object.category,
            controls: vec![
                range("width", "Width", 1.0, 100.0, 10.0),
                Control::Color {
                    key: "color",
                    label: "Color",
                    default: ColorDefault::Rgb(0),
                },
                Control::Checkbox {
                    key: "antiAlias",
                    label: "Anti-Alias outline",
                    default: true,
                },
                range("blur", "Blur radius", 0.0, 100.0, 0.0),
            ],
            kind: EffectKind::Gpu(GPU_CONTEXT, outline_object),
            dialog: None,
            dialog_title: Some("Outline Object"),
            indirect: Some(&OBJECT_UI),
        },
        EffectDef {
            id: selection.id,
            icon: selection.icon,
            category: selection.category,
            controls: vec![
                range("width", "Outline width", 1.0, 100.0, 5.0),
                Control::Color {
                    key: "color",
                    label: "Color",
                    default: ColorDefault::Rgb(0),
                },
                Control::Select {
                    key: "location",
                    label: "",
                    default: "on",
                    choices: &[
                        ("on", "On selection"),
                        ("inside", "Inside selection"),
                        ("outside", "Outside selection"),
                    ],
                },
                Control::Select {
                    key: "corner",
                    label: "",
                    default: "miter",
                    choices: &[
                        ("miter", "Miter"),
                        ("bevel", "Bevel"),
                        ("round", "Round"),
                        ("miterOrBevel", "Miter or Bevel"),
                    ],
                },
                Control::Select {
                    key: "style",
                    label: "",
                    default: "solid",
                    choices: &[("solid", "Solid"), ("dash", "Dash")],
                },
                Control::Checkbox {
                    key: "antiAlias",
                    label: "Anti-Alias outline",
                    default: true,
                },
            ],
            kind: EffectKind::Cpu {
                render: outline_selection,
                clip_to_selection: false,
            },
            dialog: None,
            dialog_title: Some("Outline Selection"),
            indirect: Some(&SELECTION_UI),
        },
    ]
}

fn outline_object(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let mask: Vec<bool> = src.px.iter().map(|p| p[3] > 0.5).collect();
    let mut coverage: Vec<u8> = disc_dilate(&mask, src.w, src.h, num(v, "width") as u32)
        .into_iter()
        .map(|inside| if inside { 255 } else { 0 })
        .collect();
    if flag(v, "antiAlias") {
        coverage = staircase(&coverage, src.w, src.h, AA_BYTE_CONVERSION).0;
    }
    let colour = unpremultiply(colour_exact(color_value_alpha(v, "color", env), info));
    let outline = EffectSurface::from_fn(src.w, src.h, |x, y| {
        [
            colour[0],
            colour[1],
            colour[2],
            coverage[(y * src.w + x) as usize] as f32 / 255.0 * colour[3],
        ]
    });
    // quirk(bb): object-outline-object.md Behaviour 5 (straight RGB and alpha blur independently).
    let outline = if num(v, "blur") > 0.0 {
        // approximation: shared high-quality Gaussian for undocumented option 3 (object-outline-object.md Notes / uncertainties).
        gaussian(
            &outline,
            num(v, "blur") * RADIUS_TO_SIGMA,
            Edge::Transparent,
        )
    } else {
        outline
    };
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let original = match info.input {
            Alpha::Straight => src.at(x, y),
            Alpha::Premultiplied => unpremultiply(src.at(x, y)),
        };
        let out = straight_over(outline.at(x, y), original);
        match info.output {
            Alpha::Straight => out,
            Alpha::Premultiplied => premultiply(out),
        }
    })
}

fn outline_selection(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    selection_with(src, v, env, GPU_CONTEXT)
}

// CPU dispatch has no RenderInfo. The topology is premultiplied; only colour space varies in tests.
fn selection_with(src: &ImageData, v: &Values, env: &EffectEnv, info: RenderInfo) -> ImageData {
    let colour = colour_exact(color_value_alpha(v, "color", env), info);
    if colour[3] == 0.0 {
        return src.clone();
    }
    let Some(path) = raster::polygon_path(&raster::selection_or_canvas(env), [0.0, 0.0], true)
    else {
        return src.clone();
    };
    let location = choice(v, "location");
    let width = num(v, "width") as f32 * if location == "on" { 1.0 } else { 2.0 };
    let pen = Pen {
        width,
        join: match choice(v, "corner") {
            "bevel" => Join::Bevel,
            "round" => Join::Round,
            "miterOrBevel" => Join::MiterOrBevel,
            _ => Join::Miter,
        },
        cap: Cap::Flat,
        miter_limit: 10.0,
        dash: (choice(v, "style") == "dash").then_some([2.0 * width; 2]),
    };
    let aa = flag(v, "antiAlias");
    let mut band = raster::stroke_coverage(&path, &pen, src.width, src.height, aa);
    if location != "on" {
        let fill = raster::fill_coverage(&path, src.width, src.height, aa);
        // approximation: D2D geometry combine on coverage; exact on pixel-aligned edges (selection-outline-selection.md Behaviour 3).
        for (coverage, fill) in band.iter_mut().zip(fill) {
            *coverage *= if location == "inside" {
                fill
            } else {
                1.0 - fill
            };
        }
    }
    let mut surface = EffectSurface::from_rgba8(src, info.space, Alpha::Premultiplied);
    for (p, &coverage) in surface.px.iter_mut().zip(&band) {
        *p = source_over(*p, colour.map(|c| c * coverage));
    }
    let mut out = surface.to_rgba8(info.space, Alpha::Premultiplied);
    for ((p, original), &coverage) in out
        .data
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.data.as_chunks::<4>().0)
        .zip(&band)
    {
        if coverage == 0.0 {
            *p = *original;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, GPU_CONTEXT};
    use crate::effect::bb::indirect_defaults;
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::{Help, HelpWindow, Note};
    use crate::effect::{Category, ColorDefault, Control, EffectEnv, EffectKind, find};
    use serde_json::{Value, json};

    const OBJECT: &str = "bbOutlineObject";
    const SELECTION: &str = "bbOutlineSelection";
    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const WHITE: [u8; 4] = [255; 4];
    const DISC: [[u8; 7]; 7] = [
        [0, 0, 63, 223, 31, 0, 0],
        [0, 255, 255, 255, 223, 192, 0],
        [62, 255, 255, 255, 255, 255, 62],
        [223, 255, 255, 255, 255, 255, 223],
        [63, 255, 255, 255, 255, 255, 63],
        [0, 192, 255, 255, 223, 192, 0],
        [0, 0, 63, 223, 31, 0, 0],
    ];

    fn dot(a: u8) -> ImageData {
        from_fn(21, 21, |x, y| {
            if (x, y) == (10, 10) {
                [255, 0, 0, a]
            } else {
                [0; 4]
            }
        })
    }

    fn selected(src: &ImageData, values: Value) -> ImageData {
        let (mask, polys) = rect_selection(100, 100, 20, 20, 80, 80);
        bb_env(
            SELECTION,
            values,
            src,
            &with_selection(100, 100, &mask, &polys),
        )
    }

    fn os(values: Value) -> ImageData {
        selected(&solid(100, 100, WHITE), values)
    }

    /// object-outline-object.md Expected results 1: exact disc, original above the outline.
    #[test]
    fn hard_disc() {
        let out = bb(OBJECT, json!({"width":3,"antiAlias":false}), &dot(255));
        let mut count = 0;
        for y in 0..21 {
            for x in 0..21 {
                let (dx, dy) = (x as i32 - 10, y as i32 - 10);
                let want = if dx == 0 && dy == 0 {
                    RED
                } else if dx * dx + dy * dy <= 9 {
                    BLACK
                } else {
                    [0; 4]
                };
                assert_eq!(px(&out, x, y), want, "({x},{y})");
                count += usize::from(px(&out, x, y)[3] > 0);
            }
        }
        assert_eq!(count, 29);
    }

    /// object-outline-object.md Expected results 2: the full asymmetric staircase vector.
    #[test]
    fn aa_disc() {
        let out = bb(OBJECT, json!({"width":3}), &dot(255));
        for y in 0..21 {
            for x in 0..21 {
                let a = if (7..14).contains(&x) && (7..14).contains(&y) {
                    DISC[(y - 7) as usize][(x - 7) as usize]
                } else {
                    0
                };
                let want = if (x, y) == (10, 10) {
                    RED
                } else {
                    [0, 0, 0, a]
                };
                assert_px(&out, x, y, want);
            }
        }
    }

    /// object-outline-object.md Expected results 3: 127 excluded, 128 included in both AA modes.
    #[test]
    fn alpha_threshold() {
        for aa in [false, true] {
            assert_eq!(
                bb(OBJECT, json!({"width":3,"antiAlias":aa}), &dot(127)).data,
                dot(127).data
            );
            let out = bb(OBJECT, json!({"width":3,"antiAlias":aa}), &dot(128));
            assert_px(&out, 10, 8, BLACK);
            assert_px(&out, 8, 10, BLACK);
            let non_red = from_fn(21, 21, |x, y| {
                if (x, y) == (10, 10) {
                    [32, 180, 64, 128]
                } else {
                    [0; 4]
                }
            });
            let other = bb(OBJECT, json!({"width":3,"antiAlias":aa}), &non_red);
            assert_eq!(px(&out, 10, 8), px(&other, 10, 8));
        }
    }

    /// object-outline-object.md Expected results 4: every outline pixel has the chosen colour/alpha.
    #[test]
    fn colour_alpha() {
        let out = bb(
            OBJECT,
            json!({"width":3,"antiAlias":false,"color":"#0000ff80"}),
            &dot(255),
        );
        for y in 0..21 {
            for x in 0..21 {
                let (dx, dy) = (x as i32 - 10, y as i32 - 10);
                let want = if (dx, dy) == (0, 0) {
                    RED
                } else if dx * dx + dy * dy <= 9 {
                    [0, 0, 255, 128]
                } else {
                    [0; 4]
                };
                assert_px_tol(&out, x, y, want, 1);
            }
        }
    }

    /// object-outline-object.md Expected results 5: full rotations/reflections and outward falloff.
    #[test]
    fn blurred_disc_properties() {
        let out = bb(
            OBJECT,
            json!({"width":3,"antiAlias":false,"blur":6}),
            &dot(255),
        );
        assert_eq!(px(&out, 10, 10), RED);
        for y in 0..21 {
            for x in 0..21 {
                let a = px(&out, x, y)[3];
                for (u, v) in [(20 - x, y), (x, 20 - y), (y, x), (20 - y, x)] {
                    assert!(
                        a.abs_diff(px(&out, u, v)[3]) <= TOL,
                        "({x},{y}) vs ({u},{v})"
                    );
                }
                if (10..20).contains(&x) {
                    assert!(px(&out, x + 1, y)[3] <= a.saturating_add(1));
                }
                if (10..20).contains(&y) {
                    assert!(px(&out, x, y + 1)[3] <= a.saturating_add(1));
                }
                if (x as i32 - 10).pow(2) + (y as i32 - 10).pow(2) >= 100 {
                    assert!(a <= 4, "distant ({x},{y}): {a}");
                }
            }
        }
    }

    /// object-outline-object.md Expected results 6: shape from the full layer, then host clipping.
    #[test]
    fn object_selection_clips_after_dilation() {
        let src = dot(255);
        let (mask, polys) = rect_selection(21, 21, 0, 0, 10, 21);
        let out = bb_env(
            OBJECT,
            json!({"width":3,"antiAlias":false}),
            &src,
            &with_selection(21, 21, &mask, &polys),
        );
        for y in 0..21 {
            for x in 0..21 {
                let (dx, dy) = (x as i32 - 10, y as i32 - 10);
                let want = if x < 10 && dx * dx + dy * dy <= 9 {
                    BLACK
                } else {
                    px(&src, x, y)
                };
                assert_eq!(px(&out, x, y), want, "({x},{y})");
            }
        }
    }

    /// object-outline-object.md Behaviour 4–6 / common.md §1: both effective colour contexts.
    #[test]
    fn object_partial_alpha_contexts() {
        for (info, want) in [
            (GPU_CONTEXT, [213, 0, 156, 192]),
            (ALT_GPU_CONTEXT, [170, 0, 85, 192]),
        ] {
            let out = bb_ctx(
                OBJECT,
                info,
                json!({"width":3,"antiAlias":false,"color":"#0000ff80"}),
                &dot(128),
            );
            assert_px(&out, 10, 10, want);
            assert_px_tol(&out, 10, 8, [0, 0, 255, 128], 1);
        }
    }

    /// R88: each GPU invocation's real input/output alpha modes reach the outline shader.
    #[test]
    fn effective_alpha_modes() {
        use crate::effect::{RenderInfo, surface::Alpha};
        for (context, want) in [
            (GPU_CONTEXT, [213, 0, 156, 192]),
            (ALT_GPU_CONTEXT, [170, 0, 85, 192]),
        ] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = RenderInfo {
                        space: context.space,
                        input,
                        output,
                    };
                    let out = bb_ctx(
                        OBJECT,
                        info,
                        json!({"width":3,"antiAlias":false,"color":"#0000ff80"}),
                        &dot(128),
                    );
                    assert_px(&out, 10, 10, want);
                }
            }
        }
    }

    /// selection-outline-selection.md Expected results 1: centred band on all four sides.
    #[test]
    fn selection_on() {
        let out = os(json!({}));
        for k in 14..26 {
            let p = px(&out, k, 50);
            for (x, y) in [(99 - k, 50), (50, k), (50, 99 - k)] {
                assert_px(&out, x, y, p);
            }
            if (18..22).contains(&k) {
                assert_eq!(p, BLACK);
            } else if k == 17 || k == 22 {
                assert!(p[0] > 2 && p[0] < 253);
                assert_eq!(p, [p[0], p[0], p[0], 255]);
            } else {
                assert_eq!(p, WHITE);
            }
        }
        assert_px(&out, 18, 18, BLACK);
        assert_px(&out, 81, 18, BLACK);
        assert_px(&out, 18, 81, BLACK);
        assert_px(&out, 81, 81, BLACK);
    }

    /// selection-outline-selection.md Expected results 2: exact inside band, complete exterior identity.
    #[test]
    fn selection_inside() {
        let out = os(json!({"location":"inside"}));
        for y in 0..100 {
            for x in 0..100 {
                let band = (20..80).contains(&x)
                    && (20..80).contains(&y)
                    && (!(25..75).contains(&x) || !(25..75).contains(&y));
                assert_eq!(
                    px(&out, x, y),
                    if band { BLACK } else { WHITE },
                    "({x},{y})"
                );
            }
        }
    }

    /// selection-outline-selection.md Expected results 3: round exterior corners and miter square.
    #[test]
    fn selection_outside_corners() {
        let out = os(json!({"location":"outside","corner":"round"}));
        for k in 15..20 {
            for (x, y) in [(k, 50), (99 - k, 50), (50, k), (50, 99 - k)] {
                assert_eq!(px(&out, x, y), BLACK);
            }
        }
        for (a, b) in [(15, 15), (84, 15), (15, 84), (84, 84)] {
            assert_eq!(px(&out, a, b), WHITE);
        }
        for (a, b) in [(18, 18), (81, 18), (18, 81), (81, 81)] {
            assert_eq!(px(&out, a, b), BLACK);
        }
        let miter = os(json!({"location":"outside"}));
        for (a, b) in [(15, 15), (84, 15), (15, 84), (84, 84)] {
            assert_eq!(px(&miter, a, b), BLACK);
        }
        for y in 20..80 {
            for x in 20..80 {
                assert_eq!(px(&out, x, y), WHITE);
            }
        }
    }

    /// selection-outline-selection.md Behaviour 2: four joins and the fixed miter limit of 10.
    #[test]
    fn acute_join_styles() {
        use crate::pixel::{IntRect, mask::Mask};
        let polys = vec![vec![[70.0, 50.0], [20.0, 54.0], [20.0, 46.0]]];
        let mask = Mask {
            rect: IntRect::new(0, 0, 160, 100),
            data: vec![0; 16000],
        };
        let env = with_selection(160, 100, &mask, &polys);
        // The apex angle is about 9.15 degrees. Its miter exceeds 10 half-widths.
        for (corner, near, far) in [
            ("miter", BLACK, BLACK),
            ("bevel", WHITE, WHITE),
            ("round", BLACK, WHITE),
            ("miterOrBevel", WHITE, WHITE),
        ] {
            let out = bb_env(
                SELECTION,
                json!({"width":10,"corner":corner,"antiAlias":false}),
                &solid(160, 100, WHITE),
                &env,
            );
            assert_eq!(px(&out, 73, 50), near, "{corner}");
            assert_eq!(px(&out, 90, 50), far, "{corner}");
            assert_eq!(
                px(&out, 125, 50),
                WHITE,
                "miter clips at the limit: {corner}"
            );
        }
    }

    /// selection-outline-selection.md Expected results 4: binary output, exactly five pixels per side.
    #[test]
    fn selection_aliased() {
        let out = os(json!({"antiAlias":false}));
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == BLACK || *p == WHITE)
        );
        for edge in 0..4 {
            let n = (0..50)
                .filter(|&k| {
                    let (x, y) = match edge {
                        0 => (k, 50),
                        1 => (99 - k, 50),
                        2 => (50, k),
                        _ => (50, 99 - k),
                    };
                    px(&out, x, y) == BLACK
                })
                .count();
            assert_eq!(n, 5, "side {edge}");
        }
    }

    /// selection-outline-selection.md Expected results 5 and Behaviour 3: all-side flat dashes.
    #[test]
    fn selection_dash_periods() {
        for (location, sample, period) in
            [("inside", 22, 20u32), ("outside", 17, 20), ("on", 20, 10)]
        {
            let out = os(json!({"location":location,"style":"dash"}));
            for side in 0..4 {
                let at = |k| match side {
                    0 => (k, sample),
                    1 => (99 - sample, k),
                    2 => (99 - k, 99 - sample),
                    _ => (sample, 99 - k),
                };
                let mut runs: Vec<(bool, u32)> = vec![];
                // Keep more than a half-pen away from corners where the adjacent side overlaps.
                for k in 26..74 {
                    let (x, y) = at(k);
                    let p = px(&out, x, y);
                    assert!(p == BLACK || p == WHITE, "flat end {side} ({x},{y}): {p:?}");
                    let ink = p == BLACK;
                    match runs.last_mut() {
                        Some((v, n)) if *v == ink => *n += 1,
                        _ => runs.push((ink, 1)),
                    }
                }
                assert!(runs.len() >= 3, "{location} side {side}: {runs:?}");
                for &(_, n) in &runs[1..runs.len() - 1] {
                    assert!(n.abs_diff(period) <= 1, "{location} side {side}: {runs:?}");
                }
            }
        }
    }

    /// selection-outline-selection.md Behaviour 1,3–4: canvas fallback and zero-coverage stored RGBA.
    #[test]
    fn canvas_and_untouched_rgba() {
        let src = from_fn(100, 100, |x, y| {
            [x as u8, y as u8, 123, if (x + y) % 2 == 0 { 0 } else { 91 }]
        });
        assert_eq!(
            bb(SELECTION, json!({"location":"outside"}), &src).data,
            src.data
        );
        assert_eq!(selected(&src, json!({"color":"#4080c000"})).data, src.data);
        for location in ["on", "inside", "outside"] {
            let out = selected(&src, json!({"location":location}));
            for y in 0..100 {
                for x in 0..100 {
                    if (25..75).contains(&x) && (25..75).contains(&y)
                        || !(14..=85).contains(&x)
                        || !(14..=85).contains(&y)
                    {
                        assert_eq!(px(&out, x, y), px(&src, x, y), "{location} ({x},{y})");
                    }
                }
            }
        }
        let canvas = bb(SELECTION, json!({}), &solid(100, 100, WHITE));
        for k in 0..5 {
            let want = match k {
                0 | 1 => BLACK,
                2 => [188, 188, 188, 255],
                _ => WHITE,
            };
            for (x, y) in [(k, 50), (99 - k, 50), (50, k), (50, 99 - k)] {
                assert_px(&canvas, x, y, want);
            }
        }
    }

    /// selection-outline-selection.md Behaviour 1,3: an ellipse follows its path, not its bounds.
    #[test]
    fn ellipse_outline() {
        let (mask, polys) = ellipse_selection(60, 60, [30.0, 30.0], [20.0, 12.0]);
        let out = bb_env(
            SELECTION,
            json!({"width":2}),
            &solid(60, 60, WHITE),
            &with_selection(60, 60, &mask, &polys),
        );
        for (x, y) in [(50, 30), (30, 18), (10, 30), (30, 42)] {
            assert!(px(&out, x, y)[0] < 128, "({x},{y})");
        }
        for (x, y) in [(30, 30), (2, 2), (10, 18), (49, 41)] {
            assert_eq!(px(&out, x, y), WHITE, "({x},{y})");
        }
    }

    /// selection-outline-selection.md Expected results 1 / Notes, Behaviour 4: both colour columns.
    #[test]
    fn selection_context_and_colour_alpha() {
        let (mask, polys) = rect_selection(100, 100, 20, 20, 80, 80);
        let mut env = with_selection(100, 100, &mask, &polys);
        let def = find(SELECTION).unwrap();
        let mut values = indirect_defaults(&def, &env);
        for (info, half, partial, white_tint) in [
            (
                GPU_CONTEXT,
                [188, 188, 188, 255],
                [92, 112, 161, 192],
                [187, 187, 187, 255],
            ),
            (
                ALT_GPU_CONTEXT,
                [128, 128, 128, 255],
                [85, 107, 139, 192],
                [127, 127, 127, 255],
            ),
        ] {
            let out = super::selection_with(&solid(100, 100, WHITE), &values, &env, info);
            assert_px(&out, 17, 50, half);
            assert_px(&out, 22, 50, half);
            values.insert("color".into(), json!("#4080c080"));
            let src = solid(100, 100, [128, 64, 32, 128]);
            let out = super::selection_with(&src, &values, &env, info);
            assert_px(&out, 20, 50, partial);
            assert_eq!(px(&out, 50, 50), [128, 64, 32, 128]);
            values.insert("color".into(), json!("#00000080"));
            let out = super::selection_with(&solid(100, 100, WHITE), &values, &env, info);
            assert_px(&out, 20, 50, white_tint);
            let out = super::selection_with(&solid(100, 100, [33, 44, 55, 0]), &values, &env, info);
            assert_px(&out, 20, 50, [0, 0, 0, 128]);
            assert_eq!(px(&out, 50, 50), [33, 44, 55, 0]);
            values.insert("color".into(), json!("#000000"));
        }
        // CPU dispatch clears caller-supplied render_info and never borrows it for its GPU maths.
        env.render_info = Some(ALT_GPU_CONTEXT);
        let out = bb_env(SELECTION, json!({}), &solid(100, 100, WHITE), &env);
        assert_px(&out, 17, 50, [188, 188, 188, 255]);
        assert_eq!(env.render_info, Some(ALT_GPU_CONTEXT));
    }

    /// selection-outline-selection.md Behaviour 1,3: fractional vector coverage, no host mask clip.
    #[test]
    fn fractional_selection_uses_geometry_once() {
        use crate::pixel::{IntRect, mask::Mask};
        let polys = vec![vec![[20.5, 20.5], [79.5, 20.5], [79.5, 79.5], [20.5, 79.5]]];
        let mask = Mask {
            rect: IntRect::new(0, 0, 100, 100),
            data: vec![0; 10000],
        };
        let env = with_selection(100, 100, &mask, &polys);
        for location in ["inside", "outside"] {
            let out = bb_env(
                SELECTION,
                json!({"location":location,"color":"#4080c080"}),
                &solid(100, 100, [33, 44, 55, 0]),
                &env,
            );
            assert_px(&out, 20, 50, [64, 128, 192, 64]);
            assert_eq!(px(&out, 50, 50), [33, 44, 55, 0]);
        }
    }

    /// object-outline-object.md Behaviour 5: straight RGB is constant inside, darkened at canvas edges.
    #[test]
    fn independent_rgb_blur_and_transparent_border() {
        for info in [GPU_CONTEXT, ALT_GPU_CONTEXT] {
            let src = from_fn(41, 41, |x, y| if (x, y) == (20, 20) { RED } else { [0; 4] });
            let out = bb_ctx(
                OBJECT,
                info,
                json!({"width":3,"antiAlias":false,"blur":6,"color":"#4080c080"}),
                &src,
            );
            for (x, y) in [(23, 20), (25, 20), (20, 23), (20, 25)] {
                let p = px(&out, x, y);
                assert!(p[3] > 0);
                assert_px(&out, x, y, [64, 128, 192, p[3]]);
            }
            let edge = from_fn(21, 21, |x, y| if (x, y) == (0, 10) { RED } else { [0; 4] });
            let out = bb_ctx(
                OBJECT,
                info,
                json!({"width":3,"antiAlias":false,"blur":3,"color":"#4080c080"}),
                &edge,
            );
            let p = px(&out, 0, 8);
            assert!(p[3] > 0);
            assert!(
                p[0] > 0 && p[0] < 62 && p[1] < 126 && p[2] < 190,
                "{info:?}: {p:?}"
            );
        }
    }

    /// common.md §2, §3: session preview/Cancel, successful values and Repeat use the registered effects.
    #[test]
    fn session_values_cancel_and_repeat() {
        use crate::effect::session::{EffectSession, repeat_last};
        use crate::testing::fresh_doc;
        for id in [OBJECT, SELECTION] {
            let mut app = fresh_doc(21, 21, None);
            if id == OBJECT {
                app.documents[0]
                    .document
                    .layers
                    .get_at_mut(0)
                    .unwrap()
                    .surface
                    .put_image_data(&dot(255), 0, 0);
            }
            let def = find(id).unwrap();
            let session = EffectSession::begin(&mut app, def.clone()).unwrap();
            let source = session.source().clone();
            let mut values = session.defaults();
            values.insert("width".into(), json!(3));
            values.insert("color".into(), json!("#4080c080"));
            values.insert("antiAlias".into(), json!(false));
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

    /// Both specs Controls / common.md §3–4: metadata consumed by the shared dialog.
    #[test]
    fn registered_dialog_contracts() {
        for (id, title, category, icon, keys) in [
            (
                OBJECT,
                "Outline Object",
                "object",
                "object_tools__outline_object.png",
                vec!["width", "color", "antiAlias", "blur"],
            ),
            (
                SELECTION,
                "Outline Selection",
                "selection",
                "selection_tools__outline_selection.png",
                vec!["width", "color", "location", "corner", "style", "antiAlias"],
            ),
        ] {
            let def = find(id).unwrap();
            assert_eq!(def.category, Category::Effect(category));
            assert_eq!(def.dialog_title, Some(title));
            assert_eq!(def.icon, icon);
            assert!(def.dialog.is_none());
            assert_eq!(
                def.controls.iter().map(Control::key).collect::<Vec<_>>(),
                keys
            );
            if id == OBJECT {
                assert!(matches!(def.kind,EffectKind::Gpu(info,_) if info==GPU_CONTEXT));
            } else {
                assert!(matches!(
                    def.kind,
                    EffectKind::Cpu {
                        clip_to_selection: false,
                        ..
                    }
                ));
            }
            let ui = def.indirect.unwrap();
            let values = indirect_defaults(&def, &EffectEnv::whole(100, 100));
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
            for key in keys {
                let c = ui.control(key);
                assert!(!c.header_line);
                assert!(matches!(c.note, Note::None));
                assert!((ui.enabled)(&values, key));
            }
            assert!(ui.control("color").alpha);
            assert!(!ui.control("color").reset);
            assert!(matches!(
                def.controls[1],
                Control::Color {
                    label: "Color",
                    default: ColorDefault::Rgb(0),
                    ..
                }
            ));
            assert_eq!(values["color"], json!("#000000"));
            assert_eq!(values["antiAlias"], json!(true));
            let label = if id == OBJECT {
                "Width"
            } else {
                "Outline width"
            };
            let default = if id == OBJECT { 10.0 } else { 5.0 };
            assert!(
                matches!(def.controls[0],Control::Range {label:l,min:1.0,max:100.0,default:d,step:1.0,exponential:false,keyboard,..} if l==label && d==default && keyboard.decimals==0 && keyboard.small==1.0 && keyboard.large==5.0 && !keyboard.exponential)
            );
            assert!(matches!(
                def.controls[if id == OBJECT { 2 } else { 5 }],
                Control::Checkbox {
                    label: "Anti-Alias outline",
                    default: true,
                    ..
                }
            ));
            if id == OBJECT {
                assert!(
                    matches!(def.controls[3],Control::Range {label:"Blur radius",min:0.0,max:100.0,default:0.0,step:1.0,keyboard,..} if keyboard.decimals==0 && keyboard.small==1.0 && keyboard.large==5.0 && !keyboard.exponential)
                );
            } else {
                for (key, caption, default, choices) in [
                    (
                        "location",
                        "Location     ",
                        "on",
                        vec![
                            ("on", "On selection"),
                            ("inside", "Inside selection"),
                            ("outside", "Outside selection"),
                        ],
                    ),
                    (
                        "corner",
                        "Corner style",
                        "miter",
                        vec![
                            ("miter", "Miter"),
                            ("bevel", "Bevel"),
                            ("round", "Round"),
                            ("miterOrBevel", "Miter or Bevel"),
                        ],
                    ),
                    (
                        "style",
                        "Line style    ",
                        "solid",
                        vec![("solid", "Solid"), ("dash", "Dash")],
                    ),
                ] {
                    assert_eq!(ui.control(key).caption, caption);
                    assert_eq!(values[key], json!(default));
                    assert!(
                        matches!(def.controls.iter().find(|c| c.key()==key).unwrap(),Control::Select {label:"",default:d,choices:c,..} if *d==default && *c==choices)
                    );
                }
            }
        }
    }
}
