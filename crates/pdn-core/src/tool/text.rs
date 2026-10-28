//! Port of: paintdotjs js/src/engine/document/tool/tool/TextTool.js (preview/editor structure).
//! Paint.NET: painting-tools-2.md §§2.6,10; ui-text-and-glyphs.md §2.8; compositing.md §2.7.
//! R96: editing preserves UTF-16 units; only rendering projects isolated units to U+FFFD.
//! R97: IME preedit is provisional, not a native managed Text protocol or an undo transaction.
mod color;
mod shape;
use super::types::ToolSettings;
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolBase, ToolType};
use crate::app::AppWorkspace;
use crate::document::layer::{BlendMode, Layer};
use crate::document::surface::Surface;
use crate::document::text_layer::{self, TextSpec};
use crate::document::workspace::DocumentWorkspace;
use crate::fonts::{self, FaceStyle, FontPolicy};
use crate::history::{BitmapMemento, HistoryMemento, MementoKind, TextLayerMemento};
use crate::pixel::{
    IntRect,
    content::{ContentBlendMode, ContentLayer, render_content},
    mask::SelectionQuality,
};
use crate::render::{
    Renderer, RendererId,
    nubs::{Handle, HandleShape},
};
use crate::util::{color::Color, geom::Rectangle, region::Region};
pub use color::{Bitmap, DrawItem, TextRaster, rasterize_text_content, rasterize_text_surface};
use shape::Placement;
pub use shape::TextShape;
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{
        DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
    },
};
use std::sync::{Arc, LazyLock};
use tiny_skia::{Path, PathBuilder, Transform};

/// The serde names are a text layer's stored spec (S§4.5, R43): the toolbar's values, frozen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TextRenderingMode {
    #[serde(rename = "smooth")]
    Smooth,
    #[serde(rename = "sharp")]
    Sharp,
    #[serde(rename = "aliased")]
    Classic,
}
impl TextRenderingMode {
    pub fn from_value(v: &str) -> Self {
        match v {
            "sharp" => Self::Sharp,
            "aliased" => Self::Classic,
            _ => Self::Smooth,
        }
    }
}
/// Stored names as `TextRenderingMode`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FontSizeMetric {
    #[serde(rename = "points")]
    Points,
    #[serde(rename = "pixels")]
    Fixed96Dpi,
}
impl FontSizeMetric {
    pub fn from_value(v: &str) -> Self {
        if v == "pixels" {
            Self::Fixed96Dpi
        } else {
            Self::Points
        }
    }
}
/// Stored names as `TextRenderingMode`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TextAlign {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "center")]
    Center,
    #[serde(rename = "right")]
    Right,
}
/// Serialized as a text layer's stored settings (S§4.5, R43): missing fields take `Default`,
/// unknown ones are ignored.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TextSettings {
    pub family: String,
    pub size: f64,
    pub metric: FontSizeMetric,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub mode: TextRenderingMode,
    pub align: TextAlign,
    pub antialias: bool,
    /// Where the fonts come from: the environment's, never stored.
    #[serde(skip)]
    pub policy: FontPolicy,
    /// Extras (`shape`): the turn in degrees, counter-clockwise about the click point, and the
    /// path the text follows and how strongly it bends (−100…100). Not stored at their defaults,
    /// so straight text stores what it did before them.
    #[serde(skip_serializing_if = "unturned")]
    pub angle: f64,
    #[serde(skip_serializing_if = "straight")]
    pub shape: TextShape,
    #[serde(skip_serializing_if = "default_bend")]
    pub bend: f64,
}
/// `TextSettings::bend`'s default: half a circle, a gentle wave, a spiral of 2.5 turns.
pub const DEFAULT_BEND: f64 = 50.0;
fn unturned(angle: &f64) -> bool {
    *angle == 0.0
}
fn straight(shape: &TextShape) -> bool {
    *shape == TextShape::Straight
}
fn default_bend(bend: &f64) -> bool {
    *bend == DEFAULT_BEND
}
impl TextSettings {
    pub fn from_tool_settings(s: &ToolSettings, policy: FontPolicy) -> Self {
        let get = |k| s.get(ToolType::Text, k);
        Self {
            family: get("fontFamily").as_str().unwrap_or("Calibri").into(),
            size: get("fontSize").as_f64().unwrap_or(12.0),
            metric: FontSizeMetric::from_value(get("fontUnit").as_str().unwrap_or("points")),
            bold: get("bold").as_bool().unwrap_or(false),
            italic: get("italic").as_bool().unwrap_or(false),
            underline: get("underline").as_bool().unwrap_or(false),
            strikeout: get("strikeout").as_bool().unwrap_or(false),
            mode: TextRenderingMode::from_value(get("textRendering").as_str().unwrap_or("smooth")),
            align: match get("align").as_str() {
                Some("center") => TextAlign::Center,
                Some("right") => TextAlign::Right,
                _ => TextAlign::Left,
            },
            antialias: get("antialias").as_bool().unwrap_or(true),
            policy,
            angle: get("textAngle").as_f64().unwrap_or(0.0),
            shape: TextShape::from_value(get("textShape").as_str().unwrap_or("straight")),
            bend: get("textBend").as_f64().unwrap_or(DEFAULT_BEND),
        }
    }
    /// The inverse of `from_tool_settings`, for a text layer whose editing resumes (S§4.5): the
    /// stored names are the toolbar's values.
    pub fn to_tool_settings(&self, s: &mut ToolSettings) {
        let v = serde_json::to_value(self).expect("plain data");
        for (key, field) in [
            ("fontFamily", "family"),
            ("fontSize", "size"),
            ("fontUnit", "metric"),
            ("bold", "bold"),
            ("italic", "italic"),
            ("underline", "underline"),
            ("strikeout", "strikeout"),
            ("textRendering", "mode"),
            ("align", "align"),
            ("antialias", "antialias"),
        ] {
            s.set(ToolType::Text, key, v[field].clone());
        }
        s.set(ToolType::Text, "textAngle", number(self.angle));
        s.set(ToolType::Text, "textShape", self.shape.value().into());
        s.set(ToolType::Text, "textBend", number(self.bend));
    }
}
/// The Text tool's Extra "Editable text" toggle (S§4.5): its commits make text layers.
pub fn editable_text(s: &ToolSettings) -> bool {
    s.get(ToolType::Text, "editableText") == true
}
/// `layout` as drawn: on its shape and turned (`shape`), or as laid out.
pub fn placed(layout: TextLayout, s: &TextSettings, click: [f64; 2]) -> TextLayout {
    match Placement::new(&layout, s, click) {
        Some(p) => p.apply(&layout),
        None => layout,
    }
}
/// A setting's number as the toolbar writes it (whole numbers as integers).
fn number(v: f64) -> serde_json::Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        serde_json::json!(v as i64)
    } else {
        serde_json::json!(v)
    }
}
/// S§4.5, R74: the topmost visible text layer whose rendered text (at its own DPI) lies under `p`,
/// as upstream's `hit_layer`: a Text press there edits it. Its index, spec and space height.
fn text_at(ws: &DocumentWorkspace, p: [f64; 2]) -> Option<(usize, TextSpec, f64)> {
    let layers = &ws.document.layers;
    (0..layers.size()).rev().find_map(|i| {
        let layer = layers.get_at(i).filter(|l| l.properties.visible)?;
        let spec = layer.text.as_ref()?;
        let origin = [spec.origin.0, spec.origin.1];
        let layout = layout_text(&spec.text, &spec.settings, origin, spec.dpi);
        let space_height = layout.space_height;
        // ponytail: a turned text's box is its bounds, so a press beside a slanted line may reach
        // it; test the placed glyph boxes if that gets in the way.
        let layout = placed(layout, &spec.settings, origin);
        let bounds = layout
            .lines
            .iter()
            .map(line_bounds)
            .reduce(|a, b| Rectangle::union(&a, &b))?;
        bounds
            .contains(p[0], p[1])
            .then(|| (i, spec.clone(), space_height))
    })
}
/// R79: text of only whitespace is empty (no layer of its own).
fn blank(text: &str) -> bool {
    text.trim().is_empty()
}
/// A text layer being edited again (S§4.5).
struct Reedit {
    /// Its spec and its pixels before.
    spec: TextSpec,
    surface: Surface,
    /// What re-entry replaced with the spec's in state other tools share: the antialias setting
    /// (R17) and the primary colour.
    antialias: serde_json::Value,
    primary: Color,
}
/// Where document text finds its fonts in this build: the system's natively, the bundled set on
/// the web and in tests.
pub fn document_font_policy() -> FontPolicy {
    if cfg!(any(test, feature = "testing", target_arch = "wasm32")) {
        FontPolicy::Bundled
    } else {
        FontPolicy::System
    }
}
/// The Text tool's defaults (`from_tool_settings` of unset settings) in this build's font policy;
/// a text layer's stored spec takes them for missing fields (S§4.5, R43).
impl Default for TextSettings {
    fn default() -> Self {
        Self {
            family: "Calibri".into(),
            size: 12.0,
            metric: FontSizeMetric::Points,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            mode: TextRenderingMode::Smooth,
            align: TextAlign::Left,
            antialias: true,
            policy: document_font_policy(),
            angle: 0.0,
            shape: TextShape::Straight,
            bend: DEFAULT_BEND,
        }
    }
}
pub fn text_px_size(size: f64, metric: FontSizeMetric, dpi: f64) -> f64 {
    size * if metric == FontSizeMetric::Points {
        dpi
    } else {
        96.0
    } / 72.0
}
pub fn text_top(click_y: f64, space_height: f64) -> f64 {
    (click_y - space_height / 2.0).floor()
}
pub fn line_origin_x(click_x: f64, align: TextAlign, width: f64) -> f64 {
    aligned_x(click_x, align, width).round()
}
fn aligned_x(click_x: f64, align: TextAlign, width: f64) -> f64 {
    click_x
        + match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -width / 2.0,
            TextAlign::Right => -width,
        }
}
// Paint.NET maps a text position to a point in float arithmetic, then returns a double point.
fn logical_position(
    click: [f64; 2],
    prefix: [f64; 2],
    width: f64,
    row: usize,
    align: TextAlign,
) -> [f64; 2] {
    let offset = match align {
        TextAlign::Left => prefix[0] as f32,
        TextAlign::Center => prefix[0] as f32 - width as f32 / 2.0,
        TextAlign::Right => prefix[0] as f32 - width as f32,
    };
    [
        (click[0] as f32 + offset) as f64,
        (click[1] as f32 + prefix[1] as f32 * row as f32) as f64,
    ]
}
/// Resolved glyph metadata in line-relative coordinates; source spans count UTF-16 units.
#[derive(Clone, Debug)]
pub struct TextGlyph {
    pub utf16: std::ops::Range<usize>,
    pub face: Arc<[u8]>,
    pub glyph_id: u16,
    pub origin: [f64; 2],
    pub advance: f64,
    pub ink: Option<Rectangle>,
}
#[derive(Clone, Debug)]
pub struct TextLine {
    pub text: String,
    pub origin: [f64; 2],
    pub width: f64,
    pub height: f64,
    pub advances: Vec<f64>,
    pub items: Vec<DrawItem>,
    pub glyphs: Vec<TextGlyph>,
    clip: Rectangle,
    /// The glyph (index in `glyphs`) each of `items` draws, for `shape`'s placement.
    item_glyphs: Vec<usize>,
    /// The baseline's distance below `origin`.
    baseline: f64,
}
#[derive(Clone, Debug)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub space_height: f64,
}

// Resolve document fonts independently of R47's always-bundled chrome.
static SYSTEM_FILES: LazyLock<Vec<std::path::PathBuf>> =
    LazyLock::new(|| fonts::system_font_files(FontPolicy::System));
