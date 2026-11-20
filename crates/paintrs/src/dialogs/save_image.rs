//! paintdotjs SaveImageDialog.js and window.css `.save-config-*` at b4e6fc5.
//! Paint.NET format order/defaults: document-ops-and-formats §2.1. Actual encoder output
//! supplies both preview pixels and size (R21); UI text uses the bundled faces (R47).
use super::effect_config::{
    OpenSelect, RangeSpec, SelectLook, SliderState, Track, button_clicked, checkbox_line,
    range_line, select,
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
use pdn_core::canvas::ImageData;
use pdn_core::effect::Control;
use pdn_core::io::encode::{BitDepth, Quantizer, SaveOptions};
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub struct FormatInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub quality_default: Option<u8>,
    pub supports_layers: bool,
    /// A FileType plug-in's save options (its property collection); empty for the built-ins.
    pub controls: &'static [pdn_core::effect::Control],
}
macro_rules! format_info {($id:literal,$name:literal,[$($ext:literal),+],$quality:expr,$layers:expr)=>{FormatInfo{id:$id,name:$name,extensions:&[$($ext),+],quality_default:$quality,supports_layers:$layers,controls:&[]}};}
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

/// One of Paint.NET's own save options of the 8-bit pipeline formats (document-ops-and-formats
/// §2.1; `PngFileType`, `BmpFileType`, `TiffFileType`, `GifFileType`, `TgaFileType` save property
/// collections and config UIs), shown as Paint.NET shows a property: its header, then its control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum SaveProp {
    /// "Bit Depth", a drop-down of these depths in Paint.NET's order.
    Depth(&'static [BitDepth]),
    Quantization,
    Dithering,
    Threshold,
    Interlaced,
    ConvertToSrgb,
    Rle,
}
const BMP_DEPTHS: &[BitDepth] = {
    use BitDepth::*;
    &[Auto, B32, B24, B8, B4, B1]
};
/// The built-in format's own options, in Paint.NET's order.
pub(super) fn save_props(format: &str) -> &'static [SaveProp] {
    use BitDepth::*;
    use SaveProp::*;
    match format {
        "png" => &[
            Depth(&[Auto, B32, B24, B8, B4, B2, B1]),
            Quantization,
            Dithering,
            Threshold,
            Interlaced,
        ],
        "bmp" => &[Depth(BMP_DEPTHS), Quantization, Dithering, ConvertToSrgb],
        "tiff" => &[Depth(BMP_DEPTHS), Quantization, Dithering],
        "gif" => &[Quantization, Dithering, Threshold, ConvertToSrgb],
        "tga" => &[Depth(&[Auto, B32, B24]), ConvertToSrgb, Rle],
        _ => &[],
    }
}
const QUANTIZERS: [(Quantizer, &str); 2] = [
    (Quantizer::MedianCut, "Median Cut"),
    (Quantizer::Octree, "Octree"),
];
fn depth_label(d: BitDepth) -> &'static str {
    match d {
        BitDepth::Auto => "Auto-detect",
        BitDepth::B32 => "32-bit",
        BitDepth::B24 => "24-bit",
        BitDepth::B8 => "8-bit",
        BitDepth::B4 => "4-bit",
        BitDepth::B2 => "2-bit",
        BitDepth::B1 => "1-bit",
    }
}
/// A depth as the remembered options keep it.
fn depth_token(d: BitDepth) -> &'static str {
    match d {
        BitDepth::Auto => "auto",
        BitDepth::B32 => "32",
        BitDepth::B24 => "24",
        BitDepth::B8 => "8",
        BitDepth::B4 => "4",
        BitDepth::B2 => "2",
        BitDepth::B1 => "1",
    }
}
impl SaveProp {
    /// The remembered options' key.
    fn key(self) -> &'static str {
        match self {
            SaveProp::Depth(_) => "bitDepth",
            SaveProp::Quantization => "quantization",
            SaveProp::Dithering => "ditherLevel",
            SaveProp::Threshold => "transparencyThreshold",
            SaveProp::Interlaced => "interlaced",
            SaveProp::ConvertToSrgb => "convertToSrgb",
            SaveProp::Rle => "rleCompress",
        }
    }
    /// The header over the control; the check boxes without one have their text only.
    fn header(self) -> &'static str {
        match self {
            SaveProp::Depth(_) => "Bit Depth",
            SaveProp::Quantization => "Quantization algorithm",
            SaveProp::Dithering => "Dithering level",
            SaveProp::Threshold => "Transparency threshold",
            SaveProp::Interlaced => "Other options",
            SaveProp::ConvertToSrgb | SaveProp::Rle => "",
        }
    }
    /// Paint.NET's read-only rules: quantization and dithering apply at an indexed depth only, and
    /// PNG's threshold at 8-bit only. GIF is always indexed, so its rows are always editable.
    fn enabled(self, format: &str, depth: BitDepth) -> bool {
        use BitDepth::*;
        match self {
            _ if format == "gif" => true,
            SaveProp::Quantization | SaveProp::Dithering => matches!(depth, B8 | B4 | B2 | B1),
            SaveProp::Threshold => depth == B8,
            _ => true,
        }
    }
    /// The option as the remembered options keep it.
    fn value(self, o: &SaveOptions) -> Value {
        match self {
            SaveProp::Depth(_) => depth_token(o.bit_depth).into(),
            SaveProp::Quantization => match o.quantizer {
                Quantizer::Octree => "octree".into(),
                Quantizer::MedianCut => "medianCut".into(),
            },
            SaveProp::Dithering => o.dither.into(),
            SaveProp::Threshold => o.alpha_threshold.into(),
            SaveProp::Interlaced => o.interlaced.into(),
            SaveProp::ConvertToSrgb => o.convert_to_srgb.into(),
            SaveProp::Rle => o.rle.into(),
        }
    }
    /// Takes a remembered value the property accepts; anything else keeps `o` as it is.
    fn restore(self, v: &Value, o: &mut SaveOptions) {
        let level = |max: f64| {
            v.as_f64()
                .filter(|n| n.is_finite())
                .map(|n| n.round().clamp(0.0, max) as u8)
        };
        match self {
            SaveProp::Depth(choices) => {
                if let Some(&d) = choices
                    .iter()
                    .find(|&&d| v.as_str() == Some(depth_token(d)))
                {
                    o.bit_depth = d;
                }
            }
            SaveProp::Quantization => match v.as_str() {
                Some("octree") => o.quantizer = Quantizer::Octree,
                Some("medianCut") => o.quantizer = Quantizer::MedianCut,
                _ => {}
            },
            SaveProp::Dithering => o.dither = level(8.0).unwrap_or(o.dither),
            SaveProp::Threshold => o.alpha_threshold = level(255.0).unwrap_or(o.alpha_threshold),
            SaveProp::Interlaced => o.interlaced = v.as_bool().unwrap_or(o.interlaced),
            SaveProp::ConvertToSrgb => o.convert_to_srgb = v.as_bool().unwrap_or(o.convert_to_srgb),
            SaveProp::Rle => o.rle = v.as_bool().unwrap_or(o.rle),
        }
    }
}
/// `format`'s own options into the remembered options `value`.
pub(super) fn save_prop_values(format: &str, options: &SaveOptions, value: &mut Value) {
    for p in save_props(format) {
        value[p.key()] = p.value(options);
    }
}
pub fn find_format(id: &str) -> &'static FormatInfo {
    formats().find(|f| f.id == id).unwrap_or(&FORMATS[1])
}

