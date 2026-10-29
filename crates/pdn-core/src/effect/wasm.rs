//! Installed WebAssembly plug-ins (`extra::plugins`, PhotoCraft's ABI v1) and LightCraft looks
//! (`.cube`/`.3dl` LUTs and `.xmp` develop presets) as effects: each one is an Effects ▸ Plugins
//! item. A plug-in's dialog is built from its manifest's parameters (a number or an int is a
//! slider, a bool a checkbox, a choice a drop-down) and its render runs the module; a LUT applies
//! its table with an Amount slider; a preset runs Camera Raw with its settings, at once. The menus
//! mark them with a puzzle piece, as third-party content, where built-in plugins carry
//! Paint.NET's gray plus.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use serde_json::Value;

use super::bb::manifest::PluginItem;
use super::{Category, Control, EffectDef, EffectEnv, EffectKind, Values, range};
use crate::action::ActionDef;
use crate::canvas::ImageData;
use crate::extra::fx::lut::LutFile;
use crate::extra::plugins::{ParamSpec, Plugin, registry};

/// The Effects submenu the plug-ins live in.
pub const CATEGORY: &str = "plugins";
/// Their menu icon: none of their own (the puzzle piece marks them instead).
const ICON: &str = "blank_icon.png";

/// One installed plug-in's effect, menu row and action.
struct Installed {
    def: EffectDef,
    item: &'static PluginItem,
    action: &'static ActionDef,
}

/// The entries as of the plug-in registry's and the looks' revisions, rebuilt only when they
/// change.
// ponytail: a rebuild leaks each entry's strings again (EffectDef and the menus want 'static); a
// few hundred bytes per entry per install or removal.
#[cfg(not(any(test, feature = "testing")))]
static INSTALLED: std::sync::Mutex<((u64, u64), Vec<Installed>)> =
    std::sync::Mutex::new(((u64::MAX, 0), Vec::new()));

/// An installed LightCraft look.
#[derive(Clone)]
enum Look {
    Table(Arc<LutFile>),
    /// Camera Raw's values with the preset loaded over its defaults.
    Preset(Arc<Values>),
}

/// The installed looks by effect id (`registry::key`): their name and content.
static LOOKS: RwLock<BTreeMap<String, (String, Look)>> = RwLock::new(BTreeMap::new());
static LOOKS_REVISION: AtomicU64 = AtomicU64::new(0);

const CAMERA_RAW: &str = "pcCameraRaw";

fn install(id: &str, name: &str, look: Look) {
    let mut looks = LOOKS.write().unwrap_or_else(|e| e.into_inner());
    looks.insert(registry::key(id), (name.into(), look));
    LOOKS_REVISION.fetch_add(1, Ordering::Relaxed);
}

/// Installs LUT look `id` (an effect id, `lut.…`) named `name`, replacing one with the same id.
pub fn install_look(id: &str, name: &str, table: LutFile) {
    install(id, name, Look::Table(Arc::new(table)));
}

/// Installs Camera Raw develop preset `bytes` (`.xmp`) as look `id` named `name`; an error when
/// Camera Raw cannot read it.
pub fn install_preset(id: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
    let def = super::find(CAMERA_RAW).ok_or("Camera Raw is unavailable")?;
    let load = def
        .indirect
        .and_then(|ui| ui.preset)
        .ok_or("Camera Raw reads no presets")?;
    let mut values = super::defaults(&def);
    (load.load)(bytes, &mut values)?;
    install(id, name, Look::Preset(Arc::new(values)));
    Ok(())
}

/// Uninstalls look `id`; false if it wasn't installed.
pub fn remove_look(id: &str) -> bool {
    let gone = LOOKS
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&registry::key(id))
        .is_some();
    if gone {
        LOOKS_REVISION.fetch_add(1, Ordering::Relaxed);
    }
    gone
}

fn leak(s: impl Into<String>) -> &'static str {
    String::leak(s.into())
}

/// `scale_factor` or `blurRadius` → "Scale factor", "Blur radius".
fn label(key: &str) -> String {
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        match c {
            '_' => out.push(' '),
            c if c.is_ascii_uppercase() && i > 0 => {
                out.push(' ');
                out.push(c.to_ascii_lowercase());
            }
            c if i == 0 => out.extend(c.to_uppercase()),
            c => out.push(c),
        }
    }
    out
}

