//! Port of: paintdotjs js/src/io/DocumentIO.js
//! Reference: paintdotjs @ b4e6fc5.
//! Browser file adapter. Port: DocumentIO.js; R113 save/activation/export contract.
use super::{Files, Pending, PickedFile};
use crate::dialogs::{
    flows::{SaveDestination, SaveRequest, SaveTarget, TargetRequest, WriteOutcome},
    task::{TaskChoice, TaskDialog, TaskDialogSpec},
};
use pdn_core::io::codecs::MAX_DECODE_BYTES;
use std::{cell::RefCell, collections::HashMap, rc::Rc, task::Poll};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
#[wasm_bindgen(module = "/src/platform/web_files.js")]
extern "C" {
    #[wasm_bindgen(js_name=hasActivation)]
    fn has_activation() -> bool;
    #[wasm_bindgen(catch,js_name=chooseOpen)]
    fn choose_open_js(
        multiple: bool,
        accept: &str,
        all_files: bool,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(catch,js_name=chooseSave)]
    fn choose_save_js(
        name: &str,
        description: &str,
        extensions: &js_sys::Array,
        types: &js_sys::Array,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(catch,js_name=readSource)]
    fn read_source(handle: &JsValue, limit: f64) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(catch,js_name=prepareOutput)]
    fn prepare_output(handle: &JsValue, request: &JsValue) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_name=abortPreparation)]
    fn abort_preparation(request: &JsValue);
    #[wasm_bindgen(catch,js_name=writeOutput)]
    fn write_output(output: &JsValue, bytes: &[u8]) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_name=abortOutput)]
    fn abort_output(output: &JsValue);
    #[wasm_bindgen(catch,js_name=downloadOutput)]
    fn download_output(bytes: &[u8], name: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch,js_name=printPng)]
    fn print_png_js(bytes: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(catch,js_name=paletteFiles)]
    fn palette_files_js() -> Result<js_sys::Promise, JsValue>;
}
fn error(e: JsValue) -> String {
    js_sys::Reflect::get(&e, &"message".into())
        .ok()
        .and_then(|v| v.as_string())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "Browser file operation failed.".into())
}
fn promise(op: Result<js_sys::Promise, JsValue>) -> Pending<Result<JsValue, String>> {
    match op {
        Ok(p) => Pending::future(async move { JsFuture::from(p).await.map_err(error) }),
        Err(e) => Pending::ready(Err(error(e))),
    }
}
/// The starter itself is called by the gesture callback, before polling/awaiting its Promise.
fn gesture<T: 'static>(
    start: impl FnOnce() -> Pending<Result<T, String>> + 'static,
) -> Pending<Result<T, String>> {
    if has_activation() {
        return start();
    }
    let result = Rc::new(RefCell::new(None::<Pending<Result<T, String>>>));
    let callback = result.clone();
    TaskDialog::show_then(
        TaskDialogSpec {
            title: "File access".into(),
            message: Some(
                "Continue to let your browser open the file dialog or start the download.".into(),
            ),
            choices: vec![
                TaskChoice {
                    value: "continue",
                    title: "Continue".into(),
                    ..Default::default()
                },
                TaskChoice {
                    value: "cancel",
                    title: "Cancel".into(),
                    ..Default::default()
                },
            ],
            cancel_value: "cancel",
            ..Default::default()
        },
        move |choice, _| {
            *callback.borrow_mut() = Some(if choice == "continue" {
                start()
            } else {
                Pending::ready(Err("Canceled".into()))
            });
        },
    );
    Pending::future(std::future::poll_fn(move |_| {
        let mut result = result.borrow_mut();
        match result.as_mut().and_then(Pending::poll) {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }
    }))
}
#[derive(Clone)]
struct Target {
    destination: SaveDestination,
    handle: Option<web_sys::FileSystemFileHandle>,
}
struct Preparing {
    token: JsValue,
    armed: bool,
}
impl Drop for Preparing {
    fn drop(&mut self) {
        if self.armed {
            abort_preparation(&self.token);
        }
    }
}
fn prepare_target(handle: JsValue) -> Pending<Result<Output, String>> {
    let mut request = Preparing {
        token: js_sys::Object::new().into(),
        armed: true,
    };
    let token = request.token.clone();
    let mut op = promise(prepare_output(&handle, &token));
    Pending::future(async move {
        let value =
            match std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await
            {
                Err(e) if e == "ACTIVATION_REQUIRED" => {
                    let mut retry = gesture(move || promise(prepare_output(&handle, &token)));
                    std::future::poll_fn(move |_| retry.poll().map_or(Poll::Pending, Poll::Ready))
                        .await?
                }
                r => r?,
            };
        request.armed = false;
        drop(request);
        Ok(Output((!value.is_null()).then_some(value)))
    })
}
struct Output(Option<JsValue>);
impl Drop for Output {
    fn drop(&mut self) {
        if let Some(stream) = self.0.take() {
            abort_output(&stream);
        }
    }
}
#[derive(Default)]
pub struct FileState {
    current: HashMap<u64, Target>,
    chosen: HashMap<u64, Target>,
    prepared: HashMap<u64, Output>,
}
fn field(value: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(value, &key.into()).unwrap_or(JsValue::NULL)
}
pub fn choose_open(multiple: bool, accept: &str) -> Pending<Result<Vec<PickedFile>, String>> {
    choose_open_with(multiple, accept, false)
}
/// R156 (BB27): File System Access gets the ordered types with All files allowed; the input
/// fallback only hints `accept`, its All-files entry is the browser's.
pub fn choose_open_with(
    multiple: bool,
    accept: &str,
    all_files: bool,
) -> Pending<Result<Vec<PickedFile>, String>> {
    let accept = accept.to_owned();
    let mut op = gesture(move || promise(choose_open_js(multiple, &accept, all_files)));
    Pending::future(async move {
        let value =
            std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?;
        js_sys::Array::from(&value)
            .iter()
            .map(|v| {
                let name = field(&v, "name")
                    .as_string()
                    .ok_or("Invalid browser file name.")?;
                let mut f = PickedFile::snapshot(name, vec![]);
                f.mime = field(&v, "mime").as_string();
                f.handle = Some(field(&v, "handle").unchecked_into());
                Ok(f)
            })
            .collect()
    })
}
pub fn pick_open(multiple: bool, accept: &str) -> Pending<Result<Vec<PickedFile>, String>> {
    let mut op = choose_open(multiple, accept);
    Pending::future(async move {
        let files =
            std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?;
        let mut result = vec![];
        for source in files {
            let mut read = reread(source, MAX_DECODE_BYTES);
            result.push(
                std::future::poll_fn(move |_| read.poll().map_or(Poll::Pending, Poll::Ready))
                    .await?,
            );
        }
        Ok(result)
    })
}
/// Reads a handle's file again; one over `limit` bytes is refused by its size, unread.
pub fn reread(mut file: PickedFile, limit: u64) -> Pending<Result<PickedFile, String>> {
    let Some(handle) = &file.handle else {
        return Pending::ready(Ok(file));
    };
    let mut op = promise(read_source(handle.as_ref(), limit as f64));
    Pending::future(async move {
        let value =
            std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?;
        file.bytes = js_sys::Uint8Array::new(&value).to_vec().into();
        Ok(file)
    })
}
pub fn adopt_open(
    state: &Rc<RefCell<FileState>>,
    id: u64,
    file: &PickedFile,
    format: &'static str,
) {
    if let Some(handle) = &file.handle {
        state.borrow_mut().current.insert(
            id,
            Target {
                destination: SaveDestination {
                    name: file.name.clone(),
                    path: None,
                    format,
                },
                handle: Some(handle.clone()),
            },
        );
    }
}
impl SaveTarget for Files {
    fn choose(&mut self, _: &TargetRequest) -> Result<Option<SaveDestination>, String> {
        Err("Browser file choice is pending.".into())
    }
    fn choose_pending(
        &mut self,
        r: TargetRequest,
    ) -> Pending<Result<Option<SaveDestination>, String>> {
        let state = self.state.clone();
        let mut op = gesture(move || {
            let format = crate::dialogs::save_image::find_format(r.initial_format);
            let extensions = format
                .extensions
                .iter()
                .map(|e| JsValue::from_str(e))
                .collect();
            let types = js_sys::Array::new();
            for info in super::files::chooser_formats(r.initial_format) {
                let t = js_sys::Object::new();
                let accept = js_sys::Object::new();
                let exts: js_sys::Array = info
                    .extensions
                    .iter()
                    .map(|e| JsValue::from_str(e))
                    .collect();
                let _ = js_sys::Reflect::set(&accept, &"application/octet-stream".into(), &exts);
                let _ = js_sys::Reflect::set(&t, &"description".into(), &info.name.into());
                let _ = js_sys::Reflect::set(&t, &"accept".into(), &accept);
                types.push(&t);
            }
            let mut choose = promise(choose_save_js(
                &r.suggested_name,
                format.name,
                &extensions,
                &types,
            ));
            Pending::future(async move {
                let value =
                    std::future::poll_fn(move |_| choose.poll().map_or(Poll::Pending, Poll::Ready))
                        .await?;
                if value.is_null() {
                    return Ok(None);
                }
                let name = field(&value, "name")
                    .as_string()
                    .ok_or("Invalid browser target name.")?;
                let accepted = super::files::accepted_format(&name)?;
                let destination = SaveDestination {
                    name,
                    path: None,
                    format: accepted,
                };
                let handle = field(&value, "handle");
                state.borrow_mut().chosen.insert(
                    r.workspace_id,
                    Target {
                        destination: destination.clone(),
                        handle: (!handle.is_null()).then(|| handle.unchecked_into()),
                    },
                );
                Ok(Some(destination))
            })
        });
        Pending::future(async move {
            match std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await
            {
                Err(e) if e == "Canceled" => Ok(None),
                r => r,
            }
        })
    }
    fn prepare_pending(&mut self, id: u64, _: &SaveDestination) -> Pending<Result<(), String>> {
        let target = {
            let state = self.state.borrow();
            state
                .chosen
                .get(&id)
                .or_else(|| state.current.get(&id))
                .cloned()
        };
        let Some(target) = target else {
            return Pending::ready(Err("The browser target is no longer available.".into()));
        };
        let state = self.state.clone();
        let handle = target.handle.map_or(JsValue::NULL, |h| h.into());
        let mut op = prepare_target(handle);
        Pending::future(async move {
            let output =
                std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?;
            state.borrow_mut().prepared.insert(id, output);
            Ok(())
        })
    }

