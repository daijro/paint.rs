//! Colors ported from paintdotjs css/theme.css. Field names = CSS custom property names.
//! Shell strip metrics are the `pub const`s at the end, in Paint.NET DIPs (= egui points), each
//! citing its CSS rule and, where paint.rs changes it for Paint.NET's UI scale, the change. The
//! "UI scale" rules: 1, no extra zoom (`--pdn-ui-scale: 1`); 2, paintdotjs's 125 % lengths divided
//! by 1.25; 3, other lengths 1:1; 4, Paint.NET's own sizes for some rows and icons; 5, 12-DIP text.
use egui::Color32;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    // Chromium stores CSS alpha as round(a * 255).
    Color32::from_rgba_unmultiplied_const(r, g, b, (a * 255.0 + 0.5) as u8)
}

/// A CSS length as Blink lays it out at device scale `ppp` (zoom for DSF): a LayoutUnit, 1/64
/// device px, truncated (`LayoutUnit(float)`); points. The UI-scale patches' fractional lengths
/// land on these (3.2px → 3.1875 at 1×, 3.1953125 at 2×). The fractional constants below are the
/// CSS values; their users quantize them with this at the frame's scale.
pub(crate) fn lu(px: f32, ppp: f32) -> f32 {
    (px * ppp * 64.0) as i32 as f32 / 64.0 / ppp
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scheme {
    Dark,
    Blue,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// The colour scheme this theme was derived from (custom tweaks keep it).
    pub scheme: Scheme,
    pub background: Color32,                         // --color-background
    pub title_bar: Color32,                          // --color-title-bar
    pub title_bar_border: Color32,                   // --color-title-bar-border
    pub menu_background: Color32,                    // --color-menu-background
    pub menu_item_text: Color32,                     // --color-menu-item-text
    pub menu_item_hover: Color32,                    // --color-menu-item-hover
    pub menu_item_border: Color32,                   // --color-menu-item-border
    pub menu_item_selected: Color32,                 // --color-menu-item-selected
    pub menu_item_selected_border: Color32,          // --color-menu-item-selected-border
    pub menu_item_disabled_hover: Color32,           // --color-menu-item-disabled-hover
    pub menu_item_disabled_hover_border: Color32,    // --color-menu-item-disabled-hover-border
    pub scrollbar_background: Color32,               // --color-scrollbar-background
    pub scrollbar_thumb: Color32,                    // --color-scrollbar-thumb
    pub close_button: Color32,                       // --color-close-button
    pub close_button_hover: Color32,                 // --color-close-button-hover
    pub close_button_text: Color32,                  // --color-close-button-text
    pub window_background: Color32,                  // --color-window-background
    pub window_border: Color32,                      // --color-window-border
    pub slider_tick: Color32,                        // --color-slider-tick
    pub slider: Color32,                             // --color-slider
    pub slider_outline: Color32,                     // --color-slider-outline
    pub slider_hover1: Color32,                      // --color-slider-hover1
    pub slider_hover2: Color32,                      // --color-slider-hover2
    pub slider_hover_outline: Color32,               // --color-slider-hover-outline
    pub button: Color32,                             // --color-button
    pub button_text: Color32,                        // --color-button-text
    pub button_outline: Color32,                     // --color-button-outline
    pub checkbox_checked: Color32,                   // --color-checkbox-checked
    pub number_input_background: Color32,            // --color-number-input-background
    pub pdn_toolbar_background: Color32,             // --pdn-toolbar-background (derived)
    pub pdn_control_background: Color32,             // --pdn-control-background (derived)
    pub pdn_menu_background: Color32,                // --pdn-menu-background
    pub pdn_menu_gutter: Color32,                    // --pdn-menu-gutter (derived)
    pub pdn_control_border: Color32,                 // --pdn-control-border
    pub pdn_menu_border: Color32,                    // --pdn-menu-border
    pub pdn_separator: Color32,                      // --pdn-separator
    pub pdn_text: Color32,                           // --pdn-text (derived)
    pub pdn_arrow_background: Color32,               // --pdn-arrow-background (derived)
    pub pdn_arrow_foreground: Color32,               // --pdn-arrow-foreground (derived)
    pub pdn_combo_arrow_foreground: Color32,         // --pdn-combo-arrow-foreground
    pub pdn_combo_arrow_hover_background: Color32,   // --pdn-combo-arrow-hover-background
    pub pdn_combo_arrow_hover_border: Color32,       // --pdn-combo-arrow-hover-border
    pub pdn_combo_arrow_hover_foreground: Color32,   // --pdn-combo-arrow-hover-foreground
    pub pdn_combo_arrow_pressed_background: Color32, // --pdn-combo-arrow-pressed-background
    pub pdn_scrollbar_track: Color32,                // --pdn-scrollbar-track
    pub pdn_scrollbar_thumb: Color32,                // --pdn-scrollbar-thumb
    pub pdn_slider_background: Color32,              // --pdn-slider-background
    pub pdn_slider_text: Color32,                    // --pdn-slider-text
    pub pdn_slider_filled_text: Color32,             // --pdn-slider-filled-text
    pub dialog_surface: Color32,                     // --color-dialog-surface
    pub dialog_surface_alt: Color32,                 // --color-dialog-surface-alt
    pub dialog_border: Color32,                      // --color-dialog-border
    pub muted_text: Color32,                         // --color-muted-text
    pub subtle_text: Color32,                        // --color-subtle-text
    pub link: Color32,                               // --color-link
    pub link_hover: Color32,                         // --color-link-hover
    pub preview_background: Color32,                 // --color-preview-background
    pub curve_background: Color32,                   // --color-curve-background
    pub curve_grid: Color32,                         // --color-curve-grid
    pub curve_line: Color32,                         // --color-curve-line
    pub curve_point: Color32,                        // --color-curve-point
    pub curve_point_border: Color32,                 // --color-curve-point-border
    pub item_redo_background: Color32,               // --color-item-redo-background
    pub item_redo_text: Color32,                     // --color-item-redo-text
    pub item_redo_hover_background: Color32,         // --color-item-redo-hover-background
    pub item_redo_hover_text: Color32,               // --color-item-redo-hover-text
    pub error_text: Color32,                         // port: BB27 inline errors (critical text)
}

const DARK: Theme = Theme {
    scheme: Scheme::Dark,
    background: rgb(0x2F2F2F),
    title_bar: rgb(0x0D0D0D),
    title_bar_border: rgb(0x66666A),
    menu_background: rgb(0x202020),
    menu_item_text: rgb(0xFFFFFF),
    menu_item_hover: rgb(0x18324B),
    menu_item_border: rgb(0x074F97),
    menu_item_selected: rgba(0, 120, 215, 0.4),
    menu_item_selected_border: rgb(0x0078D7),
    menu_item_disabled_hover: rgb(0x2A2A2A),
    menu_item_disabled_hover_border: rgb(0x454545),
    scrollbar_background: rgb(0x171717),
    scrollbar_thumb: rgb(0x4D4D4D),
    close_button: rgb(0xC75050),
    close_button_hover: rgb(0xE04343),
    close_button_text: rgb(0xFFFFFF),
    window_background: rgb(0x282828),
    window_border: rgba(195, 195, 195, 0.21),
    slider_tick: rgb(0xDADADA),
    slider: rgb(0xD0D0D0),
    slider_outline: rgb(0x707070),
    slider_hover1: rgb(0xDDEFFA),
    slider_hover2: rgb(0xB7E2F9),
    slider_hover_outline: rgb(0x3C7FB1),
    button: rgb(0xFDFDFD),
    button_text: rgb(0x000000),
    button_outline: rgb(0xD0D0D0),
    checkbox_checked: rgb(0x005FB8),
    number_input_background: rgb(0x1E1E1E),
    pdn_toolbar_background: rgb(0x202020),
    pdn_control_background: rgb(0x1E1E1E),
    pdn_menu_background: rgb(0x1C1C1C),
    pdn_menu_gutter: rgb(0x202020),
    pdn_control_border: rgb(0x8D8D8D),
    pdn_menu_border: rgb(0x646464),
    pdn_separator: rgb(0x46464A),
    pdn_text: rgb(0xFFFFFF),
    pdn_arrow_background: rgb(0xFDFDFD),
    pdn_arrow_foreground: rgb(0x000000),
    pdn_combo_arrow_foreground: rgb(0xA1A1A1),
    pdn_combo_arrow_hover_background: rgb(0xCCE8FF),
    pdn_combo_arrow_hover_border: rgb(0x0078D7),
    pdn_combo_arrow_hover_foreground: rgb(0x0067C0),
    pdn_combo_arrow_pressed_background: rgb(0x99D1FF),
    pdn_scrollbar_track: rgb(0x101010),
    pdn_scrollbar_thumb: rgb(0x777777),
    pdn_slider_background: rgb(0xCFCFCF),
    pdn_slider_text: rgb(0x111111),
    pdn_slider_filled_text: rgb(0xFFFFFF),
    dialog_surface: rgb(0x181818),
    dialog_surface_alt: rgb(0x1B1B1B),
    dialog_border: rgb(0x606060),
    muted_text: rgb(0xC8C8C8),
    subtle_text: rgb(0xAAAAAA),
    link: rgb(0x62B5FF),
    link_hover: rgb(0x91CDFF),
    preview_background: rgb(0x303030),
    curve_background: rgb(0x171717),
    curve_grid: rgb(0x8B8B8B),
    curve_line: rgb(0xF2F2F2),
    curve_point: rgb(0xFFFFFF),
    curve_point_border: rgb(0x111111),
    item_redo_background: rgb(0x5D5D5D),
    item_redo_text: rgb(0x000000),
    item_redo_hover_background: rgb(0x466079),
    item_redo_hover_text: rgb(0xFFFFFF),
    error_text: rgb(0xFF99A4),
};

impl Theme {
    pub fn for_scheme(scheme: Scheme) -> Theme {
        let mut t = DARK;
        t.scheme = scheme;
        match scheme {
            Scheme::Dark => {}
            Scheme::Blue => {
                t.title_bar = rgb(0x07172B);
                t.menu_background = rgb(0x182534);
                t.window_background = rgb(0x24303C);
                t.number_input_background = rgb(0x15202C);
                t.pdn_menu_background = rgb(0x14202B);
                t.pdn_control_border = rgb(0x71869A);
                t.pdn_menu_border = rgb(0x52677B);
                t.pdn_separator = rgb(0x40566B);
                t.pdn_scrollbar_track = rgb(0x101923);
                t.pdn_scrollbar_thumb = rgb(0x60758A);
                t.dialog_surface = rgb(0x111B25);
                t.dialog_surface_alt = rgb(0x14202B);
                t.dialog_border = rgb(0x52677B);
                t.muted_text = rgb(0xBCC8D4);
                t.subtle_text = rgb(0x9EAFBF);
                t.preview_background = rgb(0x1D2B38);
                t.curve_background = rgb(0x101923);
                t.curve_grid = rgb(0x71869A);
            }
            Scheme::Light => {
                t.background = rgb(0xD7D7D7);
                t.title_bar = rgb(0xE9E9E9);
                t.title_bar_border = rgb(0xA0A0A0);
                t.menu_background = rgb(0xF1F1F1);
                t.menu_item_text = rgb(0x111111);
                t.menu_item_hover = rgb(0xCCE8FF);
                t.menu_item_border = rgb(0x99D1FF);
                t.menu_item_selected = rgba(0, 120, 215, 0.22);
                t.menu_item_disabled_hover = rgb(0xE7E7E7);
                t.menu_item_disabled_hover_border = rgb(0xC8C8C8);
                t.scrollbar_background = rgb(0xE6E6E6);
                t.scrollbar_thumb = rgb(0x8A8A8A);
                t.window_background = rgb(0xF0F0F0);
                t.window_border = rgba(0, 0, 0, 0.28);
                t.number_input_background = rgb(0xFFFFFF);
                t.pdn_menu_background = rgb(0xFFFFFF);
                t.pdn_control_border = rgb(0x767676);
                t.pdn_menu_border = rgb(0xA0A0A0);
                t.pdn_separator = rgb(0xC2C2C2);
                t.pdn_scrollbar_track = rgb(0xE6E6E6);
                t.pdn_scrollbar_thumb = rgb(0x8A8A8A);
                t.pdn_slider_background = rgb(0xE5E5E5);
                t.pdn_slider_text = rgb(0x111111);
                t.pdn_slider_filled_text = rgb(0xFFFFFF);
                t.dialog_surface = rgb(0xFFFFFF);
                t.dialog_surface_alt = rgb(0xF7F7F7);
                t.dialog_border = rgb(0xA0A0A0);
                t.muted_text = rgb(0x555555);
                t.subtle_text = rgb(0x707070);
                t.error_text = rgb(0xC42B1C);
                t.link = rgb(0x0067B8);
                t.link_hover = rgb(0x004F8F);
                t.preview_background = rgb(0xD0D0D0);
                t.curve_background = rgb(0xFFFFFF);
                t.curve_grid = rgb(0x888888);
                t.curve_line = rgb(0x202020);
            }
        }
        // Derived custom properties (resolved at use time in CSS).
        t.pdn_toolbar_background = t.menu_background;
        t.pdn_control_background = t.number_input_background;
        t.pdn_menu_gutter = t.menu_background;
        t.pdn_text = t.menu_item_text;
        t.pdn_arrow_background = t.button;
        t.pdn_arrow_foreground = t.button_text;
        t
    }
}

// ---- Shell metrics (DIPs) and CSS color literals ----

/// css/menu.css `.menu-item { margin: 1px; border: 1px solid transparent }`.
pub const MENU_ITEM_MARGIN: f32 = 1.0;
pub const MENU_ITEM_BORDER: f32 = 1.0;
/// css/menu.css `.menu-item[disabled] { color: #66666A }`: a literal, unlike the separators'
/// `var(--color-title-bar-border)` (which the light theme redefines).
pub const MENU_ITEM_DISABLED_TEXT: Color32 = rgb(0x66666A);
/// css/menu.css `.clickable:hover:before, .menu-item[active]:before { border: 1px }`.
pub const MENU_ITEM_RING: f32 = 1.0;

/// css/layout.css `header { border-bottom-width: 1px }` (color `--color-title-bar-border`).
pub const HEADER_BORDER: f32 = 1.0;
/// css/header.css `#titleBar { height: 37px; border-top: 1px; border-bottom: 1px }`;
/// Paint.NET's menu
/// row is 28 DIP from the tab's top to its foot (the bar's menu-coloured bottom border), so the
/// bar's content is the tab's 1-px margin + 27.
pub const TITLE_BAR_HEIGHT: f32 = 28.0;
pub const TITLE_BAR_BORDER: f32 = 1.0;
/// css/header.css `#mainMenu { height: 36px (border-box); margin-left: 3px; margin-top: 1px;
/// padding: 2px 1px 1px; border: 1px; border-bottom-width: 0 }`; here 27 high,
/// `padding: 1px 1px 0` (the items rnd(2s) below the tab's top).
pub const MAIN_MENU_HEIGHT: f32 = 27.0;
pub const MAIN_MENU_MARGIN_LEFT: f32 = 3.0;
pub const MAIN_MENU_MARGIN_TOP: f32 = 1.0;
pub const MAIN_MENU_PADDING: (f32, f32, f32) = (1.0, 1.0, 0.0); // top, left/right, bottom
pub const MAIN_MENU_BORDER: f32 = 1.0; // top, left, right
/// css/header.css `#mainMenu > .menu-item { margin: 0 1px; padding: 6px 7px; font-size: 14px;
/// line-height: 17px }`; here the text is 12 DIP (rule 5) and the padding
/// `3px 7px` (25-DIP items filling the 28-DIP menu row).
pub const MAIN_MENU_ITEM_MARGIN: f32 = 1.0;
pub const MAIN_MENU_ITEM_PADDING: (f32, f32) = (7.0, 3.0); // x, y
pub const MAIN_MENU_FONT_SIZE: f32 = 12.0;
pub const MAIN_MENU_LINE_HEIGHT: f32 = 17.0;
/// css/header.css `#documentStrip { margin-left: 24px; top: 2px; --document-item-size: 72px }`;
/// one item is the cell plus the selected item's border each side (css/component.css
/// `.selected-item { border: 1px }`), plus the 19 px list button (`.document-strip-list`).
/// Here (user ruling: beside the menus) the strip starts at the menu row's top (`top: 1px`, the tab's
/// margin) and its item (the strip height) ends `DOCUMENT_STRIP_CLEARANCE` above the toolbar
/// row's bottom (Paint.NET: toolbar bottom − top − 1), so it clears the tool-options row; the
/// shell derives it from the rows (`ShellLayout::document_strip`, 54 at these rows).
pub const DOCUMENT_STRIP_MARGIN_LEFT: f32 = 24.0;
pub const DOCUMENT_STRIP_TOP: f32 = MAIN_MENU_MARGIN_TOP;
pub const DOCUMENT_STRIP_CLEARANCE: f32 = 1.0;
pub const DOCUMENT_ITEM_BORDER: f32 = 1.0;
pub const DOCUMENT_STRIP_LIST_BUTTON: f32 = 19.0;
/// `#documentStrip .scroll-item .header { position: absolute; top/left/right: 0; height: 16px;
/// padding: 2px; justify-content: space-between; align-items: center }` (height, padding).
pub const DOCUMENT_HEADER: (f32, f32) = (16.0, 2.0);
/// `.header .dirty-indicator { 10px }`: js/src/engine/util/ImageUtil.js `createDirtyStar(10)`
/// draws 8 spokes in #FFFFFF at 8·s px under #FFA500 at 3·s px, then a #FFFFFF dot of radius 3·s
/// (s = size / 32) on a 10×10 canvas shown as an `<img>`.
pub const DOCUMENT_DIRTY_SIZE: f32 = 10.0;
pub const DIRTY_STAR_COLOR: [u8; 4] = [0xFF, 0xA5, 0x00, 0xFF];
pub const DIRTY_STAR_OUTLINE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
/// `.header .window-close-button { 14px; opacity: 0; transition: opacity 0.2s }` (1 on the item's
/// `:hover`), css/window.css `.window-close-button { padding: 0 0 1px; font: bold 10px 'Arial' }`
/// (`--color-close-button*`; Arial is Noto Sans here, Ruling (P2) 5).
pub const DOCUMENT_CLOSE_SIZE: f32 = 14.0;
pub const DOCUMENT_CLOSE_FONT: f32 = 10.0;
pub const DOCUMENT_CLOSE_FAMILY: &str = "Noto Sans";
/// `.scroll-item .thumbnail { left: 50%; bottom: 4px; width (landscape) / height (portrait):
/// calc(100% - 10px); max-height / max-width: calc(100% - 10px) }` (inset per side, bottom);
/// here `calc(100% - 8px)`, Paint.NET's 4-DIP pad.
pub const DOCUMENT_THUMB_INSET: f32 = 4.0;
pub const DOCUMENT_THUMB_BOTTOM: f32 = 4.0;
/// `.document-strip-button { width: 18px; border: 1px; img 9×9 }` (`-previous { left: 0 }`,
/// `-next { right: 19px }`), `.document-strip-list { width: 19px; img 9×10 }`; `:hover`
/// `color-mix(in srgb, var(--color-menu-item-hover) 55%, transparent)` with
/// `--color-menu-item-border`; `.pushed`/`:active` `--color-menu-item-selected(-border)`.
pub const DOCUMENT_NAV_BUTTON: f32 = 18.0;
pub const DOCUMENT_NAV_IMAGE: (f32, f32) = (9.0, 9.0);
pub const DOCUMENT_LIST_IMAGE: (f32, f32) = (9.0, 10.0);
pub const DOCUMENT_NAV_HOVER_ALPHA: f32 = 0.55;
/// `.document-list-popup { top: calc(100% - 1px); right: 0; padding: 2px; border: 1px solid
/// var(--color-menu-item-selected-border); box-shadow: 0 4px 12px rgba(0, 0, 0, .55) }`; its
/// width is DocumentStrip.populateDocumentList's `min(360, max(200, ⌈125 + longest name⌉))`.
pub const DOCUMENT_LIST_WIDTH: (f32, f32, f32) = (200.0, 360.0, 125.0); // min, max, name extra
pub const DOCUMENT_LIST_PADDING: f32 = 2.0;
pub const DOCUMENT_LIST_SHADOW: (f32, f32) = (4.0, 12.0); // offset y, blur
pub const DOCUMENT_LIST_SHADOW_COLOR: Color32 = rgba(0, 0, 0, 0.55);
/// `.document-list-entry { grid-template-columns: 96px 1fr; height: 82px (border-box); padding:
/// 5px 8px 5px 5px; gap: 8px; border: 1px }` (hover / `.selected` like menu items, no ring),
/// `.document-list-preview { 96×70 }`, `.document-list-thumbnail { max 88×64; box-shadow: 0 2px
/// 5px rgba(0, 0, 0, .55) }`, `.document-list-name { 14px }` (rule 5: 12px).
pub const DOCUMENT_LIST_ENTRY_HEIGHT: f32 = 82.0;
pub const DOCUMENT_LIST_ENTRY_PADDING: (f32, f32, f32) = (5.0, 8.0, 5.0); // y, right, left
pub const DOCUMENT_LIST_GAP: f32 = 8.0;
pub const DOCUMENT_LIST_PREVIEW: (f32, f32) = (96.0, 70.0);
pub const DOCUMENT_LIST_THUMB_MAX: (f32, f32) = (88.0, 64.0);
pub const DOCUMENT_LIST_THUMB_SHADOW: (f32, f32) = (2.0, 5.0); // offset y, blur
pub const DOCUMENT_LIST_FONT: f32 = 12.0;
/// `ui-drawn-graphics.md §2.1 P7`, the content background (the document list's thumbnail
/// backdrop, §2.8 Display step 6): dark #1E1E1E, light #FFFFFF.
pub const CONTENT_BACK_DARK: Color32 = rgb(0x1E1E1E);
pub const CONTENT_BACK_LIGHT: Color32 = rgb(0xFFFFFF);
/// css/menu.css `.document-context-menu { --command-menu-entry-min-width: 234px; min-width:
/// 246px }`, `.document-context-title { height: 29px; padding: 5px 10px 5px calc(divider + gap);
/// line-height: 19px }`; rule 2: 187.2, 196.8, 23.2, 4px 8px, 15.2.
pub const DOCUMENT_CONTEXT_MIN_WIDTH: f32 = 196.8;
pub const DOCUMENT_CONTEXT_ENTRY_MIN_WIDTH: f32 = 187.2;
pub const DOCUMENT_CONTEXT_TITLE: (f32, f32, f32, f32) = (23.2, 4.0, 8.0, 15.2); // height, pad y, pad right, line height
/// css/header.css `#settingsMenu { top: 7px; margin: 0 2px 0 auto; padding: 1px; gap: 1px }`
/// (here `top: 2px`, rnd(s) below the menu row's top),
/// `.horizontal-separator { height: 16px; margin: 0 6px }` (+ css/menu.css `margin-bottom: 1px`);
/// rules 2 and 4: `.icon-item { 16px; padding: 2px }` (Paint.NET's 22-DIP aux
/// buttons), `.icon-item .icon { 16px }`.
pub const SETTINGS_MENU_TOP: f32 = 2.0;
pub const SETTINGS_MENU_MARGIN_RIGHT: f32 = 2.0;
pub const SETTINGS_MENU_PADDING: f32 = 1.0;
pub const SETTINGS_MENU_GAP: f32 = 1.0;
pub const SETTINGS_ICON_ITEM: f32 = 16.0 + 2.0 * 2.0 + 2.0 * MENU_ITEM_BORDER;
pub const SETTINGS_ICON: f32 = 16.0;
pub const SETTINGS_SEPARATOR: (f32, f32, f32) = (16.0, 6.0, 1.0); // height, margin x, margin-bottom
/// css/header.css `#commonMenu { zoom: var(--pdn-ui-scale); height: 26px; margin-left: 3px;
/// margin-top: 2px }` (rule 1: `--pdn-ui-scale: 1`; here `margin-top: 1px`, the rnd(s) gap under
/// the menu row; the toolbar (common actions) row is 26 DIP, which snaps to Paint.NET's 32 px at
/// 125 %); css/menu.css `.icon-item {
/// width/height: 16px; padding: 2px; margin: 0 }`, `.icon-item .icon { 16px }`,
/// `.horizontal-separator { width: 1px; height: 13px; margin: 0 5px 1px 5px; opacity: .5 }`.
pub const COMMON_MENU_HEIGHT: f32 = 26.0;
pub const COMMON_MENU_MARGIN: (f32, f32) = (3.0, 1.0); // left, top
pub const ICON_ITEM_CONTENT: f32 = 16.0;
pub const ICON_ITEM_PADDING: f32 = 2.0;
pub const SEPARATOR_SIZE: (f32, f32) = (1.0, 13.0);
pub const SEPARATOR_MARGIN: (f32, f32) = (5.0, 1.0); // x, bottom
/// Paint.NET plugin indicator size and right inset, DIP (Photocraft research §D; plan BB6).
pub const PLUGIN_INDICATOR_SIZE: (f32, f32) = (12.0, 16.0);
pub const PLUGIN_INDICATOR_RIGHT: f32 = 9.0;
/// css/menu.css `.vertical-separator { height: 1px; margin: 2px 2px 2px 27px; opacity: .2 }`.
pub const RULE_SEPARATOR: (f32, f32) = (1.0, 2.0); // height, margin (top/right/bottom)
/// css/header.css `#toolMenu { height: 35px; width: calc(100% - 4px); margin-left: 2px;
/// border-top: 1px (toolbar bg); border-bottom: 1px var(--pdn-menu-border) }`; the 35 px encode
/// the 125 % controls: rule 2 (35 / 1.25). The top border is Paint.NET's rnd(s)
/// gap under the toolbar row and the 26-DIP content its tool-options row, its top 56 below the
/// menu row's. Here `height: 27px; border-bottom-width: 0` — the header ends one gap (the
/// header's own bottom border) below the row (header client height = row bottom + rnd(s));
/// paintdotjs's extra `#toolMenu` border made it two.
pub const TOOL_MENU_HEIGHT: f32 = 27.0;
pub const TOOL_MENU_MARGIN_X: f32 = 2.0;
pub const TOOL_MENU_BORDER: (f32, f32) = (1.0, 0.0); // top, bottom
/// css/footer.css `footer { height: 32px }`, `#footerMenu .menu-item { margin-top/bottom: 1px;
/// padding: 4px 6px; font-size: 14px; line-height: 20px }`, `.icon-item { 30px; padding: 4px }`,
/// `.icon-item .icon, .label-menu-item .icon { 20px }`, `#toolInfoItem .icon { margin: 0 3px 0
/// 5px }`, `#canvasSizeItem { flex: 0 0 140px }`, `#cursorPositionItem { flex: 0 0 150px }`,
/// `#windowResize { 14px; margin: 15px 3px 0 4px }`; rule 5 sets the text to 12 DIP, rule 2 the
/// icons to 16, rule 4 the image-size and cursor labels to Paint.NET's 105 DIP.
pub const FOOTER_HEIGHT: f32 = 32.0;
pub const FOOTER_ITEM_PADDING: (f32, f32) = (6.0, 4.0); // x, y
pub const FOOTER_FONT_SIZE: f32 = 12.0;
pub const FOOTER_LINE_HEIGHT: f32 = 20.0;
pub const FOOTER_ICON_ITEM: f32 = 30.0;
pub const FOOTER_ICON: f32 = 16.0;
pub const FOOTER_TOOL_ICON_MARGIN: (f32, f32) = (5.0, 3.0); // left, right
pub const CANVAS_SIZE_ITEM_WIDTH: f32 = 105.0;
pub const CURSOR_POSITION_ITEM_WIDTH: f32 = 105.0;
pub const WINDOW_RESIZE: (f32, f32, f32, f32) = (14.0, 4.0, 3.0, 15.0); // size, ml, mr, mt
/// css/component.css `input[type="range"] { min-height: 28px }` with Chromium's UA `margin: 2px`
/// left/right (css zeroes top/bottom); the runnable track is 1 px with a 1 px `box-shadow` below
/// it. The width is Paint.NET's 120-DIP zoom slider (rule 4; paintdotjs: 85).
pub const RANGE_INPUT: (f32, f32) = (120.0, 28.0);
pub const RANGE_INPUT_MARGIN_X: f32 = 2.0;
pub const RANGE_TRACK_HEIGHT: f32 = 1.0;
/// css/footer.css `#footerMenu > .slider-wrapper { height: 30px }`.
pub const SLIDER_WRAPPER_HEIGHT: f32 = 30.0;
/// css/component.css `.slider-tick { width: 1.6px; height: 10px; translate(-50%, -25%) }`, at
/// `left: 50%`; css/footer.css `#footerMenu > .slider-wrapper .slider-tick { top: 12px }`.
pub const SLIDER_TICK: (f32, f32) = (1.6, 10.0);
pub const FOOTER_SLIDER_TICK_TOP: f32 = 12.0;
/// css/footer.css `#footerMenu .selector .separator { height: 18px }`.
pub const FOOTER_SELECTOR_SEPARATOR: f32 = 18.0;
/// `#zoomPercentageField`: rule 4 makes its border box Paint.NET's 48-DIP zoom box
/// (paintdotjs: `width: 50px`, content box); css/
/// component.css `input[type="text"]` gives the 1-px border and Chromium's UA `padding: 1px 2px`
/// around the 15-px line of 12-DIP text (rule 5).
pub const ZOOM_FIELD: (f32, f32) = (48.0, 19.0);
pub const ZOOM_FIELD_PADDING: (f32, f32) = (2.0, 1.0);
/// `input[type="text"]:focus-visible { box-shadow: 0 0 0 2px var(--color-menu-item-border) }`.
pub const ZOOM_FIELD_FOCUS_RING: f32 = 2.0;
/// Chromium's selection highlight of a focused text field, as the references show it
/// (status-zoom-menu, dark scheme; paintdotjs sets no `::selection`).
pub const TEXT_SELECTION: Color32 = rgb(0x063AA4);
/// css/menu.css `.selector .separator { width: 2px; margin: 2px }`, `.arrow-up { border: 3px }`.
pub const SELECTOR_SEPARATOR: (f32, f32) = (2.0, 2.0);
pub const SELECTOR_ARROW: (f32, f32, f32) = (6.0, 3.0, 2.0); // width, height, margin
/// css/layout.css `.editor-ruler*`: the 17 px ruler strips and the corner box
/// (js/src/engine/AppView.js `RULER_SIZE`), which hold Paint.NET's rulers (spec §0.10); their
/// colours are `pdn_core::render::ruler::RulerColors`. `canvas-rendering.md §2.11` step 4: labels
/// in Calibri at 10.667 DIP (8 pt); step 6: the units box abbreviation in the system Menu font
/// (Segoe UI 9 pt = 12 DIP, Global Constraints "UI scale" rule 5).
pub const RULER_SIZE: f32 = 17.0;
pub const RULER_LABEL_FONT_SIZE: f32 = 32.0 / 3.0;
pub const RULER_UNITS_FONT_SIZE: f32 = 12.0;

/// css/menu.css `.drop-menu { border: 1px solid var(--color-title-bar-border); background-color:
/// var(--color-menu-background); box-shadow: 2px 2px 5px rgba(0, 0, 0, 0.55) }`.
pub const DROP_MENU_BORDER: f32 = 1.0;
pub const DROP_MENU_SHADOW_OFFSET: f32 = 2.0;
pub const DROP_MENU_SHADOW_BLUR: f32 = 5.0;
pub const DROP_MENU_SHADOW_COLOR: Color32 = rgba(0, 0, 0, 0.55);
/// The `--drop-menu-*` custom properties a drop menu resolves, its font size, its entries'
/// margin and a fixed popup width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropMenuMetrics {
    pub icon: f32,
    pub gutter: f32,
    pub divider: f32,
    pub gap: f32,
    pub entry_height: f32,
    pub padding_y: f32,
    pub padding_right: f32,
    pub font: f32,
    pub entry_margin: f32,
    /// A fixed popup width (`width: 233px`, patched 186.4).
    pub width: Option<f32>,
}

