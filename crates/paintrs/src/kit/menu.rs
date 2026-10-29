//! Drop menus: the popup box, its entries, separators and one level of submenus.
//! Port of: js/src/interface/item/menu/action/drop/{DropEntry,DropMenuPopup,SubmenuDropEntry}.js,
//! js/src/interface/item/menu/action/ActionItem.js (`getIconPath`, `getText`, `getShortcut`),
//! css/menu.css (`.drop-menu`, `.command-drop-menu`, `.effect-submenu`, `.drop-entry`,
//! `.checked-drop-entry`, `.submenu-entry`, `.submenu-arrow`, `.vertical-separator`)
//! The submenu arrow is Paint.NET's (`kit::glyph`); the metrics are the UI-scale patched ones
//! (`theme::DropMenuMetrics`).
//! Reference: paintdotjs @ b4e6fc5
//!
//! The document strip's context menu (js/src/interface/panel/header/documents/
//! DocumentContextMenu.js; css/menu.css `.document-context-menu`, `.document-context-title`) adds
//! a title row and entries that are not actions.
//!
//! Interface notes (vs the plan): the open submenu lives in a caller-owned `DropMenuState`;
//! theme and icons come from the context; `pixels` feeds the box shadow's backdrop replay.
//! Only the command metrics (`.command-drop-menu`, `.effect-submenu`) are ported: every menu of
//! this task uses them. `MenuPick::Submenu` reports a press that keeps the menu open.
use egui::{Color32, Id, Pos2, Rect, Response, Sense, Ui, Vec2};
use pdn_core::action::{self, ShortcutRegistry};
use pdn_core::app::AppWorkspace;

use super::button::{self, MenuItemStyle};
use super::glyph::{self, ArrowDir};
use super::img::{self, Filter};
use super::px;
use super::shadow::{self, BoxShadow, TexturePixels};
use super::text::{self, FontSpec};
use super::tooltip::{self, Path};
use crate::theme::{self, *};