fn faces(s: &TextSettings) -> Vec<Arc<[u8]>> {
    let style = match (s.bold, s.italic) {
        (true, true) => FaceStyle::BoldItalic,
        (true, false) => FaceStyle::Bold,
        (false, true) => FaceStyle::Italic,
        _ => FaceStyle::Regular,
    };
    let candidates = if s.policy == FontPolicy::System && !cfg!(target_arch = "wasm32") {
        SYSTEM_FILES.as_slice()
    } else {
        &[]
    };
    let ui: Arc<[u8]> = fonts::ui_face_from(candidates, s.policy, style)
        .bytes
        .into_owned()
        .into();
    let primary = if s.family.eq_ignore_ascii_case("Calibri")
        || s.family.eq_ignore_ascii_case("Carlito")
    {
        fonts::calibri_face_from(candidates, s.policy, style)
            .bytes
            .into_owned()
            .into()
    } else if s.family.eq_ignore_ascii_case("Segoe UI") || s.family.eq_ignore_ascii_case("Selawik")
    {
        ui.clone()
    } else {
        fonts::registered_face(&s.family, style).unwrap_or_else(|| ui.clone())
    };
    // R183 follow-up: after Noto Sans, the RTL faces and every registered script fallback;
    // the colour face, when registered, stays last.
    static STATIC_FALLBACKS: LazyLock<[Arc<[u8]>; 3]> = LazyLock::new(|| {
        use crate::canvas::text::{NOTO_SANS, NOTO_SANS_ARABIC, NOTO_SANS_HEBREW};
        [NOTO_SANS, NOTO_SANS_ARABIC, NOTO_SANS_HEBREW].map(Arc::from)
    });
    let mut result = vec![primary, ui];
    result.extend(STATIC_FALLBACKS.iter().cloned());
    result.extend(
        fonts::SCRIPT_FALLBACKS
            .iter()
            .filter_map(|(family, ..)| fonts::registered_face(family, FaceStyle::Regular)),
    );
    if let Some(color) = fonts::registered_face("Noto Color Emoji", FaceStyle::Regular) {
        result.push(color);
    }
    result
}
/// The faces before a trailing colour face (`faces` pushes it last; tests push it at 3).
fn ordinary_faces(ttf: &[ttf_parser::Face]) -> usize {
    let n = ttf.len();
    n - usize::from(n > 3 && ttf[n - 1].tables().cbdt.is_some())
}
fn metrics(bytes: &[u8], px: f64) -> (f64, f64) {
    let f = ttf_parser::Face::parse(bytes, 0).expect("validated font");
    let k = px / f.units_per_em() as f64;
    if let Some(o) = f.tables().os2 {
        if o.use_typographic_metrics() {
            return (
                o.typographic_ascender() as f64 * k,
                (o.typographic_ascender() as f64 - o.typographic_descender() as f64
                    + o.typographic_line_gap() as f64)
                    * k,
            );
        }
        return (
            o.windows_ascender() as f64 * k,
            (o.windows_ascender() as f64 - o.windows_descender() as f64) * k,
        );
    }
    (
        f.ascender() as f64 * k,
        (f.ascender() as f64 - f.descender() as f64 + f.line_gap() as f64) * k,
    )
}
/// quirk(pdn): painting-tools-2.md §10.6: if unit zero is uncovered, the entire range-assignment
/// scan is skipped.
fn fallback_units(units: &[u16], covered: impl Fn(u16) -> bool) -> Vec<bool> {
    let mut out = vec![false; units.len()];
    let mut i = 0;
    while i < units.len() && covered(units[i]) {
        i += 1;
        while i < units.len() && !covered(units[i]) {
            out[i] = true;
            i += 1;
        }
    }
    out
}
struct Pen(PathBuilder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, -y)
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, -y)
    }
    fn quad_to(&mut self, x: f32, y: f32, x1: f32, y1: f32) {
        self.0.quad_to(x, -y, x1, -y1)
    }
    fn curve_to(&mut self, x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.0.cubic_to(x, -y, x1, -y1, x2, -y2)
    }
    fn close(&mut self) {
        self.0.close()
    }
}
fn line(units: &[u16], s: &TextSettings, px: f64, bytes: &[Arc<[u8]>]) -> TextLine {
    if units.is_empty() {
        // Text measurement: only height comes from the full space layout; width and
        // all overhangs are zero for both empty lines and independently measured prefixes.
        let space = line(&[32], s, px, bytes);
        let height = space.height;
        return TextLine {
            text: String::new(),
            origin: [0.0, 0.0],
            width: 0.0,
            height,
            advances: vec![],
            items: vec![],
            glyphs: vec![],
            clip: Rectangle::absolute(-1.0, -1.0, 1.0, (height + 1.0).ceil()),
            item_glyphs: vec![],
            baseline: space.baseline,
        };
    }
    let fs: Vec<_> = bytes
        .iter()
        .map(|b| FontRef::from_index(b, 0).expect("validated font"))
        .collect();
    let ttf: Vec<_> = bytes
        .iter()
        .map(|b| ttf_parser::Face::parse(b, 0).expect("validated font"))
        .collect();
    let ordinary = ordinary_faces(&ttf);
    let covered = |u| char::from_u32(u as u32).is_some_and(|c| fs[0].charmap().map(c).is_some());
    let fallback = fallback_units(units, covered);
    let glyphs: Vec<_> = fs.iter().map(|f| f.outline_glyphs()).collect();
    let hints: Vec<_> = glyphs
        .iter()
        .map(|g| {
            if s.mode != TextRenderingMode::Smooth || !s.antialias {
                HintingInstance::new(
                    g,
                    Size::new(px as f32),
                    LocationRef::default(),
                    HintingOptions {
                        engine: Engine::Interpreter,
                        target: Target::Smooth {
                            mode: SmoothMode::Normal,
                            symmetric_rendering: true,
                            preserve_linear_metrics: true,
                        },
                    },
                )
                .ok()
            } else {
                None
            }
        })
        .collect();
    let mut items = vec![];
    let mut resolved = vec![];
    let mut advances = vec![];
    let mut width = 0.0;
    let (mut ascent, mut height) = metrics(&bytes[0], px);
    let decoded: Vec<_> = char::decode_utf16(units.iter().copied())
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    let mut unit = 0;
    let mut positioned = vec![];
    let mut bitmap_budget = color::MAX_RUN_BYTES;
    let mut bitmaps = std::collections::HashMap::new();
    for (ci, &c) in decoded.iter().enumerate() {
        #[cfg(test)]
        LINE_GLYPHS.with(|n| n.set(n.get() + 1));
        // Selectors and the Calendar trailing zero-width space have no ink or advance.
        // They remain untouched in the UTF-16 editor buffer. No GSUB/ZWJ shaping is added.
        if matches!(c, '\u{fe0e}' | '\u{fe0f}' | '\u{200b}') {
            advances.push(0.0);
            unit += c.len_utf16();
            continue;
        }
        let selector = decoded.get(ci + 1).copied();
        let assigned = fallback.get(unit).copied().unwrap_or(false);
        let mut fi = if assigned { 1 } else { 0 };
        // R117: colour is decided by the original trio (primary, UI, Noto Sans); the RTL and
        // script faces only fill what it lacks, never a colour emoji such as ♥ or ㊗.
        let lacks = fs[fi].charmap().map(c).is_none() && fs[2].charmap().map(c).is_none();
        if fs[fi].charmap().map(c).is_none() {
            fi = (2..ordinary)
                .find(|&i| fs[i].charmap().map(c).is_some())
                .unwrap_or(2);
        }
        let ordinary_fi = fi;
        let text_presentation = selector == Some('\u{fe0e}');
        let wants_color = !text_presentation && (selector == Some('\u{fe0f}') || lacks);
        if wants_color && fs.len() > ordinary && fs[ordinary].charmap().map(c).is_some() {
            fi = ordinary;
        }
        if text_presentation && ttf[fi].tables().cbdt.is_some() {
            fi = (0..ordinary)
                .find(|&i| ttf[i].tables().cbdt.is_none() && fs[i].charmap().map(c).is_some())
                .unwrap_or(2);
        }
        let mut gid = fs[fi].charmap().map(c).unwrap_or_default();
        let bitmap = bitmaps
            .entry((fi, gid.to_u32()))
            .or_insert_with(|| {
                Bitmap::from_glyph(&ttf[fi], gid.to_u32() as u16, px, 0.0, &mut bitmap_budget)
            })
            .clone()
            .map(|mut bitmap| {
                bitmap.origin[0] += width;
                bitmap
            });
        // A cmap hit is insufficient: a malformed/unsupported bitmap must retain the
        // ordinary missing-glyph path and metrics, rather than turn the range invisible.
        if bitmap.is_none()
            && ttf[fi].tables().cbdt.is_some()
            && !c.is_whitespace()
            && c != '\u{200d}'
            && glyphs[fi].get(gid).is_none()
        {
            fi = if ttf[ordinary_fi].tables().cbdt.is_none() {
                ordinary_fi
            } else {
                2
            };
            gid = fs[fi].charmap().map(c).unwrap_or_default();
        }
        let font = &fs[fi];
        let f = &ttf[fi];
        let mut advance = font
            .glyph_metrics(Size::new(px as f32), LocationRef::default())
            .advance_width(gid)
            .unwrap_or(0.0) as f64;
        if s.mode == TextRenderingMode::Classic || !s.antialias {
            advance = advance.round();
        }
        let (a, h) = metrics(&bytes[fi], px);
        ascent = ascent.max(a);
        height = height.max(h);
        let item_start = positioned.len();
        let gi = resolved.len();
        let mut pen = Pen(PathBuilder::new());
        if let Some(bitmap) = bitmap {
            positioned.push((gi, DrawItem::Bitmap(bitmap)));
        } else if let Some(g) = glyphs[fi].get(gid) {
            let settings = hints[fi].as_ref().map_or_else(
                || DrawSettings::unhinted(Size::new(px as f32), LocationRef::default()),
                |h| DrawSettings::hinted(h, false),
            );
            if g.draw(settings, &mut pen).is_ok()
                && let Some(path) = pen.0.finish()
            {
                // R111 portable synthetic style coefficients; no native raster parity claim.
                let skew = if s.italic && !f.is_italic() && !f.is_oblique() {
                    -0.2
                } else {
                    0.0
                };
                if let Some(path) =
                    path.transform(Transform::from_row(1.0, 0.0, skew, 1.0, width as f32, 0.0))
                {
                    // Synthetic weight only when an actual bold face is unavailable.
                    if s.bold
                        && f.weight().to_number() <= 600
                        && let Some(outline) = path.stroke(
                            &tiny_skia::Stroke {
                                width: (px / 24.0) as f32,
                                ..Default::default()
                            },
                            1.0,
                        )
                    {
                        positioned.push((gi, DrawItem::Outline(outline)));
                    }
                    positioned.push((gi, DrawItem::Outline(path)));
                }
            }
        }
        let scale = px / f.units_per_em() as f64;
        for decoration in [
            s.underline.then(|| f.underline_metrics()).flatten(),
            s.strikeout.then(|| f.strikeout_metrics()).flatten(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(r) = tiny_skia::Rect::from_xywh(
                width as f32,
                (-decoration.position as f64 * scale) as f32,
                advance as f32,
                (decoration.thickness as f64 * scale) as f32,
            ) {
                positioned.push((gi, DrawItem::Outline(PathBuilder::from_rect(r))));
            }
        }
        resolved.push(TextGlyph {
            utf16: unit..unit
                + c.len_utf16()
                + usize::from(matches!(selector, Some('\u{fe0e}' | '\u{fe0f}'))),
            face: bytes[fi].clone(),
            glyph_id: gid.to_u32() as u16,
            origin: [width, 0.0],
            advance,
            ink: positioned[item_start..]
                .iter()
                .map(|(_, item)| item.bounds())
                .reduce(|a, b| Rectangle::union(&a, &b)),
        });
        width += advance;
        advances.push(advance);
        unit += c.len_utf16();
    }
    let mut bounds = Rectangle::new(0.0, 0.0, width, height);
    let mut item_glyphs = vec![];
    for (gi, item) in positioned {
        if let Some(item) = item.translate_y(ascent) {
            bounds = Rectangle::union(&bounds, &item.bounds());
            items.push(item);
            item_glyphs.push(gi);
        }
    }
    for glyph in &mut resolved {
        glyph.origin[1] = ascent;
        if let Some(ink) = &mut glyph.ink {
            ink.y += ascent;
        }
    }
    let clip = Rectangle::absolute(
        (bounds.left() - 1.0).floor(),
        (bounds.top() - 1.0).floor(),
        (bounds.right() + 1.0).ceil(),
        (bounds.bottom() + 1.0).ceil(),
    );
    TextLine {
        text: String::from_utf16_lossy(units),
        origin: [0.0, 0.0],
        width: width as f32 as f64,
        height: height as f32 as f64,
        advances,
        items,
        glyphs: resolved,
        clip,
        item_glyphs,
        baseline: ascent,
    }
}
/// R111 defensive adapter fallback, not a native em-size clamp.
fn portable_text_size(px: f64) -> f64 {
    const INVALID_SIZE_FALLBACK: f32 = 0.01;
    let px = px as f32;
    if px.is_finite() && px > 0.0 {
        px as f64
    } else {
        INVALID_SIZE_FALLBACK as f64
    }
}
pub fn valid_font_size(value: f64) -> bool {
    value.is_finite() && (1.0..=2000.0).contains(&value)
}
fn text_nub_position(
    origin: [f64; 2],
    size: [f64; 2],
    dpi_scale: f64,
    zoom: f64,
    layer: [f64; 2],
) -> [f64; 2] {
    let q = ((zoom * 10000.0).floor() / 10000.0).clamp(0.01, 100.0);
    let offset = (10.0 * dpi_scale / q).trunc();
    std::array::from_fn(|i| {
        (origin[i] + size[i].ceil() + offset)
            .min(layer[i] - 13.0)
            .round()
            .max(13.0)
            .round()
    })
}
fn nub_contains(center: [f64; 2], point: [f64; 2], dpi_scale: f64, zoom: f64) -> bool {
    // R111: analytical bounds of the default 6.5-radius, thickness-1 Compass drawing.
    let r = 7.0 * dpi_scale / zoom;
    (0..2).all(|i| point[i] >= center[i] - r && point[i] < center[i] + r)
}
fn text_visibility_center(view: Rectangle, point: [f64; 2], height: f64) -> Option<[f64; 2]> {
    let bounds = [
        view.left().floor(),
        view.top().floor(),
        view.right().ceil(),
        view.bottom().ceil(),
    ];
    let k = point.map(f64::floor);
    let side = height.round();
    if k[0] >= bounds[0]
        && k[1] >= bounds[1]
        && k[0] + side <= bounds[2]
        && k[1] + side <= bounds[3]
    {
        return None;
    }
    Some(std::array::from_fn(|i| {
        if k[i] < bounds[i] || k[i] > bounds[i + 2] {
            k[i]
        } else {
            (bounds[i] + bounds[i + 2]) / 2.0
        }
    }))
}
fn layout_units(lines: &[Vec<u16>], s: &TextSettings, click: [f64; 2], dpi: f64) -> TextLayout {
    layout_units_px(lines, s, click, text_px_size(s.size, s.metric, dpi))
}
fn layout_units_px(lines: &[Vec<u16>], s: &TextSettings, click: [f64; 2], px: f64) -> TextLayout {
    let px = portable_text_size(px);
    let bytes = faces(s);
    let space_height = line(&[32], s, px, &bytes).height;
    let mut y = text_top(click[1], space_height);
    let lines = lines
        .iter()
        .map(|units| {
            let mut l = line(units, s, px, &bytes);
            l.origin = [line_origin_x(click[0], s.align, l.width), y.round()];
            y += l.height;
            l
        })
        .collect();
    TextLayout {
        lines,
        space_height,
    }
}
/// Appendix B B-P16 calibrates the line height and decorations.
pub fn layout_text(text: &str, s: &TextSettings, click: [f64; 2], dpi: f64) -> TextLayout {
    layout_units(
        &text
            .split('\n')
            .map(|l| l.encode_utf16().collect())
            .collect::<Vec<_>>(),
        s,
        click,
        dpi,
    )
}
/// Layout with an explicit pixel em size. Unlike Fixed96Dpi's point setting, 12 means 12px.
/// Shares Text's fallback, baseline, alignment, glyph order and decorations.
pub fn layout_text_px(text: &str, s: &TextSettings, click: [f64; 2], px: f64) -> TextLayout {
    layout_units_px(
        &text
            .split('\n')
            .map(|l| l.encode_utf16().collect())
            .collect::<Vec<_>>(),
        s,
        click,
        px,
    )
}

#[cfg(test)]
thread_local! {
    /// Scalars `line` has laid out on this thread (tests bound layout work with it).
    pub(crate) static LINE_GLYPHS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// R158 Q68, with the faces resolved once: the width `line` measures for each raw prefix
/// `units[..k]` (k < units.len()), and each of `lines` as one logical line at an explicit pixel em
/// (no LF split, no click origin: origins 0, 0; a valid pair decodes, an isolated half is U+FFFD).
pub fn layout_raw_units_px(
    units: &[u16],
    lines: &[Vec<u16>],
    s: &TextSettings,
    px: f64,
) -> (Vec<f64>, Vec<TextLine>) {
    let px = portable_text_size(px);
    let bytes = faces(s);
    let widths = prefix_widths(units, s, px, &bytes);
    (
        widths,
        lines.iter().map(|u| line(u, s, px, &bytes)).collect(),
    )
}

/// `line(units[..k]).width` for every k < units.len(), from one line of all of them. With no
/// kerning or shaping, and decoding, the fallback scan and the bitmap budget all running left to
/// right, a prefix's scalars advance as in the whole line, except its last when the prefix splits
/// a pair (U+FFFD) or stops before U+FE0E / U+FE0F (no selector follows it). That advance comes
/// from the scalar measured after the line's first unit alone (the fallback scan assigns a unit
/// from the first unit's coverage and its own), and the f64 sum is rounded through f32 as `line`
/// rounds its width.
fn prefix_widths(units: &[u16], s: &TextSettings, px: f64, bytes: &[Arc<[u8]>]) -> Vec<f64> {
    if units.is_empty() {
        return vec![];
    }
    let full = line(units, s, px, bytes);
    // With a glyph's worth of bitmap budget left after the whole line, no prefix's last scalar
    // meets the budget differently.
    let mut seen = std::collections::HashSet::new();
    let used: usize = full
        .items
        .iter()
        .filter_map(|i| match i {
            DrawItem::Bitmap(b) => seen
                .insert(Arc::as_ptr(&b.image))
                .then(|| b.image.data.len()),
            DrawItem::Outline(_) => None,
        })
        .sum();
    let glyph = (color::MAX_GLYPH_SIDE as usize).pow(2) * 4;
    let roomy = used + glyph <= color::MAX_RUN_BYTES;
    let high = |u: u16| (0xd800..0xdc00).contains(&u);
    let low = |u: u16| (0xdc00..0xe000).contains(&u);
    // Each scalar's first unit and the advances summed before it, in `line`'s order.
    let (mut starts, mut unit, mut sum) = (Vec::with_capacity(units.len()), 0, 0.0f64);
    for (r, advance) in char::decode_utf16(units.iter().copied()).zip(&full.advances) {
        starts.push((unit, sum));
        unit += r.map_or(1, char::len_utf16);
        sum += advance;
    }
    drop(full);
    let width = |w: f64| w as f32 as f64;
    let mut c = 0;
    (0..units.len())
        .map(|k| {
            while c < starts.len() && starts[c].0 < k {
                c += 1;
            }
            let split = k > 0 && high(units[k - 1]) && low(units[k]);
            let before_selector = k > 0 && matches!(units[k], 0xfe0e | 0xfe0f);
            if !split && !before_selector {
                return width(starts[c].1);
            }
            if !roomy {
                // ponytail: re-measures such a prefix whole past ~15 MB of colour bitmaps in one
                // text (hundreds of distinct emoji); thread the budget through `line` if needed.
                return line(&units[..k], s, px, bytes).width;
            }
            let (j, before) = starts[c - 1];
            let mut context = Vec::with_capacity(3);
            if j > 0 {
                // The same covered/uncovered first unit, never pairing with the scalar.
                context.push(if high(units[0]) { 0xdc00 } else { units[0] });
            }
            context.extend_from_slice(&units[j..k]);
            let last = line(&context, s, px, bytes).advances.last().copied();
            width(before + last.unwrap_or(0.0))
        })
        .collect()
}

/// Whether this document text needs the lazy bundled color face. Ordinary text does not fetch it.
pub fn needs_color_font(text: &str, s: &TextSettings) -> bool {
    let bytes = faces(s);
    let fs: Vec<_> = bytes[..3]
        .iter()
        .filter_map(|b| FontRef::from_index(b, 0).ok())
        .collect();
    let chars: Vec<_> = text.chars().collect();
    chars.iter().enumerate().any(|(i, &c)| {
        color::bundled_bitmap_scalar(c)
            && chars.get(i + 1) != Some(&'\u{fe0e}')
            && (chars.get(i + 1) == Some(&'\u{fe0f}')
                || !fs.iter().any(|f| f.charmap().map(c).is_some()))
    })
}
/// R183 follow-up: the lazy `fonts::SCRIPT_FALLBACKS` families this document text needs, for
/// characters no face it already has covers.
pub fn script_fallbacks(text: &str, s: &TextSettings) -> Vec<&'static str> {
    let bytes = faces(s);
    let fs: Vec<_> = bytes
        .iter()
        .filter_map(|b| FontRef::from_index(b, 0).ok())
        .collect();
    let mut out = vec![];
    let chars: Vec<_> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        // R117: a colour-capable scalar stays with the colour face unless VS15 asks for text.
        if color::bundled_bitmap_scalar(c) && chars.get(i + 1) != Some(&'\u{fe0e}') {
            continue;
        }
        if let Some(family) = fonts::script_fallback(c)
            && !out.contains(&family)
            && !fs.iter().any(|f| f.charmap().map(c).is_some())
        {
            out.push(family);
        }
    }
    out
}
pub(crate) fn line_bounds(l: &TextLine) -> Rectangle {
    Rectangle::new(
        l.origin[0] + l.clip.x,
        l.origin[1] + l.clip.y,
        l.clip.width,
        l.clip.height,
    )
}
pub fn rasterize_text(layout: &TextLayout, s: &TextSettings, rect: IntRect) -> Vec<u8> {
    if layout
        .lines
        .iter()
        .any(|l| l.items.iter().any(|i| matches!(i, DrawItem::Bitmap(_))))
    {
        return rasterize_text_content(
            layout,
            s,
            rect,
            [255; 4],
            crate::effect::surface::Space::Srgb,
        )
        .coverage;
    }
    if rect.is_empty() {
        return vec![];
    }
    let Some(mut pm) = tiny_skia::Pixmap::new(rect.w, rect.h) else {
        return vec![];
    };
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = s.antialias;
    for l in &layout.lines {
        let b = line_bounds(l);
        let clip =
            IntRect::new(b.x as i32, b.y as i32, b.width as u32, b.height as u32).intersect(&rect);
        if clip.is_empty() {
            continue;
        }
        let mut mask = tiny_skia::Mask::new(rect.w, rect.h).expect("same size as pixmap");
        for y in clip.y..clip.bottom() {
            let start = ((y - rect.y) as usize * rect.w as usize) + (clip.x - rect.x) as usize;
            mask.data_mut()[start..start + clip.w as usize].fill(255);
        }
        let tr = Transform::from_translate(
            (l.origin[0] - rect.x as f64) as f32,
            (l.origin[1] - rect.y as f64) as f32,
        );
        for item in &l.items {
            if let DrawItem::Outline(p) = item {
                pm.fill_path(p, &paint, tiny_skia::FillRule::Winding, tr, Some(&mask));
            }
        }
    }
    pm.data().as_chunks::<4>().0.iter().map(|p| p[3]).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum EditMode {
    #[default]
    None,
    Empty,
    Editing,
}
// R109: monotonic timer and cubic fade approximate the platform timer/Windows animation.
struct TextPulse {
    epoch: f64,
    last_tick: f64,
    ticks: u64,
    caret_target: bool,
    from: f64,
    to: f64,
    fade_start: f64,
    nub_visible: bool,
    nub_opacity: f64,
}
impl Default for TextPulse {
    fn default() -> Self {
        Self {
            epoch: 0.0,
            last_tick: 0.0,
            ticks: 0,
            caret_target: false,
            from: 1.0,
            to: 1.0,
            fade_start: 0.0,
            nub_visible: false,
            nub_opacity: 1.0,
        }
    }
}
impl TextPulse {
    fn opacity(&self, now: f64) -> f64 {
        self.from
            + (self.to - self.from)
                * crate::render::color_ui::smooth_stop((now - self.fade_start) / 100.0)
    }
    fn stop(&mut self, now: f64) {
        self.to = self.opacity(now);
        self.from = self.to;
        self.nub_visible = false;
    }
    fn sample(
        &mut self,
        now: f64,
        focused: bool,
        active: bool,
        ctrl: bool,
        tracking: bool,
        animations: bool,
    ) {
        let elapsed = now - self.epoch;
        let on = (elapsed / 300.0).floor() as i64 % 2 == 0 && focused && !(active && ctrl);
        if on != self.caret_target {
            self.caret_target = on;
            let to = f64::from(on);
            if self.to != to {
                self.from = if animations { self.opacity(now) } else { to };
                self.to = to;
                self.fade_start = now;
            }
        }
        self.nub_visible = !tracking && !(active && ctrl);
        self.nub_opacity = (0.75
            + 0.5
                * (std::f64::consts::TAU * (elapsed / 2000.0 % 1.0))
                    .sin()
                    .min(0.5))
        .clamp(0.0, 1.0);
    }
}
#[derive(Default)]
pub struct TextTool {
    base: ToolBase,
    mode: EditMode,
    lines: Vec<Vec<u16>>,
    caret: (usize, usize),
    click: [f64; 2],
    compound: Option<u64>,
    affected: Option<Rectangle>,
    layout: Option<TextLayout>,
    composition: Option<Vec<u16>>,
    composition_cursor: usize,
    pub interrupt_composition: bool,
    drag: Option<([f64; 2], [f64; 2])>,
    handles: Option<RendererId>,
    pulse: TextPulse,
    modifier_keys: [bool; 2],
    workspace_focused: bool,
    cursor_name: Option<&'static str>,
    status: Option<String>,
    environment: Option<[f64; 5]>,
    font_generation: u64,
    /// Editable text (S§4.5) was on when this text began: its commit makes a text layer.
    editable: bool,
    /// Editing a text layer (the active one) again.
    reedit: Option<Reedit>,
    /// The active layer is new text's own, not yet in history (`add_preview_layer`).
    preview: bool,
    /// Extra (`shape`): the drawn text's bounds while it is placed on a shape or turned.
    placed_bounds: Option<Rectangle>,
    /// The rotate handle and the turn's pivot (the text's middle), as last drawn.
    turn_handle: Option<([f64; 2], [f64; 2])>,
    /// The angle the text was last drawn at.
    angle: f64,
    /// A rotate-handle drag.
    turning: Option<Turning>,
    /// Extra (paint.rs; Paint.NET's Text tool has a caret only): the selection's other end, the
    /// caret being one; a press selecting (its drag moves the caret); the last press in the text
    /// (time, point) for a double-click's word; and text Ctrl+C / Ctrl+X took, for the app's
    /// clipboard (`take_copied`).
    anchor: Option<(usize, usize)>,
    selecting: bool,
    last_press: Option<(f64, [f64; 2])>,
    copied: Option<String>,
}
/// A rotate-handle drag: its pivot, the pointer's screen angle at the press (radians), and the
/// text's angle and click point then.
#[derive(Clone, Copy)]
struct Turning {
    pivot: [f64; 2],
    from: f64,
    angle: f64,
    click: [f64; 2],
}
/// The rotate handle's distance above the text, in screen px.
const TURN_GAP: f64 = 24.0;
impl TextTool {
    pub fn is_editing(&self) -> bool {
        self.mode != EditMode::None
    }
    pub fn can_finish(&self) -> bool {
        self.mode == EditMode::Editing
    }
    /// R109 portable timer delivery. Focus predicates remain separate from pulse policy.
    pub fn update_pulse(
        &mut self,
        app: &mut AppWorkspace,
        focused: bool,
        active: bool,
        minimized: bool,
        animations: bool,
    ) {
        self.sync_environment(app);
        self.workspace_focused = focused;
        if !self.is_editing() || minimized || app.now_ms - self.pulse.last_tick < 16.0 {
            return;
        }
        self.pulse.last_tick = app.now_ms;
        self.pulse.ticks = self.pulse.ticks.wrapping_add(1);
        if active || self.pulse.ticks.is_multiple_of(4) {
            self.pulse.sample(
                app.now_ms,
                focused,
                active,
                app.control_key_down,
                self.drag.is_some(),
                animations,
            );
            self.sync_handle(app);
        }
    }
    /// Native brush-cache opacity reduction first narrows to float32, then truncates to byte.
    pub fn caret_alpha(&self, now: f64) -> u8 {
        ((self.pulse.opacity(now) as f32 as f64) * 255.0).clamp(0.0, 255.0) as u8
    }
    fn settings(app: &AppWorkspace) -> TextSettings {
        TextSettings::from_tool_settings(&app.tool_settings, document_font_policy())
    }
    fn start(&mut self, app: &mut AppWorkspace, click: [f64; 2]) {
        self.commit(app);
        self.pulse.epoch = app.now_ms;
        self.pulse.last_tick = app.now_ms;
        self.pulse.nub_visible = true;
        self.modifier_keys = [app.control_key_down, app.shift_key_down];
        // S§4.5: with Editable text, a click on a text layer's text edits it again, in its own
        // settings and colour.
        self.editable = editable_text(&app.tool_settings);
        let target = app
            .active_ws()
            .filter(|_| self.editable)
            .and_then(|ws| text_at(ws, click));
        let shared = (
            app.tool_settings.get(ToolType::Text, "antialias"),
            app.primary_color,
        );
        if let Some((_, spec, _)) = &target {
            spec.settings.to_tool_settings(&mut app.tool_settings);
            let [r, g, b, a] = spec.color;
            app.set_colors(Color::new(r, g, b, a), app.secondary_color);
        }
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        if let Some((i, spec, _)) = &target {
            // R74: the press makes its layer active (no history item, as in Paint.NET). The layer
            // shows the edit alone while it runs; `commit` renders the result.
            ws.set_active_layer(*i);
            let layer = ws.document.layers.get_at_mut(*i).expect("the hit layer");
            let blank = Surface::create(layer.surface.width(), layer.surface.height());
            let surface = std::mem::replace(&mut layer.surface, blank);
            let (antialias, primary) = shared;
            self.reedit = Some(Reedit {
                spec: spec.clone(),
                surface,
                antialias,
                primary,
            });
            ws.invalidate(None);
        }
        // New editable text has no layer before its first keystroke (`promote`).
        if !self.editable || self.reedit.is_some() {
            self.base.begin_bitmap_transaction(ws);
        }
        self.mode = EditMode::Empty;
        self.lines = vec![vec![]];
        self.caret = (0, 0);
        (self.anchor, self.selecting) = (None, false);
        self.click = click;
        if self.handles.is_none() {
            self.handles = Some(ws.surface_box.add_renderer(Renderer::Handles(vec![])));
        }
        if let Some((_, spec, h)) = target {
            self.mode = EditMode::Editing;
            self.lines = spec
                .text
                .split('\n')
                .map(|l| l.encode_utf16().collect())
                .collect();
            self.click = [spec.origin.0, spec.origin.1];
            // The caret goes to the click, as a click inside the text being typed puts it.
            self.hit_position(app, click[0], click[1] + h / 2.0);
        }
        self.redraw(app);
    }
    fn promote(&mut self, app: &mut AppWorkspace) {
        if self.mode == EditMode::Empty {
            let m = HistoryMemento::new(
                "Text",
                &ToolType::Text.icon_src(),
                MementoKind::Compound(vec![]),
            );
            self.compound = Some(m.id);
            if let Some(ws) = app.active_ws_mut() {
                ws.push_memento(m);
            }
            self.mode = EditMode::Editing;
        }
    }
    /// New editable text's own layer (S§4.5): there exactly while the text has a non-whitespace
    /// character, so it previews as it will land whatever the layer below shows, and blank text
    /// has none (R76, R79).
    /// It comes and goes with the keystrokes, not the click or the commit, so a command that
    /// picked a layer by index before committing the text still finds it.
    fn sync_preview_layer(&mut self, app: &mut AppWorkspace) {
        if !self.editable || self.reedit.is_some() {
            return;
        }
        let text = String::from_utf16_lossy(&self.preview_lines().concat());
        let want = self.mode == EditMode::Editing && !blank(&text);
        if want && !self.preview {
            self.add_preview_layer(app);
        } else if !want {
            self.drop_preview_layer(app);
        }
    }
    /// Inserts the preview layer above the active one, with no history item until `commit`
    /// records it.
    fn add_preview_layer(&mut self, app: &mut AppWorkspace) {
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let n = (ws.document.layers.size() + 1).to_string();
        let name = crate::i18n::tf("addNewBlankLayer.layerName.format", &[&n]);
        let layer = Layer::create_layer(ws.document.width, ws.document.height, &name);
        ws.document.insert_layer_at(ws.active_layer + 1, layer);
        ws.sync_after_change();
        self.base.begin_bitmap_transaction(ws);
        self.preview = true;
    }
    fn drop_preview_layer(&mut self, app: &mut AppWorkspace) {
        if !std::mem::take(&mut self.preview) {
            return;
        }
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        self.base.cancel_bitmap_transaction(ws);
        ws.document.remove_layer_at(ws.active_layer);
        ws.sync_after_change();
    }
    /// The resolution the text lays out at: a re-edited text layer's own, so an unchanged edit
    /// after a DPI-only Resize stays unchanged; else the document's.
    fn dpi(&self, app: &AppWorkspace) -> f64 {
        match &self.reedit {
            Some(r) => r.spec.dpi,
            None => app.active_ws().map_or(96.0, |ws| ws.document.resolution),
        }
    }
    fn character(&mut self, app: &mut AppWorkspace, u: u16) {
        self.promote(app);
        if u < 32 || (127..=159).contains(&u) {
            return;
        }
        self.lines[self.caret.0].insert(self.caret.1, u);
        self.caret.1 += 1;
    }
    fn newline(&mut self) {
        let tail = self.lines[self.caret.0].split_off(self.caret.1);
        self.caret.0 += 1;
        self.lines.insert(self.caret.0, tail);
        self.caret.1 = 0;
    }
    pub fn insert_text(&mut self, app: &mut AppWorkspace, text: &str) {
        if !self.is_editing() || self.drag.is_some() {
            return;
        }
        self.cancel_composition();
        // Typing replaces the selection.
        self.delete_selection();
        for u in text.encode_utf16() {
            self.character(app, u);
        }
        self.redraw(app);
    }
    pub fn paste(&mut self, app: &mut AppWorkspace, text: &str) -> bool {
        if !self.is_editing() || text.is_empty() {
            return false;
        }
        self.cancel_composition();
        if self.drag.is_none() {
            self.delete_selection();
        }
        for u in text.encode_utf16() {
            if u == 10 {
                self.newline();
            } else if self.drag.is_none() {
                self.character(app, u);
            } else if self.workspace_focused {
                // quirk(pdn): painting-tools-2.md §10.8: declined pasted units reach focused base
                // character shortcuts.
                match u {
                    88 | 120 => app.swap_colors(),
                    67 | 99 => app.fire(crate::app::AppEvent::ToggleColorSlot),
                    _ => {}
                }
            }
        }
        self.pulse.nub_visible = true;
        self.redraw(app);
        true
    }
    fn cancel_composition(&mut self) {
        if self.composition.take().is_some() {
            self.interrupt_composition = true;
        }
    }
    pub fn ime_preedit(&mut self, app: &mut AppWorkspace, text: &str) {
        self.ime_preedit_range(app, text, None);
    }
    pub fn ime_preedit_range(
        &mut self,
        app: &mut AppWorkspace,
        text: &str,
        range: Option<std::ops::Range<usize>>,
    ) {
        if !self.is_editing() || self.drag.is_some() {
            return;
        }
        self.composition = (!text.is_empty()).then(|| text.encode_utf16().collect());
        self.composition_cursor = text
            .chars()
            .take(range.map_or(usize::MAX, |r| r.end))
            .map(char::len_utf16)
            .sum();
        self.redraw(app);
    }
    pub fn ime_commit(&mut self, app: &mut AppWorkspace, text: &str) {
        self.composition = None;
        if !text.is_empty() {
            self.insert_text(app, text);
        } else {
            self.redraw(app);
        }
    }
    pub fn ime_delete_surrounding(&mut self, app: &mut AppWorkspace, before: usize, after: usize) {
        if !self.is_editing() || self.drag.is_some() {
            return;
        }
        self.cancel_composition();
        self.anchor = None;
        let (row, col) = self.caret;
        let l = &mut self.lines[row];
        // Scalar boundaries of each independently decoded side preserve a caret inside a pair.
        let mut lo = col;
        for _ in 0..before.min(col) {
            if lo == 0 {
                break;
            }
            lo -= 1;
            if lo > 0
                && (0xdc00..=0xdfff).contains(&l[lo])
                && (0xd800..=0xdbff).contains(&l[lo - 1])
            {
                lo -= 1;
            }
        }
        let mut hi = col;
        for _ in 0..after.min(l.len() - col) {
            if hi == l.len() {
                break;
            }
            let u = l[hi];
            hi += 1;
            if (0xd800..=0xdbff).contains(&u) && hi < l.len() && (0xdc00..=0xdfff).contains(&l[hi])
            {
                hi += 1;
            }
        }
        l.drain(lo..hi);
        self.caret.1 = lo;
        self.redraw(app);
    }
    /// The selected range, its start first; `None` when nothing is selected.
    fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let a = self.anchor.filter(|&a| a != self.caret)?;
        Some(if a < self.caret {
            (a, self.caret)
        } else {
            (self.caret, a)
        })
    }
    /// The selected text, its lines joined with LF.
    pub fn selected_text(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.selection()?;
        let mut units = vec![];
        for r in r0..=r1 {
            let l = &self.lines[r];
            let a = if r == r0 { c0 } else { 0 };
            let b = if r == r1 { c1 } else { l.len() };
            units.extend_from_slice(&l[a..b]);
            if r < r1 {
                units.push(10);
            }
        }
        Some(String::from_utf16_lossy(&units))
    }
    /// The text Ctrl+C or Ctrl+X took since the last call, for the clipboard.
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }
    /// Removes the selected text, the caret at its start, and clears the selection. False when
    /// nothing was selected.
    fn delete_selection(&mut self) -> bool {
        let selected = self.selection();
        self.anchor = None;
        let Some(((r0, c0), (r1, c1))) = selected else {
            return false;
        };
        let tail = self.lines[r1].split_off(c1);
        self.lines[r0].truncate(c0);
        self.lines[r0].extend(tail);
        self.lines.drain(r0 + 1..=r1);
        self.caret = (r0, c0);
        true
    }
    /// Selects the run of the caret's character class (a word, its punctuation or its spaces).
    fn select_word(&mut self) {
        let (row, col) = self.caret;
        let l = &self.lines[row];
        if l.is_empty() {
            return;
        }
        let at = col.min(l.len() - 1);
        let class = category(l[at]);
        let (mut a, mut b) = (at, at + 1);
        while a > 0 && category(l[a - 1]) == class {
            a -= 1;
        }
        while b < l.len() && category(l[b]) == class {
            b += 1;
        }
        (self.anchor, self.caret) = (Some((row, a)), (row, b));
    }
    /// The selection's highlight on the canvas viewport (as `caret_rect`): a box per line, or per
    /// character where the text is placed on a shape or turned, so it follows the glyphs.
    pub fn selection_quads(&self, app: &AppWorkspace) -> Vec<[[f64; 2]; 4]> {
        let (Some(((r0, c0), (r1, c1))), Some(layout), Some(ws)) =
            (self.selection(), &self.layout, app.active_ws())
        else {
            return vec![];
        };
        let s = Self::settings(app);
        let px = text_px_size(s.size, s.metric, self.dpi(app));
        let place = Placement::new(layout, &s, self.click);
        let (t, k) = (ws.view.view_transform(), ws.view.ppp);
        let screen = |p: [f64; 2]| {
            let q = place.as_ref().map_or(p, |pl| pl.map(p));
            [
                (q[0] * t.scale + t.origin[0]) / k,
                (q[1] * t.scale + t.origin[1]) / k,
            ]
        };
        let mut out = vec![];
        for r in r0..=r1.min(layout.lines.len().saturating_sub(1)) {
            let (l, units) = (&layout.lines[r], &self.lines[r]);
            let (mut widths, _) = layout_raw_units_px(units, &[], &s, px);
            widths.push(l.width);
            let a = if r == r0 { c0 } else { 0 };
            let b = if r == r1 { c1 } else { units.len() };
            let xs: Vec<f64> = (a..=b)
                .map(|i| l.origin[0] + widths[i.min(widths.len() - 1)])
                .collect();
            let mut spans: Vec<(f64, f64)> = if place.is_some() {
                xs.windows(2).map(|w| (w[0], w[1])).collect()
            } else {
                vec![(xs[0], xs[xs.len() - 1])]
            };
            // A selected line break shows past the line's end.
            if r < r1 {
                let end = xs[xs.len() - 1];
                spans.push((end, end + layout.space_height / 4.0));
            }
            let (top, bottom) = (l.origin[1], l.origin[1] + l.height);
            for (x0, x1) in spans.into_iter().filter(|(x0, x1)| x1 > x0) {
                out.push([
                    screen([x0, top]),
                    screen([x1, top]),
                    screen([x1, bottom]),
                    screen([x0, bottom]),
                ]);
            }
        }
        out
    }
    fn preview_lines(&self) -> Vec<Vec<u16>> {
        let mut lines = self.lines.clone();
        if let Some(c) = &self.composition {
            lines[self.caret.0].splice(self.caret.1..self.caret.1, c.iter().copied());
        }
        lines
    }
    fn caret_image(&self, app: &AppWorkspace) -> Rectangle {
        let Some(layout) = &self.layout else {
            return Rectangle::default();
        };
        let row = self.caret.0;
        let l = &layout.lines[row];
        let mut prefix = self.lines[row][..self.caret.1].to_vec();
        if let Some(c) = &self.composition {
            prefix.extend(&c[..self.composition_cursor.min(c.len())]);
        }
        let dpi = self.dpi(app);
        let p = layout_units(&[prefix], &Self::settings(app), [0.0, 0.0], dpi);
        let x = l.origin[0] + p.lines[0].width;
        let y = l.origin[1];
        Rectangle::new(
            x.trunc(),
            y.trunc(),
            (x + 2.0).trunc() - x.trunc(),
            (y + l.height).trunc() - y.trunc(),
        )
    }
    /// Coordinates relative to the canvas viewport, in egui points. CanvasView adds its origin.
    pub fn caret_rect(&self, app: &AppWorkspace) -> Rectangle {
        let r = self.caret_image(app);
        let Some(ws) = app.active_ws() else {
            return r;
        };
        let t = ws.view.view_transform();
        let p = ws.view.ppp;
        Rectangle::new(
            (r.x * t.scale + t.origin[0]) / p,
            (r.y * t.scale + t.origin[1]) / p,
            r.width * t.scale / p,
            r.height * t.scale / p,
        )
    }
    /// The caret's corners on the canvas viewport (as `caret_rect`) while the text is placed on a
    /// shape or turned (`shape`); `None` while it lies straight.
    pub fn caret_quad(&self, app: &AppWorkspace) -> Option<[[f64; 2]; 4]> {
        let place = Placement::new(self.layout.as_ref()?, &Self::settings(app), self.click)?;
        let r = self.caret_image(app);
        let ws = app.active_ws()?;
        let (t, k) = (ws.view.view_transform(), ws.view.ppp);
        let corners = [
            [r.left(), r.top()],
            [r.right(), r.top()],
            [r.right(), r.bottom()],
            [r.left(), r.bottom()],
        ];
        Some(corners.map(|c| {
            let q = place.map(c);
            [
                (q[0] * t.scale + t.origin[0]) / k,
                (q[1] * t.scale + t.origin[1]) / k,
            ]
        }))
    }
    fn nub(&self, app: &AppWorkspace) -> [f64; 2] {
        let Some(ws) = app.active_ws() else {
            return [0.0; 2];
        };
        let Some(last) = self.layout.as_ref().and_then(|l| l.lines.last()) else {
            return [0.0; 2];
        };
        // A placed or turned text's nub goes by its drawn bounds.
        let (origin, size) = match self.placed_bounds {
            Some(b) => ([b.x, b.y], [b.width, b.height]),
            None => (last.origin, [last.width, last.height]),
        };
        let surface = &ws.document.layers.get_at(ws.active_layer).unwrap().surface;
        text_nub_position(
            origin,
            size,
            ws.view.ppp,
            ws.view.zoom,
            [surface.width() as f64, surface.height() as f64],
        )
    }
    fn touches_nub(&self, app: &AppWorkspace, x: f64, y: f64) -> bool {
        app.active_ws()
            .is_some_and(|ws| nub_contains(self.nub(app), [x, y], ws.view.ppp, ws.view.zoom))
    }
    /// The press is on the rotate handle (shown with the nub).
    fn touches_turn(&self, app: &AppWorkspace, x: f64, y: f64) -> bool {
        let shown = self.is_editing() && self.pulse.nub_visible;
        (self.turn_handle.zip(app.active_ws()))
            .is_some_and(|((h, _), ws)| shown && nub_contains(h, [x, y], ws.view.ppp, ws.view.zoom))
    }
    /// The rotate handle `TURN_GAP` screen px above the middle of the text's top, in its turned
    /// frame, and the pivot it turns about: the text's middle. Only for text with characters.
    fn turn_geometry(
        &self,
        app: &AppWorkspace,
        layout: &TextLayout,
        s: &TextSettings,
    ) -> Option<([f64; 2], [f64; 2])> {
        let ws = app.active_ws()?;
        if self.mode != EditMode::Editing {
            return None;
        }
        let lines = || {
            layout
                .lines
                .iter()
                .map(line_bounds)
                .reduce(|a, b| Rectangle::union(&a, &b))
        };
        let b = match Placement::turned(layout, s, self.click, 0.0) {
            Some(p) => p.ink_bounds(layout).or_else(lines),
            None => lines(),
        }?;
        let gap = TURN_GAP * ws.view.ppp / ws.view.zoom;
        let mid = [b.x + b.width / 2.0, b.y + b.height / 2.0];
        let top = [mid[0], b.top() - gap];
        let turn = |p| shape::turn_point(p, self.click, s.angle);
        Some((turn(top), turn(mid)))
    }
    /// The press is within `pad` of the text's bounds (half-open, at its pixel).
    fn near(&self, x: f64, y: f64, pad: f64) -> bool {
        self.affected.is_some_and(|r| {
            x.floor() >= r.left() - pad
                && x.floor() < r.right() + pad
                && y.floor() >= r.top() - pad
                && y.floor() < r.bottom() + pad
        })
    }
    /// Upstream #5956: with Editable text, a press up to 8 px (on screen) outside the text being
    /// typed moves it.
    fn in_move_reach(&self, app: &AppWorkspace, x: f64, y: f64) -> bool {
        let Some(ws) = app.active_ws() else {
            return false;
        };
        let reach = 8.0 * ws.view.ppp / ws.view.zoom;
        self.editable && self.is_editing() && !self.near(x, y, 0.0) && self.near(x, y, reach)
    }
    fn update_status(&mut self, app: &mut AppWorkspace) {
        let t = crate::i18n::t;
        let text = if self.drag.is_some() {
            let Some(ws) = app.active_ws() else {
                return;
            };
            let Some(first) = self.layout.as_ref().and_then(|l| l.lines.first()) else {
                return;
            };
            let pixel = app.measurement_unit == crate::app::MeasurementUnit::Pixel;
            let coord = |v: f64| {
                if pixel {
                    crate::app::format_integer(v)
                } else {
                    crate::app::format_fixed2(
                        ws.document.pixel_to_physical(v, app.measurement_unit.id()),
                    )
                }
            };
            let suffix = if pixel {
                String::new()
            } else {
                t(&format!(
                    "measurementUnit.{}.abbreviation",
                    app.measurement_unit.id()
                ))
            };
            crate::i18n::tf(
                "textTool.statusText.textInfo.format",
                &[
                    &coord(first.origin[0]),
                    &suffix,
                    &coord(first.origin[1]),
                    &suffix,
                ],
            )
        } else {
            t("textTool.statusText.startTyping")
        };
        self.status = Some(text);
        app.fire(crate::app::AppEvent::ToolStatusChanged);
    }
    /// Includes provisional IME text without committing it or changing history.
    pub fn needs_color_font(&self, app: &AppWorkspace) -> bool {
        self.is_editing()
            && self
                .preview_lines()
                .iter()
                .any(|l| needs_color_font(&String::from_utf16_lossy(l), &Self::settings(app)))
    }
    /// The lazy script faces the edited (and provisional IME) text needs.
    pub fn script_fallbacks(&self, app: &AppWorkspace) -> Vec<&'static str> {
        if !self.is_editing() {
            return vec![];
        }
        let text: Vec<_> = self
            .preview_lines()
            .iter()
            .map(|l| String::from_utf16_lossy(l))
            .collect();
        script_fallbacks(&text.join("\n"), &Self::settings(app))
    }
    fn sync_environment(&mut self, app: &mut AppWorkspace) {
        let Some(ws) = app.active_ws() else {
            return;
        };
        let e = [
            ws.view.zoom,
            ws.view.ppp,
            ws.document.resolution,
            ws.view.get_width(),
            ws.view.get_height(),
        ];
        if self.is_editing()
            && (self.environment != Some(e)
                || self.font_generation != crate::canvas::text::font_generation())
        {
            self.redraw(app);
        }
    }
    fn ensure_caret_visible(&self, app: &mut AppWorkspace) {
        let Some(ws) = app.active_ws() else {
            return;
        };
        let s = Self::settings(app);
        let dpi = self.dpi(app);
        let layout = layout_units(&self.lines, &s, self.click, dpi);
        let row = self.caret.0;
        let prefix = layout_units(
            &[self.lines[row][..self.caret.1].to_vec()],
            &s,
            [0.0; 2],
            dpi,
        );
        let p = logical_position(
            self.click,
            [prefix.lines[0].width, prefix.lines[0].height],
            layout.lines[row].width,
            row,
            s.align,
        );
        let p = match Placement::new(&layout, &s, self.click) {
            Some(place) => place.map(p),
            None => p,
        };
        if let Some(center) =
            text_visibility_center(ws.view.text_viewport_bounds(), p, layout.lines[row].height)
        {
            app.active_ws_mut()
                .unwrap()
                .view
                .set_text_view_center(center);
        }
    }
    fn sync_handle(&self, app: &mut AppWorkspace) {
        let canvas = self.nub(app);
        let visible = self.is_editing() && self.pulse.nub_visible;
        if let (Some(id), Some(ws)) = (self.handles, app.active_ws_mut()) {
            let opacity = self.pulse.nub_opacity as f32;
            let mut list = vec![];
            if visible {
                list.push(Handle {
                    canvas,
                    shape: HandleShape::Compass,
                    opacity,
                    hover: 0.0,
                });
                if let Some((canvas, _)) = self.turn_handle {
                    list.push(Handle {
                        canvas,
                        shape: HandleShape::Rotate,
                        opacity,
                        hover: 0.0,
                    });
                }
            }
            if matches!(ws.surface_box.renderer(id),Some(Renderer::Handles(h)) if *h==list) {
                return;
            }
            if let Some(Renderer::Handles(h)) = ws.surface_box.renderer_mut(id) {
                *h = list;
            }
        }
    }
    fn redraw(&mut self, app: &mut AppWorkspace) {
        if !self.is_editing() {
            return;
        }
        self.sync_preview_layer(app);
        let s = Self::settings(app);
        let Some(ws) = app.active_ws() else {
            return;
        };
        let dpi = ws.document.resolution;
        self.font_generation = crate::canvas::text::font_generation();
        self.environment = Some([
            ws.view.zoom,
            ws.view.ppp,
            dpi,
            ws.view.get_width(),
            ws.view.get_height(),
        ]);
        let layout_dpi = self.dpi(app);
        self.layout = Some(layout_units(
            &self.preview_lines(),
            &s,
            self.click,
            layout_dpi,
        ));
        let caret = self.caret_image(app);
        let layout = self.layout.as_ref().unwrap();
        // Extra (`shape`): the text drawn on its shape and turned.
        let place = Placement::new(layout, &s, self.click);
        let drawn = place.as_ref().map(|p| p.apply(layout));
        let caret = place.as_ref().map_or(caret, |p| p.quad_bounds(&caret));
        self.turn_handle = self.turn_geometry(app, layout, &s);
        self.angle = s.angle;
        let layout = drawn.as_ref().unwrap_or(layout);
        self.placed_bounds = place.and_then(|_| {
            layout
                .lines
                .iter()
                .map(line_bounds)
                .reduce(|a, b| Rectangle::union(&a, &b))
        });
        let bounds = layout
            .lines
            .iter()
            .map(line_bounds)
            .fold(caret, |a, b| Rectangle::union(&a, &b));
        let bounds = Rectangle::intersect_of(
            &bounds,
            &ws.document
                .layers
                .get_at(ws.active_layer)
                .unwrap()
                .surface
                .get_bounds(),
        );
        let dirty = self
            .affected
            .map_or(bounds, |old| Rectangle::union(&old, &bounds));
        self.affected = Some(bounds);
        let rect = IntRect::new(
            dirty.x.floor() as i32,
            dirty.y.floor() as i32,
            (dirty.right().ceil() - dirty.x.floor()).max(0.0) as u32,
            (dirty.bottom().ceil() - dirty.y.floor()).max(0.0) as u32,
        );
        if !rect.is_empty() && self.base.is_open() {
            let c = app.primary_color;
            let raster = rasterize_text_content(
                layout,
                &s,
                rect,
                [c.red, c.green, c.blue, c.alpha],
                crate::effect::surface::Space::Srgb,
            );
            let content = raster.content;
            let mask = raster.coverage;
            let contents = [ContentLayer {
                content: &content,
                mask: Some(&mask),
            }];
            if self.editable {
                // A text layer's own pixels (`text_layer::render`): Normal, no selection clip.
                let original = self.base.original();
                let normal = ContentBlendMode::Layer(BlendMode::Normal);
                let w = original.width();
                let data = render_content(original.data(), w, rect, &contents, None, normal);
                self.base
                    .put_transaction(app.active_ws_mut().unwrap(), rect, data);
            } else {
                let mode = ContentBlendMode::from_value(
                    app.tool_settings
                        .get(ToolType::Text, "blendMode")
                        .as_str()
                        .unwrap_or("normal"),
                );
                let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
                let ws = app.active_ws_mut().unwrap();
                self.base
                    .render_transaction(ws, rect, &contents, mode, quality);
            }
        }
        self.sync_handle(app);
        self.update_status(app);
    }
    pub fn commit(&mut self, app: &mut AppWorkspace) {
        if !self.is_editing() {
            return;
        }
        self.cancel_composition();
        self.redraw(app);
        if self.editable {
            self.commit_text_layer(app);
        }
        if let Some(ws) = app.active_ws_mut() {
            if !self.editable && self.mode == EditMode::Editing {
                let m = self
                    .affected
                    .filter(|b| b.width > 0.0 && b.height > 0.0)
                    .map(|b| {
                        HistoryMemento::new(
                            "Text",
                            &ToolType::Text.icon_src(),
                            MementoKind::Bitmap(BitmapMemento::from_surface(
                                self.base.original(),
                                ws.active_layer,
                                Some(&Region::from_rectangle(b)),
                            )),
                        )
                    });
                self.base
                    .commit_bitmap_transaction(ws, "Text", &ToolType::Text.icon_src());
                if let Some(m) = m {
                    if let Some(parent) = self
                        .compound
                        .and_then(|id| ws.history.undo.iter_mut().find(|m| m.id == id))
                    {
                        if let MementoKind::Compound(children) = &mut parent.kind {
                            children.push(m);
                        }
                    } else {
                        ws.push_memento(m);
                    }
                }
            } else {
                self.base.cancel_bitmap_transaction(ws);
            }
            if let Some(id) = self.handles.take() {
                ws.surface_box.remove_renderer(id);
            }
        }
        self.pulse.stop(app.now_ms);
        self.mode = EditMode::None;
        self.compound = None;
        self.affected = None;
        self.layout = None;
        self.drag = None;
        self.turning = None;
        (self.placed_bounds, self.turn_handle) = (None, None);
        (self.anchor, self.selecting) = (None, false);
        // The angle is the edited text's: the next text starts straight.
        if Self::settings(app).angle != 0.0 {
            app.tool_settings
                .set(ToolType::Text, "textAngle", number(0.0));
        }
    }
    /// The text being typed as a text layer's spec (S§4.5).
    fn spec(&self, app: &AppWorkspace) -> TextSpec {
        let lines: Vec<_> = self
            .lines
            .iter()
            .map(|l| String::from_utf16_lossy(l))
            .collect();
        let c = app.primary_color;
        TextSpec {
            text: lines.join("\n"),
            settings: Self::settings(app),
            origin: (self.click[0], self.click[1]),
            color: [c.red, c.green, c.blue, c.alpha],
            dpi: self.dpi(app),
        }
    }
    /// A re-edit's writes to the shared antialias setting (R17) and the primary colour go back,
    /// unless they changed since: at its commit, and when its image closes without one.
    pub fn restore_shared(&self, app: &mut AppWorkspace) {
        let Some(r) = &self.reedit else {
            return;
        };
        if app.tool_settings.get(ToolType::Text, "antialias") == r.spec.settings.antialias {
            let v = r.antialias.clone();
            app.tool_settings.set(ToolType::Text, "antialias", v);
        }
        let [red, green, blue, alpha] = r.spec.color;
        if app.primary_color == Color::new(red, green, blue, alpha) {
            app.set_colors(r.primary, app.secondary_color);
        }
    }
    /// Editable text's commit (S§4.5), as one history item:
    /// - new text: its preview layer, named after the text's first 20 characters (blank text has
    ///   none, R76, R79);
    /// - a re-edit: the layer's new spec and pixels; nothing when unchanged; when left blank, the
    ///   layer's deletion (R76), or on the only layer its rasterize as an empty raster layer.
    ///
    /// `restore_shared` runs first.
    fn commit_text_layer(&mut self, app: &mut AppWorkspace) {
        let spec = self.spec(app);
        self.restore_shared(app);
        let reedit = self.reedit.take();
        let preview = std::mem::take(&mut self.preview);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        self.base.cancel_bitmap_transaction(ws);
        let (w, h, i) = (ws.document.width, ws.document.height, ws.active_layer);
        let kind = match reedit {
            Some(r) if blank(&spec.text) && ws.document.layers.size() > 1 => {
                let mut layer = ws.document.remove_layer_at(i);
                layer.surface = r.surface;
                MementoKind::DeleteLayer {
                    index: i,
                    layer: Box::new(layer),
                }
            }
            Some(r) if blank(&spec.text) => {
                // The cancelled preview left it transparent.
                ws.document
                    .layers
                    .get_at_mut(i)
                    .expect("the edited layer")
                    .text = None;
                ws.invalidate(None);
                MementoKind::TextLayer(TextLayerMemento {
                    layer: i,
                    old: Some(r.spec),
                    new: None,
                    old_surface: Some(r.surface),
                })
            }
            Some(r) => {
                let layer = ws.document.layers.get_at_mut(i).expect("the edited layer");
                if r.spec == spec {
                    layer.surface = r.surface;
                    ws.invalidate(None);
                    return;
                }
                layer.surface = text_layer::render(&spec, w, h);
                layer.text = Some(spec.clone());
                ws.invalidate(None);
                MementoKind::TextLayer(TextLayerMemento {
                    layer: i,
                    old: Some(r.spec),
                    new: Some(spec),
                    old_surface: Some(r.surface),
                })
            }
            // `redraw` dropped the layer of empty text.
            None if !preview => return,
            None => {
                let layer = ws.document.layers.get_at_mut(i).expect("the preview layer");
                let name = spec.text.chars().take(20);
                layer.properties.name = name.map(|c| if c == '\n' { ' ' } else { c }).collect();
                layer.surface = text_layer::render(&spec, w, h);
                layer.text = Some(spec);
                ws.invalidate(None);
                MementoKind::NewLayer { index: i }
            }
        };
        let m = HistoryMemento::new("Text", &ToolType::Text.icon_src(), kind);
        let parent = self
            .compound
            .and_then(|id| ws.history.undo.iter_mut().find(|m| m.id == id));
        if let Some(MementoKind::Compound(children)) = parent.map(|p| &mut p.kind) {
            children.push(m);
            ws.after_history_change();
        } else {
            ws.push_memento(m);
        }
    }
    fn hit_position(&mut self, app: &AppWorkspace, x: f64, y: f64) {
        let dpi = self.dpi(app);
        let s = Self::settings(app);
        let layout = layout_units(&self.lines, &s, self.click, dpi);
        if let Some(place) = Placement::new(&layout, &s, self.click) {
            // Placed or turned text: the caret slot whose middle lands nearest the press (the
            // callers add H/2).
            let at = [x, y - layout.space_height / 2.0];
            let px = text_px_size(s.size, s.metric, dpi);
            let mut best = (f64::MAX, (0, 0));
            for (row, l) in layout.lines.iter().enumerate() {
                let (mut widths, _) = layout_raw_units_px(&self.lines[row], &[], &s, px);
                widths.push(l.width);
                for (col, w) in widths.into_iter().enumerate() {
                    let q = place.map([l.origin[0] + w, l.origin[1] + l.height / 2.0]);
                    let d = (q[0] - at[0]).hypot(q[1] - at[1]);
                    if d < best.0 {
                        best = (d, (row, col));
                    }
                }
            }
            self.caret = best.1;
            return;
        }
        let first = &layout.lines[0];
        // The lookup origin is the click point, not the rendered top. The pointer caller adds H/2.
        let row = ((y - self.click[1]) / (first.height as f32 as f64))
            .floor()
            .max(0.0) as usize;
        let row = row.min(self.lines.len() - 1);
        let dx = x - self.click[0];
        let col = (0..self.lines[row].len())
            .find(|&i| {
                let p =
                    &layout_units(&[self.lines[row][..i].to_vec()], &s, [0.0, 0.0], dpi).lines[0];
                logical_position(
                    self.click,
                    [p.width, p.height],
                    layout.lines[row].width,
                    row,
                    s.align,
                )[0] - self.click[0]
                    >= dx
            })
            .unwrap_or(self.lines[row].len());
        self.caret = (row, col);
    }
    /// Text keydown checks visibility after direct commands, before base-dispatched commands.
    pub fn key(&mut self, app: &mut AppWorkspace, k: &KeyPress) -> bool {
        if !self.is_editing() {
            return false;
        }
        let direct = matches!(
            k.key.as_str(),
            "Home" | "End" | "PageUp" | "PageDown" | "Backspace" | "Delete"
        ) || (k.key == "Tab" && !k.ctrl);
        let modifier = matches!(k.key.as_str(), "Control" | "Ctrl" | "Shift" | "Alt");
        if !direct && !modifier {
            self.ensure_caret_visible(app);
        }
        let handled = self.command(app, k);
        if direct && self.is_editing() {
            self.ensure_caret_visible(app);
        }
        handled
    }
    fn command(&mut self, app: &mut AppWorkspace, k: &KeyPress) -> bool {
        if !self.is_editing() {
            return false;
        }
        if self.drag.is_some() {
            return matches!(
                k.key.as_str(),
                "Escape"
                    | "Enter"
                    | "Backspace"
                    | "Delete"
                    | "ArrowLeft"
                    | "ArrowRight"
                    | "ArrowUp"
                    | "ArrowDown"
                    | "Home"
                    | "End"
                    | "PageUp"
                    | "PageDown"
                    | " "
                    | "Space"
            ) || (k.key == "Tab" && !k.ctrl);
        }
        if k.key == "Escape" {
            if self.composition.is_some() {
                self.cancel_composition();
                self.redraw(app);
            } else {
                self.commit(app);
            }
            return true;
        }
        // R97 port adapter: the IME accepts Enter through Commit; do not also split a line.
        if k.key == "Enter" && self.composition.is_some() {
            return true;
        }
        if k.key == " " || k.key == "Space" {
            return true;
        }
        if !(k.alt || app.alt_key_down) {
            self.pulse.epoch = app.now_ms;
        }
        if k.key == "Tab" {
            return !k.ctrl;
        }
        if k.key == "PageUp" || k.key == "PageDown" {
            return true;
        }
        if k.alt || app.alt_key_down {
            return false;
        }
        let plain_ctrl = k.ctrl && !k.shift && !k.meta;
        // Ctrl+A selects all the text being typed, not the image.
        if plain_ctrl && k.key.eq_ignore_ascii_case("a") {
            self.cancel_composition();
            let last = self.lines.len() - 1;
            (self.anchor, self.caret) = (Some((0, 0)), (last, self.lines[last].len()));
            self.redraw(app);
            return true;
        }
        // Ctrl+C / Ctrl+X copy or cut the selected text; with none they are the image's.
        let cut = k.key.eq_ignore_ascii_case("x");
        if plain_ctrl && (cut || k.key.eq_ignore_ascii_case("c")) {
            let Some(text) = self.selected_text() else {
                return false;
            };
            self.copied = Some(text);
            if cut {
                self.cancel_composition();
                self.delete_selection();
                self.redraw(app);
            }
            return true;
        }
        let ctrl = plain_ctrl;
        // Ctrl moves by words, with Shift too (it then selects them).
        let word = k.ctrl && !k.meta;
        let (row, col) = self.caret;
        let len = self.lines[row].len();
        if matches!(
            k.key.as_str(),
            "Enter"
                | "Backspace"
                | "Delete"
                | "ArrowLeft"
                | "ArrowRight"
                | "ArrowUp"
                | "ArrowDown"
                | "Home"
                | "End"
        ) {
            self.cancel_composition();
        }
        // The selection: Shift with a move extends it from where the caret was; a plain Left or
        // Right goes to its side; Backspace, Delete and Enter remove it (Enter then breaks the
        // line); any other key clears it.
        let nav = matches!(
            k.key.as_str(),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End"
        );
        let selected = self.selection();
        if nav && k.shift {
            self.anchor.get_or_insert(self.caret);
        } else if let Some((start, end)) =
            selected.filter(|_| !word && matches!(k.key.as_str(), "ArrowLeft" | "ArrowRight"))
        {
            self.caret = if k.key == "ArrowLeft" { start } else { end };
            self.anchor = None;
            self.redraw(app);
            return true;
        } else if selected.is_some() && matches!(k.key.as_str(), "Backspace" | "Delete" | "Enter") {
            self.delete_selection();
            if k.key == "Enter" {
                self.newline();
            }
            self.redraw(app);
            return true;
        } else {
            self.anchor = None;
        }
        match k.key.as_str() {
            "Enter" => self.newline(),
            "Home" => self.caret.1 = 0,
            "End" => self.caret.1 = len,
            "ArrowLeft" => {
                if col == 0 {
                    if row > 0 {
                        self.caret = (row - 1, self.lines[row - 1].len());
                    }
                } else {
                    self.caret.1 = if word {
                        word_left(&self.lines[row], col)
                    } else {
                        col - 1
                    };
                }
            }
            "ArrowRight" => {
                if col == len {
                    if row + 1 < self.lines.len() {
                        self.caret = (row + 1, 0);
                    }
                } else {
                    self.caret.1 = if word {
                        word_right(&self.lines[row], col)
                    } else {
                        col + 1
                    };
                }
            }
            "Backspace" => {
                if col == 0 {
                    if row > 0 {
                        let tail = self.lines.remove(row);
                        self.caret = (row - 1, self.lines[row - 1].len());
                        self.lines[row - 1].extend(tail);
                    }
                } else {
                    let l = &self.lines[row];
                    let mut lo = if ctrl { word_left(l, col) } else { col - 1 };
                    if ctrl && col < len && !white(l[col - 1]) && !letter(l[col]) {
                        while lo > 0 && white(l[lo - 1]) {
                            lo -= 1;
                        }
                    }
                    self.lines[row].drain(lo..col);
                    self.caret.1 = lo;
                }
            }
            "Delete" => {
                if col == len {
                    if row + 1 < self.lines.len() {
                        let tail = self.lines.remove(row + 1);
                        self.lines[row].extend(tail);
                    }
                } else {
                    let l = &self.lines[row];
                    let mut hi = col + 1;
                    if ctrl {
                        hi = col;
                        while hi < len && white(l[hi]) {
                            hi += 1;
                        }
                        if hi < len {
                            let cat = category(l[hi]);
                            hi += 1;
                            if matches!(cat, 1 | 2) {
                                while hi < len && category(l[hi]) == cat {
                                    hi += 1;
                                }
                            }
                        }
                        if col == 0 || (!letter(l[col - 1]) && !white(l[col])) {
                            while hi < len && white(l[hi]) {
                                hi += 1;
                            }
                        }
                    }
                    self.lines[row].drain(col..hi);
                }
                if self.lines.len() == 1 && self.lines[0].is_empty() {
                    self.mode = EditMode::Empty;
                    self.compound = None;
                }
            }
            "ArrowUp" | "ArrowDown" => {
                if k.key == "ArrowUp" || row + 1 < self.lines.len() {
                    // quirk(pdn): painting-tools-2.md §10.2: navigation uses independently measured
                    // prefix height times the row, not the renderer's accumulated line heights or
                    // truncated caret.
                    let dpi = self.dpi(app);
                    let prefix = layout_units(
                        &[self.lines[row][..col].to_vec()],
                        &Self::settings(app),
                        [0.0, 0.0],
                        dpi,
                    );
                    let settings = Self::settings(app);
                    let layout = layout_units(&self.lines, &settings, self.click, dpi);
                    let p = logical_position(
                        self.click,
                        [prefix.lines[0].width, prefix.lines[0].height],
                        layout.lines[row].width,
                        row,
                        settings.align,
                    );
                    let sign = if k.key == "ArrowUp" { -1.0 } else { 1.0 };
                    self.hit_position(
                        app,
                        p[0],
                        p[1] + sign * (layout.lines[0].height as f32 as f64),
                    );
                }
            }
            _ => return !(k.ctrl || k.alt || k.meta) && k.key.chars().count() == 1,
        }
        self.redraw(app);
        true
    }
}
fn category(u: u16) -> u8 {
    use unicode_general_category::{GeneralCategory::*, get_general_category as gc};
    let Some(c) = char::from_u32(u as u32) else {
        return 0;
    };
    if c.is_whitespace() {
        return 3;
    }
    match gc(c) {
        UppercaseLetter | LowercaseLetter | TitlecaseLetter | ModifierLetter | OtherLetter
        | DecimalNumber => 1,
        ConnectorPunctuation | DashPunctuation | OpenPunctuation | ClosePunctuation
        | InitialPunctuation | FinalPunctuation | OtherPunctuation => 2,
        _ => 0,
    }
}
fn white(u: u16) -> bool {
    category(u) == 3
}
fn letter(u: u16) -> bool {
    category(u) == 1
}
fn word_left(l: &[u16], mut i: usize) -> usize {
    while i > 0 && white(l[i - 1]) {
        i -= 1;
    }
    if i > 0 {
        let c = category(l[i - 1]);
        i -= 1;
        if matches!(c, 1 | 2) {
            while i > 0 && category(l[i - 1]) == c {
                i -= 1;
            }
        }
    }
    i
}
fn word_right(l: &[u16], mut i: usize) -> usize {
    let c = category(l[i]);
    let run = c == 1 || (c == 2 && i > 0);
    i += 1;
    if run {
        while i < l.len() && category(l[i]) == c {
            i += 1;
        }
    }
    while i < l.len() && white(l[i]) {
        i += 1;
    }
    i
}
impl Tool for TextTool {
    fn tool_type(&self) -> ToolType {
        ToolType::Text
    }
    fn on_activate(&mut self, _: &mut AppWorkspace) {
        self.cursor_name = Some("text_tool_cursor");
        self.status = None;
    }
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.sync_environment(app);
    }
    fn on_selection_changed(&mut self, app: &mut AppWorkspace) {
        self.redraw(app);
    }
    fn status_text(&self) -> Option<&str> {
        self.status.as_deref()
    }
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        self.commit(app);
    }
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _: &PointerInput,
    ) -> bool {
        if b == MouseButton::Left
            && self.touches_turn(app, x, y)
            && let Some((_, pivot)) = self.turn_handle
        {
            self.cancel_composition();
            self.turning = Some(Turning {
                pivot,
                from: (y - pivot[1]).atan2(x - pivot[0]),
                angle: Self::settings(app).angle,
                click: self.click,
            });
            self.cursor_name = Some("rotate_cursor");
            self.redraw(app);
            return true;
        }
        let hit = self.touches_nub(app, x, y);
        let reach = b == MouseButton::Left && self.in_move_reach(app, x, y);
        if self.is_editing() && (b == MouseButton::Right || hit || reach) {
            let canceled_preedit = self.composition.is_some();
            self.cancel_composition();
            self.drag = Some(([x.floor(), y.floor()], self.click));
            self.cursor_name = Some("hand_closed_cursor");
            if canceled_preedit {
                self.redraw(app);
            } else {
                self.update_status(app);
            }
            return true;
        }
        if b != MouseButton::Left {
            return false;
        }
        self.cancel_composition();
        let h = self.layout.as_ref().map_or(0.0, |l| l.space_height);
        if self.is_editing() && self.near(x, y, h.ceil()) {
            let before = self.caret;
            self.hit_position(app, x, y + h / 2.0);
            // Shift extends the selection; a double-click selects the word; a press starts one,
            // which its drag spans.
            let now = app.now_ms;
            let reach = app
                .active_ws()
                .map_or(4.0, |ws| 4.0 * ws.view.ppp / ws.view.zoom);
            let double = self
                .last_press
                .is_some_and(|(t, p)| now - t <= 500.0 && (p[0] - x).hypot(p[1] - y) <= reach);
            if app.shift_key_down {
                self.anchor.get_or_insert(before);
            } else if double {
                self.select_word();
            } else {
                self.anchor = Some(self.caret);
            }
            self.selecting = !double;
            self.last_press = (!double).then_some((now, [x, y]));
            self.redraw(app);
        } else {
            self.start(app, [x.floor(), y.floor()]);
        }
        true
    }
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _: &PointerInput) -> bool {
        if let Some(t) = self.turning {
            // Screen angles grow clockwise, the text's counter-clockwise; Shift steps by 15°.
            let turned = ((y - t.pivot[1]).atan2(x - t.pivot[0]) - t.from).to_degrees();
            let mut angle = t.angle - turned;
            if app.shift_key_down {
                angle = (angle / 15.0).round() * 15.0;
            }
            let angle = ((angle + 180.0).rem_euclid(360.0) - 180.0) * 10.0;
            let angle = angle.round() / 10.0;
            // The text spins about its middle: the click point goes round it too.
            self.click = shape::turn_point(t.click, t.pivot, angle - t.angle);
            app.tool_settings
                .set(ToolType::Text, "textAngle", number(angle));
            self.redraw(app);
            return true;
        }
        if self.selecting {
            let h = self.layout.as_ref().map_or(0.0, |l| l.space_height);
            self.hit_position(app, x, y + h / 2.0);
            self.redraw(app);
            return true;
        }
        if let Some((start, origin)) = self.drag {
            self.click = [
                origin[0] + x.floor() - start[0],
                origin[1] + y.floor() - start[1],
            ];
            self.redraw(app);
            true
        } else {
            let nub = self.pulse.nub_visible && self.touches_nub(app, x, y);
            self.cursor_name = Some(if self.touches_turn(app, x, y) {
                "rotate_cursor"
            } else if nub || self.in_move_reach(app, x, y) {
                "hand_open_cursor"
            } else {
                "text_tool_cursor"
            });
            false
        }
    }
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        _: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if self.turning.is_some() {
            self.on_mouse_move(app, x, y, i);
            self.turning = None;
            self.update_status(app);
            return true;
        }
        if self.selecting {
            self.on_mouse_move(app, x, y, i);
            self.selecting = false;
            if self.anchor == Some(self.caret) {
                self.anchor = None;
            }
            return true;
        }
        if self.drag.is_none() {
            return false;
        }
        let moved = self.on_mouse_move(app, x, y, i);
        self.drag = None;
        if moved {
            self.update_status(app);
        }
        moved
    }
    fn on_key_press(&mut self, app: &mut AppWorkspace, k: &KeyPress) -> bool {
        self.key(app, k)
    }
    fn on_executing_history_memento(&mut self, app: &mut AppWorkspace) {
        self.commit(app);
    }
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        // The toggle takes effect from the next text: the open one commits as it began.
        if key == "editableText" {
            self.commit(app);
        } else {
            // A typed angle spins the text about its middle, as the rotate handle does.
            if key == "textAngle"
                && let Some((_, pivot)) = self.turn_handle.filter(|_| self.is_editing())
            {
                let angle = Self::settings(app).angle;
                self.click = shape::turn_point(self.click, pivot, angle - self.angle);
            }
            self.redraw(app);
        }
    }
    fn on_colors_changed(&mut self, app: &mut AppWorkspace) {
        self.redraw(app);
    }
    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        // egui exposes modifier transitions rather than standalone Ctrl/Shift key events.
        let keys = [app.control_key_down, app.shift_key_down];
        if self.is_editing()
            && self.workspace_focused
            && self.drag.is_none()
            && !app.alt_key_down
            && keys
                .iter()
                .zip(self.modifier_keys)
                .any(|(&now, old)| now && !old)
        {
            self.pulse.epoch = app.now_ms;
        }
        self.modifier_keys = keys;
    }
    fn commit_pending(&mut self, app: &mut AppWorkspace) -> bool {
        let yes = self.can_finish();
        if yes {
            self.commit(app);
        }
        yes
    }
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }
    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(self.cursor_name.unwrap_or("text_tool_cursor"))
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;
    fn edit(text: &str) -> (TextTool, AppWorkspace) {
        let mut app = crate::testing::fresh_doc(120, 80, None);
        // Paint.NET's text editing: the Extra "Editable text" (on by default) is off.
        app.tool_settings
            .set(ToolType::Text, "editableText", false.into());
        let mut t = TextTool::default();
        t.start(&mut app, [20.0, 20.0]);
        t.workspace_focused = true;
        t.insert_text(&mut app, text);
        (t, app)
    }
    fn key(t: &mut TextTool, app: &mut AppWorkspace, s: &str, ctrl: bool, shift: bool) {
        t.key(
            app,
            &KeyPress {
                key: s.into(),
                ctrl,
                shift,
                alt: false,
                meta: false,
            },
        );
    }
    #[test]
    fn final_text_presentation_arithmetic() {
        for (align, x) in [
            (TextAlign::Left, 140.0),
            (TextAlign::Center, 125.0),
            (TextAlign::Right, 110.0),
        ] {
            let origin = [line_origin_x(100.0, align, 30.0), 60.0];
            assert_eq!(
                text_nub_position(origin, [30.0, 20.0], 1.0, 1.0, [1000.0; 2]),
                [x, 90.0]
            );
            assert_eq!(
                text_nub_position([100.0, 60.0], [0.0, 20.0], 1.0, 1.0, [1000.0; 2]),
                [110.0, 90.0]
            );
        }
        assert_eq!(
            text_nub_position([100.0; 2], [0.0; 2], 1.25, 2.0, [1000.0; 2]),
            [106.0; 2]
        );
        assert_eq!(
            text_nub_position([100.0; 2], [0.0; 2], 1.0, 32.0, [1000.0; 2]),
            [100.0; 2]
        );
        assert_eq!(
            text_nub_position([100.0; 2], [0.0; 2], 1.0, 2.0 / 3.0, [1000.0; 2]),
            [115.0; 2]
        );
        assert_eq!(
            text_nub_position([500.0, -50.0], [0.0; 2], 1.0, 1.0, [200.0; 2]),
            [187.0, 13.0]
        );
        assert_eq!(
            text_nub_position([500.0; 2], [0.0; 2], 4.0, 1.0, [10.0; 2]),
            [13.0; 2]
        );
        for (x, yes) in [(93.0, true), (106.999, true), (107.0, false)] {
            assert_eq!(nub_contains([100.0; 2], [x, 100.0], 1.0, 1.0), yes);
        }
        let viewport = Rectangle::new(0.0, 0.0, 100.0, 100.0);
        for (x, center) in [(95.0, 50.0), (100.0, 50.0), (101.0, 101.0)] {
            assert_eq!(
                text_visibility_center(viewport, [x, 50.0], 20.0),
                Some([center, 50.0])
            );
        }
        assert_eq!(text_visibility_center(viewport, [10.0, 10.0], 20.0), None);
        assert_eq!(
            portable_text_size(2000.0 * 3000.0 / 72.0),
            (2000.0 * 3000.0 / 72.0) as f32 as f64
        );
        assert_eq!(portable_text_size(0.001), 0.001f32 as f64);
        assert_eq!(portable_text_size(f64::INFINITY), 0.01f32 as f64);
        for (v, ok) in [
            (1.0, true),
            (2000.0, true),
            (0.99, false),
            (2000.1, false),
            (f64::NAN, false),
        ] {
            assert_eq!(valid_font_size(v), ok);
        }
    }
    #[test]
    fn final_text_cursor_status_and_scroll() {
        let (mut t, mut app) = edit("AB");
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        let n = t.nub(&app);
        t.on_mouse_move(&mut app, n[0], n[1], &input);
        assert_eq!(t.cursor(), CursorRequest::Image("hand_open_cursor"));
        t.on_mouse_down(&mut app, n[0], n[1], MouseButton::Left, &input);
        assert_eq!(t.cursor(), CursorRequest::Image("hand_closed_cursor"));
        assert!(t.status_text().unwrap().starts_with("Anchor point:"));
        t.on_mouse_up(&mut app, n[0], n[1], MouseButton::Left, &input);
        assert_eq!(t.cursor(), CursorRequest::Image("hand_closed_cursor"));
        assert!(t.status_text().unwrap().starts_with("Drag the handle"));
        t.commit(&mut app);
        assert_eq!(t.cursor(), CursorRequest::Image("hand_closed_cursor"));
        t.on_mouse_move(&mut app, n[0], n[1], &input);
        assert_eq!(t.cursor(), CursorRequest::Image("text_tool_cursor"));
        let v = &mut app.active_ws_mut().unwrap().view;
        v.view_width = 100.0;
        v.view_height = 100.0;
        v.zoom = 1.5;
        v.set_text_view_center([80.2, 60.3]);
        let b = v.text_viewport_bounds();
        assert!((b.x - (80.2 - 100.0 / 3.0)).abs() < 1e-9);
        assert!(v.has_native_text_scroll());
        v.set_view_position(1.0, 2.0);
        assert!(!v.has_native_text_scroll());
    }
    #[test]
    fn fix1_move_press_cancels_preedit_pixels_and_layout_immediately() {
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        for button in [MouseButton::Right, MouseButton::Left] {
            for visible in [true, false] {
                let (mut t, mut app) = edit("A");
                let accepted_pixels = crate::testing::layer_bytes(&app, None);
                let layout = |t: &TextTool| {
                    t.layout
                        .as_ref()
                        .unwrap()
                        .lines
                        .iter()
                        .map(|l| (l.text.clone(), l.origin, l.width, l.height))
                        .collect::<Vec<_>>()
                };
                let accepted_layout = layout(&t);
                let origin = t.click;
                let caret = t.caret;
                let history_len = app.active_ws().unwrap().history.undo.len();
                t.ime_preedit(&mut app, "draft");
                assert_ne!(crate::testing::layer_bytes(&app, None), accepted_pixels);
                assert_ne!(layout(&t), accepted_layout);
                t.pulse.sample(300.0, true, true, false, false, true);
                app.now_ms = 350.0;
                t.pulse.nub_visible = visible;
                let epoch = t.pulse.epoch;
                let opacity = t.pulse.opacity(app.now_ms);
                let nub_opacity = t.pulse.nub_opacity;
                let p = if button == MouseButton::Left {
                    t.nub(&app)
                } else {
                    [80.25, 50.75]
                };
                assert!(t.on_mouse_down(&mut app, p[0], p[1], button, &input));
                assert!(t.composition.is_none());
                assert!(t.interrupt_composition);
                assert_eq!(t.drag, Some((p.map(f64::floor), origin)));
                assert_eq!(t.click, origin);
                assert_eq!(t.caret, caret);
                assert_eq!(t.cursor(), CursorRequest::Image("hand_closed_cursor"));
                assert_eq!(t.pulse.epoch, epoch);
                assert_eq!(t.pulse.opacity(app.now_ms), opacity);
                assert_eq!(t.pulse.nub_opacity, nub_opacity);
                assert_eq!(t.pulse.nub_visible, visible);
                assert_eq!(app.active_ws().unwrap().history.undo.len(), history_len);
                assert_eq!(
                    crate::testing::layer_bytes(&app, None),
                    accepted_pixels,
                    "move press must remove provisional pixels: {button:?}, visible={visible}"
                );
                assert_eq!(layout(&t), accepted_layout);
                t.on_frame(&mut app);
                app.now_ms += 16.0;
                t.update_pulse(&mut app, true, true, false, true);
                assert!(
                    !t.pulse.nub_visible,
                    "tracking suppression still waits for the pulse"
                );
                assert_eq!(crate::testing::layer_bytes(&app, None), accepted_pixels);
                assert_eq!(layout(&t), accepted_layout);
            }
        }
    }
    #[test]
    fn fix1_outstanding_release_after_finish_preserves_stored_cursor() {
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        for restart in [false, true] {
            let (mut t, mut app) = edit("AB");
            let p = [80.0, 50.0];
            t.on_mouse_down(&mut app, p[0], p[1], MouseButton::Right, &input);
            assert_eq!(t.cursor(), CursorRequest::Image("hand_closed_cursor"));
            assert!(t.commit_pending(&mut app));
            assert!(t.drag.is_none());
            if restart {
                // A left press starts a new edit before the outstanding right release.
                t.on_mouse_down(&mut app, 40.0, 30.0, MouseButton::Left, &input);
                assert_eq!(t.mode, EditMode::Empty);
            }
            let status = t.status.clone();
            let origin = t.click;
            assert!(!t.on_mouse_up(&mut app, p[0], p[1], MouseButton::Right, &input));
            assert_eq!(
                t.cursor(),
                CursorRequest::Image("hand_closed_cursor"),
                "release without tracking, restart={restart}"
            );
            assert_eq!(t.status, status);
            assert_eq!(t.click, origin);
            t.on_mouse_move(&mut app, -10.0, -10.0, &input);
            assert_eq!(t.cursor(), CursorRequest::Image("text_tool_cursor"));
        }
    }
    #[test]
    fn final_text_hidden_nub_and_half_open_inside_bounds() {
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        let (mut t, mut app) = edit("ab");
        let n = t.nub(&app);
        t.pulse.nub_visible = false;
        t.on_mouse_move(&mut app, n[0], n[1], &input);
        assert_eq!(t.cursor(), CursorRequest::Image("text_tool_cursor"));
        t.on_mouse_down(&mut app, n[0], n[1], MouseButton::Left, &input);
        assert!(t.drag.is_some(), "hidden nub remains pressable");
        t.on_mouse_up(&mut app, n[0], n[1], MouseButton::Left, &input);
        for (p, inside) in [
            ([49.999, 59.999], true),
            ([50.0, 40.0], false),
            ([30.0, 60.0], false),
        ] {
            let (mut t, mut app) = edit("ab");
            t.layout.as_mut().unwrap().space_height = 5.0;
            t.affected = Some(Rectangle::new(15.0, 25.0, 30.0, 30.0));
            // Keep the nub away so this exercises the independent inflated-bounds branch.
            t.layout.as_mut().unwrap().lines.last_mut().unwrap().origin = [-100.0; 2];
            let old = t.click;
            t.on_mouse_down(&mut app, p[0], p[1], MouseButton::Left, &input);
            assert_eq!(t.click == old, inside, "point {p:?}");
        }
    }
    #[test]
    fn final_text_cursor_survives_ctrl_and_middle_pan_precedes_hidden_hit() {
        let (mut t, mut app) = edit("ab");
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        let n = t.nub(&app);
        t.on_mouse_move(&mut app, n[0], n[1], &input);
        t.pulse.sample(0.0, true, true, true, false, true);
        assert!(!t.pulse.nub_visible);
        assert_eq!(t.cursor(), CursorRequest::Image("hand_open_cursor"));
        app.tool = Some(Box::new(t));
        app.on_mouse_down(n[0], n[1], MouseButton::Middle, &input);
        assert!(app.pan_tool.is_tracking());
        assert_eq!(app.cursor(), CursorRequest::Image("hand_closed_cursor"));
        app.with_tool(|t, _| {
            assert!(
                t.as_any_mut()
                    .downcast_ref::<TextTool>()
                    .unwrap()
                    .drag
                    .is_none()
            )
        });
        app.on_mouse_up(n[0], n[1], MouseButton::Middle, &input);
        assert_eq!(app.cursor(), CursorRequest::Image("hand_open_cursor"));
        app.on_mouse_move(n[0], n[1], &input);
        assert_eq!(app.cursor(), CursorRequest::Image("text_tool_cursor"));
    }
    #[test]
    fn final_text_visibility_respects_key_order_and_direct_paste() {
        let (mut t, mut app) = edit(&"M".repeat(30));
        {
            let v = &mut app.active_ws_mut().unwrap().view;
            v.view_width = 100.0;
            v.view_height = 80.0;
            v.zoom = 1.0;
            v.set_text_view_center([50.0, 40.0]);
        }
        t.caret = (0, 0);
        let origin = app.active_ws().unwrap().view.text_viewport_bounds();
        key(&mut t, &mut app, "ArrowRight", true, false);
        assert_eq!(t.caret.1, 30);
        assert_eq!(
            app.active_ws().unwrap().view.text_viewport_bounds(),
            origin,
            "CtrlRight checks old caret before moving"
        );
        key(&mut t, &mut app, "a", false, false);
        assert!(app.active_ws().unwrap().view.text_viewport_bounds().x > origin.x);
        app.active_ws_mut()
            .unwrap()
            .view
            .set_text_view_center([50.0, 40.0]);
        t.caret = (0, 0);
        key(&mut t, &mut app, "End", false, false);
        assert!(
            app.active_ws().unwrap().view.text_viewport_bounds().x > origin.x,
            "End checks new caret"
        );
        for name in ["Control", "Shift", "Alt"] {
            app.active_ws_mut()
                .unwrap()
                .view
                .set_text_view_center([50.0, 40.0]);
            key(&mut t, &mut app, name, false, false);
            assert_eq!(app.active_ws().unwrap().view.text_viewport_bounds(), origin);
        }
        t.paste(&mut app, "pasted");
        assert_eq!(app.active_ws().unwrap().view.text_viewport_bounds(), origin);
        t.drag = Some(([0.0; 2], t.click));
        key(&mut t, &mut app, "a", false, false);
        assert!(
            app.active_ws().unwrap().view.text_viewport_bounds().x > origin.x,
            "tracking still checks visibility"
        );
    }
    #[test]
    fn final_text_status_is_cached_until_native_trigger_and_uses_first_line() {
        let (mut t, mut app) = edit("A");
        t.paste(&mut app, "\nB");
        t.layout.as_mut().unwrap().lines[0].origin = [85.0, 40.0];
        t.drag = Some(([0.0; 2], t.click));
        t.update_status(&mut app);
        assert_eq!(t.status_text(), Some("Anchor point: 85, 40"));
        app.active_ws_mut().unwrap().document.resolution = 100.0;
        app.measurement_unit = crate::app::MeasurementUnit::Inch;
        assert_eq!(
            t.status_text(),
            Some("Anchor point: 85, 40"),
            "unit change alone does not recompute"
        );
        t.caret = (1, 1);
        t.update_status(&mut app);
        assert_eq!(t.status_text(), Some("Anchor point: 0.85in, 0.40in"));
        t.drag = None;
        t.update_status(&mut app);
        let instruction = t.status.clone();
        t.commit(&mut app);
        assert_eq!(t.status, instruction);
    }
    #[test]
    fn final_text_view_center_rounds_bounds_and_floor_snaps_only_when_required() {
        let r = Rectangle::new(0.2, 0.2, 100.0, 100.0);
        assert_eq!(
            text_visibility_center(r, [95.0, 95.0], 20.0),
            Some([50.5; 2])
        );
        let (_, mut app) = edit("");
        let v = &mut app.active_ws_mut().unwrap().view;
        v.view_width = 100.0;
        v.view_height = 100.0;
        v.zoom = 1.0;
        v.set_text_view_center([50.5; 2]);
        assert_eq!(v.text_viewport_bounds().x, 0.0);
        v.set_text_view_center([49.8; 2]);
        assert_eq!(
            v.text_viewport_bounds().x,
            -1.0,
            "negative origins floor, not truncate"
        );
        v.zoom = 1.5;
        v.view_width = 150.0;
        v.view_height = 150.0;
        v.set_text_view_center([50.5; 2]);
        assert_eq!(v.text_viewport_bounds().x, 0.5);
        assert_eq!(
            portable_text_size(text_px_size(2000.0, FontSizeMetric::Points, 300.0)),
            (2000.0 * 300.0 / 72.0) as f32 as f64
        );
    }
    #[test]
    fn utf16_edits_keep_split_surrogates_and_prefixes() {
        let (mut t, mut app) = edit("a😀b");
        t.caret = (0, 1);
        key(&mut t, &mut app, "ArrowRight", false, false);
        assert_eq!(t.caret, (0, 2));
        key(&mut t, &mut app, "Backspace", false, false);
        assert_eq!(t.lines[0], [97, 0xde00, 98]);
        assert_eq!(String::from_utf16_lossy(&t.lines[0]), "a�b");
        t.redraw(&mut app);
        assert_eq!(t.lines[0], [97, 0xde00, 98]);
    }
    #[test]
    fn selector_bearing_caret_prefix_and_partial_delete_keep_raw_utf16() {
        const NOTO: &[u8] =
            include_bytes!("../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        let (mut t, mut app) = edit("☀\u{fe0f}X");
        assert_eq!(t.lines[0], [0x2600, 0xfe0f, 0x58]);
        let s = TextTool::settings(&app);
        let mut bytes = faces(&s);
        bytes.truncate(3);
        bytes.push(Arc::from(NOTO));
        let full = line(&t.lines[0][..2], &s, 12.0, &bytes);
        let prefix = line(&t.lines[0][..1], &s, 12.0, &bytes);
        assert!(matches!(full.items[0], DrawItem::Bitmap(_)));
        assert!(
            prefix
                .items
                .iter()
                .all(|i| matches!(i, DrawItem::Outline(_)))
        );
        assert_eq!(full.glyphs[0].utf16, 0..2);
        assert_eq!(full.advances, [full.width, 0.0]);
        assert_ne!(
            full.width, prefix.width,
            "caret prefix is measured independently"
        );
        t.caret = (0, 1); // between base and selector
        t.redraw(&mut app);
        assert_eq!(t.lines[0], [0x2600, 0xfe0f, 0x58]);
        key(&mut t, &mut app, "Delete", false, false);
        assert_eq!(t.lines[0], [0x2600, 0x58]);
        t.redraw(&mut app);
        assert_eq!(t.lines[0], [0x2600, 0x58]);

        let (mut t, mut app) = edit("🎂\u{fe0f}X");
        assert_eq!(t.lines[0], [0xd83c, 0xdf82, 0xfe0f, 0x58]);
        t.caret = (0, 2); // between surrogate pair and selector
        key(&mut t, &mut app, "Delete", false, false);
        assert_eq!(t.lines[0], [0xd83c, 0xdf82, 0x58]);
        t.redraw(&mut app);
        assert_eq!(t.lines[0], [0xd83c, 0xdf82, 0x58]);
    }
    #[test]
    fn word_movement_and_deletion_match_managed_units() {
        let (mut t, mut app) = edit("!!abc  def");
        t.caret = (0, 0);
        for expected in [1, 2, 7, 10] {
            key(&mut t, &mut app, "ArrowRight", true, false);
            assert_eq!(t.caret, (0, expected));
        }
        for expected in [7, 2, 0] {
            key(&mut t, &mut app, "ArrowLeft", true, false);
            assert_eq!(t.caret, (0, expected));
        }
        let (mut t, mut app) = edit("ab cd ef");
        t.caret = (0, 5);
        key(&mut t, &mut app, "Backspace", true, false);
        assert_eq!(t.lines[0], "ab ef".encode_utf16().collect::<Vec<_>>());
        assert_eq!(t.caret, (0, 2));
        let (mut t, mut app) = edit("ab cd ef");
        t.caret = (0, 3);
        key(&mut t, &mut app, "Delete", true, false);
        assert_eq!(t.lines[0], "ab ef".encode_utf16().collect::<Vec<_>>());
        assert_eq!(t.caret, (0, 3));
        // Ctrl+Shift selects by word (Paint.NET moved one character).
        key(&mut t, &mut app, "ArrowRight", true, true);
        assert_eq!(t.caret, (0, 5));
        assert_eq!(t.selected_text().as_deref(), Some("ef"));
    }
    #[test]
    fn enter_home_and_paste_control_rules() {
        let (mut t, mut app) = edit("abCD");
        t.caret = (0, 2);
        key(&mut t, &mut app, "Enter", true, true);
        assert_eq!(t.lines, vec![vec![97, 98], vec![67, 68]]);
        assert_eq!(t.caret, (1, 0));
        key(&mut t, &mut app, "End", true, false);
        assert_eq!(t.caret, (1, 2));
        key(&mut t, &mut app, "Home", true, false);
        assert_eq!(t.caret, (1, 0));
        t.paste(&mut app, "x\r\ny\rz\t");
        assert_eq!(
            t.lines,
            vec![vec![97, 98], vec![120], vec![121, 122, 67, 68]]
        );
        let (mut t, mut app) = edit("");
        let n = app.active_ws().unwrap().history.undo.len();
        t.paste(&mut app, "\n\n");
        assert_eq!(t.mode, EditMode::Empty);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        t.insert_text(&mut app, "\r");
        assert_eq!(t.mode, EditMode::Editing);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
    }
    #[test]
    fn delete_empty_keeps_placeholder_and_retyping_pushes_another() {
        let (mut t, mut app) = edit("a");
        let n = app.active_ws().unwrap().history.undo.len();
        t.caret = (0, 0);
        key(&mut t, &mut app, "Delete", false, false);
        assert_eq!(t.mode, EditMode::Empty);
        t.insert_text(&mut app, "b");
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
        t.commit(&mut app);
        let h = &app.active_ws().unwrap().history.undo;
        assert!(matches!(&h[n-1].kind,MementoKind::Compound(v) if v.is_empty()));
        assert!(matches!(&h[n].kind,MementoKind::Compound(v) if v.len()==1));
        let (mut t, mut app) = edit("a");
        key(&mut t, &mut app, "Backspace", false, false);
        assert_eq!(t.mode, EditMode::Editing);
        t.commit(&mut app);
        assert!(
            matches!(&app.active_ws().unwrap().history.undo.last().unwrap().kind,MementoKind::Compound(v) if v.len()==1)
        );
    }
    #[test]
    fn ime_is_provisional_and_scalar_deletion_preserves_unit_boundaries() {
        let (mut t, mut app) = edit("");
        let n = app.active_ws().unwrap().history.undo.len();
        t.ime_preedit(&mut app, "你");
        t.ime_preedit(&mut app, "你好");
        assert_eq!(t.lines, vec![Vec::<u16>::new()]);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        t.ime_preedit(&mut app, "");
        assert!(t.composition.is_none());
        t.ime_commit(&mut app, "😀a");
        assert_eq!(t.lines[0], [0xd83d, 0xde00, 97]);
        t.ime_delete_surrounding(&mut app, 2, usize::MAX);
        assert!(t.lines[0].is_empty());
        t.insert_text(&mut app, "x😀y");
        t.caret = (0, 2);
        t.ime_delete_surrounding(&mut app, 1, 1);
        assert_eq!(t.lines[0], [120, 121]);
        t.ime_preedit(&mut app, "draft");
        t.commit(&mut app);
        assert_eq!(t.mode, EditMode::None);
    }
    #[test]
    fn leading_uncovered_unit_disables_all_explicit_fallback_runs() {
        assert_eq!(
            fallback_units(&[97, 0xd83d, 98, 0xde00], |u| u < 128),
            [false, true, false, true]
        );
        assert_eq!(
            fallback_units(&[0xd83d, 97, 0xde00], |u| u < 128),
            [false, false, false]
        );
    }
    #[test]
    fn text_history_finalizes_pixels_and_document_parking_retains_edit() {
        let mut app = crate::testing::fresh_doc(120, 80, None);
        app.set_active_tool_from_type(ToolType::Text);
        app.with_tool(|t, app| {
            let t = t.as_any_mut().downcast_mut::<TextTool>().unwrap();
            t.start(app, [20.0, 20.0]);
            t.insert_text(app, "a");
        });
        let pixels = crate::testing::layer_bytes(&app, None);
        let old = app.active.unwrap();
        let other = app.create_blank_document_in_new_workspace(120, 80, 96.0, false);
        app.set_active_document_workspace(Some(old));
        app.with_tool(|t, _| {
            assert!(
                t.as_any_mut()
                    .downcast_mut::<TextTool>()
                    .unwrap()
                    .is_editing()
            )
        });
        assert_eq!(crate::testing::layer_bytes(&app, None), pixels);
        assert!(app.undo());
        assert!(
            crate::testing::layer_bytes(&app, None)
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [255; 4])
        );
        assert!(app.redo());
        assert_eq!(crate::testing::layer_bytes(&app, None), pixels);
        app.with_tool(|t, _| {
            assert!(
                !t.as_any_mut()
                    .downcast_mut::<TextTool>()
                    .unwrap()
                    .is_editing()
            )
        });
        app.set_active_document_workspace(Some(other));
    }
    #[test]
    fn text_right_chords_restart_snapshot_and_any_release_ends_move() {
        let (mut t, mut app) = edit("a");
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        t.click = [10.0, 20.0];
        t.on_mouse_down(&mut app, 4.9, -0.1, MouseButton::Right, &input);
        t.on_mouse_move(&mut app, 7.2, 2.8, &input);
        assert_eq!(t.click, [13.0, 23.0]);
        key(&mut t, &mut app, "Escape", false, false);
        assert!(t.drag.is_some());
        t.on_mouse_up(&mut app, 8.2, 3.8, MouseButton::Left, &input);
        assert_eq!(t.click, [14.0, 24.0]);
        assert!(t.drag.is_none());
    }
    #[test]
    fn tracking_paste_consumes_units_splits_lf_and_runs_base_color_shortcut() {
        let (mut t, mut app) = edit("");
        let n = app.active_ws().unwrap().history.undo.len();
        let before = (app.primary_color, app.secondary_color);
        t.drag = Some(([0.0, 0.0], t.click));
        assert!(t.paste(&mut app, "x\nA"));
        assert_eq!(t.lines, vec![Vec::<u16>::new(), vec![]]);
        assert_eq!(t.mode, EditMode::Empty);
        assert!(t.drag.is_some());
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        assert_eq!(
            (app.primary_color, app.secondary_color),
            (before.1, before.0)
        );
    }

    #[test]
    fn ordinary_paste_offers_text_before_commit_first_image_path() {
        let mut app = crate::testing::fresh_doc(120, 80, None);
        app.set_active_tool_from_type(ToolType::Text);
        app.with_tool(|t, app| {
            let t = t.as_any_mut().downcast_mut::<TextTool>().unwrap();
            t.start(app, [20.0, 20.0]);
            t.insert_text(app, "a");
        });
        crate::action::perform(&mut app, "menu.edit.paste");
        app.with_tool(|t, _| {
            assert!(
                t.as_any_mut()
                    .downcast_mut::<TextTool>()
                    .unwrap()
                    .is_editing()
            )
        });
    }

    #[test]
    fn ime_enter_accepts_only_commit_and_plain_enter_still_splits() {
        let (mut t, mut app) = edit("");
        let n = app.active_ws().unwrap().history.undo.len();
        t.ime_preedit(&mut app, "你");
        key(&mut t, &mut app, "Enter", false, false);
        assert_eq!(t.lines, vec![Vec::<u16>::new()]);
        assert!(t.composition.is_some());
        t.ime_commit(&mut app, "你");
        assert_eq!(t.lines, vec![vec![0x4f60]]);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
        key(&mut t, &mut app, "Enter", false, false);
        assert_eq!(t.lines, vec![vec![0x4f60], vec![]]);
    }
    #[test]
    fn text_pulse_targets_and_nub_formula() {
        let (mut t, mut app) = edit("A");
        for (ms, on) in [
            (0.0, true),
            (299.0, true),
            (300.0, false),
            (599.0, false),
            (600.0, true),
        ] {
            t.pulse.sample(ms, true, true, false, false, true);
            assert_eq!(t.pulse.caret_target, on);
        }
        for (ms, alpha) in [
            (0.0, 0.75),
            (500.0, 1.0),
            (1000.0, 0.75),
            (1500.0, 0.25),
            (2000.0, 0.75),
        ] {
            t.pulse.sample(ms, true, true, false, false, true);
            assert!((t.pulse.nub_opacity - alpha).abs() < 1e-6);
        }
        for (focused, active, ctrl, tracking, caret, nub) in [
            (true, true, true, false, false, false),
            (true, false, true, false, true, true),
            (false, true, false, false, false, true),
            (true, true, false, true, true, false),
        ] {
            t.pulse.sample(0.0, focused, active, ctrl, tracking, true);
            assert_eq!(t.pulse.caret_target, caret);
            assert_eq!(t.pulse.nub_visible, nub);
        }
        app.now_ms = 123.0;
        key(&mut t, &mut app, "a", false, false);
        assert_eq!(t.pulse.epoch, 123.0);
        app.now_ms = 140.0;
        t.insert_text(&mut app, "a");
        t.redraw(&mut app);
        key(&mut t, &mut app, " ", false, false);
        assert_eq!(t.pulse.epoch, 123.0);
        t.paste(&mut app, "B");
        assert_eq!(t.pulse.epoch, 123.0);
        assert!(t.pulse.nub_visible);
    }
    #[test]
    fn tracking_paste_color_shortcuts_require_workspace_focus() {
        let (mut t, mut app) = edit("AB");
        t.drag = Some(([0.0, 0.0], t.click));
        t.workspace_focused = false;
        let colors = (app.primary_color, app.secondary_color);
        t.paste(&mut app, "x\n");
        assert_eq!((app.primary_color, app.secondary_color), colors);
        assert_eq!(t.lines.len(), 2);
        assert!(t.drag.is_some());
    }
    #[test]
    fn text_pulse_command_resets_and_modifier_edges() {
        for name in ["Tab", "PageUp", "PageDown", "a", "Home", "ArrowLeft"] {
            for ctrl in [false, true] {
                let (mut t, mut app) = edit("AB");
                app.now_ms = 100.0;
                key(&mut t, &mut app, name, ctrl, true);
                assert_eq!(t.pulse.epoch, 100.0, "{name} ctrl={ctrl}");
                app.now_ms = 200.0;
                app.alt_key_down = true;
                key(&mut t, &mut app, name, ctrl, false);
                assert_eq!(t.pulse.epoch, 100.0);
                app.alt_key_down = false;
                t.drag = Some(([0.0, 0.0], t.click));
                key(&mut t, &mut app, name, ctrl, false);
                assert_eq!(t.pulse.epoch, 100.0);
            }
        }
        let (mut t, mut app) = edit("AB");
        t.ime_preedit(&mut app, "中");
        app.now_ms = 100.0;
        key(&mut t, &mut app, "Enter", false, false);
        assert_eq!(t.pulse.epoch, 0.0);
        t.update_pulse(&mut app, true, true, false, true);
        app.control_key_down = true;
        t.on_modifier_keys_changed(&mut app);
        assert_eq!(t.pulse.epoch, 100.0);
        app.now_ms = 200.0;
        app.control_key_down = false;
        t.on_modifier_keys_changed(&mut app);
        assert_eq!(t.pulse.epoch, 100.0);
        t.workspace_focused = false;
        app.shift_key_down = true;
        t.on_modifier_keys_changed(&mut app);
        assert_eq!(t.pulse.epoch, 100.0);
    }
    #[test]
    fn text_pulse_redraw_retarget_and_restart_preserve_alpha() {
        let (mut t, mut app) = edit("A");
        t.pulse.sample(0.0, true, true, false, false, true);
        t.pulse.sample(300.0, true, true, false, false, true);
        app.now_ms = 350.0;
        let alpha = t.pulse.opacity(app.now_ms);
        assert!(alpha > 0.0 && alpha < 1.0);
        t.redraw(&mut app);
        assert_eq!(t.pulse.opacity(app.now_ms), alpha);
        t.pulse.sample(app.now_ms, false, true, false, false, true);
        assert_eq!(t.pulse.fade_start, 300.0); // unchanged target never restarts a transition
        t.pulse.epoch = app.now_ms;
        t.pulse.sample(app.now_ms, true, true, false, false, true);
        assert_eq!(t.pulse.opacity(app.now_ms), alpha);
        t.commit(&mut app);
        app.now_ms = 400.0;
        t.start(&mut app, [20.0, 20.0]);
        assert_eq!(t.pulse.opacity(app.now_ms), alpha);
        t.pulse.from = 0.5;
        t.pulse.to = 0.5;
        assert_eq!(t.caret_alpha(app.now_ms), 127);
    }
    #[test]
    fn text_pulse_delivery_and_paste_visibility_are_not_per_frame() {
        let (mut t, mut app) = edit("A");
        t.drag = Some(([0.0, 0.0], t.click));
        t.paste(&mut app, "B");
        app.now_ms = 15.0;
        t.update_pulse(&mut app, true, true, false, true);
        assert!(t.pulse.nub_visible);
        app.now_ms = 16.0;
        t.update_pulse(&mut app, true, true, false, true);
        assert!(!t.pulse.nub_visible);
        t.paste(&mut app, "B");
        for ms in [32.0, 48.0] {
            app.now_ms = ms;
            t.update_pulse(&mut app, true, false, false, true);
            assert!(t.pulse.nub_visible);
        }
        app.now_ms = 64.0;
        t.update_pulse(&mut app, true, false, false, true);
        assert!(!t.pulse.nub_visible);
        t.paste(&mut app, "B");
        app.now_ms = 1000.0;
        t.update_pulse(&mut app, true, true, true, true);
        assert!(t.pulse.nub_visible);
        t.update_pulse(&mut app, true, true, false, true);
        assert!(!t.pulse.nub_visible);
    }
    #[test]
    fn paste_tracking_preserves_mode_history_and_nub_then_pulse_hides_it() {
        for accepted in ["", "AB"] {
            for pasted in ["abc", "\n", "\r\n", "A\nB\n", "\t", " "] {
                let (mut t, mut app) = edit(accepted);
                let original = t.lines[0].clone();
                let mode = t.mode;
                let n = app.active_ws().unwrap().history.undo.len();
                t.drag = Some(([0.0, 0.0], t.click));
                assert!(t.paste(&mut app, pasted));
                assert_eq!(t.mode, mode);
                assert!(t.drag.is_some());
                assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
                assert_eq!(t.lines[0], original);
                assert_eq!(
                    t.lines.len(),
                    1 + pasted.bytes().filter(|&b| b == b'\n').count()
                );
                let id = t.handles.unwrap();
                // The nub (and the rotate handle once there is text).
                assert!(
                    matches!(app.active_ws().unwrap().surface_box.renderer(id),Some(Renderer::Handles(h)) if h[0].shape == HandleShape::Compass)
                );
                app.now_ms += 16.0;
                t.update_pulse(&mut app, true, true, false, true);
                assert!(
                    matches!(app.active_ws().unwrap().surface_box.renderer(id),Some(Renderer::Handles(h)) if h.is_empty())
                );
                assert!(!t.paste(&mut app, ""));
            }
        }
    }
    #[test]
    fn vertical_movement_loses_prior_x_and_hit_lookup_is_prefix_ceiling() {
        let (mut t, mut app) = edit("abcdef");
        key(&mut t, &mut app, "Enter", false, false);
        t.insert_text(&mut app, "a");
        key(&mut t, &mut app, "Enter", false, false);
        t.insert_text(&mut app, "abcdef");
        key(&mut t, &mut app, "ArrowUp", false, false);
        assert_eq!(t.caret, (1, 1));
        key(&mut t, &mut app, "ArrowUp", false, false);
        assert_eq!(t.caret, (0, 1));
        let l = t.layout.as_ref().unwrap().lines[0].clone();
        let first = l.advances[0];
        t.hit_position(&app, l.origin[0] + first * 0.1, l.origin[1]);
        assert_eq!(t.caret, (0, 1));
        t.hit_position(&app, l.origin[0] + first, l.origin[1]);
        assert_eq!(t.caret, (0, 1));
        t.hit_position(&app, l.origin[0] + first + 0.01, l.origin[1]);
        assert_eq!(t.caret, (0, 2));
    }
    #[test]
    fn clicking_first_line_center_keeps_first_line() {
        let (mut t, mut app) = edit("abcdef");
        key(&mut t, &mut app, "Enter", false, false);
        t.insert_text(&mut app, "abcdef");
        let x = t.click[0] + t.layout.as_ref().unwrap().lines[0].advances[0] * 0.1;
        let y = t.click[1];
        t.on_mouse_down(
            &mut app,
            x,
            y,
            MouseButton::Left,
            &PointerInput {
                pointer_type: super::super::PointerType::Mouse,
                samples: vec![],
            },
        );
        assert_eq!(t.caret, (0, 1));
    }
    #[test]
    fn centered_hit_test_uses_logical_unrounded_prefix_edges() {
        let (mut t, mut app) = edit("abcdef");
        app.tool_settings
            .set(ToolType::Text, "align", serde_json::json!("center"));
        t.redraw(&mut app);
        let l = &t.layout.as_ref().unwrap().lines[0];
        let x = logical_position(
            t.click,
            [l.advances[0], l.height],
            l.width,
            0,
            TextAlign::Center,
        )[0];
        let y = t.click[1];
        t.hit_position(&app, x, y);
        assert_eq!(t.caret, (0, 1));
        t.hit_position(&app, x + 0.001, y);
        assert_eq!(t.caret, (0, 2));
        let s = TextTool::settings(&app);
        let empty = layout_text("", &s, [0.0, 0.0], 96.0);
        let space = layout_text(" ", &s, [0.0, 0.0], 96.0);
        assert_eq!(empty.lines[0].width, 0.0);
        assert_eq!(empty.lines[0].height, space.lines[0].height);
    }
    #[test]
    fn logical_position_has_native_float_arithmetic_and_prefix_height() {
        assert_eq!(
            logical_position([10.0, 20.0], [3.25, 7.5], 8.5, 2, TextAlign::Center),
            [9.0, 35.0]
        );
        assert_eq!(
            logical_position([16777217.0, 20.0], [0.0, 7.5], 0.0, 0, TextAlign::Left),
            [16777216.0, 20.0]
        );
    }
    #[test]
    fn color_glyphs_render_intrinsic_palette_through_text_transaction() {
        // Removing bitmap paint or tinting it with primary RGB must fail this check.
        let bytes = include_bytes!("../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        fonts::register_lazy_face("task35c-palette", FaceStyle::Regular, bytes.to_vec()).unwrap();
        let (mut t, mut app) = edit("");
        app.tool_settings.set(
            ToolType::Text,
            "fontFamily",
            serde_json::json!("task35c-palette"),
        );
        app.tool_settings
            .set(ToolType::Text, "fontSize", serde_json::json!(48));
        t.insert_text(&mut app, "🎂");
        let pixels = crate::testing::layer_bytes(&app, None);
        let colored: Vec<_> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 128 && p[0] != p[1] && p[1] != p[2])
            .collect();
        assert!(
            colored.len() > 50,
            "actual cake palette must survive Text's content compositor; got {} colored pixels",
            colored.len()
        );
    }

    #[test]
    fn color_font_completion_repaints_parked_owner_without_new_history() {
        fonts::register_lazy_face(
            "task35c-owner",
            FaceStyle::Regular,
            fonts::SELAWIK_REGULAR.to_vec(),
        )
        .unwrap();
        let mut app = crate::testing::fresh_doc(120, 80, None);
        app.tool_settings.set(
            ToolType::Text,
            "fontFamily",
            serde_json::json!("task35c-owner"),
        );
        app.set_active_tool_from_type(ToolType::Text);
        app.with_tool(|t, app| {
            let t = t.as_any_mut().downcast_mut::<TextTool>().unwrap();
            t.start(app, [20.0, 20.0]);
            t.insert_text(app, "MMMM");
        });
        let old = app.active.unwrap();
        let before = crate::testing::layer_bytes(&app, None);
        let history: Vec<_> = app
            .active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.id)
            .collect();
        app.create_blank_document_in_new_workspace(120, 80, 96.0, false);
        let other = crate::testing::layer_bytes(&app, None);
        fonts::register_lazy_face(
            "task35c-owner",
            FaceStyle::Regular,
            fonts::CARLITO_REGULAR.to_vec(),
        )
        .unwrap();
        app.on_frame(10.0);
        assert_eq!(crate::testing::layer_bytes(&app, None), other);
        app.set_active_document_workspace(Some(old));
        app.on_frame(20.0);
        assert!(
            crate::testing::layer_bytes(&app, None) != before,
            "returning to unchanged geometry must observe new font bytes"
        );
        assert_eq!(
            app.active_ws()
                .unwrap()
                .history
                .undo
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            history
        );
    }
    #[test]
    fn color_completion_does_not_resurrect_canceled_preedit_or_closed_owner() {
        let family = "task35c-canceled";
        fonts::register_lazy_face(family, FaceStyle::Regular, fonts::SELAWIK_REGULAR.to_vec())
            .unwrap();
        let (mut t, mut app) = edit("");
        app.tool_settings
            .set(ToolType::Text, "fontFamily", serde_json::json!(family));
        t.ime_preedit(&mut app, "🎂");
        assert!(t.needs_color_font(&app));
        key(&mut t, &mut app, "Escape", false, false);
        assert!(!t.needs_color_font(&app));
        let before = crate::testing::layer_bytes(&app, None);
        let history: Vec<_> = app
            .active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.id)
            .collect();
        let bytes = include_bytes!("../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        fonts::register_lazy_face(family, FaceStyle::Regular, bytes.to_vec()).unwrap();
        t.on_frame(&mut app);
        assert_eq!(crate::testing::layer_bytes(&app, None), before);
        assert_eq!(
            app.active_ws()
                .unwrap()
                .history
                .undo
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            history
        );
        assert!(t.lines.iter().all(Vec::is_empty));
        // A real parked tool is dropped on closing its image; a late completion cannot act on
        // the next image, even when it has another Text instance with the same settings.
        app.tool = Some(Box::new(t));
        let closed = app.active.unwrap();
        app.create_blank_document_in_new_workspace(120, 80, 96.0, false);
        let other = crate::testing::layer_bytes(&app, None);
        let history: Vec<_> = app
            .active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.id)
            .collect();
        assert!(app.close_document_workspace(closed));
        fonts::register_lazy_face(family, FaceStyle::Regular, fonts::CARLITO_REGULAR.to_vec())
            .unwrap();
        app.on_frame(25.0);
        assert_eq!(crate::testing::layer_bytes(&app, None), other);
        assert_eq!(
            app.active_ws()
                .unwrap()
                .history
                .undo
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            history
        );
    }
    #[test]
    fn color_font_completion_after_commit_preserves_saved_transaction_pixels() {
        let family = "task35c-committed";
        fonts::register_lazy_face(family, FaceStyle::Regular, fonts::SELAWIK_REGULAR.to_vec())
            .unwrap();
        let (mut t, mut app) = edit("");
        app.tool_settings
            .set(ToolType::Text, "fontFamily", serde_json::json!(family));
        t.insert_text(&mut app, "🎂");
        t.commit(&mut app);
        let before = crate::testing::layer_bytes(&app, None);
        let history: Vec<_> = app
            .active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.id)
            .collect();
        fonts::register_lazy_face(
            family,
            FaceStyle::Regular,
            include_bytes!("../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf").to_vec(),
        )
        .unwrap();
        t.on_frame(&mut app);
        t.on_setting_changed(&mut app, "fontFamily");
        assert_eq!(crate::testing::layer_bytes(&app, None), before);
        assert_eq!(
            app.active_ws()
                .unwrap()
                .history
                .undo
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            history
        );
    }
    #[test]
    fn color_extension_ordinary_outline_baseline() {
        let (_, app) = edit("");
        let mut settings = TextTool::settings(&app);
        let mut hashes = vec![];
        for mode in [
            TextRenderingMode::Smooth,
            TextRenderingMode::Sharp,
            TextRenderingMode::Classic,
        ] {
            for aa in [true, false] {
                for style in [false, true] {
                    settings.mode = mode;
                    settings.antialias = aa;
                    settings.bold = style;
                    settings.italic = style;
                    settings.underline = style;
                    settings.strikeout = style;
                    let layout = layout_text("Abé 8g\n \nΩQ", &settings, [20.0, 30.0], 96.0);
                    let mask = rasterize_text(&layout, &settings, IntRect::new(0, 0, 120, 80));
                    hashes.push(mask.iter().fold(14695981039346656037u64, |h, b| {
                        (h ^ u64::from(*b)).wrapping_mul(1099511628211)
                    }));
                }
            }
        }
        assert_eq!(
            hashes,
            [
                7735873197622586517,
                8515749305184134107,
                15693865397859997102,
                17532759746238768281,
                8447946342614716856,
                14444803708182668300,
                15693865397859997102,
                17532759746238768281,
                3552981683738885835,
                14508122127939849123,
                15693865397859997102,
                17532759746238768281
            ]
        );
    }

    // PC16 (S§4.5): the Editable text toggle.
    use crate::testing::{Ev, drive_tool, layer_bytes};
    /// A 60×40 white document with the Text tool active and Editable text `on`.
    fn editable_app(on: bool) -> AppWorkspace {
        let mut app = crate::testing::fresh_doc(60, 40, None);
        app.tool_settings
            .set(ToolType::Text, "editableText", on.into());
        app.set_active_tool_from_type(ToolType::Text);
        app
    }
    fn click(app: &mut AppWorkspace, x: f64, y: f64) {
        let b = MouseButton::Left;
        drive_tool(app, ToolType::Text, &[Ev::Down(x, y, b), Ev::Up(x, y, b)]);
    }
    fn type_text(app: &mut AppWorkspace, text: &str) {
        crate::testing::with_tool::<TextTool>(app, |t, app| t.insert_text(app, text));
    }
    fn escape(app: &mut AppWorkspace) {
        drive_tool(app, ToolType::Text, &[Ev::Key("Escape")]);
    }
    fn names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }
    fn spec_of(app: &AppWorkspace, i: usize) -> Option<crate::document::text_layer::TextSpec> {
        let ws = app.active_ws().unwrap();
        ws.document.layers.get_at(i)?.text.clone()
    }
    fn layer_count(app: &AppWorkspace) -> usize {
        app.active_ws().unwrap().document.layers.size()
    }
    /// Off is Paint.NET: the text lands in the active layer's pixels as one "Text" item.
    #[test]
    fn toggle_off_is_paint_net() {
        let mut app = editable_app(false);
        let (before, n) = (layer_bytes(&app, None), names(&app).len());
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        escape(&mut app);
        assert_eq!(layer_count(&app), 1);
        assert_eq!(spec_of(&app, 0), None);
        assert_ne!(layer_bytes(&app, None), before);
        assert_eq!(names(&app)[n..], ["Text"]);
    }
    /// On: the commit is a new text layer above the active one, named after the text, one item
    /// that undo removes and redo brings back; the layer below keeps its pixels.
    #[test]
    fn toggle_on_creates_a_text_layer() {
        let mut app = editable_app(true);
        app.tool_settings
            .set(ToolType::Text, "blendMode", "multiply".into());
        let (background, n) = (layer_bytes(&app, None), names(&app).len());
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi there, a long line");
        escape(&mut app);
        assert_eq!(names(&app)[n..], ["Text"]);
        assert_eq!(layer_count(&app), 2);
        let spec = spec_of(&app, 1).expect("a text layer");
        assert_eq!(
            (spec.text.as_str(), spec.origin, spec.color, spec.dpi),
            ("Hi there, a long line", (8.0, 10.0), [0, 0, 0, 255], 96.0)
        );
        assert_eq!(spec.settings, TextTool::settings(&app));
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.active_layer, 1);
        let layer = ws.document.layers.get_at(1).unwrap();
        assert_eq!(layer.properties.name, "Hi there, a long lin");
        let rendered = crate::document::text_layer::render(&spec, 60, 40);
        assert_eq!(layer.surface.data(), rendered.data());
        assert!(rendered.data().chunks(4).any(|p| p[3] > 0), "no ink");
        assert_eq!(layer_bytes(&app, Some(0)), background);
        assert!(app.undo());
        assert_eq!(layer_count(&app), 1);
        assert_eq!(layer_bytes(&app, Some(0)), background);
        assert!(app.redo());
        assert_eq!(spec_of(&app, 1), Some(spec));
        assert_eq!(layer_bytes(&app, Some(1)), rendered.data());
    }
    /// A click on the active text layer's text re-enters editing with its spec: its settings and
    /// colour, the caret at the click, the nub shown. Committing replaces the spec as one item.
    #[test]
    fn clicking_a_text_layer_reenters_editing() {
        let mut app = editable_app(true);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        escape(&mut app);
        let old = spec_of(&app, 1).unwrap();
        let old_pixels = layer_bytes(&app, Some(1));
        app.tool_settings.set(ToolType::Text, "bold", true.into());
        app.set_colors(crate::util::color::Color::WHITE, app.secondary_color);
        let layout = layout_text(&old.text, &old.settings, [8.0, 10.0], 96.0);
        let b = line_bounds(&layout.lines[0]);
        let n = names(&app).len();
        click(&mut app, b.right() - 1.0, b.y + b.height / 2.0);
        assert!(app.ui_requests.is_empty(), "no rasterize prompt");
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            assert!(t.can_finish());
            assert_eq!(t.lines, ["Hi".encode_utf16().collect::<Vec<_>>()]);
            assert_eq!((t.click, t.caret), ([8.0, 10.0], (0, 2)));
            assert!(t.pulse.nub_visible && t.handles.is_some());
        });
        assert_eq!(TextTool::settings(&app), old.settings);
        let c = app.primary_color;
        assert_eq!([c.red, c.green, c.blue, c.alpha], old.color);
        // Unchanged so far: no item.
        assert_eq!(names(&app).len(), n);
        type_text(&mut app, "!");
        escape(&mut app);
        assert_eq!(layer_count(&app), 2);
        assert_eq!(names(&app)[n..], ["Text"]);
        let new = spec_of(&app, 1).unwrap();
        assert_eq!((new.text.as_str(), new.origin), ("Hi!", old.origin));
        let new_pixels = layer_bytes(&app, Some(1));
        let rendered = crate::document::text_layer::render(&new, 60, 40);
        assert_eq!(new_pixels, rendered.data());
        assert!(app.undo());
        assert_eq!(
            (spec_of(&app, 1), layer_bytes(&app, Some(1))),
            (Some(old.clone()), old_pixels.clone())
        );
        assert!(app.redo());
        assert_eq!(
            (spec_of(&app, 1), layer_bytes(&app, Some(1))),
            (Some(new.clone()), new_pixels)
        );
        // Re-entering and leaving without a change adds nothing and keeps the pixels.
        let pixels = layer_bytes(&app, Some(1));
        click(&mut app, b.x + 1.0, b.y + b.height / 2.0);
        escape(&mut app);
        assert_eq!(names(&app).len(), n + 1);
        assert_eq!(
            (spec_of(&app, 1), layer_bytes(&app, Some(1))),
            (Some(new), pixels)
        );
        // A click off the text starts another text layer above it.
        click(&mut app, 30.0, 30.0);
        type_text(&mut app, "x");
        escape(&mut app);
        assert_eq!(layer_count(&app), 3);
        assert_eq!(spec_of(&app, 2).unwrap().text, "x");
    }
    /// The text layer's pixels are what Paint.NET mode commits on an empty layer.
    #[test]
    fn editable_text_matches_committed_text_pixels() {
        let run = |on: bool| {
            let mut app = editable_app(on);
            if !on {
                crate::action::perform(&mut app, "menu.layers.add.new.layer");
            }
            click(&mut app, 8.0, 10.0);
            type_text(&mut app, "Hi\nthere");
            escape(&mut app);
            layer_bytes(&app, Some(1))
        };
        let pdn = run(false);
        assert!(pdn.chunks(4).any(|p| p[3] > 0), "no ink");
        assert_eq!(run(true), pdn);
    }
    /// Upstream #5956: while typing, a drag from just outside the text moves it, and typing
    /// goes on; off, the same press places the caret, as in Paint.NET.
    #[test]
    fn drag_just_outside_moves_text() {
        for on in [true, false] {
            let mut app = editable_app(on);
            click(&mut app, 8.0, 10.0);
            type_text(&mut app, "Hi");
            let mut r = Rectangle::default();
            crate::testing::with_tool::<TextTool>(&mut app, |t, _| r = t.affected.unwrap());
            let (x, y) = (r.right() + 4.0, r.y + r.height / 2.0);
            let b = MouseButton::Left;
            let drag = [
                Ev::Down(x, y, b),
                Ev::Move(x + 10.0, y + 5.0),
                Ev::Up(x + 10.0, y + 5.0, b),
            ];
            drive_tool(&mut app, ToolType::Text, &drag);
            crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
                assert!(t.is_editing(), "{on}");
                let moved = if on { [18.0, 15.0] } else { [8.0, 10.0] };
                assert_eq!(t.click, moved, "{on}");
            });
            if on {
                escape(&mut app);
                assert_eq!(spec_of(&app, 1).unwrap().origin, (18.0, 15.0));
            }
        }
    }

    // PC16 fix round 1.
    type Spec = crate::document::text_layer::TextSpec;
    /// The rendered bounds of `spec`'s text, at its own DPI.
    fn bounds(spec: &Spec) -> Rectangle {
        let origin = [spec.origin.0, spec.origin.1];
        let layout = layout_text(&spec.text, &spec.settings, origin, spec.dpi);
        let lines = layout.lines.iter().map(line_bounds);
        lines.reduce(|a, b| Rectangle::union(&a, &b)).unwrap()
    }
    /// "Hi" at (8, 10) as text layer 1 of the white document, Editable text on.
    fn hi_layer() -> (AppWorkspace, Spec, Rectangle) {
        let mut app = editable_app(true);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        escape(&mut app);
        let spec = spec_of(&app, 1).unwrap();
        let b = bounds(&spec);
        (app, spec, b)
    }
    fn press_key(app: &mut AppWorkspace, k: &'static str) {
        drive_tool(app, ToolType::Text, &[Ev::Key(k)]);
    }
    fn active(app: &AppWorkspace) -> usize {
        app.active_ws().unwrap().active_layer
    }
    /// Fix 1: re-entry writes the text's antialias (shared with other tools, R17) and colour;
    /// leaving puts back what was there, unless it changed meanwhile.
    #[test]
    fn reentry_restores_shared_settings() {
        let mut app = editable_app(true);
        app.tool_settings
            .set(ToolType::Text, "antialias", false.into());
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        escape(&mut app);
        let spec = spec_of(&app, 1).unwrap();
        assert_eq!(
            (spec.settings.antialias, spec.color),
            (false, [0, 0, 0, 255])
        );
        let b = bounds(&spec);
        let (red, black) = (Color::new(200, 10, 10, 255), Color::new(0, 0, 0, 255));
        app.tool_settings
            .set(ToolType::PaintBrush, "antialias", true.into());
        app.set_colors(red, app.secondary_color);
        let brush = |app: &AppWorkspace| app.tool_settings.get(ToolType::PaintBrush, "antialias");
        let inside = (b.x + 1.0, b.y + b.height / 2.0);
        click(&mut app, inside.0, inside.1);
        assert_eq!((brush(&app), app.primary_color), (false.into(), black));
        escape(&mut app);
        assert_eq!((brush(&app), app.primary_color), (true.into(), red));
        // A colour picked during the edit stays, and is the text's.
        click(&mut app, inside.0, inside.1);
        let blue = Color::new(10, 10, 200, 255);
        app.set_colors(blue, app.secondary_color);
        escape(&mut app);
        assert_eq!(app.primary_color, blue);
        assert_eq!(spec_of(&app, 1).unwrap().color, [10, 10, 200, 255]);
    }
    /// Fix 2: new text previews on its own layer, there from the first keystroke, so a hidden
    /// active layer still shows it; the preview is the result.
    #[test]
    fn new_text_previews_on_its_own_layer() {
        let mut app = editable_app(true);
        let ws = app.active_ws_mut().unwrap();
        ws.document.layers.get_at_mut(0).unwrap().properties.visible = false;
        ws.invalidate(None);
        let background = layer_bytes(&app, Some(0));
        click(&mut app, 8.0, 10.0);
        assert_eq!(layer_count(&app), 1, "no layer before typing");
        type_text(&mut app, "Hi");
        assert_eq!((layer_count(&app), active(&app)), (2, 1));
        let mut spec = None;
        crate::testing::with_tool::<TextTool>(&mut app, |t, app| spec = Some(t.spec(app)));
        let rendered = crate::document::text_layer::render(&spec.unwrap(), 60, 40);
        assert_eq!(layer_bytes(&app, Some(1)), rendered.data());
        let composite = app.active_ws().unwrap().document.composite();
        assert!(composite.data().chunks(4).any(|p| p[3] > 0), "not shown");
        escape(&mut app);
        assert_eq!(layer_bytes(&app, Some(1)), rendered.data());
        assert_eq!(layer_bytes(&app, Some(0)), background);
    }
    /// R74: a press hits the topmost visible text layer under it, which becomes active with no
    /// history item; a hidden one's text never re-opens.
    #[test]
    fn a_press_reenters_the_topmost_visible_text_layer() {
        let (mut app, spec, b) = hi_layer();
        crate::action::perform(&mut app, "menu.layers.add.new.layer");
        assert_eq!(active(&app), 2);
        let n = names(&app).len();
        let (x, y) = (b.x + 1.0, b.y + b.height / 2.0);
        click(&mut app, x, y);
        assert_eq!((active(&app), names(&app).len()), (1, n));
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            assert!(t.can_finish());
            assert_eq!(String::from_utf16_lossy(&t.lines[0]), spec.text);
        });
        escape(&mut app);
        // Hidden: the press starts new text on the active layer.
        let hide = |app: &mut AppWorkspace, visible| {
            let ws = app.active_ws_mut().unwrap();
            ws.document.layers.get_at_mut(1).unwrap().properties.visible = visible;
            ws.invalidate(None);
        };
        hide(&mut app, false);
        app.set_active_layer(2);
        click(&mut app, x, y);
        assert_eq!(active(&app), 2);
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            assert!(!t.can_finish() && t.lines == [Vec::<u16>::new()]);
        });
        type_text(&mut app, "Yo");
        escape(&mut app);
        let yo = spec_of(&app, 3).unwrap();
        assert_eq!(yo.text, "Yo");
        // Both shown: the top one is hit.
        hide(&mut app, true);
        app.set_active_layer(0);
        let both = Rectangle::intersect_of(&b, &bounds(&yo));
        click(
            &mut app,
            both.x + both.width / 2.0,
            both.y + both.height / 2.0,
        );
        assert_eq!(active(&app), 3);
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            assert_eq!(String::from_utf16_lossy(&t.lines[0]), "Yo");
        });
    }
    /// R76: new text emptied before its commit makes no layer; a re-edit committed empty deletes
    /// its layer as one item that undo reverts. Picking a layer by its index from before that
    /// commit still picks it.
    #[test]
    fn empty_text_makes_no_layer_and_an_emptied_layer_goes() {
        let mut app = editable_app(true);
        click(&mut app, 8.0, 10.0);
        escape(&mut app);
        assert_eq!(layer_count(&app), 1);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "a");
        press_key(&mut app, "Backspace");
        assert_eq!((layer_count(&app), active(&app)), (1, 0));
        escape(&mut app);
        assert_eq!(layer_count(&app), 1);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        escape(&mut app);
        crate::action::perform(&mut app, "menu.layers.add.new.layer");
        let (spec, pixels) = (spec_of(&app, 1).unwrap(), layer_bytes(&app, Some(1)));
        let b = bounds(&spec);
        click(&mut app, b.right() - 1.0, b.y + b.height / 2.0);
        press_key(&mut app, "Backspace");
        press_key(&mut app, "Backspace");
        let n = names(&app).len();
        app.set_active_layer(2);
        assert_eq!((layer_count(&app), names(&app).len()), (2, n + 1));
        assert_eq!(active(&app), 1, "the layer that was above");
        assert_eq!((spec_of(&app, 0), spec_of(&app, 1)), (None, None));
        assert!(app.undo());
        assert_eq!(
            (
                layer_count(&app),
                spec_of(&app, 1),
                layer_bytes(&app, Some(1))
            ),
            (3, Some(spec), pixels)
        );
        assert!(app.redo());
        assert_eq!(layer_count(&app), 2);
    }
    /// Fix 5: the hit test and the edit lay out at the layer's own DPI, so an unchanged re-edit
    /// after a DPI-only Resize adds nothing.
    #[test]
    fn unchanged_reedit_after_a_dpi_change_adds_nothing() {
        let (mut app, spec, b) = hi_layer();
        let pixels = layer_bytes(&app, Some(1));
        app.active_ws_mut().unwrap().document.resolution = 300.0;
        let n = names(&app).len();
        click(&mut app, b.x + 1.0, b.y + b.height / 2.0);
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| assert!(t.can_finish()));
        escape(&mut app);
        assert_eq!(names(&app).len(), n);
        assert_eq!(
            (spec_of(&app, 1), layer_bytes(&app, Some(1))),
            (Some(spec), pixels)
        );
    }
    /// Fix 8: a tool switch or an undo during a re-edit ends it as one item, which undo reverts
    /// exactly.
    #[test]
    fn tool_switch_and_undo_end_a_reedit_as_one_item() {
        let (mut app, old, b) = hi_layer();
        let old_pixels = layer_bytes(&app, Some(1));
        let n = names(&app).len();
        let (x, y) = (b.right() - 1.0, b.y + b.height / 2.0);
        click(&mut app, x, y);
        type_text(&mut app, "!");
        app.set_active_tool_from_type(ToolType::Zoom);
        assert_eq!(names(&app)[n..], ["Text"]);
        let new = spec_of(&app, 1).unwrap();
        assert_eq!(new.text, "Hi!");
        let rendered = crate::document::text_layer::render(&new, 60, 40);
        assert_eq!(layer_bytes(&app, Some(1)), rendered.data());
        assert!(app.undo());
        let restored = (spec_of(&app, 1), layer_bytes(&app, Some(1)));
        assert_eq!(restored, (Some(old.clone()), old_pixels.clone()));
        // An undo while editing commits the edit, then reverts it.
        click(&mut app, x, y);
        type_text(&mut app, "?");
        assert!(app.undo());
        assert_eq!(names(&app).len(), n);
        let restored = (spec_of(&app, 1), layer_bytes(&app, Some(1)));
        assert_eq!(restored, (Some(old), old_pixels));
        assert!(app.redo());
        assert_eq!(spec_of(&app, 1).unwrap().text, "Hi?");
    }
    /// Fix 8: a press 9 px outside the text is past the move reach: it places the caret and the
    /// text stays, as in Paint.NET.
    #[test]
    fn press_nine_px_outside_places_the_caret() {
        let mut app = editable_app(true);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        press_key(&mut app, "Home");
        let view = &app.active_ws().unwrap().view;
        assert_eq!((view.zoom, view.ppp), (1.0, 1.0));
        let mut r = Rectangle::default();
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            assert_eq!(t.caret, (0, 0));
            r = t.affected.unwrap();
        });
        let (x, y) = (r.right() + 9.0, r.y + r.height / 2.0);
        let b = MouseButton::Left;
        let drag = [
            Ev::Down(x, y, b),
            Ev::Move(x + 10.0, y + 5.0),
            Ev::Up(x + 10.0, y + 5.0, b),
        ];
        drive_tool(&mut app, ToolType::Text, &drag);
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            assert_eq!((t.click, t.caret), ([8.0, 10.0], (0, 2)));
        });
    }

    // PC16 fix round 2.
    /// I1: closing the image (Don't Save, Exit) drops a re-edit without its commit; the shared
    /// settings it wrote still go back, from the active tool and from a parked one.
    #[test]
    fn closing_the_image_restores_shared_settings() {
        for parked in [false, true] {
            let (mut app, _, b) = hi_layer();
            let red = Color::new(200, 10, 10, 255);
            app.tool_settings
                .set(ToolType::PaintBrush, "antialias", false.into());
            app.set_colors(red, app.secondary_color);
            let brush =
                |app: &AppWorkspace| app.tool_settings.get(ToolType::PaintBrush, "antialias");
            click(&mut app, b.x + 1.0, b.y + b.height / 2.0);
            let black = Color::new(0, 0, 0, 255);
            assert_eq!((brush(&app), app.primary_color), (true.into(), black));
            let doc = app.active.unwrap();
            if parked {
                let other = app.create_blank_document_in_new_workspace(60, 40, 96.0, false);
                app.set_active_document_workspace(Some(other));
                assert!(app.documents[doc].parked_tool.is_some());
            }
            assert!(app.close_document_workspace(doc));
            assert_eq!(
                (brush(&app), app.primary_color),
                (false.into(), red),
                "{parked}"
            );
        }
    }
    /// R79: whitespace-only text is empty. New text of spaces or a newline has no layer, and a
    /// re-edit left with a newline deletes its layer.
    #[test]
    fn whitespace_only_text_is_empty() {
        let mut app = editable_app(true);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "  ");
        assert_eq!(layer_count(&app), 1);
        escape(&mut app);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "a");
        press_key(&mut app, "Home");
        press_key(&mut app, "Enter");
        assert_eq!(layer_count(&app), 2);
        press_key(&mut app, "Delete");
        assert_eq!(layer_count(&app), 1, "a newline alone");
        escape(&mut app);
        assert_eq!(layer_count(&app), 1);
        click(&mut app, 8.0, 10.0);
        type_text(&mut app, "Hi");
        escape(&mut app);
        let spec = spec_of(&app, 1).unwrap();
        let b = bounds(&spec);
        click(&mut app, b.right() - 1.0, b.y + b.height / 2.0);
        press_key(&mut app, "Backspace");
        press_key(&mut app, "Backspace");
        press_key(&mut app, "Enter");
        escape(&mut app);
        assert_eq!(layer_count(&app), 1);
        assert!(app.undo());
        assert_eq!((layer_count(&app), spec_of(&app, 1)), (2, Some(spec)));
    }
    /// A re-edit emptied on the document's only layer keeps the layer as a raster one (the
    /// rasterize shape, `new: None`), in one item that undo reverts.
    #[test]
    fn an_emptied_only_layer_becomes_raster() {
        let (mut app, spec, b) = hi_layer();
        app.set_active_layer(0);
        crate::action::perform(&mut app, "menu.layers.delete.layer");
        assert_eq!(
            (layer_count(&app), spec_of(&app, 0)),
            (1, Some(spec.clone()))
        );
        let pixels = layer_bytes(&app, Some(0));
        let n = names(&app).len();
        click(&mut app, b.right() - 1.0, b.y + b.height / 2.0);
        press_key(&mut app, "Backspace");
        press_key(&mut app, "Backspace");
        escape(&mut app);
        let state = (layer_count(&app), names(&app).len(), spec_of(&app, 0));
        assert_eq!(state, (1, n + 1, None));
        assert!(layer_bytes(&app, Some(0)).chunks(4).all(|p| p[3] == 0));
        assert!(app.undo());
        let restored = (spec_of(&app, 0), layer_bytes(&app, Some(0)));
        assert_eq!(restored, (Some(spec), pixels));
        assert!(app.redo());
        assert_eq!(spec_of(&app, 0), None);
    }
    /// The rotate handle and pivot of the text being edited.
    fn turn_of(app: &mut AppWorkspace) -> ([f64; 2], [f64; 2]) {
        let mut out = None;
        crate::testing::with_tool::<TextTool>(app, |t, _| out = t.turn_handle);
        out.expect("a rotate handle")
    }
    fn angle_setting(app: &AppWorkspace) -> f64 {
        app.tool_settings
            .get(ToolType::Text, "textAngle")
            .as_f64()
            .unwrap()
    }
    fn near(a: [f64; 2], b: [f64; 2], d: f64) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) <= d
    }
    /// Extra (`shape`): the rotate handle sits over the text and turns it about its middle, Shift
    /// in 15° steps; a typed angle turns it the same way. The angle is the text's: the next text
    /// starts straight, and its layer brings it back.
    #[test]
    fn the_rotate_handle_turns_text_about_its_middle() {
        let mut app = crate::testing::fresh_doc(240, 160, None);
        app.set_active_tool_from_type(ToolType::Text);
        click(&mut app, 80.0, 80.0);
        type_text(&mut app, "Turn me");
        let (handle, pivot) = turn_of(&mut app);
        assert!(handle[1] < pivot[1] - 20.0 && (handle[0] - pivot[0]).abs() < 0.5);
        // Up to right of the pivot: a quarter turn clockwise on screen, -90°.
        let (b, right) = (MouseButton::Left, [pivot[0] + 40.0, pivot[1]]);
        let drag = [
            Ev::Down(handle[0], handle[1], b),
            Ev::Move(right[0], right[1]),
            Ev::Up(right[0], right[1], b),
        ];
        drive_tool(&mut app, ToolType::Text, &drag);
        // Within the pointer's pixel at 40 px.
        assert!(
            (angle_setting(&app) + 90.0).abs() < 2.0,
            "{}",
            angle_setting(&app)
        );
        let (handle, turned) = turn_of(&mut app);
        assert!(near(turned, pivot, 1.5), "{turned:?} {pivot:?}");
        assert!(
            handle[0] > pivot[0] + 20.0,
            "the handle turned with the text"
        );
        // Shift: 37° further counter-clockwise lands on -45°.
        // The handle points along (−sin θ, −cos θ) at text angle θ.
        let a = (-90f64 + 37.0).to_radians();
        let to = [pivot[0] - 40.0 * a.sin(), pivot[1] - 40.0 * a.cos()];
        let shift = [
            Ev::DownMods(handle[0], handle[1], b, "Shift"),
            Ev::Move(to[0], to[1]),
            Ev::Up(to[0], to[1], b),
        ];
        drive_tool(&mut app, ToolType::Text, &shift);
        assert_eq!(angle_setting(&app), -45.0);
        // A typed angle: about the middle too.
        app.update_tool_setting(ToolType::Text, "textAngle", serde_json::json!(30));
        assert!(near(turn_of(&mut app).1, pivot, 1.5));
        escape(&mut app);
        let spec = spec_of(&app, 1).unwrap();
        assert_eq!((spec.settings.angle, angle_setting(&app)), (30.0, 0.0));
        click(&mut app, pivot[0], pivot[1]);
        assert_eq!(angle_setting(&app), 30.0);
    }
    /// Extra (`shape`): a whole circle of text is about as tall as it is wide, and its spec stores
    /// the shape (straight text stores what it did before, `text_spec_json_is_pinned`).
    #[test]
    fn circle_text_goes_round() {
        let mut app = crate::testing::fresh_doc(200, 200, None);
        app.set_active_tool_from_type(ToolType::Text);
        app.tool_settings
            .set(ToolType::Text, "textShape", "circle".into());
        app.tool_settings
            .set(ToolType::Text, "textBend", serde_json::json!(100));
        click(&mut app, 20.0, 40.0);
        type_text(&mut app, "round and round");
        escape(&mut app);
        let spec = spec_of(&app, 1).unwrap();
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains(r#""shape":"circle","bend":100.0"#), "{json}");
        let surface = &app
            .active_ws()
            .unwrap()
            .document
            .layers
            .get_at(1)
            .unwrap()
            .surface;
        let (w, data) = (surface.width() as usize, surface.data());
        let inked: Vec<_> = (0..data.len() / 4)
            .filter(|i| data[i * 4 + 3] > 0)
            .collect();
        let xs = inked.iter().map(|i| i % w);
        let ys = inked.iter().map(|i| i / w);
        let width = xs.clone().max().unwrap() - xs.min().unwrap();
        let height = ys.clone().max().unwrap() - ys.min().unwrap();
        assert!(width.abs_diff(height) <= width / 5, "{width} × {height}");
    }
    /// Extra: Shift with the arrows, Home/End and Ctrl (by words) selects; typing, Backspace
    /// and Enter replace it; a plain arrow goes to its side; Ctrl+A selects the text (not the
    /// image) and Ctrl+C / Ctrl+X copy and cut it, leaving the image's when nothing is selected.
    #[test]
    fn keyboard_selects_text() {
        let text = |t: &TextTool| String::from_utf16_lossy(&t.lines.join(&[10u16][..]));
        let (mut t, mut app) = edit("hello big world");
        key(&mut t, &mut app, "ArrowLeft", true, true);
        assert_eq!(t.selected_text().as_deref(), Some("world"));
        key(&mut t, &mut app, "ArrowLeft", false, true);
        assert_eq!(t.selected_text().as_deref(), Some(" world"));
        t.insert_text(&mut app, "!");
        assert_eq!(text(&t), "hello big!");
        key(&mut t, &mut app, "Home", false, true);
        assert_eq!(t.selected_text().as_deref(), Some("hello big!"));
        key(&mut t, &mut app, "ArrowRight", false, false);
        assert_eq!((t.selected_text(), t.caret), (None, (0, 10)));
        key(&mut t, &mut app, "A", true, false);
        assert_eq!(t.selected_text().as_deref(), Some("hello big!"));
        key(&mut t, &mut app, "C", true, false);
        assert_eq!(t.take_copied().as_deref(), Some("hello big!"));
        assert_eq!(text(&t), "hello big!");
        key(&mut t, &mut app, "X", true, false);
        assert_eq!(t.take_copied().as_deref(), Some("hello big!"));
        assert_eq!(text(&t), "");
        t.insert_text(&mut app, "one");
        key(&mut t, &mut app, "C", true, false);
        assert_eq!(t.take_copied(), None, "no selection: the image's copy");
        key(&mut t, &mut app, "Enter", false, false);
        t.insert_text(&mut app, "two");
        for _ in 0..4 {
            key(&mut t, &mut app, "ArrowLeft", false, true);
        }
        assert_eq!(t.selected_text().as_deref(), Some("\ntwo"));
        key(&mut t, &mut app, "Backspace", false, false);
        assert_eq!(text(&t), "one");
    }
    /// Extra: a drag across the text selects it, Shift+press extends the selection and a
    /// double-click selects a word; the highlight has a box for the selected line.
    #[test]
    fn the_mouse_selects_text() {
        let mut app = crate::testing::fresh_doc(240, 80, None);
        app.set_active_tool_from_type(ToolType::Text);
        click(&mut app, 10.0, 30.0);
        type_text(&mut app, "hello world");
        let mut line = (0.0, 0.0, 0.0);
        crate::testing::with_tool::<TextTool>(&mut app, |t, _| {
            let l = &t.layout.as_ref().unwrap().lines[0];
            line = (l.origin[0], l.width, l.origin[1] + l.height / 2.0);
        });
        let (x0, w, y) = line;
        let b = MouseButton::Left;
        let drag = [
            Ev::Down(x0 - 1.0, y, b),
            Ev::Move(x0 + w / 2.0, y),
            Ev::Up(x0 + w - 0.5, y, b),
            Ev::Wait(600.0),
        ];
        drive_tool(&mut app, ToolType::Text, &drag);
        let selected = |app: &mut AppWorkspace| {
            let mut out = (None, 0);
            crate::testing::with_tool::<TextTool>(app, |t, app| {
                out = (t.selected_text(), t.selection_quads(app).len());
            });
            out
        };
        assert_eq!(selected(&mut app), (Some("hello world".into()), 1));
        let word = x0 + w * 0.8;
        let twice = [
            Ev::Down(word, y, b),
            Ev::Up(word, y, b),
            Ev::Down(word, y, b),
            Ev::Up(word, y, b),
        ];
        drive_tool(&mut app, ToolType::Text, &twice);
        assert_eq!(selected(&mut app).0.as_deref(), Some("world"));
    }
}
