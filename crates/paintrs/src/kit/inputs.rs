//! Form inputs: the spin box, checkbox, push button, text field and range slider.
//! Port of: js/src/interface/item/component/{NumberInput,NumberItem,SliderItem,CheckboxItem,
//! TextFieldItem}.js, css/component.css (`input[type=...]`, `.pdn-number-input*`, `button`),
//! css/header.css (`#toolMenu .tool-option input[type="number"]`, `> .pdn-number-input`)
//! Reference: paintdotjs @ b4e6fc5
//!
//! Text is edited by an egui `TextEdit` (frame and margins off, so egui handles IME, selection and
//! the caret) inside the custom-painted frame; unfocused, the value is drawn like the DOM does.
//! The dialog styles follow css/component.css and are verified by the dialog tasks.
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::aa;
use super::px;
use super::text::{self, FontSpec};
use crate::theme::{self, *};

/// NumberInput.createButton: a step on `pointerdown`, then `setTimeout(400)` starts a
/// `setInterval(65)`: the first repeat lands at 465 ms.
const REPEAT_DELAY_MS: f64 = 400.0;
const REPEAT_INTERVAL_MS: f64 = 65.0;

/// The spin arrow (`border-left/right: 3px; border-bottom/top: 4px; translate(-50%, -50%)`):
/// Skia's coverage of the 6x4 down triangle on whole pixels, measured from the references (Windows
/// draws the spin buttons in Paint.NET, so paintdotjs's look stands, spec §0.4); the up arrow is
/// its mirror.
const SPIN_ARROW_COVERAGE: [[u8; 6]; 4] = [
    [160, 255, 255, 255, 255, 160],
    [10, 212, 255, 255, 212, 10],
    [0, 40, 244, 244, 40, 0],
    [0, 0, 94, 94, 0, 0],
];

/// Skia's `dither` stage (SkRasterPipeline_opts.h): an 8x8 ordered dither of the raster tile's
/// device coordinates (`X = x`, `Y = x ^ y`, bits mixed as `fcebda`), scaled to +-63/128 of one
/// 8-bit step. Measured exact on both spin-button gradients.
fn skia_dither(x: u32, y: u32) -> f32 {
    let (xx, yy) = (x, x ^ y);
    let m = (yy & 1) << 5
        | (xx & 1) << 4
        | (yy & 2) << 2
        | (xx & 2) << 1
        | (yy & 4) >> 1
        | (xx & 4) >> 2;
    m as f32 * (2.0 / 128.0) - 63.0 / 128.0
}

/// A vertical `linear-gradient(top, bottom)` over device rect `r` (snapped): Skia's f32 raster
/// pipeline (evenly spaced two stops, dither, `to_unorm`), coordinates local to cc's 254 px raster
/// tiles. Returned per pixel so arrows can blend over each one.
pub(crate) fn gradient_pixels(
    r: Rect,
    (top, bottom): (Color32, Color32),
    ppp: f32,
) -> Vec<Vec<Color32>> {
    let (x0, y0) = (
        (r.min.x * ppp).round() as i32,
        (r.min.y * ppp).round() as i32,
    );
    let (w, h) = (
        (r.width() * ppp).round() as i32,
        (r.height() * ppp).round() as i32,
    );
    (0..h)
        .map(|j| {
            let t = (j as f32 + 0.5) / h as f32;
            (0..w)
                .map(|i| {
                    let (x, y) = (
                        super::img::tile_local(x0 + i),
                        super::img::tile_local(y0 + j),
                    );
                    let d = skia_dither(x as u32, y as u32) * (1.0 / 255.0);
                    let c = |a: u8, b: u8| {
                        let (a, b) = (a as f32 / 255.0, b as f32 / 255.0);
                        let v = (a + t * (b - a) + d).clamp(0.0, 1.0);
                        (v * 255.0 + 0.5).floor() as u8
                    };
                    Color32::from_rgb(
                        c(top.r(), bottom.r()),
                        c(top.g(), bottom.g()),
                        c(top.b(), bottom.b()),
                    )
                })
                .collect()
        })
        .collect()
}

pub(crate) fn paint_pixels(painter: &Painter, at: Pos2, px: &[Vec<Color32>]) {
    let ppp = painter.pixels_per_point();
    for (j, row) in px.iter().enumerate() {
        for (i, c) in row.iter().enumerate() {
            let p = at + Vec2::new(i as f32, j as f32) / ppp;
            painter.rect_filled(Rect::from_min_size(p, Vec2::splat(1.0 / ppp)), 0.0, *c);
        }
    }
}

/// One spin button: `rect` its border box (layout px), `up` the first child.
fn spin_button(
    ui: &mut Ui,
    id: egui::Id,
    rect: Rect,
    up: bool,
    enabled: bool,
    fade: &dyn Fn(Color32) -> Color32,
) -> Response {
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    // `tabIndex = -1`: not a Tab stop.
    let response = ui.interact(snapped, id, Sense::CLICK | Sense::DRAG);
    let b = px::border_width(1.0, ppp);
    // Border-left, and border-bottom on the first child; the gradient fills the padding box.
    painter.rect_filled(
        Rect::from_min_max(snapped.min, Pos2::new(snapped.min.x + b, snapped.max.y)),
        0.0,
        fade(NUMBER_SPIN_BORDER),
    );
    let mut pad = Rect::from_min_max(Pos2::new(snapped.min.x + b, snapped.min.y), snapped.max);
    if up {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(pad.min.x, pad.max.y - b), pad.max),
            0.0,
            fade(NUMBER_SPIN_BORDER),
        );
        pad.max.y -= b;
    }
    let mut pixels = if !enabled {
        gradient_pixels(pad, NUMBER_SPIN_DISABLED_GRADIENT, ppp)
    } else if response.is_pointer_button_down_on() {
        let (w, h) = ((pad.width() * ppp) as usize, (pad.height() * ppp) as usize);
        vec![vec![NUMBER_SPIN_ACTIVE; w]; h]
    } else if response.hovered() {
        gradient_pixels(pad, NUMBER_SPIN_HOVER_GRADIENT, ppp)
    } else {
        gradient_pixels(pad, NUMBER_SPIN_GRADIENT, ppp)
    };
    // The arrow: the layout padding box's center, snapped, then `translate(-50%, -50%)`.
    let mut layout_pad = Rect::from_min_max(Pos2::new(rect.min.x + b, rect.min.y), rect.max);
    if up {
        layout_pad.max.y -= b;
    }
    let c = Pos2::new(
        (layout_pad.center().x * ppp).round() / ppp,
        (layout_pad.center().y * ppp).round() / ppp,
    );
    let at = c - Vec2::new(3.0, 2.0) / ppp;
    let (ox, oy) = (
        ((at.x - pad.min.x) * ppp).round() as i32,
        ((at.y - pad.min.y) * ppp).round() as i32,
    );
    for (j, row) in SPIN_ARROW_COVERAGE.iter().enumerate() {
        let row_y = if up { 3 - j } else { j };
        for (i, &a) in row.iter().enumerate() {
            let (x, y) = (ox + i as i32, oy + row_y as i32);
            if a == 0 || y < 0 || x < 0 || y as usize >= pixels.len() {
                continue;
            }
            if let Some(p) = pixels[y as usize].get_mut(x as usize) {
                // `:disabled::after { opacity: .5 }`.
                let a = if enabled { a } else { a / 2 };
                *p = aa::blit_aa(NUMBER_SPIN_ARROW, a, *p);
            }
        }
    }
    for p in pixels.iter_mut().flatten() {
        *p = fade(*p);
    }
    paint_pixels(&painter, pad.min, &pixels);
    response
}

