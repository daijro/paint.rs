//! `TaskDialog`: a message over a column of command choices, each a button with an icon, a title
//! and a description; the result is the picked choice's value, or the cancel value for Escape and
//! the close button.
//! Port of: js/src/interface/window/dialog/TaskDialog.js, css/window.css (`.task-dialog*`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's command-button action text is 1.25 × the 12-DIP font: 15 DIP. The frames, task
//! dialogs and attention flash keep paintdotjs's look.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use egui::{Key, Pos2, Rect, Sense, Ui, Vec2};

use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::text::{self, FontSpec, Run};
use crate::kit::window::dialog_frame;
use crate::kit::{img, px};
use crate::theme::{self, *};

/// One command (`options.choices[]`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskChoice {
    pub value: &'static str,
    pub title: String,
    pub description: Option<String>,
    /// An `assets/icons` file name.
    pub icon: Option<&'static str>,
}

/// `options.preview` as `ColorsForm.saveCurrentPaletteAs` builds it: a `<label>` with its text,
/// then a 260-px text `<input>` 10 px after it, focused with its text selected when the dialog
/// opens (`input.focus(); input.select()`). The opener reads `text` once the dialog closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskTextField {
    pub label: String,
    pub text: Rc<RefCell<String>>,
    /// Display name -> destination filename, excluding unrelated retained legacy entries.
    pub palette_names: Option<std::collections::BTreeMap<String, String>>,
}

impl TaskTextField {
    fn valid(&self) -> bool {
        let Some(names) = &self.palette_names else {
            return true;
        };
        crate::platform::palettes::destination_name(&self.text.borrow(), names).is_ok()
    }
}

/// Selectable image previews in the unsaved-changes dialog. Workspace IDs survive Flatten.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskImages {
    pub documents: Vec<u64>,
    pub explanation: String,
}

/// `TaskDialog.show` options.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskDialogSpec {
    pub title: String,
    pub icon: Option<&'static str>,
    pub message: Option<String>,
    /// Optional assets/icons image beside the message; existing dialogs reserve no icon column.
    pub message_icon: Option<&'static str>,
    pub choices: Vec<TaskChoice>,
    pub cancel_value: &'static str,
    /// The `preview` element: a labelled text field.
    pub field: Option<TaskTextField>,
    pub images: Option<TaskImages>,
    /// `className: "clone-stamp-warning-dialog"`: Paint.NET's compact error box — a red "×" disc
    /// beside the message, the choices as small buttons right-aligned in a bordered row.
    pub error_box: bool,
}

/// The pending result of a shown task dialog (JS's promise), polled each frame.
#[derive(Clone, Debug, Default)]
pub struct TaskDialogHandle(Rc<Cell<Option<&'static str>>>);

impl TaskDialogHandle {
    /// The picked value once the dialog closed.
    pub fn poll(&self) -> Option<&'static str> {
        self.0.get()
    }
}

