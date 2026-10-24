//! Thumbnail scaling off the frame, shared by the document strip and the Layers form.
//! Paint.NET's thumbnail renderer: one background thread, rendering no earlier than 200 ms after
//! the last change.
//!
//! The scaling is `pdn_core::render::thumbnail` (`scale_thumbnail` / `ThumbnailScaler`); this
//! module only decides where and when it runs. One job at a time per `Scaler`.
use pdn_core::render::thumbnail::{ThumbnailScaler, scale_thumbnail};

/// `ui-drawn-graphics.md §2.8` Scheduling: a thumbnail renders no earlier than 200 ms after its
/// source last changed.
pub const THUMBNAIL_LATENCY_S: f64 = 0.2;
/// `ThumbnailMode::Sliced`: scaling time per frame, checked after every row pass (an 8000-px
/// source row takes ~0.2 ms).
const SLICE_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);
const SLICE_ROWS: usize = 1;

/// §2.8 Scheduling for one source: `seen` holds the last revision and when it changed (s). True
/// once 200 ms have passed since the last change (a new change restarts the wait); a source's
/// first revision is due at once.
pub fn due(seen: &mut Option<(u64, f64)>, revision: u64, now_s: f64) -> bool {
    let (rev, changed_at) = seen.get_or_insert((revision, f64::NEG_INFINITY));
    if *rev != revision {
        *rev = revision;
        *changed_at = now_s;
    }
    now_s - *changed_at >= THUMBNAIL_LATENCY_S
}

/// Where thumbnails are scaled (`ui-drawn-graphics.md §2.8` Scheduling: Paint.NET
/// renders them on its own thread).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbnailMode {
    /// In the frame that needs one (tests: deterministic frames).
    Synchronous,
    /// On one worker thread (native builds).
    Worker,
    /// A few rows a frame within `SLICE_BUDGET`, reading the live source (the web build has no
    /// threads; `Worker` there means this).
    Sliced,
}

impl Default for ThumbnailMode {
    fn default() -> Self {
        if cfg!(target_arch = "wasm32") {
            ThumbnailMode::Sliced
        } else {
            ThumbnailMode::Worker
        }
    }
}

/// A job's owner (a document or layer id) and key (source revision, device max edge).
pub type JobKey = (u64, (u64, u32));

/// `scale_thumbnail`'s result: RGBA8, width, height.
pub type Scaled8 = (Vec<u8>, u32, u32);

/// A scaled thumbnail: its owner, key and result.
pub type Done = (u64, (u64, u32), Scaled8);

/// A worker job: its owner and key, and a snapshot of the source (the reused buffer, returned
/// with the result).
#[cfg(not(target_arch = "wasm32"))]
struct Job {
    owner: u64,
    key: (u64, u32),
    src: Vec<u8>,
    size: [u32; 2],
}

/// The worker's ends: jobs in, finished jobs (their buffers) with results out.
#[cfg(not(target_arch = "wasm32"))]
type WorkerEnds = (
    std::sync::mpsc::Sender<Job>,
    std::sync::mpsc::Receiver<(Job, Scaled8)>,
);

/// One thumbnail at a time off the frame (`mode`). The worker gets a snapshot of the source (the
/// UI thread only copies it into the reused `spare` buffer); a sliced job reads the live source
/// each frame and is dropped once its source is gone or changed, so the web build copies nothing.
#[derive(Default)]
pub struct Scaler {
    mode: ThumbnailMode,
    /// The worker's snapshot buffer while no job holds it.
    spare: Vec<u8>,
    /// The job in flight.
    busy: Option<JobKey>,
    sliced: Option<(JobKey, ThumbnailScaler)>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<WorkerEnds>,
}

impl Scaler {
    pub fn set_mode(&mut self, mode: ThumbnailMode) {
        self.mode = mode;
    }

    /// A job is in flight (its result asks for a frame).
    pub fn busy(&self) -> bool {
        self.busy.is_some()
    }

