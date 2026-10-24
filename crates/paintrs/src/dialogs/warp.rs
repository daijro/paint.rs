//! Ported from PhotoCraft crates/ui-egui/src/puppet_ui.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only the pin interaction (a press on the mesh adds a pin there and drags it, a press on a pin drags it keeping the grab offset, Delete or Backspace removes the selected pin, every change solved from rest without the per-round energies), and the mesh and pin (in soft gold) look; the mesh and preview read `warp_source` (with a selection, its pixels lifted; anchored on their content); a new mesh when a Density is picked or the Expansion slider is let go, not on every step; "The active layer has no pixels here." when there is nothing to warp, and a hint line; the Effect Config kit's frame, rows, radio buttons and OK / Cancel instead of upstream's canvas mode and options bar; the preview is the warp itself (the same kernel as OK) on a downsampled copy of the layer, not a GPU-textured mesh; no pin rotation or depth, Alt-click removal, Remove All Pins or in-session undo; OK hands the pins to the Effect Config dialog's final render (`EffectConfigDialog::final_only`) instead of running a command. Perspective Warp, once this dialog's too, runs on the canvas (`pdn_core::tool::perspective_warp`).
//!
//! Layers ▸ Puppet Warp (an Extra): a large preview of the layer (or of the selection's pixels,
//! lifted), warped as the pins are dragged; the mesh starts on the pixels' bounds. A press on the
//! mesh adds a pin, dragging a pin warps, Delete removes the selected one; Mode, Density,
//! Expansion and Show Mesh on the right. OK applies the record at full resolution as one history
//! item named after the command (Effect Config's cancellable "Applying" step); Cancel changes
//! nothing. Being a modal dialog, it blocks tool switches as Paint.NET's Move does.
//! Photocraft phase spec: S§4.4 "Interactive and Tier 3 items"; S§4.1 (the title's plus).

use egui::{
    Color32, ColorImage, Key, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2,
};
use pdn_core::canvas::ImageData;
use pdn_core::effect::extra::warp::{puppet, warp_source};
use pdn_core::effect::session::EffectSession;
use pdn_core::effect::slider::{RangeRole, SliderKeys};
use pdn_core::effect::{Values, choice, flag};
use pdn_core::extra::fx::liquify::ProxyImage;
use pdn_core::extra::geom;
use pdn_core::extra::interrupt::Interrupt;
use pdn_core::extra::puppet::{
    ITERATIONS, PuppetDensity, PuppetMesh, PuppetPin, PuppetSolver, build_mesh, render_deformed,
    rest_of,
};
use pdn_core::extra::warp::Interp;
use pdn_core::render::checker::{checker_colors, checker_is_light};
use serde_json::json;

use super::effect_config::{
    EffectConfigDialog, RangeSpec, SliderState, Track, checkbox_line, paint_title_plus, range_line,
    range_rects,
};
use super::image_size::{footer_pair, rule};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::px;
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{Footer, dialog_frame_with};
use crate::theme::{self, *};

/// Mode and Density: the value and its label (upstream's options bar).
const MODES: [(&str, &str); 3] = [
    ("rigid", "Rigid"),
    ("normal", "Normal"),
    ("distort", "Distort"),
];
const DENSITIES: [(&str, &str); 3] = [
    ("fewer", "Fewer Points"),
    ("normal", "Normal"),
    ("more", "More Points"),
];
/// Expansion (px): key, label, min, max, default (upstream's field).
const EXPANSION: (&str, &str, f64, f64, f64) = ("expansion", "Expansion", -50.0, 100.0, 2.0);
const SHOW_MESH: &str = "showMesh";
/// The preview's longer side: larger layers preview downsampled; OK still applies at full
/// resolution.
const PROXY_SIDE: usize = 1024;
/// The options column's width and the largest body (border-box width, content height), DIP.
const PANEL_W: f32 = 236.0;
const MAX_BODY: Vec2 = Vec2::new(1000.0, 640.0);
/// Room around the layer in the preview, so a pin can be dragged past it, DIP.
const MARGIN: f32 = 24.0;
/// How near a press must be to a pin to take it, DIP.
const HIT: f32 = 8.0;
/// A radio row's height and the gap between its radio and label, DIP.
const RADIO_ROW: f32 = 22.0;
const RADIO_GAP: f32 = 7.0;
/// Pins: a soft gold disc with a darker ring (upstream's yellow, softened for the user's
/// palette); the selected pin has a dark centre.
const PIN_FILL: Color32 = Color32::from_rgb(0xe9, 0xb6, 0x64);
const PIN_RING: Color32 = Color32::from_rgb(0xb5, 0x87, 0x3a);
const HINT: &str = "Click to add a pin, drag to move it, Delete removes the selected pin";
const EMPTY: &str = "The active layer has no pixels here.";
/// The transparency checkerboard's square, proxy pixels.
const CHECKER: u32 = 8;

/// Puppet Warp's pins on its mesh, solved as upstream's canvas mode solves them.
struct Puppet {
    pins: Vec<PuppetPin>,
    selected: Option<usize>,
    mesh: PuppetMesh,
    /// The Density and Expansion `mesh` was built with.
    meshed: (PuppetDensity, f64),
    solver: PuppetSolver,
    deformed: Vec<[f64; 2]>,
}

