//! BB spec: effects/render-chart-or-graph.md "Window 1", "Window 2", "Window 3", "Quirks"
//!
//! Chart or Graph's own windows (BB36). Window 1 is the effect dialog's content
//! (`EffectConfigDialog`, which owns the one committing session, previews and OK/Cancel); the data
//! editor (Window 2), the colour picker (Window 3), the column-rename box and the message boxes
//! are child dialogs on the stack. Children work on drafts and hand back one result through a
//! shared cell; they close themselves once the main dialog is gone.
//!
//! The spec's fixable quirks are fixed (1: controls synchronised on open; 2: Enter Data's OK
//! re-applies Key and refreshes the Explode list; 4: rows and series stay in step; 8: a series is
//! kept); quirks 3, 5 and 6 are kept (`chart::edit`).
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, ColorImage, Id, Key, Pos2, Rect, Sense, TextureHandle, Ui, Vec2};
use pdn_core::effect::Values;
use pdn_core::effect::bb::chart::edit::{self, Grid, NamedColors, PickerHsv, SortJob};
use pdn_core::effect::bb::chart::model::{self, Chart, ChartError, ChartType};
use pdn_core::effect::bb::chart::{render_cancellable, xml};
use pdn_core::effect::bb::conventions::{
    CHART_MAX_ENCODED_BYTES, CHART_MAX_STRING_UTF16, CHART_UNREADABLE_TOKEN,
};
use pdn_core::effect::session::EffectSession;
use serde_json::Value;

use super::effect_config::{OpenSelect, SelectLook, initial_editor, merge, select};
use super::task::{TaskChoice, TaskDialog, TaskDialogHandle, TaskDialogSpec};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle, Utf16Text};
use crate::kit::text::{self, Alignment, FontSpec};
use crate::kit::window::{DialogFrameState, dialog_frame};
use crate::kit::{img, px, scroll};
use crate::platform::{DataFile, FileSaveOutcome, Files, Pending, PickedFile};
use crate::theme::{self, *};

pub const EDITOR_TITLE: &str = "Edit Chart Data      (Right-click on column names to change them.)";
const LOAD: DataFile = DataFile {
    title: "Load Chart Data",
    filter: "XML Files (*.xml)",
    extension: "xml",
};
const SAVE: DataFile = DataFile {
    title: "Save Chart Data",
    ..LOAD
};
/// WinForms' default dialog text: 8.25 pt.
const FONT: f32 = FONT_SIZE_PX;
const BLACK: Color32 = Color32::BLACK;
const WHITE: Color32 = Color32::WHITE;
/// DataGridView's grid and header colours (ControlDark, Control) and its selection (Highlight).
const GRID_LINE: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
const CONTROL: Color32 = Color32::from_rgb(0xF0, 0xF0, 0xF0);
const HIGHLIGHT: Color32 = Color32::from_rgb(0x00, 0x78, 0xD7);
const WELL_NAMES: [&str; 8] = [
    "Black", "White", "Red", "Green", "Blue", "Magenta", "Cyan", "Yellow",
];

/// What the windows share for the main dialog's life: the random named-colour stream (R146),
/// the editor window's size and splitter, and Window 3's palette wells.
pub struct Life {
    pub colors: NamedColors,
    pub wells: [u32; 8],
    editor_size: Option<Vec2>,
    bottom: f32,
    /// The open editor's grid, mirrored for tests.
    #[cfg(test)]
    pub last_grid: Option<Grid>,
}

fn rect_at(origin: Pos2, [x, y, w, h]: [f32; 4]) -> Rect {
    Rect::from_min_size(origin + Vec2::new(x, y), Vec2::new(w, h))
}

/// `response`'s accessible node: role, name and bounds (also for widgets that take no focus).
fn node(ui: &Ui, response: &egui::Response, role: egui::accesskit::Role, name: &str) {
    let ppp = ui.ctx().pixels_per_point() as f64;
    let r = response.rect;
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(role);
        n.set_label(name);
        n.set_bounds(egui::accesskit::Rect {
            x0: r.min.x as f64 * ppp,
            y0: r.min.y as f64 * ppp,
            x1: r.max.x as f64 * ppp,
            y1: r.max.y as f64 * ppp,
        });
    });
}

/// Grid cells, list items and strips take clicks without taking the keyboard focus.
const CLICK: Sense = Sense::CLICK.union(Sense::DRAG);

/// A static label with an accessible node.
fn label(ui: &mut Ui, at: Pos2, s: &str) -> Rect {
    let r = text::draw(ui.painter(), at, s, FontSpec::new(FONT), BLACK);
    let response = ui.interact(r, ui.scope_id().with(("label", s)), Sense::hover());
    node(ui, &response, egui::accesskit::Role::Label, s);
    r
}

/// A WinForms button: optional 16-px icon at the left, then the text.
fn button(ui: &mut Ui, rect: Rect, icon: Option<&str>, s: &str) -> bool {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(snapped, ui.scope_id().with(("button", s)), Sense::click());
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label(s);
    });
    let border = if response.has_focus() {
        theme.menu_item_selected_border
    } else {
        theme.button_outline
    };
    let painter = ui.painter();
    painter.rect_filled(snapped, 0.0, border);
    painter.rect_filled(
        snapped.shrink(px::border_width(1.0, ppp)),
        0.0,
        theme.button,
    );
    let font = FontSpec::new(FONT);
    let w = text::layout_width(painter, s, font);
    let mut left = rect.center().x - w / 2.0;
    if let Some(icon) = icon {
        let side = CHART_BUTTON_ICON;
        let at = Pos2::new(rect.min.x + 4.0, rect.center().y - side / 2.0);
        img::paint_icon(
            painter,
            icon,
            Rect::from_min_size(at, Vec2::splat(side)),
            theme.button,
            img::Filter::None,
        );
        left = (at.x + side + 2.0).max(left);
    }
    let top = rect.center().y - font.resolved_line_height(ppp) / 2.0;
    text::draw(painter, Pos2::new(left, top), s, font, theme.button_text);
    inputs::take_enter(ui, &response);
    inputs::button_activated(ui, &response)
}

/// A checkbox with its caption to the right, one click target (R119).
fn check(ui: &mut Ui, rect: Rect, s: &str, value: &mut bool) -> bool {
    let side = 13.0;
    let boxed = Rect::from_min_size(
        Pos2::new(rect.min.x, rect.center().y - side / 2.0),
        Vec2::splat(side),
    );
    let before = *value;
    let response = inputs::checkbox_in(ui, ("check", s), boxed, rect, value, true, WHITE);
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::CheckBox);
        n.set_label(s);
    });
    let line = FontSpec::new(FONT).resolved_line_height(ui.ctx().pixels_per_point());
    let at = Pos2::new(boxed.max.x + 3.0, rect.center().y - line / 2.0);
    text::draw(ui.painter(), at, s, FontSpec::new(FONT), BLACK);
    before != *value
}

/// A single-line text box with an accessible name; true when its text changed.
fn text_box(ui: &mut Ui, rect: Rect, name: &str, text: &mut String) -> (bool, egui::Response) {
    let mut buffer = Utf16Text {
        text,
        max_units: pdn_core::effect::bb::conventions::CHART_MAX_STRING_UTF16,
    };
    let response = inputs::text_field(ui, ("text", name), rect, &mut buffer);
    ui.ctx()
        .accesskit_node_builder(response.id, |n| n.set_label(name));
    (response.changed(), response)
}

/// A preview render in progress: what it draws, its cancellation and where its picture lands.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // browsers render in place
struct PreviewJob {
    key: (u64, [u32; 2]),
    cancel: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: std::sync::mpsc::Receiver<Option<ColorImage>>,
}

/// The chart at `size`, or `None` once cancelled; an undrawable chart is blank white (the
/// original shows an error cross).
fn picture(
    chart: &Chart,
    [w, h]: [u32; 2],
    culture: &str,
    cancel: &AtomicBool,
) -> Option<ColorImage> {
    match render_cancellable(chart, w, h, culture, Some(cancel)) {
        Ok(img) => Some(ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &img.data,
        )),
        Err(ChartError::Cancelled) => None,
        Err(_) => Some(blank([w, h])),
    }
}

/// A blank white picture of `size`.
fn blank([w, h]: [u32; 2]) -> ColorImage {
    ColorImage::new(
        [w as usize, h as usize],
        vec![WHITE; w as usize * h as usize],
    )
}

/// A rendered chart as a texture. A dense chart takes over a second even in release (16 384
/// points of 3-D Spline Area: 1.7 s at 534 × 295), so renders run on a worker, one at a time:
/// a newer chart cancels the running one and starts once it has stopped; the last picture
/// stays up meanwhile. (Browsers have no worker here: they render in place, like the effect.)
#[derive(Default)]
struct Preview {
    /// The newest chart and size wanted, and the one the texture shows.
    want: Option<(u64, [u32; 2])>,
    shown: Option<(u64, [u32; 2])>,
    texture: Option<TextureHandle>,
    job: Option<PreviewJob>,
}

/// A closed window's render stops at its next check.
impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Preview {
    fn present(&mut self, ui: &Ui, key: (u64, [u32; 2]), image: ColorImage, name: &str) {
        let options = egui::TextureOptions::NEAREST;
        match &mut self.texture {
            Some(t) => t.set(image, options),
            None => self.texture = Some(ui.ctx().load_texture(name, image, options)),
        }
        self.shown = Some(key);
    }

    // spec-question: Q99 Chart previews under interface scaling
    fn show(
        &mut self,
        ui: &Ui,
        rect: Rect,
        chart: &Chart,
        revision: u64,
        culture: &str,
        name: &str,
    ) {
        let size = [
            rect.width().round().max(1.0) as u32,
            rect.height().round().max(1.0) as u32,
        ];
        let key = (revision, size);
        if self.want != Some(key) {
            self.want = Some(key);
            if let Some(job) = &self.job {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        // A finished (or abandoned) render: its key and picture.
        #[cfg(not(target_arch = "wasm32"))]
        let done = self.job.as_ref().and_then(|job| {
            use std::sync::mpsc::TryRecvError;
            match job.rx.try_recv() {
                Ok(image) => Some((job.key, image)),
                // The worker died without a picture (a renderer panic): shown blank, as an
                // undrawable chart, so the same render does not start again every frame.
                Err(TryRecvError::Disconnected) => Some((job.key, Some(blank(job.key.1)))),
                Err(TryRecvError::Empty) => None,
            }
        });
        #[cfg(target_arch = "wasm32")]
        let done: Option<((u64, [u32; 2]), Option<ColorImage>)> = None;
        if let Some((finished, image)) = done {
            self.job = None;
            if let Some(image) = image
                && Some(finished) == self.want
            {
                self.present(ui, finished, image, name);
            }
            // A newer chart starts on the next frame.
            ui.ctx().request_repaint();
        } else if self.job.is_none() && self.want != self.shown {
            let cancel = Arc::new(AtomicBool::new(false));
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (tx, rx) = std::sync::mpsc::channel();
                let (chart, culture, flag) = (chart.clone(), culture.to_string(), cancel.clone());
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let _ = tx.send(picture(&chart, size, &culture, &flag));
                    ctx.request_repaint();
                });
                self.job = Some(PreviewJob { key, cancel, rx });
            }
            #[cfg(target_arch = "wasm32")]
            if let Some(image) = picture(chart, size, culture, &cancel) {
                self.present(ui, key, image, name);
            }
        }
        if let Some(t) = &self.texture {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            ui.painter().image(t.id(), rect, uv, WHITE);
        }
    }
}

/// An error message box (OK, error icon).
fn error_box(title: &str, message: String) {
    TaskDialog::show(TaskDialogSpec {
        title: title.into(),
        message: Some(message),
        choices: vec![TaskChoice {
            value: "ok",
            title: "OK".into(),
            ..Default::default()
        }],
        cancel_value: "ok",
        error_box: true,
        ..Default::default()
    });
}

enum Load {
    Choose(Pending<Result<Vec<PickedFile>, String>>),
    Read(Pending<Result<PickedFile, String>>),
}

/// OK or Cancel of Window 1, for its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartAction {
    Ok,
    Cancel,
}

/// Window 1 ("Chart or Graph"): the stored chart and its controls.
pub struct ChartDialog {
    /// The stored chart; `values["chart"]` is its JSON.
    pub chart: Chart,
    /// The Title box (the chart title is its trimmed text).
    pub title: String,
    pub key: bool,
    /// The Explode list: the first series' category names and their selection.
    pub explode: Vec<(String, bool)>,
    anchor: usize,
    explode_scroll: f32,
    /// Remembered slice colours, by position.
    pub memory: Vec<Option<u32>>,
    pub life: Rc<RefCell<Life>>,
    pub seed: u32,
    culture: String,
    /// Children hold a weak reference: gone with the main dialog.
    alive: Rc<()>,
    /// The last chart that encoded: the effect token (R143, never an empty stand-in).
    valid: String,
    revision: u64,
    preview: Preview,
    editor: Option<Rc<RefCell<Option<Chart>>>>,
    load: Option<(u64, Load)>,
    save: Option<Pending<Result<FileSaveOutcome, String>>>,
    pub files: Files,
    pub action: Option<ChartAction>,
    /// The client rectangle last shown.
    pub client: Rect,
}

impl ChartDialog {
    /// Opens on the stored chart (an absent or unreadable token is the default chart; an empty
    /// one gets its first series, quirk 8) with its controls synchronised and no action run.
    // spec-question: Q98 Chart unreadable remembered settings
    pub fn open(values: &mut Values, session: &EffectSession) -> Self {
        let env = session.env();
        let mut chart = match values.get("chart") {
            Some(Value::String(s)) if s.is_empty() => Chart {
                series: vec![],
                ..Chart::default()
            },
            Some(Value::String(s)) => model::decode(s).unwrap_or_else(|_| CHART_UNREADABLE_TOKEN()),
            _ => Chart::default(),
        };
        edit::editable(&mut chart);
        let valid = model::encode(&chart).unwrap_or_else(|_| {
            chart = CHART_UNREADABLE_TOKEN();
            model::encode(&chart).expect("the default chart encodes")
        });
        let mut d = ChartDialog {
            chart,
            title: String::new(),
            key: false,
            explode: vec![],
            anchor: 0,
            explode_scroll: 0.0,
            memory: vec![],
            life: Rc::new(RefCell::new(Life {
                colors: NamedColors(env.instance_seed),
                wells: CHART_PICKER_WELL_COLORS,
                editor_size: None,
                bottom: CHART_EDITOR_BOTTOM,
                #[cfg(test)]
                last_grid: None,
            })),
            seed: env.instance_seed,
            culture: env.culture.to_string(),
            alive: Rc::new(()),
            valid,
            revision: 0,
            preview: Preview::default(),
            editor: None,
            load: None,
            save: None,
            files: Files::current(),
            action: None,
            client: Rect::NOTHING,
        };
        d.sync();
        values.insert("chart".into(), d.token());
        d
    }

    /// The effect token: the last valid chart's JSON.
    pub fn token(&self) -> Value {
        Value::String(self.valid.clone())
    }

