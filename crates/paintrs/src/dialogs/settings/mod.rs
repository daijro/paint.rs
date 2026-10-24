//! Settings shell and shared entry helpers. Port of SettingsDialog.js, SettingsEntryRegistry.js,
//! SettingsEntry.js and css/window.css. Reference: paintdotjs @ b4e6fc5.
//! Native values: canvas-rendering.md §§2.6–2.7, painting-tools-1.md §2; spec §§0.5,0.10 (R47).
mod asset_widgets;
pub mod canvas;
pub mod fonts;
pub mod icon_packs;
pub mod info_pages;
pub mod keyboard;
pub mod shapes;
pub mod tools;
pub mod ui;

use super::effect_config::{self, OpenSelect, SelectLook};
use super::{Dialog, DialogCx, DialogResult, DialogStack, DialogState};
use crate::{
    kit::{
        img, inputs, px,
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
        window::{self, DialogFrameState, Footer},
    },
    theme::{self, *},
};
use egui::{Color32, Key, Pos2, Rect, Sense, Ui, Vec2};
use std::{cell::RefCell, rc::Rc};

/// SettingsEntryRegistry.groups, excluding the dropped Updates and Plugins entries (R12).
pub const ENTRIES: &[(&str, &str, &str)] = &[
    ("ui", "User Interface", "settings_u_i_24.png"),
    ("canvas", "Canvas", "settings_canvas_24.png"),
    ("tools", "Tools", "settings_tools_24.png"),
    ("pen", "Pen & Tablet", "settings_pen_and_tablet_24.png"),
    ("graphics", "Graphics", "settings_graphics_24.png"),
    (
        "colorManagement",
        "Color Management",
        "settings_color_management_24.png",
    ),
    ("diagnostics", "Diagnostics", "settings_diagnostics_24.png"),
    ("fonts", "Fonts", "text_tool_icon.png"),
    ("shapes", "Shapes", "shapes_tool_icon.png"),
    ("assets", "Icon Packs", "menu_file_open_icon.png"),
    (
        "keyboard",
        "Keyboard Shortcuts",
        "menu_utilities_settings_icon.png",
    ),
];

/// Persistent handle: reopening selects the existing Settings dialog rather than stacking copies.
#[derive(Clone, Default)]
pub struct SettingsHandle(Rc<RefCell<SettingsState>>);
impl SettingsHandle {
    pub(crate) fn take_reload_requested(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().reload_requested)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn language_applied(&self, ctx: &egui::Context) {
        let mut state = self.0.borrow_mut();
        state.applied_language = Some(crate::language::preference(ctx));
        state.status.clear();
    }
    pub fn section(&self) -> &'static str {
        ENTRIES[self.0.borrow().section].0
    }
    pub fn is_open(&self) -> bool {
        self.0.borrow().open
    }
    pub fn open(&self, stack: &mut DialogStack, section: Option<&str>) {
        let mut state = self.0.borrow_mut();
        state.section = ENTRIES
            .iter()
            .position(|e| Some(e.0) == section)
            .unwrap_or(0);
        state.page_scroll = 0.;
        state.page_horizontal = 0.;
        state.open_select = None;
        state.keyboard = keyboard::KeyboardState::default();
        if !state.open {
            state.open = true;
            stack.push(Box::new(SettingsDialog(self.clone())));
        }
    }
}

#[derive(Default)]
pub struct SettingsState {
    open: bool,
    keyboard: keyboard::KeyboardState,
    diagnostics_copy_pending: bool,
    section: usize,
    pub status: String,
    open_select: Option<OpenSelect>,
    nav_scroll: f64,
    page_scroll: f64,
    page_horizontal: f64,
    /// Task55 applies the requested language only through Reload, never the live display path.
    pub applied_language: Option<String>,
    pub reload_requested: bool,
    fonts: fonts::FontsState,
    shapes: shapes::ShapesState,
    icon_packs: icon_packs::PacksState,
    wheel: Option<(bool, Color32, egui::TextureHandle, egui::ColorImage)>,
}

