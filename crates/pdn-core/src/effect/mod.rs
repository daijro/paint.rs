//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (definition model only)
//! Paint.NET reference: adjustments-and-effect-framework.md §2.2–2.6; effects-1.md §2.1; effects-2.md §2.0

pub mod adjustments;
pub mod artistic;
pub mod bb;
pub mod blur;
pub mod color;
pub mod distort;
pub mod extra;
pub mod levels_ui;
pub mod mix;
pub mod noise;
pub mod object;
pub mod perlin;
pub mod photo;
pub mod render;
pub mod rng;
pub mod rotate_zoom;
pub mod sample;
pub mod session;
pub mod slider;
pub mod stylize;
pub mod surface;

use crate::canvas::ImageData;
use crate::i18n;
use crate::pixel::mask::{Mask, SelectionQuality};
use crate::pixel::{IntRect, Px, ops};
use crate::selection::Selection;
use crate::util::color::Color;
use serde_json::Value;
use surface::{Alpha, EffectSurface, Space};

/// Control values by key (paintdotjs's `values` object).
pub type Values = serde_json::Map<String, Value>;

/// The default of every Q* "Quality" control: Paint.NET's default quality level on a
/// "Good" GPU (effects-1.md §2.0, effects-2.md §2.0 item 4).
pub const GPU_QUALITY: f64 = 2.0;

/// A GPU effect's working space and the alpha modes it takes and returns (`OnInitializeRenderInfo`,
/// adjustments-and-effect-framework.md §2.1, §2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderInfo {
    pub space: Space,
    pub input: Alpha,
    pub output: Alpha,
}

impl RenderInfo {
    pub const fn new(space: Space, alpha: Alpha) -> Self {
        Self {
            space,
            input: alpha,
            output: alpha,
        }
    }
}

/// A document layer as a layer-reading effect sees it (BB common.md §5.11 "Layer list").
#[derive(Clone, Debug)]
pub struct LayerInfo {
    pub name: String,
    pub pixels: std::sync::Arc<ImageData>,
}

/// `render::color_ui::DEFAULT_PALETTE` as `[r, g, b, a]`.
static DEFAULT_PALETTE_PX: [Px; 96] = {
    let mut out = [[0; 4]; 96];
    let mut i = 0;
    while i < 96 {
        let [a, r, g, b] = crate::render::color_ui::DEFAULT_PALETTE[i].to_be_bytes();
        out[i] = [r, g, b, a];
        i += 1;
    }
    out
};

/// What an effect sees besides its values (§2.2 step 1).
#[derive(Clone, Copy, Debug)]
pub struct EffectEnv<'a> {
    /// Immutable external bitmap captured by this invocation.
    pub image: Option<&'a ImageData>,
    /// The 3D LUT file this invocation read (`External::Lut`).
    pub lut: Option<&'a crate::extra::fx::lut::LutFile>,
    /// Effective GPU context during `apply`'s GPU callback. None for defaults/UI environments and
    /// CPU callbacks; `apply` overrides this on a local copy, never mutating the caller (R88).
    pub render_info: Option<RenderInfo>,
    /// Optional per-render message, delivered only after the UI accepts this generation.
    pub notice: Option<&'a std::sync::Mutex<Option<String>>>,
    /// Borrowed cancellation for effects with interruptible computation.
    pub cancel: Option<&'a std::sync::atomic::AtomicBool>,
    pub width: u32,
    pub height: u32,
    /// The selection bounds ∩ the layer (`selection_bounds`).
    pub bounds: IntRect,
    /// The clipping mask; `None` when nothing or everything is selected (`selection_mask`).
    pub mask: Option<&'a Mask>,
    pub primary: Px,
    pub secondary: Px,
    /// Paint.NET's per-instance clock seed (§2.6 step 5); tests use 0.
    pub instance_seed: u32,
    /// The document's layers, bottom first; empty unless a control is `Control::LayerSelect`.
    pub layers: &'a [LayerInfo],
    /// The index of the layer the effect runs on (bottom = 0).
    pub layer_index: usize,
    /// The Colors window's 96 palette colours.
    pub palette: &'a [Px],
    /// The selection outline in canvas pixels (even-odd polygons); empty when nothing is selected.
    pub selection: &'a [Vec<[f64; 2]>],
    /// The UTC date when the effect began (spec-question Q7).
    pub today: rng::CivilDate,
    // spec-question: Q54 Horizon instance state lifetime
    /// The numeric culture the invocation captured once (R152); never the live global.
    pub culture: &'a str,
}