    /// R143/R145: a change stays only while the chart encodes within the limits; otherwise the
    /// last valid chart and its controls come back, and the reason is shown.
    fn validated(&mut self) {
        match model::encode(&self.chart) {
            Ok(token) => self.valid = token,
            Err(e) => {
                if let Ok(chart) = model::decode(&self.valid) {
                    self.chart = chart;
                }
                self.sync();
                self.revision += 1;
                error_box("Chart", e.to_string());
            }
        }
    }

    fn first_type(&self) -> ChartType {
        self.chart.series[0].chart_type
    }

    fn explodes(&self) -> bool {
        matches!(self.first_type(), ChartType::Pie | ChartType::Doughnut)
    }

    /// The preview's size: taller for Pie and Doughnut.
    pub fn preview_size(&self) -> (u32, u32) {
        let h = if self.explodes() {
            CHART_PREVIEW_PIE_HEIGHT
        } else {
            CHART_PREVIEW[3]
        };
        (CHART_PREVIEW[2] as u32, h as u32)
    }

    /// "On dialog open" / "Load Data": the controls from the chart, no action run (R143).
    fn sync(&mut self) {
        self.title = self.chart.title.clone();
        self.key = self.chart.series[0].legend_visible;
        self.refill_explode();
    }

    /// The Explode list: the first series' names, selected where exploded.
    fn refill_explode(&mut self) {
        self.explode = match self.explodes() {
            true => self.chart.series[0]
                .points
                .iter()
                .map(|p| (p.category.clone(), p.exploded))
                .collect(),
            false => vec![],
        };
        self.anchor = 0;
    }

    fn changed(&mut self) -> Option<bool> {
        self.revision += 1;
        Some(false)
    }

    fn set_chart(&mut self, chart: Chart) -> Option<bool> {
        self.chart = chart;
        edit::editable(&mut self.chart);
        self.changed()
    }

    fn apply_explode(&mut self) -> Option<bool> {
        let names: Vec<&str> = self
            .explode
            .iter()
            .filter(|e| e.1)
            .map(|e| e.0.as_str())
            .collect();
        edit::apply_explode(&mut self.chart, &names);
        self.changed()
    }

    /// One frame of Window 1 in its client `r`; the change kind when the chart changed.
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        open_select: &mut Option<OpenSelect>,
    ) -> Option<bool> {
        self.client = r;
        let mut changed = self.poll(ui.ctx());
        ui.painter().rect_filled(r, 0.0, WHITE);
        let o = r.min + Vec2::new(CHART_PANEL[0], CHART_PANEL[1]);
        let at = |p: (f32, f32)| o + Vec2::new(p.0, p.1);
        // Tab order: Title, Enter Data, Load Data, Type, 3D, Key, Save Data, the axes, Explode.
        label(ui, at(CHART_TITLE_LABEL), "Title:");
        let (edited, title) = text_box(ui, rect_at(o, CHART_TITLE_BOX), "Title", &mut self.title);
        initial_editor(ui, title.id);
        if edited {
            self.chart.title = self.title.trim().to_string();
            changed = merge(changed, self.changed());
        }
        if button(
            ui,
            rect_at(o, CHART_ENTER_DATA),
            Some("menu_view_grid_icon.png"),
            "Enter Data",
        ) {
            self.open_editor();
        }
        if button(
            ui,
            rect_at(o, CHART_LOAD_DATA),
            Some("menu_file_open_icon.png"),
            "Load Data",
        ) {
            self.load = Some((self.revision, Load::Choose(self.files.choose_data(LOAD))));
        }
        label(ui, at(CHART_TYPE_LABEL), "Type:");
        let labels: Vec<&str> = ChartType::ALL.iter().map(|t| t.label()).collect();
        let current = ChartType::ALL
            .iter()
            .position(|&t| t == self.first_type())
            .unwrap_or(8);
        let look = SelectLook {
            native: false,
            pad_left: 0.0,
            font: FontSpec::new(FONT),
        };
        let id = ui.scope_id().with("chart-type");
        if let Some(i) = select(
            ui,
            open_select,
            id,
            rect_at(o, CHART_TYPE_LIST),
            "Type",
            &labels,
            current,
            look,
        ) && i != current
        {
            let mut life = self.life.borrow_mut();
            edit::change_type(
                &mut self.chart,
                ChartType::ALL[i],
                self.key,
                &mut self.memory,
                &mut life.colors,
            );
            drop(life);
            self.refill_explode();
            changed = merge(changed, self.changed());
        }
        let mut three_d = self.chart.three_d;
        if check(ui, rect_at(o, CHART_3D), "3D", &mut three_d) {
            self.chart.three_d = three_d;
            changed = merge(changed, self.changed());
        }
        if check(ui, rect_at(o, CHART_KEY), "Key", &mut self.key) {
            edit::apply_key(&mut self.chart, self.key);
            changed = merge(changed, self.changed());
        }
        if button(
            ui,
            rect_at(o, CHART_SAVE_DATA),
            Some("menu_file_save_icon.png"),
            "Save Data",
        ) {
            match xml::to_xml(&self.chart) {
                Ok(text) => {
                    self.save = Some(
                        self.files
                            .save_data(SAVE, "UntitledChart", text.into_bytes()),
                    )
                }
                Err(e) => error_box("Save Error", e.to_string()),
            }
        }
        let (w, h) = self.preview_size();
        let preview = Rect::from_min_size(
            o + Vec2::new(CHART_PREVIEW[0], CHART_PREVIEW[1]),
            Vec2::new(w as f32, h as f32),
        );
        self.preview.show(
            ui,
            preview,
            &self.chart,
            self.revision,
            &self.culture,
            "chart-preview",
        );
        if !self.first_type().is_pie_like() {
            label(ui, at(CHART_Y_LABEL), "Y-Axis:");
            if text_box(
                ui,
                rect_at(o, CHART_Y_BOX),
                "Y-Axis",
                &mut self.chart.y_title,
            )
            .0
            {
                changed = merge(changed, self.changed());
            }
            label(ui, at(CHART_X_LABEL), "X-Axis:");
            if text_box(
                ui,
                rect_at(o, CHART_X_BOX),
                "X-Axis",
                &mut self.chart.x_title,
            )
            .0
            {
                changed = merge(changed, self.changed());
            }
        }
        if self.explodes() {
            label(ui, at(CHART_EXPLODE_LABEL), "Explode:");
            if self.explode_list(ui, rect_at(o, CHART_EXPLODE_LIST)) {
                changed = merge(changed, self.apply_explode());
            }
        }
        if button(ui, rect_at(r.min, CHART_OK), None, "OK") {
            self.action = Some(ChartAction::Ok);
        }
        if button(ui, rect_at(r.min, CHART_CANCEL), None, "Cancel") {
            self.action = Some(ChartAction::Cancel);
        }
        if self.load.is_some() || self.save.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(10));
        }
        if changed.is_some() {
            self.validated();
        }
        changed
    }

    /// The multi-select list: a click selects one name, Ctrl+click toggles, Shift+click selects
    /// from the anchor; Up/Down move (Shift extends). True when the selection changed.
    fn explode_list(&mut self, ui: &mut Ui, rect: Rect) -> bool {
        let ppp = ui.ctx().pixels_per_point();
        let painter = ui.painter().clone();
        let r = px::snap_rect(rect, ppp);
        painter.rect_filled(r, 0.0, GRID_LINE);
        painter.rect_filled(r.shrink(1.0), 0.0, WHITE);
        let id = ui.scope_id().with("explode");
        let list = ui.interact(r, id, Sense::click());
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_role(egui::accesskit::Role::ListBox);
            n.set_label("Explode");
        });
        let inner = r.shrink(1.0);
        let item = CHART_LIST_ITEM;
        let total = self.explode.len() as f32 * item;
        if list.hovered() || ui.rect_contains_pointer(inner) {
            self.explode_scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        self.explode_scroll = self
            .explode_scroll
            .clamp(0.0, (total - inner.height()).max(0.0));
        let before: Vec<bool> = self.explode.iter().map(|e| e.1).collect();
        let mods = ui.input(|i| i.modifiers);
        let clip = painter.with_clip_rect(inner.intersect(painter.clip_rect()));
        let mut pick = None;
        for (i, (name, selected)) in self.explode.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(
                    inner.min.x,
                    inner.min.y + i as f32 * item - self.explode_scroll,
                ),
                Vec2::new(inner.width(), item),
            );
            if !row.intersects(inner) {
                continue;
            }
            let response = ui.interact(row.intersect(inner), id.with(i), CLICK);
            node(ui, &response, egui::accesskit::Role::ListBoxOption, name);
            if response.clicked() {
                pick = Some(i);
            }
            let (fill, color) = if *selected {
                (HIGHLIGHT, WHITE)
            } else {
                (WHITE, BLACK)
            };
            clip.rect_filled(row, 0.0, fill);
            text::draw(
                &clip,
                row.min + Vec2::new(2.0, 0.0),
                name,
                FontSpec::new(FONT),
                color,
            );
        }
        if let Some(i) = pick {
            list.request_focus();
            self.select_explode(i, mods);
        } else if list.has_focus() {
            let step = ui.input(|i| {
                i.key_pressed(Key::ArrowDown) as i32 - i.key_pressed(Key::ArrowUp) as i32
            });
            if step != 0 && !self.explode.is_empty() {
                let at =
                    (self.anchor as i32 + step).clamp(0, self.explode.len() as i32 - 1) as usize;
                self.select_explode(at, mods);
            }
        }
        self.explode.iter().map(|e| e.1).ne(before)
    }

    fn select_explode(&mut self, i: usize, mods: egui::Modifiers) {
        let ctrl = mods.command || mods.ctrl;
        if mods.shift {
            let (a, b) = (self.anchor.min(i), self.anchor.max(i));
            for (k, e) in self.explode.iter_mut().enumerate() {
                e.1 = (a..=b).contains(&k) || (ctrl && e.1);
            }
            return;
        }
        if ctrl {
            self.explode[i].1 = !self.explode[i].1;
        } else {
            for (k, e) in self.explode.iter_mut().enumerate() {
                e.1 = k == i;
            }
        }
        self.anchor = i;
    }

    fn open_editor(&mut self) {
        let result = Rc::new(RefCell::new(None));
        self.editor = Some(result.clone());
        super::open(Box::new(ChartEditor::new(
            Grid::new(self.chart.clone(), &self.culture),
            self.life.clone(),
            Rc::downgrade(&self.alive),
            result,
            self.culture.clone(),
        )));
    }

    /// The children's and the file services' results, each applied once.
    fn poll(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut changed = None;
        if let Some(cell) = &self.editor {
            let accepted = cell.borrow_mut().take();
            let gone = Rc::strong_count(cell) == 1;
            if accepted.is_some() || gone {
                self.editor = None;
            }
            // Quirk 2 fixed: the current Key and the accepted names (R143).
            if let Some(mut chart) = accepted {
                edit::apply_key(&mut chart, self.key);
                changed = self.set_chart(chart);
                self.refill_explode();
            }
        }
        if let Some(save) = &mut self.save
            && let Some(outcome) = save.poll()
        {
            self.save = None;
            if let Err(e) = outcome {
                error_box("Save Error", e);
            }
        }
        // A chooser answer and an already-read file finish in one frame.
        for _ in 0..2 {
            changed = merge(changed, self.poll_load(ctx));
        }
        changed
    }

    fn poll_load(&mut self, ctx: &egui::Context) -> Option<bool> {
        let mut changed = None;
        if let Some((revision, load)) = &mut self.load {
            let revision = *revision;
            // `Some(Ok(None))`: the chooser was cancelled.
            let outcome: Option<Result<Option<Chart>, String>> = match load {
                Load::Choose(p) => match p.poll() {
                    None => None,
                    Some(Err(e)) => Some(Err(e)),
                    Some(Ok(files)) => match files.into_iter().next() {
                        None => Some(Ok(None)),
                        Some(f) => {
                            *load = Load::Read(f.reread_within(CHART_MAX_ENCODED_BYTES as u64));
                            ctx.request_repaint();
                            None
                        }
                    },
                },
                // `reread_within` refused a file over the chart limit before reading it.
                Load::Read(p) => p.poll().map(|r| {
                    r.and_then(|f| {
                        let text = std::str::from_utf8(&f.bytes).map_err(|e| e.to_string())?;
                        xml::from_xml(text).map(Some).map_err(|e| e.to_string())
                    })
                }),
            };
            if let Some(result) = outcome {
                self.load = None;
                match result {
                    Ok(None) => {}
                    // A newer edit wins over an older completion.
                    Ok(Some(chart)) if revision == self.revision => {
                        changed = self.set_chart(chart);
                        self.sync();
                    }
                    Ok(Some(_)) => {}
                    Err(e) => error_box("Load Error", e),
                }
            }
        }
        changed
    }
}

/// A grid column: the swatch, the name, or data column `j`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Col {
    Swatch,
    Name,
    Data(usize),
}

/// A cell being edited: its draft text (`focus`: seat the caret at its end next frame).
struct CellEdit {
    row: usize,
    col: Col,
    text: String,
    focus: bool,
}

/// An open column menu (data column, at the pointer).
const MENU_ITEMS: [&str; 5] = [
    "Rename",
    "Delete",
    "",
    "Add Column to Left",
    "Add Column to Right",
];

/// Window 2, "Edit Chart Data": a grid over a chart copy, its preview and OK/Cancel.
struct ChartEditor {
    grid: Grid,
    life: Rc<RefCell<Life>>,
    alive: Weak<()>,
    result: Rc<RefCell<Option<Chart>>>,
    culture: String,
    /// Its own children (picker, rename box) hold a weak reference.
    children: Rc<()>,
    current: (usize, Col),
    selected_row: Option<usize>,
    editing: Option<CellEdit>,
    /// A press on a row: (row, where, dragging).
    press: Option<(usize, Pos2, bool)>,
    sort: Option<(Option<usize>, bool)>,
    menu: Option<(usize, Pos2)>,
    rename: Option<(usize, Rc<RefCell<Option<String>>>)>,
    confirm: Option<(usize, TaskDialogHandle)>,
    picker: Option<(usize, Rc<RefCell<Option<u32>>>)>,
    scroll: [f64; 2],
    preview: Preview,
    revision: u64,
    /// The stack asks for the initial focus: the grid takes it.
    seat: bool,
    /// A header click's sort, run over frames (R199); the rows move only once it finishes.
    sorting: Option<SortJob>,
    /// Tab out of the grid, applied as the grid registers next frame (egui moves the focus
    /// then).
    leave: Option<egui::FocusDirection>,
}

impl ChartEditor {
    fn new(
        grid: Grid,
        life: Rc<RefCell<Life>>,
        alive: Weak<()>,
        result: Rc<RefCell<Option<Chart>>>,
        culture: String,
    ) -> Self {
        ChartEditor {
            grid,
            life,
            alive,
            result,
            culture,
            children: Rc::new(()),
            current: (0, Col::Name),
            selected_row: None,
            editing: None,
            press: None,
            sort: None,
            menu: None,
            rename: None,
            confirm: None,
            picker: None,
            scroll: [0.0; 2],
            preview: Preview::default(),
            revision: 0,
            seat: false,
            sorting: None,
            leave: None,
        }
    }

