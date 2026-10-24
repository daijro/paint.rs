use super::{EffectConfigDialog, initial_editor, merge, num, set};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::text::{self, FontSpec};
use crate::kit::{aa, img, px};
use crate::theme::{self, *};
use egui::{Color32, ColorImage, Id, Key, PointerButton, Pos2, Rect, Response, Sense, Ui, Vec2};
use pdn_core::canvas::Canvas;
use pdn_core::effect::{Control, Provenance, Values, provenance_of};
use serde_json::json;

/// The Extra plus's gap after the dialog title text, DIP (S§4.1).
const TITLE_PLUS_GAP: f32 = 4.0;

/// The Extra plus in the title bar of the dialog whose border box is `frame`: `kit::window`'s
/// title layout (with its icon), then `TITLE_PLUS_GAP` after the text, vertically centred.
// ponytail: past an ellipsized title the plus would overlap the close button; effect names are
// short. Clamp to the title box if a long one appears.
pub(in crate::dialogs) fn title_plus_rect(
    painter: &egui::Painter,
    frame: Rect,
    title: &str,
) -> Rect {
    let text_x = frame.min.x
        + DIALOG_BORDER
        + DIALOG_TITLE_PADDING_LEFT
        + DIALOG_TITLE_ICON
        + DIALOG_TITLE_GAP;
    let title_w = text::measure(painter, title, FontSpec::new(DIALOG_FONT_SIZE));
    let (w, h) = PLUGIN_INDICATOR_SIZE;
    let content_h = DIALOG_TITLE_BAR - DIALOG_BORDER;
    Rect::from_min_size(
        Pos2::new(
            text_x + title_w + TITLE_PLUS_GAP,
            frame.min.y + DIALOG_BORDER + (content_h - h) / 2.0,
        ),
        Vec2::new(w, h),
    )
}

/// S§4.1: an Extra effect's or command's (`id`: an effect id or action id) title carries the
/// plus after its text.
pub(in crate::dialogs) fn paint_title_plus(ui: &Ui, id: Option<&str>, frame: Rect, title: &str) {
    if !id
        .is_some_and(|id| provenance_of(id) == Provenance::Extra || pdn_core::action::is_extra(id))
    {
        return;
    }
    let painter = ui.painter();
    let r = px::snap_rect(
        title_plus_rect(painter, frame, title),
        ui.ctx().pixels_per_point(),
    );
    let fill = theme::current(ui.ctx()).title_bar;
    img::paint_icon(painter, "plugin_indicator.png", r, fill, img::Filter::None);
}

/// A range's numbers.
#[derive(Clone, Copy)]
pub(in crate::dialogs) struct RangeSpec {
    pub(in crate::dialogs) min: f64,
    pub(in crate::dialogs) max: f64,
    pub(in crate::dialogs) default: f64,
    pub(in crate::dialogs) step: f64,
    pub(in crate::dialogs) role: pdn_core::effect::slider::RangeRole,
    pub(in crate::dialogs) keyboard: pdn_core::effect::slider::SliderKeys,
}

impl RangeSpec {
    pub(super) fn of(c: &Control, effect: &str) -> Option<(&'static str, &'static str, RangeSpec)> {
        match c {
            Control::Range {
                key,
                label,
                min,
                max,
                default,
                step,
                keyboard,
                ..
            } => Some((
                key,
                label,
                RangeSpec {
                    min: *min,
                    max: *max,
                    default: *default,
                    step: *step,
                    role: pdn_core::effect::slider::range_role(effect, key),
                    keyboard: *keyboard,
                },
            )),
            _ => None,
        }
    }

    /// Actual sliders use native integer coordinates. Angle substitutes retain their old grid.
    fn input(self) -> (f64, f64, f64) {
        if self.role != pdn_core::effect::slider::RangeRole::Slider {
            (self.min, self.max, self.step)
        } else {
            (
                self.keyboard.coordinate(self.min, self.min, self.max) as f64,
                self.keyboard.coordinate(self.max, self.min, self.max) as f64,
                1.0,
            )
        }
    }

    fn input_value(self, v: f64) -> f64 {
        if self.role != pdn_core::effect::slider::RangeRole::Slider {
            v
        } else {
            self.keyboard.coordinate(v, self.min, self.max) as f64
        }
    }

    fn value_of_input(self, r: f64) -> f64 {
        if self.role != pdn_core::effect::slider::RangeRole::Slider {
            r
        } else {
            self.keyboard.value(r as i64, self.min, self.max)
        }
    }

