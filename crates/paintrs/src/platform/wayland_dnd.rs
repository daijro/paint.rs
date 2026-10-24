//! Native Wayland drag and drop. winit 0.30 emits `DroppedFile` only from its X11 backend, so on a
//! Wayland window this listens on the window's own connection: a guest `wayland-client` backend on
//! winit's `wl_display` (`Backend::from_foreign_display`) with its own event queue on a background
//! thread, one `wl_data_device` per seat, and the window recognised by its `wl_surface` pointer.
//! Drops reach egui's `RawInput::dropped_files`, the path X11 drops take
//! (`PaintApp::raw_input_hook`). A failure logs once and leaves the app without drag and drop.
//!
//! Nothing here may raise a protocol error: it would end winit's connection, so the app.

use std::{
    io::{self, Read as _},
    os::fd::{AsFd as _, AsRawFd as _, RawFd},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread::JoinHandle,
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    backend::Backend,
    event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_data_device::{self, WlDataDevice},
        wl_data_device_manager::{DndAction, WlDataDeviceManager},
        wl_data_offer::{self, WlDataOffer},
        wl_registry::{self, WlRegistry},
        wl_seat::{self, WlSeat},
    },
};
use winit::raw_window_handle::{
    HasDisplayHandle as _, HasWindowHandle as _, RawDisplayHandle, RawWindowHandle,
};

const URI_LIST: &str = "text/uri-list";
/// Image data without a file (a browser's image): saved to a temporary file, as a browser hands
/// Paint.NET one on Windows, so it takes the same Open / Add layer path.
const PNG: &str = "image/png";
/// A source that stops writing for this long is abandoned.
const READ_TIMEOUT_MS: i32 = 10_000;