/// css/menu.css `.command-drop-menu`, `.drop-menu[data-owner-id="measurementUnit"]`,
/// `.drop-menu[data-owner-id="swatch"]` and `.effect-submenu` share one metric set (`font-weight:
/// 400`, `.icon-drop-menu .drop-entry { margin: 1px; border: 1px (.menu-item); padding: y right y
/// 0; line-height: var(--drop-menu-entry-height) }`): paintdotjs's 125 % values (20/30/34/10/20/
/// 4/5, 14px) divided by 1.25, text 12 DIP (rule 2).
pub const COMMAND_MENU: DropMenuMetrics = DropMenuMetrics {
    icon: 16.0,
    gutter: 24.0,
    divider: 27.2,
    gap: 8.0,
    entry_height: 16.0,
    padding_y: 3.2,
    padding_right: 4.0,
    font: 12.0,
    entry_margin: 1.0,
    width: None,
};
/// css/header.css `.drop-menu[data-owner-id="toolStripChooser.chooseToolButton"]` (`padding-y:
/// 3.75px; font-size: 14.5px; width: 233px`, `.drop-entry { margin: 1.25px }`, otherwise the
/// command set): rule 2 (3px, 12px, 186.4px, margin 1px).
pub const TOOL_CHOOSER_MENU: DropMenuMetrics = DropMenuMetrics {
    padding_y: 3.0,
    entry_margin: 1.0,
    width: Some(186.4),
    ..COMMAND_MENU
};
/// `--command-menu-entry-min-width: 208px`, `.effect-submenu .drop-entry { min-width: 190px }`,
/// `.drop-menu[data-owner-id="menu.effects"] .submenu-entry { min-width: 182px }`; rule 2:
/// 166.4, 152, 145.6.
pub const COMMAND_ENTRY_MIN_WIDTH: f32 = 166.4;
/// `.drop-menu[data-owner-id="swatch"] { min-width: 258px }`, `.drop-entry { min-width: 246px }`
/// (css/menu.css:387-407); rule 2: 206.4, 196.8.
pub const PALETTE_MIN_WIDTH: f32 = 206.4;
pub const PALETTE_ENTRY_MIN_WIDTH: f32 = 196.8;
pub const EFFECT_SUBMENU_ENTRY_MIN_WIDTH: f32 = 152.0;
pub const EFFECTS_CATEGORY_ENTRY_MIN_WIDTH: f32 = 145.6;
/// `.command-drop-menu .drop-entry .shortcut { min-width: 72px; margin-left: 16px;
/// justify-self: end; text-align: right }`; rule 2: 57.6, 12.8.
pub const DROP_SHORTCUT_MIN_WIDTH: f32 = 57.6;
pub const DROP_SHORTCUT_MARGIN: f32 = 12.8;
/// `.drop-menu .submenu-entry { grid-template-columns: gutter 1fr 12px }` (rule 2: 9.6px); `.submenu-arrow { border-left: 5px solid; border-top/bottom: 4px transparent; color:
/// var(--color-menu-item-text) }` (justify-self: end), reshaped to Paint.NET's 4×8 arrow.
pub const SUBMENU_ARROW_COLUMN: f32 = 9.6;
pub const SUBMENU_ARROW: (f32, f32) = (4.0, 8.0);
/// `.command-drop-menu .vertical-separator { margin-left: var(--drop-menu-icon-divider) }` with
/// `.vertical-separator { height: 1px; margin: 2px 2px 2px 27px; opacity: .2 }`.
pub const DROP_SEPARATOR_MARGIN: f32 = 2.0;
/// `.drop-menu:before { width: 1px; left: var(--drop-menu-icon-divider); opacity: .2 }`.
pub const DROP_DIVIDER_OPACITY: f32 = 0.2;
/// `.checked-drop-entry::after { 26x26 border-box; left: (gutter - 26px) / 2; top: 50%;
/// translateY(-50%); background: color-mix(in srgb, var(--color-checkbox-checked) 55%,
/// transparent); border: 1px solid var(--color-menu-item-selected-border) }`; rule 2: 20.8.
pub const CHECKED_CELL: f32 = 20.8;
pub const CHECKED_CELL_FILL_ALPHA: f32 = 0.55;
/// js DropMenuPopup.positionAt `margin = 2`; SubmenuDropEntry.open overlaps its entry by 1 px;
/// `.effect-submenu { max-height: calc(100vh - 4px) }`.
pub const DROP_MENU_VIEWPORT_MARGIN: f32 = 2.0;
pub const SUBMENU_OVERLAP: f32 = 1.0;
pub const SUBMENU_MAX_HEIGHT_INSET: f32 = 4.0;