    fn numeric_step(self) -> f64 {
        if self.role == pdn_core::effect::slider::RangeRole::BuiltinAngle {
            1.0
        } else {
            self.step
        }
    }
}

/// Portable nearest-grid sanitizing, based at min and clamped into [min, max].
fn sanitize(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let k = ((v - min) / step).round();
    let max_k = ((max - min) / step + 1e-9).floor();
    min + k.clamp(0.0, max_k) * step
}

// ---- painting helpers ----

pub(super) fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Paints `color` with per-device-pixel `coverage` (rows from `at`, device px) over the pixels
/// `under` gives.
fn paint_coverage_over(
    painter: &egui::Painter,
    at: Pos2,
    rows: &[Vec<u8>],
    color: Color32,
    under: impl Fn(Pos2) -> Color32,
) {
    let ppp = painter.pixels_per_point();
    for (j, row) in rows.iter().enumerate() {
        for (i, &a) in row.iter().enumerate() {
            if a == 0 {
                continue;
            }
            let p = Pos2::new(at.x + i as f32, at.y + j as f32) / ppp;
            let c = aa::blit_aa(color, a, under(p + Vec2::splat(0.5 / ppp)));
            painter.rect_filled(Rect::from_min_size(p, Vec2::splat(1.0 / ppp)), 0.0, c);
        }
    }
}

/// The colour at fraction `t` of a CSS gradient with evenly spaced `stops`.
fn gradient_at(stops: &[u32], t: f32) -> Color32 {
    let n = stops.len() - 1;
    let f = t.clamp(0.0, 1.0) * n as f32;
    let i = (f.floor() as usize).min(n - 1);
    let (a, b) = (rgb(stops[i]), rgb(stops[i + 1]));
    let k = f - i as f32;
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// A range's track look.
#[derive(Clone, Copy)]
pub(in crate::dialogs) enum Track {
    /// component.css standard range, reused by Save Configuration.
    Standard,
    /// `.effect-config-row`: the selected-border colour up to the value, then the slider
    /// background.
    Progress(f32),
    /// `.effect-special-slider-line`: the slider background (or a gradient) with a 1-px
    /// `--pdn-control-border` ring.
    Special(Option<&'static [u32]>),
}

/// `<input type=range>` in the effect dialogs' look at `range` (its border box): the 4-px track
/// centred in it and the 14×23 pointed thumb at `ratio`. Returns the input's response.
pub(in crate::dialogs) fn paint_range(
    ui: &mut Ui,
    id: Id,
    range: Rect,
    ratio: f32,
    track: Track,
) -> Response {
    if matches!(track, Track::Standard) {
        let mut percent = ratio as f64 * 100.0;
        return inputs::slider(ui, id, range, &mut percent, 0.0, 100.0, &[]);
    }
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let response = ui.interact(px::snap_rect(range, ppp), id, Sense::click_and_drag());
    let top = ((range.center().y - EFFECT_TRACK_HEIGHT / 2.0) * ppp).round() / ppp;
    let t = Rect::from_min_max(
        Pos2::new(range.min.x, top),
        Pos2::new(range.max.x, top + EFFECT_TRACK_HEIGHT),
    );
    let t_px = px::snap_rect(t, ppp);
    let bg = theme.window_background;
    let track_color = |x: f32| -> Color32 {
        match track {
            Track::Progress(f) => {
                if x < t.min.x + t.width() * f {
                    theme.menu_item_selected_border
                } else {
                    theme.pdn_slider_background
                }
            }
            Track::Standard => theme.slider,
            Track::Special(None) => theme.pdn_slider_background,
            Track::Special(Some(stops)) => gradient_at(stops, (x - t.min.x) / t.width()),
        }
    };
    if let Track::Special(_) = track {
        painter.rect_filled(t_px.expand(1.0 / ppp), 0.0, theme.pdn_control_border);
    }
    match track {
        Track::Special(Some(_)) => {
            let w = (t_px.width() * ppp).round() as usize;
            for i in 0..w {
                let x0 = t_px.min.x + i as f32 / ppp;
                let c = track_color(x0 + 0.5 / ppp);
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(x0, t_px.min.y),
                        Pos2::new(x0 + 1.0 / ppp, t_px.max.y),
                    ),
                    0.0,
                    c,
                );
            }
        }
        Track::Progress(f) => {
            let split = ((t.min.x + t.width() * f) * ppp).round() / ppp;
            painter.rect_filled(
                Rect::from_min_max(t_px.min, Pos2::new(split.max(t_px.min.x), t_px.max.y)),
                0.0,
                theme.menu_item_selected_border,
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(split.min(t_px.max.x), t_px.min.y), t_px.max),
                0.0,
                theme.pdn_slider_background,
            );
        }
        Track::Special(None) | Track::Standard => {
            painter.rect_filled(t_px, 0.0, theme.pdn_slider_background);
        }
    }
    // The thumb: `top: 50%; translateY(-50%)` on the snapped track.
    let (tw, th, shoulder) = EFFECT_THUMB;
    let left = range.min.x + ratio.clamp(0.0, 1.0) * (range.width() - tw);
    let thumb_top = t_px.min.y + EFFECT_TRACK_HEIGHT / 2.0 - th / 2.0;
    let poly = [
        Pos2::new(left, thumb_top),
        Pos2::new(left + tw, thumb_top),
        Pos2::new(left + tw, thumb_top + th * shoulder),
        Pos2::new(left + tw / 2.0, thumb_top + th),
        Pos2::new(left, thumb_top + th * shoulder),
    ];
    // As Chromium draws it (measured from the references): the box's fill on whole pixels
    // across (from the rounded left edge) but at its fractional top, clipped by the polygon at
    // the fractional left edge and the floored top.
    let dy = (thumb_top * ppp).floor() / ppp - thumb_top;
    let poly = poly.map(|p| p + Vec2::new(0.0, dy));
    let (at, mut rows) = aa::polygon_area_coverage(&poly.map(|p| p * ppp));
    let frac = Rect::from_min_size(
        Pos2::new((left * ppp).round(), thumb_top * ppp),
        Vec2::new(tw, th) * ppp,
    );
    for (j, row) in rows.iter_mut().enumerate() {
        for (i, a) in row.iter_mut().enumerate() {
            let pxr = Rect::from_min_size(at + Vec2::new(i as f32, j as f32), Vec2::splat(1.0));
            let o = pxr.intersect(frac);
            *a *= if o.is_positive() {
                (o.width() * o.height()) as f64
            } else {
                0.0
            };
        }
    }
    let rows: Vec<Vec<u8>> = rows
        .into_iter()
        .map(|r| {
            r.into_iter()
                .map(|a| (a * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8)
                .collect()
        })
        .collect();
    let ring = if let Track::Special(_) = track {
        t_px.expand(1.0 / ppp)
    } else {
        t_px
    };
    paint_coverage_over(&painter, at, &rows, theme.menu_item_selected_border, |p| {
        if t_px.contains(p) {
            track_color(p.x)
        } else if ring.contains(p) {
            theme.pdn_control_border
        } else {
            bg
        }
    });
    response
}

