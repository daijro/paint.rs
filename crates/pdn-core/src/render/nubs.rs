//! Tool handles (nubs): the drawings the Magic Wand, Move, Paint Bucket, Shapes and Text tools put
//! on the canvas.
//! Port of: js/src/engine/document/render/renderer/MoveNubRenderer.js (its role and the compass
//! glyph's outline; the sizes follow Paint.NET)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's handle drawings, sizes in device px and opacity layer; tiny-skia anti-aliasing
//! stands in for Direct2D's (B-C11 calibrates the drawings).

use std::sync::Arc;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

use super::display::{DisplayTarget, RenderMode, blend_over, to_space};
use super::view::ViewTransform;
use crate::pixel::IntRect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandleShape {
    Square,
    Circle,
    Compass,
    Reset { angle: f64, state: ResetState },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetState {
    Normal,
    Hot,
    Pressed,
    Disabled,
}

/// One handle at a canvas (image) point; `hover` 0…1 scales the drawing 1…1.4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub canvas: [f64; 2],
    pub shape: HandleShape,
    pub opacity: f32,
    pub hover: f32,
}

/// The drawing scale: u × (1 + 0.4·hover), clamped so a bad UI scale cannot ask for a huge layer.
fn unit(t: &ViewTransform, h: &Handle) -> f64 {
    let hover = if matches!(h.shape, HandleShape::Reset { .. }) || h.hover.is_nan() {
        0.0
    } else {
        h.hover.clamp(0.0, 1.0)
    };
    let u = if t.ui_scale.is_finite() {
        t.ui_scale.clamp(0.25, 16.0)
    } else {
        1.0
    };
    u * (1.0 + 0.4 * hover as f64)
}

/// §2.10 "Move tool": the hover growth (`Handle::hover` 0 → 1, i.e. ×1 → ×1.4) takes 0.2 s.
pub const HOVER_GROW_S: f64 = 0.2;

/// §2.10 "Opacity pulse": the opacity of the Move tool's handle `index` (in the order anchor, NW,
/// N, NE, E, SE, S, SW, W, translate) at `t` seconds — a 2 s sinusoid over 0.25…1.5, phase
/// index/10 of the period, clipped at 1.
// calibrate: B-C11 — the sinusoid's phase origin (sine, rising at t = 0).
pub fn pulse_opacity(index: usize, t: f64) -> f32 {
    let phase = t / 2.0 + index as f64 / 10.0;
    let v = 0.875 + 0.625 * (std::f64::consts::TAU * phase).sin();
    if v.is_finite() {
        v.min(1.0) as f32
    } else {
        1.0
    }
}

/// §2.10 radius in DIP: 3, the compass 6.5 (drawing and hit testing share it).
pub fn radius(shape: HandleShape) -> f64 {
    match shape {
        HandleShape::Compass => 6.5,
        _ => 3.0,
    }
}

/// Half the drawn extent in device px.
fn extent(shape: HandleShape, u: f64) -> f64 {
    let (r, th) = (radius(shape) * u, u);
    match shape {
        HandleShape::Circle => r + 1.25 * th,
        HandleShape::Square => r + (th + 2.5) / 2.0,
        HandleShape::Compass => r,
        HandleShape::Reset { angle, .. } => 10.0 * u * (angle.cos().abs() + angle.sin().abs()),
    }
}

/// Measured visual width in canvas units, before panel hit padding (§8.5.3).
pub(crate) fn canvas_width(t: &ViewTransform, h: &Handle) -> f64 {
    2.0 * extent(h.shape, unit(t, h)) / t.scale
}

/// The handle's device centre.
pub fn device_center(t: &ViewTransform, h: &Handle) -> [f64; 2] {
    [
        h.canvas[0] * t.scale + t.origin[0],
        h.canvas[1] * t.scale + t.origin[1],
    ]
}

/// The device pixels the drawing touches (plus 1 px of anti-aliasing), ∩ the viewport.
pub fn device_box(t: &ViewTransform, h: &Handle) -> Option<IntRect> {
    let c = device_center(t, h);
    if !(c[0].is_finite() && c[1].is_finite()) {
        return None;
    }
    let e = extent(h.shape, unit(t, h)) + 1.0;
    let edge = |v: f64| v.clamp(-1e9, 1e9) as i32;
    let (x0, y0) = (edge((c[0] - e).floor()), edge((c[1] - e).floor()));
    let (x1, y1) = (edge((c[0] + e).ceil()), edge((c[1] + e).ceil()));
    let r = IntRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32).intersect(&IntRect::new(
        0,
        0,
        t.viewport[0],
        t.viewport[1],
    ));
    (!r.is_empty()).then_some(r)
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn stroke(pm: &mut Pixmap, path: &tiny_skia::Path, width: f64, c: Color) {
    let s = Stroke {
        width: width as f32,
        ..Default::default()
    };
    pm.stroke_path(path, &paint(c), &s, Transform::identity(), None);
}

