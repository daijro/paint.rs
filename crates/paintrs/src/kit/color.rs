//! The Paint.NET-drawn Colors-form controls: colour wheel, gradient sliders, swatches, the
//! primary/secondary rectangles and the colour-add icon, each rendered through a `DisplayTarget`
//! in device pixels and recorded as Paint.NET's pixels (R21 (c)).
//! Port of: js/src/interface/item/component/{ColorCircleItem,ColorSliderItem,ColorPreviewItem}.js
//! and js/src/interface/window/form/colors/ColorAddItem.js (boxes and positions), css/component.css
//! (`.color-slider`, `.color-circle`, `.color-preview`)
//! Reference: paintdotjs @ b4e6fc5
//! Task 29: the wheel's second nub and guides, the slider's pointer mapping, the
//! palette's press/release hit test, hover rings and add-mode edge.
//!
//! Interface notes (vs the plan): the controls take a `ColorLook` (the render mode, the checker
//! brightness, the scheme and the form's disabled state) instead of the bare `RenderMode`: the UI
//! checkerboards follow the canvas brightness (P2 step 2) and a disabled form greys them (P6).
//! egui's own `Ui::disable` would fade the pixels, so it is not used.
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use egui::{
    Color32, ColorImage, Id, PointerButton, Pos2, Rect, Response, Sense, TextureHandle,
    TextureOptions, Ui, Vec2,
};
use pdn_core::pixel::gamma::srgb_to_linear_f32;
use pdn_core::pixel::{IntRect, Px};
use pdn_core::render::checker::{checker_colors, checker_is_light, checker_square};
use pdn_core::render::color_ui::{
    Guide, PickMods, WheelGeometry, WidgetParams, disabled_gray, gradient_colour, notch_size,
    palette_hit, slider_col, slider_value, swatch_row_alpha, wheel_bitmap, wheel_geometry,
    wheel_guides, wheel_nub_centre, widget_inset,
};
use pdn_core::render::display::{DisplayTarget, RenderMode};
use pdn_core::render::thumbnail::{ring_alpha, ring_extent};
use pdn_core::util::color::{Color, HsvColor};

use super::aa::polygon_area_coverage;
use super::img;
use super::px;
use super::text::record_pdn_rect;
use crate::theme::*;

/// How the Colors-form controls render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorLook {
    /// `graphics.hardwareAcceleration` (R22): linear light or 8-bit sRGB blending.
    pub mode: RenderMode,
    /// `canvas.checkerboardBrightness` / 100: the UI checkerboards' colours (P2 step 2).
    pub checker_brightness: f32,
    /// Aero Dark (the Dark and Blue schemes): the colour-add icon's shadow colour (P3 step 5).
    pub dark: bool,
    /// The form is disabled (no document open): P6 greys the wheel, gradients, swatches and
    /// colours when the last document closes.
    pub disabled: bool,
}

impl ColorLook {
    fn key(&self) -> (u32, bool, bool, bool) {
        (
            self.checker_brightness.to_bits(),
            self.mode == RenderMode::Hardware,
            self.dark,
            self.disabled,
        )
    }

    /// P4/P6: the colour as drawn.
    fn colour(&self, c: Color) -> Px {
        let p = [c.red, c.green, c.blue, c.alpha];
        if self.disabled { disabled_gray(p) } else { p }
    }
}

fn px_of(c: Color32) -> Px {
    [c.r(), c.g(), c.b(), c.a()]
}

/// The Small checkerboard (P2) over `r`, its (0, 0) at `anchor` (target px).
fn fill_checker(t: &mut DisplayTarget, r: IntRect, anchor: [i32; 2], u: f64, brightness: f32) {
    let side = checker_square(u, false);
    let (light, dark) = checker_colors(brightness);
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let v = if checker_is_light(x - anchor[0], y - anchor[1], side) {
                light
            } else {
                dark
            };
            let c = t.to_space([v, v, v, 255]);
            t.set(x, y, c);
        }
    }
}

/// P4: `c` over `r`, row i at α = 1 + (A/255 − 1)·(i + ½)/h (solid when A = 255).
fn fill_colour(t: &mut DisplayTarget, r: IntRect, c: Px) {
    for i in 0..r.h {
        let a = swatch_row_alpha(i, r.h, c[3]);
        for x in r.x..r.right() {
            t.over_srgb(x, r.y + i as i32, [c[0], c[1], c[2], 255], a);
        }
    }
}

/// P5 over what `r` holds: column i samples the gradient at (i + ½)/w, sRGB values lerped, then
/// composited in the target's space (P1: linearized in Hardware).
fn paint_gradient(t: &mut DisplayTarget, r: IntRect, stops: &[Px]) {
    let hardware = t.mode == RenderMode::Hardware;
    for i in 0..r.w {
        let c = gradient_colour(stops, (i as f64 + 0.5) / r.w as f64);
        let a = c[3] / 255.0;
        let ch = |v: f32| {
            let v = v / 255.0;
            a * if hardware { srgb_to_linear_f32(v) } else { v }
        };
        let premul = [ch(c[0]), ch(c[1]), ch(c[2]), a];
        for y in r.y..r.bottom() {
            t.over(r.x + i as i32, y, premul);
        }
    }
}

/// A gradient strip of `w × h` px `top` px down its control: the Small checkerboard anchored at
/// the control's origin (P2 step 4), then the gradient of `stops` (`ui-drawn-graphics.md §2.4`
/// Geometry step 2), as `color_slider` draws it.
#[cfg(test)]
pub(crate) fn render_strip(
    stops: &[Px],
    w: u32,
    h: u32,
    top: i32,
    u: f64,
    brightness: f32,
    mode: RenderMode,
) -> Vec<u8> {
    let mut t = DisplayTarget::new(w, h, mode);
    let all = IntRect::new(0, 0, w, h);
    fill_checker(&mut t, all, [0, -top], u, brightness);
    paint_gradient(&mut t, all, stops);
    t.to_rgba8()
}

/// What the palette shows over its swatches: the pointer ring on one swatch at an opacity
/// (§2.6 Grid step 2 (3): 1 hot, 0.5 pressed) and the add-mode edge colour (step 6).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaletteMarks {
    pub ring: Option<(usize, f32)>,
    pub edge: Option<Px>,
}

