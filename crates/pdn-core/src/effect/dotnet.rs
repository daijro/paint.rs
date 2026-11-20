//! Real Paint.NET plugin effects, loaded from DLLs through the embedded .NET host. On the
//! desktop that host is the `pdn-bridge` crate (CoreCLR via hostfxr, `pdn-host/docs/pdn-bridge.md`);
//! on the web it is the Mono browser host (`pdn-host/paintnet-web`), driven through a small JS
//! bridge over the SAME JSON protocol. Both register each loaded effect as a leaked-`'static`
//! [`EffectDef`] in Effects ▸ Plugins and render it through the host, reusing the same
//! session/dialog/undo pipeline as every built-in effect.

use super::{EffectDef, EffectEnv, Values};
use crate::canvas::ImageData;
use crate::effect::{ColorDefault, Control, range};
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

/// What an installed plug-in file provided when it loaded, for Settings ▸ Plugins: its effects (their registry
/// keys) and its file types (each one's name and the extensions it opens and saves).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Provided {
    pub effects: Vec<&'static str>,
    pub file_types: Vec<(String, Vec<String>, Vec<String>)>,
}

static PROVIDED: std::sync::LazyLock<
    std::sync::RwLock<std::collections::HashMap<String, Provided>>,
> = std::sync::LazyLock::new(Default::default);

/// Records what plug-in file `file` (its name) provided: `effects`, and the file types its enumeration lists.
pub fn remember_provided(file: &str, effects: Vec<&'static str>, enumerated: &Value) {
    let names = |ft: &Value, key: &str| -> Vec<String> {
        ft.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let file_types = enumerated
        .get("fileTypes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|ft| {
            let name = ft
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            (
                name,
                names(ft, "loadExtensions"),
                names(ft, "saveExtensions"),
            )
        })
        .collect();
    PROVIDED.write().unwrap_or_else(|e| e.into_inner()).insert(
        file.to_owned(),
        Provided {
            effects,
            file_types,
        },
    );
}

/// What installed plug-in file `file` provided, or `None` when it did not load (or is not a .NET plug-in).
pub fn provided(file: &str) -> Option<Provided> {
    PROVIDED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(file)
        .cloned()
}

fn forget_provided() {
    PROVIDED.write().unwrap_or_else(|e| e.into_inner()).clear();
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

/// Where an effect's values keep the token a press of one of its window's buttons made (`press`), or its live
/// canvas's window (`Canvas::token`).
pub const TOKEN: &str = "$token";

/// Where a live canvas sits in its plug-in window ([`Canvas::layout`]): the window's client size, and the canvas's
/// place and size in it, in the window's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasLayout {
    pub window: (u32, u32),
    pub at: (i32, i32),
    pub size: (u32, u32),
}

impl CanvasLayout {
    /// The host's "formW,formH,x,y,w,h".
    fn parse(text: &str) -> Option<CanvasLayout> {
        let n: Vec<i64> = text
            .split(',')
            .map(|v| v.trim().parse().ok())
            .collect::<Option<_>>()?;
        let &[fw, fh, x, y, w, h] = &n[..] else {
            return None;
        };
        let size = |v: i64| u32::try_from(v).ok().filter(|&v| v > 0);
        Some(CanvasLayout {
            window: (size(fw)?, size(fh)?),
            at: (x as i32, y as i32),
            size: (size(w)?, size(h)?),
        })
    }
}

/// Each live canvas of `controls` with its layout in the open window: as the host gives it, else the schema's size.
fn canvas_layouts(
    controls: &[Control],
    layout_of: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, CanvasLayout)> {
    controls
        .iter()
        .filter_map(|c| match *c {
            Control::PluginCanvas {
                key, width, height, ..
            } => {
                let schema = CanvasLayout {
                    window: (width, height),
                    at: (0, 0),
                    size: (width, height),
                };
                let live = layout_of(key).as_deref().and_then(CanvasLayout::parse);
                Some((key, live.unwrap_or(schema)))
            }
            _ => None,
        })
        .collect()
}

/// `props` (a render's, `render_props`) with the size of the window the plug-in's window opens over
/// (`$environment.window`), as Paint.NET gives a settings window its main window as its owner.
fn with_window(props: String, window: [u32; 2]) -> String {
    let mut v: Value = serde_json::from_str(&props).unwrap_or_else(|_| serde_json::json!({}));
    v["$environment"]["window"] = serde_json::json!(window);
    v.to_string()
}

/// What the live canvas `key` drew, as straight RGBA8 from the host, at its size in `layouts`.
fn canvas_image(
    layouts: &[(&'static str, CanvasLayout)],
    key: &str,
    rgba: Vec<u8>,
) -> Result<ImageData, String> {
    let (width, height) = layouts
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| l.size)
        .ok_or("the window has no such canvas")?;
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(format!(
            "the canvas drew {} bytes, not {width}×{height}",
            rgba.len()
        ));
    }
    Ok(ImageData {
        width,
        height,
        data: rgba,
    })
}

/// A file a press asked the user for (the window's own file prompt, e.g. Blur Map's Open): the prompt's title and
/// the file extensions it accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePick {
    pub title: String,
    pub extensions: Vec<String>,
}