pub struct SettingsDialog(SettingsHandle);
impl Drop for SettingsDialog {
    fn drop(&mut self) {
        self.0.0.borrow_mut().open = false;
    }
}
impl Dialog for SettingsDialog {
    fn title(&self) -> String {
        "Settings".into()
    }
    fn persistence_id(&self) -> Option<&'static str> {
        Some("settings")
    }
    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let size = ui.ctx().content_rect().size();
        let width = SETTINGS_SIZE
            .0
            .min((size.x - SETTINGS_SCREEN_INSET).max(SETTINGS_MIN_SIZE.0));
        let height = SETTINGS_SIZE
            .1
            .min((size.y - SETTINGS_SCREEN_INSET).max(SETTINGS_MIN_SIZE.1));
        let handle = self.0.clone();
        let footer_handle = self.0.clone();
        let mut closed = false;
        let seat_focus = cx.seat_focus;
        let mut body = |ui: &mut Ui, rect: Rect| {
            let mut s = handle.0.borrow_mut();
            if s.applied_language.is_none() {
                s.applied_language = Some(crate::language::preference(ui.ctx()));
            }
            s.show(ui, rect, cx);
        };
        let mut footer = |ui: &mut Ui, rect: Rect| {
            let state = footer_handle.0.borrow();
            let t = theme::current(ui.ctx());
            let close = Rect::from_min_size(
                Pos2::new(
                    rect.max.x - SETTINGS_FOOTER.1 - SETTINGS_FOOTER.2,
                    rect.center().y - SETTINGS_FOOTER.3 / 2.,
                ),
                Vec2::new(SETTINGS_FOOTER.2, SETTINGS_FOOTER.3),
            );
            let status = Rect::from_min_max(
                rect.min + Vec2::new(SETTINGS_FOOTER.1, 0.),
                Pos2::new(close.min.x - SETTINGS_FOOTER.1, rect.max.y),
            );
            text::draw_ellipsized(
                ui.painter(),
                Rect::from_min_size(
                    Pos2::new(status.min.x, status.center().y - 7.5),
                    Vec2::new(status.width(), 15.),
                ),
                &state.status,
                FontSpec::new(DIALOG_FONT_SIZE),
                mix(t.menu_item_text, t.menu_background, SETTINGS_MIX.0),
            );
            let response = settings_button(ui, "close-settings", close, "Close", seat_focus);
            closed = inputs::button_activated(ui, &response);
        };
        let frame = window::dialog_frame_with_title(
            ui,
            "Settings",
            Some("menu_utilities_settings_icon.png"),
            false,
            (
                Vec2::new(width, height - SETTINGS_TITLE.0 - SETTINGS_FOOTER.0 - 2.),
                &mut body,
            ),
            Some(&mut footer),
            Footer::DEFAULT,
            SETTINGS_TITLE,
        );
        let popup = self.0.0.borrow().open_select;
        let mut fs = DialogFrameState::get(ui.ctx(), ui.scope_id());
        fs.popup = popup.map_or(Rect::NOTHING, |s| s.list);
        fs.set(ui.ctx(), ui.scope_id());
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        if escape && popup.is_some() {
            self.0.0.borrow_mut().open_select = None;
            return DialogState::Open;
        }
        if frame.closed || escape || closed {
            return DialogState::Closed(if escape || frame.closed {
                DialogResult::Cancel
            } else {
                DialogResult::Ok
            });
        }
        DialogState::Open
    }
}

fn settings_button(ui: &mut Ui, id: &str, rect: Rect, label: &str, seat: bool) -> egui::Response {
    let response = inputs::push_button(ui, id, rect, label, true, false);
    if seat {
        response.request_focus();
    }
    if response.has_focus() && window::dialog_focus_visible(ui.ctx()) {
        let t = theme::current(ui.ctx());
        settings_focus(ui, rect, t.button, t.menu_background);
    }
    response
}

/// The custom scrollbars retain their offsets; egui still owns Tab/Shift+Tab focus order.
fn reveal_focus(ui: &Ui, response: &egui::Response, scroll: &mut f64, max: f64) {
    if !response.gained_focus() {
        return;
    }
    let clip = ui.clip_rect();
    let delta = if response.rect.top() < clip.top() {
        response.rect.top() - clip.top()
    } else {
        (response.rect.bottom() - clip.bottom()).max(0.)
    };
    let next = (*scroll + delta as f64).clamp(0., max);
    if next != *scroll {
        *scroll = next;
        ui.ctx().request_repaint();
    }
}

