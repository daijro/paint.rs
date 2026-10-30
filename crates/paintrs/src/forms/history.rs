//! The History form: the active document's undo mementos, then its redo mementos (`.redo`), and
//! the Undo/Redo strip.
//! Port of: js/src/interface/window/form/HistoryForm.js, form/history/{HistoryMementoItem,
//! ItemType}.js, css/window.css (`#historyForm`, "Consistent form density")
//! Reference: paintdotjs @ b4e6fc5
//!
//! Paint.NET's sizes (rule 4: row 20, icon 16, strip icons 16; rule 5: 12-DIP text) in
//! paintdotjs's look: the `#5D5D5D` redo band with upright black text and the
//! `rgba(0,120,215,.4)` + `#0078D7` selection (`ui-drawn-graphics.md §4`, `ui-text-and-glyphs.md
//! §4` ruling flags).
use egui::{Color32, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::app::AppWorkspace;

use crate::kit::button::{self, IconItem, MenuItemStyle};
use crate::kit::img::{self, Filter};
use crate::kit::px;
use crate::kit::scroll::{ScrollList, Scrollbar, THICKNESS};
use crate::kit::text::{self, FontSpec};
use crate::kit::tooltip::{self, Path};
use crate::theme::{self, *};

/// Rows a wheel notch scrolls (`HistoryControl`: Windows' default `MouseWheelScrollLines`).
const WHEEL_ROWS: f32 = 3.0;

#[derive(Default)]
pub struct HistoryForm {
    list: ScrollList,
    /// The (document, selected memento) last scrolled to (`postInitialize` → `scrollToSelected`).
    shown: Option<(u64, u64)>,
}

/// A row: memento id, name, icon file, redo.
struct Row {
    id: u64,
    name: String,
    icon: String,
    redo: bool,
}

/// The strip panel under a form's list (`#historyStripPanel`, `#layerStripPanel`): its rect at the
/// bottom of `content` and the list above it.
pub fn split_strip(content: Rect) -> (Rect, Rect) {
    let h =
        FORM_STRIP_ICON + 2.0 * (ICON_ITEM_PADDING + MENU_ITEM_BORDER + FORM_STRIP_PADDING) + 1.0;
    let strip = Rect::from_min_max(Pos2::new(content.min.x, content.max.y - h), content.max);
    (
        Rect::from_min_max(content.min, Pos2::new(content.max.x, strip.min.y)),
        strip,
    )
}

/// Paints the strip panel and its icon items (`(action id, enabled, tooltip key)`, PDN-path tips);
/// returns the action of an enabled item clicked.
pub fn strip(
    ui: &mut Ui,
    rect: Rect,
    items: &[(&'static str, bool, Option<&'static str>)],
) -> Option<&'static str> {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter().clone();
    let snapped = px::snap_rect(rect, ppp);
    let bw = px::border_width(1.0, ppp);
    painter.rect_filled(snapped, 0.0, theme.title_bar_border);
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(snapped.min.x, snapped.min.y + bw), snapped.max),
        0.0,
        theme.menu_background,
    );
    let size = FORM_STRIP_ICON + 2.0 * (ICON_ITEM_PADDING + MENU_ITEM_BORDER);
    let mut clicked = None;
    for (i, &(id, enabled, tip)) in items.iter().enumerate() {
        let at = Pos2::new(
            rect.min.x + FORM_STRIP_PADDING + i as f32 * size,
            rect.min.y + 1.0 + FORM_STRIP_PADDING,
        );
        let icon = crate::assets::action_icon_name(id);
        let item = IconItem {
            id,
            rect: Rect::from_min_size(at, Vec2::splat(size)),
            icon: &icon,
            icon_size: FORM_STRIP_ICON,
            enabled,
            active: false,
            open: false,
        };
        let response = button::icon_item(ui, &item, theme.menu_background);
        if let Some(key) = tip {
            tooltip::item(&response, Path::Pdn, enabled, || pdn_core::i18n::t(key));
        }
        if response.clicked() && enabled {
            clicked = Some(id);
        }
    }
    clicked
}

