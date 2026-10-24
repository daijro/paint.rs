use super::color_popup::color_popup_activated;
use super::special::{draw_pan, pan_thumbnail};
use super::widgets::{
    RangeSpec, SelectLook, SliderState, Track, button_clicked, canvas_image, checkbox_line,
    device_box, paint_canvas, range_line, range_rects, reset_button, select,
};
use super::{EffectConfigDialog, initial_editor, merge, num};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::px;
use crate::kit::text::{self, FontSpec};
use crate::theme::{self, *};
use egui::{Color32, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::canvas::ImageData;
use pdn_core::effect::bb::{self, ui::ControlUi, ui::IndirectUi, ui::Note, ui::Tab, ui::TabStrip};
use pdn_core::effect::{Control, DialogLayout, LabelText, PanThumb, Values};
use pdn_core::util::color::Color;
use serde_json::{Value, json};

impl EffectConfigDialog {
    /// The dialog's border-box width and its content box height (paddings included).
    pub(super) fn size(&self, ui: &Ui) -> (f32, f32) {
        let screen = ui.ctx().content_rect();
        let special = |w: f32| w.min(screen.width() - EFFECT_SPECIAL_MARGIN);
        let ppp = ui.ctx().pixels_per_point();
        let line = FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(ppp);
        match self.layout() {
            Some(DialogLayout::HueSaturation) => {
                let (t, _, b) = EFFECT_HUE_PADDING;
                let row = line + EFFECT_SPECIAL_GAP.0 + EFFECT_LINE_HEIGHT;
                (
                    special(EFFECT_HUE_WIDTH),
                    t + b + 3.0 * row + 2.0 * EFFECT_HUE_GAP.0,
                )
            }
            Some(DialogLayout::RotateZoom) => {
                let (t, _, b) = EFFECT_ROTATE_PADDING;
                let (gap, section, pan_margin, _, slider_gap) = EFFECT_ROTATE_GAPS;
                let roll = line + section + (3.0 * EFFECT_LINE_HEIGHT + 2.0 * slider_gap);
                let pan = line + section + (EFFECT_PAN_SIZE.1 + 2.0);
                let labelled = line + EFFECT_SPECIAL_GAP.1 + EFFECT_LINE_HEIGHT;
                let select = EFFECT_SPECIAL_SELECT.1;
                let h = roll + pan + pan_margin + 2.0 * labelled + 2.0 * select + 5.0 * gap;
                (special(EFFECT_ROTATE_WIDTH), t + b + h)
            }
            Some(DialogLayout::Curves) => {
                let (t, _, b) = EFFECT_CURVES_PADDING;
                let (gap, select_gap) = EFFECT_CURVES_GAPS;
                let transfer = line + select_gap + EFFECT_CURVES_SELECT.1;
                let channels = CHECKBOX_SIZE + 2.0 * CHECKBOX_MARGIN.0;
                let tip = EFFECT_TIP_MARGIN.0 + line + EFFECT_TIP_MARGIN.1;
                let h = transfer + EFFECT_CURVE_CANVAS.1 + channels + tip + 3.0 * gap;
                (special(EFFECT_CURVES_WIDTH), t + b + h)
            }
            Some(DialogLayout::Levels) => {
                let (t, _, b) = EFFECT_LEVELS_PADDING;
                let (r0, r1) = EFFECT_LEVELS_ROWS;
                (special(EFFECT_LEVELS_WIDTH), t + b + r0 + r1)
            }
            // render-chart-or-graph.md "Window 1": the 484 × 426 client, its own OK/Cancel.
            Some(DialogLayout::Chart) => (CHART_CLIENT.0 + 2.0 * DIALOG_BORDER, CHART_CLIENT.1),
            None => {
                let ind = self.session.as_ref().and_then(|s| s.def.indirect);
                let width = dialog_width(ind, screen.width());
                let controls = self.controls();
                let rows: Vec<f32> = match ind {
                    Some(u) => {
                        let w = width - 2.0 * (DIALOG_BORDER + EFFECT_CONTENT_PADDING.1);
                        let laid = self.laid_rows(ui.painter(), u, &controls, w);
                        laid.iter().map(|l| l.height).collect()
                    }
                    None => controls
                        .iter()
                        .filter_map(|c| generic_row_height(c, line))
                        .collect(),
                };
                let gaps = EFFECT_CONTENT_GAP * rows.len().saturating_sub(1) as f32;
                // ponytail: past the max height the rows are clipped (paintdotjs scrolls them);
                // add the kit scroll bar if an effect's control list ever needs it.
                let (min, max, inset) = EFFECT_CONTENT_HEIGHT;
                let content = (rows.iter().sum::<f32>() + gaps)
                    .max(min)
                    .min(max.min(screen.height() - inset));
                let (t, _, b) = EFFECT_CONTENT_PADDING;
                (width, t + b + content)
            }
        }
    }
}

/// A generic control row's height: label line, gap, then its line (`Value` and `Curve` draw
/// nothing generically).
fn generic_row_height(c: &Control, line: f32) -> Option<f32> {
    let label = line + EFFECT_LABEL_PADDING + 1.0 + EFFECT_ROW_GAP;
    match c {
        Control::Range { .. } => Some(label + EFFECT_LINE_HEIGHT),
        Control::Select { .. } => Some(label + EFFECT_SELECT_HEIGHT),
        Control::Checkbox { .. } => Some(EFFECT_CHECKBOX.2),
        Control::Color { .. } => Some(line + EFFECT_ROW_GAP + COLOR_INPUT.1),
        // BB5 lays out the plugin kinds.
        Control::Curve { .. }
        | Control::Value { .. }
        | Control::Text { .. }
        | Control::Label { .. }
        | Control::LayerSelect { .. }
        | Control::FontSelect { .. }
        | Control::File { .. }
        | Control::Pan { .. } => None,
    }
}

/// Chromium's `input[type=color]`: 50×27.
const COLOR_INPUT: (f32, f32) = (50.0, 27.0);

// ---- plugin dialogs: the IndirectUI layout (common.md §3; BB plan BB5) ----

/// A plugin dialog's row: a tab strip (its key's value is the selected tab) or a control.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Row {
    Tabs {
        key: &'static str,
        captions: Vec<&'static str>,
        selected: usize,
    },
    Control {
        key: &'static str,
        enabled: bool,
    },
}

/// common.md §3 "Tabs", "Enable rules": every top-level tab strip (one whose key no page lists),
/// each followed by its selected page's controls (a nested strip where its page lists it), then
/// the controls on no page, in definition order. `Value` controls never show; the selected index
/// is clamped to the strip.
pub(crate) fn indirect_rows(ui: &IndirectUi, controls: &[Control], values: &Values) -> Vec<Row> {
    let on_page = |k: &str| {
        ui.tabs
            .iter()
            .flat_map(|s| s.tabs)
            .any(|t| t.keys.contains(&k))
    };
    let mut rows = vec![];
    for strip in ui.tabs.iter().filter(|s| !on_page(s.key)) {
        push_strip(ui, strip, controls, values, &mut rows);
    }
    for c in controls.iter().filter(|c| !on_page(c.key())) {
        push_control(ui, c.key(), controls, values, &mut rows);
    }
    rows
}

fn push_strip(
    ui: &IndirectUi,
    strip: &TabStrip,
    controls: &[Control],
    values: &Values,
    rows: &mut Vec<Row>,
) {
    let selected = bb::index(values, strip.key).min(strip.tabs.len().saturating_sub(1));
    rows.push(Row::Tabs {
        key: strip.key,
        captions: strip.tabs.iter().map(|t| t.caption).collect(),
        selected,
    });
    for &k in strip.tabs.get(selected).map_or(&[][..], |t| t.keys) {
        match ui.tabs.iter().find(|t| t.key == k) {
            Some(inner) => push_strip(ui, inner, controls, values, rows),
            None => push_control(ui, k, controls, values, rows),
        }
    }
}

fn push_control(
    ui: &IndirectUi,
    key: &'static str,
    controls: &[Control],
    values: &Values,
    rows: &mut Vec<Row>,
) {
    if controls
        .iter()
        .any(|c| c.key() == key && !matches!(c, Control::Value { .. }))
    {
        rows.push(Row::Control {
            key,
            enabled: (ui.enabled)(values, key),
        });
    }
}

/// The generic dialog's border-box width: `EFFECT_DIALOG_WIDTH`, ×1.1 for a plugin that asks
/// (common.md §3 "Width"), within the viewport.
pub(crate) fn dialog_width(ui: Option<&IndirectUi>, screen_w: f32) -> f32 {
    (EFFECT_DIALOG_WIDTH * ui.map_or(1.0, |u| u.width)).min(screen_w - DIALOG_VIEWPORT_MARGIN)
}

/// A picked colour as a colour value: `"#rrggbb"`, or `"#rrggbbaa"` for a colour row "with
/// alpha" (common.md §3 "Colour wheel").
pub(crate) fn pick_hex(c: Color, alpha: bool) -> String {
    let rgb = format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue);
    if alpha {
        format!("{rgb}{:02x}", c.alpha)
    } else {
        rgb
    }
}

/// A pan value for the pointer at `p` (common.md §3 "Pan (offset) control"): (0, 0) at the centre
/// of the box `inner`, ±1 at the edges of the `fit`-sized thumbnail centred in it, clamped to
/// [min, max].
pub(crate) fn pan_from_pointer(
    p: Pos2,
    inner: Rect,
    fit: (f64, f64),
    min: f64,
    max: f64,
) -> [f64; 2] {
    let c = inner.center();
    [
        ((p.x - c.x) as f64 / (fit.0 / 2.0)).clamp(min, max),
        ((p.y - c.y) as f64 / (fit.1 / 2.0)).clamp(min, max),
    ]
}

/// A label's lines, top to bottom: a leading `\n` gives an empty first line.
pub(crate) fn label_lines(label: &str) -> Vec<&str> {
    label.split('\n').collect()
}

/// A plugin control's accessible name: its label's text, else its key.
fn accessible_name<'a>(label: &'a str, key: &'a str) -> &'a str {
    Some(label.trim()).filter(|n| !n.is_empty()).unwrap_or(key)
}

/// The text above a plugin control (a check box's label is its caption beside the box).
fn header_label(c: &Control) -> &'static str {
    match c {
        Control::Range { label, .. }
        | Control::Select { label, .. }
        | Control::Color { label, .. }
        | Control::Text { label, .. }
        | Control::Label { label, .. }
        | Control::LayerSelect { label, .. }
        | Control::FontSelect { label, .. }
        | Control::File { label, .. }
        | Control::Pan { label, .. } => label,
        Control::Checkbox { .. } | Control::Curve { .. } | Control::Value { .. } => "",
    }
}

/// A plugin row's texts: the header lines (none for an empty label), a static label's wrapped
/// lines, a file row's wrapped inline error, and the footnote's wrapped lines (`Some(vec![])`: a
/// single-space footnote's gap).
struct RowText {
    header: Vec<&'static str>,
    body: Vec<String>,
    error: Vec<String>,
    note: Option<Vec<String>>,
}

/// A laid-out plugin row and its height; a control row with its presentation and texts.
pub(super) struct Laid {
    row: Row,
    pub(super) height: f32,
    control: Option<(Control, ControlUi, RowText)>,
}