/// Chromium's auto outline around a filled button: black outside, two white pixels inside.
fn settings_focus(ui: &Ui, rect: Rect, fill: Color32, bg: Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let r = px::snap_rect(rect, ppp);
    let inside = |x: f32, y: f32, rect: Rect, radius: f32| {
        if !rect.contains(Pos2::new(x, y)) {
            return false;
        }
        let cx = x.clamp(rect.min.x + radius, rect.max.x - radius);
        let cy = y.clamp(rect.min.y + radius, rect.max.y - radius);
        (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
    };
    for y in ((r.min.y - 1.) * ppp).floor() as i32..((r.max.y + 1.) * ppp).ceil() as i32 {
        for x in ((r.min.x - 1.) * ppp).floor() as i32..((r.max.x + 1.) * ppp).ceil() as i32 {
            let mut black = 0;
            let mut white = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (fx, fy) = (
                        (x as f32 + (sx as f32 + 0.5) / 4.) / ppp,
                        (y as f32 + (sy as f32 + 0.5) / 4.) / ppp,
                    );
                    if inside(fx, fy, r.expand(1.), SETTINGS_FOCUS.2) {
                        if !inside(fx, fy, r, SETTINGS_FOCUS.3) {
                            black += 1;
                        } else if !inside(fx, fy, r.shrink(2.), SETTINGS_FOCUS.4) {
                            white += 1;
                        }
                    }
                }
            }
            if black + white > 0 {
                let pos = Pos2::new(x as f32 / ppp, y as f32 / ppp);
                let base = if r.contains(pos + Vec2::splat(0.5 / ppp)) {
                    fill
                } else {
                    bg
                };
                let blend = |v: u8| {
                    ((v as f32 * (16 - black - white) as f32
                        + SETTINGS_FOCUS.0.r() as f32 * black as f32
                        + SETTINGS_FOCUS.1.r() as f32 * white as f32)
                        / 16.)
                        .round() as u8
                };
                ui.painter().rect_filled(
                    Rect::from_min_size(pos, Vec2::splat(1. / ppp)),
                    0.,
                    Color32::from_rgb(blend(base.r()), blend(base.g()), blend(base.b())),
                );
            }
        }
    }
}

pub(crate) fn mix(fg: Color32, bg: Color32, alpha: f32) -> Color32 {
    let c = |a: u8, b: u8| (a as f32 * alpha + b as f32 * (1. - alpha)).round() as u8;
    Color32::from_rgb(c(fg.r(), bg.r()), c(fg.g(), bg.g()), c(fg.b(), bg.b()))
}

