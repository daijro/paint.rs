//! The `#canvas` element and the rulers: the active document rendered by `SurfaceBox` into a
//! device-pixel `DisplayTarget` (Paint.NET's canvas, R22) and uploaded 1:1 as one egui texture;
//! Paint.NET's rulers drawn inside paintdotjs's ruler strips (spec §0.10). The base (view, grid)
//! is cached; the overlays (selection layers) are drawn over copies of its rows as the texture
//! pixels are written, and an ants frame alone rewrites just the outline spans.
//! The canvas input: pointer routing to the tools and the middle-button pan, wheel scroll and
//! Ctrl+wheel zoom, auto-scroll while dragging, and the tool cursor drawn over the canvas.
//! Port of: js/src/engine/AppView.js (`render`, `renderRulers`, `drawRuler`, the pointer and
//! wheel listeners, `startAutoScroll`/`autoScrollIfNecessary`), css/layout.css (`#canvas`,
//! `.editor-ruler*`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's canvas rendering, selection highlight, wheel, a second button while tracking,
//! all-button capture and cursors.
use std::sync::{Arc, LazyLock};

use crate::kit::tiles::Tiles;
use egui::{
    Color32, ColorImage, CursorIcon, Event, FontFamily, FontId, Id, MouseWheelUnit, Painter,
    PointerButton, Pos2, Rect, Sense, TextureHandle, TextureId, TextureOptions, Ui, Vec2,
};
use pdn_core::app::{AppEvent, AppWorkspace, MeasurementUnit};
use pdn_core::fonts::WinMetrics;
use pdn_core::pixel::IntRect;
use pdn_core::pixel::mask::SelectionQuality;
use pdn_core::render::Renderer;
use pdn_core::render::canvas::{CanvasPixels, Upload};
use pdn_core::render::display::{RenderMode, span_bands};
use pdn_core::render::ruler::{
    self, HIGHLIGHT, HIGHLIGHT_TEXT, HOT_TRACK, RulerColors, RulerTicks, RulerUnit,
};
use pdn_core::render::selection::{SelectionRenderer, mask_builds};
use pdn_core::render::view::{CanvasStyle, ViewTransform};
use pdn_core::settings::AppSettings;
use pdn_core::tool::{CursorRequest, MouseButton, PointerInput, PointerSample, PointerType};
use pdn_core::util::color::Color;
use pdn_core::util::geom::Point;
use pdn_core::util::geom::Rectangle;

use crate::kit::text::{self, FontSpec, record_pdn_rect};
use crate::theme::{self, RULER_LABEL_FONT_SIZE, RULER_SIZE, RULER_UNITS_FONT_SIZE, Scheme};

/// Everything a ruler strip's pixels depend on.
#[derive(Clone, Debug, PartialEq)]
struct RulerKey {
    len: u32,
    thick: u32,
    vertical: bool,
    colors: RulerColors,
    /// (unit, dpu, scale, offset) when a document is open.
    view: Option<(RulerUnit, f64, f64, f64)>,
    marker: Option<(f64, f64)>,
    /// The selection's extent in ruler px (§2.11 step 8).
    highlight: Option<(f64, f64)>,
}

#[derive(Default)]
pub struct CanvasView {
    pub pen: crate::platform::PenInput,
    /// The base kept between renders and the overlays drawn over it on the way to `pixels`; a
    /// frame redraws only what changed (Task 27r).
    canvas: CanvasPixels,
    /// The selection's bounds per selection revision (the ruler highlight).
    selection_bounds: Option<(u64, Option<Rectangle>)>,
    texture: Tiles,
    /// The texture's pixels, written in place each render (also `kit::shadow`'s backdrop replay).
    pixels: Option<Arc<ColorImage>>,
    /// A zoom draft at 1/k resolution (`Upload::Half`), shown in place of `texture` at k times its
    /// size until another upload, its pixels and k.
    half: Option<(Tiles, Arc<ColorImage>, u32)>,
    /// Where `texture` holds `pixels`: device px (x, y) at texel ((x + wrap[0]) mod w, (y +
    /// wrap[1]) mod h), so a pan moves the copy instead of uploading every pixel (until its
    /// gesture ends); `None` once the pixels changed under it (a zoom draft spread over them).
    wrap: Option<[u32; 2]>,
    /// Per ruler (horizontal, vertical): its key, texture, labels and the texture's pixels.
    rulers: [Option<Ruler>; 2],
    /// The units box in the corner (the abbreviation is drawn as text each frame).
    corner: Option<Corner>,
    /// All physical canvas buttons still held. Capture survives the first release (which
    /// ends brush/picker drags) until the last button is up, including three-button chords.
    buttons: u8,
    /// `lastAutoScrollTime` (app ms).
    last_auto_scroll: f64,
    /// The document and view last shown, and when the view last changed (app ms, frame): a change
    /// soon after another, or in the frame after one, is a pan or zoom in progress.
    view: Option<(u64, ViewTransform, f64, Option<u64>)>,
    /// The canvas texture's top-left and the part of the canvas shown (not under the rulers), as last drawn: where a
    /// live session draws the others (`crate::live`).
    pub shown: Option<(Pos2, Rect)>,
}

/// The most pixels a zoom draft draws on the web, where the canvas renders on one thread (about
/// 7 ms there): a large view drafts at a third or a quarter of its resolution. Native drafts stay
/// at half.
const DRAFT_PIXELS: Option<usize> = if cfg!(target_arch = "wasm32") {
    Some(600_000)
} else {
    None
};

/// The pieces of the span `[a, a + n)` of a `size`-long axis whose texels sit `wrap` further on,
/// modulo `size`: (first px, first texel, length), at most two.
fn wrapped(a: i32, n: i32, wrap: i32, size: i32) -> Vec<(i32, i32, i32)> {
    let t = (a + wrap).rem_euclid(size);
    let first = n.min(size - t);
    [(a, t, first), (a + first, 0, n - first)]
        .into_iter()
        .filter(|p| p.2 > 0)
        .collect()
}

/// A view change within this long of the last one draws a draft (`CanvasPixels::draft`); this
/// long after the last change the view settles, drawn exactly.
const SETTLE_MS: f64 = 150.0;

/// A ruler's key, texture, labels (major x in ruler px, text) and the texture's pixels.
type Ruler = (RulerKey, Tiles, Vec<(f64, String)>, Arc<ColorImage>);

/// The units box's key (colours, side), texture and pixels.
type Corner = ((RulerColors, u32), TextureHandle, Arc<ColorImage>);

/// The display settings Paint.NET's canvas and its thumbnails share: the checkerboard brightness
/// (`canvas.checkerboardBrightness` / 100, in 0.25..=1), the render mode
/// (`graphics.hardwareAcceleration`, R22) and Aero Dark (spec §0.10: the Dark and Blue schemes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayPrefs {
    pub checker_brightness: f32,
    pub mode: RenderMode,
    pub dark: bool,
}

impl DisplayPrefs {
    pub fn new(settings: &AppSettings, theme: &theme::Theme) -> Self {
        DisplayPrefs {
            checker_brightness: (settings.get_f64("canvas.checkerboardBrightness") / 100.0)
                .clamp(0.25, 1.0) as f32,
            mode: RenderMode::from_setting(settings.get_bool("graphics.hardwareAcceleration")),
            dark: theme.scheme != Scheme::Light,
        }
    }
}