    fn write(&mut self, _: &SaveRequest) -> Result<(), String> {
        Err("Browser writing is pending.".into())
    }
    fn write_pending(&mut self, r: SaveRequest) -> Pending<Result<WriteOutcome, String>> {
        let Some(mut output) = self.state.borrow_mut().prepared.remove(&r.workspace_id) else {
            return Pending::ready(Err("Browser output was not prepared.".into()));
        };
        if let Some(stream) = output.0.as_ref() {
            let mut op = promise(write_output(stream, &r.bytes));
            return Pending::future(async move {
                std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?;
                drop(output.0.take());
                Ok(WriteOutcome::Written)
            });
        }
        gesture(move || {
            Pending::ready(
                download_output(&r.bytes, &r.destination.name)
                    .map(|()| WriteOutcome::DownloadStarted)
                    .map_err(error),
            )
        })
    }
    fn existing(&self, id: u64) -> Option<SaveDestination> {
        self.state
            .borrow()
            .current
            .get(&id)
            .map(|t| t.destination.clone())
    }
    fn abandon(&mut self, id: u64) {
        let mut state = self.state.borrow_mut();
        state.chosen.remove(&id);
        state.prepared.remove(&id);
    }
    fn committed(&mut self, id: u64, _: &SaveDestination) {
        let mut state = self.state.borrow_mut();
        if let Some(target) = state.chosen.remove(&id) {
            state.current.insert(id, target);
        }
    }
}
pub fn print_png(bytes: Vec<u8>) -> Pending<Result<(), String>> {
    gesture(move || Pending::ready(print_png_js(&bytes).map_err(error)))
}
pub fn palette_files() -> Pending<Result<Vec<PickedFile>, String>> {
    let mut op = gesture(|| promise(palette_files_js()));
    Pending::future(async move {
        let value =
            std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?;
        let mut files = vec![];
        for v in js_sys::Array::from(&value).iter() {
            let mut file =
                PickedFile::snapshot(field(&v, "name").as_string().unwrap_or_default(), vec![]);
            file.handle = Some(field(&v, "handle").unchecked_into());
            let mut op = reread(file, MAX_DECODE_BYTES);
            files.push(
                std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready)).await?,
            );
        }
        Ok(files)
    })
}