/// A drop menu's children (`DropEntry`, `VerticalSeparator`, `SubmenuDropEntry`).
#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    /// `ActionRegistry.get(id).createDropEntry()`, or `new DropEntry(id, null)` (disabled) for ids
    /// that are not actions.
    Action(&'static str),
    /// The same with `.withNoIcon()`.
    ActionNoIcon(&'static str),
    Separator,
    Submenu {
        key: &'static str,
        entries: Vec<MenuEntry>,
    },
    /// A radio entry (`checked-drop-entry` + `tool_strip_checked` icon while checked).
    Checked(&'static str, bool),
    /// A `ToolSelector` entry: `new DropEntry(id).withTranslationKey("name", false)`, with the
    /// selector's icon overrides.
    Tool(pdn_core::tool::types::ToolType),
    /// `DocumentContextMenu.createTitle`: a `.document-context-title` row.
    Title(String),
    /// `DocumentContextMenu.createEntry`: a `DropEntry` with its own text, icon (empty: none) and
    /// enabled state, showing `id`'s shortcut when `id` is an action. A press picks `id`.
    Command {
        id: &'static str,
        label: String,
        icon: &'static str,
        enabled: bool,
    },
}

/// A popup owned by `owner` (`data-owner-id`), dropped from `anchor` (the owner's border box,
/// CSS px), with the metrics of `style`.
#[derive(Clone, Debug, PartialEq)]
pub struct DropMenu {
    pub owner: &'static str,
    pub style: MenuStyle,
    pub anchor: Rect,
    pub entries: Vec<MenuEntry>,
    /// `DropMenuItem.isDropUp()`.
    pub drop_up: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DropMenuState {
    /// The open `SubmenuDropEntry` (index into `entries`).
    pub submenu: Option<usize>,
    /// The popup's border box as last shown (its owner's outside-press test).
    pub popup: Option<Rect>,
    /// The open submenu's border box as last shown.
    pub sub_popup: Option<Rect>,
}

impl DropMenuState {
    /// Whether `p` is on the popup or its open submenu.
    pub fn contains(&self, p: Pos2) -> bool {
        [self.popup, self.sub_popup]
            .into_iter()
            .flatten()
            .any(|r| r.contains(p))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPick {
    /// An enabled entry was pressed: the owner closes, then runs the action.
    Action(&'static str),
    /// A submenu entry was pressed (`stopPropagation`): the menu stays open.
    Submenu,
}

/// What the menu needs from the app.
pub struct MenuContext<'a> {
    pub app: &'a AppWorkspace,
    pub shortcuts: &'a ShortcutRegistry,
    pub pixels: TexturePixels<'a>,
}

/// The popup's class, which selects its metrics. (The base `.icon-drop-menu` metrics, 16 px
/// icons and rows, are not ported: every drop menu so far overrides them.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuStyle {
    /// `.command-drop-menu` (`DropMenuItem`s whose id starts with "menu.").
    Command,
    /// `.effect-submenu` (`SubmenuDropEntry` popups): no shortcut column when empty.
    EffectSubmenu,
    /// `.drop-menu[data-owner-id="toolStripChooser.chooseToolButton"]`: a fixed width, no
    /// shortcuts.
    ToolChooser,
    /// `.command-drop-menu.document-context-menu`: the command metrics with its own minimum
    /// widths, placed at `anchor.min` (`DropMenuPopup.positionAt`, a pointer position).
    DocumentContext,
    /// `.drop-menu[data-owner-id="swatch"]` (the Colors form's palette menu): the command
    /// metrics, its own minimum widths, no shortcuts.
    Palette,
    /// A plain `.drop-menu` (a `DropMenuItem` whose id is not "menu.…", e.g. the status bar's
    /// unit selector): the command metrics, no minimum widths, no shortcuts.
    Plain,
}

impl MenuStyle {
    /// The metrics as Blink lays them out at `ppp` (`theme::lu`).
    fn metrics(self, ppp: f32) -> DropMenuMetrics {
        let m = match self {
            MenuStyle::Command
            | MenuStyle::EffectSubmenu
            | MenuStyle::DocumentContext
            | MenuStyle::Palette
            | MenuStyle::Plain => COMMAND_MENU,
            MenuStyle::ToolChooser => TOOL_CHOOSER_MENU,
        };
        let q = |v: f32| lu(v, ppp);
        DropMenuMetrics {
            icon: q(m.icon),
            gutter: q(m.gutter),
            divider: q(m.divider),
            gap: q(m.gap),
            entry_height: q(m.entry_height),
            padding_y: q(m.padding_y),
            padding_right: q(m.padding_right),
            font: m.font,
            entry_margin: q(m.entry_margin),
            width: m.width.map(q),
        }
    }

    /// `.shortcut` shows (`display: none` in the tool chooser and the palette menu).
    fn shortcuts(self) -> bool {
        !matches!(
            self,
            MenuStyle::ToolChooser | MenuStyle::Palette | MenuStyle::Plain
        )
    }
}

impl DropMenuMetrics {
    fn spec(&self) -> FontSpec {
        FontSpec::new(self.font).line_height(self.entry_height)
    }

    fn entry_height(&self) -> f32 {
        self.entry_height + 2.0 * (self.padding_y + MENU_ITEM_BORDER)
    }

    fn margin(&self, row: &Row) -> f32 {
        match row {
            Row::Entry(_) => self.entry_margin,
            Row::Separator => DROP_SEPARATOR_MARGIN,
            Row::Title(_) => 0.0,
        }
    }

    fn height(&self, row: &Row, ppp: f32) -> f32 {
        match row {
            Row::Entry(_) => self.entry_height(),
            Row::Separator => RULE_SEPARATOR.0,
            Row::Title(_) => lu(DOCUMENT_CONTEXT_TITLE.0, ppp),
        }
    }
}

/// One entry as built (`DropEntry.buildElement`).
struct View<'e> {
    id: &'static str,
    label: String,
    shortcut: String,
    icon: Option<String>,
    enabled: bool,
    checked: bool,
    mark: Mark,
    submenu: Option<&'e [MenuEntry]>,
    /// A press picks `id` (an action, or a `Command` entry).
    pick: bool,
    /// A tool chooser entry: it tips like the Tools window's button (tooltips.md §3.2).
    tool: Option<pdn_core::tool::types::ToolType>,
}

fn label_of(app: &AppWorkspace, id: &str, suffix: &str) -> String {
    match action::get(id) {
        Some(_) => action::menu_label(app, id),
        None => pdn_core::i18n::t(&format!("{id}.{suffix}")),
    }
}

/// `ActionItem.getIconPath` (`definition.icon` for effects; Repeat shows the last effect's).
fn icon_of(app: &AppWorkspace, id: &str) -> String {
    if id == "menu.effects.repeat" {
        let last = app.last_effect.as_deref().unwrap_or("");
        let def = ["adjustment.", "menu.effects."]
            .iter()
            .find_map(|p| action::get(&format!("{p}{last}")));
        return def.and_then(|d| d.icon).unwrap_or("effect_icon.png").into();
    }
    match action::get(id).and_then(|a| a.icon) {
        Some(icon) => icon.into(),
        None => crate::assets::action_icon_name(id),
    }
}

/// What an entry's plugin slot marks (S§4.1): a plugin or an Extra (the same gray plus, their
/// tooltips differ), an installed third-party plug-in or look (a puzzle piece), or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    None,
    Plugin,
    Extra,
    ThirdParty,
}

/// An entry's mark; submenu parents never carry one.
pub(crate) fn mark(entry: &MenuEntry) -> Mark {
    match *entry {
        MenuEntry::Action(id)
        | MenuEntry::ActionNoIcon(id)
        | MenuEntry::Checked(id, _)
        | MenuEntry::Command { id, .. } => {
            let installed = id.strip_prefix("menu.effects.");
            if installed.is_some_and(pdn_core::effect::wasm::is_plugin) {
                Mark::ThirdParty
            } else if action::plugin_item(id).is_some() {
                Mark::Plugin
            } else if action::is_extra(id) {
                Mark::Extra
            } else {
                Mark::None
            }
        }
        MenuEntry::Tool(tool) if tool.is_extra() => Mark::Extra,
        MenuEntry::Tool(_) => Mark::None,
        MenuEntry::Separator | MenuEntry::Title(_) | MenuEntry::Submenu { .. } => Mark::None,
    }
}

fn view<'e>(cx: &MenuContext, entry: &'e MenuEntry) -> Option<View<'e>> {
    let shortcut = |id: &str| {
        cx.shortcuts
            .shortcut(id)
            .and_then(|k| k.to_string())
            .unwrap_or_default()
    };
    let app = cx.app;
    let is_action = |id: &str| action::get(id).is_some();
    Some(match *entry {
        MenuEntry::Separator | MenuEntry::Title(_) => return None,
        MenuEntry::Command {
            id,
            ref label,
            icon,
            enabled,
        } => View {
            id,
            mark: mark(entry),
            label: label.clone(),
            shortcut: shortcut(id),
            // `withNoIcon()`: an empty name.
            icon: (!icon.is_empty()).then(|| icon.into()),
            enabled,
            checked: false,
            submenu: None,
            pick: true,
            tool: None,
        },
        MenuEntry::Action(id) | MenuEntry::ActionNoIcon(id) => View {
            id,
            mark: mark(entry),
            label: label_of(app, id, "text"),
            shortcut: shortcut(id),
            icon: matches!(entry, MenuEntry::Action(_)).then(|| icon_of(app, id)),
            enabled: action::is_executable(app, id),
            checked: false,
            submenu: None,
            pick: is_action(id),
            tool: None,
        },
        MenuEntry::Checked(id, checked) => View {
            id,
            mark: mark(entry),
            label: label_of(app, id, "text"),
            shortcut: shortcut(id),
            icon: checked.then(|| "tool_strip_checked.png".into()),
            enabled: action::is_executable(app, id),
            checked,
            submenu: None,
            pick: is_action(id),
            tool: None,
        },
        MenuEntry::Tool(tool) => View {
            id: tool.id(),
            mark: mark(entry),
            label: pdn_core::i18n::t(&format!("{}.name", tool.id())),
            shortcut: shortcut(tool.id()),
            icon: Some(tool.icon().into()),
            enabled: true,
            checked: false,
            submenu: None,
            pick: is_action(tool.id()),
            tool: Some(tool),
        },
        MenuEntry::Submenu { key, ref entries } => View {
            id: key,
            mark: Mark::None,
            label: label_of(app, key, "name"),
            shortcut: String::new(),
            icon: None,
            // `setEnabledFromActionExecutable`: enabled while any entry is.
            enabled: entries
                .iter()
                .filter_map(|e| view(cx, e))
                .any(|v| v.enabled),
            checked: false,
            submenu: Some(entries),
            pick: false,
            tool: None,
        },
    })
}

