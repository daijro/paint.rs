//! Settings ▸ Plugins ▸ Browse Store…: the ArtCraft Store's catalog (its read-only launcher API,
//! `catalog.json`, fetched fresh each time the dialog opens), filtered to PhotoCraft plug-ins and
//! LightCraft looks and presets, each installable in one click (`platform::plugins`).

use std::sync::{Arc, Mutex};

use egui::{Key, Pos2, Rect, Ui, Vec2};
use serde::Deserialize;

use super::effect_config::tab_strip;
use super::settings::mix;
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::scroll::{Scrollbar, THICKNESS};
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{Footer, dialog_frame_with};
use crate::kit::{img, inputs};
use crate::theme::{self, *};

const TITLE: &str = "Plugin Store";
const ICON: &str = "settings_plugins_24.png";
/// The store's launcher API (README "Launcher API (v1)").
const STORE: &str = "https://akkk09.github.io/artcraft-store/";
const APPS: [(&str, &str); 2] = [("photocraft", "PhotoCraft"), ("lightcraft", "LightCraft")];
const SIZE: Vec2 = Vec2::new(700., 560.);
/// A row's text column starts after the icon; the action column is this wide; rows this far apart.
const TEXT_X: f32 = 60.;
const BUTTON_W: f32 = 104.;
const GAP: f32 = 8.;

/// A catalog entry; unknown fields are ignored (the API's rule).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub release_asset: Option<String>,
    #[serde(default)]
    pub artifact: Option<String>,
}

impl StoreItem {
    /// The download: `downloadUrl` when absolute, else `releaseAsset` or `artifact` under the
    /// store's `downloads/` (the manifest's resolution rule). `None` without an artifact.
    pub fn download(&self) -> Option<String> {
        let relative = |f: &String| format!("{STORE}downloads/{f}");
        match &self.download_url {
            Some(u) if u.starts_with("https://") || u.starts_with("http://") => Some(u.clone()),
            Some(u) => Some(relative(u)),
            None => self
                .release_asset
                .as_ref()
                .or(self.artifact.as_ref())
                .map(relative),
        }
    }

    /// The file name an install stores (the download's last path segment).
    fn file_name(&self) -> Option<String> {
        let url = self.download()?;
        let name = url.split(['?', '#']).next()?.rsplit('/').next()?.to_owned();
        (!name.is_empty()).then_some(name)
    }
}

#[derive(Deserialize)]
struct Catalog {
    plugins: Vec<StoreItem>,
}

/// The catalog's PhotoCraft and LightCraft entries.
pub fn parse_catalog(bytes: &[u8]) -> Result<Vec<StoreItem>, String> {
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let apps = APPS.map(|a| a.0);
    Ok(catalog
        .plugins
        .into_iter()
        .filter(|p| apps.contains(&p.app.as_str()))
        .collect())
}

type Slot = Arc<Mutex<Option<Result<Vec<u8>, String>>>>;

/// GET `url`; the bytes land in the returned slot (and the UI repaints) when it finishes.
fn fetch(ctx: &egui::Context, url: &str) -> Slot {
    let slot: Slot = Arc::default();
    let (done, ctx) = (slot.clone(), ctx.clone());
    ehttp::fetch(ehttp::Request::get(url), move |r| {
        let r = r.and_then(|r| {
            if r.ok {
                Ok(r.bytes)
            } else {
                Err(format!("{} {}", r.status, r.status_text))
            }
        });
        *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        ctx.request_repaint();
    });
    slot
}

fn take(slot: &Slot) -> Option<Result<Vec<u8>, String>> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

pub struct PluginStoreDialog {
    catalog: Option<Result<Vec<StoreItem>, String>>,
    loading: Option<Slot>,
    /// Downloads in flight: item id and its slot.
    downloads: Vec<(String, String, Slot)>,
    /// Ids installed from this dialog (a download's file may name the effect differently).
    installed: Vec<String>,
    errors: Vec<(String, String)>,
    search: String,
    app: usize,
    scroll: f64,
}

impl PluginStoreDialog {
    pub fn open() {
        super::open(Box::new(Self {
            catalog: None,
            loading: None,
            downloads: vec![],
            installed: vec![],
            errors: vec![],
            search: String::new(),
            app: 0,
            scroll: 0.,
        }));
    }

