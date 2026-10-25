//! Startup diagnostics (desktop). A release build has no console (`windows_subsystem`), so a
//! failure to start would vanish without a trace. Each launch writes `startup.log`
//! ([`log_path`]): every renderer tried, the adapters it saw and the one it used, and any panic.
//! A launch that fails ends in an error dialog.
//!
//! Renderers ([`Renderer::chain`]): eframe's own adapter choice first. When it fails before the
//! app exists, a software adapter (WARP, part of Windows 10 and later, or llvmpipe on Linux),
//! then OpenGL. `PAINTRS_RENDERER=default`, `software` or `gl` picks one. `WGPU_BACKEND` limits
//! the backends as before, and the fallbacks stay within it.
use eframe::{egui_wgpu, wgpu};
use std::{
    any::Any,
    cell::Cell,
    fmt, fs,
    io::Write as _,
    panic,
    path::{Path, PathBuf},
    sync::Arc,
};

/// `%LOCALAPPDATA%\paint.rs\startup.log` on Windows, `startup.log` in the app's data folder
/// elsewhere ([`crate::icon_pack::app_data_dir`]).
pub fn log_path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("paint.rs"))
    } else {
        crate::icon_pack::app_data_dir()
    };
    Some(dir?.join("startup.log"))
}

/// This launch's log file, when it could be created.
#[derive(Clone, Debug)]
pub struct Log(Option<PathBuf>);

impl Log {
    /// Starts this launch's log at `path`. The last launch's becomes `startup.previous.log`, so
    /// relaunching after the error dialog keeps what it pointed to, and two files at most exist.
    pub fn start(path: Option<PathBuf>) -> Log {
        let path = path.filter(|path| {
            let dir = path
                .parent()
                .is_none_or(|dir| fs::create_dir_all(dir).is_ok());
            let _ = fs::rename(path, path.with_extension("previous.log"));
            dir && fs::write(path, "").is_ok()
        });
        let log = Log(path);
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        log.line(&format!(
            "paint.rs {} ({} {}) starting at {now}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        log
    }

    pub fn path(&self) -> Option<&Path> {
        self.0.as_deref()
    }

    /// Appends a line. The file is opened for each one, so a crash inside a graphics driver
    /// still leaves every line before it.
    pub fn line(&self, text: &str) {
        if let Some(path) = &self.0 {
            let _ = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| writeln!(file, "{text}"));
        }
    }
}

/// Panics on any thread also go to the log, after the usual message on stderr.
pub fn log_panics(log: Log) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        previous(info);
        let thread = std::thread::current();
        log.line(&format!(
            "thread '{}' {info}",
            thread.name().unwrap_or("<unnamed>")
        ));
    }));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// eframe's default: wgpu's own choice among the `WGPU_BACKEND` backends (all by default).
    Default,
    /// A software adapter (wgpu's fallback adapter): WARP, through DirectX 12 alone, on Windows.
    Software,
    /// The OpenGL backend only, hardware adapters first.
    Gl,
}

impl fmt::Display for Renderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::Software => "software",
            Self::Gl => "OpenGL",
        })
    }
}

impl Renderer {
    /// The renderers to try in turn for `PAINTRS_RENDERER`'s value `var`: just the one it names,
    /// or the default and then the fallbacks. OpenGL only when `options` (eframe's, which read
    /// `WGPU_BACKEND`) allow it. The macOS host builds its own options and has only Metal.
    pub fn chain(var: Option<&str>, options: &eframe::NativeOptions) -> Vec<Renderer> {
        let gl = match &options.wgpu_options.wgpu_setup {
            egui_wgpu::WgpuSetup::CreateNew(setup) => setup.instance_descriptor.backends,
            egui_wgpu::WgpuSetup::Existing(_) => wgpu::Backends::empty(),
        }
        .contains(wgpu::Backends::GL);
        match var.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            _ if cfg!(target_os = "macos") => vec![Self::Default],
            Some("default") => vec![Self::Default],
            Some("software") => vec![Self::Software],
            Some("gl" | "opengl") => vec![Self::Gl],
            _ if gl => vec![Self::Default, Self::Software, Self::Gl],
            _ => vec![Self::Default, Self::Software],
        }
    }

    /// Points eframe's wgpu setup at this renderer's backends and adapters, logging the backends
    /// and every adapter the selection sees. The default renderer leaves the setup alone.
    pub fn apply(self, options: &mut eframe::NativeOptions, log: &Log) {
        let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup else {
            return;
        };
        let backends = &mut setup.instance_descriptor.backends;
        match self {
            // WARP is DirectX 12's: on Windows no other backend starts, so a Vulkan layer or an
            // OpenGL driver that crashes as it loads stays out of the way.
            Self::Software if cfg!(windows) => *backends &= wgpu::Backends::DX12,
            Self::Gl => *backends = wgpu::Backends::GL,
            Self::Default | Self::Software => {}
        }
        log.line(&format!("  backends: {backends:?}"));
        let allow: fn(&wgpu::AdapterInfo) -> bool = match self {
            Self::Default => return,
            Self::Software => |info| info.device_type == wgpu::DeviceType::Cpu,
            Self::Gl => |_| true,
        };
        let log = log.clone();
        setup.native_adapter_selector = Some(Arc::new(
            move |adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>| {
                let mut found = vec![];
                for adapter in adapters {
                    let info = adapter.get_info();
                    let presents = surface.is_none_or(|s| adapter.is_surface_supported(s));
                    let note = if presents {
                        ""
                    } else {
                        " (cannot draw to the window)"
                    };
                    let summary = egui_wgpu::adapter_info_summary(&info);
                    log.line(&format!("  adapter: {summary}{note}"));
                    found.push((info.device_type, presents && allow(&info)));
                }
                pick(&found).map(|i| adapters[i].clone()).ok_or_else(|| {
                    let n = adapters.len();
                    format!("none of the {n} adapters found suits the {self} renderer")
                })
            },
        ));
    }
}

