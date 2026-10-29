//! Ported from pdn-content-aware-fill src/ContentAwareFillConfigDialog.cs and its Designer file @ b54eec8b (GPL-2.0-or-later; Copyright (C) 2018, 2020–2025 Nicholas Hayes). Changes: the Effect Config kit's rows, buttons and footer instead of WinForms (headings over a separator, full-width drop-downs, the slider's ticks drawn under its track); the status strip sits left of OK / Cancel; no Donate! link and no version in the title; a render is restarted at once (the old job is dropped and stops at its next check) instead of after the worker drains; the whole-image message closes the dialog before it shows.
//!
//! Content Aware Fill's dialog (Effects ▸ Selection, Edit ▸ Fill Selection (Content-Aware)):
//! Sample area size, Sample from, Fill direction and Seed. With Render automatically on, every
//! change renders again in the background (Apply renders when it is off); the status strip shows
//! Initializing..., then Rendering... with the progress, then Ready. Each finished render shows
//! in the layer. OK applies the last one as one history item (waiting for a render under way);
//! Cancel leaves the layer as it was.

use egui::{Key, Pos2, Rect, Ui, Vec2};
use pdn_core::canvas::ImageData;
use pdn_core::effect::forum::content_aware_fill::{FILL_DIRECTION, SAMPLE_FROM, params};
use pdn_core::effect::session::EffectSession;
use pdn_core::effect::slider::{RangeRole, SliderKeys};
use pdn_core::effect::{Values, choice, num};
use pdn_core::extra::interrupt::Cancelled;
use pdn_core::extra::job::Job;
use pdn_core::extra::resynthesizer::{self, DEFAULT_SEED, WHOLE_IMAGE_SELECTED};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::effect_config::{
    OpenSelect, RangeSpec, SelectLook, SliderState, Track, button_clicked, checkbox_line,
    icon_button, range_line, range_rects, select,
};
use super::image_size::{footer_pair, rule};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{DialogFrameState, Footer, dialog_button, dialog_button_width};
use crate::theme::{self, *};

/// The sample size slider: 1–100, default 50, a tick every 5.
const SAMPLE_SIZE: RangeSpec = RangeSpec {
    min: 1.0,
    max: 100.0,
    default: 50.0,
    step: 1.0,
    role: RangeRole::Slider,
    keyboard: SliderKeys::INTEGER,
};
const TICK: (f32, f32) = (1.0, 3.0);
/// The Seed box's width (the plugin's) and the status strip's progress bar, DIP.
const SEED_W: f32 = 152.0;
const PROGRESS: Vec2 = Vec2::new(100.0, 16.0);
/// No progress reported yet: the job is still initializing.
const INITIALIZING: u32 = u32::MAX;

/// A render: the fill with the selection's coverage applied, or the plugin's error.
type Render = Result<ImageData, resynthesizer::Error>;

pub struct ContentAwareFillDialog {
    session: Option<EffectSession>,
    title: String,
    icon: &'static str,
    values: Values,
    slider: SliderState,
    open_select: Option<OpenSelect>,
    /// The render under way and its last progress (`INITIALIZING` before the first).
    job: Option<(Job<Render>, Arc<AtomicU32>)>,
    /// The last finished render, shown in the layer.
    output: Option<ImageData>,
    /// The first frame checked the selection (and started the first render).
    opened: bool,
    /// OK was pressed during a render: it applies when the render ends.
    close_pending: bool,
}

impl ContentAwareFillDialog {
    pub fn new(session: EffectSession, values: Values) -> Self {
        let title = session.def.display_name();
        let icon = session.def.icon;
        Self {
            session: Some(session),
            title,
            icon,
            values,
            slider: SliderState::default(),
            open_select: None,
            job: None,
            output: None,
            opened: false,
            close_pending: false,
        }
    }

    fn auto(&self) -> bool {
        pdn_core::effect::flag(&self.values, "renderAutomatically")
    }

