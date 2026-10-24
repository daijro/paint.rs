use super::EffectConfigDialog;
use super::external::ExternalInput;
use crate::dialogs::task::{TaskChoice, TaskDialog, TaskDialogSpec};
use crate::dialogs::{DialogCx, DialogResult, DialogState};
use pdn_core::canvas::ImageData;
use pdn_core::effect::bb;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// paintdotjs's `previewDelay`: min(250, max(16, elapsed / 2)) ms.
fn preview_delay(elapsed_ms: f64) -> f64 {
    (elapsed_ms / 2.0).clamp(16.0, 250.0)
}

/// A finished preview: its generation, the pixels, the render time (ms).
pub(super) struct JobResult {
    pub(super) generation: u64,
    pub(super) result: Option<(ImageData, Option<String>)>,
    pub(super) elapsed: f64,
}

#[cfg(target_arch = "wasm32")]
pub(super) struct SlicedSeam {
    generation: u64,
    carver: bb::seam::Carver,
    started: web_time::Instant,
}

fn seam_warning(message: String) -> TaskDialogSpec {
    TaskDialogSpec {
        title: "Warning".into(),
        message: Some(message),
        message_icon: Some("menu_help_about_icon.png"),
        choices: vec![TaskChoice {
            value: "ok",
            title: "OK".into(),
            ..Default::default()
        }],
        cancel_value: "ok",
        ..Default::default()
    }
}

impl EffectConfigDialog {
    pub(super) fn cancel_job(&self) {
        if let Some(cancel) = &self.job_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Cancels in-flight work. Browser slices retire now; a native worker cannot be interrupted,
    /// so it stays owned until it drains and `run_previews` discards its result before starting
    /// the latest generation: edits never stack renders.
    pub(super) fn retire_work(&mut self) {
        self.cancel_job();
        if self.job.is_none() {
            self.job_cancel = None;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.sliced_seam = None;
        }
    }

    pub(super) fn seam(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| bb::seam::orientation(s.def.id).is_some())
    }

    pub(super) fn deliver_notice(&mut self, notice: Option<String>) {
        // spec-question: Q37 Seam carving warning frequency
        if bb::conventions::SEAM_NOTICE_ONCE_PER_GENERATION
            && self.noticed_generation == Some(self.generation)
        {
            return;
        }
        if let Some(notice) = notice {
            self.noticed_generation = Some(self.generation);
            TaskDialog::show(seam_warning(notice));
        }
    }

    /// Native workers and browser slices pass through the same generation acceptance check.
    pub(super) fn accept_result(&mut self, cx: &mut DialogCx, result: JobResult) -> bool {
        if result.generation != self.generation
            || (!self.preview && !self.finalizing)
            || !self.input_ready()
            || self
                .session
                .as_ref()
                .is_none_or(|s| !s.owner_exists(cx.app))
        {
            return false;
        }
        let Some((pixels, notice)) = result.result else {
            self.failed = self.finalizing;
            return false;
        };
        self.due = None;
        self.delay = preview_delay(result.elapsed);
        // Keep this generation's notice on its owner until this frame's edits/cancel resolve.
        // `notify` and `cancel` discard it; final acceptance publishes only after committing.
        self.pending_notice = notice;
        if !self.finalizing {
            self.session.as_ref().unwrap().show(cx.app, Some(&pixels));
        }
        self.shown = Some((result.generation, pixels));
        true
    }

    pub(super) fn finish_ready(&mut self, cx: &mut DialogCx) -> Option<DialogState> {
        if !self.finalizing || self.session.is_none() {
            return None;
        }
        if self.failed {
            let state = self.cancel(cx);
            if self.worker_lost {
                let text = "The effect could not be rendered.".into();
                crate::platform::files::message(&self.title, text);
            }
            return Some(state);
        }
        if let Some(error) = self.font_error.clone() {
            // R157: a delayed ordinary OK returns editable; a final-only Repeat cancels.
            let state = if self.repeat {
                Some(self.cancel(cx))
            } else {
                self.finalizing = false;
                None
            };
            crate::platform::files::message(&self.title, error);
            return state;
        }
        // spec-question: Q65 From File failed acquisition transactions
        if matches!(self.input, ExternalInput::Failed) {
            if !self.repeat {
                self.finalizing = false;
                return None;
            }
            let error = self.file_error.clone().unwrap_or_default();
            let state = self.cancel(cx);
            crate::platform::files::message(&self.title, error);
            return Some(state);
        }
        if !self.input_ready() || self.chooser.is_some() {
            return None;
        }
        if self
            .shown
            .as_ref()
            .is_none_or(|(g, _)| *g != self.generation)
        {
            return None;
        }
        let (_, pixels) = self.shown.take().unwrap();
        let file_key = self.file_key();
        let session = self.session.take().unwrap();
        if session.commit_with(cx.app, self.values.clone(), Some(pixels)) {
            self.publish_file_source(file_key);
            let notice = self.pending_notice.take();
            self.deliver_notice(notice);
            Some(DialogState::Closed(DialogResult::Ok))
        } else {
            Some(DialogState::Closed(DialogResult::Cancel))
        }
    }