/// `text` broken into lines within `w` (`text::wrap_runs`: `\n` breaks, words wrap), without
/// their trailing spaces.
// spec-question: Q25 Long static labels and footnotes
fn wrap_text(painter: &egui::Painter, text: &str, spec: FontSpec, w: f32) -> Vec<String> {
    let runs = [text::Run {
        text,
        spec,
        color: Color32::WHITE,
        underline: false,
    }];
    text::wrap_runs(painter, &runs, w)
        .into_iter()
        .map(|l| {
            let line: String = l.into_iter().map(|p| p.text).collect();
            line.trim_end().to_string()
        })
        .collect()
}

/// A plugin control's own line(s) below its header.
fn body_height(c: &Control, t: &RowText, line: f32, ppp: f32) -> f32 {
    match c {
        Control::Range { .. } => EFFECT_LINE_HEIGHT,
        Control::Select { .. }
        | Control::LayerSelect { .. }
        | Control::FontSelect { .. }
        | Control::Text {
            multiline: false, ..
        } => EFFECT_SELECT_HEIGHT,
        Control::Text { .. } => inputs::text_area_height(EFFECT_TEXT_AREA_LINES, ppp),
        Control::Checkbox { .. } => EFFECT_CHECKBOX.2,
        Control::Color { .. } => COLOR_INPUT.1,
        Control::Label { .. } => t.body.len() as f32 * line,
        // The path line, then its inline error (fill-from-file.md "Existing file with an
        // extension no built-in type handles": shown inline).
        Control::File { .. } if t.error.is_empty() => EFFECT_SELECT_HEIGHT,
        Control::File { .. } => EFFECT_SELECT_HEIGHT + EFFECT_ROW_GAP + t.error.len() as f32 * line,
        Control::Pan { .. } => EFFECT_PAN_SIZE.1 + 2.0,
        Control::Curve { .. } | Control::Value { .. } => 0.0,
    }
}

/// A plugin control row's height: the header (its lines and the row gap; its header line runs
/// beside the last one, ruling R84), the body and the footnote (a gap, or the row gap and its
/// 10-DIP lines).
fn row_height(c: &Control, t: &RowText, ppp: f32) -> f32 {
    let line = FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(ppp);
    let header = if t.header.is_empty() {
        0.0
    } else {
        t.header.len() as f32 * line + EFFECT_ROW_GAP
    };
    let note_line = FontSpec::new(EFFECT_NOTE_FONT).resolved_line_height(ppp);
    let note = match &t.note {
        None => 0.0,
        Some(l) if l.is_empty() => EFFECT_NOTE_GAP,
        Some(l) => EFFECT_ROW_GAP + l.len() as f32 * note_line,
    };
    header + body_height(c, t, line, ppp) + note
}

/// A row's key.
fn row_key(row: &Row) -> &'static str {
    match row {
        Row::Tabs { key, .. } | Row::Control { key, .. } => key,
    }
}

/// Ruling R83 (Q24): a tab strip's block (its row and its selected page's rows) is as tall as its
/// tallest page, a WinForms tab control's fixed page area, so neither the dialog nor the rows below
/// move when the tab changes; the difference is a gap after the block's last row. `laid[i]` starts
/// a block (a control row is one of its own); returns the index after it. `extent` is a control's
/// height and the column gap after it.
// spec-question: Q24 Tab page height
fn pad_block(ind: &IndirectUi, laid: &mut [Laid], i: usize, extent: &dyn Fn(&str) -> f32) -> usize {
    let Row::Tabs { key, selected, .. } = laid[i].row else {
        return i + 1;
    };
    let Some(strip) = ind.tabs.iter().find(|s| s.key == key) else {
        return i + 1;
    };
    let mut j = i + 1;
    let mut used = laid[i].height + EFFECT_CONTENT_GAP;
    for &k in strip.tabs.get(selected).map_or(&[][..], |t| t.keys) {
        if laid.get(j).is_some_and(|l| row_key(&l.row) == k) {
            let end = pad_block(ind, laid, j, extent);
            used += laid[j..end]
                .iter()
                .map(|l| l.height + EFFECT_CONTENT_GAP)
                .sum::<f32>();
            j = end;
        }
    }
    laid[j - 1].height += (strip_extent(ind, strip, extent) - used).max(0.0);
    j
}

/// A strip's block at its tallest page: its row, then that page's rows (a nested strip at its own
/// tallest page), each with the column gap after it.
fn strip_extent(ind: &IndirectUi, strip: &TabStrip, extent: &dyn Fn(&str) -> f32) -> f32 {
    let page = |t: &Tab| -> f32 {
        t.keys
            .iter()
            .map(|k| match ind.tabs.iter().find(|s| s.key == *k) {
                Some(inner) => strip_extent(ind, inner, extent),
                None => extent(k),
            })
            .sum()
    };
    let tallest = strip.tabs.iter().map(page).fold(0.0, f32::max);
    EFFECT_TAB_HEIGHT + EFFECT_CONTENT_GAP + tallest
}

/// common.md §3 (a drop-down's caption beside its box): each row's box offset, the widest
/// caption of its run of consecutive captioned drop-downs plus `EFFECT_CAPTION_GAP` (0 otherwise).
fn caption_columns(laid: &[Laid]) -> Vec<f32> {
    let captioned = |l: &Laid| {
        l.control
            .as_ref()
            .filter(|(c, cu, _)| {
                matches!(
                    c,
                    Control::Select { .. }
                        | Control::LayerSelect { .. }
                        | Control::FontSelect { .. }
                ) && !cu.caption.is_empty()
            })
            .map(|(_, cu, _)| text::css_width(cu.caption, DIALOG_FONT_SIZE))
    };
    let mut out = vec![0.0; laid.len()];
    let mut i = 0;
    while i < laid.len() {
        let start = i;
        let mut widest = 0.0f32;
        while let Some(w) = laid.get(i).and_then(captioned) {
            widest = widest.max(w);
            i += 1;
        }
        for o in &mut out[start..i] {
            *o = widest + EFFECT_CAPTION_GAP;
        }
        i = i.max(start + 1);
    }
    out
}

/// Tab `i` of the strip `key`.
fn tab_id(ui: &Ui, key: &str, i: usize) -> Id {
    ui.scope_id().with(("tab", key, i))
}

/// A plugin tab strip at `y` (common.md §3 "Tabs"; BB plan BB5): one row of captions from
/// `x0`, each padded `EFFECT_TAB_PADDING`; the selected tab in the window background with a 1-px
/// `--pdn-control-border` frame open at the bottom, the others `--pdn-separator`; a 1-px
/// `--pdn-control-border` line under the strip across `r`. As a WinForms tab control: only the
/// selected tab is a Tab stop (a dotted focus rectangle inside it while focused); there
/// ArrowLeft / ArrowRight pick the previous / next tab, and `ctrl_tab` (±1, Ctrl+Tab and
/// Ctrl+Shift+Tab, given to the strip that has the focus or else the first) cycles. A click (also
/// AccessKit's) picks a tab; Enter on the focused tab is the dialog's OK (`inputs::toggled`'s
/// rule). Returns the picked tab.
#[allow(clippy::too_many_arguments)]
fn tab_strip(
    ui: &mut Ui,
    r: Rect,
    x0: f32,
    y: f32,
    key: &str,
    captions: &[&str],
    selected: usize,
    ctrl_tab: Option<isize>,
) -> Option<usize> {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let n = captions.len();
    let focus_id = tab_id(ui, key, selected);
    let focused = ui.memory(|m| m.has_focus(focus_id));
    let mut picked = None;
    if focused {
        // The arrows are the strip's, not egui's focus moves.
        let filter = egui::EventFilter {
            horizontal_arrows: true,
            ..Default::default()
        };
        ui.memory_mut(|m| m.set_focus_lock_filter(focus_id, filter));
        let (left, right) = ui.input(|i| {
            let plain = !i.modifiers.any();
            (
                plain && i.key_pressed(Key::ArrowLeft),
                plain && i.key_pressed(Key::ArrowRight),
            )
        });
        if left && selected > 0 {
            picked = Some(selected - 1);
        } else if right && selected + 1 < n {
            picked = Some(selected + 1);
        }
    }
    if let Some(d) = ctrl_tab
        && n > 1
    {
        picked = Some((selected as isize + d).rem_euclid(n as isize) as usize);
    }
    let b = px::border_width(1.0, ppp);
    let spec = FontSpec::new(DIALOG_FONT_SIZE);
    let line = spec.resolved_line_height(ppp);
    let bottom = y + EFFECT_TAB_HEIGHT;
    let under = px::snap_rect(
        Rect::from_min_max(Pos2::new(r.min.x, bottom - 1.0), Pos2::new(r.max.x, bottom)),
        ppp,
    );
    painter.rect_filled(
        Rect::from_min_size(under.min, Vec2::new(under.width(), b)),
        0.0,
        theme.pdn_control_border,
    );
    let mut x = x0;
    for (i, caption) in captions.iter().enumerate() {
        let tw = text::css_width(caption, spec.size) + 2.0 * EFFECT_TAB_PADDING;
        let tab = px::snap_rect(
            Rect::from_min_size(Pos2::new(x, y), Vec2::new(tw, EFFECT_TAB_HEIGHT)),
            ppp,
        );
        let id = tab_id(ui, key, i);
        let sense = if i == selected {
            Sense::click()
        } else {
            Sense::CLICK
        };
        let response = ui.interact(tab, id, sense);
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_role(egui::accesskit::Role::Tab);
            n.set_label(*caption);
            n.set_selected(i == selected);
            // egui fills these for focusable widgets only.
            n.set_bounds(egui::accesskit::Rect {
                x0: tab.min.x.into(),
                y0: tab.min.y.into(),
                x1: tab.max.x.into(),
                y1: tab.max.y.into(),
            });
            n.add_action(egui::accesskit::Action::Click);
        });
        if i == selected {
            painter.rect_filled(tab, 0.0, theme.pdn_control_border);
            painter.rect_filled(
                Rect::from_min_max(
                    tab.min + Vec2::splat(b),
                    Pos2::new(tab.max.x - b, tab.max.y),
                ),
                0.0,
                theme.window_background,
            );
            if focused {
                focus_dots(&painter, tab.shrink(3.0 * b), theme.menu_item_text);
            }
        } else {
            painter.rect_filled(
                Rect::from_min_max(tab.min, Pos2::new(tab.max.x, under.min.y)),
                0.0,
                theme.pdn_separator,
            );
        }
        text::draw(
            &painter,
            Pos2::new(x + EFFECT_TAB_PADDING, y + (EFFECT_TAB_HEIGHT - line) / 2.0),
            caption,
            spec,
            theme.menu_item_text,
        );
        let enter = response.has_focus() && ui.input(|inp| inp.key_pressed(Key::Enter));
        if response.clicked() && !enter && i != selected {
            picked = Some(i);
        }
        x += tw + EFFECT_TAB_GAP;
    }
    // The focus follows a keyboard pick (requested after the new tab registered this frame, so
    // its unfocusable pass does not drop it; it is the Tab stop from the next frame).
    if focused && let Some(i) = picked {
        ui.memory_mut(|m| m.request_focus(tab_id(ui, key, i)));
    }
    picked
}

/// Windows's focus cue: a 1-px dotted rectangle (every other device pixel) along `r`'s edges.
fn focus_dots(painter: &egui::Painter, r: Rect, color: Color32) {
    let ppp = painter.pixels_per_point();
    let r = px::snap_rect(r, ppp);
    let (w, h) = (
        (r.width() * ppp).round() as i32,
        (r.height() * ppp).round() as i32,
    );
    let dot = |x: i32, y: i32| {
        let at = r.min + Vec2::new(x as f32, y as f32) / ppp;
        painter.rect_filled(Rect::from_min_size(at, Vec2::splat(1.0 / ppp)), 0.0, color);
    };
    for x in (0..w).step_by(2) {
        dot(x, 0);
        dot(x, h - 1);
    }
    for y in (0..h).step_by(2) {
        dot(0, y);
        dot(w - 1, y);
    }
}

