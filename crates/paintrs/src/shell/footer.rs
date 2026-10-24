//! `#footerMenu`: the status bar, live: the texts follow the JS items' event listeners with
//! Paint.NET's numbers; the unit selector, zoom drop menu, zoom buttons and zoom slider act.
//! Port of: js/src/interface/panel/footer/FooterMenu.js,
//! footer/menu/{ToolInfoItem,CanvasSizeItem,CursorPositionItem,MeasurementUnitSelectorItem,
//! ViewZoomPercentageItem,ViewActualSizeItem,ViewZoomOutItem,ViewZoomSliderItem,ViewZoomInItem}.js,
//! js/src/interface/item/component/{SliderItem,TextFieldItem}.js, css/footer.css,
//! css/component.css (`input[type=range]`, `input[type=text]`), css/menu.css `.selector`,
//! `#windowResize` (index.html)
//! Implements Paint.NET's status bar (the selection info in the context
//! label; the size text "W × H", empty with no document; the cursor "x, y" of the pointer's ⌊document position⌋, "0, 0" when the document
//! list changes; in/cm "F2" numbers; the zoom text ⌊100·r⌉ %; the presets behind the buttons; the
//! zoom slider's preset index, click-steps and double-click); the units split button's ▲ is
//! Paint.NET's (`kit::glyph`); labels 105, slider 120, zoom box 48 and 12-DIP text.
//! Reference: paintdotjs @ b4e6fc5
use egui::{Color32, Id, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::app::{
    AppEvent, AppWorkspace, MeasurementUnit, ToolType, UnitValue, format_size, format_zoom,
};
use pdn_core::document::view::{zoom_from_slider_index, zoom_slider_index};
use pdn_core::util::geom::Point;

use crate::kit::aa;
use crate::kit::button::{self, IconItem, MenuItemStyle};
use crate::kit::glyph::{self, ArrowDir};
use crate::kit::img::{self, Filter};
use crate::kit::inputs;
use crate::kit::px;
use crate::kit::shadow::{self, BoxShadow, TexturePixels};
use crate::kit::text::{self, FontSpec};
use crate::theme::{self, *};

const SPEC: FontSpec = FontSpec::new(FOOTER_FONT_SIZE).line_height(FOOTER_LINE_HEIGHT);
/// `#zoomPercentageField`: 12 DIP, `line-height: normal`.
const FIELD_SPEC: FontSpec = FontSpec::new(FOOTER_FONT_SIZE);
/// The zoom slider's preset-index domain (the patched `getMax`).
const SLIDER_MAX: f64 = 44.0;
/// §2.10 "Zoom slider" step 3: a press off the thumb steps one preset, repeating every 200 ms.
const SLIDER_REPEAT_MS: f64 = 200.0;
/// S§4.6 (Extra): the Shift trail's hint (English, like the Extra names), its gap after the tool
/// info and the plus's after it (the effect title's).
const TRAIL_HINT: &str = "Shift: leave a trail";
const TRAIL_HINT_GAP: f32 = 16.0;
const TRAIL_PLUS_GAP: f32 = 4.0;

/// ViewZoomPercentageItem's open drop menu: its `TextFieldItem` (focused; `autoSelect` selects
/// the whole text until the first click inside it).
#[derive(Clone, Debug, PartialEq)]
struct ZoomField {
    text: String,
    selected: bool,
    /// The `{once: true}` document click listener (registered after the opening click).
    listener: bool,
}

/// What the status bar asks of the shell this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FooterOut {
    /// The unit selector's border box (the anchor of its drop menu).
    pub unit_selector: Rect,
    /// A click on the selector's arrow part (`MeasurementUnitSelectorItem.onPress`): open its menu.
    pub open_units: bool,
}

/// The `#footerMenu` items' state, kept like the JS items' `innerHTML` and inputs.
pub struct Footer {
    tool_info: String,
    /// `ToolInfoItem.getIconPath`: the selector entry's icon, or the selection icon.
    tool_icon: Option<&'static str>,
    /// `ToolInfoItem.showSelection`: the selection text replaces the tool's help.
    show_selection: bool,
    canvas_size: String,
    cursor_position: String,
    /// The cursor text follows a pointer move (not a document-list reset).
    cursor_live: bool,
    zoom_percentage: String,
    zoom_to_window: bool,
    /// MeasurementUnitSelectorItem's selection. quirk: it starts at pixel and follows only
    /// `app:update_measurement_unit`, so a restored inch/cm unit still reads "px".
    selected_unit: MeasurementUnit,
    /// The zoom slider's `input.value` (a preset index).
    slider_value: f64,
    /// Dragging the slider's thumb.
    slider_drag: bool,
    /// A press off the thumb: the step direction and when the next step repeats.
    slider_repeat: Option<(f64, f64)>,
    zoom_menu: Option<ZoomField>,
}

impl Default for Footer {
    /// The texts the items build with before any document exists.
    fn default() -> Self {
        Self {
            tool_info: String::new(),
            tool_icon: None,
            show_selection: false,
            canvas_size: String::new(),
            cursor_position: "0, 0".into(),
            cursor_live: false,
            zoom_percentage: "100%".into(),
            zoom_to_window: false,
            selected_unit: MeasurementUnit::Pixel,
            slider_value: 22.0,
            slider_drag: false,
            slider_repeat: None,
            zoom_menu: None,
        }
    }
}