/// The formats an image saves to: the built-ins, then those of the installed FileType plug-ins.
pub fn formats() -> impl Iterator<Item = &'static FormatInfo> {
    FORMATS.iter().chain(plugin_formats())
}

/// The installed FileType plug-ins' save formats, rebuilt when the loaded plug-ins change (the
/// list is leaked each time: a few bytes per plug-in reload).
fn plugin_formats() -> &'static [FormatInfo] {
    static CACHE: std::sync::Mutex<(u64, &'static [FormatInfo])> =
        std::sync::Mutex::new((u64::MAX, &[]));
    let revision = pdn_core::effect::dotnet::revision();
    let mut cache = CACHE.lock().unwrap();
    if cache.0 != revision {
        let list: Vec<FormatInfo> = pdn_core::io::plugins::formats()
            .into_iter()
            .filter(|f| f.can_save)
            .map(|f| FormatInfo {
                id: f.id,
                name: f.name,
                extensions: Box::leak(
                    f.extensions
                        .iter()
                        .map(|e| &*Box::leak(format!(".{e}").into_boxed_str()))
                        .collect::<Vec<&'static str>>()
                        .into_boxed_slice(),
                ),
                quality_default: None,
                supports_layers: false,
                controls: f.controls,
            })
            .collect();
        *cache = (revision, Box::leak(list.into_boxed_slice()));
    }
    cache.1
}

