//! SettingsIconPacksEntry.js / AssetPackManager.js, css/window.css; paintdotjs b4e6fc5.
use super::{EntryUi, asset_widgets as w, mix};
use crate::{
    assets::Assets,
    icon_pack::IconPackId,
    kit::{
        img, inputs, px,
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
    },
    theme::{self, *},
};
use egui::{Color32, Pos2, Rect, Sense, Vec2};
use std::{collections::HashMap, sync::Arc};
pub(super) const PREVIEWS: [&str; 6] = [
    "paint_brush_tool_icon.png",
    "paint_bucket_icon.png",
    "pencil_tool_icon.png",
    "eraser_tool_icon.png",
    "shapes_tool_icon.png",
    "color_picker_tool_icon.png",
];
#[derive(Default)]
pub(super) struct PacksState {
    scroll: f64,
}
type PreviewKey = (String, String, u32, bool);
type PreviewValue = (Arc<image::RgbaImage>, u64);
#[derive(Clone, Default)]
struct PreviewCache {
    pass: u64,
    images: HashMap<PreviewKey, PreviewValue>,
}
pub fn show(e: &mut EntryUi) {
    e.add_heading("Icon Packs", "");
    e.y -= 4.;
    w::paragraph(
        e,
        "Install local icon packs containing icons, images, cursors, or translations.",
        0.,
        6.,
    );
    w::paragraph(
        e,
        "Icon packs stay on this device. Only install packs whose contents you have permission to use.",
        0.,
        10.,
    );
    let Some(m) = e.cx.asset_library.as_deref() else {
        return;
    };
    let ready = m.ready() && !m.busy();
    let active = Assets::selected(e.ui.ctx()).saved();
    e.state.status = m.status.clone();
    let mut rows = vec![(
        "paint-rs".to_owned(),
        "paint.rs".to_owned(),
        "Bundled SVG icons · original paint.rs artwork".to_owned(),
        "The default colour icon set.".to_owned(),
    )];
    rows.extend(m.packs.iter().map(|p| {
        let manifest = &p.pack.manifest;
        let count = p.pack.files.len();
        let locales = p.pack.translations.len();
        (
            format!("user:{}", p.id),
            p.name().to_owned(),
            format!(
                "{} by {} · {} {} · {} {}",
                manifest["version"].as_str().unwrap_or("1.0.0"),
                manifest["author"].as_str().unwrap_or("Unknown"),
                count,
                if count == 1 { "image" } else { "images" },
                locales,
                if locales == 1 {
                    "language"
                } else {
                    "languages"
                }
            ),
            manifest["description"].as_str().unwrap_or("").to_owned(),
        )
    }));
    let y = e.y;
    let label = "Install icon pack…";
    let width =
        (text::css_width(label, DIALOG_FONT_SIZE) + SETTINGS_BUTTON.2).max(SETTINGS_BUTTON.0);
    let add = w::button(
        e,
        label,
        label,
        Rect::from_min_size(
            Pos2::new(e.rect.min.x, y + 3.),
            Vec2::new(width, ASSET_BUTTON_HEIGHT),
        ),
        ready,
    );
    let reload = "Reload translations";
    if w::button(
        e,
        reload,
        reload,
        Rect::from_min_size(
            Pos2::new(e.rect.min.x + width + ASSET_CONTROL_GAP, y + 3.),
            Vec2::new(
                text::css_width(reload, DIALOG_FONT_SIZE) + SETTINGS_BUTTON.2,
                ASSET_BUTTON_HEIGHT,
            ),
        ),
        true,
    ) {
        e.state.reload_requested = true;
    }
    e.y = y + 34.;
    let drop = w::drop_zone(e, "Drop an icon-pack ZIP here", "asset-drop", ready);
    let t = theme::current(e.ui.ctx());
    let label = "Learn how to create your own icon pack";
    let link = Rect::from_min_size(
        Pos2::new(e.rect.min.x, e.y),
        Vec2::new(text::css_width(label, DIALOG_FONT_SIZE), 15.),
    );
    let response = e.ui.interact(
        link,
        e.ui.scope_id().with("pack-documentation"),
        Sense::click(),
    );
    e.ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Link);
        n.set_label(label);
    });
    text::draw(
        e.ui.painter(),
        link.min,
        label,
        FontSpec::new(DIALOG_FONT_SIZE),
        if response.hovered() {
            t.link_hover
        } else {
            t.link
        },
    );
    if response.clicked() || inputs::button_activated(e.ui, &response) {
        e.ui.ctx().open_url(egui::OpenUrl::new_tab(
            "https://github.com/LabyStudio/paintdotjs/blob/master/docs/asset-packs.md",
        ));
    }
    e.y += 25.;
    w::section(e, "Installed icon packs", 5.);
    let list = Rect::from_min_max(
        Pos2::new(e.rect.min.x, e.y),
        Pos2::new(e.rect.max.x, e.ui.clip_rect().max.y - 14.),
    );
    let content =
        rows.len() as f64 * (ASSET_PACK_ROW_HEIGHT + ASSET_ROW_GAP) as f64 - ASSET_ROW_GAP as f64;
    let max = (content - list.height() as f64).max(0.);
    if list.contains(e.ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
        e.state.icon_packs.scroll = (e.state.icon_packs.scroll
            - e.ui.input(|i| i.smooth_scroll_delta.y) as f64)
            .clamp(0., max);
    }
    let clip = e.ui.clip_rect();
    e.ui.set_clip_rect(clip.intersect(list));
    let mut activate = None;
    let mut remove = None;
    for (i, (id, name, metadata, description)) in rows.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(
                list.min.x,
                list.min.y + i as f32 * (ASSET_PACK_ROW_HEIGHT + ASSET_ROW_GAP)
                    - e.state.icon_packs.scroll as f32,
            ),
            Vec2::new(
                list.width() - if max > 0. { THICKNESS } else { 0. },
                ASSET_PACK_ROW_HEIGHT,
            ),
        );
        if !row.intersects(list) {
            continue;
        }
        e.ui.painter().rect_filled(row, 0., t.window_border);
        e.ui.painter()
            .rect_filled(row.shrink(1.), 0., t.number_input_background);
        let details = Rect::from_min_size(
            row.min + Vec2::splat(ASSET_ROW_PADDING),
            Vec2::new((row.width() - 167.).max(0.), 77.),
        );
        text::draw_ellipsized(
            e.ui.painter(),
            Rect::from_min_size(details.min, Vec2::new(details.width(), 15.)),
            name,
            FontSpec::new(DIALOG_FONT_SIZE).bold(),
            t.menu_item_text,
        );
        for (line, offset) in [(metadata, 17.), (description, 32.)] {
            text::draw_ellipsized(
                e.ui.painter(),
                Rect::from_min_size(
                    details.min + Vec2::new(0., offset),
                    Vec2::new(details.width(), 13.),
                ),
                line,
                FontSpec::new(ASSET_METADATA_SIZE),
                mix(t.menu_item_text, t.number_input_background, 0.64),
            );
        }
        let button = Rect::from_min_size(
            Pos2::new(row.max.x - 154., row.min.y + ASSET_ROW_PADDING),
            Vec2::new(SETTINGS_BUTTON.0, ASSET_BUTTON_HEIGHT),
        );
        let chosen = active == *id;
        let label = if chosen { "Active" } else { "Use" };
        if row_button(
            e,
            label,
            &format!("pack-{id}"),
            button,
            ready && !chosen,
            t.number_input_background,
            &format!("{} {name}", if chosen { "Active" } else { "Use" }),
        ) {
            activate = Some(id.clone());
        }
        let cancel = Rect::from_min_size(
            Pos2::new(row.max.x - 36., row.min.y + ASSET_ROW_PADDING),
            Vec2::new(ASSET_REMOVE_SIZE.0, ASSET_REMOVE_SIZE.1),
        );
        if row_button(
            e,
            "",
            &format!("Remove {name}"),
            cancel,
            ready && id.starts_with("user:"),
            t.number_input_background,
            &format!("Remove {name}"),
        ) {
            remove = Some(id.clone());
        }
        if !id.starts_with("user:") {
            img::paint_icon_in_layer(
                e.ui.painter(),
                "cancel_icon.png",
                cancel.shrink(4.),
                t.button,
                img::Filter::Opacity {
                    alpha: px::css_alpha(0.55),
                    background: t.number_input_background,
                },
                cancel.min,
            );
        } else {
            img::paint_icon(
                e.ui.painter(),
                "cancel_icon.png",
                cancel.shrink(4.),
                t.button,
                img::Filter::None,
            );
        }
        let preview_y = details.min.y + 49.;
        let previews = preview_files(e, id);
        for (j, (key, pixels)) in previews.into_iter().take(6).enumerate() {
            let outer = Rect::from_min_size(
                Pos2::new(
                    details.min.x + j as f32 * (ASSET_PREVIEW_SIZE + ASSET_PREVIEW_GAP),
                    preview_y,
                ),
                Vec2::splat(ASSET_PREVIEW_SIZE),
            );
            if outer.max.x > details.max.x {
                break;
            }
            let bg = mix(t.window_background, t.number_input_background, 0.62);
            e.ui.painter().rect_filled(outer, 0., t.window_border);
            e.ui.painter().rect_filled(outer.shrink(1.), 0., bg);
            let area = outer.shrink(3.);
            let scale =
                (area.width() / pixels.width() as f32).min(area.height() / pixels.height() as f32);
            let size = Vec2::new(pixels.width() as f32, pixels.height() as f32) * scale;
            img::paint_generated(
                e.ui.painter(),
                &format!("pack-preview-{id}-{key}"),
                &pixels,
                px::snap_rect(
                    Rect::from_center_size(area.center(), size),
                    e.ui.ctx().pixels_per_point(),
                ),
                bg,
            );
        }
    }
    e.ui.set_clip_rect(clip);
    if max > 0. {
        Scrollbar::vertical(Rect::from_min_size(
            Pos2::new(list.max.x - THICKNESS, list.min.y),
            Vec2::new(THICKNESS, list.height()),
        ))
        .id_salt("installed-packs")
        .show(
            e.ui,
            content,
            list.height() as f64,
            &mut e.state.icon_packs.scroll,
        );
    }
    if let Some(m) = e.cx.asset_library.as_deref_mut() {
        if add || drop {
            m.pick("assets");
        }
        if let Some(id) = activate
            && let Err(error) =
                m.select_icon(IconPackId::from_saved(Some(&id)), e.ui.ctx(), e.cx.prefs)
        {
            e.state.status = error;
        }
        if let Some(id) = remove {
            m.remove_pack(
                id.strip_prefix("user:").unwrap_or(&id),
                e.ui.ctx(),
                e.cx.prefs,
            );
        }
    }
}
fn row_button(
    e: &mut EntryUi,
    label: &str,
    id: &str,
    rect: Rect,
    enabled: bool,
    bg: Color32,
    accessible: &str,
) -> bool {
    if enabled {
        let (clicked, response) = super::fonts::action(e, label, id, rect, true);
        e.ui.ctx()
            .accesskit_node_builder(response.id, |n| n.set_label(accessible));
        return clicked;
    }
    let t = theme::current(e.ui.ctx());
    let alpha = px::css_alpha(0.55);
    e.ui.painter()
        .rect_filled(rect, 0., px::blend(bg, t.button_outline, alpha));
    e.ui.painter()
        .rect_filled(rect.shrink(1.), 0., px::blend(bg, t.button, alpha));
    if !label.is_empty() {
        text::draw(
            e.ui.painter(),
            Pos2::new(
                rect.center().x - text::css_width(label, DIALOG_FONT_SIZE) / 2.,
                rect.center().y - 7.5,
            ),
            label,
            FontSpec::new(DIALOG_FONT_SIZE),
            px::blend(bg, t.button_text, alpha),
        );
    }
    let response =
        e.ui.interact(rect, e.ui.scope_id().with(id), Sense::hover());
    e.ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label(accessible);
        n.set_disabled();
    });
    false
}
fn preview_files(e: &EntryUi, id: &str) -> Vec<(String, Arc<image::RgbaImage>)> {
    let pass = e.ui.ctx().cumulative_pass_nr();
    let cache_id = egui::Id::unique("asset-pack-previews");
    let mut out = vec![];
    let paths = if !id.starts_with("user:") {
        PREVIEWS
            .iter()
            .map(|name| format!("assets/icons/{name}"))
            .collect()
    } else {
        let Some(m) = e.cx.asset_library.as_deref() else {
            return out;
        };
        let Some(pack) = m
            .packs
            .iter()
            .find(|p| Some(p.id.as_str()) == id.strip_prefix("user:"))
        else {
            return out;
        };
        let mut paths = PREVIEWS
            .iter()
            .map(|name| format!("assets/icons/{name}"))
            .filter(|p| pack.pack.files.contains_key(p))
            .collect::<Vec<_>>();
        let mut rest = pack
            .pack
            .files
            .keys()
            .filter(|p| !paths.contains(p))
            .cloned()
            .collect::<Vec<_>>();
        rest.sort_by_key(|p| (!p.starts_with("assets/icons/"), p.clone()));
        paths.extend(rest);
        paths
    };
    for path in paths.into_iter().take(6) {
        let key = (
            id.to_owned(),
            path.clone(),
            e.ui.ctx().pixels_per_point().to_bits(),
            matches!(
                theme::current(e.ui.ctx()).scheme,
                crate::theme::Scheme::Dark | crate::theme::Scheme::Blue
            ),
        );
        let cached = e.ui.ctx().data_mut(|d| {
            let cache = d.get_temp_mut_or_default::<PreviewCache>(cache_id);
            if cache.pass != pass {
                cache.images.retain(|_, (_, p)| p.saturating_add(1) >= pass);
                cache.pass = pass;
            }
            cache.images.get_mut(&key).map(|(image, p)| {
                *p = pass;
                image.clone()
            })
        });
        let image = if let Some(image) = cached {
            image
        } else {
            let name = path.strip_prefix("assets/icons/").unwrap_or(&path);
            let image = if id == "paint-rs" {
                Assets::paint_rs_icon_image(e.ui.ctx(), name)
            } else {
                e.cx.asset_library
                    .as_deref()
                    .and_then(|m| {
                        m.packs
                            .iter()
                            .find(|p| Some(p.id.as_str()) == id.strip_prefix("user:"))
                    })
                    .and_then(|p| p.pack.files.get(&path))
                    .and_then(|bytes| pdn_core::asset_pack::decode_image(bytes).ok())
            };
            let Some(image) = image else {
                continue;
            };
            let image = Arc::new(if image.width() > 64 || image.height() > 64 {
                image::imageops::thumbnail(&image, 64, 64)
            } else {
                image
            });
            e.ui.ctx().data_mut(|d| {
                d.get_temp_mut_or_default::<PreviewCache>(cache_id)
                    .images
                    .insert(key, (image.clone(), pass));
            });
            image
        };
        out.push((path, image));
    }
    out
}
