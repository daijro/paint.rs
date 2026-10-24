//! Layers ▸ Layer Properties: name, opacity, blend mode and visibility, previewed on the layer
//! while the dialog is open; one history entry on OK.
//! Port of: js/src/interface/window/dialog/LayerPropertiesDialog.js, css/window.css
//! (`.layer-properties-*`), js/src/engine/document/history/action/actions/document/layer/
//! LayerPropertiesAction.js
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET reference: ui-drawn-graphics.md §2.11 (a live layer change, no preview widget);
//! compositing.md §2.4, §3.2 (the 14 layer blend modes in Paint.NET's order).
use egui::{Id, Key, PointerButton, Pos2, Rect, Ui, Vec2};
use pdn_core::app::AppEvent;
use pdn_core::app::AppWorkspace;
use pdn_core::document::layer::{BlendMode, LayerProperties};
use pdn_core::history::HistoryMemento;

use super::effect_config::{OpenSelect, SelectLook, Track, paint_range, select};
use super::image_size::{LINE, font, footer_pair, label};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::px;
use crate::kit::window::{DialogFrameState, Footer, dialog_frame_with};
use crate::theme::{self, *};

const ICON: &str = "menu_layers_layer_properties_icon.png";

/// The dialog's title and history name: `menu.layers.layerProperties.text` without its "...".
fn title() -> String {
    pdn_core::i18n::t("menu.layers.layerProperties.text")
        .trim_end_matches("...")
        .to_string()
}

/// `applyProperties`: the layer takes `p` and invalidates; `document:layer_properties_changed`.
fn apply(app: &mut AppWorkspace, index: usize, p: &LayerProperties) {
    let Some(ws) = app.active_ws_mut() else {
        return;
    };
    let Some(layer) = ws.document.layers.get_at_mut(index) else {
        return;
    };
    if layer.properties == *p {
        return;
    }
    layer.properties = p.clone();
    ws.invalidate(None);
    ws.view.events.push(AppEvent::LayerPropertiesChanged);
}

pub struct LayerPropertiesDialog {
    index: usize,
    original: LayerProperties,
    name: String,
    opacity: f64,
    blend: usize,
    visible: bool,
    /// `LayerPropertiesHistoryMemento` of the layer as it was when the dialog opened.
    memento: Option<HistoryMemento>,
    open_select: Option<OpenSelect>,
    /// The range drag: the press's offset from the thumb's centre.
    drag: Option<f32>,
    focus: Option<Id>,
}

impl LayerPropertiesDialog {
    /// The dialog for the active layer (`LayerPropertiesAction.performAction`); None without one.
    pub fn new(app: &AppWorkspace) -> Option<Self> {
        let ws = app.active_ws()?;
        let index = ws.active_layer;
        let p = ws.document.layers.get_at(index)?.properties.clone();
        let memento = HistoryMemento::layer_properties(
            &title(),
            &format!("assets/icons/{ICON}"),
            &ws.document,
            index,
        );
        Some(LayerPropertiesDialog {
            index,
            name: p.name.clone(),
            opacity: p.opacity as f64,
            blend: BlendMode::ALL
                .iter()
                .position(|m| *m == p.blend_mode)
                .unwrap_or(0),
            visible: p.visible,
            original: p,
            memento: Some(memento),
            open_select: None,
            drag: None,
            focus: None,
        })
    }

    /// `readProperties`.
    fn read(&self) -> LayerProperties {
        LayerProperties {
            name: self.name.clone(),
            visible: self.visible,
            is_background: self.original.is_background,
            opacity: self.opacity.round().clamp(0.0, 255.0) as u8,
            blend_mode: BlendMode::ALL[self.blend],
        }
    }

    /// The content height (padding included).
    fn content_height() -> f32 {
        let (pad_top, _, pad_bottom) = LAYER_PROPS_PADDING;
        let (gap, label_gap) = LAYER_PROPS_GAP;
        let (name_h, _) = LAYER_PROPS_NAME;
        let (row_h, ..) = LAYER_PROPS_OPACITY;
        let (_, select_h, _, margin, _) = LAYER_PROPS_BLEND;
        let (.., visible_h) = LAYER_PROPS_VISIBLE;
        pad_top
            + (LINE + label_gap + name_h)
            + gap
            + (LINE + label_gap + row_h)
            + gap
            + (margin + select_h)
            + gap
            + visible_h
            + pad_bottom
    }