/// `CanvasStyle` from the settings and colour scheme.
fn canvas_style(settings: &AppSettings, theme: &theme::Theme) -> CanvasStyle {
    let custom = settings.get_bool("canvas.customBorder").then(|| {
        let c = Color::from_hex(settings.get_str("canvas.borderColor"));
        [c.red, c.green, c.blue, 255]
    });
    let p = DisplayPrefs::new(settings, theme);
    CanvasStyle::for_scheme(
        p.dark,
        settings.get_bool("canvas.dropShadow"),
        custom,
        p.checker_brightness,
        p.mode,
    )
}

/// Gives every selection layer this frame's frame count and quality (writing only what changed, so
/// the overlays stay cached between frames) and its ants frame (a frame-only change); true while an
/// outline with geometry shows.
fn sync_selection_layers(
    ws: &mut pdn_core::document::workspace::DocumentWorkspace,
    frame: u32,
    frames: u32,
    quality: SelectionQuality,
) -> bool {
    let stale: Vec<_> = ws
        .surface_box
        .renderers
        .iter()
        .filter_map(|(id, r)| match r {
            Renderer::Selection(s) if (s.frames, s.quality) != (frames, quality) => Some(*id),
            _ => None,
        })
        .collect();
    for id in stale {
        if let Some(Renderer::Selection(s)) = ws.surface_box.renderer_mut(id) {
            (s.frames, s.quality) = (frames, quality);
        }
    }
    ws.surface_box.set_ants_frame(frame);
    let empty = ws.selection.is_empty();
    ws.surface_box.renderers.iter().any(|(_, r)| {
        matches!(r, Renderer::Selection(s)
            if s.visible && s.outline && s.path.as_ref().map_or(!empty, |p| !p.is_empty()))
    })
}

/// `p` snapped to the device-pixel grid, so a texture drawn there maps 1:1 to device pixels.
fn snap(p: Pos2, ppp: f32) -> Pos2 {
    Pos2::new((p.x * ppp).round() / ppp, (p.y * ppp).round() / ppp)
}

/// §10.11: fill supplied bounds, then stroke one hair outside them with the same byte alpha.
fn paint_text_caret(painter: &Painter, cursor: Rect, ppp: f32, alpha: u8) {
    painter.rect_filled(cursor, 0.0, Color32::from_black_alpha(alpha));
    let hair = 1.0 / ppp;
    painter.rect_stroke(
        cursor.expand(hair),
        0.0,
        egui::Stroke::new(hair, Color32::from_white_alpha(alpha)),
        egui::StrokeKind::Middle,
    );
}

impl CanvasView {
    /// A button still holding canvas capture, if any.
    pub fn captured(&self) -> Option<MouseButton> {
        MouseButton::ALL
            .into_iter()
            .find(|&b| self.buttons & (1 << b as u8) != 0)
    }

    /// The pixels of this view's textures (canvas, rulers, corner), by texture id. Not the canvas's
    /// while its texture holds a moved copy (`wrap`) or a zoom draft shows: those replay as unknown.
    pub fn texture_pixels(&self, id: TextureId) -> Option<Arc<ColorImage>> {
        if let Some(px) = self
            .pixels
            .as_ref()
            .filter(|_| self.wrap == Some([0, 0]) && self.half.is_none())
            .and_then(|img| self.texture.pixels(id, img))
        {
            return Some(px);
        }
        if let Some((_, t, img)) = &self.corner
            && t.id() == id
        {
            return Some(img.clone());
        }
        self.rulers
            .iter()
            .flatten()
            .find_map(|(_, t, _, img)| t.pixels(id, img))
    }

    /// `DocumentInvalidated { rects }` (empty = the whole document).
    pub fn invalidate(&mut self, rects: &[Rectangle]) {
        self.canvas.invalidate(rects);
    }

    /// Whether the last render drew a pan or zoom draft.
    pub fn drafted(&self) -> bool {
        self.canvas.drafted()
    }

    /// The transform a draft shows, while one shows (`CanvasPixels::draft_view`).
    pub fn draft_view(&self) -> Option<pdn_core::render::view::ViewTransform> {
        self.canvas.draft_view()
    }

    /// The next render redraws everything (tests compare it with what the frames redrew).
    pub fn refresh_all(&mut self) {
        self.canvas = CanvasPixels::default();
    }