/// After a press of one of a plug-in window's buttons: keep the token it made and take the window's settings as
/// it now shows them (`{"token": ..., "properties": [...]}`, the schema's form, each value as its default), in the
/// value forms of `controls`. The file the press asked for, if it prompted for one (`"pick"`).
fn take_press(
    result: &str,
    controls: &[Control],
    values: &mut Values,
) -> Result<Option<FilePick>, String> {
    let parsed: Value = serde_json::from_str(result).map_err(|e| e.to_string())?;
    match parsed.get("token").and_then(Value::as_str) {
        Some(token) => values.insert(TOKEN.into(), Value::from(token)),
        None => values.remove(TOKEN),
    };
    take_properties(&parsed, controls, values);
    let pick = parsed
        .get("pick")
        .filter(|p| p.get("kind").and_then(Value::as_str) == Some("open"));
    Ok(pick.map(|p| FilePick {
        title: p
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Open")
            .to_owned(),
        extensions: p
            .get("extensions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| {
                e.as_str()
                    .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
            })
            .collect(),
    }))
}

/// The settings a plug-in window shows (`parsed["properties"]`, as [`take_press`] reads them) into `values`.
fn take_properties(parsed: &Value, controls: &[Control], values: &mut Values) {
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
            Control::Text { .. } | Control::PluginCode { .. } => shown,
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
}

/// Turn the effect-schema JSON into paint.rs controls (and the names of choice props).
fn controls_from_schema(schema: &str) -> (Vec<Control>, Vec<&'static str>) {
    serde_json::from_str::<Value>(schema)
        .map(|parsed| controls_from_properties(&parsed))
        .unwrap_or_default()
}

/// A live window's controls from its settings (`parsed`) when they list one it did not have (a row a load brought:
/// Blur Map's file and layer lists after Open), with their choice keys. Only from settings the host shaped as the
/// dialog's rows (`"shape": "dialog"`: linked pairs as one row, repeated labels numbered); a raw list names every
/// control, which would read as new rows.
fn grown_controls(
    parsed: &Value,
    controls: &[Control],
) -> Option<(Vec<Control>, Vec<&'static str>)> {
    if parsed.get("shape").and_then(Value::as_str) != Some("dialog") {
        return None;
    }
    let grown = parsed
        .get("properties")?
        .as_array()?
        .iter()
        .filter_map(|p| p.get("name")?.as_str())
        .any(|n| controls.iter().all(|c| c.key() != n));
    grown.then(|| controls_from_properties(parsed))
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
            "canvas" => controls.push(Control::PluginCanvas {
                key,
                label,
                width: p.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                height: p.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
            }),
            "code" => controls.push(Control::PluginCode {
                key,
                label,
                width: p.get("width").and_then(Value::as_u64).unwrap_or(640) as u32,
                height: p.get("height").and_then(Value::as_u64).unwrap_or(400) as u32,
            }),
            _ => {}
        }
    }
    (controls, choice_keys)
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

/// When a live window's next running timer is due: its read-back's `due` (ms; -1 for none).
fn due(parsed: &Value) -> Option<u32> {
    parsed
        .get("due")
        .and_then(Value::as_i64)
        .and_then(|d| u32::try_from(d).ok())
}

/// What the user reads when a plug-in effect throws (rendering, or a press of its window's button): the effect left
/// the image as it was.
pub fn failed(name: &str, reason: &str) -> String {
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

/// A `.pdn` file for the managed `Document.FromStream`, read with paint.rs's own reader: little-endian width and
/// height (i32), dots per inch (f64) and layer count (i32), then bottom layer first: the name (i32 UTF-8 length, the
/// bytes), visible, background, opacity and Paint.NET's blend mode (one byte each), and straight BGRA8 pixels.
/// `None` when it is not a document paint.rs reads.
pub fn decode_document(bytes: &[u8]) -> Option<Vec<u8>> {
    let doc = crate::io::pdn::decode(bytes).ok()?;
    let mut out = Vec::new();
    out.extend((doc.width as i32).to_le_bytes());
    out.extend((doc.height as i32).to_le_bytes());
    out.extend(doc.resolution.to_le_bytes());
    out.extend((doc.layers.len() as i32).to_le_bytes());
    for (props, image) in &doc.layers {
        out.extend((props.name.len() as i32).to_le_bytes());
        out.extend(props.name.as_bytes());
        out.extend([
            props.visible as u8,
            props.is_background as u8,
            props.opacity,
            props.blend_mode.pdn_value(),
        ]);
        out.extend(
            image
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| [b, g, r, a]),
        );
    }
    Some(out)
}