    /// `notify(immediate)`: a new generation, its preview after the adaptive delay (now when
    /// immediate).
    pub(super) fn notify(&mut self, cx: &mut DialogCx, ctx: &egui::Context, immediate: bool) {
        self.on_change();
        self.invalidate(cx.app.now_ms, immediate);
        self.acquire_external(cx.app, ctx, cx.clipboard.as_deref_mut());
        if (!self.preview || matches!(self.input, ExternalInput::Failed))
            && let Some(session) = &self.session
        {
            session.show(cx.app, None);
        }
    }

    pub(super) fn invalidate(&mut self, now: f64, immediate: bool) {
        self.retire_work();
        self.pending_notice = None;
        self.shown = None;
        // R155 / BB33: the new values decide. Leaving an external source retires its read and
        // image at once; returning to one reads afresh.
        if let Some(session) = &mut self.session {
            session.set_image(None);
            session.set_lut(None);
        }
        self.input = if self.external() {
            ExternalInput::Unstarted
        } else {
            ExternalInput::Resolved
        };
        // R156: a newer value retires an open chooser; its late answer changes nothing.
        self.chooser = None;
        self.generation += 1;
        self.due = Some(if immediate { now } else { now + self.delay });
    }

    /// One native worker or one bounded browser job; abandoned jobs cannot publish.
    pub(super) fn run_previews(&mut self, cx: &mut DialogCx, ctx: &egui::Context) {
        if !self.input_ready()
            || self
                .session
                .as_ref()
                .is_none_or(|s| !s.owner_exists(cx.app))
        {
            return;
        }
        let now = cx.app.now_ms;
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok(result) => {
                    self.job = None;
                    self.job_cancel = None;
                    self.accept_result(cx, result);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                    // Only the current worker's loss fails a final; a retired one's flag is set.
                    if !self
                        .job_cancel
                        .take()
                        .is_some_and(|f| f.load(Ordering::Relaxed))
                    {
                        self.failed = self.finalizing;
                        self.worker_lost = self.finalizing;
                    }
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(job) = &mut self.sliced_seam {
            use bb::seam::CarveStatus;
            let frame_start = web_time::Instant::now();
            let result = loop {
                match job.carver.step(128, self.job_cancel.as_deref()) {
                    CarveStatus::Pending => {
                        if frame_start.elapsed() >= std::time::Duration::from_millis(4) {
                            ctx.request_repaint();
                            return;
                        }
                    }
                    CarveStatus::Complete(pixels, cut) => {
                        break Some((
                            pixels,
                            bb::seam::shortfall_message(
                                pdn_core::effect::num(&self.values, "amount") as u32,
                                cut,
                            ),
                        ));
                    }
                    CarveStatus::Cancelled => break None,
                }
            };
            let result = JobResult {
                generation: job.generation,
                result,
                elapsed: job.started.elapsed().as_secs_f64() * 1000.0,
            };
            self.sliced_seam = None;
            self.job_cancel = None;
            self.accept_result(cx, result);
        }
        let Some(due) = self.due else {
            return;
        };
        if now < due {
            ctx.request_repaint_after(
                std::time::Duration::from_millis((due - now).max(1.0) as u64),
            );
            return;
        }
        self.due = None;
        let Some(session) = &self.session else {
            return;
        };
        if !self.preview && !self.finalizing {
            session.show(cx.app, None);
            return;
        }
        match self.fonts_ready(cx, ctx) {
            Ok(true) => {}
            Ok(false) => {
                self.due = Some(now);
                return;
            }
            Err(_) => {
                // The source stays; ok() and finish_ready() report the failure.
                if let Some(session) = &self.session {
                    session.show(cx.app, None);
                }
                return;
            }
        }
        let Some(session) = &self.session else {
            return;
        };
        let (generation, values) = (self.generation, self.values.clone());
        let flag = Arc::new(AtomicBool::new(false));
        self.job_cancel = Some(flag.clone());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let session = session.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let start = web_time::Instant::now();
                let rendered = session.render_with(&values, Some(&flag));
                let result = (!flag.load(Ordering::Relaxed)).then_some(rendered);
                let _ = tx.send(JobResult {
                    generation,
                    result,
                    elapsed: start.elapsed().as_secs_f64() * 1000.0,
                });
                repaint.request_repaint();
            });
            self.job = Some(rx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(carver) = session.seam_carver(&values) {
                self.sliced_seam = Some(SlicedSeam {
                    generation,
                    carver,
                    started: web_time::Instant::now(),
                });
                ctx.request_repaint();
            } else {
                // Other effects retain their existing synchronous browser contract.
                let start = web_time::Instant::now();
                let result = Some(session.render_with(&values, Some(&flag)));
                self.job_cancel = None;
                self.accept_result(
                    cx,
                    JobResult {
                        generation,
                        result,
                        elapsed: start.elapsed().as_secs_f64() * 1000.0,
                    },
                );
            }
        }
    }

    /// The shown preview when it is the current values' (same session seed, current generation,
    /// nothing pending): OK reuses it, byte-identical to a fresh render.
    pub(super) fn current_preview(&mut self) -> Option<ImageData> {
        let current =
            self.input_ready() && self.preview && self.due.is_none() && self.job_cancel.is_none();
        self.shown
            .take()
            .filter(|(g, _)| current && *g == self.generation)
            .map(|(_, p)| p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// paintdotjs's adaptive preview delay (adjustments-and-effect-framework.md §4 "Preview
    /// cadence": paintdotjs kept).
    #[test]
    fn preview_delay_is_half_the_render_time_within_16_and_250() {
        assert_eq!(preview_delay(4.0), 16.0);
        assert_eq!(preview_delay(100.0), 50.0);
        assert_eq!(preview_delay(1000.0), 250.0);
    }
}

#[cfg(test)]
mod seam_tests {
    use super::super::*;
    use super::*;
    use crate::dialogs::task::TaskDialogSpec;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn pending_notices() -> usize {
        crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()).len())
    }

    fn dialog() -> (pdn_core::app::AppWorkspace, EffectConfigDialog) {
        pending_notices();
        let mut app = pdn_core::testing::fresh_doc(3, 3, Some(1));
        let def = pdn_core::effect::find("bbSeamCarvingVertical").unwrap();
        let values = pdn_core::effect::with(&def, &json!({"amount": 9}));
        let session = EffectSession::begin(&mut app, def).unwrap();
        (app, EffectConfigDialog::new(session, values))
    }
    fn with_cx<T>(app: &mut pdn_core::app::AppWorkspace, f: impl FnOnce(&mut DialogCx) -> T) -> T {
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

    fn frame(
        ctx: &egui::Context,
        app: &mut pdn_core::app::AppWorkspace,
        d: &mut EffectConfigDialog,
        events: Vec<egui::Event>,
        seat: bool,
    ) -> (DialogState, Rect) {
        let mut result = (DialogState::Open, Rect::NOTHING);
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                events,
                ..Default::default()
            },
            |ui| {
                result.0 = with_cx(app, |cx| {
                    cx.seat_focus = seat;
                    d.show(ui, cx)
                });
                result.1 = DialogFrameState::get(ctx, ui.scope_id()).rect;
            },
        );
        out.textures_delta.clear();
        result
    }

    fn key(key: Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    /// A ready receiver makes completion coincide with input, without timing a worker.
    fn ready_preview(d: &mut EffectConfigDialog, tx: std::sync::mpsc::Sender<JobResult>) {
        let result = d.session.as_ref().unwrap().render_with(&d.values, None);
        assert!(result.1.is_some());
        tx.send(JobResult {
            generation: d.generation,
            result: Some(result),
            elapsed: 1.0,
        })
        .unwrap();
    }

    /// Final review I3: OK's worker lost without a result (a panic) cancels with a message.
    #[test]
    fn lost_final_worker_reports_failure() {
        let (mut app, mut d) = dialog();
        let ctx = egui::Context::default();
        let before = pdn_core::testing::layer_bytes(&app, None);
        with_cx(&mut app, |cx| assert_eq!(d.ok(cx, &ctx), DialogState::Open));
        assert!(d.finalizing);
        let (tx, rx) = std::sync::mpsc::channel::<JobResult>();
        (d.due, d.job) = (None, Some(rx));
        d.job_cancel = Some(Arc::new(AtomicBool::new(false)));
        drop(tx);
        let state = with_cx(&mut app, |cx| {
            d.run_previews(cx, &ctx);
            d.finish_ready(cx)
        });
        assert_eq!(state, Some(DialogState::Closed(DialogResult::Cancel)));
        let pending = crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
        let messages: Vec<_> = pending.iter().map(|p| p.message()).collect();
        assert_eq!(messages, [Some("The effect could not be rendered.".into())]);
        assert_eq!(pdn_core::testing::layer_bytes(&app, None), before);
    }

    /// R108: Escape, footer Cancel and window close invalidate a result polled in this frame.
    #[test]
    fn same_frame_preview_cancel_suppresses_only_its_notice() {
        for cancel in ["escape", "footer", "close"] {
            let (mut app, mut d) = dialog();
            let ctx = egui::Context::default();
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            let baseline = pdn_core::testing::layer_bytes(&app, None);
            let history = app.active_ws().unwrap().history.undo.len();
            let tokens = app.effect_tokens.clone();
            let last = (app.last_effect.clone(), app.last_effect_values.clone());
            let (tx, rx) = std::sync::mpsc::channel();
            d.job = Some(rx);
            let (_, rect) = frame(&ctx, &mut app, &mut d, vec![], true);
            let events = if cancel == "escape" {
                key(Key::Escape)
            } else {
                let pos = if cancel == "footer" {
                    Pos2::new(rect.max.x - 40.0, rect.max.y - 20.0)
                } else {
                    Pos2::new(rect.max.x - 15.0, rect.min.y + 15.0)
                };
                let button = |pressed| egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(
                    &ctx,
                    &mut app,
                    &mut d,
                    vec![egui::Event::PointerMoved(pos)],
                    false,
                );
                frame(&ctx, &mut app, &mut d, vec![button(true)], false);
                vec![button(false)]
            };
            ready_preview(&mut d, tx);
            // A cancellation must leave someone else's pending dialog alone.
            TaskDialog::show(TaskDialogSpec {
                title: "Unrelated".into(),
                ..Default::default()
            });
            assert!(
                matches!(
                    frame(&ctx, &mut app, &mut d, events, false).0,
                    DialogState::Closed(DialogResult::Cancel)
                ),
                "{cancel}"
            );
            assert_eq!(
                pdn_core::testing::layer_bytes(&app, None),
                baseline,
                "{cancel}"
            );
            assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
            assert_eq!(app.effect_tokens, tokens);
            assert_eq!(
                (app.last_effect.clone(), app.last_effect_values.clone()),
                last
            );
            let mut stack = crate::dialogs::DialogStack::default();
            stack.take_pending();
            assert_eq!(
                stack.entries.len(),
                1,
                "{cancel}: cancelled warning survived"
            );
            assert!(stack.top_title_is("Unrelated"));
            assert_eq!(pending_notices(), 0);
            assert_eq!(d.noticed_generation, None);
        }
    }

    /// A real numeric key edit invalidates the just-polled generation before warning delivery.
    #[test]
    fn same_frame_preview_edit_suppresses_obsolete_notice() {
        let (mut app, mut d) = dialog();
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let baseline = pdn_core::testing::layer_bytes(&app, None);
        let history = app.active_ws().unwrap().history.undo.len();
        let tokens = app.effect_tokens.clone();
        let last = (app.last_effect.clone(), app.last_effect_values.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        d.job = Some(rx);
        frame(&ctx, &mut app, &mut d, vec![], true);
        frame(&ctx, &mut app, &mut d, vec![], false);
        ready_preview(&mut d, tx);
        assert!(matches!(
            frame(&ctx, &mut app, &mut d, key(Key::ArrowUp), false).0,
            DialogState::Open
        ));
        assert_eq!(d.values["amount"].as_f64(), Some(10.0));
        assert_eq!(d.generation, 1);
        assert_eq!(
            pending_notices(),
            0,
            "edited generation must not open a warning"
        );
        assert_eq!(d.noticed_generation, None);
        assert!(matches!(
            frame(&ctx, &mut app, &mut d, key(Key::Escape), false).0,
            DialogState::Closed(DialogResult::Cancel)
        ));
        assert_eq!(pdn_core::testing::layer_bytes(&app, None), baseline);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
        assert_eq!(app.effect_tokens, tokens);
        assert_eq!(
            (app.last_effect.clone(), app.last_effect_values.clone()),
            last
        );
        assert_eq!(pending_notices(), 0);
    }

    /// A ready preview accepted by OK in the polling frame still warns once after its commit.
    #[test]
    fn same_frame_preview_ok_commits_then_warns_once() {
        let (mut app, mut d) = dialog();
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let history = app.active_ws().unwrap().history.undo.len();
        let (tx, rx) = std::sync::mpsc::channel();
        d.job = Some(rx);
        frame(&ctx, &mut app, &mut d, vec![], true);
        frame(&ctx, &mut app, &mut d, vec![], false);
        ready_preview(&mut d, tx);
        assert!(matches!(
            frame(&ctx, &mut app, &mut d, key(Key::Enter), false).0,
            DialogState::Closed(DialogResult::Ok)
        ));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        assert_eq!(app.effect_tokens["bbSeamCarvingVertical"]["amount"], 9);
        assert_eq!(d.noticed_generation, Some(0));
        assert_eq!(pending_notices(), 1);
        assert!(d.pending_notice.is_none());
        assert!(with_cx(&mut app, |cx| d.finish_ready(cx)).is_none());
        assert_eq!(pending_notices(), 0);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
    }

    /// R108: edits cancel active work before a newer generation can publish pixels or warnings.
    #[test]
    fn stale_preview_job_is_cancelled() {
        let (mut app, mut d) = dialog();
        let flag = Arc::new(AtomicBool::new(false));
        d.job_cancel = Some(flag.clone());
        let stale = d.generation;
        d.invalidate(10.0, false);
        assert!(flag.load(Ordering::Relaxed));
        let before = pdn_core::testing::layer_bytes(&app, None);
        let pixels = ImageData {
            width: 3,
            height: 3,
            data: [0, 0, 0, 0].repeat(9),
        };
        let accepted = with_cx(&mut app, |cx| {
            d.accept_result(
                cx,
                JobResult {
                    generation: stale,
                    result: Some((pixels, Some("stale".into()))),
                    elapsed: 1.0,
                },
            )
        });
        assert!(!accepted);
        assert!(d.shown.is_none());
        assert_eq!(d.noticed_generation, None);
        assert_eq!(pending_notices(), 0);
        assert_eq!(pdn_core::testing::layer_bytes(&app, None), before);
    }

    /// R108: pending/off previews wait for an accepted final; only success changes history/tokens.
    #[test]
    fn final_and_repeat_accept_once_or_cancel_without_history() {
        for repeat in [false, true] {
            for cancel in [false, true] {
                let (mut app, mut d) = dialog();
                d.preview = false;
                if repeat {
                    d.finalizing = true;
                }
                let baseline = pdn_core::testing::layer_bytes(&app, None);
                let history = app.active_ws().unwrap().history.undo.len();
                let tokens = app.effect_tokens.clone();
                let last = app.last_effect.clone();
                if !repeat {
                    assert!(matches!(
                        with_cx(&mut app, |cx| d.ok(cx, &egui::Context::default())),
                        DialogState::Open
                    ));
                }
                assert!(d.finalizing);
                if cancel {
                    with_cx(&mut app, |cx| d.cancel(cx));
                    assert_eq!(pdn_core::testing::layer_bytes(&app, None), baseline);
                    assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
                    assert_eq!(app.effect_tokens, tokens);
                    assert_eq!(app.last_effect, last);
                    assert_eq!(d.noticed_generation, None);
                    assert_eq!(pending_notices(), 0);
                } else {
                    let (pixels, notice) = d.session.as_ref().unwrap().render_with(&d.values, None);
                    d.due = None;
                    let generation = d.generation;
                    assert!(with_cx(&mut app, |cx| d.accept_result(
                        cx,
                        JobResult {
                            generation,
                            result: Some((pixels, notice)),
                            elapsed: 1.0
                        }
                    )));
                    assert_eq!(pending_notices(), 0, "final warning waits for the commit");
                    assert!(matches!(
                        with_cx(&mut app, |cx| d.finish_ready(cx)),
                        Some(DialogState::Closed(DialogResult::Ok))
                    ));
                    assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
                    assert_eq!(app.effect_tokens["bbSeamCarvingVertical"]["amount"], 9);
                    assert_eq!(d.noticed_generation, Some(generation));
                    assert_eq!(pending_notices(), 1);
                    assert!(with_cx(&mut app, |cx| d.finish_ready(cx)).is_none());
                    assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
                }
            }
        }
    }

    /// Spec warning has the information symbol, exact title, one OK choice and OK cancel value.
    #[test]
    fn warning_contract_and_preview_ok_deduplicates() {
        let spec: TaskDialogSpec = seam_warning("message".into());
        assert_eq!(spec.title, "Warning");
        assert_eq!(spec.message.as_deref(), Some("message"));
        assert_eq!(spec.message_icon, Some("menu_help_about_icon.png"));
        assert_eq!(spec.cancel_value, "ok");
        assert_eq!(spec.choices.len(), 1);
        assert_eq!(
            (spec.choices[0].value, spec.choices[0].title.as_str()),
            ("ok", "OK")
        );
        let (mut app, mut d) = dialog();
        d.due = None;
        let (pixels, notice) = d.session.as_ref().unwrap().render_with(&d.values, None);
        let generation = d.generation;
        with_cx(&mut app, |cx| {
            d.accept_result(
                cx,
                JobResult {
                    generation,
                    result: Some((pixels, notice)),
                    elapsed: 1.0,
                },
            )
        });
        assert_eq!(pending_notices(), 0, "notice waits for this frame's inputs");
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        frame(&ctx, &mut app, &mut d, vec![], false);
        assert_eq!(d.noticed_generation, Some(generation));
        assert_eq!(pending_notices(), 1);
        let n = app.active_ws().unwrap().history.undo.len();
        assert!(matches!(
            with_cx(&mut app, |cx| d.ok(cx, &egui::Context::default())),
            DialogState::Closed(DialogResult::Ok)
        ));
        assert_eq!(d.noticed_generation, Some(generation));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
        assert_eq!(
            pending_notices(),
            0,
            "OK must not repeat the accepted preview notice"
        );
    }

    #[test]
    fn failed_final_suppresses_pixels_history_tokens_and_notice() {
        let (mut app, mut d) = dialog();
        d.finalizing = true;
        let before = pdn_core::testing::layer_bytes(&app, None);
        let n = app.active_ws().unwrap().history.undo.len();
        let tokens = app.effect_tokens.clone();
        let generation = d.generation;
        assert!(!with_cx(&mut app, |cx| d.accept_result(
            cx,
            JobResult {
                generation,
                result: None,
                elapsed: 1.0
            }
        )));
        assert!(matches!(
            with_cx(&mut app, |cx| d.finish_ready(cx)),
            Some(DialogState::Closed(DialogResult::Cancel))
        ));
        assert_eq!(pdn_core::testing::layer_bytes(&app, None), before);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        assert_eq!(app.effect_tokens, tokens);
        assert_eq!(d.noticed_generation, None);
    }

    /// R108 preserves the existing tool commit before Repeat; Cancel restores only its session baseline.
    #[test]
    fn repeat_cancel_preserves_the_prior_tool_commit() {
        use pdn_core::action::{ActionOutcome, UiRequest, perform};
        use pdn_core::testing::{Ev, drive_tool, fresh_doc, layer_bytes};
        use pdn_core::tool::{MouseButton, ToolType};
        let mut app = fresh_doc(10, 10, None);
        let def = pdn_core::effect::find("bbSeamCarvingVertical").unwrap();
        let values = pdn_core::effect::with(&def, &json!({"amount":9}));
        app.last_effect = Some(def.id.into());
        app.last_effect_values = Some(values.clone());
        drive_tool(
            &mut app,
            ToolType::MagicWand,
            &[
                Ev::Down(1.0, 1.0, MouseButton::Left),
                Ev::Up(1.0, 1.0, MouseButton::Left),
            ],
        );
        let before = app.active_ws().unwrap().history.undo.len();
        assert_eq!(
            perform(&mut app, "menu.effects.repeat"),
            ActionOutcome::Ui(UiRequest::RepeatSeam)
        );
        let history = app.active_ws().unwrap().history.undo.len();
        assert_eq!(history, before + 1);
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().name,
            "Finish Magic Wand"
        );
        let bytes = layer_bytes(&app, None);
        let tokens = app.effect_tokens.clone();
        let session = EffectSession::begin(&mut app, def).unwrap();
        let mut d = EffectConfigDialog::final_only(session, values);
        with_cx(&mut app, |cx| d.cancel(cx));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
        assert_eq!(app.effect_tokens, tokens);
        assert_eq!(layer_bytes(&app, None), bytes);
    }
}