// ---- #toolMenu (css/header.css, under `zoom: var(--pdn-ui-scale)` = 1: DIPs as written) ----

/// `#toolMenu { font-size: 12px }` (Paint.NET's toolstrip font, Segoe UI 9 pt); the Finish
/// `button` keeps the UA's 13.333px, 12px here.
pub const TOOL_FONT_SIZE: f32 = 12.0;
/// `#toolMenu .menu-item { padding: 2px; height: 23px }`, `#toolStripChooser... { width: 68px;
/// box-sizing: border-box }`, `.selector .icon { 16px; margin: 2px }`, `.selector .separator {
/// 2x14; margin: 2px; opacity: 0 (hover .2) }`, `.selector .arrow-down { 3px borders; margin 2px }`.
pub const TOOL_SELECTOR_SIZE: (f32, f32) = (68.0, 23.0);
pub const TOOL_SELECTOR_PADDING: f32 = 2.0;
pub const TOOL_SELECTOR_ICON: f32 = 16.0;
pub const TOOL_SELECTOR_GAP: f32 = 2.0; // the icon/separator/arrow margins
pub const TOOL_SELECTOR_SEPARATOR: (f32, f32) = (2.0, 14.0);
pub const TOOL_SELECTOR_SEPARATOR_HOVER_OPACITY: f32 = 0.2;
pub const TOOL_SELECTOR_ARROW: f32 = 3.0; // border widths
/// Ruling R24: Paint.NET's tool-strip separator is 11
/// px at 96 DPI, i.e. 11 DIP (5 + the 1-DIP line slot + 5; the line itself 1 device px, drawn by
/// `px::snap_line`): `#toolMenu > .horizontal-separator { margin-left: 7px; margin-right: 5px }`
/// (here 5px; `.horizontal-separator { 1x13; margin-bottom: 1px; opacity:
/// .5 }`) and `.tool-option-section { border-left: 1px solid var(--pdn-separator); margin-left:
/// 5px; padding-left: 5px }`.
pub const TOOL_SEPARATOR_MARGIN: (f32, f32) = (5.0, 5.0);
pub const TOOL_SECTION_GAP: f32 = 5.0;
/// `#toolMenu .tool-option > span:first-child { height: 23px }`. A tool-strip text box T is GDI's
/// `MeasureText` width, A_gdi (`kit::text::gdi_width`) + ⌈h/6⌉ + ⌈1.5·h/6⌉ = A_gdi + 7 at 96 DPI
/// (R24: DIPs), the text centred in it; a label is T.
pub const TOOL_OPTION_HEIGHT: f32 = 23.0;
// calibrate: B-L2 — DrawTextEx including its margins in DT_CALCRECT is inferred (§8.6).
pub const TOOL_TEXT_EXTRA: f32 = 7.0;
/// `[fillStyle] { margin-right: 1px }`, `[fillStyle] .tool-dropdown { margin-left: 1px }`,
/// `[fontSize] { margin-left: 1px }` (Paint.NET's combo margin, §7.3 "Combo margin").
pub const TOOL_FILL_MARGIN: f32 = 1.0;
/// `.tool-dropdown { height: 24px; border: 1px }`, `.tool-dropdown-toolbar { height: 23px }`,
/// `.tool-dropdown-icon { min-width: 35px; padding: 1px 2px 1px 3px; gap: 1px }` (here
/// `padding: 1px 0 1px 1px; gap: 2px`, the image-only split); a field's value
/// label `padding-left: 4px`, value icon `16px; margin-left: 3px`, and its arrow 20px border-box
/// with `margin: -1px -1px -1px auto`. Field dropdowns default to `width: 130px`.
pub const TOOL_DROPDOWN_HEIGHT: (f32, f32) = (24.0, 23.0); // field, toolbar
pub const TOOL_DROPDOWN_LABEL_PADDING: f32 = 4.0;
pub const TOOL_DROPDOWN_VALUE_ICON: f32 = 16.0;
pub const TOOL_DROPDOWN_VALUE_ICON_MARGIN: f32 = 3.0;
pub const TOOL_DROPDOWN_FIELD_ARROW: f32 = 20.0;
pub const TOOL_DROPDOWN_DEFAULT_WIDTH: f32 = 130.0;
/// Paint.NET's metrics (R24: its 96-DPI px as DIPs) for toolbar splits and the
/// shape drop-down, positions from the face's left edge: the 2-DIP face
/// border, then the 16-DIP image (`TOOL_DROPDOWN_VALUE_ICON`) and the text box T (text centred);
/// a split ends in the 1-DIP splitter and the 14-DIP drop part (the CSS arrow span's 1-px border
/// and 13 px, the face's right border), W = 4 + I + T + 15; a drop-down ends in the 5-DIP arrow
/// and its two 2-DIP pads (`TOOL_DROP_DOWN_TAIL` 9, the CSS arrow span 10 after the text box), W =
/// 4 + I + T + 9. Either way the chevron's centre is 7 from the right edge (its 6-DIP box at W − 10).
pub const TOOL_FACE_PAD: f32 = 2.0;
pub const TOOL_SPLIT_TAIL: f32 = 15.0; // splitter 1 + drop part 14
pub const TOOL_DROP_DOWN_TAIL: f32 = 9.0; // arrow 5 + 2·2
pub const TOOL_ARROW_FROM_RIGHT: f32 = 10.0; // the chevron box's left, from W
/// `[dash] .tool-dropdown-value-icon { width: 28px }`.
pub const TOOL_DASH_ICON_WIDTH: f32 = 28.0;
/// The toolbar arrows: `.tool-dropdown-toolbar .tool-dropdown-arrow:after { left: 3px; top: 8px;
/// border-left/right: 3px; border-top: 4px }`, split ones centered (`translate(-50%, -50%)`),
/// reshaped to Paint.NET's 6×3 chevron (drawn by `kit::glyph`) and placed by
/// `TOOL_ARROW_FROM_RIGHT`: the drop-down's `top: 8px`, the
/// splits' vertically centred.
pub const TOOL_ARROW_TOP: f32 = 8.0;
pub const TOOL_ARROW: (f32, f32) = (6.0, 3.0);
/// `.tool-dropdown-split .tool-dropdown-arrow:hover { background-size: 1px 14px }`.
pub const TOOL_SPLIT_HOVER_LINE: f32 = 14.0;
/// `.tool-dropdown-icon[aria-expanded] { box-shadow: inset 0 0 0 1px rgba(195, 228, 255, .18) }`
/// (also active icon-group images and toggles); the arrow's `border-left-color: #0c4169`.
pub const TOOL_INSET_HIGHLIGHT: Color32 = rgba(195, 228, 255, 0.18);
pub const TOOL_EXPANDED_ARROW_BORDER: Color32 = rgb(0x0C4169);
/// `.tool-image-button { height: 23px; min-width: 23px; padding: 2px; border: 1px }` with 16 px
/// images (Paint.NET's max(4 + 16, rnd(23s)));
/// `.tool-finish-button { width: 58px; gap: 2px; padding: 2px 4px 2px 2px }`, here
/// `width: auto; gap: 0; padding: 2px 1px; font-size: 12px` round the
/// image at 2 and the T-wide centred text, Paint.NET's max(4 + 16 + T, 23) = 58 for "Finish".
pub const TOOL_BUTTON: f32 = 23.0;
pub const TOOL_BUTTON_ICON: f32 = 16.0;
pub const TOOL_FINISH_PAD_Y: f32 = 2.0;
/// `.tool-size-control { gap: 1px }`, `.tool-size-combo { 55x24 (fontSize 75); border: 1px }`,
/// `.tool-size-input { height: 22px; padding: 1px 3px; font: 12px "Segoe UI", sans-serif }`,
/// `.tool-size-arrow { 20x24; margin: -1px -1px -1px 0 }`. Here (R24) every size combo is
/// WinForms' 75-px floor at 96 DPI, i.e. 75 DIP.
pub const TOOL_SIZE_GAP: f32 = 1.0;
// calibrate: B-L1 — a 100 % capture confirms the 75-DIP floor (§8.6).
pub const TOOL_SIZE_COMBO: (f32, f32) = (75.0, 24.0); // width, height
pub const TOOL_SIZE_INPUT: (f32, f32, f32) = (22.0, 1.0, 3.0); // height, padding y, padding x
pub const TOOL_SIZE_ARROW: f32 = 20.0;
/// `.tool-slider-control { width: clamp(135px, 11vw, 221px); height: 23px }`, `.tool-slider-meter
/// { min-width: 89px; border: 1px }`, `.tool-slider-fill { background: #0078d7 }` (its
/// `text-shadow` dropped: Paint.NET's tolerance slider draws none, R39). Here a fixed 175-DIP
/// meter between the ± buttons (`width: 221px`,
/// whatever the window), its value text at 0.9 × 12 DIP, rnd(3s) in, vertically centred (`left:
/// 3px; top: 0; line-height: 21px; font-size: 10.8px`).
pub const TOOL_SLIDER_METER: f32 = 175.0;
pub const TOOL_SLIDER_FILL: Color32 = rgb(0x0078D7);
pub const TOOL_SLIDER_TEXT: (f32, f32) = (3.0, 10.8); // left, font size
/// `.tool-option-toggle { 23x23; border: 1px }`, `.tool-font-style-icon { 16x16; font-size: 16px;
/// line-height: 16px }` (strikeout 11px, bold 700).
pub const TOOL_TOGGLE: f32 = 23.0;
pub const TOOL_FONT_STYLE_SIZE: (f32, f32) = (16.0, 11.0);
/// `#toolMenu .tool-option input[type="number"] { 75x24; border: 1px; padding: 1px 3px }` (an
/// option's `width` overrides it); the Rectangle Select W/H are Paint.NET's 50-DIP text boxes
/// (width 50, no spin buttons).
pub const TOOL_NUMBER_INPUT: (f32, f32) = (75.0, 24.0);

// ---- Form inputs (css/component.css) ----