/// Spin-box styling: the toolbar's (inner input border) or a dialog's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberStyle {
    /// `#toolMenu .tool-option input[type="number"]`: a bare input (no `NumberInput.wrap`, no spin
    /// buttons: Paint.NET's toolbar text boxes) with a 1px
    /// `--pdn-control-border` border, the 12px toolbar font and `text-align: left`; otherwise
    /// the wrapped `input[type="number"] { text-align: right }`.
    pub toolbar: bool,
    /// The input's `min-height`: a row taller than the box's content (`.pdn-number-input`,
    /// `overflow: hidden`) lays out the input and the spin column that tall and clips them.
    pub min_height: f32,
    /// The value's font size.
    pub font: f32,
    /// The input's `padding-right` (component.css: 3 px).
    pub pad_right: f32,
    /// The input's own border inside the wrapper (`--pdn-control-border`), 0 for none.
    pub inner_border: f32,
    /// Chromium's `::-webkit-inner-spin-button` (`-webkit-appearance: auto`): the width it keeps
    /// at the right of the text area, 0 when hidden.
    pub spin_reserve: f32,
    /// `toFixed(n)` instead of the number's own string (`input.value = numeric`).
    pub decimals: Option<usize>,
    /// The input's `padding-left` and vertical padding (component.css: 3 px and 1 px).
    pub pad_left: f32,
    pub pad_y: f32,
    /// Chromium draws its inner spin button (`spin_reserve` wide) while the input is hovered or
    /// focused.
    pub spin_glyph: bool,
    /// The input matches `:hover` through its `<label>` (Blink: a hovered label hovers its
    /// control).
    pub hovered: bool,
    /// A strict step overflow lands on the opposite endpoint (plugin RangeWraps, R107).
    pub wrap: bool,
    /// Native hue numeric stepping discards overshoot (§7.4).
    pub wrap_endpoint: bool,
    /// Native property numeric controls add the increment without HTML grid snapping (R107).
    pub native_step: bool,
}

impl NumberStyle {
    pub const TOOLBAR: NumberStyle = NumberStyle {
        toolbar: true,
        min_height: 0.0,
        font: TOOL_FONT_SIZE,
        pad_right: NUMBER_PADDING.1,
        inner_border: 0.0,
        spin_reserve: 0.0,
        decimals: None,
        pad_left: NUMBER_PADDING.1,
        pad_y: NUMBER_PADDING.0,
        spin_glyph: false,
        hovered: false,
        wrap: false,
        wrap_endpoint: false,
        native_step: false,
    };
    pub const DIALOG: NumberStyle = NumberStyle {
        toolbar: false,
        min_height: 0.0,
        font: FONT_SIZE_PX,
        pad_right: NUMBER_PADDING.1,
        inner_border: 0.0,
        spin_reserve: 0.0,
        decimals: None,
        pad_left: NUMBER_PADDING.1,
        pad_y: NUMBER_PADDING.0,
        spin_glyph: false,
        hovered: false,
        wrap: false,
        wrap_endpoint: false,
        native_step: false,
    };
    /// css/window.css `#colorsForm #sliderPanel .channel .row input { min-height: 25px }`, in a
    /// `.window` (12 DIP).
    pub const COLORS: NumberStyle = NumberStyle {
        toolbar: false,
        min_height: 25.0,
        font: FORM_FONT_SIZE,
        pad_right: NUMBER_PADDING.1,
        inner_border: 0.0,
        spin_reserve: 0.0,
        decimals: None,
        pad_left: NUMBER_PADDING.1,
        pad_y: NUMBER_PADDING.0,
        spin_glyph: false,
        hovered: false,
        wrap: false,
        wrap_endpoint: false,
        native_step: false,
    };
    /// `.effect-config-row input[type=number] { padding-right: 5px }` in a dialog (12 DIP).
    pub const EFFECT: NumberStyle = NumberStyle {
        toolbar: false,
        min_height: 0.0,
        font: DIALOG_FONT_SIZE,
        pad_right: EFFECT_NUMBER_PADDING_RIGHT,
        inner_border: 0.0,
        spin_reserve: 0.0,
        decimals: None,
        pad_left: NUMBER_PADDING.1,
        pad_y: NUMBER_PADDING.0,
        spin_glyph: false,
        hovered: false,
        wrap: false,
        wrap_endpoint: false,
        native_step: false,
    };
    /// `.effect-special-slider-line input[type=number]`, `.levels-column input[type=number]`: a
    /// 2-px `--pdn-control-border` border and Chromium's inner spin button (15 px, measured from
    /// the references; shown while hovered or focused, as for the other numeric fields).
    pub const EFFECT_SPECIAL: NumberStyle = NumberStyle {
        toolbar: false,
        min_height: 0.0,
        font: DIALOG_FONT_SIZE,
        pad_right: NUMBER_PADDING.1,
        inner_border: EFFECT_SPECIAL_NUMBER_BORDER,
        spin_reserve: 15.0,
        decimals: None,
        pad_left: NUMBER_PADDING.1,
        pad_y: NUMBER_PADDING.0,
        spin_glyph: true,
        hovered: false,
        wrap: false,
        wrap_endpoint: false,
        native_step: false,
    };

    /// `.new-file-row input[type=number] { padding: 2px 5px }` (12 DIP).
    pub const NEW_FILE: NumberStyle = NumberStyle {
        pad_left: NEW_FILE_NUMBER_PADDING.1,
        pad_y: NEW_FILE_NUMBER_PADDING.0,
        pad_right: NEW_FILE_NUMBER_PADDING.1,
        ..NumberStyle::EFFECT
    };
    /// `.image-size-row input[type=number] { padding: 2px 4px }` with
    /// `::-webkit-inner-spin-button { -webkit-appearance: auto }` (15 px, measured).
    pub const IMAGE_SIZE: NumberStyle = NumberStyle {
        pad_left: IMAGE_SIZE_NUMBER_PADDING.1,
        pad_y: IMAGE_SIZE_NUMBER_PADDING.0,
        pad_right: IMAGE_SIZE_NUMBER_PADDING.1,
        spin_reserve: INNER_SPIN_SIZE.0,
        spin_glyph: true,
        ..NumberStyle::EFFECT
    };
    /// `.image-size-percent`: its own `padding: 2px 4px` loses to `.pdn-number-input >
    /// input[type=number] { padding: 1px 3px }` (more specific).
    pub const IMAGE_SIZE_PERCENT: NumberStyle = NumberStyle {
        pad_left: NUMBER_PADDING.1,
        pad_y: NUMBER_PADDING.0,
        pad_right: NUMBER_PADDING.1,
        ..NumberStyle::IMAGE_SIZE
    };
    /// `.layer-properties-opacity-value[type=number]` (12 DIP): its `padding-right: 5px` loses
    /// to `.pdn-number-input > input[type=number] { padding: 1px 3px }` (more specific).
    pub const LAYER_OPACITY: NumberStyle = NumberStyle {
        pad_right: NUMBER_PADDING.1,
        ..NumberStyle::EFFECT
    };

    /// `hovered` set (`label:hover`).
    pub const fn hovered(mut self, on: bool) -> Self {
        self.hovered = on;
        self
    }

    /// `wrap` set.
    pub const fn wrap(mut self, on: bool) -> Self {
        self.wrap = on;
        self
    }

    pub const fn decimals(mut self, n: usize) -> Self {
        self.decimals = Some(n);
        self
    }

    fn format(&self, v: f64) -> String {
        match self.decimals {
            Some(n) => format!("{v:.n$}"),
            None => pdn_core::js::number_to_string(v),
        }
    }
}

/// Per-widget editing state.
#[derive(Clone, Default)]
struct NumberEdit {
    /// The input's text while it has focus (egui edits it).
    text: Option<String>,
    /// When the held spin button steps next (ms).
    repeat_at: Option<f64>,
    /// The text was typed or stepped since the focus came: blurring applies it (`change` fires
    /// only for a value the user changed).
    edited: bool,
}

/// `HTMLInputElement.stepUp()`/`stepDown()` by one step from `value`: an off-grid value snaps to
/// the step grid (based at `min`) in the step's direction instead; the result clamps to the
/// grid-aligned `[min, max]`, and a step that would move backwards leaves the value.
pub fn html_step(value: f64, dir: f64, min: f64, max: f64, step: f64) -> f64 {
    let base = if min.is_finite() { min } else { 0.0 };
    let k = (value - base) / step;
    let mut v = if (k - k.round()).abs() > 1e-9 {
        base + if dir > 0.0 { k.ceil() } else { k.floor() } * step
    } else {
        value + dir * step
    };
    if min.is_finite() && v < min {
        v = min;
    }
    if max.is_finite() && v > max {
        v = base + ((max - base) / step).floor() * step;
    }
    if (dir > 0.0 && v < value) || (dir < 0.0 && v > value) {
        value
    } else {
        v
    }
}

/// Native wrapping numeric step (R107 / Q23): clamp pending text first; strict overflow lands
/// on the opposite endpoint. Non-wrapping callers keep the existing HTML step behavior.
pub fn number_step(value: f64, dir: f64, min: f64, max: f64, step: f64, wrap: bool) -> f64 {
    if wrap {
        native_number_step(value, dir, min, max, step, true)
    } else {
        html_step(value, dir, min, max, step)
    }
}

fn native_number_step(value: f64, dir: f64, min: f64, max: f64, step: f64, wrap: bool) -> f64 {
    let raw = value.clamp(min, max) + dir * step;
    if wrap && raw > max {
        min
    } else if wrap && raw < min {
        max
    } else {
        raw.clamp(min, max)
    }
}