    /// Renders the active document into `rect` (the `#canvas` box, view client size) at device
    /// resolution; only `visible` (the part not under the rulers) is painted and recorded as a pdn
    /// rect. The selection layers take this frame's ants phase (`canvas-rendering.md §2.9` step
    /// 4), and a repaint is requested for the next one while an outline shows.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        visible: Rect,
        app: &mut AppWorkspace,
        settings: &AppSettings,
        text_owner: bool,
    ) {
        let (active, minimized) = ui.input(|i| {
            (
                i.viewport().focused.unwrap_or(i.focused),
                i.viewport().minimized.unwrap_or(false),
            )
        });
        let focused = text_owner && !ui.ctx().text_edit_focused() && active;
        app.with_tool(|tool, app| {
            if let Some(t) = tool
                .as_any_mut()
                .downcast_mut::<pdn_core::tool::text::TextTool>()
            {
                t.update_pulse(
                    app,
                    focused,
                    active,
                    minimized,
                    settings.get_bool("ui.animations"),
                );
                if t.is_editing() && !minimized {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(16));
                }
            }
        });
        // §2.9 step 1: complex selections' masks render on worker threads.
        let ctx = ui.ctx().clone();
        let masks = mask_builds(Arc::new(move || ctx.request_repaint()));
        let style = canvas_style(settings, &theme::current(ui.ctx()));
        let frames = SelectionRenderer::frames_for(style.mode, settings.get_bool("ui.animations"));
        let now_s = app.now_ms / 1000.0;
        let frame = SelectionRenderer::frame_at(now_s, frames);
        // One shared setting (`selection-and-move.md §2.0` "Every tool").
        let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
        let Some(ws) = app.active_ws_mut() else {
            // No image (the last one closed, maybe mid-drag): its float and draft buffers go.
            (self.canvas, self.half) = (CanvasPixels::default(), None);
            self.shown = None;
            return;
        };
        let animating = sync_selection_layers(ws, frame, frames, quality);
        if animating && frames > 1 {
            // The counter steps every 1/30 s.
            let next = ((30.0 * now_s).floor() + 1.0) / 30.0 - now_s;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(next.clamp(0.0, 1.0)));
        }
        let ws = &*ws;
        let ppp = ui.painter().pixels_per_point();
        let t = ws.view.view_transform();
        let size = t.viewport.map(|v| v.max(1) as usize);
        if self.texture.is_empty() || self.pixels.as_ref().is_none_or(|p| p.size != size) {
            self.canvas = CanvasPixels::default();
            self.pixels = Some(Arc::new(ColorImage::filled(size, Color32::TRANSPARENT)));
        }
        let now = now_s * 1000.0;
        let doc = ws.document.id;
        let frame = ui.ctx().cumulative_frame_nr();
        let gesture = match self.view {
            Some((d, old, at, then)) if d == doc && old != t => {
                self.view = Some((doc, t, now, Some(frame)));
                // Frame after frame too: frames slower than SETTLE_MS would otherwise render each
                // change exactly, which keeps them that slow (a large view on the web, a busy CPU).
                now - at < SETTLE_MS || then.is_some_and(|f| f + 1 == frame)
            }
            Some((d, _, at, _)) if d == doc => now - at < SETTLE_MS,
            _ => {
                self.view = Some((doc, t, f64::NEG_INFINITY, None));
                false
            }
        };
        self.canvas.draft = gesture;
        self.canvas.draft_pixels = DRAFT_PIXELS;
        let floating = ws.float.is_some();
        if let (true, Some((_, _, at, _))) = (gesture, self.view) {
            let settle = (at + SETTLE_MS - now).max(0.0) / 1000.0;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(settle));
        }
        let image = self.pixels.as_mut().expect("sized above");
        let conv = |[r, g, b, a]: [u8; 4]| Color32::from_rgba_premultiplied(r, g, b, a);
        // In place unless egui still holds last frame's upload.
        let pixels = &mut *image;
        let out = move || {
            let pixels = pixels;
            &mut Arc::make_mut(pixels).pixels[..]
        };
        let upload = self.canvas.render(ws, &t, &style, masks, out, conv);
        if !matches!(upload, Upload::Nothing | Upload::Half) {
            self.half = None;
        }
        let ctx = ui.ctx();
        // What the texture holds as it was: the pixels (unwrapped) unless panned.
        let held = self.wrap == Some([0, 0]);
        // While it holds a moved copy (`wrap`), what changed goes up where the copy keeps it.
        let moves = self.texture.single()
            && self.wrap.is_some()
            && image.size == [t.viewport[0] as usize, t.viewport[1] as usize];
        let upload = match upload {
            Upload::Rect(r) if !held && moves => Upload::Panned {
                d: [0, 0],
                rects: vec![r],
            },
            Upload::Spans(s) if !held && moves => Upload::Panned {
                d: [0, 0],
                rects: span_bands(&s, 32),
            },
            upload => upload,
        };
        match upload {
            Upload::Nothing => {}
            Upload::Rect(r) if held => self.texture.upload(ctx, "canvas-view", image, Some(r)),
            // Row bands of them.
            Upload::Spans(spans) if held => {
                for r in span_bands(&spans, 32) {
                    self.texture.upload(ctx, "canvas-view", image, Some(r));
                }
            }
            Upload::Panned { d, rects } if moves => {
                // The texture's copy moves by `d` (its wrap), so only `rects` go up.
                let [w, h] = [t.viewport[0] as i32, t.viewport[1] as i32];
                let wrap = self.wrap.expect("checked");
                let wrap = [
                    (wrap[0] as i32 - d[0]).rem_euclid(w),
                    (wrap[1] as i32 - d[1]).rem_euclid(h),
                ];
                for r in rects {
                    for (x, tx, cw) in wrapped(r.x, r.w as i32, wrap[0], w) {
                        for (y, ty, ch) in wrapped(r.y, r.h as i32, wrap[1], h) {
                            let src = IntRect::new(x, y, cw as u32, ch as u32);
                            self.texture
                                .upload_at(ctx, image, src, [tx as usize, ty as usize]);
                        }
                    }
                }
                self.wrap = Some(wrap.map(|v| v as u32));
            }
            Upload::Half => {
                let ([hw, hh], k, px) = self.canvas.half_shown().expect("Upload::Half");
                let (mut tiles, small, _) = self.half.take().unwrap_or_default();
                let mut small = Arc::try_unwrap(small).map_or_else(|_| vec![], |i| i.pixels);
                small.clear();
                small.extend(px.iter().map(|&p| conv(p)));
                let small = Arc::new(ColorImage::new([hw as usize, hh as usize], small));
                tiles.upload(ctx, "canvas-half", &small, None);
                self.half = Some((tiles, small, k));
                // The pixels change under the texture's copy once a pan spreads these over them.
                self.wrap = None;
            }
            // Everything (whatever the texture held).
            _ => {
                self.texture.upload(ctx, "canvas-view", image, None);
                self.wrap = Some([0, 0]);
            }
        }
        // A pan moves the texture's copy (`wrap`): once the gesture is over it goes up whole again,
        // so that the windows' backdrops replay the texture as the pixels.
        let wrapped = self.wrap.is_some_and(|w| w != [0, 0]);
        if wrapped && !gesture {
            self.texture.upload(ctx, "canvas-view", image, None);
            self.wrap = Some([0, 0]);
        }
        // While the canvas changes every frame (or its copy is wrapped), the windows over it blend
        // it on the GPU.
        if self.canvas.drafted() || floating || (wrapped && gesture) {
            let shown = self.half.as_ref().map_or(&self.texture, |h| &h.0);
            crate::kit::shadow::set_live(ctx, shown.ids());
        }
        let painter = ui.painter().with_clip_rect(visible);
        let min = snap(rect.min, ppp);
        match (&self.half, self.wrap) {
            (Some((half, _, k)), _) => half.paint_scaled(&painter, min, ppp, *k as f32),
            (None, Some(wrap)) if wrap != [0, 0] => {
                self.texture.paint_wrapped(&painter, min, ppp, wrap)
            }
            _ => self.texture.paint(&painter, min, ppp),
        }
        self.shown = Some((min, visible));
        record_pdn_rect(ui.ctx(), visible);
        paint_perspective(
            &ui.painter().with_clip_rect(visible),
            snap(rect.min, ppp),
            app,
        );
        {
            app.with_tool(|tool, app| {
                if let Some(t) = tool
                    .as_any_mut()
                    .downcast_mut::<pdn_core::tool::text::TextTool>()
                    && t.is_editing()
                {
                    let r = t.caret_rect(app);
                    let cursor = Rect::from_min_size(
                        rect.min + Vec2::new(r.x as f32, r.y as f32),
                        Vec2::new(r.width as f32, r.height as f32),
                    );
                    let alpha = t.caret_alpha(app.now_ms);
                    paint_text_caret(&ui.painter().with_clip_rect(visible), cursor, ppp, alpha);
                    // Only the canvas owner sets IME output. Widgets can replace it later.
                    if focused {
                        ui.ctx().output_mut(|o| {
                            o.ime = Some(egui::output::IMEOutput {
                                purpose: egui::IMEPurpose::Normal,
                                rect: visible,
                                cursor_rect: cursor,
                                should_interrupt_composition: std::mem::take(
                                    &mut t.interrupt_composition,
                                ),
                            })
                        });
                    }
                }
            });
        }
    }

    /// The selection's vertex bounds, recomputed only when it changed (`None` when empty).
    fn selection_bounds(
        &mut self,
        ws: &pdn_core::document::workspace::DocumentWorkspace,
    ) -> Option<Rectangle> {
        let rev = ws.selection.revision();
        match self.selection_bounds {
            Some((r, b)) if r == rev => b,
            _ => {
                let b = (!ws.selection.is_empty()).then(|| ws.selection.get_bounds());
                self.selection_bounds = Some((rev, b));
                b
            }
        }
    }

    /// `#editor.rulers-visible`: paintdotjs's strips (17 DIP, the corner box) with Paint.NET's
    /// rulers inside (`canvas-rendering.md §2.11`). `client` is the `#view` client size (the
    /// rulers' lengths), `canvas` the `#canvas` box, `pointer` the pointer when it is over the
    /// visible canvas (the mouse marker).
    pub fn show_rulers(
        &mut self,
        ui: &mut Ui,
        editor: Rect,
        client: Vec2,
        canvas: Rect,
        pointer: Option<Pos2>,
        app: &AppWorkspace,
    ) {
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        let colors = RulerColors::for_scheme(theme::current(ui.ctx()).scheme != Scheme::Light);
        // §2.11 step 1: thickness round-away(17u) (`dip_px`, pinned at 1/1.25/1.5/2 by
        // `kit::glyph`'s `ruler_thickness_rounds_per_scale`).
        let thick = crate::kit::glyph::dip_px(RULER_SIZE, ppp) as u32;
        let device = |v: f32| (v * ppp).round();
        let selection = app.active_ws().and_then(|ws| self.selection_bounds(ws));
        let strips = [
            Rect::from_min_size(
                editor.min + Vec2::new(RULER_SIZE, 0.0),
                Vec2::new(client.x.max(1.0), RULER_SIZE),
            ),
            Rect::from_min_size(
                editor.min + Vec2::new(0.0, RULER_SIZE),
                Vec2::new(RULER_SIZE, client.y.max(1.0)),
            ),
        ];
        for (i, strip) in strips.into_iter().enumerate() {
            let vertical = i == 1;
            let axis = |p: Pos2| if vertical { p.y } else { p.x };
            let start = device(axis(strip.min));
            let len = (device(axis(strip.max)) - start).max(1.0) as u32;
            let mut key = RulerKey {
                len,
                thick,
                vertical,
                colors,
                view: None,
                marker: None,
                highlight: None,
            };
            if let Some(ws) = app.active_ws() {
                let t = ws.view.view_transform();
                let s = t.scale;
                // The image origin in this strip's px (Paint.NET: offset = −(document coordinate
                // at the ruler's origin); its top ruler spans the corner and adds height/s).
                let origin = t.origin[i] - (start - device(axis(canvas.min))) as f64;
                let res = ws.document.resolution;
                let (unit, dpu) = match app.measurement_unit {
                    MeasurementUnit::Pixel => (RulerUnit::Pixel, 1.0),
                    MeasurementUnit::Inch => (RulerUnit::Inch, res),
                    MeasurementUnit::Centimeter => (RulerUnit::Centimeter, res / 2.54),
                };
                key.view = Some((unit, dpu, s, origin / s));
                // §2.11 step 9: max(1, s) px over the document pixel under the pointer, found
                // as the status bar finds it (`to_document_position`).
                // calibrate: B-C6 — the marker's exact columns.
                key.marker = pointer.map(|p| {
                    let o = snap(canvas.min, ppp);
                    let at = Point::new((p.x - o.x) as f64, (p.y - o.y) as f64);
                    let doc = ws.view.to_document_position(at, false);
                    let x0 = [doc.x, doc.y][i] * s + origin;
                    (x0, x0 + s.max(1.0))
                });
                // §2.11 step 8: the selection's extent.
                key.highlight = selection.map(|b| {
                    let (lo, hi) = if vertical {
                        (b.y, b.bottom())
                    } else {
                        (b.x, b.right())
                    };
                    (lo * s + origin, hi * s + origin)
                });
            }
            if self.rulers[i].as_ref().is_none_or(|(k, ..)| *k != key) {
                let ticks = key
                    .view
                    .map_or_else(RulerTicks::default, |(u, dpu, s, off)| {
                        ruler::ticks(u, dpu, s, off, len, thick)
                    });
                let image = Arc::new(ruler_image(&ticks, &key));
                let name = if vertical { "ruler-v" } else { "ruler-h" };
                let mut tex = Tiles::default();
                tex.upload(ui.ctx(), name, &image, None);
                let labels = ticks
                    .majors
                    .iter()
                    .map(|m| (m.x, format!("{}", m.value as i64)))
                    .collect();
                self.rulers[i] = Some((key, tex, labels, image));
            }
            let rect = if vertical {
                Rect::from_min_size(
                    Pos2::new(device(strip.min.x) / ppp, start / ppp),
                    Vec2::new(thick as f32, len as f32) / ppp,
                )
            } else {
                Rect::from_min_size(
                    Pos2::new(start / ppp, device(strip.min.y) / ppp),
                    Vec2::new(len as f32, thick as f32) / ppp,
                )
            };
            if let Some((key, tex, labels, _)) = &self.rulers[i] {
                tex.paint(&painter, rect.min, ppp);
                // §2.11 step 8: labels over the highlight in HighlightText.
                let (lo, hi) = key.highlight.map_or((0.0, 0.0), |(x0, x1)| {
                    let c = covered(x0, x1, len);
                    (c.start as f32 / ppp, c.end as f32 / ppp)
                });
                let clip = rect.intersect(strip);
                let part = |a: f32, b: f32| {
                    let o = if vertical { rect.min.y } else { rect.min.x };
                    let (a, b) = (o + a, o + b);
                    if vertical {
                        Rect::from_min_max(
                            Pos2::new(clip.min.x, a.max(clip.min.y)),
                            Pos2::new(clip.max.x, b.min(clip.max.y)),
                        )
                    } else {
                        Rect::from_min_max(
                            Pos2::new(a.max(clip.min.x), clip.min.y),
                            Pos2::new(b.min(clip.max.x), clip.max.y),
                        )
                    }
                };
                let parts = [
                    (part(f32::NEG_INFINITY, lo), colors.text),
                    (part(lo, hi), HIGHLIGHT_TEXT),
                    (part(hi, f32::INFINITY), colors.text),
                ];
                for (r, color) in parts.into_iter().filter(|(r, _)| r.is_positive()) {
                    let clipped = painter.with_clip_rect(r);
                    for (x, label) in labels {
                        draw_label(&clipped, rect.min, *x, label, color, vertical);
                    }
                }
            }
            record_pdn_rect(ui.ctx(), strip);
        }
        let corner = Rect::from_min_size(editor.min, Vec2::splat(RULER_SIZE));
        self.show_units_box(ui, corner, thick, colors, app);
    }

    /// §2.11 step 6 in paintdotjs's corner box: the units box (background, a tick-colour line at
    /// x = side + 0.5 and the top ruler's border row) and the unit abbreviation centred in the UI
    /// font.
    fn show_units_box(
        &mut self,
        ui: &mut Ui,
        corner: Rect,
        thick: u32,
        colors: RulerColors,
        app: &AppWorkspace,
    ) {
        let painter = ui.painter();
        let ppp = painter.pixels_per_point();
        let key = (colors, thick);
        if self.corner.as_ref().is_none_or(|(k, ..)| *k != key) {
            let (bg, tick) = (rgb(colors.background), rgb(colors.tick));
            let side = thick - 1;
            let pixels = (0..thick * thick)
                .map(|i| {
                    if i % thick == side || i / thick == side {
                        tick
                    } else {
                        bg
                    }
                })
                .collect();
            let image = Arc::new(ColorImage::new([thick as usize; 2], pixels));
            let tex = ui
                .ctx()
                .load_texture("ruler-corner", image.clone(), TextureOptions::NEAREST);
            self.corner = Some((key, tex, image));
        }
        let min = snap(corner.min, ppp);
        let rect = Rect::from_min_size(min, Vec2::splat(thick as f32 / ppp));
        if let Some((_, tex, _)) = &self.corner {
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let unit = pdn_core::i18n::t(&format!(
            "measurementUnit.{}.abbreviation",
            app.measurement_unit.id()
        ));
        let galley = painter.layout_no_wrap(
            unit,
            FontId::new(RULER_UNITS_FONT_SIZE, FontFamily::Proportional),
            rgb(colors.text),
        );
        // DirectWrite centring of the line box (win metrics) in the side × side box.
        let side = (thick - 1) as f32;
        let m = text::ui_metrics();
        let em = RULER_UNITS_FONT_SIZE * ppp / m.units_per_em as f32;
        let line = (m.ascent + m.descent) as f32 * em;
        let baseline = ((side - line) / 2.0 + m.ascent as f32 * em).round() / ppp;
        let left = ((side - galley.size().x * ppp) / 2.0).round() / ppp;
        let pos = min + Vec2::new(left, baseline - galley_baseline(&galley));
        painter
            .with_clip_rect(rect)
            .galley(pos, galley, Color32::WHITE);
        record_pdn_rect(ui.ctx(), corner);
    }
}