/// §2.6 Grid steps 1–2 in a `w × h` target: the Small checkerboard under the `cols`-wide grid of
/// `cell`-px swatches (anchored at the grid origin; empty cells of the last row show it), then
/// each swatch, rect rounded (col·s, row·s, s, s), per P4. No border. Then `marks`: the ring (a
/// black 2-px stroke on the rect inset 1 and a white 1-px one on the rect inset 1.5, i.e. black
/// 0–1 px and white 1–2 px inside, at its opacity in the target's space) and the 1-px edge.
#[allow(clippy::too_many_arguments)]
fn render_swatches(
    colors: &[Px],
    cols: usize,
    cell: f64,
    w: u32,
    h: u32,
    u: f64,
    brightness: f32,
    mode: RenderMode,
    marks: PaletteMarks,
) -> Vec<u8> {
    let mut t = DisplayTarget::new(w, h, mode);
    let cols = cols.max(1);
    let edge = |i: usize| (i as f64 * cell).round() as i32;
    let rows = colors.len().div_ceil(cols);
    let grid = IntRect::new(0, 0, edge(cols) as u32, edge(rows) as u32);
    // calibrate: B-G4 — the checkerboard's anchor (P2 step 4, Q4).
    fill_checker(
        &mut t,
        grid.intersect(&IntRect::new(0, 0, w, h)),
        [0, 0],
        u,
        brightness,
    );
    let cell_rect = |i: usize| {
        let (col, row) = (i % cols, i / cols);
        let (x0, y0) = (edge(col), edge(row));
        IntRect::new(
            x0,
            y0,
            (edge(col + 1) - x0) as u32,
            (edge(row + 1) - y0) as u32,
        )
    };
    for (i, &c) in colors.iter().enumerate() {
        fill_colour(&mut t, cell_rect(i), c);
    }
    if let Some((i, a)) = marks.ring.filter(|(i, _)| *i < colors.len()) {
        let r = cell_rect(i);
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let d = (x - r.x)
                    .min(r.right() - 1 - x)
                    .min(y - r.y)
                    .min(r.bottom() - 1 - y);
                if d <= 1 {
                    t.over_srgb(x, y, [0, 0, 0, 255], a);
                }
                if d == 1 {
                    t.over_srgb(x, y, [255, 255, 255, 255], a);
                }
            }
        }
    }
    if let Some(c) = marks.edge {
        let (w, h) = (w as i32, h as i32);
        for y in 0..h {
            for x in 0..w {
                if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                    t.set(x, y, t.to_space(c));
                }
            }
        }
    }
    t.to_rgba8()
}

/// One `side`-px swatch over the checkerboard (the grid renderer with one cell).
#[cfg(test)]
pub(crate) fn render_swatch(
    c: Px,
    side: u32,
    u: f64,
    brightness: f32,
    mode: RenderMode,
) -> Vec<u8> {
    let marks = PaletteMarks::default();
    render_swatches(&[c], 1, side as f64, side, side, u, brightness, mode, marks)
}

/// quirk(pdn): ui-drawn-graphics.md §2.3 Nubs step 4 — the nub fill's 8-bit sRGB value goes to
/// the target unconverted: c/255 (premultiplied by `coverage`) is taken as linear light in
/// Hardware mode (so 127 shows as 187) and is the stored sRGB value in Software mode
/// (inferred; B-G7 confirms).
fn nub_fill(c: [u8; 3], coverage: f32) -> [f32; 4] {
    let v = |c: u8| c as f32 / 255.0 * coverage;
    [v(c[0]), v(c[1]), v(c[2]), coverage]
}

/// What a nub fill channel `c` shows as (the quirk's test hook).
#[cfg(test)]
pub(crate) fn nub_fill_display(c: u8, mode: RenderMode) -> u8 {
    let mut t = DisplayTarget::new(1, 1, mode);
    t.over(0, 0, nub_fill([c; 3], 1.0));
    t.to_rgba8()[0]
}

/// Exact-area coverage of polygon `points` (device px) at pixel (x, y): 0 outside its box.
fn coverage(points: &[[f64; 2]]) -> impl Fn(i32, i32) -> f64 + use<> {
    let pts: Vec<Pos2> = points
        .iter()
        .map(|p| Pos2::new(p[0] as f32, p[1] as f32))
        .collect();
    let (at, rows) = if pts.len() < 3 {
        (Pos2::ZERO, vec![])
    } else {
        polygon_area_coverage(&pts)
    };
    move |x, y| {
        let (i, j) = (x - at.x as i32, y - at.y as i32);
        if i < 0 || j < 0 {
            return 0.0;
        }
        rows.get(j as usize)
            .and_then(|r| r.get(i as usize))
            .copied()
            .unwrap_or(0.0)
    }
}