/// `ToolInfoItem.getText`: Perspective Warp's hint while it runs, the selection text when
/// `show_selection` and a selection exists, else "{tool name}: {help text}".
fn tool_info_text(app: &AppWorkspace, show_selection: bool) -> String {
    let (Some(_), Some(tool)) = (app.active_ws(), app.tool_type) else {
        return String::new();
    };
    if let Some(hint) = pdn_core::tool::perspective_warp::status_text(app) {
        return hint.into();
    }
    if show_selection && let Some(text) = pdn_core::app::selection_info_text(app) {
        return text;
    }
    if let Some(status) = app.tool.as_ref().and_then(|t| t.status_text()) {
        return status.to_owned();
    }
    let id = tool.id();
    let t = pdn_core::i18n::t;
    // `i18n(id + ".helpText")`, or its `.text` when that is not a string.
    let key = format!("{id}.helpText");
    let mut help = t(&key);
    if help == key {
        help = t(&format!("{key}.text"));
    }
    pdn_core::i18n::tf(
        "statusBar.context.help.text.format",
        &[&t(&format!("{id}.name")), &help],
    )
}

/// `String(app.toUnit(v))`.
pub(crate) fn unit_text(v: UnitValue) -> String {
    match v {
        UnitValue::Pixels(p) => pdn_core::app::format_integer(p),
        UnitValue::Formatted(s) => s,
    }
}

/// JS `parseInt(text.replace("%", ""))`: the leading (signed) integer after whitespace.
fn parse_int(text: &str) -> Option<f64> {
    let t = text.replacen('%', "", 1);
    let t = t.trim_start();
    let digits = t
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')))
        .count();
    t[..digits].parse::<f64>().ok()
}

/// Blink's range input (`SliderThumbElement::SetPositionFromPoint`): the thumb's centre follows
/// the pointer over the track width less the thumb, the value rounds to the step.
fn slider_value_at(x: f32, input: Rect) -> f64 {
    let tw = SLIDER_THUMB.0;
    let span = input.width() - tw;
    let pos = (x - input.min.x - tw / 2.0).clamp(0.0, span);
    (SLIDER_MAX * (pos / span) as f64).round()
}

/// The thumb's border box at `value` (unsnapped): Blink's left = (W − tw)·fraction; `top: 50%` of
/// the 1-px track centred in the input, then `translateY(-50%)`.
fn slider_thumb(input: Rect, value: f64) -> Rect {
    let (tw, th) = SLIDER_THUMB;
    let left = input.min.x + (input.width() - tw) * (value / SLIDER_MAX) as f32;
    let track = input.min.y + (RANGE_INPUT.1 - RANGE_TRACK_HEIGHT) / 2.0;
    let top = track + RANGE_TRACK_HEIGHT / 2.0 - th / 2.0;
    Rect::from_min_size(Pos2::new(left, top), Vec2::new(tw, th))
}

impl Footer {
    pub fn on_event(&mut self, event: &AppEvent, app: &AppWorkspace) {
        use AppEvent::*;
        if matches!(event, UpdateMeasurementUnit) {
            self.selected_unit = app.measurement_unit;
        }
        if matches!(
            event,
            UpdateActiveDocument | UpdateSize { .. } | UpdateMeasurementUnit
        ) {
            // §2.10 "Size": empty with no document.
            self.canvas_size = match app.active_ws() {
                Some(ws) => format_size(
                    &unit_text(app.to_unit(ws.view.get_width())),
                    &unit_text(app.to_unit(ws.view.get_height())),
                ),
                None => String::new(),
            };
        }
        // ToolInfoItem: `app:active_tool_updated` reinitializes (Move tools and Magic Wand keep
        // showing the selection), a selection change or a unit change shows the selection; the
        // document and history events update.
        match event {
            ActiveToolUpdated => {
                self.show_selection = matches!(
                    app.tool_type,
                    Some(ToolType::Move | ToolType::MoveSelection | ToolType::MagicWand)
                );
            }
            SelectionChanged | UpdateMeasurementUnit => self.show_selection = true,
            ToolStatusChanged => self.show_selection = false,
            _ => {}
        }
        if matches!(
            event,
            ActiveToolUpdated
                | ToolStatusChanged
                | UpdateActiveDocument
                | UpdateSize { .. }
                | UpdateMeasurementUnit
                | SelectionChanged
                | HistoryChanged
        ) {
            let selection =
                self.show_selection && app.active_ws().is_some_and(|ws| !ws.selection.is_empty());
            self.tool_icon = if selection {
                Some("selection_icon.png")
            } else {
                app.tool_type.map(|t| t.icon())
            };
            self.tool_info = tool_info_text(app, self.show_selection);
        }
        if matches!(
            event,
            UpdateActiveDocument | MouseMove { .. } | UpdateMeasurementUnit
        ) {
            self.cursor_position = match app.active_ws() {
                Some(ws) => {
                    let (x, y) = app.last_mouse;
                    let p = ws.view.to_document_position(Point::new(x, y), false);
                    format!(
                        "{}, {}",
                        unit_text(app.to_unit(p.x)),
                        unit_text(app.to_unit(p.y))
                    )
                }
                None => "0, 0".into(),
            };
        }
        // §2.10 "Cursor": "0, 0" whenever the document list changes.
        if matches!(event, CreateDocument | CloseDocument) {
            self.cursor_position = "0, 0".into();
        }
        if matches!(event, MouseMove { .. } | CreateDocument | CloseDocument) {
            self.cursor_live = matches!(event, MouseMove { .. });
        }
        if matches!(event, UpdateActiveDocument | UpdateViewport) {
            let ws = app.active_ws();
            self.zoom_to_window = ws.is_some_and(|ws| ws.view.zoom_to_window);
            if matches!(event, UpdateViewport) {
                self.zoom_percentage = ws.map_or("100%".into(), |ws| format_zoom(ws.view.zoom));
                // `SliderItem.updateCurrentValue`: `Math.round`.
                self.slider_value = ws.map_or(22.0, |ws| zoom_slider_index(ws.view.zoom).round());
                // `textField.setText`: a new value drops the selection.
                if let Some(field) = &mut self.zoom_menu
                    && field.text != self.zoom_percentage
                {
                    field.text = self.zoom_percentage.clone();
                    field.selected = false;
                }
            }
        }
    }

