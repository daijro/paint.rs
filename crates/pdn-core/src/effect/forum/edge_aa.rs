//! Basic Antialias (pyjo, 2008) and AA's Assistant (dpy, 2009, after an idea by Boude), written
//! from their published behaviour only. Both change alpha and nothing else, except that a pixel
//! which was fully transparent and becomes visible takes the colour of its visible neighbours (its
//! own stored colour was never shown).
//!
//! Basic Antialias smooths the staircase where a fully opaque pixel meets a fully transparent one,
//! MLAA style: each run of boundary pixels along a row (or column) that ends in a step gets the
//! coverage of the straight line from the middle of that step to the run's middle (or its far end
//! when the run's other end has no step). Partly transparent edges are left alone.
//!
//! AA's Assistant averages each edge pixel's alpha over 21 taps (the 5 × 5 block without its
//! corners; dpy's published weights, as paint.c's clean-room AA's Assistant (MIT) lists them) and
//! maps the result through an alpha curve. The curve keeps 0 and 255 and, at Offset 0, keeps 50 %
//! where it is, so repeated runs settle instead of eating the object. With Soften off it is the
//! curve alone, on every pixel.

use crate::canvas::ImageData;
use crate::effect::bb::ui::{ControlUi, Help, IndirectUi};
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, flag, num, range};
use crate::extra::par;

pub const BASIC_ANTIALIAS_ID: &str = "pyjoBasicAntialias";
pub const AA_ASSISTANT_ID: &str = "dpyAaAssistant";

const FULLY_OPAQUE: u8 = 255;
const FULLY_TRANSPARENT: u8 = 0;

/// Above it the result reads as two decimals, like Bevel Object's Strength.
const TWO_DECIMALS: ControlUi = ControlUi {
    decimals: Some(2),
    ..ControlUi::DEFAULT
};

static AA_ASSISTANT_UI: IndirectUi = IndirectUi {
    controls: &[
        ("soften", ControlUi::NO_HEADER),
        ("sharpness", TWO_DECIMALS),
        ("gamma", TWO_DECIMALS),
        ("offset", TWO_DECIMALS),
    ],
    help: Help::None,
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let basic = super::manifest(BASIC_ANTIALIAS_ID).unwrap();
    let assistant = super::manifest(AA_ASSISTANT_ID).unwrap();
    let decimal = |c: Control| c.step(0.01).slider_keys(2, 0.01, 0.1, false);
    vec![
        EffectDef {
            id: assistant.id,
            icon: assistant.icon,
            category: assistant.category,
            controls: vec![
                Control::Checkbox {
                    key: "soften",
                    label: "Soften the edges",
                    default: true,
                },
                decimal(range("sharpness", "Sharpness", 1.0, 4.0, 2.0)),
                decimal(range("gamma", "Gamma", 0.5, 2.0, 1.0)),
                decimal(range("offset", "Offset", -1.0, 1.0, 0.0)),
            ],
            kind: EffectKind::Cpu {
                render: aa_assistant,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some("AA's Assistant"),
            indirect: Some(&AA_ASSISTANT_UI),
        },
        EffectDef {
            id: basic.id,
            icon: basic.icon,
            category: basic.category,
            controls: vec![],
            kind: EffectKind::Cpu {
                render: basic_antialias,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: None,
            indirect: None,
        },
    ]
}

fn alpha_of(src: &ImageData) -> Vec<u8> {
    src.data.as_chunks::<4>().0.iter().map(|p| p[3]).collect()
}

// --- Basic Antialias ------------------------------------------------------------------------

fn basic_antialias(src: &ImageData, _: &Values, _: &EffectEnv) -> ImageData {
    let (w, h) = (src.width as usize, src.height as usize);
    let alpha = alpha_of(src);
    let across_rows = staircase_change(&alpha, w, h);
    let across_columns = transpose(&staircase_change(&transpose(&alpha, w, h), h, w), h, w);
    let mut out = src.clone();
    par::rows_mut(&mut out.data, w * 4, |y, row| {
        for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let i = y * w + x;
            let change = across_rows[i].max(across_columns[i]);
            if change == 0 {
                continue;
            }
            let is_opaque_side = alpha[i] == FULLY_OPAQUE;
            if is_opaque_side {
                p[3] = FULLY_OPAQUE - change;
                continue;
            }
            p[3] = change;
            p[..3].copy_from_slice(&opaque_neighbour_colour(src, &alpha, x, y));
        }
    });
    out
}

/// Per pixel, how far Basic Antialias moves it toward the other side (0…255), from the runs along
/// each row of the w × h `alpha` against the rows above and below.
fn staircase_change(alpha: &[u8], w: usize, h: usize) -> Vec<u8> {
    debug_assert_eq!(alpha.len(), w * h, "alpha is w × h");
    let mut change = vec![0; alpha.len()];
    par::rows_mut(&mut change, w, |y, row_change| {
        let row = &alpha[y * w..][..w];
        if y > 0 {
            staircase_row(row, &alpha[(y - 1) * w..][..w], row_change);
        }
        if y + 1 < h {
            staircase_row(row, &alpha[(y + 1) * w..][..w], row_change);
        }
    });
    change
}

/// The hard boundary runs of `row` against `beside` (the row above or below): a run is a stretch
/// where `row` is one of 0 | 255 and `beside` the other. Each run end where `row` itself switches
/// (a step) ramps the run's pixels toward the other value. Keeps the larger change in `change`.
fn staircase_row(row: &[u8], beside: &[u8], change: &mut [u8]) {
    debug_assert_eq!(row.len(), beside.len(), "rows have one length");
    debug_assert_eq!(row.len(), change.len(), "rows have one length");
    let len = row.len();
    let mut x = 0;
    while x < len {
        let is_hard_pair = row[x].abs_diff(beside[x]) == FULLY_OPAQUE;
        if !is_hard_pair {
            x += 1;
            continue;
        }
        let own = row[x];
        let start = x;
        while x < len && row[x] == own && beside[x] == FULLY_OPAQUE - own {
            x += 1;
        }
        let left = run_end(row, beside, start.checked_sub(1), own);
        let right = run_end(row, beside, Some(x).filter(|&e| e < len), own);
        ramp_run(start, x, left, right, change);
    }
}

/// What lies just past a run end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunEnd {
    /// The canvas border, partial alpha or a checkerboard: no step to smooth.
    Open,
    /// The run's own row switches to the other value.
    OwnStep,
    /// The row beside switches to the run's value.
    BesideStep,
}