/// A circle as a 64-gon (`Q5`: analytic coverage of D2D's ellipses, ±2).
// calibrate: B-G5 — Direct2D's per-primitive AA of the nub circles, triangles and notch.
fn circle(c: [f64; 2], r: f64) -> Vec<[f64; 2]> {
    (0..64)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 64.0;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// Calls `f(x, y, coverage)` for the pixels of the ring between radii `r0` < `r1` around `c`.
fn ring(c: [f64; 2], r0: f64, r1: f64, mut f: impl FnMut(i32, i32, f32)) {
    let outer = coverage(&circle(c, r1));
    let inner = coverage(&circle(c, r0.max(0.0)));
    let (x0, y0) = ((c[0] - r1).floor() as i32, (c[1] - r1).floor() as i32);
    let (x1, y1) = ((c[0] + r1).ceil() as i32, (c[1] + r1).ceil() as i32);
    for y in y0..y1 {
        for x in x0..x1 {
            let a = outer(x, y) - if r0 > 0.0 { inner(x, y) } else { 0.0 };
            if a > 0.0 {
                f(x, y, a as f32);
            }
        }
    }
}

/// §2.3 Nubs steps 1–4: the disk of radius r = sw ÷ 2 filled with HSV(H, S, 100), a 1-px white
/// stroke on radius r − 1 and a 1-px black one on radius r (disabled: silver fill, #808080 ring),
/// at `opacity` (step 5).
fn paint_nub(
    t: &mut DisplayTarget,
    g: &WheelGeometry,
    hsv: HsvColor,
    disabled: bool,
    opacity: f32,
) {
    let c = wheel_nub_centre(g, hsv);
    let r = g.nub_radius as f64;
    let fill = if disabled {
        [192, 192, 192]
    } else {
        HsvColor::new(hsv.h, hsv.s, 100).to_rgb()
    };
    let edge: Px = if disabled {
        [128, 128, 128, 255]
    } else {
        [0, 0, 0, 255]
    };
    // The rings are linearized like any colour (P1); whether the disabled #808080 ring shares the
    // fill's no-lin() quirk is open (calibrate: B-G7).
    let k = opacity;
    ring(c, 0.0, r, |x, y, a| t.over(x, y, nub_fill(fill, a * k)));
    ring(c, r - 1.5, r - 0.5, |x, y, a| {
        t.over_srgb(x, y, [255, 255, 255, 255], a * k)
    });
    ring(c, r - 0.5, r + 0.5, |x, y, a| {
        t.over_srgb(x, y, edge, a * k)
    });
}

/// §2.3 Picking step 8: one guide in black at `alpha` (1-px strokes; D2D's flat line caps).
fn paint_guide(t: &mut DisplayTarget, guide: Guide, alpha: f32) {
    let black = [0, 0, 0, 255];
    match guide {
        Guide::Dot { c, r } => ring(c, 0.0, r, |x, y, a| t.over_srgb(x, y, black, a * alpha)),
        Guide::Circle { c, r } => ring(c, r - 0.5, r + 0.5, |x, y, a| {
            t.over_srgb(x, y, black, a * alpha)
        }),
        Guide::Line { from, to } => {
            let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
            let len = dx.hypot(dy);
            if len == 0.0 {
                return;
            }
            let n = [-dy / len * 0.5, dx / len * 0.5];
            let poly = [
                [from[0] + n[0], from[1] + n[1]],
                [to[0] + n[0], to[1] + n[1]],
                [to[0] - n[0], to[1] - n[1]],
                [from[0] - n[0], from[1] - n[1]],
            ];
            let cov = coverage(&poly);
            let (x0, x1) = (from[0].min(to[0]) - 1.0, from[0].max(to[0]) + 1.0);
            let (y0, y1) = (from[1].min(to[1]) - 1.0, from[1].max(to[1]) + 1.0);
            for y in y0.floor() as i32..y1.ceil() as i32 {
                for x in x0.floor() as i32..x1.ceil() as i32 {
                    let a = cov(x, y);
                    if a > 0.0 {
                        t.over_srgb(x, y, black, a as f32 * alpha);
                    }
                }
            }
        }
    }
}

/// `wheel_bitmap`, kept for the last (side, disabled) (§2.3 Bitmap: rebuilt only on resize or
/// enable change; the nub moves over it).
fn cached_wheel_bitmap(side: u32, disabled: bool) -> Arc<Vec<u8>> {
    type Kept = Option<((u32, bool), Arc<Vec<u8>>)>;
    static KEPT: Mutex<Kept> = Mutex::new(None);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    match &*kept {
        Some((key, bitmap)) if *key == (side, disabled) => bitmap.clone(),
        _ => {
            let bitmap = Arc::new(wheel_bitmap(side, disabled));
            *kept = Some(((side, disabled), bitmap.clone()));
            bitmap
        }
    }
}

/// What the wheel shows over its bitmap (§2.3 Nubs steps 5–6, Picking step 8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelView {
    /// The colour the wheel edits (colour 1) and its nub's opacity (0.5 during a right drag).
    pub main: (HsvColor, f32),
    /// The other colour (colour 2): its nub shows only during a right drag (opacity 0 = hidden).
    pub other: (HsvColor, f32),
    /// The guides for `mods` around the nub of `locked`, at `guide_opacity` (× black α 0.5).
    pub locked: HsvColor,
    pub mods: PickMods,
    pub guide_opacity: f32,
}

impl WheelView {
    /// One nub at `hsv`, no guides.
    pub fn of(hsv: HsvColor) -> Self {
        WheelView {
            main: (hsv, 1.0),
            other: (hsv, 0.0),
            locked: hsv,
            mods: PickMods::default(),
            guide_opacity: 0.0,
        }
    }

    fn key(&self) -> impl Hash + use<> {
        let bits = |v: f32| v.to_bits();
        (
            (self.main.0, bits(self.main.1)),
            (self.other.0, bits(self.other.1)),
            (self.locked, self.mods, bits(self.guide_opacity)),
        )
    }
}

/// §2.3 in a `w × h` target: the wheel bitmap of side L = min(w, h) over `backdrop` (linear light
/// in Hardware, P1), the guides, then the nubs of `view`. Pixels none of them touches stay
/// transparent (paintdotjs's canvas box overlaps the secondary colour).
fn render_wheel(
    w: u32,
    h: u32,
    view: &WheelView,
    disabled: bool,
    backdrop: Px,
    mode: RenderMode,
) -> Vec<u8> {
    let side = w.min(h);
    let mut t = DisplayTarget::new(w, h, mode);
    t.fill(IntRect::new(0, 0, w, h), backdrop);
    let bitmap = cached_wheel_bitmap(side, disabled);
    let mut touched = vec![false; (w * h) as usize];
    for (i, p) in bitmap.as_chunks::<4>().0.iter().enumerate() {
        if p[3] > 0 {
            let (x, y) = (i as u32 % side, i as u32 / side);
            t.over_srgb(x as i32, y as i32, *p, 1.0);
            touched[(y * w + x) as usize] = true;
        }
    }
    let g = wheel_geometry(side);
    // (centre, reach): the farthest pixel centre whose pixel still meets a drawn circle's edge.
    let half_diag = std::f64::consts::FRAC_1_SQRT_2;
    let mut kept = vec![];
    if view.guide_opacity > 0.0 {
        for guide in wheel_guides(&g, view.locked, view.mods) {
            paint_guide(&mut t, guide, 0.5 * view.guide_opacity);
        }
        // The rim dots reach R + 2.
        let c = g.centre as f64 + 0.5;
        kept.push(([c, c], g.radius as f64 + 2.0 + half_diag));
    }
    for (hsv, opacity) in [view.main, view.other] {
        if opacity > 0.0 {
            paint_nub(&mut t, &g, hsv, disabled, opacity);
            let reach = g.nub_radius as f64 + 0.5 + half_diag;
            kept.push((wheel_nub_centre(&g, hsv), reach));
        }
    }
    let mut rgba = t.to_rgba8();
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as f64 + 0.5, (i as u32 / w) as f64 + 0.5);
        let near = |&([cx, cy], reach): &([f64; 2], f64)| (x - cx).hypot(y - cy) <= reach;
        if !touched[i] && !kept.iter().any(near) {
            *px = [0; 4];
        }
    }
    rgba
}