#[derive(Default)]
struct Shared {
    /// Surface-local position of the accepted drag while it is over the window.
    hover: Option<(f64, f64)>,
    /// The drag left the window: egui's pointer leaves with it.
    left: bool,
    dropped: Vec<egui::DroppedFileHandle>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The running listener. Dropping it stops and joins the thread, which eframe does (with the app,
/// on exit) before winit closes the display the thread borrows.
pub struct Dnd {
    shared: Arc<Mutex<Shared>>,
    stop: Option<io::PipeWriter>,
    thread: Option<JoinHandle<()>>,
}

impl Dnd {
    /// Starts listening when `window` is a Wayland window.
    pub fn start(window: &winit::window::Window, ctx: &egui::Context) -> Option<Dnd> {
        let (Ok(RawDisplayHandle::Wayland(display)), Ok(RawWindowHandle::Wayland(surface))) = (
            window.display_handle().map(|h| h.as_raw()),
            window.window_handle().map(|h| h.as_raw()),
        ) else {
            return None;
        };
        let (display, surface) = (
            display.display.as_ptr() as usize,
            surface.surface.as_ptr() as usize,
        );
        let (stop_reader, stop) = io::pipe()
            .inspect_err(|e| eprintln!("paint.rs: Wayland drag and drop is unavailable: {e}"))
            .ok()?;
        let shared = Arc::<Mutex<Shared>>::default();
        let mut state = State {
            surface,
            shared: shared.clone(),
            ctx: ctx.clone(),
            stop: stop_reader,
            offer: None,
        };
        let thread = std::thread::Builder::new()
            .name("wayland-dnd".into())
            .spawn(move || {
                // SAFETY: winit's display outlives this thread (see `Dnd`).
                let conn = Connection::from_backend(unsafe {
                    Backend::from_foreign_display(display as _)
                });
                if let Err(e) = run(&conn, &mut state) {
                    eprintln!("paint.rs: Wayland drag and drop is unavailable: {e}");
                }
            })
            .inspect_err(|e| eprintln!("paint.rs: Wayland drag and drop is unavailable: {e}"))
            .ok()?;
        Some(Dnd {
            shared,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// Gives this frame the drops, and a drag over the window as a hovered file under the
    /// pointer (the compositor sends the window no pointer events during a drag).
    pub fn feed(&self, raw: &mut egui::RawInput, zoom: f32) {
        let mut shared = lock(&self.shared);
        raw.dropped_files.append(&mut shared.dropped);
        if let Some((x, y)) = shared.hover {
            raw.hovered_files.push(egui::HoveredFile::default());
            raw.events.push(egui::Event::PointerMoved(egui::pos2(
                x as f32 / zoom,
                y as f32 / zoom,
            )));
        } else if std::mem::take(&mut shared.left) {
            raw.events.push(egui::Event::PointerGone);
        }
    }
}

impl Drop for Dnd {
    fn drop(&mut self) {
        drop(self.stop.take()); // the thread's poll sees the hang-up
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(conn: &Connection, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    let (globals, mut queue) = registry_queue_init::<State>(conn)?;
    let qh = queue.handle();
    let manager: WlDataDeviceManager = globals.bind(&qh, 1..=3, ())?;
    // ponytail: the seats present at startup; a seat added later gets no drag and drop.
    let devices: Vec<WlDataDevice> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == WlSeat::interface().name)
            .map(|g| {
                let seat: WlSeat = globals.registry().bind(g.name, 1, &qh, ());
                manager.get_data_device(&seat, &qh, ())
            })
            .collect()
    });
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            queue.dispatch_pending(state)?;
            conn.flush()?;
            let Some(guard) = queue.prepare_read() else {
                continue;
            };
            if !readable(
                guard.connection_fd().as_raw_fd(),
                state.stop.as_raw_fd(),
                -1,
            )? {
                return Ok(()); // dropping the guard cancels the read
            }
            guard.read()?;
        }
    })();
    for device in devices.iter().filter(|d| d.version() >= 2) {
        device.release();
    }
    let _ = conn.flush();
    result
}

/// Waits for `fd` to be readable (or hung up): false when `stop` hung up first or the timeout
/// (ms, -1 for none) passed.
fn readable(fd: RawFd, stop: RawFd, timeout: i32) -> io::Result<bool> {
    let mut fds = [fd, stop].map(|fd| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    });
    loop {
        // SAFETY: `fds` is two valid `pollfd`s.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) } >= 0 {
            return Ok(fds[1].revents == 0 && fds[0].revents != 0);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// The `file://` paths of a `text/uri-list` (RFC 2483: CRLF lines, `#` comments), percent-decoded.
/// Other schemes and hosts are skipped.
fn file_paths(list: &[u8]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;
    String::from_utf8_lossy(list)
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("file://")?;
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            path.starts_with('/').then(|| {
                let bytes: Vec<u8> = percent_encoding::percent_decode_str(path).collect();
                PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))
            })
        })
        .collect()
}

#[derive(Debug)]
struct DroppedPath(PathBuf);
impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// What an offer advertised.
#[derive(Default)]
struct Offered {
    mimes: Vec<String>,
    /// The compositor chose an action: `finish` is allowed.
    action: bool,
}

fn offered(offer: &WlDataOffer) -> MutexGuard<'_, Offered> {
    offer
        .data::<Mutex<Offered>>()
        .expect("offers are created with Offered")
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

struct State {
    /// The window's `wl_surface`.
    surface: usize,
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    stop: io::PipeReader,
    /// The drag over the window and the MIME type it was accepted with.
    offer: Option<(WlDataOffer, &'static str)>,
}

impl State {
    fn hover(&self, at: Option<(f64, f64)>, left: bool) {
        let mut shared = lock(&self.shared);
        (shared.hover, shared.left) = (at, left);
        self.ctx.request_repaint();
    }

    fn drop_offer(&mut self, conn: &Connection) {
        let Some((offer, mime)) = self.offer.take() else {
            return;
        };
        self.hover(None, false);
        let png = offered(&offer).mimes.iter().any(|m| m == PNG);
        let result = self.read_drop(conn, &offer, mime, png);
        if result.is_ok() && offer.version() >= 3 && offered(&offer).action {
            offer.finish();
        }
        offer.destroy();
        let _ = conn.flush();
        match result {
            Ok(files) if files.is_empty() => eprintln!("paint.rs: the drop held no local file"),
            Ok(files) => {
                lock(&self.shared).dropped.extend(files);
                self.ctx.request_repaint();
            }
            Err(e) => eprintln!("paint.rs: reading the drop failed: {e}"),
        }
    }

