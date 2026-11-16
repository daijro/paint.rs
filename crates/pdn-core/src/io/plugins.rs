//! File formats contributed by Paint.NET FileType plugins, loaded through the .NET host (the
//! desktop's CoreCLR bridge or the browser's Mono host). `detect_format`/`decode`/`encode` consult
//! this registry after the built-ins, so a plugin format opens and saves through the same code
//! paths as a native one, and the Open and Save dialogs list it ([`formats`]). A plugin never
//! shadows a built-in format (built-in extensions are skipped at registration). A format's save
//! options are its FileType's property collection, as [`Control`]s.

use crate::canvas::ImageData;
use crate::effect::{Control, Values};
use crate::io::codecs::{CodecError, Decoded};
use serde_json::Value;

/// Built-in extensions (from `detect_format`); a plugin never shadows these.
const BUILTIN: &[&str] = &[
    "pdn", "png", "jpg", "jpeg", "jpe", "gif", "bmp", "tif", "tiff", "tga", "webp", "dds", "jxl",
    "avif", "heic", "heif", "jxr", "wdp", "wmp", "psd", "psb",
];

/// A plugin format as the file dialogs show it.
#[derive(Clone, Copy, Debug)]
pub struct PluginFormat {
    /// The first non-built-in extension, without the dot.
    pub id: &'static str,
    pub name: &'static str,
    /// Without dots, the id first.
    pub extensions: &'static [&'static str],
    /// The save options (empty: none, or a load-only format).
    pub controls: &'static [Control],
    pub can_save: bool,
}

/// One registered format and how to reach it: the host's plugin (`P`) and FileType index.
struct Fmt<P> {
    info: PluginFormat,
    choice_keys: Vec<&'static str>,
    plugin: P,
    index: u32,
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Each FileType of a plugin's `enumerate` JSON whose extensions are not all built-in, with its
/// FileType index.
fn parse(enumerated: &Value) -> Vec<(PluginFormat, Vec<&'static str>, u32)> {
    let Some(fts) = enumerated.get("fileTypes").and_then(Value::as_array) else {
        return vec![];
    };
    let mut out = vec![];
    for (index, ft) in fts.iter().enumerate() {
        let list = |k: &str| -> Vec<String> {
            ft.get(k)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty() && !BUILTIN.contains(&e.as_str()))
                .collect()
        };
        let save = list("saveExtensions");
        let mut seen = std::collections::HashSet::new();
        let exts: Vec<&'static str> = list("loadExtensions")
            .into_iter()
            .chain(save.iter().cloned())
            .filter(|e| seen.insert(e.clone()))
            .map(|e| leak(&e))
            .collect();
        let Some(&id) = exts.first() else { continue }; // all extensions were built-in
        let (controls, choice_keys) = crate::effect::dotnet::controls_from_properties(ft);
        let info = PluginFormat {
            id,
            name: leak(ft.get("name").and_then(Value::as_str).unwrap_or("Plugin")),
            extensions: Box::leak(exts.into_boxed_slice()),
            controls: Box::leak(controls.into_boxed_slice()),
            can_save: !save.is_empty(),
        };
        out.push((info, choice_keys, index as u32));
    }
    out
}

fn register<P: Clone>(reg: &mut Vec<Fmt<P>>, plugin: &P, enumerated: &Value) {
    for (info, choice_keys, index) in parse(enumerated) {
        reg.retain(|f| f.info.id != info.id);
        reg.push(Fmt {
            info,
            choice_keys,
            plugin: plugin.clone(),
            index,
        });
    }
}

/// The props JSON a save passes: the values the Save dialog set (the plugin's own defaults for
/// the rest).
fn props_json(values: &Values, choice_keys: &[&'static str]) -> Option<String> {
    (!values.is_empty())
        .then(|| crate::effect::dotnet::build_props(values, choice_keys).to_string())
}

/// The format id of a plugin FileType whose extension `ext` (no dot) is not a built-in.
pub fn format_for_ext(ext: &str) -> Option<&'static str> {
    host::with(|reg| {
        reg.iter()
            .find(|f| f.info.extensions.contains(&ext))
            .map(|f| f.info.id)
    })
}