/// The built-in formats plus any plugin formats.
fn all_formats() -> Vec<FormatInfo> {
    formats().copied().collect()
}
pub struct SaveSelection {
    pub format: &'static str,
    pub options: SaveOptions,
}
type OnClose = Box<dyn FnOnce(Option<SaveSelection>, &mut DialogCx)>;
/// Batch Export keeps one per image and shows its settings and preview (`body`).
pub struct SaveImageDialog {
    pub(super) document: u64,
    pub(super) format: usize,
    /// Built-in formats plus plugin formats, snapshotted when the dialog opens.
    pub(super) formats: Vec<FormatInfo>,
    pub(super) options: Vec<SaveOptions>,
    initial: Value,
    loaded: bool,
    pub(super) open_select: Option<OpenSelect>,
    slider: SliderState,
    on_close: Option<OnClose>,
    pub(super) due: f64,
    pub(super) preview: Option<TextureHandle>,
    /// The browser decode of the last encoded file, while it runs (web AVIF and HEIC).
    decoding: Option<crate::platform::Pending<Result<ImageData, String>>>,
    preview_size: Vec2,
    bytes: Option<usize>,
    /// The format's FileType asks for its options in a window of its own when the image is saved, so it has no
    /// preview, but OK saves (and shows that window).
    asks_window: bool,
    pub(super) status: String,
    zoom: f32,
    auto_fit: bool,
    scroll: Vec2,
    last_frame: Vec2,
}
impl SaveImageDialog {
    pub fn default_options(format: &str) -> SaveOptions {
        SaveOptions {
            plugin: find_format(format)
                .controls
                .iter()
                .map(|c| (c.key().to_string(), c.default_value()))
                .collect(),
            ..SaveOptions::default_for(format)
        }
    }
    pub fn options_from(format: &str, value: &Value) -> SaveOptions {
        let mut options = Self::default_options(format);
        // A plug-in format's remembered options, for the properties it still has.
        if let Some(props) = value["properties"].as_object() {
            for (k, v) in props {
                if let Some(slot) = options.plugin.get_mut(k) {
                    *slot = v.clone();
                }
            }
        }
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
        for p in save_props(format) {
            p.restore(&value[p.key()], &mut options);
        }
        options
    }
    pub fn new(
        document: u64,
        format: &str,
        initial: Value,
        on_close: impl FnOnce(Option<SaveSelection>, &mut DialogCx) + 'static,
    ) -> Self {
        let formats = all_formats();
        Self {
            document,
            format: formats.iter().position(|f| f.id == format).unwrap_or(1),
            options: formats
                .iter()
                .map(|f| Self::default_options(f.id))
                .collect(),
            formats,
            initial,
            loaded: false,
            open_select: None,
            slider: SliderState::default(),
            on_close: Some(Box::new(on_close)),
            due: 0.0,
            preview: None,
            decoding: None,
            preview_size: Vec2::splat(1.0),
            bytes: None,
            asks_window: false,
            status: String::new(),
            zoom: 1.0,
            auto_fit: true,
            scroll: Vec2::ZERO,
            last_frame: Vec2::ZERO,
        }
    }
    fn finish(&mut self, ok: bool, cx: &mut DialogCx) -> DialogState {
        let selection = ok.then(|| SaveSelection {
            format: self.formats[self.format].id,
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
        self.due = f64::INFINITY;
        self.decoding = None;
        let f = self.formats[self.format].id;
        let options = &self.options[self.format];
        let bytes = match super::flows::encode_document(cx.app, self.document, f, options) {
            Ok(bytes) => bytes,
            Err(e) => return self.preview_failed(e),
        };
        self.bytes = Some(bytes.len());
        self.asks_window = false;
        self.status.clear();
        // The web build decodes AVIF and HEIC in the browser: the picture follows when it lands.
        #[cfg(target_arch = "wasm32")]
        if matches!(f, "avif" | "heic") {
            self.decoding = Some(crate::platform::Pending::future(async move {
                crate::platform::web::decode_async(f, &bytes)
                    .await
                    .map_err(|e| e.to_string())
            }));
            return self.poll_decoding(ui);
        }
        let decoded = if f == "pdn" || f == "psd" {
            let doc = if f == "pdn" {
                pdn_core::io::pdn::decode(&bytes).map_err(|e| e.to_string())
            } else {
                pdn_core::io::psd::decode(&bytes).map_err(|e| e.to_string())
            };
            doc.map(|d| {
                let doc = d.to_document();
                doc.composite().get_image_data(0, 0, doc.width, doc.height)
            })
        } else {
            pdn_core::io::codecs::decode(f, &bytes)
                .map(|d| d.image)
                .map_err(|e| e.to_string())
        };
        match decoded {
            Ok(image) => self.show_picture(ui, image),
            Err(e) => self.preview_failed(e),
        }
    }
    /// The decoded file as the preview's picture.
    fn show_picture(&mut self, ui: &Ui, img: ImageData) {
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
    fn preview_failed(&mut self, e: String) {
        self.bytes = None;
        self.preview = None;
        self.asks_window = pdn_core::io::plugins::PluginWindow::from_error(&e).is_some();
        self.status = super::flows::readable(e);
    }
    /// A browser decode of the encoded file (the web's AVIF and HEIC), once it lands.
    fn poll_decoding(&mut self, ui: &Ui) {
        let Some(op) = &mut self.decoding else {
            return;
        };
        let Some(result) = op.poll() else {
            ui.ctx().request_repaint();
            return;
        };
        self.decoding = None;
        match result {
            Ok(image) => self.show_picture(ui, image),
            Err(e) => self.preview_failed(e),
        }
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
            // On whole device pixels: a fractional edge let the checker beneath show through as
            // a line of "transparency" under an opaque image.
            let ppp = ui.ctx().pixels_per_point();
            let r = px::snap_rect(
                Rect::from_center_size(viewport.center() - self.scroll, size),
                ppp,
            );
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
pub(super) fn heading(ui: &Ui, at: Pos2, width: f32, label: &str, pdn: bool) {
    let theme = theme::current(ui.ctx());
    heading_with(ui, at, width, label, pdn, theme.menu_item_text);
}
/// `heading` in `color` (a read-only option's header).
fn heading_in(ui: &Ui, at: Pos2, width: f32, label: &str, color: Color32) {
    heading_with(ui, at, width, label, false, color);
}
fn heading_with(ui: &Ui, at: Pos2, width: f32, label: &str, pdn: bool, color: Color32) {
    let painter = ui.painter();
    let font = FontSpec::new(DIALOG_FONT_SIZE).weight(700);
    let theme = theme::current(ui.ctx());
    let label_at = at
        + Vec2::new(
            0.0,
            (20.0 - font.resolved_line_height(ui.ctx().pixels_per_point())) / 2.0,
        );
    if pdn {
        text::draw_pdn(painter, label_at, label, font, color);
    } else {
        text::draw(painter, label_at, label, font, color);
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
impl SaveImageDialog {
    /// The remembered options with the document's token over its format's, once.
    pub(super) fn load(&mut self, settings: &pdn_core::settings::AppSettings) {
        if self.loaded {
            return;
        }
        for (i, f) in self.formats.iter().enumerate() {
            self.options[i] =
                Self::options_from(f.id, &settings.values["fileTypes"]["saveOptions"][f.id]);
        }
        let f = self.formats[self.format].id;
        let mut initial = settings.values["fileTypes"]["saveOptions"][f].clone();
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
    /// The frame's clock; the preview renders once its debounce is due.
    pub(super) fn tick(&mut self, ui: &Ui, cx: &DialogCx) {
        self.slider.now = cx.app.now_ms;
        self.poll_decoding(ui);
        if cx.app.now_ms >= self.due {
            self.render_preview(ui, cx);
        } else if self.due.is_finite() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(
                    (self.due - cx.app.now_ms).max(1.0) as u64,
                ));
        }
    }
    /// The Settings column and the Preview in the body box `r`, with `extra` points free under
    /// the Settings heading (Batch Export's file name). Returns where the column's controls end;
    /// a change renders the preview again 100 ms later.
    pub(super) fn body(&mut self, ui: &mut Ui, r: Rect, extra: f32) -> f32 {
        let mut changed = false;
        let font = FontSpec::new(DIALOG_FONT_SIZE);
        let theme = theme::current(ui.ctx());
        let side = side(ui);
        let at = r.min + Vec2::splat(10.0);
        let right = at.x + side;
        heading(ui, at, side, "Settings", false);
        let y = at.y + 29.0 + extra;
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
        let labels: Vec<String> = self
            .formats
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
        let info = &self.formats[self.format];
        let mut y = format_rect.max.y + 10.0;
        let mut bottom = y;
        if let Some(default_quality) = info.quality_default {
            heading(ui, Pos2::new(at.x, y), side, "Quality", false);
            y += 28.0;
            let mut values = pdn_core::effect::Values::new();
            values.insert(
                "quality".into(),
                serde_json::json!(self.options[self.format].quality),
            );
            let track =
                Rect::from_min_size(Pos2::new(at.x + 2.0, y + 0.5), Vec2::new(side - 78.0, 28.0));
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
                "Quality",
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
        }
        let props = save_props(info.id);
        if !props.is_empty() && self.own_options(ui, at.x, side, &mut y) {
            changed = true;
        }
        let info = &self.formats[self.format];
        // A FileType plug-in's save options: its properties, as an effect's are shown.
        if !info.controls.is_empty() {
            let mut values = self.options[self.format].plugin.clone();
            for (i, c) in info.controls.iter().enumerate() {
                match c {
                    Control::Range {
                        key,
                        label,
                        min,
                        max,
                        default,
                        step,
                        keyboard,
                        ..
                    } => {
                        heading(ui, Pos2::new(at.x, y), side, label, false);
                        y += 28.0;
                        let track = Rect::from_min_size(
                            Pos2::new(at.x + 2.0, y + 0.5),
                            Vec2::new(side - 78.0, 28.0),
                        );
                        let number =
                            Rect::from_min_size(Pos2::new(right - 68.0, y), Vec2::new(68.0, 29.0));
                        let spec = RangeSpec {
                            min: *min,
                            max: *max,
                            default: *default,
                            step: *step,
                            role: pdn_core::effect::slider::RangeRole::Slider,
                            keyboard: *keyboard,
                        };
                        let style =
                            NumberStyle::LAYER_OPACITY.decimals(if *step < 1.0 { 2 } else { 0 });
                        if range_line(
                            ui,
                            &mut self.slider,
                            key,
                            label,
                            spec,
                            &mut values,
                            track,
                            number,
                            None,
                            Track::Standard,
                            style,
                        )
                        .is_some()
                        {
                            changed = true;
                        }
                        y += 39.0;
                    }
                    Control::Checkbox { key, label, .. } => {
                        let color = theme.menu_item_text;
                        if checkbox_line(ui, &mut values, key, label, at.x, y, side, color, 1.0)
                            .is_some()
                        {
                            changed = true;
                        }
                        y += 28.0;
                    }
                    Control::Select {
                        key,
                        label,
                        choices,
                        ..
                    } => {
                        heading(ui, Pos2::new(at.x, y), side, label, false);
                        y += 28.0;
                        let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
                        let current = choices
                            .iter()
                            .position(|c| values.get(*key).and_then(Value::as_str) == Some(c.0))
                            .unwrap_or(0);
                        if let Some(pick) = select(
                            ui,
                            &mut self.open_select,
                            ui.scope_id().with(("plugin-option", i)),
                            Rect::from_min_size(Pos2::new(at.x, y), Vec2::new(side, 31.0)),
                            label,
                            &labels,
                            current,
                            SelectLook {
                                native: true,
                                pad_left: 0.0,
                                font,
                            },
                        ) && pick != current
                        {
                            values.insert(key.to_string(), Value::from(choices[pick].0));
                            changed = true;
                        }
                        y += 40.0;
                    }
                    // ponytail: text and other property kinds keep the plug-in's defaults; add an
                    // editor when a FileType's save options need one.
                    _ => {}
                }
            }
            self.options[self.format].plugin = values;
        }
        if info.quality_default.is_some() || !info.controls.is_empty() || !props.is_empty() {
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
            bottom = y + 38.0;
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
        if changed {
            self.due = self.slider.now + 100.0;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        bottom
    }
}
impl SaveImageDialog {
    /// The format's own options (`save_props`) from `y` down, each as Paint.NET shows a property:
    /// its header, then its control. A read-only row (`SaveProp::enabled`) takes no input and
    /// keeps its value, greyed out as an effect dialog's disabled rows are.
    fn own_options(&mut self, ui: &mut Ui, x: f32, side: f32, y: &mut f32) -> bool {
        let format = self.formats[self.format].id;
        let theme = theme::current(ui.ctx());
        let font = FontSpec::new(DIALOG_FONT_SIZE);
        let line = font.resolved_line_height(ui.ctx().pixels_per_point());
        let mut changed = false;
        for &prop in save_props(format) {
            let enabled = prop.enabled(format, self.options[self.format].bit_depth);
            let key = prop.key();
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("save-option", key))
                    .max_rect(ui.max_rect()),
            );
            let base = row.opacity();
            if !enabled {
                row.disable();
            }
            row.set_opacity(base);
            let (color, fade) = if enabled {
                (theme.menu_item_text, 1.0)
            } else {
                (MENU_ITEM_DISABLED_TEXT, NUMBER_DISABLED_OPACITY)
            };
            let header = prop.header();
            if !header.is_empty() {
                heading_in(&row, Pos2::new(x, *y), side, header, color);
                *y += 28.0;
            }
            let o = &self.options[self.format];
            match prop {
                SaveProp::Depth(_) | SaveProp::Quantization => {
                    let (labels, current): (Vec<&str>, usize) = match prop {
                        SaveProp::Depth(choices) => (
                            choices.iter().map(|&d| depth_label(d)).collect(),
                            choices.iter().position(|&d| d == o.bit_depth).unwrap_or(0),
                        ),
                        _ => (
                            QUANTIZERS.iter().map(|q| q.1).collect(),
                            QUANTIZERS
                                .iter()
                                .position(|q| q.0 == o.quantizer)
                                .unwrap_or(1),
                        ),
                    };
                    row.set_opacity(base * fade);
                    let id = row.scope_id().with("choice");
                    let at = Rect::from_min_size(Pos2::new(x, *y), Vec2::new(side, 31.0));
                    let look = SelectLook {
                        native: true,
                        pad_left: 0.0,
                        font,
                    };
                    if let Some(pick) = select(
                        &mut row,
                        &mut self.open_select,
                        id,
                        at,
                        header,
                        &labels,
                        current,
                        look,
                    ) && pick != current
                    {
                        let o = &mut self.options[self.format];
                        match prop {
                            SaveProp::Depth(choices) => o.bit_depth = choices[pick],
                            _ => o.quantizer = QUANTIZERS[pick].0,
                        }
                        changed = true;
                    }
                    *y += 40.0;
                }
                SaveProp::Dithering | SaveProp::Threshold => {
                    let (max, default) = match prop {
                        SaveProp::Dithering => (8.0, 7.0),
                        _ => (255.0, 128.0),
                    };
                    let mut values = pdn_core::effect::Values::new();
                    values.insert(key.into(), prop.value(o));
                    let track = Rect::from_min_size(
                        Pos2::new(x + 2.0, *y + 0.5),
                        Vec2::new(side - 78.0, 28.0),
                    );
                    let number =
                        Rect::from_min_size(Pos2::new(x + side - 68.0, *y), Vec2::new(68.0, 29.0));
                    let spec = RangeSpec {
                        min: 0.0,
                        max,
                        default,
                        step: 1.0,
                        role: pdn_core::effect::slider::RangeRole::Slider,
                        keyboard: pdn_core::effect::slider::SliderKeys::INTEGER,
                    };
                    row.set_opacity(base * fade);
                    if range_line(
                        &mut row,
                        &mut self.slider,
                        key,
                        header,
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
                    let level = pdn_core::effect::num(&values, key) as u8;
                    let o = &mut self.options[self.format];
                    match prop {
                        SaveProp::Dithering => o.dither = level,
                        _ => o.alpha_threshold = level,
                    }
                    *y += 39.0;
                    if prop == SaveProp::Threshold {
                        // The property's description, under its slider.
                        let runs = [text::Run {
                            text: "Pixels with an alpha value less than the threshold will be \
                                   fully transparent.",
                            spec: font,
                            color,
                            underline: false,
                        }];
                        let lines = text::wrap_runs(row.painter(), &runs, side);
                        text::draw_lines(row.painter(), Pos2::new(x, *y), &runs, &lines, line);
                        *y += lines.len() as f32 * line + 8.0;
                    }
                }
                SaveProp::Interlaced | SaveProp::ConvertToSrgb | SaveProp::Rle => {
                    let label = match prop {
                        SaveProp::Interlaced => "Interlaced",
                        SaveProp::ConvertToSrgb => "Convert to sRGB",
                        _ => "Compress (RLE)",
                    };
                    let mut values = pdn_core::effect::Values::new();
                    values.insert(key.into(), prop.value(o));
                    if checkbox_line(&mut row, &mut values, key, label, x, *y, side, color, fade)
                        .is_some()
                    {
                        prop.restore(&values[key], &mut self.options[self.format]);
                        changed = true;
                    }
                    *y += 28.0;
                }
            }
        }
        changed
    }
}
/// The Settings column's width.
pub(super) fn side(ui: &Ui) -> f32 {
    if ui.ctx().content_rect().width() <= 800.0 {
        210.0
    } else {
        250.0
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
        self.load(cx.settings);
        let list_open = self.open_select.is_some();
        let plain = ui.input(|i| !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.mac_cmd);
        if plain && ui.input(|i| i.key_pressed(Key::Escape)) {
            if list_open {
                self.open_select = None;
            } else {
                return self.finish(false, cx);
            }
        }
        self.tick(ui, cx);
        let screen = ui.ctx().content_rect();
        let width = 1500.0_f32.min(screen.width() - 20.0);
        let total_h = 900.0_f32.min(screen.height() - 20.0);
        let body_h = total_h - DIALOG_TITLE_BAR - 48.0 - 2.0 * DIALOG_BORDER;
        let mut submit = false;
        let mut cancel = false;

        let status = self.status.clone();
        // Like onsubmit in SaveImageDialog.js, OK flushes a pending debounce before saving.
        let ready = self.bytes.is_some() || self.asks_window;
        let mut body = |ui: &mut Ui, r: Rect| {
            self.body(ui, r, 0.0);
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
        let enter = plain
            && ui.input(|i| i.key_pressed(Key::Enter))
            && !inputs::enter_taken(ui.ctx())
            && !list_open
            && self.open_select.is_none();
        if submit || enter {
            if self.due.is_finite() {
                self.render_preview(ui, cx);
            }
            if self.bytes.is_some() || self.asks_window {
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
            Self::with(
                "jpeg",
                serde_json::json!({"quality":17,"subsampling":"4:2:0"}),
            )
        }
        fn with(format: &str, initial: Value) -> Self {
            let ctx = egui::Context::default();
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            let app = pdn_core::testing::fresh_doc(64, 32, Some(4));
            let result = std::rc::Rc::new(std::cell::RefCell::new(None));
            let out = result.clone();
            let dialog = SaveImageDialog::new(
                app.active_ws().unwrap().id,
                format,
                initial,
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
    /// Paint.NET's own save options per format: the rows in order, the defaults, which rows the
    /// bit depth makes read-only, and the drop-down labels.
    #[test]
    fn built_in_options_follow_paint_net() {
        use BitDepth::*;
        let rows = |f: &str| save_props(f).iter().map(|p| p.key()).collect::<Vec<_>>();
        assert_eq!(
            rows("png"),
            [
                "bitDepth",
                "quantization",
                "ditherLevel",
                "transparencyThreshold",
                "interlaced"
            ]
        );
        assert_eq!(
            rows("bmp"),
            ["bitDepth", "quantization", "ditherLevel", "convertToSrgb"]
        );
        assert_eq!(rows("tiff"), ["bitDepth", "quantization", "ditherLevel"]);
        assert_eq!(
            rows("gif"),
            [
                "quantization",
                "ditherLevel",
                "transparencyThreshold",
                "convertToSrgb"
            ]
        );
        assert_eq!(rows("tga"), ["bitDepth", "convertToSrgb", "rleCompress"]);
        assert!(save_props("jpeg").is_empty());
        let depths = |f: &str| match save_props(f)[0] {
            SaveProp::Depth(d) => d.iter().map(|&d| depth_label(d)).collect::<Vec<_>>(),
            _ => vec![],
        };
        assert_eq!(
            depths("png"),
            [
                "Auto-detect",
                "32-bit",
                "24-bit",
                "8-bit",
                "4-bit",
                "2-bit",
                "1-bit"
            ]
        );
        assert_eq!(
            depths("bmp"),
            ["Auto-detect", "32-bit", "24-bit", "8-bit", "4-bit", "1-bit"]
        );
        assert_eq!(depths("tiff"), depths("bmp"));
        assert_eq!(depths("tga"), ["Auto-detect", "32-bit", "24-bit"]);
        assert_eq!(QUANTIZERS.map(|q| q.1), ["Median Cut", "Octree"]);
        for f in ["png", "bmp", "tiff", "gif", "tga"] {
            let o = SaveImageDialog::default_options(f);
            assert_eq!(
                (o.bit_depth, o.quantizer, o.dither, o.alpha_threshold),
                (Auto, Quantizer::Octree, 7, 128),
                "{f}"
            );
            assert!(!o.interlaced && o.convert_to_srgb && o.rle, "{f}");
        }
        let on = |f: &str, p: SaveProp, d| p.enabled(f, d);
        for (f, indexed) in [
            ("png", &[B8, B4, B2, B1][..]),
            ("bmp", &[B8, B4, B1]),
            ("tiff", &[B8, B4, B1]),
        ] {
            let SaveProp::Depth(offered) = save_props(f)[0] else {
                unreachable!()
            };
            for &d in offered {
                let want = indexed.contains(&d);
                assert_eq!(on(f, SaveProp::Quantization, d), want, "{f} {d:?}");
                assert_eq!(on(f, SaveProp::Dithering, d), want, "{f} {d:?}");
            }
        }
        assert!(on("png", SaveProp::Threshold, B8) && !on("png", SaveProp::Threshold, B4));
        assert!(!on("png", SaveProp::Threshold, Auto) && on("png", SaveProp::Interlaced, Auto));
        assert!(on("gif", SaveProp::Dithering, Auto) && on("gif", SaveProp::Threshold, Auto));
    }
    /// The options a save remembers come back as they were; a value the format does not take
    /// keeps the default, and a level outside its range is clamped.
    #[test]
    fn built_in_options_are_remembered() {
        for (f, depth) in [
            ("png", BitDepth::B2),
            ("bmp", BitDepth::B4),
            ("tiff", BitDepth::B1),
            ("gif", BitDepth::Auto),
            ("tga", BitDepth::B24),
        ] {
            let o = SaveOptions {
                bit_depth: depth,
                quantizer: Quantizer::MedianCut,
                dither: 3,
                alpha_threshold: 40,
                interlaced: true,
                convert_to_srgb: false,
                rle: false,
                ..SaveImageDialog::default_options(f)
            };
            let back = SaveImageDialog::options_from(f, &super::super::flows::option_values(f, &o));
            for p in save_props(f) {
                assert_eq!(p.value(&back), p.value(&o), "{f} {}", p.key());
            }
        }
        let bmp = SaveImageDialog::options_from(
            "bmp",
            &serde_json::json!({"bitDepth": "2", "ditherLevel": 99, "quantization": "best"}),
        );
        assert_eq!(bmp.bit_depth, BitDepth::Auto);
        assert_eq!(bmp.dither, 8);
        assert_eq!(bmp.quantizer, Quantizer::Octree);
        let values = super::super::flows::option_values("bmp", &bmp);
        assert_eq!(
            values,
            serde_json::json!({"bitDepth": "auto", "quantization": "octree", "ditherLevel": 8,
                "convertToSrgb": true})
        );
    }
    /// BMP's Save Configuration shows Paint.NET's rows, and the preview's file size follows the
    /// bit depth.
    #[test]
    fn bmp_configuration_shows_its_options_and_sizes() {
        let mut f = Fixture::with("bmp", serde_json::json!({}));
        text::enable_text_recording(&f.ctx);
        f.show();
        let runs = text::take_text_runs(&f.ctx);
        let drawn: Vec<String> = runs.into_iter().filter_map(|r| r.2).collect();
        for label in [
            "Bit Depth",
            "Auto-detect",
            "Quantization algorithm",
            "Octree",
            "Dithering level",
            "Convert to sRGB",
            "Defaults",
        ] {
            assert!(drawn.iter().any(|t| t == label), "{label} in {drawn:?}");
        }
        let bmp = f.dialog.format;
        let size = |f: &mut Fixture, depth| {
            f.dialog.options[bmp].bit_depth = depth;
            f.dialog.due = 0.0;
            f.frame(vec![], egui::Modifiers::NONE, false);
            f.dialog.bytes.unwrap()
        };
        // 64×32: 24-bit rows are 192 bytes, 8-bit ones 64 plus the palette.
        let (b24, b8, b1) = (
            size(&mut f, BitDepth::B24),
            size(&mut f, BitDepth::B8),
            size(&mut f, BitDepth::B1),
        );
        assert_eq!(b24, 54 + 64 * 32 * 3);
        assert!(b1 < b8 && b8 < b24, "{b1} {b8} {b24}");
    }
    /// JPEG XR previews as the other formats do, from the bytes it encodes at the current quality.
    #[test]
    fn jxr_previews_its_encoded_file() {
        let mut f = Fixture::with("jxr", serde_json::json!({}));
        text::enable_text_recording(&f.ctx);
        f.show();
        let drawn: Vec<String> = text::take_text_runs(&f.ctx)
            .into_iter()
            .filter_map(|r| r.2)
            .collect();
        let status = &f.dialog.status;
        assert!(
            drawn.iter().any(|t| t.starts_with("Preview, file size:")),
            "{status} {drawn:?}"
        );
        assert!(f.dialog.preview.is_some());
        let jxr = f.dialog.format;
        let at_90 = f.dialog.bytes.unwrap();
        f.dialog.options[jxr].quality = Some(20);
        f.dialog.due = 0.0;
        f.frame(vec![], egui::Modifiers::NONE, false);
        assert!(f.dialog.bytes.unwrap() < at_90);
    }
    /// The web's AVIF (and HEIC) preview lands from a browser decode that resolves frames later:
    /// the dialog polls it each frame, shows its picture when it resolves, and an error reads as a
    /// failed preview.
    #[test]
    fn a_pending_decode_shows_its_picture_when_it_lands() {
        let later = |result: Result<ImageData, String>| {
            let mut result = Some(result);
            let mut polled = false;
            crate::platform::Pending::future(std::future::poll_fn(move |_| {
                if std::mem::replace(&mut polled, true) {
                    std::task::Poll::Ready(result.take().unwrap())
                } else {
                    std::task::Poll::Pending
                }
            }))
        };
        let mut f = Fixture::with("png", serde_json::json!({}));
        f.show();
        let picture = ImageData {
            width: 2,
            height: 3,
            data: vec![255; 24],
        };
        f.dialog.decoding = Some(later(Ok(picture)));
        f.frame(vec![], egui::Modifiers::NONE, false);
        assert!(f.dialog.decoding.is_some(), "still decoding");
        assert_eq!(f.dialog.preview_size, Vec2::new(64.0, 32.0));
        f.frame(vec![], egui::Modifiers::NONE, false);
        assert!(f.dialog.decoding.is_none());
        assert_eq!(f.dialog.preview_size, Vec2::new(2.0, 3.0));
        assert!(f.dialog.bytes.is_some());
        f.dialog.decoding = Some(later(Err("no decoder".into())));
        f.frame(vec![], egui::Modifiers::NONE, false);
        f.frame(vec![], egui::Modifiers::NONE, false);
        assert!(f.dialog.decoding.is_none());
        assert_eq!(
            (f.dialog.bytes, f.dialog.status.as_str()),
            (None, "no decoder")
        );
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
