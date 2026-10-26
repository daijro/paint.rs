//! `#toolMenu`: the tool selector, its separator and the active tool's options, plus the
//! options' popups (dropdown menus, the size presets menu).
//! Port of: js/src/interface/panel/header/tool/{ToolMenu,ToolOptionDropdown,ToolSelector}.js,
//! js/src/interface/item/menu/action/drop/SelectorMenuItem.js, css/header.css (`#toolMenu`,
//! `.tool-option*`, `.tool-dropdown-menu*`, `.tool-size-dropdown-menu`, `.tool-shape-grid*`,
//! `.tool-font-dropdown-*`), css/menu.css (`.selector`)
//! Reference: paintdotjs @ b4e6fc5
//!
//! The row is laid out as Blink does it in DIPs (the toolbar's `zoom: var(--pdn-ui-scale)` is 1;
//! the strip is 28 high): 1 px borders are one device pixel, text
//! runs take their shaped width rounded up to 1/64 px. The selector's drop menu is a `kit::menu`
//! popup owned by the shell's `Menus`; its arrow is Paint.NET's chevron (`kit::glyph`).
use egui::{Color32, Pos2, Rect, Ui, Vec2};
use pdn_core::app::AppWorkspace;
use pdn_core::pixel::content::ContentBlendMode;
use pdn_core::tool::perspective_warp;
use pdn_core::tool::types::{OptionDef, OptionKind, ToolType, option_definitions};
use serde_json::Value;

use crate::kit::button::{self, MenuItemStyle};
use crate::kit::glyph::{self, ArrowDir};
use crate::kit::img::{self, Filter};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::px;
use crate::kit::text;
use crate::kit::tool_option::{
    self as opt, DropdownClick, DropdownFace, SizeAction, ValueIcon, border, spec,
};
use crate::theme::{self, *};

pub mod popup;

fn definitions(ctx: &egui::Context, tool: ToolType, app: &AppWorkspace) -> Vec<OptionDef> {
    let mut defs = option_definitions(tool, &app.tool_settings);
    if tool == ToolType::Text
        && let Some(fonts) = crate::platform::fonts::choices(ctx)
        && let Some(def) = defs.iter_mut().find(|d| d.key == "fontFamily")
    {
        def.font_choices = Some(fonts);
    }
    defs
}

/// Whose options the row shows: the active tool's, or Layers ▸ Perspective Warp's while its
/// canvas mode runs (`tool::perspective_warp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Tool(ToolType),
    Perspective,
}

impl Owner {
    fn of(app: &AppWorkspace) -> Option<Owner> {
        if app.perspective.is_some() {
            Some(Owner::Perspective)
        } else {
            app.tool_type.map(Owner::Tool)
        }
    }

    fn definitions(self, ctx: &egui::Context, app: &AppWorkspace) -> Vec<OptionDef> {
        match self {
            Owner::Tool(tool) => definitions(ctx, tool, app),
            Owner::Perspective => perspective_warp::tool_options(app),
        }
    }

    fn value(self, app: &AppWorkspace, key: &str) -> Value {
        match self {
            Owner::Tool(tool) => app.tool_settings.get(tool, key),
            Owner::Perspective => perspective_warp::option_value(app, key),
        }
    }

    fn number(self, app: &AppWorkspace, key: &str) -> f64 {
        self.value(app, key).as_f64().unwrap_or(0.0)
    }

    fn set(self, app: &mut AppWorkspace, key: &str, value: Value) {
        match self {
            Owner::Tool(tool) => update_setting(app, tool, key, value),
            Owner::Perspective => perspective_warp::set_option(app, key, &value),
        }
    }
}

/// What the strip reported this frame.
pub struct ToolMenuOutput {
    /// The selector's border box (its drop menu's anchor).
    pub selector_rect: Rect,
    pub selector_clicked: bool,
}