/// A pan thumbnail's size: a `w`×`h` image fitted into 90×66 (`createPanVisual`).
pub(super) fn pan_fit(w: u32, h: u32) -> (f64, f64) {
    let fit = (90.0 / w as f64).min(66.0 / h as f64);
    (w as f64 * fit, h as f64 * fit)
}

/// The part of `src` inside `r` (the selection bounds, within the layer).
fn crop(src: &ImageData, r: pdn_core::pixel::IntRect) -> ImageData {
    let (x, y) = (r.x.max(0) as usize, r.y.max(0) as usize);
    let w = (r.w as usize).min(src.width as usize - x.min(src.width as usize));
    let h = (r.h as usize).min(src.height as usize - y.min(src.height as usize));
    let mut data = Vec::with_capacity(w * h * 4);
    for j in y..y + h {
        let at = (j * src.width as usize + x) * 4;
        data.extend_from_slice(&src.data[at..at + w * 4]);
    }
    ImageData {
        width: w as u32,
        height: h as u32,
        data,
    }
}

/// `.effect-config-row > label`'s bottom border: the 1-px `--pdn-separator` line at `y` across
/// `w` from `x0`.
fn separator(ui: &Ui, x0: f32, w: f32, y: f32) {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let b = px::border_width(1.0, ppp);
    let at = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, 1.0));
    let s = px::snap_rect(at, ppp);
    ui.painter().rect_filled(
        Rect::from_min_size(s.min, Vec2::new(s.width(), b)),
        0.0,
        theme.pdn_separator,
    );
}

/// A font drop-down's (family, display name) entries from the app's font collection (Task 53;
/// the bundled list before it has synced) in Calendar's order (R154 Q61), and the index of
/// `value`, which is listed first when the collection no longer has it.
pub(super) fn font_select_entries(
    collection: Option<Vec<(String, String)>>,
    value: &str,
) -> (Vec<(String, String)>, usize) {
    let collection = collection.unwrap_or_else(|| {
        pdn_core::fonts::text_tool_choices()
            .into_iter()
            .map(|(f, n)| (f.into(), n.into()))
            .collect()
    });
    let mut entries = bb::calendar::font_choices(&collection);
    let index = entries
        .iter()
        .position(|e| e.0.eq_ignore_ascii_case(value))
        .unwrap_or_else(|| {
            entries.insert(0, (value.into(), value.into()));
            0
        });
    (entries, index)
}

/// The index of the select `key`'s value in `choices` (the first when it is none of them).
fn choice_index(choices: &[(&str, &str)], values: &Values, key: &str) -> usize {
    choices
        .iter()
        .position(|c| c.0 == pdn_core::effect::choice(values, key))
        .unwrap_or(0)
}

/// What a colour row's `"primary"`/`"secondary"` values show: the current palette.
fn palette_env(palette: [Color; 2]) -> pdn_core::effect::EffectEnv<'static> {
    pdn_core::effect::EffectEnv {
        primary: color_px(palette[0]),
        secondary: color_px(palette[1]),
        ..pdn_core::effect::EffectEnv::whole(1, 1)
    }
}

impl EffectConfigDialog {
    /// `createGenericControl` for each control, in a column with 10-px gaps.
    pub(super) fn generic(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        controls: &[Control],
        now: f64,
        palette: [pdn_core::util::color::Color; 2],
    ) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let (pt, px_, _) = EFFECT_CONTENT_PADDING;
        let x0 = r.min.x + px_;
        let w = r.width() - 2.0 * px_;
        let mut y = r.min.y + pt;
        let mut changed = None;
        let mut state = self.slider_state(now);
        for c in controls {
            let Some(h) = generic_row_height(c, line) else {
                continue;
            };
            let label_h = line + EFFECT_LABEL_PADDING + 1.0;
            if let Control::Range { label, .. } | Control::Select { label, .. } = c
                && !label.is_empty()
            {
                text::draw(
                    ui.painter(),
                    Pos2::new(x0, y),
                    label,
                    spec,
                    theme.menu_item_text,
                );
                separator(ui, x0, w, y + line + EFFECT_LABEL_PADDING);
            }
            let line_top = y + label_h + EFFECT_ROW_GAP;
            match c {
                Control::Range { .. } => {
                    let (key, _, rs) =
                        RangeSpec::of(c, self.session.as_ref().map_or("", |s| s.def.id))
                            .expect("a range");
                    let (range, number, reset) = range_rects(x0, w, line_top);
                    let mut style = NumberStyle::EFFECT;
                    if rs.role == pdn_core::effect::slider::RangeRole::BuiltinAngle {
                        style.decimals = Some(2);
                        style.native_step = true;
                        style.wrap = rs.max - rs.min == 360.0;
                    }
                    let c = range_line(
                        ui,
                        &mut state,
                        key,
                        rs,
                        &mut self.values,
                        range,
                        number,
                        Some(reset),
                        Track::Progress(0.0),
                        style,
                    );
                    changed = merge(changed, c);
                }
                Control::Select {
                    key,
                    label,
                    choices,
                    ..
                } => {
                    let at = Rect::from_min_size(
                        Pos2::new(x0, line_top),
                        Vec2::new(w, EFFECT_SELECT_HEIGHT),
                    );
                    let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
                    let current = choice_index(choices, &self.values, key);
                    let look = SelectLook {
                        native: true,
                        pad_left: 0.0,
                        font: spec,
                    };
                    if let Some(i) = select(
                        ui,
                        &mut self.open_select,
                        ui.scope_id().with(("select", *key)),
                        at,
                        label,
                        &labels,
                        current,
                        look,
                    ) {
                        self.values.insert((*key).into(), json!(choices[i].0));
                        changed = Some(true);
                    }
                }
                Control::Checkbox { key, label, .. } => {
                    let color = theme.menu_item_text;
                    let c = checkbox_line(ui, &mut self.values, key, label, x0, y, w, color, 1.0);
                    changed = merge(changed, c);
                }
                Control::Color { key, label, .. } => {
                    if !label.is_empty() {
                        text::draw(
                            ui.painter(),
                            Pos2::new(x0, y),
                            label,
                            spec,
                            theme.menu_item_text,
                        );
                    }
                    let at = Rect::from_min_size(
                        Pos2::new(x0, y + line + EFFECT_ROW_GAP),
                        Vec2::from(COLOR_INPUT),
                    );
                    let env = palette_env(palette);
                    let shown = pdn_core::effect::color_value(&self.values, key, &env);
                    let response = inputs::color_input(ui, at, shown);
                    initial_editor(ui, response.id);
                    ui.ctx().accesskit_node_builder(response.id, |n| {
                        n.set_role(egui::accesskit::Role::Button);
                        n.set_label(accessible_name(label, key));
                    });
                    if color_popup_activated(ui, &response) {
                        let [r, g, b, a] = shown;
                        self.open_color_popup(key, Color::new(r, g, b, a), false, false, &response);
                    } else if response.secondary_clicked() {
                        self.values
                            .insert((*key).into(), json!(pick_hex(palette[1], false)));
                        changed = Some(true);
                    }
                }
                Control::Curve { .. }
                | Control::Value { .. }
                | Control::Text { .. }
                | Control::Label { .. }
                | Control::LayerSelect { .. }
                | Control::FontSelect { .. }
                | Control::File { .. }
                | Control::Pan { .. } => {}
            }
            y += h + EFFECT_CONTENT_GAP;
        }
        self.put_slider_state(state);
        changed
    }
}

/// The plugin path (common.md §3; BB plan BB5).
impl EffectConfigDialog {
    /// `indirect_rows` laid out across `w`, each with its height.
    pub(super) fn laid_rows(
        &self,
        painter: &egui::Painter,
        ind: &IndirectUi,
        controls: &[Control],
        w: f32,
    ) -> Vec<Laid> {
        let ppp = painter.pixels_per_point();
        let control = |key: &str| {
            Self::find(controls, key)
                .filter(|c| !matches!(c, Control::Value { .. }))
                .map(|c| {
                    let cu = ind.control(key);
                    let t = self.row_text(painter, &c, cu, w);
                    (c, cu, t)
                })
        };
        let mut laid: Vec<Laid> = indirect_rows(ind, controls, &self.values)
            .into_iter()
            .map(|row| {
                let control = match &row {
                    Row::Control { key, .. } => control(key),
                    Row::Tabs { .. } => None,
                };
                let height = control
                    .as_ref()
                    .map_or(EFFECT_TAB_HEIGHT, |(c, _, t)| row_height(c, t, ppp));
                Laid {
                    row,
                    height,
                    control,
                }
            })
            .collect();
        let extent = |key: &str| {
            control(key).map_or(0.0, |(c, _, t)| {
                row_height(&c, &t, ppp) + EFFECT_CONTENT_GAP
            })
        };
        let mut i = 0;
        while i < laid.len() {
            i = pad_block(ind, &mut laid, i, &extent);
        }
        laid
    }

    fn row_text(&self, painter: &egui::Painter, c: &Control, cu: ControlUi, w: f32) -> RowText {
        let label = cu.header.unwrap_or_else(|| header_label(c));
        let header = if label.is_empty() {
            vec![]
        } else {
            label_lines(label)
        };
        let body = match c {
            Control::Label { text, .. } => {
                let s = match text {
                    LabelText::Fixed(s) => s.to_string(),
                    LabelText::CurrentLayer(prefix) => {
                        let name = self
                            .session
                            .as_ref()
                            .and_then(|s| s.layer_names().get(s.env().layer_index).cloned());
                        format!("{prefix}{}", name.unwrap_or_default())
                    }
                };
                wrap_text(painter, &s, FontSpec::new(DIALOG_FONT_SIZE), w)
            }
            _ => vec![],
        };
        let error = match (c, &self.file_error) {
            (Control::File { .. }, Some(e)) => {
                wrap_text(painter, e, FontSpec::new(DIALOG_FONT_SIZE), w)
            }
            _ => vec![],
        };
        let note = match cu.note {
            Note::None => None,
            Note::Text(t) => Some(t.to_string()),
            Note::Env(f) => self.session.as_ref().map(|s| f(&s.env())),
        }
        .map(|t| {
            if t == " " {
                vec![]
            } else {
                wrap_text(painter, &t, FontSpec::new(EFFECT_NOTE_FONT), w)
            }
        });
        RowText {
            header,
            body,
            error,
            note,
        }
    }

