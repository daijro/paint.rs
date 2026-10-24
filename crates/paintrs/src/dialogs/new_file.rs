//! File ▸ New: the size of a new image, no preview.
//! Port of: js/src/interface/window/dialog/NewFileDialog.js, css/window.css (`.new-file-*`),
//! js/src/io/DocumentIO.js (`createNewDocument`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET reference: ui-drawn-graphics.md §2.11 (New Image shares ImageSizeDialog: its size
//! rules and limits, no preview); document-ops-and-formats.md §2.2 step 1 (1…262144).
//! The clipboard image's size as the initial size is Task 50's.
use egui::{Id, Key, Pos2, Rect, Ui, Vec2};
use pdn_core::io::ops::{self, MAX_SIDE};
use serde_json::Value;

use super::image_size::{
    LINE, SizeModel, check_label, fieldset, footer_buttons, label, label_right, summary,
};
use super::task::{TaskChoice, TaskDialog, TaskDialogSpec};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::window::dialog_frame;
use crate::theme::{self, *};

/// AppSettingsStore's `workspace.newFileMaintainAspectRatio`.
const ASPECT_SETTING: &str = "workspace.newFileMaintainAspectRatio";

pub struct NewFileDialog {
    model: SizeModel,
    dpi: f64,
    focus: Option<Id>,
    /// The aspect setting was read (the first `show`).
    loaded: bool,
}

impl NewFileDialog {
    /// `NewFileDialog.open(width, height)`: the active image's size (800×600 without one).
    pub fn new(w: u32, h: u32) -> Self {
        NewFileDialog {
            model: SizeModel::new(w as f64, h as f64, false),
            dpi: 96.0,
            focus: None,
            loaded: false,
        }
    }

    /// The `.new-file-content` height (padding included).
    fn content_height() -> f32 {
        let (pad_top, _, pad_bottom) = NEW_FILE_PADDING;
        let (fs_top, fs_bottom, body_top) = NEW_FILE_FIELDSET;
        let (_, _, ih, _) = NEW_FILE_ROW;
        let rows = |n: f32| n * ih + (n - 1.0) * IMAGE_SIZE_GAP;
        let fieldset = |n: f32| LINE + fs_top + body_top + rows(n) + fs_bottom;
        let g = NEW_FILE_GAP;
        pad_top
            + (LINE + NEW_FILE_SUMMARY_PAD + 1.0)
            + g
            + LINE
            + g
            + fieldset(3.0)
            + g
            + fieldset(2.0)
            + pad_bottom
    }

    fn body(&mut self, ui: &mut Ui, r: Rect) -> Option<Id> {
        let theme = theme::current(ui.ctx());
        let color = theme.menu_item_text;
        let (pad_top, pad_x, _) = NEW_FILE_PADDING;
        let (fs_top, _, body_top) = NEW_FILE_FIELDSET;
        let (label_w, iw, ih, col_gap) = NEW_FILE_ROW;
        let g = NEW_FILE_GAP;
        let (x0, x1) = (r.min.x + pad_x, r.max.x - pad_x);
        let (pw, ph) = self.model.pixels();
        let mut y = r.min.y + pad_top;
        let mb = (pw as f64 * ph as f64 * 4.0 / 1024.0 / 1024.0).max(0.1);
        y = summary(
            ui,
            x0,
            x1,
            y,
            &format!("New size: {mb:.1} MB"),
            700,
            NEW_FILE_SUMMARY_PAD,
        ) + g;
        let mut maintain = self.model.maintain;
        if check_label(
            ui,
            "aspect",
            Pos2::new(x0, y),
            LINE,
            g,
            "Maintain aspect ratio",
            &mut maintain,
            color,
        ) {
            // `aspect.onchange`: the ratio is the current one.
            self.model.maintain = maintain;
            (self.model.w0, self.model.h0) = (self.model.w, self.model.h);
        }
        y += LINE + g;
        let input_x = x0 + label_w + col_gap;
        let unit_x = input_x + iw + col_gap;
        let row = |ui: &mut Ui,
                   id: &str,
                   name: &str,
                   unit: &str,
                   top: f32,
                   value: &mut f64,
                   (min, step): (f64, f64),
                   style: NumberStyle| {
            label_right(ui, x0 + label_w, top + (ih - LINE) / 2.0, name, color);
            let rect = Rect::from_min_size(Pos2::new(input_x, top), Vec2::new(iw, ih));
            let max = if step < 1.0 { 65536.0 } else { MAX_SIDE as f64 };
            let field =
                inputs::number_input(ui, id, name, rect, value, min, max, step, true, style);
            label(
                ui,
                Pos2::new(unit_x, top + (ih - LINE) / 2.0),
                unit,
                400,
                color,
            );
            field
        };
        let style = NumberStyle::NEW_FILE;
        let stride = ih + IMAGE_SIZE_GAP;
        y = fieldset(ui, x0, x1, y, "Pixel size") + fs_top + body_top;
        let mut v = pw as f64;
        let width = row(
            ui,
            "width",
            "Width:",
            "pixels",
            y,
            &mut v,
            (1.0, 1.0),
            style,
        );
        if v != pw as f64 {
            self.model.set_width(v);
        }
        let mut v = ph as f64;
        row(
            ui,
            "height",
            "Height:",
            "pixels",
            y + stride,
            &mut v,
            (1.0, 1.0),
            style,
        );
        if v != ph as f64 {
            self.model.set_height(v);
        }
        let mut v = self.dpi;
        row(
            ui,
            "resolution",
            "Resolution:",
            "pixels/inch",
            y + 2.0 * stride,
            &mut v,
            (0.01, 0.01),
            style,
        );
        self.dpi = v.max(0.01);
        y += 2.0 * stride + ih + NEW_FILE_FIELDSET.1 + g;
        y = fieldset(ui, x0, x1, y, "Print size") + fs_top + body_top;
        let print = style.decimals(2);
        let shown = pw as f64 / self.dpi;
        let mut v = shown;
        row(
            ui,
            "print-width",
            "Width:",
            "inches",
            y,
            &mut v,
            (0.01, 0.01),
            print,
        );
        if v != shown {
            self.model.set_width(v * self.dpi);
        }
        let shown = ph as f64 / self.dpi;
        let mut v = shown;
        row(
            ui,
            "print-height",
            "Height:",
            "inches",
            y + stride,
            &mut v,
            (0.01, 0.01),
            print,
        );
        if v != shown {
            self.model.set_height(v * self.dpi);
        }
        Some(width.id)
    }