pub(in crate::dialogs) use crate::kit::inputs::button_activated as button_clicked;

/// `.effect-row-reset`: the 24×24 button with `reset_icon.png`.
pub(super) fn reset_button(ui: &mut Ui, id: Id, rect: Rect) -> bool {
    let response = paint_icon_button(ui, id, rect, EFFECT_RESET.1);
    button_clicked(ui, &response)
}

/// A `button`-look box at `rect` with `reset_icon.png` (`icon` px) centred in it. Enter on it
/// clicks it only (`inputs::take_enter`).
pub(in crate::dialogs) fn icon_button(ui: &mut Ui, id: Id, rect: Rect, icon: f32) -> bool {
    paint_icon_button(ui, id, rect, icon).clicked()
}

fn paint_icon_button(ui: &mut Ui, id: Id, rect: Rect, icon: f32) -> Response {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let r = px::snap_rect(rect, ppp);
    let response = ui.interact(r, id, Sense::click());
    inputs::take_enter(ui, &response);
    ui.ctx().accesskit_node_builder(id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label("Reset");
    });
    painter.rect_filled(r, 0.0, theme.button_outline);
    painter.rect_filled(r.shrink(px::border_width(1.0, ppp)), 0.0, theme.button);
    let inset = (rect.width() - icon) / 2.0;
    let at = Rect::from_min_size(rect.min + Vec2::splat(inset), Vec2::splat(icon));
    img::paint_icon(
        &painter,
        "reset_icon.png",
        px::snap_rect(at, ppp),
        theme.button,
        img::Filter::None,
    );
    response
}