    /// The cursor text shows a pointer position (a view change moves it).
    pub fn cursor_live(&self) -> bool {
        self.cursor_live
    }

    /// The unit selector's selection (its drop menu checks that entry).
    pub fn selected_unit(&self) -> MeasurementUnit {
        self.selected_unit
    }

    /// The zoom field has the keyboard (AppView skips shortcuts while an INPUT has focus).
    pub fn owns_keys(&self) -> bool {
        self.zoom_menu.is_some()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        app: &mut AppWorkspace,
        units_open: bool,
    ) -> FooterOut {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        let bg = theme.menu_background;
        painter.rect_filled(px::snap_rect(rect, ppp), 0.0, bg);
        let (grip, ml, mr, mt) = WINDOW_RESIZE;
        let menu_right = rect.max.x - (grip + ml + mr);
        // Text items (`.menu-item`) carry 1 px margins on every side and a 1 px border; icon
        // items drop the side margins (`.icon-item { margin: 0 }`, top/bottom come back from
        // `#footerMenu .menu-item`); the `.label-menu-item` wrappers have none.
        let (m, b) = (MENU_ITEM_MARGIN, MENU_ITEM_BORDER);
        let item_y = rect.min.y + m;
        let text_y = item_y + b + FOOTER_ITEM_PADDING.1;
        // `.label-menu-item { align-items: center }` over the 32 px footer.
        let icon_y = rect.min.y + (FOOTER_HEIGHT - FOOTER_ICON) / 2.0;
        let pad = 2.0 * (FOOTER_ITEM_PADDING.0 + b);
        let measure = |s: &str| text::measure(&painter, s, SPEC);

        // Right to left: everything right of #toolInfoItem has a fixed or content width.
        let zoom_in = menu_right - FOOTER_ICON_ITEM;
        let slider = zoom_in - (RANGE_INPUT.0 + 2.0 * RANGE_INPUT_MARGIN_X);
        let zoom_out = slider - FOOTER_ICON_ITEM;
        let actual = zoom_out - FOOTER_ICON_ITEM;
        let zoom_w = measure(&self.zoom_percentage) + pad;
        // #zoomPercentageItem's right margin (its neighbour is an icon item: no margin).
        let zoom_x = actual - m - zoom_w;
        let unit = pdn_core::i18n::t(&format!(
            "measurementUnit.{}.abbreviation",
            self.selected_unit.id()
        ));
        let (sep_w, sep_m) = SELECTOR_SEPARATOR;
        let (arrow_w, arrow_h, arrow_m) = SELECTOR_ARROW;
        let unit_w = measure(&unit) + sep_w + 2.0 * sep_m + arrow_w + 2.0 * arrow_m + pad;
        // Two text items meet: #zoomPercentageItem's left margin + #measurementUnit's right one.
        let unit_x = zoom_x - m - m - unit_w;
        // #measurementUnit's left margin; the #cursorPositionItem wrapper has none.
        let cursor_x = unit_x - m - CURSOR_POSITION_ITEM_WIDTH;
        let canvas_x = cursor_x - CANVAS_SIZE_ITEM_WIDTH;

        // #toolInfoItem: icon (margin 0 3px 0 5px), then the label with ellipsis.
        let icon_x = rect.min.x + FOOTER_TOOL_ICON_MARGIN.0;
        if let Some(tool_icon) = self.tool_icon {
            label_icon(ui, tool_icon, Pos2::new(icon_x, icon_y), bg);
        }
        // The `.label` div's own 1 px side margins.
        let label_x = icon_x + FOOTER_ICON + FOOTER_TOOL_ICON_MARGIN.1 + m;
        // quirk: no `text-overflow: ellipsis` shows in the references; the `.label`'s
        // `overflow: hidden` clips the text at its padding box, which ends its 1 px margin and
        // border before #canvasSizeItem.
        let clip = Rect::from_min_max(rect.min, Pos2::new(canvas_x - m - b, rect.max.y));
        let label = painter.with_clip_rect(px::snap_rect(clip, ppp));
        let info_x = label_x + b + FOOTER_ITEM_PADDING.0;
        text::draw(
            &label,
            Pos2::new(info_x, text_y),
            &self.tool_info,
            SPEC,
            theme.menu_item_text,
        );
        // S§4.6 (Extra): while Move Selected Pixels translates without Shift, the trail's hint
        // after the tool info, then the plus.
        if !app.shift_key_down && app.tool.as_ref().is_some_and(|t| t.shift_trail_hint()) {
            let x = info_x + measure(&self.tool_info) + TRAIL_HINT_GAP;
            text::draw(
                &label,
                Pos2::new(x, text_y),
                TRAIL_HINT,
                SPEC,
                theme.menu_item_text,
            );
            let (w, h) = PLUGIN_INDICATOR_SIZE;
            let plus = Rect::from_min_size(
                Pos2::new(
                    x + measure(TRAIL_HINT) + TRAIL_PLUS_GAP,
                    rect.center().y - h / 2.0,
                ),
                Vec2::new(w, h),
            );
            let plus = px::snap_rect(plus, ppp);
            img::paint_icon(&label, "plugin_indicator.png", plus, bg, Filter::None);
        }
        // #canvasSizeItem / #cursorPositionItem: icon then label (no icon margin).
        for (x, w, icon, label) in [
            (
                canvas_x,
                CANVAS_SIZE_ITEM_WIDTH,
                "image_size_icon.png",
                &self.canvas_size,
            ),
            (
                cursor_x,
                CURSOR_POSITION_ITEM_WIDTH,
                "cursor_x_y_icon.png",
                &self.cursor_position,
            ),
        ] {
            label_icon(ui, icon, Pos2::new(x, icon_y), bg);
            let clip = Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(w, FOOTER_HEIGHT));
            let tx = x + FOOTER_ICON + m + b + FOOTER_ITEM_PADDING.0;
            text::draw(
                &painter.with_clip_rect(clip),
                Pos2::new(tx, text_y),
                label,
                SPEC,
                theme.menu_item_text,
            );
        }

