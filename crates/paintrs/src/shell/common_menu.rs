//! `#commonMenu`: the icon toolbar under the title bar (its `zoom: var(--pdn-ui-scale)` is 1).
//! Port of: js/src/interface/panel/header/common/CommonMenu.js, css/header.css `#commonMenu`,
//! css/menu.css `.icon-item`, `.horizontal-separator`
//! Reference: paintdotjs @ b4e6fc5
use egui::{Pos2, Rect, Ui, Vec2};
use pdn_core::action;
use pdn_core::app::AppWorkspace;

use crate::kit::button::{self, IconItem};
use crate::kit::tooltip::{self, Path};
use crate::theme::{self, *};

/// `CommonMenu` items: action ids (icon = `ActionItem.getIconPath`), `None` = separator.
pub const ITEMS: [Option<&str>; 17] = [
    Some("menu.file.new"),
    Some("menu.file.open"),
    Some("menu.file.save"),
    None,
    Some("menu.file.print"),
    None,
    Some("menu.edit.cut"),
    Some("menu.edit.copy"),
    Some("menu.edit.paste"),
    Some("menu.image.crop"),
    Some("menu.edit.deselect"),
    None,
    Some("menu.edit.undo"),
    Some("menu.edit.redo"),
    None,
    Some("menu.view.grid"),
    Some("menu.view.rulers"),
];

/// The common actions strip's ToolTipText of item `id` (tooltips.md §3.3).
fn tip(id: &str) -> String {
    let key = match id {
        "menu.image.crop" => "commonAction.cropToSelection".into(),
        "menu.view.grid" => "workspaceOptionsConfigWidget.drawGridToggleButton.toolTipText".into(),
        "menu.view.rulers" => "workspaceOptionsConfigWidget.rulersToggleButton.toolTipText".into(),
        _ => format!("commonAction.{}", id.rsplit('.').next().unwrap_or(id)),
    };
    pdn_core::i18n::t(&key)
}

/// `.icon-item` border box.
fn item_size() -> f32 {
    ICON_ITEM_CONTENT + 2.0 * (ICON_ITEM_PADDING + MENU_ITEM_BORDER)
}

fn separator_width() -> f32 {
    SEPARATOR_SIZE.0 + 2.0 * SEPARATOR_MARGIN.0
}

pub fn width() -> f32 {
    ITEMS
        .iter()
        .map(|i| {
            if i.is_some() {
                item_size()
            } else {
                separator_width()
            }
        })
        .sum()
}

/// Items are enabled while their action is executable; the grid and rulers toggles are active
/// while shown (`CommonMenu.updateToggleStates`). Returns the action of a clicked item.
pub fn show(ui: &mut Ui, rect: Rect, app: &AppWorkspace) -> Option<&'static str> {
    let mut clicked = None;
    let theme = theme::current(ui.ctx());
    let mut x = rect.min.x;
    for item in ITEMS {
        match item {
            Some(id) => {
                let s = item_size();
                let y = rect.min.y + (rect.height() - s) / 2.0;
                let icon = crate::assets::action_icon_name(id);
                let item = IconItem {
                    id,
                    rect: Rect::from_min_size(Pos2::new(x, y), Vec2::splat(s)),
                    icon: &icon,
                    icon_size: ICON_ITEM_CONTENT,
                    enabled: action::is_executable(app, id),
                    active: match id {
                        "menu.view.grid" => app.is_grid_visible(),
                        "menu.view.rulers" => app.is_rulers_visible(),
                        _ => false,
                    },
                    open: false,
                };
                let response = button::icon_item(ui, &item, theme.menu_background);
                tooltip::item(&response, Path::Pdn, item.enabled, || tip(id));
                if response.clicked() {
                    clicked = Some(id);
                }
                x += s;
            }
            None => {
                // align-items: center with the 1 px bottom margin.
                let (w, h) = SEPARATOR_SIZE;
                let outer_h = h + SEPARATOR_MARGIN.1;
                let bar = Rect::from_min_size(
                    Pos2::new(
                        x + SEPARATOR_MARGIN.0,
                        rect.min.y + (rect.height() - outer_h) / 2.0,
                    ),
                    Vec2::new(w, h),
                );
                button::separator_vertical_bar(ui.painter(), bar, &theme, theme.menu_background);
                x += separator_width();
            }
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    /// tooltips.md §3.3, in strip order.
    #[test]
    fn buttons_tip_with_paint_nets_texts() {
        let tips: Vec<String> = super::ITEMS
            .iter()
            .flatten()
            .map(|id| super::tip(id))
            .collect();
        assert_eq!(
            tips,
            [
                "New",
                "Open",
                "Save",
                "Print",
                "Cut",
                "Copy",
                "Paste",
                "Crop to Selection",
                "Deselect",
                "Undo",
                "Redo",
                "Display pixel grid when zoomed in",
                "Rulers",
            ]
        );
    }
}
