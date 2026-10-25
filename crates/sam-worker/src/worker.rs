use std::cell::RefCell;
use std::collections::VecDeque;

use js_sys::{Array, Reflect, Uint8Array};
use pdn_core::canvas::ImageData;
use pdn_core::extra::geom::Rect;
use pdn_core::extra::interrupt::Interrupt;
use pdn_core::extra::segment::remote::message::{Reply, Request};
use pdn_core::extra::segment::sam::{Embedding, Model, SHA256, SIZE};
use pdn_core::extra::segment::{Prompt, Region, Sampler};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Cache, DedicatedWorkerGlobalScope, MessageEvent, ReadableStreamDefaultReader, RequestCache,
    RequestInit, Response, Url,
};

/// The weights as the site serves them, in parts: `…-<the SHA-256's first 8 hex digits>
/// .safetensors.0`, `.1`, … (Cloudflare Pages takes files of up to 25 MiB). Trunk's hook writes
/// them (`tools/model-parts.py`), and only it knows the part size: the worker reads parts until it
/// has [`SIZE`] bytes. The SHA in the name versions them, so a host may cache them for good.
fn part_url(part: usize) -> String {
    format!(
        "models/mobilesam/mobile_sam-tiny-vitt-{}.safetensors.{part}",
        &SHA256[..8]
    )
}

/// The Cache API's key for the joined weights (every hit is checked against the pinned SHA-256).
const KEY: &str = "models/mobilesam/mobile_sam-tiny-vitt.safetensors";

/// A snapshot's straight RGBA8 pixels.
struct Pixels(ImageData);

impl Sampler for Pixels {
    fn rgba(&self, r: Rect) -> Vec<[f32; 4]> {
        let (w, h) = (self.0.width as i32, self.0.height as i32);
        let px = self.0.data.as_chunks::<4>().0;
        let mut out = Vec::with_capacity(r.width() as usize * r.height() as usize);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                out.push(if (0..w).contains(&x) && (0..h).contains(&y) {
                    px[y as usize * w as usize + x as usize].map(|v| f32::from(v) / 255.0)
                } else {
                    [0.0; 4]
                });
            }
        }
        out
    }
}

