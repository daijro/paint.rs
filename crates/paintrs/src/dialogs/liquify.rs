//! Ported from PhotoCraft crates/ui-egui/src/liquify_ui.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only the stroke recording (`begin` / `extend` / `end`, the held-still dabs at 30 a second, Alt reversing Twirl and swapping Pucker and Bloat), the brush outline, the freeze-mask tint (blended into the preview instead of a second texture) and the dirty-rect proxy upload; the Effect Config kit's frame, rows, radio buttons and OK / Cancel instead of upstream's full-window workspace; no zoom, pan, backdrop, mesh, lasso, Density, Rate, Reconstruct / Restore All buttons or in-dialog undo; OK hands the strokes to the Effect Config dialog's final render (`EffectConfigDialog::final_only`) instead of running a command.
//!
//! Effects ▸ Distort ▸ Liquify (an Extra): a large preview of the layer, brushed with Forward
//! Warp, Twirl, Pucker, Bloat, Freeze, Thaw and Reconstruct at a Size and Pressure. The brushes
//! edit the displacement field directly, and only the proxy pixels a dab changed are re-rendered
//! and uploaded; layers past `PROXY_SIDE` preview downsampled. OK applies the recorded strokes at
//! full resolution as one history item named "Liquify" (Effect Config's cancellable "Applying"
//! step); Cancel changes nothing.
//! Photocraft phase spec: S§4.4 "Interactive and Tier 3 items"; S§4.1 (the title's plus).

use egui::{
    Color32, ColorImage, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2,
};
use pdn_core::effect::extra::liquify::field;
use pdn_core::effect::session::EffectSession;
use pdn_core::effect::slider::{RangeRole, SliderKeys};
use pdn_core::effect::{Values, num};
use pdn_core::extra::fx::liquify::{LiquifyField, LiquifyStroke, LiquifyTool, ProxyImage};
use pdn_core::render::checker::{checker_colors, checker_is_light};
use serde_json::json;

use super::effect_config::{
    EffectConfigDialog, RangeSpec, SliderState, Track, paint_title_plus, range_line, range_rects,
};
use super::image_size::{footer_pair, rule};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::px;
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{Footer, dialog_frame_with};
use crate::theme::{self, *};

/// The brushes, in the strip's order, with their labels.
const TOOLS: [(LiquifyTool, &str); 7] = [
    (LiquifyTool::ForwardWarp, "Forward Warp"),
    (LiquifyTool::TwirlCw, "Twirl"),
    (LiquifyTool::Pucker, "Pucker"),
    (LiquifyTool::Bloat, "Bloat"),
    (LiquifyTool::Freeze, "Freeze"),
    (LiquifyTool::Thaw, "Thaw"),
    (LiquifyTool::Reconstruct, "Reconstruct"),
];
/// Size (the brush diameter, px) and Pressure (%): key, label, min, max (upstream's sliders).
const SIZE: (&str, &str, f64, f64) = ("size", "Size", 1.0, 3000.0);
const PRESSURE: (&str, &str, f64, f64) = ("pressure", "Pressure", 0.0, 100.0);
/// The preview's longer side: larger layers preview downsampled (upstream's `PROXY_SIDE`, under
/// 2 MP at 4:3); OK still applies at full resolution.
const PROXY_SIDE: usize = 1600;
/// The options column's width and the largest body (border-box width, content height), DIP.
const PANEL_W: f32 = 236.0;
const MAX_BODY: Vec2 = Vec2::new(1000.0, 640.0);
/// A brush radio row's height and the gap between its radio and label, DIP.
const RADIO_ROW: f32 = 22.0;
const RADIO_GAP: f32 = 7.0;
/// A stationary brush held still dabs every this many ms (upstream: 30 a second).
const HELD_DAB_MS: f64 = 33.0;
/// The transparency checkerboard's square, proxy pixels.
const CHECKER: u32 = 8;
/// Upstream's freeze-mask overlay: this red at 140/255.
const FROZEN: [f32; 3] = [230.0, 40.0, 40.0];
const FROZEN_ALPHA: f32 = 140.0 / 255.0;