/// Where the canvas input lands: `visible` is the canvas viewport (screen points, the part of
/// `#canvas` not under the rulers or scrollbars), `origin` the canvas's device-aligned top-left
/// (canvas coordinates are points from it), `view` the `#view` box (auto-scroll's bounds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasInput {
    pub visible: Rect,
    pub origin: Pos2,
    pub view: Rect,
    /// `ui.autoScrollWhileDrawing`.
    pub auto_scroll: bool,
}

/// Physical button identity is retained for capture, palette choice and tool chord rules.
fn mouse_button(b: PointerButton) -> MouseButton {
    match b {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Middle => MouseButton::Middle,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Extra1 => MouseButton::X1,
        PointerButton::Extra2 => MouseButton::X2,
    }
}

/// Event-local physical state starts with the prior frame's mask. Replaying edges forward
/// makes duplicate press/release packets idempotent without changing the frame-end polling mask.
pub(crate) fn physical_buttons_at(events: &[Event], index: usize, initial_mask: u8) -> u8 {
    events[..index]
        .iter()
        .fold(initial_mask, |mask, event| match event {
            Event::PointerButton {
                button, pressed, ..
            } => {
                let bit = 1 << mouse_button(*button) as u8;
                if *pressed { mask | bit } else { mask & !bit }
            }
            Event::WindowFocused(false) => 0,
            _ => mask,
        })
}