/// css/theme.css `--font-size: 8.25pt` (11px).
pub const FONT_SIZE_PX: f32 = 11.0;
/// `.pdn-number-input { grid-template-columns: minmax(0, 1fr) 17px }`, `> input[type="number"]
/// { padding: 1px 3px }`.
pub const NUMBER_SPIN_COLUMN: f32 = 17.0;
pub const NUMBER_PADDING: (f32, f32) = (1.0, 3.0); // y, x
/// The UA's `input[type=text] { padding: 1px 2px }`.
pub const TEXT_INPUT_PADDING: (f32, f32) = (1.0, 2.0); // y, x
/// `.pdn-number-input-buttons > button { background: linear-gradient(#fafafa, #dedede);
/// border-left: 1px solid #a0a0a0 }` (`:first-child` also `border-bottom`), `:hover
/// linear-gradient(#f5fbff, #cce8f9)`, `:active #b8dcf2`, `:disabled linear-gradient(#eee,
/// #d5d5d5)`; the `::after` arrows `#555`.
pub const NUMBER_SPIN_GRADIENT: (Color32, Color32) = (rgb(0xFAFAFA), rgb(0xDEDEDE));
pub const NUMBER_SPIN_HOVER_GRADIENT: (Color32, Color32) = (rgb(0xF5FBFF), rgb(0xCCE8F9));
pub const NUMBER_SPIN_DISABLED_GRADIENT: (Color32, Color32) = (rgb(0xEEEEEE), rgb(0xD5D5D5));
pub const NUMBER_SPIN_ACTIVE: Color32 = rgb(0xB8DCF2);
pub const NUMBER_SPIN_BORDER: Color32 = rgb(0xA0A0A0);
pub const NUMBER_SPIN_ARROW: Color32 = rgb(0x555555);
/// `input[type="range"]::-webkit-slider-thumb { width: 10px; height: 14px }`.
pub const SLIDER_THUMB: (f32, f32) = (10.0, 14.0);

// ---- Tool option popups (css/header.css `.tool-dropdown-menu*`; body-level) ----
// paintdotjs sizes them for its 125 % header: rule 2 divides
// their lengths by 1.25 (borders, the shadow and the size menu's scrollbar keep theirs) and sets
// their text to 12 DIP (rule 5).

/// `.tool-dropdown-menu { border: 1px solid var(--pdn-menu-border); box-shadow: 2px 2px 4px
/// rgba(0, 0, 0, 0.7); font-size: 15px }` (`.tool-dropdown-menu-text` borders in
/// `--color-menu-item-selected-border`); patched 12px.
pub const TOOL_POPUP_SHADOW: (f32, f32, Color32) = (2.0, 4.0, rgba(0, 0, 0, 0.7));
pub const TOOL_POPUP_FONT_SIZE: f32 = 12.0;
/// `.tool-dropdown-menu-text button { height: 29px; padding: 0 6px }`; patched 23.2, 4.8.
pub const TOOL_TEXT_ENTRY: (f32, f32) = (23.2, 4.8);
/// `.tool-dropdown-menu-checkmarks { min-width: 174px }` with a 30px gutter and its divider;
/// `button { gap: 8px; height: 31px; padding: 0 8px 0 2px }`, `.tool-dropdown-check { 26x27;
/// font-size: 18px }`, `[active]`: `linear-gradient(#246ca5, #1d5c90)`, `border: 1px solid
/// #0b73bd`, `box-shadow: inset 0 0 0 1px rgba(104, 183, 235, .22)`; patched 139.2, 24, gap 6.4,
/// 24.8, `0 6.4px 0 1.6px`, 20.8x21.6 (14.4px), and the "\2713" replaced by the 16-DIP
/// `tool_strip_checked.png`.
pub const TOOL_CHECK_MIN_WIDTH: f32 = 139.2;
pub const TOOL_CHECK_GUTTER: f32 = 24.0;
pub const TOOL_CHECK_ENTRY: (f32, f32, f32, f32) = (24.8, 6.4, 1.6, 6.4); // h, gap, pl, pr
pub const TOOL_CHECK_CELL: (f32, f32) = (20.8, 21.6); // w, h
pub const TOOL_CHECK_ICON: f32 = 16.0;
pub const TOOL_CHECK_ACTIVE: (Color32, Color32) = (rgb(0x246CA5), rgb(0x1D5C90));
pub const TOOL_CHECK_ACTIVE_BORDER: Color32 = rgb(0x0B73BD);
pub const TOOL_CHECK_ACTIVE_INSET: Color32 = rgba(104, 183, 235, 0.22);
/// `.tool-size-dropdown-menu { max-height: min(450px, calc(100vh - 40px)); scrollbar-color:
/// --pdn-scrollbar-thumb --pdn-scrollbar-track }`, `button { height: 24px; padding: 0 5px;
/// line-height: 22px }` (patched 360/32, 19.2, 4, 17.6); ToolMenu.toggleSizeMenu: `minWidth =
/// round(combo width * 1.5)`, drops below when `min(scrollHeight, 220)` fits.
pub const TOOL_SIZE_ENTRY: (f32, f32, f32) = (19.2, 4.0, 17.6); // h, padding x, line-h
pub const TOOL_SIZE_MENU_MAX: (f32, f32) = (360.0, 32.0); // px, vh inset
pub const TOOL_SIZE_MENU_WIDTH_FACTOR: f32 = 1.5;
pub const TOOL_SIZE_MENU_DROP_MIN: f32 = 220.0;
/// `.tool-shape-grid-menu { box-sizing: border-box; width: 338px; max-height: calc(100vh -
/// 76px) }`, `.tool-shape-grid-heading { height: 26px; padding: 3px 5px; font: 600 14px;
/// background: var(--pdn-menu-gutter) }`, `.tool-shape-grid { repeat(8, 38px); gap: 2px;
/// padding: 4px 7px }`, buttons `38x38; padding: 3px` with 30px images, `:hover, [active] {
/// selected fill and border; box-shadow: inset 0 0 0 1px rgba(195, 228, 255, .28) }`; patched
/// 270.4, 60.8, 20.8 (4px, 12px), 30.4/1.6/3.2/5.6, 24.
pub const TOOL_SHAPE_MENU_WIDTH: f32 = 270.4;
pub const TOOL_SHAPE_MENU_MAX_INSET: f32 = 60.8;
pub const TOOL_SHAPE_HEADING: (f32, f32, f32) = (20.8, 4.0, 12.0); // height, padding x, font
pub const TOOL_SHAPE_GRID: (f32, f32, f32, f32) = (30.4, 1.6, 3.2, 5.6); // cell, gap, py, px
pub const TOOL_SHAPE_COLUMNS: usize = 8;
pub const TOOL_SHAPE_IMAGE: f32 = 24.0;
pub const TOOL_SHAPE_INSET: Color32 = rgba(195, 228, 255, 0.28);
/// `.tool-font-dropdown-menu { width: min(380px, 100vw - 12px); max-height: min(520px, 100vh -
/// 130px) }`, `button { grid: minmax(105px, 155px) minmax(160px, 1fr); gap: 12px }` (patched
/// 304/9.6/416/104, 124, 9.6), `.tool-font-dropdown-preview { font-size: 17px; line-height: 1 }`
/// (unpatched: not a 125 % length).
pub const TOOL_FONT_MENU: (f32, f32, f32, f32) = (304.0, 9.6, 416.0, 104.0);
pub const TOOL_FONT_COLUMNS: (f32, f32, f32) = (124.0, 9.6, 17.0); // label max, gap, preview px
pub const TOOL_FONT_PREVIEW_TEXT: &str = "The quick brown fox";

// ---- Floating forms (css/window.css) ----
// Rule 3: form lengths map 1:1, except the rule 4 rows and icons and the rule 5 text below.

/// `.window { border: 1px solid var(--color-window-border); box-shadow: 0 0 20px rgba(0, 0, 0,
/// 0.2) }` (`width`/`height` set the content box).
pub const WINDOW_BORDER: f32 = 1.0;
pub const WINDOW_SHADOW_BLUR: f32 = 20.0;
pub const WINDOW_SHADOW_COLOR: Color32 = rgba(0, 0, 0, 0.2);
/// `.window .title-bar { height: 20px; margin-top: 1px; padding: 5px 1px 5px 5px; background:
/// var(--color-title-bar) }` (31 px with the margin); "Consistent form density" `.title {
/// max-width: calc(100% - 28px) }` and `.window .window-close-button { 24×24; font-size: 14px }`
/// (css/window.css `.window-close-button { padding: 0 0 1px; font: bold 'Arial' }`; Arial is
/// Noto Sans here, `DOCUMENT_CLOSE_FAMILY`).
pub const WINDOW_TITLE_HEIGHT: f32 = 20.0;
pub const WINDOW_TITLE_MARGIN_TOP: f32 = 1.0;
pub const WINDOW_TITLE_PADDING: (f32, f32, f32) = (5.0, 1.0, 5.0); // y, right, left
pub const WINDOW_TITLE_RESERVE: f32 = 28.0;
pub const WINDOW_CLOSE: f32 = 24.0;
pub const WINDOW_CLOSE_FONT: f32 = 14.0;
/// `.window-resize-n/s { left/right: 7px; height: 7px }`, `-e/w { top/bottom: 7px; width: 7px }`,
/// the corners 11×11, all 4 px outside the padding box (`top: -4px` …).
pub const WINDOW_RESIZE_EDGE: f32 = 7.0;
pub const WINDOW_RESIZE_CORNER: f32 = 11.0;
pub const WINDOW_RESIZE_OUTSET: f32 = 4.0;
/// WebWindow.js's default `minWidth = 120; minHeight = 90` (a `resizeToPointer` minimum when
/// `setResizable` set none).
pub const WINDOW_DEFAULT_MIN: (f32, f32) = (120.0, 90.0);
/// WebWindow.js's `getViewBounds` margin (`margin = 10`): only to place a saved paintdotjs
/// anchor once, when `windows.<id>` migrates to a snap description (`floating-windows.md` §5.3).
pub const WINDOW_VIEW_MARGIN: f32 = 10.0;
/// `floating-windows.md` §2.4 step 4 (controller ruling, R19): the visible frame sits ≈ 5.6 DIP
/// inside W (≈ 7 px measured at 125 %); the default snap offset.
// calibrate: B-W4 — measured on Paint.NET at 125 %, not derived (OQ5).
pub const WINDOW_INSET: f32 = 5.6;
/// `floating-windows.md` §2.6: W edges and other forms' outer edges snap within 15 DIP
/// (snap proximity), doubled after a form snap in the same pass; forms keep
/// paintdotjs's `threshold / 2` gap from each other (OQ4).
pub const WINDOW_SNAP: f32 = 15.0;
/// A translucent floating window's opacity, and the fades to opaque and to translucent (s):
/// `ui-drawn-graphics.md` "Addendum (controller, 2026-10-19)" (floating tool windows 0.75; 0.15 s
/// and 0.5 s); paintdotjs's css/theme.css `.window.window-over-image { opacity: .88 }` is
/// patched to .75 and to Paint.NET's condition.
pub const WINDOW_TRANSLUCENT_OPACITY: f32 = 0.75;
pub const WINDOW_FADE: (f32, f32) = (0.15, 0.5); // to opaque, to translucent
/// `floating-windows.md` §2.9 step 4, §5.6: an inactive app's caption title and close glyph,
/// #7F7F7F in the dark schemes, #999999 in the light one.
// calibrate: B-W3 — measured on blurred captures, light theme from Paint.NET's own rule (OQ3).
pub const WINDOW_INACTIVE_TEXT: (Color32, Color32) = (rgb(0x7F7F7F), rgb(0x999999)); // dark, light
/// How far past the image Paint.NET's canvas differs from paintdotjs's by design: the wider drop
/// shadow (paintdotjs's `shadowBlur = 4.5`, a σ 2.25 blur reaching 3σ ≈ 7 px,
/// SurfaceBoxBaseRenderer.js:39; Paint.NET's ⌈4.5u⌉, `canvas-rendering.md §2.7`). A translucent
/// window over the image and this band shows Paint.NET's pixels.
pub const CANVAS_SHADOW_REACH: f32 = 7.0;
/// Rule 5: form text (titles, list names, history items) is 12 DIP.
pub const FORM_FONT_SIZE: f32 = 12.0;