/// The `.pdn` file for the managed `Document.SaveToStream`: `blob` as `decode_document` gives one, written with
/// paint.rs's writer. `None` when the blob is malformed or has no layers.
pub fn encode_document(blob: &[u8]) -> Option<Vec<u8>> {
    use crate::document::layer::{BlendMode, LayerProperties};
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = blob.get(at..at.checked_add(n)?)?;
        at += n;
        Some(s)
    };
    let i32_at = |b: &[u8]| i32::from_le_bytes(b.try_into().unwrap());
    let width = u32::try_from(i32_at(take(4)?)).ok().filter(|&w| w > 0)?;
    let height = u32::try_from(i32_at(take(4)?)).ok().filter(|&h| h > 0)?;
    let resolution = f64::from_le_bytes(take(8)?.try_into().unwrap());
    let count = usize::try_from(i32_at(take(4)?)).ok()?;
    let pixels = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let mut layers = Vec::new();
    for _ in 0..count {
        let name_len = usize::try_from(i32_at(take(4)?)).ok()?;
        let name = String::from_utf8_lossy(take(name_len)?).into_owned();
        let &[visible, background, opacity, blend] = take(4)? else {
            return None;
        };
        let mut props = LayerProperties::new(&name, visible != 0, background != 0, opacity);
        props.blend_mode = BlendMode::from_pdn_value(blend);
        let src = take(pixels)?;
        let mut image = ImageData::new(width, height);
        for (d, &[b, g, r, a]) in image
            .data
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src.as_chunks::<4>().0)
        {
            *d = [r, g, b, a];
        }
        layers.push((props, image));
    }
    let doc = crate::io::pdn::PdnDocument {
        width,
        height,
        resolution,
        layers,
        texts: Vec::new(),
        header_xml: String::new(),
    };
    crate::io::pdn::encode(&doc.to_document()).ok()
}

