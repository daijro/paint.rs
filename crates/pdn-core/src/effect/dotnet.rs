//! Real Paint.NET plugin effects, loaded from DLLs through the embedded .NET host. On the
//! desktop that host is the `pdn-bridge` crate (CoreCLR via hostfxr, `pdn-host/docs/pdn-bridge.md`);
//! on the web it is the Mono browser host (`pdn-host/paintnet-web`), driven through a small JS
//! bridge over the SAME JSON protocol. Both register each loaded effect as a leaked-`'static`
//! [`EffectDef`] in Effects ▸ Plugins and render it through the host, reusing the same
//! session/dialog/undo pipeline as every built-in effect.

use super::{EffectDef, EffectEnv, Values};
use crate::canvas::ImageData;
use crate::effect::{ColorDefault, Control, CurveChannels, range};
use serde_json::Value;

/// The Effects submenu key installed plugins land under.
pub const CATEGORY: &str = "plugins";
pub const ICON: &str = "blank_icon.svg";

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// The menu actions of the registered effects (`menu.effects.<id>`, named for the effect) and of the submenus they
/// declare (`SubMenuName`, `effects.plugins.<name>.submenu`), which group them under Effects ▸ Plugins as Paint.NET
/// groups them under Effects. Made as effects register and kept for the process, as menus hold their ids.
#[derive(Default)]
struct Menu {
    actions: std::collections::HashMap<String, &'static crate::action::ActionDef>,
    submenus: std::collections::HashMap<&'static str, &'static str>,
}

static MENU: std::sync::LazyLock<std::sync::RwLock<Menu>> =
    std::sync::LazyLock::new(Default::default);

fn menu_action(
    id: String,
    label: &'static str,
    category: &'static str,
    icon: Option<&'static str>,
) -> &'static crate::action::ActionDef {
    Box::leak(Box::new(crate::action::ActionDef {
        id: leak(&id),
        name_key: None,
        label: Some(label),
        tooltip_key: None,
        default_shortcut: None,
        icon,
        category,
    }))
}

/// Gives registered effect `id`, named `name`, its menu action, and the submenu it declares its own: one per name,
/// whatever its case, as Paint.NET merges them. With none it sits directly under Effects ▸ Plugins.
pub fn remember_menu(id: &'static str, name: &'static str, submenu: Option<&str>) {
    let mut menu = MENU.write().unwrap_or_else(|e| e.into_inner());
    let action = format!("menu.effects.{id}");
    menu.actions
        .entry(action.clone())
        .or_insert_with(|| menu_action(action, name, "Effects", Some(ICON)));
    match submenu.map(str::trim).filter(|s| !s.is_empty()) {
        Some(sub) => {
            let key = format!("effects.plugins.{}.submenu", sub.to_lowercase());
            let action = *menu
                .actions
                .entry(key.clone())
                .or_insert_with(|| menu_action(key, leak(sub), "General", None));
            menu.submenus.insert(id, action.id);
        }
        None => {
            menu.submenus.remove(id);
        }
    }
}

/// A registered effect's or declared submenu's menu action.
pub fn action(action_id: &str) -> Option<&'static crate::action::ActionDef> {
    MENU.read()
        .unwrap_or_else(|e| e.into_inner())
        .actions
        .get(action_id)
        .copied()
}

/// The submenu action (`effects.plugins.<name>.submenu`) registered effect `id` declares, if any.
pub fn submenu_of(id: &str) -> Option<&'static str> {
    MENU.read()
        .unwrap_or_else(|e| e.into_inner())
        .submenus
        .get(id)
        .copied()
}

/// The registry key for an effect: its .NET type name, unless another plug-in already registered that name
/// (MadJik's Splashes and Tiles both define `PaintDotNet.Effects.TileEffect`), then `<type>@<tag>` (the plug-in's
/// file name), so neither replaces the other.
fn unique_key(
    id: &'static str,
    tag: &str,
    taken_by_another: impl Fn(&str) -> bool,
) -> &'static str {
    if !taken_by_another(id) {
        return id;
    }
    let mut key = format!("{id}@{tag}");
    let mut n = 2;
    while taken_by_another(&key) {
        key = format!("{id}@{tag}{n}");
        n += 1;
    }
    leak(&key)
}

