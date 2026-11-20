//! The Effect Config dialog: the generic control rows, the Hue / Saturation, Rotate / Zoom,
//! Curves and Levels layouts, the Preview checkbox and the adaptive preview.
//! Port of: js/src/interface/window/dialog/EffectConfigDialog.js (`open`, `createGenericControl`,
//! `createFooter`, `createSpecializedSlider`, `createSpecializedSelect`, the four
//! `create*Content`, `createRollVisual`, `createPanVisual`, `createLevels*`,
//! `enableSliderThumbReset`), css/window.css (`.effect-config-*`, `.effect-special-*`,
//! `.hue-saturation-*`, `.rotate-*`, `.curves-*`, `.levels-*`), css/component.css (`input`,
//! `button`); BitmapEffectAction.js (`preview`, worker jobs).
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's flow and Levels and Curves dialog behaviour; the preview cadence, the Preview
//! checkbox and the curve editor keep paintdotjs's; colour properties use paintdotjs's
//! `input[type=color]` row (Ruling (P3) 1); the pan thumbnail uses the thumbnail scaler.
//! BB spec: common.md §3 (plugin dialogs: tabs, header lines, footnotes, track colours,
//! enable rules, ×1.1, captions, text / label / layer / pan / file rows; BB plan BB5);
//! effects/fill-from-file.md (the file source, BB27).

mod canvas;
mod code;
mod color_popup;
mod external;
mod layouts;
mod preview;
mod special;
mod widgets;

pub use external::{FileAttachment, FileSource};
#[cfg(test)]
pub(super) use widgets::title_plus_rect;
pub(super) use widgets::{
    OpenSelect, RangeSpec, SelectLook, SliderState, Track, button_clicked, checkbox_line,
    icon_button, paint_range, paint_title_plus, range_line, range_rects, select,
};

use crate::platform::{Files, Pending, PickedFile};
use pdn_core::effect::bb::ui::External;

use egui::{ColorImage, Id, Key, PointerButton, Pos2, Rect, Ui, Vec2};
use pdn_core::canvas::{Canvas, ImageData};
use pdn_core::effect::bb::ui::Help;
use pdn_core::effect::permission::Request;
use pdn_core::effect::session::EffectSession;
use pdn_core::effect::{Control, DialogLayout, PanThumb, Values};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::AtomicBool};

use super::curve_editor::{self, CurveEditor, Point};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs;
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{DialogFrameState, Footer, dialog_button, dialog_frame_with};
use crate::theme::{self, *};
use color_popup::ColorPopup;
use external::{ExternalInput, PresetLoad};
use preview::JobResult;
#[cfg(target_arch = "wasm32")]
use preview::{SlicedPlugin, SlicedSeam};

/// A plug-in FileType's window (`EffectConfigDialog::for_window`): its controls, and where OK or Cancel goes.
struct WindowAnswer {
    controls: Vec<Control>,
    done: Option<WindowDone>,
}

/// Where a FileType's window's OK (its values) or Cancel (`None`) goes.
type WindowDone = Box<dyn FnOnce(Option<Values>, &mut DialogCx)>;

/// The title-bar icon of a FileType's window: the plug-in effects' one.
const PLUGIN_WINDOW_ICON: &str = pdn_core::effect::dotnet::ICON;

/// The Curves layout's editors: one per mode, each keeping its own points.
struct CurvesState {
    luminosity: CurveEditor,
    rgb: CurveEditor,
}

const CURVE_KEYS: [&str; 4] = ["points", "redPoints", "greenPoints", "bluePoints"];

/// The Levels triples Reset restores.
const LEVELS_KEYS: [&str; 5] = ["inputLow", "inputHigh", "gamma", "outputLow", "outputHigh"];

fn points_of(values: &Values, key: &str) -> Vec<Point> {
    let mut pts: Vec<Point> = values
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some([
                p.get(0)?.as_f64()?.round().clamp(0.0, 255.0) as i32,
                p.get(1)?.as_f64()?.round().clamp(0.0, 255.0) as i32,
            ])
        })
        .collect();
    pts.sort_by_key(|p| p[0]);
    if pts.len() < 2 {
        pts = curve_editor::IDENTITY.to_vec();
    }
    pts
}

fn points_json(points: &[Point]) -> Value {
    points.iter().map(|p| json!([p[0], p[1]])).collect()
}

pub struct EffectConfigDialog {
    input: ExternalInput,
    /// The file service captured at open.
    files: Files,
    /// Shell's committed attachment; only this session's successful commit replaces it.
    file_source: FileSource,
    /// This invocation's attached pick (byte or browser-handle sources; native picks are paths).
    candidate: Option<PickedFile>,
    /// The open chooser, apart from the load it does not retire.
    chooser: Option<Pending<Result<Vec<PickedFile>, String>>>,
    /// Load Preset...'s file work (`IndirectUi::preset`).
    preset_load: Option<PresetLoad>,
    /// The current inline file error, until a later load succeeds or resolves to no image.
    file_error: Option<String>,
    /// Final-only Repeat: a failed load cancels it with a message (Q65).
    repeat: bool,
    /// Tests turn native typed-path reads off (a browser never reads them, Q64).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    typed_paths: bool,
    #[cfg(test)]
    acquisitions: usize,
    session: Option<EffectSession>,
    /// A plug-in FileType's window shown while a file opens or saves (`for_window`), in place of a session.
    window: Option<WindowAnswer>,
    values: Values,
    title: String,
    icon: String,
    /// The footer's Preview checkbox (`previewEnabled`).
    preview: bool,
    /// `previewGeneration`: bumped by every change; stale results are dropped.
    generation: u64,
    /// When the next preview starts (app clock, ms); `None` = nothing scheduled.
    due: Option<f64>,
    delay: f64,
    job: Option<std::sync::mpsc::Receiver<JobResult>>,
    job_cancel: Option<Arc<AtomicBool>>,
    #[cfg(target_arch = "wasm32")]
    sliced_seam: Option<SlicedSeam>,
    #[cfg(target_arch = "wasm32")]
    sliced_plugin: Option<SlicedPlugin>,
    finalizing: bool,
    failed: bool,
    /// The final render's worker ended without a result (a panic): `failed`, and reported.
    worker_lost: bool,
    pending_notice: Option<String>,
    noticed_generation: Option<u64>,
    /// A plug-in's outside request being asked about ([`EffectConfigDialog::ask`]).
    asking: Option<Asking>,
    /// The network requests whose responses this run handed the host, so a host that keeps asking for one stops.
    fetched: Vec<String>,
    /// The request last run again after an answer the host already had, so a host that keeps asking stops here.
    rerun_for: Option<Request>,
    /// The plug-in asked about, whose Allow once answers end with this run.
    asked: Option<String>,
    /// A request that cannot happen here was explained; the plug-in's failure after it needs no second box.
    explained: bool,
    curves: Option<CurvesState>,
    /// A plug-in window's live canvas (`plugin_canvas`): the window kept open while this dialog is.
    live_canvas: Option<canvas::LiveCanvas>,
    /// Why that window did not open, so it is not tried again every frame.
    canvas_error: Option<String>,
    /// The sizes the plug-in window's canvases are shown at, in points.
    canvas_shown: Vec<(&'static str, (u32, u32))>,
    /// A plug-in window's code editors (`plugin_code`), by key.
    code_edits: std::collections::HashMap<&'static str, code::CodeEdit>,
    /// The Levels gradient handle being dragged: (output column, handle index).
    levels_drag: Option<(bool, usize)>,
    /// The Rotate/Zoom roll canvas drag: (start pointer, start roll vector, on the sphere).
    roll_drag: Option<(Pos2, (f64, f64), bool)>,
    /// The last thumb press per slider and button (`enableSliderThumbReset`), app ms.
    thumb_presses: Vec<(Id, PointerButton, f64)>,
    /// A slider drag: its id and the pointer's offset from the thumb centre.
    slider_drag: Option<(Id, f32)>,
    /// The open select.
    open_select: Option<OpenSelect>,
    color_popup: Option<ColorPopup>,
    /// An outside dismissal owns the pointer through its release.
    color_dismiss_gesture: bool,
    /// Cached canvases (`<canvas>` elements): content hash and texture per id.
    canvases: Vec<(Id, u64, egui::TextureHandle)>,
    /// The Levels dialog's input histogram (selected pixels, §2.5).
    histogram: Option<Box<[[u32; 256]; 4]>>,
    /// The roll sphere as last drawn, by its three values' bits.
    roll_image: Option<([u64; 3], ColorImage)>,
    /// The pan thumbnail (the layer does not change while the dialog is open).
    pan_thumb: Option<Canvas>,
    /// A plugin's pan thumbnails: per kind, the fitted size and the thumbnail.
    pan_thumbs: Vec<(PanThumb, (f64, f64), Canvas)>,
    /// The preview in the layer: its generation and pixels.
    shown: Option<(u64, ImageData)>,
    /// R157: a required document face failed to load; nothing renders or commits.
    font_error: Option<String>,
    /// R157: the required faces' bytes the current generation renders with, and the font
    /// manager revision they were last compared at.
    faces: (u64, Vec<Arc<[u8]>>),
    /// BB36: Chart or Graph's Window 1, its children and its file work (`DialogLayout::Chart`).
    pub(super) chart: Option<super::chart::ChartDialog>,
    /// A plugin dialog taller than its window: the rows' scroll offset (px) and the focused
    /// widget it last scrolled into view.
    rows_scroll: (f64, Option<Id>),
}

impl EffectConfigDialog {
    /// `EffectConfigDialog.open` for `session`'s effect with `values` (the remembered token or
    /// the defaults). R154 Q60: a value outside a dynamic slider range clamps in this draft
    /// only, once.
    pub fn new(session: EffectSession, values: Values) -> Self {
        let mut d = Self::build(session, values);
        if let Some(s) = &d.session
            && let Some(ui) = s.def.indirect
        {
            let env = s.env();
            for c in &s.def.controls {
                let Some(range) = ui.control(c.key()).range else {
                    continue;
                };
                let (lo, hi) = range(&env);
                if let Some(v) = d.values.get(c.key()).and_then(Value::as_f64)
                    && !(lo..=hi).contains(&v)
                {
                    set(&mut d.values, c.key(), v.clamp(lo, hi));
                }
            }
        }
        d
    }

    fn build(mut session: EffectSession, mut values: Values) -> Self {
        let def = &session.def;
        let title = def
            .dialog_title
            .map_or_else(|| def.display_name(), String::from);
        let icon = def.icon.to_string();
        let curves = (def.dialog == Some(DialogLayout::Curves)).then(|| {
            for key in CURVE_KEYS {
                let pts = points_of(&values, key);
                values.insert(key.into(), points_json(&pts));
            }
            CurvesState {
                luminosity: CurveEditor::new(points_of(&values, "points")),
                rgb: CurveEditor::with_channels(
                    CURVE_KEYS[1..]
                        .iter()
                        .map(|k| points_of(&values, k))
                        .collect(),
                ),
            }
        });
        if def.dialog == Some(DialogLayout::Levels) {
            // Paint.NET opens Levels with R, G and B all checked (§2.9 "all on at load").
            for k in ["red", "green", "blue"] {
                values.insert(k.into(), json!(true));
            }
        }
        let histogram = (def.dialog == Some(DialogLayout::Levels)).then(|| {
            Box::new(pdn_core::effect::adjustments::histogram(
                session.source(),
                session.mask(),
            ))
        });
        if let Some(ui) = session.def.indirect {
            (ui.on_change)(&mut values, &session.env());
        }
        let chart = (session.def.dialog == Some(DialogLayout::Chart))
            .then(|| super::chart::ChartDialog::open(&mut values, &session));
        let external = session
            .def
            .indirect
            .is_some_and(|u| (u.external_for)(&values) != External::None);
        if external {
            session.set_image(None);
        }
        let mut d = Self::blank(values, title, icon);
        if external {
            d.input = ExternalInput::Unstarted;
        }
        (d.session, d.curves, d.histogram, d.chart) = (Some(session), curves, histogram, chart);
        d
    }