/// §2.5 step 4: the inner polygon P of a `w × h` rectangle at inset `s` with an `nw × nh` notch
/// poking up from the bottom edge (consecutive duplicates dropped: no notch at k = 0).
fn widget_polygon(w: f64, h: f64, s: f64, nw: f64, nh: f64) -> Vec<[f64; 2]> {
    let cx = w / 2.0;
    let mut p = vec![
        [s, s],
        [w - s, s],
        [w - s, h - s],
        [cx + nw / 2.0, h - s],
        [cx, h - s - nh],
        [cx - nw / 2.0, h - s],
        [s, h - s],
    ];
    p.dedup();
    p
}

/// Polygon `p` (clockwise on screen) moved `d` outward with mitred joins.
fn outset(p: &[[f64; 2]], d: f64) -> Vec<[f64; 2]> {
    let n = p.len();
    let sub = |a: [f64; 2], b: [f64; 2]| [b[0] - a[0], b[1] - a[1]];
    let normal = |v: [f64; 2]| {
        let l = v[0].hypot(v[1]);
        [v[1] / l, -v[0] / l]
    };
    (0..n)
        .map(|i| {
            let (a, b, c) = (p[(i + n - 1) % n], p[i], p[(i + 1) % n]);
            let (d1, d2) = (sub(a, b), sub(b, c));
            let (n1, n2) = (normal(d1), normal(d2));
            let p1 = [a[0] + n1[0] * d, a[1] + n1[1] * d];
            let p2 = [b[0] + n2[0] * d, b[1] + n2[1] * d];
            let cross = d1[0] * d2[1] - d1[1] * d2[0];
            if cross.abs() < 1e-12 {
                return [b[0] + n1[0] * d, b[1] + n1[1] * d];
            }
            let t = ((p2[0] - p1[0]) * d2[1] - (p2[1] - p1[1]) * d2[0]) / cross;
            [p1[0] + d1[0] * t, p1[1] + d1[1] * t]
        })
        .collect()
}

/// §2.5 steps 1–7 for a `w × h` client: the Small checkerboard (control-anchored) in the client
/// inset by e = min(4, ⌈u⌉), the colour per P4, then black = client − P and white = (client
/// inset by o) − (P outset by i, mitred), each anti-aliased (exact area; §6 Q5). `p`: the state's
/// parameters (step 2; animated by the caller).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_preview(
    w: u32,
    h: u32,
    c: Px,
    p: WidgetParams,
    u: f64,
    brightness: f32,
    mode: RenderMode,
) -> Vec<u8> {
    let mut t = DisplayTarget::new(w, h, mode);
    let e = ring_extent(u);
    let area = IntRect::new(
        e as i32,
        e as i32,
        w.saturating_sub(2 * e),
        h.saturating_sub(2 * e),
    );
    fill_checker(&mut t, area, [0, 0], u, brightness);
    fill_colour(&mut t, area, c);
    let (nw, nh) = notch_size(&p, u);
    let poly = widget_polygon(w as f64, h as f64, widget_inset(&p), nw, nh);
    let inside = coverage(&poly);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let a = 1.0 - inside(x, y);
            if a > 0.0 {
                t.over_srgb(x, y, [0, 0, 0, 255], a as f32);
            }
        }
    }
    let inner = coverage(&outset(&poly, p.inner));
    let o = p.outer as i32;
    for y in o..h as i32 - o {
        for x in o..w as i32 - o {
            let a = 1.0 - inner(x, y);
            if a > 0.0 {
                t.over_srgb(x, y, [255, 255, 255, 255], a as f32);
            }
        }
    }
    t.to_rgba8()
}

/// §2.6 "Colour-add icon" in a `w × h` 8-bit sRGB bitmap over `backdrop`: the Small checkerboard
/// in the rect inset by e (anchored at the bitmap's origin), the colour (P4), then the P3 drop
/// shadow outside that rect (black, or white at α 192 in Aero Dark). The overlay icon follows.
fn render_color_add(w: u32, h: u32, c: Px, u: f64, look: &ColorLook, backdrop: Px) -> Vec<u8> {
    let mut t = DisplayTarget::new(w, h, RenderMode::Software);
    t.fill(IntRect::new(0, 0, w, h), backdrop);
    let e = ring_extent(u);
    let (rw, rh) = (w.saturating_sub(2 * e), h.saturating_sub(2 * e));
    let r = IntRect::new(e as i32, e as i32, rw, rh);
    fill_checker(&mut t, r, [0, 0], u, look.checker_brightness);
    fill_colour(&mut t, r, c);
    let shadow: Px = if look.dark {
        [255, 255, 255, 192]
    } else {
        [0, 0, 0, 255]
    };
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let a = ring_alpha(rw, rh, e, x - e as i32, y - e as i32);
            if a > 0 {
                t.over_srgb(x, y, shadow, a as f32 / 255.0);
            }
        }
    }
    t.to_rgba8()
}

#[derive(Clone)]
struct Rendered(u64, TextureHandle);

/// Paints the device-pixel image `render(w, h)` makes over `rect` (pixel-snapped), rendering
/// again only when `key` (or the size) changes; `rect` holds Paint.NET's pixels (R21 (c)).
fn paint_rendered(
    ui: &Ui,
    id: Id,
    rect: Rect,
    key: impl Hash,
    render: impl FnOnce(u32, u32) -> Vec<u8>,
) {
    let ppp = ui.ctx().pixels_per_point();
    let (w, h) = (
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    );
    if w == 0 || h == 0 {
        return;
    }
    let mut hasher = DefaultHasher::new();
    (key, w, h).hash(&mut hasher);
    let key = hasher.finish();
    let cached = ui
        .data(|d| d.get_temp::<Rendered>(id))
        .filter(|r| r.0 == key);
    let texture = match cached {
        Some(r) => r.1,
        None => {
            let rgba = render(w, h);
            let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            let texture =
                ui.ctx()
                    .load_texture(format!("colors/{id:?}"), image, TextureOptions::NEAREST);
            ui.data_mut(|d| d.insert_temp(id, Rendered(key, texture.clone())));
            texture
        }
    };
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
    record_pdn_rect(ui.ctx(), rect.intersect(ui.clip_rect()));
}

/// The id of the next control (as egui's widgets take one).
fn next_id(ui: &mut Ui) -> Id {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    id
}

