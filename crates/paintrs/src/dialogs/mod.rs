//! Modal dialogs: the stack over the app, its input blocking and the attention flash.
//! Port of: js/src/interface/window/ModalDialogController.js (with the backdrops the dialogs of
//! js/src/interface/window/dialog/*.js create), css/window.css (`.app-dialog-backdrop`,
//! `.modal-dialog-attention`, `@keyframes modal-dialog-attention-*`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET reference: `ui-drawn-graphics.md` §4 row "Dialog frames, task dialogs, attention
//! flash" and §5 "Task 22" (native forms in Paint.NET: paintdotjs's look stands);
//! `ui-text-and-glyphs.md` §2.1 row "Dialogs", §2.7 (the DWM-drawn close glyph keeps paintdotjs's
//! look), §5 "Task 22" (12-DIP dialog text, 15-DIP task-dialog command titles).
//!
//! Keys: paintdotjs moves the focus into the active dialog and makes the app `inert`, so every key
//! reaches the dialog and none reaches the app's shortcuts (AppView skips them while a modal is
//! active); no key lands outside the dialog to flash it. Here the top dialog's layer is egui's
//! modal layer (keyboard focus and Tab stay in it) and the shell's input — pointer, wheel, keys and
//! dropped files, the forms' Ctrl+Shift+F5…F8 resets included — is withheld while a dialog is
//! open (`without_input`).
pub mod about;
pub mod batch_export;
pub mod chart;
pub mod content_aware_fill;
pub mod curve_editor;
pub mod effect_config;
pub mod fade;
pub mod flows;
pub mod help;
pub mod image_size;
pub mod layer_properties;
pub mod liquify;
pub mod new_file;
pub mod plugin_store;
pub mod save_image;
pub mod settings;
pub mod task;
pub mod vanishing_point;
pub mod warp;

use std::cell::RefCell;

use egui::{Id, LayerId, Order, Ui};
use pdn_core::app::AppWorkspace;
use pdn_core::settings::AppSettings;

use crate::kit::window::{DialogFrameState, attention_amount};
use crate::prefs::Prefs;
use crate::theme::DIALOG_ATTENTION_MS;

/// How a dialog closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogResult {
    /// Its primary action (Close, Continue, OK).
    Ok,
    /// Escape or the title bar's close button.
    Cancel,
    /// A `TaskDialog` choice's value (or its `cancel_value`).
    Choice(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogState {
    Open,
    Closed(DialogResult),
}

/// What a dialog may use while it shows.
pub struct DialogCx<'a> {
    pub fonts: Option<&'a mut crate::platform::fonts::FontManager>,
    pub asset_library: Option<&'a mut crate::platform::asset_library::AssetLibrary>,
    pub app: &'a mut AppWorkspace,
    pub settings: &'a mut AppSettings,
    /// localStorage (`paintdotjs.welcome.dismissed`).
    pub prefs: &'a mut Prefs,
    /// `settings` changed: the caller persists them.
    pub settings_changed: bool,
    /// Settings Keyboard and Diagnostics borrow the existing app-owned services.
    pub shortcuts: Option<&'a mut pdn_core::action::ShortcutRegistry>,
    pub clipboard: Option<&'a mut crate::platform::Clipboard>,
    /// Pending Settings imports live on Shell, across navigation and dialog closure.
    pub settings_imports:
        Option<&'a mut std::collections::VecDeque<crate::platform::files::PendingImport>>,
    /// Set by the stack for the dialog it shows: put the keyboard focus on your initial control
    /// now (ModalDialogController.sync: the dialog just became the top one, or the focus sits
    /// outside it).
    pub seat_focus: bool,
}

/// A modal dialog. `show` paints it (usually through `kit::window::dialog_frame`) into the `Ui`
/// the stack gives it, which only the topmost dialog receives input in.
pub trait Dialog {
    fn title(&self) -> String;
    /// A message box's text (tests).
    #[cfg(test)]
    fn message(&self) -> Option<String> {
        None
    }
    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState;
    /// DialogMover's `persistenceId`: the dragged position persists as `dialogs.<id>` = `{left,
    /// top}` and the dialog opens there.
    fn persistence_id(&self) -> Option<&'static str> {
        None
    }
}