/// The popup a tool option opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionPopup {
    /// `ToolOptionDropdown.active` (by option key).
    Dropdown(&'static str),
    /// `ToolMenu.sizeMenu` (by option key).
    Size(&'static str),
}

#[derive(Default)]
pub struct ToolMenu {
    pub popup: Option<OptionPopup>,
    /// The dropdown's `{once: true}` document click listener (registered after the opening
    /// click, `setTimeout`).
    listener: bool,
    opened_now: bool,
    /// Face / combo border boxes of the last layout, by option key (popup anchors; Perspective
    /// Warp's buttons too).
    anchors: Vec<(&'static str, Rect)>,
    pub popup_state: popup::PopupState,
    /// The open popup's border box (last frame): clicks there belong to the popup.
    popup_rect: Option<Rect>,
    /// A preset was picked: the size input takes focus with its text selected (by option key).
    focus_size: Option<&'static str>,
    /// Whose options the last frame showed: a popup of the other's closes.
    owner: Option<Owner>,
}

/// One option's horizontal metrics (layout px).
struct Slot {
    margin_left: f32,
    /// `.tool-option-section`: border-left + padding-left.
    section: bool,
    label_w: f32,
    control_margin: f32,
    control_w: f32,
    control_h: f32,
    margin_right: f32,
}

/// `#toolMenu .tool-option > span:first-child`: Paint.NET's label is its text box T, GDI's text
/// width + 7, the text centred (R24).
fn label_width(def: &OptionDef) -> f32 {
    let text = def.label.unwrap_or_default();
    if matches!(
        def.kind,
        OptionKind::Action
            | OptionKind::FontStyleToggle
            | OptionKind::Button
            | OptionKind::Checkbox
    ) || text.is_empty()
    {
        // `label.textContent = ""` (`:empty`).
        return 0.0;
    }
    text::gdi_width(text) + TOOL_TEXT_EXTRA
}

/// `#toolMenu`'s padding box as painted: its snapped border box inside its 1-px top border (whole
/// device pixels, `px::border_width`; the patched strip has no bottom border). The top border is
/// Paint.NET's rnd(s) gap under the toolbar row, the box its tool-options row
/// (`ui-text-and-glyphs.md §7.3`).
pub fn padding_box(rect: Rect, ppp: f32) -> Rect {
    let (top, bottom) = TOOL_MENU_BORDER;
    let r = px::snap_rect(rect, ppp);
    Rect::from_min_max(
        Pos2::new(r.min.x, r.min.y + px::border_width(top, ppp)),
        Pos2::new(r.max.x, r.max.y - px::border_width(bottom, ppp)),
    )
}

/// The selected entry (`getSelectedEntry`: the match, else the first).
fn selected_index(def: &OptionDef, value: &Value) -> usize {
    def.index_of(value).unwrap_or(0)
}

/// A value icon's width; its height is [`TOOL_DROPDOWN_VALUE_ICON`]. The dash styles' are wide.
fn value_icon_width(def: &OptionDef) -> f32 {
    if def.key == "dash" {
        TOOL_DASH_ICON_WIDTH
    } else {
        TOOL_DROPDOWN_VALUE_ICON
    }
}

/// The face a select / icon-choice / shape-choice option builds.
pub fn face<'d>(def: &'d OptionDef, value: &'d Value) -> DropdownFace<'d> {
    let entry = def.entry(selected_index(def, value));
    let shape = def.kind == OptionKind::ShapeChoice;
    let icon_only = def.kind == OptionKind::IconChoice && !def.show_text;
    let value_icon = if shape {
        Some(ValueIcon::Shape(entry.0))
    } else {
        entry
            .2
            .map(|icon| ValueIcon::Png(icon, value_icon_width(def), TOOL_DROPDOWN_VALUE_ICON))
    };
    DropdownFace {
        field: def.kind == OptionKind::Select && !def.toolbar,
        icon_only,
        // `cycleOnMainClick: definition.split` (shape choices have none).
        split: def.split,
        fit_selected: def.fit_selected,
        // The blend button's icon follows the value (compositing.md §2.7 "Toolbar list").
        leading_icon: def
            .icon
            .filter(|_| def.kind == OptionKind::Select)
            .map(|icon| {
                if def.key == "blendMode" {
                    ContentBlendMode::from_value(value.as_str().unwrap_or_default()).icon()
                } else {
                    icon
                }
            }),
        value_icon,
        label: if def.font_preview && def.index_of(value).is_none() {
            value.as_str().unwrap_or(entry.1)
        } else {
            entry.1
        },
        // A field's `padding-left` (toolbar faces centre their text, §8.2).
        label_padding: TOOL_DROPDOWN_LABEL_PADDING,
    }
}

/// `.tool-slider-control`: the fixed 175-DIP meter between the ± buttons (§7.3).
const SLIDER_WIDTH: f32 = 2.0 * TOOL_BUTTON + TOOL_SLIDER_METER;

fn slot(def: &OptionDef, value: &Value) -> Slot {
    let control_h = TOOL_OPTION_HEIGHT;
    let (control_w, control_h) = match def.kind {
        OptionKind::Select | OptionKind::IconChoice | OptionKind::ShapeChoice => {
            let face = face(def, value);
            let w = if face.fit_selected {
                face.fit_width()
            } else if def.kind == OptionKind::Select {
                // `width: definition.width || 130` (min-width beats the 130px max-width).
                def.width.unwrap_or(TOOL_DROPDOWN_DEFAULT_WIDTH)
            } else {
                def.width.unwrap_or(0.0)
            };
            (w, face.height())
        }
        OptionKind::Size => {
            let w = 2.0 * TOOL_BUTTON + 2.0 * TOOL_SIZE_GAP + TOOL_SIZE_COMBO.0;
            (w, TOOL_SIZE_COMBO.1)
        }
        OptionKind::Slider => (SLIDER_WIDTH, control_h),
        OptionKind::IconGroup => (def.values.len() as f32 * TOOL_BUTTON, control_h),
        OptionKind::FontStyleToggle => (TOOL_TOGGLE, control_h),
        // A checkbox is laid out as Finish, its box in the image slot.
        OptionKind::Action | OptionKind::Checkbox => (opt::finish_width(def), control_h),
        OptionKind::Button => (TOOL_BUTTON, control_h),
        OptionKind::Number => (
            def.width.unwrap_or(TOOL_NUMBER_INPUT.0),
            TOOL_NUMBER_INPUT.1,
        ),
    };
    let fill = def.key == "fillStyle";
    Slot {
        // The separator's 5-DIP margin (§8.2 "Separator", R24).
        margin_left: if def.section {
            TOOL_SECTION_GAP
        } else if def.key == "fontSize" {
            TOOL_FILL_MARGIN
        } else {
            0.0
        },
        section: def.section,
        label_w: label_width(def),
        control_margin: if fill { TOOL_FILL_MARGIN } else { 0.0 },
        control_w,
        control_h,
        margin_right: if fill { TOOL_FILL_MARGIN } else { 0.0 },
    }
}

impl ToolMenu {
    /// `renderOptions`: `ToolOptionDropdown.closeActive()` and `closeSizeMenu()`.
    pub fn render_options(&mut self) {
        self.close_popup();
    }

    /// The open tool-option popup's border box.
    pub fn popup_rect(&self) -> Option<Rect> {
        self.popup_rect
    }

    pub fn close_popup(&mut self) {
        self.popup = None;
        self.listener = false;
        self.popup_state = Default::default();
        self.popup_rect = None;
    }

    fn open_popup(&mut self, popup: OptionPopup) {
        self.close_popup();
        self.popup = Some(popup);
        self.opened_now = true;
    }

    /// Whether the open font picker's capture-phase `keydown` listener (`onFontTypeahead`) takes
    /// this key before the app's shortcuts: Escape and the arrows always, Enter on a focused
    /// entry, printable keys without Ctrl/Alt/Meta. Other keys reach the shortcuts.
    pub fn font_picker_owns(
        &self,
        app: &AppWorkspace,
        key: egui::Key,
        printable: bool,
        modifiers: egui::Modifiers,
    ) -> bool {
        let (Some(OptionPopup::Dropdown(option)), Some(tool)) = (self.popup, app.tool_type) else {
            return false;
        };
        let picker = option_definitions(tool, &app.tool_settings)
            .iter()
            .any(|d| d.key == option && popup::is_font_picker(d));
        picker
            && match key {
                egui::Key::Escape | egui::Key::ArrowUp | egui::Key::ArrowDown => true,
                egui::Key::Enter => self.popup_state.focused.is_some(),
                _ => printable && !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd,
            }
    }

    /// The anchor rect of option `key` from the last layout.
    pub fn anchor(&self, key: &str) -> Option<Rect> {
        self.anchors
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, r)| *r)
    }

    /// Paints `#toolMenu` (`rect` = its border box) and runs its controls.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        app: &mut AppWorkspace,
        selector_open: bool,
        wheel: bool,
    ) -> ToolMenuOutput {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        let bg = theme.pdn_toolbar_background;
        let tool_px = px::snap_rect(rect, ppp);
        let padding = padding_box(rect, ppp);
        painter.rect_filled(tool_px, 0.0, bg);
        // `overflow: hidden` at the padding box (inside the top border); the layout runs in DIPs
        // from the unsnapped box, as Blink lays it out.
        let content = Rect::from_min_max(
            Pos2::new(rect.min.x, rect.min.y + TOOL_MENU_BORDER.0),
            Pos2::new(rect.max.x, rect.max.y - TOOL_MENU_BORDER.1),
        );
        let outer_clip = ui.clip_rect();
        ui.set_clip_rect(padding.intersect(outer_clip));
        let selector = self.show_selector(ui, content, app, selector_open);
        // `#toolMenu > .horizontal-separator`: 11 DIP, its line 1 device px wide in its 1-DIP slot
        // (§8.2 "Separator", R24).
        let (sep_w, sep_h) = SEPARATOR_SIZE;
        let (ml, mr) = TOOL_SEPARATOR_MARGIN;
        let sep_x = selector.rect.max.x + MENU_ITEM_MARGIN + ml;
        let outer_h = sep_h + SEPARATOR_MARGIN.1;
        let bar = Rect::from_min_size(
            Pos2::new(sep_x, content.min.y + (content.height() - outer_h) / 2.0),
            Vec2::new(1.0 / ppp, sep_h),
        );
        button::separator_vertical_bar(&painter, bar, &theme, bg);
        let options_x = sep_x + sep_w + mr;
        let owner = Owner::of(app);
        if owner != self.owner {
            self.owner = owner;
            self.close_popup();
        }
        if let Some(owner) = owner {
            self.show_options(ui, content, options_x, owner, app, wheel);
        }
        ui.set_clip_rect(outer_clip);
        // The open dropdown's document click listener (`{once: true}`) closes it.
        if self.opened_now {
            self.opened_now = false;
            self.listener = matches!(self.popup, Some(OptionPopup::Dropdown(_)));
        }
        ToolMenuOutput {
            selector_rect: selector.rect,
            selector_clicked: selector.clicked,
        }
    }

    /// `ToolSelector` (a `SelectorMenuItem`): "Tool:", the tool's icon, the hover separator and
    /// the drop arrow, in a 68x23 `.menu-item.clickable.selector`.
    fn show_selector(
        &mut self,
        ui: &mut Ui,
        content: Rect,
        app: &AppWorkspace,
        open: bool,
    ) -> SelectorInput {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        let b = border(ppp);
        let (w, h) = TOOL_SELECTOR_SIZE;
        let margin = MENU_ITEM_MARGIN;
        let outer_h = h + 2.0 * margin;
        let rect = Rect::from_min_size(
            Pos2::new(
                content.min.x + margin,
                content.min.y + (content.height() - outer_h) / 2.0 + margin,
            ),
            Vec2::new(w, h),
        );
        let snapped = px::snap_rect(rect, ppp);
        let response = ui.interact(
            snapped,
            ui.scope_id().with("toolStripChooser.chooseToolButton"),
            egui::Sense::click(),
        );
        let state = MenuItemStyle::of(response.hovered(), open, false, true);
        let fill = button::paint_menu_item_frame(
            &painter,
            snapped,
            state,
            &theme,
            theme.pdn_toolbar_background,
        );
        // Content box: `padding: 2px`, items centered (`.selector { display: flex }`).
        let inner = rect.shrink(b + TOOL_SELECTOR_PADDING);
        let center_top = |hh: f32| inner.min.y + (inner.height() - hh) / 2.0;
        let label = pdn_core::i18n::t("toolStripChooser.chooseToolButton.text");
        let mut x = inner.min.x;
        let spec = spec();
        text::draw(
            &painter,
            Pos2::new(x, center_top(spec.resolved_line_height(ppp))),
            &label,
            spec,
            theme.menu_item_text,
        );
        x += text::css_width(&label, spec.size);
        let gap = TOOL_SELECTOR_GAP;
        if let Some(tool) = app.tool_type {
            let s = TOOL_SELECTOR_ICON;
            let icon = Rect::from_min_size(Pos2::new(x + gap, center_top(s)), Vec2::splat(s));
            img::paint_icon(
                &painter,
                tool.icon(),
                px::snap_rect(icon, ppp),
                fill,
                Filter::None,
            );
            x = icon.max.x + gap;
        }
        // `.selector .separator` (opacity 0, `.selector:hover` .2).
        // Flex shrink keeps the fixed arrow and icon inside the selector when the selected
        // UI face makes the anonymous label wider (menu.css .selector .separator).
        let (sw, sh) = TOOL_SELECTOR_SEPARATOR;
        let sw = sw.min((inner.max.x - (x + gap) - 3.0 * gap - 2.0 * TOOL_SELECTOR_ARROW).max(0.0));
        let sep = Rect::from_min_size(Pos2::new(x + gap, center_top(sh)), Vec2::new(sw, sh));
        if response.hovered() {
            let c = px::blend(
                fill,
                theme.title_bar_border,
                px::css_alpha(TOOL_SELECTOR_SEPARATOR_HOVER_OPACITY),
            );
            painter.rect_filled(px::snap_rect(sep, ppp), 0.0, c);
        }
        x = sep.max.x + gap;
        // `.selector .arrow-down`: a 6x3 border-triangle box, Paint.NET's chevron centred in it
        // (`ui-text-and-glyphs.md §2.7` "Toolstrip dropdown").
        let side = TOOL_SELECTOR_ARROW;
        let r = Rect::from_min_size(
            Pos2::new(x + gap, center_top(side)),
            Vec2::new(2.0 * side, side),
        );
        glyph::paint_arrow(&painter, ArrowDir::Down, r, theme.menu_item_text, fill);
        SelectorInput {
            rect,
            clicked: response.clicked(),
        }
    }

    /// `renderOptions(tool)`: the `.tool-options` row. `wheel`: the wheel over a size box is
    /// its (no menu or popup lies over the pointer).
    fn show_options(
        &mut self,
        ui: &mut Ui,
        content: Rect,
        x0: f32,
        owner: Owner,
        app: &mut AppWorkspace,
        wheel: bool,
    ) {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        let b = border(ppp);
        let defs = owner.definitions(ui.ctx(), app);
        let placed = layout_row(&defs, |k| owner.value(app, k), content, x0, ppp);
        let row = placed.row;
        let outer_clip = ui.clip_rect();
        ui.set_clip_rect(px::snap_rect(row, ppp).intersect(outer_clip));
        let bg = theme.pdn_toolbar_background;
        self.anchors.clear();
        let mut changes: Vec<(&'static str, Value)> = vec![];
        let mut doc_click_consumed = false;
        for (def, p) in defs.iter().zip(&placed.options) {
            if let Some(line) = p.section_line {
                painter.rect_filled(px::snap_line(line, ppp), 0.0, theme.pdn_separator);
            }
            paint_label(&painter, def, p, theme.menu_item_text);
            let control = p.control;
            let value = owner.value(app, def.key);
            match def.kind {
                OptionKind::Select | OptionKind::IconChoice | OptionKind::ShapeChoice => {
                    self.anchors.push((def.key, control));
                    let face = face(def, &value);
                    let expanded = self.popup == Some(OptionPopup::Dropdown(def.key));
                    // A disabled option (S§4.5) is greyed out and takes no clicks; its child Ui
                    // takes no space.
                    let builder = egui::UiBuilder::new().max_rect(control).disabled();
                    let mut greyed = def.disabled.then(|| ui.new_child(builder));
                    let target = greyed.as_mut().unwrap_or(&mut *ui);
                    match opt::tool_option_dropdown(target, def, control, &face, expanded, bg) {
                        Some(DropdownClick::Main) => {
                            // `cycle()`: close if active, then the next value.
                            doc_click_consumed = true;
                            if expanded {
                                self.close_popup();
                            }
                            let i = (selected_index(def, &value) + 1) % def.values.len();
                            changes.push((def.key, def.value(i)));
                        }
                        Some(DropdownClick::Arrow) => {
                            // `open()`: toggles; the click stops propagating.
                            doc_click_consumed = true;
                            if expanded {
                                self.close_popup();
                            } else {
                                self.open_popup(OptionPopup::Dropdown(def.key));
                            }
                        }
                        None => {}
                    }
                }
                OptionKind::Size => {
                    let parts = opt::size_parts(control, TOOL_SIZE_COMBO.0, b);
                    self.anchors.push((def.key, parts[1]));
                    let v = owner.number(app, def.key);
                    let open = self.popup == Some(OptionPopup::Size(def.key));
                    let step = def.step.unwrap_or(1.0);
                    let actions = opt::size_control(
                        ui,
                        def.key,
                        control,
                        TOOL_SIZE_COMBO.0,
                        def,
                        v,
                        open,
                        self.focus_size.take_if(|k| *k == def.key).is_some(),
                        wheel,
                        bg,
                    );
                    // The ± buttons: ±1, ±5 with Ctrl (the tips say
                    // so), rounded to 0.01 (painting-tools-1.md §2 "Brush size controls").
                    let ctrl = ui.input(|i| i.modifiers.ctrl);
                    let step = if ctrl { 5.0 * step } else { step };
                    let add = |d: f64| clamp_json(def, ((v + d) * 100.0).round() / 100.0);
                    for action in actions {
                        match action {
                            SizeAction::Decrease => changes.push((def.key, add(-step))),
                            SizeAction::Increase => changes.push((def.key, add(step))),
                            SizeAction::Input(nv) | SizeAction::Change(nv) => {
                                changes.push((def.key, clamp_json(def, nv)))
                            }
                            SizeAction::Toggle => {
                                // `toggleSizeMenu` (preventDefault + stopPropagation).
                                doc_click_consumed = true;
                                if self.popup == Some(OptionPopup::Size(def.key)) {
                                    self.close_popup();
                                } else {
                                    self.open_popup(OptionPopup::Size(def.key));
                                }
                            }
                            // `closeSizeMenu`.
                            SizeAction::Close => {
                                if matches!(self.popup, Some(OptionPopup::Size(_))) {
                                    self.close_popup();
                                }
                            }
                        }
                    }
                }
                OptionKind::Slider => {
                    let v = owner.number(app, def.key);
                    if let Some(nv) = opt::slider_meter(ui, def.key, control, def, v, bg) {
                        changes.push((def.key, json_number(nv)));
                    }
                }
                OptionKind::IconGroup => {
                    let selected = def.index_of(&value);
                    if let Some(i) = opt::icon_group(ui, control, def, selected, bg) {
                        changes.push((def.key, def.value(i)));
                    }
                }
                OptionKind::FontStyleToggle => {
                    let pressed = value.as_bool().unwrap_or(false);
                    if opt::font_style_toggle(ui, control, def, pressed, bg) {
                        changes.push((def.key, Value::Bool(!pressed)));
                    }
                }
                OptionKind::Checkbox => {
                    self.anchors.push((def.key, control));
                    let on = value.as_bool().unwrap_or(false);
                    if opt::checkbox_button(ui, control, def, on, bg, !def.disabled) {
                        changes.push((def.key, Value::Bool(!on)));
                    }
                }
                OptionKind::Action if owner == Owner::Perspective => {
                    // Straighten (greyed out in Layout), Commit and Cancel.
                    self.anchors.push((def.key, control));
                    if opt::finish_button(ui, control, def, bg, !def.disabled) {
                        perspective_warp::action(app, def.key);
                    }
                }
                OptionKind::Action => {
                    // Finish (no key): commits the active tool's pending interaction; another
                    // action (Object Selection's Select Subject) is the tool's own.
                    let enabled = app
                        .with_tool(|t, _| {
                            // Select Subject waits for its job.
                            (def.key.is_empty() || !t.busy())
                                && t.as_any_mut()
                                    .downcast_mut::<pdn_core::tool::text::TextTool>()
                                    .is_none_or(|t| t.can_finish())
                        })
                        .unwrap_or(true);
                    if opt::finish_button(ui, control, def, bg, enabled) {
                        app.with_tool(|t, app| match def.key {
                            "" => {
                                t.commit_pending(app);
                            }
                            key => t.on_action(app, key),
                        });
                    }
                }
                OptionKind::Button => {
                    // Rectangle Select's swap: Width ↔ Height.
                    let icon = def.icon.unwrap_or_default();
                    if opt::image_button(ui, def.key, control, icon, false, bg).clicked() {
                        let (w, h) = (
                            owner.value(app, "selectionWidth"),
                            owner.value(app, "selectionHeight"),
                        );
                        changes.push(("selectionWidth", h));
                        changes.push(("selectionHeight", w));
                    }
                }
                OptionKind::Number => {
                    let mut v = owner.number(app, def.key);
                    let before = v;
                    inputs::number_input(
                        ui,
                        def.key,
                        def.label.unwrap_or(def.key),
                        control,
                        &mut v,
                        def.min.unwrap_or(f64::MIN),
                        def.max.unwrap_or(f64::MAX),
                        def.step.unwrap_or(1.0),
                        true,
                        NumberStyle::TOOLBAR,
                    );
                    if v != before {
                        changes.push((def.key, json_number(v)));
                    }
                }
            }
        }
        ui.set_clip_rect(outer_clip);
        // The open dropdown's `{once: true}` document listener: any click that propagates to the
        // document (not a dropdown face or entry) closes it.
        // (Clicks on the popup itself are the popup's: `show_popup` runs them.)
        let clicked = inputs::primary_click_pos(ui)
            .is_some_and(|p| !self.popup_rect.is_some_and(|r| r.contains(p)));
        if clicked && self.listener && !self.opened_now && !doc_click_consumed {
            self.close_popup();
        }
        for (key, value) in changes {
            owner.set(app, key, value);
            if key == "selectionMode" || key == "shape" || key == "textShape" {
                self.render_options();
            }
        }
    }

    /// The open option popup, painted over everything else (before the shell's drop menus).
    pub fn show_popup(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        canvas_view: &super::canvas_view::CanvasView,
    ) {
        let Some(open) = self.popup else {
            return;
        };
        let Some(owner) = Owner::of(app) else {
            return;
        };
        let key = match open {
            OptionPopup::Dropdown(k) | OptionPopup::Size(k) => k,
        };
        let defs = owner.definitions(ui.ctx(), app);
        let (Some(def), Some(anchor)) = (defs.iter().find(|d| d.key == key), self.anchor(key))
        else {
            self.close_popup();
            return;
        };
        let ctx = ui.ctx().clone();
        let viewport = ctx.content_rect();
        let pixels = |id| {
            crate::kit::img::texture_pixels(&ctx, id).or_else(|| canvas_view.texture_pixels(id))
        };
        let value = owner.value(app, key);
        let (pick, rect) = match open {
            OptionPopup::Dropdown(_) => popup::dropdown_menu(
                ui,
                def,
                &value,
                anchor,
                &mut self.popup_state,
                &pixels,
                viewport,
            ),
            OptionPopup::Size(_) => popup::size_menu(
                ui,
                def,
                value.as_f64().unwrap_or(0.0),
                anchor,
                &mut self.popup_state,
                &pixels,
                viewport,
            ),
        };
        match pick {
            Some(popup::PopupPick::Value(v)) => {
                // `selectEntry` / `change(value)`, then `close()` (the click stops propagating);
                // a size preset also focuses and selects the input.
                self.close_popup();
                if matches!(open, OptionPopup::Size(_)) {
                    self.focus_size = Some(key);
                }
                owner.set(app, key, v);
                if key == "selectionMode" || key == "shape" || key == "textShape" {
                    self.render_options();
                }
            }
            Some(popup::PopupPick::Select(v)) => owner.set(app, key, v),
            Some(popup::PopupPick::Close) => self.close_popup(),
            // A click inside the popup still reaches the document listener (it closes a
            // dropdown; the size menu only closes on outside pointerdowns).
            Some(popup::PopupPick::Inside)
                if matches!(open, OptionPopup::Dropdown(_)) && self.listener =>
            {
                self.close_popup();
            }
            Some(popup::PopupPick::Inside) | None => {}
        }
        // The size menu's capture-phase `pointerdown` outside listener (not on the menu, not
        // on its combo).
        if matches!(open, OptionPopup::Size(_)) && self.popup == Some(open) {
            let pressed = inputs::press_pos(ui.ctx(), Some(egui::PointerButton::Primary));
            if let Some(p) = pressed
                && !anchor.contains(p)
                && !rect.contains(p)
            {
                self.close_popup();
            }
        }
        if self.popup == Some(open) {
            self.popup_rect = Some(rect);
        }
    }
}

