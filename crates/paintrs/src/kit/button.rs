//! Menu-item frames, icon/text menu items and separators.
//! Port of: css/menu.css (`.menu-item`, `.clickable:hover:before`, `.menu-item[active]`,
//! `.menu-item[disabled]`, `.icon-item`, `.horizontal-separator`, `.vertical-separator`),
//! js/src/interface/item/{Item,menu/MenuItem,menu/action/ActionItem,menu/action/button/IconItem}.js
//! Reference: paintdotjs @ b4e6fc5
//!
//! Interface notes (vs the plan): frames and icons take the flat `backdrop` color under them,
//! because Chromium composites the translucent `[active]` fill and the icons with Skia's integer
//! src-over, which egui's GPU blending does not reproduce; icons are named files (`kit::img`
//! resamples the 64 px PNG on the CPU) rather than `TextureHandle`s.
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::img::{self, Filter};
use super::px;
use super::text::{self, FontSpec};
use crate::theme::{
    MENU_ITEM_BORDER, MENU_ITEM_DISABLED_TEXT, MENU_ITEM_RING, RULE_SEPARATOR, Theme,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItemStyle {
    Normal,
    /// `.clickable:hover.menu-item:not([disabled])`
    Hover,
    /// `.clickable.open` (drop menu showing, pointer elsewhere)
    Open,
    /// `.menu-item[active]`
    Active,
    /// `.menu-item[disabled]`
    Disabled,
    /// `.menu-item[disabled]:hover`
    DisabledHover,
}

impl MenuItemStyle {
    /// The frame the CSS cascade picks. `.menu-item[active]` sets background and border with
    /// `!important`, so it wins over hover and `[disabled]:hover` alike; a disabled item's
    /// content (text color, grayscale icon) stays disabled under any frame.
    pub fn of(hovered: bool, open: bool, active: bool, enabled: bool) -> Self {
        match (active, enabled, hovered, open) {
            (true, ..) => Self::Active,
            (false, false, true, _) => Self::DisabledHover,
            (false, false, false, _) => Self::Disabled,
            (false, true, true, _) => Self::Hover,
            (false, true, false, true) => Self::Open,
            _ => Self::Normal,
        }
    }
}

/// The background, 1 px border and `:before` ring (1 px of `--color-menu-background` inside the
/// border) of a `.menu-item` whose pixel-snapped border box is `rect`. Returns the fill color
/// (the backdrop for the item's content).
pub fn paint_menu_item_frame(
    painter: &Painter,
    rect: Rect,
    state: MenuItemStyle,
    theme: &Theme,
    backdrop: Color32,
) -> Color32 {
    // `border: 1px` floors to one device pixel.
    let ppp = painter.pixels_per_point();
    let bw = px::border_width(MENU_ITEM_BORDER, ppp);
    let ring_w = px::border_width(MENU_ITEM_RING, ppp);
    let (fill, border, ring) = match state {
        MenuItemStyle::Normal | MenuItemStyle::Disabled => return backdrop,
        MenuItemStyle::Hover => (theme.menu_item_hover, theme.menu_item_border, true),
        MenuItemStyle::Open => (theme.menu_item_hover, theme.menu_item_border, false),
        MenuItemStyle::Active => (
            px::over(backdrop, theme.menu_item_selected),
            theme.menu_item_selected_border,
            true,
        ),
        MenuItemStyle::DisabledHover => (
            theme.menu_item_disabled_hover,
            theme.menu_item_disabled_hover_border,
            true,
        ),
    };
    painter.rect_filled(rect, 0.0, border);
    let mut inner = rect.shrink(bw);
    if ring {
        painter.rect_filled(inner, 0.0, theme.menu_background);
        inner = inner.shrink(ring_w);
    }
    painter.rect_filled(inner, 0.0, fill);
    fill
}

/// An `.icon-item`: `rect` is its (layout, possibly fractional) border box; the `icon_size`
/// square icon is centered in it. `id` is salted with the caller's `Ui` id.
pub struct IconItem<'a> {
    pub id: &'a str,
    pub rect: Rect,
    /// `assets/icons` file name.
    pub icon: &'a str,
    pub icon_size: f32,
    pub enabled: bool,
    pub active: bool,
    /// `.clickable.open`: its drop menu is showing.
    pub open: bool,
}