        // #measurementUnit selector: label, separator (shown at 0.2 on hover), arrow-up.
        let unit_box = Rect::from_min_size(
            Pos2::new(unit_x, item_y),
            Vec2::new(unit_w, FOOTER_ICON_ITEM),
        );
        let snapped = px::snap_rect(unit_box, ppp);
        let response = ui.interact(
            snapped,
            Id::unique("footer-measurement-unit"),
            Sense::click(),
        );
        let state = MenuItemStyle::of(response.hovered(), units_open, false, true);
        let fill = button::paint_menu_item_frame(&painter, snapped, state, &theme, bg);
        let unit_text_x = unit_x + b + FOOTER_ITEM_PADDING.0;
        text::draw(
            &painter,
            Pos2::new(unit_text_x, text_y),
            &unit,
            SPEC,
            theme.menu_item_text,
        );
        let sep_x = unit_text_x + measure(&unit) + sep_m;
        if response.hovered() {
            // `#footerMenu .selector .separator { height: 18px }`, centred in the content box.
            let sep = Rect::from_min_size(
                Pos2::new(
                    sep_x,
                    text_y + (FOOTER_LINE_HEIGHT - FOOTER_SELECTOR_SEPARATOR) / 2.0,
                ),
                Vec2::new(sep_w, FOOTER_SELECTOR_SEPARATOR),
            );
            let alpha = px::css_alpha(TOOL_SELECTOR_SEPARATOR_HOVER_OPACITY);
            painter.rect_filled(
                px::snap_rect(sep, ppp),
                0.0,
                px::blend(fill, theme.title_bar_border, alpha),
            );
        }
        // Paint.NET's ▲ in the `.arrow-up` border triangle's box (apex on its top row).
        let arrow = Rect::from_min_size(
            Pos2::new(
                sep_x + sep_w + sep_m + arrow_m,
                text_y + (FOOTER_LINE_HEIGHT - arrow_h - 2.0 * arrow_m) / 2.0 + arrow_m,
            ),
            Vec2::new(arrow_w, arrow_h),
        );
        glyph::paint_arrow_in_box(&painter, ArrowDir::Up, arrow, theme.menu_item_text, fill);
        let mut out = FooterOut {
            unit_selector: unit_box,
            open_units: false,
        };
        if response.clicked() {
            // `onPress`: right of the separator opens the menu, else the next unit.
            let x = inputs::interact_pos(ui, &response).map_or(0.0, |p| p.x);
            if x >= sep_x + sep_w {
                out.open_units = true;
            } else {
                let i = MeasurementUnit::ALL
                    .iter()
                    .position(|&u| u == self.selected_unit)
                    .unwrap_or(0);
                let next = MeasurementUnit::ALL[(i + 1) % MeasurementUnit::ALL.len()];
                self.selected_unit = next;
                app.set_measurement_unit(next);
            }
        }

        // #zoomPercentageItem: a DropMenuItem holding the zoom field.
        let zoom_box = Rect::from_min_size(
            Pos2::new(zoom_x, item_y),
            Vec2::new(zoom_w, FOOTER_ICON_ITEM),
        );
        let snapped = px::snap_rect(zoom_box, ppp);
        let response = ui.interact(
            snapped,
            Id::unique("footer-zoom-percentage"),
            Sense::click(),
        );
        let state = MenuItemStyle::of(response.hovered(), self.zoom_menu.is_some(), false, true);
        button::paint_menu_item_frame(&painter, snapped, state, &theme, bg);
        text::draw(
            &painter,
            Pos2::new(zoom_x + b + FOOTER_ITEM_PADDING.0, text_y),
            &self.zoom_percentage,
            SPEC,
            theme.menu_item_text,
        );
        if response.clicked() && self.zoom_menu.is_none() {
            self.zoom_menu = Some(ZoomField {
                text: self.zoom_percentage.clone(),
                selected: true,
                listener: false,
            });
        }

