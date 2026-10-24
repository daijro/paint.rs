//! BB spec: effects/color-complementary-colors.md; effects/photo-level-horizon-plumb-bob.md;
//! effects/render-dimensions.md; common.md §1–3, §5.6, §5.7, §5.10
//!
//! The fixed-font text effects: Complementary Colors' RYB chart, Level Horizon / Plumb Bob's
//! guideline overlay and rotation, and Dimensions' dimension line with its knocked-out label.

use super::blocks::{affine_cubic, colour_exact, source_over};
use super::conventions::{
    DIMENSIONS_HALO_STROKE_ALPHA, DIMENSIONS_OUTLINE_PRESENTATION, GPU_CONTEXT,
    HORIZON_DASH_ROUND_CAPS, HORIZON_INDICATOR_FONT, HORIZON_LABEL_GLYPH_ROUTE,
    HORIZON_SWAPPED_ATAN2, SANS_FONT,
};
use super::manifest::manifest;
use super::raster::{Cap, Join, Pen, fill_coverage, polygon_path, stroke_coverage};
use super::text::{coverage, glyph_run, pt_to_px};
use super::ui::{ControlUi, Help, IndirectUi, Note, Tab, TabStrip};
use super::{pan, text};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::effect::slider::SliderKeys;
use crate::effect::surface::{Edge, EffectSurface};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, color_value, flag,
    num, range,
};
use crate::fonts::FaceStyle;
use crate::pixel::{Px, ops::compose};
use crate::util::color::HsvColor;
use serde_json::json;
use tiny_skia::{FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

#[rustfmt::skip]
/// color-complementary-colors.md "The four colours": RYB angle → RGB hue, exactly as listed
/// (non-monotonic around 274–275, 322 and 329).
pub const RYB_TO_RGB_HUE: [u16; 361] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 11, 12, 13, 13,
    14, 15, 15, 16, 17, 18, 19, 20, 20, 21, 22, 24, 24, 24, 25, 26, 28, 28, 28, 29, 30, 32, 32, 32, 33, 34, 36, 36, 36, 37,
    38, 38, 38, 38, 38, 40, 40, 40, 40, 40, 40, 40, 40, 40, 40, 41, 41, 41, 41, 41, 42, 42, 42, 42, 42, 44, 44, 44, 44, 44,
    44, 44, 45, 45, 46, 46, 48, 48, 48, 48, 49, 49, 50, 50, 52, 52, 52, 52, 53, 53, 54, 54, 56, 56, 56, 56, 57, 57, 58, 58,
    60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89,
    90, 90, 90, 92, 92, 93, 93, 93, 95, 96, 96, 97, 98, 98, 99, 100, 100, 102, 103, 104, 105, 106, 108, 109, 110, 112, 113, 115, 117, 118,
    122, 125, 128, 132, 135, 138, 142, 144, 145, 148, 150, 152, 154, 157, 158, 161, 161, 163, 166, 166, 168, 169, 171, 172, 173, 175, 175, 177, 178, 179,
    181, 183, 185, 187, 189, 191, 193, 195, 197, 199, 201, 203, 205, 207, 208, 211, 213, 214, 217, 219, 220, 223, 225, 226, 229, 231, 232, 235, 236, 238,
    240, 240, 241, 242, 243, 244, 245, 246, 248, 248, 249, 250, 252, 252, 253, 254, 255, 256, 257, 258, 259, 260, 261, 262, 264, 264, 265, 266, 268, 268,
    269, 270, 270, 270, 272, 271, 272, 273, 273, 274, 275, 275, 276, 277, 278, 279, 280, 281, 283, 284, 285, 286, 287, 289, 290, 291, 293, 295, 296, 297,
    301, 303, 303, 305, 307, 309, 310, 312, 313, 313, 314, 315, 318, 318, 319, 319, 321, 322, 323, 323, 324, 325, 324, 326, 326, 326, 328, 328, 329, 328,
    331, 332, 332, 333, 334, 335, 336, 337, 338, 340, 340, 341, 342, 343, 344, 345, 346, 347, 348, 349, 350, 351, 352, 353, 354, 355, 356, 357, 358, 359,
    360,
];

/// The RYB angle of an RGB hue: the smallest index whose entry is ≥ it (an ordered scan; the
/// table is not monotonic).
fn ryb_angle(hue: u16) -> usize {
    RYB_TO_RGB_HUE.iter().position(|&h| h >= hue).unwrap_or(360)
}

/// color-complementary-colors.md "The four colours": C1 (the 1-colour complement), C2 and C3
/// (the split complement) of opaque P, in Paint.NET's truncating integer HSV.
// approximation: Paint.NET 5's HSV assumed to truncate as 4.2's (color-complementary-colors.md)
pub fn complements(p: [u8; 3]) -> [[u8; 3]; 3] {
    let hsv = HsvColor::from_rgb(p[0], p[1], p[2]);
    let spread = p.iter().max().unwrap() - p.iter().min().unwrap();
    if spread <= 10 {
        // Near grey: only the value moves, wrapping by 100.
        return [50, 30, 60].map(|d| {
            let v = hsv.v - d;
            HsvColor::new(hsv.h, hsv.s, if v < 0 { v + 100 } else { v }).to_rgb()
        });
    }
    let angle = ryb_angle(hsv.h as u16) as i32;
    [180, 150, -150].map(|d| {
        let a = match angle + d {
            a if a > 360 => a - 360,
            a if a < 0 => a + 360,
            a => a,
        };
        HsvColor::new(RYB_TO_RGB_HUE[a as usize] as i32, hsv.s, hsv.v).to_rgb()
    })
}

/// A transparent drawing layer (premultiplied 8-bit), composited over the layer at the end.
struct Layer(Option<Pixmap>);

impl Layer {
    fn new(w: u32, h: u32) -> Self {
        Self(Pixmap::new(w, h))
    }
    fn paint(c: Px) -> Paint<'static> {
        let mut paint = Paint::default();
        paint.set_color_rgba8(c[0], c[1], c[2], c[3]);
        paint.anti_alias = true;
        paint
    }
    fn fill(&mut self, path: &Path, c: Px, t: Transform) {
        if let Some(pm) = &mut self.0 {
            pm.fill_path(path, &Self::paint(c), FillRule::Winding, t, None);
        }
    }
    fn stroke(&mut self, path: &Path, stroke: &Stroke, c: Px) {
        if let Some(pm) = &mut self.0 {
            pm.stroke_path(path, &Self::paint(c), stroke, Transform::identity(), None);
        }
    }
    /// Copy mode with a transparent colour (`DIMENSIONS_HALO_STROKE_ALPHA`), premultiplied.
    fn erase(&mut self, coverage: &[f32]) {
        if let Some(pm) = &mut self.0 {
            for (p, k) in pm
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(coverage)
            {
                if *k > 0.0 {
                    *p = p.map(|c| DIMENSIONS_HALO_STROKE_ALPHA(c, *k));
                }
            }
        }
    }
    /// Normal source-over onto `src` in straight 8-bit encoded sRGB.
    fn over(&self, src: &ImageData) -> ImageData {
        let mut out = src.clone();
        if let Some(pm) = &self.0 {
            for (d, s) in out.data.as_chunks_mut::<4>().0.iter_mut().zip(pm.pixels()) {
                let s = s.demultiply();
                *d = compose(
                    BlendMode::Normal,
                    *d,
                    [s.red(), s.green(), s.blue(), s.alpha()],
                    255,
                );
            }
        }
        out
    }
}

fn opaque(v: &Values, env: &EffectEnv) -> Px {
    let [r, g, b, _] = color_value(v, "color", env);
    [r, g, b, 255]
}

const BLACK: Px = [0, 0, 0, 255];
const WHITE: Px = [255, 255, 255, 255];

/// color-complementary-colors.md "Chart layout": swatches and texts from the selection bounds'
/// top-left, GDI+ pixel centres on integers (the +0.5 shift).
fn complementary(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let [r, g, b, _] = opaque(v, env);
    let [c1, c2, c3] = complements([r, g, b]);
    let at = [env.bounds.x as f32 + 0.5, env.bounds.y as f32 + 0.5];
    let mut layer = Layer::new(src.width, src.height);
    // approximation: GDI+ default pixel offset, round-join frame corners (color-complementary-colors.md)
    let swatch = |layer: &mut Layer, x: f32, y: f32, s: f32, c: [u8; 3]| {
        let Some(r) = Rect::from_xywh(at[0] + x, at[1] + y, s, s) else {
            return;
        };
        let path = PathBuilder::from_rect(r);
        layer.fill(&path, [c[0], c[1], c[2], 255], Transform::identity());
        let frame = Stroke {
            width: 1.0,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        layer.stroke(&path, &frame, BLACK);
    };
    if flag(v, "compact") {
        for (x, c) in [(5.0, [r, g, b]), (100.0, c1), (200.0, c2), (260.0, c3)] {
            swatch(&mut layer, x, 5.0, 50.0, c);
        }
        return layer.over(src);
    }
    // The table's order: a later swatch covers an earlier text's descenders.
    let caption = format!("(1 color){}(2 colors)", " ".repeat(16));
    let text = |x: f32, y: f32, s: &str, layer: &mut Layer| {
        let origin = [(at[0] + x) as f64, (at[1] + y) as f64];
        for p in glyph_run(s, SANS_FONT, pt_to_px(12.0), false, origin).outlines() {
            layer.fill(&p, BLACK, Transform::identity());
        }
    };
    text(
        0.0,
        0.0,
        "Primary Color (Use this for your primary design color)",
        &mut layer,
    );
    swatch(&mut layer, 5.0, 20.0, 75.0, [r, g, b]);
    text(
        0.0,
        100.0,
        "Complementary Colors: (Use these for accent colors)",
        &mut layer,
    );
    swatch(&mut layer, 5.0, 120.0, 75.0, c1);
    text(95.0, 150.0, "or", &mut layer);
    swatch(&mut layer, 135.0, 120.0, 75.0, c2);
    swatch(&mut layer, 215.0, 120.0, 75.0, c3);
    text(0.0, 200.0, &caption, &mut layer);
    layer.over(src)
}

/// A handle's canvas point: the pan value over the selection bounds, in f64, then f32.
// quirk(bb): photo-level-horizon-plumb-bob.md "Mapping the handles to canvas points"
fn handle(v: &Values, key: &str, env: &EffectEnv) -> [f32; 2] {
    let [x, y] = pan(v, key);
    let b = env.bounds;
    [
        (b.x as f64 + (x + 1.0) / 2.0 * b.w as f64) as f32,
        (b.y as f64 + (y + 1.0) / 2.0 * b.h as f64) as f32,
    ]
}

// quirk(bb): photo-level-horizon-plumb-bob.md "Reversed handles"
/// photo-level-horizon-plumb-bob.md "The correction angle": the rotation (degrees, positive
/// clockwise) and whether the line is plumb, by `HORIZON_SWAPPED_ATAN2`; coincident points give 0.
pub fn horizon_angle(start: [f32; 2], end: [f32; 2]) -> (f32, bool) {
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    if dx == 0.0 && dy == 0.0 {
        return (0.0, false);
    }
    HORIZON_SWAPPED_ATAN2(dx, dy)
}

/// A straight segment between two canvas points, with the Direct2D +0.5 offset.
fn segment(a: [f32; 2], b: [f32; 2]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a[0] + 0.5, a[1] + 0.5);
    pb.line_to(b[0] + 0.5, b[1] + 0.5);
    pb.finish()
}