/// ToolForm.js `setSize(64, 331)` for its 19 tools; "Consistent form density" `#toolForm
/// .menu-item { min-width: 30px; height: 30px; padding: 4px; box-sizing: border-box }` (`margin:
/// 0`, `flex-grow: 1`), icons 20×20 (the Shapes tool's `.large-icon` keeps its aspect). Each
/// further pair of `shown` tools (two buttons to a row) adds a row: the six Extra tools, three
/// (S§4.1).
pub fn tool_form_size(shown: usize) -> (f32, f32) {
    let rows = shown.saturating_sub(19).div_ceil(2);
    (64.0, 331.0 + rows as f32 * TOOL_FORM_BUTTON.1)
}
/// ToolForm.js: the Shapes tool's `.large-icon` (`assets/icons/shapes_tool_icon.png`, 164×80) at
/// `height: 20px; width: auto` ("Consistent form density").
pub const TOOL_FORM_LARGE_ICON: (f32, f32) = (41.0, 20.0);
/// ColorsForm.js `updateWindowSize` (compact: `setSize(270, 315)`); Task 20 owns the form (and
/// its expanded 492×371).
pub const COLORS_FORM_SIZE: (f32, f32) = (270.0, 315.0);
/// ColorsForm.js `updateWindowSize` (expanded: `setSize(492, 371)`).
pub const COLORS_FORM_EXPANDED_SIZE: (f32, f32) = (492.0, 371.0);
/// css/window.css `#colorsForm #currentColors { left: 8px; top: 16px; 66×66 }`.
pub const COLORS_CURRENT: (f32, f32, f32) = (8.0, 16.0, 66.0); // x, y, side
/// `#colorsForm .color-preview { 39×39 }` (window.css:3510) with css/component.css's `border: 1px`
/// and `outline: 1px`: the 43×43 box Paint.NET's 35-DIP rectangle fills (§2.5, its rings drawn
/// where paintdotjs draws the outline and border); `#mainColor { left: 1px; top: 1px }`,
/// `#secondaryColor { right: 1px; bottom: 1px }`, so the secondary's box is 23 px in.
pub const COLOR_PREVIEW: f32 = 43.0;
pub const COLOR_PREVIEW_SECONDARY: f32 = 23.0;
/// Rule 4: the swap (top-right) and black-and-white (bottom-left) icons are 16 DIP.
pub const COLORS_SWAP_ICON: f32 = 16.0;
/// `#colorsForm #moreLessButton { left: 160px; top: 15px; width: 92px; min-height: 27px }`.
pub const COLORS_MORE_LESS: (f32, f32, f32, f32) = (160.0, 15.0, 92.0, 27.0); // x, y, w, h
/// `#colorsForm .color-circle { left: 72px; top: 48px; 180×180 }` (the canvas size, Ruling K-Q1).
pub const COLOR_WHEEL_BOX: (f32, f32, f32) = (72.0, 48.0, 180.0); // x, y, side
/// css/component.css `#colorsForm .color-circle .cursor { 6×6; border: 1px; outline: 1px;
/// translate(-50%, -50%) }`, centred on the colour's point of the 180-px circle: it reaches 5 px
/// past the box where the circle meets the box's edges (Paint.NET's nub stays inside, §2.3).
pub const COLOR_WHEEL_CURSOR_REACH: f32 = 5.0;
/// `#colorsForm #colorSettingsStrip { left: 8px; top: 205px }`: `#colorAdd` (22×22 + `padding:
/// 1px` + the `.menu-item` border = 26×26) with its 20-px `.color` box and icon 3 px in, then the
/// `#swatch` selector (44×26: icon 3 px in, `.separator` 2×14 at (27, 6), `.arrow-down` 6×3 at
/// (33, 11.5)).
pub const COLORS_STRIP: (f32, f32) = (8.0, 205.0);
pub const COLORS_ADD_ITEM: f32 = 26.0;
pub const COLORS_ADD_ICON: (f32, f32) = (3.0, 20.0); // inset, side
pub const COLORS_SWATCH_SELECTOR: (f32, f32) = (44.0, 26.0);
pub const COLORS_SWATCH_SEPARATOR: (f32, f32, f32, f32) = (27.0, 6.0, 2.0, 14.0); // x, y, w, h
pub const COLORS_SWATCH_ARROW: (f32, f32) = (33.0, 11.5);
/// `#colorsForm .color-palette { left: 8px; width: 240px }`, `#basicColorPalette { top: 239px }`
/// (`#extendedColorPalette { top: 269px }` follows it), `.color { 15×15 }`; 16 columns.
pub const COLORS_PALETTE: (f32, f32, f32, f32) = (8.0, 239.0, 240.0, 15.0); // x, y, w, cell
/// `#colorsForm #sliderPanel { left: 280px; top: 10px; width: 196px }`: `.entry { height: 29px }`
/// (`.header { height: 25px; gap: 5px }`), the 25-px `.row` of a channel, `.text { min-width:
/// 25px; line-height: 1 }`, the slider 33 px in (label 25 + `justify-content: space-between`),
/// the 68×25 inputs at the right; the `hr` divider (`border: 1px` → 2 px tall, UA margins 0.5em)
/// 11.5 px down the header.
pub const COLORS_SLIDER_PANEL: (f32, f32, f32) = (280.0, 10.0, 196.0); // x, y, w
pub const COLORS_PANEL_HEADER: f32 = 25.0;
pub const COLORS_PANEL_ENTRY: f32 = 29.0;
pub const COLORS_PANEL_ROW: f32 = 25.0;
pub const COLORS_PANEL_LABEL: f32 = 25.0;
pub const COLORS_PANEL_GAP: f32 = 5.0;
pub const COLORS_PANEL_SLIDER_X: f32 = 33.0;
pub const COLORS_PANEL_INPUT: (f32, f32) = (68.0, 25.0);
pub const COLORS_PANEL_DIVIDER: (f32, f32) = (11.5, 2.0); // y, height
/// css/component.css `.color-slider { 85×25 }`, `.canvas { top: 7px }` (85×10).
pub const COLOR_SLIDER: (f32, f32) = (85.0, 25.0);
pub const COLOR_SLIDER_STRIP: (f32, f32) = (7.0, 10.0); // top, height
/// `.color-slider .cursor { top: 17px; 4×5 + padding 2px 2px 1px; translateX(-50%) }`: an 8×8
/// triangle (top, half width, height), its `::before` inset by (x, top, bottom).
pub const COLOR_SLIDER_NUB: (f32, f32, f32) = (17.0, 4.0, 8.0);
pub const COLOR_SLIDER_NUB_PADDING: (f32, f32, f32) = (2.0, 2.0, 1.0);
/// Windows system colours Paint.NET's slider nubs use (`ui-drawn-graphics.md §2.4` step 5, §6
/// Q10 Win10/11 defaults): WindowText, Window, Highlight, HighlightText.
pub const SYSTEM_WINDOW_TEXT: Color32 = rgb(0x000000);
pub const SYSTEM_WINDOW: Color32 = rgb(0xFFFFFF);
pub const SYSTEM_HIGHLIGHT: Color32 = rgb(0x0078D7);
pub const SYSTEM_HIGHLIGHT_TEXT: Color32 = rgb(0xFFFFFF);
pub const TOOL_FORM_BUTTON: (f32, f32) = (30.0, 30.0); // min width, height
pub const TOOL_FORM_BUTTON_PADDING: f32 = 4.0;
/// paintdotjs's 20 px: the SVG icons rasterize at the size they are drawn (`Assets::icon_image_sized`),
/// so they stay sharp here, where stretching their 16-px rendering blurred them.
pub const TOOL_FORM_ICON: f32 = 20.0;
/// S§4.1: the plus at an Extra tool button's top-right (6×6, 2-px arms, inset 2 px), gray as
/// `plugin_indicator.svg` (user, 2026-10-24). One flat mid gray between its fill (#AAAAAA, too
/// light on the light window) and edge (#727272, dim on the dark one): about 3.2–3.6:1 on the
/// dark, blue and light windows and their hover fills.
pub const TOOL_FORM_EXTRA_PLUS: Color32 = rgb(0x7D7D7D);
/// HistoryForm.js `setSize(200, 230)`; the minimum is the default size (`floating-windows.md`
/// §2.1, §2.7; controller ruling, R19; paintdotjs: 140×100).
pub const HISTORY_FORM_SIZE: (f32, f32) = (200.0, 230.0);
pub const HISTORY_FORM_MIN: (f32, f32) = HISTORY_FORM_SIZE;
/// Rule 4 (`#historyForm #historyList .history-item { height: 20px }`,
/// `.icon { 16px }`): row 20 (border box, `margin:
/// 0`), icon 16 with css/window.css's `margin-right: 5px`.
pub const HISTORY_ROW: f32 = 20.0;
pub const HISTORY_ICON: f32 = 16.0;
pub const HISTORY_ICON_GAP: f32 = 5.0;
/// LayerForm.js `setSize(200, 276)`; the minimum is the default size (`floating-windows.md` §2.1,
/// §2.7; controller ruling, R19; paintdotjs: 180×120).
pub const LAYER_FORM_SIZE: (f32, f32) = (200.0, 276.0);
pub const LAYER_FORM_MIN: (f32, f32) = LAYER_FORM_SIZE;
/// Rule 4 (`#layerForm #layerList .layer-item { min-height: 54px }`,
/// `.thumbnail { margin-left: 2px }`, LayerItem.js's thumbnail at 50; controller ruling on Task 19
/// concern 3): Paint.NET's row is 62 in all (the list pitch), its image box the row inset by the
/// 4-DIP pad (54), the thumbnail at most 50 DIPs centred in it. The item's margin (1), border (1)
/// and `.scroll-item` padding (2) make the pad, so the box is the item's content box;
/// css/window.css `.thumbnail { margin-right: 5px }` puts the name after the 50-DIP column.
pub const LAYER_ROW: f32 = 62.0;
pub const LAYER_THUMB_PAD: f32 = 4.0;
pub const LAYER_THUMB_BOX: f32 = LAYER_ROW - 2.0 * LAYER_THUMB_PAD;
pub const LAYER_THUMB_MAX: f32 = 50.0;
pub const LAYER_THUMB_GAP: f32 = 5.0;
pub const LAYER_ITEM_MARGIN: f32 = 1.0;
/// css/component.css `.scroll-list .scroll-content .scroll-item { padding: 2px }`;
/// `.dragging-item { box-shadow: 0 4px 10px rgb(0 0 0 / 45%); opacity: 0.92 }`.
pub const SCROLL_ITEM_PADDING: f32 = 2.0;
pub const DRAGGING_ITEM_SHADOW: (f32, f32, Color32) = (4.0, 10.0, rgba(0, 0, 0, 0.45)); // y, blur
pub const DRAGGING_ITEM_OPACITY: f32 = 0.92;
/// `#historyStripPanel`, `#layerStripPanel { padding: 2px; border-top: 1px solid
/// var(--color-title-bar-border); background: var(--color-menu-background) }`; rule 4:
/// `.icon-item { 16px }` (Paint.NET's 16-DIP strip icons), so each item is 22.
pub const FORM_STRIP_PADDING: f32 = 2.0;
pub const FORM_STRIP_ICON: f32 = 16.0;
/// css/component.css `input[type="checkbox"] { 12×12; border-radius: 2px; outline: 1px }` with
/// the UA's `margin: 3px 3px 3px 4px`.
pub const CHECKBOX_SIZE: f32 = 12.0;
pub const CHECKBOX_RADIUS: f32 = 2.0;
pub const CHECKBOX_MARGIN: (f32, f32, f32) = (3.0, 3.0, 4.0); // y, right, left

// ---- Modal dialogs (css/window.css `.app-dialog*`) ----
// Rule 3: dialog lengths map 1:1; rule 5: their text is 12 DIP.

/// Rule 5 (Paint.NET's dialogs use SystemFonts.MenuFont, 12 DIP): every `.app-dialog` text, its buttons and inputs, and the
/// paintdotjs selectors of the "Dialogs use the same typography" block (paintdotjs: 14 px).
pub const DIALOG_FONT_SIZE: f32 = 12.0;
/// `.task-dialog-choice strong { 15px }` — Paint.NET's command buttons draw their action text at
/// 1.25 × the 12-DIP font.
pub const TASK_CHOICE_TITLE_SIZE: f32 = 15.0;
/// The dialogs' widths are `min(W, 100vw − 32px)` (`.app-dialog-backdrop` centres them).
pub const DIALOG_VIEWPORT_MARGIN: f32 = 32.0;
/// `.app-dialog { border: 1px solid var(--color-title-bar-border); background:
/// var(--color-window-background); box-shadow: 0 12px 38px rgba(0, 0, 0, .65) }`.
pub const DIALOG_BORDER: f32 = 1.0;
pub const DIALOG_SHADOW: (f32, f32, Color32) = (12.0, 38.0, rgba(0, 0, 0, 0.65)); // offset y, blur
/// `.app-dialog-title-bar { height: 31px (border-box); padding-left: 8px; border-bottom: 1px
/// solid var(--color-window-border) }`, `.app-dialog-title { gap: 7px }` with the typography
/// block's 20-px `img`, `strong { font-weight: var(--font-weight) }` (500: Selawik Regular).
pub const DIALOG_TITLE_BAR: f32 = 31.0;
pub const DIALOG_TITLE_PADDING_LEFT: f32 = 8.0;
pub const DIALOG_TITLE_GAP: f32 = 7.0;
pub const DIALOG_TITLE_ICON: f32 = 20.0;
/// `.app-dialog-close { width: 46px; height: 30px; padding: 0 0 2px }`: the box; the × is
/// `aa::paint_close_x`. `:hover` `--color-close-button-hover` behind `--color-close-button-text`.
pub const DIALOG_CLOSE: (f32, f32) = (46.0, 30.0);
pub const DIALOG_CLOSE_PADDING_BOTTOM: f32 = 2.0;
/// `.app-dialog-footer { min-height: 45px; padding: 8px 10px; background:
/// var(--color-menu-background); border-top: 1px solid var(--color-window-border) }`.
pub const DIALOG_FOOTER_HEIGHT: f32 = 45.0;
pub const DIALOG_FOOTER_PADDING: (f32, f32) = (8.0, 10.0); // y, x
/// `.app-dialog-footer button { min-width: 88px; min-height: 26px; padding: 3px 12px }` (the
/// global `button`'s 1-px border).
pub const DIALOG_BUTTON_MIN: (f32, f32) = (88.0, 26.0);
pub const DIALOG_BUTTON_PADDING_X: f32 = 12.0;
/// A focused primary button: `outline: 2px solid var(--color-menu-item-selected-border);
/// outline-offset: 1px` (`.project-info-primary-button:focus-visible`).
pub const DIALOG_FOCUS_RING: (f32, f32) = (2.0, 1.0); // width, offset
/// ModalDialogController.signalAttention: `modal-dialog-attention-*` run 160 ms × 5 (`ease-in-out`
/// per keyframe pair; at 50 % no shadow and a #606060 title); a new flash waits 250 ms; the class
/// goes after 850 ms.
pub const DIALOG_ATTENTION_MS: (f64, u32, f64, f64) = (160.0, 5, 250.0, 850.0); // period, runs, throttle, clear
pub const DIALOG_ATTENTION_TITLE: Color32 = rgb(0x606060);

/// `.task-dialog { width: min(390px, …) }`, `.task-dialog-content { grid: auto 1fr; gap: 12px;
/// padding: 12px }`, `> .task-dialog-message { margin: 0 0 2px; font-weight: 600 }`,
/// `.task-dialog-choices { gap: 4px }`.
pub const TASK_DIALOG_WIDTH: f32 = 390.0;
pub const TASK_DIALOG_PADDING: f32 = 12.0;
pub const TASK_MESSAGE_MARGIN_BOTTOM: f32 = 2.0;
pub const TASK_CHOICES_GAP: f32 = 4.0;
/// ColorsForm.js `saveCurrentPaletteAs`'s preview: the input's inline `margin-left: 10px;
/// width: 260px`.
pub const TASK_FIELD_INPUT: (f32, f32) = (10.0, 260.0); // margin, width
/// `.task-dialog-choice { min-height: 54px; grid: 25px 1fr; gap: 8px; padding: 7px 9px; border:
/// 1px }` (`:hover` `--color-menu-item-hover` / `--color-menu-item-border`), `img { 20px;
/// margin-top: 2px }`, `> span { gap: 2px }`, `span span { line-height: 1.35 }`.
pub const TASK_CHOICE_MIN_HEIGHT: f32 = 54.0;
pub const TASK_CHOICE_COLUMN: f32 = 25.0;
pub const TASK_CHOICE_GAP: f32 = 8.0;
pub const TASK_CHOICE_PADDING: (f32, f32) = (7.0, 9.0); // y, x
pub const TASK_CHOICE_ICON: (f32, f32) = (20.0, 2.0); // side, margin-top
pub const TASK_CHOICE_COPY_GAP: f32 = 2.0;
pub const TASK_CHOICE_LINE_HEIGHT: f32 = 1.35;
/// `.clone-stamp-warning-dialog` (css/window.css; Paint.NET's compact error box, Task 31):
/// `width: min(440px, …)`; `.task-dialog-content { grid: 42px 1fr; column-gap: 14px; padding:
/// 14px 12px 0 }`; its `::before` disc (36 px, `margin-top: 1px`, `#ef4b0b`, a white "×" in
/// `700 27px/1 Arial`, Noto Sans here); the message in `--color-link`,
/// `line-height: 1.35`, 12 px; `.task-dialog-choices { margin: 14px -12px
/// 0; padding: 10px 14px; border-top: 1px solid var(--color-dialog-border) }` with the choices
/// right-aligned; `.task-dialog-choice { width: 82px; min-height: 28px; padding: 3px 12px;
/// border: 1px; border-radius: 3px }` (`--color-button*`; `:hover, :focus-visible` border
/// `--color-menu-item-selected-border`), its text 9pt = 12 px.
pub const ERROR_BOX_WIDTH: f32 = 440.0;
pub const ERROR_BOX_PADDING: (f32, f32) = (14.0, 12.0); // top, x
pub const ERROR_BOX_COLUMN: (f32, f32) = (42.0, 14.0); // icon column, gap
pub const ERROR_BOX_ICON: (f32, f32) = (36.0, 1.0); // side, margin-top
pub const ERROR_BOX_ICON_FILL: Color32 = rgb(0xEF4B0B);
pub const ERROR_BOX_GLYPH: f32 = 27.0;
pub const ERROR_BOX_CHOICES: (f32, f32, f32) = (14.0, 10.0, 14.0); // margin-top, padding y, x
pub const ERROR_BOX_BUTTON: (f32, f32, f32) = (82.0, 28.0, 3.0); // width, min height, radius

// ---- Effect Config dialog (css/window.css "Property-based effect dialogs" and "Paint.NET-style
// specialized adjustment dialogs"; js/src/interface/window/dialog/EffectConfigDialog.js) ----