pub fn is_plugin_format(id: &str) -> bool {
    host::with(|reg| reg.iter().any(|f| f.info.id == id))
}

/// Every registered plugin format, for the file dialogs.
pub fn formats() -> Vec<PluginFormat> {
    host::with(|reg| reg.iter().map(|f| f.info).collect())
}

/// Decode a plugin-format file to straight RGBA8.
pub fn open(format: &str, bytes: &[u8]) -> Result<Decoded, CodecError> {
    open_answered(format, bytes, &[])
}

/// [`open`], the FileType's windows taking `answers` in the order they show ([`PluginWindow::answer`]).
pub fn open_answered(format: &str, bytes: &[u8], answers: &[Value]) -> Result<Decoded, CodecError> {
    let props = (!answers.is_empty()).then(|| serde_json::json!({ ANSWERS: answers }).to_string());
    let (rgba, width, height) = host::open(format, bytes, props.as_deref())?;
    Ok(Decoded {
        image: ImageData {
            width,
            height,
            data: rgba,
        },
        dpi: None,
    })
}

/// Encode straight RGBA8 to a plugin format with the save option `values` (by property name; its windows' answers
/// under [`ANSWERS`]); `Err(unsupported)` if `format` is not one.
pub fn save(format: &str, img: &ImageData, values: &Values) -> Result<Vec<u8>, CodecError> {
    host::save(format, img, values)
}

/// Where a save's option values keep its FileType's windows' answers, a list in the order they show.
pub const ANSWERS: &str = "$windows";

/// What marks a FileType's window in the error of an open or save that stopped at it.
const WINDOW: &str = "pdn-window:";

/// A window a FileType shows while it opens or saves (TechnoRobbo's Icons, Midora's EMF): paint.rs shows it in its
/// own, as an effect's, and opens or saves again with the answers so far (docs/pdn-bridge.md "FileType windows").
#[derive(Clone, Debug)]
pub struct PluginWindow {
    pub title: String,
    pub controls: Vec<Control>,
    choice_keys: Vec<&'static str>,
}

impl PluginWindow {
    /// The window an open or save stopped at, from its error's text.
    pub fn from_error(error: &str) -> Option<Self> {
        let json = &error[error.find(WINDOW)? + WINDOW.len()..];
        let parsed: Value = serde_json::from_str(json).ok()?;
        let (controls, choice_keys) = crate::effect::dotnet::controls_from_properties(&parsed);
        let title = parsed["title"].as_str().unwrap_or_default().to_owned();
        Some(Self {
            title,
            controls,
            choice_keys,
        })
    }

    /// The answer `values` (the window's settings as paint.rs's dialog left them) give it.
    pub fn answer(&self, values: &Values) -> Value {
        crate::effect::dotnet::build_props(values, &self.choice_keys)
    }
}

pub use host::clear;
#[cfg(not(target_arch = "wasm32"))]
pub use host::register_from;
#[cfg(target_arch = "wasm32")]
pub use host::register_web;

#[cfg(not(target_arch = "wasm32"))]
mod host {
    use super::*;
    use std::sync::{Arc, OnceLock, RwLock};

    type Plugin = Arc<pdn_bridge::Plugin>;

    fn reg() -> &'static RwLock<Vec<Fmt<Plugin>>> {
        static R: OnceLock<RwLock<Vec<Fmt<Plugin>>>> = OnceLock::new();
        R.get_or_init(|| RwLock::new(Vec::new()))
    }

    pub(super) fn with<T>(f: impl FnOnce(&[Fmt<Plugin>]) -> T) -> T {
        f(&reg().read().unwrap())
    }

    /// Register every FileType of a loaded plugin from its `enumerate` JSON.
    pub fn register_from(plugin: &Plugin, enumerated: &Value) {
        register(&mut reg().write().unwrap(), plugin, enumerated);
    }

    pub fn clear() {
        reg().write().unwrap().clear();
    }

    fn find(format: &str) -> Result<(Plugin, u32, Vec<&'static str>), CodecError> {
        with(|r| {
            r.iter()
                .find(|f| f.info.id == format)
                .map(|f| (f.plugin.clone(), f.index, f.choice_keys.clone()))
        })
        .ok_or_else(|| crate::io::codecs::unsupported(format))
    }

    pub(super) fn open(
        format: &str,
        bytes: &[u8],
        props: Option<&str>,
    ) -> Result<(Vec<u8>, u32, u32), CodecError> {
        let (plugin, index, _) = find(format)?;
        plugin
            .filetype_open(index, bytes, props)
            .map_err(crate::io::codecs::invalid)
    }

    pub(super) fn save(
        format: &str,
        img: &ImageData,
        values: &Values,
    ) -> Result<Vec<u8>, CodecError> {
        let (plugin, index, choice_keys) = find(format)?;
        let props = props_json(values, &choice_keys);
        plugin
            .filetype_save(index, props.as_deref(), &img.data, img.width, img.height)
            .map_err(crate::io::codecs::invalid)
    }
}