/// The guideline's dashed stroke from `a` to `b` (Direct2D Dash: dashes and gaps of 2·width),
/// round-capped per `round_dash_caps` (Q76).
fn dashed_coverage(
    a: [f32; 2],
    b: [f32; 2],
    width: f32,
    round_dash_caps: bool,
    w: u32,
    h: u32,
) -> Vec<f32> {
    let Some(path) = segment(a, b) else {
        return vec![0.0; w as usize * h as usize];
    };
    let dash = 2.0 * width;
    let pen = |cap| Pen {
        width,
        join: Join::Miter,
        cap,
        miter_limit: 10.0,
        dash: Some([dash, dash]),
    };
    if round_dash_caps {
        return stroke_coverage(&path, &pen(Cap::Round), w, h, true);
    }
    let mut cov = stroke_coverage(&path, &pen(Cap::Flat), w, h, true);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return cov;
    }
    let u = [dx / len, dy / len];
    let phase = len % (2.0 * dash);
    // The first dash starts at `a`; the last one reaches `b` only when `b` falls inside a dash.
    let ends = [(a, [-u[0], -u[1]])]
        .into_iter()
        .chain((phase > 0.0 && phase <= dash).then_some((b, u)));
    for (p, out) in ends {
        let r = width as f64 / 2.0;
        let cap: Vec<[f64; 2]> = (0..=16)
            .map(|i| {
                let t = (i as f64 / 16.0 - 0.5) * std::f64::consts::PI;
                let (s, c) = t.sin_cos();
                [
                    p[0] as f64 + 0.5 + r * (c * out[0] as f64 - s * out[1] as f64),
                    p[1] as f64 + 0.5 + r * (c * out[1] as f64 + s * out[0] as f64),
                ]
            })
            .collect();
        if let Some(cap) = polygon_path(&[cap], [0.0; 2], true) {
            for (c, k) in cov.iter_mut().zip(fill_coverage(&cap, w, h, true)) {
                *c += k * (1.0 - *c);
            }
        }
    }
    cov
}

/// The step-1 label: the indicator, a space, the angle in `culture`, then "°". The caller passes
/// `plumb_seen` = the stored flag ∨ the current line's plumb.
// quirk(bb): photo-level-horizon-plumb-bob.md "Indicator quirk"
pub fn horizon_label(angle: f32, plumb_seen: bool, culture: &str) -> String {
    let arrow = if plumb_seen { '↕' } else { '↔' };
    format!(
        "{arrow} {}°",
        crate::number_format::format_angle(angle, culture)
    )
}

/// photo-level-horizon-plumb-bob.md "Text label": max(16, width ÷ 50) points.
pub fn horizon_label_points(canvas_width: u32) -> u32 {
    (canvas_width / 50).max(16)
}

/// R152 Q54: after every committed edit, OR the current classification into the dialog's
/// transient flag. Idempotent; it never clears it.
fn horizon_seen(v: &mut Values, env: &EffectEnv) {
    if horizon_angle(handle(v, "start", env), handle(v, "end", env)).1 {
        v.insert("plumbSeen".into(), json!(true));
    }
}

/// photo-level-horizon-plumb-bob.md "Behaviour": step 1 overlays the guidelines and label on
/// the source; step 2 scales then rotates the whole layer about the canvas centre.
fn horizon(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let (start, end) = (handle(v, "start", env), handle(v, "end", env));
    let (angle, plumb) = horizon_angle(start, end);
    // quirk(bb): photo-level-horizon-plumb-bob.md "OK on this tab"
    if num(v, "step") >= 1.0 {
        // approximation: degrees, positive clockwise (photo-level-horizon-plumb-bob.md Notes)
        let (cx, cy) = (src.w as f32 / 2.0, src.h as f32 / 2.0);
        let (sin, cos) = angle.to_radians().sin_cos();
        let zoom = num(v, "zoom") as f32;
        let (a, b) = (cos / zoom, sin / zoom);
        // Destination → source: centre + R(−angle)·(p − centre)/zoom.
        let inverse = [a, -b, b, a, cx - a * cx - b * cy, cy + b * cx - a * cy];
        let mut out = affine_cubic(src, inverse, Edge::Transparent);
        if flag(v, "preserve") {
            for (o, s) in out.px.iter_mut().zip(&src.px) {
                *o = source_over(*s, *o);
            }
        }
        return out;
    }
    let mut out = src.clone();
    let (w, h) = (src.w, src.h);
    // ponytail: one full-canvas coverage pass per stroke; clip to the strokes' bounds if large
    // previews lag.
    let mut paint = |cov: Vec<f32>, c: Px| {
        let c = colour_exact(c, info);
        for (p, k) in out.px.iter_mut().zip(cov) {
            if k > 0.0 {
                *p = source_over(*p, c.map(|v| v * k));
            }
        }
    };
    let pen = |width| Pen {
        width,
        join: Join::Miter,
        cap: Cap::Round,
        miter_limit: 10.0,
        dash: None,
    };
    if let Some(p) = segment(start, end) {
        paint(stroke_coverage(&p, &pen(4.0), w, h, true), WHITE);
    }
    paint(
        dashed_coverage(start, end, 2.0, HORIZON_DASH_ROUND_CAPS, w, h),
        BLACK,
    );
    for p in [start, end] {
        let arms = [
            segment([p[0] - 10.0, p[1]], [p[0] + 10.0, p[1]]),
            segment([p[0], p[1] - 10.0], [p[0], p[1] + 10.0]),
        ];
        for (width, c) in [(4.0, WHITE), (2.0, [255, 0, 0, 255])] {
            for arm in arms.iter().flatten() {
                paint(stroke_coverage(arm, &pen(width), w, h, true), c);
            }
        }
    }
    let label = horizon_label(angle, flag(v, "plumbSeen") || plumb, env.culture);
    // approximation: fixed 96 DPI for the UI-DPI point size (common.md §5.10)
    let px = pt_to_px(horizon_label_points(w) as f32);
    for (offset, c) in [(1.5, WHITE), (0.5, BLACK)] {
        paint(coverage(&label_paths(&label, px, offset), None, w, h), c);
    }
    out
}