/// A press and release egui saw in one frame (a touchpad tap): the click's button (left or
/// right) and position. `is_pointer_button_down_on` never held for it.
pub(crate) fn tap(ui: &Ui, response: &Response) -> Option<(PointerButton, Pos2)> {
    let button = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|&b| response.clicked_by(b))?;
    Some((button, super::inputs::interact_pos(ui, response)?))
}

/// The Colors-form controls take the pointer but are no Tab stops (paintdotjs's canvases and
/// `div`s: Tab walks the form's inputs only).
const POINTER: Sense = Sense::CLICK.union(Sense::DRAG);

/// `ColorCircleItem` with Paint.NET's wheel (§2.3) filling its box (`rect`, the 180×180 CSS box;
/// Ruling K-Q1: L = the box's device size, R 84 at 100 %) over the form background, showing
/// `view`. Returns the response and the wheel's geometry in device px from the snapped rect's
/// origin (the picking frame, §2.3 Picking).
pub fn color_wheel(
    ui: &mut Ui,
    rect: Rect,
    view: &WheelView,
    look: ColorLook,
) -> (Response, WheelGeometry) {
    let ppp = ui.ctx().pixels_per_point();
    let rect = px::snap_rect(rect, ppp);
    let id = next_id(ui);
    let response = ui.interact(rect, id, POINTER);
    let backdrop = px_of(crate::theme::current(ui.ctx()).window_background);
    let view = *view;
    let key = (view.key(), backdrop, look.key());
    paint_rendered(ui, id, rect, key, |w, h| {
        render_wheel(w, h, &view, look.disabled, backdrop, look.mode)
    });
    // The reference's nub (paintdotjs's cursor) overhangs the box where the circle meets its
    // edges: those strips hold the nub's pixels too (R21 (c)).
    let (o, r, c) = (COLOR_WHEEL_CURSOR_REACH, rect.width() / 2.0, rect.center());
    let half = (r * r - (r - o) * (r - o)).sqrt() + o;
    for strip in [
        Rect::from_min_max(
            Pos2::new(rect.max.x, c.y - half),
            Pos2::new(rect.max.x + o, c.y + half),
        ),
        Rect::from_min_max(
            Pos2::new(rect.min.x - o, c.y - half),
            Pos2::new(rect.min.x, c.y + half),
        ),
        Rect::from_min_max(
            Pos2::new(c.x - half, rect.min.y - o),
            Pos2::new(c.x + half, rect.min.y),
        ),
        Rect::from_min_max(
            Pos2::new(c.x - half, rect.max.y),
            Pos2::new(c.x + half, rect.max.y + o),
        ),
    ] {
        record_pdn_rect(ui.ctx(), strip.intersect(ui.clip_rect()));
    }
    let side = (rect.width().min(rect.height()) * ppp).round() as u32;
    (response, wheel_geometry(side))
}

/// The slider nub, apex at texture px `ax` and item-top `top` (texture px), scaled by `u`:
/// paintdotjs's `.cursor` shape (an 8×8 `clip-path` triangle 17 px down, its 4×5 `::before`
/// triangle 2 px in) in Paint.NET's colours (§2.4 step 5; §6 Q10: WindowText #000000 fill and
/// Window #FFFFFF outline, Highlight #0078D7 and HighlightText #FFFFFF while hot), linearized (P1).
fn slider_nub(ax: f64, top: f64, u: f64) -> ([[f64; 2]; 3], [[f64; 2]; 3]) {
    let (y, half, h) = COLOR_SLIDER_NUB;
    let (pad_x, pad_top, pad_bottom) = COLOR_SLIDER_NUB_PADDING;
    let s = |v: f32| v as f64 * u;
    let (y, half, h) = (top + s(y), s(half), s(h));
    let (pad_x, pad_top, pad_bottom) = (s(pad_x), s(pad_top), s(pad_bottom));
    let outer = [[ax, y], [ax + half, y + h], [ax - half, y + h]];
    let inner = [
        [ax, y + pad_top],
        [ax + half - pad_x, y + h - pad_bottom],
        [ax - half + pad_x, y + h - pad_bottom],
    ];
    (outer, inner)
}

/// `ColorSliderItem` (`rect` = its 85×25 box): Paint.NET's strip in the 85×10 canvas at top 7
/// (P5 + P1 over P2, no vertical opacity ramp; §2.4), and the nub under the column of `value`
/// (0…255, `slider_col` of the strip's device width). The texture spans the nub's overhang on
/// both sides. A press anywhere in the box sets `value` from the floored pointer (§2.4 step 4,
/// `slider_value` of the strip-relative device column); a drag updates it only when that column
/// changes; the response is marked changed then.
pub fn color_slider(
    ui: &mut Ui,
    rect: Rect,
    stops: &[Px],
    value: &mut u8,
    look: ColorLook,
) -> Response {
    let ppp = ui.ctx().pixels_per_point();
    let id = next_id(ui);
    let mut response = ui.interact(px::snap_rect(rect, ppp), id, POINTER);
    // Disabled, the slider takes no pointer highlight.
    let hot = !look.disabled && (response.hovered() || response.dragged());
    let (_, half, _) = COLOR_SLIDER_NUB;
    let area = px::snap_rect(rect.expand2(Vec2::new(half, 0.0)), ppp);
    let (top, strip_h) = COLOR_SLIDER_STRIP;
    let strip = px::snap_rect(
        Rect::from_min_size(
            rect.min + Vec2::new(0.0, top),
            Vec2::new(rect.width(), strip_h),
        ),
        ppp,
    );
    let backdrop = px_of(crate::theme::current(ui.ctx()).window_background);
    let stops: Vec<Px> = if look.disabled {
        stops.iter().map(|&p| disabled_gray(p)).collect()
    } else {
        stops.to_vec()
    };
    let d = |v: f32| (v * ppp).round() as i32;
    let strip_px = IntRect::new(
        d(strip.min.x) - d(area.min.x),
        d(strip.min.y) - d(area.min.y),
        (d(strip.max.x) - d(strip.min.x)) as u32,
        (d(strip.max.y) - d(strip.min.y)) as u32,
    );
    // The control's origin: the checkerboard's anchor (P2 step 4: the control crops it at the
    // strip's rect and draws it without offset).
    let control = [d(rect.min.x) - d(area.min.x), d(rect.min.y) - d(area.min.y)];
    // §2.4 step 4: the floored pointer column, strip-relative; a drag acts on a new column only.
    // A press and release in one frame (a tap) sets it once.
    let column = id.with("column");
    let down = !look.disabled && response.is_pointer_button_down_on();
    let last = ui.data(|m| m.get_temp::<i32>(column));
    let at = if down {
        response.interact_pointer_pos()
    } else if !look.disabled && last.is_none() {
        tap(ui, &response).map(|(_, p)| p)
    } else {
        None
    };
    let x = at.map(|p| (p.x * ppp).floor() as i32 - d(strip.min.x));
    if let Some(x) = x.filter(|&x| last != Some(x)) {
        let v = slider_value(x, strip_px.w);
        if v != *value {
            *value = v;
            response.mark_changed();
        }
    }
    if let Some(x) = x.filter(|_| down) {
        ui.data_mut(|m| m.insert_temp(column, x));
    } else if !down {
        ui.data_mut(|m| m.remove_temp::<i32>(column));
    }
    // The value's column in the strip (§2.4 step 3, strip-relative), the apex at its centre.
    let ax = strip_px.x as f64 + slider_col(*value, strip_px.w) as f64 + 0.5;
    let top = ((rect.min.y - area.min.y) * ppp) as f64;
    let (outer, inner) = slider_nub(ax, top, ppp as f64);
    let key = (
        &stops,
        *value,
        hot,
        backdrop,
        look.key(),
        strip_px.x,
        strip_px.y,
        control,
    );
    let u = ppp as f64;
    paint_rendered(ui, id, area, key, |w, h| {
        let mut t = DisplayTarget::new(w, h, look.mode);
        t.fill(IntRect::new(0, 0, w, h), backdrop);
        fill_checker(&mut t, strip_px, control, u, look.checker_brightness);
        paint_gradient(&mut t, strip_px, &stops);
        let (fill, line) = if hot {
            (SYSTEM_HIGHLIGHT, SYSTEM_HIGHLIGHT_TEXT)
        } else {
            (SYSTEM_WINDOW_TEXT, SYSTEM_WINDOW)
        };
        for (poly, colour) in [(outer, line), (inner, fill)] {
            let cov = coverage(&poly);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let a = cov(x, y);
                    if a > 0.0 {
                        t.over_srgb(x, y, px_of(colour), a as f32);
                    }
                }
            }
        }
        t.to_rgba8()
    });
    response
}