    /// `StartBackgroundWorker`: a render with the current values; one under way is dropped.
    fn start(&mut self) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let p = params(&self.values);
        let progress = Arc::new(AtomicU32::new(INITIALIZING));
        let report = progress.clone();
        let job = Job::spawn(move |ctl| {
            let (source, Some(mask)) = (session.source(), session.mask()) else {
                return Err(Cancelled);
            };
            let mut store = |v| report.store(v, Ordering::Relaxed);
            match resynthesizer::fill(source, mask, &p, ctl, &mut store) {
                Ok(mut out) => {
                    pdn_core::effect::underwrite(source, &mut out, mask);
                    Ok(Ok(out))
                }
                Err(resynthesizer::Error::Cancelled) => Err(Cancelled),
                Err(e) => Ok(Err(e)),
            }
        });
        self.job = Some((job, progress));
    }

    /// `RunWorkerCompleted`: a finished render shows in the layer (and applies when OK waits for
    /// it); an error shows its message.
    fn poll(&mut self, cx: &mut DialogCx) -> Option<DialogState> {
        let result = self.job.as_mut()?.0.poll()?;
        self.job = None;
        match result {
            Ok(Ok(out)) => {
                if let Some(s) = &self.session {
                    s.show(cx.app, Some(&out));
                }
                self.output = Some(out);
                if self.close_pending {
                    return Some(self.ok(cx));
                }
            }
            Ok(Err(e)) => {
                self.close_pending = false;
                let message = e.message().unwrap_or_default().to_string();
                crate::platform::files::message(&self.title, message);
            }
            Err(Cancelled) => {}
        }
        None
    }

    /// OK: the last render as one history item named after the effect (the layer as it was when
    /// nothing rendered, as the plugin's empty output does); during a render, once it ends.
    fn ok(&mut self, cx: &mut DialogCx) -> DialogState {
        if self.job.is_some() {
            self.close_pending = true;
            return DialogState::Open;
        }
        let Some(s) = self.session.take() else {
            return DialogState::Closed(DialogResult::Cancel);
        };
        let out = self.output.take().unwrap_or_else(|| s.source().clone());
        if s.commit_with(cx.app, self.values.clone(), Some(out)) {
            DialogState::Closed(DialogResult::Ok)
        } else {
            DialogState::Closed(DialogResult::Cancel)
        }
    }

    /// Cancel: the render dropped, the layer and its dirty flag as they were.
    fn cancel(&mut self, cx: &mut DialogCx) -> DialogState {
        self.job = None;
        if let Some(s) = self.session.take() {
            s.cancel(cx.app);
        }
        DialogState::Closed(DialogResult::Cancel)
    }

    /// A heading's height: its line, the gap to the separator, the separator and the row gap.
    fn heading_height(ppp: f32) -> f32 {
        FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(ppp)
            + EFFECT_LABEL_PADDING
            + 1.0
            + EFFECT_ROW_GAP
    }

    fn content_height(ppp: f32) -> f32 {
        let (pt, _, pb) = EFFECT_CONTENT_PADDING;
        pt + 4.0 * Self::heading_height(ppp)
            + EFFECT_LINE_HEIGHT
            + 2.0 * EFFECT_SELECT_HEIGHT
            + EFFECT_LINE_HEIGHT
            + EFFECT_CHECKBOX.2.max(DIALOG_BUTTON_MIN.1)
            + 4.0 * EFFECT_CONTENT_GAP
            + pb
    }

    /// `.effect-config-row > label`: the text over a separator; returns the row's line top.
    fn heading(ui: &Ui, x0: f32, w: f32, y: f32, s: &str) -> f32 {
        let theme = theme::current(ui.ctx());
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let ppp = ui.ctx().pixels_per_point();
        let line = spec.resolved_line_height(ppp);
        text::draw(
            ui.painter(),
            Pos2::new(x0, y),
            s,
            spec,
            theme.menu_item_text,
        );
        rule(ui, x0, x0 + w, y + line + EFFECT_LABEL_PADDING);
        y + Self::heading_height(ppp)
    }

    /// A drop-down row of `choices` for `key`.
    fn choice_row(
        &mut self,
        ui: &mut Ui,
        key: &str,
        label: &str,
        choices: &[(&str, &str)],
        at: Rect,
    ) {
        let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
        let current = choices
            .iter()
            .position(|c| c.0 == choice(&self.values, key))
            .unwrap_or(0);
        let look = SelectLook {
            native: true,
            pad_left: 0.0,
            font: FontSpec::new(DIALOG_FONT_SIZE),
        };
        let id = ui.scope_id().with(("select", key));
        if let Some(i) = select(
            ui,
            &mut self.open_select,
            id,
            at,
            label,
            &labels,
            current,
            look,
        ) {
            self.values.insert(key.into(), json!(choices[i].0));
        }
    }

    /// The rows; true when Randomize was clicked.
    fn body(&mut self, ui: &mut Ui, r: Rect, now: f64) -> bool {
        let theme = theme::current(ui.ctx());
        let (pt, side, _) = EFFECT_CONTENT_PADDING;
        let (x0, w) = (r.min.x + side, r.width() - 2.0 * side);
        let mut y = r.min.y + pt;
        // Sample area size: the slider with its ticks, the number and the reset button.
        y = Self::heading(ui, x0, w, y, "Sample area size (in pixels):");
        let (range, number, reset) = range_rects(x0, w, y);
        let tw = EFFECT_THUMB.0;
        for v in (1..100).step_by(5).chain([100]) {
            let f = (v - 1) as f32 / 99.0;
            let x = range.min.x + tw / 2.0 + f * (range.width() - tw);
            let tick = Rect::from_min_size(
                Pos2::new(x - TICK.0 / 2.0, range.max.y - TICK.1),
                Vec2::new(TICK.0, TICK.1),
            );
            let ppp = ui.ctx().pixels_per_point();
            ui.painter()
                .rect_filled(crate::kit::px::snap_rect(tick, ppp), 0.0, theme.slider_tick);
        }
        self.slider.now = now;
        range_line(
            ui,
            &mut self.slider,
            "sampleSize",
            SAMPLE_SIZE,
            &mut self.values,
            range,
            number,
            Some(reset),
            Track::Progress(0.0),
            NumberStyle::EFFECT,
        );
        y += EFFECT_LINE_HEIGHT + EFFECT_CONTENT_GAP;
        y = Self::heading(ui, x0, w, y, "Sample from:");
        let at = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, EFFECT_SELECT_HEIGHT));
        self.choice_row(ui, "sampleFrom", "Sample from:", SAMPLE_FROM, at);
        y += EFFECT_SELECT_HEIGHT + EFFECT_CONTENT_GAP;
        y = Self::heading(ui, x0, w, y, "Fill direction:");
        let at = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, EFFECT_SELECT_HEIGHT));
        self.choice_row(ui, "fillDirection", "Fill direction:", FILL_DIRECTION, at);
        y += EFFECT_SELECT_HEIGHT + EFFECT_CONTENT_GAP;
        // Seed: the number, its reset button and Randomize.
        y = Self::heading(ui, x0, w, y, "Seed");
        let seed_box = Rect::from_min_size(
            Pos2::new(x0, y + (EFFECT_LINE_HEIGHT - EFFECT_NUMBER_HEIGHT) / 2.0),
            Vec2::new(SEED_W, EFFECT_NUMBER_HEIGHT),
        );
        let mut seed = num(&self.values, "seed");
        inputs::number_input(
            ui,
            "seed",
            "Seed",
            seed_box,
            &mut seed,
            0.0,
            f64::from(i32::MAX),
            1.0,
            true,
            NumberStyle::EFFECT,
        );
        let (_, rw, gap) = EFFECT_LINE_COLUMNS;
        let reset = Rect::from_min_size(
            Pos2::new(seed_box.max.x + gap, y + (EFFECT_LINE_HEIGHT - rw) / 2.0),
            Vec2::splat(rw),
        );
        if icon_button(ui, ui.scope_id().with("seed-reset"), reset, EFFECT_RESET.1) {
            seed = f64::from(DEFAULT_SEED);
        }
        let (bw, bh) = DIALOG_BUTTON_MIN;
        let button = |ui: &Ui, label, top: f32| {
            let w = dialog_button_width(ui.painter(), label, bw);
            Rect::from_min_size(Pos2::new(r.max.x - side - w, top), Vec2::new(w, bh))
        };
        let at = button(ui, "Randomize", y + (EFFECT_LINE_HEIGHT - bh) / 2.0);
        let response = dialog_button(ui, "randomize", at, "Randomize", false);
        let randomize = button_clicked(ui, &response);
        self.values.insert("seed".into(), json!(seed.round()));
        y += EFFECT_LINE_HEIGHT + EFFECT_CONTENT_GAP;
        // Render automatically, and Apply while it is off.
        let row = EFFECT_CHECKBOX.2.max(bh);
        let apply = button(ui, "Apply", y + (row - bh) / 2.0);
        let label_w = apply.min.x - x0 - gap;
        let color = theme.menu_item_text;
        let top = y + (row - EFFECT_CHECKBOX.2) / 2.0;
        checkbox_line(
            ui,
            &mut self.values,
            "renderAutomatically",
            "Render automatically",
            x0,
            top,
            label_w,
            color,
            1.0,
        );
        let enabled = !self.auto();
        let response = inputs::push_button(ui, "apply", apply, "Apply", enabled, false);
        if enabled && button_clicked(ui, &response) {
            self.start();
        }
        randomize
    }
}