/// What runs when a task dialog closes: the picked value, with the app (JS's `await` and the
/// code after it).
pub type OnClose = Box<dyn FnOnce(&'static str, &mut DialogCx)>;

pub struct TaskDialog {
    spec: TaskDialogSpec,
    result: TaskDialogHandle,
    on_close: Option<OnClose>,
    thumbnails: HashMap<u64, (u64, f32, egui::TextureHandle)>,
}

impl TaskDialog {
    /// Opens the dialog over the app (`dialogs::open`; UI thread).
    pub fn show(spec: TaskDialogSpec) -> TaskDialogHandle {
        Self::open(spec, None)
    }

    /// `show`, then `on_close` with the picked value and the app when it closes (before the
    /// handle reads it).
    pub fn show_then(
        spec: TaskDialogSpec,
        on_close: impl FnOnce(&'static str, &mut DialogCx) + 'static,
    ) -> TaskDialogHandle {
        Self::open(spec, Some(Box::new(on_close)))
    }

    fn open(spec: TaskDialogSpec, on_close: Option<OnClose>) -> TaskDialogHandle {
        let result = TaskDialogHandle::default();
        super::open(Box::new(TaskDialog {
            spec,
            result: result.clone(),
            on_close,
            thumbnails: HashMap::new(),
        }));
        result
    }

    /// `finish(value)`.
    fn finish(&mut self, value: &'static str, cx: &mut DialogCx) -> DialogState {
        if let Some(on_close) = self.on_close.take() {
            on_close(value, cx);
        }
        self.result.0.set(Some(value));
        DialogState::Closed(DialogResult::Choice(value))
    }
}

impl TaskDialog {
    /// The `clone-stamp-warning-dialog` layout (theme `ERROR_BOX_*`): row 1 the disc and the
    /// message (`white-space: pre-line`), row 2 the choices.
    fn show_error_box(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        // With no focus seated (below), Enter picks the first choice (the message box's default
        // button), or the cancel value when there is none.
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            let first = self.spec.choices.first().map(|c| c.value);
            return self.finish(first.unwrap_or(self.spec.cancel_value), cx);
        }
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let screen = ui.ctx().content_rect();
        let width = ERROR_BOX_WIDTH.min(screen.width() - DIALOG_VIEWPORT_MARGIN);
        let (pad_top, pad_x) = ERROR_BOX_PADDING;
        let (column, gap) = ERROR_BOX_COLUMN;
        let (side, icon_top) = ERROR_BOX_ICON;
        let (choices_margin, choices_pad_y, choices_pad_x) = ERROR_BOX_CHOICES;
        let (button_w, button_h, radius) = ERROR_BOX_BUTTON;
        let inner = width - 2.0 * DIALOG_BORDER;
        let message_w = inner - 2.0 * pad_x - column - gap;
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let text = self.spec.message.clone().unwrap_or_default();
        let runs = [Run {
            text: &text,
            spec,
            color: theme.link,
            underline: false,
        }];
        let lines = text::wrap_runs(ui.painter(), &runs, message_w);
        let line = theme::lu(DIALOG_FONT_SIZE * TASK_CHOICE_LINE_HEIGHT, ppp);
        let row = (side + icon_top).max(lines.len() as f32 * line);
        let height = pad_top + row + choices_margin + 1.0 + 2.0 * choices_pad_y + button_h;
        let mut picked = None;
        let choices = &self.spec.choices;
        let mut body = |ui: &mut Ui, r: Rect| {
            let painter = ui.painter().clone();
            let (x, y) = (r.min.x + pad_x, r.min.y + pad_top);
            // The disc and its "×" (`place-items: center` on a `line-height: 1` line box).
            let disc = Rect::from_min_size(Pos2::new(x, y + icon_top), Vec2::splat(side));
            let under = theme.window_background;
            let snapped = px::snap_rect(disc, ppp);
            paint_rounded(
                &painter,
                snapped,
                &[(0.0, side / 2.0, ERROR_BOX_ICON_FILL)],
                under,
            );
            let glyph = crate::kit::fonts::face_spec(DOCUMENT_CLOSE_FAMILY, ERROR_BOX_GLYPH)
                .line_height(ERROR_BOX_GLYPH);
            let gw = text::measure(&painter, "\u{d7}", glyph);
            let at = disc.center() - Vec2::new(gw, ERROR_BOX_GLYPH) / 2.0;
            text::draw(&painter, at, "\u{d7}", glyph, egui::Color32::WHITE);
            text::draw_lines(
                &painter,
                Pos2::new(x + column + gap, y),
                &runs,
                &lines,
                line,
            );
            // The choices' row: its top border across the dialog, the buttons at its right.
            let top = y + row + choices_margin;
            let bw = px::border_width(1.0, ppp);
            let border = Rect::from_min_size(Pos2::new(r.min.x, top), Vec2::new(r.width(), bw));
            painter.rect_filled(px::snap_rect(border, ppp), 0.0, theme.dialog_border);
            let mut right = r.max.x - choices_pad_x;
            for choice in choices.iter().rev() {
                let b = Rect::from_min_size(
                    Pos2::new(right - button_w, top + 1.0 + choices_pad_y),
                    Vec2::new(button_w, button_h),
                );
                let snapped = px::snap_rect(b, ppp);
                let response =
                    ui.interact(snapped, ui.scope_id().with(choice.value), Sense::click());
                let outline = if response.hovered() || response.has_focus() {
                    theme.menu_item_selected_border
                } else {
                    theme.button_outline
                };
                let layers = [(0.0, radius, outline), (1.0, radius - 1.0, theme.button)];
                paint_rounded(&painter, snapped, &layers, theme.window_background);
                // Chromium centres a `<button>`'s content vertically, `display: block` or not.
                let w = text::css_width(&choice.title, DIALOG_FONT_SIZE);
                let at = Pos2::new(
                    b.center().x - w / 2.0,
                    b.center().y - spec.resolved_line_height(ppp) / 2.0,
                );
                text::draw(&painter, at, &choice.title, spec, theme.button_text);
                if crate::kit::inputs::button_activated(ui, &response) {
                    picked = Some(choice.value);
                }
                right = b.min.x;
            }
        };
        let frame = dialog_frame(
            ui,
            &self.spec.title,
            self.spec.icon,
            (Vec2::new(width, height), &mut body),
            None,
        );
        // A pointer release opens it, so Chromium shows no `:focus-visible` ring on the close
        // button `ModalDialogController.sync` focuses: no focus is seated (Escape cancels, Enter
        // picks the first choice, above).
        match (picked, frame.closed) {
            (Some(value), _) => self.finish(value, cx),
            (None, true) => self.finish(self.spec.cancel_value, cx),
            (None, false) => DialogState::Open,
        }
    }
}

/// A CSS `border-radius` box drawn anti-aliased as Chromium does (4×4 supersampled coverage per
/// device pixel) over the flat colour `under`: each layer (inset, radius, colour), from the
/// outside in (a border, then the background inside it), composited over the previous ones.
fn paint_rounded(
    painter: &egui::Painter,
    rect: Rect,
    layers: &[(f32, f32, egui::Color32)],
    under: egui::Color32,
) {
    let ppp = painter.pixels_per_point();
    let device = Rect::from_min_max(rect.min * ppp, rect.max * ppp);
    let inside = |r: Rect, radius: f32, x: f32, y: f32| {
        let cx = x.clamp(r.min.x + radius, r.max.x - radius);
        let cy = y.clamp(r.min.y + radius, r.max.y - radius);
        r.contains(Pos2::new(x, y)) && (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
    };
    for py in device.min.y.floor() as i32..device.max.y.ceil() as i32 {
        for px_ in device.min.x.floor() as i32..device.max.x.ceil() as i32 {
            let mut c = under;
            for &(inset, radius, color) in layers {
                let r = device.shrink(inset * ppp);
                let hits = (0..16)
                    .filter(|i| {
                        let (sx, sy) = ((i % 4) as f32 + 0.5, (i / 4) as f32 + 0.5);
                        inside(r, radius * ppp, px_ as f32 + sx / 4.0, py as f32 + sy / 4.0)
                    })
                    .count();
                let k = hits as f32 / 16.0;
                c = egui::Color32::from_rgb(
                    (c.r() as f32 + (color.r() as f32 - c.r() as f32) * k).round() as u8,
                    (c.g() as f32 + (color.g() as f32 - c.g() as f32) * k).round() as u8,
                    (c.b() as f32 + (color.b() as f32 - c.b() as f32) * k).round() as u8,
                );
            }
            if c != under {
                let at = Pos2::new(px_ as f32, py as f32) / ppp;
                painter.rect_filled(Rect::from_min_size(at, Vec2::splat(1.0 / ppp)), 0.0, c);
            }
        }
    }
}

/// A choice's laid-out copy: the title and description lines, and the copy column's x and width.
struct ChoiceLayout<'a> {
    runs: [Run<'a>; 2],
    title: Vec<Vec<text::Placed>>,
    description: Vec<Vec<text::Placed>>,
    copy_x: f32,
    height: f32,
}

fn description_line(ppp: f32) -> f32 {
    // LayoutNG keeps `line-height: 1.35` at LayoutUnit precision.
    theme::lu(DIALOG_FONT_SIZE * TASK_CHOICE_LINE_HEIGHT, ppp)
}

fn layout_choice<'a>(ui: &Ui, choice: &'a TaskChoice, width: f32) -> ChoiceLayout<'a> {
    let theme = theme::current(ui.ctx());
    let (py, pxx) = TASK_CHOICE_PADDING;
    let inner = width - 2.0 * (1.0 + pxx);
    // grid-template-columns: 25px 1fr. quirk: without an icon the copy is the button's first
    // grid item, so it lands in the 25-px column (paintdotjs behaviour no doc analyses;
    // Appendix B B-D1).
    let (copy_x, copy_w) = if choice.icon.is_some() {
        (
            TASK_CHOICE_COLUMN + TASK_CHOICE_GAP,
            inner - TASK_CHOICE_COLUMN - TASK_CHOICE_GAP,
        )
    } else {
        (0.0, TASK_CHOICE_COLUMN)
    };
    let runs = [
        Run {
            text: &choice.title,
            spec: FontSpec::new(TASK_CHOICE_TITLE_SIZE),
            color: theme.menu_item_text,
            underline: false,
        },
        Run {
            text: choice.description.as_deref().unwrap_or(""),
            spec: FontSpec::new(DIALOG_FONT_SIZE),
            color: theme.muted_text,
            underline: false,
        },
    ];
    let painter = ui.painter();
    let title = text::wrap_runs_aligned(painter, &runs[..1], copy_w, text::Alignment::Left);
    let description = if choice.description.is_some() {
        text::wrap_runs_aligned(painter, &runs[1..], copy_w, text::Alignment::Left)
    } else {
        vec![]
    };
    let ppp = ui.ctx().pixels_per_point();
    let mut copy = title.len() as f32 * runs[0].spec.resolved_line_height(ppp);
    if !description.is_empty() {
        copy += TASK_CHOICE_COPY_GAP + description.len() as f32 * description_line(ppp);
    }
    ChoiceLayout {
        runs,
        title,
        description,
        copy_x,
        height: (copy + 2.0 * (py + 1.0)).max(TASK_CHOICE_MIN_HEIGHT),
    }
}