/// The ArrowUp/ArrowDown presses (without Alt/Ctrl/Meta) this frame, consumed: +1 / -1 each
/// (`NumberInput.enableKeyboardStepping`).
pub fn take_arrow_steps(ui: &Ui) -> Vec<f64> {
    arrow_steps(ui, false)
}

fn arrow_steps(ui: &Ui, unmodified: bool) -> Vec<f64> {
    ui.input_mut(|i| {
        let mut steps = vec![];
        i.events.retain(|e| match e {
            egui::Event::Key {
                key: key @ (egui::Key::ArrowUp | egui::Key::ArrowDown),
                pressed,
                modifiers,
                ..
            } if !modifiers.alt
                && !modifiers.ctrl
                && !modifiers.mac_cmd
                && (!unmodified || !modifiers.shift) =>
            {
                if *pressed {
                    steps.push(if *key == egui::Key::ArrowUp {
                        1.0
                    } else {
                        -1.0
                    });
                }
                false
            }
            _ => true,
        });
        steps
    })
}

/// CSS `opacity: k` over the flat `backdrop` for an opaque group: every colour painted inside it
/// mixes toward the backdrop (exact for opaque layers: compositing is linear).
fn fade(c: Color32, k: f32, backdrop: Color32) -> Color32 {
    if k >= 1.0 {
        return c;
    }
    let m = |a: u8, b: u8| (a as f32 * k + b as f32 * (1.0 - k)).round() as u8;
    Color32::from_rgb(
        m(c.r(), backdrop.r()),
        m(c.g(), backdrop.g()),
        m(c.b(), backdrop.b()),
    )
}

/// egui's `TextEdit` working an `<input>`'s text invisibly at `rect` (keys, IME, the selection
/// and the caret): the caller paints the text, the selection and the caret as Blink does.
/// Returns the response and the selected char range (start ≤ end; equal for a caret).
fn hidden_text_edit(
    ui: &mut Ui,
    id: egui::Id,
    rect: Rect,
    buffer: &mut String,
    size: f32,
    enter_blurs: bool,
) -> (Response, (usize, usize)) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let v = child.visuals_mut();
    v.selection.bg_fill = Color32::TRANSPARENT;
    v.selection.stroke.color = Color32::TRANSPARENT;
    v.text_cursor.stroke.color = Color32::TRANSPARENT;
    let response = child.put(
        rect,
        egui::TextEdit::singleline(buffer)
            .id(id)
            // An `<input>` keeps its focus on Escape and the arrows.
            .event_filter(egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
                tab: false,
            })
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(egui::FontId::proportional(size))
            .text_color(Color32::TRANSPARENT)
            .return_key(enter_blurs.then_some(egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Enter,
            ))),
    );
    let n = buffer.chars().count();
    let range = egui::text_edit::TextEditState::load(ui.ctx(), id)
        .and_then(|s| s.cursor.char_range())
        .map_or((n, n), |r| {
            let (a, b) = (
                usize::from(r.primary.index).min(n),
                usize::from(r.secondary.index).min(n),
            );
            (a.min(b), a.max(b))
        });
    (response, range)
}

/// Gives the text edit `id` the keyboard focus with all of `text` selected (`input.select()`,
/// or Tab into an `<input>`).
fn select_all(ctx: &egui::Context, id: egui::Id, text: &str) {
    use egui::text::{CCursor, CCursorRange};
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(0),
        CCursor::new(text.chars().count()),
    )));
    state.store(ctx, id);
    ctx.memory_mut(|m| m.request_focus(id));
}

/// Shift+Tab out of an `<input>`'s hidden editor `edit_id` lands on the Tab stop `field_id` in
/// front of it: that stop passes the focus on to the widget before it. Call before `field_id`
/// registers (egui moves a focus `Previous` as the focused widget registers). True when it did;
/// any other arrival on the stop (Tab, a dialog's `input.select()`) selects the editor's text.
fn pass_back(ctx: &egui::Context, field_id: egui::Id, edit_id: egui::Id) -> bool {
    let back = ctx.memory(|m| m.has_focus(field_id)) && had_focus(ctx, edit_id);
    if back {
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Previous));
    }
    back
}

/// Whether the editor `edit_id` had the keyboard focus at the end of the last frame.
fn had_focus(ctx: &egui::Context, edit_id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(edit_id.with("had_focus")))
        .unwrap_or(false)
}

/// Records whether the editor `edit_id` has the keyboard focus now (for `had_focus`).
fn note_focus(ctx: &egui::Context, edit_id: egui::Id) {
    let on = ctx.memory(|m| m.has_focus(edit_id));
    ctx.data_mut(|d| d.insert_temp(edit_id.with("had_focus"), on));
}

/// A press on `response` that toggles a checkbox or radio: a pointer click or Space. Enter on a
/// focused one submits its form instead (egui clicks focused widgets on Enter too).
fn toggled(ui: &Ui, response: &Response) -> bool {
    response.clicked() && !(response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
}

/// Enter activated the focused push button `response` (one outside a dialog's footer: a reset
/// button, an anchor cell): the dialog's Enter → OK skips this frame (`enter_taken`).
pub fn take_enter(ui: &Ui, response: &Response) {
    if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let pass = ui.ctx().cumulative_pass_nr();
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::unique("paintrs.enter_taken"), pass));
    }
}

/// A push button took this frame's Enter (`take_enter`).
pub fn enter_taken(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::unique("paintrs.enter_taken"))) == Some(pass)
}

/// An edited `<input>`'s text at `x` on the line box at `top`, as Blink paints a focused
/// control: the selection's box (`::selection`, white text) or the 1-px caret, kept left of
/// `caret_max` (Blink moves a caret at the content box's right edge inside it).
#[allow(clippy::too_many_arguments)]
fn paint_edit_text(
    painter: &Painter,
    text: &str,
    x: f32,
    top: f32,
    font: FontSpec,
    color: Color32,
    (start, end): (usize, usize),
    caret_max: f32,
) {
    let ppp = painter.pixels_per_point();
    let line = font.resolved_line_height(ppp);
    let at = |i: usize| {
        let prefix: String = text.chars().take(i).collect();
        x + text::css_width(&prefix, font.size)
    };
    let snap = |v: f32| (v * ppp).round() / ppp;
    if start != end {
        let r = Rect::from_min_max(
            Pos2::new(snap(at(start)), snap(top)),
            Pos2::new(snap(at(end)), snap(top + line)),
        );
        painter.rect_filled(r, 0.0, theme::text_selection(painter.ctx()));
        text::draw(painter, Pos2::new(x, top), text, font, Color32::WHITE);
    } else {
        text::draw(painter, Pos2::new(x, top), text, font, color);
        let cx = snap(at(start).min(caret_max - 1.0 / ppp));
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(cx, snap(top)), Vec2::new(1.0 / ppp, snap(line))),
            0.0,
            color,
        );
    }
}

