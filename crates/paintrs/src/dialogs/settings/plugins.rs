//! Settings ▸ Plugins: the installed plug-in files (`platform::plugins`): Paint.NET plug-ins
//! (`.dll`), PhotoCraft filters (`.wasm`), LightCraft looks (`.cube`, `.3dl`) and develop presets
//! (`.xmp`), each an Effects ▸ Plugins item; Install… (or a drop) takes any of them or a `.zip`.
//! Laid out as Fonts is.
use super::fonts::action;
use super::{EntryUi, mix, reveal_focus};
use crate::{
    kit::{
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
    },
    theme::{self, DIALOG_FONT_SIZE},
};
use egui::{Pos2, Rect, Sense, Vec2};

const ROW: f32 = 40.;
const GAP: f32 = 6.;
/// A row's subtext line: what the plug-in added, or why it did not load.
const LINE: f32 = 15.;

/// One line of a row's subtext: a menu path (drawn with the menus' arrows between its parts), or a sentence.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Line {
    Path(Vec<String>),
    Text(String),
    /// An outside request the user always allows this plug-in to make, with a Revoke link.
    Grant(pdn_core::effect::permission::Grant),
}

/// The subtext under installed file `file`'s name: where each effect it added sits in the menus, the formats each of
/// its file types opens and saves, or why it did not load. A plug-in that loaded and added nothing says so; the
/// files a plug-in reads beside itself (.dlc, .json) have none.
pub(crate) fn row_lines(
    app: &pdn_core::app::AppWorkspace,
    manager: &crate::platform::plugins::PluginManager,
    file: &str,
    failed: &[(String, String)],
) -> Vec<Line> {
    let mut lines = what_it_added(app, manager, file, failed);
    // What the user always allows it, each revocable.
    let grants = pdn_core::effect::permission::with(|p| p.always_of(file));
    lines.extend(grants.into_iter().map(Line::Grant));
    lines
}

/// Where its effects went, its formats, or why it did not load.
fn what_it_added(
    app: &pdn_core::app::AppWorkspace,
    manager: &crate::platform::plugins::PluginManager,
    file: &str,
    failed: &[(String, String)],
) -> Vec<Line> {
    // One that loaded only in part lists what it added, then why the rest did not load.
    let reason = failed.iter().find(|(f, _)| f == file).map(|(_, r)| r);
    let added = pdn_core::effect::dotnet::provided(file)
        .filter(|p| !p.effects.is_empty() || !p.file_types.is_empty());
    if let Some(reason) = reason
        && added.is_none()
    {
        return vec![Line::Text(format!("Did not load: {reason}"))];
    }
    if let Some(id) = manager.effect_of(file) {
        return vec![Line::Path(crate::shell::main_menu::effect_menu_path(
            app, id,
        ))];
    }
    let Some(provided) = added.or_else(|| pdn_core::effect::dotnet::provided(file)) else {
        // A plug-in the host has not answered for yet (it loads, or a load never finished) says so.
        if file.to_ascii_lowercase().ends_with(".dll") {
            let state = if manager.busy() {
                "Loading…"
            } else {
                "Not loaded."
            };
            return vec![Line::Text(state.into())];
        }
        return vec![];
    };
    let mut lines: Vec<Line> = provided
        .effects
        .iter()
        .map(|id| Line::Path(crate::shell::main_menu::effect_menu_path(app, id)))
        .collect();
    for (name, open, save) in &provided.file_types {
        let list = |exts: &[String]| exts.join(", ");
        // An extension paint.rs reads or writes itself never reaches the plug-in: say so instead.
        let split = |exts: &[String], save| -> (Vec<String>, Vec<String>) {
            exts.iter().cloned().partition(|e| !built_in(e, save))
        };
        let ((open, own_open), (save, own_save)) = (split(open, false), split(save, true));
        let (open, save) = (&open, &save);
        let uses = |exts: &[String], what: &str| {
            let verb = if exts.len() == 1 { "uses" } else { "use" };
            Line::Text(format!(
                "{} {verb} paint.rs's own {what}, not {name}'s.",
                list(exts)
            ))
        };
        let mut own = Vec::new();
        if !own_open.is_empty() && own_open == own_save {
            own.push(uses(&own_open, "reader and writer"));
        } else {
            if !own_open.is_empty() {
                own.push(uses(&own_open, "reader"));
            }
            if !own_save.is_empty() {
                own.push(uses(&own_save, "writer"));
            }
        }
        match (open.is_empty(), save.is_empty()) {
            (false, false) if open == save => lines.push(Line::Text(format!(
                "Open and Save: {} ({name})",
                list(open)
            ))),
            _ => {
                if !open.is_empty() {
                    lines.push(Line::Text(format!("Open: {} ({name})", list(open))));
                }
                if !save.is_empty() {
                    lines.push(Line::Text(format!("Save: {} ({name})", list(save))));
                }
            }
        }
        lines.extend(own);
    }
    if lines.is_empty() {
        lines.push(Line::Text(
            "Loaded, but it adds no effects or file formats.".into(),
        ));
    }
    lines.extend(reason.map(|r| Line::Text(r.clone())));
    lines
}

