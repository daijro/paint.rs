//! Image ▸ Resize and Image ▸ Canvas Size: one dialog in two modes, its size model and its
//! remembered choices; the form pieces the New dialog shares.
//! Port of: js/src/interface/window/dialog/ImageSizeDialog.js, css/window.css (`.image-size-*`),
//! js/src/io/DocumentIO.js (`resizeImage`, `changeCanvasSize`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET reference: document-ops-and-formats.md §2.2 step 1 (choices, defaults, limits), §2.3
//! step 1 (Canvas Size defaults and fills), §5 "Task 44"; ui-drawn-graphics.md §2.11 (size
//! rules, anchor chooser, no previews), §5 "Task 44".
use egui::{Color32, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::io::ops::{self, Anchor, CanvasFill, MAX_SIDE, OpError};
use pdn_core::resample::{Gamma, ResizeMode};
use serde_json::{Value, json};

use super::effect_config::{OpenSelect, SelectLook, icon_button, select};
use super::task::{TaskChoice, TaskDialog, TaskDialogSpec};
use super::{Dialog, DialogCx, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{DialogFrameState, dialog_button, dialog_frame};
use crate::kit::{aa, img, px};
use crate::prefs::Prefs;
use crate::theme::{self, *};

/// Paint.NET's resampling choices in its order, with paintdotjs's ids and Paint.NET's labels.
pub const RESAMPLING_CHOICES: [(&str, &str); 8] = [
    ("cubic", "Bicubic"),
    ("cubicSmooth", "Bicubic (Smooth)"),
    ("linear", "Bilinear"),
    ("linearLowQuality", "Bilinear (Low Quality)"),
    ("adaptiveHighQuality", "Adaptive (Sharp)"),
    ("lanczos3", "Lanczos"),
    ("fant", "Fant"),
    ("nearestNeighbor", "Nearest Neighbor"),
];
/// §2.3 step 1's fills in Paint.NET's order, labelled as paintdotjs labels them.
pub const FILL_CHOICES: [(&str, &str); 5] = [
    ("transparent", "Transparent"),
    ("primary", "Primary color"),
    ("secondary", "Secondary color"),
    ("white", "White"),
    ("black", "Black"),
];
/// The anchors in paintdotjs's order (row-major), labelled as it labels them.
pub const ANCHOR_CHOICES: [(&str, &str); 9] = [
    ("topLeft", "Top Left"),
    ("top", "Top"),
    ("topRight", "Top Right"),
    ("left", "Left"),
    ("middle", "Center"),
    ("right", "Right"),
    ("bottomLeft", "Bottom Left"),
    ("bottom", "Bottom"),
    ("bottomRight", "Bottom Right"),
];
const RESOLUTION_UNITS: [(&str, &str); 2] = [("inch", "pixels/inch"), ("centimeter", "pixels/cm")];
const PRINT_UNITS: [(&str, &str); 2] = [("inches", "inches"), ("centimeters", "centimeters")];

/// `ImageSizeDialog.storageKey`.
pub const STORAGE_KEY: &str = "paintdotjs.imageSize.v1";

/// The percent up-down's maximum (ui-drawn-graphics.md §2.11).
pub const MAX_PERCENT: f64 = 2000.0;

/// `ImageSizeDialog`'s sizes (ui-drawn-graphics.md §2.11): kept as doubles;
/// under the aspect lock the other side is value ÷ (W₀/H₀) or value × (W₀/H₀); a percentage
/// gives W₀·p/100 × H₀·p/100. Pixels are the doubles rounded half away from zero.
#[derive(Clone, Debug, PartialEq)]
pub struct SizeModel {
    pub w0: f64,
    pub h0: f64,
    pub w: f64,
    pub h: f64,
    pub percent: f64,
    /// "By percentage" is chosen: the percent drives the sizes (and must be in range).
    pub by_percent: bool,
    pub maintain: bool,
}

impl SizeModel {
    pub fn new(w0: f64, h0: f64, maintain: bool) -> Self {
        SizeModel {
            w0,
            h0,
            w: w0,
            h: h0,
            percent: 100.0,
            by_percent: false,
            maintain,
        }
    }

    pub fn set_width(&mut self, v: f64) {
        self.w = v;
        if self.maintain {
            self.h = v / (self.w0 / self.h0);
        }
        self.absolute();
    }

    pub fn set_height(&mut self, v: f64) {
        self.h = v;
        if self.maintain {
            self.w = v * (self.w0 / self.h0);
        }
        self.absolute();
    }

    /// An absolute size: the percent field follows the width (`toFixed(2)`, `updateDimension`).
    fn absolute(&mut self) {
        self.by_percent = false;
        self.percent = (self.w / self.w0 * 10_000.0).round() / 100.0;
    }

    pub fn set_percent(&mut self, p: f64) {
        self.by_percent = true;
        self.percent = p;
        self.w = self.w0 * p / 100.0;
        self.h = self.h0 * p / 100.0;
    }

    /// The final pixel size: the up-downs' ⌊·⌉ of the doubles.
    pub fn pixels(&self) -> (u32, u32) {
        let px = |v: f64| v.round().clamp(0.0, u32::MAX as f64) as u32;
        (px(self.w), px(self.h))
    }

    /// The size the same choice gives a `w0 × h0` image (Resize on the strip's picked images):
    /// the percent; else the width, at the image's own aspect ratio when that is maintained, or
    /// the very size.
    pub fn applied_to(&self, w0: f64, h0: f64) -> (u32, u32) {
        let mut m = SizeModel::new(w0, h0, self.maintain);
        if self.by_percent {
            m.set_percent(self.percent);
        } else {
            m.set_width(self.w);
            if !self.maintain {
                m.set_height(self.h);
            }
        }
        m.pixels()
    }

    /// When OK is enabled: 1 ≤ w, h ≤ 262144 and, by percentage, the percent within its range.
    pub fn valid(&self) -> bool {
        let (w, h) = self.pixels();
        let side = |v: u32| (1..=MAX_SIDE).contains(&v);
        side(w) && side(h) && (!self.by_percent || self.percent <= MAX_PERCENT)
    }
}

/// `paintdotjs.imageSize.v1` with paintdotjs's keys. `Default` is Paint.NET's (R19).
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSizePrefs {
    pub resize_maintain: bool,
    pub canvas_maintain: bool,
    pub resampling: String,
    pub gamma_correct: bool,
    pub anchor: String,
    pub fill: String,
    pub resolution_unit: String,
    pub print_unit: String,
}

impl Default for ImageSizePrefs {
    fn default() -> Self {
        ImageSizePrefs {
            resize_maintain: true,
            canvas_maintain: false,
            resampling: "cubic".into(),
            gamma_correct: true,
            anchor: "topLeft".into(),
            fill: "transparent".into(),
            resolution_unit: "inch".into(),
            print_unit: "inches".into(),
        }
    }
}

/// The index of `id` in `choices`, else 0.
fn index_of(choices: &[(&str, &str)], id: &str) -> usize {
    choices.iter().position(|c| c.0 == id).unwrap_or(0)
}

impl ImageSizePrefs {
    /// `loadPreferences`: the stored keys over the defaults, the old resampling ids mapped
    /// (`high` → `adaptiveHighQuality` …); unreadable JSON gives the defaults.
    pub fn load(prefs: &Prefs) -> Self {
        let mut p = ImageSizePrefs::default();
        let Some(v) = prefs
            .get(STORAGE_KEY)
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
        else {
            return p;
        };
        let flag = |key: &str, d: &mut bool| {
            if let Some(b) = v[key].as_bool() {
                *d = b;
            }
        };
        let id = |key: &str, d: &mut String| {
            if let Some(s) = v[key].as_str() {
                *d = s.to_string();
            }
        };
        flag("resizeMaintain", &mut p.resize_maintain);
        flag("canvasMaintain", &mut p.canvas_maintain);
        flag("gammaCorrect", &mut p.gamma_correct);
        id("resampling", &mut p.resampling);
        id("anchor", &mut p.anchor);
        id("fill", &mut p.fill);
        id("resolutionUnit", &mut p.resolution_unit);
        id("printUnit", &mut p.print_unit);
        let legacy = match p.resampling.as_str() {
            "high" => Some("adaptiveHighQuality"),
            "medium" => Some("linear"),
            "low" => Some("linearLowQuality"),
            "nearest" => Some("nearestNeighbor"),
            _ => None,
        };
        if let Some(l) = legacy {
            p.resampling = l.into();
        }
        p
    }

    /// `savePreferences`.
    pub fn save(&self, prefs: &mut Prefs) {
        let v = json!({
            "resizeMaintain": self.resize_maintain,
            "canvasMaintain": self.canvas_maintain,
            "resampling": self.resampling,
            "gammaCorrect": self.gamma_correct,
            "anchor": self.anchor,
            "fill": self.fill,
            "resolutionUnit": self.resolution_unit,
            "printUnit": self.print_unit,
        });
        prefs.set(STORAGE_KEY, &v.to_string());
    }
}

// ---- form pieces shared with the New dialog ----

pub(super) fn font(weight: u16) -> FontSpec {
    FontSpec {
        weight,
        ..FontSpec::new(DIALOG_FONT_SIZE)
    }
}

/// The 15-px line box of the dialog font.
pub(super) const LINE: f32 = 15.0;

/// Text whose line box starts at `at`.
pub(super) fn label(ui: &Ui, at: Pos2, s: &str, weight: u16, color: Color32) {
    text::draw(ui.painter(), at, s, font(weight), color);
}

/// Text right-aligned at `right` (`text-align: right`), its line box at `top`.
pub(super) fn label_right(ui: &Ui, right: f32, top: f32, s: &str, color: Color32) {
    let w = text::css_width(s, DIALOG_FONT_SIZE);
    label(ui, Pos2::new(right - w, top), s, 400, color);
}

/// A 1-px `--pdn-separator` rule from `x0` to `x1` at `y` (snapped).
pub(super) fn rule(ui: &Ui, x0: f32, x1: f32, y: f32) {
    let theme = theme::current(ui.ctx());
    let r = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + 1.0));
    ui.painter().rect_filled(
        px::snap_rect(r, ui.ctx().pixels_per_point()),
        0.0,
        theme.pdn_separator,
    );
}

