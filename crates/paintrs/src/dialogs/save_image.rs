//! paintdotjs SaveImageDialog.js and window.css `.save-config-*` at b4e6fc5.
//! Paint.NET format order/defaults: document-ops-and-formats §2.1. Actual encoder output
//! supplies both preview pixels and size (R21); UI text uses the bundled faces (R47).
use super::effect_config::{
    OpenSelect, RangeSpec, SelectLook, SliderState, Track, button_clicked, range_line, select,
};
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::{
    kit::{
        inputs::{self, NumberStyle},
        px,
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
        window::{DialogFrameState, Footer, dialog_button, dialog_frame_with},
    },
    theme::{self, *},
};
use egui::{Color32, Key, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use pdn_core::io::encode::SaveOptions;
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub struct FormatInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub quality_default: Option<u8>,
    pub supports_layers: bool,
}
macro_rules! format_info {($id:literal,$name:literal,[$($ext:literal),+],$quality:expr,$layers:expr)=>{FormatInfo{id:$id,name:$name,extensions:&[$($ext),+],quality_default:$quality,supports_layers:$layers}};}
pub const FORMATS: &[FormatInfo] = &[
    format_info!("pdn", "Paint.NET image", [".pdn"], None, true),
    format_info!("png", "PNG", [".png"], None, false),
    format_info!("jpeg", "JPEG", [".jpg", ".jpeg", ".jpe"], Some(95), false),
    format_info!("jxl", "JPEG XL", [".jxl"], Some(90), false),
    format_info!("avif", "AV1 (AVIF)", [".avif"], Some(90), false),
    format_info!("webp", "WebP", [".webp"], Some(90), false),
    format_info!("dds", "DirectDraw Surface (DDS)", [".dds"], None, false),
    format_info!("heic", "HEIC", [".heic", ".heif"], Some(90), false),
    format_info!("tiff", "TIFF", [".tif", ".tiff"], None, false),
    format_info!("gif", "GIF", [".gif"], None, false),
    format_info!("bmp", "BMP", [".bmp"], None, false),
    format_info!("tga", "TGA", [".tga"], None, false),
    format_info!("jxr", "JPEG XR", [".jxr", ".wdp", ".wmp"], Some(90), false),
    format_info!("psd", "Photoshop", [".psd", ".psb"], None, true),
];
pub const CHROMA_CHOICES: [&str; 4] = ["4:4:4", "4:4:0", "4:2:2", "4:2:0"];
pub fn find_format(id: &str) -> &'static FormatInfo {
    FORMATS.iter().find(|f| f.id == id).unwrap_or(&FORMATS[1])
}
pub struct SaveSelection {
    pub format: &'static str,
    pub options: SaveOptions,
}
type OnClose = Box<dyn FnOnce(Option<SaveSelection>, &mut DialogCx)>;
pub struct SaveImageDialog {
    document: u64,
    format: usize,
    options: Vec<SaveOptions>,
    initial: Value,
    loaded: bool,
    open_select: Option<OpenSelect>,
    slider: SliderState,
    on_close: Option<OnClose>,
    due: f64,
    preview: Option<TextureHandle>,
    preview_size: Vec2,
    bytes: Option<usize>,
    status: String,
    zoom: f32,
    auto_fit: bool,
    scroll: Vec2,
    last_frame: Vec2,
}
impl SaveImageDialog {
    pub fn default_options(format: &str) -> SaveOptions {
        SaveOptions::default_for(format)
    }
    pub fn options_from(format: &str, value: &Value) -> SaveOptions {
        let mut options = Self::default_options(format);
        if options.quality.is_some()
            && let Some(q) = value["quality"].as_f64().filter(|q| q.is_finite())
        {
            options.quality = Some(q.round().clamp(0.0, 100.0) as u8);
        }
        if format == "jpeg"
            && let Some(c) = value["subsampling"]
                .as_str()
                .filter(|c| CHROMA_CHOICES.contains(c))
        {
            options.chroma = Some(c.into());
        }
        options
    }
    pub fn new(
        document: u64,
        format: &str,
        initial: Value,
        on_close: impl FnOnce(Option<SaveSelection>, &mut DialogCx) + 'static,
    ) -> Self {
        Self {
            document,
            format: FORMATS.iter().position(|f| f.id == format).unwrap_or(1),
            options: FORMATS
                .iter()
                .map(|f| Self::default_options(f.id))
                .collect(),
            initial,
            loaded: false,
            open_select: None,
            slider: SliderState::default(),
            on_close: Some(Box::new(on_close)),
            due: 0.0,
            preview: None,
            preview_size: Vec2::splat(1.0),
            bytes: None,
            status: String::new(),
            zoom: 1.0,
            auto_fit: true,
            scroll: Vec2::ZERO,
            last_frame: Vec2::ZERO,
        }
    }
    fn finish(&mut self, ok: bool, cx: &mut DialogCx) -> DialogState {
        let selection = ok.then(|| SaveSelection {
            format: FORMATS[self.format].id,
            options: self.options[self.format].clone(),
        });
        if let Some(done) = self.on_close.take() {
            done(selection, cx);
        }
        DialogState::Closed(if ok {
            DialogResult::Ok
        } else {
            DialogResult::Cancel
        })
    }
    fn render_preview(&mut self, ui: &Ui, cx: &DialogCx) {
        let f = FORMATS[self.format].id;
        let result =
            super::flows::encode_document(cx.app, self.document, f, &self.options[self.format])
                .and_then(|bytes| {
                    let image = if f == "pdn" || f == "psd" {
                        let doc = if f == "pdn" {
                            pdn_core::io::pdn::decode(&bytes).map_err(|e| e.to_string())?
                        } else {
                            pdn_core::io::psd::decode(&bytes).map_err(|e| e.to_string())?
                        }
                        .to_document();
                        doc.composite().get_image_data(0, 0, doc.width, doc.height)
                    } else {
                        pdn_core::io::codecs::decode(f, &bytes)
                            .map_err(|e| e.to_string())?
                            .image
                    };
                    Ok((bytes.len(), image))
                });
        match result {
            Ok((len, img)) => {
                self.bytes = Some(len);
                self.status.clear();
                self.preview_size = Vec2::new(img.width as f32, img.height as f32);
                self.preview = Some(ui.ctx().load_texture(
                    "saved-image-preview",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [img.width as usize, img.height as usize],
                        &img.data,
                    ),
                    TextureOptions::LINEAR,
                ));
            }
            Err(e) => {
                self.bytes = None;
                self.preview = None;
                self.status = e;
            }
        }
        self.due = f64::INFINITY;
    }
    fn fit_zoom(&self, frame: Vec2) -> f32 {
        ((frame.x - 64.0) / self.preview_size.x)
            .min((frame.y - 64.0) / self.preview_size.y)
            .clamp(0.01, 1.0)
    }
    fn fit(&mut self, frame: Vec2) {
        self.auto_fit = true;
        self.zoom = self.fit_zoom(frame);
        self.scroll = Vec2::ZERO;
    }
    fn preview_viewport(&self, frame: Rect) -> Rect {
        let content = self.preview_size * self.zoom + Vec2::splat(64.0);
        let mut size = frame.size();
        // One bar can cause overflow on the other axis.
        for _ in 0..2 {
            size = frame.size()
                - Vec2::new(
                    if content.y > size.y { THICKNESS } else { 0.0 },
                    if content.x > size.x { THICKNESS } else { 0.0 },
                );
        }
        Rect::from_min_size(frame.min, size)
    }
    fn zoom_at(&mut self, value: f32, frame: Rect, anchor: Pos2) {
        let old_size = self.preview_size * self.zoom;
        let old_origin = self.preview_viewport(frame).center() - old_size / 2.0 - self.scroll;
        let image_point = (anchor - old_origin) / self.zoom;
        self.zoom = value.clamp(0.01, 16.0);
        self.auto_fit = false;
        self.scroll = self.preview_viewport(frame).center().to_vec2() - anchor.to_vec2()
            + image_point * self.zoom
            - self.preview_size * self.zoom / 2.0;
    }
    fn preview_ui(&mut self, ui: &mut Ui, frame: Rect) {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().with_clip_rect(frame);
        painter.rect_filled(
            px::snap_rect(frame, ui.ctx().pixels_per_point()),
            0.0,
            theme.dialog_border,
        );
        let inner = frame.shrink(1.0);
        painter.rect_filled(inner, 0.0, theme.preview_background);
        if self.auto_fit || self.last_frame != inner.size() {
            if self.auto_fit {
                self.fit(inner.size());
            }
            self.last_frame = inner.size();
        }
        let response = ui.interact(inner, ui.scope_id().with("preview"), Sense::click());
        ui.ctx().accesskit_node_builder(response.id, |n| {
            n.set_role(egui::accesskit::Role::Image);
            n.set_label("Saved image preview");
        });
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            if delta != 0.0 {
                let at = ui
                    .input(|i| i.pointer.hover_pos())
                    .unwrap_or(inner.center());
                self.zoom_at(self.zoom * (delta * 0.002).exp(), inner, at);
                ui.input_mut(|i| {
                    i.smooth_scroll_delta = Vec2::ZERO;
                });
            }
        }
        if response.double_clicked() {
            if (self.zoom - self.fit_zoom(inner.size())).abs() < 0.01 {
                self.zoom_at(
                    1.0,
                    inner,
                    crate::kit::inputs::interact_pos(ui, &response).unwrap_or(inner.center()),
                );
            } else {
                self.fit(inner.size());
            }
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Num0)) {
            self.fit(inner.size());
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Num1)) {
            self.zoom_at(1.0, inner, inner.center());
        }
        let size = self.preview_size * self.zoom;
        let viewport = self.preview_viewport(inner);
        let content = (size + Vec2::splat(64.0)).max(viewport.size());
        let limit = (content - viewport.size()) / 2.0;
        self.scroll = self.scroll.clamp(-limit, limit);
        for vertical in [false, true] {
            let (edge, view, length, offset) = if vertical {
                (
                    Rect::from_min_max(
                        Pos2::new(viewport.max.x, inner.min.y),
                        Pos2::new(inner.max.x, viewport.max.y),
                    ),
                    viewport.height(),
                    content.y,
                    &mut self.scroll.y,
                )
            } else {
                (
                    Rect::from_min_max(
                        Pos2::new(inner.min.x, viewport.max.y),
                        Pos2::new(viewport.max.x, inner.max.y),
                    ),
                    viewport.width(),
                    content.x,
                    &mut self.scroll.x,
                )
            };
            if length > view {
                let mut position = (*offset + (length - view) / 2.0) as f64;
                let bar = if vertical {
                    Scrollbar::vertical(edge)
                } else {
                    Scrollbar::horizontal(edge)
                };
                bar.id_salt("save-preview")
                    .colors(theme.pdn_scrollbar_track, theme.pdn_scrollbar_thumb)
                    .show(ui, length as f64, view as f64, &mut position);
                *offset = position as f32 - (length - view) / 2.0;
            }
        }
        if viewport.width() < inner.width() && viewport.height() < inner.height() {
            painter.rect_filled(
                Rect::from_min_max(viewport.max, inner.max),
                0.0,
                theme.pdn_scrollbar_track,
            );
        }
        let painter = painter.with_clip_rect(viewport);
        if let Some(texture) = &self.preview {
            let r = Rect::from_center_size(viewport.center() - self.scroll, size);
            painter.rect_filled(
                px::snap_rect(r.expand(1.0), ui.ctx().pixels_per_point()),
                0.0,
                theme.dialog_border,
            );
            let clip = painter.with_clip_rect(viewport.intersect(r));
            // Checker beneath transparent preview pixels, anchored at the image's top-left.
            let visible = r.intersect(viewport);
            let edge = 8.0;
            let start = ((visible.min - r.min) / edge).floor();
            let end = ((visible.max - r.min) / edge).ceil();
            for y in start.y as i32..end.y as i32 {
                for x in start.x as i32..end.x as i32 {
                    let c = if (x + y) % 2 == 0 {
                        Color32::from_gray(255)
                    } else {
                        Color32::from_gray(204)
                    };
                    clip.rect_filled(
                        Rect::from_min_size(
                            r.min + Vec2::new(x as f32, y as f32) * edge,
                            Vec2::splat(edge),
                        ),
                        0.0,
                        c,
                    );
                }
            }
            clip.image(
                texture.id(),
                r,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            text::record_pdn_rect(ui.ctx(), visible);
        }
    }
}
fn heading(ui: &Ui, at: Pos2, width: f32, label: &str, pdn: bool) {
    let painter = ui.painter();
    let font = FontSpec::new(DIALOG_FONT_SIZE).weight(700);
    let theme = theme::current(ui.ctx());
    let label_at = at
        + Vec2::new(
            0.0,
            (20.0 - font.resolved_line_height(ui.ctx().pixels_per_point())) / 2.0,
        );
    if pdn {
        text::draw_pdn(painter, label_at, label, font, theme.menu_item_text);
    } else {
        text::draw(painter, label_at, label, font, theme.menu_item_text);
    }
    let left = at.x + text::measure(painter, label, font) + 7.0;
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(left, at.y + 10.0),
            Vec2::new((width - (left - at.x)).max(15.0), 1.0),
        ),
        0.0,
        theme.pdn_separator,
    );
}
fn file_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
impl Dialog for SaveImageDialog {
    fn title(&self) -> String {
        "Save Configuration".into()
    }
    fn persistence_id(&self) -> Option<&'static str> {
        Some("saveConfiguration")
    }
    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        if !self.loaded {
            for (i, f) in FORMATS.iter().enumerate() {
                self.options[i] =
                    Self::options_from(f.id, &cx.settings.values["fileTypes"]["saveOptions"][f.id]);
            }
            let f = FORMATS[self.format].id;
            let mut initial = cx.settings.values["fileTypes"]["saveOptions"][f].clone();
            if let Some(values) = self.initial.as_object() {
                if !initial.is_object() {
                    initial = serde_json::json!({});
                }
                for (k, v) in values {
                    initial[k] = v.clone();
                }
            }
            self.options[self.format] = Self::options_from(f, &initial);
            self.loaded = true;
        }
        let list_open = self.open_select.is_some();
        let plain = ui.input(|i| !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.mac_cmd);
        if plain && ui.input(|i| i.key_pressed(Key::Escape)) {
            if list_open {
                self.open_select = None;
            } else {
                return self.finish(false, cx);
            }
        }
        if cx.app.now_ms >= self.due {
            self.render_preview(ui, cx);
        } else if self.due.is_finite() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(
                    (self.due - cx.app.now_ms).max(1.0) as u64,
                ));
        }
        let screen = ui.ctx().content_rect();
        let width = 1500.0_f32.min(screen.width() - 20.0);
        let total_h = 900.0_f32.min(screen.height() - 20.0);
        let body_h = total_h - DIALOG_TITLE_BAR - 48.0 - 2.0 * DIALOG_BORDER;
        let mut changed = false;
        let mut submit = false;
        let mut cancel = false;

        self.slider.now = cx.app.now_ms;
        let status = self.status.clone();
        // Like onsubmit in SaveImageDialog.js, OK flushes a pending debounce before saving.
        let ready = self.bytes.is_some();
        let mut body = |ui: &mut Ui, r: Rect| {
            let font = FontSpec::new(DIALOG_FONT_SIZE);
            let theme = theme::current(ui.ctx());
            let side = if screen.width() <= 800.0 {
                210.0
            } else {
                250.0
            };
            let at = r.min + Vec2::splat(10.0);
            let right = at.x + side;
            heading(ui, at, side, "Settings", false);
            let y = at.y + 29.0;
            text::draw(
                ui.painter(),
                Pos2::new(at.x, y),
                "File type",
                font.weight(600),
                theme.menu_item_text,
            );
            let format_rect = Rect::from_min_size(
                Pos2::new(
                    at.x,
                    y + font.resolved_line_height(ui.ctx().pixels_per_point()) + 9.0,
                ),
                Vec2::new(side, 31.0),
            );
            let labels: Vec<String> = FORMATS
                .iter()
                .map(|f| format!("{} ({})", f.name, f.extensions.join(", ")))
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let id = ui.scope_id().with("file-type");

            if let Some(pick) = select(
                ui,
                &mut self.open_select,
                id,
                format_rect,
                "File type",
                &labels,
                self.format,
                SelectLook {
                    native: true,
                    pad_left: 0.0,
                    font,
                },
            ) && pick != self.format
            {
                self.format = pick;
                changed = true;
            }
            let info = &FORMATS[self.format];
            let mut y = format_rect.max.y + 10.0;
            if let Some(default_quality) = info.quality_default {
                heading(ui, Pos2::new(at.x, y), side, "Quality", false);
                y += 28.0;
                let mut values = pdn_core::effect::Values::new();
                values.insert(
                    "quality".into(),
                    serde_json::json!(self.options[self.format].quality),
                );
                let track = Rect::from_min_size(
                    Pos2::new(at.x + 2.0, y + 0.5),
                    Vec2::new(side - 78.0, 28.0),
                );
                let number = Rect::from_min_size(Pos2::new(right - 68.0, y), Vec2::new(68.0, 29.0));
                let spec = RangeSpec {
                    min: 0.0,
                    max: 100.0,
                    default: default_quality as f64,
                    step: 1.0,
                    role: pdn_core::effect::slider::RangeRole::Slider,
                    keyboard: pdn_core::effect::slider::SliderKeys::INTEGER,
                };
                if range_line(
                    ui,
                    &mut self.slider,
                    "quality",
                    spec,
                    &mut values,
                    track,
                    number,
                    None,
                    Track::Standard,
                    NumberStyle::LAYER_OPACITY,
                )
                .is_some()
                {
                    changed = true;
                }
                self.options[self.format].quality =
                    Some(pdn_core::effect::num(&values, "quality") as u8);
                y += 39.0;
                if info.id == "jpeg" {
                    heading(ui, Pos2::new(at.x, y), side, "Chroma Subsampling", false);
                    y += 28.0;
                    let current = CHROMA_CHOICES
                        .iter()
                        .position(|c| Some(*c) == self.options[self.format].chroma.as_deref())
                        .unwrap_or(1);
                    if let Some(pick) = select(
                        ui,
                        &mut self.open_select,
                        ui.scope_id().with("chroma"),
                        Rect::from_min_size(Pos2::new(at.x, y), Vec2::new(side, 31.0)),
                        "Chroma Subsampling",
                        &CHROMA_CHOICES,
                        current,
                        SelectLook {
                            native: true,
                            pad_left: 0.0,
                            font,
                        },
                    ) {
                        self.options[self.format].chroma = Some(CHROMA_CHOICES[pick].into());
                        changed = true;
                    }
                    y += 40.0;
                }
                let response = dialog_button(
                    ui,
                    "defaults",
                    Rect::from_min_size(
                        Pos2::new(at.x + (side - 105.0) / 2.0, y + 7.0),
                        Vec2::new(105.0, 21.0),
                    ),
                    "Defaults",
                    false,
                );
                if response.clicked_by(egui::PointerButton::Primary) {
                    response.request_focus();
                }
                inputs::take_enter(ui, &response);
                if button_clicked(ui, &response) {
                    self.options[self.format] = Self::default_options(info.id);
                    changed = true;
                }
            }
            let preview_x = right + 14.0;
            let preview_w = r.max.x - 10.0 - preview_x;
            let title = match self.bytes {
                Some(n) => format!("Preview, file size: {}", file_size(n)),
                None => "Preview unavailable".into(),
            };
            heading(ui, Pos2::new(preview_x, at.y), preview_w, &title, true);
            self.preview_ui(
                ui,
                Rect::from_min_max(Pos2::new(preview_x, at.y + 28.0), r.max - Vec2::splat(10.0)),
            );
        };
        let mut footer = |ui: &mut Ui, r: Rect| {
            let theme = theme::current(ui.ctx());
            text::draw_ellipsized(
                ui.painter(),
                Rect::from_min_max(
                    r.min + Vec2::new(10.0, 15.0),
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
                .add_enabled_ui(ready, |ui| dialog_button(ui, "ok", ok_rect, "OK", false))
                .inner;
            if button_clicked(ui, &ok) && ready {
                submit = true;
            }
            let c = dialog_button(ui, "cancel", cancel_rect, "Cancel", false);
            if button_clicked(ui, &c) {
                cancel = true;
            }
        };
        let frame = dialog_frame_with(
            ui,
            "Save Configuration",
            Some("menu_file_save_icon.png"),
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
        let mut state = DialogFrameState::get(ui.ctx(), ui.scope_id());
        state.popup = if self.open_select.is_some() {
            screen
        } else {
            Rect::NOTHING
        };
        state.set(ui.ctx(), ui.scope_id());
        let close = frame.closed && (plain || !ui.input(|i| i.key_pressed(Key::Enter)));
        if cancel || close {
            return self.finish(false, cx);
        }
        if changed {
            self.due = cx.app.now_ms + 100.0;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let enter = plain
            && ui.input(|i| i.key_pressed(Key::Enter))
            && !inputs::enter_taken(ui.ctx())
            && !list_open
            && self.open_select.is_none();
        if submit || enter {
            if self.due.is_finite() {
                self.render_preview(ui, cx);
            }
            if self.bytes.is_some() {
                return self.finish(true, cx);
            }
        }
        DialogState::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_order_and_defaults() {
        assert_eq!(
            FORMATS.iter().map(|f| f.id).collect::<Vec<_>>(),
            [
                "pdn", "png", "jpeg", "jxl", "avif", "webp", "dds", "heic", "tiff", "gif", "bmp",
                "tga", "jxr", "psd"
            ]
        );
        assert_eq!(CHROMA_CHOICES, ["4:4:4", "4:4:0", "4:2:2", "4:2:0"]);
        assert_eq!(
            SaveImageDialog::default_options("jpeg").chroma.as_deref(),
            Some("4:4:0")
        );
    }
    struct Fixture {
        ctx: egui::Context,
        app: pdn_core::app::AppWorkspace,
        settings: pdn_core::settings::AppSettings,
        prefs: crate::prefs::Prefs,
        dialog: SaveImageDialog,
        result: std::rc::Rc<std::cell::RefCell<Option<SaveSelection>>>,
        time: f64,
    }
    impl Fixture {
        fn new() -> Self {
            let ctx = egui::Context::default();
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            let app = pdn_core::testing::fresh_doc(64, 32, Some(4));
            let result = std::rc::Rc::new(std::cell::RefCell::new(None));
            let out = result.clone();
            let dialog = SaveImageDialog::new(
                app.active_ws().unwrap().id,
                "jpeg",
                serde_json::json!({"quality":17,"subsampling":"4:2:0"}),
                move |selection, _| *out.borrow_mut() = selection,
            );
            Self {
                ctx,
                app,
                settings: pdn_core::settings::AppSettings::load(None),
                prefs: Default::default(),
                dialog,
                result,
                time: 0.0,
            }
        }
        fn frame(
            &mut self,
            mut events: Vec<egui::Event>,
            modifiers: egui::Modifiers,
            seat: bool,
        ) -> DialogState {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            self.time += 16.0;
            self.app.now_ms = self.time;
            let mut state = DialogState::Open;
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                    time: Some(self.time / 1000.0),
                    events,
                    ..Default::default()
                },
                |ui| {
                    state = self.dialog.show(
                        ui,
                        &mut DialogCx {
                            app: &mut self.app,
                            settings: &mut self.settings,
                            prefs: &mut self.prefs,
                            settings_changed: false,
                            shortcuts: None,
                            clipboard: None,
                            settings_imports: None,
                            fonts: None,
                            asset_library: None,
                            seat_focus: seat,
                        },
                    );
                },
            );
            output.textures_delta.clear();
            state
        }
        fn key(&mut self, key: Key, modifiers: egui::Modifiers) -> DialogState {
            self.frame(
                vec![
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers,
                    },
                ],
                modifiers,
                false,
            )
        }
        fn click(&mut self, pos: Pos2) -> DialogState {
            let mut state = DialogState::Open;
            for pressed in [true, false] {
                state = self.frame(
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    egui::Modifiers::NONE,
                    false,
                );
            }
            state
        }
        fn show(&mut self) {
            self.frame(vec![], egui::Modifiers::NONE, true);
            self.frame(vec![], egui::Modifiers::NONE, false);
        }
    }
    #[test]
    fn configuration_keyboard_modified_enter_does_not_activate_close() {
        let mut f = Fixture::new();
        f.show();
        assert_eq!(f.key(Key::Enter, egui::Modifiers::CTRL), DialogState::Open);
        assert!(f.result.borrow().is_none());
    }
    #[test]
    fn configuration_keyboard_defaults_return_provisional_options_without_remembering() {
        let mut f = Fixture::new();
        f.settings.values["fileTypes"]["saveOptions"]["jpeg"] =
            serde_json::json!({"quality":70,"subsampling":"4:2:0"});
        let remembered = f.settings.values["fileTypes"].clone();
        f.dialog.initial = serde_json::json!({"quality":80,"subsampling":"4:2:2"});
        f.app.active_ws_mut().unwrap().save_options = f.dialog.initial.clone();
        f.show();
        assert_eq!(f.dialog.options[2].quality, Some(80));
        assert_eq!(f.click(Pos2::new(146.0, 298.0)), DialogState::Open);
        assert_eq!(f.key(Key::Enter, egui::Modifiers::NONE), DialogState::Open);
        assert!(f.result.borrow().is_none());
        assert_eq!(
            f.click(Pos2::new(1093.0, 765.0)),
            DialogState::Closed(DialogResult::Ok)
        );
        let result = f.result.borrow();
        let options = &result.as_ref().unwrap().options;
        assert_eq!(options.quality, Some(95));
        assert_eq!(options.chroma.as_deref(), Some("4:4:0"));
        assert_eq!(f.settings.values["fileTypes"], remembered);
    }
    #[test]
    fn configuration_keyboard_numeric_preview_waits_one_hundred_ms() {
        let mut f = Fixture::new();
        f.settings.values["fileTypes"]["saveOptions"]["jpeg"] =
            serde_json::json!({"quality":70,"subsampling":"4:2:0"});
        f.dialog.initial = serde_json::json!({"quality":80,"subsampling":"4:2:2"});
        f.app.active_ws_mut().unwrap().save_options = f.dialog.initial.clone();
        f.show();
        let original = f.dialog.bytes;
        let remembered = f.settings.values["fileTypes"].clone();
        f.click(Pos2::new(238.0, 188.0));
        f.key(Key::A, egui::Modifiers::CTRL | egui::Modifiers::COMMAND);
        f.frame(
            vec![egui::Event::Text("90".into())],
            egui::Modifiers::NONE,
            false,
        );
        let changed_at = f.time;
        assert_eq!(f.dialog.options[2].quality, Some(90));
        assert_eq!(f.dialog.bytes, original);
        f.time = changed_at + 99.0 - 16.0;
        f.frame(vec![], egui::Modifiers::NONE, false);
        assert_eq!(f.dialog.bytes, original);
        f.time = changed_at + 100.0 - 16.0;
        f.frame(vec![], egui::Modifiers::NONE, false);
        assert_ne!(f.dialog.bytes, original);
        assert_eq!(
            f.key(Key::Escape, egui::Modifiers::NONE),
            DialogState::Closed(DialogResult::Cancel)
        );
        assert_eq!(f.settings.values["fileTypes"], remembered);
        assert_eq!(f.app.active_ws().unwrap().save_options["quality"], 80);
    }
    #[test]
    fn reopened_configuration_uses_document_token_and_new_image_uses_nested_defaults() {
        let mut f = Fixture::new();
        f.settings.values["fileTypes"]["saveOptions"]["jpeg"] =
            serde_json::json!({"quality":90,"subsampling":"4:4:4"});
        f.dialog.initial = serde_json::json!({"quality":80,"subsampling":"4:2:2"});
        f.show();
        assert_eq!(f.dialog.options[2].quality, Some(80));
        assert_eq!(f.dialog.options[2].chroma.as_deref(), Some("4:2:2"));
        let values = f.settings.values["fileTypes"].clone();
        f.dialog = SaveImageDialog::new(
            f.app.active_ws().unwrap().id,
            "jpeg",
            Value::Null,
            |_, _| {},
        );
        f.show();
        assert_eq!(f.dialog.options[2].quality, Some(90));
        assert_eq!(f.dialog.options[2].chroma.as_deref(), Some("4:4:4"));
        assert_eq!(f.settings.values["fileTypes"], values);
    }
    #[test]
    fn preview_overflow_reserves_both_scrollbars_and_keeps_zoom_anchor() {
        let mut f = Fixture::new();
        f.dialog.preview_size = Vec2::new(800.0, 600.0);
        let frame = Rect::from_min_size(Pos2::new(100.0, 80.0), Vec2::new(400.0, 300.0));
        f.dialog.fit(frame.size());
        let at = Pos2::new(250.0, 200.0);
        let before = (at
            - (frame.center() - f.dialog.preview_size * f.dialog.zoom / 2.0 - f.dialog.scroll))
            / f.dialog.zoom;
        f.dialog.zoom_at(2.0, frame, at);
        let viewport = f.dialog.preview_viewport(frame);
        assert_eq!(viewport.size(), Vec2::new(385.0, 285.0));
        let after = (at
            - (viewport.center() - f.dialog.preview_size * f.dialog.zoom / 2.0 - f.dialog.scroll))
            / f.dialog.zoom;
        assert!((before - after).length() < 0.001);
    }
    #[test]
    fn preview_zoom_keeps_pointer_anchor_and_fit_limits() {
        let mut f = Fixture::new();
        f.dialog.preview_size = Vec2::new(800.0, 600.0);
        let frame = Rect::from_min_size(Pos2::new(100.0, 80.0), Vec2::new(400.0, 300.0));
        f.dialog.fit(frame.size());
        assert!((f.dialog.zoom - 236.0 / 600.0).abs() < 0.0001);
        let at = Pos2::new(250.0, 200.0);
        let before = (at
            - (frame.center() - f.dialog.preview_size * f.dialog.zoom / 2.0 - f.dialog.scroll))
            / f.dialog.zoom;
        f.dialog.zoom_at(2.0, frame, at);
        let after = (at
            - (f.dialog.preview_viewport(frame).center()
                - f.dialog.preview_size * f.dialog.zoom / 2.0
                - f.dialog.scroll))
            / f.dialog.zoom;
        assert!((before - after).length() < 0.001);
        f.dialog.zoom_at(100.0, frame, at);
        assert_eq!(f.dialog.zoom, 16.0);
    }
}