fn run_end(row: &[u8], beside: &[u8], past: Option<usize>, own: u8) -> RunEnd {
    let Some(p) = past else {
        return RunEnd::Open;
    };
    let other = FULLY_OPAQUE - own;
    match (row[p], beside[p]) {
        (r, b) if r == other && b == other => RunEnd::OwnStep,
        (r, b) if r == own && b == own => RunEnd::BesideStep,
        _ => RunEnd::Open,
    }
}

/// The run [start, end): a step at one end ramps from half a pixel at that end down to nothing at
/// the run's middle, or at the far end when that end is open (an L rather than a Z or U shape).
fn ramp_run(start: usize, end: usize, left: RunEnd, right: RunEnd, change: &mut [u8]) {
    let run_len = (end - start) as f32;
    let ramp_len = |far: RunEnd| {
        if far == RunEnd::Open {
            run_len
        } else {
            run_len / 2.0
        }
    };
    let has_left_step = left == RunEnd::OwnStep;
    let has_right_step = right == RunEnd::OwnStep;
    if !has_left_step && !has_right_step {
        return;
    }
    let (left_len, right_len) = (ramp_len(right), ramp_len(left));
    for (offset, c) in change[start..end].iter_mut().enumerate() {
        let x = offset as f32;
        let mut area = 0.0;
        if has_left_step {
            area += ramp_area(x, x + 1.0, left_len);
        }
        if has_right_step {
            area += ramp_area(run_len - x - 1.0, run_len - x, right_len);
        }
        *c = (*c).max((area * 255.0).round() as u8);
    }
}

/// The area under h(d) = ½·(1 − d / len) on [d0, d1] ∩ [0, len]: the part of a pixel spanning d0…d1
/// (distance from the step) that the edge line moves to the other side.
fn ramp_area(d0: f32, d1: f32, len: f32) -> f32 {
    let (a, b) = (d0.max(0.0), d1.min(len));
    if b <= a {
        return 0.0;
    }
    (b - a) * 0.5 * (1.0 - (a + b) / (2.0 * len))
}