pub fn icon_item(ui: &mut Ui, item: &IconItem, backdrop: Color32) -> Response {
    let theme = crate::theme::current(ui.ctx());
    let ppp = ui.painter().pixels_per_point();
    let snapped = px::snap_rect(item.rect, ppp);
    let response = ui.interact(snapped, ui.scope_id().with(item.id), Sense::click());
    let state = MenuItemStyle::of(response.hovered(), item.open, item.active, item.enabled);
    let fill = paint_menu_item_frame(ui.painter(), snapped, state, &theme, backdrop);
    let icon = Rect::from_center_size(item.rect.center(), Vec2::splat(item.icon_size));
    // `.menu-item[disabled] img { filter: grayscale(100%) }`, whatever the frame.
    let filter = if item.enabled {
        Filter::None
    } else {
        Filter::Grayscale
    };
    img::paint_icon(
        ui.painter(),
        item.icon,
        px::snap_rect(icon, ppp),
        fill,
        filter,
    );
    response
}

/// A text `.menu-item` with `padding` around the label's line box; `rect` is the border box and
/// `id` (the action id) is salted with the caller's `Ui` id. `open`: `.clickable.open`.
#[allow(clippy::too_many_arguments)]
pub fn text_menu_item(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    label: &str,
    spec: FontSpec,
    padding: Vec2,
    enabled: bool,
    open: bool,
) -> Response {
    let theme = crate::theme::current(ui.ctx());
    let ppp = ui.painter().pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(snapped, ui.scope_id().with(id), Sense::click());
    let state = MenuItemStyle::of(response.hovered(), open, false, enabled);
    paint_menu_item_frame(ui.painter(), snapped, state, &theme, theme.menu_background);
    let color = if enabled {
        theme.menu_item_text
    } else {
        MENU_ITEM_DISABLED_TEXT
    };
    let top_left = rect.min + Vec2::splat(MENU_ITEM_BORDER) + padding;
    text::draw(ui.painter(), top_left, label, spec, color);
    response
}

/// CSS `.horizontal-separator` (a 1 px vertical bar, `var(--color-title-bar-border)` at
/// opacity .5): `rect` is its layout box, snapped here to a whole-device-pixel line.
pub fn separator_vertical_bar(painter: &Painter, rect: Rect, theme: &Theme, backdrop: Color32) {
    let color = px::blend(backdrop, theme.title_bar_border, px::css_alpha(0.5));
    painter.rect_filled(px::snap_line(rect, painter.pixels_per_point()), 0.0, color);
}

/// `.vertical-separator`'s color over `backdrop` (`var(--color-title-bar-border)` at opacity .2).
pub fn separator_rule_color(theme: &Theme, backdrop: Color32) -> Color32 {
    px::blend(backdrop, theme.title_bar_border, px::css_alpha(0.2))
}

/// CSS `.vertical-separator` (a 1 px rule, `var(--color-title-bar-border)` at opacity .2,
/// `margin: 2px 2px 2px 27px` or `left_margin`) across `[x0, x1)` at `y` (the rule's top).
pub fn separator_rule(
    painter: &Painter,
    x0: f32,
    x1: f32,
    y: f32,
    theme: &Theme,
    left_margin: f32,
    backdrop: Color32,
) {
    let color = separator_rule_color(theme, backdrop);
    let (h, m) = RULE_SEPARATOR;
    let rect = Rect::from_min_max(Pos2::new(x0 + left_margin, y), Pos2::new(x1 - m, y + h));
    painter.rect_filled(px::snap_line(rect, painter.pixels_per_point()), 0.0, color);
}

/// History/Layers f16 targets retain the chrome geometry but blend their fills in R22's space.
pub fn paint_display_item_frame(
    painter: &Painter,
    rect: Rect,
    state: MenuItemStyle,
    theme: &Theme,
    backdrop: Color32,
    mode: pdn_core::render::display::RenderMode,
) -> Color32 {
    let mut display_theme = *theme;
    display_theme.menu_item_selected =
        super::color::display_over(backdrop, theme.menu_item_selected, mode);
    paint_menu_item_frame(painter, rect, state, &display_theme, backdrop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_precedence_follows_css() {
        assert_eq!(
            MenuItemStyle::of(true, false, true, true),
            MenuItemStyle::Active
        );
        assert_eq!(
            MenuItemStyle::of(true, true, false, true),
            MenuItemStyle::Hover
        );
        assert_eq!(
            MenuItemStyle::of(false, true, false, true),
            MenuItemStyle::Open
        );
        assert_eq!(
            MenuItemStyle::of(true, false, false, false),
            MenuItemStyle::DisabledHover
        );
        assert_eq!(
            MenuItemStyle::of(false, false, false, true),
            MenuItemStyle::Normal
        );
        // `.menu-item[active]` background/border are !important: active beats [disabled](:hover).
        assert_eq!(
            MenuItemStyle::of(false, false, true, false),
            MenuItemStyle::Active
        );
        assert_eq!(
            MenuItemStyle::of(true, false, true, false),
            MenuItemStyle::Active
        );
    }
}