        // Icon items: ViewActualSizeItem (icon follows Fit to Window), zoom out, zoom in.
        let actual_icon = if self.zoom_to_window {
            "menu_view_zoom_to_window_icon.png"
        } else {
            "menu_view_actual_size_icon.png"
        };
        for (id, x, icon) in [
            ("menuViewActualSize", actual, actual_icon),
            ("menuViewZoomOut", zoom_out, "menu_view_zoom_out_icon.png"),
            ("menuViewZoomIn", zoom_in, "menu_view_zoom_in_icon.png"),
        ] {
            let item = IconItem {
                id,
                rect: Rect::from_min_size(Pos2::new(x, item_y), Vec2::splat(FOOTER_ICON_ITEM)),
                icon,
                icon_size: FOOTER_ICON,
                enabled: true,
                active: false,
                open: false,
            };
            if button::icon_item(ui, &item, bg).clicked() {
                pdn_core::action::perform(app, id);
            }
        }
        // `.slider-wrapper { height: 30px; align-items: center }` in the 32 px footer.
        let wrapper = Rect::from_min_size(
            Pos2::new(
                slider,
                rect.min.y + (FOOTER_HEIGHT - SLIDER_WRAPPER_HEIGHT) / 2.0,
            ),
            Vec2::new(
                RANGE_INPUT.0 + 2.0 * RANGE_INPUT_MARGIN_X,
                SLIDER_WRAPPER_HEIGHT,
            ),
        );
        self.zoom_slider(ui, wrapper, app, &theme);
        // align-items: center over the grip plus its top margin.
        let grip_y = rect.min.y + (FOOTER_HEIGHT - grip - mt) / 2.0 + mt;
        paint_resize_grip(ui, Pos2::new(menu_right + ml, grip_y), bg);
        out
    }

    /// ViewZoomSliderItem: paintdotjs's range input over the 45 presets (`ui-drawn-graphics.md
    /// §2.10` "Zoom slider" steps 2–3): dragging the thumb picks the preset under it, a press
    /// off the thumb steps one preset towards the pointer and repeats every 200 ms, a
    /// double-click on the thumb returns to 100 %.
    fn zoom_slider(
        &mut self,
        ui: &mut Ui,
        wrapper: Rect,
        app: &mut AppWorkspace,
        theme: &theme::Theme,
    ) {
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        let input = Rect::from_min_size(
            Pos2::new(
                wrapper.min.x + RANGE_INPUT_MARGIN_X,
                wrapper.min.y + (SLIDER_WRAPPER_HEIGHT - RANGE_INPUT.1) / 2.0,
            ),
            Vec2::new(RANGE_INPUT.0, RANGE_INPUT.1),
        );
        let response = ui.interact(
            px::snap_rect(input, ppp),
            Id::unique("footer-zoom-slider"),
            Sense::click_and_drag(),
        );
        let thumb = slider_thumb(input, self.slider_value);
        let pointer = inputs::interact_pos(ui, &response);
        let mut value = self.slider_value;
        if response.double_clicked() && pointer.is_some_and(|p| thumb.contains(p)) {
            value = 22.0;
        } else if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
        {
            if let Some(p) = pointer {
                if thumb.contains(p) {
                    self.slider_drag = true;
                } else {
                    let dir = if p.x < thumb.center().x { -1.0 } else { 1.0 };
                    value += dir;
                    self.slider_repeat = Some((dir, app.now_ms + SLIDER_REPEAT_MS));
                }
            }
        } else if self.slider_drag {
            if let Some(p) = pointer {
                value = slider_value_at(p.x, input);
            }
        } else if let (Some((dir, at)), Some(p)) = (self.slider_repeat, pointer)
            && response.is_pointer_button_down_on()
            && app.now_ms >= at
        {
            // Repeats while the pointer stays beyond the thumb on that side.
            if (p.x - thumb.center().x) as f64 * dir > SLIDER_THUMB.0 as f64 / 2.0 {
                value += dir;
            }
            self.slider_repeat = Some((dir, at + SLIDER_REPEAT_MS));
        }
        if !response.is_pointer_button_down_on() {
            self.slider_drag = false;
            self.slider_repeat = None;
        }
        if let Some((_, at)) = self.slider_repeat {
            let ms = (at - app.now_ms).max(0.0);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(ms / 1000.0));
        }
        let value = value.clamp(0.0, SLIDER_MAX);
        if value != self.slider_value {
            self.slider_value = value;
            // `onChange` → `setZoom(preset)` about the viewport centre.
            if let Some(ws) = app.active_ws_mut() {
                ws.view
                    .set_zoom_center(zoom_from_slider_index(value as u32));
            }
        }
        let bg = theme.menu_background;
        // The runnable track is centred in the input box (snapped to device px), its box-shadow
        // right below it.
        let track_y =
            ((input.min.y + (RANGE_INPUT.1 - RANGE_TRACK_HEIGHT) / 2.0) * ppp).round() / ppp;
        let track = Rect::from_min_size(
            Pos2::new(input.min.x, track_y),
            Vec2::new(RANGE_INPUT.0, RANGE_TRACK_HEIGHT),
        );
        painter.rect_filled(px::snap_rect(track, ppp), 0.0, theme.slider_outline);
        painter.rect_filled(
            px::snap_rect(track.translate(Vec2::new(0.0, RANGE_TRACK_HEIGHT)), ppp),
            0.0,
            theme.slider,
        );
        // `.slider-tick` at 50 % of the wrapper (`top: 12px`, `translate(-50%, -25%)`), above the
        // track, below the thumb.
        let (tick_w, tick_h) = (crate::theme::lu(SLIDER_TICK.0, ppp), SLIDER_TICK.1);
        let tick = Rect::from_min_size(
            Pos2::new(
                wrapper.center().x - tick_w / 2.0,
                wrapper.min.y + FOOTER_SLIDER_TICK_TOP - tick_h / 4.0,
            ),
            Vec2::new(tick_w, tick_h),
        );
        let corners = [tick.min, tick.right_top(), tick.max, tick.left_bottom()];
        let (tick_at, tick_rows) = aa::polygon_coverage(&corners.map(|q| q * ppp));
        aa::paint_coverage(&painter, tick_at / ppp, &tick_rows, theme.slider_tick, bg);
        // What lies under the thumb, per device pixel: the footer, the track and its shadow, the
        // tick.
        let track_row = (track_y * ppp).round() as i32;
        let track_rows = px::border_width(RANGE_TRACK_HEIGHT, ppp).round() as i32;
        let backdrop = |x: i32, y: i32| {
            let c = match y - track_row {
                d if (0..track_rows).contains(&d) => theme.slider_outline,
                d if (track_rows..2 * track_rows).contains(&d) => theme.slider,
                _ => bg,
            };
            let (i, j) = (x - tick_at.x.round() as i32, y - tick_at.y.round() as i32);
            let a = usize::try_from(j)
                .ok()
                .zip(usize::try_from(i).ok())
                .and_then(|(j, i)| tick_rows.get(j)?.get(i).copied())
                .unwrap_or(0);
            px::blend(c, theme.slider_tick, a)
        };
        let thumb = slider_thumb(input, self.slider_value);
        let hovered = response
            .hover_pos()
            .or(pointer)
            .is_some_and(|p| thumb.contains(p));
        let state = (hovered, self.slider_drag);
        paint_thumb(&painter, thumb, input.min.x, theme, state, &backdrop);
    }

    /// ViewZoomPercentageItem's open drop menu over everything (`.drop-menu` holding the focused
    /// zoom field), and its keys and document click listener. Closing applies the field's zoom.
    pub fn show_zoom_menu(
        &mut self,
        ui: &mut Ui,
        footer: Rect,
        app: &mut AppWorkspace,
        pixels: TexturePixels,
    ) {
        let shadow_id = Id::unique("footer-zoom-menu-shadow");
        let Some(mut field) = self.zoom_menu.take() else {
            shadow::forget(ui.ctx(), shadow_id);
            return;
        };
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().clone();
        let ppp = painter.pixels_per_point();
        // The item's box (the anchor): recomputed as `show` laid it out.
        let anchor = self.zoom_item_rect(&painter, footer);
        let (fw, fh) = ZOOM_FIELD;
        let b = DROP_MENU_BORDER;
        let size = Vec2::new(fw + 2.0 * b, fh + 2.0 * b);
        // `positionAtAnchor({alignEnd: right > innerWidth / 2, dropUp: true})`.
        let screen = ui.ctx().content_rect();
        let x = if anchor.max.x > screen.width() / 2.0 {
            anchor.max.x - size.x
        } else {
            anchor.min.x
        };
        let popup = Rect::from_min_size(Pos2::new(x, anchor.min.y - size.y), size);
        let snapped = px::snap_rect(popup, ppp);
        shadow::paint_box_shadow(
            ui,
            shadow_id,
            snapped,
            BoxShadow {
                offset: Vec2::splat(DROP_MENU_SHADOW_OFFSET),
                blur: DROP_MENU_SHADOW_BLUR,
                color: DROP_MENU_SHADOW_COLOR,
            },
            pixels,
        );
        let input = Rect::from_min_size(popup.min + Vec2::splat(b), Vec2::new(fw, fh));
        let ring = px::snap_rect(input.expand(ZOOM_FIELD_FOCUS_RING), ppp);
        text::occlude(ui.ctx(), snapped.union(ring));
        ui.interact(
            snapped,
            Id::unique("footer-zoom-menu"),
            Sense::click_and_drag(),
        );
        painter.rect_filled(snapped, 0.0, theme.title_bar_border);
        painter.rect_filled(
            snapped.shrink(px::border_width(b, ppp)),
            0.0,
            theme.menu_background,
        );
        // `input[type="text"]:focus-visible { box-shadow: 0 0 0 2px }`, the 1-px border, the field.
        painter.rect_filled(ring, 0.0, theme.menu_item_border);
        let field_box = px::snap_rect(input, ppp);
        painter.rect_filled(field_box, 0.0, theme.menu_item_text);
        painter.rect_filled(
            field_box.shrink(px::border_width(1.0, ppp)),
            0.0,
            theme.menu_background,
        );
        let (pad_x, pad_y) = ZOOM_FIELD_PADDING;
        let content = input.shrink2(Vec2::new(1.0 + pad_x, 1.0 + pad_y));
        let top = content.min.y + (content.height() - FIELD_SPEC.resolved_line_height(ppp)) / 2.0;
        let w = text::measure(&painter, &field.text, FIELD_SPEC);
        if field.selected {
            let sel = Rect::from_min_size(
                content.min,
                Vec2::new(w.min(content.width()), content.height()),
            );
            painter.rect_filled(
                px::snap_rect(sel, ppp),
                0.0,
                crate::theme::text_selection(ui.ctx()),
            );
        }
        let clip = painter.with_clip_rect(content);
        text::draw(
            &clip,
            Pos2::new(content.min.x, top),
            &field.text,
            FIELD_SPEC,
            if field.selected {
                Color32::WHITE
            } else {
                theme.menu_item_text
            },
        );
        if !field.selected {
            // The caret after the text.
            let caret = Rect::from_min_size(
                Pos2::new(content.min.x + w, top),
                Vec2::new(1.0, FIELD_SPEC.resolved_line_height(ppp)),
            );
            clip.rect_filled(px::snap_rect(caret, ppp), 0.0, theme.menu_item_text);
        }
        // Keys: typing (or pasting) replaces the selection; Backspace; Enter submits (`close()`).
        // ponytail: an append-only editor (no caret movement, IME or undo); back the field with
        // a real text editor if zoom entry needs more than typing a number.
        let mut submit = false;
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(t) | egui::Event::Paste(t) => {
                    if std::mem::take(&mut field.selected) {
                        field.text.clear();
                    }
                    field.text.push_str(&t);
                }
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    if std::mem::take(&mut field.selected) {
                        field.text.clear();
                    } else {
                        field.text.pop();
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => submit = true,
                _ => {}
            }
        }
        // The document click listener; a click in the field only places the caret.
        let click = inputs::primary_click_pos(ui);
        let mut close = submit;
        if let Some(p) = click {
            if field_box.contains(p) {
                field.selected = false;
            } else if field.listener {
                close = true;
            }
        }
        field.listener = true;
        if close {
            shadow::forget(ui.ctx(), shadow_id);
            if let (Some(zoom), Some(ws)) = (parse_int(&field.text), app.active_ws_mut()) {
                ws.view.set_zoom_center(zoom / 100.0);
            }
            ui.ctx().request_repaint();
        } else {
            self.zoom_menu = Some(field);
        }
    }

    /// The #zoomPercentageItem border box in `footer` (as `show` lays it out).
    fn zoom_item_rect(&self, painter: &egui::Painter, footer: Rect) -> Rect {
        let (grip, ml, mr, _) = WINDOW_RESIZE;
        let menu_right = footer.max.x - (grip + ml + mr);
        let actual =
            menu_right - 3.0 * FOOTER_ICON_ITEM - (RANGE_INPUT.0 + 2.0 * RANGE_INPUT_MARGIN_X);
        let pad = 2.0 * (FOOTER_ITEM_PADDING.0 + MENU_ITEM_BORDER);
        let w = text::measure(painter, &self.zoom_percentage, SPEC) + pad;
        Rect::from_min_size(
            Pos2::new(
                actual - MENU_ITEM_MARGIN - w,
                footer.min.y + MENU_ITEM_MARGIN,
            ),
            Vec2::new(w, FOOTER_ICON_ITEM),
        )
    }
}