    /// A dialog with no session yet: nothing to preview, no file work.
    fn blank(values: Values, title: String, icon: String) -> Self {
        Self {
            input: ExternalInput::Resolved,
            files: Files::current(),
            file_source: FileSource::default(),
            candidate: None,
            chooser: None,
            preset_load: None,
            file_error: None,
            repeat: false,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            typed_paths: true,
            #[cfg(test)]
            acquisitions: 0,
            session: None,
            window: None,
            values,
            title,
            icon,
            preview: true,
            generation: 0,
            // `notify(true)` as the dialog opens.
            due: Some(f64::NEG_INFINITY),
            delay: 0.0,
            job: None,
            job_cancel: None,
            #[cfg(target_arch = "wasm32")]
            sliced_seam: None,
            #[cfg(target_arch = "wasm32")]
            sliced_plugin: None,
            finalizing: false,
            failed: false,
            worker_lost: false,
            pending_notice: None,
            noticed_generation: None,
            asking: None,
            fetched: vec![],
            rerun_for: None,
            asked: None,
            explained: false,
            curves: None,
            live_canvas: None,
            canvas_error: None,
            canvas_shown: Vec::new(),
            code_edits: Default::default(),
            levels_drag: None,
            roll_drag: None,
            thumb_presses: vec![],
            slider_drag: None,
            open_select: None,
            color_popup: None,
            color_dismiss_gesture: false,
            canvases: vec![],
            histogram: None,
            roll_image: None,
            pan_thumb: None,
            pan_thumbs: vec![],
            shown: None,
            font_error: None,
            faces: (0, vec![]),
            chart: None,
            rows_scroll: (0.0, None),
        }
    }

    /// A plug-in FileType's window (`PluginWindow`) shown while a file opens or saves: its settings in the generic
    /// rows, at their defaults, and OK and Cancel. OK hands `done` the values, Cancel (or closing it) `None`.
    pub fn for_window(
        title: String,
        controls: Vec<Control>,
        done: impl FnOnce(Option<Values>, &mut DialogCx) + 'static,
    ) -> Self {
        let values = controls
            .iter()
            .map(|c| (c.key().to_owned(), c.default_value()))
            .collect();
        let mut d = Self::blank(values, title, PLUGIN_WINDOW_ICON.into());
        d.window = Some(WindowAnswer {
            controls,
            done: Some(Box::new(done)),
        });
        d
    }

    /// R152 Q54: the plugin's idempotent hook on the current values.
    fn on_change(&mut self) {
        if let Some(s) = &self.session
            && let Some(ui) = s.def.indirect
        {
            (ui.on_change)(&mut self.values, &s.env());
        }
    }

    /// A plug-in window's button (`Control::Button`): pressed in the plug-in's own window over the layer, keeping the
    /// token that makes and the window's settings after it (a list's new items). False when it could not be
    /// pressed; the user is told why, as for a render that stops with an error.
    pub(super) fn press(&mut self, key: &str) -> bool {
        // A window kept open for its canvases takes the press itself, on its own state.
        let live = self.press_live(key);
        let Some(s) = &self.session else {
            return false;
        };
        let env = s.env();
        let pressed = match live {
            Some(done) => done,
            None => {
                pdn_core::effect::dotnet::press(s.def.id, key, s.source(), &mut self.values, &env)
            }
        };
        match pressed {
            Ok(()) => true,
            Err(e) if let Some(request) = Request::from_error(&e) => {
                self.ask(request, Rerun::Press(key.to_owned()));
                false
            }
            Err(e) => {
                let text = pdn_core::effect::dotnet::failed(&self.title, &e);
                self.deliver_notice(Some(text));
                false
            }
        }
    }

    /// A committed value; the hook sees every one, so several edits in one frame all count.
    fn set_value(&mut self, key: &str, v: Value) {
        self.values.insert(key.into(), v);
        self.on_change();
    }

    /// Repeat bypasses controls while retaining the same final result owner and cancellation.
    pub fn final_only(session: EffectSession, values: Values) -> Self {
        let mut dialog = Self::build(session, values);
        dialog.preview = false;
        dialog.finalizing = true;
        dialog.repeat = true;
        dialog
    }

    /// `final_only` with the external image its opener already read (Vanishing Point pastes the
    /// one its preview showed), so the final render cannot read another.
    pub fn final_with_image(
        session: EffectSession,
        values: Values,
        image: Option<Arc<ImageData>>,
    ) -> Self {
        let mut dialog = Self::final_only(session, values);
        if let Some(s) = &mut dialog.session {
            s.set_image(image);
        }
        dialog.input = ExternalInput::Resolved;
        dialog
    }

    fn layout(&self) -> Option<DialogLayout> {
        self.session.as_ref().and_then(|s| s.def.dialog)
    }

    /// The dialog's controls; a plug-in window's canvas at the size it is shown (`fit_canvas`).
    fn controls(&self) -> Vec<Control> {
        // A live window's own: rows its input brought (a file it loaded) join the schema's.
        let mut controls = match (&self.window, &self.live_canvas) {
            (Some(w), _) => w.controls.clone(),
            (None, Some(live)) => live.canvas.controls().to_vec(),
            (None, None) => self
                .session
                .as_ref()
                .map(|s| s.def.controls.clone())
                .unwrap_or_default(),
        };
        for c in &mut controls {
            if let Control::PluginCanvas {
                key, width, height, ..
            } = c
                && let Some(&(_, shown)) = self.canvas_shown.iter().find(|s| s.0 == *key)
            {
                (*width, *height) = shown;
            }
        }
        controls
    }

    /// The footer's Reset: Curves' identity curves; Levels' five triples from the defaults (§2.9,
    /// ruling R61); then an immediate preview.
    fn reset(&mut self) {
        if let Some(c) = &mut self.curves {
            c.luminosity.reset();
            c.rgb.reset();
            sync_curves(c, &mut self.values);
        } else if let Some(def) = self.session.as_ref().map(|s| &s.def) {
            let d = pdn_core::effect::defaults(def);
            for k in LEVELS_KEYS {
                if let Some(v) = d.get(k) {
                    self.values.insert(k.into(), v.clone());
                }
            }
        }
    }

    /// A plugin row's reset arrow: the session's default (dynamic ones included, artistic-dream.md
    /// "Default Radius"), then an immediate preview.
    pub(crate) fn reset_control(&mut self, key: &str) {
        if let Some(v) = self.session.as_ref().and_then(|s| s.defaults().remove(key)) {
            self.values.insert(key.into(), v);
        }
    }

    fn ok(&mut self, cx: &mut DialogCx, ctx: &egui::Context) -> DialogState {
        if let Some(done) = self.window.as_mut().and_then(|w| w.done.take()) {
            done(Some(self.values.clone()), cx);
            return DialogState::Closed(DialogResult::Ok);
        }
        self.poll_external(cx.app);
        if self
            .session
            .as_ref()
            .is_none_or(|s| !s.owner_exists(cx.app))
        {
            return self.cancel(cx);
        }
        if matches!(self.input, ExternalInput::Retired) {
            self.input = ExternalInput::Unstarted;
        }
        self.acquire_external(cx.app, ctx, cx.clipboard.as_deref_mut());
        // spec-question: Q65 From File failed acquisition transactions
        if matches!(self.input, ExternalInput::Failed) {
            return DialogState::Open;
        }
        let fonts = match self.fonts_ready(cx, ctx) {
            Ok(ready) => ready,
            Err(error) => {
                crate::platform::files::message(&self.title, error);
                return DialogState::Open;
            }
        };
        let rendered = self.current_preview();
        // A Paint.NET plug-in renders its final through the dialog too, so an outside request it stops at is asked.
        let plugin = self
            .session
            .as_ref()
            .is_some_and(|s| matches!(s.def.kind, pdn_core::effect::EffectKind::Plugin));
        if !fonts
            || !self.input_ready()
            || self.chooser.is_some()
            || (self.seam() && rendered.is_none())
            || (plugin && rendered.is_none())
        {
            self.finalizing = true;
            self.open_select = None;
            self.due = Some(cx.app.now_ms);
            return DialogState::Open;
        }
        self.cancel_job();
        if rendered.is_some() && self.plugin_failed() {
            return self.fail_plugin(cx);
        }
        let file_key = self.file_key();
        if let Some(s) = self.session.take()
            && s.commit_with(cx.app, self.values.clone(), rendered)
        {
            self.keep_canvas();
            self.publish_file_source(file_key);
            let notice = self.pending_notice.take();
            self.deliver_notice(notice);
        }
        DialogState::Closed(DialogResult::Ok)
    }

    fn cancel(&mut self, cx: &mut DialogCx) -> DialogState {
        if let Some(done) = self.window.as_mut().and_then(|w| w.done.take()) {
            done(None, cx);
            return DialogState::Closed(DialogResult::Cancel);
        }
        self.cancel_job();
        self.input = ExternalInput::Retired;
        self.chooser = None;
        self.pending_notice = None;
        if let Some(s) = self.session.take() {
            s.cancel(cx.app);
        }
        DialogState::Closed(DialogResult::Cancel)
    }

    fn applying(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let mut cancel = false;
        let mut body = |ui: &mut Ui, r: Rect| {
            text::draw(
                ui.painter(),
                r.min + Vec2::splat(16.0),
                "Applying",
                FontSpec::new(DIALOG_FONT_SIZE),
                theme::current(ui.ctx()).menu_item_text,
            );
        };
        let mut footer = |ui: &mut Ui, r: Rect| {
            let button = Rect::from_min_size(
                Pos2::new(r.max.x - 90.0, r.min.y + 8.0),
                Vec2::new(80.0, 24.0),
            );
            let response = dialog_button(ui, "cancel", button, "Cancel", false);
            cancel = inputs::button_activated(ui, &response);
        };
        let frame = dialog_frame_with(
            ui,
            &self.title,
            Some(&self.icon),
            false,
            (Vec2::new(390.0, 65.0), &mut body),
            Some(&mut footer),
            Footer::DEFAULT,
        );
        paint_title_plus(
            ui,
            self.session.as_ref().map(|s| s.def.id),
            frame.rect,
            &self.title,
        );
        if cx.seat_focus {
            ui.ctx().memory_mut(|m| m.request_focus(frame.close_id));
        }
        if cancel || frame.closed {
            self.cancel(cx)
        } else {
            if let Some(immediate) = self.poll_chooser() {
                self.notify(cx, ui.ctx(), immediate);
            }
            let ctx = ui.ctx().clone();
            self.rerun_answered(cx, &ctx);
            self.poll_external(cx.app);
            self.check_faces(cx);
            self.run_previews(cx, ui.ctx());
            self.repaint_file_work(ui.ctx());
            self.finish_ready(cx).unwrap_or(DialogState::Open)
        }
    }
}

impl EffectConfigDialog {
    /// Window 1 of Chart or Graph; every change writes `values["chart"]` (R143's token).
    fn chart_content(&mut self, ui: &mut Ui, r: Rect) -> Option<bool> {
        let chart = self.chart.as_mut()?;
        let changed = chart.show(ui, r, &mut self.open_select);
        if changed.is_some() {
            self.values.insert("chart".into(), chart.token());
        }
        changed
    }