impl EffectEnv<'_> {
    /// The whole w×h layer, no selection, primary black, secondary white, instance seed 0, no
    /// layers, the default palette, 2026-02-01, English numbers.
    pub fn whole(w: u32, h: u32) -> Self {
        Self {
            image: None,
            lut: None,
            render_info: None,
            notice: None,
            cancel: None,
            width: w,
            height: h,
            bounds: IntRect::new(0, 0, w, h),
            mask: None,
            primary: [0, 0, 0, 255],
            secondary: [255, 255, 255, 255],
            instance_seed: 0,
            layers: &[],
            layer_index: 0,
            palette: &DEFAULT_PALETTE_PX,
            selection: &[],
            today: rng::CivilDate::new(2026, 2, 1),
            culture: "en",
        }
    }

    pub fn selected(&self, x: u32, y: u32) -> bool {
        self.mask.is_none_or(|m| m.get(x as i32, y as i32) > 0)
    }
}

/// A GPU effect on f32 in its declared working space and alpha mode; `apply` supplies that
/// effective `RenderInfo` through a local env and does §2.4's conversions around the callback.
pub type GpuFn = fn(&EffectSurface, &Values, &EffectEnv<'_>) -> EffectSurface;
/// A CPU BitmapEffect on the 8-bit layer (Quantize, Red Eye Removal; §2.8).
pub type CpuFn = fn(&ImageData, &Values, &EffectEnv<'_>) -> ImageData;

#[derive(Clone, Copy, Debug)]
pub enum EffectKind {
    Gpu(RenderInfo, GpuFn),
    /// `clip_to_selection: false` turns selection clipping off (Red Eye Removal, §2.3).
    Cpu {
        render: CpuFn,
        clip_to_selection: bool,
    },
}

/// The menu an effect lives in: Adjustments, an Effects submenu key, or Layers (Rotate/Zoom).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Adjustment,
    Effect(&'static str),
    Layers,
}

/// Specialized dialogs (Task 42); every other configurable effect uses the generic form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogLayout {
    HueSaturation,
    RotateZoom,
    Curves,
    Levels,
    /// BB's Chart or Graph: its own three windows (render-chart-or-graph.md).
    Chart,
}

#[derive(Clone, Debug)]
pub struct EffectDef {
    pub id: &'static str,
    pub icon: &'static str,
    pub category: Category,
    pub controls: Vec<Control>,
    pub kind: EffectKind,
    pub dialog: Option<DialogLayout>,
    pub dialog_title: Option<&'static str>,
    /// A plugin dialog's IndirectUI presentation (BB common.md §3); `None` for built-ins.
    pub indirect: Option<&'static bb::ui::IndirectUi>,
}

impl EffectDef {
    pub fn translation_key(&self) -> String {
        format!("{}.name", self.id)
    }

    /// The menu name: a plugin's untranslated manifest name, else the translated name.
    pub fn display_name(&self) -> String {
        display_name_of(self.id)
    }

    pub fn action_id(&self) -> String {
        match self.category {
            Category::Adjustment => format!("adjustment.{}", self.id),
            Category::Effect(_) => format!("menu.effects.{}", self.id),
            Category::Layers => extra::manifest::layers_action(self.id)
                .unwrap_or("menu.layers.rotateZoom")
                .into(),
        }
    }

    pub fn is_configurable(&self) -> bool {
        !self.controls.is_empty()
    }
}

/// Effect `id`'s menu name: a plugin's or an Extra's untranslated manifest name, else the
/// translated name.
pub fn display_name_of(id: &str) -> String {
    bb::manifest::manifest(id)
        .map(|m| m.name)
        .or_else(|| extra::manifest::manifest(id).map(|m| m.name))
        .map_or_else(|| i18n::t(&format!("{id}.name")), String::from)
}

/// Where an effect comes from (S§4.1): Paint.NET, BB's plugin pack, or an Extra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    PaintNet,
    Plugin,
    Extra,
}

/// Effect `effect_id`'s provenance; an id is never both Plugin and Extra.
pub fn provenance_of(effect_id: &str) -> Provenance {
    if bb::manifest::manifest(effect_id).is_some() {
        Provenance::Plugin
    } else if extra::manifest::manifest(effect_id).is_some() {
        Provenance::Extra
    } else {
        Provenance::PaintNet
    }
}

/// A colour control's default: `0xRRGGBB`, or the palette's primary/secondary colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorDefault {
    Rgb(u32),
    Primary,
    Secondary,
}

/// A static label's text: fixed, or a prefix followed by the current layer's name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LabelText {
    Fixed(&'static str),
    CurrentLayer(&'static str),
}

/// A pan control's thumbnail: the layer, the layer cropped to the selection bounds, or a plain
/// white square (BB common.md §3 "Pan (offset) control").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanThumb {
    Source,
    SelectionCrop,
    White,
}