/// Whether paint.rs opens (`save` false) or saves files with extension `ext` (".psd") with its own
/// codec, asking the same routing a real open or save does.
fn built_in(ext: &str, save: bool) -> bool {
    let name = format!("x.{}", ext.trim_start_matches('.'));
    let routed = if save {
        crate::platform::files::accepted_format(&name).ok()
    } else {
        pdn_core::io::codecs::detect_format(&name, None, &[])
    };
    routed.is_some_and(|id| !pdn_core::io::plugins::is_plugin_format(id))
}

/// A row's height for `lines` subtext lines.
fn row_height(lines: usize) -> f32 {
    if lines == 0 {
        ROW
    } else {
        30. + lines as f32 * LINE + 4.
    }
}

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
        "Install Paint.NET plug-ins (.dll), PhotoCraft filters (.wasm), LightCraft looks (.cube, .3dl) and \
         develop presets (.xmp), or a .zip of them. Effects and adjustments appear in the Effects menu's \
         Plugins submenu, and FileType plug-ins add formats to Open and Save. Filters, looks and presets run \
         sandboxed: they cannot reach your files, the network or other apps. A plug-in that cannot load is \
         listed below with the reason.",
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
    let failed = manager.failed.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let folder = manager.folder().map(|f| f.display().to_string());
    let install = Rect::from_min_size(Pos2::new(e.rect.min.x, e.y), Vec2::new(112., 27.));
    let install = action(e, "Install…", "Install plug-ins…", install, ready && !busy).0;
    e.y += 27. + 10.;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(folder) = folder {
        paragraph(e, &format!("Folder: {folder}"), soft);
    }
    if !status.is_empty() {
        paragraph(e, &status, t.menu_item_text);
    }
    if !failed.is_empty() {
        e.y += 6.;
        e.add_section_heading("Did not load");
        for (file, reason) in &failed {
            paragraph(e, &format!("{file}: {reason}"), t.menu_item_text);
            e.y += 4.;
        }
    }
    e.y += 6.;
    e.add_section_heading("Installed");
    let list = Rect::from_min_max(
        Pos2::new(e.rect.min.x, e.y),
        Pos2::new(e.rect.max.x, e.rect.max.y - 10.),
    );
    let mut remove = None;
    let mut revoke = None;
    if items.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(list.width(), 70.));
        e.ui.painter().rect_filled(empty, 0., t.pdn_menu_border);
        e.ui.painter()
            .rect_filled(empty.shrink(1.), 0., t.number_input_background);
        let label = if ready {
            "Nothing is installed yet. Install a plug-in (.dll, .wasm), a look (.cube, .3dl), a preset (.xmp) or a .zip of them."
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
        let lines: Vec<Vec<Line>> = match e.cx.asset_library.as_deref() {
            Some(library) => items
                .iter()
                .map(|f| row_lines(e.cx.app, &library.plugins, f, &failed))
                .collect(),
            None => vec![vec![]; items.len()],
        };
        let tops: Vec<f32> = lines
            .iter()
            .scan(0., |top, l| {
                let at = *top;
                *top += row_height(l.len()) + GAP;
                Some(at)
            })
            .collect();
        let content = lines
            .iter()
            .map(|l| (row_height(l.len()) + GAP) as f64)
            .sum::<f64>()
            - GAP as f64;
        let max = (content - list.height() as f64).max(0.);
        if list.contains(e.ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
            scroll = (scroll - e.ui.input(|i| i.smooth_scroll_delta.y) as f64).clamp(0., max);
        }
        let clip = e.ui.clip_rect();
        e.ui.set_clip_rect(clip.intersect(list));
        for (i, file) in items.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(list.min.x, list.min.y + tops[i] - scroll as f32),
                Vec2::new(
                    list.width() - if max > 0. { THICKNESS } else { 4. },
                    row_height(lines[i].len()),
                ),
            );
            if !row.intersects(list) {
                continue;
            }
            e.ui.painter().rect_filled(row, 0., t.window_border);
            e.ui.painter()
                .rect_filled(row.shrink(1.), 0., t.number_input_background);
            let name_y = if lines[i].is_empty() { 12. } else { 8. };
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(
                    row.min + Vec2::new(8., name_y),
                    Vec2::new(row.width() - 104., 15.),
                ),
                file,
                FontSpec::new(DIALOG_FONT_SIZE).bold(),
                t.menu_item_text,
            );
            // Its subtext: where its effects went, its formats, or why it did not load.
            let soft = mix(t.menu_item_text, t.number_input_background, 0.4);
            for (k, line) in lines[i].iter().enumerate() {
                let at = row.min + Vec2::new(8., 28. + k as f32 * LINE);
                let width = row.width() - 104.;
                match line {
                    Line::Path(parts) => crate::kit::glyph::draw_path(
                        e.ui.painter(),
                        at,
                        parts,
                        FontSpec::new(11.),
                        soft,
                        t.number_input_background,
                        width,
                    ),
                    Line::Text(sentence) => text::draw_ellipsized(
                        e.ui.painter(),
                        Rect::from_min_size(at, Vec2::new(width, 14.)),
                        sentence,
                        FontSpec::new(11.),
                        soft,
                    ),
                    Line::Grant(grant) => {
                        let sentence = format!("Always allowed to {}.", grant.wants());
                        let spec = FontSpec::new(11.);
                        // The width the ellipsizing judges by (`css_width`), so it fits whole when there is room.
                        let w = (text::css_width(&sentence, 11.).ceil() + 1.).min(width - 50.);
                        text::draw_ellipsized(
                            e.ui.painter(),
                            Rect::from_min_size(at, Vec2::new(w, 14.)),
                            &sentence,
                            spec,
                            soft,
                        );
                        let link = Rect::from_min_size(
                            at + Vec2::new(w + 8., 0.),
                            Vec2::new(text::measure(e.ui.painter(), "Revoke", spec), 14.),
                        );
                        let id = format!("revoke:{file}:{}:{}", grant.category, grant.target);
                        let response =
                            e.ui.interact(link, e.ui.scope_id().with(id), Sense::click());
                        let color = if response.hovered() {
                            t.link_hover
                        } else {
                            t.link
                        };
                        text::draw(e.ui.painter(), link.min, "Revoke", spec, color);
                        e.ui.ctx().accesskit_node_builder(response.id, |n| {
                            n.set_role(egui::accesskit::Role::Link);
                            n.set_label(format!("Revoke: {file} may {}", grant.wants()));
                        });
                        if response.clicked() {
                            revoke = Some((file.clone(), grant.clone()));
                        }
                    }
                }
            }
            let rect = Rect::from_min_size(
                Pos2::new(row.max.x - 78., row.center().y - 13.),
                Vec2::new(70., 26.),
            );
            let (clicked, response) =
                action(e, "Remove", &format!("remove-plugin:{file}"), rect, !busy);
            e.ui.ctx()
                .accesskit_node_builder(response.id, |n| n.set_label(format!("Remove {file}")));
            if clicked {
                remove = Some(file.clone());
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
    if let Some(file) = remove {
        library.plugins.remove(&file);
    }
    if let Some((file, grant)) = revoke {
        pdn_core::effect::permission::with(|p| p.revoke(&file, &grant));
        crate::platform::plugins::remember_permissions(e.cx.prefs);
    }
}