impl Dialog for TaskDialog {
    fn title(&self) -> String {
        self.spec.title.clone()
    }
    #[cfg(test)]
    fn message(&self) -> Option<String> {
        self.spec.message.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let theme = theme::current(ui.ctx());
        // The keydown listener: Escape cancels.
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return self.finish(self.spec.cancel_value, cx);
        }
        if self.spec.error_box {
            return self.show_error_box(ui, cx);
        }
        let screen = ui.ctx().content_rect();
        let width = if self.spec.images.is_some() {
            570.0
        } else {
            TASK_DIALOG_WIDTH
        }
        .min(screen.width() - DIALOG_VIEWPORT_MARGIN);
        let inner = width - 2.0 * DIALOG_BORDER - 2.0 * TASK_DIALOG_PADDING;
        let message_spec = FontSpec::new(DIALOG_FONT_SIZE).weight(600);
        let message_runs = self.spec.message.as_deref().map(|m| {
            [Run {
                text: m,
                spec: message_spec,
                color: theme.menu_item_text,
                underline: false,
            }]
        });
        let message_inset = if self.spec.message_icon.is_some() {
            40.0
        } else {
            0.0
        };
        let message = message_runs
            .as_ref()
            .map(|runs| text::wrap_runs(ui.painter(), runs, inner - message_inset));
        let ppp = ui.ctx().pixels_per_point();
        let line = message_spec.resolved_line_height(ppp);
        let image_font = FontSpec::new(DIALOG_FONT_SIZE).weight(600);
        let image_runs = self.spec.images.as_ref().map(|images| {
            [Run {
                text: images.explanation.as_str(),
                spec: image_font,
                color: theme.menu_item_text,
                underline: false,
            }]
        });
        let image_lines = image_runs
            .as_ref()
            .map(|runs| text::wrap_runs(ui.painter(), runs, inner));
        let image_line = theme::lu(DIALOG_FONT_SIZE * 1.45, ppp);
        let image_height = image_lines.as_ref().map_or(0.0, |lines| {
            lines.len() as f32 * image_line + 10.0 + 96.0 + 12.0
        });
        let mut height = 2.0 * TASK_DIALOG_PADDING + image_height;
        // The field's line box: the input's border box (its line, UA padding 1px 2px, 1-px border).
        let field_font = FontSpec::new(DIALOG_FONT_SIZE);
        let (pad_y, pad_x) = TEXT_INPUT_PADDING;
        let field_h = field_font.resolved_line_height(ppp) + 2.0 * (pad_y + 1.0);
        if self.spec.field.is_some() {
            height += field_h + TASK_DIALOG_PADDING;
        }
        if let Some(lines) = &message {
            height += (lines.len() as f32 * line).max(if message_inset > 0.0 { 32.0 } else { 0.0 })
                + TASK_MESSAGE_MARGIN_BOTTOM
                + TASK_DIALOG_PADDING;
        }
        let choices: Vec<ChoiceLayout> = self
            .spec
            .choices
            .iter()
            .map(|c| layout_choice(ui, c, inner))
            .collect();
        height += choices.iter().map(|c| c.height).sum::<f32>()
            + TASK_CHOICES_GAP * choices.len().saturating_sub(1) as f32;