/// Upstream's Alt variant of a brush: the twirl reverses, Pucker and Bloat swap.
fn alt_tool(t: LiquifyTool) -> LiquifyTool {
    match t {
        LiquifyTool::TwirlCw => LiquifyTool::TwirlCcw,
        LiquifyTool::TwirlCcw => LiquifyTool::TwirlCw,
        LiquifyTool::Pucker => LiquifyTool::Bloat,
        LiquifyTool::Bloat => LiquifyTool::Pucker,
        t => t,
    }
}

/// Two proxy rects `[x0, y0, x1, y1]` joined; an empty `b` adds nothing.
fn join(a: Option<[usize; 4]>, b: [usize; 4]) -> Option<[usize; 4]> {
    if b[0] >= b[2] || b[1] >= b[3] {
        return a;
    }
    Some(a.map_or(b, |a| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    }))
}

pub struct LiquifyDialog {
    session: Option<EffectSession>,
    title: String,
    icon: &'static str,
    /// Size and Pressure as the rows edit them (the remembered token's, else the defaults); OK
    /// adds the strokes.
    values: Values,
    /// Size's opening default: a tenth of the layer's longer side (upstream).
    size_default: f64,
    tool: LiquifyTool,
    field: LiquifyField,
    strokes: Vec<LiquifyStroke>,
    /// The stroke being drawn and its last point.
    cur: Option<(LiquifyStroke, [f64; 3])>,
    /// When the last dab landed (app ms).
    last_dab: f64,
    proxy: ProxyImage,
    /// The proxy through the field, premultiplied RGBA8.
    out: Vec<[u8; 4]>,
    tex: Option<TextureHandle>,
    /// Proxy pixels to re-render and upload, `[x0, y0, x1, y1]`.
    dirty: Option<[usize; 4]>,
    slider: SliderState,
}

impl LiquifyDialog {
    /// Liquify over `session`'s layer with `values` (the remembered token or the defaults): a
    /// fresh field each time, Size and Pressure as last used.
    pub fn new(session: EffectSession, mut values: Values) -> Self {
        let def = &session.def;
        let title = def
            .dialog_title
            .map_or_else(|| def.display_name(), String::from);
        let icon = def.icon;
        let src = session.source();
        let field = field(src.width, src.height);
        let proxy = ProxyImage::new(&src.data, field.bounds, PROXY_SIDE);
        let size_default = (f64::from(src.width.max(src.height)) / 10.0)
            .round()
            .clamp(10.0, 1500.0);
        values.remove("strokes");
        values.entry(SIZE.0).or_insert(json!(size_default));
        values.entry(PRESSURE.0).or_insert(json!(PRESSURE.3));
        LiquifyDialog {
            title,
            icon,
            values,
            size_default,
            tool: LiquifyTool::ForwardWarp,
            field,
            strokes: vec![],
            cur: None,
            last_dab: 0.0,
            out: vec![[0; 4]; proxy.w * proxy.h],
            proxy,
            tex: None,
            dirty: None,
            slider: SliderState::default(),
            session: Some(session),
        }
    }

    /// A stroke of `tool` at the current Size and Pressure.
    fn template(&self, tool: LiquifyTool) -> LiquifyStroke {
        LiquifyStroke {
            pressure: num(&self.values, PRESSURE.0),
            ..LiquifyStroke::new(tool, num(&self.values, SIZE.0).max(1.0))
        }
    }

    fn mark(&mut self, r: pdn_core::extra::geom::Rect) {
        if !r.is_empty() {
            self.dirty = join(self.dirty, self.proxy.proxy_rect(r));
        }
    }

    /// Starts a stroke at `p` (document px and pressure); `alt` uses the brush's Alt variant.
    fn begin(&mut self, p: [f64; 3], now: f64, alt: bool) {
        self.end();
        let tool = if alt { alt_tool(self.tool) } else { self.tool };
        let mut s = self.template(tool);
        s.points.push(p.to_vec());
        let d = self.field.stroke_begin(&s, p);
        self.mark(d);
        self.cur = Some((s, p));
        self.last_dab = now;
    }

    fn extend(&mut self, p: [f64; 3], now: f64) {
        let Some((mut s, last)) = self.cur.take() else {
            return;
        };
        let d = self.field.stroke_segment(&s, last, p);
        s.points.push(p.to_vec());
        self.mark(d);
        self.cur = Some((s, p));
        self.last_dab = now;
    }

    fn end(&mut self) {
        if let Some((s, _)) = self.cur.take() {
            self.strokes.push(s);
        }
    }