    /// `dialog.onsubmit`: remembers the aspect choice, then opens the new image; one whose memory
    /// cannot be had shows `newImageAction.error.outOfMemory` instead (the document allocates
    /// infallibly).
    fn ok(&mut self, cx: &mut DialogCx) -> DialogState {
        cx.settings
            .set(ASPECT_SETTING, Value::Bool(self.model.maintain));
        cx.settings_changed = true;
        let (w, h) = self.model.pixels();
        if ops::check_memory(1, w, h, 0).is_err() {
            let t = pdn_core::i18n::t;
            TaskDialog::show(TaskDialogSpec {
                message_icon: None,
                title: t("newImageAction.name"),
                icon: None,
                message: Some(t("newImageAction.error.outOfMemory")),
                choices: vec![TaskChoice {
                    value: "ok",
                    title: "OK".into(),
                    description: None,
                    icon: None,
                }],
                cancel_value: "ok",
                field: None,
                images: None,
                error_box: false,
            });
        } else {
            cx.app
                .create_blank_document_in_new_workspace(w, h, self.dpi, false);
        }
        DialogState::Closed(DialogResult::Ok)
    }
}

impl Dialog for NewFileDialog {
    fn title(&self) -> String {
        "New".into()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        if !std::mem::replace(&mut self.loaded, true) {
            self.model.maintain = cx.settings.get_bool(ASPECT_SETTING);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return DialogState::Closed(DialogResult::Cancel);
        }
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let width = NEW_FILE_WIDTH.min(ctx.content_rect().width() - DIALOG_VIEWPORT_MARGIN);
        let valid = self.model.valid();
        let mut width_field = None;
        let mut action = None;
        let mut body = |ui: &mut Ui, r: Rect| width_field = self.body(ui, r);
        let mut footer = |ui: &mut Ui, f: Rect| {
            action = footer_buttons(ui, f, DIALOG_BUTTON_MIN.0, DIALOG_BUTTON_MIN.1, valid);
        };
        let frame = dialog_frame(
            ui,
            "New",
            Some("menu_file_new_icon.png"),
            (Vec2::new(width, Self::content_height()), &mut body),
            Some(&mut footer),
        );
        // `content.width.input.select()`.
        if let Some(id) = self.focus.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if cx.seat_focus {
            self.focus = width_field;
            ctx.request_repaint();
        }
        if enter && action.is_none() && valid {
            return self.ok(cx);
        }
        match action {
            Some(true) => self.ok(cx),
            Some(false) => DialogState::Closed(DialogResult::Cancel),
            // Ruling R65: a press outside only flashes the dialog (no `backdrop.onclick`).
            None if frame.closed => DialogState::Closed(DialogResult::Cancel),
            None => DialogState::Open,
        }
    }
}