/// ModalDialogController.signalAttention's timing: a flash at most every 250 ms, its class
/// removed 850 ms after it started. Times in ms.
#[derive(Clone, Copy, Debug)]
pub struct Attention {
    last: f64,
}

impl Default for Attention {
    fn default() -> Self {
        Attention {
            last: f64::NEG_INFINITY,
        }
    }
}

impl Attention {
    /// A background event at `now`: true when it (re)starts the flash.
    pub fn trigger(&mut self, now: f64) -> bool {
        if now - self.last < DIALOG_ATTENTION_MS.2 {
            return false;
        }
        self.last = now;
        true
    }

    /// The `modal-dialog-attention` class is on the dialog.
    pub fn active(&self, now: f64) -> bool {
        (0.0..DIALOG_ATTENTION_MS.3).contains(&(now - self.last))
    }

    /// The keyframe amount at `now` (0 without a flash).
    fn amount(&self, now: f64) -> f32 {
        if self.active(now) {
            attention_amount(now - self.last)
        } else {
            0.0
        }
    }
}

thread_local! {
    /// Dialogs opened from anywhere on the UI thread (`TaskDialog::show`, a dialog opening
    /// another), taken by the stack at its next `show`.
    static PENDING: RefCell<Vec<Box<dyn Dialog>>> = RefCell::default();
}

/// The initial focus of a dialog with an OK button and no first control: none. Enter is then the
/// dialog's OK (Paint.NET's accept button), never a click on the title bar's close button.
pub(crate) fn seat_no_focus(ctx: &egui::Context) {
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
}

/// Opens `dialog` over the app; the stack shows it from its next `show` (it is the frame's last
/// painter, so this frame when called before it). UI thread only: the queue is thread-local
/// (`ErrorForm::report` is the one entry point other threads may call).
pub fn open(dialog: Box<dyn Dialog>) {
    PENDING.with(|p| p.borrow_mut().push(dialog));
}

/// Task 42's dialog for an effect session: Liquify's, Vanishing Point's or Puppet Warp's own
/// large preview (Extras, S§4.4; Perspective Warp runs on the canvas), Content Aware Fill's own
/// dialog, else the Effect Config dialog, which `configure` sets up (file source, external
/// acquisition).
pub fn effect_dialog(
    session: pdn_core::effect::session::EffectSession,
    values: pdn_core::effect::Values,
    configure: impl FnOnce(&mut effect_config::EffectConfigDialog),
) -> Box<dyn Dialog> {
    use pdn_core::effect::extra::warp::PUPPET;
    if session.def.id == pdn_core::effect::extra::liquify::ID {
        Box::new(liquify::LiquifyDialog::new(session, values))
    } else if session.def.id == pdn_core::effect::extra::vanishing::ID {
        Box::new(vanishing_point::VanishingPointDialog::new(session, values))
    } else if session.def.id == PUPPET {
        Box::new(warp::WarpDialog::new(session, values))
    } else if session.def.id == pdn_core::effect::forum::content_aware_fill::ID {
        Box::new(content_aware_fill::ContentAwareFillDialog::new(
            session, values,
        ))
    } else {
        let mut dialog = effect_config::EffectConfigDialog::new(session, values);
        configure(&mut dialog);
        Box::new(dialog)
    }
}

/// An open dialog: its serial (its layer and `Ui` scope id: a new dialog never inherits a closed
/// one's frame state) and whether it has shown (the input of the frame that opened it belongs to
/// the app).
struct Entry {
    dialog: Box<dyn Dialog>,
    serial: u64,
    shown: bool,
}

/// The open dialogs, bottom to top, each in its own foreground layer; the topmost is egui's modal
/// layer (focus and Tab stay in it) and the only one that receives input.
#[derive(Default)]
pub struct DialogStack {
    entries: Vec<Entry>,
    next: u64,
    attention: Attention,
    /// The serial of the top dialog that seated its focus.
    seated: Option<u64>,
}