    /// A current value (tests).
    #[cfg(test)]
    pub(super) fn value(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// The shown preview is the current generation's (tests).
    #[cfg(test)]
    pub(super) fn preview_current(&self) -> bool {
        self.shown
            .as_ref()
            .is_some_and(|(g, _)| *g == self.generation)
            && self.due.is_none()
    }
}

impl Drop for EffectConfigDialog {
    fn drop(&mut self) {
        self.cancel_job();
        // Allow once was for this run.
        if let Some(plugin) = &self.asked {
            pdn_core::effect::permission::with(|p| p.end_once(plugin));
        }
    }
}

/// What runs again once an outside request has its answer.
#[derive(Clone, Debug, PartialEq)]
enum Rerun {
    Render,
    Press(String),
}

/// An outside request being asked about: what runs again, whether the prompt has its answer, and for a network
/// request paint.rs makes itself, where its response lands.
struct Asking {
    rerun: Rerun,
    request: Request,
    answered: std::rc::Rc<std::cell::Cell<bool>>,
    response: Option<std::sync::Arc<std::sync::Mutex<Option<String>>>>,
}

impl EffectConfigDialog {
    /// A plug-in stopped at an outside request (the network, files, a program, a system setting, native code):
    /// asks once, and the answer runs `rerun` again, so the plug-in goes on allowed, or fails the call as it would
    /// offline. One the host should already have the answer to (it has just been sent) runs again once.
    fn ask(&mut self, request: Request, rerun: Rerun) {
        use pdn_core::effect::permission;
        if self.asking.is_some() {
            return;
        }
        self.asked = Some(request.plugin.clone());
        let answered = std::rc::Rc::new(std::cell::Cell::new(false));
        if request.http.is_some() && permission::with(|p| p.allows(&request)) {
            // Allowed: the host waits for paint.rs to make the request; once only for each one.
            if self.fetched.contains(&request.key) {
                let text = format!("{}, and it was not passed on.", request.message());
                crate::platform::files::message(&self.title, text);
                return;
            }
            answered.set(true);
        } else if permission::with(|p| p.answered(&request)) {
            if self.rerun_for.as_ref() == Some(&request) {
                let text = format!("{}, and it was not passed on.", request.message());
                crate::platform::files::message(&self.title, text);
                return;
            }
            self.rerun_for = Some(request.clone());
            answered.set(true);
        } else if !request.unavailable.is_empty() {
            // It cannot happen here (a browser cannot change the wallpaper): the plug-in carries on without it.
            let text = format!("{}, but {}.", request.message(), request.unavailable);
            permission::with(|p| p.deny(&request));
            crate::platform::files::message(&self.title, text);
            self.explained = true;
            answered.set(true);
        } else {
            let spec = crate::platform::plugins::permission_prompt(&request);
            let (done, asked) = (answered.clone(), request.clone());
            super::task::TaskDialog::show_then(spec, move |choice, cx| {
                crate::platform::plugins::answer_permission(&asked, choice, cx.prefs);
                done.set(true);
            });
        }
        self.asking = Some(Asking {
            rerun,
            request,
            answered,
            response: None,
        });
    }

    /// A Paint.NET plug-in's render ended with an error (its notice): the layer stays as it was.
    fn plugin_failed(&self) -> bool {
        self.pending_notice.is_some()
            && self
                .session
                .as_ref()
                .is_some_and(|s| matches!(s.def.kind, pdn_core::effect::EffectKind::Plugin))
    }

    /// As a plug-in run that throws without the dialog: say why, and add no history step.
    fn fail_plugin(&mut self, cx: &mut DialogCx) -> DialogState {
        let notice = self.pending_notice.take();
        let state = self.cancel(cx);
        if !self.explained {
            self.deliver_notice(notice);
        }
        state
    }

    /// The answer came: run again what stopped at the request.
    fn rerun_answered(&mut self, cx: &mut DialogCx, ctx: &egui::Context) {
        let Some(mut asking) = self.asking.take() else {
            return;
        };
        if !asking.answered.get() {
            self.asking = Some(asking);
            return;
        }
        // An allowed network request: make it, give the host its response, then run again.
        if let Some(http) = asking.request.http.clone()
            && pdn_core::effect::permission::with(|p| p.allows(&asking.request))
        {
            let Some(slot) = &asking.response else {
                asking.response = Some(crate::platform::plugins::fetch_for_plugin(
                    http,
                    ctx.clone(),
                ));
                self.asking = Some(asking);
                return;
            };
            let Some(json) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else {
                self.asking = Some(asking);
                return;
            };
            pdn_core::effect::dotnet::store_network_response(&asking.request.key, &json);
            self.fetched.push(asking.request.key.clone());
        }
        match asking.rerun {
            // As `notify`: a new generation, its external input (the clipboard, a file) read again.
            Rerun::Render => {
                self.invalidate(cx.app.now_ms, true);
                self.acquire_external(cx.app, ctx, cx.clipboard.as_deref_mut());
            }
            Rerun::Press(key) => {
                if self.press(&key) {
                    self.notify(cx, ctx, true);
                }
            }
        }
    }
}

// ---- values ----

fn num(values: &Values, key: &str) -> f64 {
    pdn_core::effect::num(values, key)
}

fn set(values: &mut Values, key: &str, v: f64) {
    values.insert(key.into(), json!(v));
}

fn triple(values: &Values, key: &str) -> [f32; 3] {
    let a = values.get(key).and_then(Value::as_array);
    std::array::from_fn(|i| {
        a.and_then(|a| a.get(i))
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32
    })
}

fn set_triple(values: &mut Values, key: &str, t: [f32; 3]) {
    values.insert(key.into(), json!([t[0], t[1], t[2]]));
}

const INITIAL_EDITOR: &str = "paintrs.effect.initial-editor";

pub(super) fn initial_editor(ui: &Ui, id: Id) {
    if ui.is_enabled() && ui.is_visible() {
        ui.ctx().data_mut(|d| {
            let key = Id::unique(INITIAL_EDITOR);
            if d.get_temp::<Id>(key).is_none() {
                d.insert_temp(key, id);
            }
        });
    }
}

impl Dialog for EffectConfigDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        let window = self.window.is_some();
        if !window
            && self
                .session
                .as_ref()
                .is_none_or(|s| !s.owner_exists(cx.app))
        {
            return self.cancel(cx);
        }
        ctx.data_mut(|d| d.remove::<Id>(Id::unique(INITIAL_EDITOR)));
        let popup_was_open = self.color_popup.is_some();
        let block_parent = popup_was_open || self.color_dismiss_gesture;
        // A click or key that works an open select's list is the list's.
        let list_open = self.open_select.is_some();
        // Escape closes an open list, else cancels.
        if !block_parent
            && ui.input(|i| {
                !i.modifiers.ctrl
                    && !i.modifiers.alt
                    && !i.modifiers.mac_cmd
                    && i.key_pressed(Key::Escape)
            })
        {
            if !list_open {
                return self.cancel(cx);
            }
            self.open_select = None;
        }
        if self.finalizing {
            return self.applying(ui, cx);
        }
        let enter = ui.input(|i| {
            !i.modifiers.ctrl
                && !i.modifiers.alt
                && !i.modifiers.mac_cmd
                && i.key_pressed(Key::Enter)
        });
        let now = cx.app.now_ms;
        self.fit_canvas(ui);
        let (width, height) = self.size(ui);
        let mut changed: Option<bool> = self.settle_live(ui, now);
        let mut action: Option<FooterAction> = None;
        let layout = self.layout();
        let controls = self.controls();
        let (title, icon) = (self.title.clone(), self.icon.clone());
        let help = self
            .session
            .as_ref()
            .and_then(|s| s.def.indirect)
            .map_or(Help::None, |u| u.help);
        let effect_id = self.session.as_ref().map(|s| s.def.id);
        let preset = self.preset().is_some();
        let preview_before = self.preview;
        let mut preview = self.preview;
        let primary = cx.app.primary_color;
        let secondary = cx.app.secondary_color;
        let mut body = |ui: &mut Ui, r: Rect| {
            let c = match layout {
                None => match self.session.as_ref().and_then(|s| s.def.indirect) {
                    Some(ind) => self.indirect(ui, r, ind, &controls, now, [primary, secondary]),
                    None => self.generic(ui, r, &controls, now, [primary, secondary]),
                },
                Some(DialogLayout::HueSaturation) => self.hue_saturation(ui, r, &controls, now),
                Some(DialogLayout::RotateZoom) => self.rotate_zoom(ui, r, &controls, now),
                Some(DialogLayout::Curves) => self.curves_content(ui, r),
                Some(DialogLayout::Levels) => self.levels_content(ui, r),
                Some(DialogLayout::Chart) => self.chart_content(ui, r),
            };
            changed = merge(changed, c);
        };
        let mut footer = |ui: &mut Ui, f: Rect| {
            action = footer_ui(ui, f, layout, preset, (!window).then_some(&mut preview));
        };
        let chart = layout == Some(DialogLayout::Chart);
        let mut draw_parent = || {
            dialog_frame_with(
                ui,
                &title,
                Some(&icon),
                !matches!(help, Help::None),
                (Vec2::new(width, height), &mut body),
                (!chart).then_some(&mut footer as crate::kit::window::DialogPainter),
                Footer {
                    border: false,
                    ..Footer::DEFAULT
                },
            )
        };
        let frame = if block_parent {
            super::without_input(&ctx, draw_parent)
        } else {
            draw_parent()
        };
        if !block_parent
            && frame.help_clicked
            && let Some(id) = effect_id
        {
            super::help::open(id, help, frame.rect);
        }
        paint_title_plus(ui, effect_id, frame.rect, &title);
        self.preview = preview;
        let display =
            crate::shell::canvas_view::DisplayPrefs::new(cx.settings, &theme::current(&ctx));
        changed = merge(
            changed,
            self.show_color_popup(
                ui,
                crate::kit::color::ColorLook {
                    mode: display.mode,
                    checker_brightness: display.checker_brightness,
                    dark: display.dark,
                    disabled: false,
                },
            ),
        );
        let consuming_gesture = self.color_dismiss_gesture;
        if consuming_gesture && !ui.input(|i| i.pointer.any_down()) {
            self.color_dismiss_gesture = false;
        }
        // The open list belongs to the dialog: the stack does not flash for presses on it.
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        state.popup =
            if self.open_select.is_some() || self.color_popup.is_some() || consuming_gesture {
                ctx.content_rect()
            } else {
                Rect::NOTHING
            };
        state.set(&ctx, ui.scope_id());
        if cx.seat_focus && !block_parent && self.color_popup.is_none() {
            // Native first selection, §7.6 and R94. The first enabled visible property wins.
            let first = if matches!(layout, Some(DialogLayout::Curves | DialogLayout::Levels)) {
                // Custom editors (Curves, Levels) seat no focus.
                None
            } else {
                ctx.data(|d| d.get_temp::<Id>(Id::unique(INITIAL_EDITOR)))
            };
            match first {
                Some(id) => ctx.memory_mut(|m| m.request_focus(id)),
                None => super::seat_no_focus(&ctx),
            }
        }
        if preview != preview_before {
            changed = merge(changed, Some(true));
        }
        match self.chart.as_mut().and_then(|c| c.action.take()) {
            Some(super::chart::ChartAction::Ok) => action = Some(FooterAction::Ok),
            Some(super::chart::ChartAction::Cancel) => action = Some(FooterAction::Cancel),
            None => {}
        }
        // `dialog.onsubmit`: Enter submits the form (Paint.NET's accept button), also while the
        // title bar's close button holds the focus (Enter's click there is not a close); a
        // focused footer button takes it instead (`action`), local lists and popups keep it.
        let submit = (enter
            && !block_parent
            && self.color_popup.is_none()
            && !list_open
            && self.open_select.is_none()
            && action.is_none()
            && !inputs::enter_taken(&ctx))
            || matches!(action, Some(FooterAction::Ok));
        if !submit && (frame.closed || matches!(action, Some(FooterAction::Cancel))) {
            return self.cancel(cx);
        }
        match action {
            Some(FooterAction::Reset) => {
                self.reset();
                changed = merge(changed, Some(true));
            }
            Some(FooterAction::Auto) => {
                if let Some(h) = &self.histogram {
                    let p = pdn_core::effect::adjustments::auto_level(h);
                    let bytes = |t: [f32; 3]| t.map(|v| v * 255.0);
                    set_triple(&mut self.values, "inputLow", bytes(p.lo));
                    set_triple(&mut self.values, "inputHigh", bytes(p.hi));
                    set_triple(&mut self.values, "gamma", p.gamma);
                    set_triple(&mut self.values, "outputLow", [0.0; 3]);
                    set_triple(&mut self.values, "outputHigh", [255.0; 3]);
                }
                changed = merge(changed, Some(false));
            }
            Some(FooterAction::LoadPreset) => self.choose_preset(),
            _ => {}
        }
        changed = merge(changed, self.poll_preset());
        // A value event this frame retires the chooser instead (R156).
        if changed.is_none() {
            changed = self.poll_chooser();
        }
        changed = merge(changed, self.poll_canvas_pick());
        // A FileType's window has no image to preview.
        if !window {
            if let Some(immediate) = changed {
                self.notify(cx, &ctx, immediate);
            }
            self.rerun_answered(cx, &ctx);
            self.acquire_external(cx.app, &ctx, cx.clipboard.as_deref_mut());
            self.poll_external(cx.app);
            self.check_faces(cx);
            self.run_previews(cx, &ctx);
            self.repaint_file_work(&ctx);
        }
        // Edits and cancellation were routed before this frame's results; acceptance comes last.
        if submit {
            return self.ok(cx, &ctx);
        }
        let notice = self.pending_notice.take();
        self.deliver_notice(notice);
        // Ruling R65: no `backdrop.onclick` cancel (paintdotjs's web dialog). Paint.NET's effect
        // dialogs are modal windows: a press outside one only flashes it (`DialogStack`'s
        // attention), and a press that closes an open list is the list's.
        DialogState::Open
    }
}