fn control(key: &str, spec: &ParamSpec) -> Control {
    let (key, label) = (leak(key), leak(label(key)));
    match spec {
        ParamSpec::Number {
            min, max, default, ..
        } => range(key, label, *min, *max, default.clamp(*min, *max))
            .step(0.01)
            .slider_keys(2, 0.01, 0.1, false),
        ParamSpec::Int {
            min, max, default, ..
        } => range(
            key,
            label,
            *min as f64,
            *max as f64,
            (*default).clamp(*min, *max) as f64,
        ),
        ParamSpec::Bool { default } => Control::Checkbox {
            key,
            label,
            default: *default,
        },
        ParamSpec::Choice { options, default } => {
            let choices: Vec<(&'static str, &'static str)> = options
                .iter()
                .map(|o| {
                    let o = leak(o.as_str());
                    (o, o)
                })
                .collect();
            let default = default.as_deref().unwrap_or(&options[0]);
            Control::Select {
                key,
                label,
                default: choices.iter().find(|c| c.0 == default).map_or("", |c| c.0),
                choices: Vec::leak(choices),
            }
        }
    }
}

fn plugin_entry(plugin: &Plugin) -> Installed {
    let m = plugin.manifest();
    let controls = m.params.iter().map(|(k, s)| control(k, s)).collect();
    entry(&m.id, &m.name, controls)
}

fn entry(id: &str, name: &str, controls: Vec<Control>) -> Installed {
    let (id, name) = (leak(id), leak(name));
    let item = Box::leak(Box::new(PluginItem {
        id,
        name,
        category: Category::Effect(CATEGORY),
        configurable: !controls.is_empty(),
        icon: ICON,
    }));
    let action = Box::leak(Box::new(ActionDef {
        id: leak(format!("menu.effects.{id}")),
        name_key: None,
        label: Some(name),
        tooltip_key: None,
        default_shortcut: None,
        icon: Some(ICON),
        category: "Effects",
    }));
    let def = EffectDef {
        id,
        icon: ICON,
        category: Category::Effect(CATEGORY),
        controls,
        kind: EffectKind::Plugin,
        dialog: None,
        dialog_title: Some(name),
        indirect: None,
    };
    Installed { def, item, action }
}

fn build() -> Vec<Installed> {
    let looks = LOOKS.read().unwrap_or_else(|e| e.into_inner());
    let looks = looks
        .iter()
        .filter(|(k, _)| registry::mine(k))
        .map(|(k, (name, look))| {
            let controls = match look {
                Look::Table(_) => vec![range("amount", "Amount", 0.0, 100.0, 100.0)],
                Look::Preset(_) => vec![],
            };
            entry(registry::id_of(k), name, controls)
        });
    let plugins = registry::list();
    plugins
        .iter()
        .map(|p| plugin_entry(p))
        .chain(looks)
        .collect()
}

fn with<T>(f: impl FnOnce(&[Installed]) -> T) -> T {
    // A test build's entries differ per thread (`registry::key`): no shared cache.
    #[cfg(any(test, feature = "testing"))]
    return f(&build());
    #[cfg(not(any(test, feature = "testing")))]
    {
        let mut cache = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        let revision = (registry::revision(), LOOKS_REVISION.load(Ordering::Relaxed));
        if cache.0 != revision {
            *cache = (revision, build());
        }
        f(&cache.1)
    }
}

/// The installed plug-ins' effects.
pub fn effects() -> Vec<EffectDef> {
    with(|all| all.iter().map(|i| i.def.clone()).collect())
}

/// The menu row of installed plug-in `effect_id`.
pub fn manifest(effect_id: &str) -> Option<&'static PluginItem> {
    with(|all| all.iter().find(|i| i.def.id == effect_id).map(|i| i.item))
}

/// The action of installed plug-in menu item `action_id` (`menu.effects.<id>`).
pub fn action(action_id: &str) -> Option<&'static ActionDef> {
    with(|all| {
        all.iter()
            .find(|i| i.action.id == action_id)
            .map(|i| i.action)
    })
}