static RESET_ICON: std::sync::OnceLock<Pixmap> = std::sync::OnceLock::new();

/// The reset button's 16×16 DIP icon (any square size), supplied once at startup by the app
/// (paint.rs's SVG; it does not follow the selected pack); until then the button is drawn
/// without it.
pub fn set_reset_icon(icon: Pixmap) {
    let _ = RESET_ICON.set(icon);
}

// §8.5.3: bundled reset artwork and local button paint stand in for the uncalibrated
// Windows visual-style push button. Its geometry is 20×20 DIP, with a 16×16 DIP icon.
fn draw_reset(pm: &mut Pixmap, cx: f64, cy: f64, u: f64, angle: f64, state: ResetState) {
    let (sin, cos) = angle.sin_cos();
    let m = Transform::from_row(
        (u * cos) as f32,
        (u * sin) as f32,
        (-u * sin) as f32,
        (u * cos) as f32,
        cx as f32,
        cy as f32,
    );
    let (fill, border) = match state {
        ResetState::Normal => (
            Color::from_rgba8(240, 240, 240, 255),
            Color::from_rgba8(112, 112, 112, 255),
        ),
        ResetState::Hot => (
            Color::from_rgba8(229, 241, 251, 255),
            Color::from_rgba8(0, 120, 215, 255),
        ),
        ResetState::Pressed => (
            Color::from_rgba8(204, 228, 247, 255),
            Color::from_rgba8(0, 84, 153, 255),
        ),
        ResetState::Disabled => (
            Color::from_rgba8(240, 240, 240, 255),
            Color::from_rgba8(160, 160, 160, 255),
        ),
    };
    pm.fill_rect(
        Rect::from_xywh(-10.0, -10.0, 20.0, 20.0).unwrap(),
        &paint(fill),
        m,
        None,
    );
    let outline = PathBuilder::from_rect(Rect::from_xywh(-9.5, -9.5, 19.0, 19.0).unwrap());
    pm.stroke_path(
        &outline,
        &paint(border),
        &Stroke {
            width: 1.0,
            ..Default::default()
        },
        m,
        None,
    );
    let Some(icon) = RESET_ICON.get() else {
        return;
    };
    let icon_transform =
        Transform::from_scale(16.0 / icon.width() as f32, 16.0 / icon.height() as f32)
            .post_translate(-8.0, -8.0)
            .post_concat(m);
    let brush = tiny_skia::PixmapPaint {
        opacity: if state == ResetState::Disabled {
            0.4
        } else {
            1.0
        },
        quality: tiny_skia::FilterQuality::Bilinear,
        ..Default::default()
    };
    pm.draw_pixmap(0, 0, icon.as_ref(), &brush, icon_transform, None);
}

/// §2.10: radius r = 3u, thickness t = u (× the hover scale). Circle: a ring of diameter 2r stroked
/// white at 2.5t, then black at t. Square: side 2r, white at t + 2.5, then black at t. Compass:
/// r = 6.5u, a white square with a black outline and paintdotjs's 4-arrow glyph scaled to it.
/// The drawing goes to its own layer, composited at `opacity` (Paint.NET's D2D layer).
pub fn draw_handle(target: &mut DisplayTarget, t: &ViewTransform, h: &Handle) {
    if let Some(p) = HandlePainter::new(t, h, target.mode) {
        target.paint_rows(|y, x0, row| p.paint(y, x0, row));
    }
}

/// One handle's layer for a `mode` target: per pixel of `rect`, the premultiplied colour (target
/// space) composited over the canvas, zero where the drawing leaves it.
pub(crate) struct HandlePainter {
    rect: IntRect,
    px: Vec<[f32; 4]>,
    mode: RenderMode,
}

