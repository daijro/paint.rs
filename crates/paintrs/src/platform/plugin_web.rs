//! Browser storage for installed Paint.NET plug-in `.dll` files (IndexedDB), behind
//! [`PluginStore::Browser`](super::plugins::PluginStore). The bytes are read back at start and
//! handed to the Mono browser host (`pdn_core::effect::dotnet`).
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/platform/plugin_web.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = pluginList)]
    async fn plugin_list() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = pluginAll)]
    async fn plugin_all() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = pluginPut)]
    async fn plugin_put(name: &str, bytes: &[u8]) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = pluginDelete)]
    async fn plugin_delete(name: &str) -> Result<JsValue, JsValue>;
}

fn message(v: JsValue) -> String {
    js_sys::Reflect::get(&v, &JsValue::from_str("message"))
        .ok()
        .and_then(|m| m.as_string())
        .or_else(|| v.as_string())
        .unwrap_or_else(|| "Plug-in storage failed".into())
}

/// The installed file names.
pub async fn list() -> Result<Vec<String>, String> {
    let v = plugin_list().await.map_err(message)?;
    Ok(js_sys::Array::from(&v)
        .iter()
        .filter_map(|x| x.as_string())
        .collect())
}

/// Every installed file, by name, with its bytes — for loading into the host at start.
pub async fn all() -> Result<Vec<(String, Vec<u8>)>, String> {
    let v = plugin_all().await.map_err(message)?;
    let mut out = Vec::new();
    for row in js_sys::Array::from(&v).iter() {
        let name = js_sys::Reflect::get(&row, &"name".into())
            .ok()
            .and_then(|n| n.as_string());
        let data = js_sys::Reflect::get(&row, &"data".into()).ok();
        if let (Some(name), Some(data)) = (name, data) {
            out.push((name, js_sys::Uint8Array::new(&data).to_vec()));
        }
    }
    Ok(out)
}

pub async fn put(name: String, bytes: Vec<u8>) -> Result<(), String> {
    plugin_put(&name, &bytes).await.map(|_| ()).map_err(message)
}

pub async fn delete(name: String) -> Result<(), String> {
    plugin_delete(&name).await.map(|_| ()).map_err(message)
}