/// Whether effect `effect_id` is an installed plug-in (third-party code: the puzzle mark).
pub fn is_plugin(effect_id: &str) -> bool {
    manifest(effect_id).is_some()
}

/// Runs installed plug-in or look `id` on the layer. A failure (or one no longer installed) leaves
/// the layer as it was and says why through the render's notice; a cancelled run says nothing.
pub fn render(id: &str, src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    let look = LOOKS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&registry::key(id))
        .cloned();
    match look.map(|(_, l)| l) {
        Some(Look::Table(table)) => {
            return apply_look(&table, src, super::num(values, "amount") / 100.0);
        }
        // Camera Raw's own render, unclipped: `effect::apply` clips this result once.
        Some(Look::Preset(preset)) => {
            return match super::find(CAMERA_RAW).map(|d| d.kind) {
                Some(EffectKind::Cpu { render, .. }) => render(src, &preset, env),
                _ => src.clone(),
            };
        }
        None => {}
    }
    let Some(plugin) = registry::get(id) else {
        notice(env, format!("The plug-in {id} is no longer installed."));
        return src.clone();
    };
    let params = Value::Object(values.clone());
    match plugin.apply(src, env.bounds, &params, env.cancel) {
        Ok(out) => out,
        Err(crate::extra::plugins::Error::Cancelled) => src.clone(),
        Err(e) => {
            notice(env, format!("{}: {e}", plugin.manifest().name));
            src.clone()
        }
    }
}

/// `table` over the layer (tetrahedral, no dither), mixed with it by `amount` (0–1).
fn apply_look(table: &LutFile, src: &ImageData, amount: f64) -> ImageData {
    use crate::extra::shim::{from_f32, to_f32};
    let orig = to_f32(src);
    let mut buf = orig.clone();
    table.apply(&mut buf, src.width as usize, true, false);
    let k = amount.clamp(0.0, 1.0) as f32;
    for (v, o) in buf.iter_mut().zip(&orig) {
        *v = o + (*v - o) * k;
    }
    from_f32(&buf, src.width, src.height)
}