enum Row<'e> {
    Entry(View<'e>),
    Separator,
    Title(String),
}

/// A laid-out popup: border box and children tops (CSS px, fractional as Blink lays them out).
struct Popup<'e> {
    owner: &'static str,
    m: DropMenuMetrics,
    /// `.shortcut` shows.
    shortcuts: bool,
    rect: Rect,
    /// Entry content-box width (`grid` max-content, at least the min-width).
    content_w: f32,
    style: MenuStyle,
    rows: Vec<(f32, Row<'e>)>,
}

/// `offsetWidth`/`offsetHeight` of a box laid out at the origin.
fn offset(v: f32) -> f32 {
    (v + 0.5).floor()
}

impl<'e> Popup<'e> {
    fn layout(
        painter: &egui::Painter,
        cx: &MenuContext,
        owner: &'static str,
        style: MenuStyle,
        entries: &'e [MenuEntry],
    ) -> Popup<'e> {
        let rows: Vec<Row> = entries
            .iter()
            .map(|e| match e {
                MenuEntry::Title(t) => Row::Title(t.clone()),
                _ => view(cx, e).map_or(Row::Separator, Row::Entry),
            })
            .collect();
        let ppp = painter.pixels_per_point();
        let q = |v: f32| lu(v, ppp);
        let m = style.metrics(ppp);
        let measure = |s: &str| text::measure(painter, s, m.spec());
        let content_w = rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(v) => Some(v),
                Row::Separator | Row::Title(_) => None,
            })
            .map(|v| {
                let min = match (style, v.submenu.is_some()) {
                    (MenuStyle::Command, true) if owner == "menu.effects" => {
                        q(EFFECTS_CATEGORY_ENTRY_MIN_WIDTH)
                    }
                    (MenuStyle::Command, _) => q(COMMAND_ENTRY_MIN_WIDTH),
                    (MenuStyle::DocumentContext, _) => q(DOCUMENT_CONTEXT_ENTRY_MIN_WIDTH),
                    (MenuStyle::EffectSubmenu, _) => EFFECT_SUBMENU_ENTRY_MIN_WIDTH,
                    (MenuStyle::Palette, _) => q(PALETTE_ENTRY_MIN_WIDTH),
                    (MenuStyle::ToolChooser | MenuStyle::Plain, _) => 0.0,
                };
                let last_column = match (style, v.submenu.is_some()) {
                    (_, true) => q(SUBMENU_ARROW_COLUMN),
                    (MenuStyle::Command | MenuStyle::DocumentContext, false) => {
                        q(DROP_SHORTCUT_MARGIN)
                            + measure(&v.shortcut).max(q(DROP_SHORTCUT_MIN_WIDTH))
                    }
                    (MenuStyle::EffectSubmenu, false) if v.shortcut.is_empty() => 0.0,
                    (MenuStyle::EffectSubmenu, false) => measure(&v.shortcut),
                    // `.shortcut { display: none }`.
                    (MenuStyle::ToolChooser | MenuStyle::Palette | MenuStyle::Plain, false) => 0.0,
                };
                let indicator = if v.mark != Mark::None {
                    PLUGIN_INDICATOR_SIZE.0 + PLUGIN_INDICATOR_RIGHT
                } else {
                    0.0
                };
                let max_content = m.gutter + m.gap + measure(&v.label) + last_column + indicator;
                max_content.max(min)
            })
            .fold(0.0, f32::max);
        // Block layout inside the border: adjacent vertical margins collapse.
        let mut y = DROP_MENU_BORDER;
        let mut prev_margin = 0.0;
        let mut tops = Vec::with_capacity(rows.len());
        for row in &rows {
            y += m.margin(row).max(prev_margin);
            tops.push(y);
            y += m.height(row, ppp);
            prev_margin = m.margin(row);
        }
        let h = y + prev_margin + DROP_MENU_BORDER;
        let entry_extra = m.padding_right + 2.0 * (MENU_ITEM_BORDER + m.entry_margin);
        // The popup's content box fits its widest child (`width: fit-content`), at least its
        // `min-width`; a `.document-context-title` (border-box, its own min-width) is one.
        let (_, _, title_pad_right, _) = DOCUMENT_CONTEXT_TITLE;
        let doc_min = q(DOCUMENT_CONTEXT_MIN_WIDTH);
        let title_w = rows
            .iter()
            .filter_map(|r| match r {
                Row::Title(t) => {
                    Some((m.divider + m.gap + measure(t) + title_pad_right).max(doc_min))
                }
                _ => None,
            })
            .fold(0.0, f32::max);
        let popup_min = match style {
            MenuStyle::DocumentContext => doc_min,
            MenuStyle::Palette => q(PALETTE_MIN_WIDTH),
            _ => 0.0,
        };
        let content_w = content_w.max(title_w.max(popup_min) - entry_extra);
        // A fixed width sets the content box; the entries fill it.
        let content_w = m.width.map_or(content_w, |w| w - entry_extra);
        let w = content_w + entry_extra + 2.0 * DROP_MENU_BORDER;
        Popup {
            owner,
            m,
            shortcuts: style.shortcuts(),
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h)),
            content_w,
            style,
            rows: tops.into_iter().zip(rows).collect(),
        }
    }

    /// `DropMenuPopup.positionAt`: clamped 2 px inside the window, by the rounded size.
    fn position_at(&mut self, x: f32, y: f32, screen: Rect) {
        let m = DROP_MENU_VIEWPORT_MARGIN;
        let (ow, oh) = (offset(self.rect.width()), offset(self.rect.height()));
        let x = m.max(x.min(screen.max.x - ow - m));
        let y = m.max(y.min(screen.max.y - oh - m));
        self.rect = Rect::from_min_size(Pos2::new(x, y), self.rect.size());
    }

    /// `positionAtAnchor` (DropMenuItem.open: right-aligned past half the window width).
    fn position_at_anchor(&mut self, anchor: Rect, drop_up: bool, screen: Rect) {
        let align_end = anchor.max.x > screen.width() / 2.0;
        let x = if align_end {
            anchor.max.x - offset(self.rect.width())
        } else {
            anchor.min.x
        };
        let y = if drop_up {
            anchor.min.y - offset(self.rect.height())
        } else {
            anchor.max.y
        };
        self.position_at(x, y, screen);
    }

    /// `SubmenuDropEntry.open`: beside the entry overlapping it by 1 px, flipped left at the
    /// window edge, raised to fit; `max-height: calc(100vh - 4px)`.
    fn position_beside(&mut self, entry: Rect, screen: Rect) {
        let max_h = screen.height() - SUBMENU_MAX_HEIGHT_INSET;
        if self.rect.height() > max_h {
            // ponytail: `overflow-y: auto` is not ported (no scrollbar, no wheel); the content is
            // clipped. Add scrolling when an effect category outgrows the window.
            self.rect.set_height(max_h);
        }
        let (ow, oh) = (offset(self.rect.width()), offset(self.rect.height()));
        let opens_left = entry.max.x + ow > screen.max.x;
        let x = if opens_left {
            entry.min.x - ow + SUBMENU_OVERLAP
        } else {
            entry.max.x - SUBMENU_OVERLAP
        };
        let y = entry
            .min
            .y
            .min(screen.max.y - oh - DROP_MENU_VIEWPORT_MARGIN);
        self.position_at(x, y.max(0.0), screen);
    }

    fn entry_rect(&self, top: f32) -> Rect {
        let inset = DROP_MENU_BORDER + self.m.entry_margin;
        Rect::from_min_size(
            Pos2::new(self.rect.min.x + inset, self.rect.min.y + top),
            Vec2::new(self.rect.width() - 2.0 * inset, self.m.entry_height()),
        )
    }
}

