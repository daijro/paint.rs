//! Port of: js/src/engine/document/tool/ToolType.js (ids, icons, hotkeys, `skipIfActiveOnHotKey`,
//! `VALUES` order, default settings, `getSetting`/`setSetting`, the shared selection rendering
//! quality), js/src/interface/panel/header/tool/ToolMenu.js (`getOptionDefinitions`),
//! js/src/interface/panel/header/tool/ToolSelector.js (the three icon overrides)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's blend list (R16), shared settings (R17) and defaults, DPI-dependent ones too (R19)
use std::collections::BTreeMap;

use crate::pixel::content::ContentBlendMode;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolType {
    RectangleSelect,
    Move,
    LassoSelect,
    MoveSelection,
    // S§4.3: the six Extra tools (not Paint.NET's).
    MagneticLasso,
    ObjectSelection,
    EllipseSelect,
    Zoom,
    MagicWand,
    Pan,
    PaintBucket,
    Gradient,
    PaintBrush,
    Eraser,
    Pencil,
    ColorPicker,
    CloneStamp,
    Recolor,
    Remove,
    Patch,
    HealingBrush,
    SpotHealingBrush,
    Text,
    Line,
    Shapes,
}

use ToolType::*;

impl ToolType {
    /// `ToolType.VALUES`: the tool strip order, which is also the hotkey cycle order, with the
    /// Extra tools at their Tools-window places (R§A.4). Iterate [`ToolType::shown`] in the UI.
    pub const VALUES: [ToolType; 25] = [
        RectangleSelect,
        Move,
        LassoSelect,
        MoveSelection,
        MagneticLasso,
        ObjectSelection,
        EllipseSelect,
        Zoom,
        MagicWand,
        Pan,
        PaintBucket,
        Gradient,
        PaintBrush,
        Eraser,
        Pencil,
        ColorPicker,
        CloneStamp,
        Recolor,
        Remove,
        Patch,
        HealingBrush,
        SpotHealingBrush,
        Text,
        Line,
        Shapes,
    ];

    /// An Extra tool (S§4.1): not Paint.NET's.
    pub const fn is_extra(self) -> bool {
        matches!(
            self,
            MagneticLasso | ObjectSelection | Remove | Patch | HealingBrush | SpotHealingBrush
        )
    }

    /// Whether the tool is in the list `extras` picks: every tool, or Paint.NET's alone.
    pub const fn is_shown(self, extras: bool) -> bool {
        extras || !self.is_extra()
    }

    /// [`ToolType::VALUES`] without the Extra tools unless `extras`.
    pub fn shown(extras: bool) -> impl Iterator<Item = ToolType> {
        Self::VALUES.into_iter().filter(move |t| t.is_shown(extras))
    }

    /// (id, iconName, hotKey, skipIfActiveOnHotKey)
    const fn info(self) -> (&'static str, &'static str, char, bool) {
        match self {
            // quirk: Pan's iconName already ends in "_icon", so `getIconSrc()` (the Tools window,
            // history mementos) asks for the missing "pan_tool_icon_icon.png".
            Pan => ("panTool", "pan_tool_icon", 'H', false),
            RectangleSelect => ("rectangleSelectTool", "rectangle_select_tool", 'S', false),
            LassoSelect => ("lassoSelectTool", "lasso_select_tool", 'S', false),
            EllipseSelect => ("ellipseSelectTool", "ellipse_select_tool", 'S', false),
            MoveSelection => ("moveSelectionTool", "move_selection_tool", 'M', false),
            Move => ("moveTool", "move_tool", 'M', false),
            Zoom => ("zoomTool", "zoom_tool", 'Z', false),
            PaintBucket => ("paintBucketTool", "paint_bucket", 'F', false),
            PaintBrush => ("paintBrushTool", "paint_brush_tool", 'B', true),
            Eraser => ("eraserTool", "eraser_tool", 'E', true),
            Pencil => ("pencilTool", "pencil_tool", 'P', true),
            ColorPicker => ("colorPickerTool", "color_picker_tool", 'K', true),
            Line => ("lineTool", "line_tool", 'O', false),
            Shapes => ("shapesTool", "shapes_tool", 'O', false),
            MagicWand => ("magicWandTool", "magic_wand_tool", 'S', false),
            Gradient => ("gradientTool", "gradient_tool", 'G', false),
            CloneStamp => ("cloneStampTool", "clone_stamp_tool", 'L', false),
            Recolor => ("recolorTool", "recoloring_tool", 'R', true),
            Text => ("textTool", "text_tool", 'T', false),
            MagneticLasso => ("magneticLassoTool", "magnetic_lasso_tool", 'S', false),
            ObjectSelection => ("objectSelectionTool", "object_selection_tool", 'S', false),
            Remove => ("removeTool", "remove_tool", 'J', false),
            Patch => ("patchTool", "patch_tool", 'J', false),
            HealingBrush => ("healingBrushTool", "healing_brush_tool", 'J', false),
            SpotHealingBrush => (
                "spotHealingBrushTool",
                "spot_healing_brush_tool",
                'J',
                false,
            ),
        }
    }

    pub const fn id(self) -> &'static str {
        self.info().0
    }

    /// The tool's icon in the tool selector, its drop menu and the status bar
    /// (`ToolSelector`'s `DropEntry` icon path): `ActionItem.getIconPath` of the id, except the
    /// three `withIconPathKey` overrides.
    pub const fn icon(self) -> &'static str {
        match self {
            RectangleSelect => "rectangle_select_tool_icon.png",
            Move => "move_tool_icon.png",
            LassoSelect => "lasso_select_tool_icon.png",
            MoveSelection => "move_selection_tool_icon.png",
            EllipseSelect => "ellipse_select_tool_icon.png",
            Zoom => "zoom_tool_icon.png",
            MagicWand => "magic_wand_tool_icon.png",
            Pan => "pan_tool_icon.png",
            // "Fix the icon path of the paint bucket and recolor tools".
            PaintBucket => "paint_bucket_icon.png",
            Gradient => "gradient_tool_icon.png",
            PaintBrush => "paint_brush_tool_icon.png",
            Eraser => "eraser_tool_icon.png",
            Pencil => "pencil_tool_icon.png",
            ColorPicker => "color_picker_tool_icon.png",
            CloneStamp => "clone_stamp_tool_icon.png",
            Recolor => "recoloring_tool_icon.png",
            Text => "text_tool_icon.png",
            Line => "line_tool_icon.png",
            // The compact toolbar and dropdown use the square glyph.
            Shapes => "shapes_tool_icon_16.png",
            // assets/icons-svg/photocraft/.
            MagneticLasso => "magnetic_lasso_tool_icon.png",
            ObjectSelection => "object_selection_tool_icon.png",
            Remove => "remove_tool_icon.png",
            Patch => "patch_tool_icon.png",
            HealingBrush => "healing_brush_tool_icon.png",
            SpotHealingBrush => "spot_healing_brush_tool_icon.png",
        }
    }

    /// `getIconSrc()`: the history memento icon (`assets/icons/<iconName>_icon.png`).
    pub fn icon_src(self) -> String {
        format!("assets/icons/{}_icon.png", self.info().1)
    }

    pub const fn hot_key(self) -> char {
        self.info().2
    }

    pub const fn skip_if_active_on_hot_key(self) -> bool {
        self.info().3
    }

    pub fn from_id(id: &str) -> Option<ToolType> {
        Self::VALUES.into_iter().find(|t| t.id() == id)
    }

    /// The `setSettings` defaults of the `ToolType` static block at UI scale 1.
    pub fn default_settings(self) -> Map<String, Value> {
        self.default_settings_scaled(1.0)
    }

    /// The `setSettings` defaults with Paint.NET's values (R19, [`R19_TOOL_DEFAULTS`]); `s` is the
    /// UI scale the DPI-dependent sizes follow.
    pub fn default_settings_scaled(self, s: f32) -> Map<String, Value> {
        let width = (2.0 * s).trunc() as i64;
        let selection = json!({"combineMode": "replace", "renderingQuality": "high"});
        let brush = json!({
            "width": width, "pressure": true, "hardness": 75, "spacing": 15, "smoothing": true,
            "antialias": true, "renderingQuality": "high"
        });
        let merge = |a: Value, b: &Value| {
            let mut m = a.as_object().unwrap().clone();
            m.extend(b.as_object().unwrap().clone());
            Value::Object(m)
        };
        let v = match self {
            RectangleSelect => merge(
                json!({
                    "selectionMode": "normal", "selectionWidth": 400, "selectionHeight": 300,
                    "selectionUnits": "pixel"
                }),
                &selection,
            ),
            LassoSelect | EllipseSelect => selection,
            MoveSelection => json!({"renderingQuality": "high", "snap": false}),
            Zoom | Pan => json!({"renderingQuality": "high"}),
            Move => json!({
                "resampling": "highQualityCubic", "gammaCorrected": true, "renderingQuality": "high",
                "distort": false, "snap": false
            }),
            PaintBucket => json!({
                "tolerance": 50, "floodMode": "contiguous", "fillStyle": "solid",
                "alphaMode": "premultiplied", "sampleMode": "layer", "antialias": true,
                "blendMode": "normal", "renderingQuality": "high"
            }),
            PaintBrush => merge(json!({"fillStyle": "solid", "blendMode": "normal"}), &brush),
            Eraser => brush,
            Pencil => json!({"blendMode": "normal", "renderingQuality": "high"}),
            ColorPicker => json!({
                "sampleMode": "layer", "sampleSize": 1, "afterClick": "none",
                "renderingQuality": "high"
            }),
            Line => json!({
                "width": width, "dash": "solid", "startCap": "flat", "endCap": "flat",
                "fillStyle": "solid", "curveType": "spline", "blendMode": "normal",
                "antialias": true, "renderingQuality": "high"
            }),
            Shapes => json!({
                "shape": "rectangle", "width": width, "drawType": "outline", "dash": "solid",
                "fillStyle": "solid", "radius": (10.0 * s).trunc() as i64, "antialias": true,
                "blendMode": "normal", "renderingQuality": "high"
            }),
            MagicWand => json!({
                "tolerance": 50, "floodMode": "contiguous", "combineMode": "replace",
                "alphaMode": "premultiplied", "sampleMode": "layer", "renderingQuality": "high"
            }),
            Gradient => json!({
                "gradientType": "linear", "gradientMode": "color", "repeatMode": "none",
                "antialias": true, "blendMode": "normal", "renderingQuality": "high"
            }),
            // S§4.3: Aligned and Sample: Layer, Paint.NET's behaviour (the Extra options).
            CloneStamp => merge(
                json!({"blendMode": "normal", "aligned": true, "sampleImage": false}),
                &brush,
            ),
            Recolor => merge(
                json!({"tolerance": 50, "alphaMode": "premultiplied", "recolorSampling": "once"}),
                &brush,
            ),
            Text => json!({
                "fontFamily": "Calibri", "fontSize": (12.0 * s).trunc() as i64, "fontUnit": "points",
                "bold": false, "italic": false, "underline": false, "strikeout": false,
                "textRendering": "smooth", "align": "left", "antialias": true,
                "blendMode": "normal", "renderingQuality": "high", "editableText": true,
                "textAngle": 0, "textShape": "straight", "textBend": 50
            }),
            // S§4.3. The brush tools take the Paintbrush's brush keys, not shared with it (R17 is
            // Paint.NET's sharing).
            // Their combine mode is the selection tools' (Ruling R20, [`READ_THROUGH`]).
            MagneticLasso => json!({
                "width": 10, "contrast": 10, "frequency": 57, "renderingQuality": "high"
            }),
            // The method: MobileSAM on desktop (built in), GrabCut on the web (MobileSAM's model
            // loads there only once picked).
            ObjectSelection => json!({
                "segmenter": if cfg!(target_arch = "wasm32") { "grabcut" } else { "mobilesam" },
                "sampleImage": false, "objectFinder": true, "renderingQuality": "high"
            }),
            Remove => merge(
                json!({"sampleImage": false, "removeEachStroke": true}),
                &brush,
            ),
            Patch => json!({
                "patchMode": "normal", "patchSource": "source", "structure": 4, "colorAmount": 0,
                "sampleImage": false, "renderingQuality": "high"
            }),
            HealingBrush => merge(
                json!({"blendMode": "normal", "aligned": true, "sampleImage": false}),
                &brush,
            ),
            SpotHealingBrush => merge(
                json!({"spotType": "contentAware", "sampleImage": false}),
                &brush,
            ),
        };
        v.as_object().unwrap().clone()
    }
}