/// The first usable adapter in wgpu's high-performance order: discrete, integrated, other,
/// virtual, then software.
fn pick(adapters: &[(wgpu::DeviceType, bool)]) -> Option<usize> {
    use wgpu::DeviceType::*;
    let rank = |kind: wgpu::DeviceType| match kind {
        DiscreteGpu => 0,
        IntegratedGpu => 1,
        Other => 2,
        VirtualGpu => 3,
        Cpu => 4,
    };
    (0..adapters.len())
        .filter(|&i| adapters[i].1)
        .min_by_key(|&i| rank(adapters[i].0))
}

/// Logs the adapter eframe used and the others it found (the app creator calls it).
pub fn log_adapter(log: &Log, cc: &eframe::CreationContext<'_>) {
    let Some(state) = &cc.wgpu_render_state else {
        return;
    };
    let summary = |adapter: &wgpu::Adapter| egui_wgpu::adapter_info_summary(&adapter.get_info());
    log.line(&format!("Using {}", summary(&state.adapter)));
    for adapter in &state.available_adapters {
        log.line(&format!("  adapter: {}", summary(adapter)));
    }
}

/// Runs the app with each renderer in turn. `run` sets `started` once the app exists: until
/// then an error is logged and the next renderer tried, and so is a panic on Windows. A normal
/// quit is `true`. The last renderer failing, or the app failing after it started, is `false`,
/// with the reasons shown through `show` ([`show_dialog`]) in a `release` build. A debug build
/// shows no dialog, and a panic goes on unwinding once it is logged (exit code 101, as before).
pub fn launch(
    log: &Log,
    renderers: &[Renderer],
    release: bool,
    mut run: impl FnMut(Renderer, &Cell<bool>) -> eframe::Result,
    show: impl FnOnce(&str),
) -> bool {
    let started = Cell::new(false);
    let mut failures = vec![];
    for &renderer in renderers {
        log.line(&format!("Trying the {renderer} renderer"));
        let outcome = panic::catch_unwind(panic::AssertUnwindSafe(|| run(renderer, &started)));
        let (error, panicked) = match &outcome {
            Ok(Ok(())) => return true,
            Ok(Err(error)) => (error.to_string(), false),
            Err(payload) => (panic_message(&**payload), true),
        };
        eprintln!("paint.rs: the {renderer} renderer failed: {error}");
        log.line(&format!("The {renderer} renderer failed: {error}"));
        failures.push(format!("The {renderer} renderer: {error}"));
        if let (Err(payload), false) = (outcome, release) {
            panic::resume_unwind(payload);
        }
        // A panic leaves winit's X11 and Wayland event loops marked running, so another
        // `run_native` would never open its window. Windows resets its loop before unwinding.
        if started.get() || (panicked && !cfg!(windows)) {
            break;
        }
    }
    if release {
        show(&failure_text(started.get(), &failures, log.path()));
    }
    false
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    let text = payload.downcast_ref::<&str>().copied();
    let text = text.or(payload.downcast_ref::<String>().map(String::as_str));
    format!("panicked: {}", text.unwrap_or("(no message)"))
}

