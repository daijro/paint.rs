//! Owned asynchronous clipboard reads. Consumers retain the handle until their current
//! session resolves; dropping/canceling it makes a late completion inert.
use pdn_core::canvas::ImageData;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

pub enum ImageRead {
    Pending,
    Ready(Result<Option<ImageData>, String>),
    Canceled,
}

pub struct ImageReadRequest(Rc<RefCell<ImageRead>>, Rc<RefCell<Option<String>>>);
impl ImageReadRequest {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn take_selection_text(&mut self) -> Option<String> {
        self.1.borrow_mut().take()
    }
    pub fn poll(&mut self) -> ImageRead {
        let mut state = self.0.borrow_mut();
        if matches!(*state, ImageRead::Pending) {
            ImageRead::Pending
        } else {
            std::mem::replace(&mut *state, ImageRead::Canceled)
        }
    }
    pub fn cancel(&mut self) {
        *self.0.borrow_mut() = ImageRead::Canceled;
        self.1.borrow_mut().take();
    }
}
impl Drop for ImageReadRequest {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn complete(weak: &Weak<RefCell<ImageRead>>, value: Result<Option<ImageData>, String>) {
    if let Some(state) = weak.upgrade() {
        let mut state = state.borrow_mut();
        if matches!(*state, ImageRead::Pending) {
            *state = ImageRead::Ready(value);
        }
    }
}

impl super::Clipboard {
    pub(crate) fn cancel_image_read(&mut self) {
        if let Some(state) = self.image_read.take().and_then(|w| w.upgrade()) {
            *state.borrow_mut() = ImageRead::Canceled;
        }
    }