/// The egui cursor matching a CSS `cursor` keyword.
fn css_cursor(keyword: &str) -> CursorIcon {
    match keyword {
        "none" => CursorIcon::None,
        "pointer" => CursorIcon::PointingHand,
        "crosshair" => CursorIcon::Crosshair,
        "move" => CursorIcon::Move,
        "text" => CursorIcon::Text,
        "grab" => CursorIcon::Grab,
        "grabbing" => CursorIcon::Grabbing,
        "not-allowed" => CursorIcon::NotAllowed,
        "nwse-resize" => CursorIcon::ResizeNwSe,
        "nesw-resize" => CursorIcon::ResizeNeSw,
        "ew-resize" => CursorIcon::ResizeHorizontal,
        "ns-resize" => CursorIcon::ResizeVertical,
        // The Zoom tool's (Task 31).
        "zoom-in" => CursorIcon::ZoomIn,
        "zoom-out" => CursorIcon::ZoomOut,
        _ => CursorIcon::Default,
    }
}

impl CanvasView {
    /// AppView's canvas listeners for this frame's events: presses on the visible canvas capture
    /// the pointer and go to `AppWorkspace::on_mouse_down` (the middle button pans), moves while
    /// over it or captured to `on_mouse_move` (one call per frame, every move a sample), and
    /// every captured button's release to `on_mouse_up`; wheel scroll and Ctrl+wheel zoom over it
    /// (`ui-drawn-graphics.md §2.10` "Wheel"); auto-scroll while captured. Registered before
    /// the chrome, so anything painted over the canvas takes the pointer first. Returns whether
    /// the pointer is over the canvas.
    pub fn input(
        &mut self,
        ui: &mut Ui,
        at: CanvasInput,
        app: &mut AppWorkspace,
        initial_buttons: u8,
        mut keyboard: impl FnMut(usize, &Event, &mut AppWorkspace),
    ) -> bool {
        let response = ui.interact(
            at.visible,
            Id::unique("canvas-input"),
            Sense::click_and_drag(),
        );
        let over = |p: Pos2| at.visible.contains(p) && response.hovered();
        let to_canvas = |p: Pos2| ((p.x - at.origin.x) as f64, (p.y - at.origin.y) as f64);
        let now = app.now_ms;
        let enabled = self.pen.enabled;
        let sample = |p: Pos2, metadata: crate::platform::pen::PointerMetadata| {
            let (x, y) = to_canvas(p);
            PointerSample {
                x,
                y,
                pressure: pdn_core::app::get_pointer_pressure(
                    metadata.pointer_type,
                    metadata.pressure,
                    enabled,
                ),
                time_ms: metadata.time_ms,
            }
        };
        let input = |samples: Vec<PointerSample>, pointer_type| PointerInput {
            pointer_type,
            samples,
        };
        let hover = ui.input(|i| i.pointer.hover_pos());
        let was_over = app.zoom_anchor.is_some();
        app.zoom_anchor = hover.filter(|&p| over(p)).map(to_canvas);
        if was_over && app.zoom_anchor.is_none() {
            // The tools' `on_frame` sees the pointer leave (the brush outline hides) next frame.
            ui.ctx().request_repaint();
        }
        let mut moves: Vec<PointerSample> = vec![];
        let mut move_type = PointerType::Mouse;
        let flush = |moves: &mut Vec<PointerSample>, pointer_type, app: &mut AppWorkspace| {
            if let Some(last) = moves.last().copied() {
                app.fire(AppEvent::MouseMove {
                    x: last.x,
                    y: last.y,
                });
                app.on_mouse_move(last.x, last.y, &input(std::mem::take(moves), pointer_type));
            }
        };
        let ppp = ui.ctx().pixels_per_point() as f64;
        let events = ui.input(|i| i.events.clone());
        let final_buttons = app.mouse_buttons;
        for (index, event) in events.iter().cloned().enumerate() {
            let metadata = self.pen.metadata(index, &event, now);
            match event {
                Event::Key { .. } | Event::Text(_) | Event::Paste(_) | Event::Ime(_) => {
                    flush(&mut moves, move_type, app);
                    app.mouse_buttons = physical_buttons_at(&events, index, initial_buttons);
                    keyboard(index, &event, app);
                    app.mouse_buttons = final_buttons;
                }
                Event::PointerMoved(p) if self.buttons != 0 || over(p) => {
                    if move_type != metadata.pointer_type {
                        flush(&mut moves, move_type, app);
                    }
                    move_type = metadata.pointer_type;
                    self.pen.accept(metadata);
                    moves.push(sample(p, metadata));
                }
                Event::PointerButton { pos, .. } => {
                    flush(&mut moves, move_type, app);
                    let packet = input(vec![sample(pos, metadata)], metadata.pointer_type);
                    if self.buttons != 0 || over(pos) {
                        self.pen.accept(metadata);
                    }
                    self.dispatch_pointer(&event, packet, app, over(pos), now);
                }
                // The actual button-up may go to another window. End each held button here.
                Event::WindowFocused(false) | Event::PointerGone if self.buttons != 0 => {
                    flush(&mut moves, move_type, app);
                    self.lose_capture(app);
                }
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if hover.is_some_and(over) => {
                    let pivot = to_canvas(hover.unwrap_or_default());
                    let Some(ws) = app.active_ws_mut() else {
                        continue;
                    };
                    let dy = delta.y as f64;
                    // quirk: paintdotjs reads `event.ctrlKey` only, no Ctrl↔Cmd mapping (macOS;
                    // Paint.NET has no counterpart).
                    let only = |shift: bool, ctrl: bool| {
                        (modifiers.shift, modifiers.ctrl, modifiers.alt) == (shift, ctrl, false)
                    };
                    if only(false, true) {
                        // Steps 2–3: one notch per wheel line, or per 100 px of a pixel delta
                        // (Chromium's notch); the pivot is the pointer over the viewport.
                        let notches = match unit {
                            MouseWheelUnit::Point => dy / 100.0,
                            MouseWheelUnit::Line | MouseWheelUnit::Page => dy,
                        };
                        ws.view.wheel_zoom(notches, pivot);
                    } else if !ws.view.zoom_to_window && (only(false, false) || only(true, false)) {
                        // Step 1: Paint.NET scrolls 120 device px per notch (a page as one notch);
                        // pixel deltas scroll by themselves (paintdotjs's `scrollTop += deltaY`).
                        // Shift scrolls horizontally.
                        let p = ppp as f32;
                        let d = crate::kit::scroll::wheel_points(unit, delta, p, 120.0 / p);
                        let (dx, dy) = (d.x as f64, d.y as f64);
                        let (sx, sy) = if modifiers.shift {
                            (if dy != 0.0 { dy } else { dx }, 0.0)
                        } else {
                            (dx, dy)
                        };
                        let v = &mut ws.view;
                        v.set_view_position(v.viewport_x - sx, v.viewport_y - sy);
                    }
                }
                _ => {}
            }
        }
        flush(&mut moves, move_type, app);
        // `runAutoScrollFrame` runs every animation frame while the pointer is down; frames are
        // needed only while it can scroll.
        if self.buttons != 0 && self.auto_scroll(at, app) {
            ui.ctx().request_repaint();
        }
        hover.is_some_and(over) || self.buttons != 0
    }