fn transpose(v: &[u8], w: usize, h: usize) -> Vec<u8> {
    debug_assert_eq!(v.len(), w * h, "buffer is w × h");
    let mut out = vec![0; v.len()];
    par::rows_mut(&mut out, h, |x, column| {
        for (y, c) in column.iter_mut().enumerate() {
            *c = v[y * w + x];
        }
    });
    out
}

/// The mean colour of the fully opaque 4-neighbours of (x, y); black when there are none.
fn opaque_neighbour_colour(src: &ImageData, alpha: &[u8], x: usize, y: usize) -> [u8; 3] {
    let (w, h) = (src.width as usize, src.height as usize);
    let neighbours = [
        (x > 0).then(|| y * w + x - 1),
        (x + 1 < w).then(|| y * w + x + 1),
        (y > 0).then(|| (y - 1) * w + x),
        (y + 1 < h).then(|| (y + 1) * w + x),
    ];
    let opaque: Vec<usize> = neighbours
        .into_iter()
        .flatten()
        .filter(|&i| alpha[i] == FULLY_OPAQUE)
        .collect();
    if opaque.is_empty() {
        return [0; 3];
    }
    let n = opaque.len() as u32;
    std::array::from_fn(|c| {
        let sum: u32 = opaque.iter().map(|&i| u32::from(src.data[i * 4 + c])).sum();
        ((sum + n / 2) / n) as u8
    })
}

// --- AA's Assistant -------------------------------------------------------------------------

/// The 21 taps (dx, dy, weight): dpy's coefficients ×20 (0.35 0.5 0.7 1 → 7 10 14 20), so the
/// sums are exact integers.
#[rustfmt::skip]
const TAPS: [(i32, i32, u32); 21] = [
                 (-1, -2, 7), (0, -2, 10), (1, -2, 7),
    (-2, -1, 7), (-1, -1, 14), (0, -1, 20), (1, -1, 14), (2, -1, 7),
    (-2, 0, 10), (-1, 0, 20), (0, 0, 20), (1, 0, 20), (2, 0, 10),
    (-2, 1, 7), (-1, 1, 14), (0, 1, 20), (1, 1, 14), (2, 1, 7),
                 (-1, 2, 7), (0, 2, 10), (1, 2, 7),
];
const TAP_WEIGHT_SUM: u32 = 252;
/// The largest weighted alpha sum, the curve table's last index.
const MAX_TAP_SUM: u32 = TAP_WEIGHT_SUM * 255;
const TAP_REACH: usize = 2;

fn aa_assistant(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let curve = alpha_curve(num(v, "sharpness"), num(v, "gamma"), num(v, "offset"));
    let mut out = src.clone();
    let should_soften = flag(v, "soften");
    if !should_soften {
        for p in out.data.as_chunks_mut::<4>().0 {
            p[3] = curve[(u32::from(p[3]) * TAP_WEIGHT_SUM) as usize];
        }
        return out;
    }
    let (w, h) = (src.width as usize, src.height as usize);
    let alpha = alpha_of(src);
    let (row_min, row_max) = row_window_extremes(&alpha, w);
    par::rows_mut(&mut out.data, w * 4, |y, row| {
        let rows = window_rows(y, h);
        for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let lowest = rows.clone().map(|r| row_min[r * w + x]).min();
            let highest = rows.clone().map(|r| row_max[r * w + x]).max();
            let is_edge_pixel = lowest != highest;
            if !is_edge_pixel {
                continue;
            }
            *p = softened_pixel(src, &alpha, &curve, x, y);
        }
    });
    out
}