/// paintdotjs's control model (`range`, `checkbox`, `select`, `curve`), plus colours and plain
/// values that only a specialized layout edits, plus the plugin kinds (BB common.md §3).
#[derive(Clone, Debug)]
pub enum Control {
    Range {
        key: &'static str,
        label: &'static str,
        min: f64,
        max: f64,
        default: f64,
        step: f64,
        exponential: bool,
        /// Native key steps, independent of the numeric editor and pointer mapping.
        keyboard: slider::SliderKeys,
        /// The paintdotjs CSS track class.
        track: &'static str,
    },
    Checkbox {
        key: &'static str,
        label: &'static str,
        default: bool,
    },
    Select {
        key: &'static str,
        label: &'static str,
        default: &'static str,
        /// (value, label) pairs.
        choices: &'static [(&'static str, &'static str)],
    },
    Curve {
        key: &'static str,
        default: &'static [[f64; 2]],
    },
    Color {
        key: &'static str,
        label: &'static str,
        default: ColorDefault,
    },
    Value {
        key: &'static str,
        default: fn() -> Value,
    },
    /// A text box; the value is the string.
    Text {
        key: &'static str,
        label: &'static str,
        default: &'static str,
        max_len: u32,
        multiline: bool,
    },
    /// Read-only text; no value (`Null`).
    Label {
        key: &'static str,
        label: &'static str,
        text: LabelText,
    },
    /// A drop-down of the document's layers; the value is the layer index, bottom = 0.
    LayerSelect {
        key: &'static str,
        label: &'static str,
    },
    /// A drop-down of the document font families (Task 53's collection, BB30); the value is a
    /// family name.
    FontSelect {
        key: &'static str,
        label: &'static str,
        default: &'static str,
    },
    /// A file chooser (common.md §3 "File chooser"): an editable path and a browse button whose
    /// chooser lists `filters` in order, plus All files when `all_files`; the value is the path,
    /// default empty.
    File {
        key: &'static str,
        label: &'static str,
        filters: &'static [&'static str],
        all_files: bool,
    },
    /// A pan (offset) control; the value is `[x, y]`.
    Pan {
        key: &'static str,
        label: &'static str,
        min: f64,
        max: f64,
        default: [f64; 2],
        step: f64,
        thumb: PanThumb,
    },
}

/// A range with step 1, linear, on the default track.
pub fn range(key: &'static str, label: &'static str, min: f64, max: f64, default: f64) -> Control {
    Control::Range {
        key,
        label,
        min,
        max,
        default,
        step: 1.0,
        exponential: false,
        keyboard: slider::SliderKeys::INTEGER,
        track: "default",
    }
}

impl Control {
    /// Paint.NET slider metadata (adjustments-and-effect-framework §§7.5, 8).
    pub fn slider_keys(mut self, decimals: u32, small: f64, large: f64, exponential: bool) -> Self {
        if let Self::Range { keyboard, .. } = &mut self {
            *keyboard = slider::SliderKeys {
                decimals,
                small,
                large,
                exponential,
            };
        }
        self
    }

    pub fn step(mut self, s: f64) -> Self {
        if let Self::Range { step, .. } = &mut self {
            *step = s;
        }
        self
    }

    pub fn exponential(mut self) -> Self {
        if let Self::Range { exponential, .. } = &mut self {
            *exponential = true;
        }
        self
    }

    pub fn key(&self) -> &'static str {
        match self {
            Self::Range { key, .. }
            | Self::Checkbox { key, .. }
            | Self::Select { key, .. }
            | Self::Curve { key, .. }
            | Self::Color { key, .. }
            | Self::Value { key, .. }
            | Self::Text { key, .. }
            | Self::Label { key, .. }
            | Self::LayerSelect { key, .. }
            | Self::FontSelect { key, .. }
            | Self::File { key, .. }
            | Self::Pan { key, .. } => key,
        }
    }

    /// Numbers as f64, booleans, choice strings, point arrays, colours as `"#rrggbb"` or
    /// `"primary"`/`"secondary"`, text strings, labels `Null`, layer index 0, font family names,
    /// file paths `""`, pan `[x, y]`.
    pub fn default_value(&self) -> Value {
        match self {
            Self::Range { default, .. } => Value::from(*default),
            Self::Checkbox { default, .. } => Value::Bool(*default),
            Self::Select { default, .. } => Value::from(*default),
            Self::Curve { default, .. } => default.iter().map(|p| Value::from(&p[..])).collect(),
            Self::Color { default, .. } => Value::from(match default {
                ColorDefault::Rgb(c) => format!("#{:06x}", c & 0xff_ffff),
                ColorDefault::Primary => "primary".into(),
                ColorDefault::Secondary => "secondary".into(),
            }),
            Self::Value { default, .. } => default(),
            Self::Text { default, .. } => Value::from(*default),
            Self::Label { .. } => Value::Null,
            Self::LayerSelect { .. } => Value::from(0),
            Self::FontSelect { default, .. } => Value::from(*default),
            Self::File { .. } => Value::from(""),
            Self::Pan { default, .. } => Value::from(&default[..]),
        }
    }
}