/// The worker's state. Known limit: once it has encoded, the worker's wasm memory holds about
/// 0.5 GB (it never shrinks) until the app ends the worker, when the toolbar leaves MobileSAM.
#[derive(Default)]
struct State {
    model: Option<Model>,
    failed: Option<String>,
    /// The latest snapshot encoded: its id, embedding and pixels (Select Subject reads them).
    embedding: Option<(u64, Embedding, Pixels)>,
    queue: VecDeque<Request>,
    scheduled: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn scope() -> DedicatedWorkerGlobalScope {
    js_sys::global().unchecked_into()
}

fn post(reply: Reply) {
    let (msg, transfer) = reply.to_js();
    let _ = match transfer {
        Some(b) => scope().post_message_with_transfer(&msg, &Array::of1(&b)),
        None => scope().post_message(&msg),
    };
}

pub fn run() {
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(|e: MessageEvent| {
        if let Some(r) = Request::from_js(&e.data()) {
            STATE.with_borrow_mut(|s| {
                // Only the latest snapshot counts: a newer one drops those not yet encoded (their
                // lookups then answer "stale").
                if matches!(r, Request::Embed { .. }) {
                    s.queue.retain(|q| !matches!(q, Request::Embed { .. }));
                }
                s.queue.push_back(r);
            });
            schedule();
        }
    });
    scope().set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    wasm_bindgen_futures::spawn_local(async {
        match load().await {
            Ok(model) => {
                STATE.with_borrow_mut(|s| s.model = Some(model));
                post(Reply::Ready);
            }
            Err(e) => {
                STATE.with_borrow_mut(|s| s.failed = Some(e.clone()));
                post(Reply::Failed(e));
            }
        }
        schedule();
    });
}

/// Handles the queue in a later task, once the messages that came during an encode have all
/// arrived: only the latest snapshot is then encoded.
fn schedule() {
    if STATE.with_borrow_mut(|s| std::mem::replace(&mut s.scheduled, true)) {
        return;
    }
    let drain = Closure::once_into_js(drain);
    let _ = scope().set_timeout_with_callback(drain.unchecked_ref());
}

fn drain() {
    let queue = STATE.with_borrow_mut(|s| {
        s.scheduled = false;
        // Until the model is in (or failed), the requests wait.
        if s.model.is_some() || s.failed.is_some() {
            std::mem::take(&mut s.queue)
        } else {
            VecDeque::new()
        }
    });
    let latest = queue.iter().rev().find_map(|r| match r {
        Request::Embed { id, .. } => Some(*id),
        Request::Decode { .. } => None,
    });
    for r in queue {
        match r {
            Request::Embed { id, pixels } if Some(id) == latest => embed(id, pixels),
            // A newer snapshot came: its lookups answer "stale".
            Request::Embed { .. } => {}
            Request::Decode { rid, id, prompt } => post(Reply::Answer {
                rid,
                result: decode(id, prompt),
            }),
        }
    }
}

fn embed(id: u64, pixels: ImageData) {
    STATE.with_borrow_mut(|s| {
        let Some(model) = &s.model else { return };
        let pixels = Pixels(pixels);
        let canvas = Rect::new(0, 0, pixels.0.width as i32, pixels.0.height as i32);
        // Known limit: an encode cannot be interrupted (the app ignores a dropped lookup's answer).
        s.embedding = match model.embed(&pixels, canvas, &Interrupt::NONE) {
            Ok(e) => Some((id, e, pixels)),
            Err(_) => None,
        };
        // An encoder error: the app's GrabCut answers on these pixels.
        post(if s.embedding.is_some() {
            Reply::Embedded(id)
        } else {
            Reply::EncodeFailed(id)
        });
    });
}

fn decode(id: u64, prompt: Prompt) -> Result<Option<Region>, String> {
    STATE.with_borrow(|s| {
        let (Some(model), Some((eid, e, pixels))) = (&s.model, &s.embedding) else {
            return Err(s.failed.clone().unwrap_or_else(|| "not analysed".into()));
        };
        if *eid != id {
            return Err("stale".into());
        }
        match prompt {
            Prompt::Rect(r) => model.object_in_rect(e, r),
            Prompt::Point(x, y) => model.object_at(e, x, y),
            Prompt::Subject => model.subject(e, pixels),
        }
        .map_err(|e| e.to_string())
    })
}

async fn load() -> Result<Model, String> {
    let bytes = weights().await?;
    Model::load(&bytes).map_err(|_| "The MobileSAM model did not load.".to_owned())
}

/// The weights from the Cache API, else downloaded (progress posted), checked against their size
/// and SHA-256 and then cached. A damaged download (a host serving an old part) is tried once more
/// past the HTTP cache. The Cache API exists in secure contexts only (HTTPS, localhost): elsewhere
/// `caches` is undefined, the weights are used uncached, and the loading line says so.
async fn weights() -> Result<Vec<u8>, String> {
    let scope = scope();
    let caches = scope.caches().ok().filter(|c| !c.is_undefined());
    let key = Url::new_with_base(KEY, &scope.location().href())
        .map_err(|_| "The MobileSAM model could not be downloaded.".to_owned())?
        .href();
    if let Some(caches) = &caches
        && let Ok(hit) = JsFuture::from(caches.match_with_str(&key)).await
        && let Ok(hit) = hit.dyn_into::<Response>()
        && let Ok(buffer) = hit.array_buffer()
        && let Ok(buffer) = JsFuture::from(buffer).await
    {
        let bytes = Uint8Array::new(&buffer).to_vec();
        if verify(&bytes) {
            return Ok(bytes);
        }
    }
    let cached = caches.is_some();
    let mut bytes = download(cached, false).await?;
    if !verify(&bytes) {
        bytes = download(cached, true).await?;
        if !verify(&bytes) {
            return Err("The MobileSAM model's download is damaged.".to_owned());
        }
    }
    if let Some(caches) = &caches
        && let Ok(cache) = JsFuture::from(caches.open("paintrs-models")).await
        && let Ok(copy) = Response::new_with_opt_buffer_source(Some(&Uint8Array::from(&bytes[..])))
    {
        let _ = JsFuture::from(cache.unchecked_into::<Cache>().put_with_str(&key, &copy)).await;
    }
    Ok(bytes)
}

/// The parts joined, until [`SIZE`] bytes came (a missing part before that fails); `reload`
/// bypasses the HTTP cache.
async fn download(cached: bool, reload: bool) -> Result<Vec<u8>, String> {
    let failed = |_| "The MobileSAM model could not be downloaded.".to_owned();
    let init = RequestInit::new();
    if reload {
        init.set_cache(RequestCache::Reload);
    }
    let mut bytes = Vec::with_capacity(SIZE as usize);
    let mut shown = 0;
    post(Reply::Loading {
        done: 0,
        total: SIZE,
        cached,
    });
    let mut part = 0;
    while (bytes.len() as u64) < SIZE {
        let response = JsFuture::from(scope().fetch_with_str_and_init(&part_url(part), &init))
            .await
            .map_err(failed)?
            .dyn_into::<Response>()
            .map_err(failed)?;
        let reader = response
            .ok()
            .then(|| response.body())
            .flatten()
            .ok_or_else(|| failed(JsValue::NULL))?
            .get_reader()
            .unchecked_into::<ReadableStreamDefaultReader>();
        let before = bytes.len();
        loop {
            let chunk = JsFuture::from(reader.read()).await.map_err(failed)?;
            if Reflect::get(&chunk, &"done".into()).is_ok_and(|d| d.is_truthy()) {
                break;
            }
            let value = Reflect::get(&chunk, &"value".into()).map_err(failed)?;
            bytes.extend(Uint8Array::new(&value).to_vec());
            if bytes.len() as u64 > SIZE {
                return Err(failed(JsValue::NULL));
            }
            // A message per percent.
            let percent = bytes.len() as u64 * 100 / SIZE;
            if percent > shown {
                shown = percent;
                post(Reply::Loading {
                    done: bytes.len() as u64,
                    total: SIZE,
                    cached,
                });
            }
        }
        if bytes.len() == before {
            return Err(failed(JsValue::NULL));
        }
        part += 1;
    }
    Ok(bytes)
}

/// The bytes are the pinned weights: their size and SHA-256 match.
fn verify(bytes: &[u8]) -> bool {
    use sha2::Digest;
    let hex: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    bytes.len() as u64 == SIZE && hex == SHA256
}