/// One primary/secondary `ColorPreviewItem`: `rect` is the client (the `.color-preview` box with
/// its border and outline, 43×43 at 100 %), drawn per §2.5 with the state parameters `p`.
pub fn color_preview(
    ui: &mut Ui,
    rect: Rect,
    color: Color,
    p: WidgetParams,
    look: ColorLook,
) -> Response {
    let ppp = ui.ctx().pixels_per_point();
    let rect = px::snap_rect(rect, ppp);
    let id = next_id(ui);
    let response = ui.interact(rect, id, POINTER);
    let c = look.colour(color);
    let p_key = [p.outer, p.middle, p.inner, p.notch_k].map(f64::to_bits);
    paint_rendered(ui, id, rect, (c, p_key, look.key()), |w, h| {
        let d = look;
        render_preview(w, h, c, p, ppp as f64, d.checker_brightness, d.mode)
    });
    response
}

/// The palette (§2.6) in `rect`: `cell`-px swatches, cols = ⌊W/s⌉, one
/// checkerboard anchored at `rect`'s origin; `edge` the add-mode blink colour (step 6). The
/// pointer ring shows on the hovered swatch (opacity 1), or at 0.5 on the pressed one while the
/// press is held over it (step 2 (3)). Returns a click — the same swatch (step 3 hit test, device
/// px) under the press and the release of the left or right button (step 4) — and its button.
/// Disabled, it takes no pointer.
pub fn swatch_grid(
    ui: &mut Ui,
    rect: Rect,
    colors: &[Color],
    cell: f32,
    look: ColorLook,
    edge: Option<Color32>,
) -> Option<(usize, PointerButton)> {
    let ppp = ui.ctx().pixels_per_point();
    let rect = px::snap_rect(rect, ppp);
    let id = next_id(ui);
    let response = ui.interact(rect, id, POINTER);
    let cols = (rect.width() / cell).round().max(1.0) as usize;
    let colors: Vec<Px> = colors.iter().map(|&c| look.colour(c)).collect();
    let s = (cell * ppp) as f64;
    let hit = |p: Pos2| {
        let d = (p - rect.min) * ppp;
        palette_hit(d.x as f64, d.y as f64, s, cols as u32, colors.len())
    };
    // The press: its swatch and button, kept until the release.
    let pressed_id = id.with("press");
    type Press = Option<(Option<usize>, PointerButton)>;
    let mut press: Press = ui.data(|d| d.get_temp::<Press>(pressed_id)).flatten();
    let had_press = press.is_some();
    let (origin, latest, released) = ui.input(|i| {
        let p = &i.pointer;
        let released = press.filter(|(_, b)| p.button_released(*b)).is_some();
        (p.press_origin(), p.latest_pos(), released)
    });
    let mut clicked = None;
    if look.disabled {
        press = None;
    } else if press.is_none() && response.is_pointer_button_down_on() {
        let right = ui.input(|i| i.pointer.button_down(PointerButton::Secondary));
        let button = if right {
            PointerButton::Secondary
        } else {
            PointerButton::Primary
        };
        press = Some((origin.and_then(hit), button));
    } else if let Some((i, button)) = press
        && (released || !ui.input(|i| i.pointer.button_down(button)))
    {
        if released && i.is_some() && latest.and_then(hit) == i {
            clicked = i.map(|i| (i, button));
        }
        press = None;
    }
    // A press and release in one frame (a tap): one swatch under both by definition.
    if !look.disabled
        && !had_press
        && press.is_none()
        && let Some((button, p)) = tap(ui, &response)
    {
        clicked = hit(p).map(|i| (i, button));
    }
    ui.data_mut(|d| d.insert_temp(pressed_id, press));
    let hovered = response.hover_pos().and_then(hit);
    let ring = match press {
        _ if look.disabled => None,
        Some((Some(i), _)) => (hovered == Some(i)).then_some((i, 0.5)),
        Some((None, _)) => None,
        None => hovered.map(|i| (i, 1.0)),
    };
    let marks = PaletteMarks {
        ring,
        edge: edge.map(px_of),
    };
    let ring_key = ring.map(|(i, a)| (i, a.to_bits()));
    paint_rendered(
        ui,
        id,
        rect,
        (
            &colors,
            cols,
            cell.to_bits(),
            look.key(),
            ring_key,
            marks.edge,
        ),
        |w, h| {
            let d = look;
            render_swatches(
                &colors,
                cols,
                s,
                w,
                h,
                ppp as f64,
                d.checker_brightness,
                d.mode,
                marks,
            )
        },
    );
    clicked
}