/// paintdotjs `EffectsMenu.CATEGORIES` with Paint.NET 5.1.12's content (R16; effects-1.md §2.0,
/// effects-2.md §1; spec §0.1 Ruling (P1)): 40 effects in 9 submenus, ids sorted by English name.
/// The menu sorts each submenu by translated name, as paintdotjs `EffectsMenu.createCategory` does.
pub const CATEGORIES: [(&str, &[&str]); 9] = [
    (
        "artistic",
        &["inkSketchEffect", "oilPaintingEffect", "pencilSketchEffect"],
    ),
    (
        "blurring",
        &[
            "bokehEffect",
            "fragmentEffect",
            "blurEffect",
            "medianBlurEffect",
            "motionBlurEffect",
            "radialBlurEffect",
            "sketchBlurEffect",
            "squareBlurEffect",
            "surfaceBlurEffect",
            "zoomBlurEffect",
        ],
    ),
    ("color", &["quantizeEffect"]),
    (
        "distort",
        &[
            "bulgeEffect",
            "crystalizeEffect",
            "dentsEffect",
            "frostedGlassEffect",
            "morphologyEffect",
            "pixelateEffect",
            "polarInversion",
            "tileEffect",
            "twistEffect",
        ],
    ),
    ("noise", &["addNoiseEffect", "reduceNoiseEffect"]),
    ("object", &["dropShadowEffect"]),
    (
        "photo",
        &[
            "glowEffect",
            "redEyeRemoveEffect",
            "sharpenEffect",
            "softenPortraitEffect",
            "straightenEffect",
            "vignetteEffect",
        ],
    ),
    (
        "render",
        &[
            "cloudsEffect",
            "juliaFractalEffect",
            "mandelbrotFractalEffect",
            "turbulenceEffect",
        ],
    ),
    (
        "stylize",
        &[
            "edgeDetectEffect",
            "embossEffect",
            "outlineEffect",
            "reliefEffect",
        ],
    ),
];

/// The registered plugin effects, then the Extra effects, each when shown.
fn registered(plugins: bool, extras: bool) -> impl Iterator<Item = EffectDef> {
    let plugins = plugins.then(bb::effects).into_iter().flatten();
    plugins.chain(extras.then(extra::effects).into_iter().flatten())
}

/// Built-in submenus plus registered plugin and Extra effects (BB README "Menu tree").
/// Plugin- and Extra-only submenus appear only when one of their effects is implemented.
pub fn menu_categories(plugins: bool, extras: bool) -> Vec<(&'static str, Vec<&'static str>)> {
    menu_categories_from(registered(plugins, extras))
}

fn menu_categories_from(
    registered: impl IntoIterator<Item = EffectDef>,
) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut categories: Vec<_> = CATEGORIES
        .iter()
        .map(|(key, ids)| (*key, ids.to_vec()))
        .collect();
    for def in registered {
        if let Category::Effect(key) = def.category {
            if key.is_empty() {
                continue;
            }
            if let Some((_, ids)) = categories.iter_mut().find(|(k, _)| *k == key) {
                ids.push(def.id);
            } else {
                categories.push((key, vec![def.id]));
            }
        }
    }
    categories
}

/// Registered plugins and Extras placed after a separator at the end of Effects (README "Menu
/// tree").
pub fn root_items(plugins: bool, extras: bool) -> Vec<&'static str> {
    plugin_items_from(registered(plugins, extras), Category::Effect(""))
}

/// Registered plugin and Extra adjustments, merged alphabetically with the built-ins (README
/// "Menu tree").
pub fn plugin_adjustments(plugins: bool, extras: bool) -> Vec<&'static str> {
    plugin_items_from(registered(plugins, extras), Category::Adjustment)
}

fn plugin_items_from(
    registered: impl IntoIterator<Item = EffectDef>,
    category: Category,
) -> Vec<&'static str> {
    registered
        .into_iter()
        .filter(|def| def.category == category)
        .map(|def| def.id)
        .collect()
}

/// The 13 adjustments, in Paint.NET's menu order.
pub fn definitions() -> Vec<EffectDef> {
    let mut defs = adjustments::definitions();
    defs.iter_mut().for_each(slider::configure_builtin);
    defs
}

/// The Effects-menu effects (Tasks 39–41b).
pub fn effects() -> Vec<EffectDef> {
    let mut e = blur::effects();
    e.extend(artistic::effects());
    e.extend(color::effects());
    e.extend(distort::effects());
    e.extend(noise::effects());
    e.extend(object::effects());
    e.extend(photo::effects());
    e.extend(render::effects());
    e.extend(stylize::effects());
    e.iter_mut().for_each(slider::configure_builtin);
    e
}

