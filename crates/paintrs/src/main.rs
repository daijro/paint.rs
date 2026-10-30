#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    use paintrs::platform::startup;
    let files = paintrs::platform::native::argv_files(std::env::args_os().skip(1));
    let bridge = paintrs::platform::launch::Bridge::default();
    use paintrs::platform::launch::Instance;
    let started = Instance::start(files.clone(), bridge.clone());
    let Some(owner) = Instance::settle(started, files, &bridge) else {
        return std::process::ExitCode::SUCCESS;
    };
    // Release builds have no console: startup.log and an error dialog say why a launch failed.
    let log = &startup::Log::start(startup::log_path());
    startup::log_panics(log.clone());
    let var = std::env::var("PAINTRS_RENDERER").ok();
    if let Some(var) = &var {
        log.line(&format!("PAINTRS_RENDERER={var}"));
    }
    let options = native_options();
    let renderers = startup::Renderer::chain(var.as_deref(), &options);
    let run = |renderer: startup::Renderer, started: &std::cell::Cell<bool>| {
        let bridge = bridge.clone();
        #[cfg(target_os = "macos")]
        {
            let _ = (renderer, started, log, &options);
            paintrs::platform::launch::macos::run(bridge, native_options)
        }
        #[cfg(not(target_os = "macos"))]
        {
            eframe::run_native(
                "paint.rs",
                run_options(renderer, log),
                Box::new(move |cc| {
                    started.set(true);
                    startup::log_adapter(log, cc);
                    let mut app = paintrs::PaintApp::new(cc);
                    bridge.attach(&cc.egui_ctx);
                    #[cfg(target_os = "linux")]
                    if let Some(window) = cc.winit_window() {
                        paintrs::platform::tablet::start_x11(
                            &app.shell.canvas_view.pen.stylus,
                            window,
                        );
                        bridge.attach_wayland_attention(window);
                        app.wayland_dnd =
                            paintrs::platform::wayland_dnd::Dnd::start(window, &cc.egui_ctx);
                    }
                    app.native_launch = Some(bridge);
                    Ok(Box::new(app))
                }),
            )
        }
    };
    let show = move |text: &str| {
        // A launch while the dialog is open starts afresh instead of handing its files over.
        drop(owner);
        startup::show_dialog(text);
    };
    let release = !cfg!(debug_assertions);
    if startup::launch(log, &renderers, release, run, show) {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

/// The options one renderer runs with, built afresh: `NativeOptions::clone` drops
/// `event_loop_builder`, and with it the Xwayland choice a pen needs.
#[cfg(not(any(target_arch = "wasm32", target_os = "macos")))]
fn run_options(
    renderer: paintrs::platform::startup::Renderer,
    log: &paintrs::platform::startup::Log,
) -> eframe::NativeOptions {
    let mut options = native_options();
    renderer.apply(&mut options, log);
    options
}

/// Whether the window opens through Xwayland (platform/tablet.rs). Decided once: every renderer
/// builds its options again.
#[cfg(target_os = "linux")]
static XWAYLAND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
fn native_options() -> eframe::NativeOptions {
    // js/platform/desktop/main.js: default1080×720, minimum640×480; OS owns DPI.
    let viewport = egui::ViewportBuilder::default()
        .with_title("paint.rs")
        .with_inner_size([1080.0, 720.0])
        .with_min_inner_size([640.0, 480.0])
        .with_icon(egui::IconData {
            rgba: paintrs::assets::app_icon(256).into_raw(),
            width: 256,
            height: 256,
        });
    #[cfg(target_os = "macos")]
    let viewport = viewport
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false);
    #[cfg(not(target_os = "macos"))]
    let viewport = viewport.with_decorations(false);
    // Wayland ignores `with_icon`: the compositor takes the icon from the desktop file named after
    // the app id (packaging/linux, scripts/linux-install-desktop.sh). eframe would also store its
    // state under the app id, so it stays where the app name put it.
    #[cfg(target_os = "linux")]
    let viewport = viewport.with_app_id("paint-rs");
    let mut options = eframe::NativeOptions {
        viewport,
        clipboard_shortcuts: false,
        #[cfg(target_os = "linux")]
        persistence_path: eframe::storage_dir("paint.rs").map(|dir| dir.join("app.ron")),
        ..Default::default()
    };
    // winit picks Wayland whenever `WAYLAND_DISPLAY` is set; a pen needs Xwayland there.
    #[cfg(target_os = "linux")]
    if *XWAYLAND.get_or_init(paintrs::platform::tablet::use_xwayland) {
        use winit::platform::x11::EventLoopBuilderExtX11 as _;
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
    }
    // eframe's default surface (`SurfaceConfig::LOW_LATENCY`) already queues one frame ahead of
    // the display, vsync'd. `PAINTRS_FRAME_LATENCY` (1-3) overrides it: 2 or 3 trade a refresh of
    // input lag each for headroom when frames overrun the refresh.
    if let Ok(var) = std::env::var("PAINTRS_FRAME_LATENCY") {
        match var.trim().parse().ok().filter(|v| (1..=3).contains(v)) {
            Some(latency) => {
                options.wgpu_options.surface.desired_maximum_frame_latency = Some(latency)
            }
            None => eprintln!("paint.rs: PAINTRS_FRAME_LATENCY={var:?} is not 1, 2 or 3; ignored"),
        }
    }
    options
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("paintrs_canvas"))
            .expect("#paintrs_canvas missing")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#paintrs_canvas is not a canvas");
        let runner = eframe::WebRunner::new();
        let pen = std::rc::Rc::new(std::cell::RefCell::new(
            paintrs::platform::web::PenEvents::default(),
        ));
        let observed = pen.clone();
        let options = eframe::WebOptions {
            should_stop_propagation: Box::new(move |event| {
                observed.borrow_mut().emitted(event);
                true
            }),
            ..Default::default()
        };
        runner
            .start(
                canvas.clone(),
                options,
                Box::new(move |cc| {
                    let mut app = paintrs::PaintApp::new(cc);
                    app.web_pen_events = pen;
                    Ok(Box::new(app))
                }),
            )
            .await
            .expect("failed to start eframe");
        paintrs::platform::web::install_services(&runner).expect("web services");
        paintrs::platform::web::install_pen_input(&runner, &canvas)
            .expect("failed to install pen input");
        // Live editing, offered to a Playground page (`paintrs::live`).
        if let Err(e) = paintrs::live::web::attach(runner.clone()) {
            eprintln!("live editing: {e:?}");
        }
    });
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use paintrs::platform::startup::{Log, Renderer};

    /// Every renderer opens the window through Xwayland once that is decided (a pen on Wayland).
    #[test]
    fn each_renderer_keeps_the_xwayland_choice() {
        assert!(*super::XWAYLAND.get_or_init(|| true));
        for renderer in [Renderer::Default, Renderer::Software, Renderer::Gl] {
            let options = super::run_options(renderer, &Log::start(None));
            assert!(options.event_loop_builder.is_some(), "{renderer:?}");
        }
    }
}