/// Settings Paint.NET shares across tools (R17), each with its owners in migration precedence
/// (painting-tools-1.md §2; selection-and-move.md §2.0; painting-tools-2.md §2.0).
/// `renderingQuality` keeps its own shared key (`paintdotjs.selectionRenderingQuality`).
pub const SHARED_KEYS: &[(&str, &[ToolType])] = &[
    (
        "width",
        &[PaintBrush, Eraser, CloneStamp, Recolor, Line, Shapes],
    ),
    ("hardness", &[PaintBrush, Eraser, CloneStamp, Recolor]),
    ("spacing", &[PaintBrush, Eraser, CloneStamp, Recolor]),
    ("pressure", &[PaintBrush, Eraser, CloneStamp, Recolor]),
    ("smoothing", &[PaintBrush, Eraser, CloneStamp, Recolor]),
    (
        "antialias",
        &[
            PaintBrush,
            Eraser,
            CloneStamp,
            Recolor,
            PaintBucket,
            Gradient,
            Line,
            Shapes,
            Text,
        ],
    ),
    (
        "blendMode",
        &[
            PaintBrush,
            Pencil,
            CloneStamp,
            PaintBucket,
            Gradient,
            Line,
            Shapes,
            Text,
        ],
    ),
    ("tolerance", &[MagicWand, PaintBucket, Recolor]),
    ("alphaMode", &[MagicWand, PaintBucket, Recolor]),
    ("sampleMode", &[MagicWand, PaintBucket, ColorPicker]),
    ("floodMode", &[MagicWand, PaintBucket]),
    (
        "combineMode",
        &[RectangleSelect, LassoSelect, EllipseSelect, MagicWand],
    ),
];

/// Ruling R20: (tool, key) an Extra tool reads and writes through to its [`SHARED_KEYS`] group
/// without owning it, so the group and its migration stay Paint.NET's.
const READ_THROUGH: &[(ToolType, &str)] = &[
    (MagneticLasso, "combineMode"),
    (ObjectSelection, "combineMode"),
    (Patch, "combineMode"),
];

/// Paint.NET defaults that differ from paintdotjs's `setSettings` (R19).
pub const R19_TOOL_DEFAULTS: &[(ToolType, &str, &str)] = &[
    (
        Recolor,
        "tolerance",
        "painting-tools-1.md §2: Tolerance 0.5 (shared)",
    ),
    (
        RectangleSelect,
        "selectionWidth",
        "selection-and-move.md §2.0: Width 400",
    ),
    (
        RectangleSelect,
        "selectionHeight",
        "selection-and-move.md §2.0: Height 300",
    ),
    (
        PaintBrush,
        "width",
        "painting-tools-1.md §2: twice the UI scale, truncated",
    ),
    (
        RectangleSelect,
        "selectionUnits",
        "selection-and-move.md §2.0: units Pixel (the Fixed Size units combo, ui-text-and-glyphs.md §7.5)",
    ),
    (Shapes, "radius", "painting-tools-2.md §2.0: trunc(10·s)"),
    (Text, "fontSize", "ui-text-and-glyphs.md §2.8: trunc(12·s)"),
];

/// paintdotjs's `setSettings` default of `key` (rule B's untouched value): the scale-1 defaults
/// with the one [`R19_TOOL_DEFAULTS`] change to a shared key (Recolor tolerance) undone.
fn paintdotjs_default(tool: ToolType, key: &str) -> Value {
    if (tool, key) == (Recolor, "tolerance") {
        25.into()
    } else {
        tool.default_settings()[key].clone()
    }
}

/// localStorage key prefix of a tool's settings JSON.
pub const TOOL_SETTINGS_KEY_PREFIX: &str = "paintdotjs.toolSettings.";
/// localStorage key of the shared selection rendering quality.
pub const SELECTION_RENDERING_QUALITY_KEY: &str = "paintdotjs.selectionRenderingQuality";
/// Marker of the one-time R17 shared-settings migration (controller ruling, Task 17h): while it
/// does not hold [`SHARED_MIGRATION_VERSION`], [`ToolSettings::load_scaled`] migrates by rule B
/// and the app then writes the version; once set, valid shared values load verbatim.
pub const SHARED_MIGRATION_KEY: &str = "paintdotjs.toolSettings.sharedMigrated";
/// The [`SHARED_MIGRATION_KEY`] value. Bump it whenever [`SHARED_KEYS`] grows (a new key or a new
/// owner), so stores migrated before run rule B for the new sharing.
pub const SHARED_MIGRATION_VERSION: &str = "1";

/// Every tool's `settings` (defaults merged with the saved JSON) and the shared
/// `ToolType.selectionRenderingQuality`. Writes are queued for the UI to persist.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSettings {
    /// Session capability, gated by pen.pointerInput; never written to preferences.
    pub pen_seen: bool,
    pub per_tool: BTreeMap<String, Map<String, Value>>,
    pub rendering_quality: String,
    /// (tool id, key) written since the last `dirty_keys`.
    dirty: Vec<(String, String)>,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self::load(|_| None)
    }
}

/// `setSettings` application order of the `ToolType` static block (it decides the rendering
/// quality migration), the Extra tools after it.
const SET_SETTINGS_ORDER: [ToolType; 25] = [
    RectangleSelect,
    LassoSelect,
    EllipseSelect,
    MoveSelection,
    Move,
    Zoom,
    Pan,
    PaintBucket,
    PaintBrush,
    Eraser,
    Pencil,
    ColorPicker,
    Line,
    Shapes,
    MagicWand,
    Gradient,
    CloneStamp,
    Recolor,
    Text,
    MagneticLasso,
    ObjectSelection,
    Remove,
    Patch,
    HealingBrush,
    SpotHealingBrush,
];

impl ToolSettings {
    /// [`ToolSettings::load_scaled`] at UI scale 1.
    pub fn load(prefs_get: impl Fn(&str) -> Option<String>) -> ToolSettings {
        Self::load_scaled(prefs_get, 1.0)
    }

    /// The `ToolType` constructors (saved JSON) and static block (`setSettings`, migrations),
    /// with the defaults of `ui_scale` and the R17 shared-key migration.
    pub fn load_scaled(prefs_get: impl Fn(&str) -> Option<String>, ui_scale: f32) -> ToolSettings {
        let saved = |t: ToolType| -> Map<String, Value> {
            prefs_get(&format!("{TOOL_SETTINGS_KEY_PREFIX}{}", t.id()))
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default()
        };
        let mut per_tool: BTreeMap<String, Map<String, Value>> = ToolType::VALUES
            .iter()
            .map(|t| (t.id().to_string(), saved(*t)))
            .collect();
        let saved = per_tool.clone();
        let mut quality: Option<String> = None;
        for t in SET_SETTINGS_ORDER {
            // setSettings: defaults, then the saved values over them.
            let mut merged = t.default_settings_scaled(ui_scale);
            merged.extend(per_tool[t.id()].clone());
            per_tool.insert(t.id().into(), merged);
            // getSelectionRenderingQuality(this.settings.renderingQuality), cached on first use.
            if quality.is_none() {
                let fallback = per_tool[t.id()]["renderingQuality"].clone();
                quality = Some(Self::migrate_quality(&prefs_get, &per_tool, fallback));
            }
            let q = quality.clone().unwrap();
            per_tool.get_mut(t.id()).unwrap()["renderingQuality"] = Value::String(q);
        }
        let mut settings = ToolSettings {
            pen_seen: false,
            per_tool,
            rendering_quality: quality.unwrap(),
            dirty: vec![],
        };
        // Migrate settings saved by the earlier three-choice implementation (not persisted).
        let resampling = settings.per_tool.get_mut(Move.id()).unwrap();
        match resampling["resampling"].as_str() {
            Some("bilinear") => resampling["resampling"] = "linear".into(),
            Some("bicubic") => resampling["resampling"] = "highQualityCubic".into(),
            _ => {}
        }
        // The legacy round/flat line default becomes flat/flat (persisted via setSetting).
        if settings.get(Line, "startCap") == "round" && settings.get(Line, "endCap") == "flat" {
            settings.set(Line, "startCap", "flat".into());
        }
        // Saved paintdotjs CSS blend modes outside Paint.NET's 15 load as Normal (spec §0.7).
        for t in SET_SETTINGS_ORDER {
            let Some(v) = settings.per_tool[t.id()].get("blendMode") else {
                continue;
            };
            let known = v
                .as_str()
                .is_some_and(|v| ContentBlendMode::from_value(v).value() == v);
            if !known {
                settings.set_one(t, "blendMode", "normal".into());
            }
        }
        // R17 migration, controller ruling "rule B" (Task 17h; it replaces the plan's
        // precedence-only rule): paintdotjs saves whole objects, so a saved value counts only if it
        // differs from that owner's paintdotjs default. The first counting owner in precedence
        // wins, else the Paint.NET default (R19). The value is mirrored into every owner and
        // persisted where it changed. Second ruling: this runs once, until SHARED_MIGRATION_KEY
        // is set; afterwards every saved value counts, so an explicit default survives restarts.
        // Review fix: an invalid stored value never counts, so it falls back as if absent.
        let migrated = prefs_get(SHARED_MIGRATION_KEY).as_deref() == Some(SHARED_MIGRATION_VERSION);
        for &(key, owners) in SHARED_KEYS {
            let chosen = owners.iter().find_map(|&t| {
                let v = &settings.per_tool[t.id()][key];
                let counts = saved[t.id()].contains_key(key)
                    && settings.is_valid(t, key, v)
                    && (migrated || *v != paintdotjs_default(t, key));
                counts.then(|| v.clone())
            });
            let v =
                chosen.unwrap_or_else(|| owners[0].default_settings_scaled(ui_scale)[key].clone());
            for &t in owners {
                if settings.per_tool[t.id()][key] != v {
                    settings.set_one(t, key, v.clone());
                }
            }
        }
        settings
    }

    /// Whether `tool`'s `key` can hold `value`: the JSON type of its default and, for a choice
    /// option, one of the listed values (Task 17h review).
    fn is_valid(&self, tool: ToolType, key: &str, value: &Value) -> bool {
        let default = &tool.default_settings()[key];
        std::mem::discriminant(default) == std::mem::discriminant(value)
            && option_definitions(tool, self)
                .iter()
                .find(|d| d.key == key)
                .is_none_or(|d| d.values.is_empty() || d.index_of(value).is_some())
    }

    /// `getSelectionRenderingQuality(fallback)` before the value is cached: the saved shared
    /// value, else the first selection tool whose current settings hold a valid one (only the
    /// tools `setSettings` already ran for are merged with their defaults).
    fn migrate_quality(
        prefs_get: &impl Fn(&str) -> Option<String>,
        per_tool: &BTreeMap<String, Map<String, Value>>,
        fallback: Value,
    ) -> String {
        let mut quality = prefs_get(SELECTION_RENDERING_QUALITY_KEY).map(Value::String);
        if !matches!(
            quality.as_ref().and_then(Value::as_str),
            Some("high" | "aliased")
        ) {
            let valid = |v: &Value| matches!(v.as_str(), Some("high" | "aliased"));
            let saved = [
                MoveSelection,
                RectangleSelect,
                LassoSelect,
                EllipseSelect,
                MagicWand,
                Move,
            ]
            .iter()
            .map(|t| &per_tool[t.id()])
            .find(|s| s.get("renderingQuality").is_some_and(valid));
            quality = Some(match saved {
                Some(s) => s["renderingQuality"].clone(),
                None => fallback,
            });
        }
        if quality.as_ref().and_then(Value::as_str) == Some("aliased") {
            "aliased".into()
        } else {
            "high".into()
        }
    }