/// The Layers-menu Rotate/Zoom effect (effects-2.md §2.16).
pub fn rotate_zoom() -> EffectDef {
    let mut def = rotate_zoom::definition();
    slider::configure_builtin(&mut def);
    def
}

/// The adjustments, the Effects-menu effects, the plugin effects, the Extras, then Rotate/Zoom.
fn all() -> impl Iterator<Item = EffectDef> {
    definitions()
        .into_iter()
        .chain(effects())
        .chain(bb::effects())
        .chain(extra::effects())
        .chain(std::iter::once(rotate_zoom()))
}

/// An adjustment, an Effects-menu effect, or Rotate/Zoom (checked last).
pub fn find(id: &str) -> Option<EffectDef> {
    all().find(|d| d.id == id)
}

/// Whether menu action `action` runs a configurable effect, whose menu item gets "..."
/// (ui-text-and-glyphs.md §10).
pub fn is_configurable_action(action: &str) -> bool {
    static IDS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        all()
            .filter(EffectDef::is_configurable)
            .map(|d| d.action_id())
            .collect()
    });
    IDS.iter().any(|a| a == action)
}

pub fn defaults(def: &EffectDef) -> Values {
    def.controls
        .iter()
        .map(|c| (c.key().to_string(), c.default_value()))
        .collect()
}

/// The defaults with the keys of the `overrides` object replaced.
pub fn with(def: &EffectDef, overrides: &Value) -> Values {
    let mut v = defaults(def);
    if let Some(o) = overrides.as_object() {
        v.extend(o.iter().map(|(k, x)| (k.clone(), x.clone())));
    }
    v
}