    fn poll(&mut self, cx: &mut DialogCx) {
        if let Some(r) = self.loading.as_ref().and_then(take) {
            self.loading = None;
            self.catalog = Some(r.and_then(|b| parse_catalog(&b)));
        }
        let mut finished = vec![];
        self.downloads.retain(|(id, name, slot)| match take(slot) {
            Some(r) => {
                finished.push((id.clone(), name.clone(), r));
                false
            }
            None => true,
        });
        let Some(library) = cx.asset_library.as_deref_mut() else {
            return;
        };
        for (id, name, r) in finished {
            match r {
                Ok(bytes) => {
                    let before = library.plugins.errors.len();
                    library.plugins.install(&name, &bytes);
                    match library.plugins.errors.get(before) {
                        Some(e) => self.errors.push((id, e.clone())),
                        None => self.installed.push(id),
                    }
                }
                Err(e) => self.errors.push((id, format!("Download failed: {e}"))),
            }
        }
    }

    fn body(&mut self, ui: &mut Ui, r: Rect, cx: &mut DialogCx) {
        let t = theme::current(ui.ctx());
        let soft = mix(t.menu_item_text, t.window_background, 0.7);
        let inner = r.shrink(12.);
        // The apps as tabs, the search box at the strip's right end.
        let captions = ["All", APPS[0].1, APPS[1].1];
        let (x0, y0) = (inner.min.x, inner.min.y);
        if let Some(i) = tab_strip(ui, inner, x0, y0, "store-apps", &captions, self.app, None) {
            (self.app, self.scroll) = (i, 0.);
        }
        let search = Rect::from_min_size(
            Pos2::new(inner.max.x - 220., y0 + (EFFECT_TAB_HEIGHT - 24.) / 2. - 1.),
            Vec2::new(220., 22.),
        );
        let colors = (t.number_input_background, t.pdn_control_border);
        let size = DIALOG_FONT_SIZE;
        inputs::text_input(
            ui,
            "store-search",
            "Search",
            search,
            &mut self.search,
            colors,
            6.,
            size,
            false,
            true,
        );
        if self.search.is_empty() {
            let at = Pos2::new(search.min.x + 7., search.center().y - 7.5);
            text::draw(
                ui.painter(),
                at,
                "Search the store",
                FontSpec::new(size),
                soft,
            );
        }
        let mut y = y0 + EFFECT_TAB_HEIGHT + 10.;
        let items: Vec<StoreItem> = match &self.catalog {
            Some(Ok(items)) => {
                let q = self.search.to_lowercase();
                items
                    .iter()
                    .filter(|p| self.app == 0 || p.app == APPS[self.app - 1].0)
                    .filter(|p| {
                        q.is_empty()
                            || p.name.to_lowercase().contains(&q)
                            || p.description.to_lowercase().contains(&q)
                    })
                    .cloned()
                    .collect()
            }
            _ => vec![],
        };
        let status = match &self.catalog {
            None => "Loading the ArtCraft Store…".to_owned(),
            Some(Err(e)) => format!("The store could not be reached: {e}"),
            Some(Ok(_)) if items.is_empty() && self.app == 2 && self.search.is_empty() => {
                "The store lists no LightCraft items right now.".to_owned()
            }
            Some(Ok(_)) if items.is_empty() => "Nothing matches.".to_owned(),
            Some(Ok(_)) => format!("{} available from the ArtCraft Store", items.len()),
        };
        text::draw(
            ui.painter(),
            Pos2::new(x0, y),
            &status,
            FontSpec::new(11.),
            soft,
        );
        y += 22.;
        let list = Rect::from_min_max(Pos2::new(x0, y), inner.max);
        // Each row as tall as its wrapped description (at most three lines).
        let text_w = list.width() - TEXT_X - BUTTON_W - 36. - THICKNESS;
        let rows: Vec<(StoreItem, Vec<String>)> = items
            .into_iter()
            .map(|p| {
                let lines = wrap(&p.description, text_w, 11.5, 3);
                (p, lines)
            })
            .collect();
        let height = |lines: usize| 52. + lines as f32 * 16. + 10.;
        let content: f32 = rows.iter().map(|(_, l)| height(l.len()) + GAP).sum();
        let max = (content as f64 - list.height() as f64).max(0.);
        if ui.rect_contains_pointer(list) {
            self.scroll =
                (self.scroll - ui.input(|i| i.smooth_scroll_delta.y) as f64).clamp(0., max);
        }
        let library = cx.asset_library.as_deref();
        let clip = ui.clip_rect();
        ui.set_clip_rect(clip.intersect(list));
        let mut install = None;
        let mut top = list.min.y - self.scroll as f32;
        for (item, lines) in &rows {
            let h = height(lines.len());
            let row = Rect::from_min_size(
                Pos2::new(list.min.x, top),
                Vec2::new(list.width() - if max > 0. { THICKNESS + 4. } else { 0. }, h),
            );
            top += h + GAP;
            if !row.intersects(list) {
                continue;
            }
            let hovered = ui.rect_contains_pointer(row.intersect(list));
            let (border, fill) = if hovered {
                (t.menu_item_border, t.menu_item_hover)
            } else {
                (t.window_border, t.number_input_background)
            };
            ui.painter().rect_filled(row, 0., border);
            ui.painter().rect_filled(row.shrink(1.), 0., fill);
            let icon = if item.app == "lightcraft" {
                "color_lookup_adjustment.png"
            } else {
                "settings_plugins_24.png"
            };
            let icon_rect = Rect::from_min_size(row.min + Vec2::new(14., 14.), Vec2::splat(32.));
            img::paint_icon(ui.painter(), icon, icon_rect, fill, img::Filter::None);
            let app = APPS.iter().find(|a| a.0 == item.app).map_or("", |a| a.1);
            let meta = [item.version.as_str(), app]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("  ·  ");
            let meta = if item.author.is_empty() {
                meta
            } else {
                format!("{meta}  ·  by {}", item.author)
            };
            let at = |y: f32, w: f32| {
                Rect::from_min_size(row.min + Vec2::new(TEXT_X, y), Vec2::new(w, 17.))
            };
            let name = FontSpec::new(DIALOG_FONT_SIZE + 1.).bold();
            text::draw_ellipsized(
                ui.painter(),
                at(11., text_w),
                &item.name,
                name,
                t.menu_item_text,
            );
            text::draw_ellipsized(
                ui.painter(),
                at(31., text_w),
                &meta,
                FontSpec::new(11.),
                soft,
            );
            for (k, line) in lines.iter().enumerate() {
                let p = row.min + Vec2::new(TEXT_X, 54. + k as f32 * 16.);
                text::draw(ui.painter(), p, line, FontSpec::new(11.5), t.menu_item_text);
            }
            // The action column.
            let installed = self.installed.contains(&item.id)
                || library.is_some_and(|l| l.plugins.installed(&item.id));
            let downloading = self.downloads.iter().any(|d| d.0 == item.id);
            let button = Rect::from_min_size(
                Pos2::new(row.max.x - BUTTON_W - 14., row.min.y + 14.),
                Vec2::new(BUTTON_W, 28.),
            );
            let id = ("store-install", &item.id);
            let clicked = if downloading {
                inputs::push_button(ui, id, button, "Installing…", false, false);
                false
            } else if item.download().is_none() {
                inputs::push_button(ui, id, button, "Unavailable", false, false);
                false
            } else if installed {
                let response = inputs::push_button(ui, id, button, "Reinstall", true, false);
                inputs::button_activated(ui, &response)
            } else {
                accent_button(ui, id, button, &format!("Install {}", item.name), "Install")
            };
            if clicked {
                install = Some(item.clone());
            }
            let note = Pos2::new(button.min.x, button.max.y + 8.);
            if let Some((_, e)) = self.errors.iter().rev().find(|e| e.0 == item.id) {
                for (k, line) in wrap(e, BUTTON_W + 6., 11., 2).iter().enumerate() {
                    let p = note + Vec2::new(0., k as f32 * 14.);
                    text::draw(ui.painter(), p, line, FontSpec::new(11.), t.error_text);
                }
            } else if installed {
                // Installed, and where to find it.
                for (k, line) in ["Installed in", "Effects > Plugins"].iter().enumerate() {
                    let p = Pos2::new(
                        button.center().x - text::css_width(line, 11.) / 2.,
                        note.y + k as f32 * 14.,
                    );
                    let spec = FontSpec::new(11.).bold();
                    text::draw(ui.painter(), p, line, spec, t.menu_item_selected_border);
                }
            }
        }
        ui.set_clip_rect(clip);
        if max > 0. {
            Scrollbar::vertical(Rect::from_min_size(
                Pos2::new(list.max.x - THICKNESS, list.min.y),
                Vec2::new(THICKNESS, list.height()),
            ))
            .id_salt("plugin-store")
            .show(ui, content as f64, list.height() as f64, &mut self.scroll);
        }
        if let Some(item) = install
            && let (Some(url), Some(name)) = (item.download(), item.file_name())
        {
            self.errors.retain(|e| e.0 != item.id);
            self.downloads
                .push((item.id.clone(), name, fetch(ui.ctx(), &url)));
        }
    }
}