/// `NumberInput.wrap(input[type=number])` with the tool options' `createNumberControl`
/// handlers: `rect` is the `.pdn-number-input` border box (layout px), `name` the accessible
/// name of its text field. Returns the field's
/// response; `value` changes on steps and edits (`oninput` applies a finite entry clamped,
/// `onchange` normalizes). A click puts the caret in it; the keyboard focus arriving otherwise
/// (Tab, a dialog's initial focus on the returned field) selects its text (`input.select()`).
/// While it has the focus the wrapper takes `:focus-within`'s colours and Blink's selection or
/// caret shows. Disabled, the wrapper is at `opacity: .55` (`:has(input:disabled)`).
#[allow(clippy::too_many_arguments)]
pub fn number_input(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    rect: Rect,
    value: &mut f64,
    min: f64,
    max: f64,
    step: f64,
    enabled: bool,
    style: NumberStyle,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let b = px::border_width(1.0, ppp);
    let snapped = px::snap_rect(rect, ppp);
    let id = ui.scope_id().with(("number_input", id_salt));
    let edit_id = id.with("edit");
    let wrapped = !style.toolbar;
    let focused = enabled && ui.memory(|m| m.has_focus(edit_id));
    let k = if wrapped && !enabled {
        NUMBER_DISABLED_OPACITY
    } else {
        1.0
    };
    let f = |c: Color32| fade(c, k, theme.window_background);
    if wrapped {
        // `outline: 1px` outside the border, then `border: 1px`, both `--color-button-outline`
        // (`:focus-within`: `--color-menu-item-selected-border`).
        let ring = if focused {
            theme.menu_item_selected_border
        } else {
            theme.button_outline
        };
        painter.rect_filled(snapped.expand(b), 0.0, f(ring));
        painter.rect_filled(snapped.shrink(b), 0.0, f(theme.number_input_background));
    }
    let (column, inner) = if wrapped {
        (NUMBER_SPIN_COLUMN, rect.shrink(b))
    } else {
        (0.0, rect)
    };
    // The grid row: the content height, or the input's taller `min-height` (clipped below).
    let row_h = inner.height().max(style.min_height);
    let input = Rect::from_min_size(inner.min, Vec2::new(inner.width() - column, row_h));
    let input_px = px::snap_rect(input, ppp);
    let mut content = input;
    if style.toolbar {
        painter.rect_filled(input_px, 0.0, theme.pdn_control_border);
        painter.rect_filled(input_px.shrink(b), 0.0, theme.pdn_control_background);
        content = content.shrink(b);
    } else if style.inner_border > 0.0 {
        let ib = px::border_width(style.inner_border, ppp);
        painter.rect_filled(input_px, 0.0, f(theme.pdn_control_border));
        painter.rect_filled(input_px.shrink(ib), 0.0, f(theme.number_input_background));
        content = content.shrink(style.inner_border);
    }
    let padded = Rect::from_min_max(
        content.min + Vec2::new(style.pad_left, style.pad_y),
        content.max - Vec2::new(style.pad_right, style.pad_y),
    );
    // Chromium's inner spin button keeps its width at the right of the text area.
    let content = Rect::from_min_max(padded.min, padded.max - Vec2::new(style.spin_reserve, 0.0));
    let font = FontSpec::new(style.font);
    let mut edit = ui
        .data(|d| d.get_temp::<NumberEdit>(id))
        .unwrap_or_default();
    // The Tab stop in front of the editor (a disabled input is none): the editor takes the focus.
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let ctx = ui.ctx().clone();
    let back = pass_back(&ctx, id.with("field"), edit_id);
    let field = ui.interact(input_px, id.with("field"), sense);
    let focus = |edit: &mut NumberEdit, value: f64| {
        if edit.text.is_none() {
            edit.text = Some(style.format(value));
        }
        ctx.memory_mut(|m| m.request_focus(edit_id));
    };
    // A field created by this click would surrender its focus to the same click.
    let started = field.clicked() && enabled && edit.text.is_none();
    if field.clicked() && enabled {
        focus(&mut edit, *value);
    } else if field.has_focus() && enabled && !back {
        let text = edit.text.get_or_insert_with(|| style.format(*value));
        select_all(&ctx, edit_id, text);
    }
    // `stepInput`: `stepUp()`/`stepDown()` on the input's value (an invalid one counts as 0),
    // then its `input` and `change` events: the new text, applied clamped.
    let step_input = |edit: &mut NumberEdit, value: &mut f64, dir: f64| {
        let current = if style.native_step && !edit.edited {
            Some(*value)
        } else {
            match &edit.text {
                Some(t) => t.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
                None => Some(*value),
            }
        };
        let current = current.unwrap_or(0.0);
        let raw = current + dir * step;
        let stepped = if style.native_step {
            native_number_step(current, dir, min, max, step, style.wrap)
        } else if style.wrap_endpoint && raw > max {
            min
        } else if style.wrap_endpoint && raw < min {
            max
        } else {
            number_step(current, dir, min, max, step, style.wrap)
        };
        if edit.text.is_some() {
            edit.text = Some(if style.native_step {
                style.format(stepped)
            } else {
                pdn_core::js::number_to_string(stepped)
            });
            // Native formatting never rounds the stored numeric value on blur or the next step.
            edit.edited = !style.native_step;
        }
        *value = stepped.clamp(min, max);
    };
    if focused {
        for dir in arrow_steps(ui, style.native_step) {
            step_input(&mut edit, value, dir);
        }
    }
    // The spin buttons' `pointerdown` is default-prevented: pressing them keeps the input's
    // focus (egui would surrender it to a press elsewhere).
    let col = Rect::from_min_size(
        Pos2::new(input.max.x, inner.min.y),
        Vec2::new(column, row_h),
    );
    // `.pdn-number-input { overflow: hidden }`.
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(px::snap_rect(inner, ppp).intersect(outer_clip));
    let painter = painter.with_clip_rect(ui.clip_rect());
    // Where the press (else the click's release) went, not the pointer at the frame's end.
    let spin_press = ui.input(|i| i.pointer.any_pressed() || i.pointer.any_click())
        && press_pos(ui.ctx(), None)
            .or_else(|| last_button_event(ui.ctx(), false, None))
            .is_some_and(|p| col.contains(p));
    let text_top = content.min.y + (content.height() - font.resolved_line_height(ppp)) / 2.0;
    // Where a value of css width `w` starts: an overflowing one at the left, cut at the text area
    // (Blink's inner editor), whatever the alignment.
    let start_x = |w: f32| {
        if style.toolbar || w > content.width() {
            content.min.x
        } else {
            content.max.x - w
        }
    };
    // A wrapped input keeps its author colour when disabled (the wrapper's opacity fades it).
    let color = if enabled || wrapped {
        f(theme.menu_item_text)
    } else {
        MENU_ITEM_DISABLED_TEXT
    };
    let text_clip = |w: f32| {
        let area = if style.toolbar || w <= content.width() {
            input_px
        } else {
            content
        };
        let area = Rect::from_min_max(
            Pos2::new(area.min.x, input_px.min.y),
            Pos2::new(area.max.x, input_px.max.y),
        );
        painter.with_clip_rect(px::snap_rect(area, ppp).intersect(painter.clip_rect()))
    };
    let hot = enabled && (focused || field.hovered() || style.hovered);
    if style.spin_glyph && hot {
        paint_inner_spin(&painter, padded, text_top);
    }
    match edit.text.as_mut() {
        Some(buffer) => {
            let edit_rect = Rect::from_min_max(Pos2::new(content.min.x, text_top), content.max);
            let (response, range) =
                hidden_text_edit(ui, edit_id, edit_rect, buffer, font.size, true);
            ui.ctx()
                .accesskit_node_builder(edit_id, |node| node.set_label(name));
            let w = text::css_width(buffer, font.size);
            if response.has_focus() {
                let clip = text_clip(w);
                let (x, max) = (start_x(w), f32::INFINITY);
                paint_edit_text(&clip, buffer, x, text_top, font, color, range, max);
            } else {
                text::draw(
                    &text_clip(w),
                    Pos2::new(start_x(w), text_top),
                    buffer,
                    font,
                    color,
                );
            }
            let entered = buffer.trim().parse::<f64>().ok().filter(|v| v.is_finite());
            // `oninput`: a finite entry applies, clamped.
            if response.changed() {
                edit.edited = true;
                if let Some(v) = entered {
                    *value = v.clamp(min, max);
                }
            }
            let keep_focus = (spin_press && enabled) || started;
            if keep_focus {
                ui.memory_mut(|m| m.request_focus(edit_id));
            }
            // `onchange` (blur), only after an edit: `Number(input.value)`; an invalid entry's
            // value is "" (0). An unedited field leaves the value as it was (its shown text may
            // be rounded).
            if response.lost_focus() && !keep_focus {
                if std::mem::take(&mut edit.edited) {
                    *value = entered.unwrap_or(0.0).clamp(min, max);
                }
                edit.text = None;
            }
        }
        None => {
            let shown = style.format(*value);
            let w = text::css_width(&shown, font.size);
            text::draw(
                &text_clip(w),
                Pos2::new(start_x(w), text_top),
                &shown,
                font,
                color,
            );
        }
    }
    note_focus(&ctx, edit_id);
    if !wrapped {
        ui.set_clip_rect(outer_clip);
        ui.data_mut(|d| d.insert_temp(id, edit));
        return field;
    }
    // The button column: two 1fr rows.
    let half = col.height() / 2.0;
    let up_rect = Rect::from_min_size(col.min, Vec2::new(col.width(), half));
    let down_rect =
        Rect::from_min_size(col.min + Vec2::new(0.0, half), Vec2::new(col.width(), half));
    let now = ui.input(|i| i.time) * 1000.0;
    for (r, dir, up) in [(up_rect, 1.0, true), (down_rect, -1.0, false)] {
        let response = spin_button(ui, id.with(up), r, up, enabled, &|c| f(c));
        if !enabled {
            continue;
        }
        if response.is_pointer_button_down_on() {
            match edit.repeat_at {
                // `onpointerdown`: focus the input, step now; the repeats start at 465 ms.
                None if ui.input(|i| i.pointer.primary_pressed()) => {
                    focus(&mut edit, *value);
                    step_input(&mut edit, value, dir);
                    edit.repeat_at = Some(now + REPEAT_DELAY_MS + REPEAT_INTERVAL_MS);
                }
                // A late timer fires once and reschedules from now (no catch-up burst).
                Some(t) if now >= t => {
                    step_input(&mut edit, value, dir);
                    edit.repeat_at = Some(now + REPEAT_INTERVAL_MS);
                }
                _ => {}
            }
            // The next repeat (a held button repaints only when one is due).
            let wait = edit.repeat_at.map_or(0.0, |t| (t - now).max(0.0));
            ui.ctx().request_repaint_after_secs((wait / 1000.0) as f32);
        } else if response.drag_stopped() || ui.input(|i| i.pointer.any_released()) {
            edit.repeat_at = None;
        }
    }
    ui.set_clip_rect(outer_clip);
    ui.data_mut(|d| d.insert_temp(id, edit));
    field
}