    /// One original pointer event and its event-local samples. Task36 supplies pressure here.
    fn dispatch_pointer(
        &mut self,
        event: &Event,
        input: PointerInput,
        app: &mut AppWorkspace,
        over: bool,
        now: f64,
    ) {
        let Event::PointerButton {
            button,
            pressed,
            modifiers,
            ..
        } = event
        else {
            return;
        };
        let Some(p) = input.samples.last() else {
            return;
        };
        let (x, y) = (p.x, p.y);
        app.set_modifier_keys(modifiers.ctrl, modifiers.shift, modifiers.alt);
        let b = mouse_button(*button);
        let bit = 1 << b as u8;
        if *pressed && self.buttons & bit == 0 && (self.buttons != 0 || over) {
            if self.buttons == 0 {
                self.last_auto_scroll = now;
            }
            self.buttons |= bit;
            app.fire(AppEvent::MouseDown {
                x,
                y,
                button: b as i32,
            });
            app.on_mouse_down(x, y, b, &input);
        } else if !*pressed && self.buttons & bit != 0 {
            self.buttons &= !bit;
            app.fire(AppEvent::MouseUp {
                x,
                y,
                button: b as i32,
            });
            app.on_mouse_up(x, y, b, &input);
        }
    }

    /// The pointer capture is lost mid-press (focus loss; a modal dialog opening, which takes
    /// every input): each held button is released at the pointer's last canvas position with no
    /// new point, as Windows synthesizes it (painting-tools-1.md §8.6 "Focus loss", §8.11: a
    /// stroke commits, never cancels).
    pub fn lose_capture(&mut self, app: &mut AppWorkspace) {
        self.pen.reset();
        let buttons = std::mem::take(&mut self.buttons);
        let (x, y) = app.last_mouse;
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        for b in MouseButton::ALL {
            if buttons & (1 << b as u8) != 0 {
                app.fire(AppEvent::MouseUp {
                    x,
                    y,
                    button: b as i32,
                });
                app.on_mouse_up(x, y, b, &input);
            }
        }
    }

    /// `runAutoScrollFrame` → `autoScrollIfNecessary`: while a tool drags (not the middle pan),
    /// the view scrolls at 2000 px/s towards the pointer once it is 2 % past the `#view` edge, and
    /// the stationary pointer is moved again. True while the pointer is in that zone.
    fn auto_scroll(&mut self, at: CanvasInput, app: &mut AppWorkspace) -> bool {
        let elapsed = ((app.now_ms - self.last_auto_scroll) / 1000.0).clamp(0.0, 0.1);
        self.last_auto_scroll = app.now_ms;
        if !at.auto_scroll || !app.tool_active || app.pan_tool.is_tracking() {
            return false;
        }
        let (x, y) = app.last_mouse;
        let view = at.view.translate(-at.origin.to_vec2());
        let bounds = (
            view.min.x as f64,
            view.min.y as f64,
            view.max.x as f64,
            view.max.y as f64,
        );
        let in_zone =
            pdn_core::document::view::auto_scroll_direction(Point::new(x, y), bounds).is_some();
        let Some(ws) = app.active_ws_mut() else {
            return false;
        };
        if ws.view.auto_scroll(Point::new(x, y), bounds, elapsed) {
            app.fire(AppEvent::MouseMove { x, y });
            let input = PointerInput {
                pointer_type: self.pen.state.pointer_type,
                samples: vec![PointerSample {
                    x,
                    y,
                    pressure: pdn_core::app::get_pointer_pressure(
                        self.pen.state.pointer_type,
                        self.pen.state.pressure,
                        self.pen.enabled,
                    ),
                    time_ms: app.now_ms,
                }],
            };
            app.on_mouse_move(x, y, &input);
        }
        in_zone
    }

    /// Spec §5.6: over the canvas the tool's cursor image replaces the OS cursor, as an OS
    /// (hardware) cursor so it moves at the compositor's rate rather than the frame rate
    /// ([`finish_cursor`] hands it over). A CSS request maps to the matching egui cursor.
    pub fn paint_cursor(ctx: &egui::Context, app: &AppWorkspace) {
        // S§4.3: a tool's background job (Remove) shows the busy cursor; the UI stays live.
        if app.tool_busy() {
            ctx.set_cursor_icon(CursorIcon::Progress);
            return;
        }
        match app.cursor() {
            CursorRequest::Css(keyword) => ctx.set_cursor_icon(css_cursor(keyword)),
            CursorRequest::None => ctx.set_cursor_icon(CursorIcon::None),
            CursorRequest::Image(name) => {
                // The crosshair shows where the bitmap can't (an unresolved cursor, a backend
                // without bitmap cursors).
                ctx.set_cursor_icon(CursorIcon::Crosshair);
                if let Some(image) = crate::assets::Assets::native_cursor(ctx, name) {
                    let pass = ctx.cumulative_pass_nr();
                    ctx.data_mut(|d| d.insert_temp(tool_cursor_id(), ToolCursor(pass, image)));
                }
            }
        }
    }
}

