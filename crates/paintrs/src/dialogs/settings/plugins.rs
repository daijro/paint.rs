//! Settings ▸ Plugins: the installed plug-ins and looks (`platform::plugins`), Install… for files
//! and Browse Store… for the ArtCraft Store (`dialogs::plugin_store`), laid out as Fonts is.
use super::fonts::action;
use super::{EntryUi, mix, reveal_focus};
use crate::{
    kit::{
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
    },
    theme::{self, DIALOG_FONT_SIZE},
};
use egui::{Pos2, Rect, Vec2};

const ROW: f32 = 64.;
const PITCH: f32 = 70.;

/// `text` wrapped to the page width in the Settings paragraph colour.
fn paragraph(e: &mut EntryUi, text: &str, color: egui::Color32) {
    let mut line = String::new();
    let mut lines = vec![];
    for word in text.split_whitespace() {
        let next = if line.is_empty() {
            word.into()
        } else {
            format!("{line} {word}")
        };
        if text::css_width(&next, DIALOG_FONT_SIZE) > e.rect.width() && !line.is_empty() {
            lines.push(std::mem::replace(&mut line, word.into()));
        } else {
            line = next;
        }
    }
    lines.push(line);
    for l in lines {
        let at = Pos2::new(e.rect.min.x, e.y);
        text::draw(
            e.ui.painter(),
            at,
            &l,
            FontSpec::new(DIALOG_FONT_SIZE),
            color,
        );
        e.y += 18.;
    }
}

pub fn show(e: &mut EntryUi) {
    let t = theme::current(e.ui.ctx());
    let soft = mix(t.menu_item_text, t.window_background, 0.74);
    e.add_heading("Plugins", "");
    paragraph(
        e,
        "Install PhotoCraft filter plug-ins (.wasm), LightCraft looks (.cube, .3dl) and develop \
         presets (.xmp). They appear under Effects > Plugins, marked with a puzzle piece. \
         Plug-ins run sandboxed: they cannot reach your files, the network or other apps.",
        soft,
    );
    e.y += 9.;
    let Some(library) = e.cx.asset_library.as_deref_mut() else {
        return;
    };
    let manager = &library.plugins;
    let (ready, busy) = (manager.ready(), manager.busy());
    let items = manager.items.clone();
    let status = manager.status.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let folder = manager.folder().map(|f| f.display().to_string());
    let button =
        |x: f32, w: f32| Rect::from_min_size(Pos2::new(e.rect.min.x + x, e.y), Vec2::new(w, 27.));
    let (install, browse) = (button(0., 112.), button(120., 128.));
    let install = action(e, "Install…", "Install plug-ins…", install, ready && !busy).0;
    let browse = action(e, "Browse Store…", "Browse Store…", browse, true).0;
    e.y += 27. + 10.;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(folder) = folder {
        paragraph(e, &format!("Folder: {folder}"), soft);
    }
    if !status.is_empty() {
        paragraph(e, &status, t.menu_item_text);
    }
    e.y += 6.;
    e.add_section_heading("Installed");
    let list = Rect::from_min_max(
        Pos2::new(e.rect.min.x, e.y),
        Pos2::new(e.rect.max.x, e.rect.max.y - 10.),
    );
    let mut remove = None;
    if items.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(list.width(), 70.));
        e.ui.painter().rect_filled(empty, 0., t.pdn_menu_border);
        e.ui.painter()
            .rect_filled(empty.shrink(1.), 0., t.number_input_background);
        let label = if ready {
            "Nothing is installed yet. Install a file or browse the store."
        } else {
            "Loading installed plug-ins…"
        };
        let color = mix(t.menu_item_text, t.number_input_background, 0.66);
        text::draw(
            e.ui.painter(),
            empty.min + Vec2::splat(13.),
            label,
            FontSpec::new(DIALOG_FONT_SIZE),
            color,
        );
    } else {
        let scroll_id = e.ui.scope_id().with("settings-plugins-scroll");
        let mut scroll = e.ui.data(|d| d.get_temp::<f64>(scroll_id)).unwrap_or(0.);
        let content = items.len() as f64 * PITCH as f64 - (PITCH - ROW) as f64;
        let max = (content - list.height() as f64).max(0.);
        if list.contains(e.ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
            scroll = (scroll - e.ui.input(|i| i.smooth_scroll_delta.y) as f64).clamp(0., max);
        }
        let clip = e.ui.clip_rect();
        e.ui.set_clip_rect(clip.intersect(list));
        for (i, item) in items.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(list.min.x, list.min.y + i as f32 * PITCH - scroll as f32),
                Vec2::new(list.width() - if max > 0. { THICKNESS } else { 4. }, ROW),
            );
            if !row.intersects(list) {
                continue;
            }
            e.ui.painter().rect_filled(row, 0., t.window_border);
            e.ui.painter()
                .rect_filled(row.shrink(1.), 0., t.number_input_background);
            let text_w = row.width() - 104.;
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(row.min + Vec2::new(8., 7.), Vec2::new(text_w, 15.)),
                &item.name,
                FontSpec::new(DIALOG_FONT_SIZE).bold(),
                t.menu_item_text,
            );
            // Where it is in the menus.
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(row.min + Vec2::new(8., 25.), Vec2::new(text_w, 15.)),
                &format!("Effects > Plugins > {}", item.name),
                FontSpec::new(11.).bold(),
                t.menu_item_selected_border,
            );
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(row.min + Vec2::new(8., 42.), Vec2::new(text_w, 15.)),
                &format!("{} · {}", item.detail, item.file),
                FontSpec::new(11.),
                mix(t.menu_item_text, t.number_input_background, 0.64),
            );
            let rect = Rect::from_min_size(
                Pos2::new(row.max.x - 78., row.center().y - 13.),
                Vec2::new(70., 26.),
            );
            let (clicked, response) = action(
                e,
                "Remove",
                &format!("remove-plugin:{}", item.id),
                rect,
                !busy,
            );
            e.ui.ctx().accesskit_node_builder(response.id, |n| {
                n.set_label(format!("Remove {}", item.name))
            });
            if clicked {
                remove = Some(item.id.clone());
            }
            reveal_focus(e.ui, &response, &mut scroll, max);
        }
        e.ui.set_clip_rect(clip);
        if max > 0. {
            Scrollbar::vertical(Rect::from_min_size(
                Pos2::new(list.max.x - THICKNESS, list.min.y),
                Vec2::new(THICKNESS, list.height()),
            ))
            .id_salt("installed-plugins")
            .show(e.ui, content, list.height() as f64, &mut scroll);
        }
        e.ui.data_mut(|d| d.insert_temp(scroll_id, scroll));
    }
    let Some(library) = e.cx.asset_library.as_deref_mut() else {
        return;
    };
    if install {
        library.plugins.pick();
    }
    if let Some(id) = remove {
        library.plugins.remove(&id);
    }
    if browse {
        crate::dialogs::plugin_store::PluginStoreDialog::open();
    }
}