pub struct WarpDialog {
    session: Option<EffectSession>,
    id: &'static str,
    title: String,
    icon: &'static str,
    /// Mode, Density, Expansion and Show Mesh (the remembered token's, else the defaults); OK
    /// adds the pins.
    values: Values,
    puppet: Box<Puppet>,
    /// With a selection, the layer with its unselected pixels cleared: what the warp reads
    /// (`warp_source`, ruling R83); `None` reads the layer itself.
    lifted: Option<ImageData>,
    /// The pixels the warp reads inside the layer or selection bounds: where the mesh starts
    /// (`warp_source`, I1).
    anchor: geom::Rect,
    /// The layer's size (px).
    layer: Vec2,
    /// Layer pixels per proxy pixel (a whole number).
    scale: f64,
    /// The warp's source (`lifted`, else the layer), downsampled.
    proxy: ImageData,
    /// With a selection, the layer itself downsampled: what shows outside the selection.
    base: Option<ImageData>,
    /// The proxy through the current warp.
    out: ImageData,
    tex: Option<TextureHandle>,
    stale: bool,
    /// The pin being dragged and the pointer's offset from it (layer px).
    drag: Option<(usize, [f64; 2])>,
    /// The primary button went down here and is still held.
    held: bool,
    /// Expansion's slider bar as last laid out.
    expansion_bar: Rect,
    slider: SliderState,
}