/// Perspective Warp's plane over the canvas (`tool::perspective_warp`): its outline, black under
/// white, and its corners as the Move tool's square handles (white ring, black ring; 6 DIP), plus
/// the plane being drawn; `origin` is the canvas texture's top-left.
fn paint_perspective(painter: &Painter, origin: Pos2, app: &AppWorkspace) {
    let (Some(p), Some(ws)) = (&app.perspective, app.active_ws()) else {
        return;
    };
    let screen = |c: [f64; 2]| {
        let s = ws.view.to_screen_position(Point::new(c[0], c[1]));
        origin + Vec2::new(s.x as f32, s.y as f32)
    };
    let outline = |q: [[f64; 2]; 4]| {
        let pts: Vec<Pos2> = q.iter().map(|c| screen(*c)).collect();
        for (w, c) in [(2.0, Color32::BLACK), (1.0, Color32::WHITE)] {
            painter.add(egui::Shape::closed_line(
                pts.clone(),
                egui::Stroke::new(w, c),
            ));
        }
    };
    let quad = p.quad();
    outline(quad);
    if let Some((a, b)) = p.new_plane() {
        outline([a, [b[0], a[1]], b, [a[0], b[1]]]);
    }
    for c in quad {
        let r = Rect::from_center_size(screen(c), Vec2::splat(6.0));
        for (w, color) in [(3.5, Color32::WHITE), (1.0, Color32::BLACK)] {
            let stroke = egui::Stroke::new(w, color);
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
        }
    }
}

/// The OS cursor bitmap a tool asked for in a pass.
#[derive(Clone)]
struct ToolCursor(u64, egui::CustomCursorImage);

fn tool_cursor_id() -> Id {
    Id::unique("paintrs.tool-cursor")
}