    /// Scales `src` (`size`) for `owner` at `key` (revision, device max edge): at once in
    /// `Synchronous` mode (the result), else queued (`poll` returns it).
    pub fn start(
        &mut self,
        ctx: &egui::Context,
        owner: u64,
        key: (u64, u32),
        src: &[u8],
        size: [u32; 2],
    ) -> Option<Done> {
        let scale = || (owner, key, scale_thumbnail(src, size[0], size[1], key.1));
        match self.mode {
            ThumbnailMode::Synchronous => return Some(scale()),
            #[cfg(not(target_arch = "wasm32"))]
            ThumbnailMode::Worker => {
                if self.worker.is_none() {
                    self.worker = spawn_worker(ctx.clone());
                }
                if let Some((jobs, _)) = &self.worker {
                    let mut snapshot = std::mem::take(&mut self.spare);
                    snapshot.clear();
                    snapshot.extend_from_slice(src);
                    let job = Job {
                        owner,
                        key,
                        src: snapshot,
                        size,
                    };
                    match jobs.send(job) {
                        Ok(()) => {
                            self.busy = Some((owner, key));
                            return None;
                        }
                        // The worker is gone (it panicked): slice from now on.
                        Err(e) => self.fail_over(e.0.src),
                    }
                } else {
                    // No thread: slice.
                    self.mode = ThumbnailMode::Sliced;
                }
            }
            _ => {}
        }
        self.busy = Some((owner, key));
        self.sliced = Some((
            (owner, key),
            ThumbnailScaler::new(src, size[0], size[1], key.1),
        ));
        ctx.request_repaint();
        None
    }

    /// The finished job, if any: the worker's result, or this frame's slice of the sliced one.
    /// `source(owner, key)` is the sliced job's live source while it is still wanted (`None`
    /// drops the job: its document closed or its source changed).
    pub fn poll<'a>(
        &mut self,
        ctx: &egui::Context,
        source: impl Fn(u64, (u64, u32)) -> Option<&'a [u8]>,
    ) -> Option<Done> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((_, results)) = &self.worker {
            match results.try_recv() {
                Ok((job, out)) => {
                    self.spare = job.src;
                    self.busy = None;
                    return Some((job.owner, job.key, out));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                // The worker panicked mid-job (its sender dropped): drop it, slice from now on;
                // the lost job is started again by its owner.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.fail_over(Vec::new());
                    ctx.request_repaint();
                }
            }
        }
        let ((owner, key), scaler) = self.sliced.as_mut()?;
        let (owner, key) = (*owner, *key);
        let Some(src) = source(owner, key) else {
            // Stale or closed: cancel.
            self.sliced = None;
            self.busy = None;
            ctx.request_repaint();
            return None;
        };
        let start = web_time::Instant::now();
        while !scaler.step(src, SLICE_ROWS) {
            if start.elapsed() >= SLICE_BUDGET {
                ctx.request_repaint();
                return None;
            }
        }
        let (_, scaler) = self.sliced.take()?;
        self.busy = None;
        Some((owner, key, scaler.finish(src)))
    }

    /// Releases the snapshot buffer when it is larger than the largest source still open
    /// (`needed` bytes); the next worker job allocates what it needs.
    pub fn trim(&mut self, needed: usize) {
        if self.spare.capacity() > needed {
            self.spare = Vec::new();
        }
    }

    /// The snapshot buffer's capacity (bytes held while no job runs).
    #[cfg(test)]
    pub(crate) fn spare_capacity(&self) -> usize {
        self.spare.capacity()
    }

    /// The worker latch: no worker, no job in flight, sliced scaling from now on (never
    /// `Synchronous`, which would stall the frame on large sources).
    #[cfg(not(target_arch = "wasm32"))]
    fn fail_over(&mut self, buffer: Vec<u8>) {
        self.worker = None;
        self.busy = None;
        self.mode = ThumbnailMode::Sliced;
        self.spare = buffer;
    }
}