    fn header(&self, col: Col) -> &str {
        match col {
            Col::Swatch => "Swatch",
            Col::Name => "Name",
            Col::Data(j) => &self.grid.headers[j],
        }
    }

    fn text(&self, row: usize, col: Col) -> String {
        match (row < self.grid.rows(), col) {
            (true, Col::Name) => self.grid.name(row).to_string(),
            (true, Col::Data(j)) => self.grid.cell(row, j),
            _ => String::new(),
        }
    }

    fn edited(&mut self) {
        self.revision += 1;
    }

    /// Ends the cell edit: the new row materializes first (one named colour), then the value
    /// or name rule applies; text that does not parse reverts; an edit past a limit (R145) is
    /// refused with its reason. An untouched empty draft in the new row makes no row.
    // spec-question: Q96 Chart data-editor edit mechanics
    // Structural, not an R76 constant: it decides when a row's colour is drawn, not which colour
    // or how anything is drawn; rows created the same way keep the same stream order.
    fn commit(&mut self) {
        let Some(e) = self.editing.take() else {
            return;
        };
        let mut row = e.row;
        if row == self.grid.rows() {
            if e.text.is_empty() {
                return;
            }
            match self.grid.add_row(&mut self.life.borrow_mut().colors) {
                Ok(r) => row = r,
                Err(why) => return error_box("Chart", why),
            }
        }
        let result = match e.col {
            Col::Name => self.grid.edit_name(row, &e.text),
            Col::Data(j) => self.grid.edit_value(row, j, &e.text),
            Col::Swatch => Ok(false),
        };
        if let Err(why) = result {
            error_box("Chart", why);
        }
        self.edited();
    }

    fn start_edit(&mut self, text: Option<String>) {
        let (row, col) = self.current;
        if col == Col::Swatch {
            return;
        }
        let text = text.unwrap_or_else(|| self.text(row, col));
        self.editing = Some(CellEdit {
            row,
            col,
            text,
            focus: true,
        });
    }

    fn columns(&self) -> Vec<Col> {
        let data = (0..self.grid.headers.len()).map(Col::Data);
        [Col::Swatch, Col::Name].into_iter().chain(data).collect()
    }

    /// Moves the current cell by rows and columns, within the grid and its new row.
    fn step(&mut self, down: i64, right: i64) {
        let cols = self.columns();
        let at = cols.iter().position(|&c| c == self.current.1).unwrap_or(1) as i64;
        let col = (at + right).clamp(0, cols.len() as i64 - 1) as usize;
        let row = (self.current.0 as i64 + down).clamp(0, self.grid.rows() as i64);
        self.current = (row as usize, cols[col]);
        self.selected_row = None;
    }

    /// Tab inside the grid (DataGridView, StandardTab false): the next or previous cell,
    /// wrapping rows; false past the last cell or before the first, where the focus leaves.
    fn tab_step(&mut self, back: bool) -> bool {
        let cols = self.columns();
        let n = cols.len() as i64;
        let at = cols.iter().position(|&c| c == self.current.1).unwrap_or(1) as i64;
        let i = self.current.0 as i64 * n + at + if back { -1 } else { 1 };
        if i < 0 || i >= (self.grid.rows() as i64 + 1) * n {
            return false;
        }
        self.current = ((i / n) as usize, cols[(i % n) as usize]);
        self.selected_row = None;
        true
    }

    /// A Tab press: Ctrl (or Cmd) leaves the grid, Shift goes backwards.
    fn tab(&mut self, mods: egui::Modifiers) {
        let back = mods.shift;
        if mods.ctrl || mods.command || !self.tab_step(back) {
            self.leave = Some(match back {
                true => egui::FocusDirection::Previous,
                false => egui::FocusDirection::Next,
            });
        }
    }

    /// The picker's, the rename box's and the delete question's answers.
    fn poll_children(&mut self) {
        if let Some((row, cell)) = self.picker.take() {
            let picked = cell.borrow_mut().take();
            match picked {
                Some(rgb) => {
                    self.grid.set_color(row, rgb);
                    self.edited();
                }
                None if Rc::strong_count(&cell) > 1 => self.picker = Some((row, cell)),
                None => {}
            }
        }
        if let Some((col, cell)) = self.rename.take() {
            let name = cell.borrow_mut().take();
            match name {
                Some(name) => {
                    if let Err(why) = self.grid.rename_column(col, &name) {
                        error_box("Chart", why);
                    }
                    self.edited();
                }
                None if Rc::strong_count(&cell) > 1 => self.rename = Some((col, cell)),
                None => {}
            }
        }
        if let Some((col, handle)) = self.confirm.take() {
            match handle.poll() {
                Some("yes") => {
                    self.grid.delete_column(col);
                    self.current = (self.current.0, Col::Name);
                    self.edited();
                }
                Some(_) => {}
                None => self.confirm = Some((col, handle)),
            }
        }
    }

    /// Sorts for about 8 ms in batches of 4096 comparisons; true while unfinished. A finished
    /// sort moves the rows (R149 Q47, R199).
    fn step_sort(&mut self) -> bool {
        let Some(job) = &mut self.sorting else {
            return false;
        };
        let deadline = web_time::Instant::now() + std::time::Duration::from_millis(8);
        while !job.step(4096) {
            if web_time::Instant::now() >= deadline {
                return true;
            }
        }
        let job = self.sorting.take().unwrap();
        self.grid.finish_sort(&job);
        self.sort = Some((job.col, job.ascending));
        self.selected_row = None;
        self.edited();
        // The grid was not drawn while sorting, so it lost the focus: it takes it back.
        self.seat = true;
        false
    }

    /// The grid while a sort runs: its progress and Cancel Sort, which keeps the old order.
    fn sorting_ui(&mut self, ui: &mut Ui, area: Rect) {
        let Some(job) = &self.sorting else {
            return;
        };
        ui.painter().rect_filled(area, 0.0, WHITE);
        let text = format!("Sorting… {:.0} %", job.progress() * 100.0);
        let at = area.center() - Vec2::new(60.0, 24.0);
        label(ui, at, &text);
        let cancel = Rect::from_min_size(at + Vec2::new(0.0, 20.0), Vec2::new(90.0, 23.0));
        if button(ui, cancel, None, "Cancel Sort") {
            self.sorting = None;
            self.seat = true;
        }
    }

    fn open_picker(&mut self, row: usize) {
        let cell = Rc::new(RefCell::new(None));
        super::open(Box::new(ColorChooser::new(
            self.grid.color(row).unwrap_or(0),
            self.life.clone(),
            Rc::downgrade(&self.children),
            cell.clone(),
        )));
        self.picker = Some((row, cell));
    }

    fn menu_pick(&mut self, item: usize, col: usize) {
        match MENU_ITEMS[item] {
            "Rename" => {
                let cell = Rc::new(RefCell::new(None));
                super::open(Box::new(RenameBox {
                    text: self.grid.headers[col].clone(),
                    result: cell.clone(),
                    alive: Rc::downgrade(&self.children),
                    seated: false,
                }));
                self.rename = Some((col, cell));
            }
            "Delete" => {
                let choice = |value, title: &str| TaskChoice {
                    value,
                    title: title.into(),
                    ..Default::default()
                };
                let handle = TaskDialog::show(TaskDialogSpec {
                    title: "Chart".into(),
                    message: Some("Delete this column?".into()),
                    message_icon: Some("menu_help_icon.png"),
                    choices: vec![choice("yes", "Yes"), choice("no", "No")],
                    cancel_value: "no",
                    ..Default::default()
                });
                self.confirm = Some((col, handle));
            }
            "Add Column to Left" | "Add Column to Right" => {
                if let Err(why) = self.grid.insert_column(col, item == 4) {
                    error_box("Chart", why);
                }
                self.edited();
            }
            _ => {}
        }
    }

    /// The client: the grid over the splitter, the preview and the OK/Cancel strip.
    fn body(&mut self, ui: &mut Ui, r: Rect) -> Option<DialogResult> {
        ui.painter().rect_filled(r, 0.0, CONTROL);
        let min_bottom = CHART_EDITOR_STRIP + 8.0;
        let max_bottom = (r.height() - CHART_GRID_HEADER - CHART_EDITOR_SPLITTER).max(min_bottom);
        let mut bottom = self.life.borrow().bottom.clamp(min_bottom, max_bottom);
        let grid_h = r.height() - bottom - CHART_EDITOR_SPLITTER;
        let split = Rect::from_min_size(
            r.min + Vec2::new(0.0, grid_h),
            Vec2::new(r.width(), CHART_EDITOR_SPLITTER),
        );
        let drag = ui.interact(split, ui.scope_id().with("splitter"), Sense::DRAG);
        if drag.dragged() {
            bottom = (bottom - drag.drag_delta().y).clamp(min_bottom, max_bottom);
        }
        if drag.hovered() || drag.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        self.life.borrow_mut().bottom = bottom;
        let grid = Rect::from_min_size(r.min, Vec2::new(r.width(), grid_h));
        self.grid_ui(ui, grid);
        let mut outcome = None;
        let strip_y = r.max.y - CHART_EDITOR_STRIP;
        let preview = Rect::from_min_max(
            Pos2::new(r.min.x, split.max.y),
            Pos2::new(
                r.max.x,
                (strip_y
                    - (CHART_EDITOR_BOTTOM - CHART_EDITOR_STRIP - CHART_EDITOR_PREVIEW_HEIGHT))
                    .max(split.max.y + 1.0),
            ),
        );
        self.preview.show(
            ui,
            preview,
            &self.grid.chart,
            self.revision,
            &self.culture,
            "chart-editor-preview",
        );
        let right = |[x, y, w, h]: [f32; 4]| {
            Rect::from_min_size(
                Pos2::new(r.max.x - (CHART_EDITOR_CLIENT.0 - x), strip_y + y),
                Vec2::new(w, h),
            )
        };
        if button(ui, right(CHART_EDITOR_OK), None, "OK") {
            self.sorting = None;
            self.commit();
            // R143: only a chart that encodes within the limits is handed back.
            match model::encode(&self.grid.chart) {
                Ok(_) => {
                    *self.result.borrow_mut() = Some(self.grid.chart.clone());
                    outcome = Some(DialogResult::Ok);
                }
                Err(e) => error_box("Chart", e.to_string()),
            }
        }
        if button(ui, right(CHART_EDITOR_CANCEL), None, "Cancel") {
            outcome = Some(DialogResult::Cancel);
        }
        self.menu_ui(ui);
        outcome
    }