/// At the end of the frame: the tool's cursor bitmap from [`CanvasView::paint_cursor`], unless
/// something painted later changed the cursor. Every pass starts with no `cursor_image` (egui's
/// `end_pass` takes the whole `PlatformOutput`), so the bitmap is handed over on every frame it
/// shows; egui-winit keeps its OS cursor while the `Arc` stays the same.
pub fn finish_cursor(ctx: &egui::Context) -> Option<egui::CustomCursorImage> {
    let pass = ctx.cumulative_pass_nr();
    let requested = ctx
        .data(|d| d.get_temp::<ToolCursor>(tool_cursor_id()))
        .filter(|c| c.0 == pass);
    let image = requested
        .filter(|_| ctx.output(|o| o.cursor_icon) == CursorIcon::Crosshair)
        .map(|c| c.1);
    ctx.output_mut(|o| o.cursor_image = image.clone());
    image
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The first glyph's baseline below the galley's top.
fn galley_baseline(galley: &egui::Galley) -> f32 {
    galley.rows.first().map_or(0.0, |r| {
        r.pos.y + r.row.glyphs.first().map_or(r.row.size.y, |g| g.pos.y)
    })
}

/// Device pixels whose centre lies in [x0, x1), within 0..n (the D2D aliased rule).
fn covered(x0: f64, x1: f64, n: u32) -> std::ops::Range<u32> {
    let pix = |v: f64| (v - 0.5).ceil().clamp(0.0, n as f64) as u32;
    pix(x0)..pix(x1)
}

/// §2.11 steps 3, 7–9 in device px along × across: the background, the selection highlight
/// (lerp(white, Highlight, 0.8), its ticks in HighlightText), the mouse marker (HotTrack at 50 %,
/// lerp(Highlight, black, 0.5) at 50 % over the highlight), 1-px aliased ticks at x + 0.5 from
/// y = H − 1 upwards, and the border row y = H − 0.5. The vertical ruler is the same strip
/// transposed.
fn ruler_image(ticks: &RulerTicks, key: &RulerKey) -> ColorImage {
    let (len, thick) = (key.len, key.thick);
    let colors = key.colors;
    let mut strip = vec![rgb(colors.background); (len * thick) as usize];
    let mut column = |a: u32, rows: std::ops::Range<u32>, c: Color32| {
        for r in rows {
            strip[(r * len + a) as usize] = c;
        }
    };
    let highlight = key.highlight.map_or(0..0, |(x0, x1)| covered(x0, x1, len));
    let highlight_bg = RulerColors::highlight_background();
    for a in highlight.clone() {
        column(a, 0..thick, rgb(highlight_bg));
    }
    if let Some((x0, x1)) = key.marker {
        // An 8-bit sRGB target: 50 % of the marker colour over the background.
        let mix = |c: [u8; 3], b: [u8; 3]| {
            let m = |i: usize| {
                pdn_core::pixel::gamma::quantize(
                    c[i] as f32 / 255.0 * 0.5 + b[i] as f32 / 255.0 * 0.5,
                )
            };
            Color32::from_rgb(m(0), m(1), m(2))
        };
        let dark = HIGHLIGHT.map(|c| (c as f64 * 0.5).round() as u8);
        for a in covered(x0, x1, len) {
            let c = if highlight.contains(&a) {
                mix(dark, highlight_bg)
            } else {
                mix(HOT_TRACK, colors.background)
            };
            column(a, 0..thick, c);
        }
    }
    let h = thick as f64;
    let marks = ticks
        .majors
        .iter()
        .map(|m| (m.x, m.height))
        .chain(ticks.minors.iter().map(|m| (m.x, m.height)));
    for (x, height) in marks {
        for a in covered(x, x + 1.0, len) {
            let c = if highlight.contains(&a) {
                HIGHLIGHT_TEXT
            } else {
                colors.tick
            };
            column(a, covered(h - 1.0 - height, h - 1.0, thick), rgb(c));
        }
    }
    for a in 0..len {
        column(a, thick - 1..thick, rgb(colors.tick));
    }
    if !key.vertical {
        return ColorImage::new([len as usize, thick as usize], strip);
    }
    let mut out = Vec::with_capacity(strip.len());
    for a in 0..len {
        for c in 0..thick {
            out.push(strip[(c * len + a) as usize]);
        }
    }
    ColorImage::new([thick as usize, len as usize], out)
}

/// Calibri's (Carlito's) win metrics: DirectWrite puts the baseline `ascent` below the box top.
static CALIBRI: LazyLock<WinMetrics> = LazyLock::new(|| {
    pdn_core::fonts::win_metrics(pdn_core::fonts::CARLITO_REGULAR).expect("Carlito metrics")
});

/// §2.11 step 4: a major's label in Calibri (Carlito) 10.667 DIP: horizontal at (x + 2, −1) as the
/// layout box's top-left; vertical rotated −90° with the text end at y = ⌊x + 2⌋ and the box's top
/// at x = −1. The baseline snaps to whole device px. Records nothing: the rulers are pdn rects.
// calibrate: B-C6 — label positions, font and AA mode.
fn draw_label(
    painter: &Painter,
    origin: Pos2,
    x: f64,
    label: &str,
    color: [u8; 3],
    vertical: bool,
) {
    let ppp = painter.pixels_per_point();
    let m = *CALIBRI;
    let em = |v: u16| v as f32 / m.units_per_em as f32;
    let baseline = (-1.0 + RULER_LABEL_FONT_SIZE * ppp * em(m.ascent)).round() / ppp;
    let along = (x + 2.0).floor() as f32 / ppp;
    let spec = FontSpec::new(RULER_LABEL_FONT_SIZE).face("Calibri", em(m.ascent), em(m.descent));
    if vertical {
        let width = text::measure(painter, label, spec);
        let at = origin + Vec2::new(baseline, along + width);
        text::draw_rotated_ccw(painter, at, label, spec, rgb(color));
        return;
    }
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        FontId::new(RULER_LABEL_FONT_SIZE, FontFamily::Name("Calibri".into())),
        rgb(color),
    );
    let pos = origin + Vec2::new(along, baseline - galley_baseline(&galley));
    painter.galley(pos, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdn_core::render::ruler::{Major, Minor};

    #[test]
    fn pen_pressure_auto_scroll_uses_last_accepted_sample_and_current_setting() {
        use std::{cell::RefCell, rc::Rc};
        struct Probe(Rc<RefCell<Vec<PointerInput>>>);
        impl pdn_core::tool::Tool for Probe {
            fn tool_type(&self) -> pdn_core::tool::ToolType {
                pdn_core::tool::ToolType::PaintBrush
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
            fn on_mouse_move(
                &mut self,
                _: &mut AppWorkspace,
                _: f64,
                _: f64,
                p: &PointerInput,
            ) -> bool {
                self.0.borrow_mut().push(p.clone());
                true
            }
        }
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(1000, 1000, 96.0, true);
        let doc = app.active_ws_mut().unwrap();
        doc.view.on_resize(100.0, 100.0);
        doc.view.set_zoom_to_window(false);
        let seen = Rc::new(RefCell::new(vec![]));
        app.tool = Some(Box::new(Probe(seen.clone())));
        app.tool_active = true;
        app.last_mouse = (110.0, 50.0);
        let mut canvas = CanvasView::default();
        canvas.pen.enabled = true;
        canvas.pen.accept(crate::platform::pen::PointerMetadata {
            pointer_type: PointerType::Pen,
            pressure: Some(0.25),
            time_ms: 0.0,
        });
        let at = || CanvasInput {
            visible: Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 100.0)),
            origin: Pos2::ZERO,
            view: Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 100.0)),
            auto_scroll: true,
        };
        app.now_ms = 16.0;
        assert!(canvas.auto_scroll(at(), &mut app));
        canvas.pen.enabled = false;
        app.now_ms = 32.0;
        assert!(canvas.auto_scroll(at(), &mut app));
        let seen = seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].pointer_type, PointerType::Pen);
        assert_eq!(seen[0].samples[0].pressure, Some(0.25));
        assert_eq!(seen[1].samples[0].pressure, None);
    }

    #[test]
    fn text_caret_fill_outline_and_alpha_at_fractional_zoom() {
        for zoom in [0.25, 1.0, 2.0] {
            for ppp in [1.0, 2.0] {
                let ctx = egui::Context::default();
                let b = Rect::from_min_size(
                    egui::pos2(10.0, 20.0) * (zoom / ppp),
                    egui::vec2(2.0, 12.0) * (zoom / ppp),
                );
                let mut output = ctx.run_ui(Default::default(), |ui| {
                    paint_text_caret(ui.painter(), b, ppp, 127);
                });
                output.textures_delta.clear();
                let rects: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| {
                        if let egui::Shape::Rect(r) = &s.shape {
                            Some(r)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(rects.len(), 2);
                assert_eq!(rects[0].rect, b); // zoomed-out black fill is not clamped to a pixel
                assert_eq!(rects[0].fill, Color32::from_black_alpha(127));
                assert_eq!(rects[1].rect, b.expand(1.0 / ppp));
                assert_eq!(rects[1].stroke.width, 1.0 / ppp);
                assert_eq!(rects[1].stroke.color, Color32::from_white_alpha(127));
                assert_eq!(rects[1].stroke_kind, egui::StrokeKind::Middle);
            }
        }
    }

    /// Ticks cover column ⌈x − ½⌉ from the row above the border upwards, the border is the last
    /// row, the marker blends HotTrack at 50 %; the vertical strip is the transpose.
    /// A span's pieces in a wrapped axis: the texels after the wrap, then from the start.
    #[test]
    fn wrapped_spans_split_at_the_end() {
        assert_eq!(wrapped(2, 5, 0, 10), [(2, 2, 5)]);
        assert_eq!(wrapped(2, 5, 4, 10), [(2, 6, 4), (6, 0, 1)]);
        assert_eq!(wrapped(0, 10, 7, 10), [(0, 7, 3), (3, 0, 7)]);
        assert_eq!(wrapped(5, 3, 2, 10), [(5, 7, 3)]);
    }

    #[test]
    fn ruler_strip_pixels() {
        let ticks = RulerTicks {
            majors: vec![Major {
                x: 9.0,
                value: 0.0,
                height: 16.0,
            }],
            minors: vec![Minor {
                x: 4.0,
                level: 2,
                height: 5.6,
            }],
        };
        let mut key = RulerKey {
            len: 20,
            thick: 17,
            vertical: false,
            colors: RulerColors::for_scheme(true),
            view: None,
            marker: Some((12.0, 14.0)),
            highlight: None,
        };
        let img = ruler_image(&ticks, &key);
        let at = |x: usize, y: usize| img.pixels[y * 20 + x].r();
        let (bg, tick, mark) = (0x20, 0x80, 16);
        assert_eq!(
            [at(9, 0), at(9, 15), at(9, 16), at(8, 5)],
            [tick, tick, tick, bg]
        );
        assert_eq!(
            [at(4, 9), at(4, 10), at(4, 15), at(5, 12)],
            [bg, tick, tick, bg]
        );
        assert_eq!(
            [at(11, 3), at(12, 3), at(13, 3), at(14, 3), at(13, 16)],
            [bg, mark, mark, bg, tick]
        );
        // §2.11 step 8: over the highlight (columns 3–14) the background is lerp(white,
        // Highlight, 0.8), ticks are HighlightText and the marker is lerp(Highlight, black, 0.5)
        // at 50 %; outside it nothing changes.
        key.highlight = Some((3.0, 15.0));
        let hl = ruler_image(&ticks, &key);
        let px = |x: usize, y: usize| hl.pixels[y * 20 + x];
        assert_eq!(px(5, 3), Color32::from_rgb(51, 147, 223));
        assert_eq!([px(9, 0), px(9, 15), px(4, 12)], [Color32::WHITE; 3]);
        assert_eq!(px(12, 3), Color32::from_rgb(26, 104, 166));
        assert_eq!(
            (px(2, 3), px(15, 3), px(9, 16)),
            (
                img.pixels[3 * 20 + 2],
                img.pixels[3 * 20 + 15],
                img.pixels[16 * 20 + 9]
            )
        );
        key.highlight = None;
        key.vertical = true;
        let v = ruler_image(&ticks, &key);
        assert_eq!(v.size, [17, 20]);
        assert_eq!(
            (v.pixels[9 * 17 + 3].r(), v.pixels[4 * 17 + 12].r()),
            (tick, tick)
        );
    }
}
