//! Shared Shapes/Icon Packs page chrome from Settings{Shapes,IconPacks}Entry.js and window.css.
use super::{EntryUi, mix, reveal_focus};
use crate::{
    kit::{
        inputs, px,
        text::{self, FontSpec},
    },
    theme::{self, *},
};
use egui::{Pos2, Rect, Sense, Vec2};
pub(super) fn button(e: &mut EntryUi, label: &str, id: &str, rect: Rect, enabled: bool) -> bool {
    let (clicked, r) = super::fonts::action(e, label, id, rect, enabled);
    reveal_focus(e.ui, &r, &mut e.state.page_scroll, e.scroll_max);
    clicked
}
pub(super) fn paragraph(e: &mut EntryUi, label: &str, indent: f32, margin: f32) {
    let t = theme::current(e.ui.ctx());
    let mut line = String::new();
    for word in label.split_whitespace() {
        let next = if line.is_empty() {
            word.into()
        } else {
            format!("{line} {word}")
        };
        if text::css_width(&next, DIALOG_FONT_SIZE) > e.rect.width() - indent && !line.is_empty() {
            text::draw(
                e.ui.painter(),
                Pos2::new(e.rect.min.x + indent, e.y),
                &line,
                FontSpec::new(DIALOG_FONT_SIZE),
                mix(t.menu_item_text, t.window_background, SETTINGS_MIX.0),
            );
            e.y += 18.;
            line = word.into();
        } else {
            line = next;
        }
    }
    text::draw(
        e.ui.painter(),
        Pos2::new(e.rect.min.x + indent, e.y),
        &line,
        FontSpec::new(DIALOG_FONT_SIZE),
        mix(t.menu_item_text, t.window_background, SETTINGS_MIX.0),
    );
    e.y += 15. + margin;
}
pub(super) fn drop_zone(e: &mut EntryUi, label: &str, id: &str, enabled: bool) -> bool {
    e.y += ASSET_DROP_MARGIN;
    let r = Rect::from_min_size(
        Pos2::new(e.rect.min.x, e.y),
        Vec2::new(e.rect.width(), ASSET_DROP_HEIGHT),
    );
    let response = e.ui.interact(
        r,
        e.ui.scope_id().with(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let t = theme::current(e.ui.ctx());
    let hover = response.has_focus()
        || (!e.ui.input(|i| i.raw.hovered_files.is_empty()) && response.hovered());
    let fill = if hover {
        t.menu_item_hover
    } else {
        mix(t.number_input_background, t.window_background, 0.88)
    };
    let border = if hover {
        t.menu_item_selected_border
    } else {
        t.pdn_control_border
    };
    e.ui.painter().rect_filled(r, 0., fill);
    for horizontal in [true, false] {
        let length = if horizontal { r.width() } else { r.height() };
        let count = ((length + ASSET_DROP_MARGIN) / ASSET_DASH.1)
            .floor()
            .max(1.) as usize;
        let step = if count > 1 {
            (length - ASSET_DASH.0) / (count - 1) as f32
        } else {
            0.
        };
        for edge in [false, true] {
            for i in 0..count {
                let offset = (i as f32 * step).round();
                let (at, size) = if horizontal {
                    (
                        Pos2::new(r.min.x + offset, if edge { r.max.y - 1. } else { r.min.y }),
                        Vec2::new(ASSET_DASH.0, 1.),
                    )
                } else {
                    (
                        Pos2::new(if edge { r.max.x - 1. } else { r.min.x }, r.min.y + offset),
                        Vec2::new(1., ASSET_DASH.0),
                    )
                };
                e.ui.painter().rect_filled(
                    px::snap_rect(Rect::from_min_size(at, size), e.ui.ctx().pixels_per_point()),
                    0.,
                    border,
                );
            }
        }
    }
    text::draw(
        e.ui.painter(),
        Pos2::new(
            r.center().x - text::css_width(label, DIALOG_FONT_SIZE) / 2.,
            r.center().y - 7.5,
        ),
        label,
        FontSpec::new(DIALOG_FONT_SIZE),
        mix(t.menu_item_text, fill, 0.72),
    );
    e.ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label(label);
        if !enabled {
            n.set_disabled();
        }
    });
    if response.clicked() {
        response.request_focus();
    }
    inputs::take_enter(e.ui, &response);
    e.y = r.max.y + ASSET_DROP_MARGIN;
    enabled && inputs::button_activated(e.ui, &response)
}
pub(super) fn section(e: &mut EntryUi, label: &str, bottom: f32) {
    e.y += ASSET_SECTION_TOP;
    let t = theme::current(e.ui.ctx());
    let x = e.rect.min.x;
    let font = FontSpec::new(DIALOG_FONT_SIZE).weight(500);
    text::draw(
        e.ui.painter(),
        Pos2::new(x, e.y),
        label,
        font,
        t.menu_item_text,
    );
    let left = x + text::css_width(label, DIALOG_FONT_SIZE) + SETTINGS_SECTION.1;
    e.ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(left, e.y + SETTINGS_SECTION.2),
            Vec2::new((e.rect.max.x - left).max(0.), 1.),
        ),
        0.,
        t.pdn_separator,
    );
    e.y += 15. + bottom;
}