    fn menu_ui(&mut self, ui: &mut Ui) {
        let Some((col, at)) = self.menu else {
            return;
        };
        let (w, row, gap) = (160.0, 22.0, 7.0);
        let h: f32 = MENU_ITEMS
            .iter()
            .map(|i| if i.is_empty() { gap } else { row })
            .sum();
        let rect = Rect::from_min_size(at, Vec2::new(w, h + 4.0));
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, GRID_LINE);
        painter.rect_filled(rect.shrink(1.0), 0.0, CONTROL);
        let mut y = rect.min.y + 2.0;
        let mut picked = None;
        for (i, item) in MENU_ITEMS.iter().enumerate() {
            if item.is_empty() {
                let line = Rect::from_min_size(
                    Pos2::new(rect.min.x + 4.0, y + 3.0),
                    Vec2::new(w - 8.0, 1.0),
                );
                ui.painter().rect_filled(line, 0.0, GRID_LINE);
                y += gap;
                continue;
            }
            let r = Rect::from_min_size(Pos2::new(rect.min.x + 2.0, y), Vec2::new(w - 4.0, row));
            let response = ui.interact(r, ui.scope_id().with(("menu", i)), CLICK);
            node(ui, &response, egui::accesskit::Role::MenuItem, item);
            if response.hovered() {
                ui.painter()
                    .rect_filled(r, 0.0, Color32::from_rgb(0xCC, 0xE8, 0xFF));
            }
            let line = FontSpec::new(FONT).resolved_line_height(ui.ctx().pixels_per_point());
            let at = Pos2::new(r.min.x + 24.0, r.center().y - line / 2.0);
            text::draw(ui.painter(), at, item, FontSpec::new(FONT), BLACK);
            if response.clicked() {
                picked = Some(i);
            }
            y += row;
        }
        let outside = ui.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
        });
        if let Some(i) = picked {
            self.menu = None;
            self.menu_pick(i, col);
        } else if outside || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.menu = None;
        }
    }

    /// The DataGridView: row headers, the frozen swatch and Name columns, the data columns and
    /// the new row, scrolling; editing, selection, dragging, sorting and the header menu.
    fn grid_ui(&mut self, ui: &mut Ui, area: Rect) {
        if self.sorting.is_some() {
            return self.sorting_ui(ui, area);
        }
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let font = FontSpec::new(FONT);
        let line = font.resolved_line_height(ppp);
        let grid_id = ui.scope_id().with("grid");
        if let Some(direction) = self.leave.take() {
            ui.memory_mut(|m| m.move_focus(direction));
        }
        let focus = ui.interact(area, grid_id, Sense::click());
        ctx.accesskit_node_builder(grid_id, |n| {
            n.set_role(egui::accesskit::Role::Grid);
            n.set_label("Chart data");
        });
        // A click on a cell or header over the grid surrenders the grid's focus; it is asked
        // back after them (`refocus`).
        let mut refocus = std::mem::take(&mut self.seat) || focus.clicked();
        // The grid's arrows and Tab move its current cell, not egui's focus; Tab leaves through
        // `tab`.
        if focus.has_focus() {
            let filter = egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: false,
            };
            ui.memory_mut(|m| m.set_focus_lock_filter(grid_id, filter));
        }
        let painter = ui.painter().clone();
        // Keys: an edit's Enter/Tab/Escape end it; otherwise the grid's own keys. Tab is read
        // with its modifiers (`consume_key` would take Shift+Tab as Tab).
        let tab = (focus.has_focus() || self.editing.is_some())
            .then(|| {
                ui.input_mut(|i| {
                    let mods = i.events.iter().find_map(|e| match e {
                        egui::Event::Key {
                            key: Key::Tab,
                            pressed: true,
                            modifiers,
                            ..
                        } => Some(*modifiers),
                        _ => None,
                    });
                    if mods.is_some() {
                        i.events
                            .retain(|e| !matches!(e, egui::Event::Key { key: Key::Tab, .. }));
                    }
                    mods
                })
            })
            .flatten();
        let key = |k: Key| ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
        if self.editing.is_some() {
            if key(Key::Escape) {
                self.editing = None;
                focus.request_focus();
            } else if key(Key::Enter) {
                self.commit();
                self.step(1, 0);
                focus.request_focus();
            } else if let Some(mods) = tab {
                self.commit();
                self.tab(mods);
                focus.request_focus();
            }
        } else if focus.has_focus() && self.menu.is_none() {
            let typed: Option<String> = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Text(t) if !t.is_empty() => Some(t.clone()),
                    _ => None,
                })
            });
            if let Some(t) = typed {
                self.start_edit(Some(t));
            } else if key(Key::F2) {
                self.start_edit(None);
            } else if key(Key::Enter) {
                self.step(1, 0);
            } else if let Some(mods) = tab {
                self.tab(mods);
            } else if key(Key::ArrowDown) {
                self.step(1, 0);
            } else if key(Key::ArrowUp) {
                self.step(-1, 0);
            } else if key(Key::ArrowRight) {
                self.step(0, 1);
            } else if key(Key::ArrowLeft) {
                self.step(0, -1);
            } else if key(Key::Delete)
                && let Some(r) = self.selected_row
                && self.grid.delete_row(r)
            {
                self.current = (r.min(self.grid.rows()), Col::Name);
                self.selected_row = None;
                self.edited();
            }
        }
        let rows = self.grid.rows();
        let pie = self.grid.pie();
        // Geometry: the frozen columns, then data columns of the default width.
        let name_w = (0..rows)
            .map(|r| text::layout_width(&painter, self.grid.name(r), font))
            .fold(text::layout_width(&painter, "Name", font) + 16.0, f32::max)
            .max(CHART_GRID_NAME.min(40.0))
            + 8.0;
        let fixed = CHART_GRID_ROW_HEADER + CHART_GRID_SWATCH_COLUMN + name_w;
        let data_w = self.grid.headers.len() as f32 * CHART_GRID_COLUMN;
        let (rh, hh) = (CHART_GRID_ROW, CHART_GRID_HEADER);
        let content = Vec2::new(fixed + data_w, hh + (rows + 1) as f32 * rh);
        let bar = scroll::THICKNESS;
        let wide = content.x > area.width();
        let tall = content.y > area.height() - if wide { bar } else { 0.0 };
        let wide = wide || (tall && content.x > area.width() - bar);
        let view = Rect::from_min_max(
            area.min,
            area.max - Vec2::new(if tall { bar } else { 0.0 }, if wide { bar } else { 0.0 }),
        );
        if ui.rect_contains_pointer(view) {
            let d = ui.input(|i| i.smooth_scroll_delta);
            self.scroll[0] -= d.x as f64;
            self.scroll[1] -= d.y as f64;
        }
        let max_x = (content.x - view.width()).max(0.0) as f64;
        let max_y = (content.y - view.height()).max(0.0) as f64;
        // The current cell stays in view.
        let (cr, cc) = self.current;
        let top = cr as f64 * rh as f64;
        let rows_view = (view.height() - hh) as f64;
        if top < self.scroll[1] {
            self.scroll[1] = top;
        } else if top + rh as f64 > self.scroll[1] + rows_view {
            self.scroll[1] = top + rh as f64 - rows_view;
        }
        if let Col::Data(j) = cc {
            let left = j as f64 * CHART_GRID_COLUMN as f64;
            let cols_view = (view.width() - fixed) as f64;
            if left < self.scroll[0] {
                self.scroll[0] = left;
            } else if left + CHART_GRID_COLUMN as f64 > self.scroll[0] + cols_view {
                self.scroll[0] = left + CHART_GRID_COLUMN as f64 - cols_view;
            }
        }
        self.scroll = [
            self.scroll[0].clamp(0.0, max_x),
            self.scroll[1].clamp(0.0, max_y),
        ];
        if tall {
            let r = Rect::from_min_max(
                Pos2::new(view.max.x, area.min.y),
                Pos2::new(area.max.x, view.max.y),
            );
            scroll::Scrollbar::vertical(r).id_salt("chart-grid-v").show(
                ui,
                content.y as f64 - hh as f64,
                rows_view,
                &mut self.scroll[1],
            );
        }
        if wide {
            let r = Rect::from_min_max(
                Pos2::new(area.min.x, view.max.y),
                Pos2::new(view.max.x, area.max.y),
            );
            scroll::Scrollbar::horizontal(r)
                .id_salt("chart-grid-h")
                .show(
                    ui,
                    data_w as f64,
                    (view.width() - fixed) as f64,
                    &mut self.scroll[0],
                );
        }
        let (sx, sy) = (self.scroll[0], self.scroll[1]);
        let clip = painter.with_clip_rect(view.intersect(painter.clip_rect()));
        clip.rect_filled(view, 0.0, WHITE);
        // Column cells' x ranges (screen), clipped to their region.
        let x0 = view.min.x;
        let data_left = x0 + fixed;
        let col_rect = |c: Col| -> (f32, f32) {
            match c {
                Col::Swatch => (x0 + CHART_GRID_ROW_HEADER, CHART_GRID_SWATCH_COLUMN),
                Col::Name => (
                    x0 + CHART_GRID_ROW_HEADER + CHART_GRID_SWATCH_COLUMN,
                    name_w,
                ),
                Col::Data(j) => (
                    data_left + j as f32 * CHART_GRID_COLUMN - sx as f32,
                    CHART_GRID_COLUMN,
                ),
            }
        };
        let region = |c: Col| -> Rect {
            match c {
                Col::Data(_) => Rect::from_min_max(Pos2::new(data_left, view.min.y), view.max),
                _ => view,
            }
        };
        let cols = self.columns();
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mut action: Option<(usize, Col, &'static str)> = None;
        // Header row.
        let header_rect = |c: Col| {
            let (x, w) = col_rect(c);
            Rect::from_min_size(Pos2::new(x, view.min.y), Vec2::new(w, hh)).intersect(region(c))
        };
        let corner = Rect::from_min_size(view.min, Vec2::new(CHART_GRID_ROW_HEADER, hh));
        for &c in &cols {
            let r = header_rect(c);
            if r.width() <= 0.0 {
                continue;
            }
            clip.rect_filled(r, 0.0, CONTROL);
            clip.rect_filled(
                Rect::from_min_max(Pos2::new(r.max.x - 1.0, r.min.y), r.max),
                0.0,
                GRID_LINE,
            );
            clip.rect_filled(
                Rect::from_min_max(Pos2::new(r.min.x, r.max.y - 1.0), r.max),
                0.0,
                GRID_LINE,
            );
            if c == Col::Swatch {
                continue;
            }
            let name = self.header(c).to_string();
            let response = ui.interact(r, grid_id.with(("header", c)), CLICK);
            node(
                ui,
                &response,
                egui::accesskit::Role::ColumnHeader,
                &format!("Column {name}"),
            );
            let t = Rect::from_min_size(
                Pos2::new(r.min.x + 4.0, r.center().y - line / 2.0),
                Vec2::new(r.width() - 8.0, line),
            );
            text::draw_ellipsized(&clip, t, &name, font, BLACK);
            refocus |= response.clicked() || response.secondary_clicked();
            if response.clicked() {
                self.commit();
                let col = match c {
                    Col::Data(j) => Some(j),
                    _ => None,
                };
                let ascending = !matches!(self.sort, Some((s, true)) if s == col);
                self.sorting = Some(self.grid.sort_job(col, ascending));
                self.step_sort();
                focus.request_focus();
            }
            if response.secondary_clicked()
                && let (Col::Data(j), false) = (c, pie)
            {
                self.commit();
                self.menu = Some((j, pointer.unwrap_or(r.left_bottom())));
            }
        }
        clip.rect_filled(corner, 0.0, CONTROL);
        clip.rect_filled(
            Rect::from_min_max(Pos2::new(corner.max.x - 1.0, corner.min.y), corner.max),
            0.0,
            GRID_LINE,
        );
        clip.rect_filled(
            Rect::from_min_max(Pos2::new(corner.min.x, corner.max.y - 1.0), corner.max),
            0.0,
            GRID_LINE,
        );
        // Rows.
        let rows_area = Rect::from_min_max(Pos2::new(view.min.x, view.min.y + hh), view.max);
        let rows_clip = painter.with_clip_rect(rows_area.intersect(painter.clip_rect()));
        let first = (sy / rh as f64).floor().max(0.0) as usize;
        let last = ((sy + rows_area.height() as f64) / rh as f64).ceil() as usize;
        let row_at = |y: f32| ((y - rows_area.min.y) as f64 + sy).div_euclid(rh as f64) as i64;
        let edit_id = grid_id.with("edit");
        for row in first..=last.min(rows) {
            let y = rows_area.min.y + row as f32 * rh - sy as f32;
            let new = row == rows;
            // Row header: the current row's arrow, the new row's star.
            let head = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(CHART_GRID_ROW_HEADER, rh))
                .intersect(rows_area);
            rows_clip.rect_filled(
                head,
                0.0,
                if self.selected_row == Some(row) {
                    HIGHLIGHT
                } else {
                    CONTROL
                },
            );
            rows_clip.rect_filled(
                Rect::from_min_max(Pos2::new(head.max.x - 1.0, head.min.y), head.max),
                0.0,
                GRID_LINE,
            );
            rows_clip.rect_filled(
                Rect::from_min_max(Pos2::new(head.min.x, head.max.y - 1.0), head.max),
                0.0,
                GRID_LINE,
            );
            let mark = if new {
                "*"
            } else if self.current.0 == row {
                "\u{25B6}"
            } else {
                ""
            };
            text::draw(
                &rows_clip,
                Pos2::new(head.min.x + 14.0, head.center().y - line / 2.0),
                mark,
                font,
                BLACK,
            );
            let response = ui.interact(head, grid_id.with(("row", row)), CLICK);
            node(
                ui,
                &response,
                egui::accesskit::Role::RowHeader,
                &format!("Row {}", row + 1),
            );
            refocus |= response.clicked();
            if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                self.commit();
                refocus = true;
                self.current = (row, Col::Name);
                self.selected_row = (!new).then_some(row);
                if !new {
                    self.press = pointer.map(|p| (row, p, false));
                }
            }
            for &c in &cols {
                let (x, w) = col_rect(c);
                let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, rh))
                    .intersect(region(c))
                    .intersect(rows_area);
                if r.width() <= 0.0 || r.height() <= 0.0 {
                    continue;
                }
                let cell = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, rh));
                let selected = self.selected_row == Some(row)
                    || (self.current == (row, c) && c != Col::Swatch);
                let fill = if selected && self.editing.is_none() {
                    HIGHLIGHT
                } else {
                    WHITE
                };
                rows_clip.rect_filled(r, 0.0, fill);
                rows_clip.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(cell.max.x - 1.0, r.min.y),
                        Pos2::new(cell.max.x, r.max.y),
                    ),
                    0.0,
                    GRID_LINE,
                );
                rows_clip.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(r.min.x, cell.max.y - 1.0),
                        Pos2::new(r.max.x, cell.max.y),
                    ),
                    0.0,
                    GRID_LINE,
                );
                let label = format!("Row {} {}", row + 1, self.header(c));
                let response = ui.interact(r, grid_id.with(("cell", row, c)), CLICK);
                node(ui, &response, egui::accesskit::Role::Cell, &label);
                let text_color = if selected && self.editing.is_none() {
                    WHITE
                } else {
                    BLACK
                };
                match c {
                    Col::Swatch => {
                        let side = CHART_GRID_SWATCH;
                        let b = Rect::from_center_size(cell.center(), Vec2::splat(side));
                        if new {
                            let pts: Vec<Pos2> = CHART_NEW_ROW_ARROW
                                .iter()
                                .map(|&(px_, py)| b.min + Vec2::new(px_, py))
                                .collect();
                            rows_clip.add(egui::Shape::convex_polygon(
                                pts[..4].to_vec(),
                                BLACK,
                                egui::Stroke::NONE,
                            ));
                            rows_clip.add(egui::Shape::convex_polygon(
                                vec![pts[2], pts[3], pts[4]],
                                BLACK,
                                egui::Stroke::NONE,
                            ));
                            rows_clip.add(egui::Shape::convex_polygon(
                                vec![pts[0], pts[1], pts[5], pts[6]],
                                BLACK,
                                egui::Stroke::NONE,
                            ));
                        } else {
                            let rgb = self.grid.color(row);
                            rows_clip.rect_filled(b, 0.0, BLACK);
                            rows_clip.rect_filled(b.shrink(1.0), 0.0, WHITE);
                            if let Some(rgb) = rgb {
                                let c = Color32::from_rgb(
                                    (rgb >> 16) as u8,
                                    (rgb >> 8) as u8,
                                    rgb as u8,
                                );
                                rows_clip.rect_filled(b.shrink(2.0), 0.0, c);
                            }
                        }
                    }
                    _ => {
                        let editing_here = self
                            .editing
                            .as_ref()
                            .is_some_and(|e| e.row == row && e.col == c);
                        if !editing_here {
                            let t = Rect::from_min_size(
                                Pos2::new(r.min.x + 3.0, cell.center().y - line / 2.0),
                                Vec2::new(cell.width() - 6.0, line),
                            );
                            let align = if matches!(c, Col::Data(_)) {
                                Alignment::Right
                            } else {
                                Alignment::Left
                            };
                            let shown = self.text(row, c);
                            text::draw_aligned(
                                &rows_clip.with_clip_rect(r),
                                t,
                                &shown,
                                font,
                                text_color,
                                align,
                            );
                        }
                    }
                }
                refocus |= response.clicked();
                if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
                {
                    if self
                        .editing
                        .as_ref()
                        .is_some_and(|e| e.row != row || e.col != c)
                    {
                        self.commit();
                    }
                    refocus = true;
                    self.current = (row, c);
                    self.selected_row = None;
                    if !new {
                        self.press = pointer.map(|p| (row, p, false));
                    }
                }
                if response.clicked() && c == Col::Swatch && !new && self.press.is_none_or(|p| !p.2)
                {
                    action = Some((row, c, "picker"));
                }
                if response.double_clicked() && c != Col::Swatch && self.editing.is_none() {
                    action = Some((row, c, "edit"));
                }
            }
        }
        // A row dragged beyond the threshold onto another existing row moves there.
        if let Some((from, start, dragging)) = self.press {
            let down = ui.input(|i| i.pointer.primary_down());
            let at = ui.input(|i| i.pointer.hover_pos()).unwrap_or(start);
            let moved = dragging || (at - start).length() > 4.0;
            if down {
                self.press = Some((from, start, moved));
            } else {
                self.press = None;
                let to = row_at(at.y);
                if moved && to >= 0 && (to as usize) < self.grid.rows() && to as usize != from {
                    self.grid.move_row(from, to as usize);
                    self.current = (to as usize, self.current.1);
                    self.selected_row = Some(to as usize);
                    self.edited();
                }
            }
        }
        if refocus && self.editing.is_none() {
            focus.request_focus();
        }
        match action {
            Some((row, _, "picker")) => self.open_picker(row),
            Some((_, _, "edit")) => self.start_edit(None),
            _ => {}
        }
        // The cell editor over its cell.
        let cell_rect = self.editing.as_ref().map(|e| {
            let (x, w) = col_rect(e.col);
            let y = rows_area.min.y + e.row as f32 * rh - sy as f32;
            Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, rh)).intersect(region(e.col))
        });
        if let (Some(e), Some(r)) = (&mut self.editing, cell_rect) {
            let inner = r.shrink(2.0);
            let align = if matches!(e.col, Col::Data(_)) {
                egui::Align::RIGHT
            } else {
                egui::Align::LEFT
            };
            let response = ui.put(
                Rect::from_min_max(
                    Pos2::new(inner.min.x, r.center().y - line / 2.0),
                    Pos2::new(inner.max.x, r.center().y + line / 2.0),
                ),
                egui::TextEdit::singleline(&mut Utf16Text {
                    text: &mut e.text,
                    max_units: CHART_MAX_STRING_UTF16,
                })
                .id(edit_id)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .align(egui::Align2([align, egui::Align::Center]))
                .font(egui::FontId::proportional(FONT))
                .text_color(BLACK),
            );
            if e.focus {
                e.focus = false;
                response.request_focus();
                let end = egui::text::CCursor::new(e.text.chars().count());
                let mut state =
                    egui::text_edit::TextEditState::load(&ctx, edit_id).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(&ctx, edit_id);
            } else if !response.has_focus() && !ctx.memory(|m| m.has_focus(grid_id)) {
                // Focus left the grid (another control): the edit ends.
                self.commit();
            }
        }
        #[cfg(test)]
        {
            self.life.borrow_mut().last_grid = Some(self.grid.clone());
        }
    }
}