    /// `tool`, or the first owner of the group `tool` reads `key` through ([`READ_THROUGH`]).
    fn holder(tool: ToolType, key: &str) -> ToolType {
        match Self::shared_owners(key) {
            [first, ..] if READ_THROUGH.contains(&(tool, key)) => *first,
            _ => tool,
        }
    }

    /// `type.getSetting(key)` (JS fallback `null`).
    pub fn get(&self, tool: ToolType, key: &str) -> Value {
        if key == "renderingQuality" {
            return Value::String(self.rendering_quality.clone());
        }
        let tool = Self::holder(tool, key);
        self.per_tool[tool.id()]
            .get(key)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// The owners of a key Paint.NET shares ([`SHARED_KEYS`]); empty when it is per tool.
    pub fn shared_owners(key: &str) -> &'static [ToolType] {
        SHARED_KEYS
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(&[], |(_, owners)| owners)
    }

    /// `type.setSetting(key, value)`; the rendering quality is shared by every tool, and a
    /// [`SHARED_KEYS`] key set on one of its owners is written to all of them (R17).
    pub fn set(&mut self, tool: ToolType, key: &str, value: Value) {
        let tool = Self::holder(tool, key);
        if key == "renderingQuality" {
            let q = if value == "aliased" {
                "aliased"
            } else {
                "high"
            };
            self.rendering_quality = q.into();
            for settings in self.per_tool.values_mut() {
                settings.insert(key.into(), Value::String(q.into()));
            }
            self.dirty.push((tool.id().into(), key.into()));
        } else if Self::shared_owners(key).contains(&tool) {
            for &t in Self::shared_owners(key) {
                self.set_one(t, key, value.clone());
            }
        } else {
            self.set_one(tool, key, value);
        }
    }

    /// Writes `key` of `tool` alone and queues it for persistence.
    fn set_one(&mut self, tool: ToolType, key: &str, value: Value) {
        self.per_tool
            .get_mut(tool.id())
            .unwrap()
            .insert(key.into(), value);
        self.dirty.push((tool.id().into(), key.into()));
    }

    /// The (tool id, key) pairs written since the last call.
    pub fn dirty_keys(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.dirty)
    }

    /// What `setSetting(key)` of `tool_id` writes to localStorage: (key, value).
    pub fn storage_entry(&self, tool_id: &str, key: &str) -> (String, String) {
        if key == "renderingQuality" {
            return (
                SELECTION_RENDERING_QUALITY_KEY.into(),
                self.rendering_quality.clone(),
            );
        }
        let mut stored = self.per_tool[tool_id].clone();
        // An Extra option at its default is not stored, so a Paint.NET tool stores what it did
        // before it had any.
        if let Some(tool) = ToolType::from_id(tool_id) {
            let defaults = tool.default_settings();
            for d in option_definitions_with_fonts(tool, self, &[]) {
                if d.extra && stored.get(d.key) == defaults.get(d.key) {
                    stored.remove(d.key);
                }
            }
        }
        (
            format!("{TOOL_SETTINGS_KEY_PREFIX}{tool_id}"),
            Value::Object(stored).to_string(),
        )
    }
}

/// `ToolMenu.getOptionDefinitions` kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    Select,
    IconChoice,
    ShapeChoice,
    IconGroup,
    Size,
    Slider,
    Number,
    FontStyleToggle,
    Action,
    /// An image button (`createImageButton`): Rectangle Select's swap.
    Button,
    /// An on/off option (a JSON bool): a checkbox captioned with `label` (user, 2026-10-24: not
    /// a "Name: On" / "Name: Off" list).
    Checkbox,
}

/// One tool option definition. `values` are (value, text, icon file); a value is the JS literal
/// as text (see [`json_value`]).
#[derive(Clone, Debug, PartialEq)]
pub struct OptionDef {
    pub kind: OptionKind,
    /// "" for the Finish action (it has none).
    pub key: &'static str,
    /// `None` for JS's empty label.
    pub label: Option<&'static str>,
    pub values: Vec<(&'static str, &'static str, Option<&'static str>)>,
    pub width: Option<f32>,
    pub section: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub presets: &'static [f64],
    /// Select: `toolbar`, `split` (cycle on main click, checkmark menu), `fitSelected`.
    pub toolbar: bool,
    pub split: bool,
    pub fit_selected: bool,
    /// IconChoice: `showText`.
    pub show_text: bool,
    /// Select `icon` (the leading icon) / Action `icon`.
    pub icon: Option<&'static str>,
    pub menu_width: Option<f32>,
    pub font_preview: bool,
    pub font_choices: Option<Vec<(String, String)>>,
    /// ShapeChoice groups: (label, values).
    pub groups: Vec<(&'static str, Vec<&'static str>)>,
    /// FontStyleToggle `text` and `title`.
    pub text: &'static str,
    pub title: &'static str,
    /// An option only an Extra tool (or an Extra feature) has: the toolbar draws the plus after
    /// its label (S§4.1). Reused Paint.NET controls leave it false.
    pub extra: bool,
    /// Greyed out: the toolbar draws it but ignores clicks. Only the dropdown kinds (Select,
    /// IconChoice, ShapeChoice) honour it; the Text tool's blend mode sets it while Editable text
    /// is on (S§4.5).
    pub disabled: bool,
}

impl OptionDef {
    fn new(kind: OptionKind, label: &'static str, key: &'static str) -> OptionDef {
        OptionDef {
            kind,
            key,
            label: (!label.is_empty()).then_some(label),
            values: vec![],
            width: None,
            section: false,
            min: None,
            max: None,
            step: None,
            presets: &[],
            toolbar: false,
            split: false,
            fit_selected: false,
            show_text: false,
            icon: None,
            menu_width: None,
            font_preview: false,
            font_choices: None,
            groups: vec![],
            text: "",
            title: "",
            extra: false,
            disabled: false,
        }
    }

    pub fn entry_count(&self) -> usize {
        self.font_choices
            .as_ref()
            .map_or(self.values.len(), Vec::len)
    }
    pub fn entry(&self, i: usize) -> (&str, &str, Option<&str>) {
        if let Some(fonts) = &self.font_choices {
            let (value, label) = &fonts[i];
            (value, label, None)
        } else {
            self.values[i]
        }
    }
    /// `values[i][0]` as the JS value.
    pub fn value(&self, i: usize) -> Value {
        json_value(self.entry(i).0)
    }