/// The layer box-downsampled so its longer side is at most `PROXY_SIDE`, and the scale; the
/// layer itself when it fits.
fn proxy_of(src: &ImageData) -> (ImageData, f64) {
    let bounds = geom::Rect::new(0, 0, src.width as i32, src.height as i32);
    if src.width.max(src.height) as usize <= PROXY_SIDE {
        return (src.clone(), 1.0);
    }
    let p = ProxyImage::new(&src.data, bounds, PROXY_SIDE);
    let q = |v: f32| (v * 255.0 + 0.5) as u8;
    let data =
        p.px.iter()
            .flat_map(|c| match c[3] {
                a if a > 0.0 => [q(c[0] / a), q(c[1] / a), q(c[2] / a), q(a)],
                _ => [0; 4],
            })
            .collect();
    let (width, height) = (p.w as u32, p.h as u32);
    (
        ImageData {
            width,
            height,
            data,
        },
        p.scale,
    )
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl WarpDialog {
    /// The warp dialog over `session`'s layer with `values` (the remembered token or the
    /// defaults): no pins each time, the options as last used.
    pub fn new(session: EffectSession, mut values: Values) -> Self {
        let def = &session.def;
        let (id, icon) = (def.id, def.icon);
        let title = def
            .dialog_title
            .map_or_else(|| def.display_name(), String::from);
        let src = session.source();
        let (lifted, anchor) = warp_source(src, &session.env());
        let (proxy, scale) = proxy_of(lifted.as_ref().unwrap_or(src));
        let base = lifted.as_ref().map(|_| proxy_of(src).0);
        values.remove("pins");
        values.entry(SHOW_MESH).or_insert(json!(true));
        let w = puppet(&values);
        let mesh = build_mesh(
            lifted.as_ref().unwrap_or(src),
            anchor,
            w.density,
            w.expansion,
        );
        let puppet = Box::new(Puppet {
            pins: vec![],
            selected: None,
            meshed: (w.density, w.expansion),
            deformed: mesh.verts.clone(),
            solver: PuppetSolver::new(mesh.clone(), &[]),
            mesh,
        });
        WarpDialog {
            id,
            title,
            icon,
            values,
            puppet,
            lifted,
            anchor,
            layer: Vec2::new(src.width.max(1) as f32, src.height.max(1) as f32),
            scale,
            out: proxy.clone(),
            proxy,
            base,
            tex: None,
            stale: true,
            drag: None,
            held: false,
            expansion_bar: Rect::NOTHING,
            slider: SliderState::default(),
            session: Some(session),
        }
    }

    /// What OK applies: the options with the pins.
    fn ok_values(&self) -> Values {
        let mut values = self.values.clone();
        values.insert("pins".into(), json!(self.puppet.pins));
        values
    }

    /// A new mesh for the current Density and Expansion, then the pins bound to it. Pins stay
    /// through a mesh that comes out empty (hidden, and OK then changes nothing): an Expansion
    /// that brings pixels back binds them again.
    fn remesh(&mut self) {
        let Some(s) = &self.session else {
            return;
        };
        let p = &mut self.puppet;
        let w = puppet(&self.values);
        let src = self.lifted.as_ref().unwrap_or(s.source());
        p.mesh = build_mesh(src, self.anchor, w.density, w.expansion);
        p.meshed = (w.density, w.expansion);
        if p.mesh.tris.is_empty() {
            p.selected = None;
        }
        self.rebind();
    }

    /// No pixels to warp: no mesh.
    fn empty(&self) -> bool {
        self.puppet.mesh.tris.is_empty()
    }

    /// A new solver for the current pin set (a pin added or removed).
    fn rebind(&mut self) {
        let p = &mut self.puppet;
        let src: Vec<[f64; 2]> = p.pins.iter().map(|q| q.src).collect();
        p.solver = PuppetSolver::new(p.mesh.clone(), &src);
        self.solve();
    }

    /// Solves from rest, exactly as the render does.
    fn solve(&mut self) {
        let mode = puppet(&self.values).mode;
        let p = &mut self.puppet;
        let dst: Vec<[f64; 2]> = p.pins.iter().map(|q| q.dst).collect();
        // The render's path (`deformed`), without the per-round energies.
        let none = Interrupt::NONE;
        p.deformed = p
            .solver
            .deformed(&dst, mode, ITERATIONS, none)
            .unwrap_or_default();
        self.stale = true;
    }

    /// The proxy through the current warp: the effect's own path, at the proxy's scale.
    fn render(&mut self) {
        let clip = geom::Rect::new(0, 0, self.proxy.width as i32, self.proxy.height as i32);
        let (src, s, none) = (&self.proxy, self.scale, Interrupt::NONE);
        let p = &self.puppet;
        self.out = if p.pins.iter().any(|q| q.src != q.dst) {
            render_deformed(src, &p.mesh, &p.deformed, s, clip, Interp::Bicubic, none)
                .ok()
                .flatten()
                .unwrap_or_else(|| src.clone())
        } else {
            src.clone()
        };
        self.stale = false;
    }

    /// A press at `p` (layer px): a pin within `tol` is taken; a press elsewhere on the mesh adds
    /// a pin there.
    fn press(&mut self, p: [f64; 2], tol: f64) {
        if self.empty() {
            return;
        }
        let pp = &mut self.puppet;
        let near = (0..pp.pins.len())
            .filter(|&i| dist(pp.pins[i].dst, p) <= tol)
            .min_by(|&a, &b| dist(pp.pins[a].dst, p).total_cmp(&dist(pp.pins[b].dst, p)));
        if let Some(i) = near {
            // The grab offset: a click that selects a pin leaves it.
            let d = pp.pins[i].dst;
            pp.selected = Some(i);
            self.drag = Some((i, [d[0] - p[0], d[1] - p[1]]));
        } else if let Some(src) = rest_of(&pp.mesh, &pp.deformed, p) {
            pp.pins.push(PuppetPin { src, dst: p });
            pp.selected = Some(pp.pins.len() - 1);
            self.drag = pp.selected.map(|i| (i, [0.0, 0.0]));
            self.rebind();
        } else {
            pp.selected = None;
        }
    }

    /// The dragged pin to `p`.
    fn move_to(&mut self, k: usize, p: [f64; 2]) {
        let pp = &mut self.puppet;
        if pp.pins.get(k).is_some_and(|q| q.dst != p) {
            pp.pins[k].dst = p;
            self.solve();
        }
    }

    /// Delete: the selected pin and its deformation go.
    fn delete_selected(&mut self) {
        let p = &mut self.puppet;
        if let Some(i) = p.selected.take().filter(|&i| i < p.pins.len()) {
            p.pins.remove(i);
            self.drag = None;
            self.rebind();
        }
    }

    /// Re-renders when the warp changed and uploads the preview.
    fn upload(&mut self, ctx: &egui::Context, checker: (u8, u8)) {
        if !self.stale && self.tex.is_some() {
            return;
        }
        if self.stale {
            self.render();
        }
        let (w, h) = (self.proxy.width as usize, self.proxy.height as usize);
        let pixels = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| self.shown(x, y, checker))
            .collect();
        let image = ColorImage::new([w, h], pixels);
        match &mut self.tex {
            Some(t) => t.set(image, TextureOptions::LINEAR),
            None => {
                self.tex = Some(ctx.load_texture("warp-preview", image, TextureOptions::LINEAR))
            }
        }
    }

    /// Proxy pixel (x, y) as shown: the warped layer, kept to the selection as OK's clipping
    /// keeps it, over the checkerboard.
    fn shown(&self, x: usize, y: usize, (light, dark): (u8, u8)) -> Color32 {
        let i = (y * self.proxy.width as usize + x) * 4;
        let s = self.scale;
        let (dx, dy) = ((x as f64 + 0.5) * s, (y as f64 + 0.5) * s);
        let selected = self
            .session
            .as_ref()
            .and_then(EffectSession::mask)
            .map_or(1.0, |m| f32::from(m.get(dx as i32, dy as i32)) / 255.0);
        let pm = |d: &[u8]| {
            let a = f32::from(d[i + 3]) / 255.0;
            [0, 1, 2, 3].map(|c| if c == 3 { a } else { f32::from(d[i + c]) * a })
        };
        let base = self.base.as_ref().unwrap_or(&self.proxy);
        let (was, warped) = (pm(&base.data), pm(&self.out.data));
        let p: [f32; 4] = std::array::from_fn(|c| was[c] + (warped[c] - was[c]) * selected);
        let bg = f32::from(if checker_is_light(x as i32, y as i32, CHECKER) {
            light
        } else {
            dark
        });
        let c: [u8; 3] = std::array::from_fn(|c| (p[c] + bg * (1.0 - p[3]) + 0.5) as u8);
        Color32::from_rgb(c[0], c[1], c[2])
    }

    /// The border-box width and content height: up to `MAX_BODY`, inside the window.
    fn size(ctx: &egui::Context) -> Vec2 {
        let screen = ctx.content_rect();
        let chrome = DIALOG_TITLE_BAR + DIALOG_FOOTER_HEIGHT + 2.0 * DIALOG_BORDER;
        Vec2::new(
            MAX_BODY.x.min(screen.width() - DIALOG_VIEWPORT_MARGIN),
            MAX_BODY
                .y
                .min(screen.height() - DIALOG_VIEWPORT_MARGIN - chrome),
        )
    }

    /// The layer fitted into `area` less `MARGIN` (at most ×4), centred, and its zoom.
    fn fit(&self, area: Rect) -> (Rect, f32) {
        let (w, h) = (self.layer.x, self.layer.y);
        let room = area.shrink(MARGIN);
        let zoom = (room.width() / w).min(room.height() / h).min(4.0);
        (
            Rect::from_center_size(area.center(), Vec2::new(w * zoom, h * zoom)),
            zoom,
        )
    }

    /// The body: the preview, then the options column on the right.
    fn body(&mut self, ui: &mut Ui, r: Rect, now: f64) {
        let (top, side, bottom) = EFFECT_CONTENT_PADDING;
        let inner = Rect::from_min_max(
            Pos2::new(r.min.x + side, r.min.y + top),
            Pos2::new(r.max.x - side, r.max.y - bottom),
        );
        let panel = PANEL_W + side;
        let preview = Rect::from_min_max(inner.min, Pos2::new(inner.max.x - panel, inner.max.y));
        self.preview(ui, preview);
        let editing = self.options(
            ui,
            Rect::from_min_max(Pos2::new(preview.max.x + side, inner.min.y), inner.max),
            now,
        );
        let delete = ui.input(|i| {
            !i.modifiers.ctrl
                && !i.modifiers.alt
                && !i.modifiers.mac_cmd
                && (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
        });
        if delete && !editing {
            self.delete_selected();
        }
    }

    /// The preview: the warped layer fitted into `area`, the mesh and pins over it. A press takes
    /// a pin (or adds one), a drag moves it.
    fn preview(&mut self, ui: &mut Ui, area: Rect) {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, theme.background);
        let (image, zoom) = self.fit(area);
        if let Some(tex) = &self.tex {
            // The proxy's last row and column can reach past the layer: drawn clipped to it.
            let s = self.scale as f32 * zoom;
            let r = Rect::from_min_size(
                image.min,
                Vec2::new(self.proxy.width as f32 * s, self.proxy.height as f32 * s),
            );
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter
                .with_clip_rect(image.intersect(area))
                .image(tex.id(), r, uv, Color32::WHITE);
        }
        let response = ui.interact(
            area,
            ui.scope_id().with("warp-preview"),
            Sense::click_and_drag(),
        );
        let to_doc = |p: Pos2| {
            let p = p.clamp(area.min, area.max);
            [
                f64::from((p.x - image.min.x) / zoom),
                f64::from((p.y - image.min.y) / zoom),
            ]
        };
        let down = ui.input(|i| i.pointer.primary_down());
        if let (true, Some(p)) = (down, response.interact_pointer_pos()) {
            let d = to_doc(p);
            match self.drag {
                Some((k, off)) => self.move_to(k, [d[0] + off[0], d[1] + off[1]]),
                None if !self.held && response.is_pointer_button_down_on() => {
                    self.press(d, f64::from(HIT / zoom));
                }
                None => {}
            }
            self.held = true;
            ui.ctx().request_repaint();
        }
        if !down {
            self.drag = None;
            self.held = false;
        }
        if self.empty() {
            let spec = FontSpec::new(DIALOG_FONT_SIZE);
            let ppp = ui.ctx().pixels_per_point();
            let size = Vec2::new(
                text::layout_width(&painter, EMPTY, spec),
                spec.resolved_line_height(ppp),
            );
            let at = area.center() - size / 2.0;
            text::draw(&painter, at, EMPTY, spec, theme.menu_item_text);
        }
        let to_screen = |p: [f64; 2]| image.min + Vec2::new(p[0] as f32 * zoom, p[1] as f32 * zoom);
        let p = &self.puppet;
        if flag(&self.values, SHOW_MESH) && p.deformed.len() == p.mesh.verts.len() {
            let line = Stroke::new(0.6, Color32::from_rgba_unmultiplied(40, 40, 40, 150));
            let mut seen = std::collections::HashSet::new();
            for tri in &p.mesh.tris {
                for k in 0..3 {
                    let (i, j) = (tri[k].min(tri[(k + 1) % 3]), tri[k].max(tri[(k + 1) % 3]));
                    if seen.insert((i, j)) {
                        let ends = [to_screen(p.deformed[i]), to_screen(p.deformed[j])];
                        painter.line_segment(ends, line);
                    }
                }
            }
        }
        // Hidden over an empty mesh (`remesh` keeps them).
        let pins = if p.mesh.tris.is_empty() {
            &[][..]
        } else {
            &p.pins[..]
        };
        for (i, pin) in pins.iter().enumerate() {
            let c = to_screen(pin.dst);
            painter.circle_filled(c, 5.5, PIN_FILL);
            painter.circle_stroke(c, 5.5, Stroke::new(1.25, PIN_RING));
            if p.selected == Some(i) {
                painter.circle_filled(c, 2.0, PIN_RING);
            }
        }
    }

    /// The options column: Mode and Density as radio rows, Expansion as an Effect Config range
    /// row, then Show Mesh. True while Expansion's number box has the keyboard.
    fn options(&mut self, ui: &mut Ui, r: Rect, now: f64) -> bool {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let (x0, w) = (r.min.x, r.width());
        let before = puppet(&self.values);
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
        let mut y = r.min.y;
        for (key, label, choices) in [("mode", "Mode", MODES), ("density", "Density", DENSITIES)] {
            y = heading(ui, y, label);
            for (value, name) in choices {
                let radio = Rect::from_min_size(
                    Pos2::new(x0, y + (RADIO_ROW - RADIO_SIZE) / 2.0),
                    Vec2::splat(RADIO_SIZE),
                );
                let row = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, RADIO_ROW));
                let label = ui
                    .interact(
                        px::snap_rect(row, ppp),
                        ui.scope_id().with((key, "label", value)),
                        Sense::CLICK,
                    )
                    .clicked();
                let on = choice(&self.values, key) == value;
                let clicked = inputs::radio(ui, (key, value), radio, on, theme.window_background);
                text::draw(
                    ui.painter(),
                    Pos2::new(radio.max.x + RADIO_GAP, y + (RADIO_ROW - line) / 2.0),
                    name,
                    spec,
                    theme.menu_item_text,
                );
                if label || clicked {
                    self.values.insert(key.into(), json!(value));
                }
                y += RADIO_ROW;
            }
            y += EFFECT_CONTENT_GAP;
        }
        let (key, label, min, max, default) = EXPANSION;
        y = heading(ui, y, label);
        self.slider.now = now;
        let range = RangeSpec {
            min,
            max,
            default,
            step: 1.0,
            role: RangeRole::Slider,
            keyboard: SliderKeys::INTEGER,
        };
        let (bar, number, reset) = range_rects(x0, w, y);
        self.expansion_bar = bar;
        range_line(
            ui,
            &mut self.slider,
            key,
            range,
            &mut self.values,
            bar,
            number,
            Some(reset),
            Track::Progress(0.0),
            NumberStyle::EFFECT,
        );
        y += EFFECT_LINE_HEIGHT + EFFECT_CONTENT_GAP;
        let color = theme.menu_item_text;
        checkbox_line(
            ui,
            &mut self.values,
            SHOW_MESH,
            "Show Mesh",
            x0,
            y,
            w,
            color,
            1.0,
        );
        y += EFFECT_CHECKBOX.2 + EFFECT_CONTENT_GAP;
        let runs = [text::Run {
            text: HINT,
            spec,
            color,
            underline: false,
        }];
        let lines = text::wrap_runs(ui.painter(), &runs, w);
        text::draw_lines(ui.painter(), Pos2::new(x0, y), &runs, &lines, line);
        let after = puppet(&self.values);
        let meshed = self.puppet.meshed;
        // A new mesh once a Density is picked or the Expansion slider is let go (or a value is
        // typed), not on every step of a drag; building one can take a while on big layers.
        if (after.density, after.expansion) != meshed && !self.slider.dragging() {
            self.remesh();
        } else if after.mode != before.mode {
            self.solve();
        }
        let edit = ui
            .scope_id()
            .with(("number_input", ("number", key)))
            .with("edit");
        ui.memory(|m| m.has_focus(edit))
    }

    /// OK: the record goes to Effect Config's final render, which applies it at full resolution
    /// (cancellable) and commits one history item named after the command.
    fn ok(&mut self) -> DialogState {
        let values = self.ok_values();
        if let Some(s) = self.session.take() {
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

impl Dialog for WarpDialog {
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
        // The texture exists before the body draws it; this frame's edits upload after it.
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
        // S§4.1: an Extra's title carries the plus after its text.
        paint_title_plus(ui, Some(self.id), frame.rect, &title);
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
    use crate::kit::window::DialogFrameState;
    use egui::{Event, PointerButton};
    use pdn_core::app::AppWorkspace;
    use pdn_core::effect::extra::warp::PUPPET;
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
        d: &mut WarpDialog,
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

    /// A 64×48 layer: an opaque patterned block at 16..48 × 8..40, transparent around it.
    fn doc() -> AppWorkspace {
        super::super::PENDING.with(|p| p.borrow_mut().clear());
        let mut app = fresh_doc(64, 48, None);
        let data = (0..48u32)
            .flat_map(|y| (0..64u32).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                if (16..48).contains(&x) && (8..40).contains(&y) {
                    [(x * 5) as u8, (y * 6) as u8, ((x + y) % 9 * 28) as u8, 255]
                } else {
                    [0; 4]
                }
            })
            .collect();
        let img = ImageData {
            width: 64,
            height: 48,
            data,
        };
        let ws = app.active_ws_mut().unwrap();
        ws.document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .put_image_data(&img, 0, 0);
        app
    }

    fn session(app: &mut AppWorkspace, id: &str) -> EffectSession {
        EffectSession::begin(app, pdn_core::effect::find(id).unwrap()).unwrap()
    }

    fn dialog(id: &str) -> (AppWorkspace, WarpDialog) {
        let mut app = doc();
        let s = session(&mut app, id);
        let values = s.initial_values(&app);
        (app, WarpDialog::new(s, values))
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

    /// Where the preview shows layer point (x, y), computed apart from the dialog: the 64×48
    /// layer at ×4 (the cap) centred in the preview, which is the body (below the title bar,
    /// inside the border) less its padding and the options column.
    fn screen_of(ctx: &egui::Context, rect: Rect, x: f64, y: f64) -> Pos2 {
        let body = Rect::from_min_size(
            rect.min + Vec2::new(DIALOG_BORDER, DIALOG_BORDER + DIALOG_TITLE_BAR),
            Vec2::new(rect.width() - 2.0 * DIALOG_BORDER, WarpDialog::size(ctx).y),
        );
        let (top, side, bottom) = EFFECT_CONTENT_PADDING;
        let column = side + PANEL_W;
        let area = Rect::from_min_max(
            body.min + Vec2::new(side, top),
            Pos2::new(body.max.x - side - column, body.max.y - bottom),
        );
        area.center() + Vec2::new(4.0 * (x as f32 - 32.0), 4.0 * (y as f32 - 24.0))
    }

    /// A press at layer point `from`, moves through to `to`, and a release.
    fn drag(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        d: &mut WarpDialog,
        from: [f64; 2],
        to: [f64; 2],
    ) {
        let (_, rect) = frame(ctx, app, d, vec![]);
        let at = |p: [f64; 2]| screen_of(ctx, rect, p[0], p[1]);
        let press = at(from);
        frame(
            ctx,
            app,
            d,
            vec![Event::PointerMoved(press), button(press, true)],
        );
        for t in [0.25, 0.5, 1.0] {
            let p = at([
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ]);
            frame(ctx, app, d, vec![Event::PointerMoved(p)]);
        }
        frame(ctx, app, d, vec![button(at(to), false)]);
    }

    fn pins(d: &WarpDialog) -> &[PuppetPin] {
        &d.puppet.pins
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        dist(a, b) < 1e-3
    }

    /// S§4.4: Layers ▸ Puppet Warp opens this dialog through the shell's `effect_dialog`, at
    /// the large preview's width.
    #[test]
    fn effect_dialog_opens_the_warp_dialogs() {
        let (id, title) = (PUPPET, "Puppet Warp");
        let mut app = doc();
        let s = session(&mut app, id);
        let values = s.initial_values(&app);
        let ctx = context();
        let mut stack = super::super::DialogStack::default();
        stack.push(super::super::effect_dialog(s, values, |_| {}));
        stack_frame(&ctx, &mut app, &mut stack);
        assert!(stack.top_title_is(title), "{id}");
        assert_eq!(
            stack.top_rect(&ctx).unwrap().width(),
            WarpDialog::size(&ctx).x
        );
    }

    /// S§4.1: the dialog's title carries the plus 4 DIP after its text.
    #[test]
    fn warp_dialog_titles_have_plus() {
        let id = PUPPET;
        let (mut app, mut d) = dialog(id);
        let ctx = context();
        let image = image::RgbaImage::from_pixel(12, 16, image::Rgba([20, 200, 70, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        crate::assets::Assets::set_icon_override(&ctx, "plugin_indicator.png", png.into_inner());
        let mut expected = Rect::NOTHING;
        let mut output = ctx.run_ui(input(vec![]), |ui| {
            with_cx(&mut app, |cx| d.show(ui, cx));
            let frame = DialogFrameState::get(ui.ctx(), ui.scope_id()).rect;
            let plus = super::super::effect_config::title_plus_rect(ui.painter(), frame, &d.title);
            expected = px::snap_rect(plus, 1.0);
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
        assert!(
            meshes.contains(&expected),
            "{id}: {expected:?} in {meshes:?}"
        );
    }

    /// S§4.1: Puppet Warp + and Perspective Warp + follow Rotate/Zoom in the Layers menu.
    #[test]
    fn warp_menus_follow_rotate_zoom() {
        use crate::kit::menu::{Mark, MenuEntry, extra_tooltip, mark};
        use crate::shell::main_menu::{ITEMS, entries};
        let layers = ITEMS.iter().position(|i| *i == "menu.layers").unwrap();
        let ids = [
            "menu.layers.rotateZoom",
            "menu.layers.puppetWarp",
            "menu.layers.perspectiveWarp",
        ];
        let shown = entries(layers, &AppWorkspace::default());
        let at = shown
            .iter()
            .position(|e| *e == MenuEntry::Action(ids[0]))
            .unwrap();
        assert_eq!(shown[at..at + 3], ids.map(MenuEntry::Action));
        for (e, name) in shown[at + 1..at + 3]
            .iter()
            .zip(["Puppet Warp", "Perspective Warp"])
        {
            assert_eq!(mark(e), Mark::Extra);
            let MenuEntry::Action(id) = *e else {
                unreachable!()
            };
            let tip = format!("{name}\n(from PhotoCraft)");
            assert_eq!(extra_tooltip(id), Some(tip));
        }
    }

    /// Runs the Effect Config final render the OK queued to completion.
    fn finish(ctx: &egui::Context, app: &mut AppWorkspace) {
        let mut stack = super::super::DialogStack::default();
        for _ in 0..1000 {
            stack_frame(ctx, app, &mut stack);
            if !stack.is_open() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!stack.is_open());
    }

    /// Dragging changes only the preview; Enter applies the record at full resolution as one
    /// history item named after the command; undo restores the layer bit for bit; Repeat is
    /// untouched (a Layers command).
    #[test]
    fn warp_mode_commit_is_one_history_item() {
        let (id, name) = (PUPPET, "Puppet Warp");
        let (mut app, mut d) = dialog(id);
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [26.0, 18.0]);
        assert_eq!(
            layer_bytes(&app, None),
            before,
            "{id}: the layer waits for OK"
        );
        assert_ne!(
            d.out.data, d.proxy.data,
            "{id}: the preview follows the drag"
        );
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Enter));
        assert_eq!(state, DialogState::Closed(DialogResult::Ok), "{id}");
        finish(&ctx, &mut app);
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.history.undo.len(), undo + 1, "{id}");
        assert_eq!(ws.history.undo.last().unwrap().name, name);
        assert_ne!(layer_bytes(&app, None), before, "{id}");
        assert!(app.last_effect.is_none(), "{id}: not Repeat's");
        app.undo();
        assert_eq!(layer_bytes(&app, None), before, "{id}");
    }

    /// The pointer lands where the preview shows it (computed apart from the dialog), and the
    /// preview is exactly the effect's full-resolution render of what OK commits.
    #[test]
    fn the_preview_is_the_replayed_value() {
        let (mut app, mut d) = dialog(PUPPET);
        let ctx = context();
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [26.0, 18.0]);
        drag(&ctx, &mut app, &mut d, [40.0, 34.0], [44.0, 36.0]);
        let p = pins(&d);
        assert_eq!(p.len(), 2);
        assert!(
            near(p[0].dst, [26.0, 18.0]) && near(p[1].dst, [44.0, 36.0]),
            "{p:?}"
        );
        // The first pin went down on the rest mesh, so its rest point is the press.
        assert!(near(p[0].src, [20.0, 20.0]), "{p:?}");
        let render = d.session.as_ref().unwrap().render(&d.ok_values());
        assert_eq!(d.out.data, render.data, "puppet");
    }

    /// Cancel after dragging: the layer, history, dirty flag and Repeat are as they were.
    #[test]
    fn warp_cancel_changes_nothing() {
        let id = PUPPET;
        let (mut app, mut d) = dialog(id);
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [26.0, 18.0]);
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Escape));
        assert_eq!(state, DialogState::Closed(DialogResult::Cancel));
        assert!(super::super::PENDING.with(|p| p.borrow().is_empty()));
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.history.undo.len(), ws.dirty), (undo, false), "{id}");
        assert_eq!(layer_bytes(&app, None), before, "{id}");
        assert!(app.last_effect.is_none());
    }

    /// Ctrl, Alt or Cmd with Esc or Enter does neither (Effect Config's rule).
    #[test]
    fn modified_escape_and_enter_do_nothing() {
        let (mut app, mut d) = dialog(PUPPET);
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

    /// A press off the mesh adds no pin; Delete and Backspace remove the selected pin, and the
    /// warp follows.
    #[test]
    fn delete_removes_the_selected_pin() {
        let (mut app, mut d) = dialog(PUPPET);
        let ctx = context();
        drag(&ctx, &mut app, &mut d, [4.0, 4.0], [10.0, 4.0]);
        assert!(pins(&d).is_empty(), "the transparent corner has no mesh");
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [20.0, 20.0]);
        drag(&ctx, &mut app, &mut d, [40.0, 34.0], [46.0, 34.0]);
        assert_eq!(pins(&d).len(), 2);
        let moved = d.out.data.clone();
        frame(&ctx, &mut app, &mut d, key(Key::Delete));
        assert_eq!(pins(&d).len(), 1);
        frame(&ctx, &mut app, &mut d, vec![]);
        assert_eq!(d.out.data, d.proxy.data, "only the unmoved pin is left");
        assert_ne!(moved, d.proxy.data);
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [20.0, 20.0]);
        frame(&ctx, &mut app, &mut d, key(Key::Backspace));
        assert!(pins(&d).is_empty());
    }

    /// Mode, Density and Expansion re-solve or re-mesh, through the render's own source and
    /// anchor (`warp_source`): the mesh is the one the render builds.
    #[test]
    fn options_remesh_as_the_render_does() {
        let (mut app, mut d) = dialog(PUPPET);
        let ctx = context();
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [26.0, 18.0]);
        d.values.insert("density".into(), json!("more"));
        d.values.insert(EXPANSION.0.into(), json!(6));
        frame(&ctx, &mut app, &mut d, vec![]);
        let p = &d.puppet;
        assert_eq!(p.meshed, (PuppetDensity::More, 6.0));
        let s = d.session.as_ref().unwrap();
        let (lifted, anchor) = warp_source(s.source(), &s.env());
        assert_eq!(anchor, d.anchor);
        let w = puppet(&d.ok_values());
        let src = lifted.as_ref().unwrap_or(s.source());
        let none = Interrupt::NONE;
        let (solver, v) =
            pdn_core::extra::puppet::deform(src, anchor, &w, ITERATIONS, none).unwrap();
        assert_eq!((&p.mesh, &p.deformed), (&solver.mesh, &v));
        assert_eq!(d.out.data, s.render(&d.ok_values()).data);
    }

    /// I2: dragging Expansion's slider changes the value but keeps the mesh; letting go
    /// rebuilds it once.
    #[test]
    fn expansion_remeshes_when_the_slider_is_let_go() {
        let (mut app, mut d) = dialog(PUPPET);
        let ctx = context();
        frame(&ctx, &mut app, &mut d, vec![]);
        let bar = d.expansion_bar;
        let meshed = |d: &WarpDialog| d.puppet.meshed.1;
        let start = bar.left_center() + Vec2::new(4.0, 0.0);
        frame(
            &ctx,
            &mut app,
            &mut d,
            vec![Event::PointerMoved(start), button(start, true)],
        );
        for x in [0.4, 0.7, 0.9] {
            let p = Pos2::new(bar.min.x + bar.width() * x, bar.center().y);
            frame(&ctx, &mut app, &mut d, vec![Event::PointerMoved(p)]);
            assert!(puppet(&d.values).expansion > 2.0, "the value follows");
            assert_eq!(meshed(&d), 2.0, "no mesh while the slider is held");
        }
        let end = Pos2::new(bar.min.x + bar.width() * 0.9, bar.center().y);
        frame(&ctx, &mut app, &mut d, vec![button(end, false)]);
        frame(&ctx, &mut app, &mut d, vec![]);
        assert_eq!(
            meshed(&d),
            puppet(&d.values).expansion,
            "rebuilt on release"
        );
    }

    /// M3: pressing a pin keeps the grab offset, so a click that only selects it leaves it.
    #[test]
    fn pins_keep_their_grab_offset() {
        let (mut app, mut d) = dialog(PUPPET);
        let ctx = context();
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [20.0, 20.0]);
        drag(&ctx, &mut app, &mut d, [21.0, 20.5], [21.0, 20.5]);
        assert_eq!(pins(&d).len(), 1, "the press took the pin");
        assert!(near(pins(&d)[0].dst, [20.0, 20.0]), "{:?}", pins(&d));
        drag(&ctx, &mut app, &mut d, [21.0, 20.5], [25.0, 22.5]);
        assert!(near(pins(&d)[0].dst, [24.0, 22.0]), "{:?}", pins(&d));
    }

    /// M2: with nothing to warp (a selection over transparency, an empty layer) the dialog
    /// says so and a press does nothing.
    #[test]
    fn nothing_to_warp_says_so() {
        use pdn_core::selection::CombineMode;
        use pdn_core::util::geom::Rectangle;
        let id = PUPPET;
        let mut app = doc();
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 10.0, 6.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let s = session(&mut app, id);
        let values = s.initial_values(&app);
        let mut d = WarpDialog::new(s, values);
        assert!(d.empty(), "{id}");
        let ctx = context();
        drag(&ctx, &mut app, &mut d, [4.0, 3.0], [8.0, 5.0]);
        assert!(pins(&d).is_empty(), "{id}");
        assert_eq!(d.out.data, d.proxy.data, "{id}");
        let (_, d) = dialog(PUPPET);
        assert!(!d.empty());
    }

    /// R83 in the dialog: with a selection the preview warps the lifted source (`warp_source`),
    /// so inside the selection it is exactly what OK applies.
    #[test]
    fn the_preview_lifts_the_selection() {
        use pdn_core::selection::CombineMode;
        use pdn_core::util::geom::Rectangle;
        let id = PUPPET;
        let mut app = doc();
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(16.0, 8.0, 16.0, 32.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let s = session(&mut app, id);
        let values = s.initial_values(&app);
        let mut d = WarpDialog::new(s, values);
        assert_eq!(d.anchor, geom::Rect::new(16, 8, 32, 40), "{id}");
        let ctx = context();
        drag(&ctx, &mut app, &mut d, [20.0, 20.0], [26.0, 18.0]);
        let s = d.session.as_ref().unwrap();
        let render = s.render(&d.ok_values());
        let mask = s.mask().unwrap();
        let mut seen_vacated = false;
        for (x, y) in (0..48).flat_map(|y| (0..64).map(move |x| (x, y))) {
            let i = (y * 64 + x) as usize * 4;
            if mask.get(x, y) == 255 {
                assert_eq!(
                    d.out.data[i..i + 4],
                    render.data[i..i + 4],
                    "{id} ({x}, {y})"
                );
                seen_vacated |= render.data[i + 3] == 0;
            } else {
                let layer = d.base.as_ref().unwrap();
                assert_eq!(
                    render.data[i..i + 4],
                    layer.data[i..i + 4],
                    "{id} ({x}, {y})"
                );
            }
        }
        assert!(seen_vacated, "{id}: the lifted pixels leave a hole");
    }
}