/// Paints popup `p` and runs its entries. Returns (pick, hovered entry index, entry rects).
fn show_popup(
    ui: &mut Ui,
    p: &Popup,
    shadow_id: Id,
    open_submenu: Option<usize>,
    cx: &MenuContext,
) -> (Option<MenuPick>, Option<usize>, Vec<Rect>) {
    let theme = theme::current(ui.ctx());
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(p.rect, ppp);
    let owner = Id::unique(("drop-menu", p.owner));
    shadow::paint_box_shadow(
        ui,
        shadow_id,
        snapped,
        BoxShadow {
            offset: Vec2::splat(DROP_MENU_SHADOW_OFFSET),
            blur: DROP_MENU_SHADOW_BLUR,
            color: DROP_MENU_SHADOW_COLOR,
        },
        cx.pixels,
    );
    // The opaque box covers what lies below (the canvas view's pdn rect included): its pixels are
    // the popup's, judged by the frame and text rules. The translucent shadow blends over the
    // canvas's Paint.NET pixels, so where it lies on a pdn rect it stays pdn (R21 (c)).
    text::occlude(ui.ctx(), snapped);
    // The popup body takes the pointer (clicks and drags: a press on an entry must not drag what
    // lies below, e.g. a form's title bar) from whatever lies below it; its content is clipped to
    // the box (`overflow-y`, see `position_beside`).
    ui.interact(snapped, ui.scope_id().with(owner), Sense::click_and_drag());
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(snapped.intersect(outer_clip));
    let painter = ui.painter().clone();
    let bg = theme.menu_background;
    painter.rect_filled(snapped, 0.0, theme.title_bar_border);
    let inner = snapped.shrink(px::border_width(DROP_MENU_BORDER, ppp));
    painter.rect_filled(inner, 0.0, bg);
    let padding_left = p.rect.min.x + DROP_MENU_BORDER;
    let padding_right = p.rect.max.x - DROP_MENU_BORDER;
    // In-flow separators, then the positioned `:before` divider, then the positioned entries.
    let separator = button::separator_rule_color(&theme, bg);
    let mut separator_rows = vec![];
    for (top, row) in &p.rows {
        if let Row::Title(t) = row {
            let (_, pad_y, _, line_height) = DOCUMENT_CONTEXT_TITLE;
            let line_height = lu(line_height, ppp);
            let at = Pos2::new(
                padding_left + p.m.divider + p.m.gap,
                p.rect.min.y + top + pad_y,
            );
            let spec = FontSpec::new(p.m.font).line_height(line_height);
            text::draw_aligned(
                &painter,
                Rect::from_min_max(
                    at,
                    Pos2::new(p.rect.max.x - padding_right, at.y + line_height),
                ),
                t,
                spec,
                theme.menu_item_text,
                text::Alignment::Start,
            );
        }
        if let Row::Separator = row {
            let y = p.rect.min.y + top;
            button::separator_rule(
                &painter,
                padding_left,
                padding_right,
                y,
                &theme,
                p.m.divider,
                bg,
            );
            // The rule's rows as `separator_rule` paints them (whole device pixels).
            separator_rows.push(px::snap_line(
                Rect::from_min_size(Pos2::new(0.0, y), Vec2::new(0.0, RULE_SEPARATOR.0)),
                ppp,
            ));
        }
    }
    let divider_alpha = px::css_alpha(DROP_DIVIDER_OPACITY);
    // `:before { width: 1px }` inside the border: a whole-device-pixel line.
    let divider = px::snap_line(
        Rect::from_min_size(
            Pos2::new(padding_left + p.m.divider, inner.min.y),
            Vec2::new(1.0, inner.height()),
        ),
        ppp,
    );
    painter.rect_filled(
        divider,
        0.0,
        px::blend(bg, theme.title_bar_border, divider_alpha),
    );
    for s in separator_rows {
        let cross = Rect::from_x_y_ranges(divider.x_range(), s.y_range());
        let c = px::blend(separator, theme.title_bar_border, divider_alpha);
        painter.rect_filled(cross, 0.0, c);
    }

    let mut pick = None;
    let mut hovered = None;
    let mut rects = vec![None; p.rows.len()];
    for (i, (top, row)) in p.rows.iter().enumerate() {
        let Row::Entry(v) = row else {
            continue;
        };
        let rect = p.entry_rect(*top);
        rects[i] = Some(rect);
        let response = entry(ui, p, i, rect, v, open_submenu == Some(i), &theme);
        // A tool tips as in the Tools window (an Extra one says it is not Paint.NET's there);
        // Paint.NET look for an installed plugin (plan BB6; Decision 1, user); S§4.1 "Tooltips":
        // an Extra item says it is not Paint.NET's, in the plugin style.
        if let Some(tool) = v.tool {
            tooltip::item(&response, Path::Pdn, v.enabled, || {
                tooltip::tool_text(tool, true)
            });
        } else if matches!(v.mark, Mark::Plugin | Mark::ThirdParty) {
            tooltip::item(&response, Path::Pdn, v.enabled, || {
                plugin_tooltip(v.id).unwrap_or_default()
            });
        } else if v.mark == Mark::Extra {
            tooltip::item(&response, Path::Pdn, v.enabled, || {
                extra_tooltip(v.id).unwrap_or_default()
            });
        }
        if response.hovered() {
            hovered = Some(i);
        }
        if response.clicked() {
            pick = match (v.submenu, v.enabled) {
                (Some(_), true) => Some(MenuPick::Submenu),
                (None, true) if v.pick => Some(MenuPick::Action(v.id)),
                _ => None,
            };
        }
    }
    ui.set_clip_rect(outer_clip);
    let rects = rects
        .into_iter()
        .map(|r| r.unwrap_or(Rect::NOTHING))
        .collect();
    (pick, hovered, rects)
}