/// One laid-out `.tool-option`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// The section's 1 px left border.
    pub section_line: Option<Rect>,
    /// The label box (when it has text): its text line box's top, the box's width.
    pub label: Option<Rect>,
    /// The control's border box.
    pub control: Rect,
}

/// An option's label, centred in its box (§8.2: DirectWrite centres it in GDI's T). Extra options
/// carry no plus here: the user keeps it for menus and the Tools window (2026-10-25).
fn paint_label(painter: &egui::Painter, def: &OptionDef, p: &Placed, color: Color32) {
    if let (Some(label), Some(r)) = (def.label, p.label) {
        let x = r.min.x + (r.width() - text::css_width(label, TOOL_FONT_SIZE)) / 2.0;
        text::draw(painter, Pos2::new(x, r.min.y), label, spec(), color);
    }
}

/// The `.tool-options` row and its options.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub row: Rect,
    pub options: Vec<Placed>,
}

/// `renderOptions`' flex row as Blink lays it out: `content` is #toolMenu's padding
/// box, `x0` where `.tool-options` starts, `ppp` the scale (borders are whole device px, the
/// separators 11 device px), `value` the setting by key.
pub fn layout_row(
    defs: &[OptionDef],
    value: impl Fn(&str) -> Value,
    content: Rect,
    x0: f32,
    ppp: f32,
) -> Row {
    let slots: Vec<Slot> = defs.iter().map(|d| slot(d, &value(d.key))).collect();
    let group_h = |s: &Slot| s.control_h.max(TOOL_OPTION_HEIGHT);
    // `.tool-options { align-items: center }` in the #toolMenu row; each option centered in it.
    let row_h = slots.iter().map(group_h).fold(0.0, f32::max);
    let row_top = content.min.y + (content.height() - row_h) / 2.0;
    let row = Rect::from_min_max(
        Pos2::new(x0, row_top),
        Pos2::new(content.max.x, row_top + row_h),
    );
    let mut x = x0;
    let mut options = Vec::with_capacity(defs.len());
    for (def, s) in defs.iter().zip(&slots) {
        x += s.margin_left;
        let gh = group_h(s);
        let top = row_top + (row_h - gh) / 2.0;
        let mut section_line = None;
        if s.section {
            // The 1-DIP line slot (its line 1 device px) and the 5-DIP padding (§8.2, R24).
            section_line = Some(Rect::from_min_size(
                Pos2::new(x, top),
                Vec2::new(1.0 / ppp, gh),
            ));
            x += 1.0 + TOOL_SECTION_GAP;
        }
        // The label span (inline-flex, 23px, text centered): its box from the text line's top.
        let label = (def.label.is_some() && s.label_w > 0.0).then(|| {
            let span_top = top + (gh - TOOL_OPTION_HEIGHT) / 2.0;
            let line_top = span_top + (TOOL_OPTION_HEIGHT - spec().resolved_line_height(ppp)) / 2.0;
            Rect::from_min_size(
                Pos2::new(x, line_top),
                Vec2::new(s.label_w, spec().resolved_line_height(ppp)),
            )
        });
        x += s.label_w;
        x += s.control_margin;
        let control = Rect::from_min_size(
            Pos2::new(x, top + (gh - s.control_h) / 2.0),
            Vec2::new(s.control_w, s.control_h),
        );
        x += s.control_w + s.margin_right;
        options.push(Placed {
            section_line,
            label,
            control,
        });
    }
    Row { row, options }
}