/// The property values as the ABI's props JSON: choice keys become their numeric index.
pub(crate) fn build_props(values: &Values, choice_keys: &[&'static str]) -> Value {
    let mut obj = serde_json::Map::new();
    for (k, v) in values {
        let v = if choice_keys.contains(&k.as_str()) {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .map_or_else(|| v.clone(), Value::from)
        } else {
            v.clone()
        };
        obj.insert(k.clone(), v);
    }
    Value::Object(obj)
}

/// The props JSON of a render: the effect's values, and under "$environment" what Paint.NET
/// tells an effect about the app (the primary and secondary colours, the palette), each colour a
/// ColorBgra's packed BGRA word, and the selection's outline when part of the image is selected
/// (even-odd polygons in image pixels, each `[x0, y0, x1, y1, …]`).
///
/// A plug-in's own settings window (its settings keyed `w:`) can set defaults from the image when it
/// loads, such as a corner at the image's edge where the schema, read at 1×1, saw 0. Only the settings the
/// user changed are sent, so the window keeps its own defaults; once one of its buttons was pressed, the window
/// starts from the token that made (`"$token"`, which `press` kept), so all its settings are sent.
fn render_props(
    values: &Values,
    choice_keys: &[&'static str],
    controls: &[Control],
    env: &EffectEnv,
) -> String {
    let bgra = |p: &crate::pixel::Px| u32::from_le_bytes([p[2], p[1], p[0], p[3]]);
    let mut props = build_props(values, choice_keys);
    let kept = values
        .get(TOKEN)
        .and_then(Value::as_str)
        .is_some_and(|t| !t.is_empty());
    if let Some(obj) = props.as_object_mut() {
        for c in controls.iter().filter(|c| c.key().starts_with("w:")) {
            if matches!(c, Control::Button { .. })
                || (!kept && values.get(c.key()) == Some(&c.default_value()))
            {
                obj.remove(c.key());
            }
        }
        obj.retain(|k, _| !k.ends_with("#items"));
    }
    props["$environment"] = serde_json::json!({
        "primary": bgra(&env.primary),
        "secondary": bgra(&env.secondary),
        "palette": env.palette.iter().map(bgra).collect::<Vec<u32>>(),
    });
    if env.mask.is_some() && !env.selection.is_empty() {
        props["$environment"]["selection"] = env
            .selection
            .iter()
            .map(|poly| poly.iter().flatten().copied().collect::<Vec<f64>>())
            .collect();
    }
    props.to_string()
}

/// Where an effect's values keep the token a press of one of its window's buttons made (`press`).
const TOKEN: &str = "$token";

/// After a press of one of a plug-in window's buttons: keep the token it made and take the window's settings as
/// it now shows them (`{"token": ..., "properties": [...]}`, the schema's form, each value as its default), in the
/// value forms of `controls`.
fn take_press(result: &str, controls: &[Control], values: &mut Values) -> Result<(), String> {
    let parsed: Value = serde_json::from_str(result).map_err(|e| e.to_string())?;
    match parsed.get("token").and_then(Value::as_str) {
        Some(token) => values.insert(TOKEN.into(), Value::from(token)),
        None => values.remove(TOKEN),
    };
    for p in parsed
        .get("properties")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = p.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(control) = controls.iter().find(|c| c.key() == name) else {
            continue;
        };
        let shown = p.get("default").cloned().unwrap_or(Value::Null);
        let value = match control {
            Control::Range { .. } => Value::from(shown.as_f64().unwrap_or(0.0)),
            Control::Checkbox { .. } => shown,
            Control::Select { .. } => Value::from(shown.as_i64().unwrap_or(0).to_string()),
            Control::Color { .. } => {
                Value::from(format!("#{:06x}", shown.as_u64().unwrap_or(0) & 0xff_ffff))
            }
            Control::Text { .. } | Control::PluginCurve { .. } | Control::PluginGrid { .. } => {
                shown
            }
            Control::List { .. } => {
                let items = p
                    .get("items")
                    .cloned()
                    .unwrap_or_else(|| Value::Array(vec![]));
                values.insert(super::list_items_key(name), items);
                shown
            }
            _ => continue,
        };
        values.insert(name.into(), value);
    }
    Ok(())
}

/// Turn the effect-schema JSON into paint.rs controls (and the names of choice props).
fn controls_from_schema(schema: &str) -> (Vec<Control>, Vec<&'static str>) {
    serde_json::from_str::<Value>(schema)
        .map(|parsed| controls_from_properties(&parsed))
        .unwrap_or_default()
}

/// The controls of a schema's (an effect's or a FileType's save options) `properties` array, and
/// the names of its choice properties.
pub(crate) fn controls_from_properties(parsed: &Value) -> (Vec<Control>, Vec<&'static str>) {
    let mut controls = Vec::new();
    let mut choice_keys = Vec::new();
    for p in parsed
        .get("properties")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = p.get("name").and_then(Value::as_str) else {
            continue;
        };
        let key = leak(name);
        let ty = p.get("type").and_then(Value::as_str).unwrap_or("");
        // The effect's IndirectUI DisplayName, else its Description (a checkbox's caption, which
        // comes first for a checkbox), else the property name.
        let text = |k: &str| p.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let (first, second) = if ty == "bool" {
            ("description", "label")
        } else {
            ("label", "description")
        };
        let label = text(first).or_else(|| text(second)).map_or(key, leak);
        let num = |k: &str, d: f64| p.get(k).and_then(Value::as_f64).unwrap_or(d);
        match ty {
            "int" => controls.push(range(
                key,
                label,
                num("min", 0.0),
                num("max", 100.0),
                num("default", 0.0),
            )),
            "double" => controls.push(
                range(
                    key,
                    label,
                    num("min", 0.0),
                    num("max", 100.0),
                    num("default", 0.0),
                )
                .step(num("step", 0.01))
                .slider_keys(2, 0.01, 0.1, false),
            ),
            "bool" => controls.push(Control::Checkbox {
                key,
                label,
                default: p.get("default").and_then(Value::as_bool).unwrap_or(false),
            }),
            "choice" => {
                let pairs: Vec<(&'static str, &'static str)> = p
                    .get("choices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let v = c.get("value").and_then(Value::as_i64).unwrap_or(0);
                        let l = c.get("label").and_then(Value::as_str).unwrap_or("");
                        (leak(&v.to_string()), leak(l))
                    })
                    .collect();
                let default = leak(
                    &p.get("default")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .to_string(),
                );
                choice_keys.push(key);
                controls.push(Control::Select {
                    key,
                    label,
                    default,
                    choices: Box::leak(pairs.into_boxed_slice()),
                });
            }
            "color" => controls.push(Control::Color {
                key,
                label,
                default: ColorDefault::Rgb(num("default", 0.0) as u32),
            }),
            "string" => controls.push(Control::Text {
                key,
                label,
                default: leak(p.get("default").and_then(Value::as_str).unwrap_or("")),
                // As a Windows text box's, a length of 0 is no limit.
                max_len: p
                    .get("maxLength")
                    .and_then(Value::as_u64)
                    .filter(|&n| n > 0)
                    .map_or(u32::MAX, |n| n.min(u32::MAX as u64) as u32),
                multiline: false,
            }),
            "button" => controls.push(Control::Button { key, label }),
            "list" => {
                let items: Vec<&'static str> = p
                    .get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|i| leak(i.as_str().unwrap_or("")))
                    .collect();
                controls.push(Control::List {
                    key,
                    label,
                    items: Box::leak(items.into_boxed_slice()),
                    default: p.get("default").and_then(Value::as_i64).unwrap_or(-1),
                });
            }
            "grid" => controls.push(Control::PluginGrid {
                key,
                label,
                columns: p.get("columns").and_then(Value::as_u64).unwrap_or(1) as u32,
                rows: p.get("rows").and_then(Value::as_u64).unwrap_or(1) as u32,
                columns_by: p.get("columnsBy").and_then(Value::as_str).map(leak),
                rows_by: p.get("rowsBy").and_then(Value::as_str).map(leak),
            }),
            "curve" => {
                let sets: Vec<CurveChannels> = p
                    .get("sets")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(curve_channels_of)
                    .collect();
                controls.push(Control::PluginCurve {
                    key,
                    label,
                    max: p.get("max").and_then(Value::as_i64).unwrap_or(255),
                    own: curve_channels_of(p),
                    by: p.get("by").and_then(Value::as_str).map(leak),
                    sets: Box::leak(sets.into_boxed_slice()),
                });
            }
            _ => {}
        }
    }
    (controls, choice_keys)
}

/// A schema curve's (or one of its sets') `channels`, `colors` (`"#rrggbb"`, empty: none) and `default` points.
fn curve_channels_of(p: &Value) -> CurveChannels {
    let strings = |k: &str| p.get(k).and_then(Value::as_array).into_iter().flatten();
    let names: Vec<&'static str> = strings("channels")
        .map(|n| leak(n.as_str().unwrap_or("")))
        .collect();
    let colors: Vec<Option<[u8; 3]>> = strings("colors")
        .map(|c| {
            let hex = c.as_str()?.strip_prefix('#')?;
            let v = u32::from_str_radix(hex, 16).ok()?;
            Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
        })
        .collect();
    CurveChannels {
        names: Box::leak(names.into_boxed_slice()),
        colors: Box::leak(colors.into_boxed_slice()),
        default: Box::leak(Box::new(
            p.get("default").cloned().unwrap_or(Value::Array(vec![])),
        )),
    }
}

/// Build a plugin effect's leaked id and display name, its [`EffectDef`] and its choice-property
/// keys from one `enumerate` entry and a `fetch_schema` that returns the effect's schema JSON (the
/// only per-transport step). Returns `None` for an unsupported or id-less entry; the caller inserts
/// the result into its own name and effect registries.
#[allow(clippy::type_complexity)]
fn register(
    eff: &Value,
    fetch_schema: impl FnOnce(&str) -> Result<String, String>,
) -> Result<Option<(&'static str, &'static str, EffectDef, Vec<&'static str>)>, String> {
    if eff.get("supported").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    let Some(id) = eff
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let name = leak(eff.get("name").and_then(Value::as_str).unwrap_or(id));
    let configurable = eff
        .get("configurable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (controls, choice_keys) = if configurable {
        controls_from_schema(&fetch_schema(id)?)
    } else {
        (Vec::new(), Vec::new())
    };

    let id = leak(id);
    // A plug-in that reads the clipboard gets the app's clipboard image with each invocation, as Fill From
    // Clipboard does; render hands it to the host.
    let reads_clipboard = eff.get("clipboard").and_then(Value::as_bool) == Some(true);
    let def = EffectDef {
        id,
        icon: ICON,
        category: crate::effect::Category::Effect(CATEGORY),
        controls,
        kind: crate::effect::EffectKind::Plugin,
        dialog: None,
        dialog_title: Some(name),
        indirect: reads_clipboard.then_some(&CLIPBOARD_UI),
    };
    Ok(Some((id, name, def, choice_keys)))
}

/// A clipboard-reading plug-in's invocation reads the app's clipboard image (`EffectEnv::image`).
static CLIPBOARD_UI: crate::effect::bb::ui::IndirectUi = crate::effect::bb::ui::IndirectUi {
    external_for: |_| crate::effect::bb::ui::External::Clipboard,
    ..crate::effect::bb::ui::IndirectUi::DEFAULT
};