/// `#colorAdd`'s icon (§2.6): `rect` is the 16-DIP bitmap's box
/// (paintdotjs's `.color` box), `backdrop` the button's fill; the `color_add_overlay` icon over it.
pub fn color_add_icon(ui: &mut Ui, rect: Rect, color: Color, look: ColorLook, backdrop: Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let rect = px::snap_rect(rect, ppp);
    let (w, h) = (
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    );
    let bitmap = render_color_add(w, h, look.colour(color), ppp as f64, &look, px_of(backdrop));
    let under: Vec<Color32> = bitmap
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| Color32::from_rgb(p[0], p[1], p[2]))
        .collect();
    img::paint_icon_over(ui.painter(), "color_add_overlay.png", rect, &under);
    record_pdn_rect(ui.ctx(), rect.intersect(ui.clip_rect()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdn_core::render::color_ui::PickMods;
    use pdn_core::render::color_ui::{WidgetState, widget_params};

    /// ui-drawn-graphics.md §3 "Alpha strip, black, 67 px over dark 191": α 0.5 → 140 (HW).
    #[test]
    fn alpha_strip_blends_in_linear_light() {
        let stops: Vec<[u8; 4]> = (0..=255u8).map(|a| [0, 0, 0, a]).collect();
        // paintdotjs's strip, 7 px down its control; the checkerboard is control-anchored.
        let hw = render_strip(&stops, 85, 10, 7, 1.0, 0.75, RenderMode::Hardware);
        // Strip px 42 (t = 0.5): x = 42 → square 10 (4-px Small); strip row 0 = control row 7
        // (square row 1): (10 ⊕ 1) odd → light 191.
        let at = (42 * 4) as usize;
        assert_eq!(hw[at], 140);
        let sw = render_strip(&stops, 85, 10, 7, 1.0, 0.75, RenderMode::Software);
        assert_eq!(sw[at], 96);
        // Strip row 1 = control row 8 (square row 2): dark 143, as an anchor at the strip would
        // have it light.
        let dark = ((85 + 42) * 4) as usize;
        assert_eq!(hw[dark], 104);
    }

    /// §2.6 Grid step 2 (3): the hot ring is black 0–1 px and white 1–2 px inside the swatch;
    /// pressed, both at 0.5 (linear light); step 6: the add-mode edge is the control's outer px.
    #[test]
    fn palette_ring_and_edge() {
        let red = [255, 0, 0, 255];
        let render = |marks| {
            render_swatches(
                &[red, red],
                2,
                15.0,
                30,
                15,
                1.0,
                0.75,
                RenderMode::Hardware,
                marks,
            )
        };
        let px = |img: &[u8], x: u32, y: u32| {
            let i = ((y * 30 + x) * 4) as usize;
            [img[i], img[i + 1], img[i + 2]]
        };
        let hot = render(PaletteMarks {
            ring: Some((1, 1.0)),
            edge: None,
        });
        let cells: Vec<[u8; 3]> = [15, 16, 17].map(|x| px(&hot, x, 7)).to_vec();
        assert_eq!(cells, [[0, 0, 0], [255; 3], [255, 0, 0]]);
        assert_eq!(
            px(&hot, 14, 7),
            [255, 0, 0],
            "the other swatch is untouched"
        );
        let pressed = render(PaletteMarks {
            ring: Some((1, 0.5)),
            edge: None,
        });
        // Black at 0.5 over red: srgb(0.5) = 188; then white at 0.5 over that: linear (0.75,
        // 0.5, 0.5) → (225, 188, 188).
        assert_eq!(px(&pressed, 15, 7), [188, 0, 0]);
        assert_eq!(px(&pressed, 16, 7), [225, 188, 188]);
        let edge = render(PaletteMarks {
            ring: None,
            edge: Some([0, 0x78, 0xD7, 255]),
        });
        assert_eq!(
            [px(&edge, 0, 7), px(&edge, 29, 0), px(&edge, 1, 7)],
            [[0, 0x78, 0xD7], [0, 0x78, 0xD7], [255, 0, 0]]
        );
    }

    /// §2.3 Nubs step 5 and Picking step 8: during a right drag the other colour's nub shows (here
    /// at the +x rim) and the guides darken the wheel under them (a Shift spoke along +y).
    #[test]
    fn wheel_second_nub_and_guides() {
        let hsv = HsvColor::new(0, 0, 100);
        let base = WheelView::of(hsv);
        let img = |v: &WheelView| {
            render_wheel(180, 180, v, false, [40, 40, 40, 255], RenderMode::Hardware)
        };
        let px = |img: &[u8], x: u32, y: u32| img[((y * 180 + x) * 4) as usize..][..3].to_vec();
        let plain = img(&base);
        let view = WheelView {
            main: (hsv, 0.5),
            other: (HsvColor::new(0, 100, 100), 1.0),
            locked: hsv,
            mods: PickMods {
                shift: true,
                ..PickMods::default()
            },
            guide_opacity: 1.0,
        };
        let drag = img(&view);
        // The other nub: centre (173.5, 89.5); its white ring (radius 4) covers (169, 89).
        assert!(px(&plain, 169, 89)[1] < 60, "{:?}", px(&plain, 169, 89));
        assert!(px(&drag, 169, 89)[1] > 200, "{:?}", px(&drag, 169, 89));
        // The 90° spoke covers column 89 from y 92.5 down: half-black over the wheel's colour.
        let (before, after) = (px(&plain, 89, 130), px(&drag, 89, 130));
        assert!(
            after[0] < before[0] && after[0] > 150,
            "{before:?} → {after:?}"
        );
    }

    /// §3 "Swatch FF0000 α 128, 15 px, over checker": rows 0 and 7 over 143, row 14 over 191.
    #[test]
    fn translucent_swatch_gradient() {
        let img = render_swatch([255, 0, 0, 128], 15, 1.0, 0.75, RenderMode::Hardware);
        let px = |x: u32, y: u32| {
            let i = ((y * 15 + x) * 4) as usize;
            [img[i], img[i + 1], img[i + 2]]
        };
        // (0,0) and (4,7) lie in dark (143) squares, (0,14) in a light (191) one (4-px Small checker).
        assert_eq!(px(0, 0), [254, 14, 14]);
        assert_eq!(px(4, 7), [234, 74, 74]);
        assert_eq!(px(0, 14), [227, 137, 137]);
    }

    /// §2.5 at u = 1 in paintdotjs's 43-px box: normal = black | white | colour; checked = black |
    /// white | black | colour, with the 9 × 6.75 notch (white, black-edged) up from the bottom.
    #[test]
    fn preview_rings_and_notch() {
        let red = [255, 0, 0, 255];
        let px = |img: &[u8], x: u32, y: u32| {
            let i = ((y * 43 + x) * 4) as usize;
            [img[i], img[i + 1], img[i + 2]]
        };
        let n = render_preview(
            43,
            43,
            red,
            widget_params(WidgetState::Normal, 1.0),
            1.0,
            0.75,
            RenderMode::Hardware,
        );
        let row: Vec<[u8; 3]> = (0..4).map(|x| px(&n, x, 20)).collect();
        assert_eq!(row, [[0, 0, 0], [255; 3], [255, 0, 0], [255, 0, 0]]);
        assert_eq!(px(&n, 21, 40), [255, 0, 0], "no notch when normal");
        let c = render_preview(
            43,
            43,
            red,
            widget_params(WidgetState::Checked, 1.0),
            1.0,
            0.75,
            RenderMode::Hardware,
        );
        let row: Vec<[u8; 3]> = (0..4).map(|x| px(&c, x, 20)).collect();
        assert_eq!(row, [[0, 0, 0], [255; 3], [0, 0, 0], [255, 0, 0]]);
        // The notch: base 17…26 on the inner edge (y 40), apex at y 33.25 above x 21.5.
        assert_eq!(px(&c, 21, 39), [255; 3]);
        assert_eq!(px(&c, 21, 33)[1], 0, "the apex row is still mostly colour");
        assert_eq!(px(&c, 21, 31), [255, 0, 0]);
        assert_eq!(px(&c, 12, 39), [255, 0, 0]);
    }

    /// The wheel texture is transparent outside the wheel and its nub; L = min(w, h).
    #[test]
    fn wheel_is_transparent_outside_its_disk() {
        let img = render_wheel(
            180,
            181,
            &WheelView::of(HsvColor::new(0, 0, 100)),
            false,
            [40, 40, 40, 255],
            RenderMode::Hardware,
        );
        assert_eq!(img.len(), 180 * 181 * 4);
        let alpha = |x: u32, y: u32| img[((y * 180 + x) * 4 + 3) as usize];
        assert_eq!((alpha(0, 0), alpha(173, 89), alpha(174, 89)), (0, 255, 0));
        // The nub at the centre (S = 0): its black edge on radius 5 around (89.5, 89.5), over the
        // white centre.
        assert!(img[((89 * 180 + 94) * 4) as usize] < 40);
        assert_eq!(img[((89 * 180 + 89) * 4) as usize], 255);
    }

    /// P6 / §2.3 Nubs step 3 with no document open: the rim (255, 0, 0) shows as grey 151 and
    /// the nub's ring as #808080 (where the 1-px ring covers its pixel nearly whole, 128 + the
    /// sliver of the white ring under it).
    #[test]
    fn disabled_wheel_greys_the_rim_and_the_nub_ring() {
        let img = render_wheel(
            180,
            180,
            &WheelView::of(HsvColor::new(0, 0, 0)),
            true,
            [40, 40, 40, 255],
            RenderMode::Hardware,
        );
        let px = |x: u32, y: u32| {
            let i = ((y * 180 + x) * 4) as usize;
            [img[i], img[i + 1], img[i + 2]]
        };
        assert_eq!(px(173, 89), [151; 3]);
        // The nub at the centre (S = 0, r = 5): its ring through (94.5, 89.5).
        let [r, g, b] = px(94, 89);
        assert!(r == g && g == b && (128..=132).contains(&r), "{r}");
        // Enabled, the same pixels are red and black.
        let img = render_wheel(
            180,
            180,
            &WheelView::of(HsvColor::new(0, 0, 0)),
            false,
            [40, 40, 40, 255],
            RenderMode::Hardware,
        );
        assert_eq!(&img[(89 * 180 + 173) * 4..][..3], [255, 0, 0]);
        assert!(img[(89 * 180 + 94) * 4] < 40);
    }

    /// §2.3 Nubs step 4 / §3 "Nub-fill quirk": channels 64, 127, 191 display as 137, 187, 224.
    #[test]
    fn nub_fill_quirk_in_hardware_mode() {
        assert_eq!(
            [64u8, 127, 191].map(|c| nub_fill_display(c, RenderMode::Hardware)),
            [137, 187, 224]
        );
        assert_eq!(
            [64u8, 127, 191].map(|c| nub_fill_display(c, RenderMode::Software)),
            [64, 127, 191]
        );
    }
}

/// R22 / ui-drawn-graphics §2.1 P1: a translucent advanced-control fill over opaque chrome.
pub(crate) fn display_over(bg: Color32, fg: Color32, mode: RenderMode) -> Color32 {
    let mut target = DisplayTarget::new(1, 1, mode);
    target.fill(IntRect::new(0, 0, 1, 1), bg.to_srgba_unmultiplied());
    target.over_srgb(0, 0, fg.to_srgba_unmultiplied(), 1.);
    let p = target.to_rgba8();
    Color32::from_rgb(p[0], p[1], p[2])
}
#[cfg(test)]
mod display_fill_tests {
    use super::*;
    #[test]
    fn f16_and_8bit_highlight_vectors() {
        let bg = Color32::from_gray(191);
        let fg = Color32::from_black_alpha(128);
        assert_eq!(
            display_over(bg, fg, RenderMode::Hardware),
            Color32::from_gray(139)
        );
        assert_eq!(
            display_over(bg, fg, RenderMode::Software),
            Color32::from_gray(95)
        );
        let blue = Color32::from_rgba_unmultiplied(0, 120, 215, 102);
        let bg = Color32::from_gray(35);
        assert_eq!(
            display_over(bg, blue, RenderMode::Hardware),
            Color32::from_rgb(26, 82, 145)
        );
        assert_eq!(
            display_over(bg, blue, RenderMode::Software),
            Color32::from_rgb(21, 69, 107)
        );
    }
}