impl DialogStack {
    /// Opens `dialog` on top (`dialogs::open` does it from anywhere on the UI thread).
    pub fn push(&mut self, dialog: Box<dyn Dialog>) {
        self.entries.push(Entry {
            dialog,
            serial: self.next,
            shown: false,
        });
        self.next += 1;
    }

    /// A dialog is open (`ModalDialogController.isActive()`).
    pub fn is_open(&self) -> bool {
        !self.entries.is_empty() || PENDING.with(|p| !p.borrow().is_empty())
    }

    /// A pending command dialog already owns input before its first painted frame.
    pub fn top_is(&self, id: &str) -> bool {
        self.entries
            .last()
            .is_some_and(|e| e.dialog.persistence_id() == Some(id))
            && PENDING.with(|p| p.borrow().is_empty())
    }

    /// Only the currently visible top dialog can receive a raw-drop exception.
    pub fn top_title_is(&self, title: &str) -> bool {
        self.entries
            .last()
            .is_some_and(|e| e.dialog.title() == title)
            && PENDING.with(|p| p.borrow().is_empty())
    }

    /// The attention flash is on (the `modal-dialog-attention` class is on the top dialog).
    pub fn attention_active(&self, ctx: &egui::Context) -> bool {
        crate::theme::animations_enabled(ctx)
            && self.attention.active(ctx.input(|i| i.time) * 1000.0)
    }

    /// The topmost dialog's border box as last shown.
    pub fn top_rect(&self, ctx: &egui::Context) -> Option<egui::Rect> {
        let top = self.entries.last()?;
        Some(DialogFrameState::get(ctx, Self::scope_id(top.serial)).rect)
    }

    /// Takes the dialogs `open` queued; true when there were some.
    fn take_pending(&mut self) -> bool {
        let pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
        let any = !pending.is_empty();
        for dialog in pending {
            self.push(dialog);
        }
        any
    }

    fn scope_id(serial: u64) -> Id {
        Id::unique(("paintrs.dialog", serial))
    }

