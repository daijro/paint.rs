//! File ▸ Batch Export (an Extra of paint.rs's own): the images in a thumbnail strip drawn like
//! the document strip's, each with its own file name and Save Configuration (`save_image`'s
//! settings and preview, one per image), all written at once to one folder; the web build
//! downloads each file.
use super::{
    Dialog, DialogCx, DialogResult, DialogState,
    effect_config::button_clicked,
    flows,
    save_image::{self, FORMATS, SaveImageDialog, find_format},
    task::{TaskChoice, TaskDialog, TaskDialogHandle, TaskDialogSpec},
};
use crate::{
    kit::{
        img, inputs, px,
        scroll::{ScrollList, Scrollbar, THICKNESS},
        text::{self, FontSpec},
        thumbnails::{Scaler, ThumbnailMode},
        tooltip,
        window::{DialogFrameState, Footer, dialog_button, dialog_button_width, dialog_frame_with},
    },
    platform::{FileSaveOutcome, FileTarget, Files, Pending},
    shell::{canvas_view::DisplayPrefs, document_strip},
    theme::{self, *},
};
use egui::{Color32, ColorImage, Key, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use pdn_core::{
    app::AppWorkspace,
    render::thumbnail::{ThumbFrame, render_framed, ring_extent},
};

const ICON: &str = "menu_file_save_all_icon.png";
/// The strip's item height: the document strip's item at the mockup's size.
const ITEM: f32 = 72.0;

/// Image `number` of `count`'s file name from `pattern`: `##` becomes the number, zero-padded to
/// two digits (as the pattern reads) or `count`'s if more, and the extension follows `format`.
pub fn export_name(pattern: &str, number: usize, count: usize, format: &str) -> String {
    let digits = count.to_string().len().max(2);
    with_extension(
        &pattern.replace("##", &format!("{number:0digits$}")),
        format,
    )
}

/// `name` with `format`'s extension in place of an image extension it ends with (any file
/// type's), else after it.
fn with_extension(name: &str, format: &str) -> String {
    let image = |e: &str| {
        let known = FORMATS.iter().flat_map(|f| f.extensions);
        known.into_iter().any(|x| x[1..].eq_ignore_ascii_case(e))
    };
    let stem = name.rsplit_once('.').filter(|(_, e)| image(e));
    format!(
        "{}{}",
        stem.map_or(name, |(s, _)| s),
        find_format(format).extensions[0]
    )
}

struct Item {
    /// The file name, `##` for the image's number; its extension follows the file type.
    name: String,
    label: String,
    dirty: bool,
    /// The scaled composite (straight RGBA8, size) and its framed texture over a backdrop.
    scaled: Option<(Vec<u8>, [u32; 2])>,
    framed: Option<(Color32, TextureHandle)>,
    save: SaveImageDialog,
}

/// An export in progress: the next image, the write in flight (its file name), the images
/// skipped because their file exists, and the failures.
struct Export {
    next: usize,
    writing: Option<(String, Pending<Result<FileSaveOutcome, String>>)>,
    skip: Vec<bool>,
    failures: Vec<String>,
}

pub struct BatchExportDialog {
    items: Vec<Item>,
    selected: usize,
    list: ScrollList,
    scaler: Scaler,
    /// The output folder (desktop).
    folder: String,
    status: String,
    /// Replace or skip existing files: the question, and which files exist.
    ask: Option<(TaskDialogHandle, Vec<bool>)>,
    export: Option<Export>,
}

impl BatchExportDialog {
    /// Images `ids` (workspace ids) in this order, each starting on its own file type (PNG when
    /// it has none that saves) and name; thumbnails are scaled `mode`'s way.
    pub fn new(app: &AppWorkspace, ids: &[u64], mode: ThumbnailMode) -> Self {
        let docs: Vec<_> = ids
            .iter()
            .filter_map(|id| app.documents.iter().find(|ws| ws.id == *id))
            .collect();
        let items = docs
            .iter()
            .map(|ws| {
                let own = ws.file_format.as_deref().and_then(|f| {
                    FORMATS
                        .iter()
                        .find(|i| i.id == f && !matches!(f, "heic" | "jxr"))
                });
                let format = own.map_or("png", |f| f.id);
                let initial = own.map_or(serde_json::Value::Null, |_| ws.save_options.clone());
                Item {
                    name: with_extension(ws.file_name.as_deref().unwrap_or("untitled_##"), format),
                    label: ws.get_friendly_name(),
                    dirty: ws.dirty,
                    scaled: None,
                    framed: None,
                    save: SaveImageDialog::new(ws.id, format, initial, |_, _| {}),
                }
            })
            .collect();
        let folder = docs
            .iter()
            .filter_map(|ws| ws.file_path.as_deref())
            .find_map(|p| std::path::Path::new(p).parent())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut scaler = Scaler::default();
        scaler.set_mode(mode);
        Self {
            items,
            selected: 0,
            list: ScrollList::horizontal(0.75),
            scaler,
            folder,
            status: String::new(),
            ask: None,
            export: None,
        }
    }

    /// Shows image `i`; the one left drops its preview (rendered again when it shows).
    fn select(&mut self, i: usize) {
        if i == self.selected {
            return;
        }
        let old = &mut self.items[self.selected].save;
        old.preview = None;
        old.open_select = None;
        old.due = 0.0;
        self.selected = i;
    }

    /// Gives every image the shown image's file type and its options; its file name too when it
    /// numbers the images (`##`), else each name keeps its own and takes the new extension.
    fn apply_to_all(&mut self) {
        let from = &self.items[self.selected];
        let format = from.save.format;
        let options = from.save.options[format].clone();
        let name = from.name.contains("##").then(|| from.name.clone());
        for item in &mut self.items {
            item.save.format = format;
            item.save.options[format] = options.clone();
            item.name = name
                .clone()
                .unwrap_or_else(|| with_extension(&item.name, FORMATS[format].id));
        }
    }

    fn names(&self) -> Vec<String> {
        let n = self.items.len();
        let names = self.items.iter().enumerate();
        names
            .map(|(i, item)| export_name(&item.name, i + 1, n, FORMATS[item.save.format].id))
            .collect()
    }

    /// One thumbnail job at a time, at `max_edge` device px.
    fn thumbnails(&mut self, ctx: &egui::Context, app: &AppWorkspace, max_edge: u32) {
        let doc = |id: u64| app.documents.iter().find(|ws| ws.id == id);
        let source = |id: u64, (revision, _): (u64, u32)| {
            doc(id)
                .filter(|ws| ws.view.composition_revision == revision)
                .map(|ws| ws.view.composition.data())
        };
        if let Some((id, _, (px, w, h))) = self.scaler.poll(ctx, source)
            && let Some(item) = self.items.iter_mut().find(|i| i.save.document == id)
        {
            item.scaled = Some((px, [w, h]));
        }
        for item in self.items.iter_mut().filter(|i| i.scaled.is_none()) {
            let Some(ws) = doc(item.save.document).filter(|_| !self.scaler.busy()) else {
                break;
            };
            let c = &ws.view.composition;
            let key = (ws.view.composition_revision, max_edge);
            match self
                .scaler
                .start(ctx, ws.id, key, c.data(), [c.width(), c.height()])
            {
                // Synchronous: the next one this frame too.
                Some((_, _, (px, w, h))) => item.scaled = Some((px, [w, h])),
                None => break,
            }
        }
    }

    /// OK: the names must differ and, on the desktop, the folder exist; existing files ask
    /// once whether to replace or skip them.
    fn start(&mut self) {
        let names = self.names();
        let mut seen = std::collections::HashSet::new();
        for (i, name) in names.iter().enumerate() {
            let problem = if name.contains(['/', '\\']) {
                format!("\u{201C}{name}\u{201D}: a file name cannot contain / or \\.")
            } else if !seen.insert(name.to_lowercase()) {
                format!("Two images would be saved as \u{201C}{name}\u{201D}.")
            } else {
                continue;
            };
            self.status = problem;
            self.select(i);
            return;
        }
        let mut skip = vec![false; names.len()];
        if cfg!(not(target_arch = "wasm32")) {
            let folder = std::path::Path::new(&self.folder);
            if !folder.is_dir() {
                self.status = "Choose an existing folder.".into();
                return;
            }
            skip = names.iter().map(|n| folder.join(n).exists()).collect();
            let existing = skip.iter().filter(|s| **s).count();
            if existing > 0 {
                let message = if existing == 1 {
                    "One of the files already exists in this folder.".into()
                } else {
                    format!("{existing} of the files already exist in this folder.")
                };
                let choice = |value, title: &str, description: &str, icon| TaskChoice {
                    value,
                    title: title.into(),
                    description: Some(description.into()),
                    icon: Some(icon),
                };
                let handle = TaskDialog::show(TaskDialogSpec {
                    title: "Batch Export".into(),
                    icon: Some("warning_icon.png"),
                    message: Some(message),
                    choices: vec![
                        choice(
                            "replace",
                            "Replace",
                            "Write over the existing files.",
                            "menu_file_save_icon.png",
                        ),
                        choice(
                            "skip",
                            "Skip",
                            "Keep the existing files, and export the other images.",
                            "menu_file_close_icon.png",
                        ),
                        choice(
                            "cancel",
                            "Cancel",
                            "Go back to the settings.",
                            "cancel_icon.png",
                        ),
                    ],
                    cancel_value: "cancel",
                    ..Default::default()
                });
                self.ask = Some((handle, skip));
                return;
            }
        }
        self.status.clear();
        self.begin(skip);
    }

    fn begin(&mut self, skip: Vec<bool>) {
        self.export = Some(Export {
            next: 0,
            writing: None,
            skip,
            failures: vec![],
        });
    }

    /// The export's next step: the answer about existing files, the write in flight, else the
    /// next image's encoding and write. Closed when every image is done; failures are reported.
    fn advance(&mut self, ctx: &egui::Context, app: &AppWorkspace) -> Option<DialogState> {
        if let Some(choice) = self.ask.as_ref().and_then(|(h, _)| h.poll()) {
            let (_, exists) = self.ask.take()?;
            match choice {
                "replace" => self.begin(vec![false; exists.len()]),
                "skip" => self.begin(exists),
                _ => {}
            }
        }
        self.export.as_ref()?;
        let names = self.names();
        let export = self.export.as_mut()?;
        if let Some((name, write)) = &mut export.writing {
            let Some(result) = write.poll() else {
                ctx.request_repaint_after(std::time::Duration::from_millis(10));
                return None;
            };
            match result {
                Ok(FileSaveOutcome::Written(_) | FileSaveOutcome::DownloadStarted) => {}
                Ok(FileSaveOutcome::Cancelled) => export.failures.push(format!("{name}: Canceled")),
                Err(e) => export.failures.push(format!("{name}: {e}")),
            }
            export.writing = None;
        }
        while export.skip.get(export.next) == Some(&true) {
            export.next += 1;
        }
        let Some(item) = self.items.get(export.next) else {
            if !export.failures.is_empty() {
                let failed = export.failures.len();
                TaskDialog::show(TaskDialogSpec {
                    title: "Batch Export".into(),
                    icon: Some(ICON),
                    message: Some(format!(
                        "{failed} of {} images could not be exported.\n\n{}",
                        names.len(),
                        export.failures.join("\n")
                    )),
                    choices: vec![TaskChoice {
                        value: "ok",
                        title: "OK".into(),
                        ..Default::default()
                    }],
                    cancel_value: "ok",
                    ..Default::default()
                });
            }
            return Some(DialogState::Closed(DialogResult::Ok));
        };
        let name = names[export.next].clone();
        export.next += 1;
        let format = item.save.format;
        let options = &item.save.options[format];
        match flows::encode_document(app, item.save.document, FORMATS[format].id, options) {
            Ok(bytes) => {
                let target = FileTarget {
                    name: name.clone(),
                    path: cfg!(not(target_arch = "wasm32"))
                        .then(|| std::path::Path::new(&self.folder).join(&name)),
                    #[cfg(target_arch = "wasm32")]
                    handle: None,
                };
                let write = Files::current().save(&name, bytes, Some(target));
                export.writing = Some((name, write));
            }
            Err(e) => export.failures.push(format!("{name}: {e}")),
        }
        ctx.request_repaint();
        None
    }

    /// The strip of images in `strip`, drawn like the document strip's items. Returns the one
    /// clicked.
    fn strip_ui(&mut self, ui: &mut Ui, strip: Rect, prefs: DisplayPrefs) -> Option<usize> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let item_w = ITEM + 2.0 * DOCUMENT_ITEM_BORDER;
        let content = self.items.len() as f32 * item_w;
        let now = ui.input(|i| i.time);
        self.list.wheel(ui, strip, content, now, "batch-export");
        self.list.clamp(content, strip.width());
        let painter = ui.painter().with_clip_rect(strip.intersect(ui.clip_rect()));
        let bw = px::border_width(DOCUMENT_ITEM_BORDER, ppp);
        let mut clicked = None;
        for (i, item) in self.items.iter_mut().enumerate() {
            let x = strip.min.x + i as f32 * item_w - self.list.scroll;
            let rect = Rect::from_min_size(Pos2::new(x, strip.min.y), Vec2::new(item_w, ITEM));
            if !rect.intersects(strip) {
                continue;
            }
            let snapped = px::snap_rect(rect, ppp);
            let id = ui.scope_id().with(("batch-export-item", i));
            let response = ui.interact(snapped.intersect(strip), id, Sense::click());
            let selected = i == self.selected;
            ui.ctx().accesskit_node_builder(id, |node| {
                node.set_role(egui::accesskit::Role::Tab);
                node.set_label(item.label.clone());
                node.set_selected(selected);
            });
            let (border, fill) = if selected {
                (theme.menu_item_selected_border, theme.menu_item_selected)
            } else if response.hovered() {
                (theme.menu_item_border, theme.menu_item_hover)
            } else {
                (Color32::TRANSPARENT, Color32::TRANSPARENT)
            };
            let fill = px::over(theme.window_background, fill);
            if border != Color32::TRANSPARENT {
                painter.rect_filled(snapped, 0.0, border);
                painter.rect_filled(snapped.shrink(bw), 0.0, fill);
            }
            let pad = rect.shrink(DOCUMENT_ITEM_BORDER);
            if let Some((rgba, [w, h])) = item.scaled.as_ref().filter(|s| s.1[0] > 0) {
                if item.framed.as_ref().is_none_or(|(c, _)| *c != fill) {
                    let frame = ThumbFrame {
                        u: ppp as f64,
                        checker_brightness: prefs.checker_brightness,
                        dark: prefs.dark,
                        mode: prefs.mode,
                        checker: true,
                        backdrop: [fill.r(), fill.g(), fill.b(), 255],
                    };
                    let (framed, fw, fh) = render_framed(rgba, *w, *h, &frame);
                    let image =
                        ColorImage::from_rgba_unmultiplied([fw as usize, fh as usize], &framed);
                    let name = format!("batch-export/{i}");
                    let texture = ui.ctx().load_texture(name, image, TextureOptions::NEAREST);
                    item.framed = Some((fill, texture));
                }
                // At the bottom centre on whole device pixels, as the document strip's.
                let e = ring_extent(ppp as f64) as f32;
                let left = (pad.center().x * ppp - *w as f32 / 2.0).round();
                let bottom = ((pad.max.y - DOCUMENT_THUMB_BOTTOM) * ppp).round();
                let at = Pos2::new((left - e) / ppp, (bottom - *h as f32 - e) / ppp);
                let size = Vec2::new(*w as f32 + 2.0 * e, *h as f32 + 2.0 * e) / ppp;
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                if let Some((_, texture)) = &item.framed {
                    painter.image(
                        texture.id(),
                        Rect::from_min_size(at, size),
                        uv,
                        Color32::WHITE,
                    );
                }
            }
            if item.dirty {
                let (header_h, header_pad) = DOCUMENT_HEADER;
                let star = Rect::from_min_size(
                    pad.min
                        + Vec2::new(
                            header_pad,
                            header_pad + (header_h - DOCUMENT_DIRTY_SIZE) / 2.0,
                        ),
                    Vec2::splat(DOCUMENT_DIRTY_SIZE),
                );
                let star = px::snap_rect(star, ppp);
                let dirty = document_strip::dirty_star();
                img::paint_generated(&painter, "batch-export-dirty-star", dirty, star, fill);
            }
            if response.clicked() {
                clicked = Some(i);
            }
        }
        self.list.cover(ui, strip, "batch-export");
        if content > strip.width() {
            let bar = Rect::from_min_size(
                Pos2::new(strip.min.x, strip.max.y + 4.0),
                Vec2::new(strip.width(), THICKNESS),
            );
            let mut scroll = self.list.scroll as f64;
            Scrollbar::horizontal(bar)
                .id_salt("batch-export")
                .colors(theme.pdn_scrollbar_track, theme.pdn_scrollbar_thumb)
                .show(ui, content as f64, strip.width() as f64, &mut scroll);
            self.list.scroll = scroll as f32;
        }
        clicked
    }
}

