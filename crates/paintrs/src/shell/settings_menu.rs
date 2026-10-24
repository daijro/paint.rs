//! `#settingsMenu`: the form toggles, Settings and Help in the title bar (`flex-direction:
//! row-reverse`, so the DOM order below runs right to left), and the Help drop menu's entries.
//! Port of: js/src/interface/panel/header/settings/{SettingsMenu,HelpMenu}.js,
//! js/src/interface/item/menu/action/button/ToggleFormItem.js, css/header.css `#settingsMenu`
//! Reference: paintdotjs @ b4e6fc5
use egui::{Pos2, Rect, Ui, Vec2};
use pdn_core::settings::AppSettings;

use crate::kit::button::{self, IconItem};
use crate::kit::menu::MenuEntry;
use crate::kit::tooltip::{self, Path};
use crate::theme::{self, *};

enum Item {
    /// (action id, icon file, form id for `ToggleFormItem`)
    Icon(&'static str, &'static str, Option<&'static str>),
    Separator,
}

/// `SettingsMenu` items in DOM order.
const ITEMS: [Item; 7] = [
    Item::Icon("menu.help", "menu_help_icon.png", None),
    Item::Icon("menu.settings", "menu_utilities_settings_icon.png", None),
    Item::Separator,
    Item::Icon(
        "menu.window.colors",
        "menu_window_colors_icon.png",
        Some("colorsForm"),
    ),
    Item::Icon(
        "menu.window.layers",
        "menu_window_layers_icon.png",
        Some("layerForm"),
    ),
    Item::Icon(
        "menu.window.history",
        "menu_window_history_icon.png",
        Some("historyForm"),
    ),
    Item::Icon(
        "menu.window.tools",
        "settings_tools_16.png",
        Some("mainToolBarForm"),
    ),
];

fn separator_width() -> f32 {
    SEPARATOR_SIZE.0 + 2.0 * SETTINGS_SEPARATOR.1
}

pub fn width() -> f32 {
    let items: f32 = ITEMS
        .iter()
        .map(|i| match i {
            Item::Icon(..) => SETTINGS_ICON_ITEM,
            Item::Separator => separator_width(),
        })
        .sum();
    items + (ITEMS.len() - 1) as f32 * SETTINGS_MENU_GAP + 2.0 * SETTINGS_MENU_PADDING
}

/// ToggleFormItem is active while its form is open (`app:window_open_state_changed`): the forms
/// save `windows.<id>.visible` whenever they open or close, so the setting mirrors `isOpen()`.
fn form_open(settings: &AppSettings, form: &str) -> bool {
    settings.values["windows"][form]["visible"].as_bool() != Some(false)
}

/// `HelpMenu` entries (`new DropEntry(id, ...)`; Plugins has no callback, so it is disabled).
pub fn help_entries() -> Vec<MenuEntry> {
    use MenuEntry::{Action as A, Separator as S};
    vec![
        A("menu.help.helpTopics"),
        A("menu.help.pdnWebsite"),
        A("menu.help.pdnSearch"),
        S,
        A("menu.help.donate"),
        A("menu.help.forum"),
        A("menu.help.tutorials"),
        A("menu.help.plugins"),
        A("menu.help.sendFeedback"),
        S,
        A("menu.help.about"),
    ]
}

/// What the strip reported this frame.
pub struct SettingsInput {
    /// The Help item's border box (its drop menu's anchor).
    pub help_rect: Rect,
    /// The action of a clicked item ("menu.help" opens the Help menu).
    pub clicked: Option<&'static str>,
}

/// `help_open`: the Help drop menu shows (`.clickable.open`).
pub fn show(ui: &mut Ui, rect: Rect, settings: &AppSettings, help_open: bool) -> SettingsInput {
    let theme = theme::current(ui.ctx());
    let content = rect.shrink(SETTINGS_MENU_PADDING);
    let mut right = content.max.x;
    let mut input = SettingsInput {
        help_rect: Rect::NOTHING,
        clicked: None,
    };
    for item in &ITEMS {
        match *item {
            Item::Icon(id, icon, form) => {
                let r = Rect::from_min_max(
                    Pos2::new(right - SETTINGS_ICON_ITEM, content.min.y),
                    Pos2::new(right, content.min.y + SETTINGS_ICON_ITEM),
                );
                let item = IconItem {
                    id,
                    rect: r,
                    icon,
                    icon_size: SETTINGS_ICON,
                    enabled: true,
                    active: form.is_some_and(|f| form_open(settings, f)),
                    open: id == "menu.help" && help_open,
                };
                let response = button::icon_item(ui, &item, theme.title_bar);
                // The aux strip's AutoToolTip: the item's Text (tooltips.md §3.4).
                tooltip::item(&response, Path::Pdn, true, || {
                    pdn_core::i18n::t(&format!("{id}.text"))
                });
                if response.clicked() {
                    input.clicked = Some(id);
                }
                if id == "menu.help" {
                    input.help_rect = r;
                }
                right = r.min.x - SETTINGS_MENU_GAP;
            }
            Item::Separator => {
                let (h, mx, mb) = SETTINGS_SEPARATOR;
                let y = content.min.y + (content.height() - h - mb) / 2.0;
                let x = right - mx - SEPARATOR_SIZE.0;
                let bar = Rect::from_min_size(Pos2::new(x, y), Vec2::new(SEPARATOR_SIZE.0, h));
                button::separator_vertical_bar(ui.painter(), bar, &theme, theme.title_bar);
                right -= separator_width() + SETTINGS_MENU_GAP;
            }
        }
    }
    input
}