/// `.effect-config-dialog { width: min(390px, 100vw − 32px) }`; the specialized dialogs `min(W,
/// 100vw − 24px)`: `.effect-hueSaturation-dialog` 446, `.effect-rotateZoom-dialog` 440,
/// `.effect-curves-dialog` 387, `.effect-levels-dialog` 580.
pub const EFFECT_DIALOG_WIDTH: f32 = 390.0;
pub const EFFECT_SPECIAL_MARGIN: f32 = 24.0;
pub const EFFECT_HUE_WIDTH: f32 = 446.0;
pub const EFFECT_ROTATE_WIDTH: f32 = 440.0;
pub const EFFECT_CURVES_WIDTH: f32 = 387.0;
pub const EFFECT_LEVELS_WIDTH: f32 = 580.0;
/// `.effect-config-content { min-height: 70px; max-height: min(640px, 100vh − 110px); gap: 10px;
/// padding: 11px 12px }` (content-box heights) and the specialized paddings (top, x, bottom):
/// hue/saturation 16 11 10, rotate/zoom 11 10 6, curves 16 10 7, levels 14 10 7.
pub const EFFECT_CONTENT_PADDING: (f32, f32, f32) = (11.0, 12.0, 11.0);
pub const EFFECT_CONTENT_HEIGHT: (f32, f32, f32) = (70.0, 640.0, 110.0); // min, max, viewport inset
pub const EFFECT_CONTENT_GAP: f32 = 10.0;
pub const EFFECT_HUE_PADDING: (f32, f32, f32) = (16.0, 11.0, 10.0);
pub const EFFECT_ROTATE_PADDING: (f32, f32, f32) = (11.0, 10.0, 6.0);
pub const EFFECT_CURVES_PADDING: (f32, f32, f32) = (16.0, 10.0, 7.0);
pub const EFFECT_LEVELS_PADDING: (f32, f32, f32) = (14.0, 10.0, 7.0);
/// `.effect-config-row { gap: 4px }`, `> label { padding-bottom: 3px; border-bottom: 1px solid
/// var(--pdn-separator) }`, `> div { grid: 1fr 72px 24px; gap: 10px; min-height: 28px }`; the UA's
/// `input[type=range] { margin: 2px }`; `.effect-config-select-row select { height: 25px }`.
pub const EFFECT_ROW_GAP: f32 = 4.0;
pub const EFFECT_LABEL_PADDING: f32 = 3.0;
pub const EFFECT_LINE_HEIGHT: f32 = 28.0;
pub const EFFECT_LINE_COLUMNS: (f32, f32, f32) = (72.0, 24.0, 10.0); // number, reset, gap
pub const EFFECT_RANGE_MARGIN: f32 = 2.0;
pub const EFFECT_SELECT_HEIGHT: f32 = 25.0;
/// `.effect-config-row input[type=number] { height: 25px; padding-right: 5px }` (component.css
/// `padding: 1px 3px` otherwise).
pub const EFFECT_NUMBER_HEIGHT: f32 = 25.0;
pub const EFFECT_NUMBER_PADDING_RIGHT: f32 = 5.0;
/// `.effect-config-row input[type=range]::-webkit-slider-runnable-track { height: 4px }` and the
/// thumb `width: 14px; height: 23px; clip-path: polygon(0 0, 100% 0, 100% 72%, 50% 100%, 0 72%)`.
pub const EFFECT_TRACK_HEIGHT: f32 = 4.0;
pub const EFFECT_THUMB: (f32, f32, f32) = (14.0, 23.0, 0.72); // width, height, shoulder
/// `EffectConfigDialog.enableSliderThumbReset`: two presses on the thumb within 500 ms, the thumb
/// hit `thumbWidth / 2 + 2` around its centre.
pub const EFFECT_THUMB_RESET: (f64, f32) = (500.0, 2.0);
/// `.effect-row-reset { 24×24; border: 1px }`, its `img { 16×16; margin: auto }`.
pub const EFFECT_RESET: (f32, f32) = (24.0, 16.0);
/// `.effect-config-checkbox { gap: 7px; min-height: 24px }`, `input { 16×16; margin: 0 }`.
pub const EFFECT_CHECKBOX: (f32, f32, f32) = (16.0, 7.0, 24.0); // box, gap, min height
/// `.effect-config-footer > div { gap: 9px }`, `button { min-width: 88px }`; the curves and
/// levels footers' `min-width: 104px`.
pub const EFFECT_FOOTER_GAP: f32 = 9.0;
pub const EFFECT_WIDE_BUTTON: f32 = 104.0;
/// `.effect-special-slider { gap: 7px }` (Rotate/Zoom's own rows 6), its `label { 12px; 600 }`;
/// `-line { grid: 1fr 88px 24px; gap: 10px }` (Rotate/Zoom 9; Hue/Saturation `margin-left: 8px`);
/// `input[type=number] { border: 2px solid var(--pdn-control-border); padding: 1px 3px }` in the
/// 88-px `.pdn-number-input`.
pub const EFFECT_SPECIAL_GAP: (f32, f32) = (7.0, 6.0);
pub const EFFECT_SPECIAL_NUMBER: f32 = 88.0;
pub const EFFECT_SPECIAL_LINE_GAP: (f32, f32) = (10.0, 9.0);
pub const EFFECT_SPECIAL_NUMBER_BORDER: f32 = 2.0;
pub const EFFECT_HUE_GAP: (f32, f32) = (18.0, 8.0); // rows, line margin-left
/// The hue/saturation tracks (`.hue-saturation-{hue,saturation,lightness}`): CSS gradients, evenly
/// spaced stops.
pub const EFFECT_HUE_TRACK: [u32; 7] = [
    0xff0000, 0xffff00, 0x00ff00, 0x00ffff, 0x0000ff, 0xff00ff, 0xff0000,
];
pub const EFFECT_SATURATION_TRACK: [u32; 5] = [0xaaaaaa, 0xddb98c, 0xb3d7d7, 0xf7a2cc, 0xf0ff51];
pub const EFFECT_LIGHTNESS_TRACK: [u32; 2] = [0x111111, 0xffffff];
/// `.rotate-zoom-content { gap: 16px }`, `section { gap: 14px }`, `section:nth-child(2) {
/// margin-bottom: 18px }`, `-group-body { grid: 100px 1fr; gap: 14px }`, `-sliders { gap: 9px }`;
/// the 98×98 roll canvas, the 100×76 pan canvas (1-px border); `.effect-special-select { gap:
/// 8px; 600 }`, `select { height: 31px; padding: 3px 30px 3px 8px; min-width: 140px }`.
pub const EFFECT_ROTATE_GAPS: (f32, f32, f32, f32, f32) = (16.0, 14.0, 18.0, 14.0, 9.0);
pub const EFFECT_ROLL_SIZE: f32 = 98.0;
pub const EFFECT_PAN_SIZE: (f32, f32) = (100.0, 76.0);
pub const EFFECT_ROTATE_COLUMN: f32 = 100.0;
pub const EFFECT_SPECIAL_SELECT: (f32, f32, f32, f32, f32) = (8.0, 31.0, 8.0, 30.0, 140.0); // gap, height, pad l, pad r, min w
/// Chromium's menulist: 4 px inside the CSS padding before the text; the dark arrow's 10×7
/// coverage (measured from the references, white on the field), its left edge 14 px inside the
/// right border edge, centred vertically; the native field (no author background): `#3b3b3b`
/// with a `#858585` border whose corner pixels are cut.
pub const SELECT_TEXT_INSET: f32 = 4.0;
/// The menulist's intrinsic width: the widest option (rounded up) plus 20 px of Chromium's inner
/// padding (4 before the text, 16 after it), the CSS padding and border (measured: "Nearest
/// Neighbor" 98.55 px → 159 px).
pub const SELECT_INNER_PADDING: f32 = 20.0;
pub const SELECT_ARROW_RIGHT: f32 = 14.0;
pub const SELECT_ARROW: [[u8; 10]; 7] = [
    [0, 113, 23, 0, 0, 0, 0, 23, 112, 0],
    [104, 255, 219, 25, 0, 0, 25, 219, 255, 100],
    [23, 222, 255, 219, 23, 23, 219, 255, 219, 23],
    [0, 27, 220, 255, 221, 221, 255, 220, 25, 0],
    [0, 0, 29, 221, 255, 255, 220, 27, 0, 0],
    [0, 0, 0, 27, 226, 222, 27, 0, 0, 0],
    [0, 0, 0, 0, 33, 29, 0, 0, 0, 0],
];
pub const SELECT_NATIVE: (Color32, Color32) = (rgb(0x3B3B3B), rgb(0x858585)); // fill, border
/// The native field in a light theme (Chromium's light `<select>`): its text is the theme's dark
/// text, which the dark field hid.
pub const SELECT_NATIVE_LIGHT: (Color32, Color32) = (rgb(0xFFFFFF), rgb(0x767676));

// ---- Form controls of the size and layer dialogs (Task 44; measured from the references) ----

/// `.pdn-number-input:has(input:disabled) { opacity: .55 }` (css/component.css).
pub const NUMBER_DISABLED_OPACITY: f32 = 0.55;
/// `.new-file-row input[type=number] { padding: 2px 5px }`; `.image-size-percent`,
/// `.image-size-row input[type=number] { padding: 2px 4px }`.
pub const NEW_FILE_NUMBER_PADDING: (f32, f32) = (2.0, 5.0); // y, x
pub const IMAGE_SIZE_NUMBER_PADDING: (f32, f32) = (2.0, 4.0); // y, x

/// `.image-size-radio input[type=radio] { 16×16; background: #f5f5f5; border: 1px solid
/// #a7a7a7; border-radius: 50% }`, `:checked { background: #0787d8; border-color: #0787d8;
/// box-shadow: inset 0 0 0 4px #f5f5f5 }`.
pub const RADIO_SIZE: f32 = 16.0;
/// Where Chromium's border ring ends inside the circle (fitted to the dialog-resize reference:
/// 1.2 px, not the 1-px border width).
pub const RADIO_INNER_EDGE: f32 = 1.2;
pub const RADIO_COLORS: (Color32, Color32, Color32) = (rgb(0xF5F5F5), rgb(0xA7A7A7), rgb(0x0787D8)); // fill, border, checked

// ---- New, Resize, Canvas Size (css/window.css "New image dialog", "Image and canvas size
// dialogs"; 12-DIP text) ----

/// A dialog fieldset (`fieldset { border: 0; border-top: 1px solid var(--pdn-separator) }`,
/// `legend { padding: 0 7px 0 0 }`): the rule runs through the legend's middle, from its
/// padding box's right edge.
pub const FIELDSET_LEGEND_PAD: f32 = 7.0;
/// `.image-size-dialog { width: min(365px, 100vw − 24px) }`; `.image-size-content { gap: 6px;
/// padding: 10px 10px 8px }`; `.image-size-summary { padding-bottom: 3px; border-bottom: 1px;
/// font-weight: 600 }`.
pub const IMAGE_SIZE_WIDTH: (f32, f32) = (365.0, 24.0); // width, viewport inset
pub const IMAGE_SIZE_PADDING: (f32, f32, f32) = (10.0, 10.0, 8.0); // top, x, bottom
pub const IMAGE_SIZE_GAP: f32 = 6.0;
pub const IMAGE_SIZE_SUMMARY_PAD: f32 = 3.0;
/// `.image-size-sizing { padding-top: 5px }`, `.image-size-choice-row { grid: 127px 90px 18px }`,
/// `.image-size-radio { gap: 7px }`, `.image-size-maintain { margin-left: 22px }` (its `gap: 7px`).
pub const IMAGE_SIZE_SIZING: (f32, f32, f32, f32) = (5.0, 127.0, 7.0, 22.0); // pad top, choice label, radio gap, maintain indent
/// `fieldset { padding: 6px 0 3px }`, `.image-size-section-body { padding-top: 5px }`.
pub const IMAGE_SIZE_FIELDSET: (f32, f32, f32) = (6.0, 3.0, 5.0); // pad top, pad bottom, body pad top
/// `.image-size-row { grid: 126px 90px 1fr }` with 90×26 number inputs and 27-px selects;
/// `.image-size-select-row { grid: 128px 1fr }` (`.image-size-resampling-row`: `+ 27px`).
pub const IMAGE_SIZE_ROW: (f32, f32, f32, f32) = (126.0, 90.0, 26.0, 27.0); // label, input w, input h, select h
pub const IMAGE_SIZE_SELECT_LABEL: f32 = 128.0;
/// `.image-size-reset { 27×27; padding: 3px }`, `img { 18×18 }`; `.image-size-gamma { gap: 6px }`.
pub const IMAGE_SIZE_RESET: (f32, f32) = (27.0, 18.0);
pub const IMAGE_SIZE_GAMMA_GAP: f32 = 6.0;
/// `.image-size-anchor-chooser { grid: repeat(3, 28px); margin: 0 0 5px 184px }`, `.image-size-
/// fill-row { margin-top: 2px }`; the arrow SVG (`viewBox 0 0 28 28`, the button's content box):
/// a 1.5-px stroke from 14 − 7·d over 14 px, a head 6 deep and 6 wide.
pub const ANCHOR_CELL: f32 = 28.0;
pub const ANCHOR_MARGIN: (f32, f32) = (184.0, 5.0); // left, bottom
pub const ANCHOR_IMAGE: f32 = 18.0;
pub const ANCHOR_ARROW: (f32, f32, f32, f32) = (7.0, 14.0, 1.5, 6.0); // back, length, stroke, head
pub const IMAGE_SIZE_FILL_MARGIN: f32 = 2.0;
/// `.image-size-footer button { min-width: 104px }`, `> div { gap: 10px }` (`.new-file-footer` too).
pub const IMAGE_SIZE_BUTTON: f32 = 104.0;
pub const SIZE_FOOTER_GAP: f32 = 10.0;
/// `.new-file-dialog { width: min(340px, 100vw − 32px) }`, `.new-file-content { gap: 7px;
/// padding: 9px 11px 5px }`, `.new-file-size-summary { padding-bottom: 4px; border-bottom: 1px }`
/// (bold), `.new-file-aspect { gap: 7px }`, `fieldset { padding: 7px 0 5px }`,
/// `.new-file-section-body { gap: 6px; padding-top: 3px }`, `.new-file-row { grid: 100px 90px
/// 1fr; gap: 8px }` with 90×25 inputs.
pub const NEW_FILE_WIDTH: f32 = 340.0;
pub const NEW_FILE_PADDING: (f32, f32, f32) = (9.0, 11.0, 5.0); // top, x, bottom
pub const NEW_FILE_GAP: f32 = 7.0;
pub const NEW_FILE_SUMMARY_PAD: f32 = 4.0;
pub const NEW_FILE_FIELDSET: (f32, f32, f32) = (7.0, 5.0, 3.0); // pad top, pad bottom, body pad top
pub const NEW_FILE_ROW: (f32, f32, f32, f32) = (100.0, 90.0, 25.0, 8.0); // label, input w, h, gap
/// `.layer-properties-dialog { width: min(380px, 100vw − 32px) }`, `.layer-properties-content {
/// gap: 8px; padding: 10px 10px 8px }`, `.layer-properties-row { gap: 3px }`, the name `input
/// { height: 28px; padding: 2px 3px }`, `.layer-properties-opacity-controls { grid: 1fr 87px;
/// gap: 14px; height: 31px }` (the range's UA `margin: 2px`, 28 tall; the 87×25 number),
/// `.layer-properties-blend { grid: max-content 123px; gap: 7px; margin-top: 1px }` (`select {
/// 123×29; padding: 2px 7px }`), `.layer-properties-visible { gap: 6px; height: 25px }` (16-px
/// box); the footer `min-height: 51px; padding: 8px 10px 10px; border-top: 0`, `> div { gap:
/// 10px }`, `button { 103×29 }`.
pub const LAYER_PROPS_WIDTH: f32 = 380.0;
pub const LAYER_PROPS_PADDING: (f32, f32, f32) = (10.0, 10.0, 8.0); // top, x, bottom
pub const LAYER_PROPS_GAP: (f32, f32) = (8.0, 3.0); // rows, label to control
pub const LAYER_PROPS_NAME: (f32, f32) = (28.0, 3.0); // height, padding-left
pub const LAYER_PROPS_OPACITY: (f32, f32, f32, f32, f32) = (31.0, 87.0, 14.0, 2.0, 28.0); // row h, number w, gap, range margin, range h
pub const LAYER_PROPS_NUMBER_HEIGHT: f32 = 25.0;
pub const LAYER_PROPS_BLEND: (f32, f32, f32, f32, f32) = (123.0, 29.0, 7.0, 1.0, 7.0); // select w, h, gap, margin-top, pad left
pub const LAYER_PROPS_VISIBLE: (f32, f32, f32) = (16.0, 6.0, 25.0); // box, gap, height
pub const LAYER_PROPS_FOOTER: (f32, f32, f32, f32) = (51.0, 8.0, 10.0, 10.0); // height, pad top, pad bottom, gap
pub const LAYER_PROPS_BUTTON: (f32, f32) = (103.0, 29.0);
/// `.curves-content { gap: 11px }`, `.curves-transfer-select { gap: 5px }` with its `select {
/// width: 105px; height: 30px; padding: 3px 18px 3px 5px }`, `.curves-coordinates { min-width:
/// 72px; padding: 0 5px 6px }`, `.effect-curve-editor { 354×312 (border-box); margin-left: 10px;
/// border: 1px }`, `.curves-channel-options label { gap: 6px }` (the UA checkbox margin 3 3 3 4),
/// `.curves-tip { margin: 1px 0 3px; bold }`.
pub const EFFECT_CURVES_GAPS: (f32, f32) = (11.0, 5.0);
pub const EFFECT_CURVES_SELECT: (f32, f32, f32, f32) = (105.0, 30.0, 5.0, 18.0); // w, h, pad l, pad r
pub const EFFECT_CURVES_COORDS: (f32, f32, f32) = (72.0, 5.0, 6.0); // min width, pad x, pad bottom
pub const EFFECT_CURVE_CANVAS: (f32, f32, f32) = (354.0, 312.0, 10.0); // w, h, margin-left
pub const EFFECT_CHANNEL_GAP: f32 = 6.0;
pub const EFFECT_TIP_MARGIN: (f32, f32) = (1.0, 3.0); // top, bottom
/// `.levels-content { grid: minmax(125px, 1fr) 92px 92px minmax(125px, 1fr) / 258px 28px; gap: 0
/// 10px }`; the histogram `canvas { 100% × 220px }`; `.levels-column .pdn-number-input { 55px;
/// top 26 / 216 }`, `.levels-gamma { top: 105px }`; `.levels-swatch { 55×25; border: 2px; top 58
/// (white), 137 (mid, #777), 184 (black) }`; `.levels-vertical-track { top: 26px; 22×215 }`; the
/// handles' 7×10 triangles 8 px outside it; `.levels-channels { gap: 16px }`, `label { gap: 5px
/// }`; h3 `::after` lines 8 px after the text.
pub const EFFECT_LEVELS_COLUMN: f32 = 92.0;
pub const EFFECT_LEVELS_ROWS: (f32, f32) = (258.0, 28.0);
pub const EFFECT_LEVELS_HISTOGRAM: f32 = 220.0;
pub const EFFECT_LEVELS_NUMBER: (f32, f32, f32, f32) = (55.0, 26.0, 216.0, 105.0); // w, high top, low top, gamma top
pub const EFFECT_LEVELS_SWATCH: (f32, f32, f32, f32, f32) = (25.0, 58.0, 137.0, 184.0, 2.0); // h, white, mid, black, border
pub const EFFECT_LEVELS_TRACK: (f32, f32, f32) = (26.0, 22.0, 215.0); // top, w, h
pub const EFFECT_LEVELS_HANDLE: (f32, f32, f32) = (7.0, 10.0, 8.0); // w, h, outside
pub const EFFECT_LEVELS_TRACK_COLORS: (Color32, Color32) = (rgb(0xFFFFFF), rgb(0x111111));
pub const EFFECT_LEVELS_MID_SWATCH: Color32 = rgb(0x777777);
pub const EFFECT_LEVELS_CHANNEL_GAPS: (f32, f32) = (16.0, 5.0);
pub const EFFECT_H3_LINE: (f32, f32) = (8.0, 8.0); // gap after the text, offset from the top