/// How a `<select>` draws: Chromium's native dark field, or the author-styled one
/// (`.effect-special-select select`).
#[derive(Clone, Copy)]
pub(in crate::dialogs) struct SelectLook {
    pub(in crate::dialogs) native: bool,
    pub(in crate::dialogs) pad_left: f32,
    pub(in crate::dialogs) font: FontSpec,
}

/// The open `<select>`: its id, the row Enter would pick, and its list (screen rect).
#[derive(Clone, Copy, Debug)]
pub(in crate::dialogs) struct OpenSelect {
    pub(super) id: Id,
    pub(super) highlight: usize,
    scroll: f32,
    pub(in crate::dialogs) list: Rect,
}

/// A `<select>` at `rect` showing `choices[current]` (`label` names it for accessibility); a click
/// opens its list below it in an area over the dialog, whose rows take the pointer (nothing
/// beneath sees the press). Arrow keys move the highlight and Enter picks it while it is open.
/// Returns the picked index.
#[allow(clippy::too_many_arguments)]
pub(in crate::dialogs) fn select(
    ui: &mut Ui,
    open: &mut Option<OpenSelect>,
    id: Id,
    rect: Rect,
    label: &str,
    choices: &[&str],
    current: usize,
    look: SelectLook,
) -> Option<usize> {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let r = px::snap_rect(rect, ppp);
    let response = ui.interact(r, id, Sense::click());
    initial_editor(ui, response.id);
    let shown = choices.get(current).copied().unwrap_or("");
    ui.ctx().accesskit_node_builder(id, |n| {
        n.set_role(egui::accesskit::Role::ComboBox);
        n.set_label(label);
        n.set_value(shown);
    });
    let b = px::border_width(1.0, ppp);
    let (fill, border) = if look.native {
        SELECT_NATIVE
    } else {
        (theme.number_input_background, theme.pdn_control_border)
    };
    painter.rect_filled(r, 0.0, border);
    painter.rect_filled(r.shrink(b), 0.0, fill);
    if look.native {
        // The native field's corner pixels are cut.
        for (x, y) in [
            (r.min.x, r.min.y),
            (r.max.x - b, r.min.y),
            (r.min.x, r.max.y - b),
            (r.max.x - b, r.max.y - b),
        ] {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), Vec2::splat(b)),
                0.0,
                theme.window_background,
            );
        }
    }
    let line = look.font.resolved_line_height(ppp);
    let x = rect.min.x + 1.0 + look.pad_left + SELECT_TEXT_INSET;
    let y = rect.min.y + (rect.height() - line) / 2.0;
    let clip = painter.with_clip_rect(r.shrink(b).intersect(painter.clip_rect()));
    text::draw_aligned(
        &clip,
        Rect::from_min_max(
            Pos2::new(x, y),
            Pos2::new(rect.max.x - SELECT_ARROW_RIGHT, y + line),
        ),
        shown,
        look.font,
        theme.menu_item_text,
        text::Alignment::Start,
    );
    // The arrow: 14 px inside the right border edge, centred.
    let ax = ((rect.max.x * ppp).round() - SELECT_ARROW_RIGHT * ppp).round();
    let ay = (r.min.y * ppp + ((r.height() * ppp - 7.0) / 2.0 + 0.5).floor()).round();
    for (j, row) in SELECT_ARROW.iter().enumerate() {
        for (i, &a) in row.iter().enumerate() {
            if a > 0 {
                let p = Pos2::new(ax + i as f32, ay + j as f32) / ppp;
                painter.rect_filled(
                    Rect::from_min_size(p, Vec2::splat(1.0 / ppp)),
                    0.0,
                    aa::blit_aa(theme.menu_item_text, a, fill),
                );
            }
        }
    }
    // The list: one row per choice under the field (the OS popup's role).
    let row_h = line + 4.0;
    let screen = ui.ctx().content_rect().shrink(SELECT_VIEWPORT_INSET);
    let list_height = (row_h * choices.len() as f32 + 2.0).min(screen.height());
    let list = Rect::from_min_size(
        Pos2::new(
            rect.min.x,
            rect.max.y.min(screen.max.y - list_height).max(screen.min.y),
        ),
        Vec2::new(rect.width(), list_height),
    );
    // Enter on an open focused field belongs to the list, not egui's synthetic field click.
    let field_clicked = response.clicked()
        && !(open.is_some_and(|o| o.id == id) && ui.input(|i| i.key_pressed(Key::Enter)));
    // The click (or Enter on the focused field) that opens the list is not also a key for it.
    let just_opened = field_clicked && open.is_none_or(|o| o.id != id);
    if field_clicked {
        *open = match open {
            Some(o) if o.id == id => None,
            _ => Some(OpenSelect {
                id,
                highlight: current,
                scroll: 0.0,
                list,
            }),
        };
    }
    // Let Tab leave the popup through the dialog's ordinary focus order.
    if open.is_some_and(|o| o.id == id)
        && ui.input(|i| {
            (i.modifiers.is_none() || i.modifiers.shift_only()) && i.key_pressed(Key::Tab)
        })
    {
        *open = None;
    }
    if just_opened && !response.has_focus() {
        response.request_focus();
    }
    let Some(state) = open.as_mut().filter(|o| o.id == id) else {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter::default()));
        return None;
    };
    state.list = list;
    let last = choices.len().saturating_sub(1);
    let mut picked = None;
    let mut moved = just_opened;
    ui.input(|i| {
        if just_opened {
            return;
        }
        moved = [Key::ArrowDown, Key::ArrowUp, Key::Home, Key::End]
            .iter()
            .any(|k| i.key_pressed(*k));
        if i.key_pressed(Key::Home) {
            state.highlight = 0;
        }
        if i.key_pressed(Key::End) {
            state.highlight = last;
        }
        if i.key_pressed(Key::ArrowDown) {
            state.highlight = (state.highlight + 1).min(last);
        }
        if i.key_pressed(Key::ArrowUp) {
            state.highlight = state.highlight.saturating_sub(1);
        }
        if i.key_pressed(Key::Enter) {
            picked = Some(state.highlight);
        }
    });
    let max_scroll = (row_h * choices.len() as f32 + 2.0 - list.height()).max(0.0);
    if moved {
        state.scroll = state
            .scroll
            .min(state.highlight as f32 * row_h)
            .max((state.highlight + 1) as f32 * row_h - list.height() + 2.0);
    }
    if list.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
        state.scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    state.scroll = state.scroll.clamp(0.0, max_scroll);
    let lr = px::snap_rect(list, ppp);
    egui::Area::new(id.with("list"))
        .order(egui::Order::Tooltip)
        .fixed_pos(lr.min)
        .constrain(false)
        .interactable(true)
        .show(ui.ctx(), |aui| {
            // As a native drop-down, the open list takes every press: one outside it only closes
            // it (the whole screen first), its box (border too) and its rows over that.
            aui.allocate_rect(aui.ctx().content_rect(), Sense::click());
            aui.allocate_rect(lr, Sense::click());
            // The opaque list hides what was recorded under it (the canvas past the dialog).
            text::occlude(aui.ctx(), lr);
            let lp = aui.painter().clone();
            lp.rect_filled(lr, 0.0, border);
            lp.rect_filled(lr.shrink(b), 0.0, theme.menu_background);
            aui.set_clip_rect(lr.shrink(b));
            let lp = lp.with_clip_rect(lr.shrink(b));
            for (i, c) in choices.iter().enumerate() {
                let row = Rect::from_min_size(
                    Pos2::new(
                        list.min.x + 1.0,
                        list.min.y + 1.0 + i as f32 * row_h - state.scroll,
                    ),
                    Vec2::new(list.width() - 2.0, row_h),
                );
                if !row.intersects(aui.clip_rect()) {
                    continue;
                }
                let rr = aui.allocate_rect(px::snap_rect(row, ppp), Sense::click());
                aui.ctx().accesskit_node_builder(rr.id, |n| {
                    n.set_role(egui::accesskit::Role::ListBoxOption);
                    n.set_label(*c);
                });
                if rr.hovered() && aui.input(|i| i.pointer.delta() != Vec2::ZERO) {
                    state.highlight = i;
                }
                if i == state.highlight {
                    lp.rect_filled(px::snap_rect(row, ppp), 0.0, theme.menu_item_hover);
                }
                text::draw_aligned(
                    &lp,
                    Rect::from_min_max(
                        Pos2::new(
                            row.min.x + look.pad_left + SELECT_TEXT_INSET,
                            row.min.y + 2.0,
                        ),
                        Pos2::new(row.max.x - SELECT_TEXT_INSET, row.max.y),
                    ),
                    c,
                    look.font,
                    theme.menu_item_text,
                    text::Alignment::Start,
                );
                if rr.clicked() {
                    picked = Some(i);
                }
            }
        });
    // A press outside the list (its field included) closes it and does nothing else.
    let outside = ui.input(|inp| {
        inp.pointer.any_pressed() && inp.pointer.interact_pos().is_some_and(|p| !lr.contains(p))
    });
    if picked.is_some() || outside {
        *open = None;
    }
    let owns_list = open.is_some_and(|o| o.id == id);
    // egui accepts a filter only after focus has survived a pass. Finish pointer opening before
    // the next input so even its first arrow/Escape belongs to the list.
    if owns_list && ui.memory(|m| m.has_focus(id) && !m.had_focus_last_frame(id)) {
        ui.ctx()
            .request_discard("install newly opened Select's focus filter");
    }
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                vertical_arrows: owns_list,
                escape: owns_list,
                ..Default::default()
            },
        );
    });
    picked
}

