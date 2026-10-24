//! BB spec: effects/render-gradient.md; effects/render-grid-checkerboard.md; common.md §1–4

use super::color_value_alpha;
use super::conventions::{GRADIENT_DEGENERATE_SQUARE_T, GRADIENT_HSV_EXACT_ENDPOINTS};
use super::manifest::manifest;
use super::ui::{ControlUi, Help, HelpWindow, IndirectUi};
use crate::canvas::ImageData;
use crate::effect::{
    ColorDefault, Control, CpuFn, EffectDef, EffectEnv, EffectKind, Values, choice, flag, num,
    range,
};
use crate::pixel::{IntRect, Px};
use crate::util::color::hsv_to_unit_rgb;

const SHAPES: [(&str, &str); 8] = [
    ("vertical", "Vertical Style"),
    ("horizontal", "Horizontal Style"),
    ("diagonal", "Diagonal Style"),
    ("diagonalOther", "Diagnal (other) Style"),
    ("radialCorner", "Radial Corner Style"),
    ("radialSide", "Radial Side Style"),
    ("square", "Square Style"),
    ("conical", "Conical Style"),
];
const INTERPOLATIONS: [(&str, &str); 3] = [
    ("rgb", "RGB type"),
    ("hsvShort", "HSV (short) type"),
    ("hsvLong", "HSV (long) type"),
];
const COLOUR: ControlUi = ControlUi {
    alpha: true,
    reset: false,
    ..ControlUi::DEFAULT
};
static GRADIENT_UI: IndirectUi = IndirectUi {
    controls: &[
        ("from", COLOUR),
        ("to", COLOUR),
        ("alphaOnly", ControlUi::NO_HEADER),
        ("shape", ControlUi::NO_HEADER),
        ("interp", ControlUi::NO_HEADER),
    ],
    enabled: |v, key| key != "interp" || !flag(v, "alphaOnly"),
    help: Help::Window(HelpWindow::Older),
    ..IndirectUi::DEFAULT
};
static GRID_UI: IndirectUi = IndirectUi {
    controls: &[
        ("pattern", ControlUi::NO_HEADER),
        (
            "color",
            ControlUi {
                header_line: false,
                ..COLOUR
            },
        ),
        ("fill", ControlUi::NO_HEADER),
        (
            "custom",
            ControlUi {
                header_line: false,
                ..COLOUR
            },
        ),
        ("swap", ControlUi::NO_HEADER),
    ],
    enabled: |v, key| match key {
        "lineWidth" => choice(v, "pattern") == "grid",
        "custom" => choice(v, "fill") == "custom",
        _ => true,
    },
    help: Help::Text(&["Grid / Checkerboard v5.2"]),
    ..IndirectUi::DEFAULT
};

/// CPU render generators in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let generator = |id, title, controls, render: CpuFn, ui| {
        let m = manifest(id).expect("registered generator");
        EffectDef {
            id,
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
    let select = |key, default, choices| Control::Select {
        key,
        label: "",
        default,
        choices,
    };
    vec![
        generator(
            "bbGradient",
            "Gradient Effect",
            vec![
                Control::Color {
                    key: "from",
                    label: "From Color / Alpha",
                    default: ColorDefault::Primary,
                },
                Control::Color {
                    key: "to",
                    label: "To Color / Alpha",
                    default: ColorDefault::Secondary,
                },
                Control::Checkbox {
                    key: "alphaOnly",
                    label: "Alpha Only",
                    default: false,
                },
                select("shape", "vertical", &SHAPES),
                select("interp", "rgb", &INTERPOLATIONS),
            ],
            gradient,
            &GRADIENT_UI,
        ),
        generator(
            "bbGridCheckerboard",
            "Grid Maker",
            vec![
                select(
                    "pattern",
                    "grid",
                    &[("grid", "Draw a grid"), ("checker", "Draw a checkerboard")],
                ),
                range("size", "Size", 1.0, 1000.0, 33.0),
                range("lineWidth", "Line width", 1.0, 100.0, 1.0),
                Control::Color {
                    key: "color",
                    label: "",
                    default: ColorDefault::Rgb(0),
                },
                select(
                    "fill",
                    "source",
                    &[
                        ("source", "Use source canvas for secondary color"),
                        ("custom", "Use custom secondary color:"),
                    ],
                ),
                Control::Color {
                    key: "custom",
                    label: "",
                    default: ColorDefault::Secondary,
                },
                Control::Checkbox {
                    key: "swap",
                    label: "Swap colors",
                    default: false,
                },
            ],
            grid,
            &GRID_UI,
        ),
    ]
}