/// `text` broken into lines at most `width` wide at `size`, at most `max` of them (the last one
/// ends in "…" when words are left over).
fn wrap(text: &str, width: f32, size: f32, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    let mut line = String::new();
    let mut words = text.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let next = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if text::css_width(&next, size) <= width || line.is_empty() {
            line = next;
            continue;
        }
        if lines.len() + 1 == max {
            // Out of lines: what fits, then the ellipsis.
            while !line.is_empty() && text::css_width(&format!("{line}…"), size) > width {
                line = line
                    .rsplit_once(' ')
                    .map_or(String::new(), |(a, _)| a.to_owned());
            }
            lines.push(format!("{line}…"));
            return lines;
        }
        lines.push(std::mem::replace(&mut line, word.to_owned()));
        let _ = words.peek();
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The store's primary action: the accent blue, filled, with white text (a press darkens it).
fn accent_button(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    name: &str,
    label: &str,
) -> bool {
    let t = theme::current(ui.ctx());
    let response = ui.interact(rect, ui.scope_id().with(id), egui::Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        t.checkbox_checked
    } else if response.hovered() || response.has_focus() {
        t.menu_item_selected_border
            .lerp_to_gamma(egui::Color32::WHITE, 0.15)
    } else {
        t.menu_item_selected_border
    };
    ui.painter().rect_filled(rect, 2., fill);
    let w = text::css_width(label, DIALOG_FONT_SIZE);
    let at = Pos2::new(rect.center().x - w / 2., rect.center().y - 8.);
    text::draw(
        ui.painter(),
        at,
        label,
        FontSpec::new(DIALOG_FONT_SIZE).bold(),
        egui::Color32::WHITE,
    );
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label(name);
    });
    inputs::button_activated(ui, &response)
}