// ---- BB plan BB5: the IndirectUI rows of plugin dialogs (common.md §3). paintdotjs has no
// counterpart, so these are paint.rs's own look on the Task 42 kit; a disabled row's controls
// fade like a disabled number box (`NUMBER_DISABLED_OPACITY`), its texts take
// `MENU_ITEM_DISABLED_TEXT`. ----

/// A tab strip: one 23-DIP row, each caption padded 6 DIP each side, 2 DIP between tabs.
pub const EFFECT_TAB_HEIGHT: f32 = 23.0;
pub const EFFECT_TAB_PADDING: f32 = 6.0;
pub const EFFECT_TAB_GAP: f32 = 2.0;
/// A control's footnote: 10-DIP text; a footnote of a single space is a 6-DIP gap.
pub const EFFECT_NOTE_FONT: f32 = 10.0;
pub const EFFECT_NOTE_GAP: f32 = 6.0;
/// A drop-down's inline caption: the gap between the widest caption and the box.
pub const EFFECT_CAPTION_GAP: f32 = 6.0;
/// A header line (ruling R84): it starts this far right of its label.
pub const EFFECT_HEADER_RULE_GAP: f32 = 6.0;
/// A multi-line text box: 4 lines tall.
pub const EFFECT_TEXT_AREA_LINES: usize = 4;
/// A pan row: the gap between the box and the X / Y column (and between its two lines), and the
/// X / Y label width before each number box.
pub const EFFECT_PAN_ROW_GAP: f32 = 10.0;
pub const EFFECT_PAN_ROW_LABEL: f32 = 16.0;

/// `.project-info-dialog { width: min(625px, …) }`, `-content { padding: 12px 12px 10px;
/// line-height: 1.45 }` (About: `padding-inline: 8px`), `.project-info-banner { height: 72px }`,
/// `-artwork { gap: 6px; padding: 5px 2px }`, `img { 62px }`, the wordmark `font: 300 50px/1
/// "Inter"; letter-spacing: -2.5px`.
pub const PROJECT_INFO_WIDTH: f32 = 625.0;
pub const PROJECT_INFO_PADDING: (f32, f32, f32, f32) = (12.0, 12.0, 10.0, 8.0); // top, x (Welcome), bottom, x (About)
pub const PROJECT_INFO_LINE_HEIGHT: f32 = 1.45;
pub const PROJECT_INFO_BANNER: f32 = 72.0;
pub const PROJECT_INFO_ARTWORK: (f32, f32, f32, f32) = (6.0, 5.0, 2.0, 62.0); // gap, padding y, padding x, img
pub const PROJECT_INFO_WORDMARK: (f32, f32) = (50.0, -2.5); // size, letter spacing
/// `.project-info-copy { gap: 12px (summary); padding: 8px 2px 2px }`, `-summary-heading { gap:
/// 10px }`, `-highlight-credit { padding: 9px 11px; border-left: 3px solid var(--color-link);
/// background: color-mix(in srgb, var(--color-dialog-surface-alt) 76%, transparent) }`.
pub const PROJECT_INFO_COPY: (f32, f32, f32) = (12.0, 8.0, 2.0); // gap, padding top, padding x/bottom
pub const PROJECT_INFO_HEADING_GAP: f32 = 10.0;
pub const PROJECT_INFO_CREDIT: (f32, f32, f32, f32) = (9.0, 11.0, 3.0, 0.76); // padding y, x, border, mix
/// `.project-info-summary-details h3 { margin: 0 0 5px }`.
pub const PROJECT_INFO_H3_MARGIN: f32 = 5.0;
/// `.project-info-font-license-list { height: 200px; overflow-y: scroll; padding: 5px 7px; color:
/// #000; background: #fff; border: 1px solid #7a7a7a; line-height: 1.55 }`, its links `#06c`,
/// underlined.
pub const LICENSE_LIST: (f32, f32, f32, f32) = (200.0, 5.0, 7.0, 1.55); // height, padding y, x, line height
pub const LICENSE_LIST_COLORS: (Color32, Color32, Color32, Color32) =
    (rgb(0x000000), rgb(0xFFFFFF), rgb(0x7A7A7A), rgb(0x0066CC)); // text, background, border, link
/// paint.rs's licence viewer (paintdotjs opens the file in a browser tab): the licence list's box
/// at this height.
pub const LICENSE_VIEW_HEIGHT: f32 = 360.0;
/// `.project-info-dialog-footer { padding-left: 14px }`, `.project-info-preference { gap: 8px }`,
/// `.project-info-primary-button { min-width: 104px }`.
pub const PROJECT_INFO_FOOTER_PADDING_LEFT: f32 = 14.0;
pub const PROJECT_INFO_PREFERENCE_GAP: f32 = 8.0;
pub const PROJECT_INFO_BUTTON_MIN_WIDTH: f32 = 104.0;

/// ErrorForm.initializeDefault: `min(680, max(360, clientWidth − 40))` × `min(460, max(260,
/// clientHeight − 80))`, centred (`setAnchor(0.5, 0.5)`).
pub const ERROR_FORM_SIZE: (f32, f32, f32, f32, f32, f32) =
    (360.0, 680.0, 40.0, 260.0, 460.0, 80.0); // min w, max w, w inset, min h, max h, h inset
/// `.error-form-content { gap: 6px; padding: 14px }`, `-heading { font-size: 11pt; bold }`,
/// `-explanation { margin-bottom: 4px; line-height: 1.4 }`, `-source { margin-bottom: 3px; padding:
/// 3px 7px; color: #ffd0d0; background: #5b2020; border: 1px solid #873838 }`, `label {
/// margin-top: 3px; bold }`, `pre { padding: 8px; font: 14px/1.4 monospace (the late `14px`
/// override); border: 1px var(--color-dialog-border); background: var(--color-dialog-surface) }`,
/// `-message { max-height: 74px }`, `-stack { min-height: 80px }`.
pub const ERROR_FORM_CONTENT: (f32, f32) = (6.0, 14.0); // gap, padding
pub const ERROR_FORM_HEADING_SIZE: f32 = 11.0 * 4.0 / 3.0;
pub const ERROR_FORM_EXPLANATION: (f32, f32) = (4.0, 1.4); // margin-bottom, line height
pub const ERROR_FORM_SOURCE: (f32, f32, f32) = (3.0, 3.0, 7.0); // margin-bottom, padding y, x
pub const ERROR_FORM_SOURCE_COLORS: (Color32, Color32, Color32) =
    (rgb(0xFFD0D0), rgb(0x5B2020), rgb(0x873838)); // text, background, border
pub const ERROR_FORM_LABEL_MARGIN: f32 = 3.0;
pub const ERROR_FORM_PRE: (f32, f32, f32) = (8.0, 14.0, 1.4); // padding, font size, line height
pub const ERROR_FORM_MESSAGE_MAX: f32 = 74.0;
pub const ERROR_FORM_STACK_MIN: f32 = 80.0;
/// `.error-form-footer { gap: 12px; padding: 9px 12px; background: var(--color-menu-background);
/// border-top: 1px }`, `-save-status { margin-right: 6px }`, `-actions { gap: 8px }`, `button {
/// min-width: 90px; min-height: 28px }`.
pub const ERROR_FORM_FOOTER: (f32, f32, f32) = (12.0, 9.0, 12.0); // gap, padding y, x
pub const ERROR_FORM_STATUS_MARGIN: f32 = 6.0;
pub const ERROR_FORM_ACTIONS_GAP: f32 = 8.0;
pub const ERROR_FORM_BUTTON_MIN: (f32, f32) = (90.0, 28.0);

pub fn current(ctx: &egui::Context) -> Theme {
    ctx.data(|d| d.get_temp::<Theme>(egui::Id::unique("paintrs.theme")))
        .unwrap_or(DARK)
}

pub fn set_current(ctx: &egui::Context, theme: Theme) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("paintrs.theme"), theme));
}

/// file-flows-and-palettes.md: invalid nonempty palette names use native red/white feedback.
pub const PALETTE_INVALID_BACKGROUND: Color32 = rgb(0xFF0000);

/// Settings geometry, css/window.css .settings-dialog, -title, -navigation and -page.
pub const SETTINGS_SIZE: (f32, f32) = (874., 670.);
pub const SETTINGS_TITLE: (f32, f32) = (39., 38.);
pub const SETTINGS_NAV: f32 = 204.;
pub const SETTINGS_NARROW_NAV: f32 = 150.;
pub const SETTINGS_NAV_ROW: f32 = 39.;
pub const SETTINGS_GROUP_HEIGHT: f32 = 28.;
pub const SETTINGS_HEADING: f32 = 16.;

/// css/window.css Settings responsive breakpoint and outer viewport margin.
pub const SETTINGS_BREAKPOINT: f32 = 660.;
pub const SETTINGS_SCREEN_INSET: f32 = 32.;
pub const SETTINGS_MIN_SIZE: (f32, f32) = (240., 180.);
/// .settings-dialog-footer: height, inline padding, Close width and height.
pub const SETTINGS_FOOTER: (f32, f32, f32, f32) = (45., 12., 88., 24.);
/// .settings-navigation: padding x/y; row right inset, icon size/inset and label offset.
pub const SETTINGS_NAV_PADDING: (f32, f32, f32) = (8., 9., 17.);
pub const SETTINGS_NAV_ICON: (f32, f32, f32) = (20., 8., 39.);
/// .settings-navigation-group-title: left inset, preceding gap, text top and line-box height.
pub const SETTINGS_GROUP_LAYOUT: (f32, f32, f32, f32) = (16., 7., 5., 21.);
/// .settings-page and .settings-page-canvas inline/block padding.
pub const SETTINGS_PAGE_PADDING: (f32, f32) = (12., 14.);
pub const SETTINGS_CANVAS_PADDING: (f32, f32) = (10., 11.);
/// .settings-checkbox min-height (UI and Canvas), box size and caption offset.
pub const SETTINGS_CHECKBOX_ROWS: (f32, f32) = (32., 27.);
pub const SETTINGS_CHECKBOX: (f32, f32) = (12., 19.);
/// .settings-field-row label gap, select width/height/top and row pitch.
pub const SETTINGS_FIELD: (f32, f32, f32, f32, f32) = (10., 240., 28., 5., 38.);
/// Responsive .settings-field-row select top and row pitch.
pub const SETTINGS_FIELD_NARROW: (f32, f32) = (18., 54.);
/// Settings headings/notes/sections and button spacing from css/window.css.
pub const SETTINGS_HEADING_PITCH: f32 = 27.;
pub const SETTINGS_NOTE: (f32, f32) = (25., 25.);
pub const SETTINGS_SECTION: (f32, f32, f32, f32) = (15., 9., 7., 23.);
pub const SETTINGS_BUTTON: (f32, f32, f32, f32, f32) = (112., 27., 26., 3., 37.);
/// Settings color-mix foreground contributions: description, navigation, group caption, disabled.
pub const SETTINGS_MIX: (f32, f32, f32, f32) = (0.74, 0.78, 0.58, 0.55);
/// Settings generic button Chromium auto-outline colors and outer/inner radii.
pub const SETTINGS_FOCUS: (Color32, Color32, f32, f32, f32) =
    (rgb(0x101010), rgb(0xFFFFFF), 4., 3., 2.);
