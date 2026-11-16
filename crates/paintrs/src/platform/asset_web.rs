//! IndexedDB adapter for AssetPackManager.js / ShapeManager.js. Reference: paintdotjs b4e6fc5.
use super::asset_library::{Change, Kind, Snapshot};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(module = "/src/platform/asset_web.js")]
extern "C" {
    #[wasm_bindgen(catch,js_name=assetRead)]
    async fn asset_read(source: &JsValue, max_bytes: f64) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=assetLoad)]
    async fn asset_load() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=assetChange)]
    async fn asset_change(
        store: &str,
        action: &str,
        id: &str,
        json: &str,
    ) -> Result<JsValue, JsValue>;
}
fn message(v: JsValue) -> String {
    js_sys::Reflect::get(&v, &JsValue::from_str("message"))
        .ok()
        .and_then(|v| v.as_string())
        .or_else(|| v.as_string())
        .unwrap_or_else(|| "Asset storage failed".into())
}
pub async fn load() -> Result<Snapshot, String> {
    let raw = asset_load()
        .await
        .map_err(message)?
        .as_string()
        .ok_or("Invalid asset snapshot")?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}
pub(super) async fn change(k: Kind, change: Change) -> Result<(), String> {
    let (action, id, json) = match change {
        Change::Put(v) => ("put", String::new(), v.to_string()),
        Change::Remove(id) => ("delete", id, String::new()),
        Change::Clear => ("clear", String::new(), String::new()),
    };
    asset_change(k.store(), action, &id, &json)
        .await
        .map_err(message)?;
    Ok(())
}

pub async fn read(mut file: super::PickedFile, limit: u64) -> Result<super::PickedFile, String> {
    let source = if let Some(handle) = &file.handle {
        JsValue::from(handle.clone())
    } else {
        file.dropped
            .as_ref()
            .and_then(|d| d.web_file())
            .ok_or("Missing browser asset source")?
            .into()
    };
    let value = asset_read(&source, limit as f64).await.map_err(message)?;
    let bytes = js_sys::Uint8Array::new(&value);
    if bytes.length() as u64 > limit {
        return Err("Asset file is too large".into());
    }
    file.bytes = bytes.to_vec().into();
    Ok(file)
}