/// One `.drop-entry`: frame, checked cell, icon, label, shortcut or submenu arrow.
fn entry(
    ui: &mut Ui,
    p: &Popup,
    index: usize,
    rect: Rect,
    v: &View,
    open: bool,
    theme: &Theme,
) -> Response {
    let painter = ui.painter().clone();
    let ppp = painter.pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(
        snapped,
        ui.scope_id().with((p.owner, index)),
        Sense::click(),
    );
    let m = p.m;
    let spec = m.spec();
    let state = MenuItemStyle::of(response.hovered(), open, false, v.enabled);
    let mut fill =
        button::paint_menu_item_frame(&painter, snapped, state, theme, theme.menu_background);
    // Content box: inside the 1 px border, no left padding.
    let content = Pos2::new(
        rect.min.x + MENU_ITEM_BORDER,
        rect.min.y + MENU_ITEM_BORDER + m.padding_y,
    );
    if v.checked {
        let checked_cell = lu(CHECKED_CELL, ppp);
        let cell = px::snap_rect(
            Rect::from_min_size(
                Pos2::new(
                    content.x + (m.gutter - checked_cell) / 2.0,
                    // `top: calc(50% - 10.4px)` of the padding box (without the
                    // `translateY(-50%)`).
                    rect.min.y + MENU_ITEM_BORDER + (rect.height() - 2.0 * MENU_ITEM_BORDER) / 2.0
                        - checked_cell / 2.0,
                ),
                Vec2::splat(checked_cell),
            ),
            ppp,
        );
        painter.rect_filled(cell, 0.0, theme.menu_item_selected_border);
        let c = theme.checkbox_checked;
        fill = px::blend(
            fill,
            Color32::from_rgb(c.r(), c.g(), c.b()),
            px::css_alpha(CHECKED_CELL_FILL_ALPHA),
        );
        let border = px::border_width(MENU_ITEM_BORDER, ppp);
        painter.rect_filled(cell.shrink(border), 0.0, fill);
    }
    if let Some(icon) = &v.icon {
        let at = Pos2::new(
            content.x + (m.gutter - m.icon) / 2.0,
            content.y + (m.entry_height - m.icon) / 2.0,
        );
        let filter = if v.enabled {
            Filter::None
        } else {
            Filter::Grayscale
        };
        let r = px::snap_rect(Rect::from_min_size(at, Vec2::splat(m.icon)), ppp);
        // Drop menus are composited layers: the icon rasters popup-local.
        let layer = px::snap_rect(p.rect, ppp).min;
        img::paint_icon_in_layer(&painter, icon, r, fill, filter, layer);
    }
    let color = if v.enabled {
        theme.menu_item_text
    } else {
        MENU_ITEM_DISABLED_TEXT
    };
    let text_x = content.x + m.gutter + m.gap;
    let last_column = if v.submenu.is_some() {
        lu(SUBMENU_ARROW_COLUMN, ppp)
    } else if matches!(p.style, MenuStyle::Command | MenuStyle::DocumentContext) {
        lu(DROP_SHORTCUT_MARGIN, ppp)
            + text::measure(&painter, &v.shortcut, spec).max(lu(DROP_SHORTCUT_MIN_WIDTH, ppp))
    } else if p.shortcuts {
        text::measure(&painter, &v.shortcut, spec)
    } else {
        0.
    };
    let indicator = if v.mark != Mark::None {
        PLUGIN_INDICATOR_SIZE.0 + PLUGIN_INDICATOR_RIGHT
    } else {
        0.
    };
    text::draw_aligned(
        &painter,
        Rect::from_min_max(
            Pos2::new(text_x, content.y),
            Pos2::new(
                content.x + p.content_w - last_column - indicator,
                content.y + m.entry_height,
            ),
        ),
        &v.label,
        spec,
        color,
        text::Alignment::Start,
    );
    let right = content.x + p.content_w
        - if v.mark != Mark::None {
            PLUGIN_INDICATOR_SIZE.0 + PLUGIN_INDICATOR_RIGHT
        } else {
            0.0
        };
    if v.submenu.is_some() {
        // The CSS box of the (patched 4x8) `.submenu-arrow`: Paint.NET's arrow centred in it.
        let (w, h) = SUBMENU_ARROW;
        let at = Pos2::new(right - w, content.y + (m.entry_height - h) / 2.0);
        let r = Rect::from_min_size(at, Vec2::new(w, h));
        glyph::paint_arrow(&painter, ArrowDir::Right, r, theme.menu_item_text, fill);
    } else if !v.shortcut.is_empty() && p.shortcuts {
        // Right-aligned in its column (`.effect-submenu`'s max-content column ends there too).
        let w = text::measure(&painter, &v.shortcut, spec);
        text::draw(
            &painter,
            Pos2::new(right - w, content.y),
            &v.shortcut,
            spec,
            color,
        );
    }
    // Paint.NET's installed-plugin slot (plan BB6): one gray plus for a plugin and for an Extra
    // (user, 2026-10-24), a puzzle piece for third-party code (user, 2026-10-29). A disabled row
    // fades it into the row.
    if v.mark != Mark::None {
        let filter = if v.enabled {
            Filter::None
        } else {
            Filter::Opacity {
                alpha: px::css_alpha(DISABLED_MARK_OPACITY),
                background: fill,
            }
        };
        let rect = px::snap_rect(plugin_indicator_rect(rect), ppp);
        let layer = px::snap_rect(p.rect, ppp).min;
        let icon = if v.mark == Mark::ThirdParty {
            "third_party_plugin_indicator.png"
        } else {
            "plugin_indicator.png"
        };
        img::paint_icon_in_layer(&painter, icon, rect, fill, filter, layer);
    }
    response
}