impl HandlePainter {
    /// `draw_handle`'s drawing, ready for any rows; `None` when it shows nothing.
    // calibrate: B-C11 — the handle drawings (the compass glyph is paintdotjs's outline).
    pub(crate) fn new(t: &ViewTransform, h: &Handle, mode: RenderMode) -> Option<Self> {
        let opacity = if h.opacity.is_nan() {
            0.0
        } else {
            h.opacity.clamp(0.0, 1.0)
        };
        device_box(t, h)?;
        if opacity == 0.0 {
            return None;
        }
        // The layer covers the whole drawing even where the viewport clips it, so the AA is
        // unchanged.
        let c = device_center(t, h);
        let u = unit(t, h);
        let e = extent(h.shape, u) + 1.0;
        let (x0, y0) = ((c[0] - e).floor(), (c[1] - e).floor());
        let size = ((c[0] + e).ceil() - x0, (c[1] + e).ceil() - y0);
        let mut pm = Pixmap::new(size.0 as u32, size.1 as u32)?;
        let (cx, cy) = (c[0] - x0, c[1] - y0);
        let (r, th) = (radius(h.shape) * u, u);
        let rect = |half: f64| {
            Rect::from_xywh(
                (cx - half) as f32,
                (cy - half) as f32,
                (2.0 * half) as f32,
                (2.0 * half) as f32,
            )
        };
        match h.shape {
            HandleShape::Reset { angle, state } => draw_reset(&mut pm, cx, cy, u, angle, state),
            HandleShape::Circle => {
                if let Some(p) = PathBuilder::from_circle(cx as f32, cy as f32, r as f32) {
                    stroke(&mut pm, &p, 2.5 * th, Color::WHITE);
                    stroke(&mut pm, &p, th, Color::BLACK);
                }
            }
            HandleShape::Square => {
                if let Some(p) = rect(r).map(PathBuilder::from_rect) {
                    stroke(&mut pm, &p, th + 2.5, Color::WHITE);
                    stroke(&mut pm, &p, th, Color::BLACK);
                }
            }
            HandleShape::Compass => {
                let big = r;
                if let Some(fill) = rect(big) {
                    pm.fill_rect(fill, &paint(Color::WHITE), Transform::identity(), None);
                }
                if let Some(p) = rect(big - th / 2.0).map(PathBuilder::from_rect) {
                    stroke(&mut pm, &p, th, Color::BLACK);
                }
                // paintdotjs drawCompass: inner = R − 3.5, narrow 0.5, shoulder 2 (× u here).
                let (inner, narrow, shoulder) = (big - 3.5 * u, 0.5 * u, 2.0 * u);
                let quarter = [
                    (-big, 0.0),
                    (-inner, shoulder),
                    (-inner, narrow),
                    (-narrow, narrow),
                    (-narrow, inner),
                    (-shoulder, inner),
                ];
                let mut pb = PathBuilder::new();
                // Four quarter turns of the left arrow give the whole glyph.
                for k in 0..4 {
                    for (i, &(x, y)) in quarter.iter().enumerate() {
                        // Clockwise on screen: left, bottom, right, top.
                        let (x, y) = match k {
                            0 => (x, y),
                            1 => (y, -x),
                            2 => (-x, -y),
                            _ => (-y, x),
                        };
                        let p = ((cx + x) as f32, (cy + y) as f32);
                        if k == 0 && i == 0 {
                            pb.move_to(p.0, p.1);
                        } else {
                            pb.line_to(p.0, p.1);
                        }
                    }
                }
                pb.close();
                if let Some(p) = pb.finish() {
                    pm.fill_path(
                        &p,
                        &paint(Color::BLACK),
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        let px = pm
            .pixels()
            .iter()
            .map(|px| {
                let a = px.alpha();
                if a == 0 {
                    return [0.0; 4];
                }
                let un = |v: u8| ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
                let coverage = a as f32 / 255.0 * opacity;
                let srgb = [un(px.red()), un(px.green()), un(px.blue()), 255];
                to_space(mode, srgb).map(|c| c * coverage)
            })
            .collect();
        let rect = IntRect::new(x0 as i32, y0 as i32, pm.width(), pm.height());
        Some(HandlePainter { rect, px, mode })
    }

    /// Composites the layer over device row `y`'s pixels `row`, which start at column `x0`.
    pub(crate) fn paint(&self, y: i32, x0: i32, row: &mut [[f32; 4]]) {
        let r = self.rect;
        if !(r.y..r.bottom()).contains(&y) {
            return;
        }
        let base = (y - r.y) as usize * r.w as usize;
        for x in r.x.max(x0)..r.right().min(x0 + row.len() as i32) {
            let s = self.px[base + (x - r.x) as usize];
            blend_over(self.mode, &mut row[(x - x0) as usize], s);
        }
    }
}

/// The brush tools' outline (painting-tools-1.md §3.14 step 5): a circle of `radius` canvas px
/// (Size/2) at the pointer centre `canvas`, × `pressure` (the raw pen pressure, `Some` only while
/// a button is down), at `opacity`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPreview {
    pub canvas: [f64; 2],
    pub radius: f64,
    pub pressure: Option<f64>,
    pub opacity: f32,
}

/// The pens in device px: white 2.5 under black 1.
const PREVIEW_PENS: (f64, f64) = (2.5, 1.0);

impl BrushPreview {
    /// The device centre and radius; `None` when nothing shows.
    fn geometry(&self, t: &ViewTransform) -> Option<([f64; 2], f64)> {
        let c = [
            self.canvas[0] * t.scale + t.origin[0],
            self.canvas[1] * t.scale + t.origin[1],
        ];
        let r = self.radius * t.scale * self.pressure.unwrap_or(1.0);
        let shown =
            self.opacity > 0.0 && c.iter().chain([&r]).all(|v| v.is_finite() && v.abs() < 1e7);
        (shown && r >= 0.0).then_some((c, r))
    }

    /// The device pixels the outline touches, ∩ the viewport.
    pub fn device_box(&self, t: &ViewTransform) -> Option<IntRect> {
        let (c, r) = self.geometry(t)?;
        let e = r + PREVIEW_PENS.0 / 2.0 + 1.0;
        let (x0, y0) = ((c[0] - e).floor() as i32, (c[1] - e).floor() as i32);
        let (x1, y1) = ((c[0] + e).ceil() as i32, (c[1] + e).ceil() as i32);
        let b = IntRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32).intersect(&IntRect::new(
            0,
            0,
            t.viewport[0],
            t.viewport[1],
        ));
        (!b.is_empty()).then_some(b)
    }
}

/// `BrushPreview`'s rows for a `mode` target, computed per row (a 2000-px brush stays cheap):
/// each pen is a ring whose coverage is its radial band box-filtered over one pixel.
// calibrate: B-C11 — Direct2D's AA of the outline (an analytic radial approximation here, where
// the handles use tiny-skia: the ring can span thousands of px).
pub(crate) struct BrushPreviewPainter {
    c: [f64; 2],
    r: f64,
    white: [f32; 4],
    opacity: f32,
    mode: RenderMode,
}

impl BrushPreviewPainter {
    pub(crate) fn new(t: &ViewTransform, p: &BrushPreview, mode: RenderMode) -> Option<Self> {
        let (c, r) = p.geometry(t)?;
        Some(BrushPreviewPainter {
            c,
            r,
            white: to_space(mode, [255; 4]),
            opacity: p.opacity.min(1.0),
            mode,
        })
    }

    pub(crate) fn paint(&self, y: i32, x0: i32, row: &mut [[f32; 4]]) {
        let (white_pen, black_pen) = PREVIEW_PENS;
        let dy = y as f64 + 0.5 - self.c[1];
        let outer = self.r + white_pen / 2.0 + 0.5;
        if dy.abs() >= outer {
            return;
        }
        let inner = self.r - white_pen / 2.0 - 0.5;
        let half = (outer * outer - dy * dy).sqrt();
        let band = |d: f64, w: f64| {
            let edge = |e: f64| (e - d + 0.5).clamp(0.0, 1.0);
            edge(self.r + w / 2.0) - edge(self.r - w / 2.0)
        };
        let start = ((self.c[0] - half).floor() as i32).max(x0);
        let end = ((self.c[0] + half).ceil() as i32).min(x0 + row.len() as i32);
        let all = start..end.max(start);
        // The pixels whose centres lie inside the inner circle get no ink: skip them.
        let hole = if inner > dy.abs() {
            let h = (inner * inner - dy * dy).sqrt();
            ((self.c[0] - h - 0.5).ceil() as i32)..((self.c[0] + h - 0.5).floor() as i32 + 1)
        } else {
            all.end..all.end
        };
        let left = all.start..hole.start.clamp(all.start, all.end);
        let right = hole.end.clamp(all.start, all.end)..all.end;
        for x in left.chain(right) {
            let d = (x as f64 + 0.5 - self.c[0]).hypot(dy);
            let (w, b) = (band(d, white_pen) as f32, band(d, black_pen) as f32);
            // White under black: black covers b, white shows through (1 − b).
            let a = (b + w * (1.0 - b)) * self.opacity;
            if a > 0.0 {
                let k = w * (1.0 - b) * self.opacity;
                let px = [self.white[0] * k, self.white[1] * k, self.white[2] * k, a];
                blend_over(self.mode, &mut row[(x - x0) as usize], px);
            }
        }
    }
}

/// The Color Picker's sample square (painting-tools-1.md §3.12 step 6): the `size`×`size`
/// canvas-px square centred on `cell` + 0.5 (`cell`
/// = ⌊pointer⌋), inflated by 1.5·hw, stroked aliased: black 3·hw, then white 1·hw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickerPreview {
    pub cell: [i32; 2],
    pub size: u32,
    pub visible: bool,
}

impl PickerPreview {
    /// The square's device edges (left, top, right, bottom): one hw is one device px.
    fn edges(&self, t: &ViewTransform) -> Option<[f64; 4]> {
        let half = self.size as f64 / 2.0 * t.scale + 1.5;
        let c = [
            (self.cell[0] as f64 + 0.5) * t.scale + t.origin[0],
            (self.cell[1] as f64 + 0.5) * t.scale + t.origin[1],
        ];
        let e = [c[0] - half, c[1] - half, c[0] + half, c[1] + half];
        (self.visible && e.iter().all(|v| v.is_finite() && v.abs() < 1e7)).then_some(e)
    }