        let mut picked = None;
        let mut field_id = None;
        let spec = &self.spec;
        let thumbnails = &mut self.thumbnails;
        let mut body = |ui: &mut Ui, r: Rect| {
            let ppp = ui.ctx().pixels_per_point();
            let painter = ui.painter().clone();
            let mut y = r.min.y + TASK_DIALOG_PADDING;
            let x = r.min.x + TASK_DIALOG_PADDING;
            if let Some(field) = &spec.field {
                let top = y + (field_h - field_font.resolved_line_height(ppp)) / 2.0;
                text::draw(
                    &painter,
                    Pos2::new(x, top),
                    &field.label,
                    field_font,
                    theme.menu_item_text,
                );
                let (margin, w) = TASK_FIELD_INPUT;
                let left = x + text::css_width(&field.label, DIALOG_FONT_SIZE) + margin;
                let rect = Rect::from_min_size(Pos2::new(left, y), Vec2::new(w, field_h));
                let colors = if !field.valid() && !field.text.borrow().is_empty() {
                    (PALETTE_INVALID_BACKGROUND, egui::Color32::WHITE)
                } else {
                    (theme.menu_background, theme.menu_item_text)
                };
                let response = crate::kit::inputs::text_input(
                    ui,
                    "task-field",
                    &field.label,
                    rect,
                    &mut field.text.borrow_mut(),
                    colors,
                    pad_x,
                    DIALOG_FONT_SIZE,
                    false,
                    true,
                );
                // Enter in the field (one line; it keeps the focus) is OK: the first choice that
                // is not the cancel, while the entry is valid (the choices' enabling below).
                let focused = crate::kit::inputs::text_input_focused(ui, "task-field");
                if focused && ui.input(|i| i.key_pressed(Key::Enter)) && field.valid() {
                    picked = spec
                        .choices
                        .iter()
                        .map(|c| c.value)
                        .find(|&v| v != spec.cancel_value);
                }
                field_id = Some(response.id);
                y += field_h + TASK_DIALOG_PADDING;
            }
            if let (Some(lines), Some(runs)) = (&message, &message_runs) {
                if let Some(icon) = spec.message_icon {
                    img::paint_icon(
                        &painter,
                        icon,
                        Rect::from_min_size(Pos2::new(x, y), Vec2::splat(32.0)),
                        theme.window_background,
                        img::Filter::None,
                    );
                }
                text::draw_lines(&painter, Pos2::new(x + message_inset, y), runs, lines, line);
                y += (lines.len() as f32 * line).max(if message_inset > 0.0 { 32.0 } else { 0.0 })
                    + TASK_MESSAGE_MARGIN_BOTTOM
                    + TASK_DIALOG_PADDING;
            }
            if let (Some(images), Some(runs), Some(lines)) =
                (&spec.images, &image_runs, &image_lines)
            {
                text::draw_lines(&painter, Pos2::new(x, y), runs, lines, image_line);
                y += lines.len() as f32 * image_line + 10.0;
                show_images(
                    ui,
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(inner, 96.0)),
                    images,
                    cx,
                    thumbnails,
                );
                y += 108.0;
            }
            for (choice, layout) in spec.choices.iter().zip(&choices) {
                let enabled = choice.value == spec.cancel_value
                    || spec.field.as_ref().is_none_or(TaskTextField::valid);
                let b = Rect::from_min_size(Pos2::new(x, y), Vec2::new(inner, layout.height));
                let snapped = px::snap_rect(b, ppp);
                let response = ui.interact(
                    snapped,
                    ui.scope_id().with(choice.value),
                    if enabled {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                ui.ctx().accesskit_node_builder(response.id, |n| {
                    n.set_role(egui::accesskit::Role::Button);
                    n.set_label(&choice.title);
                    if !enabled {
                        n.set_disabled();
                    }
                });
                let mut backdrop = theme.window_background;
                // `:hover, :focus-visible` (Tab reaches the choices; Enter or Space picks one).
                if response.hovered() || response.has_focus() {
                    let bw = px::border_width(1.0, ppp);
                    painter.rect_filled(snapped, 0.0, theme.menu_item_border);
                    painter.rect_filled(snapped.shrink(bw), 0.0, theme.menu_item_hover);
                    backdrop = theme.menu_item_hover;
                }
                let (py, pxx) = TASK_CHOICE_PADDING;
                let content = b.min + Vec2::new(1.0 + pxx, 1.0 + py);
                if let Some(icon) = choice.icon {
                    let (side, top) = TASK_CHOICE_ICON;
                    let at = Rect::from_min_size(content + Vec2::new(0.0, top), Vec2::splat(side));
                    img::paint_icon(
                        &painter,
                        icon,
                        px::snap_rect(at, ppp),
                        backdrop,
                        img::Filter::None,
                    );
                }
                let copy = content + Vec2::new(layout.copy_x, 0.0);
                let title_line = layout.runs[0].spec.resolved_line_height(ppp);
                text::draw_lines(&painter, copy, &layout.runs[..1], &layout.title, title_line);
                if !layout.description.is_empty() {
                    let at = copy
                        + Vec2::new(
                            0.0,
                            layout.title.len() as f32 * title_line + TASK_CHOICE_COPY_GAP,
                        );
                    text::draw_lines(
                        &painter,
                        at,
                        &layout.runs[1..],
                        &layout.description,
                        description_line(ui.ctx().pixels_per_point()),
                    );
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if enabled && crate::kit::inputs::button_activated(ui, &response) {
                    picked = Some(choice.value);
                }
                y += layout.height + TASK_CHOICES_GAP;
            }
        };
        let frame = dialog_frame(
            ui,
            &self.spec.title,
            self.spec.icon,
            (Vec2::new(width, height), &mut body),
            None,
        );
        if cx.seat_focus {
            // quirk: ModalDialogController.sync focuses the dialog's first button — the title
            // bar's close button — so Enter and Space cancel until Tab moves on (paintdotjs
            // behaviour no doc analyses; Appendix B B-D2). A field's opener focuses it after.
            let id = field_id.unwrap_or(frame.close_id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        match (picked, frame.closed) {
            (Some(value), _) => self.finish(value, cx),
            (None, true) => self.finish(self.spec.cancel_value, cx),
            (None, false) => DialogState::Open,
        }
    }
}

/// Task18's native scaler/frame draw the preview; the surrounding buttons retain paintdotjs's
/// 92×78 layout. Preview pixels are pdn regions, while names remain ordinary dialog text.
fn show_images(
    ui: &mut Ui,
    strip: Rect,
    images: &TaskImages,
    cx: &mut DialogCx,
    cache: &mut HashMap<u64, (u64, f32, egui::TextureHandle)>,
) {
    use pdn_core::render::{
        display::RenderMode,
        thumbnail::{ThumbFrame, render_framed, ring_extent, scale_thumbnail},
    };
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter().clone();
    painter.rect_filled(px::snap_rect(strip, ppp), 0.0, theme.dialog_border);
    painter.rect_filled(
        px::snap_rect(strip.shrink(1.0), ppp),
        0.0,
        theme.window_background,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(strip.shrink(9.0)));
    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.set_min_size(Vec2::new(images.documents.len() as f32 * 100.0 - 8.0, 78.0));
            let at = ui.min_rect().min;
            for (n, id) in images.documents.iter().enumerate() {
                let Some(i) = cx.app.documents.iter().position(|w| w.id == *id) else {
                    continue;
                };
                let ws = &cx.app.documents[i];
                let name = ws.get_friendly_name();
                let button = Rect::from_min_size(
                    at + Vec2::new(n as f32 * 100.0, 0.0),
                    Vec2::new(92.0, 78.0),
                );
                let response =
                    ui.interact(button, ui.scope_id().with(("unsaved", id)), Sense::click());
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_role(egui::accesskit::Role::Button);
                    node.set_label(&name);
                });
                let selected =
                    response.hovered() || response.has_focus() || cx.app.active == Some(i);
                let border = if selected {
                    theme.menu_item_selected_border
                } else {
                    theme.dialog_border
                };
                ui.painter()
                    .rect_filled(px::snap_rect(button, ppp), 0.0, border);
                ui.painter().rect_filled(
                    px::snap_rect(button.shrink(if selected { 2.0 } else { 1.0 }), ppp),
                    0.0,
                    theme.window_background,
                );
                let revision = ws.view.composition_revision;
                if !cache
                    .get(id)
                    .is_some_and(|(rev, scale, _)| *rev == revision && *scale == ppp)
                {
                    let d = &ws.document;
                    let composite = d.composite();
                    // The native ring also belongs inside the CSS canvas' 80×54 slot.
                    let ring = 2.0 * ring_extent(ppp as f64) as f32 / ppp;
                    let fit = ((80.0 - ring) / d.width as f32)
                        .min((54.0 - ring) / d.height as f32)
                        .min(1.0);
                    let edge = (d.width.max(d.height) as f32 * fit * ppp).round().max(1.0) as u32;
                    let (pixels, w, h) = scale_thumbnail(composite.data(), d.width, d.height, edge);
                    let brightness = cx
                        .settings
                        .get("canvas.checkerboardBrightness")
                        .as_f64()
                        .unwrap_or(50.0) as f32
                        / 100.0;
                    let hardware = cx
                        .settings
                        .get("graphics.hardwareAcceleration")
                        .as_bool()
                        .unwrap_or(true);
                    let c = theme.window_background;
                    let frame = ThumbFrame {
                        u: ppp as f64,
                        checker_brightness: brightness,
                        dark: c.r() < 128,
                        mode: if hardware {
                            RenderMode::Hardware
                        } else {
                            RenderMode::Software
                        },
                        checker: true,
                        backdrop: [c.r(), c.g(), c.b(), 255],
                    };
                    let (pixels, w, h) = render_framed(&pixels, w, h, &frame);
                    let texture = ui.ctx().load_texture(
                        format!("unsaved-{id}"),
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels),
                        egui::TextureOptions::NEAREST,
                    );
                    cache.insert(*id, (revision, ppp, texture));
                }
                if let Some((_, _, texture)) = cache.get(id) {
                    let size = texture.size_vec2() / ppp;
                    let image = Rect::from_center_size(
                        Pos2::new(button.center().x, button.min.y + 31.0),
                        size,
                    );
                    ui.painter().image(
                        texture.id(),
                        image,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    text::record_pdn_rect(ui.ctx(), image.intersect(ui.clip_rect()));
                }
                let name_width = text::css_width(&name, 10.666667).min(80.0);
                text::draw_ellipsized(
                    ui.painter(),
                    Rect::from_min_size(
                        Pos2::new(button.center().x - name_width / 2.0, button.min.y + 61.0),
                        Vec2::new(name_width, 15.0),
                    ),
                    &name,
                    FontSpec::new(10.666667),
                    theme.menu_item_text,
                );
                if crate::kit::inputs::button_activated(ui, &response) {
                    cx.app.set_active_document_workspace(Some(i));
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::DialogStack;

    fn spec() -> TaskDialogSpec {
        TaskDialogSpec {
            title: "Unsaved changes".into(),
            icon: Some("menu_file_save_icon.png"),
            message: Some("Do you want to save changes?".into()),
            choices: vec![
                TaskChoice {
                    value: "save",
                    title: "Save".into(),
                    description: Some("Save the image first.".into()),
                    icon: Some("menu_file_save_icon.png"),
                },
                TaskChoice {
                    value: "discard",
                    title: "Don't Save".into(),
                    description: None,
                    icon: Some("menu_file_close_icon.png"),
                },
            ],
            cancel_value: "cancel",
            field: None,
            images: None,
            error_box: false,
            message_icon: None,
        }
    }

    /// Runs `stack` in a 1280×800 context for one frame with `events`.
    fn frame(ctx: &egui::Context, stack: &mut DialogStack, events: Vec<egui::Event>) {
        frame_at(ctx, stack, events, None);
    }

    fn frame_with_prefs(
        ctx: &egui::Context,
        stack: &mut DialogStack,
        events: Vec<egui::Event>,
        prefs: &mut crate::prefs::Prefs,
    ) {
        run(ctx, stack, events, None, prefs);
    }

    fn frame_at(
        ctx: &egui::Context,
        stack: &mut DialogStack,
        events: Vec<egui::Event>,
        time: Option<f64>,
    ) {
        run(
            ctx,
            stack,
            events,
            time,
            &mut crate::prefs::Prefs::default(),
        );
    }

    fn run(
        ctx: &egui::Context,
        stack: &mut DialogStack,
        events: Vec<egui::Event>,
        time: Option<f64>,
        prefs: &mut crate::prefs::Prefs,
    ) {
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut settings = pdn_core::settings::AppSettings::load(None);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            events,
            time,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut cx = DialogCx {
                app: &mut app,
                settings: &mut settings,
                prefs,
                settings_changed: false,
                shortcuts: None,
                clipboard: None,
                settings_imports: None,
                fonts: None,
                asset_library: None,
                seat_focus: false,
            };
            stack.show(ui, &mut cx);
        });
        out.textures_delta.clear(); // no renderer in unit tests
    }

    fn click(at: Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
        ]
    }