/// Paints `image` (device pixels) at the snapped `rect`, re-uploading only when it changed.
pub(super) fn paint_canvas(
    ui: &Ui,
    cache: &mut Vec<(Id, u64, egui::TextureHandle)>,
    id: Id,
    rect: Rect,
    image: ColorImage,
) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    image.size.hash(&mut h);
    for p in &image.pixels {
        p.to_array().hash(&mut h);
    }
    let hash = h.finish();
    let ctx = ui.ctx();
    let entry = cache.iter_mut().find(|e| e.0 == id);
    let tex = match entry {
        Some(e) => {
            if e.1 != hash {
                e.2.set(image, egui::TextureOptions::NEAREST);
                e.1 = hash;
            }
            e.2.clone()
        }
        None => {
            let t = ctx.load_texture(
                format!("effect-canvas-{id:?}"),
                image,
                egui::TextureOptions::NEAREST,
            );
            cache.push((id, hash, t.clone()));
            t
        }
    };
    ui.painter().image(
        tex.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// A canvas's pixels shown in a box of `w`×`h` device pixels: the bilinear resample Chromium
/// applies when the CSS box differs from the buffer.
pub(super) fn canvas_image(cx: &Canvas, w: usize, h: usize) -> ColorImage {
    let src = cx.get_image_data(0, 0, cx.width(), cx.height());
    let (sw, sh) = (src.width as usize, src.height as usize);
    if (sw, sh) == (w, h) {
        return ColorImage::from_rgba_unmultiplied([w, h], &src.data);
    }
    let at = |x: usize, y: usize, c: usize| src.data[(y * sw + x) * 4 + c] as f32;
    let mut out = Vec::with_capacity(w * h * 4);
    for j in 0..h {
        let v = ((j as f32 + 0.5) * sh as f32 / h as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, fy) = (v.floor() as usize, v.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for i in 0..w {
            let u = ((i as f32 + 0.5) * sw as f32 / w as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, fx) = (u.floor() as usize, u.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
                let bottom = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
                out.push((top * (1.0 - fy) + bottom * fy).round() as u8);
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &out)
}

/// The device-pixel box of `rect`.
pub(super) fn device_box(rect: Rect, ppp: f32) -> (Rect, usize, usize) {
    let r = px::snap_rect(rect, ppp);
    let (w, h) = (
        (r.width() * ppp).round() as usize,
        (r.height() * ppp).round() as usize,
    );
    (r, w.max(1), h.max(1))
}

/// A range row's line: the slider, the number and the reset button (`None`: hidden, a plugin's
/// "no reset button"). Returns the change kind.
#[allow(clippy::too_many_arguments)]
pub(in crate::dialogs) fn range_line(
    ui: &mut Ui,
    state: &mut SliderState,
    key: &str,
    spec: RangeSpec,
    values: &mut Values,
    range: Rect,
    number: Rect,
    reset: Option<Rect>,
    track: Track,
    style: NumberStyle,
) -> Option<bool> {
    let id = ui.scope_id().with(("range", key));
    let numeric = num(values, key).clamp(spec.min, spec.max);
    let (imin, imax, istep) = spec.input();
    let shown = sanitize(spec.input_value(numeric), imin, imax, istep);
    let ratio = ((shown - imin) / (imax - imin)) as f32;
    let fill = ((numeric - spec.min) / (spec.max - spec.min)) as f32;
    let track = match track {
        Track::Progress(_) => Track::Progress(fill),
        t => t,
    };
    let response = paint_range(ui, id, range, ratio, track);
    let id = response.id;
    // R154: the value and the (possibly session-dynamic) limits the slider edits within.
    ui.ctx().accesskit_node_builder(id, |n| {
        n.set_role(egui::accesskit::Role::Slider);
        n.set_label(key);
        n.set_numeric_value(numeric);
        n.set_min_numeric_value(spec.min);
        n.set_max_numeric_value(spec.max);
    });
    let mut changed = None;
    if response.has_focus() && ui.is_enabled() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        let steps = ui.input_mut(|i| {
            let mut steps = vec![];
            i.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = event
                    && modifiers.is_none()
                    && matches!(
                        key,
                        Key::ArrowRight
                            | Key::ArrowUp
                            | Key::ArrowLeft
                            | Key::ArrowDown
                            | Key::PageUp
                            | Key::PageDown
                    )
                {
                    if *pressed {
                        steps.push(*key);
                    }
                    false
                } else {
                    true
                }
            });
            steps
        });
        let mut value = numeric;
        for key in steps {
            value = spec.keyboard.stepped(
                value,
                spec.min,
                spec.max,
                matches!(key, Key::ArrowRight | Key::ArrowUp | Key::PageUp),
                matches!(key, Key::PageUp | Key::PageDown),
            );
        }
        if value != numeric {
            set(values, key, value);
            changed = Some(false);
        }
    }
    // Refresh keyboard edits before pointer handling, so a numeric blur cannot undo a press.
    let numeric = num(values, key).clamp(spec.min, spec.max);
    let tw = if matches!(track, Track::Standard) {
        SLIDER_THUMB.0
    } else {
        EFFECT_THUMB.0
    };
    let thumb_c = range.min.x + tw / 2.0 + ratio * (range.width() - tw);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let pressed = [PointerButton::Primary, PointerButton::Secondary]
        .into_iter()
        .find(|b| ui.input(|i| i.pointer.button_pressed(*b)));
    if let (Some(button), Some(p)) = (pressed, pointer)
        && response.hovered()
    {
        let on_thumb = (p.x - thumb_c).abs() <= tw / 2.0 + EFFECT_THUMB_RESET.1;
        // `enableSliderThumbReset`: a second press on the thumb within 500 ms resets.
        let previous = state
            .presses
            .iter()
            .position(|(i, b, _)| *i == id && *b == button);
        let last = previous.map(|k| state.presses.remove(k).2);
        if !matches!(track, Track::Standard)
            && on_thumb
            && last.is_some_and(|t| state.now - t <= EFFECT_THUMB_RESET.0)
        {
            set(values, key, spec.default);
            return Some(true);
        }
        if on_thumb {
            state.presses.push((id, button, state.now));
        }
        if button == PointerButton::Primary {
            response.request_focus();
            state.drag = Some((id, if on_thumb { p.x - thumb_c } else { 0.0 }));
        }
    }
    if let (Some((drag_id, offset)), Some(p)) = (state.drag, pointer)
        && drag_id == id
        && ui.input(|i| i.pointer.primary_down())
    {
        let f = ((p.x - offset - range.min.x - tw / 2.0) / (range.width() - tw)).clamp(0.0, 1.0);
        let r = sanitize(imin + f as f64 * (imax - imin), imin, imax, istep);
        let v = spec.value_of_input(r).clamp(spec.min, spec.max);
        if v != numeric {
            set(values, key, v);
            changed = Some(false);
        }
    }
    if state.drag.is_some_and(|d| d.0 == id) && !ui.input(|i| i.pointer.primary_down()) {
        state.drag = None;
        changed = Some(true);
    }
    let mut v = numeric;
    let number_response = inputs::number_input(
        ui,
        ("number", key),
        key,
        number,
        &mut v,
        spec.min,
        spec.max,
        spec.numeric_step(),
        true,
        style,
    );
    initial_editor(ui, number_response.id);
    if v != numeric {
        set(values, key, v);
        changed = merge(changed, Some(false));
    }
    if let Some(reset) = reset
        && reset_button(ui, ui.scope_id().with(("reset", key)), reset)
    {
        set(values, key, spec.default);
        changed = Some(true);
    }
    changed
}

/// A generic range line at `top` across `w` from `x0`: the slider, the number box and the reset
/// button.
pub(in crate::dialogs) fn range_rects(x0: f32, w: f32, top: f32) -> (Rect, Rect, Rect) {
    let (nw, rw, gap) = EFFECT_LINE_COLUMNS;
    let col = w - nw - rw - 2.0 * gap;
    let range = Rect::from_min_size(
        Pos2::new(x0 + EFFECT_RANGE_MARGIN, top),
        Vec2::new(col, EFFECT_LINE_HEIGHT),
    );
    let number = Rect::from_min_size(
        Pos2::new(
            x0 + col + gap,
            top + (EFFECT_LINE_HEIGHT - EFFECT_NUMBER_HEIGHT) / 2.0,
        ),
        Vec2::new(nw, EFFECT_NUMBER_HEIGHT),
    );
    let reset = Rect::from_min_size(
        Pos2::new(number.max.x + gap, top + (EFFECT_LINE_HEIGHT - rw) / 2.0),
        Vec2::splat(rw),
    );
    (range, number, reset)
}

/// `.effect-config-checkbox` at `y`: the box (at `fade` opacity), then `label` in `color`; the
/// row's label area toggles it too. Returns the change kind.
#[allow(clippy::too_many_arguments)]
pub(in crate::dialogs) fn checkbox_line(
    ui: &mut Ui,
    values: &mut Values,
    key: &str,
    label: &str,
    x0: f32,
    y: f32,
    w: f32,
    color: Color32,
    fade: f32,
) -> Option<bool> {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let spec = FontSpec::new(DIALOG_FONT_SIZE);
    let line = spec.resolved_line_height(ppp);
    let (side, gap, min_h) = EFFECT_CHECKBOX;
    let boxed = Rect::from_min_size(Pos2::new(x0, y + (min_h - side) / 2.0), Vec2::splat(side));
    let mut on = pdn_core::effect::flag(values, key);
    let label_rect = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, min_h));
    let before = on;
    let opacity = ui.opacity();
    ui.set_opacity(opacity * fade);
    let response = inputs::checkbox_in(
        ui,
        ("checkbox", key),
        boxed,
        label_rect,
        &mut on,
        true,
        theme.window_background,
    );
    ui.set_opacity(opacity);
    initial_editor(ui, response.id);
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::CheckBox);
        n.set_label(label);
    });
    text::draw(
        ui.painter(),
        Pos2::new(boxed.max.x + gap, y + (min_h - line) / 2.0),
        label,
        spec,
        color,
    );
    (on != before).then(|| {
        values.insert(key.into(), json!(on));
        true
    })
}