/// Combines change kinds: `Some(immediate)`; an immediate one wins.
pub(super) fn merge(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x || y),
        (x, None) => x,
        (None, y) => y,
    }
}

enum FooterAction {
    Ok,
    Cancel,
    Reset,
    Auto,
    LoadPreset,
}

/// `createFooter`: Preview (generic dialogs) or the layout's left button, then a plugin's Load
/// Preset... (`preset`), then OK and Cancel.
fn footer_ui(
    ui: &mut Ui,
    f: Rect,
    layout: Option<DialogLayout>,
    preset: bool,
    preview: Option<&mut bool>,
) -> Option<FooterAction> {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let (fy, fx) = DIALOG_FOOTER_PADDING;
    let content = Rect::from_min_max(
        Pos2::new(f.min.x + fx, f.min.y + fy),
        Pos2::new(f.max.x - fx, f.max.y - fy),
    );
    let min_w = match layout {
        Some(DialogLayout::Curves | DialogLayout::Levels) => EFFECT_WIDE_BUTTON,
        _ => DIALOG_BUTTON_MIN.0,
    };
    let bh = DIALOG_BUTTON_MIN.1;
    let by = content.center().y - bh / 2.0;
    let mut action = None;
    // `leftButtons`: Curves' Reset; Levels' Auto and (R61) Reset.
    let left: Vec<(&str, FooterAction)> = match layout {
        Some(DialogLayout::Curves) => vec![("Reset", FooterAction::Reset)],
        Some(DialogLayout::Levels) => {
            vec![("Auto", FooterAction::Auto), ("Reset", FooterAction::Reset)]
        }
        _ => vec![],
    };
    let mut x = content.min.x;
    let has_left = !left.is_empty();
    for (label, a) in left {
        let w = crate::kit::window::dialog_button_width(ui.painter(), label, min_w);
        let r = Rect::from_min_size(Pos2::new(x, by), Vec2::new(w, bh));
        let response = dialog_button(ui, label, r, label, false);
        if button_clicked(ui, &response) {
            action = Some(a);
        }
        x += w + EFFECT_FOOTER_GAP;
    }
    if !has_left
        && layout.is_none()
        && let Some(preview) = preview
    {
        // `.effect-config-checkbox`: the 16-px box, 7 px, "Preview".
        let (side, gap, min_h) = EFFECT_CHECKBOX;
        let top = content.center().y - min_h / 2.0;
        let boxed = Rect::from_min_size(
            Pos2::new(content.min.x, top + (min_h - side) / 2.0),
            Vec2::splat(side),
        );
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let label = "Preview";
        let tx = boxed.max.x + gap;
        let tw = text::css_width(label, spec.size);
        let label_rect = Rect::from_min_max(
            Pos2::new(content.min.x, top),
            Pos2::new(tx + tw, top + min_h),
        );
        let response = inputs::checkbox_in(
            ui,
            "preview",
            boxed,
            label_rect,
            preview,
            true,
            theme.menu_background,
        );
        ui.ctx().accesskit_node_builder(response.id, |n| {
            n.set_role(egui::accesskit::Role::CheckBox);
            n.set_label("Preview");
        });
        text::draw(
            ui.painter(),
            Pos2::new(tx, top + (min_h - line) / 2.0),
            label,
            spec,
            theme.menu_item_text,
        );
        x = label_rect.max.x + 2.0 * EFFECT_FOOTER_GAP;
    }
    if preset {
        let label = "Load Preset...";
        let w = crate::kit::window::dialog_button_width(ui.painter(), label, min_w);
        let r = Rect::from_min_size(Pos2::new(x, by), Vec2::new(w, bh));
        let response = dialog_button(ui, "load-preset", r, label, false);
        if button_clicked(ui, &response) {
            action = Some(FooterAction::LoadPreset);
        }
    }
    let ok_w = crate::kit::window::dialog_button_width(ui.painter(), "OK", min_w);
    let cancel_w = crate::kit::window::dialog_button_width(ui.painter(), "Cancel", min_w);
    let cancel = Rect::from_min_size(
        Pos2::new(content.max.x - cancel_w, by),
        Vec2::new(cancel_w, bh),
    );
    let ok = Rect::from_min_size(
        Pos2::new(cancel.min.x - EFFECT_FOOTER_GAP - ok_w, by),
        Vec2::new(ok_w, bh),
    );
    let response = dialog_button(ui, "ok", ok, "OK", false);
    if button_clicked(ui, &response) {
        action = Some(FooterAction::Ok);
    }
    let response = dialog_button(ui, "cancel", cancel, "Cancel", false);
    if button_clicked(ui, &response) {
        action = Some(FooterAction::Cancel);
    }
    action
}

