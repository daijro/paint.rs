//! The tool options' popups: `ToolOptionDropdown`'s `.tool-dropdown-menu` (text, checkmark and
//! shape-grid menus, the font picker) and the size presets menu (`toggleSizeMenu`).
//! Port of: js/src/interface/panel/header/tool/ToolOptionDropdown.js (`open`,
//! `createEntryButton`, `buildShapeGrid`, `buildFontPicker`), ToolMenu.js (`toggleSizeMenu`),
//! css/header.css (`.tool-dropdown-menu*`, `.tool-size-dropdown-menu`, `.tool-shape-grid*`,
//! `.tool-font-dropdown-*`)
//! Reference: paintdotjs @ b4e6fc5
//!
//! The popups live under `<body>`; their 125 % lengths are divided by 1.25 to DIPs.
use egui::{Color32, Id, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::tool::types::{OptionDef, OptionKind};
use serde_json::Value;

use crate::kit::px;
use crate::kit::scroll::{self, Scrollbar};
use crate::kit::shadow::{self, BoxShadow, TexturePixels};
use crate::kit::text::{self, FontSpec};
use crate::kit::tooltip::{self, Path};
use crate::kit::{fonts, glyph, img, inputs, shape_icon, tool_option};
use crate::theme::{self, *};

/// Scroll position and keyboard state of the open popup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PopupState {
    pub scroll: f64,
    /// `scrollIntoView` ran for the selected entry.
    pub scrolled: bool,
    /// The font picker's focused entry (`:focus` outline) and typeahead prefix with the time
    /// (ms) of its last key.
    pub focused: Option<usize>,
    pub typeahead: String,
    pub typeahead_at: f64,
}

/// What the font picker's keys asked for (`onFontTypeahead`).
enum FontKey {
    Close,
    /// `selectEntry` + focus (ArrowUp/ArrowDown).
    Select(usize),
    /// Enter on the focused entry: select and close.
    Pick(usize),
    /// A typeahead match: focus (and scroll to) it.
    Focus,
}