/// A vertical list's item area and its scrollbar (`overflow-y: auto`): the bar shows when the
/// `content` length overflows `rect`.
pub fn list_area(ui: &mut Ui, list: &mut ScrollList, rect: Rect, content: f32, salt: &str) -> Rect {
    list.clamp(content, rect.height());
    if content <= rect.height() {
        return rect;
    }
    let bar = Rect::from_min_max(Pos2::new(rect.max.x - THICKNESS, rect.min.y), rect.max);
    let mut offset = list.scroll as f64;
    Scrollbar::vertical(bar).id_salt(salt).show(
        ui,
        content as f64,
        rect.height() as f64,
        &mut offset,
    );
    list.scroll = offset as f32;
    Rect::from_min_max(rect.min, Pos2::new(bar.min.x, rect.max.y))
}

impl HistoryForm {
    /// Shows the form in `ui`'s rect at time `now` (s). Returns the action of a strip button.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        now: f64,
        mode: pdn_core::render::display::RenderMode,
    ) -> Option<&'static str> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let (list_rect, strip_rect) = split_strip(ui.max_rect());
        let ws = app.active_ws();
        let rows: Vec<Row> = ws.map_or_else(Vec::new, |ws| {
            let h = &ws.history;
            let row = |m: &pdn_core::history::HistoryMemento, redo| Row {
                id: m.id,
                name: m.name.clone(),
                icon: m.image.rsplit('/').next().unwrap_or_default().to_string(),
                redo,
            };
            h.undo
                .iter()
                .map(|m| row(m, false))
                .chain(h.redo.iter().map(|m| row(m, true)))
                .collect()
        });
        let selected = ws.and_then(|ws| ws.history.undo.len().checked_sub(1));
        let enabled = ws.map_or((false, false), |ws| {
            (ws.history.undo.len() > 1, !ws.history.redo.is_empty())
        });
        // A live session takes Undo and Redo while it shares the image (`crate::live`).
        let live = |redo| pdn_core::live::takes_undo(app, redo, false);
        let enabled = (enabled.0 || live(false), enabled.1 || live(true));
        let doc = ws.map(|ws| ws.document.id);

        // Paint.NET's rnd(20·s)-px rows (`ui-text-and-glyphs.md §2.1` "History items", §2.3;
        // R37).
        let row = crate::kit::glyph::dip_pt(HISTORY_ROW, ppp);
        let content = rows.len() as f32 * row;
        let area = list_area(ui, &mut self.list, list_rect, content, "history-list");
        // postInitialize → scrollToSelected (centred unless wholly in view).
        let shown = doc.zip(selected.map(|i| rows[i].id));
        if shown != self.shown {
            self.shown = shown;
            if let Some(i) = selected {
                self.list.scroll_to(i, row, row, area.height());
                self.list.clamp(content, area.height());
            }
        }
        self.list
            .wheel_notches(ui, area, content, WHEEL_ROWS * row, "history-list");
        let keys: Vec<u64> = rows.iter().map(|r| r.id).collect();
        let slots = self.list.layout(ui, area, &keys, row, now);
        let painter = ui.painter().with_clip_rect(area.intersect(ui.clip_rect()));
        let spec = FontSpec::new(FORM_FONT_SIZE);
        let mut pick = None;
        for (i, (row, slot)) in rows.iter().zip(&slots).enumerate() {
            if !slot.intersects(area) {
                continue;
            }
            let snapped = px::snap_rect(*slot, ppp);
            let response = ui.interact(
                snapped.intersect(area),
                ui.scope_id().with(("history", row.id)),
                Sense::click(),
            );
            let hovered = response.hovered();
            let bg = theme.window_background;
            // `.selected-item` (!important) over `.redo:hover` over `.clickable:hover`.
            let (fill, text_color) = if selected == Some(i) {
                let s = MenuItemStyle::Active;
                (
                    button::paint_display_item_frame(&painter, snapped, s, &theme, bg, mode),
                    theme.menu_item_text,
                )
            } else if row.redo && hovered {
                paint_ringed(
                    &painter,
                    snapped,
                    theme.menu_item_selected_border,
                    &theme,
                    theme.item_redo_hover_background,
                );
                (theme.item_redo_hover_background, theme.item_redo_hover_text)
            } else if row.redo {
                painter.rect_filled(snapped, 0.0, theme.item_redo_background);
                (theme.item_redo_background, theme.item_redo_text)
            } else {
                let s = MenuItemStyle::of(hovered, false, false, true);
                (
                    button::paint_display_item_frame(&painter, snapped, s, &theme, bg, mode),
                    theme.menu_item_text,
                )
            };
            // `.icon` (16, centred in the 14-px content box) and the name 5 px after it.
            let inner = slot.shrink(MENU_ITEM_BORDER + SCROLL_ITEM_PADDING);
            let icon = Rect::from_min_size(
                Pos2::new(inner.min.x, inner.center().y - HISTORY_ICON / 2.0),
                Vec2::splat(HISTORY_ICON),
            );
            if let Some(shape) = row.icon.strip_prefix("shape:") {
                crate::kit::shape_icon::paint(&painter, shape, px::snap_rect(icon, ppp), fill);
            } else {
                img::paint_icon(
                    &painter,
                    &row.icon,
                    px::snap_rect(icon, ppp),
                    fill,
                    Filter::None,
                );
            }
            let line = spec.resolved_line_height(ppp);
            let at = Pos2::new(icon.max.x + HISTORY_ICON_GAP, inner.center().y - line / 2.0);
            text::draw(&painter, at, &row.name, spec, text_color);
            if selected == Some(i) {
                text::record_pdn_rect_over(ui.ctx(), snapped.intersect(area));
            }
            // ScrollList.setSelected: another item steps the history there.
            if response.clicked() && selected != Some(i) {
                pick = Some(row.id);
            }
        }
        self.list.cover(ui, list_rect, "history-list");
        if let Some(id) = pick {
            app.step_to(id);
        }
        let items = [
            (
                "menu.edit.undo",
                enabled.0,
                Some("historyForm.undoButton.toolTipText"),
            ),
            (
                "menu.edit.redo",
                enabled.1,
                Some("historyForm.redoButton.toolTipText"),
            ),
        ];
        strip(ui, strip_rect, &items)
    }
}