/// The drawn projection of a Horizon label (`HORIZON_LABEL_GLYPH_ROUTE`): runs of consecutive
/// scalars that share a face, zero-advance controls left out.
fn label_runs(label: &str) -> Vec<(String, &'static str)> {
    let mut runs: Vec<(String, &'static str)> = vec![];
    for (c, face) in label.chars().filter_map(HORIZON_LABEL_GLYPH_ROUTE) {
        match runs.last_mut() {
            Some((text, f)) if *f == face => text.push(c),
            _ => runs.push((c.into(), face)),
        }
    }
    runs
}

/// The label's outlines at an em of `px`, its layout box at (`offset`, `offset`): each run laid
/// out after the previous one's advance, all aligned at their common ascent (Q51).
fn label_paths(label: &str, px: f32, offset: f32) -> Vec<Path> {
    let runs: Vec<_> = label_runs(label)
        .iter()
        .map(|(text, face)| glyph_run(text, face, px, false, [0.0; 2]))
        .collect();
    let ascent = runs.iter().map(|r| r.ascent).fold(0.0, f64::max);
    let mut x = 0.0;
    let mut paths = vec![];
    for run in &runs {
        let t = Transform::from_translate(x as f32 + offset, (ascent - run.ascent) as f32 + offset);
        paths.extend(run.outlines().into_iter().filter_map(|p| p.transform(t)));
        x += run.width;
    }
    paths
}

/// render-dimensions.md "Endpoints", "Direction" and the label's midpoint, in GDI+ pixel
/// coordinates (centres on integers).
#[derive(Debug, PartialEq)]
struct Dimension {
    /// The rounded (ties to even) start and end.
    start: [f32; 2],
    end: [f32; 2],
    /// ((S + E) / 2) with integer division.
    mid: [f32; 2],
    /// The direction of the unrounded points, radians, clockwise on screen.
    theta: f32,
}

fn dimension_points(v: &Values, env: &EffectEnv) -> Dimension {
    let b = env.bounds;
    let point = |key| {
        let [x, y] = pan(v, key);
        [
            b.x as f64 + (x + 1.0) / 2.0 * b.w as f64,
            b.y as f64 + (y + 1.0) / 2.0 * b.h as f64,
        ]
    };
    let (s, e) = (point("start"), point("end"));
    let theta = if s == e {
        0.0
    } else {
        (e[1] - s[1]).atan2(e[0] - s[0])
    };
    let (rs, re) = (s.map(f64::round_ties_even), e.map(f64::round_ties_even));
    let mid = [0, 1].map(|i| ((rs[i] + re[i]) as i64 / 2) as f32);
    Dimension {
        start: rs.map(|c| c as f32),
        end: re.map(|c| c as f32),
        mid,
        theta: theta as f32,
    }
}

/// render-dimensions.md "Behaviour": the shapes on a transparent layer, the label's halo and
/// fill, then the layer over the source.
fn dimensions(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let d = dimension_points(v, env);
    let w = num(v, "lineWidth") as f32;
    let colour = opaque(v, env);
    let (sin, cos) = d.theta.sin_cos();
    // Rotated by θ about `tip`; then the one GDI+ +0.5 grid translation.
    let about = |tip: [f32; 2], cos: f32, sin: f32| {
        Transform::from_row(cos, sin, -sin, cos, tip[0] + 0.5, tip[1] + 0.5)
    };
    let mut layer = Layer::new(src.width, src.height);
    let mut shaft = PathBuilder::new();
    shaft.move_to(d.start[0] + 0.5, d.start[1] + 0.5);
    shaft.line_to(d.end[0] + 0.5, d.end[1] + 0.5);
    if let Some(p) = shaft.finish() {
        let butt = Stroke {
            width: w,
            line_cap: LineCap::Butt,
            ..Stroke::default()
        };
        layer.stroke(&p, &butt, colour);
    }
    let polygon = |pts: [[f32; 2]; 3]| {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0][0], pts[0][1]);
        for p in &pts[1..] {
            pb.line_to(p[0], p[1]);
        }
        pb.close();
        pb.finish()
    };
    let k = 3.0 * w;
    for (tip, back) in [(d.start, k), (d.end, -k)] {
        if let Some(p) = polygon([[0.0, 0.0], [back, -k], [back, k]]) {
            layer.fill(&p, colour, about(tip, cos, sin));
        }
    }
    if flag(v, "ticks") {
        // 3w left of travel, 6w right of it, just outside each tip.
        for (tip, x0) in [(d.start, -w), (d.end, 0.0)] {
            if let Some(r) = Rect::from_xywh(x0, -k, w, 9.0 * w) {
                layer.fill(&PathBuilder::from_rect(r), colour, about(tip, cos, sin));
            }
        }
    }
    let label = text(v, "label");
    if !label.is_empty() {
        // approximation: the surface reports 96 DPI, em 4w px (render-dimensions.md Notes)
        let run = glyph_run(
            &DIMENSIONS_OUTLINE_PRESENTATION(label),
            SANS_FONT,
            pt_to_px(k),
            false,
            [0.0; 2],
        );
        let (sin, cos) = if flag(v, "rotate") {
            (sin, cos)
        } else {
            (0.0, 1.0)
        };
        let t =
            about(d.mid, cos, sin).pre_translate(-run.width as f32 / 2.0, -run.height as f32 / 2.0);
        let glyphs: Vec<Path> = run
            .outlines()
            .into_iter()
            .filter_map(|p| p.transform(t))
            .collect();
        layer.erase(&coverage(&glyphs, Some(k), src.width, src.height));
        for p in &glyphs {
            layer.fill(p, colour, Transform::identity());
        }
    }
    layer.over(src)
}

const PAN_UI: ControlUi = ControlUi {
    decimals: Some(3),
    pan_sliders: Some(SliderKeys {
        decimals: 3,
        small: 0.05,
        large: 0.25,
        exponential: false,
    }),
    ..ControlUi::DEFAULT
};
fn sans(_: &Values) -> Vec<(String, FaceStyle)> {
    vec![(SANS_FONT.into(), FaceStyle::Regular)]
}
static COMPLEMENTARY_UI: IndirectUi = IndirectUi {
    controls: &[("color", ControlUi::NO_HEADER)],
    fonts: sans,
    ..IndirectUi::DEFAULT
};
static HORIZON_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "step",
        tabs: &[
            Tab {
                caption: "Step 1: Mark Horizon",
                keys: &["start", "end"],
            },
            Tab {
                caption: "Step 2: Finalize",
                keys: &["zoom", "preserve"],
            },
        ],
    }],
    // spec-question: Q54 Horizon instance state lifetime
    transient_keys: &["plumbSeen"],
    controls: &[
        (
            "start",
            ControlUi {
                header_line: false,
                ..PAN_UI
            },
        ),
        (
            "end",
            ControlUi {
                note: Note::Text("Move + for rough placement. Use sliders for fine control."),
                ..PAN_UI
            },
        ),
        (
            "zoom",
            ControlUi {
                decimals: Some(3),
                ..ControlUi::NO_HEADER
            },
        ),
    ],
    on_change: horizon_seen,
    fonts: |_| {
        vec![
            (SANS_FONT.into(), FaceStyle::Regular),
            (HORIZON_INDICATOR_FONT.into(), FaceStyle::Regular),
        ]
    },
    ..IndirectUi::DEFAULT
};
static DIMENSIONS_UI: IndirectUi = IndirectUi {
    controls: &[
        ("start", PAN_UI),
        ("end", PAN_UI),
        (
            "color",
            ControlUi {
                reset: false,
                ..ControlUi::DEFAULT
            },
        ),
    ],
    fonts: sans,
    help: Help::Text(&["Dimensions v1.3"]),
    ..IndirectUi::DEFAULT
};