/// What the user reads when a plug-in effect throws: the effect left the image as it was.
fn failed(name: &str, reason: &str) -> String {
    format!(
        "The plug-in effect “{name}” stopped with an error and left the image unchanged:\n{reason}"
    )
}

/// The Settings ▸ Plugins reason for a plugin that loaded only in part: the skipped effects
/// ("Name: reason" each), those with the same reason named together.
pub fn partly(skipped: &[String]) -> String {
    let mut groups: Vec<(&str, Vec<&str>)> = vec![];
    for s in skipped {
        let (name, reason) = s.split_once(": ").unwrap_or((s, ""));
        match groups.iter_mut().find(|g| g.0 == reason) {
            Some(g) => g.1.push(name),
            None => groups.push((reason, vec![name])),
        }
    }
    let parts: Vec<String> = groups
        .iter()
        .map(|(reason, names)| format!("{}: {reason}", names.join(", ")))
        .collect();
    format!("Some of its effects did not load. {}", parts.join("; "))
}

/// What did not load from a plugin, from its `enumerate` JSON's `errors` (each "Type: reason"): `Err`
/// with the reasons when nothing else came of the DLL, else the effects that were skipped.
fn problems(enumerated: &Value) -> Result<Vec<String>, String> {
    let list = |k: &str| enumerated.get(k).and_then(Value::as_array);
    let errors: Vec<String> = list("errors")
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let empty = |k: &str| list(k).is_none_or(Vec::is_empty);
    if !errors.is_empty() && empty("effects") && empty("fileTypes") {
        return Err(errors.join("; "));
    }
    Ok(errors)
}

/// One line of a plug-in's System.Drawing string as paint.rs's Text lays it out (`bb::text`'s faces and settings):
/// the browser host's managed System.Drawing (`pdn-host/paintnet-web/drawing`) fills these outlines itself. Windows
/// families paint.rs does not bundle map to the nearest bundled one. `style` is GDI+'s: bit 0 bold, bit 1 italic.
/// Out: `[width, height, ascent]`, then the outlines with the line's top-left at (0, 0), each element a verb and
/// its points: 0 move x y, 1 line x y, 2 quad x1 y1 x y, 3 cubic x1 y1 x2 y2 x y, 4 close.
pub fn text_outline(text: &str, family: &str, px: f32, style: i32) -> Vec<f32> {
    use crate::tool::text::{DrawItem, layout_text_px};
    use tiny_skia::PathSegment;
    let lower = family.to_ascii_lowercase();
    let family = match lower.as_str() {
        "times new roman" | "times" | "georgia" | "cambria" | "garamond" | "book antiqua"
        | "palatino linotype" => "serif",
        "courier new" | "courier" | "consolas" | "lucida console" => "monospace",
        "comic sans ms" => "Comic Neue",
        _ => family,
    };
    let s = super::bb::text::settings(family, style & 1 != 0, style & 2 != 0);
    let layout = |t: &str| layout_text_px(t, &s, [0.0, 0.0], px as f64);
    let run = layout(text);
    let Some(line) = run.lines.first() else {
        return vec![0.0; 3];
    };
    let ascent = line
        .glyphs
        .first()
        .map(|g| g.origin[1])
        .or_else(|| {
            layout(" ")
                .lines
                .first()?
                .glyphs
                .first()
                .map(|g| g.origin[1])
        })
        .unwrap_or(0.0);
    let mut out = vec![line.width as f32, line.height as f32, ascent as f32];
    for item in &line.items {
        let DrawItem::Outline(path) = item else {
            continue;
        };
        for seg in path.segments() {
            match seg {
                PathSegment::MoveTo(p) => out.extend([0.0, p.x, p.y]),
                PathSegment::LineTo(p) => out.extend([1.0, p.x, p.y]),
                PathSegment::QuadTo(a, p) => out.extend([2.0, a.x, a.y, p.x, p.y]),
                PathSegment::CubicTo(a, b, p) => out.extend([3.0, a.x, a.y, b.x, b.y, p.x, p.y]),
                PathSegment::Close => out.push(4.0),
            }
        }
    }
    out
}