/// An image the browser host's managed System.Drawing saves in a format it does not write itself, written with
/// paint.rs's encoder: `image` as `decode_image` gives one (a little-endian width and height, then straight
/// BGRA8), `format` the System.Drawing format's name, `quality` 1 to 100. JPEG only, with GDI+'s 4:2:0 chroma
/// (its quality when the caller sets none, 75, is the caller's to pass); alpha is dropped, as GDI+ drops it.
/// `None` for another format or a size JPEG cannot hold.
pub fn encode_image(image: &[u8], format: &str, quality: u8) -> Option<Vec<u8>> {
    let (head, bgra) = image.split_at_checked(8)?;
    let w = u32::from_le_bytes(head[..4].try_into().ok()?);
    let h = u32::from_le_bytes(head[4..].try_into().ok()?);
    if !format.eq_ignore_ascii_case("jpeg") || bgra.len() as u64 != u64::from(w) * u64::from(h) * 4
    {
        return None;
    }
    let mut out = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
    encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
    encoder
        .encode(
            bgra,
            u16::try_from(w).ok()?,
            u16::try_from(h).ok()?,
            jpeg_encoder::ColorType::Bgra,
        )
        .ok()?;
    Some(out)
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{
    Canvas, effects, init, is_configurable_action, is_plugin, load, name_of, press, render,
    revision, set_permissions, store_network_response, unload_all,
};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{
        CanvasLayout, EffectDef, EffectEnv, ImageData, Values, canvas_image, canvas_layouts,
        problems, register, render_props, unique_key, with_window,
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
            host.set_document_host(decode_document_c, encode_document_c, free_bytes_c);
            Ok(host)
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)?;
        crate::effect::permission::send();
        Ok(())
    }

    /// The outside requests plug-ins may make (`effect::permission`'s answers), for the host to check against.
    pub fn set_permissions(json: &str) {
        if let Some(Ok(host)) = HOST.get() {
            let _ = host.set_permissions(json);
        }
    }

    /// The response to a network request the host left to paint.rs (`effect::permission::Request::key`).
    pub fn store_network_response(key: &str, json: &str) {
        if let Some(Ok(host)) = HOST.get() {
            let _ = host.store_network_response(key, json);
        }
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

    /// `f` over the bridge's C ABI: a byte buffer out that `free_bytes_c` releases; null when `f` gives none (or
    /// panics).
    unsafe fn bytes_c(
        bytes: *const u8,
        len: std::ffi::c_int,
        out_len: *mut std::ffi::c_int,
        f: fn(&[u8]) -> Option<Vec<u8>>,
    ) -> *mut u8 {
        if bytes.is_null() || len <= 0 {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        }
        let out = std::panic::catch_unwind(|| {
            f(unsafe { std::slice::from_raw_parts(bytes, len as usize) })
        });
        let Ok(Some(out)) = out else {
            unsafe { *out_len = 0 };
            return std::ptr::null_mut();
        };
        let out = out.into_boxed_slice();
        unsafe { *out_len = out.len() as std::ffi::c_int };
        Box::into_raw(out) as *mut u8
    }

    /// `decode_image` over the bridge's C ABI.
    unsafe extern "C" fn decode_image_c(
        bytes: *const u8,
        len: std::ffi::c_int,
        out_len: *mut std::ffi::c_int,
    ) -> *mut u8 {
        unsafe { bytes_c(bytes, len, out_len, super::decode_image) }
    }

    /// `decode_document` over the bridge's C ABI (`PdnSetDocumentHost`).
    unsafe extern "C" fn decode_document_c(
        bytes: *const u8,
        len: std::ffi::c_int,
        out_len: *mut std::ffi::c_int,
    ) -> *mut u8 {
        unsafe { bytes_c(bytes, len, out_len, super::decode_document) }
    }

    /// `encode_document` over the bridge's C ABI (`PdnSetDocumentHost`).
    unsafe extern "C" fn encode_document_c(
        bytes: *const u8,
        len: std::ffi::c_int,
        out_len: *mut std::ffi::c_int,
    ) -> *mut u8 {
        unsafe { bytes_c(bytes, len, out_len, super::encode_document) }
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
        let mut keys = Vec::new();
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
                keys.push(key);
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
        let file = plugin_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        super::remember_provided(&file, keys, &parsed);
        REVISION.fetch_add(1, Ordering::Relaxed);
        Ok(skipped)
    }

    /// Drop every loaded plugin (e.g. before reloading the folder).
    pub fn unload_all() {
        registry().write().unwrap().clear();
        names().write().unwrap().clear();
        super::forget_provided();
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
        super::take_press(&result, &entry.def.controls, values).map(drop)
    }

    /// Plug-in effect `id`'s own settings window kept open over `src` while its dialog shows the window's live
    /// canvas (`Control::PluginCanvas`): input reaches the window as it happens, and the effect renders with the
    /// window's token (`token`). The host keeps one open; dropping this closes it.
    pub struct Canvas {
        session: pdn_bridge::CanvasSession,
        /// When the window's next running timer is due (ms after its last read) and whether it may take a while
        /// (it has not ticked yet, or its last tick was slow), as the host reads it back.
        due: Option<(u32, bool)>,
        /// How often the window has said its token changed, as of its last read.
        revision: Option<u64>,
        choice_keys: Vec<&'static str>,
        controls: Vec<super::Control>,
        layouts: Vec<(&'static str, CanvasLayout)>,
    }

    impl Canvas {
        /// Opens the window as `values` leave it, over an owner window `window` big (the room paint.rs gives it).
        pub fn open(
            id: &str,
            src: &ImageData,
            values: &Values,
            env: &EffectEnv,
            window: [u32; 2],
        ) -> Result<Canvas, String> {
            let reg = registry().read().unwrap();
            let entry = reg.get(id).ok_or("the plug-in is not loaded")?;
            let props = with_window(
                render_props(values, &entry.choice_keys, &entry.def.controls, env),
                window,
            );
            let session = entry
                .plugin
                .canvas_open(
                    entry.dotnet_id,
                    Some(&props),
                    &src.data,
                    src.width,
                    src.height,
                )
                .map_err(|e| e.to_string())?;
            let layouts = canvas_layouts(&entry.def.controls, |key| {
                session.layout(key).ok().flatten()
            });
            Ok(Canvas {
                session,
                due: None,
                revision: None,
                choice_keys: entry.choice_keys.clone(),
                controls: entry.def.controls.clone(),
                layouts,
            })
        }

        /// Where the canvas `key` sits in the open window.
        pub fn layout(&self, key: &str) -> Option<CanvasLayout> {
            self.layouts
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, l)| *l)
        }

        /// Press the open window's button `key` as `values` leave its settings, taking its token and settings after
        /// it into `values` (as [`press`] does for a closed window), and the file it asked for. `None` from a host
        /// that cannot.
        pub fn press(
            &mut self,
            key: &str,
            values: &mut Values,
            env: &EffectEnv,
        ) -> Result<Option<Option<super::FilePick>>, String> {
            let props = render_props(values, &self.choice_keys, &self.controls, env);
            match self
                .session
                .press(Some(&props), key)
                .map_err(|e| e.to_string())?
            {
                Some(result) => {
                    self.grow(&result);
                    super::take_press(&result, &self.controls, values).map(Some)
                }
                None => Ok(None),
            }
        }

        /// Give the open window the file (`name`, `bytes`) its last press asked for. `false` from a host that cannot.
        pub fn provide_file(&self, name: &str, bytes: &[u8]) -> Result<bool, String> {
            self.session
                .provide_file(name, bytes)
                .map_err(|e| e.to_string())
        }

        /// The open window's settings now, which input to it can change, into `values`. `false` from a host that
        /// cannot.
        pub fn values(&mut self, values: &mut Values) -> Result<bool, String> {
            match self.session.values().map_err(|e| e.to_string())? {
                Some(result) => {
                    self.grow(&result);
                    let parsed: Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
                    super::take_properties(&parsed, &self.controls, values);
                    self.due = super::due(&parsed).map(|d| {
                        (
                            d,
                            parsed
                                .get("dueSlow")
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                        )
                    });
                    self.revision = parsed.get("revision").and_then(Value::as_u64);
                    Ok(true)
                }
                None => Ok(false),
            }
        }

        /// The window's controls now: the schema's, grown by rows its own input brought.
        pub fn controls(&self) -> &[super::Control] {
            &self.controls
        }

        /// When the window's next running timer is due, in ms from its last read ([`Self::values`]), and whether it
        /// may take a while; taken, so it is acted on once.
        pub fn take_due(&mut self) -> Option<(u32, bool)> {
            self.due.take()
        }

        /// How often the window has said its token changed, as of its last read ([`Self::values`]).
        pub fn revision(&self) -> Option<u64> {
            self.revision
        }

        /// Take the rows a press or a read-back (`result`) lists that the window did not have.
        fn grow(&mut self, result: &str) {
            if let Some((controls, choice_keys)) = serde_json::from_str(result)
                .ok()
                .and_then(|p| super::grown_controls(&p, &self.controls))
            {
                (self.controls, self.choice_keys) = (controls, choice_keys);
            }
        }

        /// The window's other settings as `values` leave them, then `events` (a JSON array) on the canvas `key`:
        /// what it draws after.
        pub fn input(
            &self,
            key: &str,
            events: &Value,
            values: &Values,
            env: &EffectEnv,
        ) -> Result<ImageData, String> {
            let props = render_props(values, &self.choice_keys, &self.controls, env);
            let rgba = self
                .session
                .input(Some(&props), key, &events.to_string())
                .map_err(|e| e.to_string())?;
            canvas_image(&self.layouts, key, rgba)
        }

        /// `events` (a JSON array) on the window's control `key` that is not a canvas (a code editor), as
        /// [`Self::input`] sends them, its drawing left aside.
        pub fn send(
            &self,
            key: &str,
            events: &Value,
            values: &Values,
            env: &EffectEnv,
        ) -> Result<(), String> {
            let props = render_props(values, &self.choice_keys, &self.controls, env);
            self.session
                .input(Some(&props), key, &events.to_string())
                .map(drop)
                .map_err(|e| e.to_string())
        }

        /// What the canvas `key` draws now.
        pub fn paint(&self, key: &str) -> Result<ImageData, String> {
            let rgba = self.session.paint(key).map_err(|e| e.to_string())?;
            canvas_image(&self.layouts, key, rgba)
        }

        /// The window's token now: the effect's `$token` value ([`super::TOKEN`]).
        pub fn token(&self) -> Result<String, String> {
            self.session.token().map_err(|e| e.to_string())
        }

        /// The dialog's OK: the token outlives the window, so Repeat renders with it.
        pub fn keep(&self) -> Result<(), String> {
            self.session.keep().map_err(|e| e.to_string())
        }
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
    Canvas, PluginJob, effects, filetype_open, filetype_save, install, is_configurable_action,
    is_plugin, name_of, press, render, render_begin, revision, set_permissions,
    store_network_response, unload_all,
};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{
        CanvasLayout, Control, EffectDef, EffectEnv, ImageData, Values, canvas_image,
        canvas_layouts, problems, register, render_props, unique_key, with_window,
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
            encode_image: &JsValue,
            decode_document: &JsValue,
            encode_document: &JsValue,
            font_names: Vec<String>,
            font_files: Vec<js_sys::Uint8Array>,
            font_urls: Vec<String>,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnSetInstalled)]
        async fn pdn_set_installed(
            names: Vec<String>,
            files: Vec<js_sys::Uint8Array>,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnLoadInstalled)]
        fn pdn_load_installed(file_name: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnSetPermissions)]
        fn pdn_set_permissions(json: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnStoreNetworkResponse)]
        fn pdn_store_network_response(key: &str, json: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnEnumerate)]
        fn pdn_enumerate(handle: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnEffectSchema)]
        fn pdn_effect_schema(handle: i32, id: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasOpen)]
        fn pdn_canvas_open(
            handle: i32,
            id: &str,
            props: Option<String>,
            bgra: &[u8],
            width: i32,
            height: i32,
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasInput)]
        fn pdn_canvas_input(
            session: i32,
            props: Option<String>,
            key: &str,
            events: &str,
        ) -> Result<Vec<u8>, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasPaint)]
        fn pdn_canvas_paint(session: i32, key: &str) -> Result<Vec<u8>, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasToken)]
        fn pdn_canvas_token(session: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasLayout)]
        fn pdn_canvas_layout(session: i32, key: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasPress)]
        fn pdn_canvas_press(session: i32, props: &str, key: &str) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasValues)]
        fn pdn_canvas_values(session: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasProvideFile)]
        fn pdn_canvas_provide_file(
            session: i32,
            name: &str,
            bytes: &[u8],
        ) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasKeep)]
        fn pdn_canvas_keep(session: i32) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(catch, js_name = pdnCanvasClose)]
        fn pdn_canvas_close(session: i32) -> Result<JsValue, JsValue>;
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
        pub fn finish(mut self) -> Result<Vec<u8>, String> {
            let job = std::mem::take(&mut self.job);
            let mut rgba = js_sys::Uint8Array::new(&pdn_render_finish(job).map_err(msg)?).to_vec();
            swap_rb(&mut rgba);
            Ok(rgba)
        }
        /// Abandon the job without finishing.
        pub fn cancel(self) {}
    }

    /// A job dropped unfinished (Cancel, its dialog closed, its preview superseded) is cancelled, so a render running
    /// in the host's worker stops at once instead of finishing for nobody.
    impl Drop for PluginJob {
        fn drop(&mut self) {
            if self.job != 0 {
                let _ = pdn_render_cancel_job(self.job);
            }
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
        static ENCODE_IMAGE: JsValue = Closure::<dyn Fn(Vec<u8>, String, u8) -> Option<Vec<u8>>>::new(
            |image: Vec<u8>, format: String, quality: u8| super::encode_image(&image, &format, quality),
        )
        .into_js_value();
        static DECODE_DOCUMENT: JsValue =
            Closure::<dyn Fn(Vec<u8>) -> Option<Vec<u8>>>::new(|bytes: Vec<u8>| super::decode_document(&bytes))
                .into_js_value();
        static ENCODE_DOCUMENT: JsValue =
            Closure::<dyn Fn(Vec<u8>) -> Option<Vec<u8>>>::new(|blob: Vec<u8>| super::encode_document(&blob))
                .into_js_value();
    }

    /// paint.rs's own faces, which the host's DirectWrite (OpenD2D, for GPU plug-ins' text) finds in its font
    /// folder: every bundled family, plus the interface's Selawik and Carlito. Each face is handed over as either
    /// the bytes paint.rs already holds or, for the rest, the address of the shipped `font/<path>` asset for the
    /// host to fetch as it boots (so a session that never opens a plug-in downloads none of them, and the browser
    /// cache — shared with the interface's own font list — serves repeat loads). Returns parallel (file name, bytes,
    /// address) lists: an empty address means the bytes are supplied, an empty byte array means fetch the address.
    fn font_files() -> (Vec<String>, Vec<js_sys::Uint8Array>, Vec<String>) {
        use crate::fonts::{CARLITO_REGULAR, FaceStyle, SELAWIK_BOLD, SELAWIK_REGULAR};
        let mut names = Vec::new();
        let mut files = Vec::new();
        let mut urls = Vec::new();
        for (family, path) in crate::fonts::BUNDLED {
            names.push(format!("{family}.ttf"));
            match crate::fonts::registered_face(family, FaceStyle::Regular) {
                Some(bytes) => {
                    files.push(js_sys::Uint8Array::from(&bytes[..]));
                    urls.push(String::new());
                }
                None => {
                    files.push(js_sys::Uint8Array::new_with_length(0));
                    urls.push(format!("font/{path}"));
                }
            }
        }
        for (name, bytes) in [
            ("Selawik.ttf", SELAWIK_REGULAR),
            ("Selawik Bold.ttf", SELAWIK_BOLD),
            ("Carlito.ttf", CARLITO_REGULAR),
        ] {
            names.push(name.into());
            files.push(js_sys::Uint8Array::from(bytes));
            urls.push(String::new());
        }
        (names, files, urls)
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
        let (font_names, font_files, font_urls) = font_files();
        pdn_boot(
            &TEXT_OUTLINE.with(Clone::clone),
            &DECODE_IMAGE.with(Clone::clone),
            &ENCODE_IMAGE.with(Clone::clone),
            &DECODE_DOCUMENT.with(Clone::clone),
            &ENCODE_DOCUMENT.with(Clone::clone),
            font_names,
            font_files,
            font_urls,
        )
        .await
        .map_err(msg)?;
        crate::effect::permission::send();
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
        let mut keys = Vec::new();
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
                keys.push(key);
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
        super::remember_provided(file_name, keys, &parsed);
        Ok(skipped)
    }

    /// The outside requests plug-ins may make (`effect::permission`'s answers), for the host to check against: sent
    /// once it has booted (install sends them again right after the boot).
    pub fn set_permissions(json: &str) {
        let _ = pdn_set_permissions(json);
    }

    /// The response to a network request the host left to paint.rs (`effect::permission::Request::key`).
    pub fn store_network_response(key: &str, json: &str) {
        let _ = pdn_store_network_response(key, json);
    }

    pub fn unload_all() {
        REGISTRY.with(|r| r.borrow_mut().clear());
        NAMES.with(|n| n.borrow_mut().clear());
        super::forget_provided();
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
        super::take_press(&result, &controls, values).map(drop)
    }

    /// Plug-in effect `id`'s own settings window kept open over `src` while its dialog shows the window's live
    /// canvas (`Control::PluginCanvas`): input reaches the window as it happens, and the effect renders with the
    /// window's token (`token`). The host keeps one open; dropping this closes it.
    pub struct Canvas {
        session: i32,
        /// When the window's next running timer is due (ms after its last read) and whether it may take a while
        /// (it has not ticked yet, or its last tick was slow), as the host reads it back.
        due: Option<(u32, bool)>,
        /// How often the window has said its token changed, as of its last read.
        revision: Option<u64>,
        choice_keys: Vec<&'static str>,
        controls: Vec<Control>,
        layouts: Vec<(&'static str, CanvasLayout)>,
    }

    impl Canvas {
        /// Opens the window as `values` leave it, over an owner window `window` big (the room paint.rs gives it).
        pub fn open(
            id: &str,
            src: &ImageData,
            values: &Values,
            env: &EffectEnv,
            window: [u32; 2],
        ) -> Result<Canvas, String> {
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
            let props = with_window(render_props(values, &choice_keys, &controls, env), window);
            let mut bgra = src.data.clone();
            swap_rb(&mut bgra);
            let session = pdn_canvas_open(
                handle,
                dotnet_id,
                Some(props),
                &bgra,
                src.width as i32,
                src.height as i32,
            )
            .map_err(msg)?
            .as_f64()
            .filter(|&s| s > 0.0)
            .ok_or("the settings window did not open")? as i32;
            let layouts = canvas_layouts(&controls, |key| {
                pdn_canvas_layout(session, key)
                    .ok()
                    .and_then(|v| v.as_string())
            });
            Ok(Canvas {
                session,
                due: None,
                revision: None,
                choice_keys,
                controls,
                layouts,
            })
        }

        /// Where the canvas `key` sits in the open window.
        pub fn layout(&self, key: &str) -> Option<CanvasLayout> {
            self.layouts
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, l)| *l)
        }

        /// Press the open window's button `key` as `values` leave its settings, taking its token and settings after
        /// it into `values` (as [`press`] does for a closed window). `false` from a host that cannot.
        pub fn press(
            &mut self,
            key: &str,
            values: &mut Values,
            env: &EffectEnv,
        ) -> Result<Option<Option<super::FilePick>>, String> {
            let props = render_props(values, &self.choice_keys, &self.controls, env);
            let result = pdn_canvas_press(self.session, &props, key).map_err(msg)?;
            match result.as_string() {
                Some(result) => {
                    self.grow(&result);
                    super::take_press(&result, &self.controls, values).map(Some)
                }
                None => Ok(None),
            }
        }

        /// Give the open window the file (`name`, `bytes`) its last press asked for. `false` from a host that cannot.
        pub fn provide_file(&self, name: &str, bytes: &[u8]) -> Result<bool, String> {
            Ok(pdn_canvas_provide_file(self.session, name, bytes)
                .map_err(msg)?
                .as_bool()
                == Some(true))
        }

        /// The open window's settings now, which input to it can change, into `values`. `false` from a host that
        /// cannot.
        pub fn values(&mut self, values: &mut Values) -> Result<bool, String> {
            let result = pdn_canvas_values(self.session).map_err(msg)?;
            match result.as_string() {
                Some(result) => {
                    self.grow(&result);
                    let parsed: Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
                    super::take_properties(&parsed, &self.controls, values);
                    self.due = super::due(&parsed).map(|d| {
                        (
                            d,
                            parsed
                                .get("dueSlow")
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                        )
                    });
                    self.revision = parsed.get("revision").and_then(Value::as_u64);
                    Ok(true)
                }
                None => Ok(false),
            }
        }

        /// The window's controls now: the schema's, grown by rows its own input brought.
        pub fn controls(&self) -> &[Control] {
            &self.controls
        }

        /// When the window's next running timer is due, in ms from its last read ([`Self::values`]), and whether it
        /// may take a while; taken, so it is acted on once.
        pub fn take_due(&mut self) -> Option<(u32, bool)> {
            self.due.take()
        }

        /// How often the window has said its token changed, as of its last read ([`Self::values`]).
        pub fn revision(&self) -> Option<u64> {
            self.revision
        }

        /// Take the rows a press or a read-back (`result`) lists that the window did not have.
        fn grow(&mut self, result: &str) {
            if let Some((controls, choice_keys)) = serde_json::from_str(result)
                .ok()
                .and_then(|p| super::grown_controls(&p, &self.controls))
            {
                (self.controls, self.choice_keys) = (controls, choice_keys);
            }
        }

        /// The window's other settings as `values` leave them, then `events` (a JSON array) on the canvas `key`:
        /// what it draws after.
        pub fn input(
            &self,
            key: &str,
            events: &Value,
            values: &Values,
            env: &EffectEnv,
        ) -> Result<ImageData, String> {
            let props = render_props(values, &self.choice_keys, &self.controls, env);
            let mut rgba = pdn_canvas_input(self.session, Some(props), key, &events.to_string())
                .map_err(msg)?;
            swap_rb(&mut rgba);
            canvas_image(&self.layouts, key, rgba)
        }

        /// `events` (a JSON array) on the window's control `key` that is not a canvas (a code editor), as
        /// [`Self::input`] sends them, its drawing left aside.
        pub fn send(
            &self,
            key: &str,
            events: &Value,
            values: &Values,
            env: &EffectEnv,
        ) -> Result<(), String> {
            let props = render_props(values, &self.choice_keys, &self.controls, env);
            pdn_canvas_input(self.session, Some(props), key, &events.to_string())
                .map(drop)
                .map_err(msg)
        }

        /// What the canvas `key` draws now.
        pub fn paint(&self, key: &str) -> Result<ImageData, String> {
            let mut rgba = pdn_canvas_paint(self.session, key).map_err(msg)?;
            swap_rb(&mut rgba);
            canvas_image(&self.layouts, key, rgba)
        }

        /// The window's token now: the effect's `$token` value ([`super::TOKEN`]).
        pub fn token(&self) -> Result<String, String> {
            pdn_canvas_token(self.session)
                .map_err(msg)?
                .as_string()
                .ok_or_else(|| "the window gave no token".into())
        }

        /// The dialog's OK: the token outlives the window, so Repeat renders with it.
        pub fn keep(&self) -> Result<(), String> {
            pdn_canvas_keep(self.session).map(|_| ()).map_err(msg)
        }
    }

    impl Drop for Canvas {
        fn drop(&mut self) {
            let _ = pdn_canvas_close(self.session);
        }
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

    /// A live window's settings, shaped as the dialog's rows, that list a row it did not have (a list a loaded file
    /// filled) give its new controls; the same rows again, or a raw list, give none.
    #[test]
    fn a_window_grows_the_rows_its_input_brought() {
        let at_open = serde_json::json!({ "properties": [
            { "name": "w:0.1.", "type": "button", "label": "Open" },
        ]});
        let (controls, _) = super::controls_from_properties(&at_open);
        assert!(super::grown_controls(&at_open, &controls).is_none());
        let rows = serde_json::json!([
            { "name": "w:0.0.", "type": "choice", "label": "Map File", "choices": ["a.png"], "default": 0 },
            { "name": "w:0.1.", "type": "button", "label": "Open" },
        ]);
        let raw = serde_json::json!({ "properties": rows });
        assert!(super::grown_controls(&raw, &controls).is_none());
        let loaded = serde_json::json!({ "shape": "dialog", "properties": rows });
        let (grown, choice_keys) = super::grown_controls(&loaded, &controls).unwrap();
        assert_eq!(
            grown.iter().map(|c| c.key()).collect::<Vec<_>>(),
            ["w:0.0.", "w:0.1."]
        );
        assert_eq!(choice_keys, ["w:0.0."]);
    }

    /// A press that opened the window's own file prompt says which files it wants; one that did not, nothing.
    #[test]
    fn a_press_that_prompts_for_a_file_says_which() {
        let mut values = super::Values::new();
        let pressed = r#"{"token":"mem:1","properties":[],"pick":{"kind":"open","title":"Open","extensions":[".PNG","jpg"]}}"#;
        let pick = super::take_press(pressed, &[], &mut values).unwrap();
        assert_eq!(
            pick,
            Some(super::FilePick {
                title: "Open".into(),
                extensions: vec!["png".into(), "jpg".into()]
            })
        );
        assert_eq!(values[super::TOKEN], "mem:1");
        assert_eq!(
            super::take_press(r#"{"token":"mem:2","properties":[]}"#, &[], &mut values).unwrap(),
            None
        );
    }

    /// A plug-in window's custom-drawn control is a live canvas of its size with no value of its own, and what the
    /// host draws for it is taken only at that size.
    #[test]
    fn a_windows_canvas_is_a_live_canvas_of_its_size() {
        let schema = serde_json::json!({ "properties": [
            { "name": "w:3.canvas", "type": "canvas", "label": "Canvas", "width": 3, "height": 2, "default": [] },
        ]});
        let (controls, _) = super::controls_from_properties(&schema);
        assert!(matches!(
            controls[0],
            super::Control::PluginCanvas {
                key: "w:3.canvas",
                width: 3,
                height: 2,
                ..
            }
        ));
        assert_eq!(controls[0].default_value(), serde_json::json!([]));
        // Without the host's layout the schema's size; with it, the canvas's size in the open window.
        let layouts = super::canvas_layouts(&controls, |_| None);
        let image = super::canvas_image(&layouts, "w:3.canvas", vec![7; 24]).unwrap();
        assert_eq!((image.width, image.height, image.data.len()), (3, 2, 24));
        assert!(super::canvas_image(&layouts, "w:3.canvas", vec![7; 20]).is_err());
        assert!(super::canvas_image(&layouts, "w:9.canvas", vec![7; 24]).is_err());
        let live = super::canvas_layouts(&controls, |_| Some("800,600,30,25,5,4".into()));
        let expected = super::CanvasLayout {
            window: (800, 600),
            at: (30, 25),
            size: (5, 4),
        };
        assert_eq!(live, [("w:3.canvas", expected)]);
        assert_eq!(
            super::canvas_image(&live, "w:3.canvas", vec![7; 80])
                .unwrap()
                .width,
            5
        );
        assert!(
            super::canvas_layouts(&controls, |_| Some("800,600,1,2".into()))[0]
                .1
                .size
                == (3, 2)
        );
        let props = super::with_window(r#"{"$environment":{"primary":1}}"#.into(), [640, 480]);
        let props: Value = serde_json::from_str(&props).unwrap();
        assert_eq!(
            props["$environment"],
            serde_json::json!({"primary": 1, "window": [640, 480]})
        );
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

    /// A document blob (the managed Document's layers) written as a .pdn reads back as the same blob.
    #[test]
    fn document_blob_round_trips_through_pdn() {
        let mut blob = Vec::new();
        blob.extend(3i32.to_le_bytes());
        blob.extend(2i32.to_le_bytes());
        blob.extend(150f64.to_le_bytes());
        blob.extend(2i32.to_le_bytes());
        for (name, flags, base) in [
            ("Background", [1u8, 1, 255, 0], 10u8),
            ("Ink é", [0, 0, 128, 12], 100),
        ] {
            blob.extend((name.len() as i32).to_le_bytes());
            blob.extend(name.as_bytes());
            blob.extend(flags);
            blob.extend((0..24).map(|i| base + i as u8));
        }
        let pdn = super::encode_document(&blob).expect("encodes");
        assert!(pdn.starts_with(b"PDN3"));
        assert_eq!(super::decode_document(&pdn).expect("decodes"), blob);
        assert_eq!(
            super::encode_document(&blob[..blob.len() - 1]),
            None,
            "a short blob is refused"
        );
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

    /// The host's System.Drawing saving a JPEG: what `encode_image` writes is a JPEG of the same size that reads
    /// back close to the original; other formats and short buffers give nothing.
    #[test]
    fn encode_image_writes_a_jpeg_that_reads_back() {
        let (w, h) = (16u32, 8u32);
        let mut image = [w.to_le_bytes(), h.to_le_bytes()].concat();
        for i in 0..w * h {
            image.extend([40, 120, (100 + i % 16 * 4) as u8, 255]);
        }
        let jpeg = super::encode_image(&image, "Jpeg", 75).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let back = super::decode_image(&jpeg).unwrap();
        assert_eq!(&back[..8], &image[..8]);
        let worst = back[8..]
            .iter()
            .zip(&image[8..])
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst < 24, "{worst}");
        assert!(super::encode_image(&image, "Gif", 75).is_none());
        assert!(super::encode_image(&image[..20], "Jpeg", 75).is_none());
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
