//! Owns this test binary's global Rayon pool; do not combine with unrelated tests.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

use pdn_core::pixel::IntRect;
use pdn_core::render::display::{DisplayTarget, RenderMode};

struct ReleaseWorkers(Vec<mpsc::Sender<()>>);

impl Drop for ReleaseWorkers {
    fn drop(&mut self) {
        for worker in &self.0 {
            let _ = worker.send(());
        }
    }
}

#[test]
fn empty_spans_complete_while_global_workers_are_blocked() {
    const WORKERS: usize = 2;
    rayon::ThreadPoolBuilder::new()
        .num_threads(WORKERS)
        .build_global()
        .unwrap();
    let target = DisplayTarget::new(100, 80, RenderMode::Hardware);
    let sentinel = 0x1234_5678u32;
    let mut pixels = vec![sentinel; 100 * 80];
    let calls = Arc::new(AtomicUsize::new(0));
    let (started, ready) = mpsc::channel();
    let mut release = ReleaseWorkers(vec![]);
    for _ in 0..WORKERS {
        let (unblock, blocked) = mpsc::channel();
        release.0.push(unblock);
        let started = started.clone();
        rayon::spawn(move || {
            let _ = started.send(());
            let _ = blocked.recv();
        });
    }
    for _ in 0..WORKERS {
        ready
            .recv_timeout(Duration::from_secs(30))
            .expect("worker started");
    }
    let (completed, completion) = mpsc::channel();
    let callbacks = calls.clone();
    let caller = std::thread::spawn(move || {
        target.write_rgba8_over(
            IntRect::new(0, 0, 100, 80),
            Some(&[]),
            &mut pixels,
            |p| {
                callbacks.fetch_add(1, Ordering::Relaxed);
                u32::from_le_bytes(p)
            },
            |_, _, _| {
                callbacks.fetch_add(1, Ordering::Relaxed);
            },
        );
        let _ = completed.send(());
        pixels
    });
    // This is a deadlock escape, not a timing assertion: no Rayon worker can run until
    // after we receive the completion. Always release workers, even on the red path.
    let before_release = completion.recv_timeout(Duration::from_secs(30));
    drop(release);
    let pixels = caller.join().unwrap();
    assert_eq!(pixels, vec![sentinel; 100 * 80]);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(
        before_release.is_ok(),
        "empty-span write waited for the blocked global pool"
    );
}