/// A `<strong>` summary with `padding-bottom: pad; border-bottom: 1px` at `top` across
/// `x0..x1`, in `weight`. Returns its bottom.
pub(super) fn summary(ui: &Ui, x0: f32, x1: f32, top: f32, s: &str, weight: u16, pad: f32) -> f32 {
    let theme = theme::current(ui.ctx());
    label(ui, Pos2::new(x0, top), s, weight, theme.menu_item_text);
    rule(ui, x0, x1, top + LINE + pad);
    top + LINE + pad + 1.0
}

/// A `fieldset` with its `legend` at `top` across `x0..x1`: the legend text and the top border
/// through its middle after it. Returns the legend's bottom.
pub(super) fn fieldset(ui: &Ui, x0: f32, x1: f32, top: f32, legend: &str) -> f32 {
    let theme = theme::current(ui.ctx());
    label(ui, Pos2::new(x0, top), legend, 400, theme.menu_item_text);
    let w = text::css_width(legend, DIALOG_FONT_SIZE) + FIELDSET_LEGEND_PAD;
    rule(ui, x0 + w, x1, top + (LINE - 1.0) / 2.0);
    top + LINE
}

/// A `<label>` around a 12-px checkbox and its text: the box at `at.x`, centred in the `h`-tall
/// label, the text `gap` after it. A click anywhere on the label toggles (the label is no Tab
/// stop).
#[allow(clippy::too_many_arguments)]
pub(super) fn check_label(
    ui: &mut Ui,
    id: &str,
    at: Pos2,
    h: f32,
    gap: f32,
    s: &str,
    checked: &mut bool,
    color: Color32,
) -> bool {
    let theme = theme::current(ui.ctx());
    let side = CHECKBOX_SIZE;
    let boxed = Rect::from_min_size(Pos2::new(at.x, at.y + (h - side) / 2.0), Vec2::splat(side));
    let tx = boxed.max.x + gap;
    let whole = Rect::from_min_max(
        at,
        Pos2::new(tx + text::css_width(s, DIALOG_FONT_SIZE), at.y + h),
    );
    let ppp = ui.ctx().pixels_per_point();
    let label_click = ui
        .interact(
            px::snap_rect(whole, ppp),
            ui.scope_id().with((id, "label")),
            Sense::CLICK,
        )
        .clicked();
    let before = *checked;
    inputs::checkbox(ui, id, boxed, checked, true, theme.window_background);
    if label_click {
        *checked = !*checked;
    }
    label(ui, Pos2::new(tx, at.y + (h - LINE) / 2.0), s, 400, color);
    *checked != before
}

