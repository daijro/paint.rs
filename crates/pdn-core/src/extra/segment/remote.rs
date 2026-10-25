//! MobileSAM run elsewhere. On the web it runs in a Web Worker (the `sam-worker` crate, driven by
//! paintrs's `platform::sam_worker`), so the page stays responsive while it loads the model and
//! encodes. The app registers it ([`set`]); Object Selection's lookups then become jobs that the
//! worker's answers finish. Natively nothing is registered: the model runs on threads
//! (`segment::sam`).
//!
//! Known limits (web): the worker holds about 0.5 GB once it has encoded (a wasm memory never
//! shrinks), until Object Selection's toolbar picks GrabCut, another tool is picked or the last
//! image closes, which stops it ([`Remote::stop`]). An encode in progress cannot be interrupted
//! (about 7 s): a dropped lookup's answer is ignored, and stopping ends it.

use std::cell::RefCell;
use std::rc::Rc;

use super::{Prompt, Region};
use crate::canvas::ImageData;
use crate::extra::job::Job;

/// Where the model stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Loading: bytes so far, of `total`; `cached` false when the browser offers no Cache API
    /// (a page not served over HTTPS or from localhost), so the next start downloads again.
    Loading {
        done: u64,
        total: u64,
        cached: bool,
    },
    Ready,
    /// The worker did not start or the model did not load (the message says which): GrabCut
    /// answers.
    Failed(String),
}

pub trait Remote {
    /// Where the model stands; the first call starts the worker and the model's load.
    fn status(&self) -> Status;
    /// The object `prompt` asks for in `pixels`, which `snapshot` names. The worker encodes each
    /// snapshot once (only the latest counts), then decodes each lookup.
    fn lookup(&self, snapshot: u64, pixels: &ImageData, prompt: Prompt) -> Job<Option<Region>>;
    /// The worker has encoded `snapshot`.
    fn embedded(&self, snapshot: u64) -> bool;
    /// The worker could not encode `snapshot`: GrabCut answers on those pixels.
    fn failed(&self, snapshot: u64) -> bool;
    /// Ends the worker, freeing its memory (nothing can ask it now); the next `status` starts it
    /// again. The app calls it every frame meanwhile, so it must be cheap once stopped.
    fn stop(&self);
}

thread_local! {
    static REMOTE: RefCell<Option<Rc<dyn Remote>>> = const { RefCell::new(None) };
}

/// Registers the app's remote MobileSAM (per thread: the UI thread, or a test's).
pub fn set(remote: Option<Rc<dyn Remote>>) {
    REMOTE.with_borrow_mut(|r| *r = remote);
}

/// The registered remote MobileSAM.
pub fn get() -> Option<Rc<dyn Remote>> {
    REMOTE.with_borrow(Clone::clone)
}

/// The messages between the app and its worker, as `postMessage` objects. Ids travel as numbers
/// (snapshot ids and request numbers stay far below 2^53).
#[cfg(target_arch = "wasm32")]
pub mod message {
    use js_sys::wasm_bindgen::JsValue;
    use js_sys::{ArrayBuffer, Object, Reflect, Uint8Array};

    use super::{Prompt, Region};
    use crate::canvas::ImageData;
    use crate::extra::geom::Rect;

    /// What the app asks: encode a snapshot's pixels once, then decode lookups on it.
    pub enum Request {
        Embed { id: u64, pixels: ImageData },
        Decode { rid: u32, id: u64, prompt: Prompt },
    }

    /// What the worker says.
    pub enum Reply {
        Loading {
            done: u64,
            total: u64,
            cached: bool,
        },
        Ready,
        Failed(String),
        Embedded(u64),
        /// The encoder failed on this snapshot.
        EncodeFailed(u64),
        /// A lookup's object, nothing found, or why it failed (a newer snapshot came, an error).
        Answer {
            rid: u32,
            result: Result<Option<Region>, String>,
        },
    }

    fn object(fields: &[(&str, JsValue)]) -> JsValue {
        let o = Object::new();
        for (k, v) in fields {
            let _ = Reflect::set(&o, &JsValue::from_str(k), v);
        }
        o.into()
    }

    fn get(v: &JsValue, k: &str) -> Option<JsValue> {
        Reflect::get(v, &JsValue::from_str(k))
            .ok()
            .filter(|v| !v.is_undefined())
    }

    fn num(v: &JsValue, k: &str) -> Option<f64> {
        get(v, k)?.as_f64()
    }

    /// `bytes` copied once into a JS buffer, to transfer.
    fn buffer(bytes: &[u8]) -> ArrayBuffer {
        Uint8Array::from(bytes).buffer()
    }