/// The slider interaction state shared by a dialog's rows.
#[derive(Default)]
pub(in crate::dialogs) struct SliderState {
    pub(in crate::dialogs) now: f64,
    presses: Vec<(Id, PointerButton, f64)>,
    drag: Option<(Id, f32)>,
}

impl SliderState {
    /// A slider's thumb is held (a drag in progress).
    pub(in crate::dialogs) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

impl EffectConfigDialog {
    pub(super) fn slider_state(&mut self, now: f64) -> SliderState {
        SliderState {
            now,
            presses: std::mem::take(&mut self.thumb_presses),
            drag: self.slider_drag.take(),
        }
    }

    pub(super) fn put_slider_state(&mut self, s: SliderState) {
        self.thumb_presses = s.presses;
        self.slider_drag = s.drag;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_values_snap_to_the_step_grid() {
        assert_eq!(sanitize(2.4, 0.0, 300.0, 1.0), 2.0);
        assert_eq!(sanitize(2.6, 0.0, 300.0, 1.0), 3.0);
        assert_eq!(sanitize(500.0, 0.0, 300.0, 1.0), 300.0);
        // R161 pointer ties use nonnegative distance from min, so they increase the coordinate.
        assert_eq!(sanitize(-0.5, -1.0, 1.0, 1.0), 0.0);
        assert!((sanitize(0.0, -0.99, 2.0, 0.01) - 0.0).abs() < 1e-9);
        let zoom = RangeSpec {
            min: 0.06,
            max: 16.0,
            default: 1.0,
            step: 0.01,
            role: pdn_core::effect::slider::RangeRole::Slider,
            keyboard: pdn_core::effect::slider::SliderKeys {
                decimals: 2,
                exponential: true,
                ..pdn_core::effect::slider::SliderKeys::INTEGER
            },
        };
        assert_eq!(zoom.input(), (60.0, 16000.0, 1.0));
        assert_eq!(zoom.input_value(1.0), 3930.0);
        assert!((zoom.value_of_input(8030.0) - 4.045).abs() < 1e-12);
    }
}