/// .settings-canvas-color-editor dimensions: normal left margin, width, height and top margin.
pub const SETTINGS_COLOR_EDITOR: (f32, f32, f32, f32) = (136., 495., 128., 3.);
pub const SETTINGS_COLOR_SWATCH: f32 = 34.;
pub const SETTINGS_COLOR_WHEEL: (f32, f32) = (126., 110.);
pub const SETTINGS_COLOR_GAP: f32 = 9.;
/// .settings-canvas-color-bars and neutral bar width, height and relative left offset.
pub const SETTINGS_COLOR_BARS: (f32, f32, f32, f32) = (18., 15., 120., 29.);
/// .settings-canvas-rgb: number wrapper width/height, row pitch, reset top offset.
pub const SETTINGS_COLOR_RGB: (f32, f32, f32, f32) = (62., 27., 31., 94.);
/// .settings-inline-reset: normal width/height, icon size and checker reset size.
pub const SETTINGS_RESET: (f32, f32, f32, f32) = (61., 29., 18., 25.);
/// .settings-canvas-brightness: text size, label pitch, range height, numeric width.
pub const SETTINGS_CHECKER_ROW: (f32, f32, f32, f32) = (14., 20., 28., 88.);
/// Existing select popups remain inside the viewport with the dialog's 8-DIP safe inset.
pub const SELECT_VIEWPORT_INSET: f32 = 8.;

/// Chromium's focused input selection, light and dark color-scheme; Task21 carry into Task51.
pub fn text_selection(ctx: &egui::Context) -> Color32 {
    if current(ctx).scheme == Scheme::Light {
        rgb(0x3064CE)
    } else {
        TEXT_SELECTION
    }
}
pub fn set_animations(ctx: &egui::Context, enabled: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("paintrs.animations"), enabled));
}
pub fn animations_enabled(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("paintrs.animations")))
        .unwrap_or(true)
}

/// .settings-navigation-group-title: 10px, letter-spacing .02em.
pub const SETTINGS_GROUP_FONT: f32 = 10.;
pub const SETTINGS_GROUP_TRACKING: f32 = 0.2;

// SettingsKeyboardEntry / css/window.css .shortcut-settings-* (Task52).
pub const KEYBOARD_SEARCH: (f32, f32, f32) = (25., 8., 10.);
pub const KEYBOARD_GROUP: f64 = 26.;
pub const KEYBOARD_ROW: f64 = 38.;
pub const KEYBOARD_CAPTURE: (f32, f32) = (180., 24.);
pub const KEYBOARD_RESET: (f32, f32) = (58., 24.);
pub const KEYBOARD_GAP: f32 = 8.;
pub const KEYBOARD_PAD: f32 = 7.;
pub const KEYBOARD_BOTTOM: f32 = 75.;
pub const KEYBOARD_RESET_MARGIN: f32 = 6.;
pub const KEYBOARD_PLACEHOLDER: Color32 = Color32::from_rgb(117, 117, 117);
// SettingsDiagnosticsEntry / css/window.css .settings-diagnostics-table.
pub const DIAGNOSTICS_PAD: (f32, f32) = (8., 5.);
pub const DIAGNOSTICS_COLUMN: f32 = 146.609_38; // 18 mono glyphs + padding/border, CSS max-content
// Diagnostics grid rows include 5-DIP vertical padding and a 1-DIP bottom border.
pub const DIAGNOSTICS_ROW: f32 = 28.;
pub const DIAGNOSTICS_USER_AGENT_HEIGHT: f32 = 45.;
pub const DIAGNOSTICS_TABLE_HEIGHT: f32 = 271.;

// Task54: css/window.css .settings-{shape,asset}-controls, -drop-zone, -row and -preview.
pub const ASSET_BUTTON_HEIGHT: f32 = 27.;
pub const ASSET_CONTROL_GAP: f32 = 8.;
pub const ASSET_DROP_HEIGHT: f32 = 68.;
pub const ASSET_DROP_MARGIN: f32 = 2.;
pub const ASSET_DASH: (f32, f32) = (3., 5.);
pub const ASSET_SECTION_TOP: f32 = 7.;
pub const ASSET_ROW_PADDING: f32 = 7.;
pub const ASSET_ROW_GAP: f32 = 5.;
pub const ASSET_PACK_ROW_HEIGHT: f32 = 91.;
pub const ASSET_SHAPE_ROW_HEIGHT: f32 = 56.;
pub const ASSET_REMOVE_SIZE: (f32, f32) = (29., 28.);
pub const ASSET_PREVIEW_SIZE: f32 = 28.;
pub const ASSET_SHAPE_PREVIEW: f32 = 42.;
pub const ASSET_PREVIEW_GAP: f32 = 4.;
pub const ASSET_METADATA_SIZE: f32 = 11.;
pub const ASSET_SHAPE_PREVIEW_COLORS: (Color32, Color32) = (rgb(0x4f86b8), rgb(0xb9dcff));

// Task56: css/menu.css .cet-titlebar/.cet-control-icon, custom-electron-titlebar themebar.
pub const DESKTOP_TITLE_HEIGHT: f32 = 30.0;
pub const CAPTION_WIDTH: f32 = 46.0;
pub const RESIZE_BORDER: f32 = 4.0;
pub const CAPTION_CLOSE_HOVER: Color32 = rgb(0xe81123);
pub const CAPTION_CLOSE_PRESSED: Color32 = rgb(0xf1707a);
pub const CAPTION_HOVER_ALPHA: u8 = 26;
pub const CAPTION_PRESSED_ALPHA: u8 = 51;
pub const CAPTION_INACTIVE_OPACITY: f32 = 0.4;

// spec-question: Q57 Effect color popup layout
/// R153/Q57 port popup policy, not Windows ColorDialog geometry. Shared horizontal slider
/// controls.
pub const EFFECT_COLOR_POPUP_WIDTH: f32 = 232.0;
/// Opaque / alpha heights: padding + wheel + gap + 3/4 number rows + hex row.
pub const EFFECT_COLOR_POPUP_HEIGHT: (f32, f32) = (320.0, 349.0);
pub const EFFECT_COLOR_POPUP_PADDING: f32 = 8.0;
pub const EFFECT_COLOR_POPUP_WHEEL: f32 = 180.0;
pub const EFFECT_COLOR_POPUP_PREVIEW: f32 = 28.0;

// ---- BB Chart or Graph's windows (render-chart-or-graph.md "Window 1–3", 96-DPI client
// px; `[x, y, w, h]` rectangles, `(x, y)` label origins) ----
/// Window 1: the client, its 500 × 377 content panel at (12, 12) (controls are panel-relative).
pub const CHART_CLIENT: (f32, f32) = (484.0, 426.0);
pub const CHART_PANEL: [f32; 4] = [12.0, 12.0, 500.0, 377.0];
pub const CHART_TITLE_LABEL: (f32, f32) = (2.0, 6.0);
pub const CHART_TITLE_BOX: [f32; 4] = [32.0, 2.0, 196.0, 20.0];
pub const CHART_ENTER_DATA: [f32; 4] = [234.0, 1.0, 78.0, 23.0];
pub const CHART_LOAD_DATA: [f32; 4] = [318.0, 1.0, 78.0, 23.0];
pub const CHART_TYPE_LABEL: (f32, f32) = (-2.0, 32.0);
pub const CHART_TYPE_LIST: [f32; 4] = [32.0, 28.0, 196.0, 21.0];
pub const CHART_3D: [f32; 4] = [235.0, 32.0, 40.0, 17.0];
pub const CHART_KEY: [f32; 4] = [275.0, 32.0, 44.0, 17.0];
pub const CHART_SAVE_DATA: [f32; 4] = [318.0, 27.0, 78.0, 23.0];
/// The preview: 209 px tall, 226 for Pie and Doughnut.
pub const CHART_PREVIEW: [f32; 4] = [0.0, 48.0, 400.0, 209.0];
pub const CHART_PREVIEW_PIE_HEIGHT: f32 = 226.0;
pub const CHART_Y_LABEL: (f32, f32) = (-1.0, 256.0);
pub const CHART_Y_BOX: [f32; 4] = [38.0, 253.0, 108.0, 20.0];
pub const CHART_X_LABEL: (f32, f32) = (150.0, 256.0);
pub const CHART_X_BOX: [f32; 4] = [190.0, 254.0, 206.0, 20.0];
pub const CHART_EXPLODE_LABEL: (f32, f32) = (263.0, 257.0);
pub const CHART_EXPLODE_LIST: [f32; 4] = [314.0, 205.0, 82.0, 69.0];
/// Client coordinates, anchored bottom-right.
pub const CHART_OK: [f32; 4] = [316.0, 391.0, 75.0, 23.0];
pub const CHART_CANCEL: [f32; 4] = [397.0, 391.0, 75.0, 23.0];
/// The 16 × 16 button icons, left of the text (assets/icons stand-ins: grid, folder, floppy).
pub const CHART_BUTTON_ICON: f32 = 16.0;
/// A list box item's height (8.25 pt Microsoft Sans Serif rows).
// approximation: WinForms ListBox ItemHeight at 96 DPI (render-chart-or-graph.md gives none).
pub const CHART_LIST_ITEM: f32 = 13.0;

/// Window 2: client, minimum window, the panes and the OK/Cancel strip (strip coordinates,
/// anchored bottom-right).
pub const CHART_EDITOR_CLIENT: (f32, f32) = (534.0, 462.0);
pub const CHART_EDITOR_MIN_WINDOW: (f32, f32) = (550.0, 501.0);
pub const CHART_EDITOR_GRID: f32 = 115.0;
pub const CHART_EDITOR_BOTTOM: f32 = 343.0;
pub const CHART_EDITOR_SPLITTER: f32 = 4.0;
pub const CHART_EDITOR_PREVIEW_HEIGHT: f32 = 295.0;
pub const CHART_EDITOR_STRIP: f32 = 44.0;
pub const CHART_EDITOR_OK: [f32; 4] = [366.0, 9.0, 75.0, 23.0];
pub const CHART_EDITOR_CANCEL: [f32; 4] = [447.0, 9.0, 75.0, 23.0];
/// The grid: the 21-px swatch column and its 16-px swatch, the Name column's initial width.
pub const CHART_GRID_SWATCH_COLUMN: f32 = 21.0;
pub const CHART_GRID_SWATCH: f32 = 16.0;
pub const CHART_GRID_NAME: f32 = 60.0;
// approximation: DataGridView's defaults at 96 DPI (row headers 41, rows 22, data columns 100,
// header row 23); render-chart-or-graph.md names none.
pub const CHART_GRID_ROW_HEADER: f32 = 41.0;
pub const CHART_GRID_ROW: f32 = 22.0;
pub const CHART_GRID_COLUMN: f32 = 100.0;
pub const CHART_GRID_HEADER: f32 = 23.0;
/// The new row's black block arrow in its 16-px box (shaft x 1…8, y 5…9; head (8,1)–(15,7)–(8,13)).
pub const CHART_NEW_ROW_ARROW: [(f32, f32); 7] = [
    (1.0, 5.0),
    (8.0, 5.0),
    (8.0, 1.0),
    (15.0, 7.0),
    (8.0, 13.0),
    (8.0, 9.0),
    (1.0, 9.0),
];
/// The column-rename box: its final 300 × 107 client and controls.
pub const CHART_RENAME_CLIENT: (f32, f32) = (300.0, 107.0);
pub const CHART_RENAME_PROMPT: (f32, f32) = (9.0, 20.0);
pub const CHART_RENAME_BOX: [f32; 4] = [12.0, 36.0, 276.0, 20.0];
pub const CHART_RENAME_OK: [f32; 4] = [132.0, 72.0, 75.0, 23.0];
pub const CHART_RENAME_CANCEL: [f32; 4] = [213.0, 72.0, 75.0, 23.0];

/// Window 3 ("Choose a Color"): client, the picker panel, the wheel box and its disc, the
/// current-colour swatch, the right column, the wells and the buttons (client coordinates
/// except the panel's contents).
pub const CHART_PICKER_CLIENT: (f32, f32) = (411.0, 279.0);
pub const CHART_PICKER_PANEL: (f32, f32) = (12.0, 7.0);
pub const CHART_PICKER_WHEEL: [f32; 4] = [3.0, 3.0, 210.0, 210.0];
pub const CHART_PICKER_DISC: f32 = 200.0;
/// The pointer's centre and saturation radius, apart from the drawing's (as specified).
pub const CHART_PICKER_POINTER: (f32, f32) = (103.0, 103.0);
pub const CHART_PICKER_MARKER: (f32, f32) = (10.0, 99.0);
pub const CHART_PICKER_SWATCH: f32 = 30.0;
pub const CHART_PICKER_GRADIENT_END: f32 = 32.0;
pub const CHART_PICKER_LABEL_X: f32 = 230.0;
pub const CHART_PICKER_STRIP: [f32; 3] = [254.0, 73.0, 15.0];
pub const CHART_PICKER_BOX: [f32; 3] = [333.0, 49.0, 22.0];
/// (label y, strip y, box y) of R, G, B, H, S, V.
pub const CHART_PICKER_ROWS: [(f32, f32, f32); 6] = [
    (28.0, 29.0, 26.0),
    (51.0, 54.0, 49.0),
    (76.0, 76.0, 72.0),
    (145.0, 146.0, 143.0),
    (168.0, 169.0, 166.0),
    (191.0, 192.0, 189.0),
];
pub const CHART_PICKER_HEADERS: [(f32, f32, f32); 2] = [(4.0, 261.0, 9.0), (128.0, 261.0, 133.0)];
pub const CHART_PICKER_HEX_LABEL: f32 = 98.0;
pub const CHART_PICKER_HEX: [f32; 4] = [322.0, 95.0, 60.0, 20.0];
pub const CHART_PICKER_WELLS: (f32, f32, f32, f32) = (7.0, 238.0, 28.0, 28.0);
pub const CHART_PICKER_WELL_COLORS: [u32; 8] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x008000, 0x0000FF, 0xFF00FF, 0x00FFFF, 0xFFFF00,
];
pub const CHART_PICKER_PALETTE_LABEL: (f32, f32) = (5.0, 222.0);
pub const CHART_PICKER_OK: [f32; 4] = [243.0, 244.0, 75.0, 23.0];
pub const CHART_PICKER_CANCEL: [f32; 4] = [324.0, 244.0, 75.0, 23.0];

#[cfg(test)]
mod tests {
    use super::*;

    /// `Id::unique` is a fixed-seed hash (egui-0.37 src/id.rs:92, `RandomState::with_seeds(1,2,3,4)`),
    /// so set_current/current meet on the same id.
    #[test]
    fn set_current_roundtrips() {
        let ctx = egui::Context::default();
        for s in [Scheme::Light, Scheme::Blue, Scheme::Dark] {
            set_current(&ctx, Theme::for_scheme(s));
            assert_eq!(current(&ctx), Theme::for_scheme(s));
        }
    }

    /// Blink truncates lengths to 1/64 device px after DSF scaling (task 23 carry): the drop-menu
    /// row padding 3.2 is 3.1875 at 1× but 6.390625 device px (3.1953125) at 2×, so the 24.4-px
    /// row pitch differs from 2 × the 1× one.
    #[test]
    fn layout_units_truncate_at_the_device_scale() {
        assert_eq!(lu(3.2, 1.0), 3.1875);
        assert_eq!(lu(3.2, 2.0) * 2.0, 6.390625);
        assert_eq!(lu(3.2, 1.25) * 1.25, 4.0);
        assert_eq!(lu(16.0, 1.5), 16.0);
    }
}