    #[test]
    fn invalid_palette_name_cannot_activate_save() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let text = Rc::new(RefCell::new("../escape".into()));
        let handle = TaskDialog::show(TaskDialogSpec {
            title: "Save Current Palette As".into(),
            choices: vec![
                TaskChoice {
                    value: "save",
                    title: "Save".into(),
                    ..Default::default()
                },
                TaskChoice {
                    value: "cancel",
                    title: "Cancel".into(),
                    ..Default::default()
                },
            ],
            cancel_value: "cancel",
            field: Some(TaskTextField {
                label: "Palette name:".into(),
                text: text.clone(),
                palette_names: Some(Default::default()),
            }),
            ..Default::default()
        });
        frame(&ctx, &mut stack, vec![]);
        frame(&ctx, &mut stack, vec![]);
        let rect = crate::kit::window::DialogFrameState::get(&ctx, DialogStack::scope_id(0)).rect;
        let save = Pos2::new(rect.center().x, rect.max.y - 1. - 12. - 54. - 4. - 27.);
        for events in click(save) {
            frame(&ctx, &mut stack, events);
        }
        assert_eq!(handle.poll(), None);
    }

    /// ColorsForm.js `saveCurrentPaletteAs`'s preview: the field opens focused with its text
    /// selected, typing replaces it, and the opener reads it after Save.
    #[test]
    fn a_text_field_preview_is_focused_and_read_after_the_choice() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let text = Rc::new(RefCell::new("My Palette".to_string()));
        let choice = |value, title: &str| TaskChoice {
            value,
            title: title.into(),
            ..Default::default()
        };
        let handle = TaskDialog::show(TaskDialogSpec {
            title: "Save Current Palette As".into(),
            choices: vec![choice("save", "Save"), choice("cancel", "Cancel")],
            cancel_value: "cancel",
            field: Some(TaskTextField {
                label: "Palette name:".into(),
                text: text.clone(),
                palette_names: None,
            }),
            ..Default::default()
        });
        frame(&ctx, &mut stack, vec![]);
        frame(&ctx, &mut stack, vec![]);
        frame(&ctx, &mut stack, vec![egui::Event::Text("Warm".into())]);
        assert_eq!(*text.borrow(), "Warm");
        // Save: the first of the two 54-px choices above the 12-px padding and the 1-px border.
        let rect = crate::kit::window::DialogFrameState::get(&ctx, DialogStack::scope_id(0)).rect;
        let save = Pos2::new(rect.center().x, rect.max.y - 1.0 - 12.0 - 54.0 - 4.0 - 27.0);
        for events in click(save) {
            frame(&ctx, &mut stack, events);
        }
        assert_eq!(
            (handle.poll(), text.borrow().as_str()),
            (Some("save"), "Warm")
        );
    }

    /// Parity-removal review M4: Enter in a task dialog's text field is OK (the first choice that
    /// is not the cancel), but not while the entry is invalid (Save is disabled then).
    #[test]
    fn enter_in_the_text_field_picks_the_first_choice() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let choice = |value, title: &str| TaskChoice {
            value,
            title: title.into(),
            ..Default::default()
        };
        for (typed, names, expected) in [
            ("Warm", None, Some("save")),
            ("../escape", Some(Default::default()), None),
        ] {
            let text = Rc::new(RefCell::new("My Palette".to_string()));
            let handle = TaskDialog::show(TaskDialogSpec {
                title: "Save Current Palette As".into(),
                choices: vec![choice("cancel", "Cancel"), choice("save", "Save")],
                cancel_value: "cancel",
                field: Some(TaskTextField {
                    label: "Palette name:".into(),
                    text: text.clone(),
                    palette_names: names,
                }),
                ..Default::default()
            });
            frame(&ctx, &mut stack, vec![]);
            frame(&ctx, &mut stack, vec![]);
            frame(&ctx, &mut stack, vec![egui::Event::Text(typed.into())]);
            assert_eq!(*text.borrow(), typed);
            frame(&ctx, &mut stack, vec![key(Key::Enter)]);
            frame(&ctx, &mut stack, vec![]);
            assert_eq!(handle.poll(), expected, "{typed}");
            assert_eq!(stack.is_open(), expected.is_none());
        }
    }

    /// TaskDialog.js `finish`: a choice resolves its value, Escape and the close button the
    /// cancel value; the handle reads it once the dialog closed (layout from the paintdotjs DOM:
    /// a 390×(2 + 31 + 12 + 15 + 2 + 12 + 54 + 4 + 54 + 12) box centred in 1280×800).
    #[test]
    fn choices_escape_and_close_resolve_the_handle() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let handle = TaskDialog::show(spec());
        frame(&ctx, &mut stack, vec![]);
        assert!(stack.is_open());
        assert_eq!(handle.poll(), None);
        let rect = crate::kit::window::DialogFrameState::get(&ctx, DialogStack::scope_id(0)).rect;
        assert_eq!(
            rect,
            Rect::from_min_size(Pos2::new(445.0, 301.0), Vec2::new(390.0, 198.0))
        );
        // The second choice: y 301 + 1 + 31 + 12 + 15 + 2 + 12 + 54 + 4 = 432 … 486.
        for events in click(Pos2::new(640.0, 450.0)) {
            frame(&ctx, &mut stack, events);
        }
        assert_eq!(handle.poll(), Some("discard"));
        assert!(!stack.is_open());

        let escaped = TaskDialog::show(spec());
        frame(&ctx, &mut stack, vec![]);
        let key = |pressed| egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, &mut stack, vec![key(true)]);
        assert_eq!(escaped.poll(), Some("cancel"));

        let closed = TaskDialog::show(spec());
        frame(&ctx, &mut stack, vec![]);
        // The close button: the title bar's right 46 px.
        for events in click(Pos2::new(445.0 + 390.0 - 20.0, 315.0)) {
            frame(&ctx, &mut stack, events);
        }
        assert_eq!(closed.poll(), Some("cancel"));
        assert!(!stack.is_open());
    }

    /// ModalDialogController.blockBackgroundEvent: a press outside the dialog flashes it (the
    /// frame takes the keyframe amount), the dialog stays open; a second press within 250 ms
    /// does not restart the flash.
    #[test]
    fn a_press_outside_flashes_the_dialog() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let handle = TaskDialog::show(spec());
        frame_at(&ctx, &mut stack, vec![], Some(0.0));
        let outside = click(Pos2::new(100.0, 100.0));
        for (i, events) in outside.into_iter().enumerate() {
            frame_at(&ctx, &mut stack, events, Some(1.0 + 0.01 * i as f64));
        }
        // The press landed at 1.01 s: 80 ms later the flash peaks.
        assert!(stack.attention.active(1010.0));
        assert!(!stack.attention.trigger(1200.0));
        frame_at(&ctx, &mut stack, vec![], Some(1.09));
        let state = crate::kit::window::DialogFrameState::get(&ctx, DialogStack::scope_id(0));
        assert!(state.attention > 0.99, "{}", state.attention);
        frame_at(&ctx, &mut stack, vec![], Some(2.0));
        let state = crate::kit::window::DialogFrameState::get(&ctx, DialogStack::scope_id(0));
        assert_eq!(state.attention, 0.0);
        assert!(stack.is_open());
        assert_eq!(handle.poll(), None);
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// ModalDialogController.sync focuses the dialog's first button (the close button); Tab
    /// moves the focus to the first choice (the dialog is egui's modal layer: Tab stays in it)
    /// and Enter picks the focused choice.
    #[test]
    fn tab_reaches_the_choices_and_enter_picks_one() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let handle = TaskDialog::show(spec());
        frame_at(&ctx, &mut stack, vec![], Some(0.0));
        frame_at(&ctx, &mut stack, vec![], Some(0.1));
        let close = ctx.memory(|m| m.focused());
        assert!(close.is_some(), "the close button has the focus");
        frame_at(&ctx, &mut stack, vec![key(Key::Tab)], Some(0.2));
        frame_at(&ctx, &mut stack, vec![], Some(0.3));
        assert_ne!(ctx.memory(|m| m.focused()), close);
        frame_at(&ctx, &mut stack, vec![key(Key::Enter)], Some(0.4));
        assert_eq!(handle.poll(), Some("save"));
        assert!(!stack.is_open());
    }

    /// Nested dialogs: the lower one is inert (a press on its uncovered choice does nothing and
    /// flashes the top one); Escape closes only the top one, then the lower one.
    #[test]
    fn nested_dialogs_answer_from_the_top_down() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let choice = |value| TaskChoice {
            value,
            title: value.into(),
            icon: Some("menu_file_save_icon.png"),
            ..Default::default()
        };
        // 2 + 31 + 12 + 15 + 2 + 12 + 4 × 54 + 3 × 4 + 12 = 314 tall at y 243: its last choice at
        // y 490–544. The top one: 2 + 31 + 12 + 15 + 2 + 12 + 54 + 12 = 140 tall at y 330.
        let lower = TaskDialog::show(TaskDialogSpec {
            title: "Lower".into(),
            message: Some("Pick".into()),
            choices: vec![choice("a"), choice("b"), choice("c"), choice("d")],
            cancel_value: "lower-cancel",
            ..Default::default()
        });
        let top = TaskDialog::show(TaskDialogSpec {
            title: "Top".into(),
            message: Some("Pick".into()),
            choices: vec![choice("e")],
            cancel_value: "top-cancel",
            ..Default::default()
        });
        frame_at(&ctx, &mut stack, vec![], Some(0.0));
        for (i, events) in click(Pos2::new(640.0, 520.0)).into_iter().enumerate() {
            frame_at(&ctx, &mut stack, events, Some(1.0 + 0.01 * i as f64));
        }
        assert_eq!(lower.poll(), None);
        assert!(stack.attention.active(1020.0));
        frame_at(&ctx, &mut stack, vec![key(Key::Escape)], Some(2.0));
        assert_eq!((top.poll(), lower.poll()), (Some("top-cancel"), None));
        assert!(stack.is_open());
        frame_at(&ctx, &mut stack, vec![key(Key::Escape)], Some(2.1));
        assert_eq!(lower.poll(), Some("lower-cancel"));
        assert!(!stack.is_open());
    }

    /// The `clone-stamp-warning-dialog` error box: Enter picks its OK (the first choice), and
    /// with no choice the cancel value.
    #[test]
    fn enter_picks_the_error_box_default() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let error_box = |choices: Vec<TaskChoice>| TaskDialogSpec {
            title: "paint.rs".into(),
            message: Some("Could not use the Clone Stamp.\n\nUse Ctrl+Click.".into()),
            choices,
            cancel_value: "cancel",
            images: None,
            error_box: true,
            ..Default::default()
        };
        let ok = TaskChoice {
            value: "ok",
            title: "OK".into(),
            ..Default::default()
        };
        for (choices, want) in [(vec![ok], "ok"), (vec![], "cancel")] {
            let handle = TaskDialog::show(error_box(choices));
            frame(&ctx, &mut stack, vec![]);
            assert!(stack.is_open());
            frame(&ctx, &mut stack, vec![key(Key::Enter)]);
            assert_eq!(handle.poll(), Some(want));
            assert!(!stack.is_open());
        }
    }

    /// `show_then`: the continuation runs with the app and the picked value when it closes.
    #[test]
    fn the_continuation_gets_the_value_and_the_app() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut stack = DialogStack::default();
        let handle = TaskDialog::show_then(spec(), |value, cx| {
            cx.prefs.set("picked", value);
        });
        let mut prefs = crate::prefs::Prefs::default();
        frame_with_prefs(&ctx, &mut stack, vec![], &mut prefs);
        frame_with_prefs(&ctx, &mut stack, vec![key(Key::Escape)], &mut prefs);
        assert_eq!(handle.poll(), Some("cancel"));
        assert_eq!(prefs.get("picked"), Some("cancel"));
    }
}