impl SettingsState {
    fn show(&mut self, ui: &mut Ui, rect: Rect, cx: &mut DialogCx) {
        // Finish the old field's owned prefix before navigation can replace its page state.
        // Keep this inside the frame body so title/backdrop focus bookkeeping stays intact.
        keyboard::capture_input(ui, cx, self, 0);
        let t = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let nav_width = if ui.ctx().content_rect().width() <= SETTINGS_BREAKPOINT {
            SETTINGS_NARROW_NAV
        } else {
            SETTINGS_NAV
        };
        let nav = Rect::from_min_size(rect.min, Vec2::new(nav_width, rect.height()));
        let fill = mix(t.window_background, t.menu_background, SETTINGS_MIX.1);
        ui.painter().rect_filled(px::snap_rect(nav, ppp), 0., fill);
        ui.painter().rect_filled(
            px::snap_rect(
                Rect::from_min_size(
                    Pos2::new(nav.max.x - 1., nav.min.y),
                    Vec2::new(1., nav.height()),
                ),
                ppp,
            ),
            0.,
            px::over(fill, t.window_border),
        );
        let nav_content =
            ENTRIES.len() as f64 * SETTINGS_NAV_ROW as f64 + SETTINGS_GROUP_HEIGHT as f64 + 18.;
        let max = (nav_content - nav.height() as f64).max(0.);
        if self.open_select.is_none()
            && nav.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO))
        {
            self.nav_scroll =
                (self.nav_scroll - ui.input(|i| i.smooth_scroll_delta.y) as f64).clamp(0., max);
        }
        let has_scroll = max > 0.;
        let mut n = ui.new_child(egui::UiBuilder::new().max_rect(nav).id_salt("navigation"));
        n.set_clip_rect(nav.intersect(ui.clip_rect()));
        let mut y = nav.min.y + SETTINGS_NAV_PADDING.1 - self.nav_scroll as f32;
        for (i, (id, label, icon)) in ENTRIES.iter().enumerate() {
            if i == 7 {
                y += SETTINGS_GROUP_LAYOUT.1;
                text::draw(
                    n.painter(),
                    Pos2::new(
                        nav.min.x + SETTINGS_GROUP_LAYOUT.0,
                        y + SETTINGS_GROUP_LAYOUT.2,
                    ),
                    &format!("{} specific", crate::dialogs::about::PRODUCT),
                    FontSpec::new(SETTINGS_GROUP_FONT)
                        .weight(600)
                        .spacing(SETTINGS_GROUP_TRACKING),
                    mix(t.menu_item_text, fill, SETTINGS_MIX.2),
                );
                y += SETTINGS_GROUP_LAYOUT.3;
            }
            let row = Rect::from_min_size(
                Pos2::new(nav.min.x + SETTINGS_NAV_PADDING.0, y),
                Vec2::new(
                    nav.width() - SETTINGS_NAV_PADDING.2 - if has_scroll { THICKNESS } else { 0. },
                    SETTINGS_NAV_ROW,
                ),
            );
            {
                let response = n.interact(
                    px::snap_rect(row, ppp),
                    n.scope_id().with(id),
                    Sense::click(),
                );
                reveal_focus(&n, &response, &mut self.nav_scroll, max);
                n.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_role(egui::accesskit::Role::Button);
                    node.set_label(*label);
                });
                let active = self.section == i;
                let (bg, border) = if active {
                    (
                        px::over(fill, t.menu_item_selected),
                        Some(t.menu_item_selected_border),
                    )
                } else if response.hovered() {
                    (t.menu_item_hover, Some(t.menu_item_border))
                } else {
                    (fill, None)
                };
                if let Some(border) = border {
                    n.painter().rect_filled(px::snap_rect(row, ppp), 0., border);
                    n.painter()
                        .rect_filled(px::snap_rect(row.shrink(1.), ppp), 0., bg);
                }
                let mut icon_rect = Rect::from_min_size(
                    Pos2::new(
                        row.min.x + SETTINGS_NAV_ICON.1,
                        row.center().y - SETTINGS_NAV_ICON.0 / 2.,
                    ),
                    Vec2::splat(SETTINGS_NAV_ICON.0),
                );
                if let Some(image) = crate::assets::Assets::icon(n.ctx(), icon) {
                    let scale = (SETTINGS_NAV_ICON.0 / image.size()[0] as f32)
                        .min(SETTINGS_NAV_ICON.0 / image.size()[1] as f32);
                    icon_rect = Rect::from_center_size(
                        icon_rect.center(),
                        Vec2::new(image.size()[0] as f32, image.size()[1] as f32) * scale,
                    );
                }
                img::paint_icon(
                    n.painter(),
                    icon,
                    px::snap_rect(icon_rect, ppp),
                    bg,
                    img::Filter::None,
                );
                text::draw(
                    n.painter(),
                    Pos2::new(row.min.x + SETTINGS_NAV_ICON.2, row.center().y - 7.5),
                    label,
                    FontSpec::new(DIALOG_FONT_SIZE),
                    t.menu_item_text,
                );
                if response.clicked() {
                    response.request_focus();
                }
                if inputs::button_activated(&n, &response) {
                    self.section = i;
                    self.page_scroll = 0.;
                    self.page_horizontal = 0.;
                    self.open_select = None;
                    self.keyboard = keyboard::KeyboardState::default();
                }
            }
            y += SETTINGS_NAV_ROW;
        }
        if has_scroll {
            Scrollbar::vertical(Rect::from_min_size(
                Pos2::new(nav.max.x - THICKNESS - 1., nav.min.y),
                Vec2::new(THICKNESS, nav.height()),
            ))
            .show(ui, nav_content, nav.height() as f64, &mut self.nav_scroll);
        }
        let page = Rect::from_min_max(Pos2::new(nav.max.x, rect.min.y), rect.max);
        let content_height = match ENTRIES[self.section].0 {
            "ui" => {
                if ui.ctx().content_rect().width() <= SETTINGS_BREAKPOINT {
                    484.
                } else {
                    420.
                }
            }
            "canvas" => 263.,
            "tools" => {
                if ui.ctx().content_rect().width() <= SETTINGS_BREAKPOINT {
                    246.
                } else {
                    230.
                }
            }
            "pen" => 170.,
            "graphics" => 180.,
            "colorManagement" => 220.,
            "diagnostics" => 350.,
            "keyboard" => 0.,
            _ => 60.,
        };
        let max = (content_height - page.height() as f64).max(0.);
        if self.open_select.is_none()
            && page.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO))
        {
            self.page_scroll =
                (self.page_scroll - ui.input(|i| i.smooth_scroll_delta.y) as f64).clamp(0., max);
        }
        let mut p = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(page)
                .id_salt(("page", ENTRIES[self.section].0)),
        );
        p.set_clip_rect(page.intersect(ui.clip_rect()));
        let canvas = ENTRIES[self.section].0 == "canvas";
        let origin = page.min
            + if canvas {
                Vec2::new(SETTINGS_CANVAS_PADDING.0, SETTINGS_CANVAS_PADDING.1)
            } else {
                Vec2::new(SETTINGS_PAGE_PADDING.0, SETTINGS_PAGE_PADDING.1)
            }
            - Vec2::new(0., self.page_scroll as f32);
        let content = Rect::from_min_max(
            origin,
            Pos2::new(
                page.max.x
                    - if canvas {
                        SETTINGS_CANVAS_PADDING.0
                    } else {
                        SETTINGS_PAGE_PADDING.0
                    }
                    - if max > 0. { THICKNESS } else { 0. },
                page.max.y,
            ),
        )
        .translate(Vec2::new(-self.page_horizontal as f32, 0.));
        let mut entry = EntryUi {
            ui: &mut p,
            cx,
            state: self,
            rect: content,
            y: origin.y,
            scroll_max: max,
        };
        match ENTRIES[entry.state.section].0 {
            "ui" => ui::show(&mut entry),
            "canvas" => canvas::show(&mut entry),
            "tools" => tools::show(&mut entry),
            "fonts" => fonts::show(&mut entry),
            "shapes" => shapes::show(&mut entry),
            "assets" => icon_packs::show(&mut entry),
            "pen" => info_pages::pen(&mut entry),
            "graphics" => info_pages::graphics(&mut entry),
            "colorManagement" => info_pages::color_management(&mut entry),
            "diagnostics" => info_pages::diagnostics(&mut entry),
            "keyboard" => keyboard::show(&mut entry),
            _ => {
                let label = ENTRIES[entry.state.section].1;
                entry.add_heading(label, "");
            }
        }
        if max > 0. {
            Scrollbar::vertical(Rect::from_min_size(
                Pos2::new(page.max.x - THICKNESS, page.min.y),
                Vec2::new(THICKNESS, page.height()),
            ))
            .id_salt("settings-page")
            .show(
                ui,
                content_height,
                page.height() as f64,
                &mut self.page_scroll,
            );
        }
    }
}