struct SelectorInput {
    rect: Rect,
    clicked: bool,
}

fn json_number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}

fn clamp_json(def: &OptionDef, v: f64) -> Value {
    json_number(v.clamp(def.min.unwrap_or(f64::MIN), def.max.unwrap_or(f64::MAX)))
}

/// `ToolMenu.updateSetting` (`AppWorkspace::update_tool_setting`). (The font load arrives with
/// Task 53.)
pub fn update_setting(app: &mut AppWorkspace, tool: ToolType, key: &str, value: Value) {
    app.update_tool_setting(tool, key, value);
}

#[cfg(test)]
#[allow(clippy::excessive_precision)] // LayoutUnit values as the DOM reports them
mod tests {
    use super::*;
    use pdn_core::tool::types::{ToolSettings, json_value};

    /// Control border boxes from the reference DOM (`getBoundingClientRect`, 1280x800, labels in
    /// Selawik, Paint.NET's widths: the strip's padding box at y 58–84, `.tool-options` from x 83),
    /// every tool and the conditional rows.
    #[allow(clippy::type_complexity)]
    const REFERENCE_ROWS: &[(ToolType, Option<(&str, &str)>, &[(&str, [f32; 4])])] = &[
        (
            ToolType::RectangleSelect,
            None,
            &[
                ("combineMode", [83.0, 59.5, 115.0, 23.0]),
                ("selectionMode", [209.0, 59.5, 86.0, 23.0]),
                ("renderingQuality", [306.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Move,
            None,
            &[
                ("resampling", [143.0, 59.5, 65.0, 23.0]),
                ("gammaCorrected", [219.0, 59.5, 35.0, 23.0]),
                ("renderingQuality", [254.0, 59.5, 35.0, 23.0]),
                ("", [300.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::LassoSelect,
            None,
            &[
                ("combineMode", [83.0, 59.5, 115.0, 23.0]),
                ("renderingQuality", [209.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::MoveSelection,
            None,
            &[
                ("renderingQuality", [83.0, 59.5, 35.0, 23.0]),
                ("", [129.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::EllipseSelect,
            None,
            &[
                ("combineMode", [83.0, 59.5, 115.0, 23.0]),
                ("renderingQuality", [209.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Zoom,
            None,
            &[("renderingQuality", [83.0, 59.5, 35.0, 23.0])],
        ),
        (
            ToolType::MagicWand,
            None,
            &[
                ("combineMode", [83.0, 59.5, 115.0, 23.0]),
                ("floodMode", [283.0, 59.5, 35.0, 23.0]),
                ("tolerance", [390.0, 59.5, 221.0, 23.0]),
                ("alphaMode", [611.0, 59.5, 35.0, 23.0]),
                ("sampleMode", [717.0, 59.5, 70.0, 23.0]),
                ("renderingQuality", [798.0, 59.5, 35.0, 23.0]),
                ("", [844.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::Pan,
            None,
            &[("renderingQuality", [83.0, 59.5, 35.0, 23.0])],
        ),
        (
            ToolType::PaintBucket,
            None,
            &[
                ("floodMode", [157.0, 59.5, 35.0, 23.0]),
                ("fillStyle", [229.0, 59.0, 121.0, 24.0]),
                ("tolerance", [423.0, 59.5, 221.0, 23.0]),
                ("alphaMode", [644.0, 59.5, 35.0, 23.0]),
                ("sampleMode", [750.0, 59.5, 70.0, 23.0]),
                ("antialias", [831.0, 59.5, 35.0, 23.0]),
                ("blendMode", [866.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [948.0, 59.5, 35.0, 23.0]),
                ("", [994.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::Gradient,
            None,
            &[
                ("gradientType", [83.0, 59.5, 161.0, 23.0]),
                ("gradientMode", [255.0, 59.5, 35.0, 23.0]),
                ("repeatMode", [290.0, 59.5, 97.0, 23.0]),
                ("antialias", [398.0, 59.5, 35.0, 23.0]),
                ("blendMode", [433.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [515.0, 59.5, 35.0, 23.0]),
                ("", [561.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::PaintBrush,
            None,
            &[
                ("width", [145.0, 59.0, 123.0, 24.0]),
                ("pressure", [268.0, 59.5, 35.0, 23.0]),
                ("hardness", [362.0, 59.5, 221.0, 23.0]),
                ("spacing", [635.0, 59.5, 221.0, 23.0]),
                ("fillStyle", [893.0, 59.0, 121.0, 24.0]),
                ("smoothing", [1026.0, 59.5, 35.0, 23.0]),
                ("antialias", [1061.0, 59.5, 35.0, 23.0]),
                ("blendMode", [1096.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [1178.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Eraser,
            None,
            &[
                ("width", [145.0, 59.0, 123.0, 24.0]),
                ("pressure", [268.0, 59.5, 35.0, 23.0]),
                ("hardness", [362.0, 59.5, 221.0, 23.0]),
                ("spacing", [635.0, 59.5, 221.0, 23.0]),
                ("smoothing", [867.0, 59.5, 35.0, 23.0]),
                ("antialias", [902.0, 59.5, 35.0, 23.0]),
                ("renderingQuality", [937.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Pencil,
            None,
            &[
                ("blendMode", [83.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [165.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::ColorPicker,
            None,
            &[
                ("sampleMode", [143.0, 59.5, 70.0, 23.0]),
                ("sampleSize", [213.0, 59.5, 102.0, 23.0]),
                ("afterClick", [389.0, 59.5, 139.0, 23.0]),
                ("renderingQuality", [539.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::CloneStamp,
            None,
            &[
                ("width", [145.0, 59.0, 123.0, 24.0]),
                ("pressure", [268.0, 59.5, 35.0, 23.0]),
                ("hardness", [362.0, 59.5, 221.0, 23.0]),
                ("spacing", [635.0, 59.5, 221.0, 23.0]),
                ("smoothing", [867.0, 59.5, 35.0, 23.0]),
                ("antialias", [902.0, 59.5, 35.0, 23.0]),
                ("blendMode", [937.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [1019.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Recolor,
            None,
            &[
                ("width", [145.0, 59.0, 123.0, 24.0]),
                ("pressure", [268.0, 59.5, 35.0, 23.0]),
                ("hardness", [362.0, 59.5, 221.0, 23.0]),
                ("spacing", [635.0, 59.5, 221.0, 23.0]),
                ("tolerance", [928.0, 59.5, 221.0, 23.0]),
                ("alphaMode", [1149.0, 59.5, 35.0, 23.0]),
                ("recolorSampling", [1195.0, 59.5, 46.0, 23.0]),
                ("smoothing", [1252.0, 59.5, 35.0, 23.0]),
                ("antialias", [1287.0, 59.5, 35.0, 23.0]),
                ("renderingQuality", [1322.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Text,
            None,
            &[
                ("fontFamily", [117.0, 59.0, 140.0, 24.0]),
                ("fontSize", [258.0, 59.0, 123.0, 24.0]),
                ("fontUnit", [381.0, 59.5, 35.0, 23.0]),
                ("bold", [427.0, 59.5, 23.0, 23.0]),
                ("italic", [450.0, 59.5, 23.0, 23.0]),
                ("underline", [473.0, 59.5, 23.0, 23.0]),
                ("strikeout", [496.0, 59.5, 23.0, 23.0]),
                ("textRendering", [519.0, 59.5, 68.0, 23.0]),
                ("align", [598.0, 59.5, 69.0, 23.0]),
                ("antialias", [678.0, 59.5, 35.0, 23.0]),
                ("blendMode", [713.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [795.0, 59.5, 35.0, 23.0]),
                ("", [841.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::Line,
            None,
            &[
                ("curveType", [83.0, 59.5, 69.0, 23.0]),
                ("width", [225.0, 59.0, 123.0, 24.0]),
                ("startCap", [383.0, 59.5, 35.0, 23.0]),
                ("dash", [418.0, 59.5, 47.0, 23.0]),
                ("endCap", [465.0, 59.5, 35.0, 23.0]),
                ("fillStyle", [537.0, 59.0, 121.0, 24.0]),
                ("antialias", [670.0, 59.5, 35.0, 23.0]),
                ("blendMode", [705.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [787.0, 59.5, 35.0, 23.0]),
                ("", [833.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::Shapes,
            None,
            &[
                ("shape", [83.0, 59.5, 88.0, 23.0]),
                ("drawType", [171.0, 59.5, 35.0, 23.0]),
                ("width", [279.0, 59.0, 123.0, 24.0]),
                ("dash", [437.0, 59.5, 47.0, 23.0]),
                ("fillStyle", [521.0, 59.0, 121.0, 24.0]),
                ("antialias", [654.0, 59.5, 35.0, 23.0]),
                ("blendMode", [689.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [771.0, 59.5, 35.0, 23.0]),
                ("", [817.0, 59.5, 58.0, 23.0]),
            ],
        ),
        (
            ToolType::RectangleSelect,
            Some(("selectionMode", "fixedSize")),
            &[
                ("combineMode", [83.0, 59.5, 115.0, 23.0]),
                ("selectionMode", [209.0, 59.5, 93.0, 23.0]),
                ("selectionWidth", [344.0, 59.0, 50.0, 24.0]),
                ("swapSelectionSize", [394.0, 59.5, 23.0, 23.0]),
                ("selectionHeight", [463.0, 59.0, 50.0, 24.0]),
                ("selectionUnits", [513.0, 59.0, 100.0, 24.0]),
                ("renderingQuality", [624.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::RectangleSelect,
            Some(("selectionMode", "fixedRatio")),
            &[
                ("combineMode", [83.0, 59.5, 115.0, 23.0]),
                ("selectionMode", [209.0, 59.5, 100.0, 23.0]),
                ("selectionWidth", [351.0, 59.0, 50.0, 24.0]),
                ("swapSelectionSize", [401.0, 59.5, 23.0, 23.0]),
                ("selectionHeight", [470.0, 59.0, 50.0, 24.0]),
                ("renderingQuality", [531.0, 59.5, 35.0, 23.0]),
            ],
        ),
        (
            ToolType::Shapes,
            Some(("shape", "roundedRectangle")),
            &[
                ("shape", [83.0, 59.5, 136.0, 23.0]),
                ("drawType", [219.0, 59.5, 35.0, 23.0]),
                ("width", [327.0, 59.0, 123.0, 24.0]),
                ("radius", [495.0, 59.0, 123.0, 24.0]),
                ("dash", [653.0, 59.5, 47.0, 23.0]),
                ("fillStyle", [737.0, 59.0, 121.0, 24.0]),
                ("antialias", [870.0, 59.5, 35.0, 23.0]),
                ("blendMode", [905.0, 59.5, 82.0, 23.0]),
                ("renderingQuality", [987.0, 59.5, 35.0, 23.0]),
                ("", [1033.0, 59.5, 58.0, 23.0]),
            ],
        ),
    ];

    #[test]
    fn rows_match_reference_dom() {
        let content = Rect::from_min_max(Pos2::new(2.0, 58.0), Pos2::new(1278.0, 84.0));
        for (tool, setting, expected) in REFERENCE_ROWS {
            // These full-inventory geometry fixtures include the conditional pressure control.
            let mut settings = ToolSettings::default();
            settings.pen_seen = true;
            if let Some((key, value)) = setting {
                settings.set(*tool, key, (*value).into());
            }
            let mut defs = option_definitions(*tool, &settings);
            // Paint.NET's row: the inventory lists no Extra options (R18).
            defs.retain(|d| !d.extra);
            let keys: Vec<_> = defs.iter().map(|d| d.key).collect();
            let want: Vec<_> = expected.iter().map(|(k, _)| *k).collect();
            assert_eq!(keys, want, "{tool:?} {setting:?}: the §7.5 inventory");
            let row = layout_row(&defs, |k| settings.get(*tool, k), content, 83.0, 1.0);
            for (i, (key, [x, y, w, h])) in expected.iter().enumerate() {
                let r = row.options[i].control;
                let got = [r.min.x, r.min.y, r.width(), r.height()];
                // Within 1/64 per text run before it (see `text::css_width`).
                for (g, e) in got.iter().zip([x, y, w, h]) {
                    assert!(
                        (g - e).abs() <= 2.0 / 64.0,
                        "{tool:?} {setting:?} {key}: {got:?} vs {:?}",
                        [x, y, w, h]
                    );
                }
            }
        }
    }

    /// GDI's advance of `text` at 12 ppem from Selawik's hmtx (each glyph rounded to a whole
    /// pixel, no kerning; Segoe UI's hdmx 'C' 8 and 'm' 11): the independent check on
    /// `text::gdi_width` (`ui-text-and-glyphs.md §8.2`).
    fn hdmx_advance(text: &str) -> f32 {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/font/selawik/selawk.ttf"
        ))
        .unwrap();
        let face = ttf_parser::Face::parse(&data, 0).unwrap();
        text.chars()
            .map(|c| match c {
                'C' => 8,
                'm' => 11,
                _ => {
                    let a = face
                        .glyph_hor_advance(face.glyph_index(c).unwrap())
                        .unwrap();
                    (a as f64 * 12.0 / face.units_per_em() as f64).round() as i32
                }
            })
            .sum::<i32>() as f32
    }

    /// `ui-text-and-glyphs.md §8.2`, §8.3 (R24): an image + text split is A_gdi + 42, a label
    /// A_gdi + 7. §8.3: Any Size 86, Layer 70, Single Pixel 102 (paintdotjs 86, 70, 101); the
    /// patched reference DOM: Fixed Ratio 100, "Sampling:" 60.
    #[test]
    fn text_bearing_items_autosize_to_their_text() {
        let face_width = |tool, key: &str, value: &str| {
            let defs = option_definitions(tool, &ToolSettings::default());
            let def = defs.iter().find(|d| d.key == key).unwrap();
            face(def, &json_value(value)).fit_width()
        };
        for (tool, key, value, text, want) in [
            (
                ToolType::RectangleSelect,
                "selectionMode",
                "normal",
                "Any Size",
                86.0,
            ),
            (
                ToolType::RectangleSelect,
                "selectionMode",
                "fixedRatio",
                "Fixed Ratio",
                100.0,
            ),
            (ToolType::ColorPicker, "sampleMode", "layer", "Layer", 70.0),
            (
                ToolType::ColorPicker,
                "sampleSize",
                "1",
                "Single Pixel",
                102.0,
            ),
        ] {
            let got = face_width(tool, key, value);
            assert_eq!(got, hdmx_advance(text) + 42.0, "{text}");
            assert_eq!(got, want, "{text}");
        }
        let defs = option_definitions(ToolType::ColorPicker, &ToolSettings::default());
        let sampling = defs.iter().find(|d| d.key == "sampleMode").unwrap();
        assert_eq!(label_width(sampling), hdmx_advance("Sampling:") + 7.0);
        assert_eq!(label_width(sampling), 60.0);
    }

    /// `ui-text-and-glyphs.md §8.4` "Screenshot geometry" (the 96-DPI layout × 1.25, which R24
    /// makes paint.rs's at ppp 1.25): every item edge of the Paintbrush and Paint Bucket rows within
    /// 1 device px, x from the first separator's left edge (combos with their 1-DIP margins).
    #[test]
    fn rows_land_on_paint_net_at_125_percent() {
        let ppp = 1.25;
        let content = Rect::from_min_max(Pos2::new(2.0, 58.0), Pos2::new(1278.0, 84.0));
        let x0 = 83.0;
        let rows: [(ToolType, &[(f32, f32)]); 2] = [
            (
                ToolType::PaintBrush,
                &[
                    (0.0, 13.75),
                    (13.75, 77.5),
                    (91.25, 28.75),
                    (120.0, 96.25),
                    (216.25, 28.75),
                    (245.0, 43.75),
                    (288.75, 73.75),
                    (362.5, 28.75),
                    (391.25, 218.75),
                    (610.0, 28.75),
                    (638.75, 65.0),
                    (703.75, 28.75),
                    (732.5, 218.75),
                    (951.25, 28.75),
                    (980.0, 13.75),
                    (993.75, 31.25),
                    (1025.0, 153.75),
                    (1178.75, 13.75),
                    (1192.5, 43.75),
                    (1236.25, 43.75),
                    (1280.0, 102.5),
                    (1382.5, 43.75),
                ],
            ),
            (
                ToolType::PaintBucket,
                &[
                    (0.0, 13.75),
                    (13.75, 92.5),
                    (106.25, 43.75),
                    (150.0, 13.75),
                    (163.75, 31.25),
                    (195.0, 153.75),
                    (348.75, 13.75),
                    (362.5, 76.25),
                    (438.75, 28.75),
                    (467.5, 218.75),
                    (686.25, 28.75),
                    (715.0, 43.75),
                    (758.75, 13.75),
                    (772.5, 75.0),
                    (847.5, 87.5),
                    (935.0, 13.75),
                    (948.75, 43.75),
                    (992.5, 102.5),
                    (1095.0, 43.75),
                    (1138.75, 13.75),
                    (1152.5, 72.5),
                ],
            ),
        ];
        for (tool, want) in rows {
            // These full-inventory geometry fixtures include the conditional pressure control.
            let mut settings = ToolSettings::default();
            settings.pen_seen = true;
            let defs = option_definitions(tool, &settings);
            let row = layout_row(&defs, |k| settings.get(tool, k), content, x0, ppp);
            // The items as §8.4 lists them, in DIPs: (x, w).
            let mut items = vec![(x0 - 11.0, 11.0)];
            for (def, p) in defs.iter().zip(&row.options) {
                if let Some(line) = p.section_line {
                    items.push((line.min.x - TOOL_SECTION_GAP, 11.0));
                }
                if let Some(l) = p.label {
                    items.push((l.min.x, l.width()));
                }
                let c = p.control;
                match def.kind {
                    OptionKind::Size => items.extend([
                        (c.min.x, TOOL_BUTTON),
                        (c.min.x + TOOL_BUTTON, TOOL_SIZE_COMBO.0 + 2.0),
                        (c.max.x - TOOL_BUTTON, TOOL_BUTTON),
                    ]),
                    OptionKind::Slider => items.extend([
                        (c.min.x, TOOL_BUTTON),
                        (c.min.x + TOOL_BUTTON, TOOL_SLIDER_METER),
                        (c.max.x - TOOL_BUTTON, TOOL_BUTTON),
                    ]),
                    _ if def.key == "fillStyle" => items.push((c.min.x - 1.0, c.width() + 2.0)),
                    _ => items.push((c.min.x, c.width())),
                }
            }
            let origin = x0 - 11.0;
            let got: Vec<(f32, f32)> = items
                .iter()
                .map(|&(x, w)| ((x - origin) * ppp, w * ppp))
                .collect();
            assert_eq!(got.len(), want.len(), "{tool:?}: {got:?}");
            for (i, (g, e)) in got.iter().zip(want).enumerate() {
                assert!(
                    (g.0 - e.0).abs() <= 1.0 && (g.0 + g.1 - e.0 - e.1).abs() <= 1.0,
                    "{tool:?} item {}: {g:?} vs {e:?}",
                    i + 1
                );
            }
        }
    }

    /// User (2026-10-25): the gray plus marks Extras in menus and the Tools window only; an Extra
    /// option in the toolbar lays out exactly as a plain one.
    #[test]
    fn extra_options_carry_no_plus_in_the_toolbar() {
        let content = Rect::from_min_max(Pos2::new(2.0, 58.0), Pos2::new(1278.0, 84.0));
        let settings = ToolSettings::default();
        let tool = ToolType::PaintBucket;
        let plain = option_definitions(tool, &settings);
        let mut defs = plain.clone();
        for d in &mut defs {
            d.extra = matches!(d.key, "tolerance" | "antialias");
        }
        let value = |k: &str| settings.get(tool, k);
        let before = layout_row(&plain, value, content, 83.0, 1.0);
        let after = layout_row(&defs, value, content, 83.0, 1.0);
        assert_eq!(after, before);
    }

    /// Ruling R18 (PC10): Clone Stamp's Extra options, Aligned and Sample, follow its row.
    #[test]
    fn clone_stamp_extra_options() {
        let ctx = egui::Context::default();
        let app = AppWorkspace::default();
        let all = definitions(&ctx, ToolType::CloneStamp, &app);
        let extra: Vec<_> = all.iter().filter(|d| d.extra).map(|d| d.key).collect();
        assert_eq!(extra, ["aligned", "sampleImage"]);
    }

    /// PC40: every Extra item (manifest rows, commands, tools) carries `Mark::Extra` in its menu
    /// and tooltip.
    #[test]
    fn extras_carry_the_plus() {
        use crate::kit::menu::{Mark, MenuEntry, extra_tooltip, mark};
        use crate::shell::main_menu::{ITEMS, entries};
        use pdn_core::effect::extra::manifest::{MANIFEST, layers_action};
        fn flat(entries: Vec<MenuEntry>, out: &mut Vec<MenuEntry>) {
            for e in entries {
                match e {
                    MenuEntry::Submenu { entries, .. } => flat(entries, out),
                    e => out.push(e),
                }
            }
        }
        let all = |app: &AppWorkspace| {
            let mut out = vec![];
            (0..ITEMS.len()).for_each(|i| flat(entries(i, app), &mut out));
            out
        };
        let mut want: Vec<String> = MANIFEST
            .iter()
            .map(|m| match m.category {
                pdn_core::effect::Category::Adjustment => format!("adjustment.{}", m.id),
                pdn_core::effect::Category::Layers => layers_action(m.id).unwrap().into(),
                _ => format!("menu.effects.{}", m.id),
            })
            .collect();
        want.extend(
            [
                "menu.edit.fillSelectionContentAware",
                "menu.edit.fade",
                pdn_core::document::text_layer::RASTERIZE_ACTION,
            ]
            .map(String::from),
        );
        let app = AppWorkspace::default();
        let shown = all(&app);
        for id in &want {
            let entry = shown.iter().find(|e| {
                matches!(e, MenuEntry::Action(a) | MenuEntry::ActionNoIcon(a) | MenuEntry::Checked(a, _)
                    | MenuEntry::Command { id: a, .. } if a == id)
            });
            let entry = entry.unwrap_or_else(|| panic!("{id} is in no menu"));
            assert_eq!(mark(entry), Mark::Extra, "{id}");
            assert!(extra_tooltip(id).is_some(), "{id}");
        }
        for t in ToolType::VALUES.into_iter().filter(|t| t.is_extra()) {
            assert_eq!(mark(&MenuEntry::Tool(t)), Mark::Extra, "{}", t.id());
        }
    }

    /// PC16: the Text tool's Editable text toggle follows Finish, then the turn and shape Extras.
    #[test]
    fn text_editable_option_follows_finish() {
        let ctx = egui::Context::default();
        let defs = definitions(&ctx, ToolType::Text, &AppWorkspace::default());
        let keys: Vec<_> = defs.iter().rev().take(4).map(|d| d.key).collect();
        assert_eq!(keys, ["textShape", "textAngle", "editableText", ""]);
    }

    /// R230: Move Selected Pixels' Resize / Warp choice comes last, an Extra (the plus), Resize
    /// (`distort` false, the default) first.
    #[test]
    fn move_corner_option_comes_last() {
        let ctx = egui::Context::default();
        let app = AppWorkspace::default();
        let defs = definitions(&ctx, ToolType::Move, &app);
        let last = defs.last().unwrap();
        assert_eq!((last.key, last.extra), ("distort", true));
        let entries: Vec<_> = last.values.iter().map(|&(v, t, _)| (v, t)).collect();
        assert_eq!(entries, [("false", "Resize"), ("true", "Warp")]);
        assert_eq!(face(last, &Value::Bool(false)).label, "Resize");
    }

    /// Object Selection's method follows the combine row, an Extra (the plus): MobileSAM, the
    /// desktop's default (the web's is GrabCut), then GrabCut, each with its own icon.
    #[test]
    fn object_selection_method_follows_the_combine_row() {
        let ctx = egui::Context::default();
        let app = AppWorkspace::default();
        let defs = definitions(&ctx, ToolType::ObjectSelection, &app);
        let method = &defs[1];
        assert_eq!((method.key, method.extra), ("segmenter", true));
        assert_eq!(
            method.values,
            [
                (
                    "mobilesam",
                    "MobileSAM",
                    Some("object_selection_mobilesam_icon.png")
                ),
                (
                    "grabcut",
                    "GrabCut",
                    Some("object_selection_grabcut_icon.png")
                )
            ]
        );
        let default = app
            .tool_settings
            .get(ToolType::ObjectSelection, "segmenter");
        assert_eq!(face(method, &default).label, "MobileSAM");
    }

    /// compositing.md §2.7 "Toolbar list": AlphaBlending.True for the layer ops, False for Overwrite.
    #[test]
    fn blend_button_icon_follows_the_value() {
        let defs = option_definitions(ToolType::PaintBrush, &ToolSettings::default());
        let blend = defs.iter().find(|d| d.key == "blendMode").unwrap();
        let icon = |v: &str| {
            let value = Value::from(v);
            face(blend, &value).leading_icon.map(str::to_owned)
        };
        assert_eq!(icon("xor").as_deref(), Some("alpha_blending_true.png"));
        assert_eq!(
            icon("overwrite").as_deref(),
            Some("alpha_blending_false.png")
        );
    }
}