/// An image file's pixels for the browser host's managed System.Drawing, read with paint.rs's own codecs (it reads
/// PNG, BMP, GIF and ICO itself; this gives it JPEG, TIFF and WebP): a little-endian width and height, then
/// straight BGRA8. `None` when no codec reads it.
pub fn decode_image(bytes: &[u8]) -> Option<Vec<u8>> {
    // "png" lets the codecs sniff the format from the bytes, as they do for every WIC type.
    let image = crate::io::codecs::decode("png", bytes).ok()?.image;
    let mut out = Vec::with_capacity(8 + image.data.len());
    out.extend(image.width.to_le_bytes());
    out.extend(image.height.to_le_bytes());
    out.extend(
        image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| [b, g, r, a]),
    );
    Some(out)
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{
    effects, init, is_configurable_action, is_plugin, load, name_of, press, render, revision,
    unload_all,
};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{
        EffectDef, EffectEnv, ImageData, Values, problems, register, render_props, unique_key,
    };
    use serde_json::Value;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, OnceLock, RwLock};

    struct Entry {
        def: EffectDef,
        dotnet_id: &'static str,
        choice_keys: Vec<&'static str>,
        plugin: Arc<pdn_bridge::Plugin>,
        /// The DLL it came from: loading the same DLL again replaces its entries.
        path: std::path::PathBuf,
    }

    static HOST: OnceLock<Result<pdn_bridge::Host, String>> = OnceLock::new();
    static REVISION: AtomicU64 = AtomicU64::new(0);

    fn registry() -> &'static RwLock<HashMap<&'static str, Entry>> {
        static R: OnceLock<RwLock<HashMap<&'static str, Entry>>> = OnceLock::new();
        R.get_or_init(|| RwLock::new(HashMap::new()))
    }

    /// The display names of loaded effects, for `display_name_of`/`provenance_of`.
    fn names() -> &'static RwLock<HashMap<&'static str, &'static str>> {
        static N: OnceLock<RwLock<HashMap<&'static str, &'static str>>> = OnceLock::new();
        N.get_or_init(|| RwLock::new(HashMap::new()))
    }

    /// Start CoreCLR once; `bridge_dir` holds PdnBridge.dll + its runtimeconfig + the
    /// replacement PaintDotNet.*.dll. Idempotent; later calls return the first outcome.
    pub fn init(bridge_dir: &Path) -> Result<(), String> {
        HOST.get_or_init(|| {
            let host = pdn_bridge::Host::start(bridge_dir).map_err(|e| e.to_string())?;
            host.set_drawing_host(text_outline_c, free_floats_c, decode_image_c, free_bytes_c);
            Ok(host)
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
    }

    /// `text_outline` over the bridge's C ABI (`PdnSetDrawingHost`): UTF-16 text, a UTF-8 family, a float buffer
    /// out that `free_floats_c` releases. Null when it panics.
    unsafe extern "C" fn text_outline_c(
        text: *const u16,
        len: std::ffi::c_int,
        family: *const std::ffi::c_char,
        px: f32,
        style: std::ffi::c_int,
        out_len: *mut std::ffi::c_int,
    ) -> *mut f32 {
        let floats = std::panic::catch_unwind(|| {
            let units = match len {
                1.. if !text.is_null() => unsafe { std::slice::from_raw_parts(text, len as usize) },
                _ => &[],
            };
            let text = String::from_utf16_lossy(units);
            let family = unsafe { std::ffi::CStr::from_ptr(family) }.to_string_lossy();
            super::text_outline(&text, &family, px, style).into_boxed_slice()
        });
        let Ok(floats) = floats else {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        };
        unsafe { *out_len = floats.len() as std::ffi::c_int };
        Box::into_raw(floats) as *mut f32
    }

    unsafe extern "C" fn free_floats_c(floats: *mut f32, len: std::ffi::c_int) {
        if !floats.is_null() {
            drop(unsafe {
                Box::from_raw(std::ptr::slice_from_raw_parts_mut(floats, len as usize))
            });
        }
    }

    /// `decode_image` over the bridge's C ABI: a byte buffer out that `free_bytes_c` releases; null when nothing
    /// reads the bytes (or it panics).
    unsafe extern "C" fn decode_image_c(
        bytes: *const u8,
        len: std::ffi::c_int,
        out_len: *mut std::ffi::c_int,
    ) -> *mut u8 {
        if bytes.is_null() || len <= 0 {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        }
        let decoded = std::panic::catch_unwind(|| {
            super::decode_image(unsafe { std::slice::from_raw_parts(bytes, len as usize) })
        });
        let Ok(Some(decoded)) = decoded else {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        };
        let decoded = decoded.into_boxed_slice();
        unsafe { *out_len = decoded.len() as std::ffi::c_int };
        Box::into_raw(decoded) as *mut u8
    }

    unsafe extern "C" fn free_bytes_c(bytes: *mut u8, len: std::ffi::c_int) {
        if !bytes.is_null() {
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes, len as usize)) });
        }
    }

    fn host() -> Result<&'static pdn_bridge::Host, String> {
        match HOST.get() {
            Some(Ok(h)) => Ok(h),
            Some(Err(e)) => Err(e.clone()),
            None => Err("the .NET plugin host is not initialized".into()),
        }
    }

    /// Load a plugin DLL and register its supported effects and FileTypes. `Err` says why nothing
    /// loaded; `Ok` lists the plugin's effects that were skipped, each "Type: reason".
    pub fn load(plugin_path: &Path) -> Result<Vec<String>, String> {
        let host = host()?;
        let plugin = Arc::new(host.load_plugin(plugin_path).map_err(|e| e.to_string())?);
        let json = plugin.enumerate().map_err(|e| e.to_string())?;
        let parsed: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let skipped = problems(&parsed)?;

        // Register the plugin's FileTypes so its formats open/save through io::codecs.
        crate::io::plugins::register_from(&plugin, &parsed);

        let mut reg = registry().write().unwrap();
        let mut nm = names().write().unwrap();
        for eff in parsed
            .get("effects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some((id, name, mut def, choice_keys)) = register(eff, |id| {
                plugin.effect_schema(id).map_err(|e| e.to_string())
            })? {
                let tag = plugin_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let key = unique_key(id, &tag, |k| {
                    reg.get(k).is_some_and(|e| e.path != plugin_path)
                });
                def.id = key;
                nm.insert(key, name);
                super::remember_menu(key, name, eff.get("submenu").and_then(Value::as_str));
                reg.insert(
                    key,
                    Entry {
                        def,
                        dotnet_id: id,
                        choice_keys,
                        plugin: plugin.clone(),
                        path: plugin_path.to_path_buf(),
                    },
                );
            }
        }
        REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(skipped)
    }

    /// Drop every loaded plugin (e.g. before reloading the folder).
    pub fn unload_all() {
        registry().write().unwrap().clear();
        names().write().unwrap().clear();
        crate::io::plugins::clear();
        REVISION.fetch_add(1, Ordering::Relaxed);
    }

    pub fn effects() -> Vec<EffectDef> {
        registry()
            .read()
            .unwrap()
            .values()
            .map(|e| e.def.clone())
            .collect()
    }

    pub fn name_of(id: &str) -> Option<&'static str> {
        names().read().unwrap().get(id).copied()
    }

    pub fn is_plugin(id: &str) -> bool {
        names().read().unwrap().contains_key(id)
    }

    pub fn is_configurable_action(action: &str) -> bool {
        action.strip_prefix("menu.effects.").is_some_and(|id| {
            registry()
                .read()
                .unwrap()
                .get(id)
                .is_some_and(|e| !e.def.controls.is_empty())
        })
    }

    pub fn revision() -> u64 {
        REVISION.load(Ordering::Relaxed)
    }

    /// The host's clipboard image for a clipboard-reading plug-in, set only when it changed: an invocation keeps
    /// its clipboard image (one allocation) for all its renders. The key also samples the pixels, so a new image
    /// that happens to reuse a freed one's memory is still sent.
    fn set_clipboard(plugin: &pdn_bridge::Plugin, image: Option<&ImageData>) {
        type Key = Option<(usize, u32, u32, u64)>;
        static SET: std::sync::Mutex<Option<Key>> = std::sync::Mutex::new(None);
        let key: Key = image.map(|i| {
            let step = (i.data.len() / 997).max(1);
            let sample = i
                .data
                .iter()
                .step_by(step)
                .fold(0u64, |h, &b| h.rotate_left(5) ^ u64::from(b));
            (i.data.as_ptr() as usize, i.width, i.height, sample)
        });
        let mut set = SET.lock().unwrap();
        // Unchanged; or empty before anything was sent, when the host's clipboard is as it started (empty, or a
        // test run's fixed image).
        if *set == Some(key) || (set.is_none() && key.is_none()) {
            return;
        }
        if plugin
            .set_clipboard(image.map(|i| (&i.data[..], i.width, i.height)))
            .is_ok()
        {
            *set = Some(key);
        }
    }

    /// Presses the button `key` of plug-in effect `id`'s own settings window over `src`, the window loaded as
    /// `values` leave it; keeps the token that makes and the window's settings after it in `values`.
    pub fn press(
        id: &str,
        key: &str,
        src: &ImageData,
        values: &mut Values,
        env: &EffectEnv,
    ) -> Result<(), String> {
        let reg = registry().read().unwrap();
        let entry = reg.get(id).ok_or("the plug-in is not loaded")?;
        let props = render_props(values, &entry.choice_keys, &entry.def.controls, env);
        let result = entry
            .plugin
            .dialog_press(
                entry.dotnet_id,
                Some(&props),
                key,
                &src.data,
                src.width,
                src.height,
            )
            .map_err(|e| e.to_string())?;
        super::take_press(&result, &entry.def.controls, values)
    }

    pub fn render(id: &str, src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
        let mut out = src.clone();
        let reg = registry().read().unwrap();
        let Some(entry) = reg.get(id) else { return out };

        let props = render_props(values, &entry.choice_keys, &entry.def.controls, env);
        if entry.def.indirect.is_some() {
            set_clipboard(&entry.plugin, env.image);
        }
        let b = env.bounds;
        let roi = (b.x, b.y, b.w as i32, b.h as i32);
        let fallback = AtomicBool::new(false);
        let cancel = env.cancel.unwrap_or(&fallback);

        match entry.plugin.render_effect(
            entry.dotnet_id,
            Some(&props),
            &mut out.data,
            out.width,
            out.height,
            roi,
            cancel,
        ) {
            Ok(()) => out,
            Err(e) => {
                if let Some(notice) = env.notice {
                    *notice.lock().unwrap() =
                        Some(super::failed(name_of(id).unwrap_or(id), &e.to_string()));
                }
                src.clone()
            }
        }
    }
}