fn sync_curves(c: &CurvesState, values: &mut Values) {
    values.insert("points".into(), points_json(c.luminosity.points()));
    for (i, key) in CURVE_KEYS[1..].iter().enumerate() {
        values.insert((*key).into(), points_json(&c.rgb.channels[i]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::px;

    pub(super) fn dialog(
        id: &str,
        values: Option<Values>,
    ) -> (pdn_core::app::AppWorkspace, EffectConfigDialog) {
        let mut app = pdn_core::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(4, 3, 96.0, true);
        app.effect_seed = Some(0);
        let def = pdn_core::effect::find(id).unwrap();
        let values = values.unwrap_or_else(|| pdn_core::effect::defaults(&def));
        let s = EffectSession::begin(&mut app, def).unwrap();
        (app, EffectConfigDialog::new(s, values))
    }

    /// The dialog title: the effect's own, else its menu name (a plugin's untranslated manifest
    /// name, plan BB1).
    #[test]
    fn title_falls_back_to_the_display_name() {
        assert_eq!(dialog("levelsEffect", None).1.title, "Levels Adjustment");
        assert_eq!(dialog("blurEffect", None).1.title, "Gaussian Blur");
        let mut app = pdn_core::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(4, 3, 96.0, true);
        let def = pdn_core::effect::EffectDef {
            id: "bbDream",
            ..pdn_core::effect::find("blurEffect").unwrap()
        };
        let s = EffectSession::begin(&mut app, def).unwrap();
        assert_eq!(EffectConfigDialog::new(s, Values::new()).title, "Dream");
    }

    /// S§4.1: an Extra effect's dialog title carries the plus 4 DIP after its text.
    #[test]
    fn extra_dialog_title_has_plus() {
        let id = "pcVibrance";
        let (mut app, mut d) = dialog(id, None);
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let image = image::RgbaImage::from_pixel(12, 16, image::Rgba([20, 200, 70, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        crate::assets::Assets::set_icon_override(&ctx, "plugin_indicator.png", png.into_inner());
        let mut settings = pdn_core::settings::AppSettings::load(None);
        let mut prefs = crate::prefs::Prefs::default();
        let mut expected = Rect::NOTHING;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let mut cx = DialogCx {
                app: &mut app,
                settings: &mut settings,
                prefs: &mut prefs,
                settings_changed: false,
                shortcuts: None,
                clipboard: None,
                settings_imports: None,
                fonts: None,
                asset_library: None,
                seat_focus: false,
            };
            d.show(ui, &mut cx);
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

    pub(super) fn layer(app: &pdn_core::app::AppWorkspace) -> Vec<u8> {
        let ws = app.active_ws().unwrap();
        ws.document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data()
            .to_vec()
    }

    /// adjustments-and-effect-framework.md §2.9 Levels: R, G and B are all on at load, whatever
    /// the remembered token says; Reset restores the five triples (ruling R61).
    #[test]
    fn levels_open_with_every_channel_and_reset_the_triples() {
        let def = pdn_core::effect::find("levelsEffect").unwrap();
        let token = pdn_core::effect::with(&def, &json!({"red": false, "blue": false}));
        let (_, mut d) = dialog("levelsEffect", Some(token));
        for k in ["red", "green", "blue"] {
            assert_eq!(d.values[k], json!(true), "{k}");
        }
        set_triple(&mut d.values, "inputLow", [10.0, 20.0, 30.0]);
        set_triple(&mut d.values, "gamma", [2.0, 2.0, 2.0]);
        set_triple(&mut d.values, "outputHigh", [200.0, 200.0, 200.0]);
        let generation = d.generation;
        d.reset();
        d.invalidate(42.0, true);
        assert_eq!(
            (d.generation, d.due),
            (generation + 1, Some(42.0)),
            "notify(now, true)"
        );
        let defaults = pdn_core::effect::defaults(&def);
        for k in LEVELS_KEYS {
            assert_eq!(d.values[k], defaults[k], "{k}");
        }
    }

    /// OK reuses the finished current preview instead of rendering again (byte-identical by the
    /// session's seed); a stale one is rendered afresh.
    #[test]
    fn ok_reuses_the_current_preview() {
        let mut settings = pdn_core::settings::AppSettings::load(None);
        let mut prefs = crate::prefs::Prefs::default();
        let marker = ImageData {
            width: 4,
            height: 3,
            data: [1u8, 2, 3, 255].repeat(12),
        };
        for stale in [false, true] {
            let (mut app, mut d) = dialog("invertColorsEffect", None);
            d.due = None;
            d.shown = Some((d.generation + stale as u64, marker.clone()));
            let mut cx = DialogCx {
                app: &mut app,
                settings: &mut settings,
                prefs: &mut prefs,
                settings_changed: false,
                shortcuts: None,
                clipboard: None,
                settings_imports: None,
                fonts: None,
                asset_library: None,
                seat_focus: false,
            };
            d.ok(&mut cx, &egui::Context::default());
            let want = if stale {
                [0u8, 0, 0, 255].repeat(12)
            } else {
                marker.data.clone()
            };
            assert_eq!(layer(&app), want, "stale {stale}");
        }
    }
}

#[cfg(test)]
mod bb28_tests {
    use super::*;
    use crate::platform::fonts::{FontManager, FontStore};
    use pdn_core::effect::EffectDef;
    use pdn_core::effect::bb::{self, ui::IndirectUi};
    use pdn_core::fonts::{self, FaceStyle, FontPolicy, FontRecord};
    use pdn_core::testing::{fresh_doc, layer_bytes};

    pub(super) fn pending_messages() -> usize {
        crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()).len())
    }
    pub(super) fn with_cx<T>(
        app: &mut pdn_core::app::AppWorkspace,
        fonts: Option<&mut FontManager>,
        f: impl FnOnce(&mut DialogCx) -> T,
    ) -> T {
        f(&mut DialogCx {
            app,
            settings: &mut pdn_core::settings::AppSettings::load(None),
            prefs: &mut crate::prefs::Prefs::default(),
            settings_changed: false,
            shortcuts: None,
            clipboard: None,
            settings_imports: None,
            fonts,
            asset_library: None,
            seat_focus: false,
        })
    }
    pub(super) fn frame(
        ctx: &egui::Context,
        app: &mut pdn_core::app::AppWorkspace,
        fonts: Option<&mut FontManager>,
        d: &mut EffectConfigDialog,
    ) -> DialogState {
        let (mut state, mut fonts) = (DialogState::Open, fonts);
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                ..Default::default()
            },
            |ui| state = with_cx(app, fonts.as_deref_mut(), |cx| d.show(ui, cx)),
        );
        out.textures_delta.clear();
        state
    }
    pub(super) fn drain(m: &mut FontManager) {
        for _ in 0..10000 {
            m.poll();
            if !m.busy() {
                return;
            }
            std::thread::yield_now();
        }
        panic!("font job did not finish");
    }
    pub(super) fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, FontPolicy::Bundled, "Selawik");
        ctx
    }
    /// A configurable probe that needs the document face named by its `family` value.
    fn probe() -> EffectDef {
        static UI: IndirectUi = IndirectUi {
            fonts: |v| vec![(bb::text(v, "family").into(), FaceStyle::Regular)],
            ..IndirectUi::DEFAULT
        };
        EffectDef {
            id: "bb28FontProbe",
            controls: vec![Control::Value {
                key: "family",
                default: || json!(""),
            }],
            indirect: Some(&UI),
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        }
    }
    fn probe_dialog(
        family: &str,
        final_only: bool,
    ) -> (pdn_core::app::AppWorkspace, EffectConfigDialog) {
        pending_messages();
        let mut app = fresh_doc(6, 4, Some(4));
        let s = EffectSession::begin(&mut app, probe()).unwrap();
        let values = Values::from_iter([("family".to_string(), json!(family))]);
        let d = if final_only {
            EffectConfigDialog::final_only(s, values)
        } else {
            EffectConfigDialog::new(s, values)
        };
        (app, d)
    }
    pub(super) fn manager(record: Option<(FontRecord, Option<Vec<u8>>)>) -> FontManager {
        let store = record.map_or_else(FontStore::memory, |(r, b)| FontStore::memory_with(r, b));
        let mut m = FontManager::new(store, FontPolicy::Bundled);
        drain(&mut m);
        m
    }
    pub(super) fn user_font(
        id: &str,
        bytes: Option<&[u8]>,
    ) -> (FontRecord, Option<Vec<u8>>, String) {
        let record =
            FontRecord::from_bytes(id.into(), "user.ttf".into(), fonts::CARLITO_REGULAR, 0)
                .unwrap();
        let family = record.family.clone();
        (record, bytes.map(<[u8]>::to_vec), family)
    }
    pub(super) fn wait_closed(
        ctx: &egui::Context,
        app: &mut pdn_core::app::AppWorkspace,
        m: &mut FontManager,
        d: &mut EffectConfigDialog,
    ) -> DialogState {
        for _ in 0..2000 {
            m.poll();
            let state = frame(ctx, app, Some(m), d);
            if state != DialogState::Open {
                return state;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("dialog did not close");
    }

    /// R157: bundled Selawik/Carlito are ready at once: the first frame starts the render and
    /// no face job is queued.
    #[test]
    fn bb28_static_faces_render_without_font_jobs() {
        for id in ["bbComplementaryColors", "bbLevelHorizon", "bbDimensions"] {
            let mut app = fresh_doc(40, 30, Some(4));
            let s = EffectSession::begin(&mut app, pdn_core::effect::find(id).unwrap()).unwrap();
            let values = s.defaults();
            let mut d = EffectConfigDialog::new(s, values);
            let mut m = manager(None);
            frame(&context(), &mut app, Some(&mut m), &mut d);
            assert!(
                d.job_cancel.is_some(),
                "{id}: render started in the first frame"
            );
            assert!(!m.busy(), "{id}: no face job");
        }
    }

    /// R157: a cold required face holds the preview and OK (no substitute commit) until the
    /// app's FontManager registers it; then the delayed OK commits once.
    #[test]
    fn bb28_cold_face_waits_then_commits() {
        let (record, bytes, family) = user_font("bb28-cold", Some(fonts::CARLITO_REGULAR));
        let mut m = manager(Some((record, bytes)));
        let (mut app, mut d) = probe_dialog(&family, false);
        let ctx = context();
        let (before, history) = (
            layer_bytes(&app, None),
            app.active_ws().unwrap().history.undo.len(),
        );
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(
            d.job_cancel.is_none() && d.due.is_some(),
            "the preview waits"
        );
        assert!(m.busy(), "the face is requested from the app's manager");
        assert_eq!(
            with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
            DialogState::Open
        );
        assert!(d.finalizing);
        assert_eq!(
            (
                layer_bytes(&app, None),
                app.active_ws().unwrap().history.undo.len()
            ),
            (before.clone(), history)
        );
        assert!(!app.effect_tokens.contains_key("bb28FontProbe"));
        assert_eq!(
            wait_closed(&ctx, &mut app, &mut m, &mut d),
            DialogState::Closed(DialogResult::Ok)
        );
        assert!(fonts::registered_face(&family, FaceStyle::Regular).is_some());
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        assert_ne!(layer_bytes(&app, None), before);
        fonts::unregister_family(&family);
    }

    /// R195: a bundled script fallback is best-effort. Its failed load neither waits, reports
    /// nor blocks (the text draws missing-glyph outlines); the render starts and OK commits.
    #[test]
    fn failed_script_fallback_never_blocks() {
        let (family, ..) = fonts::SCRIPT_FALLBACKS[10];
        let mut m = manager(None);
        m.fail_family(family, FaceStyle::Regular, "missing script face");
        let (mut app, mut d) = probe_dialog(family, false);
        let ctx = context();
        let history = app.active_ws().unwrap().history.undo.len();
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(d.job_cancel.is_some(), "the render starts without the face");
        assert!(d.font_error.is_none());
        let state = with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx));
        let state = if state == DialogState::Open {
            wait_closed(&ctx, &mut app, &mut m, &mut d)
        } else {
            state
        };
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        assert_eq!(pending_messages(), 0, "no failure report");
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        assert!(fonts::registered_face(family, FaceStyle::Regular).is_none());
    }

    /// R157: a failed required face keeps the source, reports the actual load failure and
    /// cannot commit; the delayed ordinary OK returns editable, a final-only Repeat cancels.
    #[test]
    fn bb28_failed_face_preserves_source_and_cannot_commit() {
        for final_only in [false, true] {
            let (record, bytes, family) = user_font(&format!("bb28-missing-{final_only}"), None);
            let mut m = manager(Some((record, bytes)));
            let (mut app, mut d) = probe_dialog(&family, final_only);
            let ctx = context();
            let (before, history) = (
                layer_bytes(&app, None),
                app.active_ws().unwrap().history.undo.len(),
            );
            if !final_only {
                frame(&ctx, &mut app, Some(&mut m), &mut d);
                assert_eq!(
                    with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
                    DialogState::Open
                );
            }
            let state = (0..50).find_map(|_| {
                m.poll();
                let state = frame(&ctx, &mut app, Some(&mut m), &mut d);
                (state != DialogState::Open
                    || (!final_only && d.font_error.is_some() && !d.finalizing))
                    .then_some(state)
            });
            let want = if final_only {
                DialogState::Closed(DialogResult::Cancel)
            } else {
                DialogState::Open
            };
            assert_eq!(state, Some(want), "final_only {final_only}");
            assert_eq!(pending_messages(), 1, "one failure report");
            assert_eq!(layer_bytes(&app, None), before);
            assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
            assert!(!app.effect_tokens.contains_key("bb28FontProbe"));
            if !final_only {
                assert!(
                    d.font_error
                        .as_deref()
                        .is_some_and(|e| e.contains("missing")),
                    "{:?}",
                    d.font_error
                );
                assert_eq!(
                    with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
                    DialogState::Open
                );
                assert_eq!(pending_messages(), 1, "OK reports it again");
                assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
            }
        }
    }

    /// R157: a relevant face change invalidates the generation and rejects its stale result,
    /// including Preview off + a delayed OK; an unrelated font revision does not.
    #[test]
    fn bb28_face_change_invalidates_only_its_generation() {
        let ctx = context();
        let stale_rejected = |app: &mut pdn_core::app::AppWorkspace,
                              m: &mut FontManager,
                              d: &mut EffectConfigDialog,
                              stale| {
            let result = d.session.as_ref().unwrap().render_with(&d.values, None);
            !with_cx(app, Some(m), |cx| {
                d.accept_result(
                    cx,
                    JobResult {
                        generation: stale,
                        result: Some(result),
                        elapsed: 1.0,
                    },
                )
            })
        };
        let (record, bytes, family) = user_font("bb28-change", Some(fonts::CARLITO_REGULAR));
        let mut m = manager(Some((record, bytes)));
        m.ensure_family(&family, FaceStyle::Regular);
        drain(&mut m);
        let (mut app, mut d) = probe_dialog(&family, false);
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        let g = d.generation;
        assert!(d.job_cancel.is_some());
        m.revision += 1;
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert_eq!(
            d.generation, g,
            "an unrelated revision keeps the generation"
        );
        fonts::register_lazy_face(&family, FaceStyle::Regular, fonts::SELAWIK_REGULAR.to_vec())
            .unwrap();
        m.revision += 1;
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(d.generation > g, "the changed face starts a new generation");
        assert!(stale_rejected(&mut app, &mut m, &mut d, g));
        fonts::unregister_family(&family);
        // Preview off: OK waits for the cold face, then renders; a change during it re-renders.
        let (record, bytes, family) = user_font("bb28-change-ok", Some(fonts::CARLITO_REGULAR));
        let mut m = manager(Some((record, bytes)));
        let (mut app, mut d) = probe_dialog(&family, false);
        d.preview = false;
        assert_eq!(
            with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
            DialogState::Open
        );
        drain(&mut m);
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        let (g, history) = (d.generation, app.active_ws().unwrap().history.undo.len());
        assert!(
            d.finalizing && d.job_cancel.is_some(),
            "the final render started"
        );
        fonts::register_lazy_face(&family, FaceStyle::Regular, fonts::SELAWIK_REGULAR.to_vec())
            .unwrap();
        m.revision += 1;
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(d.generation > g && stale_rejected(&mut app, &mut m, &mut d, g));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
        assert_eq!(
            wait_closed(&ctx, &mut app, &mut m, &mut d),
            DialogState::Closed(DialogResult::Ok)
        );
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        fonts::unregister_family(&family);
    }

    /// R152 Q54: the hook runs on the opening values and before every notification; it never
    /// clears the flag, and each committed pan insertion counts.
    #[test]
    fn bb28_horizon_hook_on_open_notify_and_insertion() {
        let ctx = context();
        let mut app = fresh_doc(40, 40, Some(4));
        let def = pdn_core::effect::find("bbLevelHorizon").unwrap();
        let open = |app: &mut pdn_core::app::AppWorkspace, end: [f64; 2]| {
            let s = EffectSession::begin(app, def.clone()).unwrap();
            let mut values = s.defaults();
            values.insert("start".into(), json!([0.0, -0.9]));
            values.insert("end".into(), json!(end));
            EffectConfigDialog::new(s, values)
        };
        assert!(pdn_core::effect::flag(
            &open(&mut app, [0.1, 0.9]).values,
            "plumbSeen"
        ));
        let mut d = open(&mut app, [0.9, -0.9]);
        assert!(!pdn_core::effect::flag(&d.values, "plumbSeen"));
        d.set_value("end", json!([0.1, 0.9]));
        d.set_value("end", json!([0.9, -0.9]));
        assert!(
            pdn_core::effect::flag(&d.values, "plumbSeen"),
            "an intermediate insertion counts"
        );
        let mut d = open(&mut app, [0.9, -0.9]);
        d.values.insert("end".into(), json!([0.0, 0.9]));
        with_cx(&mut app, None, |cx| d.notify(cx, &ctx, true));
        assert!(pdn_core::effect::flag(&d.values, "plumbSeen"));
        d.reset_control("end");
        d.values.insert("step".into(), json!(1));
        with_cx(&mut app, None, |cx| d.notify(cx, &ctx, true));
        assert!(
            pdn_core::effect::flag(&d.values, "plumbSeen"),
            "resets and tabs keep it"
        );
    }
}

#[cfg(test)]
pub(crate) mod bb29_tests {
    use super::bb28_tests::{context, drain, manager, pending_messages, wait_closed};
    use super::*;
    use crate::kit::inputs::Utf16Text;
    use crate::platform::fonts::FontManager;
    use egui::TextBuffer as _;
    use egui::text::CharIndex;
    use pdn_core::effect::EffectDef;
    use pdn_core::effect::bb::{self, ui::IndirectUi};
    use pdn_core::fonts::{self, FaceStyle};
    use pdn_core::testing::{fresh_doc, layer_bytes};

    const ANTON: &[u8] = include_bytes!("../../../../../assets/fonts/anton/Anton-Regular.ttf");
    /// Tests that register "Impact" globally, or need it absent, hold this.
    pub(crate) static IMPACT: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn frame_with(
        ctx: &egui::Context,
        app: &mut pdn_core::app::AppWorkspace,
        fonts: Option<&mut FontManager>,
        d: &mut EffectConfigDialog,
        events: Vec<egui::Event>,
        seat: bool,
    ) -> DialogState {
        let (mut state, mut fonts) = (DialogState::Open, fonts);
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                events,
                ..Default::default()
            },
            |ui| {
                state = super::bb28_tests::with_cx(app, fonts.as_deref_mut(), |cx| {
                    cx.seat_focus = seat;
                    d.show(ui, cx)
                })
            },
        );
        out.textures_delta.clear();
        state
    }
    /// A key press and release with `modifiers` held, as an integration reports them.
    fn key(key: Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        let mut events = vec![egui::Event::ModifiersChanged(modifiers)];
        events.extend([true, false].map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        }));
        events
    }
    /// A Meme dialog whose `top` (its first editor, focused by the first frame) holds `top`,
    /// drawn in the registered Anton so a render never waits.
    fn meme(
        top: &str,
    ) -> (
        pdn_core::app::AppWorkspace,
        EffectConfigDialog,
        egui::Context,
    ) {
        fonts::register_lazy_face("Anton", FaceStyle::Regular, ANTON.to_vec()).unwrap();
        pending_messages();
        let mut app = fresh_doc(40, 30, Some(4));
        let s =
            EffectSession::begin(&mut app, pdn_core::effect::find("bbMemeMaker").unwrap()).unwrap();
        let mut values = s.defaults();
        values.insert("top".into(), json!(top));
        values.insert("face".into(), json!("Anton"));
        let mut d = EffectConfigDialog::new(s, values);
        let ctx = context();
        frame_with(&ctx, &mut app, None, &mut d, vec![], true);
        (app, d, ctx)
    }
    /// A plugin dialog taller than its window scrolls with the wheel to its last row, no further, in the plugin
    /// layout and the generic one alike (a plug-in window with canvases and many settings is generic).
    #[test]
    fn tall_plugin_rows_scroll() {
        static UI: IndirectUi = IndirectUi::DEFAULT;
        for indirect in [Some(&UI), None] {
            tall_rows_scroll(indirect);
        }
    }

    fn tall_rows_scroll(indirect: Option<&'static IndirectUi>) {
        const KEYS: [&str; 16] = [
            "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11", "r12", "r13",
            "r14", "r15",
        ];
        let def = EffectDef {
            id: "tallProbe",
            controls: KEYS
                .map(|k| pdn_core::effect::range(k, k, 0.0, 100.0, 50.0))
                .to_vec(),
            indirect,
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        };
        let mut app = fresh_doc(40, 30, Some(4));
        let s = EffectSession::begin(&mut app, def).unwrap();
        let values = s.defaults();
        let mut d = EffectConfigDialog::new(s, values);
        let ctx = context();
        frame_with(&ctx, &mut app, None, &mut d, vec![], false);
        let wheel = |lines: f32| {
            vec![
                egui::Event::PointerMoved(Pos2::new(640.0, 400.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: Vec2::new(0.0, -lines),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                },
            ]
        };
        frame_with(&ctx, &mut app, None, &mut d, wheel(1.0), false);
        let some = d.rows_scroll.0;
        assert!(some > 0.0);
        frame_with(&ctx, &mut app, None, &mut d, wheel(100.0), false);
        let end = d.rows_scroll.0;
        assert!(end > some);
        frame_with(&ctx, &mut app, None, &mut d, wheel(100.0), false);
        assert_eq!(d.rows_scroll.0, end);
    }

    fn top(d: &EffectConfigDialog) -> &str {
        bb::text(&d.values, "top")
    }
    /// OK, then any wait for its render, to the dialog's close.
    fn ok_closed(
        ctx: &egui::Context,
        app: &mut pdn_core::app::AppWorkspace,
        m: &mut FontManager,
        d: &mut EffectConfigDialog,
    ) -> DialogState {
        match super::bb28_tests::with_cx(app, Some(m), |cx| d.ok(cx, ctx)) {
            DialogState::Open => wait_closed(ctx, app, m, d),
            state => state,
        }
    }
    /// Removes a test's global face registration even when the test fails.
    struct Unregister(&'static str);
    impl Drop for Unregister {
        fn drop(&mut self) {
            fonts::unregister_family(self.0);
        }
    }

    /// R118: capacity is UTF-16 units: supplementary scalars count two, CRLF two; insertion
    /// takes the longest whole-scalar prefix and never touches text outside the selection.
    #[test]
    fn bb29_utf16_capacity() {
        let insert = |text: &str, max: usize, at: usize, add: &str| {
            let mut s = text.to_string();
            let n = Utf16Text {
                text: &mut s,
                max_units: max,
            }
            .insert_text(add, CharIndex(at));
            (s, n)
        };
        let a = "a".repeat(32765);
        assert_eq!(insert(&a, 32767, 32765, "😀"), (a.clone() + "😀", 1));
        let b = "a".repeat(32766);
        assert_eq!(insert(&b, 32767, 0, "😀x"), (b.clone(), 0));
        let pairs = "😀".repeat(16383);
        assert_eq!(insert(&pairs, 32767, 16383, "ab"), (pairs.clone() + "a", 1));
        // A middle insertion keeps the suffix; a selection deleted first frees its units.
        assert_eq!(insert("ab", 3, 1, "XYZ"), ("aXb".into(), 1));
        let mut s = "abcd".to_string();
        let mut t = Utf16Text {
            text: &mut s,
            max_units: 4,
        };
        t.delete_char_range(CharIndex(1)..CharIndex(3));
        assert_eq!(t.insert_text("😀😀", CharIndex(1)), 1);
        assert_eq!(s, "a😀d");
        // Paste of CRLF with one unit free keeps the whole-scalar prefix, the CR alone.
        assert_eq!(insert("abc", 4, 3, "\r\nz"), ("abc\r".into(), 1));
        assert_eq!(insert("", 0, 0, "x"), (String::new(), 0));
        // A scalar that does not fit ends the prefix: nothing after it is inserted.
        assert_eq!(
            insert(&"x".repeat(254), 255, 0, "😀y"),
            ("x".repeat(254), 0)
        );
        assert_eq!(
            insert(&"x".repeat(253), 255, 0, "y😀"),
            ("y".to_string() + &"x".repeat(253), 1)
        );
        // Undo snapshots and over-long values are restored and kept exactly.
        let mut s = "short".to_string();
        let long = "\r\n".repeat(20000);
        Utf16Text {
            text: &mut s,
            max_units: 5,
        }
        .replace_with(&long);
        assert_eq!(s, long);
    }

    /// R118 through the real editor: typing, paste and an IME commit stop at the UTF-16 limit;
    /// an over-long value and exact CR/LF/CRLF bytes survive untouched frames and OK.
    #[test]
    fn bb29_editor_insertions_respect_the_limit() {
        let a = "a".repeat(32765);
        let (mut app, mut d, ctx) = meme(&a);
        let mut ev = |d: &mut EffectConfigDialog, e: Vec<egui::Event>| {
            frame_with(&ctx, &mut app, None, d, e, false)
        };
        ev(&mut d, vec![egui::Event::Text("b😀".into())]);
        assert_eq!(
            top(&d),
            a.clone() + "b",
            "the pair does not fit the last unit"
        );
        ev(&mut d, vec![egui::Event::Paste("😀c".into())]);
        assert_eq!(
            top(&d),
            a.clone() + "b",
            "a paste keeps only its whole-scalar prefix"
        );
        ev(
            &mut d,
            vec![
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: "e".into(),
                    active_range_chars: None,
                }),
                egui::Event::Ime(egui::ImeEvent::Commit("か".into())),
            ],
        );
        assert_eq!(top(&d), a.clone() + "bか");
        ev(&mut d, vec![egui::Event::Text("d".into())]);
        assert_eq!(top(&d), a.clone() + "bか", "full");
        assert_eq!(top(&d).encode_utf16().count(), 32767);

        let raw = "a\rb\nc\r\n\r\nd ".to_string() + &"x".repeat(33000);
        let (mut app, mut d, ctx) = meme(&raw);
        for _ in 0..3 {
            frame_with(&ctx, &mut app, None, &mut d, vec![], false);
        }
        assert_eq!(top(&d), raw, "no frame truncates");
        let mut m = manager(None);
        assert_eq!(
            ok_closed(&ctx, &mut app, &mut m, &mut d),
            DialogState::Closed(DialogResult::Ok)
        );
        assert_eq!(app.effect_tokens["bbMemeMaker"]["top"], json!(raw));
    }

    /// R118: multiline Enter (plain, Shift, Ctrl) belongs to the editor and never submits;
    /// Tab leaves the box without inserting; Escape cancels.
    #[test]
    fn bb29_multiline_keys_stay_in_the_editor() {
        let m = egui::Modifiers::NONE;
        for modifiers in [m, egui::Modifiers::SHIFT, egui::Modifiers::CTRL] {
            let (mut app, mut d, ctx) = meme("x");
            let history = app.active_ws().unwrap().history.undo.len();
            let state = frame_with(
                &ctx,
                &mut app,
                None,
                &mut d,
                key(Key::Enter, modifiers),
                false,
            );
            assert_eq!(state, DialogState::Open, "{modifiers:?}");
            assert!(!d.finalizing);
            assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
            assert!(top(&d).starts_with('x'));
        }
        let (mut app, mut d, ctx) = meme("x");
        let focused = || ctx.memory(|mem| mem.focused());
        let first = focused();
        frame_with(&ctx, &mut app, None, &mut d, key(Key::Tab, m), false);
        assert_ne!(focused(), first, "Tab moves on");
        for _ in 0..10 {
            if format!("{:?}", focused()).contains("(\"text\", \"bottom\")") {
                break;
            }
            frame_with(&ctx, &mut app, None, &mut d, key(Key::Tab, m), false);
        }
        let bottom = focused();
        assert!(
            format!("{bottom:?}").contains("(\"text\", \"bottom\")"),
            "{bottom:?}"
        );
        // Previous-focus resolves on the frame after the press, as in a real event stream.
        let mut press = key(Key::Tab, egui::Modifiers::SHIFT);
        let release = press.pop().unwrap();
        frame_with(&ctx, &mut app, None, &mut d, press, false);
        frame_with(&ctx, &mut app, None, &mut d, vec![release], false);
        assert_ne!(focused(), bottom, "Shift+Tab moves back");
        assert_eq!(top(&d), "x");
        assert_eq!(
            bb::text(&d.values, "bottom"),
            "Enter bottom text here",
            "no tab inserted"
        );
        assert_eq!(
            frame_with(&ctx, &mut app, None, &mut d, key(Key::Escape, m), false),
            DialogState::Closed(DialogResult::Cancel)
        );
    }

    /// R178: multiline Enter, Shift+Enter and Ctrl+Enter each insert exactly one LF (one
    /// UTF-16 unit) and never submit; with no unit free nothing is inserted, with one an LF;
    /// existing CRLF counts two units and survives exactly.
    #[test]
    fn bb29_r178_enter_inserts_one_lf() {
        let m = egui::Modifiers::NONE;
        for modifiers in [m, egui::Modifiers::SHIFT, egui::Modifiers::CTRL] {
            let (mut app, mut d, ctx) = meme("x");
            let history = app.active_ws().unwrap().history.undo.len();
            let state = frame_with(
                &ctx,
                &mut app,
                None,
                &mut d,
                key(Key::Enter, modifiers),
                false,
            );
            assert_eq!(state, DialogState::Open, "{modifiers:?}");
            assert!(!d.finalizing, "{modifiers:?} submitted");
            assert_eq!(top(&d), "x\n", "{modifiers:?}");
            assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
        }
        for (start, want) in [
            ("a".repeat(32767), "a".repeat(32767)),
            ("a".repeat(32766), "a".repeat(32766) + "\n"),
            (
                "a\r\n".to_string() + &"x".repeat(32764),
                "a\r\n".to_string() + &"x".repeat(32764),
            ),
            (
                "a\r\n".to_string() + &"x".repeat(32763),
                "a\r\n".to_string() + &"x".repeat(32763) + "\n",
            ),
        ] {
            let (mut app, mut d, ctx) = meme(&start);
            frame_with(&ctx, &mut app, None, &mut d, key(Key::Enter, m), false);
            assert_eq!(top(&d), want, "{} units", start.encode_utf16().count());
        }
    }

    /// R118: a single-line box's ordinary Enter accepts the dialog once; Ctrl+Enter does not.
    #[test]
    fn bb29_single_line_enter_accepts_once() {
        static LINE_UI: IndirectUi = IndirectUi::DEFAULT;
        let probe = EffectDef {
            id: "bb29LineProbe",
            indirect: Some(&LINE_UI),
            controls: vec![Control::Text {
                key: "line",
                label: "Line",
                default: "hello world",
                max_len: 5,
                multiline: false,
            }],
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        };
        for (modifiers, accepts) in [
            (egui::Modifiers::NONE, true),
            (egui::Modifiers::CTRL, false),
        ] {
            let mut app = fresh_doc(6, 4, Some(4));
            let s = EffectSession::begin(&mut app, probe.clone()).unwrap();
            let values = s.defaults();
            let mut d = EffectConfigDialog::new(s, values);
            let ctx = context();
            frame_with(&ctx, &mut app, None, &mut d, vec![], true);
            let history = app.active_ws().unwrap().history.undo.len();
            let mut state = frame_with(
                &ctx,
                &mut app,
                None,
                &mut d,
                key(Key::Enter, modifiers),
                false,
            );
            for _ in 0..100 {
                if state != DialogState::Open || !d.finalizing {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
                state = frame_with(&ctx, &mut app, None, &mut d, vec![], false);
            }
            if accepts {
                assert_eq!(state, DialogState::Closed(DialogResult::Ok));
                assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
                assert_eq!(
                    app.effect_tokens["bb29LineProbe"]["line"],
                    json!("hello world")
                );
            } else {
                assert_eq!(state, DialogState::Open);
                assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
            }
        }
    }

    /// A plug-in's text (no IndirectUI, so the generic rows) is a box to type in, focused first.
    #[test]
    fn a_generic_text_row_takes_typing() {
        let probe = EffectDef {
            id: "genericTextProbe",
            indirect: None,
            dialog: None,
            controls: vec![Control::Text {
                key: "w:0.",
                label: "Text",
                default: "",
                max_len: 32767,
                multiline: false,
            }],
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        };
        let mut app = fresh_doc(6, 4, Some(4));
        let s = EffectSession::begin(&mut app, probe).unwrap();
        let values = s.defaults();
        let mut d = EffectConfigDialog::new(s, values);
        let ctx = context();
        frame_with(&ctx, &mut app, None, &mut d, vec![], true);
        frame_with(
            &ctx,
            &mut app,
            None,
            &mut d,
            vec![egui::Event::Text("Hi".into())],
            false,
        );
        assert_eq!(d.values["w:0."], json!("Hi"));
    }

    /// A FileType's window shown while a file opens or saves: no image needed, its settings at their defaults,
    /// typing edits them, Enter (OK) hands them over once, and Escape (Cancel) hands over nothing.
    #[test]
    fn a_filetype_window_hands_back_its_values() {
        let controls = || {
            vec![
                Control::Text {
                    key: "w:0.",
                    label: "Name",
                    default: "",
                    max_len: 32767,
                    multiline: false,
                },
                Control::Checkbox {
                    key: "w:1.",
                    label: "Merged",
                    default: true,
                },
            ]
        };
        use std::{cell::RefCell, rc::Rc};
        let got: Rc<RefCell<Vec<Option<Values>>>> = Rc::default();
        let mut app = fresh_doc(6, 4, Some(4));
        let ctx = context();
        let into = got.clone();
        let mut d =
            EffectConfigDialog::for_window("Save Options".into(), controls(), move |v, _| {
                into.borrow_mut().push(v)
            });
        frame_with(&ctx, &mut app, None, &mut d, vec![], true);
        frame_with(
            &ctx,
            &mut app,
            None,
            &mut d,
            vec![egui::Event::Text("ab".into())],
            false,
        );
        let state = frame_with(
            &ctx,
            &mut app,
            None,
            &mut d,
            key(Key::Enter, egui::Modifiers::NONE),
            false,
        );
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        let values = got.borrow()[0].clone().expect("OK hands the values over");
        assert_eq!(
            (&values["w:0."], &values["w:1."]),
            (&json!("ab"), &json!(true))
        );

        let into = got.clone();
        let mut d =
            EffectConfigDialog::for_window("Save Options".into(), controls(), move |v, _| {
                into.borrow_mut().push(v)
            });
        frame_with(&ctx, &mut app, None, &mut d, vec![], true);
        let state = frame_with(
            &ctx,
            &mut app,
            None,
            &mut d,
            key(Key::Escape, egui::Modifiers::NONE),
            false,
        );
        assert_eq!(state, DialogState::Closed(DialogResult::Cancel));
        assert_eq!(got.borrow().len(), 2);
        assert!(got.borrow()[1].is_none(), "Cancel hands nothing over");
    }

    /// R157: with no system Impact (Bundled), the session waits for the Impact attempt to
    /// finish, then moves to the bundled Anton once and commits with it; a usable regular
    /// Impact is kept (R111 synthesizes its bold). The face never reaches the token.
    #[test]
    fn bb29_meme_face_resolution() {
        let _impact = IMPACT.lock().unwrap_or_else(|e| e.into_inner());
        let open = |app: &mut pdn_core::app::AppWorkspace| {
            pending_messages();
            let s =
                EffectSession::begin(app, pdn_core::effect::find("bbMemeMaker").unwrap()).unwrap();
            let values = s.defaults();
            EffectConfigDialog::new(s, values)
        };
        let ctx = context();
        let mut m = manager(None);
        let mut app = fresh_doc(40, 30, Some(4));
        let mut d = open(&mut app);
        super::bb28_tests::frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(
            d.job_cancel.is_none(),
            "no render before the Impact attempt ends"
        );
        assert_eq!(bb::text(&d.values, "face"), "Impact");
        let history = app.active_ws().unwrap().history.undo.len();
        assert_eq!(
            super::bb28_tests::with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
            DialogState::Open,
            "a pending Impact job holds OK"
        );
        assert_eq!(
            wait_closed(&ctx, &mut app, &mut m, &mut d),
            DialogState::Closed(DialogResult::Ok)
        );
        assert_eq!(bb::text(&d.values, "face"), "Anton");
        assert!(fonts::registered_face("Impact", FaceStyle::Bold).is_none());
        assert!(fonts::registered_face("Anton", FaceStyle::Regular).is_some());
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        let token = &app.effect_tokens["bbMemeMaker"];
        assert!(!token.contains_key("face") && !token.contains_key("tab"));
        assert_eq!(pending_messages(), 0, "the fallback is not an error");

        // A regular system Impact (as the native System policy's Bold → Regular choice
        // registers it) is kept and drawn bold by R111 synthesis.
        fonts::register_lazy_face(
            "Impact",
            FaceStyle::Regular,
            fonts::CARLITO_REGULAR.to_vec(),
        )
        .unwrap();
        let _unregister = Unregister("Impact");
        let mut m = manager(None);
        let mut app = fresh_doc(40, 30, Some(4));
        let mut d = open(&mut app);
        let before = layer_bytes(&app, None);
        assert_eq!(
            ok_closed(&ctx, &mut app, &mut m, &mut d),
            DialogState::Closed(DialogResult::Ok)
        );
        assert_eq!(bb::text(&d.values, "face"), "Impact");
        let impact = fonts::registered_face("Impact", FaceStyle::Bold).unwrap();
        assert!(
            impact[..] == fonts::CARLITO_REGULAR[..],
            "regular Impact, drawn bold"
        );
        let bold = pdn_core::effect::bb::text::glyph_run("I", "Impact", 48.0, true, [0.0; 2]);
        assert_eq!(
            bold.layout.lines[0].items.len(),
            2,
            "expansion + original outline"
        );
        assert_ne!(layer_bytes(&app, None), before);
    }

    /// R157: when the fallback face fails too, the source stays and nothing commits: the
    /// delayed ordinary OK returns editable with one report, final-only Repeat cancels.
    #[test]
    fn bb29_failed_fallback_face_cannot_commit() {
        static UI: IndirectUi = IndirectUi {
            fonts: |v| vec![(bb::text(v, "face").into(), FaceStyle::Regular)],
            font_fallback: |v| {
                let first = bb::text(v, "face") == "bb29-first-missing";
                if first {
                    v.insert("face".into(), json!("bb29-second-missing"));
                }
                first
            },
            ..IndirectUi::DEFAULT
        };
        let probe = EffectDef {
            id: "bb29FallbackProbe",
            controls: vec![Control::Value {
                key: "face",
                default: || json!("bb29-first-missing"),
            }],
            indirect: Some(&UI),
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        };
        for final_only in [false, true] {
            pending_messages();
            let mut app = fresh_doc(6, 4, Some(4));
            let s = EffectSession::begin(&mut app, probe.clone()).unwrap();
            let values = s.defaults();
            let mut d = if final_only {
                EffectConfigDialog::final_only(s, values)
            } else {
                EffectConfigDialog::new(s, values)
            };
            let (ctx, mut m) = (context(), manager(None));
            let before = (
                layer_bytes(&app, None),
                app.active_ws().unwrap().history.undo.len(),
            );
            if !final_only {
                assert_eq!(
                    super::bb28_tests::with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
                    DialogState::Open
                );
            }
            let state = (0..200).find_map(|_| {
                drain(&mut m);
                let state = super::bb28_tests::frame(&ctx, &mut app, Some(&mut m), &mut d);
                (state != DialogState::Open || (d.font_error.is_some() && !d.finalizing))
                    .then_some(state)
            });
            let want = if final_only {
                DialogState::Closed(DialogResult::Cancel)
            } else {
                DialogState::Open
            };
            assert_eq!(state, Some(want), "final_only {final_only}");
            assert_eq!(bb::text(&d.values, "face"), "bb29-second-missing");
            assert_eq!(pending_messages(), 1, "one failure report");
            assert_eq!(
                (
                    layer_bytes(&app, None),
                    app.active_ws().unwrap().history.undo.len()
                ),
                before
            );
            assert!(!app.effect_tokens.contains_key("bb29FallbackProbe"));
        }
    }
}
#[cfg(test)]
mod bb30_tests {
    use super::bb28_tests::{context, drain, frame, manager, user_font};
    use super::layouts::font_select_entries;
    use super::*;
    use pdn_core::effect::EffectDef;
    use pdn_core::effect::bb::{self, ui::IndirectUi};
    use pdn_core::effect::rng::CivilDate;
    use pdn_core::fonts::FaceStyle;
    use pdn_core::testing::fresh_doc;