/// The plus on a disabled row: half over the row, on dark #202020 as dark as the disabled text.
const DISABLED_MARK_OPACITY: f32 = 0.5;

/// Paint.NET's installed-plugin slot (Photocraft research §D), in DIP.
pub fn plugin_indicator_rect(item: Rect) -> Rect {
    let (width, height) = PLUGIN_INDICATOR_SIZE;
    Rect::from_min_size(
        Pos2::new(
            item.right() - width - PLUGIN_INDICATOR_RIGHT,
            item.center().y - height / 2.0,
        ),
        Vec2::new(width, height),
    )
}

/// tooltips.md §3.9, trimmed by user choice to the plugin name only (no version, no website).
pub fn plugin_tooltip(action_id: &str) -> Option<String> {
    action::plugin_item(action_id).map(|m| format!("Plugin: {}", m.name))
}

/// S§4.1 "Tooltips": the line every Extra item's and tool's tip ends with.
pub const FROM_PHOTOCRAFT: &str = "(from PhotoCraft)";

/// S§4.1: the plus as text, after an Extra item's name in a list that draws no icon (a
/// drop-down's choices).
pub const PLUS_TEXT: &str = " +";

/// S§4.1 "Tooltips": an Extra item's name (a dialog command's without its "..."), then
/// [`FROM_PHOTOCRAFT`] on the next line. paint.rs's own Batch Export has none; Fill Selection
/// (Content-Aware) runs the Content Aware Fill plugin, so it tips as that plugin does.
pub fn extra_tooltip(action_id: &str) -> Option<String> {
    if action_id == "menu.edit.fillSelectionContentAware" {
        return Some("Plugin: Content Aware Fill".into());
    }
    let name = action::get(action_id)
        .filter(|_| action::is_extra(action_id) && action_id != action::BATCH_EXPORT)?
        .label?
        .trim_end_matches("...");
    Some(format!("{name}\n{FROM_PHOTOCRAFT}"))
}

/// Shows `menu` (and its open submenu) over everything painted in `ui`'s layer so far, and runs
/// the `DropEntry`/`SubmenuDropEntry` pointer behaviour: hovering a submenu entry opens it,
/// hovering another entry closes it. Returns the press, if any.
pub fn show_drop_menu(
    ui: &mut Ui,
    menu: &DropMenu,
    state: &mut DropMenuState,
    cx: &MenuContext,
) -> Option<MenuPick> {
    let screen = ui.ctx().content_rect();
    let painter = ui.painter().clone();
    let mut popup = Popup::layout(&painter, cx, menu.owner, menu.style, &menu.entries);
    match menu.style {
        MenuStyle::DocumentContext => {
            popup.position_at(menu.anchor.min.x, menu.anchor.min.y, screen)
        }
        _ => popup.position_at_anchor(menu.anchor, menu.drop_up, screen),
    }
    state.popup = Some(px::snap_rect(popup.rect, painter.pixels_per_point()));
    // A submenu that lost its entries' executability closes (`setEnabledFromActionExecutable`).
    let submenu = state.submenu.and_then(|i| match &popup.rows.get(i)?.1 {
        Row::Entry(v) if v.enabled => Some((i, v.id, v.submenu?)),
        _ => None,
    });
    state.submenu = submenu.map(|(i, ..)| i);
    let (main_shadow, sub_shadow) = shadow_ids(menu.owner);
    let (pick, hovered, rects) = show_popup(ui, &popup, main_shadow, state.submenu, cx);
    let mut sub_pick = None;
    if let Some((i, key, entries)) = submenu {
        let mut sub = Popup::layout(&painter, cx, key, MenuStyle::EffectSubmenu, entries);
        sub.position_beside(rects[i], screen);
        state.sub_popup = Some(px::snap_rect(sub.rect, painter.pixels_per_point()));
        let (p, ..) = show_popup(ui, &sub, sub_shadow, None, cx);
        sub_pick = p;
    } else {
        state.sub_popup = None;
        shadow::forget(ui.ctx(), sub_shadow);
    }
    if let Some(i) = hovered {
        state.submenu = match &popup.rows[i].1 {
            Row::Entry(v) if v.submenu.is_some() && v.enabled => Some(i),
            Row::Entry(v) if v.submenu.is_some() => state.submenu,
            _ => None,
        };
    }
    sub_pick.or(pick)
}