/// BB36: browsers show no chooser title; the extension is the open filter.
pub fn choose_data(request: super::files::DataFile) -> Pending<Result<Vec<PickedFile>, String>> {
    choose_open(false, &format!(".{}", request.extension))
}

/// BB36: the suggested name carries the extension, which picks the save type.
pub fn save_data(
    request: super::files::DataFile,
    name: &str,
    bytes: Vec<u8>,
) -> Pending<Result<super::files::FileSaveOutcome, String>> {
    save_bytes(&format!("{name}.{}", request.extension), bytes, None)
}

pub fn save_bytes(
    name: &str,
    bytes: Vec<u8>,
    existing: Option<super::files::FileTarget>,
) -> Pending<Result<super::files::FileSaveOutcome, String>> {
    use super::files::{FileSaveOutcome, FileTarget};
    let name = name.to_owned();
    let mut choose = if let Some(target) = existing {
        Pending::ready(Ok(Some(target)))
    } else {
        gesture(move || {
            let extension = name
                .rsplit_once('.')
                .map_or(".txt".to_owned(), |(_, e)| format!(".{e}"));
            let extensions = js_sys::Array::of1(&extension.into());
            let mut op = promise(choose_save_js(
                &name,
                "File",
                &extensions,
                &js_sys::Array::of1(&{
                    let t = js_sys::Object::new();
                    let a = js_sys::Object::new();
                    let _ =
                        js_sys::Reflect::set(&a, &"application/octet-stream".into(), &extensions);
                    let _ = js_sys::Reflect::set(&t, &"description".into(), &"File".into());
                    let _ = js_sys::Reflect::set(&t, &"accept".into(), &a);
                    t.into()
                }),
            ));
            Pending::future(async move {
                let value =
                    std::future::poll_fn(move |_| op.poll().map_or(Poll::Pending, Poll::Ready))
                        .await?;
                if value.is_null() {
                    return Ok(None);
                }
                let handle = field(&value, "handle");
                Ok(Some(FileTarget {
                    name: field(&value, "name")
                        .as_string()
                        .ok_or("Invalid browser target name.")?,
                    path: None,
                    handle: (!handle.is_null()).then(|| handle.unchecked_into()),
                }))
            })
        })
    };
    Pending::future(async move {
        let target =
            match std::future::poll_fn(move |_| choose.poll().map_or(Poll::Pending, Poll::Ready))
                .await
            {
                Ok(Some(t)) => t,
                Ok(None) => return Ok(FileSaveOutcome::Cancelled),
                Err(e) if e == "Canceled" => return Ok(FileSaveOutcome::Cancelled),
                Err(e) => return Err(e),
            };
        let handle = target
            .handle
            .as_ref()
            .map_or(JsValue::NULL, |h| h.clone().into());
        let mut prepare = prepare_target(handle);
        let mut guard =
            std::future::poll_fn(move |_| prepare.poll().map_or(Poll::Pending, Poll::Ready))
                .await?;
        if guard.0.is_none() {
            let mut download = gesture(move || {
                Pending::ready(download_output(&bytes, &target.name).map_err(error))
            });
            std::future::poll_fn(move |_| download.poll().map_or(Poll::Pending, Poll::Ready))
                .await?;
            return Ok(FileSaveOutcome::DownloadStarted);
        }
        let mut write = promise(write_output(guard.0.as_ref().unwrap(), &bytes));
        std::future::poll_fn(move |_| write.poll().map_or(Poll::Pending, Poll::Ready)).await?;
        drop(guard.0.take());
        Ok(FileSaveOutcome::Written(target))
    })
}