/// The `::-webkit-slider-thumb` (10×14, 1-px border, `clip-path: polygon(100% 0, 100% 60%, 50%
/// 100%, 0 60%, 0 0)`) at Blink's fractional position: the border box snapped to device pixels
/// (its offset in the track rounds half to even, as the references' odd presets show), clipped by
/// the unsnapped polygon's coverage over `backdrop`. `(hovered, pressed)`: `:hover` fills a
/// `linear-gradient(hover1, hover2)` with the hover outline, `:active` the reversed gradient.
fn paint_thumb(
    painter: &egui::Painter,
    thumb: Rect,
    track_left: f32,
    theme: &theme::Theme,
    (hovered, pressed): (bool, bool),
    backdrop: &dyn Fn(i32, i32) -> Color32,
) {
    let ppp = painter.pixels_per_point();
    let x0 = (track_left * ppp).round() + ((thumb.min.x - track_left) * ppp).round_ties_even();
    let y0 = (thumb.min.y * ppp).round();
    let size = (thumb.size() * ppp).round();
    let border = Rect::from_min_size(Pos2::new(x0, y0) / ppp, size / ppp);
    let pad = border.shrink(px::border_width(1.0, ppp));
    let device = |r: Rect| {
        [
            (r.min.x * ppp).round() as i32,
            (r.min.y * ppp).round() as i32,
            (r.max.x * ppp).round() as i32,
            (r.max.y * ppp).round() as i32,
        ]
    };
    let [bx0, by0, bx1, by1] = device(border);
    let [px0, py0, px1, py1] = device(pad);
    let (outline, fill): (Color32, Vec<Vec<Color32>>) = if hovered || pressed {
        let stops = if pressed {
            (theme.slider_hover2, theme.slider_hover1)
        } else {
            (theme.slider_hover1, theme.slider_hover2)
        };
        (
            theme.slider_hover_outline,
            inputs::gradient_pixels(pad, stops, ppp),
        )
    } else {
        let row = vec![theme.slider; (px1 - px0).max(0) as usize];
        (theme.slider_outline, vec![row; (py1 - py0).max(0) as usize])
    };
    let poly = [
        thumb.min,
        thumb.right_top(),
        Pos2::new(thumb.max.x, thumb.min.y + thumb.height() * 0.6),
        Pos2::new(thumb.center().x, thumb.max.y),
        Pos2::new(thumb.min.x, thumb.min.y + thumb.height() * 0.6),
    ]
    .map(|q| q * ppp);
    let (at, cover) = aa::polygon_area_coverage(&poly);
    let (ax, ay) = (at.x.round() as i32, at.y.round() as i32);
    for (j, row) in cover.iter().enumerate() {
        for (i, &c) in row.iter().enumerate() {
            let (x, y) = (ax + i as i32, ay + j as i32);
            if c <= 0.0 || x < bx0 || x >= bx1 || y < by0 || y >= by1 {
                continue;
            }
            let base = if (px0..px1).contains(&x) && (py0..py1).contains(&y) {
                fill[(y - py0) as usize][(x - px0) as usize]
            } else {
                outline
            };
            let a = (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let p = Pos2::new(x as f32, y as f32) / ppp;
            painter.rect_filled(
                Rect::from_min_size(p, Vec2::splat(1.0 / ppp)),
                0.0,
                px::blend(backdrop(x, y), base, a),
            );
        }
    }
}

/// A `.label-menu-item .icon` (16 DIP in the footer) at `at`.
fn label_icon(ui: &Ui, icon: &str, at: Pos2, bg: Color32) {
    let p = ui.painter();
    let r = px::snap_rect(
        Rect::from_min_size(at, Vec2::splat(FOOTER_ICON)),
        p.pixels_per_point(),
    );
    img::paint_icon(p, icon, r, bg, Filter::None);
}

/// `#windowResize`: the 11×11 SVG grip (2×2 white squares under `#a0a0a0` ones offset by
/// (−1, −1)) as a 14×14 background over `bg`: each square's exact-area coverage at the device
/// scale, composited in SVG order. Chromium's SVG raster lands within R21's edge rule of it (at
/// most 26 off at 1×, every pixel an edge of these 2.5-px squares).
fn paint_resize_grip(ui: &Ui, at: Pos2, bg: Color32) {
    let painter = ui.painter();
    let ppp = painter.pixels_per_point();
    let origin = (at * ppp).round();
    let (n, px) = grip_pixels(ppp, bg);
    for (idx, &c) in px.iter().enumerate() {
        let p = (origin + Vec2::new((idx % n) as f32, (idx / n) as f32)) / ppp;
        painter.rect_filled(Rect::from_min_size(p, Vec2::splat(1.0 / ppp)), 0.0, c);
    }
}

/// The SVG's squares (viewBox units) and grey levels, in paint order.
const GRIP_SQUARES: [(f32, f32, u8); 12] = [
    (9.0, 1.0, 0xFF),
    (5.0, 5.0, 0xFF),
    (9.0, 5.0, 0xFF),
    (1.0, 9.0, 0xFF),
    (5.0, 9.0, 0xFF),
    (9.0, 9.0, 0xFF),
    (8.0, 0.0, 0xA0),
    (4.0, 4.0, 0xA0),
    (8.0, 4.0, 0xA0),
    (0.0, 8.0, 0xA0),
    (4.0, 8.0, 0xA0),
    (8.0, 8.0, 0xA0),
];

/// The grip's `n × n` device pixels, row-major.
fn grip_pixels(ppp: f32, bg: Color32) -> (usize, Vec<Color32>) {
    let k = WINDOW_RESIZE.0 / 11.0 * ppp;
    let n = (WINDOW_RESIZE.0 * ppp).ceil() as usize;
    let overlap = |p: usize, a: f32| ((p + 1) as f32).min((a + 2.0) * k) - (p as f32).max(a * k);
    let mut px = vec![[bg.r(), bg.g(), bg.b()].map(f32::from); n * n];
    for (x, y, v) in GRIP_SQUARES {
        for (idx, c) in px.iter_mut().enumerate() {
            let cover = overlap(idx % n, x).max(0.0) * overlap(idx / n, y).max(0.0);
            *c = c.map(|ch| ch + (v as f32 - ch) * cover);
        }
    }
    let px = px.iter().map(|c| {
        let [r, g, b] = c.map(|ch| ch.round() as u8);
        Color32::from_rgb(r, g, b)
    });
    (n, px.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_status_is_event_driven_and_document_owned() {
        use pdn_core::tool::{MouseButton, PointerInput, PointerType, text::TextTool};
        let mut app = AppWorkspace::default();
        let a = app.create_blank_document_in_new_workspace(120, 80, 96.0, false);
        app.set_active_tool_from_type(ToolType::Text);
        let mut footer = Footer::default();
        let sync = |f: &mut Footer, a: &mut AppWorkspace| {
            for e in a.drain_events() {
                f.on_event(&e, a);
            }
        };
        sync(&mut footer, &mut app);
        assert!(footer.tool_info.starts_with("Text:"));
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        app.with_tool(|t, a| {
            t.on_mouse_down(a, 20.0, 20.0, MouseButton::Left, &input);
            t.as_any_mut()
                .downcast_mut::<TextTool>()
                .unwrap()
                .insert_text(a, "A");
        });
        sync(&mut footer, &mut app);
        let instruction = footer.tool_info.clone();
        assert!(instruction.starts_with("Drag the handle"));
        app.with_tool(|t, a| {
            t.on_mouse_down(a, 80.0, 50.0, MouseButton::Right, &input);
        });
        sync(&mut footer, &mut app);
        let tracked = footer.tool_info.clone();
        assert!(tracked.starts_with("Anchor point:"));
        app.set_measurement_unit(MeasurementUnit::Inch);
        sync(&mut footer, &mut app);
        assert_eq!(
            footer.tool_info, tracked,
            "units event queries cached Text status"
        );
        // Another status owner can replace Text. A frame without a Text trigger cannot reclaim it.
        app.active_ws_mut().unwrap().selection.set_continuation(
            &pdn_core::util::geom::Rectangle::new(1.0, 2.0, 10.0, 8.0),
            pdn_core::selection::CombineMode::Replace,
        );
        sync(&mut footer, &mut app);
        let selection_status = footer.tool_info.clone();
        assert_ne!(selection_status, tracked);
        app.with_tool(|t, a| t.on_frame(a));
        sync(&mut footer, &mut app);
        assert_eq!(footer.tool_info, selection_status);
        app.with_tool(|t, a| {
            t.on_mouse_up(a, 80.0, 50.0, MouseButton::Right, &input);
        });
        sync(&mut footer, &mut app);
        assert_eq!(footer.tool_info, instruction);
        let b = app.create_blank_document_in_new_workspace(120, 80, 96.0, false);
        sync(&mut footer, &mut app);
        assert!(footer.tool_info.starts_with("Text:"));
        assert_eq!(app.active, Some(b));
        app.set_active_document_workspace(Some(a));
        sync(&mut footer, &mut app);
        assert_eq!(footer.tool_info, instruction);
        app.with_tool(|t, a| t.commit_pending(a));
        sync(&mut footer, &mut app);
        assert_eq!(
            footer.tool_info, instruction,
            "stop does not restore activation help"
        );
    }

    /// The grip geometry against Chromium's 1× raster on the dark footer (the measured block it
    /// replaced): within R21's edge rule everywhere; at 2× it covers 28 device px.
    #[test]
    fn grip_geometry_matches_dpr1_raster() {
        #[rustfmt::skip]
        const MEASURED: [[u8; 14]; 14] = [
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x88, 0xa0, 0x7c, 0x20],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x88, 0xa0, 0xaa, 0xc8],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x54, 0x9d, 0xdc, 0xff],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x7b, 0xc8, 0xc8],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x94, 0xa0, 0x70, 0x20, 0x20, 0x88, 0xa0, 0x7c, 0x20],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x94, 0xa0, 0xae, 0xb8, 0x20, 0x88, 0xa0, 0xaa, 0xc8],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x77, 0xa3, 0xd2, 0xeb, 0x20, 0x6e, 0x9e, 0xcb, 0xff],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0xae, 0xff, 0xeb, 0x20, 0x20, 0x9a, 0xff, 0xff],
            [0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20],
            [0x80, 0x80, 0x54, 0x20, 0x20, 0x77, 0x80, 0x5c, 0x20, 0x20, 0x6e, 0x80, 0x65, 0x20],
            [0xa0, 0xa0, 0x98, 0x7b, 0x20, 0x94, 0xa0, 0x99, 0x85, 0x20, 0x88, 0xa0, 0x9b, 0x90],
            [0x80, 0xa8, 0xd9, 0xd6, 0x20, 0x77, 0xa3, 0xd2, 0xeb, 0x20, 0x6e, 0x9e, 0xcb, 0xff],
            [0x20, 0xc3, 0xff, 0xd6, 0x20, 0x20, 0xae, 0xff, 0xeb, 0x20, 0x20, 0x9a, 0xff, 0xff],
        ];
        let (n, px) = grip_pixels(1.0, Color32::from_gray(0x20));
        assert_eq!(n, 14);
        for (i, (c, want)) in px.iter().zip(MEASURED.as_flattened()).enumerate() {
            assert!(
                c.r().abs_diff(*want) <= 26,
                "({}, {}): {c:?} vs {want}",
                i % 14,
                i / 14
            );
        }
        assert_eq!(grip_pixels(2.0, Color32::from_gray(0x20)).0, 28);
    }
}