// ---- the dialog ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Resize,
    Canvas,
}

pub struct ImageSizeDialog {
    mode: Mode,
    model: SizeModel,
    dpi: f64,
    /// The resolution it opened with: left alone, the picked images keep their own.
    dpi0: f64,
    layers: usize,
    /// The remembered choices, once loaded.
    prefs: Option<ImageSizePrefs>,
    resolution_unit: usize,
    print_unit: usize,
    resampling: usize,
    gamma: bool,
    anchor: usize,
    fill: usize,
    open_select: Option<OpenSelect>,
    /// A field that takes the keyboard focus (`input.select()`) next frame.
    focus: Option<Id>,
}

/// The fields the body reports back to `show`.
#[derive(Default)]
struct BodyOut {
    width: Option<Id>,
    percent: Option<Id>,
    select_percent: bool,
    select_width: bool,
}

impl ImageSizeDialog {
    /// The dialog for the active document (`ImageSizeDialog.open(mode, {width, height,
    /// resolution, layerCount})`); the remembered choices load in its first `show`.
    pub fn new(mode: Mode, (w, h, dpi): (u32, u32, f64), layers: usize) -> Self {
        ImageSizeDialog {
            mode,
            model: SizeModel::new(w as f64, h as f64, false),
            dpi,
            dpi0: dpi,
            layers: layers.max(1),
            resolution_unit: 0,
            print_unit: 0,
            resampling: 0,
            gamma: true,
            anchor: 0,
            fill: 0,
            prefs: None,
            open_select: None,
            focus: None,
        }
    }

    /// `loadPreferences` into the controls.
    fn load(&mut self, prefs: &Prefs) {
        let p = ImageSizePrefs::load(prefs);
        self.model.maintain = match self.mode {
            Mode::Resize => p.resize_maintain,
            Mode::Canvas => p.canvas_maintain,
        };
        self.resolution_unit = index_of(&RESOLUTION_UNITS, &p.resolution_unit);
        self.print_unit = index_of(&PRINT_UNITS, &p.print_unit);
        self.resampling = index_of(&RESAMPLING_CHOICES, &p.resampling);
        self.gamma = p.gamma_correct;
        self.anchor = index_of(&ANCHOR_CHOICES, &p.anchor);
        self.fill = index_of(&FILL_CHOICES, &p.fill);
        self.prefs = Some(p);
    }

    /// The `.image-size-content` height (padding included).
    fn content_height(&self) -> f32 {
        let (pad_top, _, pad_bottom) = IMAGE_SIZE_PADDING;
        let g = IMAGE_SIZE_GAP;
        let (sizing_pad, ..) = IMAGE_SIZE_SIZING;
        let (_, _, ih, sh) = IMAGE_SIZE_ROW;
        let (fs_top, fs_bottom, body_top) = IMAGE_SIZE_FIELDSET;
        let fieldset = |body: f32| LINE + fs_top + body_top + body + fs_bottom;
        let summary = LINE + IMAGE_SIZE_SUMMARY_PAD + 1.0;
        let sizing = sizing_pad + ih + g + RADIO_SIZE + g + LINE;
        let pixel = fieldset(ih + g + ih + g + sh);
        let print = fieldset(sh + g + ih);
        let options = match self.mode {
            Mode::Resize => fieldset(sh + g + LINE),
            Mode::Canvas => fieldset(
                sh + g + 3.0 * ANCHOR_CELL + ANCHOR_MARGIN.1 + g + IMAGE_SIZE_FILL_MARGIN + sh,
            ),
        };
        pad_top + summary + g + sizing + g + pixel + g + print + g + options + pad_bottom
    }

    fn print_scale(&self) -> f64 {
        if self.print_unit == 1 { 2.54 } else { 1.0 }
    }

    /// `dialog.onsubmit`: remembers the choices, then runs the operation; a failure shows its
    /// error (`resizeAction.performAction.*`).
    fn ok(&mut self, cx: &mut DialogCx) -> DialogState {
        let (w, h) = self.model.pixels();
        let mut prefs = self.prefs.clone().unwrap_or_default();
        prefs.resolution_unit = RESOLUTION_UNITS[self.resolution_unit].0.into();
        prefs.print_unit = PRINT_UNITS[self.print_unit].0.into();
        let result = match self.mode {
            Mode::Resize => {
                prefs.resize_maintain = self.model.maintain;
                prefs.resampling = RESAMPLING_CHOICES[self.resampling].0.into();
                prefs.gamma_correct = self.gamma;
                let mode = ResizeMode::from_value(RESAMPLING_CHOICES[self.resampling].0)
                    .unwrap_or(ResizeMode::Bicubic);
                let gamma = if self.gamma {
                    Gamma::Linear
                } else {
                    Gamma::Companded
                };
                let mut result = ops::resize_image(cx.app, w, h, mode, gamma, self.dpi);
                // The strip's picked images: the same choice at their own sizes.
                let (model, dpi, dpi0) = (&self.model, self.dpi, self.dpi0);
                cx.app.for_each_picked(|app| {
                    let Some(d) = app.active_ws().map(|ws| &ws.document) else {
                        return;
                    };
                    let (pw, ph) = model.applied_to(d.width as f64, d.height as f64);
                    let dpi = if dpi == dpi0 { d.resolution } else { dpi };
                    let r = ops::resize_image(app, pw, ph, mode, gamma, dpi);
                    result = result.and(r);
                });
                result
            }
            Mode::Canvas => {
                prefs.canvas_maintain = self.model.maintain;
                prefs.anchor = ANCHOR_CHOICES[self.anchor].0.into();
                prefs.fill = FILL_CHOICES[self.fill].0.into();
                let anchor =
                    Anchor::from_id(ANCHOR_CHOICES[self.anchor].0).unwrap_or(Anchor::TopLeft);
                let fill = CanvasFill::from_id(FILL_CHOICES[self.fill].0)
                    .unwrap_or(CanvasFill::Transparent);
                ops::change_canvas_size(cx.app, w, h, anchor, fill, self.dpi)
            }
        };
        prefs.save(cx.prefs);
        if let Err(e) = result {
            show_error(self.mode, e);
        }
        DialogState::Closed(super::DialogResult::Ok)
    }