/// Shape parameter in integer selection bounds, with no half-pixel offset.
pub fn gradient_t(style: &str, x: i32, y: i32, b: IntRect) -> f64 {
    if b.is_empty() {
        return 0.0;
    }
    let (w, h) = (f64::from(b.w), f64::from(b.h));
    let (dx, dy) = (f64::from(x - b.x), f64::from(y - b.y));
    let (hw, hh) = (f64::from(b.w / 2), f64::from(b.h / 2));
    let (tx, ty) = (dx / w, dy / h);
    match style {
        "horizontal" => tx,
        "diagonal" => (tx + ty) / 2.0,
        "diagonalOther" => 0.5 + (ty - tx) / 2.0,
        "radialCorner" | "radialSide" => {
            let d = (dx - hw).hypot(dy - hh);
            let r = if style == "radialCorner" {
                hw.hypot(hh)
            } else {
                hw.min(hh)
            };
            if d >= r { 1.0 } else { d / r }
        }
        "square" => {
            if hw == 0.0 || hh == 0.0 {
                // spec-question: Q27 degenerate Square
                // approximation: prescribed zero parameter for degenerate Square (render-gradient.md).
                GRADIENT_DEGENERATE_SQUARE_T
            } else {
                // quirk(bb): render-gradient.md Frame of reference: right/bottom are exclusive.
                (dx.min(w - dx) / hw).min(dy.min(h - dy) / hh)
            }
        }
        "conical" => (dy - hh).atan2(dx - hw).abs() / std::f64::consts::PI,
        _ => ty,
    }
}