    fn read_drop(
        &self,
        conn: &Connection,
        offer: &WlDataOffer,
        mime: &str,
        png: bool,
    ) -> io::Result<Vec<egui::DroppedFileHandle>> {
        let mut paths = if mime == URI_LIST {
            file_paths(&self.receive(conn, offer, URI_LIST)?)
        } else {
            vec![]
        };
        if paths.is_empty() && png {
            let bytes = self.receive(conn, offer, PNG)?;
            let path = tempfile::Builder::new()
                .prefix("paint.rs-drop-")
                .tempdir()?
                .keep()
                .join("image.png");
            std::fs::write(&path, bytes)?;
            paths.push(path);
        }
        Ok(paths
            .into_iter()
            .map(|p| Arc::new(DroppedPath(p)) as egui::DroppedFileHandle)
            .collect())
    }

    /// One MIME type of `offer` through a pipe, without waiting past paint.rs's exit.
    fn receive(&self, conn: &Connection, offer: &WlDataOffer, mime: &str) -> io::Result<Vec<u8>> {
        let (mut reader, writer) = io::pipe()?;
        offer.receive(mime.into(), writer.as_fd());
        drop(writer); // libwayland sent a duplicate
        conn.flush().map_err(io::Error::other)?;
        let mut bytes = vec![];
        let mut chunk = [0; 1 << 16];
        while readable(reader.as_raw_fd(), self.stop.as_raw_fd(), READ_TIMEOUT_MS)? {
            match reader.read(&mut chunk)? {
                0 => return Ok(bytes),
                n if (bytes.len() + n) as u64 > pdn_core::io::codecs::MAX_DECODE_BYTES => {
                    return Err(io::Error::other("the dropped data is too large"));
                }
                n => bytes.extend_from_slice(&chunk[..n]),
            }
        }
        Err(io::ErrorKind::TimedOut.into())
    }
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::Enter {
                serial,
                surface,
                x,
                y,
                id: Some(offer),
            } => {
                if let Some((old, _)) = state.offer.take() {
                    old.destroy();
                }
                let mime = (surface.id().as_ptr() as usize == state.surface)
                    .then(|| {
                        let offered = offered(&offer);
                        [URI_LIST, PNG]
                            .into_iter()
                            .find(|m| offered.mimes.iter().any(|o| o == m))
                    })
                    .flatten();
                offer.accept(serial, mime.map(String::from));
                match mime {
                    Some(mime) => {
                        if offer.version() >= 3 {
                            offer.set_actions(DndAction::Copy, DndAction::Copy);
                        }
                        state.offer = Some((offer, mime));
                        state.hover(Some((x, y)), false);
                    }
                    None => offer.destroy(),
                }
            }
            wl_data_device::Event::Motion { x, y, .. } if state.offer.is_some() => {
                state.hover(Some((x, y)), false);
            }
            wl_data_device::Event::Leave => {
                if let Some((offer, _)) = state.offer.take() {
                    offer.destroy();
                    state.hover(None, true);
                }
            }
            wl_data_device::Event::Drop => state.drop_offer(conn),
            // The clipboard goes through arboard.
            wl_data_device::Event::Selection { id: Some(offer) } => offer.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, Mutex::<Offered>::default()),
    ]);
}

impl Dispatch<WlDataOffer, Mutex<Offered>> for State {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: wl_data_offer::Event,
        data: &Mutex<Offered>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut data = data.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            wl_data_offer::Event::Offer { mime_type } => data.mimes.push(mime_type),
            wl_data_offer::Event::Action { dnd_action } => {
                data.action = matches!(dnd_action, WEnum::Value(a) if !a.is_empty());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDeviceManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: <WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn uri_lists_yield_decoded_local_paths_only() {
        let list =
            b"# from Dolphin\r\nfile:///home/a/My%20Image.png\r\nfile://localhost/tmp/b.pdn\r\n\
            https://example.com/c.png\r\nfile://otherhost/d.png\r\nfile:///tmp/%C3%A9.jpg";
        assert_eq!(
            super::file_paths(list),
            ["/home/a/My Image.png", "/tmp/b.pdn", "/tmp/é.jpg"].map(std::path::PathBuf::from)
        );
    }
}