/// The three effects, in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let def = |id, title, controls, kind, ui| {
        let m = manifest(id).expect("registered text effect");
        EffectDef {
            id,
            icon: m.icon,
            category: m.category,
            controls,
            kind,
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(ui),
        }
    };
    let cpu = |render| EffectKind::Cpu {
        render,
        clip_to_selection: true,
    };
    let pan = |key, label, default, thumb| Control::Pan {
        key,
        label,
        min: -1.0,
        max: 1.0,
        default,
        step: 0.01,
        thumb,
    };
    let check = |key, label, default| Control::Checkbox {
        key,
        label,
        default,
    };
    let colour = Control::Color {
        key: "color",
        label: "",
        default: ColorDefault::Primary,
    };
    vec![
        def(
            "bbComplementaryColors",
            "Complementary Colors",
            vec![colour.clone(), check("compact", "Compact", false)],
            cpu(complementary),
            &COMPLEMENTARY_UI,
        ),
        def(
            "bbLevelHorizon",
            "Level Horizon/Plumb Bob",
            vec![
                Control::Value {
                    key: "step",
                    default: || json!(0),
                },
                Control::Value {
                    key: "plumbSeen",
                    default: || json!(false),
                },
                pan(
                    "start",
                    "Starting Position",
                    [-0.9, -0.33],
                    PanThumb::Source,
                ),
                pan("end", "Ending Position", [0.9, -0.33], PanThumb::Source),
                range("zoom", "\nZoom", 0.06, 16.0, 1.0)
                    .step(0.01)
                    .slider_keys(3, 0.05, 0.25, true)
                    .exponential(),
                check("preserve", "Preserve background", false),
            ],
            EffectKind::Gpu(GPU_CONTEXT, horizon),
            &HORIZON_UI,
        ),
        def(
            "bbDimensions",
            "Dimensions",
            vec![
                pan(
                    "start",
                    "Start Position",
                    [-0.9, 0.0],
                    PanThumb::SelectionCrop,
                ),
                pan("end", "End Position", [0.9, 0.0], PanThumb::SelectionCrop),
                range("lineWidth", "Line Width", 1.0, 100.0, 3.0),
                colour,
                Control::Text {
                    key: "label",
                    label: "Label",
                    default: "",
                    max_len: 255,
                    multiline: false,
                },
                check("rotate", "Rotate Label", true),
                check("ticks", "Ticks", true),
            ],
            cpu(dimensions),
            &DIMENSIONS_UI,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::conventions::{ALT_GPU_CONTEXT, SANS_FONT};
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::text::glyph_run;
    use crate::effect::bb::{indirect_defaults, ui::Help};
    use crate::effect::{Control, EffectKind, PanThumb, apply, find, flag};
    use crate::pixel::Px;
    use serde_json::json;

    const K: Px = [0, 0, 0, 255];
    const W: Px = [255; 4];

    fn hex(c: [u8; 3]) -> u32 {
        (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32
    }
    fn rgba(c: u32) -> Px {
        [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]
    }
    /// The pixels of [x0, x1] × [y0, y1].
    fn rect(x0: u32, y0: u32, x1: u32, y1: u32) -> impl Iterator<Item = (u32, u32)> {
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
    }
    /// A swatch at (x, y) of size s: the exact fill inside, the exact 1-px black frame on
    /// columns x, x + s and rows y, y + s; the four corners are don't-care.
    #[track_caller]
    fn assert_swatch(img: &ImageData, x: u32, y: u32, s: u32, fill: Px) {
        for (i, j) in rect(x + 1, y + 1, x + s - 1, y + s - 1) {
            assert_eq!(px(img, i, j), fill, "interior ({i}, {j})");
        }
        for k in 1..s {
            for (i, j) in [(x + k, y), (x + k, y + s), (x, y + k), (x + s, y + k)] {
                assert_eq!(px(img, i, j), K, "frame ({i}, {j})");
            }
        }
    }
    fn dark(p: Px) -> bool {
        p[..3].iter().all(|&c| c < 128) && p[3] > 128
    }

    /// color-complementary-colors.md Expected results 3 and 4 ("The four colours").
    #[test]
    fn complement_samples() {
        for (p, want) in [
            (0xFF0000, [0x00FF08, 0x7FFF00, 0x00FAFF]),
            (0x0000FF, [0xFFA100, 0xFF3B00, 0xFFBB00]),
            (0x808080, [0x000000, 0x333333, 0xE5E5E5]),
            (0xFFFFFF, [0x7F7F7F, 0xB2B2B2, 0x666666]),
            (0x000000, [0x7F7F7F, 0xB2B2B2, 0x666666]),
            (0xFFFF00, [0xFF00FA, 0x7B00FF, 0xFF007B]),
            (0x00FF00, [0xFF0000, 0xFF007B, 0xFF3B00]),
            (0xFF8000, [0x0055FF, 0x00FFCB, 0x5000FF]),
            (0x1E90FF, [0xFF7F1E, 0xFF351E, 0xFFB71E]),
            (0x646E6E, [0xD7EDED, 0x1E2121, 0xC0D3D3]),
            (0x646970, [0x6D6762, 0x6D6462, 0x6D6A62]),
        ] {
            let c = complements([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
            assert_eq!(c.map(hex), want, "{p:06X}");
        }
        // Spread 11 leaves the near-grey branch: the wheel moves the hue (≈ 180° here).
        let near = complements([0x64, 0x6F, 0x6F]);
        assert!(near[0][0] > near[0][1], "C1 {near:?} turned towards red");
    }

    /// color-complementary-colors.md "The four colours": the listed table and its ordered
    /// first-match lookup over the non-monotonic entries.
    #[test]
    fn ryb_table_first_match() {
        assert_eq!(
            (RYB_TO_RGB_HUE[0], RYB_TO_RGB_HUE[180], RYB_TO_RGB_HUE[360]),
            (0, 122, 360)
        );
        assert_eq!(
            RYB_TO_RGB_HUE[270..280],
            [269, 270, 270, 270, 272, 271, 272, 273, 273, 274]
        );
        assert_eq!(
            RYB_TO_RGB_HUE[320..330],
            [324, 325, 324, 326, 326, 326, 328, 328, 329, 328]
        );
        for (hue, angle) in [
            (0, 0),
            (270, 271),
            (272, 274),
            (324, 320),
            (325, 321),
            (326, 323),
            (329, 328),
            (359, 359),
        ] {
            assert_eq!(ryb_angle(hue), angle, "hue {hue}");
        }
    }

    /// color-complementary-colors.md Expected results 1.
    #[test]
    fn complementary_red_full_layout() {
        let canvas = solid(400, 300, W);
        let out = bb(
            "bbComplementaryColors",
            json!({"color": "#ff0000"}),
            &canvas,
        );
        for (x, y, c) in [
            (5, 20, 0xFF0000),
            (5, 120, 0x00FF08),
            (135, 120, 0x7FFF00),
            (215, 120, 0x00FAFF),
        ] {
            assert_swatch(&out, x, y, 75, rgba(c));
        }
        assert_eq!(px(&out, 5, 50), K);
        assert_eq!(px(&out, 350, 250), W);
        let ink = |x0, y0, x1, y1| rect(x0, y0, x1, y1).any(|(x, y)| dark(px(&out, x, y)));
        assert!(ink(0, 0, 399, 18), "primary title");
        assert!(ink(0, 100, 399, 118), "companion title");
        assert!(ink(95, 150, 115, 168), "or");
        assert!(
            !ink(84, 125, 130, 145) && !ink(84, 172, 130, 194),
            "only \"or\" between C1 and C2"
        );
        // The caption is one string: "(1 color)", 16 spaces, "(2 colors)".
        let first = glyph_run("(1 color)", SANS_FONT, 16.0, false, [0.0; 2]).width;
        let gap = glyph_run(&" ".repeat(16), SANS_FONT, 16.0, false, [0.0; 2]).width;
        let (a, b) = ((first + 2.0) as u32, (first + gap - 2.0) as u32);
        assert!(
            ink(0, 200, a - 3, 222) && ink(b + 3, 200, 399, 222),
            "both caption halves"
        );
        assert!(!ink(a, 200, b, 224), "16 spaces between them");
        // Nothing beyond the chart.
        assert!(
            rect(0, 228, 399, 299)
                .chain(rect(300, 125, 399, 199))
                .all(|(x, y)| px(&out, x, y) == W)
        );
    }

    /// color-complementary-colors.md Expected results 2.
    #[test]
    fn complementary_blue_compact() {
        let canvas = solid(400, 300, W);
        let out = bb(
            "bbComplementaryColors",
            json!({"color": "#0000ff", "compact": true}),
            &canvas,
        );
        let swatches = [
            (5, 0x0000FF),
            (100, 0xFFA100),
            (200, 0xFF3B00),
            (260, 0xFFBB00),
        ];
        for (x, c) in swatches {
            assert_swatch(&out, x, 5, 50, rgba(c));
        }
        let inside = |x: u32, y: u32| {
            (5..=55).contains(&y) && swatches.iter().any(|&(x0, _)| (x0..=x0 + 50).contains(&x))
        };
        assert!(
            rect(0, 0, 399, 299).all(|(x, y)| inside(x, y) || px(&out, x, y) == W),
            "no text anywhere"
        );
    }

    /// color-complementary-colors.md Expected results 3 through the chart, and controls 1–2:
    /// the default is the primary made opaque; the secondary is never used.
    #[test]
    fn complementary_grey_chart_and_default_colour() {
        let canvas = solid(400, 300, W);
        let grey = bb(
            "bbComplementaryColors",
            json!({"color": "#808080", "compact": true}),
            &canvas,
        );
        for (x, c) in [
            (5, 0x808080),
            (100, 0x000000),
            (200, 0x333333),
            (260, 0xE5E5E5),
        ] {
            assert_swatch(&grey, x, 5, 50, rgba(c));
        }
        let mut env = EffectEnv::whole(400, 300);
        env.primary = [0x12, 0x34, 0x56, 0x80];
        env.secondary = [0xFF, 0, 0, 255];
        let out = bb_env(
            "bbComplementaryColors",
            json!({"compact": true}),
            &canvas,
            &env,
        );
        assert_swatch(&out, 5, 5, 50, [0x12, 0x34, 0x56, 255]);
        // A translucent layer keeps its pixels wherever nothing is drawn.
        let clear = solid(400, 300, [10, 20, 30, 40]);
        let out = bb("bbComplementaryColors", json!({"compact": true}), &clear);
        assert_eq!(px(&out, 380, 290), [10, 20, 30, 40]);
        assert_eq!(px(&out, 30, 30), K);
    }

    /// color-complementary-colors.md Expected results 5.
    #[test]
    fn complementary_selection_offset() {
        let canvas = solid(400, 300, W);
        let (m, p) = rect_selection(400, 300, 100, 100, 250, 200);
        let env = with_selection(400, 300, &m, &p);
        let out = bb_env(
            "bbComplementaryColors",
            json!({"color": "#ff0000"}),
            &canvas,
            &env,
        );
        for (i, j) in rect(106, 121, 179, 194) {
            assert_eq!(px(&out, i, j), [255, 0, 0, 255], "({i}, {j})");
        }
        for k in 1..75 {
            assert_eq!(px(&out, 105, 120 + k), K);
            assert_eq!(px(&out, 105 + k, 120), K);
        }
        assert!(
            rect(100, 100, 249, 118).any(|(x, y)| dark(px(&out, x, y))),
            "title at (100, 100)"
        );
        let selected = |x, y| (100..250).contains(&x) && (100..200).contains(&y);
        assert!(rect(0, 0, 399, 299).all(|(x, y)| selected(x, y) || px(&out, x, y) == W));
    }

    #[test]
    fn complementary_metadata() {
        let d = find("bbComplementaryColors").unwrap();
        assert_eq!(d.dialog_title, Some("Complementary Colors"));
        assert!(matches!(
            d.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        assert!(matches!(
            d.controls[..],
            [
                Control::Color {
                    key: "color",
                    label: "",
                    default: crate::effect::ColorDefault::Primary
                },
                Control::Checkbox {
                    key: "compact",
                    label: "Compact",
                    default: false
                },
            ]
        ));
        let ui = d.indirect.unwrap();
        assert!(!ui.control("color").header_line && !ui.control("color").alpha);
        assert!(matches!(ui.help, Help::Window(_)));
        assert_eq!(
            (ui.fonts)(&Values::new()),
            [(SANS_FONT.to_string(), crate::fonts::FaceStyle::Regular)]
        );
    }

    /// R152 Q53: f32 swapped-axis atan2 in the exact operation order, all eight directions,
    /// both signed zeros, coincidence and the seam neighbours; no final normalization.
    #[test]
    fn horizon_angle_operation_order() {
        let o = [0.0, 0.0];
        for (end, angle, plumb) in [
            ([1.0, 0.0], 0.0, false),
            ([1.0, 1.0], 45.0, true),
            ([0.0, 1.0], 0.0, true),
            ([-0.0, 1.0], 0.0, true),
            ([-1.0, 1.0], -45.0, true),
            ([-1.0, 0.0], -180.0, false),
            ([-1.0, -0.0], -180.0, false),
            ([-1.0, -1.0], -135.0, true),
            ([0.0, -1.0], 180.0, true),
            ([-0.0, -1.0], 180.0, true),
            ([1.0, -1.0], 135.0, true),
            ([0.0, 0.0], 0.0, false),
            ([-0.0, -0.0], 0.0, false),
            // Near the upward seam a negative non-zero dx rounds the raw value to −270.
            ([-1e-20, -1.0], -180.0, true),
            ([1e-20, -1.0], 180.0, true),
        ] {
            let got = horizon_angle(o, end);
            assert_eq!(
                (got.0.to_bits(), got.1),
                ((angle as f32).to_bits(), plumb),
                "{end:?}"
            );
        }
        assert!(!horizon_angle(o, [1.0, 0.9999]).1 && horizon_angle(o, [1.0, 1.0001]).1);
        assert!(!horizon_angle(o, [-1.0, -0.9999]).1 && horizon_angle(o, [-1.0, -1.0001]).1);
        // The specified exact strings: exact points and f32-mapped control points.
        let pt = |x: f32, y: f32| [(x + 1.0) / 2.0 * 1000.0, (y + 1.0) / 2.0 * 1000.0];
        for (a, b, bits, plumb) in [
            ([50.0, 335.0], [950.0, 385.0], 0xc04b8260, false),
            (pt(-0.9, -0.33), pt(0.9, -0.23), 0xc04b8260, false),
            ([500.0, 50.0], [550.0, 950.0], 0x404b8260, true),
            (pt(0.0, -0.9), pt(0.1, 0.9), 0x404b8260, true),
        ] {
            let got = horizon_angle(a, b);
            assert_eq!((got.0.to_bits(), got.1), (bits, plumb), "{a:?} {b:?}");
        }
        // The handles map onto the selection bounds in f64, then f32 (Expected results 1–3).
        let env = EffectEnv::whole(1000, 1000);
        let v = indirect_defaults(&find("bbLevelHorizon").unwrap(), &env);
        assert_eq!(
            (handle(&v, "start", &env), handle(&v, "end", &env)),
            ([50.0, 335.0], [950.0, 335.0])
        );
    }

    /// photo-level-horizon-plumb-bob.md Expected results 1–3 and 6 labels, the indicator quirk,
    /// and the native fr-FR sequence (task-BB28-native-horizon-observations.md).
    #[test]
    fn horizon_labels() {
        let level = horizon_angle([50.0, 335.0], [950.0, 335.0]);
        let tilt = horizon_angle([50.0, 335.0], [950.0, 385.0]);
        let plumb = horizon_angle([500.0, 50.0], [550.0, 950.0]);
        let reversed = horizon_angle([950.0, 335.0], [50.0, 335.0]);
        let up = horizon_angle([500.0, 950.0], [500.0, 50.0]);
        let mut seen = false;
        for ((angle, now), en, fr) in [
            (level, "↔ 0°", "↔ 0°"),
            (tilt, "↔ -3.1798325°", "↔ -3,1798325°"),
            (plumb, "↕ 3.1798325°", "↕ 3,1798325°"),
            (level, "↕ 0°", "↕ 0°"),
            (reversed, "↕ -180°", "↕ -180°"),
            (up, "↕ 180°", "↕ 180°"),
        ] {
            seen |= now;
            assert_eq!(horizon_label(angle, seen, "en"), en);
            assert_eq!(horizon_label(angle, seen, "fr-FR"), fr);
        }
        assert_eq!(horizon_label(reversed.0, false, "en"), "↔ -180°");
        assert_eq!(horizon_label(tilt.0, false, "de-DE"), "↔ -3,1798325°");
        assert_eq!(horizon_label(tilt.0, false, "de-CH"), "↔ -3.1798325°");
        assert_eq!(
            horizon_label(tilt.0, true, "ar-EG"),
            "↕ \u{61c}-3\u{66b}1798325°"
        );
        assert_eq!(horizon_label(-0.0, false, "en"), "↔ 0°");
        for (w, pt) in [
            (1000, 20),
            (300, 16),
            (849, 16),
            (850, 17),
            (0, 16),
            (4000, 80),
        ] {
            assert_eq!(horizon_label_points(w), pt, "{w}");
        }
    }

    /// The gradient fixture: R = x, G = y.
    fn ramp(w: u32, h: u32) -> ImageData {
        from_fn(w, h, |x, y| [x as u8, y as u8, 120, 255])
    }

    /// photo-level-horizon-plumb-bob.md Expected results 1 at the stated 1000 × 1000, and the
    /// label size rule against a 300-wide canvas.
    #[test]
    fn horizon_defaults_step1() {
        let img = from_fn(1000, 1000, |x, y| {
            [(128 + x / 8) as u8, (128 + y / 8) as u8, 200, 255]
        });
        let out = bb("bbLevelHorizon", json!({}), &img);
        let red = [255, 0, 0, 255];
        for (x, y) in [
            (50, 330),
            (50, 340),
            (42, 335),
            (950, 330),
            (950, 340),
            (958, 335),
        ] {
            assert_px(&out, x, y, red);
        }
        let row: Vec<_> = (70..930).map(|x| px(&out, x, 335)).collect();
        let black = row
            .iter()
            .filter(|p| p[..3].iter().all(|&c| c < 40))
            .count();
        let white = row
            .iter()
            .filter(|p| p[..3].iter().all(|&c| c > 215))
            .count();
        // Q76: round caps on every 4-px dash leave 6 px of black and 2 px of white per period.
        assert!(
            black > 450 && white > 80,
            "dashes {black} black, {white} white"
        );
        assert!(
            (100..900).all(|x| px(&out, x, 334)[..3].iter().all(|&c| c > 100)),
            "white under the dashes"
        );
        // Untouched away from the line, the crosshairs and the label.
        assert!(
            rect(0, 360, 999, 999)
                .chain(rect(0, 40, 999, 310))
                .chain(rect(120, 0, 999, 39))
                .all(|(x, y)| px(&out, x, y) == px(&img, x, y))
        );
        // The label: black with a white shadow, 20 pt at 96 DPI, at the canvas top-left.
        let ink_rows = |out: &ImageData| {
            let rows: Vec<u32> = (0..60)
                .filter(|&y| (0..110).any(|x| dark(px(out, x, y))))
                .collect();
            (rows[0], *rows.last().unwrap())
        };
        assert!(
            rect(0, 0, 110, 40).any(|(x, y)| px(&out, x, y)[..3].iter().all(|&c| c > 240)),
            "shadow"
        );
        let (top, bottom) = ink_rows(&out);
        assert!(
            (6..=14).contains(&top) && (28..=30).contains(&bottom),
            "20 pt ink rows {top}..{bottom}"
        );
        let small = bb(
            "bbLevelHorizon",
            json!({}),
            &solid(300, 300, [200, 200, 200, 255]),
        );
        let (top, bottom) = ink_rows(&small);
        assert!(
            (4..=12).contains(&top) && (22..=24).contains(&bottom),
            "16 pt ink rows {top}..{bottom}"
        );
        // The indicator is a real arrow glyph in its own face, not the missing-glyph box.
        let arrow = glyph_run(
            "↔",
            super::super::conventions::HORIZON_INDICATOR_FONT,
            26.666666,
            false,
            [0.0; 2],
        );
        assert_ne!(arrow.layout.lines[0].glyphs[0].glyph_id, 0);
    }

    /// R175 Q77: in every number-format profile, positive and negative, plumb or not, each drawn
    /// scalar of the label is a real glyph of the face it is drawn with (no missing-glyph box).
    #[test]
    fn horizon_label_glyphs_in_every_profile() {
        let formats: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../assets/lang/number-formats.json"
        ))
        .unwrap();
        let cultures = formats["cultures"].as_object().unwrap();
        let profiles = formats["profiles"].as_array().unwrap().len();
        let mut codes: Vec<&str> = (0..profiles)
            .map(|i| {
                cultures
                    .iter()
                    .find(|(_, p)| p.as_u64() == Some(i as u64))
                    .unwrap()
                    .0
                    .as_str()
            })
            .collect();
        codes.push("en");
        let face = |name: &str| match name {
            SANS_FONT => crate::fonts::SELAWIK_REGULAR,
            _ => crate::fonts::CARLITO_REGULAR,
        };
        for code in codes {
            for angle in [3.1798325, -3.1798325, -180.0, -7.6293945e-6, 0.0] {
                for seen in [false, true] {
                    let label = horizon_label(angle, seen, code);
                    for (text, name) in label_runs(&label) {
                        let run = glyph_run(&text, name, 21.0, false, [0.0; 2]);
                        for (g, c) in run.layout.lines[0].glyphs.iter().zip(text.chars()) {
                            assert!(
                                g.glyph_id != 0 && g.face.as_ref() == face(name),
                                "{code} {label:?}: {c:?} (U+{:04X}) in {name}",
                                c as u32
                            );
                        }
                    }
                }
            }
        }
    }

    /// R175 Q77: rendered sv-SE and ar-EG labels draw a minus, a hyphen and a comma, never the
    /// tall missing-glyph box, where the sign and the decimal separator go.
    #[test]
    fn horizon_label_signs_render_without_boxes() {
        let em = pt_to_px(16.0);
        let width = |t: &str, f: &str| glyph_run(t, f, em, false, [0.0; 2]).width;
        // The ink rows of the black label within columns [x0, x1) (layout x, before +0.5); thin
        // strokes blend in linear light, so any visibly grey pixel counts.
        let ink_rows = |out: &ImageData, x0: f64, x1: f64| {
            let cols = (x0 + 1.5) as u32..(x1 - 0.5) as u32;
            (0..40)
                .filter(|&y| cols.clone().any(|x| px(out, x, y)[0] < 200))
                .count()
        };
        let canvas = solid(400, 400, W);
        let tilted = json!({"end": [0.9, -0.23]});
        let env = |c| EffectEnv {
            culture: c,
            ..EffectEnv::whole(400, 400)
        };
        let sv = bb_env("bbLevelHorizon", tilted.clone(), &canvas, &env("sv-SE"));
        let lead = width("↔", HORIZON_INDICATOR_FONT) + width(" ", SANS_FONT);
        let minus = width("\u{2212}", HORIZON_INDICATOR_FONT);
        let rows = ink_rows(&sv, lead, lead + minus);
        assert!((1..=4).contains(&rows), "sv-SE minus ink spans {rows} rows");
        let ar = bb_env("bbLevelHorizon", tilted, &canvas, &env("ar-EG"));
        let hyphen = width("-", SANS_FONT);
        let rows = ink_rows(&ar, lead, lead + hyphen);
        assert!((1..=4).contains(&rows), "ar-EG sign ink spans {rows} rows");
        let at = lead + width("-3", SANS_FONT);
        let rows = ink_rows(&ar, at, at + width(",", SANS_FONT));
        assert!(
            (1..=7).contains(&rows),
            "ar-EG separator ink spans {rows} rows"
        );
    }

    /// R174/R175/R152: the named conventions' arithmetic and both Q76 dash-cap branches. A
    /// 2-px dash runs 4 px from x = 10.5; its round cap reaches 15.5, a flat dash stops at 14.5;
    /// the line's own start cap is round either way, its end only when a dash reaches it.
    #[test]
    fn named_label_conventions() {
        assert_eq!(
            (
                DIMENSIONS_HALO_STROKE_ALPHA(255, 0.5),
                DIMENSIONS_HALO_STROKE_ALPHA(200, 1.0),
                DIMENSIONS_HALO_STROKE_ALPHA(9, 0.0)
            ),
            (128, 0, 9)
        );
        assert_eq!(
            DIMENSIONS_OUTLINE_PRESENTATION("a\nb"),
            "a\u{fe0e}\nb\u{fe0e}"
        );
        assert_eq!(HORIZON_SWAPPED_ATAN2(900.0, 50.0).0.to_bits(), 0xc04b8260);
        assert_eq!(
            "↕\u{61c}\u{200e}\u{200f}\u{2212}\u{66b}1"
                .chars()
                .map(HORIZON_LABEL_GLYPH_ROUTE)
                .collect::<Vec<_>>(),
            [
                Some(('↕', "Carlito")),
                None,
                None,
                None,
                Some(('\u{2212}', "Carlito")),
                Some((',', SANS_FONT)),
                Some(('1', SANS_FONT))
            ]
        );
        let at = |c: &[f32], x: usize| c[10 * 60 + x];
        let (round, flat) = (
            dashed_coverage([10.0, 10.0], [50.0, 10.0], 2.0, true, 60, 20),
            dashed_coverage([10.0, 10.0], [50.0, 10.0], 2.0, false, 60, 20),
        );
        assert!(
            at(&round, 15) > 0.2 && at(&flat, 15) == 0.0,
            "dash end caps"
        );
        assert!(
            at(&round, 9) > 0.2 && at(&flat, 9) > 0.2,
            "the line's start cap"
        );
        assert!(at(&round, 12) == 1.0 && at(&flat, 12) == 1.0 && at(&flat, 16) == 0.0);
        assert!(at(&flat, 50) == 0.0, "length 40 ends in a gap: no end cap");
        let ends_on = dashed_coverage([10.0, 10.0], [52.0, 10.0], 2.0, false, 60, 20);
        assert!(
            at(&ends_on, 53) > 0.0 && at(&ends_on, 54) == 0.0,
            "a dash reaching the end is capped"
        );
    }

    /// The darkness-weighted centroid along a column (`vertical`) or a row of a white image.
    fn centroid(img: &ImageData, at: u32, range: std::ops::Range<u32>, vertical: bool) -> f64 {
        let (mut sum, mut weight) = (0.0, 0.0);
        for i in range {
            let p = if vertical {
                px(img, at, i)
            } else {
                px(img, i, at)
            };
            let d = (255 - p[0]) as f64 * p[3] as f64 / 255.0;
            sum += d * i as f64;
            weight += d;
        }
        sum / weight
    }
    /// A white 200 × 200 image with a 3-px dark segment from `a` to `b`.
    fn segment(a: [f64; 2], b: [f64; 2]) -> ImageData {
        let mut img = solid(200, 200, W);
        for i in 0..=400 {
            let t = i as f64 / 400.0;
            let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
            for d in -1..=1 {
                let (x, y) = if (b[0] - a[0]).abs() > (b[1] - a[1]).abs() {
                    (x.round() as i32, y.round() as i32 + d)
                } else {
                    (x.round() as i32 + d, y.round() as i32)
                };
                let i = (y as usize * 200 + x as usize) * 4;
                img.data[i..i + 3].fill(0);
            }
        }
        img
    }

    /// photo-level-horizon-plumb-bob.md Expected results 2 (step 2) on a 200 × 200 stand-in:
    /// counter-clockwise about the canvas centre levels the marked line; corners transparent.
    #[test]
    fn horizon_tilted_rotation() {
        let tilted = json!({"step": 1, "end": [0.9, -0.23]});
        let line = bb(
            "bbLevelHorizon",
            tilted.clone(),
            &segment([10.0, 67.0], [190.0, 77.0]),
        );
        let (left, right) = (
            centroid(&line, 30, 40..110, true),
            centroid(&line, 170, 40..110, true),
        );
        assert!((left - right).abs() < 0.75, "levelled: {left} vs {right}");
        let img = ramp(200, 200);
        let out = bb("bbLevelHorizon", tilted, &img);
        assert_px(&out, 100, 100, px(&img, 100, 100));
        assert!(
            [(0, 0), (199, 0), (0, 199), (199, 199)]
                .iter()
                .all(|&(x, y)| px(&out, x, y)[3] == 0)
        );
        // Counter-clockwise: the right edge's samples come from below the centre line.
        let (s, c) = (-3.1798325f64).to_radians().sin_cos();
        for (x, y) in [(150, 100), (100, 40), (60, 160), (180, 30)] {
            let (dx, dy) = (x as f64 + 0.5 - 100.0, y as f64 + 0.5 - 100.0);
            let src = [100.0 + c * dx + s * dy - 0.5, 100.0 - s * dx + c * dy - 0.5];
            let p = px(&out, x, y);
            assert!(
                (p[0] as f64 - src[0]).abs() < 2.0 && (p[1] as f64 - src[1]).abs() < 2.0,
                "({x}, {y}) {p:?} ≈ {src:?}"
            );
        }
    }

    /// photo-level-horizon-plumb-bob.md Expected results 3: a clockwise step 2 makes the plumb
    /// line vertical.
    #[test]
    fn horizon_plumb_rotation() {
        let v = json!({"step": 1, "start": [0.0, -0.9], "end": [0.1, 0.9]});
        let out = bb("bbLevelHorizon", v, &segment([100.0, 10.0], [110.0, 190.0]));
        let (top, bottom) = (
            centroid(&out, 30, 70..140, false),
            centroid(&out, 170, 70..140, false),
        );
        assert!((top - bottom).abs() < 0.75, "vertical: {top} vs {bottom}");
    }

    /// photo-level-horizon-plumb-bob.md Expected results 4, in both GPU contexts, with a
    /// translucent source and selection clipping.
    #[test]
    fn horizon_preserve_background() {
        let v = json!({"step": 1, "end": [0.9, -0.23], "preserve": true});
        let img = ramp(200, 200);
        let keep = bb("bbLevelHorizon", v.clone(), &img);
        for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
            assert_eq!(px(&keep, x, y), px(&img, x, y));
        }
        assert!(keep.data.chunks(4).all(|q| q[3] == 255));
        let half = from_fn(200, 200, |x, y| [x as u8, y as u8, 120, 128]);
        for info in [GPU_CONTEXT_FOR_TESTS, ALT_GPU_CONTEXT] {
            let out = bb_ctx("bbLevelHorizon", info, v.clone(), &half);
            assert_px_tol(&out, 0, 0, [0, 0, 120, 128], 1);
            assert_px_tol(&out, 100, 100, [100, 100, 120, 192], 2);
        }
        let (m, p) = rect_selection(200, 200, 50, 50, 150, 150);
        let env = with_selection(200, 200, &m, &p);
        let out = bb_env(
            "bbLevelHorizon",
            json!({"step": 1, "end": [0.9, -0.23]}),
            &img,
            &env,
        );
        let inside = |x, y| (50..150).contains(&x) && (50..150).contains(&y);
        assert!(
            rect(0, 0, 199, 199).all(|(x, y)| inside(x, y) || px(&out, x, y) == px(&img, x, y))
        );
        assert_ne!(px(&out, 140, 100), px(&img, 140, 100));
    }
    const GPU_CONTEXT_FOR_TESTS: crate::effect::RenderInfo = super::super::conventions::GPU_CONTEXT;

    /// photo-level-horizon-plumb-bob.md Expected results 5: Zoom 2 shows the central half.
    #[test]
    fn horizon_zoom() {
        let img = ramp(200, 200);
        let z = bb("bbLevelHorizon", json!({"step": 1, "zoom": 2}), &img);
        assert_px(&z, 100, 100, px(&img, 100, 100));
        for i in (0..200).step_by(13) {
            let s = (100.0 + (i as f64 + 0.5 - 100.0) / 2.0 - 0.5).round() as u8;
            assert_px(&z, i, 77, [s, px(&z, i, 77)[1], 120, 255]);
            assert_px(&z, 77, i, [px(&z, 77, i)[0], s, 120, 255]);
        }
        assert_px_tol(&z, 20, 100, px(&img, 60, 100), 4);
    }

    /// photo-level-horizon-plumb-bob.md Expected results 6: reversed handles turn the photo by
    /// 180°.
    #[test]
    fn horizon_reversed_rotation() {
        let img = ramp(200, 200);
        let v = json!({"step": 1, "start": [0.9, -0.33], "end": [-0.9, -0.33]});
        let out = bb("bbLevelHorizon", v, &img);
        for (x, y) in rect(2, 2, 197, 197).step_by(97) {
            assert_px(&out, x, y, px(&img, 199 - x, 199 - y));
        }
    }

    /// photo-level-horizon-plumb-bob.md "Mapping the handles": selection-bound handles, while the
    /// label stays at the canvas top-left and step 2 pivots on the canvas centre; overlay in
    /// the alternate context too.
    #[test]
    fn horizon_selection_handles_canvas_label_and_pivot() {
        let img = ramp(300, 300);
        let (m, p) = rect_selection(300, 300, 0, 0, 200, 150);
        let env = with_selection(300, 300, &m, &p);
        let out = bb_env("bbLevelHorizon", json!({}), &img, &env);
        // Start (−0.9, −0.33) → (10, 50.25); end → (190, 50.25).
        assert_px(&out, 10, 44, [255, 0, 0, 255]);
        assert_px(&out, 190, 56, [255, 0, 0, 255]);
        assert!(
            rect(0, 0, 60, 35).any(|(x, y)| dark(px(&out, x, y))),
            "label at the canvas corner"
        );
        assert!(
            rect(200, 0, 299, 299)
                .chain(rect(0, 150, 199, 299))
                .all(|(x, y)| px(&out, x, y) == px(&img, x, y))
        );
        let z = bb_env("bbLevelHorizon", json!({"step": 1, "zoom": 2}), &img, &env);
        let s = (150.0 + (140.5 - 150.0) / 2.0 - 0.5f64).round() as u8;
        assert_px(&z, 140, 140, [s, s, 120, 255]);
        let alt = bb_ctx(
            "bbLevelHorizon",
            ALT_GPU_CONTEXT,
            json!({}),
            &ramp(200, 200),
        );
        assert_px(&alt, 10, 62, [255, 0, 0, 255]);
    }

    /// R152 Q54: the hook ORs the current plumb classification into the transient flag and
    /// never clears it; rendering reads the flag or the current classification.
    #[test]
    fn horizon_sticky_indicator_state() {
        let def = find("bbLevelHorizon").unwrap();
        let ui = def.indirect.unwrap();
        let env = EffectEnv::whole(200, 200);
        let mut v = indirect_defaults(&def, &env);
        (ui.on_change)(&mut v, &env);
        assert!(!flag(&v, "plumbSeen"));
        v.insert("start".into(), json!([0.0, -0.9]));
        v.insert("end".into(), json!([0.1, 0.9]));
        (ui.on_change)(&mut v, &env);
        assert!(flag(&v, "plumbSeen"));
        v.insert("start".into(), json!([-0.9, -0.33]));
        v.insert("end".into(), json!([0.9, -0.33]));
        for _ in 0..2 {
            (ui.on_change)(&mut v, &env);
            assert!(flag(&v, "plumbSeen"));
        }
        assert_eq!(ui.transient_keys, ["plumbSeen"]);
        let img = solid(200, 200, [90, 90, 90, 255]);
        let level = apply(&img, &def, &v, &env);
        v.insert("plumbSeen".into(), json!(false));
        assert_ne!(
            level.data,
            apply(&img, &def, &v, &env).data,
            "↕ differs from ↔"
        );
        // Explicit culture changes the label pixels; Expected results 2 in fr-FR.
        v.insert("end".into(), json!([0.9, -0.23]));
        let fr = EffectEnv {
            culture: "fr-FR",
            ..env
        };
        assert_ne!(
            apply(&img, &def, &v, &env).data,
            apply(&img, &def, &v, &fr).data
        );
    }

    /// R152 Q54: OK on step 1 burns in the sticky label but remembers only the step; Repeat
    /// renders a new indicator instance (the current line's classification) and recaptures the
    /// session culture.
    #[test]
    fn horizon_commit_keeps_step_and_repeat_starts_a_new_instance() {
        use crate::effect::session::{EffectSession, repeat_last};
        let mut app = crate::testing::fresh_doc(200, 200, None);
        let def = find("bbLevelHorizon").unwrap();
        let s = EffectSession::begin(&mut app, def.clone()).unwrap();
        assert_eq!(s.env().culture, crate::i18n::culture_code());
        let worker = s.clone();
        assert_eq!(worker.env().culture, s.env().culture);
        let source = s.source().clone();
        let mut v = s.defaults();
        v.insert("plumbSeen".into(), json!(true));
        let sticky = s.render(&v);
        assert!(s.commit(&mut app, v.clone()));
        assert_eq!(crate::testing::layer_bytes(&app, None), sticky.data);
        let token = &app.effect_tokens["bbLevelHorizon"].clone();
        assert!(!token.contains_key("plumbSeen") && token["step"] == json!(0));
        assert_eq!(app.last_effect_values.as_ref(), Some(token));
        app.undo();
        assert!(repeat_last(&mut app));
        let culture = crate::i18n::culture_code();
        let env = EffectEnv {
            culture: &culture,
            ..EffectEnv::whole(200, 200)
        };
        let fresh = apply(&source, &def, token, &env);
        assert_eq!(crate::testing::layer_bytes(&app, None), fresh.data);
        assert_ne!(fresh.data, sticky.data, "↔ after Repeat");
    }

    #[test]
    fn horizon_metadata() {
        let d = find("bbLevelHorizon").unwrap();
        assert_eq!(d.dialog_title, Some("Level Horizon/Plumb Bob"));
        assert!(
            matches!(d.kind, EffectKind::Gpu(info, _) if info == super::super::conventions::GPU_CONTEXT)
        );
        let keys: Vec<_> = d.controls.iter().map(Control::key).collect();
        assert_eq!(
            keys,
            ["step", "plumbSeen", "start", "end", "zoom", "preserve"]
        );
        let v = crate::effect::defaults(&d);
        assert_eq!(
            (v["step"].clone(), v["plumbSeen"].clone()),
            (json!(0), json!(false))
        );
        for (key, label, default) in [
            ("start", "Starting Position", [-0.9, -0.33]),
            ("end", "Ending Position", [0.9, -0.33]),
        ] {
            let c = d.controls.iter().find(|c| c.key() == key).unwrap();
            assert!(
                matches!(c, Control::Pan { label: l, min: -1.0, max: 1.0, default: dv, step: 0.01, thumb: PanThumb::Source, .. } if *l == label && *dv == default)
            );
        }
        let Some(Control::Range {
            label,
            min,
            max,
            default,
            step,
            exponential,
            keyboard,
            ..
        }) = d.controls.iter().find(|c| c.key() == "zoom")
        else {
            panic!()
        };
        assert_eq!(
            (*label, *min, *max, *default, *step, *exponential),
            ("\nZoom", 0.06, 16.0, 1.0, 0.01, true)
        );
        assert_eq!(
            (
                keyboard.decimals,
                keyboard.small,
                keyboard.large,
                keyboard.exponential
            ),
            (3, 0.05, 0.25, true)
        );
        assert!(matches!(
            d.controls[5],
            Control::Checkbox {
                key: "preserve",
                label: "Preserve background",
                default: false
            }
        ));
        let ui = d.indirect.unwrap();
        let strip = &ui.tabs[0];
        assert_eq!(strip.key, "step");
        assert_eq!(
            strip
                .tabs
                .iter()
                .map(|t| (t.caption, t.keys))
                .collect::<Vec<_>>(),
            [
                ("Step 1: Mark Horizon", &["start", "end"][..]),
                ("Step 2: Finalize", &["zoom", "preserve"][..])
            ]
        );
        let (start, end, zoom) = (ui.control("start"), ui.control("end"), ui.control("zoom"));
        assert!(!start.header_line && end.header_line && !zoom.header_line);
        assert!(matches!(
            end.note,
            super::super::ui::Note::Text(
                "Move + for rough placement. Use sliders for fine control."
            )
        ));
        for pan in [start, end] {
            let k = pan.pan_sliders.unwrap();
            assert_eq!(
                (pan.decimals, k.decimals, k.small, k.large, k.exponential),
                (Some(3), 3, 0.05, 0.25, false)
            );
        }
        assert_eq!(zoom.decimals, Some(3));
        assert!(matches!(ui.help, Help::Window(_)));
        let fonts = (ui.fonts)(&v);
        assert_eq!(
            fonts.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
            [SANS_FONT, "Carlito"]
        );
    }

    fn dims(v: serde_json::Value, w: u32, h: u32, env: &EffectEnv) -> ImageData {
        bb_env("bbDimensions", v, &solid(w, h, W), env)
    }
    fn black_env(w: u32, h: u32) -> EffectEnv<'static> {
        EffectEnv {
            primary: K,
            ..EffectEnv::whole(w, h)
        }
    }
    #[track_caller]
    fn assert_pixels(img: &ImageData, cases: &[(u32, u32, Px)]) {
        for &(x, y, want) in cases {
            assert_px_tol(img, x, y, want, 1);
        }
    }

    /// render-dimensions.md Expected results 1.
    #[test]
    fn dimensions_defaults() {
        let out = dims(json!({}), 200, 100, &black_env(200, 100));
        assert_pixels(
            &out,
            &[
                (100, 50, K),
                (100, 49, K),
                (100, 51, K),
                (100, 47, W),
                (100, 53, W),
                (17, 44, K),
                (15, 44, W),
                (8, 60, K),
                (8, 38, W),
                (8, 70, W),
                (192, 60, K),
                (195, 60, W),
                (5, 50, W),
                // The end arrowhead mirrors the start.
                (183, 44, K),
                (185, 44, W),
            ],
        );
        assert!(
            rect(0, 0, 199, 39)
                .chain(rect(0, 70, 199, 99))
                .chain(rect(0, 0, 5, 99))
                .chain(rect(194, 0, 199, 99))
                .all(|(x, y)| px(&out, x, y) == W)
        );
    }

    /// render-dimensions.md Expected results 2.
    #[test]
    fn dimensions_ticks_off() {
        let env = black_env(200, 100);
        let on = dims(json!({}), 200, 100, &env);
        let off = dims(json!({"ticks": false}), 200, 100, &env);
        assert_pixels(&off, &[(8, 60, W), (192, 60, W), (17, 44, K)]);
        let tick = |x: u32, y: u32| {
            (40..=69).contains(&y) && ((6..=11).contains(&x) || (189..=194).contains(&x))
        };
        assert!(rect(0, 0, 199, 99).all(|(x, y)| tick(x, y) || px(&on, x, y) == px(&off, x, y)));
    }

    /// render-dimensions.md Expected results 3.
    #[test]
    fn dimensions_vertical() {
        let v = dims(
            json!({"lineWidth": 2, "start": [0.0, -0.9], "end": [0.0, 0.9]}),
            100,
            200,
            &black_env(100, 200),
        );
        assert_pixels(
            &v,
            &[
                (50, 13, K),
                (46, 15, K),
                (54, 15, K),
                (50, 17, K),
                (43, 15, W),
                (40, 9, K),
                (58, 9, W),
                (40, 5, W),
                (50, 187, K),
                (40, 191, K),
                (58, 191, W),
                (40, 195, W),
                (50, 100, K),
                (52, 100, W),
            ],
        );
    }

    /// render-dimensions.md Expected results 4: the label "X" knocks the shaft out around it.
    #[test]
    fn dimensions_label_knockout() {
        let env = black_env(200, 100);
        let x = dims(json!({"label": "X"}), 200, 100, &env);
        assert_pixels(&x, &[(60, 50, K), (140, 50, K)]);
        let row: Vec<u8> = (0..200).map(|i| px(&x, i, 50)[0]).collect();
        let white = |r: std::ops::Range<usize>| r.into_iter().any(|i| row[i] > 240);
        let glyph = (94..107).filter(|&i| row[i] < 64).collect::<Vec<_>>();
        assert!(!glyph.is_empty(), "X glyph pixels on row 50: {row:?}");
        assert!(
            white(84..glyph[0]) && white(glyph[glyph.len() - 1] + 1..116),
            "white runs on both sides: {row:?}"
        );
        assert!(
            row[..84].iter().skip(70).all(|&c| c < 8) && row[116..131].iter().all(|&c| c < 8),
            "shaft beyond the halo"
        );
        // About 12 px em height: ink rows within the label box only.
        let rows: Vec<u32> = (30..70)
            .filter(|&y| (90..110).any(|i| y != 49 && y != 50 && y != 51 && dark(px(&x, i, y))))
            .collect();
        assert!(
            rows[0] >= 42 && *rows.last().unwrap() <= 58 && rows.len() >= 5,
            "{rows:?}"
        );
    }

    /// render-dimensions.md Expected results 5.
    #[test]
    fn dimensions_selection_bounds() {
        let (m, p) = rect_selection(300, 300, 100, 100, 200, 200);
        let env = EffectEnv {
            primary: K,
            ..with_selection(300, 300, &m, &p)
        };
        let s = dims(json!({}), 300, 300, &env);
        assert_pixels(
            &s,
            &[
                (150, 150, K),
                (103, 160, K),
                (112, 144, K),
                (101, 150, W),
                (196, 160, K),
                (188, 144, K),
            ],
        );
        let inside = |x, y| (100..200).contains(&x) && (100..200).contains(&y);
        assert!(rect(0, 0, 299, 299).all(|(x, y)| inside(x, y) || px(&s, x, y) == W));
    }

    /// render-dimensions.md "Endpoints", "Direction", "Label": ties-even anchors, the unrounded
    /// direction and the truncated midpoint.
    #[test]
    fn dimensions_anchor_geometry() {
        let env = EffectEnv::whole(21, 11);
        let v: Values =
            serde_json::from_value(json!({"start": [0.0, 0.0], "end": [0.1, 0.4]})).unwrap();
        let g = dimension_points(&v, &env);
        // (10.5, 5.5) → (10, 6); (11.55, 7.7) → (12, 8); θ from the unrounded points.
        assert_eq!(
            (g.start, g.end, g.mid),
            ([10.0, 6.0], [12.0, 8.0], [11.0, 7.0])
        );
        assert!((g.theta as f64 - (2.2f64).atan2(1.05)).abs() < 1e-6);
        let v: Values =
            serde_json::from_value(json!({"start": [0.0, 0.0], "end": [0.0, 0.0]})).unwrap();
        assert_eq!(dimension_points(&v, &env).theta, 0.0);
        let v: Values =
            serde_json::from_value(json!({"start": [0.1, 0.0], "end": [0.0, 0.0]})).unwrap();
        let g = dimension_points(&v, &env);
        assert_eq!(
            (g.start, g.end, g.mid),
            ([12.0, 6.0], [10.0, 6.0], [11.0, 6.0])
        );
        assert!((g.theta - std::f32::consts::PI).abs() < 1e-6, "reversed");
    }

    /// render-dimensions.md: coincident ends keep both full arrowheads and the ticks.
    #[test]
    fn dimensions_coincident_ends() {
        let out = dims(
            json!({"start": [0.0, 0.0], "end": [0.0, 0.0]}),
            200,
            100,
            &black_env(200, 100),
        );
        assert_pixels(
            &out,
            &[
                (105, 50, K),
                (95, 50, K),
                (101, 60, K),
                (99, 60, K),
                (100, 38, W),
                (110, 50, W),
                (90, 50, W),
            ],
        );
    }

    /// render-dimensions.md "Label": rotation along the line, upside down when reversed, and
    /// horizontal when "Rotate Label" is off. The ink centroid of "_" (on the baseline, so
    /// below the box centre) relative to M, the label's own pixels only.
    #[test]
    fn dimensions_label_orientation() {
        let ink = |extra: serde_json::Value, w: u32, h: u32| {
            let mut v = json!({"label": "_", "ticks": false});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let env = black_env(w, h);
            let with = dims(v.clone(), w, h, &env);
            v["label"] = json!("");
            let without = dims(v, w, h, &env);
            let (mut n, mut sx, mut sy) = (0.0, 0.0, 0.0);
            for (x, y) in rect(0, 0, w - 1, h - 1) {
                if px(&with, x, y)[0] < 200 && px(&without, x, y)[0] >= 200 {
                    (n, sx, sy) = (n + 1.0, sx + x as f64, sy + y as f64);
                }
            }
            assert!(n > 0.0);
            (sx / n - (w / 2) as f64, sy / n - (h / 2) as f64)
        };
        let (x, y) = ink(json!({}), 200, 100);
        assert!(x.abs() < 1.5 && y > 3.0, "forward: below M ({x}, {y})");
        let (x, y) = ink(json!({"start": [0.9, 0.0], "end": [-0.9, 0.0]}), 200, 100);
        assert!(
            x.abs() < 1.5 && y < -3.0,
            "reversed: upside down ({x}, {y})"
        );
        let vertical = json!({"start": [0.0, -0.9], "end": [0.0, 0.9]});
        let (x, y) = ink(vertical.clone(), 100, 200);
        assert!(
            x < -3.0 && y.abs() < 1.5,
            "rotated 90° clockwise ({x}, {y})"
        );
        let mut flat = vertical;
        flat["rotate"] = json!(false);
        let (x, y) = ink(flat, 100, 200);
        assert!(x.abs() < 1.5 && y > 3.0, "unrotated ({x}, {y})");
    }

    /// R152 Q55: the halo erases only the drawing layer (the source shows through, partial at
    /// its antialiased edge); the glyph fill comes after it.
    #[test]
    fn dimensions_halo_layer_only() {
        let v = json!({"label": "X"});
        let red = bb_env(
            "bbDimensions",
            v.clone(),
            &solid(200, 100, [255, 0, 0, 255]),
            &black_env(200, 100),
        );
        let clear = bb_env(
            "bbDimensions",
            v,
            &solid(200, 100, [0, 0, 0, 0]),
            &black_env(200, 100),
        );
        let halo: Vec<u32> = (80..120).filter(|&i| px(&clear, i, 50)[3] == 0).collect();
        assert!(!halo.is_empty());
        for &i in &halo {
            assert_eq!(
                px(&red, i, 50),
                [255, 0, 0, 255],
                "source under the halo at {i}"
            );
        }
        assert!(
            (70..120).any(|i| (1..255).contains(&px(&clear, i, 50)[3])),
            "partial halo edge"
        );
        let filled = |p: Px| p[3] > 200 && p[..3] == [0, 0, 0];
        assert!(
            (95..106).any(|i| filled(px(&clear, i, 50))),
            "glyph fill after the halo"
        );
        assert_eq!(px(&clear, 60, 50), K);
    }

    /// R152 Q52: a label renders through its outline projection, whatever its presentation; the
    /// stored value is untouched.
    #[test]
    fn dimensions_label_outline_projection() {
        let env = black_env(200, 100);
        let v = json!({"label": "\u{1f382}\u{fe0f}"});
        let out = dims(v.clone(), 200, 100, &env);
        assert!(
            rect(90, 40, 110, 60).any(|(x, y)| dark(px(&out, x, y))),
            "a visible outline"
        );
        let mut values = indirect_defaults(&find("bbDimensions").unwrap(), &env);
        values.extend(v.as_object().unwrap().clone());
        let before = values.clone();
        apply(
            &solid(200, 100, W),
            &find("bbDimensions").unwrap(),
            &values,
            &env,
        );
        assert_eq!(values, before);
    }

    #[test]
    fn dimensions_metadata() {
        let d = find("bbDimensions").unwrap();
        assert_eq!(d.dialog_title, Some("Dimensions"));
        assert!(matches!(
            d.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        let keys: Vec<_> = d.controls.iter().map(Control::key).collect();
        assert_eq!(
            keys,
            [
                "start",
                "end",
                "lineWidth",
                "color",
                "label",
                "rotate",
                "ticks"
            ]
        );
        for (c, label, default) in [
            (&d.controls[0], "Start Position", [-0.9, 0.0]),
            (&d.controls[1], "End Position", [0.9, 0.0]),
        ] {
            assert!(
                matches!(c, Control::Pan { label: l, min: -1.0, max: 1.0, default: dv, step: 0.01, thumb: PanThumb::SelectionCrop, .. } if *l == label && *dv == default)
            );
        }
        assert!(matches!(
            d.controls[2],
            Control::Range {
                label: "Line Width",
                min: 1.0,
                max: 100.0,
                default: 3.0,
                step: 1.0,
                exponential: false,
                ..
            }
        ));
        assert!(matches!(
            d.controls[3],
            Control::Color {
                label: "",
                default: crate::effect::ColorDefault::Primary,
                ..
            }
        ));
        assert!(matches!(
            d.controls[4],
            Control::Text {
                label: "Label",
                default: "",
                max_len: 255,
                multiline: false,
                ..
            }
        ));
        assert!(matches!(
            d.controls[5],
            Control::Checkbox {
                label: "Rotate Label",
                default: true,
                ..
            }
        ));
        assert!(matches!(
            d.controls[6],
            Control::Checkbox {
                label: "Ticks",
                default: true,
                ..
            }
        ));
        let ui = d.indirect.unwrap();
        assert!(!ui.control("color").reset && !ui.control("color").alpha);
        for key in ["start", "end"] {
            let k = ui.control(key).pan_sliders.unwrap();
            assert_eq!(
                (ui.control(key).decimals, k.decimals, k.small, k.large),
                (Some(3), 3, 0.05, 0.25)
            );
        }
        assert!(matches!(ui.help, Help::Text(["Dimensions v1.3"])));
        // The primary made opaque.
        let env = EffectEnv {
            primary: [0, 0, 255, 60],
            ..EffectEnv::whole(200, 100)
        };
        assert_px_tol(
            &dims(json!({}), 200, 100, &env),
            100,
            50,
            [0, 0, 255, 255],
            1,
        );
    }
}