    /// The body: every control, laid out as css/window.css lays out `.image-size-content`.
    fn body(&mut self, ui: &mut Ui, r: Rect) -> BodyOut {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let text_color = theme.menu_item_text;
        let (pad_top, pad_x, _) = IMAGE_SIZE_PADDING;
        let g = IMAGE_SIZE_GAP;
        let (x0, x1) = (r.min.x + pad_x, r.max.x - pad_x);
        let mut out = BodyOut::default();
        let mut y = r.min.y + pad_top;
        // `updateSummary`, with Paint.NET's decimal point (R73).
        let (pw, ph) = self.model.pixels();
        let mib = pw as f64 * ph as f64 * self.layers as f64 * 4.0 / 1024.0 / 1024.0;
        let s = format!("New size: {:.1} MB", mib.max(0.1));
        y = summary(ui, x0, x1, y, &s, 600, IMAGE_SIZE_SUMMARY_PAD) + g;

        // The sizing choices.
        let (sizing_pad, choice_label, radio_gap, indent) = IMAGE_SIZE_SIZING;
        let (_, iw, ih, sh) = IMAGE_SIZE_ROW;
        y += sizing_pad;
        let percent_mode = self.model.by_percent;
        let radio_row = |ui: &mut Ui, id: &str, top: f32, w: f32, s: &str, on: bool| {
            let rr = Rect::from_min_size(Pos2::new(x0, top), Vec2::splat(RADIO_SIZE));
            let whole = Rect::from_min_size(Pos2::new(x0, top), Vec2::new(w, RADIO_SIZE));
            let lr = ui.interact(
                px::snap_rect(whole, ppp),
                ui.scope_id().with((id, "label")),
                Sense::CLICK,
            );
            let radio = inputs::radio(ui, id, rr, on, theme.window_background);
            label(
                ui,
                Pos2::new(rr.max.x + radio_gap, top + (RADIO_SIZE - LINE) / 2.0),
                s,
                400,
                text_color,
            );
            (lr.clicked() || radio) && !on
        };
        let row_top = y;
        if radio_row(
            ui,
            "percent",
            row_top + (ih - RADIO_SIZE) / 2.0,
            choice_label,
            "By percentage:",
            percent_mode,
        ) {
            // `updateMode`: the percentage drives the sizes; its field is selected.
            self.model.set_percent(self.model.percent);
            out.select_percent = true;
        }
        let pr = Rect::from_min_size(Pos2::new(x0 + choice_label + g, row_top), Vec2::new(iw, ih));
        let mut p = self.model.percent;
        let field = inputs::number_input(
            ui,
            "percent",
            "By percentage",
            pr,
            &mut p,
            0.01,
            MAX_PERCENT,
            0.01,
            percent_mode,
            NumberStyle::IMAGE_SIZE_PERCENT,
        );
        out.percent = Some(field.id);
        if p != self.model.percent {
            self.model.set_percent(p);
        }
        label(
            ui,
            Pos2::new(pr.max.x + g, row_top + (ih - LINE) / 2.0),
            "%",
            700,
            text_color,
        );
        y += ih + g;
        if radio_row(
            ui,
            "absolute",
            y,
            x1 - x0,
            "By absolute size:",
            !percent_mode,
        ) {
            self.model.by_percent = false;
            out.select_width = true;
        }
        y += RADIO_SIZE + g;
        // ponytail: percent mode dims the label text and disables the inputs (their own .55 fade,
        // not .5 × .55) but leaves the unit selects live; give `select` an enabled state and a
        // group fade if a reference ever shows percent mode.
        let dim = |c: Color32| {
            if percent_mode {
                // `.image-size-disabled { opacity: .5 }`.
                Color32::from_rgb(
                    ((c.r() as u16 + theme.window_background.r() as u16) / 2) as u8,
                    ((c.g() as u16 + theme.window_background.g() as u16) / 2) as u8,
                    ((c.b() as u16 + theme.window_background.b() as u16) / 2) as u8,
                )
            } else {
                c
            }
        };
        let mut maintain = self.model.maintain;
        if check_label(
            ui,
            "maintain",
            Pos2::new(x0 + indent, y),
            LINE,
            radio_gap,
            "Maintain aspect ratio",
            &mut maintain,
            dim(text_color),
        ) && !percent_mode
        {
            // `maintain.onchange`: `updateDimension(width.input)`.
            self.model.maintain = maintain;
            let w = self.model.w;
            self.model.set_width(w);
        }
        y += LINE + g;

        // Pixel size.
        let (fs_top, fs_bottom, body_top) = IMAGE_SIZE_FIELDSET;
        let (label_w, ..) = IMAGE_SIZE_ROW;
        let input_x = x0 + label_w + g;
        let unit_x = input_x + iw + g;
        let enabled = !percent_mode;
        y = fieldset(ui, x0, x1, y, "Pixel size") + fs_top + body_top;
        let number_row = |ui: &mut Ui,
                          id: &str,
                          name: &str,
                          top: f32,
                          h: f32,
                          value: &mut f64,
                          (min, max, step): (f64, f64, f64),
                          style: NumberStyle| {
            label_right(
                ui,
                x0 + label_w,
                top + (h - LINE) / 2.0,
                name,
                dim(text_color),
            );
            let rect =
                Rect::from_min_size(Pos2::new(input_x, top + (h - ih) / 2.0), Vec2::new(iw, ih));
            inputs::number_input(ui, id, name, rect, value, min, max, step, enabled, style)
        };
        let side = (1.0, MAX_SIDE as f64, 1.0);
        let style = NumberStyle::IMAGE_SIZE;
        let mut v = pw as f64;
        let field = number_row(ui, "width", "Width:", y, ih, &mut v, side, style);
        out.width = Some(field.id);
        if v != pw as f64 {
            self.model.set_width(v);
        }
        label(
            ui,
            Pos2::new(unit_x, y + (ih - LINE) / 2.0),
            "pixels",
            400,
            dim(text_color),
        );
        y += ih + g;
        let mut v = ph as f64;
        number_row(ui, "height", "Height:", y, ih, &mut v, side, style);
        if v != ph as f64 {
            self.model.set_height(v);
        }
        label(
            ui,
            Pos2::new(unit_x, y + (ih - LINE) / 2.0),
            "pixels",
            400,
            dim(text_color),
        );
        y += ih + g;
        // Resolution (0.01…65536, 2 decimals; per cm shows dpi / 2.54).
        let cm = self.resolution_unit == 1;
        let shown = if cm { self.dpi / 2.54 } else { self.dpi };
        let mut v = shown;
        number_row(
            ui,
            "resolution",
            "Resolution:",
            y,
            sh,
            &mut v,
            (0.01, 65536.0, 0.01),
            style.decimals(2),
        );
        if v != shown {
            self.dpi = if cm { v * 2.54 } else { v };
        }
        let unit_rect = Rect::from_min_max(Pos2::new(unit_x, y), Pos2::new(x1, y + sh));
        let open = &mut self.open_select;
        if let Some(i) = choose(
            ui,
            open,
            unit_rect,
            "Resolution unit",
            &RESOLUTION_UNITS,
            self.resolution_unit,
        ) {
            self.resolution_unit = i;
        }
        y += sh + fs_bottom + g;

        // Print size.
        y = fieldset(ui, x0, x1, y, "Print size") + fs_top + body_top;
        let scale = self.print_scale();
        let print = (0.01, 1_000_000.0, 0.01);
        let shown = pw as f64 / self.dpi * scale;
        let mut v = shown;
        number_row(
            ui,
            "print-width",
            "Width:",
            y,
            sh,
            &mut v,
            print,
            style.decimals(2),
        );
        if v != shown {
            self.model.set_width(v / scale * self.dpi);
        }
        let print_rect = Rect::from_min_max(Pos2::new(unit_x, y), Pos2::new(x1, y + sh));
        let open = &mut self.open_select;
        if let Some(i) = choose(
            ui,
            open,
            print_rect,
            "Print unit",
            &PRINT_UNITS,
            self.print_unit,
        ) {
            self.print_unit = i;
        }
        y += sh + g;
        let shown = ph as f64 / self.dpi * scale;
        let mut v = shown;
        number_row(
            ui,
            "print-height",
            "Height:",
            y,
            ih,
            &mut v,
            print,
            style.decimals(2),
        );
        if v != shown {
            self.model.set_height(v / scale * self.dpi);
        }
        label(
            ui,
            Pos2::new(unit_x, y + (ih - LINE) / 2.0),
            PRINT_UNITS[self.print_unit].1,
            400,
            dim(text_color),
        );
        y += ih + fs_bottom + g;

        // Options.
        y = fieldset(ui, x0, x1, y, "Options") + fs_top + body_top;
        let select_x = x0 + IMAGE_SIZE_SELECT_LABEL + g;
        match self.mode {
            Mode::Resize => {
                let (reset, icon) = IMAGE_SIZE_RESET;
                label_right(
                    ui,
                    x0 + IMAGE_SIZE_SELECT_LABEL,
                    y + (sh - LINE) / 2.0,
                    "Resampling:",
                    text_color,
                );
                let sr =
                    Rect::from_min_max(Pos2::new(select_x, y), Pos2::new(x1 - reset - g, y + sh));
                let open = &mut self.open_select;
                if let Some(i) = choose(
                    ui,
                    open,
                    sr,
                    "Resampling",
                    &RESAMPLING_CHOICES,
                    self.resampling,
                ) {
                    self.resampling = i;
                }
                let rr = Rect::from_min_size(Pos2::new(x1 - reset, y), Vec2::splat(reset));
                if icon_button(ui, ui.scope_id().with("reset"), rr, icon) {
                    // Back to Paint.NET's defaults.
                    self.resampling = 0;
                    self.gamma = true;
                }
                y += sh + g;
                check_label(
                    ui,
                    "gamma",
                    Pos2::new(x0, y),
                    LINE,
                    IMAGE_SIZE_GAMMA_GAP,
                    "Use gamma-correct resampling",
                    &mut self.gamma,
                    text_color,
                );
            }
            Mode::Canvas => {
                label_right(
                    ui,
                    x0 + IMAGE_SIZE_SELECT_LABEL,
                    y + (sh - LINE) / 2.0,
                    "Anchor:",
                    text_color,
                );
                let sr = Rect::from_min_max(Pos2::new(select_x, y), Pos2::new(x1, y + sh));
                let open = &mut self.open_select;
                if let Some(i) = choose(ui, open, sr, "Anchor", &ANCHOR_CHOICES, self.anchor) {
                    self.anchor = i;
                }
                y += sh + g;
                let at = Pos2::new(x0 + ANCHOR_MARGIN.0, y);
                if let Some(i) = anchor_chooser(ui, at, self.anchor) {
                    self.anchor = i;
                }
                y += 3.0 * ANCHOR_CELL + ANCHOR_MARGIN.1 + g + IMAGE_SIZE_FILL_MARGIN;
                label_right(
                    ui,
                    x0 + IMAGE_SIZE_SELECT_LABEL,
                    y + (sh - LINE) / 2.0,
                    "Fill:",
                    text_color,
                );
                let sr = Rect::from_min_max(Pos2::new(select_x, y), Pos2::new(x1, y + sh));
                let open = &mut self.open_select;
                if let Some(i) = choose(ui, open, sr, "Fill", &FILL_CHOICES, self.fill) {
                    self.fill = i;
                }
            }
        }
        out
    }
}