    fn calendar(
        app: &mut pdn_core::app::AppWorkspace,
        year: i64,
        final_only: bool,
    ) -> EffectConfigDialog {
        app.date_override = Some(CivilDate::new(2026, 2, 10));
        let s = EffectSession::begin(app, pdn_core::effect::find("bbCalendar").unwrap()).unwrap();
        let mut values = s.defaults();
        values.insert("year".into(), json!(year));
        if final_only {
            EffectConfigDialog::final_only(s, values)
        } else {
            EffectConfigDialog::new(s, values)
        }
    }

    /// R154 Q60: the opened draft clamps a remembered year to the session's slider limits once;
    /// the final-only Repeat owner keeps any valid year.
    #[test]
    fn bb30_draft_year_clamps_once_repeat_keeps_it() {
        let mut app = fresh_doc(40, 30, Some(4));
        for (year, want) in [(1800, 1906), (2500, 2146), (2000, 2000)] {
            let year_of = |d: EffectConfigDialog| d.values["year"].as_f64();
            assert_eq!(year_of(calendar(&mut app, year, false)), Some(want as f64));
            assert_eq!(year_of(calendar(&mut app, year, true)), Some(year as f64));
        }
    }

    /// A probe that needs the regular and bold faces of the family its `family` value names,
    /// over a 6 × 4 document.
    fn bold_probe(family: &str) -> (pdn_core::app::AppWorkspace, EffectConfigDialog) {
        static UI: IndirectUi = IndirectUi {
            fonts: |v| {
                let f = bb::text(v, "family").to_string();
                vec![(f.clone(), FaceStyle::Regular), (f, FaceStyle::Bold)]
            },
            ..IndirectUi::DEFAULT
        };
        let probe = EffectDef {
            id: "bb30BoldProbe",
            controls: vec![Control::Value {
                key: "family",
                default: || json!(""),
            }],
            indirect: Some(&UI),
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        };
        let mut app = fresh_doc(6, 4, Some(4));
        let s = EffectSession::begin(&mut app, probe).unwrap();
        let d = EffectConfigDialog::new(s, Values::from_iter([("family".into(), json!(family))]));
        (app, d)
    }