/// Chromium's `::-webkit-inner-spin-button` (`-webkit-appearance: auto`) at the right of the
/// padding box `padded`, on the text's line box (its top `line_top`), shown while the input is
/// hovered or focused: a 15×14 box with two 7-wide triangles (measured from the dialog-resize and
/// dialog-canvas-size references).
fn paint_inner_spin(painter: &Painter, padded: Rect, line_top: f32) {
    let ppp = painter.pixels_per_point();
    let light = theme::current(painter.ctx()).scheme == theme::Scheme::Light;
    let (fill, arrow) = if light {
        INNER_SPIN_LIGHT
    } else {
        (INNER_SPIN_FILL, INNER_SPIN_ARROW)
    };
    let (w, h) = INNER_SPIN_SIZE;
    let x0 = ((padded.max.x - w) * ppp).round() / ppp;
    let y0 = (line_top * ppp).round() / ppp;
    let one = 1.0 / ppp;
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(x0, y0), Vec2::new(w, h)),
        0.0,
        fill,
    );
    // Rows 1–4 the up triangle (1, 3, 5, 7 px), rows 9–12 the down one, centred at x 7.
    for (row, half) in [
        (1, 0),
        (2, 1),
        (3, 2),
        (4, 3),
        (9, 3),
        (10, 2),
        (11, 1),
        (12, 0),
    ] {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(x0 + (7 - half) as f32 * one, y0 + row as f32 * one),
                Vec2::new((2 * half + 1) as f32 * one, one),
            ),
            0.0,
            arrow,
        );
    }
}

/// A text `<input>` (`input[type=text]`) at `rect` (its border box), its text left-aligned after
/// `pad` (`right`: right-aligned before it, `text-align: right`), vertically centred: `border`
/// 1 px, `fill`; focused, Blink's selection or caret and a 2-px `box-shadow` ring of
/// `--color-menu-item-border` (`:focus-visible`). Focus arriving without a click (Tab, the
/// dialog's `input.select()` on the returned response) selects the text; Enter keeps the focus
/// (an `<input>` has no default Enter action). `font` the text's size. Not `enabled`, it shows
/// its text and takes neither the pointer nor the keyboard.
#[allow(clippy::too_many_arguments)]
pub fn text_input(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    rect: Rect,
    text: &mut String,
    (fill, border): (Color32, Color32),
    pad: f32,
    font: f32,
    right: bool,
    enabled: bool,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let id = ui.scope_id().with(("text_input", id_salt));
    let edit_id = id.with("edit");
    let snapped = px::snap_rect(rect, ppp);
    let b = px::border_width(1.0, ppp);
    if !enabled {
        ui.memory_mut(|m| m.surrender_focus(edit_id));
    }
    let focused = ui.memory(|m| m.has_focus(edit_id));
    if focused {
        painter.rect_filled(snapped.expand(2.0 * b), 0.0, theme.menu_item_border);
    }
    painter.rect_filled(snapped, 0.0, border);
    painter.rect_filled(snapped.shrink(b), 0.0, fill);
    let font = FontSpec::new(font);
    if !enabled {
        let inner = rect.shrink(1.0);
        let w = text::css_width(text, font.size);
        let x = if right {
            (inner.max.x - pad - w).max(inner.min.x + pad)
        } else {
            inner.min.x + pad
        };
        let top = inner.min.y + (inner.height() - font.resolved_line_height(ppp)) / 2.0;
        let clip = painter.with_clip_rect(px::snap_rect(inner, ppp).intersect(painter.clip_rect()));
        text::draw(&clip, Pos2::new(x, top), text, font, theme.menu_item_text);
        return ui.interact(snapped, id.with("field"), Sense::hover());
    }
    let back = pass_back(ui.ctx(), id.with("field"), edit_id);
    let field = ui.interact(snapped, id.with("field"), Sense::click());
    if field.clicked() {
        ui.memory_mut(|m| m.request_focus(edit_id));
    } else if field.has_focus() && !back {
        select_all(ui.ctx(), edit_id, text);
    }
    let inner = rect.shrink(1.0);
    let start = |text: &str| {
        if right {
            // An overflowing value starts at the left (Blink's inner editor).
            (inner.max.x - pad - text::css_width(text, font.size)).max(inner.min.x + pad)
        } else {
            inner.min.x + pad
        }
    };
    let top = inner.min.y + (inner.height() - font.resolved_line_height(ppp)) / 2.0;
    let edit_rect = Rect::from_min_max(Pos2::new(start(text), top), inner.max);
    let (response, range) = hidden_text_edit(ui, edit_id, edit_rect, text, font.size, false);
    let x = start(text);
    ui.ctx()
        .accesskit_node_builder(edit_id, |node| node.set_label(name));
    let clip = painter.with_clip_rect(px::snap_rect(inner, ppp).intersect(painter.clip_rect()));
    if response.has_focus() {
        let caret_max = inner.max.x - pad;
        let color = theme.menu_item_text;
        paint_edit_text(&clip, text, x, top, font, color, range, caret_max);
    } else {
        text::draw(&clip, Pos2::new(x, top), text, font, theme.menu_item_text);
    }
    note_focus(ui.ctx(), edit_id);
    field.union(response)
}

/// The `text_input` of `id_salt` in `ui` has the keyboard (its editor holds the focus; the
/// returned response is its Tab stop's).
pub fn text_input_focused(ui: &Ui, id_salt: impl std::hash::Hash + std::fmt::Debug) -> bool {
    let edit_id = ui.scope_id().with(("text_input", id_salt)).with("edit");
    ui.memory(|m| m.has_focus(edit_id))
}

/// `.image-size-radio input[type=radio]` (css/window.css): a 16-px circle, `#f5f5f5` with a 1-px
/// `#a7a7a7` border; checked, `#0787d8` (border too) with a 4-px `#f5f5f5` inset ring. Chromium's
/// anti-aliased circles (16×16 supersampled coverage). `backdrop` the flat colour under it.
/// True when it is clicked or Space-pressed (`toggled`).
pub fn radio(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    checked: bool,
    backdrop: Color32,
) -> bool {
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(
        snapped,
        ui.scope_id().with(("radio", id_salt)),
        Sense::click(),
    );
    let (fill, border, dot) = RADIO_COLORS;
    let d = Rect::from_min_max(snapped.min * ppp, snapped.max * ppp);
    let (w, h) = (d.width() as usize, d.height() as usize);
    let local = Rect::from_min_size(Pos2::ZERO, d.size());
    let r = local.width() / 2.0;
    let outer = rrect_coverage(w, h, local, r);
    // Skia's inner edge of the 1-px border ring sits 0.2 px inside the padding edge (fitted to
    // the dialog-resize reference).
    let inner = RADIO_INNER_EDGE * ppp;
    let inside = rrect_coverage(w, h, local.shrink(inner), r - inner);
    // Checked: the border box in `#0787d8`, the inset shadow's `#f5f5f5` over the padding box,
    // and what the 4-px shadow leaves (the dot) in `#0787d8` again.
    let (ring, center) = if checked {
        (
            dot,
            Some(rrect_coverage(w, h, local.shrink(5.0 * ppp), r - 5.0 * ppp)),
        )
    } else {
        (border, None)
    };
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if outer[i] == 0 {
                continue;
            }
            let mut c = aa::blit_aa(ring, outer[i], backdrop);
            c = aa::blit_aa(fill, inside[i], c);
            if let Some(center) = &center {
                c = aa::blit_aa(dot, center[i], c);
            }
            let p = Rect::from_min_size(
                (d.min + Vec2::new(x as f32, y as f32)) / ppp,
                Vec2::splat(1.0 / ppp),
            );
            painter.rect_filled(p, 0.0, c);
        }
    }
    toggled(ui, &response)
}