    /// Re-renders the dirty proxy pixels and uploads them (the whole proxy the first time).
    fn upload(&mut self, ctx: &egui::Context, checker: (u8, u8)) {
        let (w, h) = (self.proxy.w, self.proxy.h);
        let dirty = if self.tex.is_none() {
            Some([0, 0, w, h])
        } else {
            self.dirty
        };
        self.dirty = None;
        let Some([x0, y0, x1, y1]) = dirty.map(|r| [r[0], r[1], r[2].min(w), r[3].min(h)]) else {
            return;
        };
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        self.proxy
            .render(&self.field, [x0, y0, x1, y1], &mut self.out);
        let pixels = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| self.shown(x, y, checker))
            .collect();
        let image = ColorImage::new([x1 - x0, y1 - y0], pixels);
        match &mut self.tex {
            Some(t) => t.set_partial([x0, y0], image, TextureOptions::LINEAR),
            None => {
                self.tex = Some(ctx.load_texture("liquify-preview", image, TextureOptions::LINEAR));
            }
        }
    }

    /// Proxy pixel (x, y) as shown: the warped layer, kept to the selection as OK's clipping
    /// keeps it, over the checkerboard; frozen areas tinted with upstream's mask red.
    fn shown(&self, x: usize, y: usize, (light, dark): (u8, u8)) -> Color32 {
        let i = y * self.proxy.w + x;
        let (s, b) = (self.proxy.scale, self.proxy.bounds);
        let (dx, dy) = (
            f64::from(b.x0) + (x as f64 + 0.5) * s,
            f64::from(b.y0) + (y as f64 + 0.5) * s,
        );
        let selected = self
            .session
            .as_ref()
            .and_then(EffectSession::mask)
            .map_or(1.0, |m| f32::from(m.get(dx as i32, dy as i32)) / 255.0);
        let p: [f32; 4] = std::array::from_fn(|c| {
            let warped = f32::from(self.out[i][c]);
            warped + (self.proxy.px[i][c] * 255.0 - warped) * (1.0 - selected)
        });
        let bg = f32::from(if checker_is_light(x as i32, y as i32, CHECKER) {
            light
        } else {
            dark
        });
        let frozen = self.field.freeze_at(dx, dy).clamp(0.0, 1.0) * FROZEN_ALPHA;
        let c: [u8; 3] = std::array::from_fn(|c| {
            let v = p[c] + bg * (1.0 - p[3] / 255.0);
            (v + (FROZEN[c] - v) * frozen + 0.5) as u8
        });
        Color32::from_rgb(c[0], c[1], c[2])
    }

    /// The border-box width and content height: up to `MAX_BODY`, inside the window.
    pub(super) fn size(ctx: &egui::Context) -> Vec2 {
        let screen = ctx.content_rect();
        let chrome = DIALOG_TITLE_BAR + DIALOG_FOOTER_HEIGHT + 2.0 * DIALOG_BORDER;
        Vec2::new(
            MAX_BODY.x.min(screen.width() - DIALOG_VIEWPORT_MARGIN),
            MAX_BODY
                .y
                .min(screen.height() - DIALOG_VIEWPORT_MARGIN - chrome),
        )
    }

    /// The body: the preview, then the options column on the right.
    fn body(&mut self, ui: &mut Ui, r: Rect, now: f64) {
        let (top, side, bottom) = EFFECT_CONTENT_PADDING;
        let inner = Rect::from_min_max(
            Pos2::new(r.min.x + side, r.min.y + top),
            Pos2::new(r.max.x - side, r.max.y - bottom),
        );
        let preview = Rect::from_min_max(
            inner.min,
            Pos2::new(inner.max.x - PANEL_W - side, inner.max.y),
        );
        self.preview(ui, preview, now);
        let panel = Rect::from_min_max(Pos2::new(preview.max.x + side, inner.min.y), inner.max);
        self.options(ui, panel, now);
    }

    /// The preview: the layer fitted into `area` (at most ×4) with the brush outline under the
    /// pointer. A press starts a stroke, a drag extends it, and a stationary brush held still
    /// keeps dabbing.
    fn preview(&mut self, ui: &mut Ui, area: Rect, now: f64) {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, theme.background);
        let b = self.field.bounds;
        let (cw, ch) = (b.width().max(1) as f32, b.height().max(1) as f32);
        let zoom = (area.width() / cw).min(area.height() / ch).min(4.0);
        let image = Rect::from_center_size(area.center(), Vec2::new(cw * zoom, ch * zoom));
        if let Some(tex) = &self.tex {
            // The proxy's last row and column can reach past the layer: drawn clipped to it.
            let s = self.proxy.scale as f32 * zoom;
            let r = Rect::from_min_size(
                image.min,
                Vec2::new(self.proxy.w as f32 * s, self.proxy.h as f32 * s),
            );
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter
                .with_clip_rect(image.intersect(area))
                .image(tex.id(), r, uv, Color32::WHITE);
        }
        let response = ui.interact(
            area,
            ui.scope_id().with("liquify-preview"),
            Sense::click_and_drag(),
        );
        if let Some(p) = response.hover_pos() {
            let radius = num(&self.values, SIZE.0) as f32 * 0.5 * zoom;
            painter.circle_stroke(p, radius, Stroke::new(1.5, Color32::BLACK));
            painter.circle_stroke(p, radius, Stroke::new(0.75, Color32::WHITE));
        }
        let (down, alt) = ui.input(|i| (i.pointer.primary_down(), i.modifiers.alt));
        if let (true, Some(p)) = (down, response.interact_pointer_pos()) {
            let p = [
                f64::from((p.x - image.min.x) / zoom),
                f64::from((p.y - image.min.y) / zoom),
                1.0,
            ];
            if let Some((s, last)) = &self.cur {
                let moved = last[0] != p[0] || last[1] != p[1];
                let held = s.tool.is_stationary() && now - self.last_dab > HELD_DAB_MS;
                if moved || held {
                    self.extend(p, now);
                }
            } else if response.is_pointer_button_down_on() {
                self.begin(p, now, alt);
            }
            ui.ctx().request_repaint();
        }
        if !down {
            self.end();
        }
    }

    /// The options column: the brushes as radio rows, then Size and Pressure as Effect Config
    /// range rows.
    fn options(&mut self, ui: &mut Ui, r: Rect, now: f64) {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let (x0, w) = (r.min.x, r.width());
        // `.effect-config-row > label`: the text over a separator.
        let heading = |ui: &Ui, y: f32, s: &str| {
            text::draw(
                ui.painter(),
                Pos2::new(x0, y),
                s,
                spec,
                theme.menu_item_text,
            );
            rule(ui, x0, x0 + w, y + line + EFFECT_LABEL_PADDING);
            y + line + EFFECT_LABEL_PADDING + 1.0 + EFFECT_ROW_GAP
        };
        let mut y = heading(ui, r.min.y, "Brush");
        for (tool, name) in TOOLS {
            let radio = Rect::from_min_size(
                Pos2::new(x0, y + (RADIO_ROW - RADIO_SIZE) / 2.0),
                Vec2::splat(RADIO_SIZE),
            );
            let row = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, RADIO_ROW));
            let label = ui
                .interact(
                    px::snap_rect(row, ppp),
                    ui.scope_id().with(("brush-label", name)),
                    Sense::CLICK,
                )
                .clicked();
            let on = self.tool == tool;
            let clicked = inputs::radio(ui, ("brush", name), radio, on, theme.window_background);
            text::draw(
                ui.painter(),
                Pos2::new(radio.max.x + RADIO_GAP, y + (RADIO_ROW - line) / 2.0),
                name,
                spec,
                theme.menu_item_text,
            );
            if label || clicked {
                self.tool = tool;
            }
            y += RADIO_ROW;
        }
        self.slider.now = now;
        for (key, label, min, max) in [SIZE, PRESSURE] {
            y = heading(ui, y + EFFECT_CONTENT_GAP, label);
            let default = if key == SIZE.0 {
                self.size_default
            } else {
                max
            };
            let spec = RangeSpec {
                min,
                max,
                default,
                step: 1.0,
                role: RangeRole::Slider,
                keyboard: SliderKeys::INTEGER,
            };
            let (range, number, reset) = range_rects(x0, w, y);
            range_line(
                ui,
                &mut self.slider,
                key,
                spec,
                &mut self.values,
                range,
                number,
                Some(reset),
                Track::Progress(0.0),
                NumberStyle::EFFECT,
            );
            y += EFFECT_LINE_HEIGHT;
        }
    }

    /// OK: the strokes go to Effect Config's final render, which applies them at full resolution
    /// (cancellable) and commits one "Liquify" history item.
    fn ok(&mut self) -> DialogState {
        self.end();
        if let Some(s) = self.session.take() {
            let mut values = self.values.clone();
            values.insert("strokes".into(), json!(self.strokes));
            super::open(Box::new(EffectConfigDialog::final_only(s, values)));
        }
        DialogState::Closed(DialogResult::Ok)
    }

    /// Cancel: the layer and the dirty flag as they were; no history.
    fn cancel(&mut self, cx: &mut DialogCx) -> DialogState {
        if let Some(s) = self.session.take() {
            s.cancel(cx.app);
        }
        DialogState::Closed(DialogResult::Cancel)
    }
}