    /// R154 Q61 / R157: a 400 main face's bold is requested and awaited even while its regular
    /// face (which the registry would fall back to) is registered.
    #[test]
    fn bb30_bold_face_is_loaded_before_rendering() {
        let (record, bytes, family) =
            user_font("bb30-bold", Some(pdn_core::fonts::CARLITO_REGULAR));
        let mut m = manager(Some((record, bytes)));
        m.ensure_family(&family, FaceStyle::Regular);
        drain(&mut m);
        assert!(
            pdn_core::fonts::registered_face(&family, FaceStyle::Bold).is_some(),
            "falls back"
        );
        let (mut app, mut d) = bold_probe(&family);
        let ctx = context();
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(
            d.job_cancel.is_none() && m.loading(&family, FaceStyle::Bold),
            "waits for bold"
        );
        drain(&mut m);
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(d.job_cancel.is_some(), "renders once it loaded");
        pdn_core::fonts::unregister_family(&family);
    }

    /// BB30 review Minor 1: once the bold load has failed while the regular face stands in, a
    /// due retry is not requested (and awaited) again on every readiness check; the stand-in
    /// renders.
    #[test]
    fn bb30_failed_bold_face_is_not_requested_again() {
        let (record, bytes, family) =
            user_font("bb30-no-bold", Some(pdn_core::fonts::CARLITO_REGULAR));
        let mut m = manager(Some((record, bytes)));
        m.ensure_family(&family, FaceStyle::Regular);
        drain(&mut m);
        m.fail_family_retry_due(&family, FaceStyle::Bold, "no bold face");
        let (mut app, mut d) = bold_probe(&family);
        let ctx = context();
        frame(&ctx, &mut app, Some(&mut m), &mut d);
        assert!(!m.loading(&family, FaceStyle::Bold), "not requested again");
        assert!(d.job_cancel.is_some(), "the regular face stands in");
        pdn_core::fonts::unregister_family(&family);
    }