/// Native button activation: arm once on Space down, activate once on release, and disarm
/// when the control is disabled or loses focus (adjustments-and-effect-framework §§7.2–7.3).
pub fn space_released(ui: &Ui, response: &Response) -> bool {
    let armed_id = response.id.with("space-armed");
    let mut armed = ui.data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
    let mut released = false;
    if response.enabled() && response.has_focus() {
        ui.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key: egui::Key::Space,
                    pressed,
                    repeat,
                    ..
                } = event
                {
                    if *pressed && !repeat {
                        armed = true;
                    } else if !pressed && std::mem::take(&mut armed) {
                        released = true;
                    }
                }
            }
        });
    } else {
        armed = false;
    }
    ui.data_mut(|d| d.insert_temp(armed_id, armed));
    released
}

// WinForms dialog defaults exclude Ctrl/Alt Enter, including a focused push button (§7.3).
pub fn button_activated(ui: &Ui, response: &Response) -> bool {
    let released = space_released(ui, response);
    let synthetic_key = response.has_focus()
        && ui.input(|i| {
            i.key_pressed(egui::Key::Space)
                || (i.key_pressed(egui::Key::Enter)
                    && (i.modifiers.ctrl || i.modifiers.alt || i.modifiers.mac_cmd))
        });
    released
        || response.clicked_by(egui::PointerButton::Primary)
        || (response.clicked() && !synthetic_key)
}

/// `input[type=checkbox]` (css/component.css): the 12×12 box with `border-radius: 2px`,
/// `--color-button` with a 1-px `--color-button-outline` outline (which follows the radius), or
/// `--color-checkbox-checked` with the white data-URI check when checked. `rect` is the box,
/// `backdrop` the flat colour under it. Anti-aliased like Chromium's rounded rects (16×16
/// supersampled coverage; the edges are within the R21 edge tolerance, the flat fills exact).
/// No hover look (the references show none).
pub fn checkbox(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    checked: &mut bool,
    enabled: bool,
    backdrop: Color32,
) -> Response {
    checkbox_in(ui, id_salt, rect, rect, checked, enabled, backdrop)
}

/// A checkbox whose box and inline caption share one click target and one Tab destination.
#[allow(clippy::too_many_arguments)]
pub fn checkbox_in(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    hit_rect: Rect,
    checked: &mut bool,
    enabled: bool,
    backdrop: Color32,
) -> Response {
    let ppp = ui.ctx().pixels_per_point();
    let id = ui.scope_id().with(("checkbox", id_salt));
    let response = ui.interact(
        px::snap_rect(hit_rect, ppp),
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let released = space_released(ui, &response);
    let synthetic_key = response.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
    if enabled && (released || (response.clicked() && !synthetic_key)) {
        *checked = !*checked;
    }
    ui.ctx().accesskit_node_builder(id, |n| {
        n.set_toggled((*checked).into());
    });
    paint_checkbox(ui.painter(), rect, *checked, backdrop);
    response
}

/// The [`checkbox`] box alone, painted in `rect` over flat `backdrop`, with no interaction.
pub fn paint_checkbox(painter: &Painter, rect: Rect, checked: bool, backdrop: Color32) {
    let theme = theme::current(painter.ctx());
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let (fill, outline) = if checked {
        (theme.checkbox_checked, theme.checkbox_checked)
    } else {
        (theme.button, theme.button_outline)
    };
    // Device pixels: the outline box and the radii (2 inside, 2 + the outline outside).
    let bw = (px::border_width(1.0, ppp) * ppp).round();
    let inner = Rect::from_min_max(snapped.min * ppp, snapped.max * ppp);
    let outer = inner.expand(bw);
    let r = CHECKBOX_RADIUS * ppp;
    let (w, h) = (outer.width() as usize, outer.height() as usize);
    let cov = checkbox_coverage(painter.ctx(), inner.size(), bw, r, checked);
    let (out_cov, in_cov, check) = (&cov.0, &cov.1, &cov.2);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut c = aa::blit_aa(outline, out_cov[i], backdrop);
            if out_cov[i] == 0 {
                c = backdrop;
            }
            if in_cov[i] > 0 {
                c = aa::blit_aa(fill, in_cov[i], c);
            }
            if let Some((at, rows)) = check {
                let (cx, cy) = (x as i64 - at.x as i64, y as i64 - at.y as i64);
                let a = (cx >= 0 && cy >= 0)
                    .then(|| rows.get(cy as usize)?.get(cx as usize).copied())
                    .flatten()
                    .unwrap_or(0);
                if a > 0 {
                    c = aa::blit_aa(Color32::WHITE, a, c);
                }
            }
            if c != backdrop {
                let p = Rect::from_min_size(
                    (outer.min + Vec2::new(x as f32, y as f32)) / ppp,
                    Vec2::splat(1.0 / ppp),
                );
                painter.rect_filled(p, 0.0, c);
            }
        }
    }
}

/// A checkbox's coverages over its outline box (device px): the outline's, the box's, and the
/// check's (top-left pixel, rows) when checked.
type CheckboxCoverage = (Vec<u8>, Vec<u8>, Option<(Pos2, Vec<Vec<u8>>)>);

/// The coverages of a checkbox whose box is `size` device px with an outline of `bw` and inner
/// radius `r`, cached per (size, outline, radius, checked): they depend on nothing else.
fn checkbox_coverage(
    ctx: &egui::Context,
    size: Vec2,
    bw: f32,
    r: f32,
    checked: bool,
) -> std::sync::Arc<CheckboxCoverage> {
    type Cache =
        std::collections::HashMap<(u32, u32, u32, u32, bool), std::sync::Arc<CheckboxCoverage>>;
    let key = (
        size.x as u32,
        size.y as u32,
        bw as u32,
        r.to_bits(),
        checked,
    );
    let id = egui::Id::unique("paintrs.kit.checkbox");
    if let Some(c) = ctx.data(|d| d.get_temp::<Cache>(id).and_then(|m| m.get(&key).cloned())) {
        return c;
    }
    let inner = Rect::from_min_size(Pos2::new(bw, bw), size);
    let outer = inner.expand(bw);
    let (w, h) = (outer.width() as usize, outer.height() as usize);
    let check = checked.then(|| {
        // `M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z` in a 24-unit box.
        let s = inner.width() / 24.0;
        let p = |x: f32, y: f32| inner.min + Vec2::new(x * s, y * s);
        aa::polygon_coverage(&[
            p(9.0, 16.17),
            p(4.83, 12.0),
            p(3.41, 13.41),
            p(9.0, 19.0),
            p(21.0, 7.0),
            p(19.59, 5.59),
        ])
    });
    let cov = std::sync::Arc::new((
        rrect_coverage(w, h, outer, r + bw),
        rrect_coverage(w, h, inner, r),
        check,
    ));
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(id)
            .insert(key, cov.clone());
    });
    cov
}

/// 8-bit coverage of the rounded rect `r` (device px, radius `radius`) over a `w`×`h` grid at
/// the origin, 16×16 samples per pixel.
fn rrect_coverage(w: usize, h: usize, r: Rect, radius: f32) -> Vec<u8> {
    const N: usize = 16;
    let inside = |x: f32, y: f32| {
        if !r.contains(Pos2::new(x, y)) {
            return false;
        }
        let cx = x.clamp(r.min.x + radius, r.max.x - radius);
        let cy = y.clamp(r.min.y + radius, r.max.y - radius);
        (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
    };
    let mut out = vec![0u8; w * h];
    for (i, o) in out.iter_mut().enumerate() {
        let (px, py) = ((i % w) as f32, (i / w) as f32);
        let mut n = 0;
        for sy in 0..N {
            for sx in 0..N {
                let (fx, fy) = ((sx as f32 + 0.5) / N as f32, (sy as f32 + 0.5) / N as f32);
                n += inside(px + fx, py + fy) as u32;
            }
        }
        *o = ((n * 255 + (N * N) as u32 / 2) / (N * N) as u32) as u8;
    }
    out
}

/// `button` (css/component.css): `--color-button` fill, 1px `--color-button-outline` border,
/// `padding: 2px 10px`, centered label; in a `.window`, so 12 DIP. `default` draws the
/// default-button ring.
pub fn push_button(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    label: &str,
    enabled: bool,
    default: bool,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(
        snapped,
        ui.scope_id().with(("button", id_salt)),
        Sense::click(),
    );
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label(label);
    });
    // No `button:hover` rule exists.
    let border = if default {
        theme.menu_item_selected_border
    } else {
        theme.button_outline
    };
    painter.rect_filled(snapped, 0.0, border);
    painter.rect_filled(
        snapped.shrink(px::border_width(1.0, ppp)),
        0.0,
        theme.button,
    );
    let font = FontSpec::new(FORM_FONT_SIZE);
    let w = text::layout_width(&painter, label, font);
    let top = Pos2::new(
        rect.center().x - w / 2.0,
        rect.center().y - font.resolved_line_height(ppp) / 2.0,
    );
    let color = if enabled {
        theme.button_text
    } else {
        MENU_ITEM_DISABLED_TEXT
    };
    text::draw(&painter, top, label, font, color);
    response
}