    /// Shows the dialogs bottom to top over everything (`Order::Foreground` layers covering the
    /// window). Only the topmost one gets the frame's input, and not in the frame it opened in
    /// (that input was the app's); it is egui's modal layer, so keyboard focus and Tab stay in
    /// it; a press or wheel outside it is swallowed by its backdrop and flashes it
    /// (`blockBackgroundEvent`).
    pub fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) {
        self.take_pending();
        let Some(top) = self.entries.len().checked_sub(1) else {
            return;
        };
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time) * 1000.0;
        let mut screen = ctx.content_rect();
        if ctx.data_mut(|d| {
            d.get_temp::<bool>(egui::Id::unique("desktop-shell"))
                .unwrap_or(false)
        }) {
            screen.min.y += crate::theme::DESKTOP_TITLE_HEIGHT;
        }
        if self.entries[top].shown {
            let state = DialogFrameState::get(&ctx, Self::scope_id(self.entries[top].serial));
            let off = |q: egui::Pos2| !state.rect.contains(q) && !state.popup.contains(q);
            let outside = ctx.input(|i| {
                let p = &i.pointer;
                let press = p.any_pressed() && p.interact_pos().is_some_and(off);
                let wheel = i
                    .events
                    .iter()
                    .any(|e| matches!(e, egui::Event::MouseWheel { .. }))
                    && p.hover_pos().is_some_and(off);
                press || wheel
            });
            if outside {
                self.attention.trigger(now);
            }
        }
        // ModalDialogController.sync: a new top dialog, or the focus on a widget outside its layer
        // (the shell's), seats the dialog's initial focus. No focus at all stays (a press on the
        // dialog's own non-focusable content blurs, as on the dialog section in paintdotjs).
        let top_serial = self.entries[top].serial;
        let top_layer = LayerId::new(Order::Foreground, Self::scope_id(top_serial));
        let focus_outside = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))
            .is_some_and(|r| r.layer_id != top_layer);
        let seat = self.seated != Some(top_serial) || focus_outside;
        self.seated = Some(top_serial);
        let mut closed = vec![];
        for (i, entry) in self.entries.iter_mut().enumerate() {
            let id = Self::scope_id(entry.serial);
            let layer = LayerId::new(Order::Foreground, id);
            let mut state = DialogFrameState::get(&ctx, id);
            if !entry.shown
                && let Some(key) = entry.dialog.persistence_id()
            {
                // DialogMover's constructor: a saved finite position places the dialog.
                let saved = &cx.settings.values["dialogs"][key];
                if let (Some(x), Some(y)) = (saved["left"].as_f64(), saved["top"].as_f64())
                    && x.is_finite()
                    && y.is_finite()
                {
                    state.pos = Some(egui::pos2(x as f32, y as f32));
                }
            }
            state.attention = if i == top && crate::theme::animations_enabled(&ctx) {
                self.attention.amount(now)
            } else {
                0.0
            };
            state.set(&ctx, id);
            // The layer covers the window (`.app-dialog-backdrop { inset: 0 }`): hover and wheel
            // land on it; the top one is modal.
            ctx.memory_mut(|m| {
                m.areas_mut().set_state(
                    layer,
                    egui::AreaState {
                        pivot_pos: Some(screen.min),
                        pivot: egui::Align2::LEFT_TOP,
                        size: Some(screen.size()),
                        interactable: true,
                        last_became_visible_at: None,
                    },
                );
                if i == top {
                    m.set_modal_layer(layer);
                }
            });
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .layer_id(layer)
                    .scope_id(id)
                    .max_rect(screen),
            );
            child.set_clip_rect(screen);
            cx.seat_focus = i == top && seat;
            let result = if i == top && entry.shown {
                entry.dialog.show(&mut child, cx)
            } else {
                without_input(&ctx, || entry.dialog.show(&mut child, cx))
            };
            entry.shown = true;
            let state = DialogFrameState::get(&ctx, id);
            if let (true, Some(key), Some(p)) =
                (state.dropped, entry.dialog.persistence_id(), state.pos)
            {
                // DialogMover.stop.
                cx.settings.set(
                    &format!("dialogs.{key}"),
                    serde_json::json!({"left": p.x.round(), "top": p.y.round()}),
                );
                cx.settings_changed = true;
            }
            if let DialogState::Closed(_) = result {
                closed.push(i);
            }
        }
        for i in closed.into_iter().rev() {
            let entry = self.entries.remove(i);
            DialogFrameState::forget(&ctx, Self::scope_id(entry.serial));
        }
        if self.attention_active(&ctx) {
            ctx.request_repaint();
        }
        // Opened by a dialog this frame: shown from the next frame.
        if self.take_pending() {
            ctx.request_repaint();
        }
    }
}

/// Runs `f` with the frame's input withheld — no pointer, wheel, keys, other events or dropped
/// files — then restores it: the app under a modal dialog (`#content.inert`, `blockBackgroundEvent`) and a
/// dialog under another.
pub fn without_input<R>(ctx: &egui::Context, f: impl FnOnce() -> R) -> R {
    let saved = ctx.input(|i| i.clone());
    ctx.input_mut(|i| {
        i.pointer = Default::default();
        i.events.clear();
        i.raw.events.clear();
        i.raw.dropped_files.clear();
        i.raw.hovered_files.clear();
        i.keys_down.clear();
        i.modifiers = Default::default();
        i.smooth_scroll_delta = egui::Vec2::ZERO;
    });
    let r = f();
    ctx.input_mut(|i| *i = saved);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_flash_throttles_and_clears() {
        let mut a = Attention::default();
        assert!(a.trigger(0.0));
        assert!(!a.trigger(100.0)); // throttled to 250 ms
        assert!(a.trigger(300.0));
        assert!(a.active(300.0 + 849.0));
        assert!(!a.active(300.0 + 851.0));
    }

    /// ui-text-and-glyphs.md §2.1 row "Dialogs": 12 DIP text, 15 DIP task-dialog command titles.
    #[test]
    fn dialog_text_sizes_follow_paint_net() {
        assert_eq!(
            (
                crate::theme::DIALOG_FONT_SIZE,
                crate::theme::TASK_CHOICE_TITLE_SIZE
            ),
            (12.0, 15.0)
        );
    }
}
