//! SettingsKeyboardEntry.js and css/window.css shortcut-settings-*. Reference: paintdotjs b4e6fc5.
//! Search/capture behavior follows spec §0.1; built-in inventory follows R16 and framework §2.1.
use super::{EntryUi, SettingsState, reveal_focus, settings_button, settings_focus};
use crate::{
    dialogs::DialogCx,
    kit::{
        img, inputs, menu, px,
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
    },
    theme::{self, *},
};
use egui::{Event, Key, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::{
    action::{self, ACTIONS, SHORTCUTS_STORAGE_KEY},
    app::AppEvent,
    tool::types::ToolType,
    util::shortcut::ShortcutKey,
};

#[derive(Default)]
pub struct KeyboardState {
    query: String,
    scroll: f64,
    entered: bool,
    capture: Option<(&'static str, Rect, egui::Id)>,
}

fn persist(cx: &mut DialogCx) {
    if let Some(registry) = cx.shortcuts.as_deref() {
        cx.prefs.set(SHORTCUTS_STORAGE_KEY, &registry.to_json());
        cx.app.fire(AppEvent::ShortcutChanged);
    }
}
/// Shared handler for capture and its UI tests. Unknown ids cannot become editable rows.
pub fn set_shortcut(cx: &mut DialogCx, id: &str, combo: Option<&str>) {
    if !ACTIONS.iter().any(|a| a.id == id) {
        return;
    }
    if let Some(registry) = cx.shortcuts.as_deref_mut() {
        registry.set_shortcut(id, combo.map(ShortcutKey::from_combo));
        persist(cx);
    }
}

// A capture already owns focus before any widget applies this frame's later pointer blur.
// Consume only its keys, stopping at the first outside press or terminal key; later events stay
// available to Search, the dialog and any newly focused owner.
pub(super) fn capture_input(ui: &Ui, cx: &mut DialogCx, state: &mut SettingsState, start: usize) {
    let Some((action, rect, focus)) = state.keyboard.capture else {
        return;
    };
    loop {
        let next = ui.input_mut(|i| {
            let index = start
                + i.events.iter().skip(start).position(|event| match event {
                    Event::Key { pressed: true, .. } | Event::WindowFocused(false) => true,
                    Event::PointerButton {
                        pos, pressed: true, ..
                    } => !rect.contains(*pos),
                    _ => false,
                })?;
            if !matches!(i.events[index], Event::Key { .. }) {
                return Some(None);
            }
            match i.events.remove(index) {
                Event::Key { key, modifiers, .. } => Some(Some((key, modifiers))),
                _ => unreachable!(),
            }
        });
        let Some(next) = next else {
            break;
        };
        let terminal = match next {
            None | Some((Key::Escape, _)) => true,
            Some((Key::Backspace | Key::Delete, _)) => {
                set_shortcut(cx, action, None);
                let name = action::display_name(cx.app, action);
                state.status = format!("Shortcut cleared for {name}.");
                true
            }
            Some((key, m)) => {
                let shortcut =
                    ShortcutKey::from_event(crate::js_key(key), m.shift, m.ctrl, m.alt, m.mac_cmd);
                if shortcut.is_empty() {
                    false
                } else {
                    set_shortcut(cx, action, shortcut.to_string().as_deref());
                    true
                }
            }
        };
        if terminal {
            state.keyboard.capture = None;
            ui.memory_mut(|m| m.surrender_focus(focus));
            break;
        }
    }
}

pub(super) fn show(e: &mut EntryUi) {
    let t = theme::current(e.ui.ctx());
    e.add_heading(
        "Keyboard shortcuts",
        "Select a shortcut field and press a key combination. Delete clears the selected shortcut.",
    );
    let search = Rect::from_min_size(
        Pos2::new(e.rect.min.x, e.y),
        Vec2::new(e.rect.width(), KEYBOARD_SEARCH.0),
    );
    let entering = !e.state.keyboard.entered;
    let old = e.state.keyboard.query.clone();
    let response = inputs::text_input(
        e.ui,
        "shortcut-search",
        "Search actions",
        search,
        &mut e.state.keyboard.query,
        (t.number_input_background, t.pdn_control_border),
        KEYBOARD_SEARCH.1,
        DIALOG_FONT_SIZE,
        false,
        true,
    );
    if entering {
        e.ui.memory_mut(|m| {
            m.request_focus(
                e.ui.scope_id()
                    .with(("text_input", "shortcut-search"))
                    .with("edit"),
            )
        });
        e.state.keyboard.entered = true;
        e.ui.ctx().request_repaint();
    }
    if inputs::text_input_focused(e.ui, "shortcut-search") {
        let outer = search.expand(2.);
        for r in [
            Rect::from_min_max(outer.min, egui::pos2(outer.max.x, search.min.y)),
            Rect::from_min_max(egui::pos2(outer.min.x, search.max.y), outer.max),
            Rect::from_min_max(
                egui::pos2(outer.min.x, search.min.y),
                egui::pos2(search.min.x, search.max.y),
            ),
            Rect::from_min_max(
                egui::pos2(search.max.x, search.min.y),
                egui::pos2(outer.max.x, search.max.y),
            ),
        ] {
            e.ui.painter().rect_filled(r, 0., t.window_background);
        }
        settings_focus(e.ui, search, t.number_input_background, t.window_background);
    }
    if e.state.keyboard.query.is_empty() {
        text::draw(
            e.ui.painter(),
            search.min + Vec2::new(KEYBOARD_SEARCH.1 + 1., 5.),
            "Search actions",
            FontSpec::new(DIALOG_FONT_SIZE),
            KEYBOARD_PLACEHOLDER,
        );
    }
    if !e.state.keyboard.query.is_empty() {
        // Blink's native search-cancel glyph: an eight-DIP diagonal cross.
        let at = egui::pos2(search.max.x - 20., (search.center().y - 4.).round());
        let rect = Rect::from_min_size(at, Vec2::splat(8.));
        let clear = e.ui.interact(
            rect.expand(4.),
            e.ui.scope_id().with("clear-shortcut-search"),
            Sense::CLICK,
        );
        clear.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Clear search"));
        e.ui.ctx().accesskit_node_builder(clear.id, |n| {
            n.set_role(egui::accesskit::Role::Button);
            n.set_label("Clear search");
        });
        e.ui.painter()
            .rect_filled(rect.expand(2.), 0., t.number_input_background);
        let ppp = e.ui.painter().pixels_per_point();
        let points = [
            (0.25, 1.25),
            (1.25, 0.25),
            (4., 3.),
            (6.75, 0.25),
            (7.75, 1.25),
            (5., 4.),
            (7.75, 6.75),
            (6.75, 7.75),
            (4., 5.),
            (1.25, 7.75),
            (0.25, 6.75),
            (3., 4.),
        ]
        .map(|(x, y)| (at + Vec2::new(x, y)) * ppp);
        let (origin, coverage) = crate::kit::aa::polygon_coverage(&points);
        crate::kit::aa::paint_coverage(
            e.ui.painter(),
            origin / ppp,
            &coverage,
            t.menu_item_text,
            t.number_input_background,
        );
        if clear.clicked() {
            e.state.keyboard.query.clear();
            e.ui.memory_mut(|m| {
                m.request_focus(
                    e.ui.scope_id()
                        .with(("text_input", "shortcut-search"))
                        .with("edit"),
                )
            });
            e.ui.ctx().request_repaint();
        }
    }
    if response.gained_focus() {
        e.state.open_select = None;
    }
    if old != e.state.keyboard.query {
        e.state.keyboard.scroll = 0.;
    }
    e.y += KEYBOARD_SEARCH.0 + KEYBOARD_SEARCH.2;
    let query = e.state.keyboard.query.trim().to_lowercase();
    let mut rows: Vec<_> = ACTIONS
        .iter()
        .map(|a| {
            (
                a,
                action::display_name(e.cx.app, a.id)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('\r')
                    .to_string(),
            )
        })
        .filter(|(a, name)| {
            query.is_empty()
                || name.to_lowercase().contains(&query)
                || a.id.to_lowercase().contains(&query)
                || a.category.to_lowercase().contains(&query)
        })
        .collect();
    rows.sort_by(|(a, an), (b, bn)| {
        a.category
            .cmp(b.category)
            .then_with(|| an.to_lowercase().cmp(&bn.to_lowercase()))
    });
    let mut category = "";
    let mut height = 0.;
    for (a, _) in &rows {
        if a.category != category {
            height += KEYBOARD_GROUP;
            category = a.category;
        }
        height += KEYBOARD_ROW;
    }
    let list = Rect::from_min_size(
        Pos2::new(e.rect.min.x, e.y),
        Vec2::new(
            e.rect.width() + 2.,
            (e.ui.clip_rect().bottom() - e.y - KEYBOARD_BOTTOM).max(40.),
        ),
    );
    let max = (height - list.height() as f64 + 2.).max(0.);
    if list.contains(e.ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
        e.state.keyboard.scroll = (e.state.keyboard.scroll
            - e.ui.input(|i| i.smooth_scroll_delta.y) as f64)
            .clamp(0., max);
    }
    let area = Rect::from_min_max(
        list.min + Vec2::splat(1.),
        list.max - Vec2::new(if max > 0. { THICKNESS + 1. } else { 1. }, 1.),
    );
    e.ui.painter().rect_filled(list, 0., t.window_border);
    e.ui.painter()
        .rect_filled(list.shrink(1.), 0., t.window_background);
    let mut ui = e.ui.new_child(
        egui::UiBuilder::new()
            .max_rect(area)
            .id_salt("shortcut-list"),
    );
    ui.set_clip_rect(area.intersect(e.ui.clip_rect()));
    let mut y = area.min.y - e.state.keyboard.scroll as f32;
    category = "";
    for (action, name) in rows {
        if action.category != category {
            category = action.category;
            let r = Rect::from_min_size(
                Pos2::new(area.min.x, y),
                Vec2::new(area.width(), KEYBOARD_GROUP as f32),
            );
            ui.painter().rect_filled(r, 0., t.menu_background);
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::new(r.min.x, r.max.y - 1.), Vec2::new(r.width(), 1.)),
                0.,
                t.window_border,
            );
            text::draw(
                ui.painter(),
                r.min + Vec2::new(8., 5.),
                category,
                FontSpec::new(DIALOG_FONT_SIZE).weight(600),
                t.menu_item_text,
            );
            y += KEYBOARD_GROUP as f32;
        }
        let row = Rect::from_min_size(
            Pos2::new(area.min.x, y),
            Vec2::new(area.width(), KEYBOARD_ROW as f32),
        );
        let capture = Rect::from_min_size(
            Pos2::new(
                row.max.x - KEYBOARD_CAPTURE.0 - KEYBOARD_RESET.0 - KEYBOARD_GAP - KEYBOARD_PAD,
                row.center().y - KEYBOARD_CAPTURE.1 / 2.,
            ),
            Vec2::new(KEYBOARD_CAPTURE.0, KEYBOARD_CAPTURE.1),
        );
        let reset = Rect::from_min_size(
            Pos2::new(
                row.max.x - KEYBOARD_RESET.0 - KEYBOARD_PAD,
                row.center().y - KEYBOARD_RESET.1 / 2. - 0.5,
            ),
            Vec2::new(KEYBOARD_RESET.0, KEYBOARD_RESET.1),
        );
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(row.min.x, row.max.y - 1.),
                Vec2::new(row.width(), 1.),
            ),
            0.,
            t.window_border,
        );
        let mut label = Rect::from_min_max(
            Pos2::new(row.min.x + KEYBOARD_PAD, row.center().y - 8.5),
            Pos2::new(capture.min.x - KEYBOARD_GAP, row.center().y + 6.5),
        );
        // S§4.1: an Extra tool's row has the plus, in the menus' plugin slot at the name's end.
        if ToolType::from_id(action.id).is_some_and(|t| t.is_extra()) {
            let slot =
                menu::plugin_indicator_rect(Rect::from_x_y_ranges(label.x_range(), row.y_range()));
            let ppp = ui.ctx().pixels_per_point();
            img::paint_icon(
                ui.painter(),
                "plugin_indicator.png",
                px::snap_rect(slot, ppp),
                t.window_background,
                img::Filter::None,
            );
            label.max.x = slot.min.x - KEYBOARD_GAP;
        }
        text::draw_ellipsized(
            ui.painter(),
            label,
            &name,
            FontSpec::new(DIALOG_FONT_SIZE),
            t.menu_item_text,
        );
        let id = ui.scope_id().with(("capture", action.id));
        let r = ui.interact(capture, id, Sense::click());
        if r.clicked() {
            r.request_focus();
        }
        let capture_start = if !r.has_focus()
            || e.state
                .keyboard
                .capture
                .is_some_and(|(owner, _, _)| owner == action.id)
        {
            0
        } else {
            // A new owner cannot take keys from before its focus event. Pointer focus starts
            // on the in-field press; keyboard focus starts after the navigation Tab/Shift+Tab.
            ui.input(|i| {
                let pointer = i.events.iter().position(|event| matches!(event,
                    Event::PointerButton {pos, button: egui::PointerButton::Primary, pressed: true, ..}
                        if capture.contains(*pos)));
                pointer.or_else(|| i.events.iter().position(|event| matches!(event,
                    Event::Key {key: Key::Tab, pressed: true, modifiers, ..}
                        if !modifiers.any() || modifiers.shift_only())))
                    .map_or(0, |index| index + 1)
            })
        };
        if r.has_focus() {
            // The DOM capture handler prevents navigation before blurring the changed field.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            e.state.keyboard.capture = Some((action.id, capture, id));
        }
        reveal_focus(&ui, &r, &mut e.state.keyboard.scroll, max);
        let focused = r.has_focus();
        let shortcut =
            e.cx.shortcuts
                .as_deref()
                .and_then(|s| s.shortcut(action.id));
        let value = shortcut
            .as_ref()
            .and_then(ShortcutKey::to_string)
            .unwrap_or_else(|| "Not assigned".into());
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_role(egui::accesskit::Role::TextInput);
            n.set_label(format!("Shortcut for {name}"));
            n.set_value(&value);
            n.set_read_only();
        });
        let border = if focused {
            t.menu_item_selected_border
        } else {
            t.pdn_control_border
        };
        ui.painter().rect_filled(
            if focused { capture.expand(1.) } else { capture },
            0.,
            border,
        );
        ui.painter()
            .rect_filled(capture.shrink(1.), 0., t.number_input_background);
        text::draw(
            ui.painter(),
            capture.min + Vec2::new(7., 4.),
            if focused { "Press shortcut…" } else { &value },
            FontSpec::new(DIALOG_FONT_SIZE),
            t.menu_item_text,
        );
        if e.state
            .keyboard
            .capture
            .is_some_and(|(id, _, _)| id == action.id)
        {
            capture_input(&ui, e.cx, e.state, capture_start);
        }
        let reset_enabled = shortcut.unwrap_or_default() != action.default_shortcut_key();
        let builder = egui::UiBuilder::new().id_salt(("reset", action.id));
        let mut reset_ui = ui.new_child(if reset_enabled {
            builder
        } else {
            builder.disabled()
        });
        reset_ui.set_opacity(ui.opacity());
        let rr = settings_button(&mut reset_ui, action.id, reset, "Reset", false);
        ui.ctx().accesskit_node_builder(rr.id, |n| {
            n.set_label(format!("Reset shortcut for {name}"));
            if !reset_enabled {
                n.set_disabled();
            }
        });
        // The source rebuilds rows after a change, blurring the capture field.
        reveal_focus(&ui, &rr, &mut e.state.keyboard.scroll, max);
        inputs::take_enter(&ui, &rr);
        if reset_enabled
            && inputs::button_activated(&ui, &rr)
            && let Some(registry) = e.cx.shortcuts.as_deref_mut()
        {
            registry.reset_shortcut(action.id);
            persist(e.cx);
        }
        y += KEYBOARD_ROW as f32;
    }
    if max > 0. {
        Scrollbar::vertical(Rect::from_min_size(
            Pos2::new(list.max.x - THICKNESS - 1., list.min.y + 1.),
            Vec2::new(THICKNESS, list.height() - 2.),
        ))
        .id_salt("shortcuts")
        .show(
            e.ui,
            height,
            list.height() as f64 - 2.,
            &mut e.state.keyboard.scroll,
        );
    }
    e.horizontal_overflow();
    e.y = list.max.y + KEYBOARD_RESET_MARGIN;
    if e.add_button("Reset all shortcuts", true) {
        if let Some(registry) = e.cx.shortcuts.as_deref_mut() {
            registry.reset_all();
            persist(e.cx);
        }
        e.state.status = "All shortcuts were reset to their defaults.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ENTRIES, SettingsState};
    use crate::dialogs::DialogCx;
    use egui::{Pos2, Rect, Vec2};

    /// The plus meshes the Keyboard page paints with `query` typed in its search box.
    fn pluses(query: &str) -> usize {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let image = image::RgbaImage::from_pixel(12, 16, image::Rgba([20, 200, 70, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        crate::assets::Assets::set_icon_override(&ctx, "plugin_indicator.png", png.into_inner());
        let mut app = pdn_core::app::AppWorkspace::default();
        let mut state = SettingsState {
            section: ENTRIES.iter().position(|e| e.0 == "keyboard").unwrap(),
            ..Default::default()
        };
        state.keyboard.query = query.into();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let mut cx = DialogCx {
                    app: &mut app,
                    settings: &mut pdn_core::settings::AppSettings::load(None),
                    prefs: &mut crate::prefs::Prefs::default(),
                    settings_changed: false,
                    shortcuts: None,
                    clipboard: None,
                    settings_imports: None,
                    fonts: None,
                    asset_library: None,
                    seat_focus: false,
                };
                state.show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 640.0)),
                    &mut cx,
                );
            },
        );
        let plus: Vec<_> = output
            .textures_delta
            .set
            .iter()
            .filter(|(_, ds)| {
                ds.iter()
                    .any(|d| d.image.width() == 12 && d.image.height() == 16)
            })
            .map(|(id, _)| *id)
            .collect();
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter(|s| matches!(&s.shape, egui::Shape::Mesh(m) if plus.contains(&m.texture_id)))
            .count()
    }

    /// S§4.1: an Extra tool's row carries the plus after its name, as its menu entry does; a
    /// Paint.NET tool's row does not.
    #[test]
    fn extra_tool_rows_paint_the_plus() {
        assert_eq!(pluses("patch"), 1);
        assert_eq!(pluses("clone stamp"), 0);
    }
}
