//! The error form: a centred floating window (not modal) that reports an unexpected error, with
//! Copy Error, Save All and Ignore.
//! Port of: js/src/interface/window/form/ErrorForm.js, css/window.css (`.error-form*`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET reference: none covers it (paintdotjs's look and behaviour, R11); its text follows
//! Global Constraints "UI scale" rule 5 (12 DIP).
//!
//! `ErrorForm.install`'s `window.onerror` hooks have no Rust counterpart: callers report.
use std::sync::{Mutex, OnceLock};

use egui::{Pos2, Rect, Ui, Vec2};

use pdn_core::settings::AppSettings;

use super::FormsInput;
use crate::kit::inputs;
use crate::kit::px;
use crate::kit::text::{self, FontSpec, Run};
use crate::kit::window::FloatingWindow;
use crate::theme::{self, *};

/// `ErrorForm.fromError`'s report.
#[derive(Clone, Debug, PartialEq)]
struct Report {
    source: String,
    message: String,
    stack: String,
}

/// Reports from any thread (a worker's failure too), shown by the shell's form at its next `show`.
static REPORTS: Mutex<Vec<Report>> = Mutex::new(Vec::new());
/// The UI's context (set by the first `show`): a report wakes the UI to show it.
static UI: OnceLock<egui::Context> = OnceLock::new();

pub struct ErrorForm {
    window: FloatingWindow,
    report: Option<Report>,
    /// `.error-form-save-status`.
    status: String,
    /// Centred since it last opened.
    placed: bool,
    /// `lastFingerprint`, `lastReportTime` (s, egui time).
    last: Option<(String, f64)>,
}

impl Default for ErrorForm {
    fn default() -> Self {
        let mut window = FloatingWindow::new("errorForm", Vec2::ZERO, None, None);
        window.visible = false;
        window.title = format!("{} Error", crate::dialogs::about::PRODUCT);
        Self {
            window,
            report: None,
            status: String::new(),
            placed: false,
            last: None,
        }
    }
}

/// `initializeDefault`: `min(680, max(360, w − 40))` × `min(460, max(260, h − 80))` for the
/// document's client size.
fn default_size(client: Vec2) -> Vec2 {
    let (min_w, max_w, w_inset, min_h, max_h, h_inset) = ERROR_FORM_SIZE;
    Vec2::new(
        max_w.min(min_w.max(client.x - w_inset)),
        max_h.min(min_h.max(client.y - h_inset)),
    )
}

impl ErrorForm {
    /// `ErrorForm.report(error, source)`: logs `message`, then shows it with `details` (the
    /// stack trace's place) in the form — replacing what an open form shows. Callable from any
    /// thread; it wakes the UI.
    pub fn report(message: impl Into<String>, details: impl Into<String>) {
        let report = Report {
            source: "Application error".into(),
            message: message.into(),
            stack: details.into(),
        };
        eprintln!("paint.rs: {}: {}", report.source, report.message);
        REPORTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(report);
        if let Some(ctx) = UI.get() {
            ctx.request_repaint();
        }
    }

    pub fn is_open(&self) -> bool {
        self.window.visible
    }

    /// `ErrorForm.show` for the queued reports: the same report within a second of the last is
    /// dropped; an open form takes the new one.
    fn take_reports(&mut self, now: f64) {
        let reports = std::mem::take(&mut *REPORTS.lock().unwrap_or_else(|e| e.into_inner()));
        for report in reports {
            let fingerprint = format!("{}\n{}", report.message, report.stack);
            if self
                .last
                .as_ref()
                .is_some_and(|(f, t)| *f == fingerprint && now - t < 1.0)
            {
                continue;
            }
            self.last = Some((fingerprint, now));
            if !self.window.visible {
                self.placed = false;
            }
            self.window.visible = true;
            self.report = Some(report);
            self.status.clear();
        }
    }

    /// `copyError`'s text.
    fn copy_text(report: &Report) -> String {
        [
            format!("{} Error", crate::dialogs::about::PRODUCT),
            format!("Source: {}", report.source),
            format!("Error: {}", report.message),
            "Stack trace:".into(),
            stack_text(report).into(),
        ]
        .join("\n\n")
    }