impl Dialog for ChartEditor {
    fn title(&self) -> String {
        EDITOR_TITLE.into()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        // Escape cancels a running sort, else the dialog, unless it ends a cell edit or closes
        // the column menu.
        let escape = ui.input(|i| i.key_pressed(Key::Escape))
            && self.editing.is_none()
            && self.menu.is_none();
        if escape && self.sorting.take().is_some() {
            self.seat = true;
            ui.ctx().request_repaint();
        } else if self.alive.upgrade().is_none() || escape {
            return DialogState::Closed(DialogResult::Cancel);
        }
        self.poll_children();
        if self.step_sort() {
            ui.ctx().request_repaint();
        }
        let ctx = ui.ctx().clone();
        let chrome = Vec2::new(2.0 * DIALOG_BORDER, DIALOG_TITLE_BAR + 2.0 * DIALOG_BORDER);
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        let client = Vec2::from(CHART_EDITOR_CLIENT);
        if state.min_size.is_none() {
            // The 550 × 501 minimum window is the initial client plus native chrome.
            state.min_size = Some(client + chrome);
            state.size = Some(self.life.borrow().editor_size.unwrap_or(client) + chrome);
            state.set(&ctx, ui.scope_id());
        }
        let mut outcome = None;
        let mut body = |ui: &mut Ui, r: Rect| outcome = self.body(ui, r);
        let frame = dialog_frame(
            ui,
            EDITOR_TITLE,
            None,
            (
                Vec2::new(client.x + 2.0 * DIALOG_BORDER, client.y),
                &mut body,
            ),
            None,
        );
        self.life.borrow_mut().editor_size = Some(frame.body.size());
        self.seat |= cx.seat_focus;
        let result = if frame.closed {
            Some(DialogResult::Cancel)
        } else {
            outcome
        };
        match result {
            Some(r) => {
                #[cfg(test)]
                {
                    self.life.borrow_mut().last_grid = None;
                }
                DialogState::Closed(r)
            }
            None => DialogState::Open,
        }
    }
}

/// The column-rename box ("Column Rename"): prompt, text box, OK/Cancel; Enter = OK, Esc = Cancel.
struct RenameBox {
    text: String,
    result: Rc<RefCell<Option<String>>>,
    alive: Weak<()>,
    seated: bool,
}

impl Dialog for RenameBox {
    fn title(&self) -> String {
        "Column Rename".into()
    }

    fn show(&mut self, ui: &mut Ui, _cx: &mut DialogCx) -> DialogState {
        if self.alive.upgrade().is_none() || ui.input(|i| i.key_pressed(Key::Escape)) {
            return DialogState::Closed(DialogResult::Cancel);
        }
        let enter = ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::Enter));
        let mut outcome = None;
        let seated = &mut self.seated;
        let text = &mut self.text;
        let mut body = |ui: &mut Ui, r: Rect| {
            ui.painter().rect_filled(r, 0.0, CONTROL);
            label(
                ui,
                r.min + Vec2::from(CHART_RENAME_PROMPT),
                "New name for column:",
            );
            let (_, field) = text_box(ui, rect_at(r.min, CHART_RENAME_BOX), "New name", text);
            if !*seated {
                *seated = true;
                field.request_focus();
            }
            if button(ui, rect_at(r.min, CHART_RENAME_OK), None, "OK") || enter {
                outcome = Some(DialogResult::Ok);
            }
            if button(ui, rect_at(r.min, CHART_RENAME_CANCEL), None, "Cancel") {
                outcome = Some(DialogResult::Cancel);
            }
        };
        let client = Vec2::from(CHART_RENAME_CLIENT);
        let frame = dialog_frame(
            ui,
            "Column Rename",
            None,
            (
                Vec2::new(client.x + 2.0 * DIALOG_BORDER, client.y),
                &mut body,
            ),
            None,
        );
        match (outcome, frame.closed) {
            (Some(DialogResult::Ok), _) => {
                *self.result.borrow_mut() = Some(self.text.clone());
                DialogState::Closed(DialogResult::Ok)
            }
            (Some(r), _) => DialogState::Closed(r),
            (None, true) => DialogState::Closed(DialogResult::Cancel),
            (None, false) => DialogState::Open,
        }
    }
}

/// What Window 3's pointer is dragging.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    Wheel,
    Strip(usize),
}

/// Window 3, "Choose a Color": the HSV wheel, RGB/HSV strips and boxes, hex, eight wells.
/// The colour lives as R144's HSV; RGB is derived for display and on OK (one round trip).
struct ColorChooser {
    hsv: PickerHsv,
    hex: String,
    hex_focused: bool,
    life: Rc<RefCell<Life>>,
    alive: Weak<()>,
    result: Rc<RefCell<Option<u32>>>,
    drag: Option<Drag>,
    wheel: Option<TextureHandle>,
}

impl ColorChooser {
    fn new(
        rgb: u32,
        life: Rc<RefCell<Life>>,
        alive: Weak<()>,
        result: Rc<RefCell<Option<u32>>>,
    ) -> Self {
        ColorChooser {
            hsv: PickerHsv::from_rgb(rgb, 0.0),
            hex: String::new(),
            hex_focused: false,
            life,
            alive,
            result,
            drag: None,
            wheel: None,
        }
    }

    fn set_rgb(&mut self, rgb: u32) {
        self.hsv = PickerHsv::from_rgb(rgb, self.hsv.hue);
    }

    /// The slider/box value of row `i` (R, G, B, H, S, V) and its maximum.
    fn value(&self, i: usize) -> (f64, f64) {
        let rgb = self.hsv.to_rgb();
        match i {
            0..=2 => (f64::from(rgb >> (16 - 8 * i) & 255), 255.0),
            3 => (self.hsv.degrees(), 360.0),
            4 => (self.hsv.s * 100.0, 100.0),
            _ => (self.hsv.v * 100.0, 100.0),
        }
    }

    /// An edited box or strip value; an RGB channel that stays the same changes nothing.
    fn set_value(&mut self, i: usize, v: f64) {
        let (current, max) = self.value(i);
        let v = v.clamp(0.0, max);
        if v == current || (i < 3 && v as u32 == current as u32) {
            return;
        }
        match i {
            0..=2 => {
                let shift = 16 - 8 * i;
                let rgb = self.hsv.to_rgb() & !(255 << shift) | (v as u32) << shift;
                self.set_rgb(rgb);
            }
            3 => self.hsv = self.hsv.with_degrees(v),
            4 => self.hsv.s = v / 100.0,
            _ => self.hsv.v = v / 100.0,
        }
    }

    /// "#" + the hex box's text: six hex digits set the colour; anything else reverts. The
    /// current colour's own text changes nothing (no second round trip).
    // spec-question: Q97 Chart picker hex text forms
    fn apply_hex(&mut self) {
        if self.hex.len() == 6
            && let Ok(rgb) = u32::from_str_radix(&self.hex, 16)
            && self.hex.chars().all(|c| c.is_ascii_hexdigit())
            && rgb != self.hsv.to_rgb()
        {
            self.set_rgb(rgb);
        }
        self.hex = format!("{:06X}", self.hsv.to_rgb());
    }

    /// A strip's colour at t ∈ 0…1 (R144's state, displayed with ordinary rounding).
    fn strip_color(&self, i: usize, t: f64) -> Color32 {
        let rgb = self.hsv.to_rgb();
        let [r, g, b] = [rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255].map(|c| c as u8);
        let full = (t * 255.0).round() as u8;
        let hsv = |h: f64, s: f64, v: f64| {
            let c = PickerHsv { hue: 0.0, s, v }.with_degrees(h).to_rgb();
            Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
        };
        match i {
            0 => Color32::from_rgb(full, g, b),
            1 => Color32::from_rgb(r, full, b),
            2 => Color32::from_rgb(r, g, full),
            3 => hsv(t * 360.0, 1.0, 1.0),
            4 => hsv(self.hsv.degrees(), t, self.hsv.v),
            _ => hsv(self.hsv.degrees(), self.hsv.s, t),
        }
    }