/// A native `<select>` of `choices` (id, label) at `rect`, named `name`; the picked index.
fn choose(
    ui: &mut Ui,
    open: &mut Option<OpenSelect>,
    rect: Rect,
    name: &str,
    choices: &[(&str, &str)],
    current: usize,
) -> Option<usize> {
    let names: Vec<&str> = choices.iter().map(|c| c.1).collect();
    let look = SelectLook {
        native: true,
        pad_left: 0.0,
        font: font(400),
    };
    let id = ui.scope_id().with(name);
    select(ui, open, id, rect, name, &names, current, look)
}

/// The error a failed Resize or Canvas Size shows (`resizeAction.performAction.*`; Canvas Size,
/// which has no strings of its own, the generic `generic.outOfMemory` and
/// `executeFunction.genericError`).
fn show_error(mode: Mode, e: OpError) {
    let t = pdn_core::i18n::t;
    let (title, message) = match (mode, e) {
        (Mode::Resize, OpError::OutOfMemory) => (
            t("resizeAction.name"),
            t("resizeAction.performAction.outOfMemory"),
        ),
        (Mode::Canvas, OpError::OutOfMemory) => {
            (t("canvasSizeAction.name"), t("generic.outOfMemory"))
        }
        (Mode::Resize, _) => (
            t("resizeAction.name"),
            t("resizeAction.performAction.unspecifiedError"),
        ),
        (Mode::Canvas, _) => (
            t("canvasSizeAction.name"),
            t("executeFunction.genericError"),
        ),
    };
    TaskDialog::show(TaskDialogSpec {
        message_icon: None,
        title,
        icon: None,
        message: Some(message),
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
}

/// `createAnchorChooser` at `at` (its top-left): 3×3 28-px buttons; the anchor shows
/// `anchor_chooser_control_anchor_image.png`, its neighbours arrows pointing away from it.
/// Returns the clicked cell.
fn anchor_chooser(ui: &mut Ui, at: Pos2, anchor: usize) -> Option<usize> {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let b = px::border_width(1.0, ppp);
    let (ax, ay) = ((anchor % 3) as i32, (anchor / 3) as i32);
    let mut clicked = None;
    for (i, (_, name)) in ANCHOR_CHOICES.iter().enumerate() {
        let (x, y) = ((i % 3) as i32, (i / 3) as i32);
        let cell = Rect::from_min_size(
            at + Vec2::new(x as f32, y as f32) * ANCHOR_CELL,
            Vec2::splat(ANCHOR_CELL),
        );
        let snapped = px::snap_rect(cell, ppp);
        let response = ui.interact(snapped, ui.scope_id().with(("anchor", i)), Sense::click());
        inputs::take_enter(ui, &response);
        ui.ctx().accesskit_node_builder(response.id, |n| {
            n.set_role(egui::accesskit::Role::RadioButton);
            n.set_label(*name);
        });
        if response.clicked() {
            clicked = Some(i);
        }
        let selected = i == anchor;
        let hot = response.hovered();
        // `.selected` (and `:active`): the hover gradient reversed; `:hover` the hover look.
        let (border, gradient) = if selected || response.is_pointer_button_down_on() {
            (
                theme.slider_hover_outline,
                Some((theme.slider_hover2, theme.slider_hover1)),
            )
        } else if hot {
            (
                theme.slider_hover_outline,
                Some((theme.slider_hover1, theme.slider_hover2)),
            )
        } else {
            (theme.button_outline, None)
        };
        painter.rect_filled(snapped, 0.0, border);
        let pad = snapped.shrink(b);
        let pixels = match gradient {
            Some(g) => inputs::gradient_pixels(pad, g, ppp),
            None => {
                let (w, h) = (
                    (pad.width() * ppp).round() as usize,
                    (pad.height() * ppp).round() as usize,
                );
                vec![vec![theme.button; w]; h]
            }
        };
        let (dx, dy) = (x - ax, y - ay);
        if selected {
            inputs::paint_pixels(&painter, pad.min, &pixels);
            // `img { 18×18; margin: auto }`: centred, over the gradient under it.
            let ir = px::snap_rect(
                Rect::from_center_size(cell.center(), Vec2::splat(ANCHOR_IMAGE)),
                ppp,
            );
            let (ox, oy) = (
                ((ir.min.x - pad.min.x) * ppp).round() as usize,
                ((ir.min.y - pad.min.y) * ppp).round() as usize,
            );
            let n = (ANCHOR_IMAGE * ppp).round() as usize;
            let under: Vec<Color32> = (0..n * n)
                .map(|k| {
                    let row = &pixels[(oy + k / n).min(pixels.len() - 1)];
                    row[(ox + k % n).min(row.len() - 1)]
                })
                .collect();
            img::paint_icon_over(
                &painter,
                "anchor_chooser_control_anchor_image.png",
                ir,
                &under,
            );
        } else if dx.abs() <= 1 && dy.abs() <= 1 {
            let mut pixels = pixels;
            anchor_arrow(
                &mut pixels,
                pad,
                cell,
                dx as f32,
                dy as f32,
                theme.button_text,
                ppp,
            );
            inputs::paint_pixels(&painter, pad.min, &pixels);
        } else {
            inputs::paint_pixels(&painter, pad.min, &pixels);
        }
    }
    clicked
}

/// `createAnchorArrow(dx, dy)` rasterized into the cell's padding-box `pixels` (device px from
/// `pad.min`): the SVG sits on the content box (`cell` inset by the 1-px border), a 1.5-px line
/// from (14 − 7·dx, 14 − 7·dy) over 14 px along the unit vector and a triangular head 6 deep and
/// 6 wide at its end, both `currentColor`.
fn anchor_arrow(
    pixels: &mut [Vec<Color32>],
    pad: Rect,
    cell: Rect,
    dx: f32,
    dy: f32,
    color: Color32,
    ppp: f32,
) {
    let (back, len, stroke, head) = ANCHOR_ARROW;
    let n = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / n, dy / n);
    let mid = ANCHOR_CELL / 2.0;
    let (x1, y1) = (mid - back * dx, mid - back * dy);
    let (x2, y2) = (x1 + len * ux, y1 + len * uy);
    let (bx, by) = (x2 - head * ux, y2 - head * uy);
    let (px_, py_) = (-uy * head / 2.0, ux * head / 2.0);
    // The SVG's origin is the content box's, snapped to device pixels.
    let origin = ((cell.min + Vec2::splat(1.0)) * ppp).round();
    let dev = |x: f32, y: f32| origin + Vec2::new(x, y) * ppp;
    let (nx, ny) = (-uy * stroke / 2.0, ux * stroke / 2.0);
    let line = [
        dev(x1 + nx, y1 + ny),
        dev(x2 + nx, y2 + ny),
        dev(x2 - nx, y2 - ny),
        dev(x1 - nx, y1 - ny),
    ];
    let tri = [
        dev(x2, y2),
        dev(bx + px_, by + py_),
        dev(bx - px_, by - py_),
    ];
    let p0 = pad.min * ppp;
    for poly in [&line[..], &tri[..]] {
        let (at, rows) = aa::polygon_coverage(poly);
        for (j, row) in rows.iter().enumerate() {
            for (i, &a) in row.iter().enumerate() {
                let (x, y) = (
                    (at.x - p0.x).round() as i32 + i as i32,
                    (at.y - p0.y).round() as i32 + j as i32,
                );
                if a == 0 || x < 0 || y < 0 {
                    continue;
                }
                if let Some(p) = pixels
                    .get_mut(y as usize)
                    .and_then(|r| r.get_mut(x as usize))
                {
                    *p = aa::blit_aa(color, a, *p);
                }
            }
        }
    }
}

impl Dialog for ImageSizeDialog {
    fn title(&self) -> String {
        match self.mode {
            Mode::Resize => "Resize".into(),
            Mode::Canvas => "Canvas Size".into(),
        }
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        if self.prefs.is_none() {
            self.load(cx.prefs);
        }
        let list_open = self.open_select.is_some();
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            if !list_open {
                return DialogState::Closed(super::DialogResult::Cancel);
            }
            self.open_select = None;
        }
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let screen = ctx.content_rect();
        let (width, inset) = IMAGE_SIZE_WIDTH;
        let width = width.min(screen.width() - inset);
        let height = self.content_height();
        let valid = self.model.valid();
        let mut out = BodyOut::default();
        let mut action = None;
        let (title, icon) = match self.mode {
            Mode::Resize => ("Resize", "menu_image_resize_icon.png"),
            Mode::Canvas => ("Canvas Size", "menu_image_canvas_size_icon.png"),
        };
        let mut body = |ui: &mut Ui, r: Rect| out = self.body(ui, r);
        let mut footer = |ui: &mut Ui, f: Rect| {
            action = footer_buttons(ui, f, IMAGE_SIZE_BUTTON, DIALOG_BUTTON_MIN.1, valid);
        };
        let frame = dialog_frame(
            ui,
            title,
            Some(icon),
            (Vec2::new(width, height), &mut body),
            Some(&mut footer),
        );
        let mut state = DialogFrameState::get(&ctx, ui.scope_id());
        state.popup = if self.open_select.is_some() {
            ctx.content_rect()
        } else {
            Rect::NOTHING
        };
        state.set(&ctx, ui.scope_id());
        // `width.input.select()` on open; `percentInput.select()` / `width.input.select()` when
        // the sizing mode changes.
        if let Some(id) = self.focus.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if cx.seat_focus || out.select_width {
            self.focus = out.width;
        }
        if out.select_percent {
            self.focus = out.percent;
        }
        if self.focus.is_some() {
            ctx.request_repaint();
        }
        if enter
            && !list_open
            && self.open_select.is_none()
            && action.is_none()
            && !inputs::enter_taken(&ctx)
            && valid
        {
            return self.ok(cx);
        }
        match action {
            Some(true) => return self.ok(cx),
            Some(false) => return DialogState::Closed(super::DialogResult::Cancel),
            None => {}
        }
        if frame.closed {
            return DialogState::Closed(super::DialogResult::Cancel);
        }
        DialogState::Open
    }
}