/// Chromium's `input[type=color]` (paintdotjs's colour row, adjustments-and-effect-framework.md /
/// ui-drawn-graphics.md §2.2 "Reused by": Ruling (P3) 1): the `button`-look box, `padding: 1px
/// 2px`, the swatch wrapper's `4px 2px`, and the swatch with its 1-px #777 border. The caller
/// reads clicks (primary colour) and secondary clicks (secondary colour) off the response. Its id
/// is `key`'s in `ui`, not its place, so it keeps the focus when a dialog's rows scroll.
pub fn color_input(ui: &mut Ui, key: &str, rect: Rect, color: pdn_core::pixel::Px) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let r = px::snap_rect(rect, ppp);
    let response = ui.interact(r, ui.scope_id().with(("color_input", key)), Sense::click());
    let b = px::border_width(1.0, ppp);
    painter.rect_filled(r, 0.0, theme.button_outline);
    painter.rect_filled(r.shrink(b), 0.0, theme.button);
    let swatch = color_swatch(rect, ppp);
    painter.rect_filled(swatch, 0.0, Color32::from_gray(0x77));
    painter.rect_filled(
        swatch.shrink(b),
        0.0,
        Color32::from_rgb(color[0], color[1], color[2]),
    );
    response
}

/// `color_input`'s swatch (its #777 border box) in the box `rect`.
fn color_swatch(rect: Rect, ppp: f32) -> Rect {
    px::snap_rect(
        rect.shrink2(Vec2::new(1.0 + 2.0 + 2.0, 1.0 + 1.0 + 4.0)),
        ppp,
    )
}

/// `color_input` for a colour row "with alpha" (BB common.md §3; BB plan BB5): the
/// swatch shows `color` over 4-px checks of the canvas checkerboard's colours.
pub fn color_input_alpha(
    ui: &mut Ui,
    key: &str,
    rect: Rect,
    color: pdn_core::pixel::Px,
) -> Response {
    let response = color_input(ui, key, rect, color);
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let inner = color_swatch(rect, ppp).shrink(px::border_width(1.0, ppp));
    let (light, dark) = pdn_core::render::checker::checker_colors(1.0);
    let clip = painter.with_clip_rect(inner.intersect(painter.clip_rect()));
    for j in 0..(inner.height() / 4.0).ceil() as usize {
        for i in 0..(inner.width() / 4.0).ceil() as usize {
            let at = inner.min + Vec2::new(i as f32, j as f32) * 4.0;
            let c = if (i + j) % 2 == 0 { light } else { dark };
            clip.rect_filled(
                Rect::from_min_size(at, Vec2::splat(4.0)),
                0.0,
                Color32::from_gray(c),
            );
        }
    }
    let [r, g, b, a] = color;
    clip.rect_filled(inner, 0.0, Color32::from_rgba_unmultiplied(r, g, b, a));
    response
}

/// R118: a plugin Text control's value with Paint.NET's capacity, counted in UTF-16 code units
/// (a supplementary scalar is two, CRLF two). egui keeps the caret, selection, IME, undo and
/// painting; an insertion (typing, paste, IME commit, accessibility) takes the longest
/// whole-scalar prefix that fits after egui has deleted the replaced selection, so no text
/// outside that selection changes. Values already over the limit (defaults, tokens, undo
/// snapshots) stay exact: nothing truncates them.
pub struct Utf16Text<'a> {
    pub text: &'a mut String,
    pub max_units: usize,
}

impl egui::TextBuffer for Utf16Text<'_> {
    fn is_mutable(&self) -> bool {
        true
    }
    fn as_str(&self) -> &str {
        self.text
    }
    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        let mut free = self
            .max_units
            .saturating_sub(self.text.encode_utf16().count());
        let end = text
            .char_indices()
            .find(|&(_, c)| {
                let over = c.len_utf16() > free;
                free = free.saturating_sub(c.len_utf16());
                over
            })
            .map_or(text.len(), |(i, _)| i);
        <String as egui::TextBuffer>::insert_text(self.text, &text[..end], char_index)
    }
    fn delete_char_range(&mut self, char_range: std::ops::Range<egui::text::CharIndex>) {
        <String as egui::TextBuffer>::delete_char_range(self.text, char_range);
    }
    /// Undo and redo restore their exact snapshots.
    fn replace_with(&mut self, text: &str) {
        text.clone_into(self.text);
    }
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Utf16Text<'static>>()
    }
}

/// `input[type=text]` (css/component.css): `--color-menu-background`, 1px `--color-menu-item-text`
/// border, 8.25pt text; egui edits the text.
pub fn text_field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    text: &mut dyn egui::TextBuffer,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    painter.rect_filled(snapped, 0.0, theme.menu_item_text);
    painter.rect_filled(
        snapped.shrink(px::border_width(1.0, ppp)),
        0.0,
        theme.menu_background,
    );
    let font = FontSpec::new(FONT_SIZE_PX);
    let inner = rect.shrink(1.0 + 1.0);
    let top = inner.min.y + (inner.height() - font.resolved_line_height(ppp)) / 2.0;
    ui.put(
        Rect::from_min_max(Pos2::new(inner.min.x, top), inner.max),
        egui::TextEdit::singleline(text)
            .id(ui.scope_id().with(("text_field", id_salt)))
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(egui::FontId::proportional(font.size))
            .text_color(theme.menu_item_text),
    )
}

/// `text_field` over `lines` lines (BB plan BB5, a plugin's multi-line text box): the same
/// frame look, its text scrolling inside the box (the caret kept in view); Enter adds a line, so
/// the dialog's Enter → OK skips it (`take_enter`).
///
/// R178 (an R118 deviation): Enter, Shift+Enter and Ctrl+Enter all insert egui's single LF (one
/// UTF-16 unit, through the buffer's capacity), not Paint.NET's CRLF; egui's caret treats a CR as
/// a stop of its own, so typed breaks stay LF. Existing CRLF text is kept exactly.
pub fn text_area(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    text: &mut dyn egui::TextBuffer,
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    painter.rect_filled(snapped, 0.0, theme.menu_item_text);
    painter.rect_filled(
        snapped.shrink(px::border_width(1.0, ppp)),
        0.0,
        theme.menu_background,
    );
    let inner = rect.shrink(1.0 + 1.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(px::snap_rect(inner, ppp).intersect(ui.clip_rect()));
    let id = ui.scope_id().with(("text_area", id_salt));
    if ui.memory(|m| m.has_focus(id)) {
        // R178: egui's Return shortcut refuses Ctrl; make the focused box's Ctrl+Enter the same
        // line break as Enter (the dialog still sees Ctrl held, so it never submits).
        ui.ctx().input_mut(|i| {
            for e in &mut i.events {
                if let egui::Event::Key {
                    key: egui::Key::Enter,
                    modifiers,
                    ..
                } = e
                    && modifiers.ctrl
                    && !modifiers.mac_cmd
                {
                    modifiers.ctrl = false;
                    modifiers.command = false;
                }
            }
        });
    }
    let response = egui::ScrollArea::vertical()
        .id_salt(id)
        // The child's height bounds the viewport; egui's default minimum (64) would pass it.
        .min_scrolled_height(inner.height())
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.add(
                egui::TextEdit::multiline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .font(egui::FontId::proportional(FONT_SIZE_PX))
                    .text_color(theme.menu_item_text)
                    .desired_width(f32::INFINITY)
                    .min_size(inner.size()),
            )
        })
        .inner;
    take_enter(ui, &response);
    response
}

/// Where `response` is pressed, dragged or clicked. egui's `interact_pointer_pos` is the frame's
/// last pointer position; when one frame brings a click's release and the moves after it (a
/// loaded machine), that lies past the click, so a click reads its release event's position.
pub fn interact_pos(ui: &Ui, response: &Response) -> Option<Pos2> {
    use egui::PointerButton::*;
    let clicked = [Primary, Secondary, Middle, Extra1, Extra2]
        .into_iter()
        .find(|&b| response.clicked_by(b));
    match clicked.and_then(|b| last_release(ui, b)) {
        Some(p) => Some(
            ui.ctx()
                .layer_transform_from_global(response.layer_id)
                .map_or(p, |t| t * p),
        ),
        None => response.interact_pointer_pos(),
    }
}