// --- web: the Mono browser host (pdn-host/paintnet-web), reached over a small JS bridge. ---
#[cfg(target_arch = "wasm32")]
pub use web::{
    PluginJob, effects, filetype_open, filetype_save, install, is_configurable_action, is_plugin,
    name_of, press, render, render_begin, revision, unload_all,
};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{
        EffectDef, EffectEnv, ImageData, Values, problems, register, render_props, unique_key,
    };
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use wasm_bindgen::prelude::*;

    // The browser host's JS bridge (pdn_web.js): lazily boots the .NET runtime, then drives the
    // same operations as the desktop C ABI. Boot is async (the ~MB runtime download); load,
    // enumerate, schema and render are synchronous calls into the booted interpreter.
    #[wasm_bindgen(module = "/src/effect/pdn_web.js")]
    extern "C" {
        #[wasm_bindgen(catch, js_name = pdnBoot)]
        async fn pdn_boot(
            text_outline: &JsValue,
            decode_image: &JsValue,
            font_names: Vec<String>,
            font_files: Vec<js_sys::Uint8Array>,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnSetInstalled)]
        async fn pdn_set_installed(
            names: Vec<String>,
            files: Vec<js_sys::Uint8Array>,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnLoadInstalled)]
        fn pdn_load_installed(file_name: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnEnumerate)]
        fn pdn_enumerate(handle: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnEffectSchema)]
        fn pdn_effect_schema(handle: i32, id: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnDialogPress)]
        fn pdn_dialog_press(
            handle: i32,
            id: &str,
            props: Option<String>,
            key: &str,
            bgra: &[u8],
            width: i32,
            height: i32,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnRender)]
        fn pdn_render(
            handle: i32,
            id: &str,
            props: Option<String>,
            bgra: &[u8],
            width: i32,
            height: i32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnFileTypeSave)]
        fn pdn_filetype_save(
            handle: i32,
            index: i32,
            props: Option<String>,
            bgra: &[u8],
            width: i32,
            height: i32,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnFileTypeOpen)]
        fn pdn_filetype_open(
            handle: i32,
            index: i32,
            bytes: &[u8],
            props: Option<String>,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnRenderBegin)]
        fn pdn_render_begin(
            handle: i32,
            id: &str,
            props: Option<String>,
            bgra: &[u8],
            width: i32,
            height: i32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnRenderStripCount)]
        fn pdn_render_strip_count(job: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnRenderStep)]
        fn pdn_render_step(job: i32, n: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnRenderFinish)]
        fn pdn_render_finish(job: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnRenderCancelJob)]
        fn pdn_render_cancel_job(job: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(js_name = pdnRuntimeBytes)]
        fn pdn_runtime_bytes() -> f64;
    }

    /// An in-flight incremental render of one plugin effect (the browser cancellation/progress
    /// path): the host renders the ROI's strips a few at a time, so the UI steps it across frames
    /// and polls a cancel flag between (see `EffectSession::plugin_slice`). Byte-identical to a
    /// one-shot [`render`].
    pub struct PluginJob {
        job: i32,
        strips: usize,
        done: usize,
    }

    /// Begin an incremental render of effect `id` over `src` (RGBA8) for the ROI `(x,y,w,h)`.
    pub fn render_begin(
        id: &str,
        src: &ImageData,
        roi: (i32, i32, i32, i32),
        values: &Values,
        env: &EffectEnv,
    ) -> Result<PluginJob, String> {
        let Some((handle, dotnet_id, choice_keys, controls)) = REGISTRY.with(|r| {
            r.borrow().get(id).map(|e| {
                (
                    e.handle,
                    e.dotnet_id,
                    e.choice_keys.clone(),
                    e.def.controls.clone(),
                )
            })
        }) else {
            return Err("effect not loaded".into());
        };
        let props = render_props(values, &choice_keys, &controls, env);
        let mut bgra = src.data.clone();
        swap_rb(&mut bgra);
        let job = pdn_render_begin(
            handle,
            dotnet_id,
            Some(props),
            &bgra,
            src.width as i32,
            src.height as i32,
            roi.0,
            roi.1,
            roi.2,
            roi.3,
        )
        .map_err(msg)?
        .as_f64()
        .ok_or("bad render job")? as i32;
        let strips = pdn_render_strip_count(job)
            .map_err(msg)?
            .as_f64()
            .unwrap_or(0.0) as usize;
        Ok(PluginJob {
            job,
            strips,
            done: 0,
        })
    }

    impl PluginJob {
        pub fn strips(&self) -> usize {
            self.strips
        }
        pub fn done(&self) -> usize {
            self.done
        }
        /// Render up to `n` more strips; returns true once the render is complete.
        pub fn step(&mut self, n: usize) -> Result<bool, String> {
            let remaining = pdn_render_step(self.job, n as i32)
                .map_err(msg)?
                .as_f64()
                .ok_or("bad step")? as usize;
            self.done = self.strips.saturating_sub(remaining);
            Ok(remaining == 0)
        }
        /// Finish and return the raw plugin output as RGBA8 (the caller clips to the selection).
        pub fn finish(self) -> Result<Vec<u8>, String> {
            let mut rgba =
                js_sys::Uint8Array::new(&pdn_render_finish(self.job).map_err(msg)?).to_vec();
            swap_rb(&mut rgba);
            Ok(rgba)
        }
        /// Abandon the job without finishing.
        pub fn cancel(self) {
            let _ = pdn_render_cancel_job(self.job);
        }
    }

    struct Entry {
        def: EffectDef,
        dotnet_id: &'static str,
        choice_keys: Vec<&'static str>,
        handle: i32,
    }

    // wasm is single-threaded, so thread-local RefCells stand in for the native RwLocks.
    thread_local! {
        static REGISTRY: RefCell<HashMap<&'static str, Entry>> = RefCell::new(HashMap::new());
        static NAMES: RefCell<HashMap<&'static str, &'static str>> = RefCell::new(HashMap::new());
        static REVISION: Cell<u64> = const { Cell::new(0) };
    }

    /// The `message` of a thrown JS error, else its string form.
    fn msg(v: JsValue) -> String {
        js_sys::Reflect::get(&v, &JsValue::from_str("message"))
            .ok()
            .and_then(|m| m.as_string())
            .or_else(|| v.as_string())
            .unwrap_or_else(|| "The .NET plugin host failed".into())
    }

    thread_local! {
        // paint.rs's text and image codecs, which the host's managed System.Drawing draws plug-in strings and reads
        // images with (as the desktop bridge's PdnSetDrawingHost gives them): JS functions for pdn_web.js to hand on.
        static TEXT_OUTLINE: JsValue = Closure::<dyn Fn(String, String, f32, i32) -> Vec<f32>>::new(
            |text: String, family: String, px: f32, style: i32| super::text_outline(&text, &family, px, style),
        )
        .into_js_value();
        static DECODE_IMAGE: JsValue =
            Closure::<dyn Fn(Vec<u8>) -> Option<Vec<u8>>>::new(|bytes: Vec<u8>| super::decode_image(&bytes))
                .into_js_value();
    }

    /// paint.rs's own faces, which the host's DirectWrite (OpenD2D, for GPU plug-ins' text) finds in its font folder:
    /// the text tool's bundled families, and the interface's Selawik and Carlito.
    fn font_files() -> (Vec<String>, Vec<js_sys::Uint8Array>) {
        use crate::fonts::{CARLITO_REGULAR, FaceStyle, SELAWIK_BOLD, SELAWIK_REGULAR};
        let mut faces: Vec<(String, std::sync::Arc<[u8]>)> = crate::fonts::bundled_families()
            .iter()
            .filter_map(|f| {
                Some((
                    format!("{f}.ttf"),
                    crate::fonts::registered_face(f, FaceStyle::Regular)?,
                ))
            })
            .collect();
        for (name, bytes) in [
            ("Selawik.ttf", SELAWIK_REGULAR),
            ("Selawik Bold.ttf", SELAWIK_BOLD),
            ("Carlito.ttf", CARLITO_REGULAR),
        ] {
            faces.push((name.into(), bytes.into()));
        }
        faces
            .into_iter()
            .map(|(name, bytes)| (name, js_sys::Uint8Array::from(&bytes[..])))
            .unzip()
    }

    /// Boot the runtime (lazily, once), then (re)load the given plugins and register their
    /// supported effects. `plugins` is (file name, bytes): the DLLs, and the files plug-ins read
    /// beside themselves (`.dlc`, `.json`), which are installed and not loaded. A plugin that
    /// fails to load is skipped and reported; the rest still load. Returns the runtime download
    /// size (MiB) and the plugins that did not load, or loaded only in part: (file name, reason).
    #[allow(clippy::type_complexity)]
    pub async fn install(
        plugins: Vec<(String, Vec<u8>)>,
    ) -> Result<(f64, Vec<(String, String)>), String> {
        let (font_names, font_files) = font_files();
        pdn_boot(
            &TEXT_OUTLINE.with(Clone::clone),
            &DECODE_IMAGE.with(Clone::clone),
            font_names,
            font_files,
        )
        .await
        .map_err(msg)?;
        unload_all();
        // Every installed file, for the plug-ins' private libraries (AForge, MathNet, a shared option
        // library) and the files they read beside themselves.
        pdn_set_installed(
            plugins.iter().map(|(name, _)| name.clone()).collect(),
            plugins
                .iter()
                .map(|(_, dll)| js_sys::Uint8Array::from(&dll[..]))
                .collect(),
        )
        .await
        .map_err(msg)?;
        let mut errors = Vec::new();
        for (name, _) in plugins
            .iter()
            .filter(|(name, _)| name.to_ascii_lowercase().ends_with(".dll"))
        {
            match load_one(name) {
                Ok(skipped) if skipped.is_empty() => {}
                Ok(skipped) => errors.push((name.clone(), super::partly(&skipped))),
                Err(e) => errors.push((name.clone(), e)),
            }
        }
        REVISION.with(|r| r.set(r.get() + 1));
        Ok((runtime_mib(), errors))
    }

    fn load_one(file_name: &str) -> Result<Vec<String>, String> {
        let handle = pdn_load_installed(file_name)
            .map_err(msg)?
            .as_f64()
            .ok_or("bad plugin handle")? as i32;
        let json = pdn_enumerate(handle)
            .map_err(msg)?
            .as_string()
            .ok_or("enumerate returned no JSON")?;
        let parsed: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let skipped = problems(&parsed)?;
        // Register the plugin's FileTypes so its formats open/save through io::codecs.
        crate::io::plugins::register_web(handle, &parsed);
        for eff in parsed
            .get("effects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let entry = register(eff, |id| {
                pdn_effect_schema(handle, id)
                    .map_err(msg)?
                    .as_string()
                    .ok_or_else(|| "schema returned no JSON".into())
            })?;
            if let Some((id, name, mut def, choice_keys)) = entry {
                let key = REGISTRY.with(|r| {
                    let r = r.borrow();
                    unique_key(id, &handle.to_string(), |k| {
                        r.get(k).is_some_and(|e| e.handle != handle)
                    })
                });
                def.id = key;
                NAMES.with(|n| n.borrow_mut().insert(key, name));
                super::remember_menu(key, name, eff.get("submenu").and_then(Value::as_str));
                REGISTRY.with(|r| {
                    r.borrow_mut().insert(
                        key,
                        Entry {
                            def,
                            dotnet_id: id,
                            choice_keys,
                            handle,
                        },
                    )
                });
            }
        }
        Ok(skipped)
    }

    pub fn unload_all() {
        REGISTRY.with(|r| r.borrow_mut().clear());
        NAMES.with(|n| n.borrow_mut().clear());
        crate::io::plugins::clear();
        REVISION.with(|r| r.set(r.get() + 1));
    }

    /// Encode an RGBA8 image through the plugin FileType at `index`; returns the file bytes.
    /// (io::plugins drives this; R<->B is swapped at the host boundary like the desktop path.)
    pub fn filetype_save(
        handle: i32,
        index: u32,
        props: Option<String>,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, String> {
        let mut bgra = rgba.to_vec();
        swap_rb(&mut bgra);
        let v = pdn_filetype_save(
            handle,
            index as i32,
            props,
            &bgra,
            width as i32,
            height as i32,
        )
        .map_err(msg)?;
        Ok(js_sys::Uint8Array::new(&v).to_vec())
    }

    /// Decode file bytes through the plugin FileType at `index` (`props`: its windows' answers); returns (RGBA8,
    /// width, height).
    pub fn filetype_open(
        handle: i32,
        index: u32,
        bytes: &[u8],
        props: Option<&str>,
    ) -> Result<(Vec<u8>, u32, u32), String> {
        let props = props.map(str::to_owned);
        let out = pdn_filetype_open(handle, index as i32, bytes, props).map_err(msg)?;
        let out = js_sys::Uint8Array::new(&out).to_vec();
        if out.len() < 8 {
            return Err("filetype open returned no header".into());
        }
        let w = u32::from_le_bytes([out[0], out[1], out[2], out[3]]);
        let h = u32::from_le_bytes([out[4], out[5], out[6], out[7]]);
        let mut rgba = out[8..].to_vec();
        swap_rb(&mut rgba);
        Ok((rgba, w, h))
    }

    pub fn effects() -> Vec<EffectDef> {
        REGISTRY.with(|r| r.borrow().values().map(|e| e.def.clone()).collect())
    }

    pub fn name_of(id: &str) -> Option<&'static str> {
        NAMES.with(|n| n.borrow().get(id).copied())
    }

    pub fn is_plugin(id: &str) -> bool {
        NAMES.with(|n| n.borrow().contains_key(id))
    }

    pub fn is_configurable_action(action: &str) -> bool {
        action.strip_prefix("menu.effects.").is_some_and(|id| {
            REGISTRY.with(|r| {
                r.borrow()
                    .get(id)
                    .is_some_and(|e| !e.def.controls.is_empty())
            })
        })
    }

    pub fn revision() -> u64 {
        REVISION.with(Cell::get)
    }

    /// MiB transferred for the .NET runtime so far (0 before boot), reported by `install`.
    fn runtime_mib() -> f64 {
        pdn_runtime_bytes() / (1u64 << 20) as f64
    }

    /// Presses the button `key` of plug-in effect `id`'s own settings window over `src`, the window loaded as
    /// `values` leave it; keeps the token that makes and the window's settings after it in `values`.
    pub fn press(
        id: &str,
        key: &str,
        src: &ImageData,
        values: &mut Values,
        env: &EffectEnv,
    ) -> Result<(), String> {
        let (handle, dotnet_id, choice_keys, controls) = REGISTRY
            .with(|r| {
                r.borrow().get(id).map(|e| {
                    (
                        e.handle,
                        e.dotnet_id,
                        e.choice_keys.clone(),
                        e.def.controls.clone(),
                    )
                })
            })
            .ok_or("the plug-in is not loaded")?;
        let props = render_props(values, &choice_keys, &controls, env);
        let mut bgra = src.data.clone();
        swap_rb(&mut bgra);
        let result = pdn_dialog_press(
            handle,
            dotnet_id,
            Some(props),
            key,
            &bgra,
            src.width as i32,
            src.height as i32,
        )
        .map_err(msg)?
        .as_string()
        .ok_or("the press returned no JSON")?;
        super::take_press(&result, &controls, values)
    }

    pub fn render(id: &str, src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
        let Some((handle, dotnet_id, choice_keys, controls)) = REGISTRY.with(|r| {
            r.borrow().get(id).map(|e| {
                (
                    e.handle,
                    e.dotnet_id,
                    e.choice_keys.clone(),
                    e.def.controls.clone(),
                )
            })
        }) else {
            return src.clone();
        };
        let props = render_props(values, &choice_keys, &controls, env);
        let b = env.bounds;
        // paint.rs stores straight RGBA8; the host speaks straight BGRA8, so swap R<->B at the
        // boundary as the desktop binding does.
        let mut bgra = src.data.clone();
        swap_rb(&mut bgra);
        match pdn_render(
            handle,
            dotnet_id,
            Some(props),
            &bgra,
            src.width as i32,
            src.height as i32,
            b.x,
            b.y,
            b.w as i32,
            b.h as i32,
        ) {
            Ok(v) => {
                let got = js_sys::Uint8Array::new(&v);
                let mut out = src.clone();
                if got.length() as usize == out.data.len() {
                    got.copy_to(&mut out.data);
                    swap_rb(&mut out.data);
                    out
                } else {
                    src.clone()
                }
            }
            Err(e) => {
                if let Some(notice) = env.notice {
                    *notice.lock().unwrap() =
                        Some(super::failed(name_of(id).unwrap_or(id), &msg(e)));
                }
                src.clone()
            }
        }
    }

    fn swap_rb(buf: &mut [u8]) {
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    /// A press of a plug-in window's button keeps the token it made and the window's settings as it shows them
    /// after (a list's new items too); a render then sends that token and every window setting, so the window
    /// loaded from the token takes them all, even one at its schema default, but no button and no list's items.
    #[test]
    fn a_press_keeps_its_token_and_the_windows_settings() {
        let schema = serde_json::json!({ "properties": [
            { "name": "w:1.", "type": "int", "label": "Width", "default": 4, "min": 1, "max": 10 },
            { "name": "w:2.", "type": "choice", "label": "Style", "default": 0,
              "choices": [{ "value": 0, "label": "Solid" }, { "value": 1, "label": "Dashed" }] },
            { "name": "w:3.", "type": "list", "label": "Set", "default": -1, "items": [] },
            { "name": "w:4.", "type": "button", "label": "Add" },
        ]});
        let (controls, choice_keys) = super::controls_from_properties(&schema);
        assert!(matches!(
            controls[3],
            super::Control::Button { label: "Add", .. }
        ));
        let mut values: super::Values = controls
            .iter()
            .map(|c| (c.key().to_owned(), c.default_value()))
            .collect();
        let pressed = r#"{"token":"{\"lines\":1}","properties":[
            {"name":"w:1.","type":"int","default":4},
            {"name":"w:2.","type":"choice","default":1},
            {"name":"w:3.","type":"list","default":0,"items":["Width 4, Style 1"]}]}"#;
        super::take_press(pressed, &controls, &mut values).unwrap();
        assert_eq!(values["$token"], "{\"lines\":1}");
        assert_eq!(values["w:2."], "1");
        assert_eq!(values["w:3."], 0);
        assert_eq!(
            crate::effect::list_items(&values, "w:3.", &[]),
            ["Width 4, Style 1"]
        );

        let env = super::EffectEnv::whole(1, 1);
        let props: Value =
            serde_json::from_str(&super::render_props(&values, &choice_keys, &controls, &env))
                .unwrap();
        assert_eq!(props["$token"], "{\"lines\":1}");
        assert_eq!(
            props["w:1."], 4.0,
            "with a token, a setting at its default is sent too"
        );
        assert_eq!(props["w:2."], 1);
        assert!(props.get("w:4.").is_none() && props.get("w:3.#items").is_none());
        // Without a token, settings at their defaults stay the window's own.
        values.remove("$token");
        let props: Value =
            serde_json::from_str(&super::render_props(&values, &choice_keys, &controls, &env))
                .unwrap();
        assert!(props.get("w:1.").is_none());
    }

    /// A plug-in window's curve is a curve control whose channels follow the choice that swaps them, its own
    /// channels when the choice's set is unknown, and a press takes the points the window shows.
    #[test]
    fn a_windows_curve_follows_its_mode() {
        let schema = serde_json::json!({ "properties": [
            { "name": "w:0.", "type": "choice", "label": "Mode", "default": 0,
              "choices": [{ "value": 0, "label": "Gray" }, { "value": 1, "label": "Colour" }, { "value": 2, "label": "Odd" }] },
            { "name": "w:1.curve", "type": "curve", "label": "Curve", "max": 255,
              "default": [[[0, 0], [255, 255]]], "channels": ["Gray"], "colors": [""], "by": "w:0.",
              "sets": [{ "default": [[[0, 0], [255, 255]]], "channels": ["Gray"], "colors": [""] },
                       { "default": [[[0, 0], [255, 255]], [[0, 0], [255, 255]]], "channels": ["Red", "Green"],
                         "colors": ["#ff0000", "#00ff00"] },
                       { "default": [], "channels": [], "colors": [] }] },
        ]});
        let (controls, _) = super::controls_from_properties(&schema);
        let super::Control::PluginCurve {
            max, own, by, sets, ..
        } = &controls[1]
        else {
            panic!("{controls:?}")
        };
        assert_eq!((*max, *by), (255, Some("w:0.")));
        let mut values = crate::effect::Values::new();
        let pick =
            |values: &super::Values| crate::effect::curve_channels(own, *by, sets, values).names;
        assert_eq!(pick(&values), ["Gray"]);
        values.insert("w:0.".into(), Value::from("1"));
        assert_eq!(pick(&values), ["Red", "Green"]);
        assert_eq!(
            crate::effect::curve_channels(own, *by, sets, &values).colors,
            [Some([255, 0, 0]), Some([0, 255, 0])]
        );
        values.insert("w:0.".into(), Value::from("2"));
        assert_eq!(pick(&values), ["Gray"], "a set the window gave no channels");

        let pressed = r#"{"properties":[{"name":"w:1.curve","type":"curve","default":[[[0,0],[100,90],[255,255]]]}]}"#;
        super::take_press(pressed, &controls, &mut values).unwrap();
        assert_eq!(
            values["w:1.curve"],
            serde_json::json!([[[0, 0], [100, 90], [255, 255]]])
        );
    }

    /// A plug-in window's displacement grid is a grid control, none of its nodes moved at first, its columns the label
    /// of the choice that sets them; a press takes the nodes the window shows.
    #[test]
    fn a_windows_grid_takes_its_size_from_its_choice() {
        let schema = serde_json::json!({ "properties": [
            { "name": "w:0.", "type": "choice", "label": "Columns", "default": 0,
              "choices": [{ "value": 0, "label": "2" }, { "value": 1, "label": "5" }] },
            { "name": "w:grid", "type": "grid", "label": "Grid", "columns": 2, "rows": 3,
              "default": [], "columnsBy": "w:0." },
        ]});
        let (controls, _) = super::controls_from_properties(&schema);
        let grid = &controls[1];
        assert_eq!(grid.default_value(), serde_json::json!([]));
        let mut values: super::Values = controls
            .iter()
            .map(|c| (c.key().to_owned(), c.default_value()))
            .collect();
        assert_eq!(crate::effect::grid_size(grid, &controls, &values), (2, 3));
        values.insert("w:0.".into(), Value::from("1"));
        assert_eq!(crate::effect::grid_size(grid, &controls, &values), (5, 3));

        let pressed =
            r#"{"properties":[{"name":"w:grid","type":"grid","default":[[1,1,0.5,-0.25]]}]}"#;
        super::take_press(pressed, &controls, &mut values).unwrap();
        assert_eq!(values["w:grid"], serde_json::json!([[1, 1, 0.5, -0.25]]));
    }

    /// A plug-in's string comes back as outlines a line wide and tall, and a missing Windows family still draws.
    #[test]
    fn text_outline_gives_a_lines_outlines() {
        let out = super::text_outline("Ag", "Arial", 32.0, 0);
        let (width, height, ascent) = (out[0], out[1], out[2]);
        assert!(
            width > 20.0 && height > 32.0 && ascent > 20.0 && ascent < height,
            "{:?}",
            &out[..3]
        );
        assert_eq!(out[3], 0.0, "the outlines start with a move");
        assert!(out.len() > 50);
        let bold = super::text_outline("Ag", "Times New Roman", 32.0, 1);
        assert!(bold[0] > 0.0 && bold.len() > 50);
        assert_eq!(super::text_outline("", "Arial", 32.0, 0).len(), 3);
    }

    /// A JPEG (which the browser host's System.Drawing cannot read itself) decodes to its size and BGRA.
    #[test]
    fn decode_image_reads_a_jpeg_as_bgra() {
        let red = crate::canvas::ImageData {
            width: 8,
            height: 4,
            data: [255, 0, 0, 255].repeat(32),
        };
        let jpeg = crate::io::encode::encode_jpeg(&red, &Default::default()).unwrap();
        let out = super::decode_image(&jpeg).unwrap();
        assert_eq!(
            (&out[..4], &out[4..8], out.len()),
            (
                &8u32.to_le_bytes()[..],
                &4u32.to_le_bytes()[..],
                8 + 8 * 4 * 4
            )
        );
        let (b, g, r, a) = (out[8], out[9], out[10], out[11]);
        assert!(
            r > 240 && g < 16 && b < 16 && a == 255,
            "{:?}",
            (b, g, r, a)
        );
        assert!(super::decode_image(b"not an image").is_none());
    }

    /// Two plug-ins defining one .NET type name each get an entry; the first keeps the plain name.
    #[test]
    fn colliding_type_names_get_distinct_keys() {
        use std::cell::RefCell;
        let taken = RefCell::new(std::collections::HashMap::<String, &str>::new());
        let add = |id: &'static str, dll: &'static str| {
            let key = super::unique_key(id, dll, |k| {
                taken.borrow().get(k).is_some_and(|d| *d != dll)
            });
            taken.borrow_mut().insert(key.into(), dll);
            key
        };
        assert_eq!(
            add("PaintDotNet.Effects.TileEffect", "Splashes"),
            "PaintDotNet.Effects.TileEffect"
        );
        assert_eq!(
            add("PaintDotNet.Effects.TileEffect", "Tiles"),
            "PaintDotNet.Effects.TileEffect@Tiles"
        );
        // The same DLL again replaces its own entry.
        assert_eq!(
            add("PaintDotNet.Effects.TileEffect", "Splashes"),
            "PaintDotNet.Effects.TileEffect"
        );
    }

    use super::*;

    /// A registered effect gets a menu action named for it, and the submenu it declares one of its own, shared by
    /// every effect that declares it whatever its case; an effect that declares none has no submenu.
    #[test]
    fn a_declared_submenu_is_a_menu_action() {
        super::remember_menu("Menu.Test.One", "One", Some("Render"));
        super::remember_menu("Menu.Test.Two", "Two", Some("render "));
        super::remember_menu("Menu.Test.Three", "Three", Some("  "));
        let one = super::submenu_of("Menu.Test.One").unwrap();
        assert_eq!(super::submenu_of("Menu.Test.Two"), Some(one));
        assert_eq!(super::action(one).and_then(|a| a.label), Some("Render"));
        assert_eq!(super::submenu_of("Menu.Test.Three"), None);
        assert_eq!(
            crate::action::get("menu.effects.Menu.Test.Two").and_then(|a| a.label),
            Some("Two")
        );
    }

    /// What did not load comes from the enumerate JSON's errors: the reasons when the DLL gave nothing
    /// else, else the skipped effects, named together by reason.
    #[test]
    fn load_problems_are_reported() {
        use serde_json::json;
        let nothing = json!({"effects": [], "fileTypes": [], "errors": ["X: missing type"]});
        assert_eq!(problems(&nothing), Err("X: missing type".into()));
        let helper = json!({"effects": [], "fileTypes": [], "errors": []});
        assert_eq!(problems(&helper), Ok(vec![]));
        let some =
            json!({"effects": [{"id": "A"}], "errors": ["B: dialog", "C: dialog", "D: other"]});
        let skipped = problems(&some).unwrap();
        assert_eq!(
            partly(&skipped),
            "Some of its effects did not load. B, C: dialog; D: other"
        );
    }

    /// IndirectUI labels: DisplayName names a slider; a checkbox's Description caption wins over
    /// its (often empty) DisplayName; with neither the property name stays.
    #[test]
    fn schema_labels_come_from_indirect_ui() {
        let schema = r#"{"properties":[
            {"name":"Amount1","label":"Radius","type":"int","default":2,"min":0,"max":100},
            {"name":"Amount2","label":"","description":"Red","type":"bool","default":true},
            {"name":"Amount3","type":"bool","default":false}]}"#;
        let (controls, _) = controls_from_schema(schema);
        let labels: Vec<&str> = controls
            .iter()
            .map(|c| match c {
                Control::Range { label, .. } | Control::Checkbox { label, .. } => *label,
                _ => "",
            })
            .collect();
        assert_eq!(labels, ["Radius", "Red", "Amount3"]);
    }

    /// A colour (an IndirectUI colour wheel, or a plug-in dialog's colour swatch) is paint.rs's colour picker
    /// with the plug-in's default; a numeric box's step carries over.
    #[test]
    fn schema_colours_and_steps() {
        let schema = r#"{"properties":[
            {"name":"Tint","label":"Tint","type":"color","default":16747520},
            {"name":"w:3.","label":"Amount","type":"double","default":1.5,"min":0,"max":10,"step":0.5}]}"#;
        let (controls, _) = controls_from_schema(schema);
        assert!(matches!(
            controls[0],
            Control::Color {
                key: "Tint",
                default: ColorDefault::Rgb(0xFF8C00),
                ..
            }
        ));
        assert!(matches!(controls[1], Control::Range { step, .. } if step == 0.5));
    }

    /// A plug-in window's settings go to a render only when the user changed them (the window keeps the
    /// defaults it sets from the image); property settings always go.
    #[test]
    fn window_settings_go_only_when_changed() {
        let (controls, keys) = controls_from_schema(
            r#"{"properties":[
            {"name":"w:0.","label":"X","type":"int","default":0,"min":0,"max":10},
            {"name":"w:1.","label":"Y","type":"int","default":0,"min":0,"max":10},
            {"name":"Amount","label":"A","type":"int","default":3,"min":0,"max":10}]}"#,
        );
        let mut values: Values = controls
            .iter()
            .map(|c| (c.key().to_string(), c.default_value()))
            .collect();
        values.insert("w:1.".into(), Value::from(7.0));
        let env = EffectEnv::whole(4, 4);
        let props: Value =
            serde_json::from_str(&render_props(&values, &keys, &controls, &env)).unwrap();
        assert!(props.get("w:0.").is_none(), "{props}");
        assert_eq!(props["w:1."], 7.0);
        assert_eq!(props["Amount"], 3.0);
    }

    /// A plug-in that reads the clipboard gets the app's clipboard image with each invocation; others read none.
    #[test]
    fn clipboard_plugins_read_the_clipboard() {
        let reads = |clipboard: bool| {
            let eff = serde_json::json!({"id": "A.B", "name": "B", "supported": true, "clipboard": clipboard});
            let (_, _, def, _) = register(&eff, |_| Ok(String::new())).unwrap().unwrap();
            def.indirect.map(|u| {
                (u.external_for)(&Values::new()) == crate::effect::bb::ui::External::Clipboard
            })
        };
        assert_eq!(reads(true), Some(true));
        assert_eq!(reads(false), None);
    }

    /// When part of the image is selected the render carries the selection's outline, each polygon flat;
    /// with everything selected (no mask) it carries none.
    #[test]
    fn a_selection_goes_as_its_outline() {
        let outline = vec![vec![[0.0, 0.0], [6.0, 0.0], [0.0, 4.0]]];
        let rect = crate::pixel::IntRect {
            x: 0,
            y: 0,
            w: 6,
            h: 4,
        };
        let mask = crate::pixel::mask::Mask::opaque(rect);
        let mut env = EffectEnv::whole(6, 4);
        env.selection = &outline;
        let whole: Value =
            serde_json::from_str(&render_props(&Values::new(), &[], &[], &env)).unwrap();
        assert!(whole["$environment"].get("selection").is_none(), "{whole}");
        env.mask = Some(&mask);
        let part: Value =
            serde_json::from_str(&render_props(&Values::new(), &[], &[], &env)).unwrap();
        assert_eq!(
            part["$environment"]["selection"],
            serde_json::json!([[0.0, 0.0, 6.0, 0.0, 0.0, 4.0]])
        );
    }
}