/// The shadow texture slots of `owner`'s popup and of its (one at a time) submenu.
fn shadow_ids(owner: &str) -> (Id, Id) {
    (
        Id::unique(("drop-menu-shadow", owner)),
        Id::unique(("drop-menu-shadow", owner, "submenu")),
    )
}

/// Releases the shadow textures of `owner`'s popups (the owner closed).
pub fn forget(ctx: &egui::Context, owner: &'static str) {
    let (main, sub) = shadow_ids(owner);
    shadow::forget(ctx, main);
    shadow::forget(ctx, sub);
}

/// `compareNames`: `localeCompare(..., {sensitivity: "base"})` ignoring a trailing "...".
/// ponytail: case-insensitive code-point order, which agrees with ICU's base collation for the
/// shipped English names; port a collator if a translation needs it.
pub fn compare_names(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| s.strip_suffix("...").unwrap_or(s).to_lowercase();
    key(a).cmp(&key(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Photocraft research §D: 12×16 DIP, right inset 9 DIP, vertically centred.
    #[test]
    fn plugin_indicator_slot() {
        let r = plugin_indicator_rect(Rect::from_min_size(
            Pos2::new(0.0, 100.0),
            Vec2::new(200.0, 22.0),
        ));
        assert_eq!(
            (r.min, r.size()),
            (Pos2::new(179.0, 103.0), Vec2::new(12.0, 16.0))
        );
        let offset = Vec2::new(23.0, 41.0);
        assert_eq!(
            plugin_indicator_rect(Rect::from_min_size(
                Pos2::new(0.0, 100.0) + offset,
                Vec2::new(200.0, 22.0)
            )),
            r.translate(offset)
        );
    }

    /// tooltips.md §3.9: the tooltip is just "Plugin: <name>" (no version or website, by user choice).
    #[test]
    fn plugin_tooltips() {
        assert_eq!(
            plugin_tooltip("menu.effects.bbFlipHorizontal").unwrap(),
            "Plugin: Flip Horizontal"
        );
        assert_eq!(
            plugin_tooltip("adjustment.bbHueSaturationPlus").unwrap(),
            "Plugin: Hue / Saturation+"
        );
        assert_eq!(plugin_tooltip("menu.effects.blurEffect"), None);
        assert_eq!(plugin_tooltip("effects.fill.submenu"), None);
    }

    /// README "Menu tree": real action registration marks plugin actions, never submenu parents.
    #[test]
    fn plugin_action_views() {
        let app = AppWorkspace::default();
        let shortcuts = ShortcutRegistry::default();
        let cx = MenuContext {
            app: &app,
            shortcuts: &shortcuts,
            pixels: &|_| None,
        };
        let mark = |e: MenuEntry| view(&cx, &e).unwrap().mark;
        assert_eq!(
            mark(MenuEntry::Action("menu.effects.bbFlipHorizontal")),
            Mark::Plugin
        );
        assert_eq!(
            mark(MenuEntry::ActionNoIcon("adjustment.bbHueSaturationPlus")),
            Mark::Plugin
        );
        assert_eq!(
            mark(MenuEntry::Action("menu.effects.pcSolarize")),
            Mark::Extra
        );
        assert_eq!(
            mark(MenuEntry::Action("menu.effects.blurEffect")),
            Mark::None
        );
        // S§4.1: the tool chooser's Extra tools carry the plus in the plugin slot.
        for tool in pdn_core::tool::types::ToolType::VALUES {
            let want = if tool.is_extra() {
                Mark::Extra
            } else {
                Mark::None
            };
            assert_eq!(mark(MenuEntry::Tool(tool)), want, "{tool:?}");
        }
        for key in ["effects.fill.submenu", "effects.texture.submenu"] {
            let submenu = MenuEntry::Submenu {
                key,
                entries: vec![],
            };
            assert_eq!(mark(submenu), Mark::None);
        }
    }

    /// Photocraft research §D: a long plugin label leaves the indicator slot clear.
    #[test]
    fn plugin_labels_leave_room_for_the_indicator() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let app = AppWorkspace::default();
        let shortcuts = ShortcutRegistry::default();
        let cx = MenuContext {
            app: &app,
            shortcuts: &shortcuts,
            pixels: &|_| None,
        };
        let entries = [MenuEntry::Command {
            id: "menu.effects.bbFlipHorizontal",
            label: "A long plugin label repeated many times ".repeat(5),
            icon: "",
            enabled: true,
        }];
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let painter = ui.painter().clone();
            let popup = Popup::layout(&painter, &cx, "test", MenuStyle::EffectSubmenu, &entries);
            let Row::Entry(v) = &popup.rows[0].1 else {
                panic!("plugin row")
            };
            let row = popup.entry_rect(popup.rows[0].0);
            let text_right = row.min.x
                + MENU_ITEM_BORDER
                + popup.m.gutter
                + popup.m.gap
                + text::measure(&painter, &v.label, popup.m.spec());
            assert!(text_right < plugin_indicator_rect(row).left());
        });
        output.textures_delta.clear();
    }

    /// The meshes and the 12 × 16 texture's pixels that `id`'s row paints with `icon` as the
    /// icon pack's `plugin_indicator.png` (`None`: the shipped SVG; empty: not supplied), and the
    /// slot it should fill.
    fn paint_mark(
        id: &'static str,
        icon: Option<Vec<u8>>,
        enabled: bool,
    ) -> (Vec<Rect>, Rect, Option<Vec<Color32>>) {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        if let Some(png) = icon {
            crate::assets::Assets::set_icon_override(&ctx, "plugin_indicator.png", png);
        }
        let app = AppWorkspace::default();
        let shortcuts = ShortcutRegistry::default();
        let cx = MenuContext {
            app: &app,
            shortcuts: &shortcuts,
            pixels: &|_| None,
        };
        let entries = [MenuEntry::ActionNoIcon(id)];
        let mut expected = Rect::NOTHING;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let ctx = ui.ctx().clone();
            let popup = Popup::layout(
                ui.painter(),
                &cx,
                "test",
                MenuStyle::EffectSubmenu,
                &entries,
            );
            let mut v = view(&cx, &entries[0]).unwrap();
            v.enabled = enabled;
            let rect = popup.entry_rect(popup.rows[0].0);
            expected = px::snap_rect(plugin_indicator_rect(rect), 1.0);
            entry(ui, &popup, 0, rect, &v, false, &theme::current(&ctx));
        });
        let meshes = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => Some(m.calc_bounds()),
                _ => None,
            })
            .collect();
        let pixels = output
            .textures_delta
            .set
            .values()
            .flatten()
            .find(|d| d.image.width() == 12 && d.image.height() == 16)
            .map(|delta| {
                let egui::ImageData::Color(image) = &delta.image;
                image.pixels.clone()
            });
        output.textures_delta.clear();
        (meshes, expected, pixels)
    }

    /// Plan BB6 and S§4.1, by the user's choice (2026-10-24): a plugin and an Extra both paint
    /// the shipped gray plus in Paint.NET's installed-plugin slot; a disabled row fades it toward
    /// the row, and an icon pack that does not supply it leaves the slot blank.
    #[test]
    fn plugins_and_extras_paint_the_gray_plus() {
        let image = image::RgbaImage::from_pixel(12, 16, image::Rgba([20, 200, 70, 255]));
        let mut green = std::io::Cursor::new(Vec::new());
        image.write_to(&mut green, image::ImageFormat::Png).unwrap();
        let green = green.into_inner();
        let row = theme::current(&egui::Context::default()).menu_background;
        let off_row = |c: &Color32| {
            [(c.r(), row.r()), (c.g(), row.g()), (c.b(), row.b())]
                .iter()
                .map(|&(a, b)| a.abs_diff(b) as u32)
                .sum::<u32>()
        };
        for id in ["menu.effects.bbFlipHorizontal", "menu.effects.pcSolarize"] {
            let (meshes, slot, pixels) = paint_mark(id, None, true);
            assert_eq!(meshes, vec![slot], "{id}");
            let pixels = pixels.unwrap();
            assert!(pixels.iter().any(|c| off_row(c) > 0), "{id}: blank");
            for c in pixels {
                assert!(c.r() == c.g() && c.g() == c.b(), "{id}: {c:?} is not gray");
            }
            assert_eq!(paint_mark(id, Some(vec![]), true).0, vec![], "{id}");
            let (meshes, _, on) = paint_mark(id, Some(green.clone()), true);
            assert_eq!(meshes, vec![slot], "{id}");
            let (meshes, _, off) = paint_mark(id, Some(green.clone()), false);
            assert_eq!(meshes, vec![slot], "{id}");
            for (on, off) in on.unwrap().iter().zip(&off.unwrap()) {
                assert!(
                    off_row(off) < off_row(on),
                    "{id}: {off:?} not dimmer than {on:?}"
                );
            }
        }
    }

    /// S§4.1 "Tooltips" (user, 2026-10-24): the name, then "(from PhotoCraft)" on the next line.
    #[test]
    fn extra_tooltip_says_from_photocraft() {
        assert_eq!(
            extra_tooltip("menu.effects.pcSolarize").unwrap(),
            "Solarize\n(from PhotoCraft)"
        );
        assert_eq!(extra_tooltip("menu.effects.bbDream"), None);
        assert_eq!(extra_tooltip("menu.effects.embossEffect"), None);
    }

    /// `DropMenuPopup.positionAt`: a popup asked past the window's right or bottom edge is clamped
    /// `DROP_MENU_VIEWPORT_MARGIN` (2 px) inside it by its rounded `offsetWidth`/`offsetHeight`;
    /// one past the top-left edge 2 px in from there; one that fits stays put.
    #[test]
    fn popups_clamp_two_px_inside_the_window() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 480.0));
        let mut popup = Popup {
            owner: "test",
            m: COMMAND_MENU,
            shortcuts: false,
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::new(200.4, 150.6)),
            content_w: 0.0,
            style: MenuStyle::Plain,
            rows: vec![],
        };
        let at = |popup: &mut Popup, x: f32, y: f32| {
            popup.position_at(x, y, screen);
            popup.rect.min
        };
        // Right edge: 640 − round(200.4) − 2 = 438; bottom: 480 − round(150.6) − 2 = 327.
        assert_eq!(at(&mut popup, 600.0, 100.0), Pos2::new(438.0, 100.0));
        assert_eq!(at(&mut popup, 100.0, 400.0), Pos2::new(100.0, 327.0));
        assert_eq!(at(&mut popup, 700.0, 900.0), Pos2::new(438.0, 327.0));
        assert_eq!(at(&mut popup, -50.0, -9.0), Pos2::new(2.0, 2.0));
        assert_eq!(at(&mut popup, 10.5, 20.25), Pos2::new(10.5, 20.25));
        let size = popup.rect.size() - Vec2::new(200.4, 150.6);
        assert!(size.length() < 1e-3, "the size is kept: {size:?}");
    }

    #[test]
    fn names_sort_like_compare_names() {
        let mut v = vec![
            "Levels",
            "Auto-Level",
            "Brightness / Contrast",
            "Black and White",
        ];
        v.sort_by(|a, b| compare_names(a, b));
        assert_eq!(
            v,
            [
                "Auto-Level",
                "Black and White",
                "Brightness / Contrast",
                "Levels"
            ]
        );
        assert_eq!(
            compare_names("Resize...", "resize"),
            std::cmp::Ordering::Equal
        );
    }
}