fn notice(env: &EffectEnv, message: String) {
    if let Some(n) = env.notice {
        *n.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px};
    use crate::effect::{EffectEnv, apply, defaults, find};
    use serde_json::json;

    /// A plug-in with one parameter of each kind that fills every sample with 3.0 / 10.
    fn plugin(id: &str) -> Vec<u8> {
        let manifest = format!(
            r#"{{"id":"{id}","name":"Fill Test","kind":"filter","params":{{"amount":{{"type":"number","min":0,"max":10,"default":5}},"radius":{{"type":"int","min":1,"max":9,"default":3}},"mono":{{"type":"bool","default":true}},"mode":{{"type":"choice","options":["soft","hard"],"default":"hard"}}}}}}"#
        );
        let escaped = manifest.replace('"', "\\\"");
        let fill = r#"(local $i i32) (local $v f32)
          (local.set $v (f32.div (f32.load (i32.const 8)) (f32.const 10)))
          (block $done (loop $l
            (br_if $done (i32.ge_u (local.get $i) (local.get $len)))
            (f32.store (i32.add (local.get $buf) (local.get $i)) (local.get $v))
            (local.set $i (i32.add (local.get $i) (i32.const 4)))
            (br $l)))
          (i32.const 0)"#;
        let src = format!(
            r#"(module (memory (export "memory") 1)
              (data (i32.const 8) "\00\00\40\40")
              (data (i32.const 64) "{escaped}")
              (func (export "pc_abi_version") (result i32) (i32.const 1))
              (func (export "pc_manifest") (result i64)
                (i64.or (i64.shl (i64.const {len}) (i64.const 32)) (i64.const 64)))
              (func (export "pc_alloc") (param $n i32) (result i32) (local $old i32)
                (local.set $old (memory.grow (i32.shr_u (i32.add (local.get $n) (i32.const 65535)) (i32.const 16))))
                (if (result i32) (i32.eq (local.get $old) (i32.const -1))
                  (then (i32.const 0)) (else (i32.mul (local.get $old) (i32.const 65536)))))
              (func (export "pc_filter") (param $buf i32) (param $len i32) (param $w i32) (param $h i32)
                (param $ch i32) (param $fmt i32) (param $pp i32) (param $pl i32) (result i32) {fill}))"#,
            len = manifest.len()
        );
        wat::parse_str(&src).unwrap()
    }

    /// An installed plug-in is an Effects ▸ Plugins effect and action with its parameters as
    /// controls; it runs through `effect::apply`, clipped to the selection like any effect.
    #[test]
    fn an_installed_plugin_is_an_effect() {
        let id = "test.wasm.effect";
        registry::install_bytes(&plugin(id)).unwrap();
        let def = find(id).expect("registered");
        assert_eq!(def.category, Category::Effect(CATEGORY));
        let kinds: Vec<_> = def
            .controls
            .iter()
            .map(|c| match c {
                Control::Range { key, step, .. } => format!("{key}:range:{step}"),
                Control::Checkbox { key, default, .. } => format!("{key}:checkbox:{default}"),
                Control::Select { key, default, .. } => format!("{key}:select:{default}"),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "amount:range:0.01",
                "radius:range:1",
                "mono:checkbox:true",
                "mode:select:hard"
            ]
        );
        let action = crate::action::get(&format!("menu.effects.{id}")).unwrap();
        assert_eq!(action.label, Some("Fill Test"));
        assert!(is_plugin(id) && crate::effect::is_configurable_action(action.id));
        assert_eq!(crate::effect::display_name_of(id), "Fill Test");

        // The module writes 3.0 / 10 = 0.3 → 77 into every sample.
        let src = from_fn(8, 4, |x, _| [x as u8 * 30, 0, 0, 255]);
        let env = EffectEnv::whole(8, 4);
        let out = apply(&src, &def, &defaults(&def), &env);
        assert_eq!(px(&out, 2, 1), [77, 77, 77, 77]);
        registry::remove(id);
        assert!(find(id).is_none() && !is_plugin(id));
    }

    /// A failing plug-in leaves the layer and says why; a missing one too.
    #[test]
    fn a_failure_leaves_the_layer() {
        let src = from_fn(4, 4, |x, y| [x as u8, y as u8, 9, 255]);
        let notice = std::sync::Mutex::new(None);
        let env = EffectEnv {
            notice: Some(&notice),
            ..EffectEnv::whole(4, 4)
        };
        assert_eq!(render("test.wasm.missing", &src, &Values::new(), &env), src);
        assert!(
            notice
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("no longer installed")
        );
    }

    /// An installed develop preset runs Camera Raw with its settings, with no dialog.
    #[test]
    fn an_installed_preset_is_an_instant_effect() {
        let id = "preset.brighter";
        let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+1.00"/></rdf:RDF></x:xmpmeta>"#;
        install_preset(id, "Brighter", xmp.as_bytes()).unwrap();
        assert!(install_preset("preset.bad", "Bad", b"not xmp").is_err());
        let def = find(id).expect("registered");
        assert!(!def.is_configurable());
        let src = from_fn(4, 4, |_, _| [80, 80, 80, 255]);
        let out = apply(&src, &def, &defaults(&def), &EffectEnv::whole(4, 4));
        assert!(px(&out, 1, 1)[0] > 100, "{:?}", px(&out, 1, 1));
        assert!(remove_look(id));
    }

    /// An installed look applies its table, mixed by Amount.
    #[test]
    fn an_installed_look_is_an_effect() {
        let id = "lut.test-invert";
        let cube = "LUT_3D_SIZE 2\n1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n";
        install_look(
            id,
            "Invert Look",
            crate::extra::fx::lut::parse("x.cube", cube.as_bytes()).unwrap(),
        );
        let def = find(id).expect("registered");
        assert_eq!(crate::effect::display_name_of(id), "Invert Look");
        let src = from_fn(2, 1, |x, _| [x as u8 * 255, 0, 0, 255]);
        let env = EffectEnv::whole(2, 1);
        let full = apply(&src, &def, &defaults(&def), &env);
        assert_eq!(px(&full, 0, 0), [255, 255, 255, 255]);
        let none = apply(
            &src,
            &def,
            &json!({"amount": 0}).as_object().unwrap().clone(),
            &env,
        );
        assert_eq!(none, src);
        assert!(remove_look(id) && find(id).is_none());
    }
}