    /// The device pixels the black pen covers, ∩ the viewport.
    pub fn device_box(&self, t: &ViewTransform) -> Option<IntRect> {
        let [l, tp, r, b] = self.edges(t)?;
        let (x0, y0) = ((l - 1.5).floor() as i32, (tp - 1.5).floor() as i32);
        let (x1, y1) = ((r + 1.5).ceil() as i32, (b + 1.5).ceil() as i32);
        let b = IntRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32).intersect(&IntRect::new(
            0,
            0,
            t.viewport[0],
            t.viewport[1],
        ));
        (!b.is_empty()).then_some(b)
    }

    /// `paint`'s row painter for a `mode` target.
    pub(crate) fn painter(
        &self,
        t: &ViewTransform,
        mode: RenderMode,
    ) -> Option<impl Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send + use<>> {
        let e = self.edges(t)?;
        let (black, white) = (to_space(mode, [0, 0, 0, 255]), to_space(mode, [255; 4]));
        // Aliased: a pixel is in a pen of width w when its centre lies in the outer rect grown by
        // w/2 but not in the inner one shrunk by w/2 (half-open, left/top inclusive).
        let pen = move |x: i32, y: i32, w: f64| {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            let inside =
                |d: f64| cx >= e[0] + d && cx < e[2] - d && cy >= e[1] + d && cy < e[3] - d;
            inside(-w / 2.0) && !inside(w / 2.0)
        };
        Some(move |y: i32, x0: i32, row: &mut [[f32; 4]]| {
            for (i, px) in row.iter_mut().enumerate() {
                let x = x0 + i as i32;
                if pen(x, y, 1.0) {
                    *px = white;
                } else if pen(x, y, 3.0) {
                    *px = black;
                }
            }
        })
    }
}