    /// The index of `value` in `values`.
    pub fn index_of(&self, value: &Value) -> Option<usize> {
        (0..self.entry_count()).find(|&i| &self.value(i) == value)
    }
}

/// A value literal of an option table: `true`/`false` and numbers are JS booleans/numbers,
/// anything else a string.
pub fn json_value(s: &str) -> Value {
    match s {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => match s.parse::<i64>() {
            Ok(n) => json!(n),
            Err(_) => Value::String(s.into()),
        },
    }
}

type Entries = Vec<(&'static str, &'static str, Option<&'static str>)>;

fn texts(values: &[(&'static str, &'static str)]) -> Entries {
    values.iter().map(|&(v, t)| (v, t, None)).collect()
}

fn icons(values: &[(&'static str, &'static str, &'static str)]) -> Entries {
    values.iter().map(|&(v, t, i)| (v, t, Some(i))).collect()
}

/// `brushSizes`.
pub const BRUSH_SIZES: [f64; 68] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 20.0, 25.0,
    30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0, 75.0, 80.0, 85.0, 90.0, 95.0, 100.0,
    125.0, 150.0, 175.0, 200.0, 225.0, 250.0, 275.0, 300.0, 325.0, 350.0, 375.0, 400.0, 425.0,
    450.0, 475.0, 500.0, 550.0, 600.0, 650.0, 700.0, 750.0, 800.0, 850.0, 900.0, 950.0, 1000.0,
    1100.0, 1200.0, 1300.0, 1400.0, 1500.0, 1600.0, 1700.0, 1800.0, 1900.0, 2000.0,
];

/// The Shapes Radius combo's presets: Paint.NET's run 1–250; the list itself is not known,
/// so it is the brush presets up to 250.
// calibrate: B-L3 — the real preset list (a screenshot of the open Radius drop-down).
pub const RADIUS_SIZES: [f64; 38] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 20.0, 25.0,
    30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0, 75.0, 80.0, 85.0, 90.0, 95.0, 100.0,
    125.0, 150.0, 175.0, 200.0, 225.0, 250.0,
];

/// `fontSizes`.
pub const FONT_SIZES: [f64; 23] = [
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0,
    84.0, 96.0, 108.0, 144.0, 192.0, 216.0, 288.0,
];

pub(crate) const FILL_STYLES: [(&str, &str); 54] = [
    ("solid", "Solid Color"),
    ("backwardDiagonal", "Backward Diagonal"),
    ("forwardDiagonal", "Forward Diagonal"),
    ("horizontal", "Horizontal"),
    ("vertical", "Vertical"),
    ("cross", "Cross"),
    ("diagonalCross", "Diagonal Cross"),
    ("percent05", "5%"),
    ("percent10", "10%"),
    ("percent20", "20%"),
    ("percent25", "25%"),
    ("percent30", "30%"),
    ("percent40", "40%"),
    ("percent50", "50%"),
    ("percent60", "60%"),
    ("percent70", "70%"),
    ("percent75", "75%"),
    ("percent80", "80%"),
    ("percent90", "90%"),
    ("lightDownwardDiagonal", "Light Downward Diagonal"),
    ("lightUpwardDiagonal", "Light Upward Diagonal"),
    ("darkDownwardDiagonal", "Dark Downward Diagonal"),
    ("darkUpwardDiagonal", "Dark Upward Diagonal"),
    ("wideDownwardDiagonal", "Wide Downward Diagonal"),
    ("wideUpwardDiagonal", "Wide Upward Diagonal"),
    ("lightHorizontal", "Light Horizontal"),
    ("lightVertical", "Light Vertical"),
    ("narrowHorizontal", "Narrow Horizontal"),
    ("narrowVertical", "Narrow Vertical"),
    ("darkHorizontal", "Dark Horizontal"),
    ("darkVertical", "Dark Vertical"),
    ("dashedDownwardDiagonal", "Dashed Downward Diagonal"),
    ("dashedUpwardDiagonal", "Dashed Upward Diagonal"),
    ("dashedHorizontal", "Dashed Horizontal"),
    ("dashedVertical", "Dashed Vertical"),
    ("smallConfetti", "Small Confetti"),
    ("largeConfetti", "Large Confetti"),
    ("zigZag", "Zig Zag"),
    ("wave", "Wave"),
    ("diagonalBrick", "Diagonal Brick"),
    ("horizontalBrick", "Horizontal Brick"),
    ("weave", "Weave"),
    ("plaid", "Plaid"),
    ("divot", "Divot"),
    ("dottedGrid", "Dotted Grid"),
    ("dottedDiamond", "Dotted Diamond"),
    ("shingle", "Shingle"),
    ("trellis", "Trellis"),
    ("sphere", "Sphere"),
    ("smallGrid", "Small Grid"),
    ("smallCheckerBoard", "Small Checker Board"),
    ("largeCheckerBoard", "Large Checker Board"),
    ("outlinedDiamond", "Outlined Diamond"),
    ("solidDiamond", "Solid Diamond"),
];

fn fill_styles() -> Entries {
    texts(&FILL_STYLES)
}

/// The tool blending drop-down (R16, spec §0.7): (value, English label as `en.json
/// contentBlendMode` has it) in `ContentBlendMode::TOOLBAR` order (compositing.md §2.7
/// "Toolbar list").
pub const CONTENT_BLEND_MODES: [(&str, &str); 15] = [
    ("normal", "Normal"),
    ("multiply", "Multiply"),
    ("additive", "Additive"),
    ("colorBurn", "Color Burn"),
    ("colorDodge", "Color Dodge"),
    ("reflect", "Reflect"),
    ("glow", "Glow"),
    ("overlay", "Overlay"),
    ("difference", "Difference"),
    ("negation", "Negation"),
    ("lighten", "Lighten"),
    ("darken", "Darken"),
    ("screen", "Screen"),
    ("xor", "Xor"),
    ("overwrite", "Overwrite"),
];

/// Native ShapeChoice catalogue order, matching the grouped gallery below (Task 34 adds custom shapes).
const SHAPES: [(&str, &str); 29] = [
    ("rectangle", "Rectangle"),
    ("roundedRectangle", "Rounded rectangle"),
    ("ellipse", "Ellipse"),
    ("diamond", "Diamond"),
    ("trapezoid", "Trapezoid"),
    ("parallelogram", "Parallelogram"),
    ("triangle", "Triangle"),
    ("rightTriangle", "Right triangle"),
    ("pentagon", "Pentagon"),
    ("hexagon", "Hexagon"),
    ("heptagon", "Heptagon"),
    ("octagon", "Octagon"),
    ("star3", "Three-point star"),
    ("star4", "Four-point star"),
    ("star5", "Five-point star"),
    ("star6", "Six-point star"),
    ("blockArrow", "Arrow"),
    ("notchedArrow", "Notched arrow"),
    ("pentagonArrow", "Pentagon arrow"),
    ("chevronArrow", "Chevron arrow"),
    ("rectangularCallout", "Rectangular callout"),
    ("roundedCallout", "Rounded rectangular callout"),
    ("ellipticalCallout", "Elliptical callout"),
    ("cloudCallout", "Cloud callout"),
    ("lightningBolt", "Lightning bolt"),
    ("checkMark", "Check mark"),
    ("multiply", "Multiply"),
    ("gear", "Gear"),
    ("heart", "Heart"),
];

/// `ToolMenu.getOptionDefinitions(type)`, same order and conditionals. `fonts` is
/// `FontManager.getTextToolChoices()`.
pub fn option_definitions(tool: ToolType, settings: &ToolSettings) -> Vec<OptionDef> {
    option_definitions_with_fonts(tool, settings, &crate::fonts::text_tool_choices())
}

pub fn option_definitions_with_fonts(
    tool: ToolType,
    settings: &ToolSettings,
    fonts: &[(&'static str, &'static str)],
) -> Vec<OptionDef> {
    use OptionKind as K;
    let choices = |label, key, values: Entries, width: Option<f32>, toolbar, split, fit| {
        let mut d = OptionDef::new(K::Select, label, key);
        d.values = values;
        d.width = width;
        d.toolbar = toolbar;
        d.split = split;
        d.fit_selected = fit;
        d
    };
    let number = |label, key, min, max, step| {
        let mut d = OptionDef::new(K::Number, label, key);
        (d.min, d.max, d.step) = (Some(min), Some(max), Some(step));
        d
    };
    let size = |label, key, min, max, step, presets: &'static [f64]| {
        let mut d = OptionDef::new(K::Size, label, key);
        (d.min, d.max, d.step, d.presets) = (Some(min), Some(max), Some(step), presets);
        d
    };
    let brush_size = || size("Brush size:", "width", 1.0, 2000.0, 1.0, &BRUSH_SIZES);
    let slider = |label, key, min, max| {
        let mut d = OptionDef::new(K::Slider, label, key);
        (d.min, d.max, d.step) = (Some(min), Some(max), Some(1.0));
        d
    };
    let icon_choice = |label,
                       key,
                       values: &[(&'static str, &'static str, &'static str)],
                       width: f32,
                       show_text| {
        let mut d = OptionDef::new(K::IconChoice, label, key);
        d.values = icons(values);
        d.width = Some(width);
        d.show_text = show_text;
        d.split = true;
        // A split with text autosizes to it; `width` stays the popup's minimum.
        d.fit_selected = show_text;
        d
    };
    let icon_group = |label, key, values: &[(&'static str, &'static str, &'static str)]| {
        let mut d = OptionDef::new(K::IconGroup, label, key);
        d.values = icons(values);
        d
    };
    let font_style_toggle = |text, key, title| {
        let mut d = OptionDef::new(K::FontStyleToggle, "", key);
        (d.text, d.title) = (text, title);
        d
    };
    let section = |mut d: OptionDef| {
        d.section = true;
        d
    };
    // S§4.3's Extra options the retouch tools share (the plus after the label).
    let sample_image = || {
        let mut d = choices(
            "Sample:",
            "sampleImage",
            texts(&[("false", "Layer"), ("true", "Image")]),
            Some(70.0),
            false,
            false,
            false,
        );
        d.extra = true;
        d
    };
    let extra_checkbox = |label, key| {
        let mut d = OptionDef::new(K::Checkbox, label, key);
        d.extra = true;
        d
    };
    // Snap (Extra, `tool::snap`): the move tools' drags lock onto edges and centres.
    let snap = || extra_checkbox("Snap", "snap");
    let combine = || {
        icon_group(
            "",
            "combineMode",
            &[
                (
                    "replace",
                    "Replace",
                    "enum_selection_combine_mode_replace.png",
                ),
                (
                    "union",
                    "Add (union)",
                    "enum_selection_combine_mode_union.png",
                ),
                (
                    "exclude",
                    "Subtract",
                    "enum_selection_combine_mode_exclude.png",
                ),
                (
                    "intersect",
                    "Intersect",
                    "enum_selection_combine_mode_intersect.png",
                ),
                ("xor", "Invert (xor)", "enum_selection_combine_mode_xor.png"),
            ],
        )
    };
    let quality = || {
        icon_choice(
            "",
            "renderingQuality",
            &[
                (
                    "high",
                    "Antialiased selection quality",
                    "enum_selection_rendering_quality_high_quality_antialiased.png",
                ),
                (
                    "aliased",
                    "Pixelated selection quality",
                    "enum_selection_rendering_quality_aliased.png",
                ),
            ],
            35.0,
            false,
        )
    };
    let antialias = || {
        icon_choice(
            "",
            "antialias",
            &[
                ("true", "Antialiased rendering", "anti_aliasing_true.png"),
                ("false", "Aliased rendering", "anti_aliasing_false.png"),
            ],
            35.0,
            false,
        )
    };
    let smoothing = || {
        icon_choice(
            "",
            "smoothing",
            &[
                (
                    "true",
                    "Smoothed path",
                    "enum_input_path_smoothing_enabled.png",
                ),
                (
                    "false",
                    "Unsmoothed path",
                    "enum_input_path_smoothing_disabled.png",
                ),
            ],
            35.0,
            false,
        )
    };
    let alpha_mode = || {
        icon_choice(
            "",
            "alphaMode",
            &[
                (
                    "premultiplied",
                    "Premultiplied",
                    "enum_tolerance_alpha_mode_premultiplied.png",
                ),
                (
                    "straight",
                    "Straight",
                    "enum_tolerance_alpha_mode_straight.png",
                ),
            ],
            35.0,
            false,
        )
    };
    let sample_scope = || {
        icon_choice(
            "Sampling:",
            "sampleMode",
            &[
                ("layer", "Layer", "sample_all_layers_false.png"),
                ("image", "Image", "sample_all_layers_true.png"),
            ],
            70.0,
            true,
        )
    };
    let blend = || {
        let mut d = OptionDef::new(K::Select, "", "blendMode");
        d.icon = Some("alpha_blending_true.png");
        d.width = Some(82.0);
        d.toolbar = true;
        // The blend split: image and text.
        d.split = true;
        d.fit_selected = true;
        d.values = texts(&CONTENT_BLEND_MODES);
        d
    };
    let finish = || {
        let mut d = OptionDef::new(K::Action, "Finish", "");
        d.icon = Some("tool_strip_checked.png");
        d
    };
    // The fill combo: WinForms' 121 (paintdotjs 150).
    let fill = || {
        choices(
            "Fill:",
            "fillStyle",
            fill_styles(),
            Some(121.0),
            false,
            false,
            false,
        )
    };
    let flood_mode = || {
        icon_choice(
            "Flood Mode:",
            "floodMode",
            &[
                ("contiguous", "Contiguous", "enum_flood_mode_local.png"),
                ("global", "Global", "enum_flood_mode_global.png"),
            ],
            35.0,
            false,
        )
    };
    let tolerance = || slider("Tolerance:", "tolerance", 0.0, 100.0);
    let dash = |label| {
        icon_choice(
            label,
            "dash",
            &[
                ("solid", "Solid", "enum_dash_style_solid.png"),
                ("dash", "Dash", "enum_dash_style_dash.png"),
                ("dot", "Dot", "enum_dash_style_dot.png"),
                ("dashDot", "Dash Dot", "enum_dash_style_dash_dot.png"),
                (
                    "dashDotDot",
                    "Dash Dot Dot",
                    "enum_dash_style_dash_dot_dot.png",
                ),
            ],
            47.0,
            false,
        )
    };
    let pressure = || {
        icon_choice(
            "",
            "pressure",
            &[
                (
                    "false",
                    "Pressure sensitivity disabled",
                    "tool_config_strip_brush_enable_pressure_sensitivity_false.png",
                ),
                (
                    "true",
                    "Pressure sensitivity enabled",
                    "tool_config_strip_brush_enable_pressure_sensitivity_true.png",
                ),
            ],
            35.0,
            false,
        )
    };
    let brush_controls = |fill_option: bool, blending: bool| {
        let mut result = vec![
            brush_size(),
            pressure(),
            slider("Hardness:", "hardness", 0.0, 100.0),
            slider("Spacing:", "spacing", 1.0, 500.0),
        ];
        if fill_option {
            // Ruling R80: the Paintbrush's Fill is Solid / Hatch / None (painting-tools-1.md §2
            // inventory, §3.8), in that enum order.
            // calibrate: the open Fill list in Paint.NET — None's text and place.
            let mut f = fill();
            f.values.push(("none", "None", None));
            result.push(section(f));
        }
        result.push(section(smoothing()));
        result.push(antialias());
        if blending {
            result.push(blend());
        }
        result.push(quality());
        result
    };

    let mut options = match tool {
        RectangleSelect => {
            let mut options = vec![
                combine(),
                section(icon_choice(
                    "",
                    "selectionMode",
                    &[
                        ("normal", "Any Size", "enum_selection_draw_mode_normal.png"),
                        (
                            "fixedSize",
                            "Fixed Size",
                            "enum_selection_draw_mode_fixed_size.png",
                        ),
                        (
                            "fixedRatio",
                            "Fixed Ratio",
                            "enum_selection_draw_mode_fixed_ratio.png",
                        ),
                    ],
                    86.0,
                    true,
                )),
            ];
            // Rectangle Select: 50-DIP text boxes round a swap button, the units combo in Fixed
            // Size, then ‖ Q.
            let mode = settings.get(tool, "selectionMode");
            if mode != "normal" {
                let boxed = |mut d: OptionDef| {
                    d.width = Some(50.0);
                    d
                };
                options.push(boxed(number(
                    "Width:",
                    "selectionWidth",
                    1.0,
                    100000.0,
                    1.0,
                )));
                let mut swap = OptionDef::new(K::Button, "", "swapSelectionSize");
                (swap.icon, swap.title) = (Some("swap_icon.png"), "Swap");
                options.push(swap);
                options.push(boxed(number(
                    "Height:",
                    "selectionHeight",
                    1.0,
                    100000.0,
                    1.0,
                )));
            }
            if mode == "fixedSize" {
                options.push(choices(
                    "",
                    "selectionUnits",
                    texts(&[
                        ("pixel", "Pixels"),
                        ("inch", "Inches"),
                        ("centimeter", "Centimeters"),
                    ]),
                    Some(100.0),
                    false,
                    false,
                    false,
                ));
            }
            options.push(section(quality()));
            options
        }
        // R16: every tool shows the selection quality (Q); after the chooser's separator, or
        // after its own.
        LassoSelect | EllipseSelect => vec![combine(), section(quality())],
        MoveSelection => vec![quality(), section(finish()), section(snap())],
        Move => vec![
            choices(
                "Sampling:",
                "resampling",
                texts(&[
                    ("nearest", "Nearest Neighbor"),
                    ("linear", "Bilinear"),
                    ("multisampleLinear", "Multisample Bilinear"),
                    ("anisotropic", "Anisotropic"),
                    ("highQualityCubic", "Bicubic"),
                ]),
                Some(166.0),
                true,
                true,
                true,
            ),
            section(icon_choice(
                "",
                "gammaCorrected",
                &[
                    ("true", "Gamma Corrected", "enum_tool_gamma_mode_linear.png"),
                    ("false", "Companded", "enum_tool_gamma_mode_companded.png"),
                ],
                35.0,
                false,
            )),
            quality(),
            section(finish()),
            section(snap()),
            // R230: the Extra option after Paint.NET's row: what a corner handle does.
            section({
                let mut d = icon_choice(
                    "",
                    "distort",
                    &[
                        ("false", "Resize", "move_pixels_resize_icon.png"),
                        ("true", "Warp", "move_pixels_warp_icon.png"),
                    ],
                    35.0,
                    true,
                );
                d.extra = true;
                d.title = "Resize: a corner scales the selection\nWarp: a corner moves on its own";
                d
            }),
        ],
        Zoom | Pan => vec![quality()],
        // S§4.3: the brush controls Remove reuses (no plus), then its own two.
        Remove => vec![
            brush_size(),
            pressure(),
            slider("Hardness:", "hardness", 0.0, 100.0),
            section(antialias()),
            section(sample_image()),
            section(extra_checkbox(
                "Remove after each stroke",
                "removeEachStroke",
            )),
            section(quality()),
        ],
        // S§4.3.
        MagneticLasso => {
            let extra = |mut d: OptionDef| {
                d.extra = true;
                d
            };
            vec![
                combine(),
                section(extra(size(
                    "Width:",
                    "width",
                    1.0,
                    256.0,
                    1.0,
                    &RADIUS_SIZES,
                ))),
                extra(slider("Contrast:", "contrast", 1.0, 100.0)),
                extra(slider("Frequency:", "frequency", 0.0, 100.0)),
                section(quality()),
            ]
        }
        // S§4.3.
        ObjectSelection => {
            let mut sample = choices(
                "Sample:",
                "sampleImage",
                texts(&[("false", "Layer"), ("true", "Image")]),
                Some(70.0),
                false,
                false,
                false,
            );
            let finder = extra_checkbox("Object Finder", "objectFinder");
            let mut subject = OptionDef::new(K::Action, "Select Subject", "selectSubject");
            subject.icon = Some("object_selection_tool_icon.png");
            (sample.extra, subject.extra) = (true, true);
            // The method every lookup uses (box, Object Finder, Select Subject).
            let mut method = icon_choice(
                "",
                "segmenter",
                &[
                    (
                        "mobilesam",
                        "MobileSAM",
                        "object_selection_mobilesam_icon.png",
                    ),
                    ("grabcut", "GrabCut", "object_selection_grabcut_icon.png"),
                ],
                35.0,
                true,
            );
            method.extra = true;
            method.title = "MobileSAM: an AI model finds the object\nGrabCut: the classic color-based \
                            method";
            vec![
                combine(),
                section(method),
                section(sample),
                section(finder),
                section(subject),
                section(quality()),
            ]
        }
        // S§4.3: Paint.NET's combine row, then the Extra options: the mode, then Source /
        // Destination (Normal) or Structure and Color (Content-Aware), then Sample.
        Patch => {
            let extra = |mut d: OptionDef| {
                d.extra = true;
                d
            };
            let kinds = texts(&[("normal", "Normal"), ("contentAware", "Content-Aware")]);
            let mode = choices(
                "Patch:",
                "patchMode",
                kinds,
                Some(121.0),
                false,
                false,
                false,
            );
            let mut options = vec![combine(), section(extra(mode))];
            if settings.get(Patch, "patchMode") == "contentAware" {
                options.extend([
                    section(extra(slider("Structure:", "structure", 1.0, 7.0))),
                    extra(slider("Color:", "colorAmount", 0.0, 10.0)),
                ]);
            } else {
                let sources = texts(&[("source", "Source"), ("destination", "Destination")]);
                let source = choices("", "patchSource", sources, Some(100.0), false, false, false);
                options.push(section(extra(source)));
            }
            options.extend([section(sample_image()), section(quality())]);
            options
        }
        // S§4.3: the brush controls (no blend mode: the heal is written Normal), then the two
        // Extra options.
        SpotHealingBrush => {
            let mut kind = choices(
                "Type:",
                "spotType",
                texts(&[
                    ("contentAware", "Content-Aware"),
                    ("createTexture", "Create Texture"),
                    ("proximityMatch", "Proximity Match"),
                ]),
                Some(121.0),
                false,
                false,
                false,
            );
            kind.extra = true;
            let mut options = brush_controls(false, false);
            options.extend([section(kind), section(sample_image())]);
            options
        }
        PaintBrush => brush_controls(true, true),
        Eraser => brush_controls(false, false),
        // S§4.3: Paint.NET's row unchanged, then the two Extra options (R18);
        // the Healing Brush reuses it.
        CloneStamp | HealingBrush => {
            let mut options = brush_controls(false, true);
            options.extend([
                section(extra_checkbox("Aligned", "aligned")),
                section(sample_image()),
            ]);
            options
        }
        Pencil => vec![blend(), quality()],
        PaintBucket => vec![
            flood_mode(),
            section(fill()),
            section(tolerance()),
            alpha_mode(),
            section(sample_scope()),
            section(antialias()),
            blend(),
            quality(),
            section(finish()),
        ],
        MagicWand => vec![
            combine(),
            section(flood_mode()),
            section(tolerance()),
            alpha_mode(),
            section(sample_scope()),
            section(quality()),
            section(finish()),
        ],
        ColorPicker => vec![
            sample_scope(),
            icon_choice(
                "",
                "sampleSize",
                &[
                    (
                        "1",
                        "Single Pixel",
                        "enum_pixel_sample_mode_point_sample.png",
                    ),
                    (
                        "3",
                        "3 x 3 Average",
                        "enum_pixel_sample_mode_average3x3.png",
                    ),
                    (
                        "5",
                        "5 x 5 Average",
                        "enum_pixel_sample_mode_average5x5.png",
                    ),
                    (
                        "11",
                        "11 x 11 Average",
                        "enum_pixel_sample_mode_average11x11.png",
                    ),
                    (
                        "31",
                        "31 x 31 Average",
                        "enum_pixel_sample_mode_average31x31.png",
                    ),
                    (
                        "51",
                        "51 x 51 Average",
                        "enum_pixel_sample_mode_average51x51.png",
                    ),
                ],
                101.0,
                true,
            ),
            section(icon_choice(
                "After click:",
                "afterClick",
                &[
                    (
                        "none",
                        "Do not switch tool",
                        "enum_color_picker_click_behavior_no_tool_switch.png",
                    ),
                    (
                        "previous",
                        "Switch to previous tool",
                        "enum_color_picker_click_behavior_switch_to_last_tool.png",
                    ),
                    (
                        "pencil",
                        "Switch to Pencil tool",
                        "enum_color_picker_click_behavior_switch_to_pencil_tool.png",
                    ),
                ],
                139.0,
                true,
            )),
            section(quality()),
        ],
        Gradient => vec![
            icon_group(
                "",
                "gradientType",
                &[
                    ("linear", "Linear", "enum_gradient_type_linear_clamped.png"),
                    (
                        "reflected",
                        "Linear (Reflected)",
                        "enum_gradient_type_linear_reflected.png",
                    ),
                    (
                        "diamond",
                        "Diamond",
                        "enum_gradient_type_linear_diamond.png",
                    ),
                    ("radial", "Radial", "enum_gradient_type_radial.png"),
                    ("conical", "Conical", "enum_gradient_type_conical.png"),
                    (
                        "spiral",
                        "Spiral (Clockwise)",
                        "enum_gradient_type_spiral.png",
                    ),
                    (
                        "spiralCounter",
                        "Spiral (Counter Clockwise)",
                        "enum_gradient_type_spiral_counter_clockwise.png",
                    ),
                ],
            ),
            section(icon_choice(
                "",
                "gradientMode",
                &[
                    ("color", "Color Mode", "gradient_is_alpha_only_false.png"),
                    (
                        "transparency",
                        "Transparency Mode",
                        "gradient_is_alpha_only_true.png",
                    ),
                ],
                35.0,
                false,
            )),
            icon_choice(
                "",
                "repeatMode",
                &[
                    (
                        "none",
                        "No Repeat",
                        "enum_gradient_repeat_type_no_repeat.png",
                    ),
                    (
                        "wrapped",
                        "Repeat Wrapped",
                        "enum_gradient_repeat_type_repeat_wrapped.png",
                    ),
                    (
                        "reflected",
                        "Repeat Reflected",
                        "enum_gradient_repeat_type_repeat_reflected.png",
                    ),
                ],
                97.0,
                true,
            ),
            section(antialias()),
            blend(),
            quality(),
            section(finish()),
        ],
        // Recolor: Pr after Size.
        Recolor => vec![
            brush_size(),
            pressure(),
            slider("Hardness:", "hardness", 0.0, 100.0),
            slider("Spacing:", "spacing", 1.0, 500.0),
            section(tolerance()),
            alpha_mode(),
            section(icon_group(
                "",
                "recolorSampling",
                &[
                    (
                        "once",
                        "Sampling: Once",
                        "enum_recolor_tool_sampling_mode_once.png",
                    ),
                    (
                        "secondary",
                        "Sampling: Secondary Color",
                        "enum_recolor_tool_sampling_mode_secondary_color.png",
                    ),
                ],
            )),
            section(smoothing()),
            antialias(),
            quality(),
        ],
        Line => vec![
            icon_group(
                "",
                "curveType",
                &[
                    ("straight", "Straight line", "enum_curve_type_straight.png"),
                    ("spline", "Spline", "enum_curve_type_spline.png"),
                    ("bezier", "Bézier", "enum_curve_type_bezier.png"),
                ],
            ),
            section(brush_size()),
            icon_choice(
                "Style:",
                "startCap",
                &[
                    ("flat", "Flat", "enum_line_curve_cap_flat_start.png"),
                    ("arrow", "Arrow", "enum_line_curve_cap_arrow_start.png"),
                    (
                        "filledArrow",
                        "Filled Arrow",
                        "enum_line_curve_cap_arrow_filled_start.png",
                    ),
                    ("round", "Rounded", "enum_line_curve_cap_rounded_start.png"),
                ],
                35.0,
                false,
            ),
            dash(""),
            icon_choice(
                "",
                "endCap",
                &[
                    ("flat", "Flat", "enum_line_curve_cap_flat_end.png"),
                    ("arrow", "Arrow", "enum_line_curve_cap_arrow_end.png"),
                    (
                        "filledArrow",
                        "Filled Arrow",
                        "enum_line_curve_cap_arrow_filled_end.png",
                    ),
                    ("round", "Rounded", "enum_line_curve_cap_rounded_end.png"),
                ],
                35.0,
                false,
            ),
            section(fill()),
            section(antialias()),
            blend(),
            quality(),
            section(finish()),
        ],
        Shapes => {
            let mut shape = OptionDef::new(K::ShapeChoice, "", "shape");
            shape.values = texts(&SHAPES);
            shape.width = Some(126.0);
            // The image + text dropdown (shape choice) autosizes.
            shape.fit_selected = true;
            shape.groups = vec![
                (
                    "Basic",
                    vec![
                        "rectangle",
                        "roundedRectangle",
                        "ellipse",
                        "diamond",
                        "trapezoid",
                        "parallelogram",
                        "triangle",
                        "rightTriangle",
                    ],
                ),
                (
                    "Polygons and Stars",
                    vec![
                        "pentagon", "hexagon", "heptagon", "octagon", "star3", "star4", "star5",
                        "star6",
                    ],
                ),
                (
                    "Arrows",
                    vec![
                        "blockArrow",
                        "notchedArrow",
                        "pentagonArrow",
                        "chevronArrow",
                    ],
                ),
                (
                    "Callouts",
                    vec![
                        "rectangularCallout",
                        "roundedCallout",
                        "ellipticalCallout",
                        "cloudCallout",
                    ],
                ),
                (
                    "Symbols",
                    vec!["lightningBolt", "checkMark", "multiply", "gear", "heart"],
                ),
            ];
            let mut options = vec![
                shape,
                icon_choice(
                    "",
                    "drawType",
                    &[
                        (
                            "outline",
                            "Draw Shape Outline",
                            "enum_shape_draw_type_outline_dark.png",
                        ),
                        (
                            "fill",
                            "Draw Filled Shape",
                            "enum_shape_draw_type_interior.png",
                        ),
                        (
                            "both",
                            "Draw Filled Shape with Outline",
                            "enum_shape_draw_type_both_dark.png",
                        ),
                    ],
                    35.0,
                    false,
                ),
                section(brush_size()),
            ];
            // Shapes: Radius is a − combo + before "Style:" (paintdotjs: a number field after
            // it).
            if settings.get(tool, "shape") == "roundedRectangle" {
                options.push(size("Radius:", "radius", 0.0, 2000.0, 1.0, &RADIUS_SIZES));
            }
            options.push(dash("Style:"));
            options.extend([
                section(fill()),
                section(antialias()),
                blend(),
                quality(),
                section(finish()),
            ]);
            options
        }
        Text => {
            let mut font = OptionDef::new(K::Select, "Font:", "fontFamily");
            font.values = fonts.iter().map(|&(v, t)| (v, t, None)).collect();
            font.width = Some(140.0);
            // paintdotjs's 125 % 380, divided by 1.25. Task 53 applies Paint.NET's 300.
            font.menu_width = Some(304.0);
            font.font_preview = true;
            let mut options = vec![
                font,
                size("", "fontSize", 1.0, 2000.0, 1.0, &FONT_SIZES),
                icon_choice(
                    "",
                    "fontUnit",
                    &[
                        (
                            "points",
                            "Points (image DPI)",
                            "enum_font_size_metric_points.png",
                        ),
                        ("pixels", "Pixels", "enum_font_size_metric_fixed96_dpi.png"),
                    ],
                    35.0,
                    false,
                ),
                section(font_style_toggle("B", "bold", "Bold")),
                font_style_toggle("I", "italic", "Italic"),
                font_style_toggle("U", "underline", "Underline"),
                font_style_toggle("abc", "strikeout", "Strikeout"),
                // Text: the rendering split (image and text), autosized.
                choices(
                    "",
                    "textRendering",
                    texts(&[
                        ("smooth", "Smooth"),
                        ("sharp", "Sharp"),
                        ("aliased", "Classic"),
                    ]),
                    Some(68.0),
                    true,
                    true,
                    true,
                ),
                section(icon_group(
                    "",
                    "align",
                    &[
                        ("left", "Align Left", "enum_text_alignment_left.png"),
                        ("center", "Center Align", "enum_text_alignment_center.png"),
                        ("right", "Align Right", "enum_text_alignment_right.png"),
                    ],
                )),
                section(antialias()),
                // S§4.5: a text layer takes its own blend mode.
                OptionDef {
                    disabled: crate::tool::text::editable_text(settings),
                    ..blend()
                },
                quality(),
                section(finish()),
                // R63: the Extra option after Paint.NET's row.
                section(extra_checkbox("Editable text", "editableText")),
            ];
            // Extras (`tool::text::shape`): the text's turn, its shape and that shape's bend.
            let extra = |mut d: OptionDef| {
                d.extra = true;
                d.width = d.width.or(Some(36.0));
                d
            };
            let shape = choices(
                "Shape:",
                "textShape",
                texts(&[
                    ("straight", "Straight"),
                    ("circle", "Circle"),
                    ("wave", "Wave"),
                    ("spiral", "Spiral"),
                ]),
                Some(72.0),
                false,
                false,
                false,
            );
            options.extend([
                section(extra(number("Angle:", "textAngle", -180.0, 180.0, 1.0))),
                extra(shape),
            ]);
            if settings.get(tool, "textShape") != "straight" {
                options.push(extra(number("Bend:", "textBend", -100.0, 100.0, 1.0)));
            }
            options
        }
    };
    if !settings.pen_seen {
        options.retain(|option| option.key != "pressure");
    }
    options
}

/// The options row while Layers ▸ Perspective Warp's canvas mode runs (an Extra,
/// `tool::perspective_warp`), in the tool options' own kinds: Mode, the Straighten buttons
/// (greyed out in Layout), Crop to plane (a checkbox as Editable text's), Commit and Cancel.
pub fn perspective_warp_options(warp: bool) -> Vec<OptionDef> {
    use OptionKind as K;
    let toggle = |label, key, values: &[(&'static str, &'static str, &'static str)]| {
        let mut d = OptionDef::new(K::IconChoice, label, key);
        d.values = icons(values);
        (d.width, d.show_text, d.split, d.fit_selected) = (Some(35.0), true, true, true);
        d
    };
    let action = |label, key, icon, title| {
        let mut d = OptionDef::new(K::Action, label, key);
        (d.icon, d.title) = (Some(icon), title);
        d
    };
    let mode = toggle(
        "Mode:",
        "mode",
        &[
            ("layout", "Layout", "menu_view_grid_icon.png"),
            ("warp", "Warp", "menu_layers_perspective_warp_icon.png"),
        ],
    );
    let icon = "straighten_effect.png";
    let mut straighten = [
        (
            "Horizontal",
            "straightenHorizontal",
            "Straighten: make near-horizontal edges horizontal",
        ),
        (
            "Vertical",
            "straightenVertical",
            "Straighten: make near-vertical edges vertical",
        ),
        (
            "Auto",
            "straightenAuto",
            "Straighten: make the plane an upright rectangle",
        ),
    ]
    .map(|(label, key, title)| {
        let mut d = action(label, key, icon, title);
        d.disabled = !warp;
        d
    });
    straighten[0].section = true;
    let mut crop = OptionDef::new(K::Checkbox, "Crop to plane", "cropToPlane");
    crop.section = true;
    let mut commit = action(
        "Commit",
        "commit",
        "tool_strip_checked.png",
        "Commit Perspective Warp (Enter)",
    );
    commit.section = true;
    let cancel = action("Cancel", "cancel", "cancel_icon.png", "Cancel (Esc)");
    let mut row = vec![mode];
    row.extend(straighten);
    row.extend([crop, commit, cancel]);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    /// paintdotjs's order, the six Extra tools at their Tools-window places (R§A.4; the user put
    /// the four retouch tools right after Clone Stamp and Recolor, 2026-10-24).
    #[test]
    fn values_order_matches_js() {
        let ids: Vec<_> = ToolType::VALUES.iter().map(|t| t.id()).collect();
        assert_eq!(
            ids,
            [
                "rectangleSelectTool",
                "moveTool",
                "lassoSelectTool",
                "moveSelectionTool",
                "magneticLassoTool",
                "objectSelectionTool",
                "ellipseSelectTool",
                "zoomTool",
                "magicWandTool",
                "panTool",
                "paintBucketTool",
                "gradientTool",
                "paintBrushTool",
                "eraserTool",
                "pencilTool",
                "colorPickerTool",
                "cloneStampTool",
                "recolorTool",
                "removeTool",
                "patchTool",
                "healingBrushTool",
                "spotHealingBrushTool",
                "textTool",
                "lineTool",
                "shapesTool"
            ]
        );
        // Paint.NET's list drops exactly the six, keeping paintdotjs's 19 in order.
        let extras: Vec<_> = ToolType::VALUES
            .into_iter()
            .filter(|t| t.is_extra())
            .collect();
        let paint_net: Vec<_> = ToolType::VALUES
            .into_iter()
            .filter(|t| !t.is_extra())
            .collect();
        assert_eq!((extras.len(), paint_net.len()), (6, 19));
        assert!(ToolType::shown(false).eq(paint_net));
        assert!(ToolType::shown(true).eq(ToolType::VALUES));
    }

    #[test]
    fn rect_select_shows_size_fields_only_when_not_normal() {
        let mut s = ToolSettings::default();
        let keys = |s: &ToolSettings| {
            option_definitions(ToolType::RectangleSelect, s)
                .iter()
                .map(|d| d.key)
                .collect::<Vec<_>>()
        };
        assert!(!keys(&s).contains(&"selectionWidth"));
        s.set(
            ToolType::RectangleSelect,
            "selectionMode",
            "fixedSize".into(),
        );
        assert!(keys(&s).contains(&"selectionWidth"));
    }

    #[test]
    fn brush_defaults_match_js() {
        let s = ToolSettings::default();
        assert_eq!(s.get(ToolType::PaintBrush, "width"), serde_json::json!(2));
        assert_eq!(
            s.get(ToolType::PaintBrush, "hardness"),
            serde_json::json!(75)
        );
        assert_eq!(
            s.get(ToolType::PaintBrush, "spacing"),
            serde_json::json!(15)
        );
        assert_eq!(
            s.get(ToolType::Text, "fontFamily"),
            serde_json::json!("Calibri")
        );
    }

    #[test]
    fn fill_styles_are_the_54_js_entries() {
        assert_eq!(fill_styles().len(), 54);
        assert_eq!(fill_styles()[7], ("percent05", "5%", None));
    }

    /// User (2026-10-24): no choice lists two values under one label ("Object Finder", "Object
    /// Finder"), in any tool's options or Perspective Warp's row; an on/off option is a checkbox
    /// with its plain name, not a list with an Off entry.
    #[test]
    fn no_choice_repeats_a_label() {
        let s = ToolSettings::default();
        let defs = ToolType::VALUES
            .iter()
            .flat_map(|&t| option_definitions(t, &s))
            .chain(perspective_warp_options(false))
            .chain(perspective_warp_options(true));
        for d in defs {
            let mut labels: Vec<_> = d
                .values
                .iter()
                .map(|v| v.1)
                .filter(|l| !l.is_empty())
                .collect();
            labels.sort();
            labels.dedup();
            let named = d.values.iter().filter(|v| !v.1.is_empty()).count();
            assert_eq!(labels.len(), named, "{}: {:?}", d.key, d.values);
            let off_entry = d.values.iter().any(|v| v.2 == Some("blank_icon.png"));
            assert!(!off_entry, "{}: {:?}", d.key, d.values);
            if d.kind == OptionKind::Checkbox {
                let name = d.label.unwrap_or_default();
                assert!(!name.is_empty() && !name.contains(':'), "{}", d.key);
            }
        }
    }

    /// R80: only the Paintbrush's Fill ends with None (painting-tools-1.md §2); the fill brush of
    /// the Bucket, Line and Shapes is Solid / Hatch (painting-tools-2.md §2.1).
    #[test]
    fn paintbrush_fill_adds_none() {
        let s = ToolSettings::default();
        let fill = |t| {
            option_definitions(t, &s)
                .into_iter()
                .find(|d| d.key == "fillStyle")
                .map(|d| (d.values.len(), d.values.last().map(|v| v.0)))
        };
        assert_eq!(fill(PaintBrush), Some((55, Some("none"))));
        for t in [PaintBucket, Line, Shapes] {
            assert_eq!(fill(t), Some((54, Some("solidDiamond"))), "{t:?}");
        }
    }

    #[test]
    fn rendering_quality_is_shared_and_migrated() {
        // An old per-tool value on Move Selection wins over the defaults.
        let saved = |k: &str| {
            (k == "paintdotjs.toolSettings.moveSelectionTool")
                .then(|| r#"{"renderingQuality":"aliased"}"#.to_string())
        };
        let mut s = ToolSettings::load(saved);
        assert_eq!(s.get(ToolType::Text, "renderingQuality"), "aliased");
        s.set(ToolType::Pan, "renderingQuality", "high".into());
        assert_eq!(s.per_tool["shapesTool"]["renderingQuality"], "high");
        let dirty = s.dirty_keys();
        assert_eq!(
            dirty,
            [("panTool".to_string(), "renderingQuality".to_string())]
        );
        assert_eq!(
            s.storage_entry("panTool", "renderingQuality"),
            (SELECTION_RENDERING_QUALITY_KEY.into(), "high".into())
        );
        assert!(s.dirty_keys().is_empty());
    }

    /// PC10: an Extra option at its default stays out of the stored JSON, so Clone Stamp stores
    /// what it stored before it had Aligned and Sample.
    #[test]
    fn extra_options_at_their_defaults_are_not_stored() {
        let mut s = ToolSettings::default();
        s.set(CloneStamp, "width", json!(5));
        let stored = |s: &ToolSettings| {
            let (_, v) = s.storage_entry("cloneStampTool", "width");
            serde_json::from_str::<Map<String, Value>>(&v).unwrap()
        };
        let keys: Vec<_> = stored(&s).keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "antialias",
                "blendMode",
                "hardness",
                "pressure",
                "renderingQuality",
                "smoothing",
                "spacing",
                "width"
            ]
        );
        s.set(CloneStamp, "aligned", json!(false));
        s.set(CloneStamp, "sampleImage", json!(true));
        let v = stored(&s);
        assert_eq!(
            (&v["aligned"], &v["sampleImage"]),
            (&json!(false), &json!(true))
        );
    }

    /// PC16 (S§4.5, R63): Editable text follows Paint.NET's Text row after Finish, with the plus,
    /// then the Extra turn and shape (`tool::text::shape`), Bend only for a curved shape; while
    /// Editable text is on the blend mode is greyed out.
    #[test]
    fn text_editable_option() {
        let mut s = ToolSettings::default();
        let keys = |s: &ToolSettings| -> Vec<_> {
            let defs = option_definitions(ToolType::Text, s);
            let at = defs
                .iter()
                .position(|d| d.kind == OptionKind::Action)
                .unwrap();
            assert_eq!(defs[at].key, "");
            defs[at + 1..].iter().map(|d| (d.key, d.extra)).collect()
        };
        assert_eq!(
            keys(&s),
            [
                ("editableText", true),
                ("textAngle", true),
                ("textShape", true)
            ]
        );
        s.set(ToolType::Text, "textShape", json!("circle"));
        assert_eq!(keys(&s).last(), Some(&("textBend", true)));
        s.set(ToolType::Text, "textShape", json!("straight"));
        // On by default (user, 2026-10-24); Paint.NET's raster text is one toggle away.
        assert_eq!(s.get(ToolType::Text, "editableText"), json!(true));
        s.set(ToolType::Text, "editableText", json!(false));
        let blend_disabled = |s: &ToolSettings, tool| {
            let defs = option_definitions(tool, s);
            defs.iter().find(|d| d.key == "blendMode").unwrap().disabled
        };
        assert!(!blend_disabled(&s, ToolType::Text));
        s.set(ToolType::Text, "editableText", json!(true));
        assert!(blend_disabled(&s, ToolType::Text));
        assert!(!blend_disabled(&s, ToolType::PaintBrush));
    }

    #[test]
    fn legacy_line_caps_and_move_resampling_migrate() {
        let saved = |k: &str| match k {
            "paintdotjs.toolSettings.lineTool" => Some(r#"{"startCap":"round"}"#.to_string()),
            "paintdotjs.toolSettings.moveTool" => Some(r#"{"resampling":"bicubic"}"#.to_string()),
            _ => None,
        };
        let mut s = ToolSettings::load(saved);
        assert_eq!(s.get(ToolType::Line, "startCap"), "flat");
        assert_eq!(s.get(ToolType::Move, "resampling"), "highQualityCubic");
        assert_eq!(
            s.dirty_keys(),
            [("lineTool".to_string(), "startCap".to_string())]
        );
    }

    #[test]
    fn content_blend_modes_follow_the_toolbar_and_unknown_saved_modes_load_as_normal() {
        let ids: Vec<_> = ContentBlendMode::TOOLBAR
            .iter()
            .map(|m| m.value())
            .collect();
        let values: Vec<_> = CONTENT_BLEND_MODES.iter().map(|e| e.0).collect();
        assert_eq!(values, ids);
        let i18n = crate::i18n::I18n::new(include_str!("../../../../assets/lang/en.json"));
        for (m, (_, label)) in ContentBlendMode::TOOLBAR.iter().zip(CONTENT_BLEND_MODES) {
            assert_eq!(i18n.t(&m.label_key(), &[]), label);
        }
        let def = option_definitions(ToolType::PaintBrush, &ToolSettings::default())
            .into_iter()
            .find(|d| d.key == "blendMode")
            .unwrap();
        let listed: Vec<_> = def.values.iter().map(|e| e.0).collect();
        assert_eq!(listed, ids);
        let saved = |k: &str| {
            (k == "paintdotjs.toolSettings.paintBrushTool")
                .then(|| r#"{"blendMode":"softLight"}"#.to_string())
        };
        let mut s = ToolSettings::load(saved);
        assert_eq!(s.get(ToolType::PaintBrush, "blendMode"), "normal");
        assert_eq!(
            s.dirty_keys(),
            [("paintBrushTool".to_string(), "blendMode".to_string())]
        );
        let saved = |k: &str| {
            (k == "paintdotjs.toolSettings.paintBrushTool")
                .then(|| r#"{"blendMode":"overwrite"}"#.to_string())
        };
        assert_eq!(
            ToolSettings::load(saved).get(ToolType::PaintBrush, "blendMode"),
            "overwrite"
        );
    }

    #[test]
    fn shapes_radius_only_for_rounded_rectangle() {
        let mut s = ToolSettings::default();
        let has_radius = |s: &ToolSettings| {
            option_definitions(ToolType::Shapes, s)
                .iter()
                .any(|d| d.key == "radius")
        };
        assert!(!has_radius(&s));
        s.set(ToolType::Shapes, "shape", "roundedRectangle".into());
        assert!(has_radius(&s));
    }

    #[test]
    fn json_values_follow_js_literals() {
        let d = &option_definitions(ToolType::ColorPicker, &ToolSettings::default())[1];
        assert_eq!(d.index_of(&serde_json::json!(11)), Some(3));
        assert_eq!(json_value("true"), serde_json::json!(true));
        assert_eq!(json_value("layer"), serde_json::json!("layer"));
    }

    use serde_json::json;

    #[test]
    fn shared_settings_follow_every_owner() {
        let mut s = ToolSettings::default();
        s.dirty_keys();
        s.set(ToolType::PaintBrush, "width", json!(7));
        for t in [
            ToolType::Eraser,
            ToolType::CloneStamp,
            ToolType::Recolor,
            ToolType::Line,
            ToolType::Shapes,
        ] {
            assert_eq!(s.get(t, "width"), json!(7), "{t:?}");
        }
        assert_eq!(s.get(ToolType::Pencil, "width"), serde_json::Value::Null);
        let dirty = s.dirty_keys();
        assert!(
            dirty.contains(&("shapesTool".into(), "width".into())),
            "{dirty:?}"
        );
        s.set(ToolType::MagicWand, "tolerance", json!(30));
        assert_eq!(
            (
                s.get(ToolType::PaintBucket, "tolerance"),
                s.get(ToolType::Recolor, "tolerance")
            ),
            (json!(30), json!(30))
        );
        s.set(ToolType::PaintBucket, "blendMode", json!("screen"));
        assert_eq!(s.get(ToolType::Text, "blendMode"), json!("screen"));
        s.set(ToolType::ColorPicker, "sampleMode", json!("image"));
        assert_eq!(s.get(ToolType::MagicWand, "sampleMode"), json!("image"));
        s.set(ToolType::LassoSelect, "combineMode", json!("union"));
        assert_eq!(s.get(ToolType::MagicWand, "combineMode"), json!("union"));
        // Not shared: the fill style stays per tool.
        s.set(ToolType::PaintBrush, "fillStyle", json!("backwardDiagonal"));
        assert_eq!(s.get(ToolType::Shapes, "fillStyle"), json!("solid"));
    }

    /// Ruling R20: the Magnetic Lasso reads and writes the selection tools' combine mode without
    /// owning it, so Lasso's Union shows on its toolbar.
    #[test]
    fn magnetic_lasso_shares_the_selection_combine_mode() {
        let mut s = ToolSettings::default();
        s.set(ToolType::LassoSelect, "combineMode", json!("union"));
        assert_eq!(
            s.get(ToolType::MagneticLasso, "combineMode"),
            json!("union")
        );
        s.set(ToolType::MagneticLasso, "combineMode", json!("xor"));
        for t in [ToolType::RectangleSelect, ToolType::MagicWand] {
            assert_eq!(s.get(t, "combineMode"), json!("xor"), "{t:?}");
        }
        assert!(!ToolSettings::shared_owners("combineMode").contains(&ToolType::MagneticLasso));
        // Its width is its own.
        s.set(ToolType::PaintBrush, "width", json!(30));
        assert_eq!(s.get(ToolType::MagneticLasso, "width"), json!(10));
    }

    /// S§4.3: the combine modes and the quality are Paint.NET's (no plus); Width, Contrast and
    /// Frequency are the Magnetic Lasso's own.
    #[test]
    fn magnetic_lasso_options() {
        let defs = option_definitions(ToolType::MagneticLasso, &ToolSettings::default());
        let got: Vec<_> = defs
            .iter()
            .map(|d| (d.key, d.label, d.min, d.max, d.extra))
            .collect();
        assert_eq!(
            got,
            [
                ("combineMode", None, None, None, false),
                ("width", Some("Width:"), Some(1.0), Some(256.0), true),
                ("contrast", Some("Contrast:"), Some(1.0), Some(100.0), true),
                (
                    "frequency",
                    Some("Frequency:"),
                    Some(0.0),
                    Some(100.0),
                    true
                ),
                ("renderingQuality", None, None, None, false),
            ]
        );
        assert_eq!(defs[1].kind, OptionKind::Size);
        assert_eq!(defs[2].kind, OptionKind::Slider);
    }

    /// Ruling R20: Object Selection reads and writes the selection tools' combine mode without
    /// owning it.
    #[test]
    fn object_selection_shares_the_selection_combine_mode() {
        let mut s = ToolSettings::default();
        s.set(ToolType::MagicWand, "combineMode", json!("intersect"));
        assert_eq!(
            s.get(ToolType::ObjectSelection, "combineMode"),
            json!("intersect")
        );
        s.set(ToolType::ObjectSelection, "combineMode", json!("exclude"));
        for t in [ToolType::LassoSelect, ToolType::MagneticLasso] {
            assert_eq!(s.get(t, "combineMode"), json!("exclude"), "{t:?}");
        }
        assert!(!ToolSettings::shared_owners("combineMode").contains(&ToolType::ObjectSelection));
    }

    /// S§4.3: the combine modes and the quality are Paint.NET's (no plus); the method (MobileSAM
    /// or GrabCut), Sample, Object Finder and Select Subject are Object Selection's own.
    #[test]
    fn object_selection_options() {
        let defs = option_definitions(ToolType::ObjectSelection, &ToolSettings::default());
        let got: Vec<_> = defs.iter().map(|d| (d.key, d.kind, d.extra)).collect();
        assert_eq!(
            got,
            [
                ("combineMode", OptionKind::IconGroup, false),
                ("segmenter", OptionKind::IconChoice, true),
                ("sampleImage", OptionKind::Select, true),
                ("objectFinder", OptionKind::Checkbox, true),
                ("selectSubject", OptionKind::Action, true),
                ("renderingQuality", OptionKind::IconChoice, false),
            ]
        );
        assert_eq!(defs[4].label, Some("Select Subject"));
        let s = ToolSettings::default();
        assert_eq!(
            s.get(ToolType::ObjectSelection, "objectFinder"),
            json!(true)
        );
        assert_eq!(
            s.get(ToolType::ObjectSelection, "sampleImage"),
            json!(false)
        );
    }

    #[test]
    fn paint_net_defaults() {
        let s = ToolSettings::default();
        assert_eq!(s.get(ToolType::Recolor, "tolerance"), json!(50));
        assert_eq!(
            s.get(ToolType::RectangleSelect, "selectionWidth"),
            json!(400)
        );
        assert_eq!(
            s.get(ToolType::RectangleSelect, "selectionHeight"),
            json!(300)
        );
        assert_eq!(s.get(ToolType::PaintBrush, "width"), json!(2));
        let s = ToolSettings::load_scaled(|_| None, 1.5);
        assert_eq!(
            (
                s.get(ToolType::PaintBrush, "width"),
                s.get(ToolType::Shapes, "radius"),
                s.get(ToolType::Text, "fontSize")
            ),
            (json!(3), json!(15), json!(18))
        );
        let s = ToolSettings::load_scaled(|_| None, 1.25);
        assert_eq!(
            (
                s.get(ToolType::Line, "width"),
                s.get(ToolType::Shapes, "radius"),
                s.get(ToolType::Text, "fontSize")
            ),
            (json!(2), json!(12), json!(15))
        );
    }

    /// R17 migration (rule B): a saved value counts only if it differs from that owner's
    /// paintdotjs default; the first counting owner (precedence order) wins, else the Paint.NET
    /// default. The result is written to every owner and persisted.
    #[test]
    fn per_tool_values_migrate_to_one_shared_value() {
        let saved = |k: &str| match k {
            "paintdotjs.toolSettings.eraserTool" => Some(r#"{"width":9}"#.to_string()),
            "paintdotjs.toolSettings.paintBrushTool" => Some(r#"{"hardness":40}"#.to_string()),
            "paintdotjs.toolSettings.recolorTool" => Some(r#"{"tolerance":25}"#.to_string()),
            _ => None,
        };
        let mut s = ToolSettings::load(saved);
        assert_eq!(
            (
                s.get(ToolType::PaintBrush, "width"),
                s.get(ToolType::Shapes, "width")
            ),
            (json!(9), json!(9))
        );
        assert_eq!(s.get(ToolType::Eraser, "hardness"), json!(40));
        // Recolor's 25 is paintdotjs's default, indistinguishable from untouched: Paint.NET's 50.
        assert_eq!(
            (
                s.get(ToolType::MagicWand, "tolerance"),
                s.get(ToolType::Recolor, "tolerance")
            ),
            (json!(50), json!(50))
        );
        let dirty = s.dirty_keys();
        assert!(dirty.contains(&("shapesTool".into(), "width".into())));
        assert!(dirty.contains(&("recolorTool".into(), "tolerance".into())));
        assert!(!dirty.contains(&("magicWandTool".into(), "tolerance".into())));
        // A non-default wand value wins over Recolor's default.
        let saved2 = |k: &str| match k {
            "paintdotjs.toolSettings.magicWandTool" => Some(r#"{"tolerance":70}"#.to_string()),
            "paintdotjs.toolSettings.recolorTool" => Some(r#"{"tolerance":25}"#.to_string()),
            _ => None,
        };
        assert_eq!(
            ToolSettings::load(saved2).get(ToolType::Recolor, "tolerance"),
            json!(70)
        );
    }

    /// Rule B: when several owners hold different non-default values, precedence decides.
    #[test]
    fn precedence_picks_among_non_default_values() {
        let saved = |k: &str| match k {
            "paintdotjs.toolSettings.recolorTool" => {
                Some(r#"{"tolerance":10,"width":40}"#.to_string())
            }
            "paintdotjs.toolSettings.paintBucketTool" => Some(r#"{"tolerance":30}"#.to_string()),
            "paintdotjs.toolSettings.lineTool" => Some(r#"{"width":5}"#.to_string()),
            _ => None,
        };
        let s = ToolSettings::load(saved);
        // tolerance: MagicWand, PaintBucket, Recolor; width: … Recolor, Line, Shapes.
        assert_eq!(s.get(ToolType::MagicWand, "tolerance"), json!(30));
        assert_eq!(s.get(ToolType::Recolor, "tolerance"), json!(30));
        assert_eq!(s.get(ToolType::Line, "width"), json!(40));
        assert_eq!(s.get(ToolType::PaintBrush, "width"), json!(40));
    }

    /// Rule B on paintdotjs's whole-object saves (P1 open risk 10): untouched defaults in a saved
    /// JSON do not win the migration.
    #[test]
    fn whole_object_saves_do_not_spread_untouched_defaults() {
        // The user changed only Recolor's sampling; its JSON still holds paintdotjs's 25 default.
        let recolor = r#"{"tolerance":25,"alphaMode":"premultiplied","recolorSampling":"secondaryColor",
            "width":2,"pressure":true,"hardness":75,"spacing":15,"smoothing":true,"antialias":true,
            "renderingQuality":"high"}"#;
        // The user picked Screen on Shapes, then only changed the Paintbrush fill.
        let shapes = r#"{"shape":"rectangle","blendMode":"screen"}"#;
        let brush = r#"{"fillStyle":"backwardDiagonal","blendMode":"normal","width":2}"#;
        let saved = |k: &str| match k {
            "paintdotjs.toolSettings.recolorTool" => Some(recolor.to_string()),
            "paintdotjs.toolSettings.shapesTool" => Some(shapes.to_string()),
            "paintdotjs.toolSettings.paintBrushTool" => Some(brush.to_string()),
            _ => None,
        };
        let s = ToolSettings::load(saved);
        for t in [
            ToolType::MagicWand,
            ToolType::PaintBucket,
            ToolType::Recolor,
        ] {
            assert_eq!(s.get(t, "tolerance"), json!(50), "{t:?}");
        }
        assert_eq!(s.get(ToolType::Shapes, "blendMode"), json!("screen"));
        assert_eq!(s.get(ToolType::PaintBrush, "blendMode"), json!("screen"));
        // The untouched paintdotjs width 2 yields to Paint.NET's trunc(2·1.5).
        let s = ToolSettings::load_scaled(saved, 1.5);
        assert_eq!(s.get(ToolType::Eraser, "width"), json!(3));
    }

    /// Second ruling: rule B runs once; with the marker set, saved values load verbatim, so an
    /// explicit width 2 at scale 1.5 survives restarts.
    #[test]
    fn explicit_width_survives_restarts_after_the_migration() {
        let mut store = BTreeMap::<String, String>::new();
        let load = |store: &BTreeMap<String, String>| {
            ToolSettings::load_scaled(|k| store.get(k).cloned(), 1.5)
        };
        let mut s = load(&store);
        assert_eq!(s.get(ToolType::PaintBrush, "width"), json!(3));
        // What the app writes after the first load.
        store.insert(SHARED_MIGRATION_KEY.into(), "1".into());
        s.set(ToolType::PaintBrush, "width", json!(2));
        for (tool, key) in s.dirty_keys() {
            let (k, v) = s.storage_entry(&tool, &key);
            store.insert(k, v);
        }
        for _ in 0..2 {
            let mut s = load(&store);
            for &t in ToolSettings::shared_owners("width") {
                assert_eq!(s.get(t, "width"), json!(2), "{t:?}");
            }
            assert!(s.dirty_keys().is_empty());
        }
        // Without the marker, rule B reads the 2 as paintdotjs's untouched default.
        store.remove(SHARED_MIGRATION_KEY);
        assert_eq!(load(&store).get(ToolType::Eraser, "width"), json!(3));
    }

    /// A store from before the migration (no marker) still migrates by rule B; the same store
    /// with the marker loads verbatim.
    #[test]
    fn pre_migration_store_migrates_by_rule_b() {
        let saved = |marker: bool| {
            move |k: &str| match k {
                "paintdotjs.toolSettings.recolorTool" => Some(r#"{"tolerance":25}"#.to_string()),
                SHARED_MIGRATION_KEY if marker => Some("1".to_string()),
                _ => None,
            }
        };
        let s = ToolSettings::load(saved(false));
        assert_eq!(s.get(ToolType::MagicWand, "tolerance"), json!(50));
        assert_eq!(s.get(ToolType::Recolor, "tolerance"), json!(50));
        let s = ToolSettings::load(saved(true));
        assert_eq!(s.get(ToolType::MagicWand, "tolerance"), json!(25));
    }

    /// Review fix: invalid stored values (wrong JSON type, unlisted choice) load as if absent and
    /// dirty only their own tool, before and after the migration marker.
    #[test]
    fn invalid_stored_values_load_as_paint_net_defaults() {
        for marker in [false, true] {
            let saved = move |k: &str| match k {
                "paintdotjs.toolSettings.eraserTool" => Some(r#"{"width":"abc"}"#.to_string()),
                "paintdotjs.toolSettings.magicWandTool" => Some(r#"{"combineMode":7}"#.to_string()),
                "paintdotjs.toolSettings.paintBucketTool" => {
                    Some(r#"{"floodMode":"sideways"}"#.to_string())
                }
                SHARED_MIGRATION_KEY if marker => Some(SHARED_MIGRATION_VERSION.to_string()),
                _ => None,
            };
            let mut s = ToolSettings::load(saved);
            assert_eq!(s.get(ToolType::Line, "width"), json!(2), "{marker}");
            assert_eq!(s.get(ToolType::Eraser, "width"), json!(2), "{marker}");
            assert_eq!(s.get(ToolType::MagicWand, "combineMode"), json!("replace"));
            assert_eq!(
                s.get(ToolType::PaintBucket, "floodMode"),
                json!("contiguous")
            );
            let dirty: Vec<_> = s
                .dirty_keys()
                .into_iter()
                .map(|(t, k)| format!("{t}.{k}"))
                .collect();
            assert_eq!(
                dirty,
                [
                    "eraserTool.width",
                    "paintBucketTool.floodMode",
                    "magicWandTool.combineMode"
                ],
                "{marker}"
            );
        }
    }

    /// The Extra tools' same-named keys (a brush `width`, Magnetic Lasso's `width`) are their own:
    /// sharing is Paint.NET's (R17).
    #[test]
    fn only_owners_have_shared_keys() {
        for (key, owners) in SHARED_KEYS {
            for t in ToolType::shown(false).filter(|t| !owners.contains(t)) {
                assert!(!t.default_settings().contains_key(*key), "{t:?} has {key}");
            }
        }
    }

    #[test]
    fn shared_keys_have_owners_that_show_them() {
        for (key, owners) in SHARED_KEYS {
            for t in *owners {
                assert!(t.default_settings().contains_key(*key), "{t:?} lacks {key}");
            }
        }
    }
}