    /// The body; returns the name field's response id and whether a value changed.
    fn body(&mut self, ui: &mut Ui, r: Rect) -> (Id, bool) {
        let theme = theme::current(ui.ctx());
        let color = theme.menu_item_text;
        let ppp = ui.ctx().pixels_per_point();
        let (pad_top, pad_x, _) = LAYER_PROPS_PADDING;
        let (gap, label_gap) = LAYER_PROPS_GAP;
        let (x0, x1) = (r.min.x + pad_x, r.max.x - pad_x);
        let before = self.read();
        let mut y = r.min.y + pad_top;
        // Name.
        label(ui, Pos2::new(x0, y), "Name:", 400, color);
        y += LINE + label_gap;
        let (name_h, name_pad) = LAYER_PROPS_NAME;
        let name = inputs::text_input(
            ui,
            "name",
            "Name",
            Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + name_h)),
            &mut self.name,
            (theme.menu_background, theme.pdn_control_border),
            name_pad,
            DIALOG_FONT_SIZE,
            false,
            true,
        );
        y += name_h + gap;
        // Opacity: the range (0–255, its track filled to the value) and the number.
        label(ui, Pos2::new(x0, y), "Opacity:", 400, color);
        y += LINE + label_gap;
        let (row_h, num_w, col_gap, margin, range_h) = LAYER_PROPS_OPACITY;
        let cell = x1 - num_w - col_gap - x0;
        let range = Rect::from_min_size(
            Pos2::new(x0 + margin, y + (row_h - range_h) / 2.0),
            Vec2::new(cell, range_h),
        );
        let ratio = (self.opacity / 255.0) as f32;
        let response = paint_range(
            ui,
            ui.scope_id().with("opacity-range"),
            range,
            ratio,
            Track::Progress(ratio),
        );
        // Blink's range: a press on the thumb drags it from where it was taken, elsewhere the
        // thumb jumps under the pointer; the value is the thumb centre's position on the track
        // (its width less the thumb's), rounded to the step.
        let (tw, ..) = EFFECT_THUMB;
        let thumb_c = range.min.x + tw / 2.0 + ratio * (range.width() - tw);
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let Some(p) = pointer
            && response.hovered()
            && ui.input(|i| i.pointer.button_pressed(PointerButton::Primary))
        {
            let on_thumb = (p.x - thumb_c).abs() <= tw / 2.0;
            self.drag = Some(if on_thumb { p.x - thumb_c } else { 0.0 });
            response.request_focus();
        }
        if let (Some(offset), Some(p)) = (self.drag, pointer) {
            if ui.input(|i| i.pointer.primary_down()) {
                let f = ((p.x - offset - range.min.x - tw / 2.0) / (range.width() - tw))
                    .clamp(0.0, 1.0);
                self.opacity = (f as f64 * 255.0).round();
            } else {
                self.drag = None;
            }
        }
        let number = Rect::from_min_size(
            Pos2::new(x1 - num_w, y + (row_h - LAYER_PROPS_NUMBER_HEIGHT) / 2.0),
            Vec2::new(num_w, LAYER_PROPS_NUMBER_HEIGHT),
        );
        inputs::number_input(
            ui,
            "opacity",
            "Opacity",
            number,
            &mut self.opacity,
            0.0,
            255.0,
            1.0,
            true,
            NumberStyle::LAYER_OPACITY,
        );
        self.opacity = self.opacity.round();
        y += row_h + gap;
        // Blend mode: the 14 layer modes (compositing.md §2.4; `LayerProperties.BLEND_MODES`).
        let (select_w, select_h, select_gap, margin, pad_left) = LAYER_PROPS_BLEND;
        y += margin;
        let label_text = "Blend Mode:";
        label(
            ui,
            Pos2::new(x0, y + (select_h - LINE) / 2.0),
            label_text,
            400,
            color,
        );
        let sx = x0 + crate::kit::text::css_width(label_text, DIALOG_FONT_SIZE) + select_gap;
        let names: Vec<&str> = BlendMode::ALL.iter().map(|m| m.label()).collect();
        let look = SelectLook {
            native: false,
            pad_left,
            font: font(400),
        };
        if let Some(i) = select(
            ui,
            &mut self.open_select,
            ui.scope_id().with("blend"),
            Rect::from_min_size(Pos2::new(sx, y), Vec2::new(select_w, select_h)),
            "Blend Mode",
            &names,
            self.blend,
            look,
        ) {
            self.blend = i;
        }
        y += select_h + gap;
        // Visible: the 16-px box, 6 px, the text.
        let (side, check_gap, visible_h) = LAYER_PROPS_VISIBLE;
        let boxed = Rect::from_min_size(
            Pos2::new(x0, y + (visible_h - side) / 2.0),
            Vec2::splat(side),
        );
        let tx = boxed.max.x + check_gap;
        let text = "Visible";
        let whole = Rect::from_min_max(
            Pos2::new(x0, y),
            Pos2::new(
                tx + crate::kit::text::css_width(text, DIALOG_FONT_SIZE),
                y + visible_h,
            ),
        );
        let label_click = ui
            .interact(
                px::snap_rect(whole, ppp),
                ui.scope_id().with("visible-label"),
                egui::Sense::CLICK,
            )
            .clicked();
        inputs::checkbox(
            ui,
            "visible",
            boxed,
            &mut self.visible,
            true,
            theme.window_background,
        );
        if label_click {
            self.visible = !self.visible;
        }
        label(
            ui,
            Pos2::new(tx, y + (visible_h - LINE) / 2.0),
            text,
            400,
            color,
        );
        (name.id, self.read() != before)
    }

    /// `dialog.onsubmit`: an empty name keeps the old one; one entry when anything changed.
    fn ok(&mut self, cx: &mut DialogCx) -> DialogState {
        let mut p = self.read();
        if p.name.trim().is_empty() {
            p.name = self.original.name.clone();
        } else {
            p.name = p.name.trim().to_string();
        }
        apply(cx.app, self.index, &p);
        if p != self.original
            && let (Some(m), Some(ws)) = (self.memento.take(), cx.app.active_ws_mut())
        {
            ws.push_memento(m);
        }
        DialogState::Closed(DialogResult::Ok)
    }

    /// `finish(null)`: the layer gets its properties back.
    fn cancel(&mut self, cx: &mut DialogCx) -> DialogState {
        apply(cx.app, self.index, &self.original);
        DialogState::Closed(DialogResult::Cancel)
    }
}