impl Dialog for LiquifyDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        // Plain Esc and Enter only, as in the Effect Config dialog.
        let plain = |key| {
            ui.input(|i| {
                !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.mac_cmd && i.key_pressed(key)
            })
        };
        if plain(Key::Escape) {
            return self.cancel(cx);
        }
        let enter = plain(Key::Enter);
        let now = cx.app.now_ms;
        let display =
            crate::shell::canvas_view::DisplayPrefs::new(cx.settings, &theme::current(&ctx));
        let checker = checker_colors(display.checker_brightness);
        // The texture exists before the body draws it; this frame's dabs upload after it.
        self.upload(&ctx, checker);
        let (title, icon) = (self.title.clone(), self.icon);
        let mut action = None;
        let mut body = |ui: &mut Ui, r: Rect| self.body(ui, r, now);
        let mut footer = |ui: &mut Ui, f: Rect| {
            let (fy, fx) = DIALOG_FOOTER_PADDING;
            let content = Rect::from_min_max(
                Pos2::new(f.min.x + fx, f.min.y + fy),
                Pos2::new(f.max.x - fx, f.max.y - fy),
            );
            action = footer_pair(ui, content, DIALOG_BUTTON_MIN.0, DIALOG_BUTTON_MIN.1, true);
        };
        let frame = dialog_frame_with(
            ui,
            &title,
            Some(icon),
            false,
            (Self::size(&ctx), &mut body),
            Some(&mut footer),
            Footer {
                border: false,
                ..Footer::DEFAULT
            },
        );
        paint_title_plus(
            ui,
            Some(pdn_core::effect::extra::liquify::ID),
            frame.rect,
            &title,
        );
        self.upload(&ctx, checker);
        if cx.seat_focus {
            super::seat_no_focus(&ctx);
        }
        match action {
            Some(true) => self.ok(),
            Some(false) => self.cancel(cx),
            None if frame.closed => self.cancel(cx),
            None if enter && !inputs::enter_taken(&ctx) => self.ok(),
            // Ruling R65: a press outside only flashes the dialog.
            None => DialogState::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::effect_config::title_plus_rect;
    use crate::kit::window::DialogFrameState;
    use egui::{Event, PointerButton};
    use pdn_core::app::AppWorkspace;
    use pdn_core::testing::{fresh_doc, layer_bytes};

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        ctx
    }

    fn with_cx<T>(app: &mut AppWorkspace, f: impl FnOnce(&mut DialogCx) -> T) -> T {
        f(&mut DialogCx {
            app,
            settings: &mut pdn_core::settings::AppSettings::load(None),
            prefs: &mut crate::prefs::Prefs::default(),
            settings_changed: false,
            shortcuts: None,
            clipboard: None,
            settings_imports: None,
            fonts: None,
            asset_library: None,
            seat_focus: false,
        })
    }

    fn input(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            events,
            ..Default::default()
        }
    }

    /// One frame of the dialog on its own: its state and border box.
    fn frame(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        d: &mut LiquifyDialog,
        events: Vec<Event>,
    ) -> (DialogState, Rect) {
        let mut result = (DialogState::Open, Rect::NOTHING);
        let mut out = ctx.run_ui(input(events), |ui| {
            result.0 = with_cx(app, |cx| d.show(ui, cx));
            result.1 = DialogFrameState::get(ctx, ui.scope_id()).rect;
        });
        out.textures_delta.clear();
        result
    }

    /// One frame of a dialog stack.
    fn stack_frame(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        stack: &mut super::super::DialogStack,
    ) {
        let mut out = ctx.run_ui(input(vec![]), |ui| with_cx(app, |cx| stack.show(ui, cx)));
        out.textures_delta.clear();
    }

    /// Liquify on a 64×48 noise layer, through the shell's `effect_dialog` path.
    fn session() -> (AppWorkspace, EffectSession) {
        super::super::PENDING.with(|p| p.borrow_mut().clear());
        let mut app = fresh_doc(64, 48, Some(3));
        let def = pdn_core::effect::find(pdn_core::effect::extra::liquify::ID).unwrap();
        let s = EffectSession::begin(&mut app, def).unwrap();
        (app, s)
    }

    fn dialog() -> (AppWorkspace, LiquifyDialog) {
        let (app, s) = session();
        let values = s.initial_values(&app);
        (app, LiquifyDialog::new(s, values))
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn key(key: Key) -> Vec<Event> {
        vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    /// A Forward Warp drag across the middle of the preview: press, three moves right, release.
    fn drag(ctx: &egui::Context, app: &mut AppWorkspace, d: &mut LiquifyDialog) {
        let (_, rect) = frame(ctx, app, d, vec![]);
        // The preview fills the body left of the options column; the layer sits in its middle.
        let start = Pos2::new(rect.min.x + (rect.width() - PANEL_W) / 2.0, rect.center().y);
        frame(
            ctx,
            app,
            d,
            vec![Event::PointerMoved(start), button(start, true)],
        );
        for dx in [10.0, 20.0, 40.0] {
            frame(
                ctx,
                app,
                d,
                vec![Event::PointerMoved(start + Vec2::new(dx, 0.0))],
            );
        }
        frame(
            ctx,
            app,
            d,
            vec![button(start + Vec2::new(40.0, 0.0), false)],
        );
    }

    /// S§4.4: Effects ▸ Distort ▸ Liquify opens this dialog, not the Effect Config one.
    #[test]
    fn effect_dialog_opens_liquify_for_its_effect() {
        let (mut app, s) = session();
        let values = s.initial_values(&app);
        let ctx = context();
        let mut stack = super::super::DialogStack::default();
        stack.push(super::super::effect_dialog(s, values, |_| {}));
        stack_frame(&ctx, &mut app, &mut stack);
        let rect = stack.top_rect(&ctx).unwrap();
        assert!(stack.top_title_is("Liquify"));
        assert_eq!(
            rect.width(),
            LiquifyDialog::size(&ctx).x,
            "the large preview's width"
        );
    }

    /// S§4.1: the dialog's title carries the plus 4 DIP after its text.
    #[test]
    fn liquify_dialog_title_has_plus() {
        let (mut app, mut d) = dialog();
        assert_eq!(d.title, "Liquify");
        let ctx = context();
        let image = image::RgbaImage::from_pixel(12, 16, image::Rgba([20, 200, 70, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        crate::assets::Assets::set_icon_override(&ctx, "plugin_indicator.png", png.into_inner());
        let mut expected = Rect::NOTHING;
        let mut output = ctx.run_ui(input(vec![]), |ui| {
            with_cx(&mut app, |cx| d.show(ui, cx));
            let frame = DialogFrameState::get(ui.ctx(), ui.scope_id()).rect;
            expected = px::snap_rect(title_plus_rect(ui.painter(), frame, &d.title), 1.0);
        });
        let meshes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => Some(m.calc_bounds()),
                _ => None,
            })
            .collect();
        output.textures_delta.clear();
        assert!(meshes.contains(&expected), "{expected:?} in {meshes:?}");
    }

    /// S§4.1: the menu item is under Distort.
    #[test]
    fn liquify_menu_item_is_under_distort() {
        use crate::kit::menu::MenuEntry;
        use crate::shell::main_menu::{ITEMS, entries};
        fn distort_has_liquify(entries: &[MenuEntry]) -> bool {
            entries.iter().any(|e| match e {
                MenuEntry::Submenu { key, entries } => {
                    *key == "effects.distort.submenu"
                        && entries.contains(&MenuEntry::Action("menu.effects.pcLiquify"))
                }
                _ => false,
            })
        }
        let effects = ITEMS.iter().position(|i| *i == "menu.effects").unwrap();
        assert!(distort_has_liquify(&entries(
            effects,
            &AppWorkspace::default()
        )));
    }

    /// Brushing edits only the preview; OK applies the strokes at full resolution as one history
    /// item named "Liquify", and undo restores the layer bit for bit.
    #[test]
    fn liquify_is_one_history_item() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        drag(&ctx, &mut app, &mut d);
        assert_eq!(d.strokes.len(), 1);
        assert!(d.strokes[0].points.len() >= 3, "{:?}", d.strokes[0].points);
        assert!(
            !d.field.is_identity(),
            "the preview's field follows the brush"
        );
        assert_eq!(layer_bytes(&app, None), before, "the layer waits for OK");
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Enter));
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        // Effect Config's final render applies the strokes on a worker.
        let mut stack = super::super::DialogStack::default();
        for _ in 0..1000 {
            stack_frame(&ctx, &mut app, &mut stack);
            if !stack.is_open() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!stack.is_open());
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.history.undo.len(), undo + 1);
        assert_eq!(ws.history.undo.last().unwrap().name, "Liquify");
        assert_ne!(layer_bytes(&app, None), before);
        let values = app.last_effect_values.as_ref().unwrap();
        assert_eq!(values["strokes"].as_array().map(Vec::len), Some(1));
        assert_eq!(app.last_effect.as_deref(), Some("pcLiquify"));
        app.undo();
        assert_eq!(layer_bytes(&app, None), before);
    }

    /// Cancel after brushing: the layer, history, dirty flag and Repeat are as they were.
    #[test]
    fn liquify_cancel_changes_nothing() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        drag(&ctx, &mut app, &mut d);
        assert!(!d.field.is_identity());
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Escape));
        assert_eq!(state, DialogState::Closed(DialogResult::Cancel));
        assert!(super::super::PENDING.with(|p| p.borrow().is_empty()));
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.history.undo.len(), ws.dirty), (undo, false));
        assert_eq!(layer_bytes(&app, None), before);
        assert!(app.last_effect.is_none());
    }

    /// Where the preview shows document point (x, y): the 64×48 layer at ×4 (the cap), centred
    /// in the preview, which is the body (below the title bar, inside the border) less its
    /// padding and the options column.
    fn screen_of(ctx: &egui::Context, rect: Rect, x: f32, y: f32) -> Pos2 {
        let body = Rect::from_min_size(
            rect.min + Vec2::new(DIALOG_BORDER, DIALOG_BORDER + DIALOG_TITLE_BAR),
            Vec2::new(
                rect.width() - 2.0 * DIALOG_BORDER,
                LiquifyDialog::size(ctx).y,
            ),
        );
        let (top, side, bottom) = EFFECT_CONTENT_PADDING;
        let area = Rect::from_min_max(
            body.min + Vec2::new(side, top),
            Pos2::new(body.max.x - side - PANEL_W - side, body.max.y - bottom),
        );
        area.center() + Vec2::new(4.0 * (x - 32.0), 4.0 * (y - 24.0))
    }

    /// The field the preview built while brushing is exactly the replay of the recorded strokes
    /// (what OK and Repeat apply), for a drag and for a stationary brush held still; the first
    /// point lands at the press position in document pixels.
    #[test]
    fn the_preview_field_is_the_replayed_strokes() {
        for tool in [LiquifyTool::ForwardWarp, LiquifyTool::Pucker] {
            let (mut app, mut d) = dialog();
            d.tool = tool;
            let ctx = context();
            let (_, rect) = frame(&ctx, &mut app, &mut d, vec![]);
            let press = screen_of(&ctx, rect, 10.0, 12.0);
            app.now_ms = 0.0;
            frame(
                &ctx,
                &mut app,
                &mut d,
                vec![Event::PointerMoved(press), button(press, true)],
            );
            let end = if tool == LiquifyTool::ForwardWarp {
                for (x, y) in [(18.0, 14.0), (30.0, 20.0), (41.0, 26.0)] {
                    let p = screen_of(&ctx, rect, x, y);
                    frame(&ctx, &mut app, &mut d, vec![Event::PointerMoved(p)]);
                }
                screen_of(&ctx, rect, 41.0, 26.0)
            } else {
                // Held still past the dab interval: each frame dabs again at the same point.
                for k in 1..=4 {
                    app.now_ms = 40.0 * f64::from(k);
                    frame(&ctx, &mut app, &mut d, vec![]);
                }
                press
            };
            frame(&ctx, &mut app, &mut d, vec![button(end, false)]);
            assert_eq!(d.strokes.len(), 1, "{tool:?}");
            let points = &d.strokes[0].points;
            assert!(
                (points[0][0] - 10.0).abs() < 1e-3 && (points[0][1] - 12.0).abs() < 1e-3,
                "{tool:?}: pressed at {:?}",
                points[0]
            );
            if tool == LiquifyTool::Pucker {
                assert!(points.len() >= 5, "{points:?}");
                assert!(points.iter().all(|p| p == &points[0]), "{points:?}");
            }
            assert!(!d.field.is_identity(), "{tool:?}");
            assert_eq!(
                d.field,
                LiquifyField::from_strokes(d.field.bounds, d.field.cell, &d.strokes),
                "{tool:?}"
            );
        }
    }

    /// Ctrl, Alt or Cmd with Esc or Enter does neither (Effect Config's rule).
    #[test]
    fn modified_escape_and_enter_do_nothing() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        frame(&ctx, &mut app, &mut d, vec![]);
        for modifiers in [
            egui::Modifiers::CTRL,
            egui::Modifiers::ALT,
            egui::Modifiers::MAC_CMD,
        ] {
            for k in [Key::Escape, Key::Enter] {
                let raw = input(vec![
                    Event::ModifiersChanged(modifiers),
                    Event::Key {
                        key: k,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                ]);
                let mut state = DialogState::Open;
                let mut out = ctx.run_ui(raw, |ui| {
                    state = with_cx(&mut app, |cx| d.show(ui, cx));
                });
                out.textures_delta.clear();
                assert_eq!(state, DialogState::Open, "{modifiers:?} {k:?}");
            }
        }
        assert!(super::super::PENDING.with(|p| p.borrow().is_empty()));
    }

    /// A dab re-renders only the proxy pixels it reached.
    #[test]
    fn a_dab_dirties_only_its_neighbourhood() {
        let (_, mut d) = dialog();
        d.values.insert(SIZE.0.into(), json!(8));
        d.begin([20.0, 20.0, 1.0], 0.0, false);
        d.extend([22.0, 20.0, 1.0], 1.0);
        let [x0, y0, x1, y1] = d.dirty.unwrap();
        assert!(x0 > 0 && y0 > 0 && x1 < 40 && y1 < 40, "{:?}", d.dirty);
        assert!((x1 - x0) * (y1 - y0) < d.proxy.w * d.proxy.h / 4);
    }

    /// With a selection the preview shows the warp only inside it, as OK's clipping does.
    #[test]
    fn the_preview_keeps_to_the_selection() {
        use pdn_core::selection::CombineMode;
        use pdn_core::util::geom::Rectangle;
        let (mut app, _) = session();
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 32.0, 48.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let def = pdn_core::effect::find(pdn_core::effect::extra::liquify::ID).unwrap();
        let mut open = || {
            let s = EffectSession::begin(&mut app, def.clone()).unwrap();
            let mut d = LiquifyDialog::new(s, Values::new());
            d.values.insert(SIZE.0.into(), json!(40));
            d.tool = LiquifyTool::Bloat;
            d
        };
        let (mut d, mut still) = (open(), open());
        d.begin([32.0, 24.0, 1.0], 0.0, false);
        d.end();
        for d in [&mut d, &mut still] {
            d.proxy
                .render(&d.field, [0, 0, d.proxy.w, d.proxy.h], &mut d.out);
        }
        let checker = (255, 191);
        // Right of the selection: the layer as it is; inside it: the warp.
        assert_eq!(d.shown(40, 24, checker), still.shown(40, 24, checker));
        assert_ne!(d.shown(24, 24, checker), still.shown(24, 24, checker));
    }

    /// Alt reverses Twirl and swaps Pucker and Bloat, as upstream's Alt-drag does.
    #[test]
    fn alt_reverses_the_brush() {
        let (_, mut d) = dialog();
        for (tool, alt) in [
            (LiquifyTool::TwirlCw, LiquifyTool::TwirlCcw),
            (LiquifyTool::Pucker, LiquifyTool::Bloat),
            (LiquifyTool::Bloat, LiquifyTool::Pucker),
            (LiquifyTool::ForwardWarp, LiquifyTool::ForwardWarp),
        ] {
            d.tool = tool;
            d.begin([20.0, 20.0, 1.0], 0.0, true);
            d.end();
            assert_eq!(d.strokes.last().unwrap().tool, alt);
        }
    }
}