pub fn num(values: &Values, key: &str) -> f64 {
    values.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

pub fn flag(values: &Values, key: &str) -> bool {
    values.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub fn choice<'a>(values: &'a Values, key: &str) -> &'a str {
    values.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `"primary"`/`"secondary"` from the env; otherwise `#rrggbb`, opaque.
pub fn color_value(values: &Values, key: &str, env: &EffectEnv) -> Px {
    match values.get(key).and_then(Value::as_str).unwrap_or("") {
        "primary" => env.primary,
        "secondary" => env.secondary,
        hex => {
            let c = Color::from_hex(hex);
            [c.red, c.green, c.blue, 255]
        }
    }
}

/// Runs `def` over the whole layer: a GPU effect gets §2.4's conversions and quantizer around it,
/// then the result is clipped to `env.mask` (§2.5) unless the effect disables clipping.
// ponytail: renders the whole layer per call; restrict to the visible rect ∩ render scans if
// large previews are slow (§2.3 preview region and tiling change no pixel).
pub fn apply(source: &ImageData, def: &EffectDef, values: &Values, env: &EffectEnv) -> ImageData {
    let mut render_env = *env;
    render_env.render_info = None;
    let (mut out, clip) = match def.kind {
        EffectKind::Gpu(info, render) => {
            let src = EffectSurface::from_rgba8(source, info.space, info.input);
            render_env.render_info = Some(info);
            (
                render(&src, values, &render_env).to_rgba8(info.space, info.output),
                true,
            )
        }
        EffectKind::Cpu {
            render,
            clip_to_selection,
        } => (render(source, values, &render_env), clip_to_selection),
    };
    if clip && let Some(mask) = env.mask {
        underwrite(source, &mut out, mask);
    }
    out
}

/// §2.5 step 2: mask 255 keeps the output, 0 restores the source, otherwise
/// `blend(source, output, m)` (alpha-weighted, floor division).
pub fn underwrite(source: &ImageData, output: &mut ImageData, mask: &Mask) {
    underwrite_range(
        source,
        output,
        mask,
        0..output.data.len().min(source.data.len()) / 4,
    );
}

/// The same clipping kernel over a bounded pixel range for seam previews.
pub(crate) fn underwrite_range(
    source: &ImageData,
    output: &mut ImageData,
    mask: &Mask,
    range: std::ops::Range<usize>,
) {
    let w = source.width.max(1) as usize;
    let src = source.data.as_chunks::<4>().0;
    let out = output.data.as_chunks_mut::<4>().0;
    for i in range {
        out[i] = ops::blend(src[i], out[i], mask.get((i % w) as i32, (i / w) as i32));
    }
}

/// The selection's clipping mask over the w×h layer (selection-and-move.md §2.3); `None` when
/// nothing or everything is selected (§2.5 step 1: constant opaque, no mask pass).
pub fn selection_mask(
    selection: &Selection,
    quality: SelectionQuality,
    w: u32,
    h: u32,
) -> Option<Mask> {
    if selection.is_empty() {
        return None;
    }
    let m = selection.clipping_mask(quality, IntRect::new(0, 0, w, h));
    (!m.data.iter().all(|&b| b == 255)).then(|| (*m).clone())
}

/// The selection's integer bounds ∩ the layer; the whole layer when nothing is selected.
pub fn selection_bounds(selection: &Selection, w: u32, h: u32) -> IntRect {
    let doc = IntRect::new(0, 0, w, h);
    if selection.is_empty() {
        doc
    } else {
        IntRect::bound_of(&selection.get_bounds()).intersect(&doc)
    }
}

/// The effect test harness (plan M5): fixtures, and `apply` with `EffectEnv::whole`.
#[cfg(test)]
pub mod test_util {
    use super::*;

    pub fn solid(w: u32, h: u32, p: Px) -> ImageData {
        ImageData {
            width: w,
            height: h,
            data: p.repeat((w * h) as usize),
        }
    }

    pub fn from_fn(w: u32, h: u32, f: impl Fn(u32, u32) -> Px) -> ImageData {
        let f = &f;
        let data = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| f(x, y)))
            .collect();
        ImageData {
            width: w,
            height: h,
            data,
        }
    }

    pub fn px(img: &ImageData, x: u32, y: u32) -> Px {
        let i = (y * img.width + x) as usize * 4;
        img.data[i..i + 4].try_into().unwrap()
    }

    /// `apply` with `EffectEnv::whole`: no selection, primary black, secondary white, seed 0.
    pub fn run(id: &str, overrides: Value, src: &ImageData) -> ImageData {
        run_env(id, overrides, src, &EffectEnv::whole(src.width, src.height))
    }

    pub fn run_env(id: &str, overrides: Value, src: &ImageData, env: &EffectEnv) -> ImageData {
        let def = find(id).unwrap_or_else(|| panic!("no effect {id}"));
        apply(src, &def, &with(&def, &overrides), env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::pixel::IntRect;
    use crate::pixel::mask::{Mask, SelectionQuality};

    /// README "Menu tree": registered definitions merge once into their category, including
    /// plugin-only submenus; the manifest alone must never expose an unimplemented effect.
    #[test]
    fn registered_plugin_menu_placement() {
        let registered: Vec<_> = bb::manifest::MANIFEST
            .iter()
            .map(|m| EffectDef {
                id: m.id,
                icon: m.icon,
                category: m.category,
                controls: vec![],
                kind: EffectKind::Gpu(RenderInfo::new(Space::Srgb, Alpha::Straight), passthrough),
                dialog: None,
                dialog_title: None,
                indirect: None,
            })
            .collect();
        let categories = menu_categories_from(registered.clone());
        assert_eq!(categories.len(), 12);
        for (key, builtin) in CATEGORIES {
            let ids = &categories.iter().find(|(k, _)| *k == key).unwrap().1;
            assert_eq!(&ids[..builtin.len()], builtin);
        }
        for m in bb::manifest::MANIFEST {
            if let Category::Effect(key) = m.category
                && !key.is_empty()
            {
                let ids = &categories.iter().find(|(k, _)| *k == key).unwrap().1;
                assert_eq!(ids.iter().filter(|id| **id == m.id).count(), 1, "{}", m.id);
            }
        }
        assert_eq!(
            plugin_items_from(registered.clone(), Category::Effect("")),
            ["bbFlipHorizontal", "bbFlipVertical"]
        );
        assert_eq!(plugin_items_from(registered, Category::Adjustment).len(), 5);
        let empty: Vec<EffectDef> = vec![];
        assert_eq!(
            menu_categories_from(empty),
            CATEGORIES
                .iter()
                .map(|(k, v)| (*k, v.to_vec()))
                .collect::<Vec<_>>()
        );
        let partial: Vec<_> = bb::manifest::MANIFEST
            .iter()
            .filter(|m| m.category == Category::Effect("fill"))
            .take(1)
            .map(|m| EffectDef {
                id: m.id,
                icon: m.icon,
                category: m.category,
                controls: vec![],
                kind: EffectKind::Gpu(RenderInfo::new(Space::Srgb, Alpha::Straight), passthrough),
                dialog: None,
                dialog_title: None,
                indirect: None,
            })
            .collect();
        let partial = menu_categories_from(partial);
        assert_eq!(partial.len(), 10);
        assert_eq!(partial.last().unwrap().0, "fill");
        assert_eq!(menu_categories(false, false), menu_categories_from(vec![]));
        assert!(root_items(false, false).is_empty());
        assert!(plugin_adjustments(false, false).is_empty());
    }

    /// R88: effective GPU context reaches each callback; CPU callbacks and the caller stay independent.
    #[test]
    fn apply_supplies_per_call_render_info() {
        fn gpu(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
            let space = if flag(v, "linear") {
                Space::Linear
            } else {
                Space::Srgb
            };
            assert_eq!(
                env.render_info,
                Some(RenderInfo::new(space, Alpha::Straight))
            );
            src.clone()
        }
        fn cpu(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
            assert_eq!(env.render_info, None);
            src.clone()
        }
        let img = test_util::solid(1, 1, [41, 89, 137, 0]);
        let mut env = EffectEnv::whole(1, 1);
        env.render_info = Some(RenderInfo::new(Space::Linear, Alpha::Premultiplied));
        for space in [Space::Srgb, Space::Linear] {
            let d = def(EffectKind::Gpu(
                RenderInfo::new(space, Alpha::Straight),
                gpu,
            ));
            let v = with(&d, &serde_json::json!({"linear":space==Space::Linear}));
            assert_eq!(apply(&img, &d, &v, &env).data, img.data);
        }
        let d = def(EffectKind::Cpu {
            render: cpu,
            clip_to_selection: true,
        });
        assert_eq!(apply(&img, &d, &Values::new(), &env).data, img.data);
        assert_eq!(
            env.render_info,
            Some(RenderInfo::new(Space::Linear, Alpha::Premultiplied))
        );
        assert_eq!(EffectEnv::whole(1, 1).render_info, None);
    }

    fn writes_orange(src: &EffectSurface, _: &Values, _: &EffectEnv) -> EffectSurface {
        EffectSurface::from_fn(src.w, src.h, |_, _| [0.5, 0.25, 1.0, 1.0])
    }
    fn passthrough(src: &EffectSurface, _: &Values, _: &EffectEnv) -> EffectSurface {
        src.clone()
    }
    fn cpu_red(src: &ImageData, _: &Values, _: &EffectEnv) -> ImageData {
        ImageData {
            width: src.width,
            height: src.height,
            data: src.data.chunks(4).flat_map(|_| [255, 0, 0, 255]).collect(),
        }
    }
    fn def(kind: EffectKind) -> EffectDef {
        EffectDef {
            id: "test",
            icon: "",
            category: Category::Effect("render"),
            controls: vec![],
            kind,
            dialog: None,
            dialog_title: None,
            indirect: None,
        }
    }
    fn layer() -> ImageData {
        ImageData {
            width: 3,
            height: 1,
            data: [[200u8, 100, 50, 255]; 3].concat(),
        }
    }

    /// §2.4 quantizer + §2.5 clipping: m 255 keeps, m 0 restores, otherwise blend(original, output, m).
    #[test]
    fn gpu_pipeline_quantizes_and_clips_to_the_mask() {
        let d = def(EffectKind::Gpu(
            RenderInfo::new(Space::Srgb, Alpha::Straight),
            writes_orange,
        ));
        let mask = Mask {
            rect: IntRect::new(0, 0, 3, 1),
            data: vec![255, 128, 0],
        };
        let mut env = EffectEnv::whole(3, 1);
        assert_eq!(
            &apply(&layer(), &d, &Values::new(), &env).data[..4],
            [128, 64, 255, 255]
        );
        env.mask = Some(&mask);
        let out = apply(&layer(), &d, &Values::new(), &env);
        assert_eq!(&out.data[0..4], [128, 64, 255, 255]);
        assert_eq!(
            &out.data[4..8],
            crate::pixel::ops::blend([200, 100, 50, 255], [128, 64, 255, 255], 128)
        );
        assert_eq!(&out.data[8..12], [200, 100, 50, 255]);
    }

    /// Red Eye Removal turns selection clipping off (§2.3): the framework does not blend it.
    #[test]
    fn cpu_effects_may_skip_clipping() {
        let mask = Mask {
            rect: IntRect::new(0, 0, 3, 1),
            data: vec![0, 0, 0],
        };
        let mut env = EffectEnv::whole(3, 1);
        env.mask = Some(&mask);
        let clipped = def(EffectKind::Cpu {
            render: cpu_red,
            clip_to_selection: true,
        });
        assert_eq!(
            apply(&layer(), &clipped, &Values::new(), &env).data,
            layer().data
        );
        let unclipped = def(EffectKind::Cpu {
            render: cpu_red,
            clip_to_selection: false,
        });
        assert_eq!(
            &apply(&layer(), &unclipped, &Values::new(), &env).data[..4],
            [255, 0, 0, 255]
        );
    }

    #[test]
    fn premultiplied_passthrough_drops_hidden_rgb() {
        let src = ImageData {
            width: 2,
            height: 1,
            data: vec![90, 180, 250, 0, 90, 180, 250, 77],
        };
        let d = def(EffectKind::Gpu(
            RenderInfo::new(Space::Linear, Alpha::Premultiplied),
            passthrough,
        ));
        assert_eq!(
            apply(&src, &d, &Values::new(), &EffectEnv::whole(2, 1)).data,
            [0, 0, 0, 0, 90, 180, 250, 77]
        );
    }

    /// §2.5 step 1: no mask pass for an empty or select-all selection; selection-and-move.md §2.3 otherwise.
    #[test]
    fn selection_masks() {
        use crate::selection::{CombineMode, Selection};
        use crate::util::geom::Point;
        let rect = |l: f64, t: f64, r: f64, b: f64| {
            [
                Point::new(l, t),
                Point::new(r, t),
                Point::new(r, b),
                Point::new(l, b),
            ]
        };
        let mut s = Selection::default();
        assert!(selection_mask(&s, SelectionQuality::HighQuality, 4, 2).is_none());
        assert_eq!(selection_bounds(&s, 4, 2), IntRect::new(0, 0, 4, 2));
        s.set_continuation_points(&rect(1.0, 0.0, 3.0, 2.0), CombineMode::Replace);
        s.commit_continuation();
        let m = selection_mask(&s, SelectionQuality::HighQuality, 4, 2).unwrap();
        assert_eq!(
            (0..4).map(|x| m.get(x, 1)).collect::<Vec<_>>(),
            [0, 255, 255, 0]
        );
        assert_eq!(selection_bounds(&s, 4, 2), IntRect::new(1, 0, 2, 2));
        s.set_continuation_points(&rect(0.0, 0.0, 4.0, 2.0), CombineMode::Replace);
        s.commit_continuation();
        assert!(selection_mask(&s, SelectionQuality::HighQuality, 4, 2).is_none());
    }

    /// §2.5 end to end on a 3×2 layer: `underwrite` indexes the mask at (i % w, i / w).
    #[test]
    fn apply_clips_a_2d_selection_mask() {
        use crate::selection::{CombineMode, Selection};
        use crate::util::geom::Point;
        let pts = [(1.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0)].map(|(x, y)| Point::new(x, y));
        let mut s = Selection::default();
        s.set_continuation_points(&pts, CombineMode::Replace);
        s.commit_continuation();
        let mask = selection_mask(&s, SelectionQuality::HighQuality, 3, 2).unwrap();
        let src = test_util::from_fn(3, 2, |x, y| [(10 * x + 100 * y) as u8, 7, 9, 255]);
        let mut env = EffectEnv::whole(3, 2);
        env.mask = Some(&mask);
        let d = def(EffectKind::Gpu(
            RenderInfo::new(Space::Srgb, Alpha::Straight),
            writes_orange,
        ));
        let out = apply(&src, &d, &Values::new(), &env);
        let orange = [128, 64, 255, 255];
        let want: Vec<_> = (0..2u32)
            .flat_map(|y| (0..3u32).map(move |x| (x, y)))
            .map(|(x, y)| {
                if y == 0 && x > 0 {
                    orange
                } else {
                    test_util::px(&src, x, y)
                }
            })
            .collect();
        assert_eq!(out.data, want.concat());
    }

    #[test]
    fn values_and_defaults() {
        let d = EffectDef {
            controls: vec![
                range("radius", "Radius", 0.0, 300.0, 2.0),
                Control::Checkbox {
                    key: "centered",
                    label: "Centered",
                    default: true,
                },
                Control::Select {
                    key: "edge",
                    label: "Edge",
                    default: "clamp",
                    choices: &[("clamp", "Clamp"), ("wrap", "Wrap")],
                },
                Control::Color {
                    key: "color",
                    label: "Color",
                    default: ColorDefault::Primary,
                },
            ],
            ..def(EffectKind::Gpu(
                RenderInfo::new(Space::Srgb, Alpha::Straight),
                passthrough,
            ))
        };
        let v = with(&d, &serde_json::json!({"radius": 7.5}));
        assert_eq!(
            (num(&v, "radius"), flag(&v, "centered"), choice(&v, "edge")),
            (7.5, true, "clamp")
        );
        let mut env = EffectEnv::whole(1, 1);
        env.primary = [1, 2, 3, 255];
        assert_eq!(color_value(&v, "color", &env), [1, 2, 3, 255]);
        let v = with(&d, &serde_json::json!({"color": "#336699"}));
        assert_eq!(color_value(&v, "color", &env), [0x33, 0x66, 0x99, 255]);
        assert!(d.is_configurable());
    }

    /// R16: effects-1.md §2.0, effects-2.md §1, spec §0.1 Ruling (P1).
    #[test]
    fn categories_are_paint_net_5_1_12() {
        let counts: Vec<usize> = CATEGORIES.iter().map(|(_, ids)| ids.len()).collect();
        assert_eq!(counts, [3, 10, 1, 9, 2, 1, 6, 4, 4]);
        assert_eq!(counts.iter().sum::<usize>(), 40);
        let all: Vec<&str> = CATEGORIES
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        assert!(!all.contains(&"unfocusEffect") && !all.contains(&"medianEffect"));
        assert_eq!(
            CATEGORIES[6].1,
            [
                "glowEffect",
                "redEyeRemoveEffect",
                "sharpenEffect",
                "softenPortraitEffect",
                "straightenEffect",
                "vignetteEffect"
            ]
        );
    }
}