    fn rect_fields(r: Rect) -> [(&'static str, JsValue); 4] {
        [
            ("x0", r.x0.into()),
            ("y0", r.y0.into()),
            ("x1", r.x1.into()),
            ("y1", r.y1.into()),
        ]
    }

    fn rect(v: &JsValue) -> Option<Rect> {
        let n = |k| num(v, k).map(|n| n as i32);
        Some(Rect::new(n("x0")?, n("y0")?, n("x1")?, n("y1")?))
    }

    /// The request to encode `pixels` as snapshot `id`, and its buffer to transfer: the pixels
    /// copied once.
    pub fn embed(id: u64, pixels: &ImageData) -> (JsValue, ArrayBuffer) {
        let rgba = buffer(&pixels.data);
        let msg = object(&[
            ("embed", (id as f64).into()),
            ("width", pixels.width.into()),
            ("height", pixels.height.into()),
            ("rgba", rgba.clone().into()),
        ]);
        (msg, rgba)
    }

    impl Request {
        /// The message, and the buffer to transfer with it rather than copy.
        pub fn to_js(&self) -> (JsValue, Option<ArrayBuffer>) {
            match self {
                Request::Embed { id, pixels } => {
                    let (msg, rgba) = embed(*id, pixels);
                    (msg, Some(rgba))
                }
                Request::Decode { rid, id, prompt } => {
                    let mut fields = vec![("decode", (*rid).into()), ("id", (*id as f64).into())];
                    match *prompt {
                        Prompt::Rect(r) => {
                            fields.push(("kind", "rect".into()));
                            fields.extend(rect_fields(r));
                        }
                        Prompt::Point(x, y) => {
                            fields.extend([
                                ("kind", "point".into()),
                                ("x", x.into()),
                                ("y", y.into()),
                            ]);
                        }
                        Prompt::Subject => fields.push(("kind", "subject".into())),
                    }
                    (object(&fields), None)
                }
            }
        }

        pub fn from_js(v: &JsValue) -> Option<Request> {
            if let Some(id) = num(v, "embed") {
                let data = Uint8Array::new(&get(v, "rgba")?).to_vec();
                let (width, height) = (num(v, "width")? as u32, num(v, "height")? as u32);
                return Some(Request::Embed {
                    id: id as u64,
                    pixels: ImageData {
                        width,
                        height,
                        data,
                    },
                });
            }
            let rid = num(v, "decode")? as u32;
            let id = num(v, "id")? as u64;
            let prompt = match get(v, "kind")?.as_string()?.as_str() {
                "rect" => Prompt::Rect(rect(v)?),
                "point" => Prompt::Point(num(v, "x")? as f32, num(v, "y")? as f32),
                _ => Prompt::Subject,
            };
            Some(Request::Decode { rid, id, prompt })
        }
    }

    impl Reply {
        /// The message, and the buffer to transfer with it rather than copy.
        pub fn to_js(&self) -> (JsValue, Option<ArrayBuffer>) {
            let msg = match self {
                Reply::Loading {
                    done,
                    total,
                    cached,
                } => object(&[
                    ("loading", (*done as f64).into()),
                    ("total", (*total as f64).into()),
                    ("cached", (*cached).into()),
                ]),
                Reply::Ready => object(&[("ready", true.into())]),
                Reply::Failed(m) => object(&[("failed", m.as_str().into())]),
                Reply::Embedded(id) => object(&[("embedded", (*id as f64).into())]),
                Reply::EncodeFailed(id) => object(&[("encodeFailed", (*id as f64).into())]),
                Reply::Answer { rid, result } => {
                    let mut fields = vec![("answer", JsValue::from(*rid))];
                    match result {
                        Ok(Some(r)) => {
                            let mask = buffer(&r.mask);
                            fields.extend(rect_fields(r.bbox));
                            fields.push(("mask", mask.clone().into()));
                            return (object(&fields), Some(mask));
                        }
                        Ok(None) => {}
                        Err(e) => fields.push(("error", e.as_str().into())),
                    }
                    object(&fields)
                }
            };
            (msg, None)
        }

        pub fn from_js(v: &JsValue) -> Option<Reply> {
            if let Some(done) = num(v, "loading") {
                let total = num(v, "total")? as u64;
                let cached = get(v, "cached").is_none_or(|c| c.is_truthy());
                return Some(Reply::Loading {
                    done: done as u64,
                    total,
                    cached,
                });
            }
            if get(v, "ready").is_some() {
                return Some(Reply::Ready);
            }
            if let Some(m) = get(v, "failed") {
                return Some(Reply::Failed(m.as_string()?));
            }
            if let Some(id) = num(v, "embedded") {
                return Some(Reply::Embedded(id as u64));
            }
            if let Some(id) = num(v, "encodeFailed") {
                return Some(Reply::EncodeFailed(id as u64));
            }
            let rid = num(v, "answer")? as u32;
            let result = if let Some(e) = get(v, "error") {
                Err(e.as_string().unwrap_or_default())
            } else if let Some(mask) = get(v, "mask") {
                Ok(Some(Region {
                    bbox: rect(v)?,
                    mask: Uint8Array::new(&mask).to_vec(),
                }))
            } else {
                Ok(None)
            };
            Some(Reply::Answer { rid, result })
        }
    }
}
