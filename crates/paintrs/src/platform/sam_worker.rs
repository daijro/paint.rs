//! The web's MobileSAM, app side (`segment::remote::Remote`). A Web Worker (the `sam-worker`
//! crate, started from Trunk's `sam_worker_loader.js`) loads the model from the site and runs the
//! encoder and decoder, so the page stays responsive. It starts the first time Object Selection's
//! toolbar picks MobileSAM, and ends when the toolbar leaves it (freeing the ~0.5 GB it holds once
//! it has encoded). Each snapshot's pixels go to it once (copied into a JS buffer, which is then
//! transferred) and each lookup is a pending job that its answer finishes. If it cannot start
//! (`sam_worker_loader.js` reports a wasm that fails to) or the model does not load, GrabCut
//! answers and the status bar says why. An encode in progress cannot be interrupted: a dropped
//! lookup's answer is ignored.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use js_sys::Array;
use pdn_core::canvas::ImageData;
use pdn_core::extra::interrupt::Cancelled;
use pdn_core::extra::job::{Done, Job};
use pdn_core::extra::segment::remote::message::{self, Reply, Request};
use pdn_core::extra::segment::remote::{Remote, Status};
use pdn_core::extra::segment::{Prompt, Region};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, Worker};

pub struct WorkerClient(Rc<RefCell<Inner>>);

/// Before the worker says how its load goes.
const STARTING: Status = Status::Loading {
    done: 0,
    total: 0,
    cached: true,
};

struct Inner {
    ctx: egui::Context,
    worker: Option<Worker>,
    status: Status,
    /// The snapshot whose pixels went last, the latest the worker encoded, and the latest it
    /// could not.
    sent: Option<u64>,
    embedded: Option<u64>,
    failed: Option<u64>,
    next: u32,
    pending: HashMap<u32, Done<Option<Region>>>,
}

impl WorkerClient {
    pub fn new(ctx: egui::Context) -> WorkerClient {
        WorkerClient(Rc::new(RefCell::new(Inner {
            ctx,
            worker: None,
            status: STARTING,
            sent: None,
            embedded: None,
            failed: None,
            next: 0,
            pending: HashMap::new(),
        })))
    }

    /// Starts the worker unless it runs or could not start.
    fn start(&self) {
        let mut inner = self.0.borrow_mut();
        if inner.worker.is_some() || matches!(inner.status, Status::Failed(_)) {
            return;
        }
        let Ok(worker) = Worker::new("sam_worker_loader.js") else {
            inner.status = Status::Failed("The MobileSAM worker did not start.".into());
            return;
        };
        let weak = Rc::downgrade(&self.0);
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Some(reply) = Reply::from_js(&e.data()) {
                hear(&weak, reply);
            }
        });
        let weak = Rc::downgrade(&self.0);
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            hear(&weak, Reply::Failed("The MobileSAM worker stopped.".into()));
        });
        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        // The worker lives as long as the page.
        on_message.forget();
        on_error.forget();
        inner.worker = Some(worker);
    }
}

/// A message from the worker; the app repaints so the tool polls its jobs.
fn hear(inner: &Weak<RefCell<Inner>>, reply: Reply) {
    let Some(inner) = inner.upgrade() else { return };
    let mut inner = inner.borrow_mut();
    match reply {
        Reply::Loading {
            done,
            total,
            cached,
        } => {
            inner.status = Status::Loading {
                done,
                total,
                cached,
            }
        }
        Reply::Ready => inner.status = Status::Ready,
        Reply::Failed(e) => {
            inner.status = Status::Failed(e);
            for (_, done) in inner.pending.drain() {
                done.finish(Err(Cancelled));
            }
        }
        Reply::Embedded(id) => inner.embedded = Some(id),
        Reply::EncodeFailed(id) => inner.failed = Some(id),
        // A "stale" or failed lookup lands nothing.
        Reply::Answer { rid, result } => {
            if let Some(done) = inner.pending.remove(&rid) {
                done.finish(result.map_err(|_| Cancelled));
            }
        }
    }
    inner.ctx.request_repaint();
}

fn send(worker: &Worker, msg: &JsValue, transfer: Option<js_sys::ArrayBuffer>) {
    let _ = match transfer {
        Some(b) => worker.post_message_with_transfer(msg, &Array::of1(&b)),
        None => worker.post_message(msg),
    };
}

impl Remote for WorkerClient {
    fn status(&self) -> Status {
        self.start();
        self.0.borrow().status.clone()
    }

    fn lookup(&self, snapshot: u64, pixels: &ImageData, prompt: Prompt) -> Job<Option<Region>> {
        self.start();
        let (job, done) = Job::pending();
        let mut inner = self.0.borrow_mut();
        let Some(worker) = inner.worker.clone() else {
            done.finish(Err(Cancelled));
            return job;
        };
        // Lookups dropped meanwhile (Escape, a newer snapshot) need no answer.
        inner.pending.retain(|_, d| !d.cancelled());
        if inner.sent != Some(snapshot) {
            let (msg, rgba) = message::embed(snapshot, pixels);
            send(&worker, &msg, Some(rgba));
            inner.sent = Some(snapshot);
        }
        let rid = inner.next;
        inner.next = inner.next.wrapping_add(1);
        inner.pending.insert(rid, done);
        let (msg, _) = Request::Decode {
            rid,
            id: snapshot,
            prompt,
        }
        .to_js();
        send(&worker, &msg, None);
        job
    }

    fn embedded(&self, snapshot: u64) -> bool {
        self.0.borrow().embedded == Some(snapshot)
    }

    fn failed(&self, snapshot: u64) -> bool {
        self.0.borrow().failed == Some(snapshot)
    }

    /// Also clears a failure, so picking MobileSAM again tries again (a network error, say).
    fn stop(&self) {
        let mut inner = self.0.borrow_mut();
        if let Some(worker) = inner.worker.take() {
            worker.terminate();
        } else if !matches!(inner.status, Status::Failed(_)) {
            return;
        }
        for (_, done) in inner.pending.drain() {
            done.finish(Err(Cancelled));
        }
        (inner.status, inner.sent, inner.embedded, inner.failed) = (STARTING, None, None, None);
    }
}