/// [`interact_pos`] for a document click listener (no `Response`; global coordinates): where the
/// frame's primary click went up, `None` without one.
pub fn primary_click_pos(ui: &Ui) -> Option<Pos2> {
    if !ui.input(|i| i.pointer.primary_clicked()) {
        return None;
    }
    last_release(ui, egui::PointerButton::Primary)
        .or_else(|| ui.input(|i| i.pointer.interact_pos()))
}

/// The press counterpart of [`primary_click_pos`] (global coordinates): where the frame's last
/// press of `button` (any button: `None`) went down, `None` without one.
pub fn press_pos(ctx: &egui::Context, button: Option<egui::PointerButton>) -> Option<Pos2> {
    last_button_event(ctx, true, button)
}

/// The frame's last release of `button` (global coordinates): egui's click is the last one.
fn last_release(ui: &Ui, button: egui::PointerButton) -> Option<Pos2> {
    last_button_event(ui.ctx(), false, Some(button))
}

fn last_button_event(
    ctx: &egui::Context,
    pressed: bool,
    button: Option<egui::PointerButton>,
) -> Option<Pos2> {
    ctx.input(|i| {
        i.events.iter().rev().find_map(|e| match *e {
            egui::Event::PointerButton {
                pos,
                button: b,
                pressed: p,
                ..
            } if p == pressed && button.is_none_or(|x| x == b) => Some(pos),
            _ => None,
        })
    })
}

/// `text_area`'s height for `lines` lines of its text.
pub fn text_area_height(lines: usize, ppp: f32) -> f32 {
    lines as f32 * FontSpec::new(FONT_SIZE_PX).resolved_line_height(ppp) + 2.0 * (1.0 + 1.0)
}

/// `input[type=range]` (css/component.css): a 1px `--color-slider-outline` track with a 1px
/// `--color-slider` shadow, the 10x14 pointed thumb, and `.slider-tick`s at `ticks` percents.
pub fn slider(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f64,
    min: f64,
    max: f64,
    ticks: &[f64],
) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let response = ui.interact(
        px::snap_rect(rect, ppp),
        ui.scope_id().with(("slider", id_salt)),
        Sense::click_and_drag(),
    );
    let (thumb_w, thumb_h) = SLIDER_THUMB;
    let track_x = (rect.min.x + thumb_w / 2.0, rect.max.x - thumb_w / 2.0);
    if let Some(p) = interact_pos(ui, &response)
        && (response.clicked() || response.dragged())
    {
        let t = ((p.x - track_x.0) / (track_x.1 - track_x.0)).clamp(0.0, 1.0) as f64;
        *value = (min + t * (max - min)).round();
    }
    for &tick in ticks {
        let x = rect.min.x + rect.width() * (tick / 100.0) as f32;
        let r = Rect::from_min_size(
            Pos2::new(x - 0.8, rect.min.y + 5.0 - 2.5),
            Vec2::new(1.6, 10.0),
        );
        painter.rect_filled(px::snap_rect(r, ppp), 0.0, theme.slider_tick);
    }
    let cy = rect.center().y;
    // A 1px runnable track is centred in the input before device snapping.
    let track = Rect::from_min_max(
        Pos2::new(rect.min.x, cy - 0.5),
        Pos2::new(rect.max.x, cy + 0.5),
    );
    painter.rect_filled(px::snap_rect(track, ppp), 0.0, theme.slider_outline);
    painter.rect_filled(
        px::snap_rect(track.translate(Vec2::Y), ppp),
        0.0,
        theme.slider,
    );
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0) as f32;
    let tx = track_x.0 + (track_x.1 - track_x.0) * t;
    let thumb = px::snap_rect(
        Rect::from_center_size(Pos2::new(tx, cy), Vec2::new(thumb_w, thumb_h)),
        ppp,
    );
    let hot = response.hovered() || response.dragged();
    let (fill, outline) = if hot {
        (theme.slider_hover2, theme.slider_hover_outline)
    } else {
        (theme.slider, theme.slider_outline)
    };
    // `clip-path: polygon(100% 0, 100% 60%, 50% 100%, 0 60%, 0 0)`.
    let poly = |r: Rect| {
        [
            r.min,
            Pos2::new(r.max.x, r.min.y),
            Pos2::new(r.max.x, r.min.y + r.height() * 0.6),
            Pos2::new(r.center().x, r.max.y),
            Pos2::new(r.min.x, r.min.y + r.height() * 0.6),
        ]
    };
    let bg = theme.window_background;
    let (at, rows) = aa::polygon_coverage(&poly(thumb).map(|q| q * ppp));
    aa::paint_coverage(&painter, at / ppp, &rows, outline, bg);
    let (at, rows) = aa::polygon_coverage(&poly(thumb.shrink(1.0)).map(|q| q * ppp));
    aa::paint_coverage(&painter, at / ppp, &rows, fill, outline);
    response
}

#[cfg(test)]
mod tests {
    use super::html_step;

    /// Known bugs (click positions): a click on a wrapped number input's spin column keeps the
    /// field's focus by where it went up, though its frame also brings a move away.
    #[test]
    fn spin_column_click_keeps_focus_past_a_batched_move() {
        use egui::{Event, PointerButton, Pos2, Rect, Vec2};
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(80.0, 23.0));
        let mut value = 5.0;
        let mut run = |events: Vec<Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                super::number_input(
                    ui,
                    "n",
                    "N",
                    rect,
                    &mut value,
                    0.0,
                    10.0,
                    1.0,
                    true,
                    super::NumberStyle::DIALOG,
                );
            });
            out.textures_delta.clear();
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let click = |run: &mut dyn FnMut(Vec<Event>), at: Pos2| {
            run(vec![Event::PointerMoved(at)]);
            run(vec![button(at, true)]);
            run(vec![button(at, false)]);
        };
        click(&mut run, rect.min + Vec2::new(10.0, 11.0));
        let focused = ctx.memory(|m| m.focused());
        assert!(focused.is_some(), "a click puts the caret in the field");
        let spin = Pos2::new(rect.max.x - 5.0, rect.center().y);
        run(vec![Event::PointerMoved(spin)]);
        run(vec![button(spin, true)]);
        run(vec![
            button(spin, false),
            Event::PointerMoved(Pos2::new(300.0, 300.0)),
        ]);
        run(vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), focused);
    }

    /// BB common.md §3 "Slider": a range that wraps continues from the other end; one that
    /// does not clamps as before.
    #[test]
    fn wrapping_steps_replace_the_overflow_endpoint() {
        use super::number_step;
        assert_eq!(number_step(360.0, 1.0, 0.0, 360.0, 1.0, true), 0.0);
        assert_eq!(number_step(0.0, -1.0, 0.0, 360.0, 1.0, true), 360.0);
        assert_eq!(number_step(-180.0, -1.0, -180.0, 180.0, 1.0, true), 180.0);
        assert_eq!(number_step(359.0, 1.0, 0.0, 360.0, 1.0, true), 360.0);
        assert_eq!(number_step(360.0, 1.0, 0.0, 360.0, 1.0, false), 360.0);
        assert_eq!(number_step(5.0, -1.0, 0.0, 360.0, 1.0, true), 4.0);
    }

    #[test]
    fn html_step_snaps_clamps_and_never_reverses() {
        // On the grid: one step.
        assert_eq!(html_step(5.0, 1.0, 1.0, 10.0, 1.0), 6.0);
        // Off the grid (based at min 1, step 2): snap in the step's direction only.
        assert_eq!(html_step(4.0, 1.0, 1.0, 100.0, 2.0), 5.0);
        assert_eq!(html_step(4.0, -1.0, 1.0, 100.0, 2.0), 3.0);
        // Clamped to the aligned range: max 10 on a grid from 1 by 2 is 9.
        assert_eq!(html_step(9.0, 1.0, 1.0, 10.0, 2.0), 9.0);
        assert_eq!(html_step(1.0, -1.0, 1.0, 10.0, 1.0), 1.0);
        // Above max: stepping up does nothing, down lands on the aligned max.
        assert_eq!(html_step(20.0, 1.0, 1.0, 10.0, 1.0), 20.0);
        assert_eq!(html_step(20.0, -1.0, 1.0, 10.0, 1.0), 10.0);
    }
}