    /// render-calendar.md controls 3 and 14 (R154 Q61): Task 53's families, the interface faces
    /// added, generics left out, in the deterministic order; a value no longer listed is shown.
    #[test]
    fn bb30_font_rows_list_and_keep_the_value() {
        let (labels, index) = font_select_entries(
            Some(vec![
                ("Lato".into(), "Lato".into()),
                ("serif".into(), "System Serif".into()),
            ]),
            "Gone Font",
        );
        let shown: Vec<&str> = labels.iter().map(|e| e.1.as_str()).collect();
        assert_eq!(shown, ["Gone Font", "Carlito", "Lato", "Selawik"]);
        assert_eq!(index, 0);
        let (labels, index) = font_select_entries(None, "selawik");
        assert_eq!(labels[index].0, "Selawik");
        assert!(
            labels.iter().any(|e| e.0 == "Noto Sans") && !labels.iter().any(|e| e.0 == "serif")
        );
    }
}

#[cfg(test)]
mod permission_tests {
    use super::bb28_tests::{context, frame, pending_messages};
    use super::*;
    use pdn_core::effect::permission::{self, Grant};
    use pdn_core::effect::{EffectDef, EffectEnv, EffectKind};
    use pdn_core::testing::{fresh_doc, layer_bytes};

    fn request() -> Request {
        Request {
            plugin: "PermProbe.dll".into(),
            grant: Grant {
                category: "network".into(),
                target: "example.invalid".into(),
            },
            detail: "GET http://example.invalid/".into(),
            ..Default::default()
        }
    }

    /// The host's gate, in-process: unanswered, the run stops at the request; allowed, the plug-in inverts the
    /// layer; denied, it carries on without the network and leaves it.
    fn gate(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
        let r = request();
        if permission::with(|p| p.allows(&r)) {
            let mut out = src.clone();
            for px in out.data.as_chunks_mut::<4>().0 {
                px[..3].iter_mut().for_each(|c| *c = 255 - *c);
            }
            return out;
        }
        if !permission::with(|p| p.answered(&r))
            && let Some(notice) = env.notice
        {
            *notice.lock().unwrap() = Some(
                "PermissionRequiredException: pdn-permission:{\"plugin\":\"PermProbe.dll\",\"category\":\"network\",\
                 \"target\":\"example.invalid\",\"detail\":\"GET http://example.invalid/\"}"
                    .into(),
            );
        }
        src.clone()
    }

    fn probe() -> EffectDef {
        EffectDef {
            id: "permProbe",
            kind: EffectKind::Cpu {
                render: gate,
                clip_to_selection: true,
            },
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        }
    }

    /// Runs a settings-free plug-in effect (its final only) until it asks, answers `choice`, then until it closes;
    /// the layer before and after.
    fn run(choice: &str) -> (Vec<u8>, Vec<u8>) {
        let ctx = context();
        let mut app = fresh_doc(6, 4, Some(4));
        let before = layer_bytes(&app, None);
        let s = EffectSession::begin(&mut app, probe()).unwrap();
        let mut d = EffectConfigDialog::final_only(s, Values::new());
        pending_messages();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            assert_eq!(frame(&ctx, &mut app, None, &mut d), DialogState::Open);
            if d.asking.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(d.asking.is_some(), "no prompt");
        assert_eq!(pending_messages(), 1, "the prompt opened once");
        crate::platform::plugins::answer_permission(
            &request(),
            choice,
            &mut crate::prefs::Prefs::default(),
        );
        d.asking.as_ref().unwrap().answered.set(true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            match frame(&ctx, &mut app, None, &mut d) {
                DialogState::Open => std::thread::sleep(std::time::Duration::from_millis(1)),
                _ => break,
            }
        }
        assert_eq!(pending_messages(), 0, "no second prompt");
        drop(d);
        (before, layer_bytes(&app, None))
    }

    /// The prompt's answer runs the plug-in again: allowed once it renders and the grant ends with the run;
    /// denied it carries on unchanged and is not asked again until the next start.
    #[test]
    fn an_outside_request_is_asked_then_run_with_the_answer() {
        let (before, after) = run("once");
        assert_ne!(before, after, "allowed: the plug-in rendered");
        assert!(
            !permission::with(|p| p.answered(&request())),
            "Allow once ended with the run"
        );

        let (before, after) = run("deny");
        assert_eq!(before, after, "denied: unchanged");
        assert!(permission::with(
            |p| p.answered(&request()) && !p.allows(&request())
        ));
        permission::with(|p| p.allow_always(&request()));
        permission::with(|p| p.revoke("PermProbe.dll", &request().grant));
    }

    static FETCH_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// The host's network gate for a request it leaves to paint.rs: the first run stops at the request (method,
    /// URL, key); the next, after paint.rs handed over the response, completes.
    fn fetch_gate(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
        use std::sync::atomic::Ordering;
        if FETCH_RUNS.fetch_add(1, Ordering::SeqCst) > 0 {
            let mut out = src.clone();
            out.data.iter_mut().for_each(|c| *c = 255 - *c);
            return out;
        }
        if let Some(notice) = env.notice {
            *notice.lock().unwrap() = Some(
                "pdn-permission:{\"plugin\":\"FetchProbe.dll\",\"category\":\"network\",\"target\":\"127.0.0.1\",\
                 \"detail\":\"http://127.0.0.1:9/\",\"key\":\"k-fetch\",\"request\":{\"key\":\"k-fetch\",\
                 \"method\":\"GET\",\"url\":\"http://127.0.0.1:9/\",\"headers\":{}}}"
                    .into(),
            );
        }
        src.clone()
    }

    /// A network request an Always allow covers is made without a prompt (here refused: nothing listens on port
    /// 9), its response handed to the host, and the plug-in run again to the end.
    #[test]
    fn an_allowed_network_request_is_made_then_the_plug_in_runs_again() {
        let fetch = Request {
            plugin: "FetchProbe.dll".into(),
            grant: Grant {
                category: "network".into(),
                target: "127.0.0.1".into(),
            },
            ..Default::default()
        };
        permission::with(|p| p.allow_always(&fetch));
        let ctx = context();
        let mut app = fresh_doc(6, 4, Some(4));
        let before = layer_bytes(&app, None);
        let def = EffectDef {
            id: "fetchProbe",
            kind: EffectKind::Cpu {
                render: fetch_gate,
                clip_to_selection: true,
            },
            ..probe()
        };
        let s = EffectSession::begin(&mut app, def).unwrap();
        let mut d = EffectConfigDialog::final_only(s, Values::new());
        pending_messages();
        let mut state = DialogState::Open;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            state = frame(&ctx, &mut app, None, &mut d);
            if state != DialogState::Open {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_ne!(state, DialogState::Open, "the run did not finish");
        assert_eq!(pending_messages(), 0, "no prompt, no message");
        assert_eq!(d.fetched, ["k-fetch"]);
        assert_ne!(
            before,
            layer_bytes(&app, None),
            "the plug-in ran to the end"
        );
        permission::with(|p| p.revoke("FetchProbe.dll", &fetch.grant));
    }

    /// One prompt for every request: the plug-in, the verb and target, the detail beneath, and Deny as what Escape
    /// (and Enter, on the focused close button) picks.
    #[test]
    fn the_prompt_reads_the_request() {
        let spec = crate::platform::plugins::permission_prompt(&request());
        assert_eq!(
            spec.message.as_deref(),
            Some("PermProbe.dll wants to connect to example.invalid\nGET http://example.invalid/")
        );
        let titles: Vec<&str> = spec.choices.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Allow once", "Always allow", "Deny"]);
        assert_eq!(spec.cancel_value, "deny");
    }
}