/// The worker thread: scales each job, posts it back with its buffer and asks for a frame. It
/// ends when its `Scaler` (the sender) goes. `None` when the thread cannot start.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_worker(ctx: egui::Context) -> Option<WorkerEnds> {
    let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
    let (outbox, results) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("paintrs-thumbnails".into())
        .spawn(move || {
            for job in inbox {
                let [w, h] = job.size;
                let out = scale_thumbnail(&job.src, w, h, job.key.1);
                if outbox.send((job, out)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .ok()?;
    Some((jobs, results))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ui-drawn-graphics.md §2.8 Scheduling.
    #[test]
    fn thumbnails_wait_200_ms_after_the_last_change() {
        let mut seen = None;
        assert!(
            due(&mut seen, 3, 10.0),
            "the first thumbnail is due at once"
        );
        assert!(due(&mut seen, 3, 10.01));
        assert!(!due(&mut seen, 4, 11.0), "a change waits");
        assert!(!due(&mut seen, 4, 11.19));
        assert!(!due(&mut seen, 5, 11.15), "a new change restarts the wait");
        assert!(!due(&mut seen, 5, 11.3));
        assert!(due(&mut seen, 5, 11.36));
    }

    fn noise(w: u32, h: u32) -> Vec<u8> {
        (0..w * h * 4)
            .map(|i| (i.wrapping_mul(2654435761) >> 11) as u8)
            .collect()
    }

    fn wait_for(s: &mut Scaler, ctx: &egui::Context, src: &[u8]) -> Done {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(done) = s.poll(ctx, |_, _| Some(src)) {
                return done;
            }
            assert!(std::time::Instant::now() < deadline, "never finished");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Task 18 fix round 1: the worker and the sliced scale give the synchronous pixels; the
    /// worker's snapshot buffer comes back for the next job, and `trim` releases it.
    #[test]
    fn background_thumbnails_match_the_synchronous_scale() {
        let (w, h) = (1203u32, 641u32);
        let src = noise(w, h);
        let expected = scale_thumbnail(&src, w, h, 60);
        let ctx = egui::Context::default();
        for mode in [ThumbnailMode::Worker, ThumbnailMode::Sliced] {
            let mut s = Scaler::default();
            s.set_mode(mode);
            assert!(
                s.start(&ctx, 7, (3, 60), &src, [w, h]).is_none(),
                "{mode:?}"
            );
            let (owner, key, out) = wait_for(&mut s, &ctx, &src);
            assert_eq!((owner, key), (7, (3, 60)));
            assert!(out == expected, "{mode:?}: not the synchronous pixels");
            assert!(!s.busy(), "{mode:?}");
            if mode == ThumbnailMode::Worker {
                assert!(s.spare.capacity() >= src.len());
                s.trim(src.len());
                assert!(s.spare.capacity() >= src.len(), "still needed");
                s.trim(src.len() / 2);
                assert_eq!(s.spare.capacity(), 0, "released");
            } else {
                assert_eq!(s.spare.capacity(), 0, "slicing copies nothing");
            }
        }
    }

    /// Task 19 carry: a sliced job whose source is gone (closed document, newer revision) is
    /// cancelled in the poll.
    #[test]
    fn sliced_jobs_cancel_when_their_source_goes() {
        let ctx = egui::Context::default();
        let src = noise(800, 600);
        let mut s = Scaler::default();
        s.set_mode(ThumbnailMode::Sliced);
        assert!(s.start(&ctx, 7, (3, 60), &src, [800, 600]).is_none());
        assert!(s.busy());
        assert!(s.poll(&ctx, |_, _| None).is_none());
        assert!(!s.busy() && s.sliced.is_none());
    }

    /// Task 19 carry: a worker that died (its result channel disconnected) is dropped, the job
    /// is no longer busy, and later jobs are sliced.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_dead_worker_fails_over_to_slicing() {
        let ctx = egui::Context::default();
        let src = noise(64, 48);
        let mut s = Scaler::default();
        s.set_mode(ThumbnailMode::Worker);
        // A worker whose thread is already gone.
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let (outbox, results) = std::sync::mpsc::channel::<(Job, Scaled8)>();
        s.worker = Some((jobs, results));
        assert!(s.start(&ctx, 1, (1, 32), &src, [64, 48]).is_none());
        assert!(s.busy());
        drop((inbox, outbox));
        assert!(s.poll(&ctx, |_, _| Some(&src[..])).is_none());
        assert!(!s.busy() && s.worker.is_none());
        assert_eq!(s.mode, ThumbnailMode::Sliced);
        // The next job slices.
        assert!(s.start(&ctx, 1, (1, 32), &src, [64, 48]).is_none());
        let (_, _, out) = wait_for(&mut s, &ctx, &src);
        assert!(out == scale_thumbnail(&src, 64, 48, 32));
        // A send to a dead worker slices too.
        let mut s = Scaler::default();
        s.set_mode(ThumbnailMode::Worker);
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let (_outbox, results) = std::sync::mpsc::channel::<(Job, Scaled8)>();
        drop(inbox);
        s.worker = Some((jobs, results));
        assert!(s.start(&ctx, 1, (1, 32), &src, [64, 48]).is_none());
        assert!(s.worker.is_none() && s.sliced.is_some());
    }
}