    /// Starts browser access immediately where user activation is required. Only the latest
    /// request on this clipboard can complete. Ready(Ok(None)) is a completed empty/text read.
    pub fn request_image(&mut self, ctx: &egui::Context) -> ImageReadRequest {
        self.request_image_inner(
            ctx,
            #[cfg(target_arch = "wasm32")]
            None,
            #[cfg(target_arch = "wasm32")]
            false,
        )
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn request_image_blob(
        &mut self,
        ctx: &egui::Context,
        blob: web_sys::Blob,
    ) -> ImageReadRequest {
        self.request_image_inner(ctx, Some(blob), false)
    }
    /// Shell selection paste reads valid selection JSON before falling back to an image.
    /// The public image-only request never reads or returns text (BB26).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn request_selection(&mut self, ctx: &egui::Context) -> ImageReadRequest {
        self.request_image_inner(ctx, None, true)
    }
    fn request_image_inner(
        &mut self,
        ctx: &egui::Context,
        #[cfg(target_arch = "wasm32")] blob: Option<web_sys::Blob>,
        #[cfg(target_arch = "wasm32")] selection_text: bool,
    ) -> ImageReadRequest {
        if let Some(previous) = self.image_read.take().and_then(|w| w.upgrade()) {
            *previous.borrow_mut() = ImageRead::Canceled;
        }
        let request = ImageReadRequest(
            Rc::new(RefCell::new(ImageRead::Pending)),
            Default::default(),
        );
        let weak = Rc::downgrade(&request.0);
        self.image_read = Some(weak.clone());
        #[cfg(test)]
        if TEST_READS.with(|slot| {
            slot.borrow().as_ref().is_some_and(|probe| {
                probe.reads.set(probe.reads.get() + 1);
                probe
                    .completions
                    .borrow_mut()
                    .push(ImageReadCompleter(weak.clone()));
                probe.deferred
            })
        }) {
            return request;
        }

        #[cfg(target_arch = "wasm32")]
        if self.web_images {
            use wasm_bindgen::prelude::*;
            #[wasm_bindgen(module = "/src/platform/web_clipboard.js")]
            extern "C" {
                #[wasm_bindgen(catch,js_name=requestClipboardImage)]
                fn read(
                    max_side: u32,
                    max_bytes: f64,
                    blob: Option<&web_sys::Blob>,
                    selection_text: bool,
                ) -> Result<js_sys::Promise, JsValue>;
            }
            let promise = read(
                pdn_core::io::codecs::MAX_DECODE_SIDE,
                pdn_core::io::codecs::MAX_DECODE_BYTES as f64,
                blob.as_ref(),
                selection_text,
            );
            let text_output = Rc::downgrade(&request.1);
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = match promise {
                    Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise).await,
                    Err(error) => Err(error),
                }
                .map_err(|e| format!("{e:?}"))
                .and_then(|value| {
                    // Retired consumers must not allocate or decode a late browser result.
                    if !weak
                        .upgrade()
                        .is_some_and(|state| matches!(*state.borrow(), ImageRead::Pending))
                    {
                        return Ok(None);
                    }
                    if value.is_null() {
                        return Ok(None);
                    }
                    if selection_text
                        && let Some(text) = js_sys::Reflect::get(&value, &JsValue::from_str("text"))
                            .ok()
                            .and_then(|v| v.as_string())
                        && pdn_core::io::clipboard::selection_from_json(&text).is_some()
                    {
                        if let Some(output) = text_output.upgrade() {
                            *output.borrow_mut() = Some(text);
                        }
                        return Ok(None);
                    }
                    if let Some(error) =
                        js_sys::Reflect::get(&value, &JsValue::from_str("imageError"))
                            .ok()
                            .and_then(|v| v.as_string())
                    {
                        return Err(error);
                    }
                    if js_sys::Reflect::get(&value, &JsValue::from_str("bytes"))
                        .is_ok_and(|v| v.is_undefined())
                    {
                        return Ok(None);
                    }
                    let format = js_sys::Reflect::get(&value, &JsValue::from_str("format"))
                        .ok()
                        .and_then(|v| v.as_string())
                        .ok_or("Invalid clipboard format")?;
                    let bytes = js_sys::Reflect::get(&value, &JsValue::from_str("bytes"))
                        .map_err(|_| "Invalid clipboard bytes")?
                        .dyn_into::<js_sys::Uint8Array>()
                        .map_err(|_| "Invalid clipboard bytes")?;
                    if bytes.length() as u64 > pdn_core::io::codecs::MAX_DECODE_BYTES {
                        return Err("Clipboard image too large".into());
                    }
                    pdn_core::io::codecs::decode(&format, &bytes.to_vec())
                        .map(|d| Some(d.image))
                        .map_err(|e| e.to_string())
                });
                complete(&weak, result);
                ctx.request_repaint();
            });
            return request;
        }
        complete(&weak, self.read_image());
        ctx.request_repaint();
        request
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn encode_web_copy(image: &ImageData) -> Result<Vec<u8>, String> {
    use pdn_core::io::{codecs, encode};
    codecs::check_size(image.width, image.height).map_err(|e| e.to_string())?;
    let options = encode::SaveOptions {
        bit_depth: encode::BitDepth::B32,
        ..Default::default()
    };
    let bytes = encode::encode(image, "png", &options).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > codecs::MAX_DECODE_BYTES {
        return Err("Clipboard image too large".into());
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
impl super::Clipboard {
    pub(crate) fn write_web_image(&mut self, image: &ImageData) {
        use wasm_bindgen::prelude::*;
        self.text_write = None;
        let completion = std::sync::Arc::new(std::sync::Mutex::new(None));
        self.image_write = Some(completion.clone());
        let weak = std::sync::Arc::downgrade(&completion);
        #[wasm_bindgen(module = "/src/platform/web_clipboard.js")]
        extern "C" {
            #[wasm_bindgen(catch, js_name = writeClipboardImage)]
            fn write(bytes: &[u8]) -> Result<js_sys::Promise, JsValue>;
        }
        #[wasm_bindgen(js_namespace = console)]
        extern "C" {
            fn warn(message: &str);
        }
        let result =
            encode_web_copy(image).and_then(|bytes| write(&bytes).map_err(|e| format!("{e:?}")));
        wasm_bindgen_futures::spawn_local(async move {
            let result = match result {
                Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise)
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("{e:?}")),
                Err(error) => Err(error),
            };
            // A superseded write cannot report an obsolete failure over the current copy. The copy inside
            // paint.rs already succeeded, so a browser refusing the system clipboard is no error to show.
            if let Some(completion) = weak.upgrade() {
                if let Err(error) = &result {
                    warn(&format!("paint.rs: clipboard image write failed: {error}"));
                }
                *completion.lock().expect("clipboard image write") = Some(result);
            }
        });
    }
    pub(crate) fn clear_completed_image_write(&mut self) {
        if self
            .image_write
            .as_ref()
            .is_some_and(|c| c.lock().expect("clipboard image write").is_some())
        {
            self.image_write = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_clipboard_write_cancels_an_unfinished_read_and_rejects_its_late_result() {
        let mut clipboard = super::super::Clipboard::in_memory();
        for image_write in [false, true] {
            let mut request = ImageReadRequest(
                Rc::new(RefCell::new(ImageRead::Pending)),
                Default::default(),
            );
            let weak = Rc::downgrade(&request.0);
            clipboard.image_read = Some(weak.clone());
            if image_write {
                clipboard.write_image(&ImageData {
                    width: 1,
                    height: 1,
                    data: vec![1, 2, 3, 255],
                });
            } else {
                clipboard.write_text("new clipboard value");
            }
            complete(&weak, Ok(None));
            assert!(matches!(request.poll(), ImageRead::Canceled));
        }
    }
    #[test]
    fn web_copy_encoder_preserves_rgba_and_rejects_oversized_images() {
        let image = ImageData {
            width: 2,
            height: 1,
            data: vec![120, 80, 40, 128, 3, 4, 5, 255],
        };
        let bytes = encode_web_copy(&image).unwrap();
        let decoded = pdn_core::io::codecs::decode("png", &bytes).unwrap().image;
        assert_eq!(decoded.data, image.data);
        assert!(
            encode_web_copy(&ImageData {
                width: pdn_core::io::codecs::MAX_DECODE_SIDE + 1,
                height: 1,
                data: vec![]
            })
            .is_err()
        );
    }
    #[test]
    fn image_read_pending_empty_denied_canceled_and_late_completion() {
        let mut pending = ImageReadRequest(
            Rc::new(RefCell::new(ImageRead::Pending)),
            Default::default(),
        );
        let weak = Rc::downgrade(&pending.0);
        assert!(matches!(pending.poll(), ImageRead::Pending));
        complete(&weak, Ok(None));
        assert!(matches!(pending.poll(), ImageRead::Ready(Ok(None))));
        assert!(matches!(pending.poll(), ImageRead::Canceled));
        let mut canceled = ImageReadRequest(
            Rc::new(RefCell::new(ImageRead::Pending)),
            Default::default(),
        );
        let weak = Rc::downgrade(&canceled.0);
        canceled.cancel();
        complete(&weak, Ok(None));
        assert!(matches!(canceled.poll(), ImageRead::Canceled));
        drop(canceled);
        complete(&weak, Err("denied".into()));
        assert!(weak.upgrade().is_none());
        let mut denied = ImageReadRequest(
            Rc::new(RefCell::new(ImageRead::Pending)),
            Default::default(),
        );
        complete(&Rc::downgrade(&denied.0), Err("denied".into()));
        assert!(matches!(denied.poll(),ImageRead::Ready(Err(e)) if e=="denied"));
    }
    #[test]
    fn image_read_replacement_retires_completed_image_and_text_clears_image() {
        let mut clipboard = super::super::Clipboard::in_memory();
        let ctx = egui::Context::default();
        clipboard.write_image(&ImageData {
            width: 1,
            height: 1,
            data: vec![1, 2, 3, 4],
        });
        let mut old = clipboard.request_image(&ctx);
        clipboard.write_text("text");
        let mut current = clipboard.request_image(&ctx);
        assert!(matches!(old.poll(), ImageRead::Canceled));
        assert!(matches!(current.poll(), ImageRead::Ready(Ok(None))));
    }
}

/// Deterministic completions for the production request handle, confined to this test thread.
#[cfg(test)]
pub(crate) struct ImageReadCompleter(Weak<RefCell<ImageRead>>);
#[cfg(test)]
impl ImageReadCompleter {
    pub(crate) fn complete(&self, result: Result<Option<ImageData>, String>) {
        complete(&self.0, result);
    }
    pub(crate) fn cancel(&self) {
        if let Some(state) = self.0.upgrade() {
            *state.borrow_mut() = ImageRead::Canceled;
        }
    }
}
#[cfg(test)]
thread_local! { static TEST_READS: RefCell<Option<Rc<TestImageReads>>> = const { RefCell::new(None) }; }
#[cfg(test)]
pub(crate) struct TestImageReads {
    pub(crate) reads: std::cell::Cell<usize>,
    pub(crate) completions: RefCell<Vec<ImageReadCompleter>>,
    deferred: bool,
}
#[cfg(test)]
impl TestImageReads {
    pub(crate) fn install(deferred: bool) -> Rc<Self> {
        let probe = Rc::new(Self {
            reads: Default::default(),
            completions: Default::default(),
            deferred,
        });
        TEST_READS.with(|slot| *slot.borrow_mut() = Some(probe.clone()));
        probe
    }
    pub(crate) fn clear() {
        TEST_READS.with(|slot| *slot.borrow_mut() = None);
    }
}