fn failure_text(started: bool, failures: &[String], log: Option<&Path>) -> String {
    let mut text = String::from(if started {
        "paint.rs stopped because of an error."
    } else {
        "paint.rs could not start."
    });
    for failure in failures {
        text += "\n\n";
        text += failure;
    }
    if let Some(path) = log {
        text += &format!("\n\nThe details are in {}", path.display());
    }
    text
}

/// The native error dialog.
pub fn show_dialog(text: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("paint.rs")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log(dir: &tempfile::TempDir) -> (Log, PathBuf) {
        let path = dir.path().join("paint.rs").join("startup.log");
        (Log::start(Some(path.clone())), path)
    }

    const ALL: [Renderer; 3] = [Renderer::Default, Renderer::Software, Renderer::Gl];

    /// A forced startup failure (every renderer fails) logs each attempt and shows the dialog
    /// text with every reason and the log's path.
    #[test]
    fn every_renderer_failing_is_logged_and_shown() {
        let dir = tempfile::tempdir().unwrap();
        let (log, path) = temp_log(&dir);
        let (mut tried, mut shown) = (vec![], None);
        let ok = launch(
            &log,
            &ALL,
            true,
            |renderer, _| {
                tried.push(renderer);
                let error = format!("forced {renderer} failure");
                Err(eframe::Error::AppCreation(error.into()))
            },
            |text| shown = Some(text.to_owned()),
        );
        assert!(!ok);
        assert_eq!(tried, ALL);
        let shown = shown.expect("the dialog was shown");
        assert!(
            shown.starts_with("paint.rs could not start.\n\n"),
            "{shown}"
        );
        for renderer in ALL {
            let reason = format!("The {renderer} renderer: app creation error: forced {renderer}");
            assert!(shown.contains(&reason), "{reason:?} missing from {shown}");
        }
        let tail = format!("\n\nThe details are in {}", path.display());
        assert!(shown.ends_with(&tail), "{shown}");
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("paint.rs "), "{written}");
        for line in [
            "Trying the default renderer",
            "The default renderer failed: ",
            "Trying the software renderer",
            "The software renderer failed: ",
            "Trying the OpenGL renderer",
            "The OpenGL renderer failed: ",
        ] {
            assert!(written.contains(line), "{line:?} missing from {written}");
        }
    }

    #[test]
    fn a_fallback_that_starts_ends_the_chain_without_a_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let (log, _) = temp_log(&dir);
        let mut tried = vec![];
        let ok = launch(
            &log,
            &ALL,
            true,
            |renderer, started| {
                tried.push(renderer);
                if renderer == Renderer::Default {
                    return Err(eframe::Error::AppCreation("no adapter".into()));
                }
                started.set(true);
                Ok(())
            },
            |text| panic!("no dialog for a normal quit: {text}"),
        );
        assert!(ok);
        assert_eq!(tried, [Renderer::Default, Renderer::Software]);
    }

    /// Once the app exists it is not started again: its failure goes straight to the dialog.
    #[test]
    fn a_failure_after_the_app_started_is_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let (log, _) = temp_log(&dir);
        let (mut tries, mut shown) = (0, String::new());
        let ok = launch(
            &log,
            &[Renderer::Default, Renderer::Software],
            true,
            |_, started| {
                tries += 1;
                started.set(true);
                panic!("broke mid-session");
            },
            |text| shown = text.to_owned(),
        );
        assert!(!ok);
        assert_eq!(tries, 1);
        assert!(shown.starts_with("paint.rs stopped because of an error.\n\n"));
        assert!(shown.contains("panicked: broke mid-session"), "{shown}");
    }

    /// Off Windows a panic before the app exists ends the chain: winit's X11 and Wayland loops
    /// would hang the next attempt.
    #[test]
    fn a_panic_before_the_app_exists_is_retried_only_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let (log, _) = temp_log(&dir);
        let (mut tried, mut shown) = (vec![], None);
        let ok = launch(
            &log,
            &ALL,
            true,
            |renderer, started| {
                tried.push(renderer);
                if renderer == Renderer::Default {
                    panic!("forced panic");
                }
                started.set(true);
                Ok(())
            },
            |text| shown = Some(text.to_owned()),
        );
        if cfg!(windows) {
            assert!(ok && shown.is_none());
            assert_eq!(tried, [Renderer::Default, Renderer::Software]);
        } else {
            assert!(!ok);
            assert_eq!(tried, [Renderer::Default]);
            let shown = shown.expect("the dialog was shown");
            assert!(
                shown.starts_with("paint.rs could not start.\n\n"),
                "{shown}"
            );
            assert!(shown.contains("The default renderer: panicked: forced panic"));
        }
    }

    /// Debug builds keep the old panic behaviour: no dialog, and the panic goes on unwinding.
    #[test]
    fn a_debug_build_shows_no_dialog_and_rethrows_panics() {
        let dir = tempfile::tempdir().unwrap();
        let (log, path) = temp_log(&dir);
        let no_dialog = |text: &str| panic!("no dialog in a debug build: {text}");
        let mut tries = 0;
        let rethrown = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            launch(
                &log,
                &ALL,
                false,
                |_, _| {
                    tries += 1;
                    panic!("debug panic")
                },
                no_dialog,
            )
        }));
        assert_eq!(
            panic_message(&*rethrown.unwrap_err()),
            "panicked: debug panic"
        );
        assert_eq!(tries, 1);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("The default renderer failed: panicked: debug panic"));
        let error = |_, _: &Cell<bool>| Err(eframe::Error::AppCreation("no adapter".into()));
        assert!(!launch(&log, &ALL, false, error, no_dialog));
    }

    #[test]
    fn starting_a_log_keeps_the_last_one_as_the_previous_log() {
        let dir = tempfile::tempdir().unwrap();
        let (first, path) = temp_log(&dir);
        first.line("first launch");
        let previous = path.with_extension("previous.log");
        assert_eq!(previous.file_name().unwrap(), "startup.previous.log");
        Log::start(Some(path.clone())).line("second launch");
        Log::start(Some(path.clone())).line("third launch");
        let read = |path: &Path| fs::read_to_string(path).unwrap();
        assert!(read(&previous).contains("second launch"));
        assert!(
            !read(&previous).contains("first launch"),
            "two files at most"
        );
        assert!(read(&path).contains("third launch"));
        assert!(!read(&path).contains("second launch"));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
    }

    #[test]
    fn a_log_that_cannot_be_written_is_left_out_of_the_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        fs::write(&blocker, "").unwrap();
        let log = Log::start(Some(blocker.join("startup.log")));
        assert_eq!(log.path(), None);
        assert_eq!(
            failure_text(false, &["The default renderer: x".into()], log.path()),
            "paint.rs could not start.\n\nThe default renderer: x"
        );
    }

    #[test]
    fn chain_reads_paintrs_renderer_and_keeps_to_wgpu_backend() {
        let options = |backends| {
            let mut options = eframe::NativeOptions::default();
            if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
                setup.instance_descriptor.backends = backends;
            }
            options
        };
        let all = options(wgpu::Backends::PRIMARY | wgpu::Backends::GL);
        use Renderer::*;
        assert_eq!(Renderer::chain(None, &all), [Default, Software, Gl]);
        assert_eq!(Renderer::chain(Some("?"), &all), [Default, Software, Gl]);
        assert_eq!(Renderer::chain(Some(" Software "), &all), [Software]);
        assert_eq!(Renderer::chain(Some("gl"), &all), [Gl]);
        assert_eq!(Renderer::chain(Some("opengl"), &all), [Gl]);
        assert_eq!(Renderer::chain(Some("default"), &all), [Default]);
        let vulkan = options(wgpu::Backends::VULKAN);
        assert_eq!(Renderer::chain(None, &vulkan), [Default, Software]);
        assert_eq!(
            Renderer::chain(Some("gl"), &vulkan),
            [Gl],
            "named, so tried"
        );
    }

    #[test]
    fn apply_sets_a_selector_only_for_the_fallbacks() {
        let selector = |renderer: Renderer| {
            let mut options = eframe::NativeOptions::default();
            renderer.apply(&mut options, &Log(None));
            let egui_wgpu::WgpuSetup::CreateNew(setup) = options.wgpu_options.wgpu_setup else {
                unreachable!()
            };
            let gl = setup.instance_descriptor.backends == wgpu::Backends::GL;
            (setup.native_adapter_selector.is_some(), gl)
        };
        assert!(!selector(Renderer::Default).0);
        assert!(selector(Renderer::Software).0);
        assert_eq!(selector(Renderer::Gl), (true, true));
    }

    #[test]
    fn pick_prefers_hardware_and_skips_unusable_adapters() {
        use wgpu::DeviceType::*;
        assert_eq!(pick(&[(Cpu, true), (IntegratedGpu, true)]), Some(1));
        assert_eq!(
            pick(&[(DiscreteGpu, false), (Other, true), (Cpu, true)]),
            Some(1)
        );
        assert_eq!(
            pick(&[(Cpu, true), (Cpu, true)]),
            Some(0),
            "first of equals"
        );
        assert_eq!(pick(&[(DiscreteGpu, false)]), None);
        assert_eq!(pick(&[]), None);
    }
}
