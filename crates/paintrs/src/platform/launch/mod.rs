//! Process-owned native launch queue and single-instance IPC. Port of desktop/main.js:133–190.
//! R114/R116/R130: callbacks enqueue only; PaintApp retains Task50 file and Task45 close ownership.
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
mod wire;
#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;
#[cfg(target_os = "macos")]
pub mod macos;
use crate::shell::window_close::{self, Intent};
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Stream, prelude::*};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
const DEADLINE: Duration = Duration::from_secs(2);
const RETRIES: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Pending {
    paths: VecDeque<Vec<PathBuf>>,
    activate: bool,
    quit: bool,
    outcome: Option<Result<Intent, ()>>,
    exiting: bool,
    reviewing_exit: bool,
    seen: VecDeque<[u8; 16]>,
    ctx: Option<egui::Context>,
    idle_wake: Option<Arc<dyn Fn() + Send + Sync>>,
    native_attention: Option<Arc<dyn Fn() + Send + Sync>>,
}
#[derive(Clone, Default)]
pub struct Bridge(Arc<Mutex<Pending>>);
impl Bridge {
    fn wake(p: &Pending) {
        if p.activate
            && let Some(attention) = &p.native_attention
        {
            attention();
        }
        if let Some(ctx) = &p.ctx {
            ctx.request_repaint();
        } else if let Some(wake) = &p.idle_wake {
            wake();
        }
    }
    fn enqueue(&self, batch: wire::Batch) -> io::Result<()> {
        let mut p = self
            .0
            .lock()
            .map_err(|_| io::Error::other("launch queue unavailable"))?;
        if p.exiting || p.reviewing_exit {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "paint.rs is exiting; launch was not accepted",
            ));
        }
        if p.seen.contains(&batch.id) {
            return Ok(());
        }
        if p.paths.len() >= 64 {
            return Err(io::Error::other(
                "launch queue full; batch was not accepted",
            ));
        }
        p.seen.push_back(batch.id);
        if p.seen.len() > 1024 {
            p.seen.pop_front();
        }
        if !batch.paths.is_empty() {
            p.paths.push_back(batch.paths);
        }
        p.activate = true;
        Self::wake(&p);
        Ok(())
    }
    pub fn launch(&self, paths: Vec<PathBuf>) -> io::Result<()> {
        let batch = wire::Batch {
            id: *uuid::Uuid::new_v4().as_bytes(),
            paths,
        };
        wire::encode(&batch)?;
        self.enqueue(batch)
    }
    pub fn request_quit(&self) {
        if let Ok(mut p) = self.0.lock()
            && !p.exiting
        {
            p.quit = true;
            p.activate = true;
            Self::wake(&p);
        }
    }
    pub fn pending_paths(&self) -> bool {
        self.0.lock().is_ok_and(|p| !p.paths.is_empty())
    }
    pub fn attach(&self, ctx: &egui::Context) {
        if let Ok(mut p) = self.0.lock() {
            p.ctx = Some(ctx.clone());
            Self::wake(&p);
        }
        let bridge = self.clone();
        window_close::set_request_notifier(
            ctx,
            window_close::RequestNotifier(Arc::new(move |intent| {
                if let Ok(mut p) = bridge.0.lock() {
                    p.reviewing_exit = intent == Intent::ExitProcess || !cfg!(target_os = "macos");
                }
            })),
        );
        let bridge = self.clone();
        window_close::set_notifier(
            ctx,
            window_close::Notifier(Arc::new(move |outcome| {
                if let Ok(mut p) = bridge.0.lock() {
                    if outcome == Ok(Intent::ExitProcess) && !p.paths.is_empty() {
                        // A Quit upgraded a root-close review after launch acceptance. Finish
                        // document delivery before taking a new exit snapshot, without replying success.
                        p.quit = true;
                        Self::wake(&p);
                        return false;
                    }
                    p.outcome = Some(outcome);
                    if matches!(outcome, Ok(Intent::ExitProcess))
                        || (!cfg!(target_os = "macos") && outcome.is_ok())
                    {
                        p.exiting = true;
                    }
                    if outcome.is_err() {
                        p.quit = false;
                        p.reviewing_exit = false;
                    }
                    Self::wake(&p);
                    return true;
                }
                false
            })),
        );
    }
    pub fn return_pending(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if let Ok(mut p) = self.0.lock() {
            p.paths.push_front(paths);
        }
    }
    /// Wayland may stop rendering a minimized surface. Request supported compositor attention
    /// from the IPC wake path, without waiting for a UI frame. A weak handle cannot keep an
    /// editor window alive; compositor policy still decides whether attention activates it.
    #[cfg(target_os = "linux")]
    pub fn attach_wayland_attention(&self, window: &Arc<winit::window::Window>) {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if !matches!(
            window.window_handle().map(|h| h.as_raw()),
            Ok(RawWindowHandle::Wayland(_))
        ) {
            return;
        }
        let weak = Arc::downgrade(window);
        if let Ok(mut p) = self.0.lock() {
            p.native_attention = Some(Arc::new(move || {
                if let Some(window) = weak.upgrade() {
                    window.request_user_attention(Some(
                        winit::window::UserAttentionType::Informational,
                    ));
                }
            }));
        }
    }
    pub fn detach(&self) {
        if let Ok(mut p) = self.0.lock() {
            p.ctx = None;
            p.native_attention = None;
        }
    }
    pub fn set_idle_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut p) = self.0.lock() {
            p.idle_wake = Some(wake);
        }
    }
    pub fn needs_editor(&self) -> bool {
        self.0
            .lock()
            .is_ok_and(|p| p.activate || !p.paths.is_empty())
    }
    pub fn quitting(&self) -> bool {
        self.0.lock().is_ok_and(|p| p.quit || p.exiting)
    }
    pub fn outcome(&self) -> Option<Result<Intent, ()>> {
        self.0.lock().ok().and_then(|p| p.outcome)
    }
    pub fn clear_outcome(&self) {
        if let Ok(mut p) = self.0.lock() {
            p.outcome = None;
        }
    }
    /// Commit windowless quit only while the same lock excludes launch acceptance.
    /// An already acknowledged path instead requires another editor delivery cycle.
    pub fn try_finish_idle_quit(&self) -> bool {
        let Ok(mut p) = self.0.lock() else {
            return false;
        };
        if (p.quit || p.exiting) && p.paths.is_empty() {
            p.exiting = true;
            true
        } else {
            false
        }
    }
    pub fn stop_accepting(&self) {
        if let Ok(mut p) = self.0.lock() {
            p.exiting = true;
        }
    }
    pub fn deliver(&self, app: &mut crate::PaintApp, ctx: &egui::Context) {
        let Ok(mut p) = self.0.lock() else {
            return;
        };
        let close = window_close::state(ctx);
        let batches = if close.reviewing || matches!(close.outcome, Some(Ok(_))) {
            VecDeque::new()
        } else {
            std::mem::take(&mut p.paths)
        };
        let activate = std::mem::take(&mut p.activate);
        let quit = std::mem::take(&mut p.quit);
        drop(p);
        for files in batches {
            app.queue_incoming_files(files);
        }
        if activate {
            for command in [
                egui::ViewportCommand::Minimized(false),
                egui::ViewportCommand::Visible(true),
                egui::ViewportCommand::Focus,
            ] {
                ctx.send_viewport_cmd(command);
            }
        }
        if quit {
            window_close::request(ctx, Intent::ExitProcess);
        }
    }
}

