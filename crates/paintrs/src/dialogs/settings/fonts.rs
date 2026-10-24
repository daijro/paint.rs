//! SettingsFontsEntry.js, css/window.css. Reference: paintdotjs @ b4e6fc5.
//! Preview rendering uses unhinted grayscale coverage, ui-text-and-glyphs §2.1.
use super::{EntryUi, mix, reveal_focus, settings_button};
use crate::dialogs::task::{TaskChoice, TaskDialog, TaskDialogHandle, TaskDialogSpec};
use crate::{
    kit::{
        inputs,
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
    },
    theme::{self, DIALOG_FONT_SIZE},
};
use egui::{Pos2, Rect, Sense, Vec2};
#[derive(Default)]
pub(super) struct FontsState {
    scroll: f64,
    clear: Option<TaskDialogHandle>,
    errors: Vec<String>,
}
pub(super) fn action(
    e: &mut EntryUi,
    label: &str,
    id: &str,
    rect: Rect,
    enabled: bool,
) -> (bool, egui::Response) {
    if !enabled {
        let t = theme::current(e.ui.ctx());
        e.ui.painter()
            .rect_filled(rect, 0., mix(t.button_outline, t.window_background, 0.55));
        e.ui.painter().rect_filled(
            rect.shrink(1.),
            0.,
            mix(t.button, t.window_background, 0.55),
        );
        text::draw(
            e.ui.painter(),
            Pos2::new(
                rect.center().x - text::css_width(label, DIALOG_FONT_SIZE) / 2.,
                rect.center().y - 7.5,
            ),
            label,
            FontSpec::new(DIALOG_FONT_SIZE),
            mix(t.button_text, t.window_background, 0.55),
        );
        let r =
            e.ui.interact(rect, e.ui.scope_id().with(id), Sense::hover());
        e.ui.ctx().accesskit_node_builder(r.id, |n| {
            n.set_role(egui::accesskit::Role::Button);
            n.set_label(id);
            n.set_disabled();
        });
        return (false, r);
    }
    let response = settings_button(e.ui, id, rect, label, false);
    e.ui.ctx()
        .accesskit_node_builder(response.id, |n| n.set_label(id));
    inputs::take_enter(e.ui, &response);
    (inputs::button_activated(e.ui, &response), response)
}
pub fn show(e: &mut EntryUi) {
    let t = theme::current(e.ui.ctx());
    e.add_heading("Fonts", "");
    let description = "Add fonts for the Text tool. Font files are stored locally in this app and are not uploaded.";
    let width = e.rect.width();
    // Wrap the ordinary page description using the same text layout as Settings paragraphs.
    let words: Vec<_> = description.split_whitespace().collect();
    let mut line = String::new();
    for word in words {
        let next = if line.is_empty() {
            word.into()
        } else {
            format!("{line} {word}")
        };
        if text::css_width(&next, DIALOG_FONT_SIZE) > width && !line.is_empty() {
            text::draw(
                e.ui.painter(),
                Pos2::new(e.rect.min.x, e.y),
                &line,
                FontSpec::new(DIALOG_FONT_SIZE),
                mix(t.menu_item_text, t.window_background, 0.74),
            );
            e.y += 18.;
            line = word.into();
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        text::draw(
            e.ui.painter(),
            Pos2::new(e.rect.min.x, e.y),
            &line,
            FontSpec::new(DIALOG_FONT_SIZE),
            mix(t.menu_item_text, t.window_background, 0.74),
        );
        e.y += 27.;
    }
    let Some(manager) = e.cx.fonts.as_deref() else {
        return;
    };
    let ready = manager.ready();
    let busy = manager.importing();
    let loading = manager.busy();
    let status = manager.status.clone();
    let mut records = manager.registry.records.clone();
    records.sort_by_key(|r| r.name.to_lowercase());
    let progress = manager.progress().map(|(a, b, c)| (a, b, c.to_owned()));
    let errors = manager.errors().to_vec();
    e.state.status = status;
    let mut add = false;
    let mut clear = false;
    let mut remove = None;
    let controls_y = e.y + 3.;
    add |= action(
        e,
        "Add fonts…",
        "Add fonts…",
        Rect::from_min_size(Pos2::new(e.rect.min.x, controls_y), Vec2::new(112., 27.)),
        ready && !busy,
    )
    .0;
    clear |= action(
        e,
        "Clear all",
        "Clear all",
        Rect::from_min_size(
            Pos2::new(e.rect.min.x + 120., controls_y),
            Vec2::new(112., 27.),
        ),
        ready && !busy && !records.is_empty(),
    )
    .0;
    e.y = controls_y + 27. + 12.;
    let drop = Rect::from_min_size(Pos2::new(e.rect.min.x, e.y), Vec2::new(width, 68.));
    let response = e.ui.interact(
        drop,
        e.ui.scope_id().with("font-drop-zone"),
        if busy { Sense::hover() } else { Sense::click() },
    );
    let hover = response.has_focus()
        || (!e.ui.input(|i| i.raw.hovered_files.is_empty()) && response.hovered());
    let fill = if hover {
        t.menu_item_hover
    } else {
        mix(t.number_input_background, t.window_background, 0.88)
    };
    e.ui.painter().rect_filled(drop, 0., fill);
    let border = if hover {
        t.menu_item_selected_border
    } else {
        t.pdn_control_border
    };
    for horizontal in [true, false] {
        let length = if horizontal {
            drop.width()
        } else {
            drop.height()
        };
        let count = ((length + 2.) / 5.).floor().max(1.) as usize;
        let step = if count > 1 {
            (length - 3.) / (count - 1) as f32
        } else {
            0.
        };
        for edge in [false, true] {
            for i in 0..count {
                let offset = (i as f32 * step).round();
                let (at, size) = if horizontal {
                    (
                        Pos2::new(
                            drop.min.x + offset,
                            if edge { drop.max.y - 1. } else { drop.min.y },
                        ),
                        Vec2::new(3., 1.),
                    )
                } else {
                    (
                        Pos2::new(
                            if edge { drop.max.x - 1. } else { drop.min.x },
                            drop.min.y + offset,
                        ),
                        Vec2::new(1., 3.),
                    )
                };
                e.ui.painter()
                    .rect_filled(Rect::from_min_size(at, size), 0., border);
            }
        }
    }
    e.ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label("Drop font files");
        if busy {
            n.set_disabled();
        }
    });
    if response.clicked() {
        response.request_focus();
    }
    let label = "Drop .ttf or .otf files here";
    text::draw(
        e.ui.painter(),
        Pos2::new(
            drop.center().x - text::css_width(label, DIALOG_FONT_SIZE) / 2.,
            drop.center().y - 7.5,
        ),
        label,
        FontSpec::new(DIALOG_FONT_SIZE),
        mix(t.menu_item_text, fill, 0.72),
    );
    e.y = drop.max.y + 2.;
    if let Some((done, total, label)) = progress {
        let rect = Rect::from_min_size(Pos2::new(e.rect.min.x, e.y + 8.), Vec2::new(width, 54.));
        e.ui.painter().rect_filled(rect, 0., t.window_border);
        e.ui.painter()
            .rect_filled(rect.shrink(1.), 0., t.number_input_background);
        text::draw_ellipsized(
            e.ui.painter(),
            Rect::from_min_size(rect.min + Vec2::new(10., 8.), Vec2::new(width - 20., 18.)),
            &label,
            FontSpec::new(DIALOG_FONT_SIZE),
            t.menu_item_text,
        );
        let bar = Rect::from_min_size(rect.min + Vec2::new(10., 35.), Vec2::new(width - 20., 9.));
        e.ui.painter().rect_filled(bar, 0., t.button);
        e.ui.painter().rect_filled(
            Rect::from_min_size(
                bar.min,
                Vec2::new(
                    bar.width() * done as f32 / total.max(1) as f32,
                    bar.height(),
                ),
            ),
            0.,
            t.menu_item_selected_border,
        );
        e.y = rect.max.y;
    }
    e.add_section_heading("Installed by you");
    let list = Rect::from_min_max(
        Pos2::new(e.rect.min.x, e.y),
        Pos2::new(e.rect.max.x, e.rect.max.y - 10.),
    );
    if records.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(list.width(), 70.));
        e.ui.painter().rect_filled(empty, 0., t.pdn_menu_border);
        e.ui.painter()
            .rect_filled(empty.shrink(1.), 0., t.number_input_background);
        text::draw(
            e.ui.painter(),
            empty.min + Vec2::splat(13.),
            if ready {
                "No custom fonts have been added."
            } else if loading {
                "Loading installed fonts…"
            } else {
                "Installed fonts could not be loaded."
            },
            FontSpec::new(DIALOG_FONT_SIZE),
            mix(t.menu_item_text, t.number_input_background, 0.66),
        );
    } else {
        let content = records.len() as f64 * 65. - 7.;
        let max = (content - list.height() as f64).max(0.);
        if list.contains(e.ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
            e.state.fonts.scroll = (e.state.fonts.scroll
                - e.ui.input(|i| i.smooth_scroll_delta.y) as f64)
                .clamp(0., max);
        }
        let clip = e.ui.clip_rect();
        e.ui.set_clip_rect(clip.intersect(list));
        for (i, r) in records.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(
                    list.min.x,
                    list.min.y + i as f32 * 65. - e.state.fonts.scroll as f32,
                ),
                Vec2::new(list.width() - if max > 0. { THICKNESS } else { 4. }, 58.),
            );
            if !row.intersects(list) {
                continue;
            }
            e.ui.painter().rect_filled(row, 0., t.window_border);
            e.ui.painter()
                .rect_filled(row.shrink(1.), 0., t.number_input_background);
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(row.min + Vec2::new(8., 8.), Vec2::new(170., 15.)),
                &r.name,
                FontSpec::new(DIALOG_FONT_SIZE).bold(),
                t.menu_item_text,
            );
            let size = if r.size < 1024 {
                format!("{} B", r.size)
            } else if r.size < 1024 * 1024 {
                format!("{:.1} KB", r.size as f64 / 1024.)
            } else {
                format!("{:.1} MB", r.size as f64 / (1024. * 1024.))
            };
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(row.min + Vec2::new(8., 26.), Vec2::new(170., 15.)),
                &format!("{} · {size}", r.file_name),
                FontSpec::new(11.),
                mix(t.menu_item_text, t.number_input_background, 0.64),
            );
            crate::platform::fonts::request_preview(e.ui.ctx(), &r.family);
            crate::kit::fonts::draw_document_preview(
                e.ui.painter(),
                Rect::from_min_max(
                    row.min + Vec2::new(188., 15.5),
                    row.max - Vec2::new(90., 8.),
                ),
                &r.family,
                "The quick brown fox jumps over the lazy dog 0123456789",
                t.menu_item_text,
            );
            let id = format!("remove-font:{}", r.id);
            let rect = Rect::from_min_size(
                Pos2::new(row.max.x - 78., row.center().y - 13.),
                Vec2::new(70., 26.),
            );
            let (clicked, response) = action(e, "Remove", &id, rect, !busy);
            e.ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_label(format!("Remove {}", r.name))
            });
            if clicked {
                remove = Some(r.id.clone());
            }
            reveal_focus(e.ui, &response, &mut e.state.fonts.scroll, max);
        }
        e.ui.set_clip_rect(clip);
        if max > 0. {
            Scrollbar::vertical(Rect::from_min_size(
                Pos2::new(list.max.x - THICKNESS, list.min.y),
                Vec2::new(THICKNESS, list.height()),
            ))
            .id_salt("installed-fonts")
            .show(
                e.ui,
                content,
                list.height() as f64,
                &mut e.state.fonts.scroll,
            );
        }
    }
    if clear {
        e.state.fonts.clear = Some(TaskDialog::show(TaskDialogSpec {
            title: "Clear Installed Fonts".into(),
            icon: Some("warning_icon.png"),
            message: Some(format!(
                "Remove all {} custom fonts from {}? This cannot be undone.",
                records.len(),
                crate::dialogs::about::PRODUCT
            )),
            cancel_value: "cancel",
            choices: vec![
                TaskChoice {
                    value: "clear",
                    title: "Clear All Fonts".into(),
                    description: Some(format!(
                        "Delete every custom font stored by {}.",
                        crate::dialogs::about::PRODUCT
                    )),
                    icon: Some("trash_can.png"),
                },
                TaskChoice {
                    value: "cancel",
                    title: "Cancel".into(),
                    description: Some("Keep the installed fonts.".into()),
                    icon: Some("cancel_icon.png"),
                },
            ],
            ..Default::default()
        }));
    }
    let confirmed = e.state.fonts.clear.as_ref().and_then(|h| h.poll());
    if confirmed.is_some() {
        e.state.fonts.clear = None;
    }
    if let Some(m) = e.cx.fonts.as_deref_mut() {
        if add {
            m.pick(&crate::platform::Files::current());
        }
        if let Some(id) = remove {
            m.remove(&id);
        }
        if confirmed == Some("clear") {
            m.clear();
        }
        if m.busy() {
            e.ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(10));
        }
    }
    if !busy && !errors.is_empty() && errors != e.state.fonts.errors {
        e.state.fonts.errors = errors.clone();
        TaskDialog::show(TaskDialogSpec {
            title: "Fonts".into(),
            message: Some(format!(
                "Some fonts could not be added:\n\n{}",
                errors.join("\n")
            )),
            cancel_value: "close",
            choices: vec![TaskChoice {
                value: "close",
                title: "Close".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
}