    fn wheel_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        let size = CHART_PICKER_WHEEL[2] as usize;
        self.wheel
            .get_or_insert_with(|| {
                let (c, radius) = (size as f64 / 2.0, CHART_PICKER_DISC as f64 / 2.0);
                let mut px_ = vec![Color32::TRANSPARENT; size * size];
                for y in 0..size {
                    for x in 0..size {
                        let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
                        let d = (dx * dx + dy * dy).sqrt();
                        let cover = (radius + 0.5 - d).clamp(0.0, 1.0);
                        if cover <= 0.0 {
                            continue;
                        }
                        // Clockwise from 3 o'clock (y grows downwards).
                        let h = dy.atan2(dx).to_degrees().rem_euclid(360.0);
                        let rgb = PickerHsv {
                            hue: 0.0,
                            s: (d / radius).min(1.0),
                            v: 1.0,
                        }
                        .with_degrees(h)
                        .to_rgb();
                        let rim = (d - (radius - 2.0)).clamp(0.0, 1.0);
                        let mix =
                            |a: u32, b: u8| (a as f64 * (1.0 - rim) + b as f64 * rim).round() as u8;
                        let a = (cover * 255.0).round() as u8;
                        px_[y * size + x] = Color32::from_rgba_unmultiplied(
                            mix(rgb >> 16 & 255, 0xF0),
                            mix(rgb >> 8 & 255, 0xF0),
                            mix(rgb & 255, 0xF0),
                            a,
                        );
                    }
                }
                let image = ColorImage::new([size, size], px_);
                ctx.load_texture("chart-picker-wheel", image, egui::TextureOptions::LINEAR)
            })
            .id()
    }

    fn body(&mut self, ui: &mut Ui, r: Rect) -> Option<DialogResult> {
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        // White to #F4F2F2 by y = 32, then flat.
        let end = Color32::from_rgb(0xF4, 0xF2, 0xF2);
        painter.rect_filled(r, 0.0, end);
        let steps = CHART_PICKER_GRADIENT_END as usize;
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            let c = Color32::from_rgb(
                (255.0 + (244.0 - 255.0) * t) as u8,
                (255.0 + (242.0 - 255.0) * t) as u8,
                (255.0 + (242.0 - 255.0) * t) as u8,
            );
            let row =
                Rect::from_min_size(r.min + Vec2::new(0.0, k as f32), Vec2::new(r.width(), 1.0));
            painter.rect_filled(row, 0.0, c);
        }
        let p = r.min + Vec2::from(CHART_PICKER_PANEL);
        let mut outcome = None;
        let rgb = self.hsv.to_rgb();
        let colour = |rgb: u32| Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        // The wheel, its marker, the current-colour swatch.
        let wb = rect_at(p, CHART_PICKER_WHEEL);
        let texture = self.wheel_texture(&ctx);
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(texture, wb, uv, WHITE);
        let wheel = ui.interact(wb, ui.scope_id().with("wheel"), Sense::click_and_drag());
        node(ui, &wheel, egui::accesskit::Role::ColorWell, "Color wheel");
        if wheel.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            self.drag = Some(Drag::Wheel);
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if self.drag == Some(Drag::Wheel)
            && ui.input(|i| i.pointer.primary_down())
            && let Some(q) = pointer
        {
            let d = q - (wb.min + Vec2::from(CHART_PICKER_POINTER));
            let h = (d.y as f64)
                .atan2(d.x as f64)
                .to_degrees()
                .rem_euclid(360.0);
            let s = (d.length() as f64 / CHART_PICKER_POINTER.0 as f64).min(1.0);
            self.hsv = PickerHsv {
                hue: 0.0,
                s,
                v: 1.0,
            }
            .with_degrees(h);
        }
        let centre = wb.min + Vec2::splat(CHART_PICKER_WHEEL[2] / 2.0);
        let (side, reach) = CHART_PICKER_MARKER;
        let angle = self.hsv.degrees().to_radians() as f32;
        let m = centre + Vec2::new(angle.cos(), angle.sin()) * (self.hsv.s as f32 * reach);
        let marker = PickerHsv { v: 1.0, ..self.hsv }.to_rgb();
        painter.circle_filled(m, side / 2.0, BLACK);
        painter.circle_filled(m, side / 2.0 - 1.0, WHITE);
        painter.circle_filled(m, side / 2.0 - 2.0, colour(marker));
        let sw = Rect::from_min_size(wb.min, Vec2::splat(CHART_PICKER_SWATCH));
        painter.rect_filled(sw, 0.0, BLACK);
        painter.rect_filled(sw.shrink(1.0), 0.0, WHITE);
        painter.rect_filled(sw.shrink(2.0), 0.0, colour(rgb));
        // The right column: headers, labels, strips, boxes and the hex box.
        let at = |x: f32, y: f32| p + Vec2::new(x, y);
        for ((y, sx, sy), name) in CHART_PICKER_HEADERS.into_iter().zip(["RGB", "HSV"]) {
            label(ui, at(CHART_PICKER_LABEL_X, y), name);
            painter.rect_filled(
                Rect::from_min_size(at(sx, sy), Vec2::new(120.0, 1.0)),
                0.0,
                Color32::from_rgb(0xD0, 0xD0, 0xD0),
            );
        }
        let names = ["R", "G", "B", "H", "S", "V"];
        for (i, &(ly, sy, by)) in CHART_PICKER_ROWS.iter().enumerate() {
            if i == 3 {
                self.hex_ui(ui, p);
            }
            label(ui, at(CHART_PICKER_LABEL_X, ly), &format!("{}:", names[i]));
            let [sx, sw_, sh] = CHART_PICKER_STRIP;
            let strip = Rect::from_min_size(at(sx, sy), Vec2::new(sw_, sh));
            self.strip_ui(ui, strip, i, names[i]);
            let [bx, bw, bh] = CHART_PICKER_BOX;
            let (mut v, max) = self.value(i);
            let before = v;
            let style = NumberStyle {
                decimals: Some(if i < 3 { 0 } else { 1 }),
                ..NumberStyle::DIALOG
            };
            let step = if i < 3 { 1.0 } else { 0.1 };
            inputs::number_input(
                ui,
                ("chart-box", i),
                names[i],
                Rect::from_min_size(at(bx, by), Vec2::new(bw, bh)),
                &mut v,
                0.0,
                max,
                step,
                true,
                style,
            );
            if v.to_bits() != before.to_bits() {
                self.set_value(i, v);
            }
        }
        // Wells, OK, Cancel.
        label(
            ui,
            r.min + Vec2::from(CHART_PICKER_PALETTE_LABEL),
            "Palette: Ctrl+Click to set color",
        );
        let ok_id = ui.scope_id().with(("button", "OK"));
        if button(ui, rect_at(r.min, CHART_PICKER_OK), None, "OK") {
            outcome = Some(DialogResult::Ok);
        }
        if button(ui, rect_at(r.min, CHART_PICKER_CANCEL), None, "Cancel") {
            outcome = Some(DialogResult::Cancel);
        }
        let (x0, y0, w, h) = CHART_PICKER_WELLS;
        for (i, name) in WELL_NAMES.iter().enumerate() {
            let well =
                Rect::from_min_size(r.min + Vec2::new(x0 + i as f32 * w, y0), Vec2::new(w, h));
            let response = ui.interact(well, ui.scope_id().with(("well", i)), Sense::click());
            node(ui, &response, egui::accesskit::Role::Button, name);
            let stored = self.life.borrow().wells[i];
            painter.rect_filled(well, 0.0, BLACK);
            painter.rect_filled(well.shrink(1.0), 0.0, colour(stored));
            // The bevel overlay: light top/left, dark bottom/right.
            let inner = well.shrink(1.0);
            painter.rect_filled(
                Rect::from_min_size(inner.min, Vec2::new(inner.width(), 2.0)),
                0.0,
                Color32::from_white_alpha(90),
            );
            painter.rect_filled(
                Rect::from_min_size(inner.min, Vec2::new(2.0, inner.height())),
                0.0,
                Color32::from_white_alpha(90),
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(inner.min.x, inner.max.y - 2.0), inner.max),
                0.0,
                Color32::from_black_alpha(70),
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(inner.max.x - 2.0, inner.min.y), inner.max),
                0.0,
                Color32::from_black_alpha(70),
            );
            if response.clicked() {
                if ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                    self.life.borrow_mut().wells[i] = self.hsv.to_rgb();
                    ctx.memory_mut(|m| m.request_focus(ok_id));
                } else {
                    self.set_rgb(stored);
                    outcome = Some(DialogResult::Ok);
                }
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.drag = None;
        }
        outcome
    }

    fn hex_ui(&mut self, ui: &mut Ui, p: Pos2) {
        label(
            ui,
            p + Vec2::new(CHART_PICKER_LABEL_X, CHART_PICKER_HEX_LABEL),
            "Hex:",
        );
        let rect = rect_at(p, CHART_PICKER_HEX);
        if !self.hex_focused {
            self.hex = format!("{:06X}", self.hsv.to_rgb());
        }
        let (_, response) = text_box(ui, rect, "Hex", &mut self.hex);
        let focused = response.has_focus();
        if (focused || self.hex_focused) && ui.input(|i| i.key_pressed(Key::Enter)) {
            // Enter applies the box; it does not also accept the dialog.
            let pass = ui.ctx().cumulative_pass_nr();
            ui.ctx()
                .data_mut(|d| d.insert_temp(Id::unique("paintrs.enter_taken"), pass));
            self.apply_hex();
        } else if self.hex_focused && !focused {
            self.apply_hex();
        }
        self.hex_focused = focused;
    }

    /// A slider strip: the gradient track 4 px in, 11 px tall; the marker below at
    /// 4 + value/max·64; a press or drag sets value = x·max/65 − 4, clamped.
    fn strip_ui(&mut self, ui: &mut Ui, strip: Rect, i: usize, name: &str) {
        let response = ui.interact(strip, ui.scope_id().with(("strip", i)), CLICK);
        node(
            ui,
            &response,
            egui::accesskit::Role::Slider,
            &format!("{name} strip"),
        );
        if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            self.drag = Some(Drag::Strip(i));
        }
        let (value, max) = self.value(i);
        if self.drag == Some(Drag::Strip(i))
            && ui.input(|i| i.pointer.primary_down())
            && let Some(q) = ui.input(|i| i.pointer.interact_pos())
        {
            let x = (q.x - strip.min.x) as f64;
            self.set_value(i, x * max / 65.0 - 4.0);
        }
        let painter = ui.painter();
        let track = Rect::from_min_size(strip.min + Vec2::new(4.0, 0.0), Vec2::new(65.0, 11.0));
        for k in 0..65 {
            let col =
                Rect::from_min_size(track.min + Vec2::new(k as f32, 0.0), Vec2::new(1.0, 11.0));
            painter.rect_filled(col, 0.0, self.strip_color(i, k as f64 / 64.0));
        }
        let x = strip.min.x + 4.0 + (value / max * 64.0) as f32;
        let color = if response.hovered() {
            Color32::from_rgb(0, 0, 0xFF)
        } else {
            BLACK
        };
        let tip = Pos2::new(x, strip.min.y + 8.0);
        painter.add(egui::Shape::convex_polygon(
            vec![
                tip,
                Pos2::new(x + 3.5, tip.y + 7.0),
                Pos2::new(x - 3.5, tip.y + 7.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
    }
}

impl Dialog for ColorChooser {
    fn title(&self) -> String {
        "Choose a Color".into()
    }

    fn show(&mut self, ui: &mut Ui, _cx: &mut DialogCx) -> DialogState {
        if self.alive.upgrade().is_none() || ui.input(|i| i.key_pressed(Key::Escape)) {
            return DialogState::Closed(DialogResult::Cancel);
        }
        let mut outcome = None;
        let mut body = |ui: &mut Ui, r: Rect| outcome = self.body(ui, r);
        let client = Vec2::from(CHART_PICKER_CLIENT);
        let frame = dialog_frame(
            ui,
            "Choose a Color",
            None,
            (
                Vec2::new(client.x + 2.0 * DIALOG_BORDER, client.y),
                &mut body,
            ),
            None,
        );
        let enter = ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::Enter))
            && !inputs::enter_taken(ui.ctx());
        if enter && outcome.is_none() {
            outcome = Some(DialogResult::Ok);
        }
        match (outcome, frame.closed) {
            (Some(DialogResult::Ok), _) => {
                *self.result.borrow_mut() = Some(self.hsv.to_rgb());
                DialogState::Closed(DialogResult::Ok)
            }
            (Some(r), _) => DialogState::Closed(r),
            (None, true) => DialogState::Closed(DialogResult::Cancel),
            (None, false) => DialogState::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::effect_config::EffectConfigDialog;
    use crate::dialogs::{DialogCx, DialogStack, DialogState, without_input};
    use crate::platform::{FileSaveOutcome, Files, PickedFile};
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Vec2};
    use pdn_core::app::AppWorkspace;
    use pdn_core::effect::bb::chart::edit::{NamedColors, PickerHsv};
    use pdn_core::effect::bb::chart::model::{self, Chart, ChartType, Point};
    use pdn_core::effect::bb::chart::{render, xml};
    use pdn_core::effect::session::EffectSession;
    use pdn_core::testing::{fresh_doc, layer_bytes};
    use serde_json::Value;

    /// The main dialog over a 400 × 300 document, under a real dialog stack for its windows.
    struct Rig {
        app: AppWorkspace,
        settings: pdn_core::settings::AppSettings,
        prefs: crate::prefs::Prefs,
        ctx: egui::Context,
        main: EffectConfigDialog,
        stack: DialogStack,
        state: DialogState,
        files: Files,
        mods: Modifiers,
        /// The history length when the dialog opened.
        base: usize,
        /// The last frame's accessible nodes: (label, bounds, window in drawing order).
        nodes: Vec<(String, Rect, Option<usize>)>,
        /// The keyboard focus's accessible label, if any.
        focused: Option<String>,
    }

    fn rig(stored: Option<&Chart>) -> Rig {
        let mut app = fresh_doc(400, 300, Some(4));
        let def = pdn_core::effect::find("bbChartOrGraph").unwrap();
        let session = EffectSession::begin(&mut app, def).unwrap();
        let mut values = session.defaults();
        if let Some(c) = stored {
            values.insert("chart".into(), Value::String(model::encode(c).unwrap()));
        }
        let mut main = EffectConfigDialog::new(session, values);
        let files = Files::in_memory();
        main.chart.as_mut().unwrap().files = files.clone();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut r = Rig {
            app,
            settings: pdn_core::settings::AppSettings::load(None),
            prefs: crate::prefs::Prefs::default(),
            ctx,
            main,
            stack: DialogStack::default(),
            state: DialogState::Open,
            files,
            mods: Modifiers::NONE,
            base: 0,
            nodes: vec![],
            focused: None,
        };
        r.base = undo_len(&r.app);
        r.frame(vec![]);
        r.frame(vec![]);
        r
    }

    impl Rig {
        fn frame(&mut self, events: Vec<Event>) {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280., 900.))),
                events: [Event::ModifiersChanged(self.mods)]
                    .into_iter()
                    .chain(events)
                    .collect(),
                ..Default::default()
            };
            self.app.now_ms += 20.0;
            let (main, stack, state) = (&mut self.main, &mut self.stack, &mut self.state);
            let mut cx = DialogCx {
                app: &mut self.app,
                settings: &mut self.settings,
                prefs: &mut self.prefs,
                settings_changed: false,
                shortcuts: None,
                clipboard: None,
                settings_imports: None,
                fonts: None,
                asset_library: None,
                seat_focus: false,
            };
            let mut out = self.ctx.run_ui(raw, |ui| {
                if *state == DialogState::Open {
                    *state = if stack.is_open() {
                        let ctx = ui.ctx().clone();
                        without_input(&ctx, || main.show(ui, &mut cx))
                    } else {
                        main.show(ui, &mut cx)
                    };
                }
                stack.show(ui, &mut cx);
            });
            out.textures_delta.clear();
            // Each node's window: the branch of the harness `Ui` it lies in, in drawing order
            // (the main dialog first, then the stack bottom to top).
            if let Some(update) = out.platform_output.accesskit_update {
                let parent: std::collections::HashMap<_, _> = update
                    .nodes
                    .iter()
                    .flat_map(|(id, n)| n.children().iter().map(move |c| (*c, *id)))
                    .collect();
                let chain = |mut at| {
                    let mut chain = vec![at];
                    while let Some(&p) = parent.get(&at) {
                        chain.push(p);
                        at = p;
                    }
                    chain
                };
                let order: Vec<_> = update
                    .nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some("Enter Data"))
                    .map(|(id, _)| {
                        let c = chain(*id);
                        let harness = c[c.len() - 2];
                        let node = &update.nodes.iter().find(|(i, _)| *i == harness).unwrap().1;
                        node.children().to_vec()
                    })
                    .unwrap_or_default();
                self.focused = update
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == update.focus)
                    .and_then(|(_, n)| n.label().map(str::to_string));
                self.nodes = update
                    .nodes
                    .iter()
                    .filter_map(|(id, n)| {
                        let b = n.bounds()?;
                        let rect = Rect::from_min_max(
                            Pos2::new(b.x0 as f32, b.y0 as f32),
                            Pos2::new(b.x1 as f32, b.y1 as f32),
                        );
                        let c = chain(*id);
                        // Popups (a select's list) lie outside the harness `Ui`: any window's.
                        let window = c
                            .get(c.len().wrapping_sub(3))
                            .and_then(|b| order.iter().position(|o| o == b));
                        Some((n.label()?.to_string(), rect, window))
                    })
                    .collect();
            }
        }

        /// The node with `label` in the top window.
        fn top_nodes(&self) -> impl Iterator<Item = &(String, Rect, Option<usize>)> {
            let top = self.nodes.iter().filter_map(|n| n.2).max();
            self.nodes
                .iter()
                .filter(move |n| n.2.is_none() || n.2 == top)
        }

        fn rect(&self, label: &str) -> Rect {
            self.top_nodes()
                .find(|n| n.0 == label)
                .unwrap_or_else(|| {
                    let all: Vec<_> = self.top_nodes().map(|n| &n.0).collect();
                    panic!("no {label:?} among {all:?}")
                })
                .1
        }

        fn has(&self, label: &str) -> bool {
            self.top_nodes().any(|n| n.0 == label)
        }

        fn press(&mut self, pos: Pos2, button: PointerButton, pressed: bool) {
            let modifiers = self.mods;
            self.frame(vec![Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers,
            }]);
        }

        fn click_at(&mut self, pos: Pos2, button: PointerButton) {
            self.frame(vec![Event::PointerMoved(pos)]);
            self.press(pos, button, true);
            self.press(pos, button, false);
            self.frame(vec![]);
        }

        fn click(&mut self, label: &str) {
            let at = self.rect(label).center();
            self.click_at(at, PointerButton::Primary);
        }

        fn key(&mut self, key: Key) {
            let modifiers = self.mods;
            let event = |pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            };
            self.frame(vec![event(true), event(false)]);
            self.frame(vec![]);
        }

        fn type_text(&mut self, text: &str) {
            self.frame(vec![Event::Text(text.into())]);
            self.frame(vec![]);
        }

        fn chart(&self) -> &ChartDialog {
            self.main.chart.as_ref().unwrap()
        }

        fn stored(&self) -> Chart {
            self.chart().chart.clone()
        }

        fn top(&self, title: &str) -> bool {
            self.stack.top_title_is(title)
        }

        /// Frames until the main dialog closes (the final render runs on a worker).
        fn settle(&mut self) {
            for _ in 0..2000 {
                if self.state != DialogState::Open {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
                self.frame(vec![]);
            }
            panic!("the dialog did not close");
        }

        /// History entries since the dialog opened.
        fn undo(&self) -> usize {
            undo_len(&self.app) - self.base
        }

        fn seed(&self) -> NamedColors {
            self.chart().life.borrow().colors
        }

        fn editor(&mut self) {
            self.click("Enter Data");
            assert!(self.top(EDITOR_TITLE));
        }
    }

    /// Ctrl as egui reports it off macOS.
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };

    fn draws(seed: NamedColors, n: usize) -> Vec<Option<u32>> {
        let mut s = seed;
        (0..n).map(|_| Some(s.draw())).collect()
    }

    fn undo_len(app: &AppWorkspace) -> usize {
        app.active_ws().unwrap().history.undo.len()
    }

    /// render-chart-or-graph.md "On dialog open" (quirk 1 fixed, R143): a stored Pie chart opens
    /// with its type, Key, 3D, titles and Explode selection shown, and no action replayed.
    #[test]
    fn opening_synchronises_the_controls() {
        let mut c = Chart::default();
        (c.title, c.x_title, c.y_title, c.three_d) =
            ("Sales".into(), " x ".into(), "y".into(), true);
        for s in &mut c.series {
            s.chart_type = ChartType::Pie;
            s.legend_visible = true;
            s.value_labels = true;
        }
        c.series[0].points[2].exploded = true;
        let r = rig(Some(&c));
        let d = r.chart();
        assert_eq!(d.chart, c, "no type change, Key or Explode action ran");
        assert_eq!(
            (&*d.title, d.key, d.chart.series[0].chart_type),
            ("Sales", true, ChartType::Pie)
        );
        let names: Vec<_> = d.explode.iter().map(|(n, s)| (n.as_str(), *s)).collect();
        assert_eq!(
            names,
            [("Sunday", false), ("Monday", false), ("Tuesday", true)]
        );
        assert!(d.memory.is_empty());
        assert_eq!(r.seed(), NamedColors(d.seed), "no colour drawn");
        assert_eq!(d.preview_size(), (400, 226));
        assert!(r.has("Explode:") && !r.has("Y-Axis:") && !r.has("X-Axis:"));
        // An empty chart opens with one empty Line series (quirk 8, R143).
        let empty = Chart {
            series: vec![],
            ..Chart::default()
        };
        let r = rig(Some(&empty));
        assert_eq!(r.stored().series.len(), 1);
        assert_eq!(r.chart().preview_size(), (400, 209));
        assert!(r.has("Y-Axis:") && r.has("X-Axis:") && !r.has("Explode:"));
    }

    /// Window 1's layout at the spec's client positions (panel at 12, 12).
    #[test]
    fn main_window_layout() {
        let r = rig(None);
        let client = r.chart().client;
        let at = |label: &str| r.rect(label).min - client.min;
        assert_eq!(client.size(), Vec2::new(484.0, 426.0));
        let near = |a: Vec2, b: Vec2| (a - b).abs().max_elem() <= 0.5;
        for (label, x, y) in [
            ("Enter Data", 246.0, 13.0),
            ("Load Data", 330.0, 13.0),
            ("Type", 44.0, 40.0),
            ("Save Data", 330.0, 39.0),
            ("OK", 316.0, 391.0),
            ("Cancel", 397.0, 391.0),
            ("Title:", 14.0, 18.0),
            ("Type:", 10.0, 44.0),
            ("Y-Axis:", 11.0, 268.0),
            ("X-Axis:", 162.0, 268.0),
        ] {
            assert!(near(at(label), Vec2::new(x, y)), "{label}: {:?}", at(label));
        }
        // Text boxes: the editor starts inside its box, 2 px in from the left.
        for (label, [x, y, w, h]) in [
            ("Title", CHART_TITLE_BOX),
            ("Y-Axis", CHART_Y_BOX),
            ("X-Axis", CHART_X_BOX),
        ] {
            let boxed =
                Rect::from_min_size(client.min + Vec2::new(12.0 + x, 12.0 + y), Vec2::new(w, h));
            let edit = r.rect(label);
            assert!(
                (boxed.min.y..boxed.max.y).contains(&edit.min.y),
                "{label} {edit:?} {boxed:?}"
            );
            assert!(
                (r.rect(label).min.x - boxed.min.x - 2.0).abs() <= 0.5,
                "{label}"
            );
        }
        assert_eq!(r.rect("Type").size(), Vec2::new(196.0, 21.0));
        assert_eq!(r.rect("Enter Data").size(), Vec2::new(78.0, 23.0));
        assert_eq!(r.rect("OK").size(), Vec2::new(75.0, 23.0));
    }

    /// render-chart-or-graph.md Expected results 3 through the dialog: the type drop-down's Pie
    /// and Key give three randomly coloured slices labelled with values, a three-item legend, and
    /// no blue series.
    #[test]
    fn pie_with_key_through_the_dialog() {
        let mut r = rig(None);
        let seed = r.seed();
        r.click("Type");
        r.click("Pie");
        let c = r.stored();
        assert!(c.series.iter().all(|s| s.chart_type == ChartType::Pie));
        let slices: Vec<_> = c.series[0].points.iter().map(|p| p.color).collect();
        assert_eq!(slices, draws(seed, 3), "one named colour per slice");
        assert!(
            c.series
                .iter()
                .all(|s| !s.legend_visible && !s.value_labels)
        );
        assert_eq!(r.chart().preview_size(), (400, 226));
        assert!(r.has("Explode:") && !r.has("Y-Axis:"));
        r.click("Key");
        let c = r.stored();
        assert!(c.series.iter().all(|s| s.legend_visible && s.value_labels));
        r.click("OK");
        r.settle();
        assert_eq!(r.undo(), 1);
        let canvas = layer_bytes(&r.app, None);
        assert_eq!(canvas, render(&c, 400, 300, "en").unwrap().data);
        let mut first_only = c.clone();
        first_only.series.truncate(1);
        assert_eq!(
            canvas,
            render(&first_only, 400, 300, "en").unwrap().data,
            "blue is not drawn"
        );
        let token = &r.app.effect_tokens["bbChartOrGraph"]["chart"];
        assert_eq!(model::decode(token.as_str().unwrap()).unwrap(), c);
    }

    /// render-chart-or-graph.md Expected results 4 through the Explode list: name matching, with
    /// Ctrl/Shift extended selection; no legend, category labels.
    #[test]
    fn explode_list_selects_by_name() {
        let mut c = Chart::default();
        let sunday = c.series[0].points[0].clone();
        c.series[0].points.push(sunday);
        let mut r = rig(Some(&c));
        r.click("Type");
        r.click("Pie");
        let exploded = |r: &Rig| -> Vec<bool> {
            r.stored().series[0]
                .points
                .iter()
                .map(|p| p.exploded)
                .collect()
        };
        r.click("Tuesday");
        assert_eq!(exploded(&r), [false, false, true, false]);
        r.mods = CTRL;
        r.click("Sunday");
        r.mods = Modifiers::NONE;
        assert_eq!(exploded(&r), [true, false, true, true], "both Sundays");
        r.click("Monday");
        assert_eq!(exploded(&r), [false, true, false, false]);
        r.mods = Modifiers::SHIFT;
        r.click("Tuesday");
        r.mods = Modifiers::NONE;
        assert_eq!(exploded(&r), [false, true, true, false]);
        let c = r.stored();
        assert!(!c.series[0].legend_visible && !c.series[0].value_labels);
        // Back to an axis type and Pie again: the list is refilled, its selection cleared.
        r.click("Type");
        r.click("Column");
        assert!(!r.has("Explode:"));
        r.click("Type");
        r.click("Doughnut");
        assert!(r.chart().explode.iter().all(|e| !e.1) && exploded(&r).iter().all(|e| !e));
    }

    /// Chart-sync review Minor 2: Window 1's text boxes are single-line boxes that leave Enter to
    /// the accept button, so Enter in one submits the dialog (OK) with the typed text; a focused
    /// push button (Enter Data) takes Enter for itself (`enter_taken`) and does not submit.
    #[test]
    fn enter_in_a_window_1_field_submits_a_focused_button_takes_it() {
        let mut r = rig(None);
        r.click("Title");
        assert_eq!(r.focused.as_deref(), Some("Title"));
        r.key(Key::Tab);
        assert_eq!(r.focused.as_deref(), Some("Enter Data"));
        r.key(Key::Enter);
        assert!(r.top(EDITOR_TITLE), "the button's own action");
        assert_eq!(r.state, DialogState::Open);
        r.click("Cancel");
        assert!(!r.top(EDITOR_TITLE));
        r.click("Title");
        r.type_text("Sales");
        r.key(Key::Enter);
        r.settle();
        assert_eq!(r.state, DialogState::Closed(DialogResult::Ok));
        assert_eq!(r.undo(), 1);
        assert_eq!(r.stored().title, "Sales");
    }

    /// Enter Data: OK replaces the chart and re-applies Key and the Explode names (quirk 2
    /// fixed); Cancel discards; a new row is materialized once with a named colour.
    #[test]
    fn enter_data_accepts_or_discards() {
        let mut r = rig(None);
        let seed = r.seed();
        r.editor();
        r.click(&cell(3, "Name"));
        r.type_text("Extra");
        r.key(Key::Enter);
        r.click("Cancel");
        assert!(!r.top(EDITOR_TITLE));
        assert_eq!(r.stored(), Chart::default(), "Cancel discards the edits");
        r.editor();
        r.click(&cell(3, "Name"));
        r.type_text("Extra");
        r.key(Key::Enter);
        r.click("OK");
        assert!(!r.top(EDITOR_TITLE));
        let c = r.stored();
        assert_eq!(c.series.len(), 3);
        let s = &c.series[2];
        assert_eq!(
            (&*s.name, s.points.len(), s.legend_visible),
            ("Extra", 4, false)
        );
        assert_eq!(
            s.color,
            draws(seed, 2)[1],
            "the cancelled row drew the first colour"
        );
        // Pie-like editing renames a slice: the Explode list follows the accepted names.
        r.click("Type");
        r.click("Pie");
        r.click("Key");
        r.editor();
        r.click(&cell(1, "Name"));
        r.type_text("Lundi");
        r.key(Key::Enter);
        r.click("OK");
        let names: Vec<_> = r.chart().explode.iter().map(|e| e.0.clone()).collect();
        assert_eq!(names, ["Lundi", "Monday", "Tuesday"]);
        assert!(r.stored().series.iter().all(|s| s.legend_visible));
    }

    fn cell(row: usize, column: &str) -> String {
        format!("Row {row} {column}")
    }

    /// render-chart-or-graph.md Expected results 6: "abc" in Red Series' Monday cell, Enter: the
    /// cell shows 10 again, nothing changes, no message.
    #[test]
    fn invalid_number_reverts() {
        let mut r = rig(None);
        r.editor();
        r.click(&cell(1, "Monday"));
        r.type_text("abc");
        r.key(Key::Enter);
        assert!(r.top(EDITOR_TITLE), "no error box");
        r.click(&cell(1, "Monday"));
        r.type_text("12,5");
        r.key(Key::Enter);
        r.click("OK");
        let c = r.stored();
        assert_eq!(c.series[0].points[1].value, 125.0, "en groups 12,5");
        let mut r = rig(None);
        r.editor();
        r.click(&cell(1, "Monday"));
        r.type_text("abc");
        r.key(Key::Enter);
        assert_eq!(r.editor_cell(0, 1), "10");
        r.click("OK");
        assert_eq!(r.stored(), Chart::default());
    }

    impl Rig {
        fn editor_cell(&self, row: usize, col: usize) -> String {
            self.chart()
                .life
                .borrow()
                .last_grid
                .as_ref()
                .unwrap()
                .cell(row, col)
        }
    }

    /// render-chart-or-graph.md Expected results 5: the first row's swatch, the Yellow well:
    /// the series and its points become FFFE00; one history entry after OK.
    #[test]
    fn yellow_well_through_the_picker() {
        let mut r = rig(None);
        r.editor();
        r.click(&cell(1, "Swatch"));
        assert!(r.top("Choose a Color"));
        r.click("Yellow");
        assert!(r.top(EDITOR_TITLE));
        r.click("OK");
        let c = r.stored();
        assert_eq!(c.series[0].color, Some(0xFFFE00));
        assert!(c.series[0].points.iter().all(|p| p.color == Some(0xFFFE00)));
        assert_eq!(c.series[1], Chart::default().series[1]);
        r.click("OK");
        r.settle();
        assert_eq!(r.undo(), 1);
    }

    /// Window 3: Cancel keeps the row; OK without edits returns the truncating round trip;
    /// Ctrl+Click stores the current colour into a well (kept for the main dialog's life);
    /// the hex box applies on Enter; a strip sets value = x · max / 65 − 4.
    #[test]
    fn picker_controls() {
        let mut r = rig(None);
        r.editor();
        r.click(&cell(2, "Swatch"));
        r.click("Cancel");
        assert!(r.top(EDITOR_TITLE));
        assert_eq!(
            r.chart().life.borrow().last_grid.as_ref().unwrap().color(1),
            Some(0x1E90FF)
        );
        // Orange through the hex box, then OK: FFA400.
        r.click(&cell(2, "Swatch"));
        r.click("Hex");
        r.key(Key::A);
        r.mods = CTRL;
        r.key(Key::A);
        r.mods = Modifiers::NONE;
        r.type_text("FFA500");
        r.key(Key::Enter);
        assert!(
            r.top("Choose a Color"),
            "Enter in the hex box only applies it"
        );
        // Ctrl+Click stores it into the first well, without closing.
        r.mods = CTRL;
        r.click("Black");
        r.mods = Modifiers::NONE;
        assert!(r.top("Choose a Color"));
        assert_eq!(r.chart().life.borrow().wells[0], 0xFFA400);
        r.click("OK");
        let grid = r.chart().life.borrow().last_grid.clone().unwrap();
        assert_eq!(grid.color(1), Some(0xFFA400));
        // The stored well survives a new picker; clicking it picks through one more round trip
        // (FFA400 → FFA300) and closes.
        r.click(&cell(1, "Swatch"));
        r.click("Black");
        let grid = r.chart().life.borrow().last_grid.clone().unwrap();
        assert_eq!(grid.color(0), Some(0xFFA300));
        // The R strip: x = 33 px into the strip gives 33 · 255 / 65 − 4 = 125.46 → 125.
        r.click(&cell(1, "Swatch"));
        let strip = r.rect("R strip");
        r.click_at(strip.min + Vec2::new(33.0, 7.0), PointerButton::Primary);
        r.click("OK");
        let grid = r.chart().life.borrow().last_grid.clone().unwrap();
        let want = PickerHsv::from_rgb(0x7DA300, 0.0).to_rgb();
        assert_eq!((grid.color(0), want >> 16), (Some(want), 125));
    }

    /// The column menu: Rename (the Column Rename box), Delete (the "Chart" question), Add
    /// Column to Left; never on Name or in pie-like mode.
    #[test]
    fn column_menu() {
        let mut r = rig(None);
        r.editor();
        let header = r.rect("Column Monday").center();
        r.click_at(header, PointerButton::Secondary);
        for item in [
            "Rename",
            "Delete",
            "Add Column to Left",
            "Add Column to Right",
        ] {
            assert!(r.has(item), "{item}");
        }
        r.click("Rename");
        assert!(r.top("Column Rename"));
        r.mods = CTRL;
        r.key(Key::A);
        r.mods = Modifiers::NONE;
        r.type_text("Lundi");
        r.key(Key::Enter);
        assert!(r.top(EDITOR_TITLE));
        let header = r.rect("Column Lundi").center();
        r.click_at(header, PointerButton::Secondary);
        r.click("Add Column to Left");
        let header = r.rect("Column Sunday").center();
        r.click_at(header, PointerButton::Secondary);
        r.click("Delete");
        assert!(r.top("Chart"));
        r.click("No");
        assert!(r.has("Column Sunday"));
        r.click_at(header, PointerButton::Secondary);
        r.click("Delete");
        r.click("Yes");
        r.click("OK");
        let c = r.stored();
        let red: Vec<_> = c.series[0]
            .points
            .iter()
            .map(|p| (p.x, p.category.as_str()))
            .collect();
        assert_eq!(red, [(1.0, "Untitled"), (2.0, "Lundi"), (3.0, "Tuesday")]);
        // No menu on Name.
        r.editor();
        let header = r.rect("Column Name").center();
        r.click_at(header, PointerButton::Secondary);
        assert!(!r.has("Rename"));
    }

    /// Rows: Delete removes the selected series but never the last (quirk 8); dragging and
    /// sorting move the series with their rows (quirk 4 fixed).
    #[test]
    fn rows_delete_drag_and_sort() {
        let mut r = rig(None);
        r.editor();
        r.click("Row 2");
        r.key(Key::Delete);
        r.click("Row 1");
        r.key(Key::Delete);
        r.click("OK");
        assert_eq!(r.stored().series.len(), 1);
        let mut r = rig(None);
        r.editor();
        let (from, to) = (r.rect("Row 1").center(), r.rect("Row 2").center());
        r.frame(vec![Event::PointerMoved(from)]);
        r.press(from, PointerButton::Primary, true);
        r.frame(vec![Event::PointerMoved(from + Vec2::new(0.0, 6.0))]);
        r.frame(vec![Event::PointerMoved(to)]);
        r.press(to, PointerButton::Primary, false);
        r.frame(vec![]);
        r.click("Column Monday");
        r.click("OK");
        let names: Vec<_> = r.stored().series.iter().map(|s| s.name.clone()).collect();
        assert_eq!(
            names,
            ["Red Series", "Blue Series"],
            "dragged, then sorted 10 < 70"
        );
        let mut r = rig(None);
        r.editor();
        r.click(&cell(1, "Monday"));
        r.click("Column Monday");
        r.click("OK");
        assert_eq!(r.stored().series[0].name, "Red Series");
    }

    /// Save Data and Load Data: the XML file, "Save Error"/"Load Error" boxes, synchronised
    /// controls on success and no change on failure.
    #[test]
    fn save_and_load_data() {
        let mut r = rig(None);
        r.click("Save Data");
        let saved = r.files.take_saved();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, "UntitledChart");
        assert_eq!(
            saved[0].1,
            xml::to_xml(&Chart::default()).unwrap().into_bytes()
        );
        assert!(!r.stack.is_open(), "a cancelled save is quiet");
        r.files.queue_save(Err("disk full".into()));
        r.click("Save Data");
        assert!(r.top("Save Error"));
        r.key(Key::Enter);
        assert!(!r.stack.is_open());
        r.files.queue_save(Ok(FileSaveOutcome::DownloadStarted));
        r.click("Save Data");
        assert!(!r.stack.is_open());
        // Load: a pie chart with Key and a title; the controls follow without actions.
        let mut c = Chart {
            title: "T".into(),
            ..Chart::default()
        };
        for s in &mut c.series {
            s.chart_type = ChartType::Doughnut;
            s.legend_visible = true;
        }
        let file = |bytes: &[u8]| Ok(vec![PickedFile::snapshot("c.xml".into(), bytes.to_vec())]);
        r.files
            .queue_pick(file(xml::to_xml(&c).unwrap().as_bytes()));
        r.click("Load Data");
        assert_eq!(r.stored(), c);
        assert_eq!((&*r.chart().title, r.chart().key), ("T", true));
        assert_eq!(r.chart().preview_size(), (400, 226));
        // Failures: nothing changes.
        for bad in [&b"<Chart"[..], b"\xff\xfe", b"<Chart title='' xTitle='' yTitle='' threeD='false'><Series name='' type='Kagi' legend='false' labels='false'/></Chart>"] {
            r.files.queue_pick(file(bad));
            r.click("Load Data");
            assert!(r.top("Load Error"), "{bad:?}");
            r.key(Key::Enter);
            assert_eq!(r.stored(), c);
        }
        // Over the chart limit: the read's own too-large text, as on every platform path.
        r.files
            .queue_pick(file(&vec![b' '; CHART_MAX_ENCODED_BYTES + 1]));
        r.click("Load Data");
        assert!(r.top("Load Error"));
        assert_eq!(
            r.stack.entries.last().unwrap().dialog.message().as_deref(),
            Some(crate::platform::files::FILE_TOO_LARGE)
        );
        r.key(Key::Enter);
        assert_eq!(r.stored(), c);
        // A cancelled chooser changes nothing either.
        r.files.queue_pick(Ok(vec![]));
        r.click("Load Data");
        assert!(!r.stack.is_open() && r.stored() == c);
        r.click("Cancel");
        assert_eq!(
            r.state,
            DialogState::Closed(crate::dialogs::DialogResult::Cancel)
        );
        assert_eq!(r.undo(), 0, "Save and Load never commit");
    }

    /// OK commits once; Cancel restores; Repeat re-renders the stored chart at the new
    /// selection with its stored colours.
    #[test]
    fn commit_cancel_and_repeat() {
        let mut r = rig(None);
        let before = layer_bytes(&fresh_doc(400, 300, Some(4)), None);
        r.click("3D");
        r.settle_preview();
        assert_ne!(layer_bytes(&r.app, None), before, "the canvas previews");
        r.click("Cancel");
        assert_eq!(layer_bytes(&r.app, None), before);
        assert_eq!(r.undo(), 0);
        let mut r = rig(None);
        r.click("Type");
        r.click("Funnel");
        let c = r.stored();
        r.key(Key::Enter);
        r.settle();
        assert_eq!(r.undo(), 1);
        let ws = r.app.active_ws_mut().unwrap();
        let rect = pdn_core::util::geom::Rectangle::new(50.0, 50.0, 200.0, 100.0);
        ws.selection
            .set_continuation(&rect, pdn_core::selection::CombineMode::Replace);
        ws.selection.commit_continuation();
        assert!(pdn_core::effect::session::repeat_last(&mut r.app));
        assert_eq!(r.undo(), 2);
        let token = &r.app.effect_tokens["bbChartOrGraph"]["chart"];
        assert_eq!(
            model::decode(token.as_str().unwrap()).unwrap(),
            c,
            "stored colours kept"
        );
        let after = layer_bytes(&r.app, None);
        let crop = render(&c, 200, 100, "en").unwrap();
        let row = |y: usize| &after[((50 + y) * 400 + 50) * 4..((50 + y) * 400 + 250) * 4];
        assert_eq!(row(37), &crop.data[37 * 800..38 * 800]);
    }

    impl Rig {
        fn settle_preview(&mut self) {
            for _ in 0..2000 {
                if self.main.preview_current() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
                self.frame(vec![]);
            }
            panic!("no preview");
        }
    }

    /// R149 Q47, R199: a long sort runs over frames with a visible Cancel Sort that keeps the
    /// rows as they were; a short one finishes at once.
    #[test]
    fn sorting_runs_over_frames_and_cancels() {
        let mut c = Chart::default();
        c.series.truncate(1);
        c.series[0].chart_type = ChartType::Funnel;
        let prefix = "q".repeat(120);
        c.series[0].points = (0..4096)
            .map(|i| Point {
                category: format!("{prefix}{:05}", 4095 - i),
                ..c.series[0].points[0].clone()
            })
            .collect();
        let mut r = rig(Some(&c));
        r.editor();
        let names = |r: &Rig| -> Vec<String> {
            let life = r.chart().life.borrow();
            let g = life.last_grid.as_ref().unwrap();
            (0..g.rows()).map(|i| g.name(i).to_string()).collect()
        };
        let before = names(&r);
        r.click("Column Name");
        for _ in 0..3 {
            assert!(r.has("Cancel Sort"), "still sorting, frames keep coming");
            r.frame(vec![]);
        }
        r.click("Cancel Sort");
        assert!(!r.has("Cancel Sort"));
        assert_eq!(names(&r), before, "Cancel keeps the pre-sort order");
        // The grid has the keyboard again: F2 edits the current cell.
        r.frame(vec![]);
        assert_eq!(r.focused.as_deref(), Some("Chart data"));
        r.key(Key::F2);
        r.type_text("Z");
        r.key(Key::Enter);
        assert_eq!(names(&r)[0], format!("{}Z", before[0]));
        r.click("OK");
        let mut want = c.clone();
        want.series[0].points[0].category.push('Z');
        assert_eq!(
            r.stored(),
            want,
            "the rows in their old order, the edit kept"
        );
    }

    /// R143/R145: a 65th row is refused with a message, an over-long paste is cut at 32 767
    /// UTF-16 units, an edit past the text limit rolls back; `values["chart"]` always holds a
    /// valid chart, never "".
    #[test]
    fn over_limit_edits_never_store_an_empty_token() {
        use pdn_core::effect::bb::conventions::{
            CHART_MAX_SERIES, CHART_MAX_STRING_UTF16, CHART_MAX_TEXT_BYTES,
        };
        let token = |r: &Rig| {
            let v = r.main.value("chart").unwrap().as_str().unwrap().to_string();
            model::decode(&v).expect("a valid token")
        };
        let mut c = Chart::default();
        c.series = (0..CHART_MAX_SERIES)
            .map(|i| model::Series {
                name: format!("S{i}"),
                ..c.series[1].clone()
            })
            .collect();
        let mut r = rig(Some(&c));
        r.editor();
        r.click(&cell(1, "Name"));
        for _ in 0..CHART_MAX_SERIES {
            r.key(Key::ArrowDown);
        }
        assert!(
            r.has(&cell(CHART_MAX_SERIES + 1, "Name")),
            "the new row scrolled into view"
        );
        r.type_text("X");
        r.key(Key::Enter);
        assert!(r.top("Chart"), "the refusal is shown");
        r.key(Key::Enter);
        // An over-long paste into a name is cut at the limit.
        assert!(
            r.has(&cell(CHART_MAX_SERIES + 1, "Name"))
                && !r.has(&cell(CHART_MAX_SERIES + 2, "Name"))
        );
        r.click(&cell(CHART_MAX_SERIES + 1, "Name"));
        for _ in 0..CHART_MAX_SERIES {
            r.key(Key::ArrowUp);
        }
        r.click(&cell(1, "Name"));
        r.key(Key::F2);
        r.mods = CTRL;
        r.key(Key::A);
        r.mods = Modifiers::NONE;
        r.frame(vec![Event::Paste("p".repeat(CHART_MAX_STRING_UTF16 + 100))]);
        r.key(Key::Enter);
        r.click("OK");
        let t = token(&r);
        assert_eq!(t.series.len(), CHART_MAX_SERIES);
        assert_eq!(t.series[0].name, "p".repeat(CHART_MAX_STRING_UTF16));
        // At the aggregate text limit, one more title character rolls back.
        let mut c = Chart::default();
        c.series = (0..64)
            .map(|i| model::Series {
                name: format!("{i:02}").repeat(CHART_MAX_TEXT_BYTES / 128),
                ..c.series[1].clone()
            })
            .collect();
        assert!(c.validate().is_ok());
        let mut r = rig(Some(&c));
        r.click("Title");
        r.type_text("x");
        assert!(r.top("Chart"));
        r.key(Key::Enter);
        assert_eq!((r.chart().title.as_str(), token(&r)), ("", c));
    }

    /// Q99's previews render on a worker (a dense chart takes over a second): a newer chart
    /// cancels the running render, and only the latest one is shown.
    #[test]
    fn previews_render_on_a_worker() {
        let ctx = egui::Context::default();
        let mut p = Preview::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 209.0));
        let run = |p: &mut Preview, chart: &Chart, revision: u64| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                p.show(ui, rect, chart, revision, "en", "test-preview");
            });
            out.textures_delta.clear();
        };
        let dense = {
            let mut c = Chart {
                three_d: true,
                ..Chart::default()
            };
            for s in &mut c.series {
                s.chart_type = ChartType::SplineArea;
                s.points = (0..4096)
                    .map(|i| Point {
                        x: i as f64,
                        value: (i % 7) as f64,
                        ..s.points[0].clone()
                    })
                    .collect();
            }
            c
        };
        run(&mut p, &dense, 1);
        let first = p.job.as_ref().map(|j| j.cancel.clone()).expect("a job");
        run(&mut p, &Chart::default(), 2);
        assert!(
            first.load(std::sync::atomic::Ordering::Relaxed),
            "the older render is cancelled"
        );
        for _ in 0..5000 {
            if p.shown == Some((2, [400, 209])) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            run(&mut p, &Chart::default(), 2);
        }
        assert_eq!(p.shown, Some((2, [400, 209])));
        assert!(p.job.is_none());
    }

    /// BB36 fix-1 re-review: a preview worker that ends without a picture (it panicked) leaves
    /// its chart shown blank instead of starting the same render again every frame.
    #[test]
    fn a_dead_preview_worker_is_not_respawned() {
        let ctx = egui::Context::default();
        let mut p = Preview::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(40.0, 20.0));
        let key = (1, [40, 20]);
        let (tx, rx) = std::sync::mpsc::channel();
        drop(tx);
        p.want = Some(key);
        p.job = Some(PreviewJob {
            key,
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
        });
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                p.show(ui, rect, &Chart::default(), 1, "en", "test-preview");
            });
            out.textures_delta.clear();
        }
        assert!(p.job.is_none(), "no new render");
        assert_eq!(p.shown, Some(key));
    }

    /// Window 2's Tab (DataGridView, StandardTab false): Tab moves cell to cell and leaves the
    /// grid after its last cell, Shift+Tab goes back, Ctrl+Tab always leaves; OK is reachable
    /// from the keyboard. A finished multi-frame sort gives the grid its focus back.
    #[test]
    fn grid_tab_order_reaches_ok() {
        let mut r = rig(None);
        r.editor();
        r.click(&cell(1, "Name"));
        assert_eq!(r.focused.as_deref(), Some("Chart data"));
        r.mods = CTRL;
        r.key(Key::Tab);
        r.mods = Modifiers::NONE;
        assert_eq!(r.focused.as_deref(), Some("OK"), "Ctrl+Tab leaves the grid");
        r.mods = Modifiers::SHIFT;
        r.key(Key::Tab);
        r.mods = Modifiers::NONE;
        assert_eq!(
            r.focused.as_deref(),
            Some("Chart data"),
            "Shift+Tab goes back"
        );
        // Plain Tab walks the cells (wrapping rows) and leaves after the new row's last cell.
        r.click(&cell(1, "Name"));
        let cells = 2 + r
            .chart()
            .life
            .borrow()
            .last_grid
            .as_ref()
            .unwrap()
            .headers
            .len();
        let rows = 3;
        for _ in 0..(rows * cells - 2) {
            r.key(Key::Tab);
            assert_eq!(r.focused.as_deref(), Some("Chart data"));
        }
        r.key(Key::Tab);
        assert_eq!(r.focused.as_deref(), Some("OK"), "Tab past the last cell");
        r.key(Key::Space);
        assert!(!r.top(EDITOR_TITLE), "OK from the keyboard");
        // Shift+Tab steps back a cell: from Monday to Sunday, then a typed value lands there.
        r.editor();
        r.click(&cell(1, "Monday"));
        r.mods = Modifiers::SHIFT;
        r.key(Key::Tab);
        r.mods = Modifiers::NONE;
        r.type_text("8");
        r.key(Key::Enter);
        assert_eq!(r.editor_cell(0, 0), "8");
    }
}