impl Dialog for LayerPropertiesDialog {
    fn title(&self) -> String {
        title()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        let list_open = self.open_select.is_some();
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            if !list_open {
                return self.cancel(cx);
            }
            self.open_select = None;
        }
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let width = LAYER_PROPS_WIDTH.min(ctx.content_rect().width() - DIALOG_VIEWPORT_MARGIN);
        let mut name_field = None;
        let mut changed = false;
        let mut action = None;
        let mut body = |ui: &mut Ui, r: Rect| {
            let (id, c) = self.body(ui, r);
            name_field = Some(id);
            changed = c;
        };
        let (fh, pad_top, pad_bottom, _) = LAYER_PROPS_FOOTER;
        let mut footer = |ui: &mut Ui, f: Rect| {
            let (_, fx) = DIALOG_FOOTER_PADDING;
            let content = Rect::from_min_max(
                Pos2::new(f.min.x + fx, f.min.y + pad_top),
                Pos2::new(f.max.x - fx, f.max.y - pad_bottom),
            );
            let (bw, bh) = LAYER_PROPS_BUTTON;
            action = footer_pair(ui, content, bw, bh, true);
        };
        let frame = dialog_frame_with(
            ui,
            &title(),
            Some(ICON),
            false,
            (Vec2::new(width, Self::content_height()), &mut body),
            Some(&mut footer),
            Footer {
                height: fh,
                border: false,
            },
        );
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        state.popup = if self.open_select.is_some() {
            ctx.content_rect()
        } else {
            Rect::NOTHING
        };
        state.set(&ctx, ui.scope_id());
        // `preview`: the layer shows the dialog's values as they change.
        if changed {
            let p = self.read();
            apply(cx.app, self.index, &p);
        }
        // `nameInput.select()`.
        if let Some(id) = self.focus.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if cx.seat_focus {
            self.focus = name_field;
            ctx.request_repaint();
        }
        if enter && !list_open && self.open_select.is_none() && action.is_none() {
            return self.ok(cx);
        }
        match action {
            Some(true) => self.ok(cx),
            Some(false) => self.cancel(cx),
            None if frame.closed => self.cancel(cx),
            // Ruling R65: a press outside only flashes the dialog (no `backdrop.onclick`).
            None => DialogState::Open,
        }
    }
}