    /// The plugin rows in a column with 10-px gaps (the generic dialog's).
    pub(super) fn indirect(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        ind: &IndirectUi,
        controls: &[Control],
        now: f64,
        palette: [Color; 2],
    ) -> Option<bool> {
        let (pt, px_, _) = EFFECT_CONTENT_PADDING;
        let x0 = r.min.x + px_;
        let w = r.width() - 2.0 * px_;
        let laid = self.laid_rows(ui.painter(), ind, controls, w);
        let columns = caption_columns(&laid);
        let defaults = self
            .session
            .as_ref()
            .map(|s| s.defaults())
            .unwrap_or_default();
        // Ctrl+Tab / Ctrl+Shift+Tab: the strip whose tab has the focus, else the first.
        let ctrl_tab = ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::Tab) {
                Some(-1)
            } else if i.consume_key(egui::Modifiers::CTRL, Key::Tab) {
                Some(1)
            } else {
                None
            }
        });
        let strips = laid.iter().filter_map(|l| match l.row {
            Row::Tabs { key, selected, .. } => Some((key, selected)),
            Row::Control { .. } => None,
        });
        let cycled = strips
            .clone()
            .find(|(k, s)| ui.memory(|m| m.has_focus(tab_id(ui, k, *s))))
            .or_else(|| strips.clone().next())
            .map(|(k, _)| k);
        let mut state = self.slider_state(now);
        let mut changed = None;
        let mut y = r.min.y + pt;
        for (l, col) in laid.iter().zip(columns) {
            let c = match (&l.row, &l.control) {
                (
                    Row::Tabs {
                        key,
                        captions,
                        selected,
                    },
                    _,
                ) => {
                    let ctrl_tab = ctrl_tab.filter(|_| cycled == Some(*key));
                    tab_strip(ui, r, x0, y, key, captions, *selected, ctrl_tab).map(|i| {
                        self.values.insert((*key).into(), json!(i));
                        true
                    })
                }
                (Row::Control { enabled, .. }, Some(ctl)) => {
                    let at = Pos2::new(x0, y);
                    let cx = (&defaults, palette);
                    self.plugin_row(ui, &mut state, ctl, *enabled, at, w, col, cx)
                }
                (Row::Control { .. }, None) => None,
            };
            changed = merge(changed, c);
            y += l.height + EFFECT_CONTENT_GAP;
        }
        self.put_slider_state(state);
        changed
    }

    /// One plugin control row at `at` across `w`, its drop-down box `col` right of its caption:
    /// the header lines (its header line right of the last one, ruling R84), the control, then its
    /// footnote. A disabled row (common.md §3 "Enable rules") takes no pointer or keys and keeps
    /// its value: its texts in `MENU_ITEM_DISABLED_TEXT`, its controls faded like a disabled
    /// number box. `cx` the session defaults (the reset arrows') and the palette.
    #[allow(clippy::too_many_arguments)]
    fn plugin_row(
        &mut self,
        ui: &mut Ui,
        state: &mut SliderState,
        (c, cu, t): &(Control, ControlUi, RowText),
        enabled: bool,
        at: Pos2,
        w: f32,
        col: f32,
        (defaults, palette): (&Values, [Color; 2]),
    ) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let key = c.key();
        // Its own scope: the ids stay the same whether the row is enabled or not.
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("row", key))
                .max_rect(ui.max_rect()),
        );
        let base = ui.opacity();
        if !enabled {
            row.disable();
        }
        row.set_opacity(base);
        let color = if enabled {
            theme.menu_item_text
        } else {
            MENU_ITEM_DISABLED_TEXT
        };
        let fade = if enabled {
            1.0
        } else {
            NUMBER_DISABLED_OPACITY
        };
        let x0 = at.x;
        let mut y = at.y;
        if !t.header.is_empty() {
            for l in &t.header {
                text::draw(row.painter(), Pos2::new(x0, y), l, spec, color);
                y += line;
            }
            // Ruling R84 (common.md §3 "Header label"): the rule runs right of the label's last
            // line, through its middle.
            let last = t.header.last().copied().unwrap_or("");
            let start = text::measure(row.painter(), last, spec) + EFFECT_HEADER_RULE_GAP;
            if cu.header_line && start < w {
                separator(&row, x0 + start, w - start, y - line / 2.0);
            }
            y += EFFECT_ROW_GAP;
        }
        row.set_opacity(base * fade);
        let mut changed = None;
        match c {
            Control::Range { .. } => {
                let (key, _, mut rs) =
                    RangeSpec::of(c, self.session.as_ref().map_or("", |s| s.def.id))?;
                if let Some(d) = defaults.get(key).and_then(Value::as_f64) {
                    rs.default = d;
                }
                // R154: the session's limits (Calendar's year), for keys, typing and pointer.
                if let (Some(range), Some(s)) = (cu.range, self.session.as_ref()) {
                    (rs.min, rs.max) = range(&s.env());
                }
                let (range, number, reset) = range_rects(x0, w, y);
                let track = cu
                    .track
                    .map_or(Track::Progress(0.0), |stops| Track::Special(Some(stops)));
                let mut style = NumberStyle::EFFECT.wrap(cu.wrap);
                style.decimals = cu.decimals;
                style.native_step = true;
                let values = &mut self.values;
                let reset = cu.reset.then_some(reset);
                changed = range_line(
                    &mut row, state, key, rs, values, range, number, reset, track, style,
                );
            }
            Control::Select { label, choices, .. } => {
                // R154: option labels from the session env (Calendar's culture month names).
                let env = self.session.as_ref().map(|s| s.env());
                let named: Vec<String> = choices
                    .iter()
                    .map(|(v, l)| {
                        (cu.choice_label.zip(env.as_ref()))
                            .and_then(|(f, env)| f(v, env))
                            .unwrap_or_else(|| l.to_string())
                    })
                    .collect();
                let labels: Vec<&str> = named.iter().map(String::as_str).collect();
                let current = choice_index(choices, &self.values, key);
                let names = [cu.caption, label, key];
                let look = (Pos2::new(x0, y), w, col, color, base, fade);
                if let Some(i) = self.captioned_select(&mut row, names, &labels, current, look) {
                    self.values.insert(key.into(), json!(choices[i].0));
                    changed = Some(true);
                }
            }
            Control::FontSelect { label, .. } => {
                let fonts = crate::platform::fonts::choices(row.ctx());
                let (entries, current) = font_select_entries(fonts, bb::text(&self.values, key));
                let labels: Vec<&str> = entries.iter().map(|e| e.1.as_str()).collect();
                let names = [cu.caption, label, key];
                let look = (Pos2::new(x0, y), w, col, color, base, fade);
                if let Some(i) = self.captioned_select(&mut row, names, &labels, current, look) {
                    self.values.insert(key.into(), json!(entries[i].0));
                    changed = Some(true);
                }
            }
            Control::LayerSelect { label, .. } => {
                let layers = self
                    .session
                    .as_ref()
                    .map(|s| s.layer_names())
                    .unwrap_or_default();
                let labels: Vec<&str> = layers.iter().map(String::as_str).collect();
                let current = bb::index(&self.values, key).min(labels.len().saturating_sub(1));
                let names = [cu.caption, label, key];
                let look = (Pos2::new(x0, y), w, col, color, base, fade);
                if let Some(i) = self.captioned_select(&mut row, names, &labels, current, look) {
                    self.values.insert(key.into(), json!(i));
                    changed = Some(true);
                }
            }
            Control::Checkbox { label, .. } => {
                row.set_opacity(base);
                let values = &mut self.values;
                changed = checkbox_line(&mut row, values, key, label, x0, y, w, color, fade);
            }
            Control::Color { .. } => {
                let env = palette_env(palette);
                let at = Rect::from_min_size(Pos2::new(x0, y), Vec2::from(COLOR_INPUT));
                let response = if cu.alpha {
                    let shown = bb::color_value_alpha(&self.values, key, &env);
                    inputs::color_input_alpha(&mut row, at, shown)
                } else {
                    let shown = pdn_core::effect::color_value(&self.values, key, &env);
                    inputs::color_input(&mut row, at, shown)
                };
                initial_editor(&row, response.id);
                let name = accessible_name(header_label(c), key);
                row.ctx()
                    .accesskit_node_builder(response.id, |n| n.set_label(name));
                row.ctx().accesskit_node_builder(response.id, |n| {
                    n.set_role(egui::accesskit::Role::Button)
                });
                if color_popup_activated(&row, &response) {
                    let [r, g, b, a] = if cu.alpha {
                        bb::color_value_alpha(&self.values, key, &env)
                    } else {
                        pdn_core::effect::color_value(&self.values, key, &env)
                    };
                    self.open_color_popup(key, Color::new(r, g, b, a), cu.alpha, false, &response);
                } else if response.secondary_clicked() {
                    self.values
                        .insert(key.into(), json!(pick_hex(palette[1], cu.alpha)));
                    changed = Some(true);
                }
                let (_, rw, gap) = EFFECT_LINE_COLUMNS;
                let reset = Rect::from_min_size(
                    Pos2::new(at.max.x + gap, y + (COLOR_INPUT.1 - rw) / 2.0),
                    Vec2::splat(rw),
                );
                let id = row.scope_id().with(("reset", key));
                if cu.reset && reset_button(&mut row, id, reset) {
                    self.reset_control(key);
                    changed = Some(true);
                }
            }
            Control::Text {
                label,
                max_len,
                multiline,
                ..
            } => {
                let mut s = bb::text(&self.values, key).to_string();
                let before = s.clone();
                let h = body_height(c, t, line, ppp);
                let rect = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, h));
                // R118: the capacity applies to insertions only, in UTF-16 units. R178: a
                // multiline Enter (plain, Shift or Ctrl) inserts one LF (`inputs::text_area`).
                let mut buffer = inputs::Utf16Text {
                    text: &mut s,
                    max_units: *max_len as usize,
                };
                let response = if *multiline {
                    inputs::text_area(&mut row, ("text", key), rect, &mut buffer)
                } else {
                    inputs::text_field(&mut row, ("text", key), rect, &mut buffer)
                };
                initial_editor(&row, response.id);
                let name = accessible_name(label, key);
                row.ctx()
                    .accesskit_node_builder(response.id, |n| n.set_label(name));
                if s != before {
                    self.values.insert(key.into(), json!(s));
                    changed = Some(false);
                }
            }
            Control::Label { .. } => {
                row.set_opacity(base);
                for (i, l) in t.body.iter().enumerate() {
                    let at = Pos2::new(x0, y + i as f32 * line);
                    text::draw(row.painter(), at, l, spec, color);
                }
            }
            Control::File {
                label,
                filters,
                all_files,
                ..
            } => {
                // common.md §3 "File chooser": the editable path, then Paint.NET's "..." button
                // in the reset column.
                let ((_, bw, gap), h) = (EFFECT_LINE_COLUMNS, EFFECT_SELECT_HEIGHT);
                let field = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w - bw - gap, h));
                let mut s = bb::text(&self.values, key).to_string();
                let before = s.clone();
                let response = inputs::text_field(&mut row, ("file", key), field, &mut s);
                initial_editor(&row, response.id);
                let name = accessible_name(label, key);
                row.ctx()
                    .accesskit_node_builder(response.id, |n| n.set_label(name));
                if s != before {
                    // R156: an actual edit detaches the pick; a name never recovers it.
                    self.candidate = None;
                    self.values.insert(key.into(), json!(s));
                    changed = Some(false);
                }
                let browse = Rect::from_min_size(Pos2::new(x0 + w - bw, y), Vec2::new(bw, h));
                let response =
                    inputs::push_button(&mut row, ("browse", key), browse, "...", enabled, false);
                inputs::take_enter(&row, &response);
                row.ctx()
                    .accesskit_node_builder(response.id, |n| n.set_label("Browse"));
                if button_clicked(&row, &response) && changed.is_none() {
                    // R156: the chooser starts in this activation and leaves the load alone.
                    let accept: Vec<String> = filters.iter().map(|e| format!(".{e}")).collect();
                    let accept = accept.join(",");
                    self.chooser = Some(self.files.choose_open_with(false, &accept, *all_files));
                    changed = self.poll_chooser();
                }
                row.set_opacity(base);
                for (i, l) in t.error.iter().enumerate() {
                    let at = Pos2::new(x0, y + h + EFFECT_ROW_GAP + i as f32 * line);
                    text::draw(row.painter(), at, l, spec, theme.error_text);
                }
            }
            Control::Pan {
                min,
                max,
                step,
                thumb,
                default,
                ..
            } => {
                let size = Vec2::new(EFFECT_PAN_SIZE.0 + 2.0, EFFECT_PAN_SIZE.1 + 2.0);
                let pan = Rect::from_min_size(Pos2::new(x0, y), size);
                let name = accessible_name(header_label(c), key);
                changed = self.pan_box(&mut row, (key, name), pan, *thumb, (*min, *max));
                // X and Y: two number boxes right of the box, centred on it.
                let mut v = bb::pan(&self.values, key);
                let (nw, nh) = (EFFECT_LINE_COLUMNS.0, EFFECT_NUMBER_HEIGHT);
                let lx = pan.max.x + EFFECT_PAN_ROW_GAP;
                let top = y + (pan.height() - 2.0 * nh - EFFECT_PAN_ROW_GAP) / 2.0;
                for (i, axis) in ["X", "Y"].into_iter().enumerate() {
                    let ty = top + i as f32 * (nh + EFFECT_PAN_ROW_GAP);
                    row.set_opacity(base);
                    let label_at = Pos2::new(lx, ty + (nh - line) / 2.0);
                    text::draw(row.painter(), label_at, axis, spec, color);
                    row.set_opacity(base * fade);
                    let number = Rect::from_min_size(
                        Pos2::new(lx + EFFECT_PAN_ROW_LABEL, ty),
                        Vec2::new(nw, nh),
                    );
                    let before = v[i];
                    let mut style = NumberStyle::EFFECT;
                    style.decimals = cu.decimals;
                    style.native_step = true;
                    if let Some(keyboard) = cu.pan_sliders {
                        // R104: X slider, X number, X Reset, then Y; the mouse pad stays out of Tab.
                        let axis_key = format!("{key}.{axis}");
                        let mut axis_values = Values::from_iter([(axis_key.clone(), json!(v[i]))]);
                        let axis_default = defaults
                            .get(key)
                            .and_then(Value::as_array)
                            .and_then(|v| v.get(i))
                            .and_then(Value::as_f64)
                            .unwrap_or(default[i]);
                        let rs = RangeSpec {
                            min: *min,
                            max: *max,
                            default: axis_default,
                            step: *step,
                            role: pdn_core::effect::slider::RangeRole::Slider,
                            keyboard,
                        };
                        let start = lx + EFFECT_PAN_ROW_LABEL;
                        let (range, number, reset) = range_rects(start, x0 + w - start, ty);
                        let result = range_line(
                            &mut row,
                            state,
                            &axis_key,
                            rs,
                            &mut axis_values,
                            range,
                            number,
                            cu.reset.then_some(reset),
                            Track::Progress(0.0),
                            style,
                        );
                        v[i] = num(&axis_values, &axis_key);
                        if v[i] != before {
                            self.set_value(key, json!(v));
                        }
                        changed = merge(changed, result);
                        continue;
                    }
                    let id = ("pan", key, i);
                    let response = inputs::number_input(
                        &mut row, id, axis, number, &mut v[i], *min, *max, *step, true, style,
                    );
                    initial_editor(&row, response.id);
                    if v[i] != before {
                        self.set_value(key, json!(v));
                        changed = merge(changed, Some(false));
                    }
                }
            }
            Control::Value { .. } | Control::Curve { .. } => {}
        }
        row.set_opacity(base);
        y += body_height(c, t, line, ppp);
        if let Some(lines) = &t.note {
            let note = FontSpec::new(EFFECT_NOTE_FONT);
            let note_line = note.resolved_line_height(ppp);
            for (i, l) in lines.iter().enumerate() {
                let at = Pos2::new(x0, y + EFFECT_ROW_GAP + i as f32 * note_line);
                text::draw(row.painter(), at, l, note, color);
            }
        }
        changed
    }

    /// A plugin drop-down at `at` across `w`: its caption (`names[0]`, may be empty) left of the
    /// box, which starts `col` right; the accessible name is the first of `names` (caption,
    /// label, key) with text. Returns the picked index.
    fn captioned_select(
        &mut self,
        ui: &mut Ui,
        names: [&str; 3],
        labels: &[&str],
        current: usize,
        (at, w, col, color, base, fade): (Pos2, f32, f32, Color32, f32, f32),
    ) -> Option<usize> {
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let caption = names[0];
        let mut x = at.x;
        if !caption.is_empty() {
            ui.set_opacity(base);
            let y = at.y + (EFFECT_SELECT_HEIGHT - line) / 2.0;
            text::draw(ui.painter(), Pos2::new(at.x, y), caption, spec, color);
            ui.set_opacity(base * fade);
            x += col;
        }
        let key = names[2];
        let name = accessible_name(caption, accessible_name(names[1], key));
        let rect = Rect::from_min_size(
            Pos2::new(x, at.y),
            Vec2::new(at.x + w - x, EFFECT_SELECT_HEIGHT),
        );
        let look = SelectLook {
            native: true,
            pad_left: 0.0,
            font: spec,
        };
        let id = ui.scope_id().with(("select", key));
        select(
            ui,
            &mut self.open_select,
            id,
            rect,
            name,
            labels,
            current,
            look,
        )
    }

    /// A plugin pan box at `rect` (its 1-px border included): the thumbnail of `kind` under
    /// `draw_pan`'s cross at the value; a press or drag sets the value (`pan_from_pointer`,
    /// within `range`).
    fn pan_box(
        &mut self,
        ui: &mut Ui,
        (key, name): (&str, &str),
        rect: Rect,
        kind: PanThumb,
        (min, max): (f64, f64),
    ) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let outer = px::snap_rect(rect, ppp);
        ui.painter()
            .rect_filled(outer, 0.0, theme.pdn_control_border);
        let inner = rect.shrink(1.0);
        let (r, w, h) = device_box(inner, ppp);
        let id = ui.scope_id().with(("pan", key));
        let response = ui.interact(r, id, Sense::CLICK | Sense::DRAG);
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_role(egui::accesskit::Role::Image);
            n.set_label(name);
            // egui only supplies bounds automatically for focusable widgets.
            n.set_bounds(egui::accesskit::Rect {
                x0: r.min.x.into(),
                y0: r.min.y.into(),
                x1: r.max.x.into(),
                y1: r.max.y.into(),
            });
        });
        let i = self.pan_thumb_index(kind);
        let fit = self.pan_thumbs[i].1;
        let mut changed = None;
        if let Some(p) = response.interact_pointer_pos()
            && response.is_pointer_button_down_on()
        {
            let v = pan_from_pointer(p, inner, fit, min, max);
            if v != bb::pan(&self.values, key) {
                self.set_value(key, json!(v));
                changed = Some(false);
            }
        }
        if response.drag_stopped() {
            changed = Some(true);
        }
        let [x, y] = bb::pan(&self.values, key);
        let cx = draw_pan(Some(&self.pan_thumbs[i].2), x, y, fit);
        paint_canvas(ui, &mut self.canvases, id, r, canvas_image(&cx, w, h));
        changed
    }

    /// The cached thumbnail of `kind` (common.md §3 "Pan"): the layer, the layer cropped to the
    /// selection bounds, or an 80×80 opaque white square (fill-from-clipboard.md control 1).
    fn pan_thumb_index(&mut self, kind: PanThumb) -> usize {
        if let Some(i) = self.pan_thumbs.iter().position(|t| t.0 == kind) {
            return i;
        }
        let white = || ImageData {
            width: 80,
            height: 80,
            data: vec![255; 80 * 80 * 4],
        };
        let (fit, thumb) = {
            let cropped;
            let white_square;
            let src = match (&self.session, kind) {
                (Some(s), PanThumb::Source) => s.source(),
                (Some(s), PanThumb::SelectionCrop) => {
                    cropped = crop(s.source(), s.env().bounds);
                    &cropped
                }
                _ => {
                    white_square = white();
                    &white_square
                }
            };
            let fit = pan_fit(src.width.max(1), src.height.max(1));
            (fit, pan_thumbnail(src, fit.0, fit.1))
        };
        self.pan_thumbs.push((kind, fit, thumb));
        self.pan_thumbs.len() - 1
    }
}