/// Holds the lock throughout editor and macOS windowless lifetimes. ACK means queued only.
pub struct Instance {
    _lock: File,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    bridge: Bridge,
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.bridge.stop_accepting();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Instance {
    pub fn start(paths: Vec<PathBuf>, bridge: Bridge) -> io::Result<Option<Self>> {
        Self::start_at(&os::private_directory()?, paths, bridge)
    }
    /// What `main` does with `start`'s result: `None` exits (another instance accepted the
    /// files); `Some(owner)` runs the editor. A failed setup fails closed (nothing is sent to an
    /// endpoint that failed its checks) yet still runs: standalone, with no IPC (`owner` is
    /// `None`), its own files queued on the in-process bridge.
    pub fn settle(
        started: io::Result<Option<Self>>,
        paths: Vec<PathBuf>,
        bridge: &Bridge,
    ) -> Option<Option<Self>> {
        match started {
            Ok(owner) => owner.map(Some),
            Err(e) => {
                eprintln!("paint.rs: single-instance launch failed, running standalone: {e}");
                if let Err(e) = bridge.launch(paths) {
                    eprintln!("paint.rs: command-line files were not opened: {e}");
                }
                Some(None)
            }
        }
    }
    fn start_at(dir: &Path, paths: Vec<PathBuf>, bridge: Bridge) -> io::Result<Option<Self>> {
        let lock = os::lock_file(dir)?;
        let batch = wire::Batch {
            id: *uuid::Uuid::new_v4().as_bytes(),
            paths,
        };
        let payload = wire::encode(&batch)?;
        let endpoint = os::endpoint(dir)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let until = Instant::now() + RETRIES;
                loop {
                    match forward(dir, &payload, until.min(Instant::now() + DEADLINE)) {
                        Ok(()) => return Ok(None),
                        Err(e)
                            if e.kind() == io::ErrorKind::PermissionDenied
                                || Instant::now() >= until =>
                        {
                            return Err(e);
                        }
                        Err(_) => thread::sleep(
                            until
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(25)),
                        ),
                    }
                }
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
        os::remove_stale(dir)?;
        let options = os::secure_listener(
            ListenerOptions::new()
                .name(endpoint.clone())
                .nonblocking(ListenerNonblockingMode::Both),
        )?;
        let listener = options.create_sync()?;
        bridge.enqueue(batch)?;
        let stop = Arc::new(AtomicBool::new(false));
        let running = stop.clone();
        let incoming = bridge.clone();
        let worker = thread::Builder::new()
            .name("paint.rs launches".into())
            .spawn(move || {
                while !running.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok(mut stream) => {
                            let until = Instant::now() + DEADLINE;
                            let result = receive(&mut stream, until)
                                .and_then(|batch| incoming.enqueue(batch));
                            let ack = [if result.is_ok() { 1 } else { 0 }];
                            let _ = write_until(&mut stream, &ack, until);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => {
                            eprintln!("paint.rs launch listener: {e}");
                            break;
                        }
                    }
                }
            })?;
        Ok(Some(Self {
            _lock: lock,
            stop,
            worker: Some(worker),
            bridge,
        }))
    }
}
fn remaining(until: Instant) -> io::Result<Duration> {
    until
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "launch transport timed out; acceptance is unknown",
            )
        })
}
fn retry(e: io::Error, until: Instant) -> io::Result<()> {
    let left = remaining(until)?;
    if e.kind() == io::ErrorKind::Interrupted {
        return Ok(());
    }
    if e.kind() != io::ErrorKind::WouldBlock {
        return Err(e);
    }
    thread::sleep(left.min(Duration::from_millis(5)));
    Ok(())
}
fn read_until(stream: &mut impl Read, mut bytes: &mut [u8], until: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        remaining(until)?;
        match stream.read(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "launch connection ended",
                ));
            }
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) => retry(e, until)?,
        }
    }
    Ok(())
}
fn write_until(stream: &mut impl Write, mut bytes: &[u8], until: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        remaining(until)?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "launch connection ended",
                ));
            }
            Ok(n) => bytes = &bytes[n..],
            Err(e) => retry(e, until)?,
        }
    }
    Ok(())
}
fn receive(stream: &mut Stream, until: Instant) -> io::Result<wire::Batch> {
    let mut len = [0; 4];
    read_until(stream, &mut len, until)?;
    let len = u32::from_le_bytes(len) as usize;
    if !(24..=wire::LIMIT).contains(&len) {
        return Err(io::Error::other("invalid launch frame length"));
    }
    let mut payload = vec![0; len];
    read_until(stream, &mut payload, until)?;
    wire::decode(&payload)
}
fn forward(_dir: &Path, payload: &[u8], until: Instant) -> io::Result<()> {
    #[cfg(unix)]
    let mut stream = interprocess::local_socket::ConnectOptions::new()
        .name(os::endpoint(_dir)?)
        .wait_mode(interprocess::ConnectWaitMode::Timeout(remaining(until)?))
        .nonblocking_stream(true)
        .connect_sync()?;
    #[cfg(windows)]
    let mut stream = os::connect(until)?;
    write_until(&mut stream, &(payload.len() as u32).to_le_bytes(), until)?;
    write_until(&mut stream, payload, until)?;
    let mut ack = [0];
    read_until(&mut stream, &mut ack, until)?;
    if ack != [1] {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "primary rejected launch; files were not queued",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Final review I2: a failed IPC setup still runs the editor, standalone, with its own files
    /// queued in-process; success and "another instance accepted" keep their meaning.
    #[test]
    fn failed_ipc_setup_runs_standalone() {
        let bridge = Bridge::default();
        let paths = vec![std::env::temp_dir().join("standalone.png")];
        let failed = Instance::settle(Err(io::Error::other("bad")), paths.clone(), &bridge);
        assert!(matches!(failed, Some(None)), "runs, without IPC");
        assert_eq!(bridge.0.lock().unwrap().paths, [paths]);
        assert!(Instance::settle(Ok(None), vec![], &Bridge::default()).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            let owner = Instance::start_at(dir.path(), vec![], Bridge::default()).unwrap();
            let settled = Instance::settle(Ok(owner), vec![], &Bridge::default());
            assert!(matches!(settled, Some(Some(_))));
        }
    }
    #[cfg(unix)]
    #[test]
    fn forwarding_does_not_reset_deadline_while_waiting_for_ack() {
        let dir = tempfile::tempdir().unwrap();
        let _listener = ListenerOptions::new()
            .name(os::endpoint(dir.path()).unwrap())
            .create_sync()
            .unwrap();
        let began = Instant::now();
        let error = forward(dir.path(), b"request", began + Duration::from_millis(80)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(began.elapsed() < Duration::from_millis(500));
    }
    #[test]
    fn expired_attempt_never_transmits_a_frame() {
        let mut bytes = Vec::new();
        let error = write_until(&mut bytes, b"private-path", Instant::now()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(bytes.is_empty());
    }
    #[test]
    fn expired_deadline_also_bounds_interrupted_operations() {
        let result = retry(io::ErrorKind::Interrupted.into(), Instant::now());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }
    #[test]
    fn idle_quit_defers_accepted_paths_and_rejects_after_final_decision() {
        let bridge = Bridge::default();
        assert!(!bridge.try_finish_idle_quit());
        bridge.request_quit();
        let path = std::env::temp_dir().join("accepted-before-quit.png");
        bridge.launch(vec![path.clone()]).unwrap();
        assert!(!bridge.try_finish_idle_quit());
        // Model the host handing the acknowledged batch to the editor's delivery flow.
        assert_eq!(bridge.0.lock().unwrap().paths.pop_front(), Some(vec![path]));
        assert!(bridge.try_finish_idle_quit());
        assert_eq!(
            bridge.launch(vec![]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }
    #[test]
    fn concurrent_idle_quit_never_approves_with_an_accepted_path() {
        for _ in 0..128 {
            let bridge = Bridge::default();
            bridge.request_quit();
            let gate = Arc::new(std::sync::Barrier::new(2));
            let sender = bridge.clone();
            let sender_gate = gate.clone();
            let thread = thread::spawn(move || {
                sender_gate.wait();
                sender.launch(vec![std::env::temp_dir().join("boundary.png")])
            });
            gate.wait();
            let finished = bridge.try_finish_idle_quit();
            let accepted = thread.join().unwrap().is_ok();
            assert_ne!(finished, accepted);
            assert_eq!(bridge.pending_paths(), accepted);
        }
    }
    #[test]
    fn native_attention_wakes_without_a_render_frame_and_detaches() {
        use std::sync::atomic::AtomicUsize;
        let bridge = Bridge::default();
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        bridge.0.lock().unwrap().native_attention = Some(Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        }));
        bridge.launch(vec![]).unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        bridge.detach();
        bridge.launch(vec![]).unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn exit_cannot_discard_a_previously_accepted_batch() {
        let bridge = Bridge::default();
        let ctx = egui::Context::default();
        bridge.attach(&ctx);
        bridge
            .launch(vec![std::env::temp_dir().join("queued.png")])
            .unwrap();
        window_close::request(&ctx, Intent::ExitProcess);
        assert!(window_close::begin(&ctx));
        assert!(bridge.launch(vec![]).is_err());
        window_close::finish(&ctx, true);
        assert_eq!(window_close::state(&ctx).outcome, None);
        assert!(bridge.pending_paths());
        assert!(!bridge.0.lock().unwrap().exiting);
        window_close::finish(&ctx, false);
        assert!(bridge.launch(vec![]).is_ok());
    }
    #[test]
    fn retry_id_is_accepted_once_and_exit_rejects_new_batches() {
        let b = Bridge::default();
        for _ in 0..2 {
            b.enqueue(wire::Batch {
                id: [1; 16],
                paths: vec![PathBuf::from("/a.png")],
            })
            .unwrap();
        }
        assert_eq!(b.0.lock().unwrap().paths.len(), 1);
        b.stop_accepting();
        assert!(b.launch(vec![]).is_err());
    }
    #[test]
    fn exit_admission_reopens_only_on_cancel_and_approval_rejects_launches() {
        let bridge = Bridge::default();
        let ctx = egui::Context::default();
        bridge.attach(&ctx);
        window_close::request(&ctx, Intent::ExitProcess);
        assert!(window_close::begin(&ctx));
        assert_eq!(
            bridge.launch(vec![]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        window_close::finish(&ctx, false);
        bridge.launch(vec![]).unwrap();
        window_close::request(&ctx, Intent::ExitProcess);
        assert!(window_close::begin(&ctx));
        window_close::finish(&ctx, true);
        assert_eq!(
            window_close::state(&ctx).outcome,
            Some(Ok(Intent::ExitProcess))
        );
        assert!(bridge.launch(vec![]).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn transport_rejects_during_exit_review_and_preserves_accepted_order() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let bridge = Bridge::default();
        let ctx = egui::Context::default();
        bridge.attach(&ctx);
        let _primary = Instance::start_at(dir.path(), vec![], bridge.clone())
            .unwrap()
            .unwrap();
        let paths = vec![dir.path().join("first.png"), dir.path().join("second.png")];
        assert!(
            Instance::start_at(dir.path(), paths.clone(), Bridge::default())
                .unwrap()
                .is_none()
        );
        window_close::request(&ctx, Intent::ExitProcess);
        let began = Instant::now();
        assert!(
            Instance::start_at(
                dir.path(),
                vec![dir.path().join("rejected.png")],
                Bridge::default()
            )
            .is_err()
        );
        assert!(began.elapsed() < Duration::from_secs(3));
        assert_eq!(
            bridge
                .0
                .lock()
                .unwrap()
                .paths
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>(),
            paths
        );
        window_close::finish(&ctx, false);
        assert!(
            Instance::start_at(dir.path(), vec![], Bridge::default())
                .unwrap()
                .is_none()
        );
    }
    #[cfg(unix)]
    #[test]
    fn actual_local_transport_elects_one_primary_and_forwards_empty_activation() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let b = Bridge::default();
        let primary = Instance::start_at(dir.path(), vec![], b.clone())
            .unwrap()
            .unwrap();
        b.0.lock().unwrap().activate = false;
        assert!(
            Instance::start_at(dir.path(), vec![], Bridge::default())
                .unwrap()
                .is_none()
        );
        assert!(b.needs_editor());
        drop(primary);
        assert!(
            Instance::start_at(dir.path(), vec![], Bridge::default())
                .unwrap()
                .is_some()
        );
    }
}
