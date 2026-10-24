//! Extra: Edit ▸ Fade, an opacity and a blend mode applied on OK as one history item. A small
//! dialog in Layer Properties' kit (its opacity row, blend-mode select and footer), not the
//! Effect Config look: the dialog has no preview and no effect session to configure.

use egui::{Key, PointerButton, Pos2, Rect, Ui, Vec2};
use pdn_core::document::layer::BlendMode;

use super::effect_config::{OpenSelect, SelectLook, Track, paint_range, paint_title_plus, select};
use super::image_size::{LINE, font, footer_pair, label};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::window::{DialogFrameState, Footer, dialog_frame_with};
use crate::theme::{self, *};

const ICON: &str = "menu_edit_fade_icon.png";
const TITLE: &str = "Fade";

pub struct FadeDialog {
    /// 0–100.
    opacity: f64,
    blend: usize,
    open_select: Option<OpenSelect>,
    /// The range drag: the press's offset from the thumb's centre.
    drag: Option<f32>,
}

impl Default for FadeDialog {
    fn default() -> Self {
        Self {
            opacity: 100.0,
            blend: 0,
            open_select: None,
            drag: None,
        }
    }
}

impl FadeDialog {
    fn content_height() -> f32 {
        let (pad_top, _, pad_bottom) = LAYER_PROPS_PADDING;
        let (gap, label_gap) = LAYER_PROPS_GAP;
        let (row_h, ..) = LAYER_PROPS_OPACITY;
        let (_, select_h, _, margin, _) = LAYER_PROPS_BLEND;
        pad_top + (LINE + label_gap + row_h) + gap + (margin + select_h) + pad_bottom
    }

    fn body(&mut self, ui: &mut Ui, r: Rect) {
        let color = theme::current(ui.ctx()).menu_item_text;
        let (pad_top, pad_x, _) = LAYER_PROPS_PADDING;
        let (gap, label_gap) = LAYER_PROPS_GAP;
        let (x0, x1) = (r.min.x + pad_x, r.max.x - pad_x);
        let mut y = r.min.y + pad_top;
        label(ui, Pos2::new(x0, y), "Opacity:", 400, color);
        y += LINE + label_gap;
        let (row_h, num_w, col_gap, margin, range_h) = LAYER_PROPS_OPACITY;
        let range = Rect::from_min_size(
            Pos2::new(x0 + margin, y + (row_h - range_h) / 2.0),
            Vec2::new(x1 - num_w - col_gap - x0, range_h),
        );
        let ratio = (self.opacity / 100.0) as f32;
        let response = paint_range(
            ui,
            ui.scope_id().with("opacity-range"),
            range,
            ratio,
            Track::Progress(ratio),
        );
        // As Layer Properties: a press on the thumb drags from where it was taken, elsewhere the
        // thumb jumps under the pointer.
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
                self.opacity = (f as f64 * 100.0).round();
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
            100.0,
            1.0,
            true,
            NumberStyle::LAYER_OPACITY,
        );
        self.opacity = self.opacity.round();
        y += row_h + gap;
        let (select_w, select_h, select_gap, margin, pad_left) = LAYER_PROPS_BLEND;
        y += margin;
        let label_text = "Mode:";
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
            ui.scope_id().with("mode"),
            Rect::from_min_size(Pos2::new(sx, y), Vec2::new(select_w, select_h)),
            "Mode",
            &names,
            self.blend,
            look,
        ) {
            self.blend = i;
        }
    }

    /// One "Fade <effect>" entry; nothing when the newest item stopped being fadable.
    fn ok(&self, cx: &mut DialogCx) -> DialogState {
        let quality = pdn_core::pixel::mask::SelectionQuality::from_value(
            &cx.app.tool_settings.rendering_quality,
        );
        if let Some(ws) = cx.app.active_ws_mut() {
            ws.fade(self.opacity, BlendMode::ALL[self.blend], quality);
        }
        DialogState::Closed(DialogResult::Ok)
    }
}

impl Dialog for FadeDialog {
    fn title(&self) -> String {
        TITLE.into()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        let list_open = self.open_select.is_some();
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            if !list_open {
                return DialogState::Closed(DialogResult::Cancel);
            }
            self.open_select = None;
        }
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let width = LAYER_PROPS_WIDTH.min(ctx.content_rect().width() - DIALOG_VIEWPORT_MARGIN);
        let mut action = None;
        let mut body = |ui: &mut Ui, r: Rect| self.body(ui, r);
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
            TITLE,
            Some(ICON),
            false,
            (Vec2::new(width, Self::content_height()), &mut body),
            Some(&mut footer),
            Footer {
                height: fh,
                border: false,
            },
        );
        // S§4.1: the plus after the title, as an Extra effect's dialog has.
        paint_title_plus(ui, Some("menu.edit.fade"), frame.rect, TITLE);
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        state.popup = if self.open_select.is_some() {
            ctx.content_rect()
        } else {
            Rect::NOTHING
        };
        state.set(&ctx, ui.scope_id());
        if enter && !list_open && action.is_none() {
            return self.ok(cx);
        }
        match action {
            Some(true) => self.ok(cx),
            Some(false) => DialogState::Closed(DialogResult::Cancel),
            None if frame.closed => DialogState::Closed(DialogResult::Cancel),
            None => DialogState::Open,
        }
    }
}