/// The status strip at `r`'s left: Ready / Initializing... / Rendering... and the progress bar (a
/// sliding block while initializing). `progress`: the render's, `None` without one.
fn status(ui: &Ui, r: Rect, progress: Option<u32>, now: f64) {
    let theme = theme::current(ui.ctx());
    let spec = FontSpec::new(DIALOG_FONT_SIZE);
    let ppp = ui.ctx().pixels_per_point();
    let line = spec.resolved_line_height(ppp);
    let label = match progress {
        None => "Ready",
        Some(INITIALIZING) => "Initializing...",
        Some(_) => "Rendering...",
    };
    let y = r.center().y;
    let painter = ui.painter();
    text::draw(
        painter,
        Pos2::new(r.min.x, y - line / 2.0),
        label,
        spec,
        theme.menu_item_text,
    );
    let x = r.min.x + text::css_width("Initializing...", DIALOG_FONT_SIZE) + EFFECT_CONTENT_GAP;
    let bar = Rect::from_min_size(Pos2::new(x, y - PROGRESS.y / 2.0), PROGRESS);
    let snap = |r| crate::kit::px::snap_rect(r, ppp);
    painter.rect_filled(snap(bar), 0.0, theme.pdn_control_border);
    let inner = bar.shrink(1.0);
    painter.rect_filled(snap(inner), 0.0, theme.pdn_slider_background);
    let (a, b) = match progress {
        None => (0.0, 0.0),
        Some(INITIALIZING) => {
            let t = (now / 1500.0).fract() as f32 * 1.3 - 0.3;
            (t.max(0.0), (t + 0.3).min(1.0))
        }
        Some(p) => (0.0, p as f32 / 100.0),
    };
    if b > a {
        let fill = Rect::from_min_max(
            Pos2::new(inner.min.x + a * inner.width(), inner.min.y),
            Pos2::new(inner.min.x + b * inner.width(), inner.max.y),
        );
        painter.rect_filled(snap(fill), 0.0, theme.menu_item_selected_border);
    }
}

