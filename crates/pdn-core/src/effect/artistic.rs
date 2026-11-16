//! The 3 Artistic effects on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` artistic entries: ids,
//! icons and control keys only).

use super::graph::{
    BORDER_HARD, CLSID_D2D1ConvolveMatrix, CLSID_D2D1Grayscale, CLSID_D2D1Invert,
    D2D1_BLEND_MODE_COLOR_DODGE, D2D1_BLEND_MODE_DARKEN, Graph, Prop, QUALITY,
};
use super::sample::circle_rows;
use super::surface::{Alpha, EffectSurface, Space, luma, unpremultiply};
use super::{
    Category, Control, EffectDef, EffectEnv, EffectKind, GpuFn, RenderInfo, Values, choice, num,
    range,
};
use crate::render::display::for_bands;

const SRGB_PM: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
const LINEAR: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Straight);

fn artistic(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    render: GpuFn,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("artistic"),
        controls,
        kind: EffectKind::Gpu(info, render),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Artistic submenu (§2.0 rows): paintdotjs ids, icons and labels; Paint.NET ranges (R19).
pub fn effects() -> Vec<EffectDef> {
    vec![
        artistic(
            "inkSketchEffect",
            "ink_sketch_effect_icon.png",
            vec![
                range("ink", "Ink outline", 0.0, 99.0, 50.0),
                range("color", "Coloring", 0.0, 100.0, 50.0),
            ],
            SRGB_PM,
            ink_sketch,
        ),
        artistic(
            "oilPaintingEffect",
            "oil_painting_effect.png",
            vec![
                range("brush", "Brush size", 1.0, 50.0, 10.0),
                range("granularity", "Coarseness", 0.01, 1.0, 0.2).step(0.01),
                Control::Select {
                    key: "kernel",
                    label: "Kernel shape",
                    default: "circle",
                    choices: &[("circle", "Circle"), ("square", "Square")],
                },
            ],
            LINEAR,
            oil_painting,
        ),
        artistic(
            "pencilSketchEffect",
            "pencil_sketch_effect_icon.png",
            vec![
                range("tip", "Pencil tip size", 1.0, 20.0, 2.0),
                range("range", "Color range", -20.0, 20.0, 0.0),
            ],
            SRGB_PM,
            pencil_sketch,
        ),
    ]
}

/// §2.2 Ink Sketch step 1's matrix, row-major top to bottom.
const INK_K: [[f32; 5]; 5] = [
    [-1.0, -1.0, -5.0, -1.0, -1.0],
    [-1.0; 5],
    [-1.0, -1.0, 30.0, -1.0, -1.0],
    [-1.0; 5],
    [-1.0; 5],
];

/// The weight of the tap at (x + dx, y + dy): Direct2D flips the matrix (SVG feConvolveMatrix), so
/// it is K[2 − dy][2 − dx] and the −5 lands on the pixel 2 rows below. 0 outside the 5×5.
pub fn ink_correlation_weight(dx: i32, dy: i32) -> f32 {
    if dx.abs() > 2 || dy.abs() > 2 {
        return 0.0;
    }
    INK_K[(2 - dy) as usize][(2 - dx) as usize]
}

/// Genuine `PdnInkSketchEffect` (`InkSketchGpuEffect`, WorkingSpace = sRGB, premultiplied), its
/// graph on d2d: the ink mask `ConvolveMatrix{5×5 INK_K, PreserveAlpha, Hard, ClampOutput} →
/// Grayscale → AdjustShader` darkens a `PdnGlowEffect` (glow 6) of the source.
fn ink_sketch(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let t = ((num(v, "ink") / 100.0) as f32).clamp(0.0, 0.99);
    let k = ((num(v, "color") / 100.0) as f32).clamp(0.0, 1.0);
    let (mut g, src) = Graph::new(src, Alpha::Premultiplied);
    let kernel: Vec<f32> = INK_K.iter().flatten().copied().collect();
    let conv = [
        (2, Prop::U32(5)),
        (3, Prop::U32(5)),
        (4, Prop::Floats(kernel.into())),
        (8, Prop::Bool(true)),
        (9, Prop::U32(BORDER_HARD)),
        (10, Prop::Bool(true)),
    ];
    let edges = g.fx(CLSID_D2D1ConvolveMatrix, &conv, [src.clone()]);
    let edges = g.fx(CLSID_D2D1Grayscale, &[], [edges]);
    // AdjustShader on the premultiplied grey: white where Y = r/a passes the ink threshold, black
    // elsewhere; alpha kept.
    let mask = g.shader(&edges, |[r, _, _, a]| {
        if a <= 0.0 {
            [0.0; 4]
        } else if r / a > t {
            [a; 4]
        } else {
            [0.0, 0.0, 0.0, a]
        }
    });
    let bc = (0.0 - (k - 0.5)) * 2.0;
    let glow = super::photo::glow_graph(&mut g, src, 6.0, bc, bc);
    let out = g.blend(D2D1_BLEND_MODE_DARKEN, mask, glow);
    g.finish(&out, Alpha::Premultiplied)
}

/// §2.2 Oil Painting step 1: clamp(trunc(256·g), 4, 256).
pub fn oil_bins(granularity: f64) -> u32 {
    ((256.0 * granularity) as i64).clamp(4, 256) as u32
}

/// §2.2 Oil Painting on linear straight colour with Clamp edges: per pixel, the lowest bin of
/// trunc(sat(Y)·(bins − 1)) with the most taps, and the premultiplied mean of its taps. Each row
/// slides a histogram of tap counts and Σ(rgb·a, a) in 2⁻⁴⁸ fixed point: every f32 term of 8-bit
/// input is an exact multiple and 10201 taps stay below 2⁶², so taps leave a bin exactly as they
/// entered and an emptied bin returns to 0 (the direct f32 sum agrees within §5's ±1).
fn oil_painting(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    const ONE: f64 = (1u64 << 48) as f64;
    let r = num(v, "brush").clamp(1.0, 50.0).ceil() as i32;
    let bins = oil_bins(num(v, "granularity")) as usize;
    let rows = if choice(v, "kernel") == "square" {
        (-r..=r).map(|dy| (dy, -r, (2 * r + 1) as u32)).collect()
    } else {
        circle_rows(r as u32)
    };
    let scale = (bins - 1) as f32;
    let binned: Vec<(usize, [i64; 4])> = src
        .px
        .iter()
        .map(|&[r, g, b, a]| {
            let bin = (luma(r, g, b).clamp(0.0, 1.0) * scale) as usize;
            (
                bin,
                [r * a, g * a, b * a, a].map(|c| (c as f64 * ONE) as i64),
            )
        })
        .collect();
    let (w, h) = (src.w as i32, src.h as i32);
    let mut out = EffectSurface::new(src.w, src.h);
    for_bands(&mut out.px, w as usize, 0..h as usize, 1, |y, row| {
        let put = |hist: &mut [(i64, [i64; 4]); 256], x: i32, dy: i32, sign: i64| {
            let i = (y as i32 + dy).clamp(0, h - 1) * w + x.clamp(0, w - 1);
            let (bin, q) = binned[i as usize];
            let (n, sum) = &mut hist[bin];
            *n += sign;
            for (s, q) in sum.iter_mut().zip(q) {
                *s += sign * q;
            }
        };
        let mut hist = [(0, [0; 4]); 256];
        for &(dy, x0, len) in &rows {
            for x in x0..x0 + len as i32 {
                put(&mut hist, x, dy, 1);
            }
        }
        for (x, o) in (0..).zip(row.iter_mut()) {
            let mut mode = 0;
            for i in 1..bins {
                if hist[i].0 > hist[mode].0 {
                    mode = i;
                }
            }
            let (n, sum) = hist[mode];
            *o = unpremultiply(sum.map(|s| (s as f64 / ONE / n.max(1) as f64) as f32));
            for &(dy, x0, len) in &rows {
                put(&mut hist, x + x0, dy, -1);
                put(&mut hist, x + x0 + len as i32, dy, 1);
            }
        }
    });
    out
}

/// Genuine `PdnPencilSketchEffect` (`PencilSketchGpuEffect`, WorkingSpace = sRGB, premultiplied), its
/// graph on d2d: `Grayscale(src)` ColorDodged by `GaussianBlur{σ = tip·1.5/3, Quality, Hard} →
/// PdnBrightnessContrast(range + 0.01, −(range + 0.01)) → Invert → Grayscale`.
fn pencil_sketch(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let tip = (num(v, "tip") as f32).clamp(1.0, 20.0);
    let n = ((num(v, "range") / 100.0) as f32).clamp(-0.2, 0.2) + 0.01;
    let (mut g, src) = Graph::new(src, Alpha::Premultiplied);
    let base = g.fx(CLSID_D2D1Grayscale, &[], [src.clone()]);
    let top = g.gaussian_blur(src, tip * 1.5 / 3.0, QUALITY, BORDER_HARD);
    let top = g.brightness_contrast(&top, n, -n);
    let top = g.fx(CLSID_D2D1Invert, &[], [top]);
    let top = g.fx(CLSID_D2D1Grayscale, &[], [top]);
    let out = g.blend(D2D1_BLEND_MODE_COLOR_DODGE, base, top);
    g.finish(&out, Alpha::Premultiplied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::sample::circle_offsets;
    use crate::effect::surface::Edge;
    use crate::effect::test_util::{from_fn, px, run, solid};
    use serde_json::json;

    /// §5 Task 39: a dark pixel 2 px above contributes −1, 2 px below −5 (Direct2D flips the matrix).
    #[test]
    fn ink_kernel_orientation() {
        assert_eq!(
            (
                ink_correlation_weight(0, -2),
                ink_correlation_weight(0, 2),
                ink_correlation_weight(0, 0)
            ),
            (-1.0, -5.0, 30.0)
        );
        let sum: f32 = (-2..=2)
            .flat_map(|dy| (-2..=2).map(move |dx| ink_correlation_weight(dx, dy)))
            .sum();
        assert_eq!(sum, 2.0);
    }

    /// Flat grey through the §2.2 graph: 128 → white mask, glow = Screen(v, v) = 191.75 → 192 (±1, B-E6); 40 → black mask → 0.
    #[test]
    fn ink_sketch_flat() {
        assert!(
            px(
                &run(
                    "inkSketchEffect",
                    json!({}),
                    &solid(9, 9, [128, 128, 128, 255])
                ),
                4,
                4
            )[0]
            .abs_diff(192)
                <= 1
        );
        assert_eq!(
            px(
                &run(
                    "inkSketchEffect",
                    json!({}),
                    &solid(9, 9, [40, 40, 40, 255])
                ),
                4,
                4
            ),
            [0, 0, 0, 255]
        );
    }

    /// Oil Painting: bins = clamp(trunc(256·g), 4, 256); ties take the lowest bin.
    #[test]
    fn oil_painting() {
        assert_eq!(
            (oil_bins(0.2), oil_bins(0.001), oil_bins(1.0)),
            (51, 4, 256)
        );
        let img = from_fn(3, 3, |x, y| match (x, y) {
            (0, _) | (1, 0) => [0, 0, 0, 255],
            (1, 1) => [128, 128, 128, 255],
            _ => [255, 255, 255, 255],
        });
        assert_eq!(
            px(&run("oilPaintingEffect", json!({"brush": 1}), &img), 1, 1),
            [0, 0, 0, 255]
        );
        let flat = run(
            "oilPaintingEffect",
            json!({}),
            &solid(7, 7, [90, 140, 30, 255]),
        );
        assert!(
            px(&flat, 3, 3)
                .iter()
                .zip([90u8, 140, 30, 255])
                .all(|(a, b)| a.abs_diff(b) <= 1)
        );
    }

    /// Genuine pencil graph on flat grey (range 0 → Brightness 0.01, Contrast −0.01; num_b 1,
    /// num_c −1), in the float graph: 128 → 253, 64 → 249 (8bpc nodes gave 247), black → 0.
    #[test]
    fn pencil_sketch_flat() {
        for (k, want) in [(128u8, 253u8), (64, 249), (0, 0)] {
            let o = px(
                &run(
                    "pencilSketchEffect",
                    json!({}),
                    &solid(9, 9, [k, k, k, 255]),
                ),
                4,
                4,
            );
            assert!(o[0].abs_diff(want) <= 1, "{k}: {o:?}");
        }
    }

    // ---- Task 39b additions: vectors from an independent Python port of effects-1.md §2.2. ----

    fn near(got: [u8; 4], want: [u8; 4]) -> bool {
        got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1)
    }

    fn grey_with(w: u32, h: u32, k: u8, at: (u32, u32), p: [u8; 4]) -> ImageData {
        from_fn(w, h, |x, y| if (x, y) == at { p } else { [k, k, k, 255] })
    }

    /// The flipped kernel inside the effect, on grey 26 (flat 2v = 0.20 < ½ → black mask): a black
    /// pixel 2 rows below the centre gives 2v + 5v = 0.71 → white mask → the glow shows; 2 rows
    /// above gives 2v + v = 0.31 → black. An unflipped kernel swaps them.
    #[test]
    fn ink_sketch_orientation_in_the_graph() {
        let below = grey_with(9, 9, 26, (4, 6), [0, 0, 0, 255]);
        assert!(px(&run("inkSketchEffect", json!({}), &below), 4, 4)[0] > 30);
        let above = grey_with(9, 9, 26, (4, 2), [0, 0, 0, 255]);
        assert_eq!(
            px(&run("inkSketchEffect", json!({}), &above), 4, 4),
            [0, 0, 0, 255]
        );
    }

    /// ConvolveMatrix details (genuine graph: BorderMode.Hard = clamp, PreserveAlpha):
    /// - Hard (clamp), NOT Mirror: on grey 15 with column 1 black, the x = 0 taps at dx −2/−1 clamp
    ///   to the grey edge column, so only the dx +1 black column subtracts → Σ = 7·0.0588 = 0.41 <
    ///   ½ → black mask → 0. Mirror would also read the black column at dx −2 (Σ = 0.71 > ½ → white).
    /// - PreserveAlpha: on grey 50 with the pixel 2 rows below at α 128, the taps are unpremultiplied
    ///   → RGB is flat (Σ = 2·0.196 = 0.39 < ½) → black; convolving premultiplied colour would not.
    #[test]
    fn ink_sketch_edges_and_preserve_alpha() {
        let col = from_fn(9, 9, |x, _| {
            if x == 1 {
                [0, 0, 0, 255]
            } else {
                [15, 15, 15, 255]
            }
        });
        assert_eq!(
            px(&run("inkSketchEffect", json!({}), &col), 0, 4),
            [0, 0, 0, 255]
        );
        let img = grey_with(9, 9, 50, (4, 6), [50, 50, 50, 128]);
        assert_eq!(
            px(&run("inkSketchEffect", json!({}), &img), 4, 4),
            [0, 0, 0, 255]
        );
    }

    /// Glow (step 4) and the min(t, 0.99) cap:
    /// - Coloring 100 → B = C = −100 on flat 128: Screen(v, 0.108 + …) → 141.70; Coloring 0 →
    ///   B = C = 100, F11's d = 0 step → white → 255 (a flipped sign swaps them);
    /// - flat translucent (128, 128, 128, 128): white mask (a, a, a, a) Darken the Screen glow
    ///   → premultiplied 0.565 at α 0.876 → (164, 164, 164, 223);
    /// - Ink outline 100 clamps to t = 0.99: flat white Y = 1 > t → 255 (t = 1 → black → 0).
    #[test]
    fn ink_sketch_glow_and_alpha() {
        let flat = solid(9, 9, [128, 128, 128, 255]);
        let at = |v: serde_json::Value, img: &ImageData| px(&run("inkSketchEffect", v, img), 4, 4);
        assert!(near(at(json!({"color": 100}), &flat), [142, 142, 142, 255]));
        assert_eq!(at(json!({"color": 0}), &flat), [255; 4]);
        let translucent = solid(9, 9, [128; 4]);
        assert!(near(at(json!({}), &translucent), [164, 164, 164, 223]));
        assert_eq!(at(json!({"ink": 100}), &solid(9, 9, [255; 4])), [255; 4]);
    }

    /// The glow blur is σ = 2.5 (radius 7.5), not σ = 7.5: ink 0 (any Y > 0 is white), coloring 0
    /// (glow white where the blur ≥ 0.108), background 1/255, a white band at x = 13..17. The blur
    /// is 0.162 at x = 10 → 255 and 0.041 at x = 8 → Screen(v, 0) = 1; σ = 7.5 gives 0.177 at x = 8.
    #[test]
    fn ink_sketch_glow_radius() {
        let img = from_fn(31, 5, |x, _| {
            if (13..=17).contains(&x) {
                [255; 4]
            } else {
                [1, 1, 1, 255]
            }
        });
        let out = run("inkSketchEffect", json!({"ink": 0, "color": 0}), &img);
        assert_eq!(px(&out, 10, 2), [255; 4]);
        assert_eq!(px(&out, 8, 2), [1, 1, 1, 255]);
    }

    /// Linear-light binning and the premultiplied mean (granularity 0.01 → 4 bins, brush 1):
    /// - 4 black + 5 grey 128 (linear 0.216 → bin 0): one bin → linear mean 0.120 → 97 (sRGB
    ///   binning keeps the greys apart → 128; an sRGB mean → 71);
    /// - 4 opaque red + 5 blue at α 51 (bin 0): Σ(rgb·a)/Σa = (0.8, 0, 0.2), α 5/9 →
    ///   (231, 0, 124, 142); a straight mean gives (178, 0, 197);
    /// - 5 white at α 51 + 4 opaque black: straight luma bins white at 3 (5 > 4) → (255, 255, 255,
    ///   51); premultiplied luma merges them → (124, 124, 124, 142).
    #[test]
    fn oil_painting_linear_premultiplied() {
        let two = |a: [u8; 4], b: [u8; 4]| {
            from_fn(
                3,
                3,
                move |x, y| if y == 0 || (x, y) == (0, 1) { a } else { b },
            )
        };
        let at = |img: &ImageData| {
            px(
                &run(
                    "oilPaintingEffect",
                    json!({"brush": 1, "granularity": 0.01}),
                    img,
                ),
                1,
                1,
            )
        };
        assert!(near(
            at(&two([0, 0, 0, 255], [128, 128, 128, 255])),
            [97, 97, 97, 255]
        ));
        assert!(near(
            at(&two([255, 0, 0, 255], [0, 0, 255, 51])),
            [231, 0, 124, 142]
        ));
        assert_eq!(
            at(&two([0, 0, 0, 255], [255, 255, 255, 51])),
            [255, 255, 255, 51]
        );
    }

    /// Kernel shape and Clamp edges, brush 2 on 5×5:
    /// - black at the 4 corners and 9 circle taps, white at the other 12: Circle (no corners) →
    ///   white 12 > 9; Square → black 13 > 12;
    /// - row 0 white, row 1 black, rows 2–4 grey, at (2, 0) with Square: Clamp reads row 0 for dy
    ///   −2..0 (white 15) → white; Mirror (dy −2 → row 1: white 10, black 10) → black by the
    ///   lowest-bin tie; Wrap → grey.
    #[test]
    fn oil_painting_kernel_and_edges() {
        let img = from_fn(5, 5, |x, y| {
            if y <= 1 || (x, y) == (0, 2) || (y == 4 && (x == 0 || x == 4)) {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            }
        });
        let oil = |v: serde_json::Value, img: &ImageData, x, y| {
            px(&run("oilPaintingEffect", v, img), x, y)
        };
        assert_eq!(oil(json!({"brush": 2}), &img, 2, 2), [255; 4]);
        assert_eq!(
            oil(json!({"brush": 2, "kernel": "square"}), &img, 2, 2),
            [0, 0, 0, 255]
        );
        let rows = from_fn(5, 5, |_, y| match y {
            0 => [255; 4],
            1 => [0, 0, 0, 255],
            _ => [128, 128, 128, 255],
        });
        assert_eq!(
            oil(json!({"brush": 2, "kernel": "square"}), &rows, 2, 0),
            [255; 4]
        );
    }

    /// Genuine graph: σ = StandardDeviation.FromRadius(tip·1.5) = tip·1.5/3 (tip 2 → σ 1),
    /// GaussianBlur with Optimization.Quality + BorderMode.Hard. On grey 128 with a white column at
    /// x = 10: x = 8 → 240, x = 9 → 204. Range ±20 on flat 128: ρ = 0.2 → Brightness 0.21 /
    /// Contrast −0.21 (num_b 21, num_c −21) → 219; ρ = −0.2 → ColorDodge clips → 255.
    #[test]
    fn pencil_sketch_blur_and_range() {
        let img = from_fn(21, 3, |x, _| {
            if x == 10 {
                [255; 4]
            } else {
                [128, 128, 128, 255]
            }
        });
        let out = run("pencilSketchEffect", json!({}), &img);
        assert!(
            near(px(&out, 8, 1), [240, 240, 240, 255]),
            "{:?}",
            px(&out, 8, 1)
        );
        assert!(
            near(px(&out, 9, 1), [204, 204, 204, 255]),
            "{:?}",
            px(&out, 9, 1)
        );
        let flat = solid(9, 9, [128, 128, 128, 255]);
        let at = |r: f64| px(&run("pencilSketchEffect", json!({"range": r}), &flat), 4, 4);
        assert!(near(at(20.0), [219, 219, 219, 255]), "{:?}", at(20.0));
        assert_eq!(at(-20.0), [255; 4]);
    }

    /// R19 controls; out-of-range values are clamped (brush 1e9 would otherwise allocate a
    /// 1e9-radius kernel).
    #[test]
    fn controls_and_clamping() {
        let ids: Vec<&str> = crate::effect::effects()
            .iter()
            .filter(|e| e.category == crate::effect::Category::Effect("artistic"))
            .map(|e| e.id)
            .collect();
        assert_eq!(
            ids,
            ["inkSketchEffect", "oilPaintingEffect", "pencilSketchEffect"]
        );
        let ranges = |id: &str| -> Vec<(&str, f64, f64, f64, f64)> {
            crate::effect::find(id)
                .unwrap()
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Range {
                        key,
                        min,
                        max,
                        default,
                        step,
                        ..
                    } => Some((*key, *min, *max, *default, *step)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            ranges("inkSketchEffect"),
            [
                ("ink", 0.0, 99.0, 50.0, 1.0),
                ("color", 0.0, 100.0, 50.0, 1.0)
            ]
        );
        assert_eq!(
            ranges("oilPaintingEffect"),
            [
                ("brush", 1.0, 50.0, 10.0, 1.0),
                ("granularity", 0.01, 1.0, 0.2, 0.01)
            ]
        );
        assert_eq!(
            ranges("pencilSketchEffect"),
            [
                ("tip", 1.0, 20.0, 2.0, 1.0),
                ("range", -20.0, 20.0, 0.0, 1.0)
            ]
        );
        let oil = crate::effect::find("oilPaintingEffect").unwrap();
        assert_eq!(crate::effect::defaults(&oil)["kernel"], "circle");
        let img = solid(4, 3, [90, 140, 30, 200]);
        for (id, v) in [
            ("inkSketchEffect", json!({"ink": -50, "color": 500})),
            (
                "oilPaintingEffect",
                json!({"brush": 1e9, "granularity": -3, "kernel": "x"}),
            ),
            (
                "oilPaintingEffect",
                json!({"brush": -4, "granularity": 1e9}),
            ),
            ("pencilSketchEffect", json!({"tip": 1e9, "range": -1e9})),
            ("pencilSketchEffect", json!({"tip": -1, "range": 1e9})),
        ] {
            assert_eq!(run(id, v, &img).data.len(), 48, "{id}");
        }
    }

    // ---- Fix round 1: vectors from an independent 2-D Python port of §2.2 (f32, W3C blends). ----

    /// Pencil on translucent, non-flat input: grey 128 at α 128 with an opaque white column at
    /// x = 10. The D2D Invert unpremultiplies before inverting (then re-premultiplies), so straight
    /// colour is inverted → (159, 159, 159, 195) at x = 8 and (131, 131, 131, 207) at x = 9;
    /// inverting the premultiplied colour directly would blow the unpremultiplied output out of range.
    #[test]
    fn pencil_sketch_translucent() {
        let img = from_fn(21, 3, |x, _| if x == 10 { [255; 4] } else { [128; 4] });
        let out = run("pencilSketchEffect", json!({}), &img);
        assert!(
            near(px(&out, 8, 1), [159, 159, 159, 195]),
            "{:?}",
            px(&out, 8, 1)
        );
        assert!(
            near(px(&out, 9, 1), [131, 131, 131, 207]),
            "{:?}",
            px(&out, 9, 1)
        );
    }

    /// Ink glow beside a translucent edge: opaque black for x < 10, white at α 64 from x = 10. The
    /// mask is white 3+ px from the edge, so the output is Darken(mask, Screen(src, blur)) where the
    /// glow's GaussianBlur runs on the PREMULTIPLIED source → (226, 226, 226, 158) at x = 13 and
    /// (244, 244, 244, 151) at x = 14, genuine Paint.NET 5.1.12's InkSketchGpuEffect captured on the
    /// VM (pdngpu --builtin); blurring straight colour gives 247 and 252. Native blurs on its halving
    /// graph at Float32 too (opend2d 09f57bb); the full-resolution float blur gave 231, 156.
    #[test]
    fn ink_sketch_glow_by_a_translucent_edge() {
        let img = from_fn(31, 5, |x, _| {
            if x < 10 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 64]
            }
        });
        let out = run("inkSketchEffect", json!({}), &img);
        assert!(
            near(px(&out, 13, 2), [226, 226, 226, 158]),
            "{:?}",
            px(&out, 13, 2)
        );
        assert!(
            near(px(&out, 14, 2), [244, 244, 244, 151]),
            "{:?}",
            px(&out, 14, 2)
        );
    }

    /// ClampOutput: (255, 60, 0) around a (255, 0, 0) centre convolves to r = 2, g = −28·60/255;
    /// clamped Y = 0.299 > t = 0.2 (ink 20) → white mask → the glow (255, 58.5, 0); unclamped
    /// Y ≈ 0.299·2 + 0.587·(−6.59) ≈ −3.27 → black → 0.
    #[test]
    fn ink_sketch_clamps_the_convolution() {
        let img = from_fn(9, 9, |x, y| {
            if (x, y) == (4, 4) {
                [255, 0, 0, 255]
            } else {
                [255, 60, 0, 255]
            }
        });
        let o = px(&run("inkSketchEffect", json!({"ink": 20}), &img), 4, 4);
        assert!(near(o, [255, 59, 0, 255]), "{o:?}");
    }

    /// The sliding histogram equals the direct evaluation of §2.2 (count every tap, sum the mode
    /// bin's taps in f32, Clamp through `fetch`) within 1 level, on a hashed image with
    /// transparent (hidden colour black, bin 0) and translucent pixels.
    #[test]
    fn oil_painting_sliding_matches_direct() {
        let img = from_fn(23, 17, |x, y| {
            let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663))
                .wrapping_mul(2_654_435_761);
            match h >> 30 {
                0 => [0, 0, 0, 0],
                1 => [h as u8, (h >> 8) as u8, (h >> 16) as u8, (h >> 22) as u8],
                _ => [h as u8, (h >> 8) as u8, (h >> 16) as u8, 255],
            }
        });
        let src = EffectSurface::from_rgba8(&img, Space::Linear, Alpha::Straight);
        for (r, g, square) in [
            (3, 0.2, false),
            (2, 0.01, true),
            (30, 0.5, false),
            (12, 0.05, true),
        ] {
            let bins = oil_bins(g) as usize;
            let taps: Vec<(i32, i32)> = if square {
                (-r..=r)
                    .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
                    .collect()
            } else {
                circle_offsets(r as u32)
            };
            let bin = |p: &[f32; 4]| {
                (luma(p[0], p[1], p[2]).clamp(0.0, 1.0) * (bins - 1) as f32) as usize
            };
            let direct = EffectSurface::from_fn(src.w, src.h, |x, y| {
                let t: Vec<[f32; 4]> = taps
                    .iter()
                    .map(|&(dx, dy)| src.fetch(x as i32 + dx, y as i32 + dy, Edge::Clamp))
                    .collect();
                let mut count = vec![0u32; bins];
                for p in &t {
                    count[bin(p)] += 1;
                }
                let mode = (0..bins).fold(0, |m, i| if count[i] > count[m] { i } else { m });
                let mut sum = [0.0f32; 4];
                for p in t.iter().filter(|p| bin(p) == mode) {
                    for (s, c) in sum
                        .iter_mut()
                        .zip([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
                    {
                        *s += c;
                    }
                }
                unpremultiply(sum.map(|s| s / count[mode] as f32))
            })
            .to_rgba8(Space::Linear, Alpha::Straight);
            let kernel = if square { "square" } else { "circle" };
            let out = run(
                "oilPaintingEffect",
                json!({"brush": r, "granularity": g, "kernel": kernel}),
                &img,
            );
            let worst = out
                .data
                .iter()
                .zip(&direct.data)
                .map(|(a, b)| a.abs_diff(*b))
                .max();
            assert!(worst <= Some(1), "r {r} g {g} {kernel}: {worst:?}");
        }
    }
}
