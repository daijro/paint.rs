//! Photocraft phase spec: S§4.3 "Resource limits"
//!
//! A tool's long job (Remove, Patch, Spot Healing, Select Subject) off the UI thread: on native a
//! thread and an `AtomicBool` cancel, as the effect worker's (`dialogs/effect_config/preview.rs`); on wasm
//! `spawn` runs it at once. The UI polls it each frame, shows a busy cursor meanwhile and cancels
//! it on Esc. A [`Job::pending`] job is finished by someone else (the web's MobileSAM worker
//! answering a message).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::interrupt::{Cancelled, Interrupt};

/// One background job returning `T`. Dropping it cancels it.
pub struct Job<T> {
    cancel: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    thread: Option<std::thread::JoinHandle<Result<T, Cancelled>>>,
    #[cfg(target_arch = "wasm32")]
    done: Option<Result<T, Cancelled>>,
    /// A pending job's result, set by its [`Done`].
    slot: Option<Slot<T>>,
}

type Slot<T> = Arc<Mutex<Option<Result<T, Cancelled>>>>;

/// Where a [`Job::pending`] job's result goes.
pub struct Done<T> {
    slot: Slot<T>,
    cancel: Arc<AtomicBool>,
}

impl<T> Done<T> {
    /// The job's result: its next `poll` answers it (`Err(Cancelled)` if it was cancelled).
    pub fn finish(self, result: Result<T, Cancelled>) {
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
    }

    /// The job was cancelled or dropped: its answer is no longer wanted.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl<T: Send + 'static> Job<T> {
    /// Starts `f` with an interrupt that `cancel` sets: on a thread natively, at once on wasm.
    pub fn spawn(f: impl FnOnce(&Interrupt) -> Result<T, Cancelled> + Send + 'static) -> Job<T> {
        let cancel = Arc::new(AtomicBool::new(false));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let flag = cancel.clone();
            let thread = std::thread::spawn(move || f(&Interrupt(Some(&flag))));
            Job {
                cancel,
                thread: Some(thread),
                slot: None,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let done = Some(f(&Interrupt(Some(&cancel))));
            Job {
                cancel,
                done,
                slot: None,
            }
        }
    }

    /// A job someone else finishes through the [`Done`].
    pub fn pending() -> (Job<T>, Done<T>) {
        let (cancel, slot) = (Arc::new(AtomicBool::new(false)), Slot::default());
        let done = Done {
            slot: slot.clone(),
            cancel: cancel.clone(),
        };
        let job = Job {
            cancel,
            #[cfg(not(target_arch = "wasm32"))]
            thread: None,
            #[cfg(target_arch = "wasm32")]
            done: None,
            slot: Some(slot),
        };
        (job, done)
    }

    /// A pending job's result, once its [`Done`] gave it.
    fn take_slot(&mut self) -> Option<Result<T, Cancelled>> {
        let r = self
            .slot
            .as_ref()?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()?;
        self.slot = None;
        Some(r)
    }