impl Dialog for BatchExportDialog {
    fn title(&self) -> String {
        "Batch Export".into()
    }
    fn persistence_id(&self) -> Option<&'static str> {
        Some("batchExport")
    }
    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        for item in &mut self.items {
            item.save.load(cx.settings);
        }
        if let Some(state) = self.advance(ui.ctx(), cx.app) {
            return state;
        }
        let busy = self.export.is_some() || self.ask.is_some();
        let list_open = self.items[self.selected].save.open_select.is_some();
        let plain = ui.input(|i| !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.mac_cmd);
        if plain && ui.input(|i| i.key_pressed(Key::Escape)) {
            if list_open {
                self.items[self.selected].save.open_select = None;
            } else {
                return DialogState::Closed(DialogResult::Cancel);
            }
        }
        let ppp = ui.ctx().pixels_per_point();
        let item_rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(ITEM));
        self.thumbnails(
            ui.ctx(),
            cx.app,
            document_strip::thumb_max_edge(item_rect, ppp),
        );
        self.items[self.selected].save.tick(ui, cx);
        let screen = ui.ctx().content_rect();
        let width = 1500.0_f32.min(screen.width() - 20.0);
        let total_h = 900.0_f32.min(screen.height() - 20.0);
        let body_h = total_h - DIALOG_TITLE_BAR - 48.0 - 2.0 * DIALOG_BORDER;
        let prefs = DisplayPrefs::new(cx.settings, &theme::current(ui.ctx()));
        let (mut pick, mut apply, mut submit, mut cancel) = (None, false, false, false);
        let status = match &self.export {
            Some(e) => format!("Exporting {} of {}...", e.next, self.items.len()),
            None if self.status.is_empty() => self.items[self.selected].save.status.clone(),
            None => self.status.clone(),
        };
        let progress = self
            .export
            .as_ref()
            .map(|e| e.next as f32 / self.items.len() as f32);
        let mut body = |ui: &mut Ui, r: Rect| {
            if busy {
                ui.disable();
            }
            let theme = theme::current(ui.ctx());
            let font = FontSpec::new(DIALOG_FONT_SIZE);
            let strip =
                Rect::from_min_size(r.min + Vec2::splat(10.0), Vec2::new(r.width() - 20.0, ITEM));
            pick = self.strip_ui(ui, strip, prefs);
            let rest = Rect::from_min_max(Pos2::new(r.min.x, strip.max.y + 4.0 + THICKNESS), r.max);
            let side = save_image::side(ui);
            let at = rest.min + Vec2::splat(10.0);
            let field_look = if theme.scheme == Scheme::Light {
                SELECT_NATIVE_LIGHT
            } else {
                SELECT_NATIVE
            };
            // File name, under the Settings heading the shared column draws.
            let line = font.resolved_line_height(ui.ctx().pixels_per_point());
            let label_y = at.y + 29.0;
            let label = Pos2::new(at.x, label_y);
            text::draw(
                ui.painter(),
                label,
                "File name",
                font.weight(600),
                theme.menu_item_text,
            );
            let field =
                Rect::from_min_size(Pos2::new(at.x, label_y + line + 9.0), Vec2::new(side, 31.0));
            let selected = self.selected;
            let item = &mut self.items[selected];
            inputs::text_input(
                ui,
                ("batch-export-name", selected),
                "File name",
                field,
                &mut item.name,
                field_look,
                SELECT_TEXT_INSET,
                DIALOG_FONT_SIZE,
                false,
                !busy,
            );
            let format = item.save.format;
            let bottom = item.save.body(ui, rest, field.max.y + 10.0 - label_y);
            if item.save.format != format {
                item.name = with_extension(&item.name, FORMATS[item.save.format].id);
            }
            let label = "Apply to All";
            let w = dialog_button_width(ui.painter(), label, 105.0);
            let rect = Rect::from_min_size(
                Pos2::new(at.x + (side - w) / 2.0, bottom),
                Vec2::new(w, 21.0),
            );
            let response = dialog_button(ui, "apply-to-all", rect, label, false);
            tooltip::control(&response, || {
                "Gives every image this file type and its options, and this file name when it numbers them with ##.".into()
            });
            inputs::take_enter(ui, &response);
            apply = button_clicked(ui, &response);
            // The output folder at the column's foot (the web downloads each file).
            #[cfg(not(target_arch = "wasm32"))]
            {
                let top = rest.max.y - 10.0 - 59.0;
                save_image::heading(ui, Pos2::new(at.x, top), side, "Folder", false);
                let field =
                    Rect::from_min_size(Pos2::new(at.x, top + 28.0), Vec2::new(side - 88.0, 31.0));
                inputs::text_input(
                    ui,
                    "batch-export-folder",
                    "Folder",
                    field,
                    &mut self.folder,
                    field_look,
                    SELECT_TEXT_INSET,
                    DIALOG_FONT_SIZE,
                    false,
                    !busy,
                );
                let rect = Rect::from_min_max(
                    Pos2::new(field.max.x + 8.0, field.min.y),
                    Pos2::new(at.x + side, field.max.y),
                );
                let response = dialog_button(ui, "browse", rect, "Browse...", false);
                inputs::take_enter(ui, &response);
                if button_clicked(ui, &response)
                    && let Some(folder) = crate::platform::native::choose_folder(&self.folder)
                {
                    self.folder = folder.to_string_lossy().into_owned();
                }
            }
        };
        let mut footer = |ui: &mut Ui, r: Rect| {
            let theme = theme::current(ui.ctx());
            let mut left = r.min.x + 10.0;
            if let Some(done) = progress {
                let bar =
                    Rect::from_min_size(Pos2::new(left, r.center().y - 4.0), Vec2::new(200.0, 8.0));
                let bar = px::snap_rect(bar, ui.ctx().pixels_per_point());
                ui.painter()
                    .rect_filled(bar, 0.0, theme.pdn_scrollbar_track);
                let fill =
                    Rect::from_min_size(bar.min, Vec2::new(bar.width() * done, bar.height()));
                ui.painter()
                    .rect_filled(fill, 0.0, theme.menu_item_selected_border);
                left = bar.max.x + 10.0;
            }
            text::draw_ellipsized(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(left, r.min.y + 15.0),
                    Pos2::new(r.max.x - 240.0, r.max.y),
                ),
                &status,
                FontSpec::new(DIALOG_FONT_SIZE),
                theme.muted_text,
            );
            let cancel_rect = Rect::from_min_size(
                Pos2::new(r.max.x - 114.0, r.min.y + (r.height() - 26.0) / 2.0),
                Vec2::new(104.0, 26.0),
            );
            let ok_rect = cancel_rect.translate(Vec2::new(-114.0, 0.0));
            let ok = ui
                .add_enabled_ui(!busy, |ui| dialog_button(ui, "ok", ok_rect, "OK", false))
                .inner;
            submit = button_clicked(ui, &ok) && !busy;
            let c = dialog_button(ui, "cancel", cancel_rect, "Cancel", false);
            cancel = button_clicked(ui, &c);
        };
        let frame = dialog_frame_with(
            ui,
            "Batch Export",
            Some(ICON),
            false,
            (Vec2::new(width, body_h), &mut body),
            Some(&mut footer),
            Footer {
                height: 48.0,
                border: true,
            },
        );
        if cx.seat_focus {
            super::seat_no_focus(ui.ctx());
        }
        let open_select = self.items[self.selected].save.open_select.is_some();
        let mut state = DialogFrameState::get(ui.ctx(), ui.scope_id());
        state.popup = if open_select { screen } else { Rect::NOTHING };
        state.set(ui.ctx(), ui.scope_id());
        let close = frame.closed && (plain || !ui.input(|i| i.key_pressed(Key::Enter)));
        if cancel || close {
            return DialogState::Closed(DialogResult::Cancel);
        }
        if let Some(i) = pick {
            self.status.clear();
            self.select(i);
        }
        if apply {
            self.apply_to_all();
        }
        let enter = plain
            && ui.input(|i| i.key_pressed(Key::Enter))
            && !inputs::enter_taken(ui.ctx())
            && !list_open
            && !open_select;
        if (submit || enter) && !busy {
            self.start();
        }
        DialogState::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_number_the_images_and_follow_the_file_type() {
        assert_eq!(
            export_name("untitled_##.png", 1, 12, "png"),
            "untitled_01.png"
        );
        assert_eq!(
            export_name("untitled_##.png", 12, 12, "jpeg"),
            "untitled_12.jpg"
        );
        assert_eq!(
            export_name("untitled_##.png", 3, 9, "png"),
            "untitled_03.png"
        );
        assert_eq!(export_name("frame_##", 7, 120, "webp"), "frame_007.webp");
        assert_eq!(export_name("photo.JPG", 2, 2, "png"), "photo.png");
        assert_eq!(
            export_name("scan.final.tiff", 1, 1, "avif"),
            "scan.final.avif"
        );
        assert_eq!(export_name("notes.v2", 1, 1, "png"), "notes.v2.png");
        assert_eq!(with_extension("photo.jpeg", "jpeg"), "photo.jpg");
    }
}