/// Shared page helpers; later pages borrow the same DialogCx and retain their own manager state.
pub struct EntryUi<'a, 'b> {
    pub ui: &'a mut Ui,
    pub cx: &'a mut DialogCx<'b>,
    pub state: &'a mut SettingsState,
    pub rect: Rect,
    pub y: f32,
    scroll_max: f64,
}
impl EntryUi<'_, '_> {
    /// The bordered 100%-width lists/tables overflow the source page by two DIPs.
    pub fn horizontal_overflow(&mut self) {
        let page = self.ui.clip_rect();
        let rect = Rect::from_min_max(Pos2::new(page.min.x, page.max.y - THICKNESS), page.max);
        Scrollbar::horizontal(rect)
            .id_salt("settings-horizontal")
            .show(
                self.ui,
                page.width() as f64 + 2.,
                page.width() as f64,
                &mut self.state.page_horizontal,
            );
    }
    pub fn add_heading(&mut self, label: &str, description: &str) {
        let t = theme::current(self.ui.ctx());
        text::draw(
            self.ui.painter(),
            Pos2::new(self.rect.min.x, self.y),
            label,
            FontSpec::new(SETTINGS_HEADING).bold(),
            t.menu_item_text,
        );
        self.y += SETTINGS_HEADING_PITCH;
        if !description.is_empty() {
            text::draw(
                self.ui.painter(),
                Pos2::new(self.rect.min.x, self.y),
                description,
                FontSpec::new(DIALOG_FONT_SIZE),
                mix(t.menu_item_text, t.window_background, SETTINGS_MIX.0),
            );
            self.y += SETTINGS_HEADING_PITCH;
        }
    }
    pub fn add_checkbox(&mut self, path: &str, label: &str, height: f32) -> bool {
        self.add_checkbox_enabled(path, label, height, true)
    }
    pub fn add_checkbox_enabled(
        &mut self,
        path: &str,
        label: &str,
        height: f32,
        enabled: bool,
    ) -> bool {
        let opacity = self.ui.opacity();
        if !enabled {
            self.ui.set_opacity(opacity * SETTINGS_MIX.3);
        }
        let t = theme::current(self.ui.ctx());
        let x = self.rect.min.x;
        let mut value = self.cx.settings.get_bool(path);
        let old = value;
        let hit = Rect::from_min_size(
            Pos2::new(x, self.y),
            Vec2::new(
                text::css_width(label, DIALOG_FONT_SIZE) + SETTINGS_CHECKBOX.1,
                height,
            ),
        );
        {
            let boxed = Rect::from_center_size(
                Pos2::new(x + SETTINGS_CHECKBOX.0 / 2., self.y + height / 2.),
                Vec2::splat(SETTINGS_CHECKBOX.0),
            );
            let response = inputs::checkbox_in(
                self.ui,
                path,
                boxed,
                hit,
                &mut value,
                enabled,
                t.window_background,
            );
            reveal_focus(
                self.ui,
                &response,
                &mut self.state.page_scroll,
                self.scroll_max,
            );
            if response.clicked() {
                response.request_focus();
            }
            self.ui.ctx().accesskit_node_builder(response.id, |n| {
                n.set_role(egui::accesskit::Role::CheckBox);
                n.set_label(label);
                if !enabled {
                    n.set_disabled();
                }
            });
            text::draw(
                self.ui.painter(),
                Pos2::new(x + SETTINGS_CHECKBOX.1, self.y + (height - 15.) / 2.),
                label,
                FontSpec::new(DIALOG_FONT_SIZE),
                t.menu_item_text,
            );
        }
        self.ui.set_opacity(opacity);
        self.y += height;
        if old != value {
            self.set(path, value.into());
        }
        value
    }
    pub fn set(&mut self, path: &str, value: serde_json::Value) {
        self.cx.settings.set(path, value);
        self.cx.settings_changed = true;
    }
    pub fn add_select(
        &mut self,
        path: Option<&str>,
        label: &str,
        choices: &[(String, String)],
        value: &str,
    ) -> Option<String> {
        let t = theme::current(self.ui.ctx());
        let line = FontSpec::new(DIALOG_FONT_SIZE);
        let narrow = self.ui.ctx().content_rect().width() <= SETTINGS_BREAKPOINT;
        let text_width = text::css_width(label, DIALOG_FONT_SIZE);
        text::draw(
            self.ui.painter(),
            Pos2::new(self.rect.min.x, self.y + if narrow { 0. } else { 11.5 }),
            label,
            line,
            t.menu_item_text,
        );
        let x = if narrow {
            self.rect.min.x
        } else {
            self.rect.min.x + text_width + SETTINGS_FIELD.0
        };
        let rect = Rect::from_min_size(
            Pos2::new(
                x,
                self.y
                    + if narrow {
                        SETTINGS_FIELD_NARROW.0
                    } else {
                        SETTINGS_FIELD.3
                    },
            ),
            Vec2::new(
                SETTINGS_FIELD.1.min(self.rect.max.x - x).max(50.),
                SETTINGS_FIELD.2,
            ),
        );
        let current = choices.iter().position(|c| c.0 == value).unwrap_or(0);
        let id = self.ui.scope_id().with(("select", path.unwrap_or(label)));
        let picked = effect_config::select(
            self.ui,
            &mut self.state.open_select,
            id,
            rect,
            label,
            &choices.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(),
            current,
            SelectLook {
                native: false,
                pad_left: 0.,
                font: line,
            },
        );
        if let Some(response) = self.ui.ctx().read_response(id) {
            reveal_focus(
                self.ui,
                &response,
                &mut self.state.page_scroll,
                self.scroll_max,
            );
        }
        self.y += if narrow {
            SETTINGS_FIELD_NARROW.1
        } else {
            SETTINGS_FIELD.4
        };
        let picked = picked.map(|i| choices[i].0.clone());
        if let (Some(path), Some(value)) = (path, picked.as_ref()) {
            self.set(path, value.clone().into());
        }
        picked
    }
    pub fn add_note(&mut self, label: &str) {
        let t = theme::current(self.ui.ctx());
        text::draw(
            self.ui.painter(),
            Pos2::new(self.rect.min.x + SETTINGS_NOTE.0, self.y),
            label,
            FontSpec::new(DIALOG_FONT_SIZE),
            mix(t.menu_item_text, t.window_background, SETTINGS_MIX.0),
        );
        self.y += SETTINGS_NOTE.1;
    }
    pub fn add_section_heading(&mut self, label: &str) {
        self.y += SETTINGS_SECTION.0;
        let t = theme::current(self.ui.ctx());
        let font = FontSpec::new(DIALOG_FONT_SIZE).weight(500);
        text::draw(
            self.ui.painter(),
            Pos2::new(self.rect.min.x, self.y),
            label,
            font,
            t.menu_item_text,
        );
        let left = self.rect.min.x + text::css_width(label, font.size) + SETTINGS_SECTION.1;
        self.ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(left, self.y + SETTINGS_SECTION.2),
                Vec2::new((self.rect.max.x - left).max(0.), 1.),
            ),
            0.,
            t.pdn_separator,
        );
        self.y += SETTINGS_SECTION.3;
    }
    pub fn add_button(&mut self, label: &str, enabled: bool) -> bool {
        self.y += SETTINGS_BUTTON.3;
        let width =
            (text::css_width(label, DIALOG_FONT_SIZE) + SETTINGS_BUTTON.2).max(SETTINGS_BUTTON.0);
        let r = Rect::from_min_size(
            Pos2::new(self.rect.min.x, self.y),
            Vec2::new(width, SETTINGS_BUTTON.1),
        );
        let clicked = if enabled {
            let response = settings_button(self.ui, label, r, label, false);
            reveal_focus(
                self.ui,
                &response,
                &mut self.state.page_scroll,
                self.scroll_max,
            );
            inputs::take_enter(self.ui, &response);
            inputs::button_activated(self.ui, &response)
        } else {
            let t = theme::current(self.ui.ctx());
            let response = self
                .ui
                .interact(r, self.ui.scope_id().with(label), Sense::hover());
            self.ui.ctx().accesskit_node_builder(response.id, |n| {
                n.set_role(egui::accesskit::Role::Button);
                n.set_label(label);
                n.set_disabled();
            });
            self.ui.painter().rect_filled(
                r,
                0.,
                mix(t.button_outline, t.window_background, SETTINGS_MIX.3),
            );
            self.ui.painter().rect_filled(
                r.shrink(1.),
                0.,
                mix(t.button, t.window_background, SETTINGS_MIX.3),
            );
            let font = FontSpec::new(DIALOG_FONT_SIZE);
            text::draw(
                self.ui.painter(),
                Pos2::new(
                    r.center().x - text::css_width(label, font.size) / 2.,
                    r.center().y - font.resolved_line_height(self.ui.ctx().pixels_per_point()) / 2.,
                ),
                label,
                font,
                mix(t.button_text, t.window_background, SETTINGS_MIX.3),
            );
            false
        };
        self.y += SETTINGS_BUTTON.4;
        clicked
    }
}