// The browser's Mono host (effect::dotnet) drives save/open by (handle, index).
#[cfg(target_arch = "wasm32")]
mod host {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static REG: RefCell<Vec<Fmt<i32>>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn with<T>(f: impl FnOnce(&[Fmt<i32>]) -> T) -> T {
        REG.with(|r| f(&r.borrow()))
    }

    /// Register every FileType of a loaded plugin (handle) from its `enumerate` JSON.
    pub fn register_web(handle: i32, enumerated: &Value) {
        REG.with(|r| register(&mut r.borrow_mut(), &handle, enumerated));
    }

    pub fn clear() {
        REG.with(|r| r.borrow_mut().clear());
    }

    fn find(format: &str) -> Result<(i32, u32, Vec<&'static str>), CodecError> {
        with(|r| {
            r.iter()
                .find(|f| f.info.id == format)
                .map(|f| (f.plugin, f.index, f.choice_keys.clone()))
        })
        .ok_or_else(|| crate::io::codecs::unsupported(format))
    }

    pub(super) fn open(
        format: &str,
        bytes: &[u8],
        props: Option<&str>,
    ) -> Result<(Vec<u8>, u32, u32), CodecError> {
        let (handle, index, _) = find(format)?;
        crate::effect::dotnet::filetype_open(handle, index, bytes, props)
            .map_err(crate::io::codecs::invalid)
    }

    pub(super) fn save(
        format: &str,
        img: &ImageData,
        values: &Values,
    ) -> Result<Vec<u8>, CodecError> {
        let (handle, index, choice_keys) = find(format)?;
        crate::effect::dotnet::filetype_save(
            handle,
            index,
            props_json(values, &choice_keys),
            &img.data,
            img.width,
            img.height,
        )
        .map_err(crate::io::codecs::invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FileType's extensions skip the built-ins (the first other one is its id), and its save
    /// properties become controls; a type of only built-in extensions registers nothing.
    #[test]
    fn parse_skips_builtins_and_reads_save_properties() {
        let json = serde_json::json!({"fileTypes": [
            {"name": "WebP", "loadExtensions": [".webp"], "saveExtensions": [".webp"]},
            {"name": "Paint Shop Pro", "loadExtensions": [".PSPIMAGE", ".psp"], "saveExtensions": [".pspimage"],
             "properties": [
                {"name": "Compression", "label": "Compression", "type": "choice", "default": 1,
                 "choices": [{"value": 0, "label": "None"}, {"value": 1, "label": "RLE"}]},
                {"name": "Version", "type": "int", "default": 12, "min": 5, "max": 12}]},
            {"name": "Viewer", "loadExtensions": [".tim"], "saveExtensions": []}]});
        let got = parse(&json);
        assert_eq!(got.len(), 2);
        let (psp, keys, index) = &got[0];
        assert_eq!(
            (psp.id, psp.name, *index),
            ("pspimage", "Paint Shop Pro", 1)
        );
        assert_eq!(psp.extensions, ["pspimage", "psp"]);
        assert!(psp.can_save && !got[1].0.can_save);
        assert_eq!(psp.controls.len(), 2);
        assert_eq!(keys, &["Compression"]);
        let mut values = Values::new();
        values.insert("Compression".into(), "0".into());
        assert_eq!(
            props_json(&values, keys).as_deref(),
            Some(r#"{"Compression":0}"#)
        );
        assert_eq!(props_json(&Values::new(), keys), None);
    }
}