/// OK and Cancel at the right of the footer `f` (inside its top border): `min` wide, `h` tall,
/// 10 px apart, centred in the padding box. Some(true) for OK, Some(false) for Cancel.
pub(super) fn footer_buttons(
    ui: &mut Ui,
    f: Rect,
    min: f32,
    h: f32,
    ok_enabled: bool,
) -> Option<bool> {
    let (fy, fx) = DIALOG_FOOTER_PADDING;
    let content = Rect::from_min_max(
        Pos2::new(f.min.x + fx, f.min.y + fy),
        Pos2::new(f.max.x - fx, f.max.y - fy),
    );
    footer_pair(ui, content, min, h, ok_enabled)
}

/// `footer_buttons` in the footer's content box `content`.
pub(super) fn footer_pair(
    ui: &mut Ui,
    content: Rect,
    min: f32,
    h: f32,
    ok_enabled: bool,
) -> Option<bool> {
    let by = content.center().y - h / 2.0;
    let cancel_w = crate::kit::window::dialog_button_width(ui.painter(), "Cancel", min);
    let ok_w = crate::kit::window::dialog_button_width(ui.painter(), "OK", min);
    let cancel = Rect::from_min_size(
        Pos2::new(content.max.x - cancel_w, by),
        Vec2::new(cancel_w, h),
    );
    let ok = Rect::from_min_size(
        Pos2::new(cancel.min.x - SIZE_FOOTER_GAP - ok_w, by),
        Vec2::new(ok_w, h),
    );
    let mut action = None;
    let ok_response = if ok_enabled {
        dialog_button(ui, "ok", ok, "OK", false)
    } else {
        inputs::push_button(ui, "ok", ok, "OK", false, false)
    };
    if ok_response.clicked() && ok_enabled {
        action = Some(true);
    }
    if dialog_button(ui, "cancel", cancel, "Cancel", false).clicked() {
        action = Some(false);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resize on the strip's picked images: the percent, the width at each image's own aspect
    /// ratio when maintained, else the very size.
    #[test]
    fn the_choice_applies_to_other_sizes() {
        let mut m = SizeModel::new(800.0, 600.0, true);
        m.set_width(400.0);
        assert_eq!(m.applied_to(1000.0, 1000.0), (400, 400));
        m.set_percent(50.0);
        assert_eq!(m.applied_to(300.0, 100.0), (150, 50));
        let mut m = SizeModel::new(800.0, 600.0, false);
        m.set_width(640.0);
        m.set_height(480.0);
        assert_eq!(m.applied_to(1000.0, 1000.0), (640, 480));
    }

    /// ui-drawn-graphics.md §2.11: doubles under the aspect lock; ⌊·⌉ at the end (1601 on 800×600 → 1201).
    #[test]
    fn aspect_lock_and_limits() {
        let mut m = SizeModel::new(800.0, 600.0, true);
        m.set_width(1601.0);
        assert_eq!(m.pixels(), (1601, 1201));
        m.set_percent(50.0);
        assert_eq!(m.pixels(), (400, 300));
        m.set_width(262_144.0);
        assert!(m.valid());
        m.set_width(262_145.0);
        assert!(!m.valid());
        m.set_percent(2001.0);
        assert!(!m.valid());
    }

    /// The height drives the width by × (W₀/H₀) under the lock, nothing unlocked; an absolute
    /// size moves the percent field (`toFixed(2)`); by percentage both sides follow W₀, H₀.
    #[test]
    fn height_ratio_unlocked_sides_and_percent_field() {
        let mut m = SizeModel::new(800.0, 600.0, true);
        m.set_height(301.0);
        assert_eq!((m.w, m.pixels()), (401.3333333333333, (401, 301)));
        assert_eq!(m.percent, 50.17);
        m.maintain = false;
        m.set_width(1000.0);
        assert_eq!(
            (m.pixels(), m.percent, m.by_percent),
            ((1000, 301), 125.0, false)
        );
        m.set_percent(25.0);
        assert_eq!((m.pixels(), m.by_percent), ((200, 150), true));
        m.set_height(0.4);
        assert!(!m.valid(), "rounds to 0");
    }

    /// R19: document-ops-and-formats.md §2.2 step 1 and §2.3 step 1.
    #[test]
    fn paint_net_defaults_and_choices() {
        let p = ImageSizePrefs::default();
        assert_eq!(
            (p.resize_maintain, p.resampling.as_str(), p.gamma_correct),
            (true, "cubic", true)
        );
        assert_eq!(
            (p.canvas_maintain, p.anchor.as_str(), p.fill.as_str()),
            (false, "topLeft", "transparent")
        );
        let ids: Vec<&str> = RESAMPLING_CHOICES.iter().map(|c| c.0).collect();
        assert_eq!(
            ids,
            [
                "cubic",
                "cubicSmooth",
                "linear",
                "linearLowQuality",
                "adaptiveHighQuality",
                "lanczos3",
                "fant",
                "nearestNeighbor"
            ]
        );
        for id in ids {
            assert!(
                pdn_core::resample::ResizeMode::from_value(id).is_some(),
                "{id}"
            );
        }
    }

    /// `loadPreferences` / `savePreferences`: paintdotjs's key and JSON keys, the stored values
    /// over the defaults, the old resampling ids mapped, unreadable JSON ignored.
    #[test]
    fn prefs_round_trip_with_paintdotjs_keys() {
        let mut prefs = Prefs::default();
        let p = ImageSizePrefs {
            resampling: "lanczos3".into(),
            anchor: "bottomRight".into(),
            canvas_maintain: true,
            ..ImageSizePrefs::default()
        };
        p.save(&mut prefs);
        let v: Value = serde_json::from_str(prefs.get(STORAGE_KEY).unwrap()).unwrap();
        assert_eq!(
            (v["resampling"].as_str(), v["canvasMaintain"].as_bool()),
            (Some("lanczos3"), Some(true))
        );
        assert_eq!(ImageSizePrefs::load(&prefs), p);
        prefs.set(STORAGE_KEY, r#"{"resampling":"high","fill":"black"}"#);
        let q = ImageSizePrefs::load(&prefs);
        assert_eq!(
            (q.resampling.as_str(), q.fill.as_str(), q.resize_maintain),
            ("adaptiveHighQuality", "black", true)
        );
        prefs.set(STORAGE_KEY, "{");
        assert_eq!(ImageSizePrefs::load(&prefs), ImageSizePrefs::default());
    }

    fn ok_with(
        mode: Mode,
        set: impl FnOnce(&mut ImageSizeDialog),
    ) -> (pdn_core::app::AppWorkspace, Prefs) {
        let mut app = pdn_core::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(8, 6, 96.0, true);
        let mut settings = pdn_core::settings::AppSettings::load(None);
        let mut prefs = Prefs::default();
        let mut d = ImageSizeDialog::new(mode, (8, 6, 96.0), 1);
        d.load(&prefs);
        set(&mut d);
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
        assert_eq!(
            d.ok(&mut cx),
            DialogState::Closed(super::super::DialogResult::Ok)
        );
        (app, prefs)
    }

    fn history(app: &pdn_core::app::AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    /// OK runs the operation with the dialog's choices and remembers them; Canvas Size anchors
    /// and fills as chosen (document-ops-and-formats.md §2.2, §2.3).
    #[test]
    fn ok_resizes_or_changes_the_canvas() {
        let (app, prefs) = ok_with(Mode::Resize, |d| {
            d.model.set_width(12.0);
            d.resampling = 7;
        });
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (12, 9));
        assert_eq!(history(&app).last().map(String::as_str), Some("Resize..."));
        let saved = ImageSizePrefs::load(&prefs);
        assert_eq!(
            (saved.resampling.as_str(), saved.resize_maintain),
            ("nearestNeighbor", true)
        );
        let (app, prefs) = ok_with(Mode::Canvas, |d| {
            d.model.set_width(10.0);
            d.anchor = 5; // Right
            d.fill = 4; // Black
        });
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (10, 6));
        let px = |x: u32| {
            let i = (x * 4) as usize;
            ws.document.layers.get_at(0).unwrap().surface.data()[i..i + 4].to_vec()
        };
        assert_eq!(
            (px(0), px(1), px(2)),
            (vec![0, 0, 0, 255], vec![0, 0, 0, 255], vec![255; 4])
        );
        assert_eq!(
            history(&app).last().map(String::as_str),
            Some("Canvas Size...")
        );
        assert_eq!(ImageSizePrefs::load(&prefs).anchor, "right");
    }

    /// "Use gamma-correct resampling" picks the scaler's light: Fant's 2→1 box average of black
    /// and white is linear 0.5 → sRGB 188 with it, 128 without (document-ops-and-formats.md §2.2
    /// step 5).
    #[test]
    fn gamma_choice_reaches_the_scaler() {
        for (gamma, want) in [(true, 188), (false, 128)] {
            let mut app = pdn_core::app::AppWorkspace::default();
            app.create_blank_document_in_new_workspace(2, 1, 96.0, true);
            let ws = app.active_ws_mut().unwrap();
            let layer = ws.document.layers.get_at_mut(0).unwrap();
            layer
                .surface
                .fill_rect(pdn_core::pixel::IntRect::new(0, 0, 1, 1), [0, 0, 0, 255]);
            let mut settings = pdn_core::settings::AppSettings::load(None);
            let mut prefs = Prefs::default();
            let mut d = ImageSizeDialog::new(Mode::Resize, (2, 1, 96.0), 1);
            d.load(&prefs);
            d.model.maintain = false;
            d.model.set_width(1.0);
            d.resampling = index_of(&RESAMPLING_CHOICES, "fant");
            d.gamma = gamma;
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
            d.ok(&mut cx);
            let ws = app.active_ws().unwrap();
            let v = ws.document.layers.get_at(0).unwrap().surface.data()[0];
            assert!((v as i32 - want).abs() <= 1, "gamma {gamma}: {v}");
        }
    }

    /// A failed Resize (the size check, `OpError::TooLarge`) leaves the image and shows
    /// `resizeAction.performAction.*`'s text in a task dialog.
    #[test]
    fn a_failed_operation_shows_its_error() {
        let pending = || super::super::PENDING.with(|p| p.borrow().len());
        let before = pending();
        let (app, _) = ok_with(Mode::Resize, |d| d.model.set_width(MAX_SIDE as f64 + 1.0));
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (8, 6));
        assert_eq!(pending(), before + 1);
        let title = super::super::PENDING.with(|p| p.borrow().last().map(|d| d.title()));
        assert_eq!(title.as_deref(), Some("Resize"));
        super::super::PENDING.with(|p| p.borrow_mut().clear());
    }
}