fn rgb_to_hsv(p: Px) -> [f64; 3] {
    let [r, g, b] = [p[0], p[1], p[2]].map(|c| f64::from(c) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d == 0.0 {
        return [0.0, 0.0, max];
    }
    let h = if max == r {
        (g - b) / d
    } else if max == g {
        2.0 + (b - r) / d
    } else {
        4.0 + (r - g) / d
    };
    [(h * 60.0).rem_euclid(360.0), d / max, max]
}

/// Hexcone HSV interpolation, retaining floating hue/saturation/value until byte truncation.
pub fn hsv_lerp(f: Px, t: Px, w: f64, long: bool) -> [u8; 3] {
    let [fh, fs, fv] = rgb_to_hsv(f);
    let [th, ts, tv] = rgb_to_hsv(t);
    let difference = th - fh;
    let d = difference.abs();
    let travel = if (long && d > 180.0) || (!long && d <= 180.0) {
        difference
    } else if fh > th {
        360.0 - d
    } else {
        // Equal hues follow this branch only for the long, decreasing full turn.
        -(360.0 - d)
    };
    hsv_to_unit_rgb(
        (fh + w * travel).rem_euclid(360.0),
        fs + w * (ts - fs),
        fv + w * (tv - fv),
    )
    .map(|c| (c * 255.0) as u8)
}

fn gradient(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (f, t) = (
        color_value_alpha(v, "from", env),
        color_value_alpha(v, "to", env),
    );
    let shape = choice(v, "shape");
    let interp = choice(v, "interp");
    let radial = matches!(shape, "radialCorner" | "radialSide");
    let alpha_only = flag(v, "alphaOnly");
    let grey = |p: Px| p[0] == p[1] && p[1] == p[2];
    // quirk(bb): render-gradient.md "Grey override": either grey endpoint forces RGB.
    let hsv = !alpha_only && interp != "rgb" && !grey(f) && !grey(t);
    let b = env
        .bounds
        .intersect(&IntRect::new(0, 0, src.width, src.height));
    let mut out = src.clone();
    for y in b.y..b.bottom() {
        for x in b.x..b.right() {
            let w = gradient_t(shape, x, y, env.bounds);
            let lerp = |channel: usize| {
                let (f, t) = (i32::from(f[channel]), i32::from(t[channel]));
                if matches!(shape, "diagonal" | "diagonalOther") {
                    // quirk(bb): render-gradient.md "Diagonal styles, integer quirk".
                    let half = (t - f) / 2;
                    let tx = f64::from(x - env.bounds.x) / f64::from(env.bounds.w);
                    let ty = f64::from(y - env.bounds.y) / f64::from(env.bounds.h);
                    let other = shape == "diagonalOther";
                    let start = f + if other { half } else { 0 };
                    let vertical = (f64::from(start) + ty * f64::from(half)) as i32;
                    (f64::from(vertical) + tx * f64::from(if other { -half } else { half })) as u8
                } else {
                    (f64::from(f) + w * f64::from(t - f)) as u8
                }
            };
            let p =
                &mut out.data.as_chunks_mut::<4>().0[y as usize * src.width as usize + x as usize];
            if !alpha_only {
                // spec-question: Q28 HSV gradient endpoint roundtrip
                let exact_endpoint = (radial && w == 1.0)
                    || (hsv && GRADIENT_HSV_EXACT_ENDPOINTS && (w == 0.0 || w == 1.0));
                let rgb = if exact_endpoint {
                    let endpoint = if w == 0.0 { f } else { t };
                    [endpoint[0], endpoint[1], endpoint[2]]
                } else if hsv {
                    hsv_lerp(f, t, w, interp == "hsvLong")
                } else {
                    [lerp(0), lerp(1), lerp(2)]
                };
                p[..3].copy_from_slice(&rgb);
            }
            p[3] = lerp(3);
        }
    }
    out
}

fn grid(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let size = num(v, "size").clamp(1.0, 1000.0) as u32;
    let width = num(v, "lineWidth").clamp(1.0, 100.0) as u32;
    let pattern = color_value_alpha(v, "color", env);
    let custom = color_value_alpha(v, "custom", env);
    let source = choice(v, "fill") == "source";
    let checker = choice(v, "pattern") == "checker";
    let swap = flag(v, "swap");
    let b = env
        .bounds
        .intersect(&IntRect::new(0, 0, src.width, src.height));
    let mut out = src.clone();
    for y in b.y as u32..b.bottom() as u32 {
        for x in b.x as u32..b.right() as u32 {
            let first = if checker {
                (x / size) % 2 == (y / size) % 2
            } else {
                x % size < width || y % size < width
            };
            let p =
                &mut out.data.as_chunks_mut::<4>().0[y as usize * src.width as usize + x as usize];
            // quirk(bb): render-grid-checkerboard.md Behaviour: replacement, including hidden RGB/alpha.
            if first != swap {
                *p = pattern;
            } else if !source {
                *p = custom;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::ui::{Help, HelpWindow, Note};
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::{ColorDefault, Control, EffectEnv, EffectKind, RenderInfo, find, with};
    use serde_json::{Value, json};

    const BK: &str = "#000000ff";
    const WH: &str = "#ffffffff";
    fn gradient(v: Value, src: &ImageData) -> ImageData {
        bb("bbGradient", v, src)
    }
    fn grid(v: Value, src: &ImageData) -> ImageData {
        bb("bbGridCheckerboard", v, src)
    }
    fn g(shape: &str, w: u32, h: u32) -> ImageData {
        gradient(
            json!({"shape": shape, "from": BK, "to": WH}),
            &solid(w, h, [9, 9, 9, 255]),
        )
    }
    fn greys<const N: usize>(img: &ImageData, points: [(u32, u32); N]) -> [u8; N] {
        points.map(|(x, y)| {
            let p = px(img, x, y);
            assert_eq!(p, [p[0], p[0], p[0], 255]);
            p[0]
        })
    }

    /// render-gradient.md Expected results 1: the last column never reaches To.
    #[test]
    fn horizontal_rgb() {
        let out = g("horizontal", 256, 4);
        assert_eq!(
            greys(&out, [(0, 0), (1, 0), (128, 0), (255, 0)]),
            [0, 0, 127, 254]
        );
        assert!((0..256).all(|x| (1..4).all(|y| px(&out, x, y) == px(&out, x, 0))));
    }

    /// render-gradient.md Expected results 2: all three complete RGB row tables.
    #[test]
    fn red_to_blue_types() {
        for (interp, want) in [
            (
                "rgb",
                [
                    [255, 0, 0],
                    [191, 0, 63],
                    [127, 0, 127],
                    [63, 0, 191],
                    [2, 0, 252],
                ],
            ),
            (
                "hsvShort",
                [
                    [255, 0, 0],
                    [255, 0, 127],
                    [255, 0, 255],
                    [127, 0, 255],
                    [5, 0, 255],
                ],
            ),
            (
                "hsvLong",
                [
                    [255, 0, 0],
                    [255, 255, 0],
                    [0, 255, 0],
                    [0, 255, 255],
                    [0, 10, 255],
                ],
            ),
        ] {
            let out = gradient(
                json!({"from":"#ff0000ff","to":"#0000ffff","interp":interp}),
                &solid(4, 100, [0; 4]),
            );
            for (y, rgb) in [0, 25, 50, 75, 99].into_iter().zip(want) {
                for x in 0..4 {
                    assert_eq!(px(&out, x, y), [rgb[0], rgb[1], rgb[2], 255]);
                }
            }
        }
    }

    /// render-gradient.md Expected results 3: equal hues make a full decreasing long turn.
    #[test]
    fn equal_hues() {
        for (interp, want) in [
            (
                "hsvShort",
                [[255, 0, 0], [223, 0, 0], [191, 0, 0], [159, 0, 0]],
            ),
            (
                "hsvLong",
                [[255, 0, 0], [111, 0, 223], [0, 191, 191], [79, 159, 0]],
            ),
        ] {
            let out = gradient(
                json!({"from":"#ff0000ff","to":"#800000ff","interp":interp}),
                &solid(4, 100, [0; 4]),
            );
            assert_eq!(
                [0, 25, 50, 75].map(|y| {
                    let p = px(&out, 0, y);
                    [p[0], p[1], p[2]]
                }),
                want
            );
        }
    }

    /// render-gradient.md Expected results 4: either grey endpoint forces RGB in both HSV types.
    #[test]
    fn grey_override() {
        for (from, to) in [
            ("#808080ff", "#ff0000ff"),
            ("#ff0000ff", "#808080ff"),
            (BK, "#ff0000ff"),
            ("#ff0000ff", WH),
        ] {
            let run = |interp| {
                gradient(
                    json!({"from":from,"to":to,"interp":interp}),
                    &solid(4, 100, [0; 4]),
                )
            };
            let rgb = run("rgb").data;
            for interp in ["hsvShort", "hsvLong"] {
                assert_eq!(run(interp).data, rgb);
            }
        }
    }

    /// render-gradient.md Expected results 5: alpha replacement and disabled interpolation.
    #[test]
    fn alpha_only() {
        let src = solid(256, 1, [255, 0, 0, 255]);
        let out = gradient(
            json!({"shape":"horizontal","alphaOnly":true,"from":BK,"to":"#ffffff00"}),
            &src,
        );
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [255, 0, 0])
        );
        assert_eq!([0, 128, 255].map(|x| px(&out, x, 0)[3]), [255, 127, 0]);
        let def = find("bbGradient").unwrap();
        let ui = def.indirect.unwrap();
        for alpha_only in [false, true] {
            let v = with(&def, &json!({"alphaOnly":alpha_only}));
            assert_eq!((ui.enabled)(&v, "interp"), !alpha_only);
            for key in ["from", "to", "alphaOnly", "shape"] {
                assert!((ui.enabled)(&v, key));
            }
        }
    }

    /// render-gradient.md Expected results 6: at and beyond radius writes exact To.
    #[test]
    fn radial_side() {
        assert_eq!(
            greys(
                &g("radialSide", 101, 101),
                [(50, 50), (75, 50), (99, 50), (100, 50), (0, 0)]
            ),
            [0, 127, 249, 255, 255]
        );
    }

    /// render-gradient.md Expected results 7: Square's asymmetric last column.
    #[test]
    fn square() {
        assert_eq!(
            greys(
                &g("square", 100, 100),
                [(0, 50), (25, 50), (50, 50), (99, 50)]
            ),
            [0, 127, 255, 5]
        );
    }

    /// render-gradient.md Expected results 8: conical quadrants and horizontal mirror symmetry.
    #[test]
    fn conical() {
        let out = g("conical", 101, 101);
        assert_eq!(
            greys(&out, [(100, 50), (0, 50), (50, 0), (50, 100), (100, 0)]),
            [0, 255, 127, 127, 63]
        );
        assert_eq!(px(&out, 50, 50), [0, 0, 0, 255]);
        for y in 0..101 {
            for x in 0..101 {
                assert_eq!(px(&out, x, y), px(&out, x, 100 - y));
            }
        }
    }

    /// render-gradient.md Expected results 9: both diagonal integer-half tables.
    #[test]
    fn diagonal_integer_halves() {
        assert_eq!(
            greys(&g("diagonal", 100, 100), [(0, 0), (50, 50), (99, 99)]),
            [0, 126, 250]
        );
        assert_eq!(
            greys(
                &g("diagonalOther", 100, 100),
                [(0, 0), (99, 0), (0, 99), (99, 99)]
            ),
            [127, 1, 252, 126]
        );
    }

    /// render-gradient.md Expected results 10: selection-relative ramp, all outside pixels unchanged.
    #[test]
    fn gradient_selection_bounds() {
        let (m, p) = rect_selection(200, 200, 100, 0, 200, 200);
        let src = from_fn(200, 200, |x, y| [x as u8, y as u8, 9, 181]);
        let out = bb_env(
            "bbGradient",
            json!({"shape":"horizontal","from":BK,"to":WH}),
            &src,
            &with_selection(200, 200, &m, &p),
        );
        assert_eq!(greys(&out, [(100, 7), (199, 7)]), [0, 252]);
        for y in 0..200 {
            for x in 0..100 {
                assert_eq!(px(&out, x, y), px(&src, x, y));
            }
        }
    }

    /// render-grid-checkerboard.md Expected results 1: every line and all 36 transparent gaps.
    #[test]
    fn default_grid() {
        let out = grid(json!({"size":4}), &solid(9, 9, [0; 4]));
        let mut lines = 0;
        for y in 0..9 {
            for x in 0..9 {
                let line = [0, 4, 8].contains(&x) || [0, 4, 8].contains(&y);
                assert_eq!(px(&out, x, y), if line { [0, 0, 0, 255] } else { [0; 4] });
                lines += usize::from(line);
            }
        }
        assert_eq!(lines, 45);
    }

    /// render-grid-checkerboard.md Expected results 2: both thick-line columns/rows and every gap.
    #[test]
    fn thick_grid_custom_fill() {
        let out = grid(
            json!({"size":4,"lineWidth":2,"fill":"custom","custom":WH}),
            &solid(8, 8, [1, 2, 3, 4]),
        );
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    px(&out, x, y),
                    if [0, 1, 4, 5].contains(&x) || [0, 1, 4, 5].contains(&y) {
                        [0, 0, 0, 255]
                    } else {
                        [255; 4]
                    }
                );
            }
        }
    }

    /// render-grid-checkerboard.md Expected results 3: all four 2x2 blocks, ignores Line width.
    #[test]
    fn checkerboard() {
        for width in [1, 100] {
            let out = grid(
                json!({"pattern":"checker","size":2,"lineWidth":width,"fill":"custom","custom":WH}),
                &solid(4, 4, [0; 4]),
            );
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(
                        px(&out, x, y),
                        if (x < 2) == (y < 2) {
                            [0, 0, 0, 255]
                        } else {
                            [255; 4]
                        }
                    );
                }
            }
        }
    }

    /// render-grid-checkerboard.md Expected results 4: swaps every pixel of all four blocks.
    #[test]
    fn swapped_checkerboard() {
        let out = grid(
            json!({"pattern":"checker","size":2,"fill":"custom","custom":WH,"swap":true}),
            &solid(4, 4, [0; 4]),
        );
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    px(&out, x, y),
                    if (x < 2) == (y < 2) {
                        [255; 4]
                    } else {
                        [0, 0, 0, 255]
                    }
                );
            }
        }
    }

    /// render-grid-checkerboard.md Expected results 5: absolute pattern origin and complete outside preservation.
    #[test]
    fn grid_canvas_anchor() {
        let (m, p) = rect_selection(20, 20, 5, 5, 15, 15);
        let out = bb_env(
            "bbGridCheckerboard",
            json!({"size":10}),
            &solid(20, 20, [255; 4]),
            &with_selection(20, 20, &m, &p),
        );
        for y in 0..20 {
            for x in 0..20 {
                let line = (5..15).contains(&x) && (5..15).contains(&y) && (x == 10 || y == 10);
                assert_eq!(px(&out, x, y), if line { [0, 0, 0, 255] } else { [255; 4] });
            }
        }
    }

    /// render-grid-checkerboard.md Expected results 6: semitransparent line pixels replace white.
    #[test]
    fn no_compositing() {
        let out = grid(
            json!({"color":"#ff000080","size":2}),
            &solid(4, 4, [255; 4]),
        );
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    px(&out, x, y),
                    if x % 2 == 0 || y % 2 == 0 {
                        [255, 0, 0, 128]
                    } else {
                        [255; 4]
                    }
                );
            }
        }
    }

    /// render-grid-checkerboard.md Expected results 7: at and above Size, every selected pixel is a line.
    #[test]
    fn line_width_at_least_size() {
        for width in [3, 4, 100] {
            let out = grid(json!({"size":3,"lineWidth":width}), &solid(6, 6, [255; 4]));
            assert!(
                out.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == [0, 0, 0, 255])
            );
        }
    }

    /// render-gradient.md Colour interpolation: descending channels truncate after each diagonal half.
    #[test]
    fn descending_diagonal_channels_and_alpha() {
        for (shape, points, want) in [
            (
                "diagonal",
                [(0, 0), (50, 50), (99, 99), (0, 99)],
                [
                    [255, 200, 100, 255],
                    [127, 100, 176, 127],
                    [3, 2, 252, 3],
                    [129, 101, 176, 129],
                ],
            ),
            (
                "diagonalOther",
                [(0, 0), (99, 0), (0, 99), (99, 99)],
                [
                    [128, 101, 177, 128],
                    [253, 199, 100, 253],
                    [2, 2, 253, 2],
                    [127, 100, 176, 127],
                ],
            ),
        ] {
            let v = json!({"shape":shape,"from":"#ffc864ff","to":"#0001ff00"});
            let out = gradient(v.clone(), &solid(100, 100, [11, 22, 33, 44]));
            assert_eq!(points.map(|(x, y)| px(&out, x, y)), want);
            let mut alpha = v;
            alpha["alphaOnly"] = json!(true);
            let a = gradient(alpha, &solid(100, 100, [11, 22, 33, 44]));
            assert_eq!(
                points.map(|(x, y)| px(&a, x, y)),
                want.map(|p| [11, 22, 33, p[3]])
            );
        }
    }

    /// render-gradient.md HSV types: both travel directions, crossing 0, complementary ties and f64 HSV.
    #[test]
    fn hsv_directions_and_precision() {
        for (from, to, short, long) in [
            ("#ff0000ff", "#00ff00ff", [255, 255, 0], [0, 0, 255]),
            ("#00ff00ff", "#ff0000ff", [255, 255, 0], [0, 0, 255]),
            ("#0000ffff", "#ff0000ff", [255, 0, 255], [0, 255, 0]),
            ("#ff0000ff", "#00ffffff", [127, 255, 0], [127, 0, 255]),
            ("#00ffffff", "#ff0000ff", [127, 255, 0], [127, 0, 255]),
            ("#010203ff", "#020406ff", [1, 3, 4], [4, 3, 1]),
        ] {
            for (interp, want) in [("hsvShort", short), ("hsvLong", long)] {
                let out = gradient(
                    json!({"shape":"horizontal","from":from,"to":to,"interp":interp}),
                    &solid(2, 1, [0; 4]),
                );
                assert_eq!(
                    px(&out, 1, 0),
                    [want[0], want[1], want[2], 255],
                    "{from} {to} {interp}"
                );
            }
        }
    }

    /// render-gradient.md HSV diagonals: colour uses real t, alpha retains integer halves.
    #[test]
    fn hsv_diagonal_alpha() {
        for shape in ["diagonal", "diagonalOther"] {
            let out = gradient(
                json!({"shape":shape,"interp":"hsvShort","from":"#ff0000ff","to":"#0000ff00"}),
                &solid(100, 100, [0; 4]),
            );
            assert_eq!(px(&out, 50, 50), [255, 0, 255, 127]);
        }
    }

    /// render-gradient.md Frame of reference: radial corner and translated odd bounds, exact To with hidden RGB.
    #[test]
    fn radial_corner_and_degenerate_shapes() {
        let out = g("radialCorner", 7, 9);
        assert_eq!(greys(&out, [(3, 4), (3, 0), (0, 0)]), [0, 204, 255]);
        for shape in ["radialCorner", "radialSide"] {
            let out = gradient(
                json!({"shape":shape,"from":"#ff0000ff","to":"#12345600"}),
                &solid(1, 1, [9; 4]),
            );
            assert_eq!(px(&out, 0, 0), [18, 52, 86, 0]);
        }
        for (w, h) in [(1, 1), (1, 7), (7, 1)] {
            let out = g("square", w, h);
            assert!(
                out.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == [0, 0, 0, 255])
            );
        }
        let (m, p) = rect_selection(12, 14, 4, 3, 11, 12);
        let out = bb_env(
            "bbGradient",
            json!({"shape":"radialCorner","from":BK,"to":WH}),
            &solid(12, 14, [9; 4]),
            &with_selection(12, 14, &m, &p),
        );
        assert_eq!(greys(&out, [(7, 7), (7, 3), (4, 3)]), [0, 204, 255]);
        assert_eq!(px(&out, 3, 7), [9; 4]);
    }

    /// common.md §1; render specs defaults: palette alpha, fixed grid black and CPU context independence.
    #[test]
    fn palette_alpha_and_cpu_context() {
        let src = from_fn(4, 4, |x, y| {
            [x as u8 + 11, y as u8 + 22, 33, if x == 1 { 0 } else { 77 }]
        });
        let mut env = EffectEnv::whole(4, 4);
        env.primary = [200, 100, 50, 128];
        env.secondary = [40, 20, 10, 0];
        let grad = bb_env("bbGradient", json!({}), &src, &env);
        assert_eq!(px(&grad, 0, 0), env.primary);
        assert_eq!(px(&grad, 0, 2), [120, 60, 30, 64]);
        let base_grid = bb_env("bbGridCheckerboard", json!({"size":2}), &src, &env);
        assert_eq!(px(&base_grid, 0, 0), [0, 0, 0, 255]);
        assert_eq!(px(&base_grid, 1, 1), px(&src, 1, 1));
        let custom = bb_env(
            "bbGridCheckerboard",
            json!({"size":2,"fill":"custom"}),
            &src,
            &env,
        );
        assert_eq!(px(&custom, 1, 1), env.secondary);
        let swap = bb_env(
            "bbGridCheckerboard",
            json!({"size":2,"swap":true}),
            &src,
            &env,
        );
        assert_eq!(px(&swap, 0, 0), px(&src, 0, 0));
        assert_eq!(px(&swap, 1, 1), [0, 0, 0, 255]);
        let alpha = bb_env(
            "bbGradient",
            json!({"alphaOnly":true,"interp":"hsvLong"}),
            &src,
            &env,
        );
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(px(&alpha, x, y)[..3], px(&src, x, y)[..3]);
            }
        }
        for space in [Space::Srgb, Space::Linear] {
            for mode in [Alpha::Straight, Alpha::Premultiplied] {
                env.render_info = Some(RenderInfo::new(space, mode));
                assert_eq!(bb_env("bbGradient", json!({}), &src, &env).data, grad.data);
                assert_eq!(
                    bb_env("bbGridCheckerboard", json!({"size":2}), &src, &env).data,
                    base_grid.data
                );
                assert_eq!(env.render_info, Some(RenderInfo::new(space, mode)));
            }
        }
    }

    /// render-gradient.md Colour interpolation, R110: literal f64 HSV truncation outside the explicit radial-To bypass.
    /// This pins the prescribed arithmetic choice, not a native-DLL observation.
    #[test]
    fn hsv_endpoints_keep_literal_arithmetic_except_radial_to() {
        let src = solid(3, 3, [9; 4]);
        let from = gradient(
            json!({"from":"#12345680","to":"#ff000040","interp":"hsvShort"}),
            &src,
        );
        assert_eq!(px(&from, 0, 0), [17, 52, 86, 128]);
        for (shape, x, y) in [("square", 1, 1), ("conical", 0, 1)] {
            let out = gradient(
                json!({"shape":shape,"from":"#ff000080","to":"#12345600","interp":"hsvShort"}),
                &src,
            );
            assert_eq!(px(&out, x, y), [17, 52, 86, 0], "{shape}");
        }
        for shape in ["radialCorner", "radialSide"] {
            let out = gradient(
                json!({"shape":shape,"from":"#ff000080","to":"#12345600","interp":"hsvShort"}),
                &src,
            );
            assert_eq!(px(&out, 0, 0), [18, 52, 86, 0], "{shape}");
            let center = gradient(
                json!({"shape":shape,"from":"#12345680","to":"#ff000040","interp":"hsvShort"}),
                &src,
            );
            assert_eq!(px(&center, 1, 1), [17, 52, 86, 128], "{shape}");
        }
    }

    /// Both render specs Controls/common.md §3–4: actual definitions consumed by the generic dialog.
    #[test]
    fn dialog_contracts() {
        for (id, title, keys) in [
            (
                "bbGradient",
                "Gradient Effect",
                vec!["from", "to", "alphaOnly", "shape", "interp"],
            ),
            (
                "bbGridCheckerboard",
                "Grid Maker",
                vec![
                    "pattern",
                    "size",
                    "lineWidth",
                    "color",
                    "fill",
                    "custom",
                    "swap",
                ],
            ),
        ] {
            let d = find(id).unwrap();
            let ui = d.indirect.unwrap();
            assert_eq!(d.dialog_title, Some(title));
            assert!(d.dialog.is_none());
            assert!(matches!(
                d.kind,
                EffectKind::Cpu {
                    clip_to_selection: true,
                    ..
                }
            ));
            assert_eq!(
                d.controls.iter().map(Control::key).collect::<Vec<_>>(),
                keys
            );
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            for c in &d.controls {
                let u = ui.control(c.key());
                assert_eq!(
                    u.header_line,
                    matches!(c.key(), "from" | "to" | "size" | "lineWidth")
                );
                assert!(
                    u.track.is_none()
                        && u.caption.is_empty()
                        && !u.wrap
                        && matches!(u.note, Note::None)
                );
            }
        }
        let grad = find("bbGradient").unwrap();
        let ui = grad.indirect.unwrap();
        assert!(matches!(ui.help, Help::Window(HelpWindow::Older)));
        for (i, key, label, default) in [
            (0, "from", "From Color / Alpha", ColorDefault::Primary),
            (1, "to", "To Color / Alpha", ColorDefault::Secondary),
        ] {
            assert!(
                matches!(grad.controls[i],Control::Color{key:k,label:l,default:d} if (k,l,d)==(key,label,default))
            );
            assert!(ui.control(key).alpha && !ui.control(key).reset);
        }
        assert!(matches!(
            grad.controls[2],
            Control::Checkbox {
                label: "Alpha Only",
                default: false,
                ..
            }
        ));
        let choices = |d: &crate::effect::EffectDef, key| match d
            .controls
            .iter()
            .find(|c| c.key() == key)
            .unwrap()
        {
            Control::Select {
                label,
                default,
                choices,
                ..
            } => {
                assert_eq!(*label, "");
                (*default, *choices)
            }
            _ => panic!("select"),
        };
        assert_eq!(
            choices(&grad, "shape"),
            (
                "vertical",
                &[
                    ("vertical", "Vertical Style"),
                    ("horizontal", "Horizontal Style"),
                    ("diagonal", "Diagonal Style"),
                    ("diagonalOther", "Diagnal (other) Style"),
                    ("radialCorner", "Radial Corner Style"),
                    ("radialSide", "Radial Side Style"),
                    ("square", "Square Style"),
                    ("conical", "Conical Style")
                ][..]
            )
        );
        assert_eq!(
            choices(&grad, "interp"),
            (
                "rgb",
                &[
                    ("rgb", "RGB type"),
                    ("hsvShort", "HSV (short) type"),
                    ("hsvLong", "HSV (long) type")
                ][..]
            )
        );
        let d = find("bbGridCheckerboard").unwrap();
        let ui = d.indirect.unwrap();
        assert!(matches!(ui.help, Help::Text(["Grid / Checkerboard v5.2"])));
        assert_eq!(
            choices(&d, "pattern"),
            (
                "grid",
                &[("grid", "Draw a grid"), ("checker", "Draw a checkerboard")][..]
            )
        );
        assert_eq!(
            choices(&d, "fill"),
            (
                "source",
                &[
                    ("source", "Use source canvas for secondary color"),
                    ("custom", "Use custom secondary color:")
                ][..]
            )
        );
        for (i, key, default) in [
            (3, "color", ColorDefault::Rgb(0)),
            (5, "custom", ColorDefault::Secondary),
        ] {
            assert!(
                matches!(d.controls[i],Control::Color{key:k,label:"",default:c} if (k,c)==(key,default))
            );
            assert!(ui.control(key).alpha && !ui.control(key).reset);
        }
        assert!(matches!(
            d.controls[6],
            Control::Checkbox {
                label: "Swap colors",
                default: false,
                ..
            }
        ));
        for (i, label, hi, initial) in [(1, "Size", 1000.0, 33.0), (2, "Line width", 100.0, 1.0)] {
            assert!(
                matches!(d.controls[i],Control::Range{label:l,min:1.0,max,default,step:1.0,exponential:false,keyboard,..} if l==label&&max==hi&&default==initial&&keyboard.decimals==0&&keyboard.small==1.0&&keyboard.large==5.0&&!keyboard.exponential&&keyboard.stepped(initial,1.0,hi,true,false)==initial+1.0&&keyboard.stepped(initial,1.0,hi,true,true)==initial+5.0)
            );
        }
        for pattern in ["grid", "checker"] {
            for fill in ["source", "custom"] {
                let v = with(&d, &json!({"pattern":pattern,"fill":fill}));
                for key in [
                    "pattern",
                    "size",
                    "lineWidth",
                    "color",
                    "fill",
                    "custom",
                    "swap",
                ] {
                    assert_eq!(
                        (ui.enabled)(&v, key),
                        match key {
                            "lineWidth" => pattern == "grid",
                            "custom" => fill == "custom",
                            _ => true,
                        }
                    );
                }
            }
        }
    }
}