impl Dialog for ContentAwareFillDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        if self
            .session
            .as_ref()
            .is_none_or(|s| !s.owner_exists(cx.app))
        {
            return self.cancel(cx);
        }
        // `OnShown`: the plugin needs some of the image unselected.
        if !self.opened {
            self.opened = true;
            if self.session.as_ref().is_some_and(|s| s.mask().is_none()) {
                crate::platform::files::message(&self.title, WHOLE_IMAGE_SELECTED.into());
                return self.cancel(cx);
            }
            if self.auto() {
                self.start();
            }
        }
        if let Some(state) = self.poll(cx) {
            return state;
        }
        let plain = |key| {
            ui.input(|i| {
                !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.mac_cmd && i.key_pressed(key)
            })
        };
        let list_open = self.open_select.is_some();
        if plain(Key::Escape) {
            if !list_open {
                return self.cancel(cx);
            }
            self.open_select = None;
        }
        let enter = plain(Key::Enter) && !list_open;
        let now = cx.app.now_ms;
        let before = params(&self.values);
        let width = EFFECT_DIALOG_WIDTH.min(ctx.content_rect().width() - DIALOG_VIEWPORT_MARGIN);
        let height = Self::content_height(ctx.pixels_per_point());
        let (title, icon) = (self.title.clone(), self.icon);
        let progress = self.job.as_ref().map(|(_, p)| p.load(Ordering::Relaxed));
        let (mut action, mut randomize) = (None, false);
        let mut body = |ui: &mut Ui, r: Rect| randomize = self.body(ui, r, now);
        let mut footer = |ui: &mut Ui, f: Rect| {
            let (fy, fx) = DIALOG_FOOTER_PADDING;
            let content = Rect::from_min_max(
                Pos2::new(f.min.x + fx, f.min.y + fy),
                Pos2::new(f.max.x - fx, f.max.y - fy),
            );
            status(ui, content, progress, now);
            action = footer_pair(ui, content, DIALOG_BUTTON_MIN.0, DIALOG_BUTTON_MIN.1, true);
        };
        let frame = crate::kit::window::dialog_frame_with(
            ui,
            &title,
            Some(icon),
            false,
            (Vec2::new(width, height), &mut body),
            Some(&mut footer),
            Footer {
                border: false,
                ..Footer::DEFAULT
            },
        );
        if randomize {
            // `Random.Shared.Next()`: 0 to int.MaxValue − 1.
            let seed = cx.app.next_instance_seed() % i32::MAX as u32;
            self.values.insert("seed".into(), json!(f64::from(seed)));
        }
        // `UpdateConfigToken`: a changed setting renders again while Render automatically is on.
        let changed = params(&self.values) != before;
        if changed && self.auto() {
            self.start();
        }
        if self.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        state.popup = if self.open_select.is_some() {
            ctx.content_rect()
        } else {
            Rect::NOTHING
        };
        state.set(&ctx, ui.scope_id());
        if cx.seat_focus {
            super::seat_no_focus(&ctx);
        }
        match action {
            Some(true) => self.ok(cx),
            Some(false) => self.cancel(cx),
            None if frame.closed => self.cancel(cx),
            None if enter && !inputs::enter_taken(&ctx) => self.ok(cx),
            None => DialogState::Open,
        }
    }
}