/// `onFontTypeahead`: the picker owns the keyboard while open (Escape, arrows, Enter and every
/// printable key, matched as a case-insensitive prefix of the display names within 1 s).
fn font_keys(ui: &Ui, def: &OptionDef, value: &Value, state: &mut PopupState) -> Option<FontKey> {
    let now = ui.input(|i| i.time) * 1000.0;
    let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
    let mut out = None;
    for e in events {
        match e {
            // Escape, the arrows and Enter come before the modifier check.
            egui::Event::Key {
                key, pressed: true, ..
            } => match key {
                egui::Key::Escape => out = Some(FontKey::Close),
                egui::Key::ArrowDown | egui::Key::ArrowUp => {
                    // `moveFontSelection`: from the focused entry, else the value's.
                    state.typeahead.clear();
                    let from = state.focused.or_else(|| def.index_of(value)).unwrap_or(0);
                    let to = if key == egui::Key::ArrowDown {
                        (from + 1).min(def.entry_count() - 1)
                    } else {
                        from.saturating_sub(1)
                    };
                    state.focused = Some(to);
                    out = Some(FontKey::Select(to));
                }
                egui::Key::Enter => {
                    if let Some(i) = state.focused {
                        out = Some(FontKey::Pick(i));
                    }
                }
                _ => {}
            },
            egui::Event::Text(t) if !(mods.ctrl || mods.alt || mods.mac_cmd) => {
                for c in t.chars() {
                    if now - state.typeahead_at > 1000.0 {
                        state.typeahead.clear();
                    }
                    state.typeahead_at = now;
                    let lower = c.to_lowercase().to_string();
                    let find = |prefix: &str| {
                        (0..def.entry_count())
                            .find(|&i| def.entry(i).1.to_lowercase().starts_with(prefix))
                    };
                    let mut candidate = format!("{}{lower}", state.typeahead);
                    let mut found = find(&candidate);
                    if found.is_none() && !state.typeahead.is_empty() {
                        candidate = lower;
                        found = find(&candidate);
                    }
                    state.typeahead = candidate;
                    if let Some(i) = found {
                        state.focused = Some(i);
                        out = Some(FontKey::Focus);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// What a popup did.
pub enum PopupPick {
    /// An entry was chosen: the new value; the popup closes.
    Value(Value),
    /// A value was selected from the keyboard; the popup stays open.
    Select(Value),
    /// Escape.
    Close,
    /// A click inside the popup that chose nothing (it stays open).
    Inside,
}

/// Whether the open dropdown is the font picker (which owns the keyboard).
pub fn is_font_picker(def: &OptionDef) -> bool {
    kind(def) == Kind::Font
}

/// The `.tool-dropdown-menu` class `open()` picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Checkmarks,
    ShapeGrid,
    Font,
}

fn kind(def: &OptionDef) -> Kind {
    match def.kind {
        OptionKind::ShapeChoice => Kind::ShapeGrid,
        _ if def.font_preview => Kind::Font,
        // `menuCheckmarks: definition.split` (icon choices always split).
        _ if def.split => Kind::Checkmarks,
        _ => Kind::Text,
    }
}

fn spec() -> FontSpec {
    FontSpec::new(TOOL_POPUP_FONT_SIZE)
}

/// A 1-px CSS border: whole device pixels (`px::border_width`).
fn b(ui: &Ui) -> f32 {
    px::border_width(1.0, ui.ctx().pixels_per_point())
}

/// The common box: shadow, border, background; returns the padding box.
fn paint_box(ui: &mut Ui, id: Id, rect: Rect, border: Color32, pixels: TexturePixels) -> Rect {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let (offset, blur, color) = TOOL_POPUP_SHADOW;
    shadow::paint_box_shadow(
        ui,
        id.with("shadow"),
        snapped,
        BoxShadow {
            offset: Vec2::splat(offset),
            blur,
            color,
        },
        pixels,
    );
    // The popup takes the pointer (clicks and drags) from whatever lies below it, and hides it;
    // the opaque box covers the recorded rects below (as `kit::menu` popups do; the translucent
    // shadow over a pdn rect stays pdn).
    ui.interact(snapped, id.with("body"), Sense::click_and_drag());
    text::occlude(ui.ctx(), snapped);
    let painter = ui.painter();
    painter.rect_filled(snapped, 0.0, border);
    let inner = snapped.shrink(b(ui));
    painter.rect_filled(inner, 0.0, theme.pdn_menu_background);
    inner
}

/// `getSelectedEntry` index for the `[active]` marker (exact match only).
fn active(def: &OptionDef, value: &Value) -> Option<usize> {
    def.index_of(value)
}

/// Text cut with "…" to fit `width` (`text-overflow: ellipsis`).
fn ellipsize(s: &str, width: f32, size: f32) -> String {
    if text::css_width(s, size) <= width {
        return s.to_string();
    }
    let mut cut = s.to_string();
    while !cut.is_empty() && text::css_width(&format!("{cut}\u{2026}"), size) > width {
        cut.pop();
    }
    format!("{cut}\u{2026}")
}

/// `ToolOptionDropdown.open()`: the menu at `(round(left), round(bottom + 1))` of the face.
#[allow(clippy::too_many_arguments)]
pub fn dropdown_menu(
    ui: &mut Ui,
    def: &OptionDef,
    value: &Value,
    face: Rect,
    state: &mut PopupState,
    pixels: TexturePixels,
    viewport: Rect,
) -> (Option<PopupPick>, Rect) {
    let at = Pos2::new(face.min.x.round(), (face.max.y + 1.0).round());
    let id = ui.scope_id().with(("tool-dropdown-menu", def.key));
    // `minWidth = max(menuWidth || 0, width)` (DIPs; the JS menu widths divided by 1.25).
    let width = def.width.unwrap_or(TOOL_DROPDOWN_DEFAULT_WIDTH);
    let q = |v: f32| lu(v, ui.ctx().pixels_per_point());
    let menu_width = match def.kind {
        OptionKind::ShapeChoice => Some(q(TOOL_SHAPE_MENU_WIDTH)),
        OptionKind::IconChoice => Some(q(TOOL_CHECK_MIN_WIDTH)),
        _ => def.menu_width,
    };
    let min_width = menu_width.unwrap_or(0.0).max(width);
    match kind(def) {
        Kind::Text => text_menu(ui, id, def, value, at, min_width, pixels),
        Kind::Checkmarks => check_menu(ui, id, def, value, at, min_width, pixels),
        Kind::ShapeGrid => shape_menu(ui, id, def, value, at, pixels, viewport),
        Kind::Font => font_menu(ui, id, def, value, at, state, pixels, viewport),
    }
}

/// A `position: fixed` box's shrink-to-fit content width at `at`: its max-content, limited by
/// the viewport room right of it (less the 1px borders) but not below its min-content, and
/// never below `min_width`.
fn shrink_to_fit(max_content: f32, min_content: f32, min_width: f32, at: Pos2, ui: &Ui) -> f32 {
    let room = ui.ctx().content_rect().max.x - at.x - 2.0;
    max_content.min(room.max(min_content)).max(min_width)
}

/// CSS `white-space: normal` line breaking at spaces, greedy, for `available` px.
fn wrap_lines(label: &str, available: f32) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for word in label.split(' ') {
        match lines.last_mut() {
            Some(line)
                if text::css_width(&format!("{line} {word}"), TOOL_POPUP_FONT_SIZE)
                    <= available =>
            {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

fn entry_click(ui: &Ui, rect: Rect, id: Id) -> (bool, bool) {
    let r = ui.interact(rect, id, Sense::click());
    (r.hovered(), r.clicked())
}

/// `.tool-dropdown-menu-text`: 29 px rows, `[active]`/hover rows outlined.
fn text_menu(
    ui: &mut Ui,
    id: Id,
    def: &OptionDef,
    value: &Value,
    at: Pos2,
    min_width: f32,
    pixels: TexturePixels,
) -> (Option<PopupPick>, Rect) {
    let theme = theme::current(ui.ctx());
    let q = |v: f32| lu(v, ui.ctx().pixels_per_point());
    let (row_h, pad) = (q(TOOL_TEXT_ENTRY.0), q(TOOL_TEXT_ENTRY.1));
    let bw = 1.0;
    let width = |t: &str| 2.0 * (bw + pad) + text::css_width(t, TOOL_POPUP_FONT_SIZE);
    let max_content = def
        .values
        .iter()
        .map(|(_, t, _)| width(t))
        .fold(0.0, f32::max);
    let min_content = def
        .values
        .iter()
        .flat_map(|(_, t, _)| t.split(' '))
        .map(width)
        .fold(0.0, f32::max);
    let content_w = shrink_to_fit(max_content, min_content, min_width, at, ui);
    // `.tool-dropdown-menu-text button` does not set `nowrap`: a label wider than a shrunken
    // popup wraps, and the lines center on the row (`align-items: center`), overflowing it.
    let available = content_w - 2.0 * (bw + pad);
    let rect = Rect::from_min_size(
        at,
        Vec2::new(
            content_w + 2.0 * bw,
            def.values.len() as f32 * row_h + 2.0 * bw,
        ),
    );
    let inner = paint_box(ui, id, rect, theme.menu_item_selected_border, pixels);
    let painter = ui.painter().clone();
    let selected = active(def, value);
    let mut pick = None;
    for (i, (_, label, _)) in def.values.iter().enumerate() {
        let row = Rect::from_min_size(
            inner.min + Vec2::new(0.0, i as f32 * row_h),
            Vec2::new(content_w, row_h),
        );
        let row_px = px::snap_rect(row, painter.pixels_per_point());
        let (hovered, clicked) = entry_click(ui, row_px, id.with(i));
        if hovered || selected == Some(i) {
            // `:hover, [active] { background: transparent; border-color: selected-border }`.
            painter.rect_filled(row_px, 0.0, theme.menu_item_selected_border);
            painter.rect_filled(row_px.shrink(b(ui)), 0.0, theme.pdn_menu_background);
        }
        let lines = wrap_lines(label, available);
        let ppp = ui.ctx().pixels_per_point();
        let line_h = spec().resolved_line_height(ppp);
        let top = row.min.y + bw + (row_h - 2.0 * bw - lines.len() as f32 * line_h) / 2.0;
        for (k, line) in lines.iter().enumerate() {
            text::draw(
                &painter,
                Pos2::new(row.min.x + bw + pad, top + k as f32 * line_h),
                line,
                spec(),
                theme.pdn_text,
            );
        }
        if clicked {
            pick = Some(PopupPick::Value(def.value(i)));
        }
    }
    (pick.or_else(|| clicked_inside(ui, rect)), rect)
}

fn clicked_inside(ui: &Ui, rect: Rect) -> Option<PopupPick> {
    crate::kit::inputs::primary_click_pos(ui)
        .is_some_and(|p| rect.contains(p))
        .then_some(PopupPick::Inside)
}

/// Entry icons that are no picture of their own. Such an entry keeps the check cell when current.
const NO_OWN_ICON: [&str; 2] = ["", "blank_icon.png"];

/// `.tool-dropdown-menu-checkmarks`: a 30 px gutter with a cell per 31 px row. As Paint.NET's split
/// menus do, each entry shows its own icon in the cell, the current one framed as a selected
/// toolbar image ([`tool_option::paint_active_icon`]); a current entry without one shows the
/// checkmark cell.
fn check_menu(
    ui: &mut Ui,
    id: Id,
    def: &OptionDef,
    value: &Value,
    at: Pos2,
    min_width: f32,
    pixels: TexturePixels,
) -> (Option<PopupPick>, Rect) {
    let theme = theme::current(ui.ctx());
    let q = |v: f32| lu(v, ui.ctx().pixels_per_point());
    let (row_h, gap, pl, pr) = TOOL_CHECK_ENTRY;
    let (row_h, gap, pl, pr) = (q(row_h), q(gap), q(pl), q(pr));
    // Wide icons (the dash styles) widen the cell and the gutter.
    let icon_w = super::value_icon_width(def);
    let wider = (icon_w - TOOL_CHECK_ICON).max(0.0);
    let (cell_w, cell_h) = (q(TOOL_CHECK_CELL.0) + wider, q(TOOL_CHECK_CELL.1));
    let gutter_w = TOOL_CHECK_GUTTER + wider;
    let bw = 1.0;
    let max_content = def
        .values
        .iter()
        .map(|(_, t, _)| {
            2.0 * bw + pl + cell_w + gap + text::css_width(t, TOOL_POPUP_FONT_SIZE) + pr
        })
        .fold(0.0, f32::max);
    // `open()`'s inline `min-width` (the select splits' own width: blend 82, rendering 68) beats
    // the class's 139.2.
    let content_w = shrink_to_fit(max_content, 0.0, min_width, at, ui);
    let rect = Rect::from_min_size(
        at,
        Vec2::new(
            content_w + 2.0 * bw,
            def.values.len() as f32 * row_h + 2.0 * bw,
        ),
    );
    let inner = paint_box(ui, id, rect, theme.pdn_menu_border, pixels);
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    // The gutter and its `:before` divider.
    let gutter = Rect::from_min_size(inner.min, Vec2::new(gutter_w, inner.height()));
    painter.rect_filled(px::snap_rect(gutter, ppp), 0.0, theme.pdn_menu_gutter);
    let selected = active(def, value);
    let mut pick = None;
    for (i, (_, label, icon)) in def.values.iter().enumerate() {
        let row = Rect::from_min_size(
            inner.min + Vec2::new(0.0, i as f32 * row_h),
            Vec2::new(content_w, row_h),
        );
        let row_px = px::snap_rect(row, ppp);
        let (hovered, clicked) = entry_click(ui, row_px, id.with(i));
        if hovered {
            painter.rect_filled(row_px, 0.0, theme.menu_item_border);
            painter.rect_filled(row_px.shrink(b(ui)), 0.0, theme.menu_item_hover);
        }
        let cell = Rect::from_min_size(
            Pos2::new(row.min.x + bw + pl, row.min.y + (row_h - cell_h) / 2.0),
            Vec2::new(cell_w, cell_h),
        );
        if let Some(icon) = icon.filter(|n| !NO_OWN_ICON.contains(n)) {
            let size = Vec2::new(icon_w, TOOL_CHECK_ICON);
            let r = px::snap_rect(Rect::from_center_size(cell.center(), size), ppp);
            let fill = if hovered {
                theme.menu_item_hover
            } else {
                theme.pdn_menu_gutter
            };
            if selected == Some(i) {
                tool_option::paint_active_icon(&painter, icon, r, fill);
            } else {
                img::paint_icon(&painter, icon, r, fill, img::Filter::None);
            }
        } else if selected == Some(i) {
            let cell_px = px::snap_rect(cell, ppp);
            painter.rect_filled(cell_px, 0.0, TOOL_CHECK_ACTIVE_BORDER);
            // The cell's `linear-gradient(#246ca5, #1d5c90)` (Skia's dithered gradient), its
            // inset ring over it, then the icon over both.
            let inside = cell_px.shrink(b(ui));
            let rows = inputs::gradient_pixels(inside, TOOL_CHECK_ACTIVE, ppp);
            let (w, h) = (rows.first().map_or(0, Vec::len), rows.len());
            let mut under: Vec<Color32> = rows.into_iter().flatten().collect();
            tool_option::inset_shadow_over(&mut under, w, h, ppp, TOOL_CHECK_ACTIVE_INSET);
            let grid: Vec<Vec<Color32>> = under.chunks(w.max(1)).map(<[_]>::to_vec).collect();
            inputs::paint_pixels(&painter, inside.min, &grid);
            // Paint.NET's menu checkmark is the Icons.ToolStrip.Checked bitmap: rnd(16·s) device
            // px centred in the cell, in place of paintdotjs's "\2713" text.
            let side = glyph::dip_px(TOOL_CHECK_ICON, ppp) as f32 / ppp;
            let icon = px::snap_rect(
                Rect::from_center_size(cell.center(), Vec2::splat(side)),
                ppp,
            );
            let (ox, oy) = (
                ((icon.min.x - inside.min.x) * ppp).round() as usize,
                ((icon.min.y - inside.min.y) * ppp).round() as usize,
            );
            let n = (side * ppp).round() as usize;
            let icon_under: Vec<Color32> = (0..n * n)
                .map(|k| {
                    under
                        .get((oy + k / n) * w + ox + k % n)
                        .copied()
                        .unwrap_or_default()
                })
                .collect();
            img::paint_icon_over(&painter, "tool_strip_checked.png", icon, &icon_under);
        }
        let top = row.min.y + (row_h - spec().resolved_line_height(ppp)) / 2.0;
        text::draw(
            &painter,
            Pos2::new(cell.max.x + gap, top),
            label,
            spec(),
            theme.pdn_text,
        );
        if clicked {
            pick = Some(PopupPick::Value(def.value(i)));
        }
    }
    // `.tool-dropdown-menu-checkmarks:before`: the 1px divider right of the gutter.
    let divider = Rect::from_min_size(
        Pos2::new(inner.min.x + gutter_w, inner.min.y),
        Vec2::new(1.0, inner.height()),
    );
    painter.rect_filled(px::snap_line(divider, ppp), 0.0, theme.pdn_separator);
    (pick.or_else(|| clicked_inside(ui, rect)), rect)
}

/// `buildShapeGrid`: a heading and an 8-column grid per group.
fn shape_menu(
    ui: &mut Ui,
    id: Id,
    def: &OptionDef,
    value: &Value,
    at: Pos2,
    pixels: TexturePixels,
    viewport: Rect,
) -> (Option<PopupPick>, Rect) {
    let theme = theme::current(ui.ctx());
    let q = |v: f32| lu(v, ui.ctx().pixels_per_point());
    let (heading_h, heading_px, heading_font) = TOOL_SHAPE_HEADING;
    let heading_h = q(heading_h);
    let (cell, gap, py, pxx) = TOOL_SHAPE_GRID;
    let (cell, gap, py, pxx) = (q(cell), q(gap), q(py), q(pxx));
    let groups: Vec<(&str, Vec<usize>)> = def
        .groups
        .iter()
        .map(|(label, values)| {
            let idx = values
                .iter()
                .filter_map(|v| def.values.iter().position(|e| e.0 == *v))
                .collect::<Vec<_>>();
            (*label, idx)
        })
        .filter(|(_, idx)| !idx.is_empty())
        .collect();
    let grid_h = |n: usize| {
        let rows = n.div_ceil(TOOL_SHAPE_COLUMNS) as f32;
        2.0 * py + rows * cell + (rows - 1.0) * gap
    };
    let content_h: f32 = groups
        .iter()
        .map(|(_, idx)| heading_h + grid_h(idx.len()))
        .sum();
    let max_h = viewport.height() - q(TOOL_SHAPE_MENU_MAX_INSET);
    let h = (content_h + 2.0).min(max_h);
    let rect = Rect::from_min_size(at, Vec2::new(q(TOOL_SHAPE_MENU_WIDTH), h));
    let inner = paint_box(ui, id, rect, theme.pdn_menu_border, pixels);
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let selected = active(def, value);
    let mut pick = None;
    let mut y = inner.min.y;
    for (label, idx) in &groups {
        let heading = Rect::from_min_size(
            Pos2::new(inner.min.x, y),
            Vec2::new(inner.width(), heading_h),
        );
        painter.rect_filled(px::snap_rect(heading, ppp), 0.0, theme.pdn_menu_gutter);
        let font = FontSpec::new(heading_font).weight(600);
        let top = heading.min.y + (heading_h - font.resolved_line_height(ppp)) / 2.0;
        text::draw(
            &painter,
            Pos2::new(heading.min.x + heading_px, top),
            label,
            font,
            theme.pdn_text,
        );
        y += heading_h + py;
        for (k, &i) in idx.iter().enumerate() {
            let (c, r) = (k % TOOL_SHAPE_COLUMNS, k / TOOL_SHAPE_COLUMNS);
            let button = Rect::from_min_size(
                Pos2::new(
                    inner.min.x + pxx + c as f32 * (cell + gap),
                    y + r as f32 * (cell + gap),
                ),
                Vec2::splat(cell),
            );
            let button_px = px::snap_rect(button, ppp);
            let cell = ui.interact(button_px, id.with(i), Sense::click());
            // A shape button: the shape's display name (tooltips.md §3.5).
            let value = def.values[i].0;
            tooltip::item(&cell, Path::Pdn, true, || shape_name(value));
            let (hovered, clicked) = (cell.hovered(), cell.clicked());
            let mut fill = theme.pdn_menu_background;
            if hovered || selected == Some(i) {
                fill = px::over(fill, theme.menu_item_selected);
                painter.rect_filled(button_px, 0.0, theme.menu_item_selected_border);
                let inside = button_px.shrink(b(ui));
                painter.rect_filled(inside, 0.0, fill);
                // `inset 0 0 0 1px` at zoom 1: one whole ring.
                let ring = px::over(fill, TOOL_SHAPE_INSET);
                for side in ring_sides(inside, b(ui)) {
                    painter.rect_filled(side, 0.0, ring);
                }
            }
            let image = Rect::from_center_size(button.center(), Vec2::splat(TOOL_SHAPE_IMAGE));
            shape_icon::paint(&painter, def.values[i].0, image, fill);
            if clicked {
                pick = Some(PopupPick::Value(def.value(i)));
            }
        }
        y += grid_h(idx.len()) - py;
    }
    (pick.or_else(|| clicked_inside(ui, rect)), rect)
}

/// Paint.NET's DisplayName of shape `value` (`<Name>Shape.Name`; paintdotjs's ids name three of
/// them differently).
fn shape_name(value: &str) -> String {
    let key = match value {
        "triangle" => "isoscelesTriangle".to_string(),
        "roundedCallout" => "roundedRectangularCallout".to_string(),
        v if v.starts_with("star") => format!("{v}Point"),
        v => v.to_string(),
    };
    pdn_core::i18n::t(&format!("{key}Shape.name"))
}

fn ring_sides(r: Rect, w: f32) -> [Rect; 4] {
    [
        Rect::from_min_max(r.min, Pos2::new(r.max.x, r.min.y + w)),
        Rect::from_min_max(Pos2::new(r.min.x, r.max.y - w), r.max),
        Rect::from_min_max(
            Pos2::new(r.min.x, r.min.y + w),
            Pos2::new(r.min.x + w, r.max.y - w),
        ),
        Rect::from_min_max(
            Pos2::new(r.max.x - w, r.min.y + w),
            Pos2::new(r.max.x, r.max.y - w),
        ),
    ]
}

/// A vertical scroll area: the visible rows' range and the scrollbar.
fn scroll_area(
    ui: &mut Ui,
    id: Id,
    view: Rect,
    content_h: f64,
    state: &mut PopupState,
    colors: Option<(Color32, Color32)>,
) {
    let bar = Rect::from_min_max(
        Pos2::new(view.max.x, view.min.y),
        Pos2::new(view.max.x + scroll::THICKNESS, view.max.y),
    );
    // The wheel scrolls the popup under the pointer.
    let hover = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| view.union(bar).contains(p));
    if hover {
        let dy = ui.input(|i| i.smooth_scroll_delta.y) as f64;
        state.scroll -= dy;
    }
    let mut sb = Scrollbar::vertical(bar).id_salt(id);
    if let Some((track, thumb)) = colors {
        sb = sb.colors(track, thumb);
    }
    sb.show(ui, content_h, view.height() as f64, &mut state.scroll);
    state.scroll = state
        .scroll
        .clamp(0.0, (content_h - view.height() as f64).max(0.0));
}

/// `toggleSizeMenu`: the presets (the current value first when it is not one) in a scrolled
/// list. `combo` is the size combo's border box (layout px).
pub fn size_menu(
    ui: &mut Ui,
    def: &OptionDef,
    value: f64,
    combo: Rect,
    state: &mut PopupState,
    pixels: TexturePixels,
    viewport: Rect,
) -> (Option<PopupPick>, Rect) {
    let theme = theme::current(ui.ctx());
    let id = ui.scope_id().with(("tool-size-menu", def.key));
    let q = |v: f32| lu(v, ui.ctx().pixels_per_point());
    let (row_h, pad, line_h) = TOOL_SIZE_ENTRY;
    let (row_h, line_h) = (q(row_h), q(line_h));
    let mut values: Vec<f64> = def.presets.to_vec();
    if !values.contains(&value) {
        values.insert(0, value);
    }
    let bw = 1.0;
    let (max_px, inset) = TOOL_SIZE_MENU_MAX;
    let max_h = max_px.min(viewport.height() - inset);
    let content_h = values.len() as f32 * row_h;
    let scrolls = content_h > max_h;
    let bar_w = if scrolls { scroll::THICKNESS } else { 0.0 };
    let min_w = (combo.width() * TOOL_SIZE_MENU_WIDTH_FACTOR).round();
    let rows_w = values
        .iter()
        .map(|v| {
            2.0 * (bw + pad)
                + text::css_width(&pdn_core::js::number_to_string(*v), TOOL_POPUP_FONT_SIZE)
        })
        .fold(0.0, f32::max);
    let content_w = (min_w - bar_w).max(rows_w) + bar_w;
    let h = content_h.min(max_h);
    // Below the combo when `min(scrollHeight, 220)` fits, else above it.
    let below = viewport.max.y - combo.max.y - 2.0;
    let top = if below >= content_h.min(TOOL_SIZE_MENU_DROP_MIN) {
        (combo.max.y + 1.0).round()
    } else {
        (combo.min.y - 1.0).round() - h - 2.0 * bw
    };
    let rect = Rect::from_min_size(
        Pos2::new(combo.min.x.round(), top),
        Vec2::new(content_w + 2.0 * bw, h + 2.0 * bw),
    );
    let inner = paint_box(ui, id, rect, theme.menu_item_selected_border, pixels);
    let view = Rect::from_min_max(inner.min, Pos2::new(inner.max.x - bar_w, inner.max.y));
    let selected = values.iter().position(|v| *v == value);
    if !state.scrolled {
        // `selectedButton.scrollIntoView({block: "nearest"})`.
        state.scrolled = true;
        if let Some(i) = selected {
            let bottom = (i + 1) as f32 * row_h;
            if bottom > view.height() {
                state.scroll = (bottom - view.height()) as f64;
            }
        }
    }
    if scrolls {
        scroll_area(
            ui,
            id,
            view,
            content_h as f64,
            state,
            Some((theme.pdn_scrollbar_track, theme.pdn_scrollbar_thumb)),
        );
    }
    let painter = ui
        .painter()
        .with_clip_rect(px::snap_rect(view, ui.ctx().pixels_per_point()));
    let mut pick = None;
    for (i, v) in values.iter().enumerate() {
        let y = view.min.y + i as f32 * row_h - state.scroll as f32;
        if y + row_h < view.min.y || y > view.max.y {
            continue;
        }
        let row = Rect::from_min_size(Pos2::new(view.min.x, y), Vec2::new(view.width(), row_h));
        let row_px = px::snap_rect(row, painter.pixels_per_point());
        let hit = row_px.intersect(view);
        let (hovered, clicked) = entry_click(ui, hit, id.with(i));
        if hovered || selected == Some(i) {
            painter.rect_filled(row_px, 0.0, theme.menu_item_selected_border);
            painter.rect_filled(row_px.shrink(b(ui)), 0.0, theme.pdn_menu_background);
        }
        // `align-items: center`: the line box centres in the content box (it overflows it a little
        // at the patched 19.2/17.6 px).
        let spec = spec().line_height(line_h);
        text::draw(
            &painter,
            Pos2::new(row.min.x + bw + pad, row.min.y + (row_h - line_h) / 2.0),
            &pdn_core::js::number_to_string(*v),
            spec,
            theme.pdn_text,
        );
        if clicked {
            pick = Some(PopupPick::Value(Value::from(*v as i64)));
        }
    }
    (pick.or_else(|| clicked_inside(ui, rect)), rect)
}

/// `buildFontPicker`: family names and a "The quick brown fox" preview in each face.
#[allow(clippy::too_many_arguments)]
fn font_menu(
    ui: &mut Ui,
    id: Id,
    def: &OptionDef,
    value: &Value,
    at: Pos2,
    state: &mut PopupState,
    pixels: TexturePixels,
    viewport: Rect,
) -> (Option<PopupPick>, Rect) {
    let theme = theme::current(ui.ctx());
    let key = font_keys(ui, def, value, state);
    let q = |v: f32| lu(v, ui.ctx().pixels_per_point());
    let (width, vw_inset, max_px, vh_inset) = TOOL_FONT_MENU;
    let vw_inset = q(vw_inset);
    let (label_max, gap, preview_size) = TOOL_FONT_COLUMNS;
    let gap = q(gap);
    let (row_h, pad) = (q(TOOL_TEXT_ENTRY.0), q(TOOL_TEXT_ENTRY.1));
    let bw = 1.0;
    let content_w = width.min(viewport.width() - vw_inset);
    let content_h = def.entry_count() as f32 * row_h;
    let max_h = max_px.min(viewport.height() - vh_inset);
    let h = content_h.min(max_h);
    let scrolls = content_h > max_h;
    let bar_w = if scrolls { scroll::THICKNESS } else { 0.0 };
    let rect = Rect::from_min_size(at, Vec2::new(content_w + 2.0 * bw, h + 2.0 * bw));
    let inner = paint_box(ui, id, rect, theme.menu_item_selected_border, pixels);
    let view = Rect::from_min_max(inner.min, Pos2::new(inner.max.x - bar_w, inner.max.y));
    // `buildFontPicker`: `pendingFontJumpValue = value`, focused once its entry exists.
    let opened = !std::mem::replace(&mut state.scrolled, true);
    if opened {
        state.focused = def.index_of(value);
    }
    // `button.scrollIntoView({block: "nearest"})` for the focused entry.
    if let Some(i) = state.focused.filter(|_| key.is_some() || opened) {
        let (top, bottom) = (i as f64 * row_h as f64, (i + 1) as f64 * row_h as f64);
        if top < state.scroll {
            state.scroll = top;
        } else if bottom > state.scroll + view.height() as f64 {
            state.scroll = bottom - view.height() as f64;
        }
    }
    if scrolls {
        scroll_area(ui, id, view, content_h as f64, state, None);
    }
    let painter = ui
        .painter()
        .with_clip_rect(px::snap_rect(view, ui.ctx().pixels_per_point()));
    // `.tool-dropdown-menu-text` rows; the grid's columns inside the padding.
    let label_w = label_max;
    let preview_w = view.width() - 2.0 * (bw + pad) - label_w - gap;
    let selected = active(def, value);
    let mut pick = None;
    for i in 0..def.entry_count() {
        let (family, label, _) = def.entry(i);
        let y = view.min.y + i as f32 * row_h - state.scroll as f32;
        if y + row_h < view.min.y || y > view.max.y {
            continue;
        }
        if fonts::face_bytes(family).is_none() {
            crate::platform::fonts::request_preview(ui.ctx(), family);
        }
        let row = Rect::from_min_size(Pos2::new(view.min.x, y), Vec2::new(view.width(), row_h));
        let row_px = px::snap_rect(row, painter.pixels_per_point());
        let (hovered, clicked) = entry_click(ui, row_px.intersect(view), id.with(i));
        if hovered || selected == Some(i) || state.focused == Some(i) {
            painter.rect_filled(row_px, 0.0, theme.menu_item_selected_border);
            painter.rect_filled(row_px.shrink(b(ui)), 0.0, theme.pdn_menu_background);
        }
        let x0 = row.min.x + bw + pad;
        let ppp = ui.ctx().pixels_per_point();
        let top = row.min.y + bw + (row_h - 2.0 * bw - spec().resolved_line_height(ppp)) / 2.0;
        let name = ellipsize(label, label_w, TOOL_POPUP_FONT_SIZE);
        let clip = painter.with_clip_rect(
            Rect::from_min_size(Pos2::new(x0, row.min.y), Vec2::new(label_w, row_h))
                .intersect(painter.clip_rect()),
        );
        text::draw(&clip, Pos2::new(x0, top), &name, spec(), theme.pdn_text);
        let preview = Rect::from_min_size(
            Pos2::new(x0 + label_w + gap, row.min.y + (row_h - preview_size) / 2.0),
            Vec2::new(preview_w, preview_size),
        );
        fonts::draw_preview(
            &painter,
            preview,
            family,
            TOOL_FONT_PREVIEW_TEXT,
            theme.pdn_text,
        );
        if clicked {
            pick = Some(PopupPick::Value(def.value(i)));
        }
    }
    let keyed = match key {
        Some(FontKey::Close) => Some(PopupPick::Close),
        Some(FontKey::Select(i)) => Some(PopupPick::Select(def.value(i))),
        Some(FontKey::Pick(i)) => Some(PopupPick::Value(def.value(i))),
        Some(FontKey::Focus) | None => None,
    };
    (pick.or(keyed).or_else(|| clicked_inside(ui, rect)), rect)
}

#[cfg(test)]
mod tests {
    /// tooltips.md §3.5 "Shape grid cells": every shape tips with Paint.NET's name.
    #[test]
    fn shape_cells_tip_with_paint_nets_names() {
        use pdn_core::tool::types::{ToolSettings, ToolType, option_definitions};
        let defs = option_definitions(ToolType::Shapes, &ToolSettings::default());
        let shape = defs
            .iter()
            .find(|d| d.key == "shape")
            .expect("the shape option");
        assert_eq!(shape.values.len(), 29);
        for (value, ..) in &shape.values {
            let name = super::shape_name(value);
            assert!(!name.ends_with("Shape.name"), "{value}: {name}");
        }
        assert_eq!(super::shape_name("star5"), "Five-point star");
        assert_eq!(super::shape_name("blockArrow"), "Arrow");
        assert_eq!(super::shape_name("triangle"), "Triangle");
        assert_eq!(
            super::shape_name("roundedCallout"),
            "Rounded rectangular callout"
        );
    }
}