fn color_px(c: pdn_core::util::color::Color) -> pdn_core::pixel::Px {
    [c.red, c.green, c.blue, c.alpha]
}

#[cfg(test)]
mod tests {
    use super::super::tests::dialog;
    use super::*;
    use pdn_core::effect::bb::ui::{ControlUi, IndirectUi, Note, Tab, TabStrip};
    use pdn_core::effect::session::EffectSession;

    fn ctrl(key: &'static str) -> Control {
        Control::Checkbox {
            key,
            label: key,
            default: false,
        }
    }
    fn only_a_when_b(v: &Values, key: &str) -> bool {
        key != "a" || pdn_core::effect::flag(v, "b")
    }
    static PROBE: IndirectUi = IndirectUi {
        width: 1.1,
        tabs: &[
            TabStrip {
                key: "outer",
                tabs: &[
                    Tab {
                        caption: "One",
                        keys: &["a", "inner"],
                    },
                    Tab {
                        caption: "Two",
                        keys: &["b", "shared"],
                    },
                ],
            },
            TabStrip {
                key: "inner",
                tabs: &[
                    Tab {
                        caption: "X",
                        keys: &["x", "shared"],
                    },
                    Tab {
                        caption: "Y",
                        keys: &["y"],
                    },
                ],
            },
        ],
        controls: &[(
            "x",
            ControlUi {
                note: Note::Text(" "),
                ..ControlUi::NO_HEADER
            },
        )],
        enabled: only_a_when_b,
        ..IndirectUi::DEFAULT
    };
    fn probe_controls() -> Vec<Control> {
        vec![
            Control::Value {
                key: "outer",
                default: || json!(0),
            },
            Control::Value {
                key: "inner",
                default: || json!(0),
            },
            ctrl("a"),
            ctrl("b"),
            ctrl("x"),
            ctrl("y"),
            ctrl("shared"),
            ctrl("below"),
        ]
    }
    fn keys(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Tabs { key, selected, .. } => format!("[{key}:{selected}]"),
                Row::Control { key, enabled } => {
                    format!("{key}{}", if *enabled { "" } else { "~" })
                }
            })
            .collect()
    }

    /// R11: Tilt-Shift's and Iris Blur's centre is a Pan row, which only the indirect layout
    /// draws, so both defs carry an `IndirectUi` whose rows include it.
    #[test]
    fn gallery_blur_dialogs_list_the_centre_pan_row() {
        for id in ["pcTiltShift", "pcIrisBlur"] {
            let def = pdn_core::effect::find(id).unwrap();
            let ui = def
                .indirect
                .unwrap_or_else(|| panic!("{id} has no IndirectUi"));
            let values = pdn_core::effect::defaults(&def);
            let rows = keys(&indirect_rows(ui, &def.controls, &values));
            assert!(rows.contains(&"center".to_string()), "{id}: {rows:?}");
            assert_eq!(rows.first().map(String::as_str), Some("blur"), "{id}");
        }
    }

    /// R11: Lens Flare's and Lighting Effects' centre is a Pan row, which only the indirect
    /// layout draws, so both defs carry an `IndirectUi` whose rows include it.
    #[test]
    fn render_dialogs_list_the_centre_pan_row() {
        for id in ["pcLensFlare", "pcLightingEffects"] {
            let def = pdn_core::effect::find(id).unwrap();
            let ui = def
                .indirect
                .unwrap_or_else(|| panic!("{id} has no IndirectUi"));
            let values = pdn_core::effect::defaults(&def);
            let rows = keys(&indirect_rows(ui, &def.controls, &values));
            assert!(rows.contains(&"center".to_string()), "{id}: {rows:?}");
        }
    }

    /// R11: Field, Spin and Path Blur place their pins and path ends with Pan rows, which only
    /// the indirect layout draws.
    #[test]
    fn blur_gallery_pin_dialogs_list_their_pan_rows() {
        for (id, pans) in [
            ("pcFieldBlur", &["center", "center2"][..]),
            ("pcSpinBlur", &["center"]),
            ("pcPathBlur", &["start", "end"]),
        ] {
            let def = pdn_core::effect::find(id).unwrap();
            let ui = def
                .indirect
                .unwrap_or_else(|| panic!("{id} has no IndirectUi"));
            let values = pdn_core::effect::defaults(&def);
            let rows = keys(&indirect_rows(ui, &def.controls, &values));
            for pan in pans {
                assert!(rows.contains(&pan.to_string()), "{id}: {rows:?}");
            }
        }
    }

    /// R11: Flame's path is two Pan rows and a curve, which only the indirect layout draws, on
    /// its Path tab; the custom colour row is greyed until "Use Custom Color" is ticked.
    #[test]
    fn flame_dialog_lists_the_path_pan_rows() {
        let def = pdn_core::effect::find("pcFlame").unwrap();
        let ui = def.indirect.expect("pcFlame has an IndirectUi");
        let mut values = pdn_core::effect::defaults(&def);
        let rows = keys(&indirect_rows(ui, &def.controls, &values));
        assert_eq!(rows.first().map(String::as_str), Some("[tab:0]"));
        assert!(rows.contains(&"color~".to_string()), "{rows:?}");
        values.insert("tab".into(), serde_json::json!(2));
        let rows = keys(&indirect_rows(ui, &def.controls, &values));
        assert_eq!(rows, ["[tab:2]", "start", "end", "curve"]);
    }

    /// Flame's tab block is as tall as its tallest page (R83), and that fits the dialog without
    /// clipping.
    #[test]
    fn flame_dialog_tallest_tab_fits() {
        let (_app, d) = dialog("pcFlame", None);
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1600.0))),
            ..Default::default()
        };
        let mut height = 0.0;
        ctx.run_ui(input, |ui| height = d.size(ui).1)
            .textures_delta
            .clear();
        // Nine rows a page plus the strip: well above the minimum, under the cap.
        let (t, _, b) = EFFECT_CONTENT_PADDING;
        let content = height - t - b;
        assert!(
            (400.0..EFFECT_CONTENT_HEIGHT.1).contains(&content),
            "{content}"
        );
    }

    /// common.md §3 "Tabs"; text-formations-text-fun-factory.md (nested tabs, a control on two
    /// tabs); adjustments-hue-saturation-plus.md (controls below the tabs); "Enable rules".
    #[test]
    fn indirect_rows_follow_tabs_rules_and_pages() {
        let mut v = pdn_core::effect::defaults(&pdn_core::effect::EffectDef {
            controls: probe_controls(),
            ..probe_def()
        });
        assert_eq!(
            keys(&indirect_rows(&PROBE, &probe_controls(), &v)),
            ["[outer:0]", "a~", "[inner:0]", "x", "shared", "below"]
        );
        v.insert("inner".into(), json!(1));
        v.insert("b".into(), json!(true));
        assert_eq!(
            keys(&indirect_rows(&PROBE, &probe_controls(), &v)),
            ["[outer:0]", "a", "[inner:1]", "y", "below"]
        );
        v.insert("outer".into(), json!(1));
        assert_eq!(
            keys(&indirect_rows(&PROBE, &probe_controls(), &v)),
            ["[outer:1]", "b", "shared", "below"]
        );
        assert!((dialog_width(Some(&PROBE), 4000.0) - EFFECT_DIALOG_WIDTH * 1.1).abs() < 1e-3);
        assert_eq!(dialog_width(None, 4000.0), EFFECT_DIALOG_WIDTH);
    }

    /// Match Color's Source drop-down is a row of its dialog (the generic layout skips it).
    #[test]
    fn match_color_dialog_lists_its_source_row() {
        let def = pdn_core::effect::find("pcMatchColor").unwrap();
        let ui = def.indirect.expect("indirect ui");
        let rows = keys(&indirect_rows(
            ui,
            &def.controls,
            &pdn_core::effect::defaults(&def),
        ));
        assert!(rows.iter().any(|k| k == "source"), "{rows:?}");
    }

    /// I1: Color Lookup's File row is a row of its dialog (the generic layout skips it), enabled
    /// only with Source: File, when Look is disabled.
    #[test]
    fn color_lookup_dialog_lists_its_file_row() {
        let def = pdn_core::effect::find("pcColorLookup").unwrap();
        let ui = def.indirect.expect("indirect ui");
        let mut values = pdn_core::effect::defaults(&def);
        let rows = keys(&indirect_rows(ui, &def.controls, &values));
        assert!(
            rows.iter().any(|k| k == "file~") && rows.iter().any(|k| k == "look"),
            "{rows:?}"
        );
        values.insert("source".into(), json!("file"));
        let rows = keys(&indirect_rows(ui, &def.controls, &values));
        assert!(
            rows.iter().any(|k| k == "file") && rows.iter().any(|k| k == "look~"),
            "{rows:?}"
        );
    }

    /// Displace's Map drop-down is a row of its dialog (the generic layout skips it).
    #[test]
    fn displace_dialog_lists_its_map_row() {
        let def = pdn_core::effect::find("pcDisplace").unwrap();
        let ui = def.indirect.expect("indirect ui");
        let rows = keys(&indirect_rows(
            ui,
            &def.controls,
            &pdn_core::effect::defaults(&def),
        ));
        assert!(rows.iter().any(|k| k == "map"), "{rows:?}");
    }

    /// Tree's Base Position pan is a row of its dialog (the generic layout skips `Pan`).
    #[test]
    fn tree_dialog_lists_its_base_position_row() {
        let def = pdn_core::effect::find("pcTree").unwrap();
        let ui = def.indirect.expect("indirect ui");
        let rows = keys(&indirect_rows(
            ui,
            &def.controls,
            &pdn_core::effect::defaults(&def),
        ));
        assert!(rows.iter().any(|k| k == "at"), "{rows:?}");
    }

    /// Camera Raw's dialog (R11): the Basic, Tone, HSL and Detail tabs, HSL and Detail with their
    /// own strips, and the tallest page fits the dialog without clipping.
    #[test]
    fn camera_raw_dialog_lists_its_tabs_and_fits() {
        let def = pdn_core::effect::find("pcCameraRaw").unwrap();
        let ui = def.indirect.expect("indirect ui");
        let mut v = pdn_core::effect::defaults(&def);
        let rows = |v: &Values| keys(&indirect_rows(ui, &def.controls, v));
        assert_eq!(
            rows(&v),
            [
                "[tab:0]",
                "temperature",
                "tint",
                "exposure",
                "contrast",
                "highlights",
                "shadows",
                "whites",
                "blacks"
            ]
        );
        v.insert("tab".into(), json!(1));
        assert_eq!(
            rows(&v),
            [
                "[tab:1]",
                "texture",
                "clarity",
                "dehaze",
                "vibrance",
                "saturation"
            ]
        );
        v.insert("tab".into(), json!(2));
        v.insert("hslTab".into(), json!(1));
        let hsl = rows(&v);
        assert_eq!(hsl.len(), 10);
        assert_eq!(hsl[..3], ["[tab:2]", "[hslTab:1]", "satRed"]);
        v.insert("tab".into(), json!(3));
        v.insert("detailTab".into(), json!(1));
        assert_eq!(
            rows(&v),
            [
                "[tab:3]",
                "[detailTab:1]",
                "noiseLuminance",
                "noiseLuminanceDetail",
                "noiseColor",
                "noiseColorDetail"
            ]
        );
        let (_app, d) = dialog("pcCameraRaw", None);
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1600.0))),
            ..Default::default()
        };
        let mut height = 0.0;
        ctx.run_ui(input, |ui| height = d.size(ui).1)
            .textures_delta
            .clear();
        // The pages are 4 to 8 sliders and a strip or two: well above the minimum, under the cap.
        let (t, _, b) = EFFECT_CONTENT_PADDING;
        let content = height - t - b;
        assert!(
            (400.0..EFFECT_CONTENT_HEIGHT.1).contains(&content),
            "{content}"
        );
    }

    fn probe_def() -> pdn_core::effect::EffectDef {
        let mut d = pdn_core::effect::find("invertColorsEffect").unwrap();
        d.id = "bbDream";
        d.indirect = Some(&PROBE);
        d
    }

    /// common.md §3 "Colour wheel … with alpha"; "Pan (offset) control"; labels with a leading
    /// line break.
    #[test]
    fn colour_pan_and_label_helpers() {
        let c = pdn_core::util::color::Color::new(0x11, 0x22, 0x33, 0x44);
        assert_eq!(
            (pick_hex(c, false), pick_hex(c, true)),
            ("#112233".to_string(), "#11223344".to_string())
        );
        let inner = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(90.0, 66.0));
        assert_eq!(
            pan_from_pointer(Pos2::new(55.0, 43.0), inner, (90.0, 66.0), -1.0, 1.0),
            [0.0, 0.0]
        );
        assert_eq!(
            pan_from_pointer(Pos2::new(10.0, 10.0), inner, (90.0, 66.0), -1.0, 1.0),
            [-1.0, -1.0]
        );
        assert_eq!(
            pan_from_pointer(Pos2::new(500.0, 10.0), inner, (90.0, 66.0), -2.0, 2.0),
            [2.0, -1.0]
        );
        assert_eq!(label_lines("\nZoom"), ["", "Zoom"]);
    }

    /// artistic-dream.md "Default Radius … the slider's reset button returns to this
    /// document-dependent value".
    #[test]
    fn reset_arrow_returns_the_session_default() {
        fn sized(v: &mut Values, env: &pdn_core::effect::EffectEnv<'_>) {
            v.insert(
                "radius".into(),
                json!((env.width.max(env.height) / 80).clamp(10, 200)),
            );
        }
        static SIZED: IndirectUi = IndirectUi {
            defaults: sized,
            ..IndirectUi::DEFAULT
        };
        let mut app = pdn_core::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(1920, 1080, 96.0, true);
        let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
        def.controls = vec![pdn_core::effect::range(
            "radius", "Radius", 0.0, 200.0, 10.0,
        )];
        def.indirect = Some(&SIZED);
        let s = EffectSession::begin(&mut app, def).unwrap();
        let mut d = EffectConfigDialog::new(s, Values::new());
        d.values.insert("radius".into(), json!(77));
        let generation = d.generation;
        d.delay = 50.0;
        d.reset_control("radius");
        d.invalidate(5.0, true);
        assert_eq!(d.values["radius"], json!(24));
        assert_eq!(
            (d.generation, d.due),
            (generation + 1, Some(5.0)),
            "notify(now, true)"
        );
        assert_eq!(
            dialog_width(Some(&PROBE), 400.0),
            400.0 - DIALOG_VIEWPORT_MARGIN
        );
    }

    /// A plugin dialog over a 4×3 document of two layers (the top one active), its painter
    /// (100 %) and its laid rows across `w`.
    pub(super) fn laid(
        def: pdn_core::effect::EffectDef,
        w: f32,
    ) -> (EffectConfigDialog, Vec<Laid>) {
        let mut app = pdn_core::app::AppWorkspace::default();
        app.create_blank_document_in_new_workspace(4, 3, 96.0, true);
        pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
        let ind = def.indirect.unwrap();
        let controls = def.controls.clone();
        let values = pdn_core::effect::defaults(&def);
        let d = EffectConfigDialog::new(EffectSession::begin(&mut app, def).unwrap(), values);
        let painter = egui::Painter::new(
            egui::Context::default(),
            egui::LayerId::background(),
            Rect::EVERYTHING,
        );
        let rows = d.laid_rows(&painter, ind, &controls, w);
        (d, rows)
    }

    pub(super) fn row<'a>(rows: &'a [Laid], key: &str) -> &'a Laid {
        rows.iter()
            .find(|l| matches!(l.row, Row::Control { key: k, .. } if k == key))
            .unwrap()
    }

    /// common.md §3 "Header label" (its lines; the header line beside the label takes no height,
    /// ruling R84; none for an empty label), "Footnote" (a single space is a gap; text computed from the env), "Static
    /// label" (the current layer's name; long text wraps).
    #[test]
    fn plugin_rows_measure_headers_notes_and_labels() {
        fn width_note(env: &pdn_core::effect::EffectEnv<'_>) -> String {
            format!("{} px wide", env.width)
        }
        static UI: IndirectUi = IndirectUi {
            controls: &[
                ("plain", ControlUi::NO_HEADER),
                (
                    "gap",
                    ControlUi {
                        note: Note::Text(" "),
                        ..ControlUi::DEFAULT
                    },
                ),
                (
                    "env",
                    ControlUi {
                        note: Note::Env(width_note),
                        ..ControlUi::DEFAULT
                    },
                ),
            ],
            ..IndirectUi::DEFAULT
        };
        let range = |key, label| pdn_core::effect::range(key, label, 0.0, 10.0, 0.0);
        let label = |key, text| Control::Label {
            key,
            label: "",
            text,
        };
        let def = pdn_core::effect::EffectDef {
            controls: vec![
                range("ruled", "Ruled"),
                range("plain", "Plain"),
                range("two", "\nTwo"),
                range("bare", ""),
                range("gap", "Gap"),
                range("env", "Env"),
                label("info", LabelText::CurrentLayer("Layer: ")),
                label("long", LabelText::Fixed("word ".repeat(40).leak())),
            ],
            indirect: Some(&UI),
            ..probe_def()
        };
        let (d, rows) = laid(def, 300.0);
        let top = d.session.as_ref().unwrap().layer_names()[1].clone();
        let line = FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(1.0);
        let note_line = FontSpec::new(EFFECT_NOTE_FONT).resolved_line_height(1.0);
        let h = |k| row(&rows, k).height;
        assert_eq!(h("ruled"), line + EFFECT_ROW_GAP + EFFECT_LINE_HEIGHT);
        assert_eq!(h("plain"), h("ruled"));
        assert_eq!(h("two"), 2.0 * line + EFFECT_ROW_GAP + EFFECT_LINE_HEIGHT);
        assert_eq!(h("bare"), EFFECT_LINE_HEIGHT);
        assert_eq!(h("gap"), h("ruled") + EFFECT_NOTE_GAP);
        assert_eq!(h("env"), h("ruled") + EFFECT_ROW_GAP + note_line);
        let text = |k| &row(&rows, k).control.as_ref().unwrap().2;
        assert_eq!(text("two").header, ["", "Two"]);
        assert_eq!(text("env").note, Some(vec!["4 px wide".to_string()]));
        assert_eq!(text("info").body, [format!("Layer: {top}")]);
        assert_eq!(h("info"), line);
        let long = &text("long").body;
        assert!(long.len() > 1 && long.iter().all(|l| text::css_width(l, 12.0) <= 300.0));
        assert_eq!(h("long"), long.len() as f32 * line);
    }

    /// common.md §3 (inline drop-down captions): the boxes of consecutive captioned drop-downs
    /// start together, after the widest caption; another row breaks the run.
    #[test]
    fn captions_align_their_runs() {
        const CAPTIONED: [(&str, ControlUi); 3] = [
            (
                "a",
                ControlUi {
                    caption: "Longer caption",
                    ..ControlUi::DEFAULT
                },
            ),
            (
                "b",
                ControlUi {
                    caption: "B",
                    ..ControlUi::DEFAULT
                },
            ),
            (
                "d",
                ControlUi {
                    caption: "D",
                    ..ControlUi::DEFAULT
                },
            ),
        ];
        static UI: IndirectUi = IndirectUi {
            controls: &CAPTIONED,
            ..IndirectUi::DEFAULT
        };
        let select = |key| Control::Select {
            key,
            label: "",
            default: "x",
            choices: &[("x", "X")],
        };
        let def = pdn_core::effect::EffectDef {
            controls: vec![
                select("a"),
                select("b"),
                ctrl("c"),
                select("d"),
                select("e"),
            ],
            indirect: Some(&UI),
            ..probe_def()
        };
        let (_, rows) = laid(def, 300.0);
        let col = |s| text::css_width(s, DIALOG_FONT_SIZE) + EFFECT_CAPTION_GAP;
        let wide = col("Longer caption");
        assert_eq!(caption_columns(&rows), [wide, wide, 0.0, col("D"), 0.0]);
    }

    /// Ruling R83 (Q24): the tab strips' blocks take their tallest page (the nested strip's
    /// included), so the column is as tall for every tab, the rows below stay put, and a page
    /// shorter than the tallest ends in the gap.
    #[test]
    fn tab_blocks_take_the_tallest_page() {
        // PROBE's strips with their pages reversed: the tallest pages come last.
        static LAST: IndirectUi = IndirectUi {
            tabs: &[
                TabStrip {
                    key: "outer",
                    tabs: &[
                        Tab {
                            caption: "Two",
                            keys: &["b", "shared"],
                        },
                        Tab {
                            caption: "One",
                            keys: &["a", "inner"],
                        },
                    ],
                },
                TabStrip {
                    key: "inner",
                    tabs: &[
                        Tab {
                            caption: "Y",
                            keys: &["y"],
                        },
                        Tab {
                            caption: "X",
                            keys: &["x", "shared"],
                        },
                    ],
                },
            ],
            ..PROBE
        };
        let def = pdn_core::effect::EffectDef {
            controls: probe_controls(),
            indirect: Some(&LAST),
            ..probe_def()
        };
        let (mut d, _) = laid(def, 300.0);
        let painter = egui::Painter::new(
            egui::Context::default(),
            egui::LayerId::background(),
            Rect::EVERYTHING,
        );
        let mut column = |outer: usize, inner: usize| {
            d.values.insert("outer".into(), json!(outer));
            d.values.insert("inner".into(), json!(inner));
            let rows = d.laid_rows(&painter, &LAST, &probe_controls(), 300.0);
            let total: f32 = rows.iter().map(|l| l.height + EFFECT_CONTENT_GAP).sum();
            let below = rows
                .iter()
                .position(|l| row_key(&l.row) == "below")
                .unwrap();
            let above: f32 = rows[..below]
                .iter()
                .map(|l| l.height + EFFECT_CONTENT_GAP)
                .sum();
            (total, above, rows.last().unwrap().height)
        };
        // Checkboxes 24; "x" has its 6-px note gap; strips 23; gaps 10.
        let (tab, row, gap) = (EFFECT_TAB_HEIGHT, EFFECT_CHECKBOX.2, EFFECT_CONTENT_GAP);
        let inner = tab + gap + (row + EFFECT_NOTE_GAP + gap) + (row + gap);
        let outer = tab + gap + (row + gap) + inner;
        let tallest = column(1, 1);
        assert_eq!(tallest, (outer + row + gap, outer, row));
        for (o, i) in [(0, 0), (0, 1), (1, 0)] {
            assert_eq!(column(o, i), tallest, "outer {o}, inner {i}");
        }
    }

    /// A strip's stored index past its last tab shows the last tab (the brief's clamp).
    #[test]
    fn tab_values_clamp_to_the_strip() {
        let mut v = pdn_core::effect::defaults(&pdn_core::effect::EffectDef {
            controls: probe_controls(),
            ..probe_def()
        });
        v.insert("outer".into(), json!(7));
        assert_eq!(
            keys(&indirect_rows(&PROBE, &probe_controls(), &v)),
            ["[outer:1]", "b", "shared", "below"]
        );
    }

    /// common.md §3 "Pan": the thumbnail is the layer, the layer cropped to the selection bounds,
    /// or an 80×80 white square (fill-from-clipboard.md), fitted into 90×66 and kept.
    #[test]
    fn pan_thumbnails_by_kind() {
        let src = ImageData {
            width: 4,
            height: 3,
            data: (0..48).collect(),
        };
        let part = crop(&src, pdn_core::pixel::IntRect::new(1, 1, 2, 2));
        assert_eq!((part.width, part.height), (2, 2));
        assert_eq!(
            part.data,
            [
                20, 21, 22, 23, 24, 25, 26, 27, 36, 37, 38, 39, 40, 41, 42, 43
            ]
        );
        assert_eq!(pan_fit(800, 600), (88.0, 66.0));
        let inner = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(100.0, 76.0));
        assert_eq!(
            pan_from_pointer(Pos2::new(50.0, 500.0), inner, (88.0, 66.0), -1.0, 1.0),
            [0.0, 1.0],
            "y clamps too"
        );
        let (mut d, _) = laid(probe_def(), 300.0);
        let white = d.pan_thumb_index(PanThumb::White);
        let source = d.pan_thumb_index(PanThumb::Source);
        assert_ne!(white, source);
        assert_eq!(d.pan_thumb_index(PanThumb::White), white, "kept");
        assert_eq!(d.pan_thumbs[white].1, (66.0, 66.0));
        assert_eq!(d.pan_thumbs[source].1, pan_fit(4, 3));
        let crop_i = d.pan_thumb_index(PanThumb::SelectionCrop);
        assert_eq!(
            d.pan_thumbs[crop_i].1,
            pan_fit(4, 3),
            "no selection: the whole layer"
        );
    }
}