    /// Shows the form over the forms (a `WebWindow` created after them, in the same
    /// `#windowOverlay`, translucent by the same rule): `others` are the forms' boxes it snaps to,
    /// `documents` the open documents (Save All needs one). Returns the actions it asks for (Save
    /// All).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        input: FormsInput,
        others: &[Rect],
        settings: &AppSettings,
        documents: usize,
    ) -> Vec<&'static str> {
        let ctx = ui.ctx().clone();
        let _ = UI.set(ctx.clone());
        self.take_reports(ui.input(|i| i.time));
        let mut actions = vec![];
        let Some(report) = self.report.clone().filter(|_| self.window.visible) else {
            return actions;
        };
        let overlay = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        // W: `#view`'s client area.
        let w = Rect::from_min_size(input.view.min, input.client);
        let pixels = input.pixels;
        // `.window-over-image`: Paint.NET's condition, as for the forms.
        let (pointer, down) = ui.input(|i| (i.pointer.hover_pos(), i.pointer.any_down()));
        let over = pointer.is_some_and(|p| {
            self.window.hit_rect().contains(p) && !input.popups.iter().any(|r| r.contains(p))
        });
        (self.window.image, self.window.overlays) = (input.image, input.overlays.to_vec());
        self.window.translucent = super::translucent(
            settings.get_bool("ui.translucentWindows"),
            self.window.rect(),
            input.image.map(|img| img.intersect(input.viewport)),
            over,
            over && down,
        );
        if !self.placed {
            // `setAnchor(0.5, 0.5)` in WebWindow's view bounds; then an absolute position (no
            // edge snap).
            self.placed = true;
            self.window.size = default_size(overlay.size());
            let v = w.shrink(WINDOW_VIEW_MARGIN);
            let p = v.min + (v.max - self.window.size - v.min) * 0.5;
            self.window.snap = None;
            self.window.set_position(p, overlay, ppp);
        }
        let status = &mut self.status;
        let mut ignore = false;
        let response = self.window.show(ui, w, overlay, others, pixels, |ui| {
            let theme = theme::current(ui.ctx());
            let r = ui.max_rect();
            let (fgap, fpy, fpx) = ERROR_FORM_FOOTER;
            let (bw, bh) = ERROR_FORM_BUTTON_MIN;
            let footer_h = 1.0 + 2.0 * fpy + bh;
            let footer = Rect::from_min_max(Pos2::new(r.min.x, r.max.y - footer_h), r.max);
            body(
                ui,
                &report,
                Rect::from_min_max(r.min, Pos2::new(r.max.x, footer.min.y)),
            );
            // .error-form-footer
            let painter = ui.painter().clone();
            let f = px::snap_rect(footer, ppp);
            painter.rect_filled(f, 0.0, theme.menu_background);
            painter.rect_filled(
                Rect::from_min_size(f.min, Vec2::new(f.width(), px::border_width(1.0, ppp))),
                0.0,
                px::over(theme.menu_background, theme.window_border),
            );
            let y = footer.min.y + 1.0 + fpy;
            let width =
                |label: &str| (text::css_width(label, DIALOG_FONT_SIZE) + 2.0 * 11.0).max(bw);
            let button = |x: f32, label: &str| {
                Rect::from_min_size(Pos2::new(x, y), Vec2::new(width(label), bh))
            };
            let copy = button(footer.min.x + fpx, "Copy Error");
            if inputs::push_button(ui, "copy", copy, "Copy Error", true, false).clicked() {
                ui.ctx().copy_text(Self::copy_text(&report));
                *status = "Error copied to clipboard.".into();
            }
            let ignore_rect = button(footer.max.x - fpx - width("Ignore"), "Ignore");
            let save = button(
                ignore_rect.min.x - ERROR_FORM_ACTIONS_GAP - width("Save All"),
                "Save All",
            );
            let can_save = documents > 0;
            if inputs::push_button(ui, "save-all", save, "Save All", can_save, false).clicked()
                && can_save
            {
                actions.push("menu.file.saveAll");
            }
            if inputs::push_button(ui, "ignore", ignore_rect, "Ignore", true, false).clicked() {
                ignore = true;
            }
            // The status sits right of the copy button, against the actions.
            let spec = FontSpec::new(DIALOG_FONT_SIZE);
            let status_w = text::css_width(status, spec.size);
            let x =
                (save.min.x - ERROR_FORM_STATUS_MARGIN - fgap - status_w).max(copy.max.x + fgap);
            let line = spec.resolved_line_height(ppp);
            text::draw(
                &painter,
                Pos2::new(x, footer.center().y + 0.5 - line / 2.0),
                status,
                spec,
                theme.muted_text,
            );
        });
        if ignore || response.closed {
            self.window.visible = false;
        }
        actions
    }
}