impl Dialog for PluginStoreDialog {
    fn title(&self) -> String {
        TITLE.into()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        if self.catalog.is_none() && self.loading.is_none() {
            self.loading = Some(fetch(&ctx, &format!("{STORE}catalog.json")));
        }
        self.poll(cx);
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            return DialogState::Closed(DialogResult::Cancel);
        }
        let size = Vec2::new(
            SIZE.x
                .min(ctx.content_rect().width() - DIALOG_VIEWPORT_MARGIN),
            SIZE.y
                .min(ctx.content_rect().height() - DIALOG_VIEWPORT_MARGIN - 80.),
        );
        let mut close = false;
        let mut body = |ui: &mut Ui, r: Rect| self.body(ui, r, cx);
        let mut footer = |ui: &mut Ui, f: Rect| {
            let (bw, bh) = LAYER_PROPS_BUTTON;
            let rect = Rect::from_min_size(
                Pos2::new(f.max.x - bw - 12., f.center().y - bh / 2.),
                Vec2::new(bw, bh),
            );
            let response = inputs::push_button(ui, "store-close", rect, "Close", true, true);
            close = inputs::button_activated(ui, &response);
            let t = theme::current(ui.ctx());
            let note = "Plug-ins run sandboxed: they cannot reach your files or the network.";
            let at = Pos2::new(f.min.x + 12., f.center().y - 7.);
            let soft = mix(t.menu_item_text, t.window_background, 0.7);
            text::draw(ui.painter(), at, note, FontSpec::new(11.), soft);
        };
        let frame = dialog_frame_with(
            ui,
            TITLE,
            Some(ICON),
            false,
            (size, &mut body),
            Some(&mut footer),
            Footer {
                height: LAYER_PROPS_FOOTER.0,
                border: true,
            },
        );
        if close || frame.closed {
            DialogState::Closed(DialogResult::Cancel)
        } else {
            DialogState::Open
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog keeps PhotoCraft and LightCraft entries and resolves their downloads.
    #[test]
    fn the_catalog_resolves_downloads() {
        let json = br#"{"apps":[],"plugins":[
            {"id":"a","app":"photocraft","name":"A","artifact":"a.wasm","extra":1},
            {"id":"b","app":"lightcraft","name":"B","downloadUrl":"https://example.com/b.zip?x=1"},
            {"id":"c","app":"soundcraft","name":"C","artifact":"c.clap"},
            {"id":"d","app":"lightcraft","name":"D"}]}"#;
        let items = parse_catalog(json).unwrap();
        let ids: Vec<_> = items.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "d"]);
        assert_eq!(
            items[0].download().as_deref(),
            Some("https://akkk09.github.io/artcraft-store/downloads/a.wasm")
        );
        assert_eq!(items[1].file_name().as_deref(), Some("b.zip"));
        assert_eq!(items[2].download(), None);
        assert!(parse_catalog(b"not json").is_err());
    }
}