/// A `.clickable:hover` frame with its own border colour and fill: border, the 1-px
/// `--color-menu-background` ring inside it, the fill.
fn paint_ringed(
    painter: &egui::Painter,
    rect: Rect,
    border: Color32,
    theme: &Theme,
    fill: Color32,
) {
    let ppp = painter.pixels_per_point();
    let bw = px::border_width(MENU_ITEM_BORDER, ppp);
    let ring = px::border_width(MENU_ITEM_RING, ppp);
    painter.rect_filled(rect, 0.0, border);
    painter.rect_filled(rect.shrink(bw), 0.0, theme.menu_background);
    painter.rect_filled(rect.shrink(bw + ring), 0.0, fill);
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The record's shape marker must reach the actual glyph painter, not the asset fallback.
    #[test]
    fn shape_history_icon_reaches_the_painter() {
        use pdn_core::{history::HistoryMemento, tool::ToolType};
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(100, 100, 96.0, true);
        app.active_ws_mut()
            .unwrap()
            .push_memento(HistoryMemento::null("Ellipse Shape", "shape:ellipse"));
        app.active_ws_mut()
            .unwrap()
            .push_memento(HistoryMemento::null(
                "Finish Ellipse",
                &ToolType::Shapes.icon_src(),
            ));
        let ctx = egui::Context::default();
        let mut defs = egui::FontDefinitions::default();
        crate::kit::fonts::install(&mut defs);
        ctx.set_fonts(defs);
        let mut form = HistoryForm::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            form.show(
                ui,
                &mut app,
                0.0,
                pdn_core::render::display::RenderMode::Hardware,
            );
        });
        output.textures_delta.clear();
        let textures = ctx.tex_manager();
        let textures = textures.read();
        assert!(textures.allocated().any(|(_, m)| m.name == "shape/ellipse"));
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().image,
            ToolType::Shapes.icon_src()
        );
    }
}