/// Pixel (x, y) with its alpha from the 21-tap mean through `curve`. A pixel that was fully
/// transparent and becomes visible takes the alpha-weighted mean colour of the taps.
fn softened_pixel(src: &ImageData, alpha: &[u8], curve: &[u8], x: usize, y: usize) -> [u8; 4] {
    let (w, h) = (src.width as usize, src.height as usize);
    let tap_index = |&(dx, dy, weight): &(i32, i32, u32)| {
        let tx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
        let ty = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
        (ty * w + tx, weight)
    };
    let alpha_sum: u32 = TAPS
        .iter()
        .map(tap_index)
        .map(|(i, weight)| weight * u32::from(alpha[i]))
        .sum();
    let i = y * w + x;
    let mut p: [u8; 4] = src.data[i * 4..i * 4 + 4].try_into().unwrap();
    p[3] = curve[alpha_sum as usize];
    let becomes_visible = alpha[i] == FULLY_TRANSPARENT && p[3] > 0;
    if !becomes_visible {
        return p;
    }
    for (c, channel) in p[..3].iter_mut().enumerate() {
        let colour_sum: u32 = TAPS
            .iter()
            .map(tap_index)
            .map(|(t, weight)| weight * u32::from(alpha[t]) * u32::from(src.data[t * 4 + c]))
            .sum();
        *channel = ((colour_sum + alpha_sum / 2) / alpha_sum) as u8;
    }
    p
}

/// The rows of the 5 × 5 window around row y, clamped to the canvas.
fn window_rows(y: usize, h: usize) -> impl Iterator<Item = usize> + Clone {
    y.saturating_sub(TAP_REACH)..=(y + TAP_REACH).min(h - 1)
}

/// Per pixel, the lowest and highest alpha within ±2 columns on its own row (clamped).
fn row_window_extremes(alpha: &[u8], w: usize) -> (Vec<u8>, Vec<u8>) {
    let mut lowest = vec![0; alpha.len()];
    let mut highest = vec![0; alpha.len()];
    par::chunks2_mut(&mut lowest, w, &mut highest, w, true, |y, low, high| {
        let row = &alpha[y * w..][..w];
        for x in 0..w {
            let window = &row[x.saturating_sub(TAP_REACH)..=(x + TAP_REACH).min(w - 1)];
            low[x] = *window.iter().min().unwrap();
            high[x] = *window.iter().max().unwrap();
        }
    });
    (lowest, highest)
}