    /// The result once the job has stopped, then `None` ever after. A cancelled job answers
    /// `Err(Cancelled)` whatever it computed; so does one that panicked.
    pub fn poll(&mut self) -> Option<Result<T, Cancelled>> {
        if self.slot.is_some() {
            let r = self.take_slot()?;
            return Some(if self.cancel.load(Ordering::Relaxed) {
                Err(Cancelled)
            } else {
                r
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let r = {
            if !self.thread.as_ref()?.is_finished() {
                return None;
            }
            self.thread.take()?.join().unwrap_or(Err(Cancelled))
        };
        #[cfg(target_arch = "wasm32")]
        let r = self.done.take()?;
        Some(if self.cancel.load(Ordering::Relaxed) {
            Err(Cancelled)
        } else {
            r
        })
    }

    /// Waits for the job to stop and gives its result, as `poll` would: Ruling R72's synchronous
    /// commit keeps the work already done.
    pub fn wait(&mut self) -> Result<T, Cancelled> {
        if self.slot.is_some() {
            // ponytail: no waiting on a pending job (nothing commits one synchronously).
            return self.poll().unwrap_or(Err(Cancelled));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let r = self
            .thread
            .take()
            .map_or(Err(Cancelled), |t| t.join().unwrap_or(Err(Cancelled)));
        #[cfg(target_arch = "wasm32")]
        let r = self.done.take().unwrap_or(Err(Cancelled));
        if self.cancel.load(Ordering::Relaxed) {
            Err(Cancelled)
        } else {
            r
        }
    }

    /// The job is still working.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn is_running(&self) -> bool {
        self.pending_open() || self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Only a pending job: on wasm a spawned job ran in `spawn`.
    #[cfg(target_arch = "wasm32")]
    pub fn is_running(&self) -> bool {
        self.pending_open()
    }

    /// A pending job whose result has not come yet.
    fn pending_open(&self) -> bool {
        self.slot
            .as_ref()
            .is_some_and(|s| s.lock().unwrap_or_else(|e| e.into_inner()).is_none())
    }
}

impl<T> Job<T> {
    /// Asks the job to stop; its next `poll` after it stopped answers `Err(Cancelled)`.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extra::inpaint::synthesize_with;
    use crate::extra::remove::remove_with;

    /// Polls `job` until it answers, for at most `secs`.
    fn wait<T: Send + 'static>(job: &mut Job<T>, secs: f64) -> Option<Result<T, Cancelled>> {
        let start = std::time::Instant::now();
        while start.elapsed().as_secs_f64() < secs {
            if let Some(r) = job.poll() {
                return Some(r);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    /// A job's result arrives through `poll`, once.
    #[test]
    fn job_returns_its_result_once() {
        let mut job = Job::spawn(|_| Ok(7));
        assert_eq!(wait(&mut job, 5.0), Some(Ok(7)));
        assert!(!job.is_running());
        assert_eq!(job.poll(), None);
        // `wait` joins it instead; a cancelled job answers `Cancelled`.
        assert_eq!(Job::spawn(|_| Ok(8)).wait(), Ok(8));
        let mut job = Job::spawn(|_| Ok(9));
        job.cancel();
        assert_eq!(job.wait(), Err(Cancelled));
    }

    /// A pending job answers what its `Done` gave, once; cancelled or dropped, the `Done` knows,
    /// and the job answers `Cancelled` whatever comes.
    #[test]
    fn pending_job_answers_its_done() {
        let (mut job, done) = Job::pending();
        assert!(job.is_running() && job.poll().is_none() && !done.cancelled());
        done.finish(Ok(3));
        assert!(!job.is_running());
        assert_eq!(job.poll(), Some(Ok(3)));
        assert_eq!(job.poll(), None);
        let (job, done) = Job::<i32>::pending();
        drop(job);
        assert!(done.cancelled());
        let (mut job, done) = Job::pending();
        job.cancel();
        done.finish(Ok(4));
        assert_eq!(job.poll(), Some(Err(Cancelled)));
    }

    /// S§4.3 "Resource limits", Review Focus 1: a cancel set before a 2048² removal's first
    /// check stops it with `Cancelled` at that check, so it never runs on. No wall clock: the job
    /// waits on a channel for the cancel.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tool_job_cancel_stops_early() {
        let (w, h) = (2048usize, 2048usize);
        let img: Vec<f32> = (0..w * h * 4)
            .map(|i| ((i / 4 % w) ^ (i / 4 / w)) as f32 % 7.0 / 7.0)
            .collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (256..1792).contains(&(i % w)) && (256..1792).contains(&(i / w)))
            .collect();
        let (go, wait_go) = std::sync::mpsc::channel::<()>();
        let (done, stopped) = std::sync::mpsc::channel();
        let mut job = Job::spawn(move |ctl| {
            wait_go.recv().unwrap();
            let r = remove_with(w, h, 4, &img, &hole, 1, ctl);
            done.send(r.as_ref().err().copied()).unwrap();
            r.map(|_| ())
        });
        job.cancel();
        go.send(()).unwrap();
        assert_eq!(stopped.recv().unwrap(), Some(Cancelled));
        assert_eq!(wait(&mut job, 120.0), Some(Err(Cancelled)));
    }

    /// PC12b: the Spot Healing Brush's Create Texture stops too. A cancel set while a 2048²
    /// `synthesize_with` runs (with 6 px blocks a full run takes far longer than this test) stops
    /// it with `Cancelled` at its next candidate or block row; `synthesize` used to run on in the
    /// dropped job. The job reports the function's own result, not `poll`'s.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn texture_job_cancel_stops_early() {
        let (w, h) = (2048usize, 2048usize);
        let img: Vec<f32> = (0..w * h * 4)
            .map(|i| ((i / 4 % w) ^ (i / 4 / w)) as f32 % 7.0 / 7.0)
            .collect();
        let hole: Vec<bool> = (0..w * h)
            .map(|i| (256..1792).contains(&(i % w)) && (256..1792).contains(&(i / w)))
            .collect();
        let (go, wait_go) = std::sync::mpsc::channel::<()>();
        let (done, stopped) = std::sync::mpsc::channel();
        let mut job = Job::spawn(move |ctl| {
            wait_go.recv().unwrap();
            let r = synthesize_with(w, h, 4, &img, &hole, 6, 1, ctl);
            done.send(r.as_ref().err().copied()).unwrap();
            r.map(|_| ())
        });
        go.send(()).unwrap();
        job.cancel();
        assert_eq!(stopped.recv().unwrap(), Some(Cancelled));
        assert_eq!(wait(&mut job, 120.0), Some(Err(Cancelled)));
    }
}