/// `report.stack || "No stack trace is available."`.
fn stack_text(report: &Report) -> &str {
    if report.stack.is_empty() {
        "No stack trace is available."
    } else {
        &report.stack
    }
}

/// `.error-form-content`: heading, explanation, source badge, the message and stack `pre`s.
fn body(ui: &mut Ui, report: &Report, r: Rect) {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter().clone();
    let (gap, pad) = ERROR_FORM_CONTENT;
    let content = r.shrink(pad);
    let mut y = content.min.y;
    let wrap = |runs: &[Run], width: f32| text::wrap_runs(&painter, runs, width);
    let product = crate::dialogs::about::PRODUCT;
    let heading = format!("{product} encountered an unexpected error.");
    let heading_spec = FontSpec::new(ERROR_FORM_HEADING_SIZE).bold();
    text::draw(
        &painter,
        Pos2::new(content.min.x, y),
        &heading,
        heading_spec,
        theme.menu_item_text,
    );
    y += heading_spec.resolved_line_height(ppp) + gap;
    let (margin, lh) = ERROR_FORM_EXPLANATION;
    let explanation = [Run {
        text: "You can ignore this error and continue working. Some actions may not have completed.",
        spec: FontSpec::new(DIALOG_FONT_SIZE),
        color: theme.muted_text,
        underline: false,
    }];
    let line = theme::lu(DIALOG_FONT_SIZE * lh, ui.ctx().pixels_per_point());
    let lines = wrap(&explanation, content.width());
    text::draw_lines(
        &painter,
        Pos2::new(content.min.x, y),
        &explanation,
        &lines,
        line,
    );
    y += lines.len() as f32 * line + margin + gap;
    // .error-form-source
    let (src_margin, src_py, src_px) = ERROR_FORM_SOURCE;
    let (src_text, src_bg, src_border) = ERROR_FORM_SOURCE_COLORS;
    let spec = FontSpec::new(DIALOG_FONT_SIZE);
    let badge = Rect::from_min_size(
        Pos2::new(content.min.x, y),
        Vec2::new(
            text::css_width(&report.source, spec.size) + 2.0 * (src_px + 1.0),
            spec.resolved_line_height(ppp) + 2.0 * (src_py + 1.0),
        ),
    );
    let b = px::snap_rect(badge, ppp);
    painter.rect_filled(b, 0.0, src_border);
    painter.rect_filled(b.shrink(px::border_width(1.0, ppp)), 0.0, src_bg);
    text::draw(
        &painter,
        badge.min + Vec2::new(src_px + 1.0, src_py + 1.0),
        &report.source,
        spec,
        src_text,
    );
    y += badge.height() + src_margin + gap;
    // The labels and their `pre`s; the stack takes the rest (`flex: 1`).
    let (pre_pad, pre_size, pre_lh) = ERROR_FORM_PRE;
    let mono = crate::kit::fonts::face_spec("monospace", pre_size);
    let pre_line = theme::lu(pre_size * pre_lh, ui.ctx().pixels_per_point());
    let label_spec = FontSpec::new(DIALOG_FONT_SIZE).bold();
    let label_h = label_spec.resolved_line_height(ppp);
    let pre = |runs: &[Run]| wrap(runs, content.width() - 2.0 * (pre_pad + 1.0));
    let message = [Run {
        text: &report.message,
        spec: mono,
        color: theme.menu_item_text,
        underline: false,
    }];
    let message_lines = pre(&message);
    let message_h =
        (message_lines.len() as f32 * pre_line + 2.0 * (pre_pad + 1.0)).min(ERROR_FORM_MESSAGE_MAX);
    let stack = [Run {
        text: stack_text(report),
        spec: mono,
        color: theme.menu_item_text,
        underline: false,
    }];
    let stack_lines = pre(&stack);
    let label_block = ERROR_FORM_LABEL_MARGIN + label_h + gap;
    let stack_top = y + label_block + message_h + gap + label_block;
    let stack_h = (content.max.y - stack_top).max(ERROR_FORM_STACK_MIN);
    for (label, runs, lines, h) in [
        ("Error", &message[..], &message_lines, message_h),
        ("Stack trace", &stack[..], &stack_lines, stack_h),
    ] {
        y += ERROR_FORM_LABEL_MARGIN;
        text::draw(
            &painter,
            Pos2::new(content.min.x, y),
            label,
            label_spec,
            theme.menu_item_text,
        );
        y += label_h + gap;
        let boxed = Rect::from_min_size(Pos2::new(content.min.x, y), Vec2::new(content.width(), h));
        let s = px::snap_rect(boxed, ppp);
        painter.rect_filled(s, 0.0, theme.dialog_border);
        let inside = s.shrink(px::border_width(1.0, ppp));
        painter.rect_filled(inside, 0.0, theme.dialog_surface);
        // ponytail: `overflow: auto` clips without scrollbars; add them if long reports matter.
        let clipped = painter.with_clip_rect(inside.intersect(painter.clip_rect()));
        text::draw_lines(
            &clipped,
            boxed.min + Vec2::splat(pre_pad + 1.0),
            runs,
            lines,
            pre_line,
        );
        y += h + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// ErrorForm.initializeDefault: 680×460 in a 1280×800 page, 360×260 at the floor.
    #[test]
    fn default_size_follows_the_page() {
        assert_eq!(
            default_size(Vec2::new(1280.0, 800.0)),
            Vec2::new(680.0, 460.0)
        );
        assert_eq!(
            default_size(Vec2::new(640.0, 480.0)),
            Vec2::new(600.0, 400.0)
        );
        assert_eq!(
            default_size(Vec2::new(300.0, 200.0)),
            Vec2::new(360.0, 260.0)
        );
    }

    /// `ErrorForm.show`: a report — from a worker thread too — opens the form; the same one
    /// again within a second is dropped, a different one replaces it. (One test: the queue is
    /// process-wide.)
    #[test]
    fn reports_open_the_form_and_repeat_reports_are_dropped() {
        let mut form = ErrorForm::default();
        // The UI's context, as `show` keeps it: the report wakes it.
        let ctx = UI.get_or_init(egui::Context::default).clone();
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = woken.clone();
        ctx.set_request_repaint_callback(move |_| flag.store(true, Ordering::SeqCst));
        std::thread::spawn(|| ErrorForm::report("boom", ""))
            .join()
            .expect("the worker reports");
        assert!(woken.load(Ordering::SeqCst));
        form.take_reports(10.0);
        assert_eq!(
            form.report.as_ref().map(|r| r.message.as_str()),
            Some("boom")
        );
        assert!(form.is_open());
        form.status = "Error copied to clipboard.".into();
        ErrorForm::report("boom", "");
        form.take_reports(10.5);
        assert_eq!(form.status, "Error copied to clipboard.");
        ErrorForm::report("bang", "at main");
        form.take_reports(10.6);
        assert_eq!(
            form.report.as_ref().map(|r| r.message.as_str()),
            Some("bang")
        );
        assert!(form.status.is_empty());
        assert!(
            ErrorForm::copy_text(form.report.as_ref().unwrap())
                .ends_with("Stack trace:\n\nat main")
        );
    }
}