/// The curve as a table over the weighted alpha sum 0…`MAX_TAP_SUM`: x = sum / max,
/// y = clamp(s·(x − x0), 0, 1)^γ with x0 = (½ + offset / 2)·(1 − 1 / s). Sharpness 1, gamma 1 and
/// offset 0 are the identity; every setting keeps 0 and 255, and offset 0 keeps 50 %.
fn alpha_curve(sharpness: f64, gamma: f64, offset: f64) -> Vec<u8> {
    let sharpness = sharpness.clamp(1.0, 4.0);
    let gamma = gamma.clamp(0.5, 2.0);
    let curve_start = (0.5 + offset.clamp(-1.0, 1.0) / 2.0) * (1.0 - 1.0 / sharpness);
    let curve: Vec<u8> = (0..=MAX_TAP_SUM)
        .map(|sum| {
            let x = f64::from(sum) / f64::from(MAX_TAP_SUM);
            let y = (sharpness * (x - curve_start)).clamp(0.0, 1.0).powf(gamma);
            (255.0 * y).round() as u8
        })
        .collect();
    debug_assert_eq!(curve[0], FULLY_TRANSPARENT, "the curve keeps 0");
    debug_assert_eq!(
        curve[MAX_TAP_SUM as usize], FULLY_OPAQUE,
        "the curve keeps 255"
    );
    curve
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::test_util::{rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use serde_json::json;

    const DISK_RGB: [u8; 3] = [200, 50, 30];
    /// The stored colour of the transparent pixels, never shown.
    const HIDDEN_RGB: [u8; 3] = [10, 20, 30];

    /// A hard-edged opaque disk of radius `r` centred on a `size` × `size` transparent layer.
    fn disk(size: u32, r: f64) -> ImageData {
        let centre = f64::from(size) / 2.0;
        from_fn(size, size, |x, y| {
            let (dx, dy) = (f64::from(x) + 0.5 - centre, f64::from(y) + 0.5 - centre);
            let is_inside = dx * dx + dy * dy <= r * r;
            let [r, g, b] = if is_inside { DISK_RGB } else { HIDDEN_RGB };
            [r, g, b, if is_inside { 255 } else { 0 }]
        })
    }

    /// The radius of a disk with the image's total coverage.
    fn coverage_radius(img: &ImageData) -> f64 {
        let area: f64 = alpha_of(img).iter().map(|&a| f64::from(a) / 255.0).sum();
        (area / std::f64::consts::PI).sqrt()
    }

    fn is_next_to_other_side(alpha: &[u8], w: usize, x: usize, y: usize) -> bool {
        let own = alpha[y * w + x];
        let neighbours = [
            (x > 0).then(|| y * w + x - 1),
            (x + 1 < w).then(|| y * w + x + 1),
            (y > 0).then(|| (y - 1) * w + x),
            Some((y + 1) * w + x).filter(|&i| i < alpha.len()),
        ];
        neighbours
            .into_iter()
            .flatten()
            .any(|i| alpha[i].abs_diff(own) == 255)
    }

    #[test]
    fn opaque_and_transparent_layers_are_untouched() {
        let opaque = from_fn(9, 7, |x, y| [(x * 20) as u8, (y * 30) as u8, 90, 255]);
        let transparent = from_fn(9, 7, |x, y| [(x * 20) as u8, (y * 30) as u8, 90, 0]);
        for id in [BASIC_ANTIALIAS_ID, AA_ASSISTANT_ID] {
            for img in [&opaque, &transparent] {
                assert_eq!(&run(id, json!({}), img), img, "{id}");
                assert_eq!(&run(id, json!({"soften": false}), img), img, "{id}");
            }
        }
    }

    /// An L-shaped run (a step on the left, the canvas border on the right) ramps over the whole
    /// run; a U-shaped one (steps at both ends) from each end to the middle.
    #[test]
    fn staircase_row_ramps_from_each_step() {
        let mut change = [0; 6];
        staircase_row(&[0, 0, 255, 255, 255, 255], &[0; 6], &mut change);
        assert_eq!(change, [0, 0, 112, 80, 48, 16]);

        let mut change = [0; 4];
        staircase_row(&[0, 255, 255, 0], &[0; 4], &mut change);
        assert_eq!(change, [0, 64, 64, 0]);

        // The transparent side of a Z: the row beside steps on the left, the row itself on the
        // right, so only the right half (2 of 4 pixels) gains.
        let mut change = [0; 6];
        staircase_row(
            &[0, 0, 0, 0, 0, 255],
            &[0, 255, 255, 255, 255, 255],
            &mut change,
        );
        assert_eq!(change, [0, 0, 0, 32, 96, 0]);
    }

    #[test]
    fn staircase_row_ignores_straight_and_soft_edges() {
        let mut change = [0; 5];
        staircase_row(&[255; 5], &[0; 5], &mut change);
        staircase_row(&[0, 0, 128, 128, 0], &[0; 5], &mut change);
        staircase_row(&[255, 255, 0, 0, 255], &[128; 5], &mut change);
        assert_eq!(change[..], [0, 0, 0, 0, 0][..]);
    }

    #[test]
    fn ramp_area_is_a_triangle_of_half_a_pixel_height() {
        assert!((ramp_area(0.0, 4.0, 4.0) - 1.0).abs() < 1e-6);
        assert!((ramp_area(-3.0, 9.0, 4.0) - 1.0).abs() < 1e-6);
        assert_eq!(ramp_area(4.0, 5.0, 4.0), 0.0);
    }

    #[test]
    fn basic_antialias_softens_only_the_disk_boundary() {
        let src = disk(40, 14.0);
        let out = run(BASIC_ANTIALIAS_ID, json!({}), &src);
        let (src_alpha, out_alpha) = (alpha_of(&src), alpha_of(&out));
        let mut softened = 0;
        for (y, x) in (0..40).flat_map(|y| (0..40).map(move |x| (y, x))) {
            let (before, after) = (px(&src, x, y), px(&out, x, y));
            let (x, y) = (x as usize, y as usize);
            if before == after {
                continue;
            }
            assert!(
                is_next_to_other_side(&src_alpha, 40, x, y),
                "({x}, {y}) is inside"
            );
            let was_visible = before[3] > 0;
            let want_rgb = if was_visible {
                &before[..3]
            } else {
                &DISK_RGB[..]
            };
            assert_eq!(&after[..3], want_rgb, "({x}, {y}) colour");
            softened += usize::from((1..255).contains(&out_alpha[y * 40 + x]));
        }
        assert!(softened > 40, "only {softened} boundary pixels softened");
        assert_eq!(px(&out, 20, 20), px(&src, 20, 20));
        assert!((coverage_radius(&out) - coverage_radius(&src)).abs() < 0.25);
    }

    #[test]
    fn basic_antialias_leaves_soft_edges_and_straight_edges() {
        let soft = from_fn(12, 12, |x, y| [9, 9, 9, if x + y < 10 { 128 } else { 0 }]);
        assert_eq!(run(BASIC_ANTIALIAS_ID, json!({}), &soft), soft);
        let half = from_fn(12, 12, |x, _| [9, 9, 9, if x < 6 { 255 } else { 0 }]);
        assert_eq!(run(BASIC_ANTIALIAS_ID, json!({}), &half), half);
    }

    #[test]
    fn aa_assistant_neutral_curve_without_soften_is_the_identity() {
        let img = from_fn(16, 16, |x, y| {
            [x as u8 * 9, y as u8 * 7, 50, (x * 16 + y) as u8]
        });
        let neutral = json!({"soften": false, "sharpness": 1.0, "gamma": 1.0, "offset": 0.0});
        assert_eq!(run(AA_ASSISTANT_ID, neutral, &img), img);
    }

    #[test]
    fn alpha_curve_keeps_the_ends_and_the_middle() {
        for (sharpness, gamma, offset) in [(1.0, 1.0, 0.0), (4.0, 0.5, -1.0), (2.0, 2.0, 1.0)] {
            let curve = alpha_curve(sharpness, gamma, offset);
            assert_eq!(curve[0], 0);
            assert_eq!(curve[MAX_TAP_SUM as usize], 255);
        }
        for sharpness in [1.0, 2.0, 4.0] {
            let curve = alpha_curve(sharpness, 1.0, 0.0);
            assert!(curve[MAX_TAP_SUM as usize / 2].abs_diff(128) <= 1);
        }
    }

    #[test]
    fn aa_assistant_softens_the_edge_and_keeps_visible_colour() {
        let src = run(BASIC_ANTIALIAS_ID, json!({}), &disk(40, 14.0));
        let out = run(AA_ASSISTANT_ID, json!({}), &src);
        assert_ne!(out, src);
        assert_eq!(px(&out, 20, 20), px(&src, 20, 20));
        assert_eq!(px(&out, 0, 0), px(&src, 0, 0));
        for (p, q) in src
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.data.as_chunks::<4>().0)
        {
            let was_visible = p[3] > 0;
            let want_rgb = if was_visible { &p[..3] } else { &q[..3] };
            assert_eq!(&q[..3], want_rgb);
            let becomes_visible = !was_visible && q[3] > 0;
            assert!(!becomes_visible || q[..3] == DISK_RGB);
        }
    }

    /// Basic Antialias, then AA's Assistant three times: the edge stays put within about a pixel.
    #[test]
    fn aa_assistant_is_stable_when_repeated() {
        let src = disk(64, 20.0);
        let mut img = run(BASIC_ANTIALIAS_ID, json!({}), &src);
        for _ in 0..3 {
            img = run(AA_ASSISTANT_ID, json!({}), &img);
        }
        let shrink = coverage_radius(&src) - coverage_radius(&img);
        assert!(shrink.abs() <= 1.0, "radius moved by {shrink}");
        assert_eq!(px(&img, 32, 32)[3], 255);
    }

    #[test]
    fn both_effects_leave_pixels_outside_the_selection() {
        let src = disk(40, 14.0);
        let (mask, polys) = rect_selection(40, 40, 0, 0, 20, 40);
        let env = with_selection(40, 40, &mask, &polys);
        for id in [BASIC_ANTIALIAS_ID, AA_ASSISTANT_ID] {
            let out = run_env(id, json!({}), &src, &env);
            let changed = |x: u32| (0..40).any(|y| px(&out, x, y) != px(&src, x, y));
            assert!((0..20).any(changed), "{id} changed nothing");
            assert!(!(20..40).any(changed), "{id} changed outside the selection");
        }
    }
}