#[cfg(test)]
mod bb31_tests {
    use super::super::bb28_tests::{context, manager, pending_messages, wait_closed, with_cx};
    use super::super::*;
    use super::tests::{laid, row};
    use super::*;
    use pdn_core::testing::{fresh_doc, layer_bytes};

    fn values(def: &pdn_core::effect::EffectDef, extra: Value) -> Values {
        let mut v = pdn_core::effect::defaults(def);
        v.extend(extra.as_object().unwrap().clone());
        v
    }

    /// text-formations-text-fun-factory.md "Controls": the outer strip, the Fill tab holding only
    /// its inner strip (whose index is the fill type), shared depth/light rows on both pages,
    /// and the enable rules on the rows.
    #[test]
    fn bb31_rows_follow_the_nested_fill_tabs() {
        let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
        let ui = def.indirect.unwrap();
        let keys = |extra: Value| {
            indirect_rows(ui, &def.controls, &values(&def, extra))
                .into_iter()
                .map(|r| match r {
                    Row::Tabs { key, selected, .. } => format!("[{key} {selected}]"),
                    Row::Control { key, enabled } => {
                        format!("{key}{}", ["-", ""][enabled as usize])
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(json!({})),
            [
                "[tab 0]", "text", "font", "bold", "italics", "size", "offsetX", "offsetY",
                "maintain"
            ]
        );
        assert_eq!(
            keys(json!({"tab": 1})),
            ["[tab 1]", "[fillTab 0]", "solidColor", "customColor-"]
        );
        assert_eq!(
            keys(json!({"tab": 1, "fillTab": 2, "metal": "gold"})),
            [
                "[tab 1]",
                "[fillTab 2]",
                "metal",
                "depth",
                "metalStrength-",
                "metalHard-",
                "light"
            ]
        );
        assert_eq!(
            keys(json!({"tab": 3, "shadowLength": 0})),
            [
                "[tab 3]",
                "shadowColor-",
                "shadowLength",
                "shadowStrength-",
                "light"
            ]
        );
        assert_eq!(
            keys(json!({"tab": 5, "rotate": "same", "reverse": "only"})),
            [
                "[tab 5]",
                "rotate",
                "rotateAngle",
                "jumble",
                "reverse",
                "reverseN",
                "overlap",
                "swapLayering"
            ]
        );
    }

    /// R158: the Outline tab's check box keeps its caption and draws a single-space heading;
    /// other check boxes have none. Drop-down captions (Font, Outline corner style) are set.
    #[test]
    fn bb31_outline_on_top_heading_and_font_row() {
        let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
        let mut tab = def.clone();
        tab.controls = def
            .controls
            .iter()
            .map(|c| match c {
                Control::Value { key: "tab", .. } => Control::Value {
                    key: "tab",
                    default: || json!(2),
                },
                c => c.clone(),
            })
            .collect();
        let (_, rows) = laid(tab, 400.0);
        let text = |k| &row(&rows, k).control.as_ref().unwrap().2;
        assert_eq!(text("outlineOnTop").header, [" "]);
        assert_eq!(text("outlineColor").header, ["", "Color"]);
        let line = FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(1.0);
        // Its own height (the Outline page's last row also takes the page's R83 padding).
        let (c, cu, t) = row(&rows, "outlineOnTop").control.as_ref().unwrap();
        assert!(matches!(
            c,
            Control::Checkbox {
                label: "Outline on top of fill",
                ..
            }
        ));
        assert!(!cu.header_line);
        assert_eq!(
            row_height(c, t, 1.0),
            line + EFFECT_ROW_GAP + EFFECT_CHECKBOX.2
        );
        let (_, rows) = laid(def, 400.0);
        assert!(text_of(&rows, "bold").header.is_empty());
        let font = row(&rows, "font").control.as_ref().unwrap();
        assert!(matches!(font.0, Control::FontSelect { .. }) && font.1.caption == "Font: ");
        assert_eq!(row(&rows, "font").height, EFFECT_SELECT_HEIGHT);
    }
    fn text_of<'a>(rows: &'a [Laid], k: &str) -> &'a RowText {
        &row(rows, k).control.as_ref().unwrap().2
    }

    /// The font drop-down's choices (BB30's `font_select_entries`, R188): the document fonts plus
    /// the bundled interface faces, one per family, by name, the saved family selected
    /// case-insensitively; a family no longer installed is listed and selected (BB30).
    #[test]
    fn bb31_font_choices() {
        let (fonts, index) = font_select_entries(None, "noto sans mono");
        let names: Vec<_> = fonts.iter().map(|f| f.1.to_lowercase()).collect();
        assert!(names.windows(2).all(|w| w[0] <= w[1]), "{names:?}");
        for f in ["Selawik", "Carlito", "Noto Sans Mono", "JetBrains Mono"] {
            assert_eq!(
                fonts.iter().filter(|c| c.0.eq_ignore_ascii_case(f)).count(),
                1,
                "{f}"
            );
        }
        assert_eq!(fonts[index].0, "Noto Sans Mono");
        let (fonts, index) = font_select_entries(None, "bb31-gone");
        assert_eq!((index, fonts[0].0.as_str()), (0, "bb31-gone"));
    }

    /// R188 / Q9: Text Fun Factory's default font is the first "Rounded" family of the document
    /// font list the app's font manager publishes (the Font drop-down's list), else Selawik.
    #[test]
    fn bb31_default_font_searches_the_document_fonts() {
        let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
        let default = || {
            let mut app = fresh_doc(4, 3, None);
            EffectSession::begin(&mut app, def.clone())
                .unwrap()
                .defaults()["font"]
                .clone()
        };
        assert_eq!(default(), json!("Selawik"));
        let record = pdn_core::fonts::FontRecord {
            id: "bb31-rounded".into(),
            name: "BB31 Rounded".into(),
            family: "paintjs-bb31-rounded".into(),
            file_name: "rounded.ttf".into(),
            kind: "font/ttf".into(),
            size: 0,
            added_at: 0,
        };
        let mut m = manager(Some((record, None)));
        let ctx = context();
        m.sync(&ctx, &mut crate::prefs::Prefs::default(), "Selawik");
        assert_eq!(default(), json!("paintjs-bb31-rounded"));
        let fonts = crate::platform::fonts::choices(&ctx);
        let (entries, index) = font_select_entries(fonts, "paintjs-bb31-rounded");
        assert_eq!(entries[index].1, "BB31 Rounded");
        pdn_core::fonts::set_document_choices(vec![]);
        assert_eq!(default(), json!("Selawik"));
    }

    /// R157 / Q9 in the dialog: a saved family that no longer loads falls back to the default
    /// face for the session, renders and commits once; the token keeps the saved name and no
    /// transient value.
    #[test]
    fn bb31_missing_saved_font_falls_back_and_commits() {
        pending_messages();
        let ctx = context();
        let mut m = manager(None);
        let mut app = fresh_doc(60, 40, Some(4));
        let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
        let s = EffectSession::begin(&mut app, def.clone()).unwrap();
        let v = values(&def, json!({"text": "A", "font": "bb31-gone", "size": 20}));
        let mut d = EffectConfigDialog::new(s, v);
        let before = layer_bytes(&app, None);
        let history = app.active_ws().unwrap().history.undo.len();
        let state = match with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)) {
            DialogState::Open => wait_closed(&ctx, &mut app, &mut m, &mut d),
            s => s,
        };
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        assert_eq!(bb::text(&d.values, "fallbackFont"), "bb31-gone");
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        assert_ne!(layer_bytes(&app, None), before);
        let token = &app.effect_tokens["bbTextFunFactory"];
        assert_eq!(token["font"], json!("bb31-gone"));
        assert!(!token.contains_key("fallbackFont") && !token.contains_key("fallbackFamily"));
        assert_eq!(
            (token["tab"].clone(), token["fillTab"].clone()),
            (json!(0), json!(0))
        );
        assert_eq!(pending_messages(), 0, "the fallback is not an error");
    }

    /// BB33 / R157 / R155: a saved family whose load already failed falls back on an OK pressed
    /// while the clipboard read is pending; that read stays the current one, so its delayed
    /// image is what commits (once, with one read).
    #[test]
    fn bb33_font_fallback_keeps_the_pending_read() {
        use super::super::external::clipboard_tests::{tff, tile};
        use crate::platform::TestImageReads;
        pending_messages();
        let probe = TestImageReads::install(true);
        let ctx = context();
        let mut m = manager(None);
        let (mut app, mut d, mut c, _) = tff(json!({"font": "bb33-gone"}));
        let ui = d.session.as_ref().unwrap().def.indirect.unwrap();
        for (family, style) in (ui.fonts)(&d.values) {
            m.ensure_family(&family, style);
        }
        super::super::bb28_tests::drain(&mut m);
        let mut expected = d.session.as_ref().unwrap().clone();
        expected.set_image(Some(Arc::new(tile())));
        let history = app.active_ws().unwrap().history.undo.len();
        d.acquire_external(&app, &ctx, Some(&mut c));
        assert_eq!(
            with_cx(&mut app, Some(&mut m), |cx| d.ok(cx, &ctx)),
            DialogState::Open
        );
        assert_eq!(bb::text(&d.values, "fallbackFont"), "bb33-gone");
        let want = expected.render(&d.values);
        let mut solid = d.values.clone();
        solid.insert("fillTab".into(), json!(0));
        assert_ne!(want, expected.render(&solid));
        probe.completions.borrow()[0].complete(Ok(Some(tile())));
        assert_eq!(
            wait_closed(&ctx, &mut app, &mut m, &mut d),
            DialogState::Closed(DialogResult::Ok)
        );
        assert_eq!(layer_bytes(&app, None), want.data);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history + 1);
        assert_eq!(probe.reads.get(), 1);
        assert_eq!(pending_messages(), 0);
        TestImageReads::clear();
    }

    /// BB33 review Minor 1: a font revision that swaps a required face re-renders with it, but
    /// it is no settings change: a pending clipboard read stays current (no second read) and a
    /// resolved image is kept.
    #[test]
    fn bb33_face_change_keeps_the_clipboard_read() {
        use super::super::bb28_tests::{drain, user_font};
        use super::super::external::clipboard_tests::{tff, tile};
        use crate::platform::fonts::FontManager;
        use crate::platform::{Clipboard, TestImageReads};
        use pdn_core::fonts;
        fn cx<T>(
            app: &mut pdn_core::app::AppWorkspace,
            m: &mut FontManager,
            c: &mut Clipboard,
            f: impl FnOnce(&mut DialogCx) -> T,
        ) -> T {
            f(&mut DialogCx {
                app,
                settings: &mut pdn_core::settings::AppSettings::load(None),
                prefs: &mut crate::prefs::Prefs::default(),
                settings_changed: false,
                shortcuts: None,
                clipboard: Some(c),
                settings_imports: None,
                fonts: Some(m),
                asset_library: None,
                seat_focus: false,
            })
        }
        let probe = TestImageReads::install(true);
        let ctx = context();
        let (record, bytes, family) = user_font("bb33-face", Some(fonts::CARLITO_REGULAR));
        let mut m = manager(Some((record, bytes)));
        let (mut app, mut d, mut c, _) = tff(json!({"font": family}));
        let ui = d.session.as_ref().unwrap().def.indirect.unwrap();
        let style = (ui.fonts)(&d.values)[0].1;
        m.ensure_family(&family, style);
        drain(&mut m);
        let mut expected = d.session.as_ref().unwrap().clone();
        expected.set_image(Some(Arc::new(tile())));
        d.acquire_external(&app, &ctx, Some(&mut c));
        assert_eq!(probe.reads.get(), 1);
        let swap = |m: &mut FontManager, bytes: &[u8]| {
            fonts::register_lazy_face(&family, style, bytes.to_vec()).unwrap();
            m.revision += 1;
        };
        assert_eq!(
            cx(&mut app, &mut m, &mut c, |cx| d.fonts_ready(cx, &ctx)),
            Ok(true)
        );
        swap(&mut m, fonts::SELAWIK_REGULAR);
        let g = d.generation;
        cx(&mut app, &mut m, &mut c, |cx| d.check_faces(cx));
        assert!(d.generation > g, "the new face renders afresh");
        assert_eq!(probe.reads.get(), 1, "no second read");
        assert!(
            matches!(d.input, ExternalInput::Pending { generation, .. } if generation == d.generation)
        );
        probe.completions.borrow()[0].complete(Ok(Some(tile())));
        d.poll_external(&mut app);
        assert!(matches!(d.input, ExternalInput::Resolved));
        assert_eq!(
            cx(&mut app, &mut m, &mut c, |cx| d.fonts_ready(cx, &ctx)),
            Ok(true)
        );
        swap(&mut m, fonts::CARLITO_REGULAR);
        let g = d.generation;
        cx(&mut app, &mut m, &mut c, |cx| d.check_faces(cx));
        assert!(d.generation > g);
        assert_eq!(probe.reads.get(), 1);
        assert!(matches!(d.input, ExternalInput::Resolved));
        let session = d.session.as_ref().unwrap();
        assert_eq!(
            session.render(&d.values),
            expected.render(&d.values),
            "the read image is kept"
        );
        fonts::unregister_family(&family);
        TestImageReads::clear();
    }
}