/// Photocraft phase spec S§4.3 (Remove's collected strokes): a tint over canvas pixels, `color`
/// at mask/255 of its alpha. `mask` is `rect.w × rect.h` A8 over the canvas rect `rect`; each
/// device pixel takes the mask pixel under its centre (nearest neighbour). An empty `rect` shows
/// nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tint {
    pub rect: IntRect,
    pub mask: Arc<Vec<u8>>,
    pub color: [u8; 4],
    /// Each masked pixel's own straight RGBA (`rect`-sized) instead of `color`: the Patch tool's
    /// preview (S§4.3).
    pub pixels: Option<Arc<Vec<u8>>>,
}

impl Tint {
    /// The device pixels the tint covers, ∩ the viewport.
    pub fn device_box(&self, t: &ViewTransform) -> Option<IntRect> {
        let r = self.rect;
        let ok = !r.is_empty() && self.mask.len() == r.w as usize * r.h as usize;
        if !ok || !(t.scale > 0.0 && t.scale.is_finite()) {
            return None;
        }
        let edge = |v: i32, o: f64| (v as f64 * t.scale + o).clamp(-1e9, 1e9);
        let (x0, y0) = (edge(r.x, t.origin[0]), edge(r.y, t.origin[1]));
        let (x1, y1) = (edge(r.right(), t.origin[0]), edge(r.bottom(), t.origin[1]));
        let (x0, y0) = (x0.floor() as i32, y0.floor() as i32);
        let (x1, y1) = (x1.ceil() as i32, y1.ceil() as i32);
        let view = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
        let b = IntRect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32).intersect(&view);
        (!b.is_empty()).then_some(b)
    }

    /// The tint's row painter for a `mode` target.
    pub(crate) fn painter(
        &self,
        t: &ViewTransform,
        mode: RenderMode,
    ) -> Option<impl Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send + use<>> {
        let b = self.device_box(t)?;
        let [r, g, bl, a] = self.color;
        let lut: Vec<[f32; 4]> = (0..=255u32)
            .map(|m| to_space(mode, [r, g, bl, ((a as u32 * m + 127) / 255) as u8]))
            .collect();
        let (rect, mask, scale, origin) = (self.rect, self.mask.clone(), t.scale, t.origin);
        let pixels = self.pixels.clone().filter(|p| p.len() == mask.len() * 4);
        // The mask column (row) under a device pixel's centre.
        let cell = move |d: i32, o: f64, lo: i32, n: u32| {
            let c = ((d as f64 + 0.5 - o) / scale).floor() as i64 - lo as i64;
            (0..n as i64).contains(&c).then_some(c as usize)
        };
        Some(move |y: i32, x0: i32, row: &mut [[f32; 4]]| {
            if y < b.y || y >= b.bottom() {
                return;
            }
            let Some(cy) = cell(y, origin[1], rect.y, rect.h) else {
                return;
            };
            let line = &mask[cy * rect.w as usize..][..rect.w as usize];
            for (i, px) in row.iter_mut().enumerate() {
                if let Some(cx) = cell(x0 + i as i32, origin[0], rect.x, rect.w)
                    && line[cx] > 0
                {
                    let m = u32::from(line[cx]);
                    let src = match &pixels {
                        Some(p) => {
                            let k = (cy * rect.w as usize + cx) * 4;
                            let a = ((u32::from(p[k + 3]) * m + 127) / 255) as u8;
                            to_space(mode, [p[k], p[k + 1], p[k + 2], a])
                        }
                        None => lut[m as usize],
                    };
                    blend_over(mode, px, src);
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::display::RenderMode;

    /// S§4.3 Remove: at zoom 2 each masked canvas pixel tints its 2×2 device pixels; unmasked
    /// ones and those outside `rect` stay as they were.
    #[test]
    fn tint_covers_its_mask() {
        let t = ViewTransform {
            scale: 2.0,
            origin: [1.0, 0.0],
            viewport: [20, 10],
            ui_scale: 1.0,
        };
        let tint = Tint {
            rect: IntRect::new(2, 1, 2, 1),
            mask: Arc::new(vec![255, 0]),
            color: [255, 0, 0, 128],
            pixels: None,
        };
        assert_eq!(tint.device_box(&t), Some(IntRect::new(5, 2, 4, 2)));
        let mut target = DisplayTarget::new(20, 10, RenderMode::Software);
        target.fill(IntRect::new(0, 0, 20, 10), [0, 0, 255, 255]);
        target.paint_rows(tint.painter(&t, RenderMode::Software).unwrap());
        let px = target.to_rgba8();
        let at = |x: usize, y: usize| px[(y * 20 + x) * 4..][..4].to_vec();
        for (x, y) in [(5, 2), (6, 2), (5, 3), (6, 3)] {
            let p = at(x, y);
            assert!(
                (126..=129).contains(&p[0]) && (126..=129).contains(&p[2]),
                "{p:?}"
            );
        }
        for (x, y) in [(4, 2), (7, 2), (8, 3), (5, 1), (5, 4)] {
            assert_eq!(at(x, y), [0, 0, 255, 255], "{x},{y}");
        }
        assert!(Tint::default().device_box(&t).is_none());
        // S§4.3 Patch: with `pixels` each masked pixel paints its own colour at its mask.
        let own = Tint {
            pixels: Some(Arc::new(vec![0, 255, 0, 255, 0, 0, 0, 255])),
            ..tint
        };
        let mut target = DisplayTarget::new(20, 10, RenderMode::Software);
        target.fill(IntRect::new(0, 0, 20, 10), [0, 0, 255, 255]);
        target.paint_rows(own.painter(&t, RenderMode::Software).unwrap());
        let px = target.to_rgba8();
        assert_eq!(px[(2 * 20 + 5) * 4..][..4], [0, 255, 0, 255]);
        assert_eq!(px[(2 * 20 + 7) * 4..][..4], [0, 0, 255, 255], "mask 0");
    }

    /// §3.12 step 6 at 100 %: a 1-px sample at (10, 10) is ringed black, white, black from the
    /// outside in (pixels 7, 8, 9 and 11, 12, 13), the sampled pixel itself left visible; at 3×3
    /// and zoom 2 the square is 6 device px, inflated by 1.5.
    #[test]
    fn picker_preview_rings_the_sample() {
        let draw = |size: u32, scale: f64, y: usize| {
            let t = ViewTransform {
                scale,
                origin: [0.0, 0.0],
                viewport: [40, 40],
                ui_scale: 1.0,
            };
            let mut target = DisplayTarget::new(40, 40, RenderMode::Software);
            target.fill(IntRect::new(0, 0, 40, 40), [64, 64, 64, 255]);
            let p = PickerPreview {
                cell: [10, 10],
                size,
                visible: true,
            };
            let painter = p.painter(&t, RenderMode::Software).unwrap();
            target.paint_rows(painter);
            let px = target.to_rgba8();
            (0..40).map(|x| px[(y * 40 + x) * 4]).collect::<Vec<u8>>()
        };
        let row = draw(1, 1.0, 10);
        assert_eq!(&row[6..15], &[64, 0, 255, 0, 64, 0, 255, 0, 64]);
        // Zoom 2, N 3: centre 21, half 3 + 1.5 → edges 16.5 and 25.5.
        let row = draw(3, 2.0, 21);
        assert_eq!(&row[14..20], &[64, 0, 255, 0, 64, 64]);
        assert_eq!(&row[23..28], &[64, 0, 255, 0, 64]);
    }

    fn draw(shape: HandleShape, at: [f64; 2], ui: f64, opacity: f32) -> Vec<u8> {
        draw_on(shape, at, ui, opacity, 64)
    }

    fn draw_on(shape: HandleShape, at: [f64; 2], ui: f64, opacity: f32, bg: u8) -> Vec<u8> {
        let t = ViewTransform {
            scale: 1.0,
            origin: [0.0, 0.0],
            viewport: [40, 40],
            ui_scale: ui,
        };
        let mut target = DisplayTarget::new(40, 40, RenderMode::Software);
        target.fill(IntRect::new(0, 0, 40, 40), [bg, bg, bg, 255]);
        let h = Handle {
            canvas: at,
            shape,
            opacity,
            hover: 0.0,
        };
        draw_handle(&mut target, &t, &h);
        target.to_rgba8().chunks(4).map(|p| p[0]).collect()
    }

    /// §2.10 Square at a pixel centre (10.5, 10.5), u = 1: edges at 7.5 and 13.5; black t = 1
    /// covers columns 7 and 13 exactly, white t + 2.5 = 3.5 covers [5.75, 9.25] and [11.75, 15.25]
    /// (columns 6, 8, 12, 14 whole, 5, 9, 11, 15 a quarter), the inside stays.
    #[test]
    fn square_handle_strokes() {
        let row: Vec<u8> = draw(HandleShape::Square, [10.5, 10.5], 1.0, 1.0)[400..420].to_vec();
        let quarter = |c: u8| (c as f32 * 0.75 + 255.0 * 0.25).round() as u8;
        assert_eq!(
            &row[4..17],
            &[
                64,
                quarter(64),
                255,
                0,
                255,
                quarter(64),
                64,
                quarter(64),
                255,
                0,
                255,
                quarter(64),
                64
            ]
        );
        // u = 2: r 6, t 2 at 4.5 and 16.5: columns 4 and 16 black, 3 and 5 half black, half white.
        let row: Vec<u8> = draw(HandleShape::Square, [10.5, 10.5], 2.0, 1.0)[400..420].to_vec();
        assert_eq!((row[4], row[16], row[10]), (0, 0, 64));
        assert!(
            (120..=136).contains(&row[3]) && (120..=136).contains(&row[5]),
            "{row:?}"
        );
    }

    /// Circle: a radius-3 ring at (10.5, 10.5): on the centre row the black t = 1 band covers ¾ of
    /// columns 7 and 13 over the opaque white (64), the white 2.5t band ¾ of columns 6, 8, 12, 14
    /// (217 over a backdrop of 100); the centre stays. Compass at (20.5, 20.5): a white square
    /// [14, 27) with black border columns 14 and 26, the glyph's vertical shaft on column 20.
    #[test]
    fn circle_and_compass() {
        let c = draw_on(HandleShape::Circle, [10.5, 10.5], 1.0, 1.0, 100);
        let row = &c[400..420];
        assert_eq!(
            &row[5..16],
            &[100, 217, 64, 217, 100, 100, 100, 217, 64, 217, 100]
        );
        assert_eq!(c[7 * 40 + 10], 64);
        let m = draw(HandleShape::Compass, [20.5, 20.5], 1.0, 1.0);
        assert_eq!((m[16 * 40 + 14], m[16 * 40 + 26]), (0, 0));
        assert_eq!(m[16 * 40 + 16], 255);
        assert_eq!(m[18 * 40 + 20], 0);
        assert_eq!((m[16 * 40 + 13], m[16 * 40 + 27]), (64, 64));
    }

    /// §2.10 "Opacity pulse": 2 s period over 0.25…1.5 clipped at 1, phase index/10.
    #[test]
    fn pulse_range_and_phase() {
        assert_eq!(pulse_opacity(0, 0.0), 0.875);
        assert_eq!((pulse_opacity(0, 0.5), pulse_opacity(0, 1.5)), (1.0, 0.25));
        // Index 5 is half a period ahead.
        assert!((pulse_opacity(5, 0.0) - 0.875).abs() < 1e-6);
        assert!((pulse_opacity(5, 1.5) - pulse_opacity(0, 0.5)).abs() < 1e-6);
        assert_eq!(pulse_opacity(5, 0.5), 0.25);
    }

    /// §3.14 step 5: the brush outline at canvas (20.25, 20.25), radius 5 canvas px at zoom 2 →
    /// a ring of radius 10 device px centred on device pixel centre (40.5, 40.5). On its row the
    /// black pen [9.5, 10.5] fills the pixels 10 px out; the white pen [8.75, 11.25] covers ¾ of
    /// their neighbours; inside and outside keep the backdrop. × 0.5 pressure halves it; opacity
    /// 0 hides it.
    #[test]
    fn brush_preview_ring() {
        let t = ViewTransform {
            scale: 2.0,
            origin: [0.0, 0.0],
            viewport: [80, 80],
            ui_scale: 1.5,
        };
        let draw = |p: BrushPreview| {
            let mut target = DisplayTarget::new(80, 80, RenderMode::Software);
            target.fill(IntRect::new(0, 0, 80, 80), [100, 100, 100, 255]);
            if let Some(painter) = BrushPreviewPainter::new(&t, &p, RenderMode::Software) {
                target.paint_rows(|y, x0, row| painter.paint(y, x0, row));
            }
            target
                .to_rgba8()
                .chunks(4)
                .map(|p| p[0])
                .collect::<Vec<u8>>()
        };
        let p = BrushPreview {
            canvas: [20.25, 20.25],
            radius: 5.0,
            pressure: None,
            opacity: 1.0,
        };
        let m = draw(p);
        let row = &m[40 * 80..41 * 80];
        let white = (255.0 * 0.75 + 100.0 * 0.25f64).round() as u8;
        assert_eq!((row[30], row[50], m[30 * 80 + 40]), (0, 0, 0));
        assert_eq!(
            (row[49], row[51], row[29], row[31]),
            (white, white, white, white)
        );
        assert_eq!((row[40], row[45], row[53], row[27]), (100, 100, 100, 100));
        assert_eq!(p.device_box(&t), Some(IntRect::new(28, 28, 25, 25)));
        let half = draw(BrushPreview {
            pressure: Some(0.5),
            ..p
        });
        assert_eq!((half[40 * 80 + 45], half[40 * 80 + 50]), (0, 100));
        let hidden = BrushPreview { opacity: 0.0, ..p };
        assert!(draw(hidden).iter().all(|&v| v == 100));
        assert_eq!(hidden.device_box(&t), None);
    }

    /// Opacity scales the whole layer; 0 draws nothing.
    #[test]
    fn opacity_layer() {
        let half = draw(HandleShape::Square, [10.5, 10.5], 1.0, 0.5)[407];
        assert_eq!(half, 32);
        assert!(
            draw(HandleShape::Square, [10.5, 10.5], 1.0, 0.0)
                .iter()
                .all(|&v| v == 64)
        );
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    #[test]
    fn reset_button_icon_states_rotation_and_no_hover_enlargement() {
        // A generated stand-in for the app's icon: a blue bar on the top half, so a quarter turn
        // shows.
        let mut icon = Pixmap::new(16, 16).unwrap();
        icon.fill_rect(
            Rect::from_xywh(2.0, 2.0, 12.0, 5.0).unwrap(),
            &paint(Color::from_rgba8(91, 149, 214, 255)),
            Transform::identity(),
            None,
        );
        set_reset_icon(icon);
        let render = |state, angle, hover, zoom| {
            let t = ViewTransform {
                scale: zoom,
                origin: [0.0, 0.0],
                viewport: [64, 64],
                ui_scale: 1.0,
            };
            let mut target = DisplayTarget::new(64, 64, RenderMode::Software);
            target.fill(IntRect::new(0, 0, 64, 64), [255; 4]);
            draw_handle(
                &mut target,
                &t,
                &Handle {
                    canvas: [32.0 / zoom, 32.0 / zoom],
                    shape: HandleShape::Reset { state, angle },
                    opacity: 1.0,
                    hover,
                },
            );
            target.to_rgba8()
        };
        let normal = render(ResetState::Normal, 0.0, 0.0, 1.0);
        assert_eq!(normal, render(ResetState::Normal, 0.0, 1.0, 1.0));
        assert_eq!(normal, render(ResetState::Normal, 0.0, 0.0, 2.0));
        assert_ne!(
            normal,
            render(ResetState::Normal, std::f64::consts::FRAC_PI_2, 0.0, 1.0)
        );
        for state in [ResetState::Hot, ResetState::Pressed, ResetState::Disabled] {
            assert_ne!(normal, render(state, 0.0, 0.0, 1.0));
        }
        assert!(
            normal
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[2] as u16 > p[0] as u16 + 20 && p[3] == 255),
            "the supplied blue reset icon is painted"
        );
    }
}
