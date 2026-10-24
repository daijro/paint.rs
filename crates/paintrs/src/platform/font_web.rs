//! IndexedDB user-font blobs and first-use bundled font delivery. FontManager.js, b4e6fc5.
use super::fonts::{FontChange, METADATA_KEY, StoreUpdate, valid_records};
use pdn_core::fonts::{FontRecord, validate_font};
use std::{cell::RefCell, rc::Rc, task::Poll};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{IdbCursorWithValue, IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};
fn message(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}
async fn request(request: &IdbRequest) -> Result<JsValue, String> {
    let state = Rc::new(RefCell::new(None));
    let good = state.clone();
    let r = request.clone();
    let success = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        *good.borrow_mut() = Some(r.result().map_err(message));
    });
    let bad = state.clone();
    let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        *bad.borrow_mut() = Some(Err("Font storage operation failed".into()));
    });
    request.set_onsuccess(Some(success.as_ref().unchecked_ref()));
    request.set_onerror(Some(error.as_ref().unchecked_ref()));
    let result =
        std::future::poll_fn(|_| state.borrow_mut().take().map_or(Poll::Pending, Poll::Ready))
            .await;
    request.set_onsuccess(None);
    request.set_onerror(None);
    result
}
async fn database() -> Result<IdbDatabase, String> {
    let factory = web_sys::window()
        .ok_or("Persistent browser storage is not available")?
        .indexed_db()
        .map_err(message)?
        .ok_or("Persistent browser storage is not available")?;
    let open = factory
        .open_with_u32("paintdotjs-user-fonts", 1)
        .map_err(message)?;
    let r = open.clone();
    let upgrade = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        if let Ok(db) = r.result().and_then(|v| v.dyn_into::<IdbDatabase>())
            && !db.object_store_names().contains("fonts")
        {
            let params = web_sys::IdbObjectStoreParameters::new();
            params.set_key_path(&JsValue::from_str("id"));
            let _ = db.create_object_store_with_optional_parameters("fonts", &params);
        }
    });
    open.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
    let db = request(open.unchecked_ref())
        .await?
        .dyn_into::<IdbDatabase>()
        .map_err(message)?;
    open.set_onupgradeneeded(None);
    Ok(db)
}
struct Commit {
    transaction: IdbTransaction,
    state: Rc<RefCell<Option<Result<(), String>>>>,
    _success: Closure<dyn FnMut(web_sys::Event)>,
    _error: Closure<dyn FnMut(web_sys::Event)>,
}
impl Commit {
    fn new(transaction: IdbTransaction) -> Self {
        let state = Rc::new(RefCell::new(None));
        let good = state.clone();
        let bad = state.clone();
        let success = Closure::new(move |_: web_sys::Event| {
            *good.borrow_mut() = Some(Ok(()));
        });
        let error = Closure::new(move |_: web_sys::Event| {
            *bad.borrow_mut() = Some(Err("Font storage transaction failed".into()));
        });
        transaction.set_oncomplete(Some(success.as_ref().unchecked_ref()));
        transaction.set_onabort(Some(error.as_ref().unchecked_ref()));
        Self {
            transaction,
            state,
            _success: success,
            _error: error,
        }
    }
    async fn wait(self) -> Result<(), String> {
        std::future::poll_fn(|_| {
            self.state
                .borrow_mut()
                .take()
                .map_or(Poll::Pending, Poll::Ready)
        })
        .await
    }
}
impl Drop for Commit {
    fn drop(&mut self) {
        self.transaction.set_oncomplete(None);
        self.transaction.set_onabort(None);
    }
}
fn metadata(value: &JsValue) -> Result<FontRecord, String> {
    let fields = [
        "id", "name", "family", "fileName", "type", "size", "addedAt",
    ];
    let o = js_sys::Object::new();
    for field in fields {
        let key = JsValue::from_str(field);
        let value = js_sys::Reflect::get(value, &key).map_err(message)?;
        js_sys::Reflect::set(&o, &key, &value).map_err(message)?;
    }
    serde_json::from_str(
        &js_sys::JSON::stringify(&o)
            .map_err(message)?
            .as_string()
            .ok_or("Invalid font metadata")?,
    )
    .map_err(|e| e.to_string())
}
pub async fn load(cache: Option<&str>) -> Result<Vec<FontRecord>, String> {
    let db = database().await?;
    let result = async {
        let tx = db
            .transaction_with_str_and_mode("fonts", IdbTransactionMode::Readonly)
            .map_err(message)?;
        let keys = request(
            &tx.object_store("fonts")
                .map_err(message)?
                .get_all_keys()
                .map_err(message)?,
        )
        .await?;
        let keys = js_sys::Array::from(&keys);
        // The cache avoids reading blobs, but the committed DB keys remain authoritative if
        // localStorage was unavailable after a successful transaction (or another tab wrote).
        if let Some(records) =
            cache.and_then(|raw| serde_json::from_str::<Vec<FontRecord>>(raw).ok())
            && valid_records(&records).is_ok()
            && records.len() == keys.length() as usize
            && keys.iter().all(|key| {
                records
                    .iter()
                    .any(|r| Some(r.id.as_str()) == key.as_string().as_deref())
            })
        {
            return Ok(records);
        }
        // A fresh readonly cursor keeps the fallback snapshot consistent across other writers.
        Ok(transaction_records(&db, None).await?.records)
    }
    .await;
    db.close();
    result
}
pub async fn read(id: &str) -> Result<Vec<u8>, String> {
    let db = database().await?;
    let result = async {
        let tx = db
            .transaction_with_str_and_mode("fonts", IdbTransactionMode::Readonly)
            .map_err(message)?;
        let value = request(
            &tx.object_store("fonts")
                .map_err(message)?
                .get(&JsValue::from_str(id))
                .map_err(message)?,
        )
        .await?;
        if value.is_undefined() {
            return Err("The stored font is missing".into());
        }
        let data = js_sys::Reflect::get(&value, &JsValue::from_str("data")).map_err(message)?;
        let array = js_sys::Uint8Array::new(&data);
        if array.length() > 64 * 1024 * 1024 {
            return Err("The font file is too large".into());
        }
        let bytes = array.to_vec();
        validate_font(&bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    .await;
    db.close();
    result
}
async fn transaction_records(
    db: &IdbDatabase,
    change: Option<FontChange>,
) -> Result<StoreUpdate, String> {
    let mode = if change.is_some() {
        IdbTransactionMode::Readwrite
    } else {
        IdbTransactionMode::Readonly
    };
    let tx = db
        .transaction_with_str_and_mode("fonts", mode)
        .map_err(message)?;
    let commit = Commit::new(tx.clone());
    let store = tx.object_store("fonts").map_err(message)?;
    let cursor_request = store.open_cursor().map_err(message)?;
    let state = Rc::new(RefCell::new(None::<Result<StoreUpdate, String>>));
    let good = state.clone();
    let bad = state.clone();
    let request = cursor_request.clone();
    let transaction = tx.clone();
    let mut records = vec![];
    // Continue from the active IDB callback; polling a Rust future on a later frame would let
    // the transaction become inactive. Keep only metadata, never an array of font payloads.
    let success = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        let result = (|| {
            let value = request.result().map_err(message)?;
            if !value.is_null() && !value.is_undefined() {
                if records.len() >= 10000 {
                    return Err("Font metadata is too large".into());
                }
                let cursor = value.dyn_into::<IdbCursorWithValue>().map_err(message)?;
                records.push(metadata(&cursor.value().map_err(message)?)?);
                cursor.continue_().map_err(message)?;
                return Ok(None);
            }
            let current = std::mem::take(&mut records);
            let update = if let Some(change) = &change {
                change.apply(current)?
            } else {
                valid_records(&current)?;
                StoreUpdate {
                    records: current,
                    removed: 0,
                }
            };
            match &change {
                Some(FontChange::Add(record, bytes)) => {
                    let value = js_sys::JSON::parse(
                        &serde_json::to_string(record).map_err(|e| e.to_string())?,
                    )
                    .map_err(message)?;
                    js_sys::Reflect::set(
                        &value,
                        &JsValue::from_str("data"),
                        &js_sys::Uint8Array::from(bytes.as_slice()).buffer(),
                    )
                    .map_err(message)?;
                    store.put(&value).map_err(message)?;
                }
                Some(FontChange::Remove(id)) => {
                    store.delete(&JsValue::from_str(id)).map_err(message)?;
                }
                Some(FontChange::Clear) => {
                    store.clear().map_err(message)?;
                }
                None => {}
            }
            Ok(Some(update))
        })();
        match result {
            Ok(Some(update)) => *good.borrow_mut() = Some(Ok(update)),
            Ok(None) => {}
            Err(error) => {
                let _ = transaction.abort();
                *good.borrow_mut() = Some(Err(error));
            }
        }
    });
    let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        *bad.borrow_mut() = Some(Err("Font storage operation failed".into()));
    });
    cursor_request.set_onsuccess(Some(success.as_ref().unchecked_ref()));
    cursor_request.set_onerror(Some(error.as_ref().unchecked_ref()));
    let result =
        std::future::poll_fn(|_| state.borrow_mut().take().map_or(Poll::Pending, Poll::Ready))
            .await;
    cursor_request.set_onsuccess(None);
    cursor_request.set_onerror(None);
    let update = result?;
    commit.wait().await?;
    Ok(update)
}
pub async fn write(change: FontChange) -> Result<StoreUpdate, String> {
    let db = database().await?;
    let result = transaction_records(&db, Some(change)).await;
    db.close();
    let update = result?;
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(
            METADATA_KEY,
            &serde_json::to_string(&update.records).map_err(|e| e.to_string())?,
        );
    }
    Ok(update)
}
pub async fn fetch(path: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("No browser window")?;
    let response = JsFuture::from(window.fetch_with_str(path))
        .await
        .map_err(message)?
        .dyn_into::<web_sys::Response>()
        .map_err(message)?;
    if !response.ok() {
        return Err(format!("Could not load font: HTTP {}", response.status()));
    }
    if response
        .headers()
        .get("Content-Length")
        .map_err(message)?
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|n| n > 64 * 1024 * 1024)
    {
        return Err("The font file is too large".into());
    }
    let data = JsFuture::from(response.array_buffer().map_err(message)?)
        .await
        .map_err(message)?;
    let bytes = js_sys::Uint8Array::new(&data);
    if bytes.length() > 64 * 1024 * 1024 {
        return Err("The font file is too large".into());
    }
    let bytes = bytes.to_vec();
    validate_font(&bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
