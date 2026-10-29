//! paint.rs app: egui/eframe port of paint.js's `js/src/interface` and `js/platform`.
//! Reference: paintdotjs @ b4e6fc5.
pub mod assets;
pub mod dialogs;
pub mod forms;
pub mod icon_pack;
pub mod kit;
pub mod language;
// Live editing in a Playground session (web only).
#[cfg(any(target_arch = "wasm32", test))]
pub mod live;
pub use language::apply_language;
pub mod platform;
pub mod prefs;
pub mod shell;
pub mod theme;

/// Tells `ctx` its native scale before any pass runs, as eframe does before creating the app
/// (the DPI-scaled tool defaults read it then). Test harnesses call it from their context setup:
/// kittest otherwise reports ppp 1 until the first pass.
pub fn seed_native_scale(ctx: &egui::Context, ppp: f32) {
    ctx.input_mut(|i| {
        i.raw
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        i.pixels_per_point = ppp;
    });
}

/// Time source. Real time in the app; a virtual clock in tests (Task 3 clock rule).
pub enum Clock {
    Real(web_time::Instant),
    Virtual(f64),
}

impl Clock {
    pub fn now_ms(&self) -> f64 {
        match self {
            Clock::Real(start) => start.elapsed().as_secs_f64() * 1000.0,
            Clock::Virtual(ms) => *ms,
        }
    }
}

pub struct PaintApp {
    #[cfg(not(target_arch = "wasm32"))]
    pub native_launch: Option<platform::launch::Bridge>,
    /// Drag and drop on a Wayland window (winit has it only on X11).
    #[cfg(target_os = "linux")]
    pub wayland_dnd: Option<platform::wayland_dnd::Dnd>,
    /// The AppKit tablet monitor (platform/tablet.rs), installed while the app lives.
    #[cfg(target_os = "macos")]
    pub tablet_monitor: Option<photocraft_tablet::macos::Monitor>,
    #[cfg(target_arch = "wasm32")]
    pub web_pen_events: std::rc::Rc<std::cell::RefCell<platform::web::PenEvents>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_pen_listener: Option<platform::web::PenListener>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_close_requested: bool,
    pub clock: Clock,
    pub prefs: prefs::Prefs,
    pub fonts: platform::fonts::FontManager,
    pub asset_library: platform::asset_library::AssetLibrary,
    pub settings: pdn_core::settings::AppSettings,
    /// `ActionRegistry`'s shortcut overrides (`paintdotjs.shortcuts.v1`).
    pub shortcuts: pdn_core::action::ShortcutRegistry,
    pub assets: assets::Assets,
    /// The engine (`AppWorkspace` documents, events); the initial document is created by the
    /// shell's first frame, once the view size is known.
    pub app: pdn_core::app::AppWorkspace,
    pub shell: shell::Shell,
    incoming_files: std::collections::VecDeque<platform::PickedFile>,
    applied_ui_font: String,
}

impl PaintApp {
    /// The app `main` runs: also on the system clipboard (Task 28), which harnesses leave alone.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::new_with_policy(cc, default_policy());
        app.shell.clipboard = platform::Clipboard::system();
        platform::Files::system().install();
        // Object Selection may use MobileSAM when its toolbar picks it; on the web it runs in a
        // worker.
        app.app.object_model = true;
        #[cfg(target_arch = "wasm32")]
        pdn_core::extra::segment::remote::set(Some(std::rc::Rc::new(
            platform::sam_worker::WorkerClient::new(cc.egui_ctx.clone()),
        )));
        app.asset_library = platform::asset_library::AssetLibrary::new(
            platform::asset_library::AssetStore::system(),
        );
        app.fonts = platform::fonts::FontManager::new(
            platform::fonts::FontStore::system(
                app.prefs
                    .get(platform::fonts::METADATA_KEY)
                    .map(str::to_owned),
            ),
            default_policy(),
        );
        #[cfg(not(target_arch = "wasm32"))]
        app.shell
            .forms
            .colors
            .enable_file_palettes(platform::palettes::PaletteStore::system());
        app
    }

    /// `policy` is `System` for normal native runs and `Bundled` for tests and the web. The
    /// interface faces are the bundled Selawik and Carlito under both (ruling R47).
    pub fn new_with_policy(
        cc: &eframe::CreationContext<'_>,
        policy: pdn_core::fonts::FontPolicy,
    ) -> Self {
        let mut app = Self::setup(
            &cc.egui_ctx,
            Clock::Real(web_time::Instant::now()),
            prefs::Prefs::from_storage(cc.storage),
            policy,
        );
        app.shell.desktop = !cfg!(target_arch = "wasm32");
        // WelcomeDialog.open: the web build only (`isApp` skips it).
        if cfg!(target_arch = "wasm32") {
            app.open_welcome();
        }
        app
    }

    /// Test constructor: virtual clock, storage seeded from a localStorage map, bundled fonts,
    /// thumbnails scaled synchronously.
    pub fn new_for_test(
        ctx: &egui::Context,
        local_storage: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        kit::text::enable_text_recording(ctx);
        let mut app = Self::setup(
            ctx,
            Clock::Virtual(0.0),
            prefs::Prefs::from_seed(local_storage),
            pdn_core::fonts::FontPolicy::Bundled,
        );
        // Deterministic effect previews: instance seeds 0, 1, … (adjustments-and-effect-framework.md
        // §2.6 step 5).
        app.app.effect_seed = Some(0);
        // Deterministic frames: thumbnails are scaled in the frame that needs them.
        app.shell
            .set_thumbnail_mode(kit::thumbnails::ThumbnailMode::Synchronous);
        // As the web build, which welcomes (WelcomeDialog.open).
        app.open_welcome();
        app
    }

    /// `WelcomeDialog.open()` at startup: unless `paintdotjs.welcome.dismissed` is set.
    fn open_welcome(&mut self) {
        if dialogs::about::AboutDialog::welcome_due(&self.prefs) {
            self.shell
                .dialogs
                .push(Box::new(dialogs::about::AboutDialog::new(true)));
        }
    }

    pub fn advance_clock(&mut self, ms: u32) {
        if let Clock::Virtual(t) = &mut self.clock {
            *t += ms as f64;
        }
    }

    fn setup(
        ctx: &egui::Context,
        clock: Clock,
        mut prefs: prefs::Prefs,
        policy: pdn_core::fonts::FontPolicy,
    ) -> Self {
        use pdn_core::settings::{AppSettings, CHECKERBOARD_MIGRATION_KEY, STORAGE_KEY};
        use pdn_core::tool::types::{SHARED_MIGRATION_KEY, SHARED_MIGRATION_VERSION};
        // The chrome is pixel-snapped boxes and per-pixel glyph tables; egui's AA feathering
        // would blur 1 px shapes (and turns rects <= 2 px into feathered line segments).
        ctx.tessellation_options_mut(|o| o.feathering = false);
        // A DOM `click` is a press and release on the same element, however long it is held.
        // Ctrl +/−/0 are the app's View shortcuts, not egui's UI zoom.
        ctx.options_mut(|o| {
            o.input_options.max_click_duration = f64::INFINITY;
            o.zoom_with_keyboard = false;
        });
        let migrated = prefs.get(CHECKERBOARD_MIGRATION_KEY) == Some("1");
        let (settings, migrated_raw) = AppSettings::load_with(prefs.get(STORAGE_KEY), migrated);
        if let Some(raw) = migrated_raw {
            prefs.set(STORAGE_KEY, &raw);
        }
        if !migrated {
            prefs.set(CHECKERBOARD_MIGRATION_KEY, "1");
        }
        let selected = icon_pack::IconPackId::from_saved(prefs.get(icon_pack::SELECTION_KEY));
        if prefs.get(icon_pack::SELECTION_KEY).is_none() {
            prefs.set(icon_pack::SELECTION_KEY, &selected.saved());
        }
        assets::Assets::select(ctx, selected);
        if let Some(icon) = assets::reset_nub_icon() {
            pdn_core::render::nubs::set_reset_icon(icon);
        }
        install_fonts(ctx, policy, settings.get_str("ui.fontFamily"));
        let shortcuts = pdn_core::action::ShortcutRegistry::load(
            prefs.get(pdn_core::action::SHORTCUTS_STORAGE_KEY),
        );
        // AppWorkspace / AppView constructors: the saved unit, grid and rulers.
        let mut app = pdn_core::app::AppWorkspace::default();
        // The canvas draws Move Selected Pixels' drags at display resolution.
        app.float_moves = true;
        app.measurement_unit =
            pdn_core::app::MeasurementUnit::from_id(settings.get_str("workspace.measurementUnit"))
                .unwrap_or(pdn_core::app::MeasurementUnit::Pixel);
        app.grid_visible = settings.get_bool("workspace.showPixelGrid");
        app.rulers_visible = settings.get_bool("workspace.showRulers");
        // ColorsForm's constructor: the saved colours, then its palettes.
        for (i, key) in forms::colors::COLOR_SETTINGS.into_iter().enumerate() {
            if let Some(c) = settings.get(key).as_i64().map(forms::colors::from_packed) {
                *if i == 0 {
                    &mut app.primary_color
                } else {
                    &mut app.secondary_color
                } = c;
            }
        }
        let locale = if matches!(clock, Clock::Virtual(_)) {
            "en".into()
        } else {
            language::system_locale()
        };
        language::apply_with_locale(ctx, &prefs, &settings, &locale);
        let mut shell = shell::Shell::default();
        shell.forms.colors.load_prefs(&prefs);
        // The ToolType constructors: `paintdotjs.toolSettings.<id>` and the shared quality, with
        // the DPI-dependent defaults of the startup scale (R19).
        app.tool_settings = pdn_core::tool::types::ToolSettings::load_scaled(
            |k| prefs.get(k).map(String::from),
            ctx.pixels_per_point(),
        );
        // The one-time shared-settings migration has run (Task 17h ruling). Its values reach
        // prefs before the marker does: eframe's web runner can `save()` before the first `ui`.
        persist_tool_settings(&mut app.tool_settings, &mut prefs);
        if prefs.get(SHARED_MIGRATION_KEY) != Some(SHARED_MIGRATION_VERSION) {
            prefs.set(SHARED_MIGRATION_KEY, SHARED_MIGRATION_VERSION);
        }
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            native_launch: None,
            #[cfg(target_os = "linux")]
            wayland_dnd: None,
            #[cfg(target_os = "macos")]
            tablet_monitor: None,
            #[cfg(target_arch = "wasm32")]
            web_pen_events: Default::default(),
            #[cfg(target_arch = "wasm32")]
            web_pen_listener: None,
            #[cfg(target_arch = "wasm32")]
            web_close_requested: false,
            clock,
            asset_library: platform::asset_library::AssetLibrary::new(
                platform::asset_library::AssetStore::memory(),
            ),
            fonts: platform::fonts::FontManager::new(
                platform::fonts::FontStore::memory(),
                pdn_core::fonts::FontPolicy::Bundled,
            ),
            prefs,
            applied_ui_font: settings.get_str("ui.fontFamily").to_owned(),
            settings,
            shortcuts,
            assets: assets::Assets,
            app,
            shell,
            incoming_files: Default::default(),
        }
    }

    fn apply_display_settings(&mut self, ctx: &egui::Context) {
        kit::text::activate_ui_face(ctx);
        use theme::Scheme;
        let scheme = match self.settings.get_str("ui.colorScheme") {
            "light" => Scheme::Light,
            "blue" => Scheme::Blue,
            "dark" => Scheme::Dark,
            _ => {
                if ctx.system_theme() == Some(egui::Theme::Light) {
                    Scheme::Light
                } else {
                    Scheme::Dark
                }
            }
        };
        theme::set_current(ctx, theme::Theme::for_scheme(scheme));
        theme::set_animations(ctx, self.settings.get_bool("ui.animations"));
        let family = self.settings.get_str("ui.fontFamily");
        if family != self.applied_ui_font {
            install_fonts(ctx, pdn_core::fonts::FontPolicy::Bundled, family);
            self.applied_ui_font = family.into();
        }
    }

    /// Incoming-file consumer for argv and Task56 OS events. Deduplicates queued paths only,
    /// preserves arrival order, and drains after startup/modal/save work becomes ready.
    pub fn queue_picked_files(&mut self, files: Vec<platform::PickedFile>) {
        for file in files {
            if !self.incoming_files.iter().any(|f| {
                file.path
                    .as_ref()
                    .is_some_and(|p| f.path.as_ref() == Some(p))
                    || f.id == file.id
            }) {
                self.incoming_files.push_back(file);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn queue_incoming_files(&mut self, paths: Vec<std::path::PathBuf>) {
        self.queue_picked_files(
            paths
                .into_iter()
                .map(|path| {
                    let mut f = platform::PickedFile::snapshot(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into(),
                        vec![],
                    );
                    f.path = Some(path);
                    f
                })
                .collect(),
        );
    }
}

/// The clipboard shortcuts egui-winit would translate (`NativeOptions::clipboard_shortcuts` is
/// off): Ctrl+C/X/V and the dedicated keys, plus Windows' Ctrl+Insert, Shift+Delete and
/// Shift+Insert.
fn text_clipboard_command(key: egui::Key, m: egui::Modifiers) -> Option<egui::Event> {
    use egui::Key::{C, Copy, Cut, Delete, Insert, Paste, V, X};
    let windows = cfg!(target_os = "windows");
    let cmd = m.command_only();
    match key {
        Cut | X if key == Cut || cmd => Some(egui::Event::Cut),
        Delete if windows && m.shift => Some(egui::Event::Cut),
        Copy | C if key == Copy || cmd => Some(egui::Event::Copy),
        Insert if windows && m.ctrl => Some(egui::Event::Copy),
        Paste | V if key == Paste || cmd => Some(egui::Event::Paste(String::new())),
        Insert if windows && m.shift => Some(egui::Event::Paste(String::new())),
        _ => None,
    }
}

/// `KeyboardEvent.key` for an egui (logical) key, as `ShortcutKey.normalizeKey` expects it:
/// punctuation as its character, the arrows as "ArrowLeft" … (egui names them "Left" …),
/// everything else by name ("A", "F5", "Delete", "PageUp").
fn js_key(key: egui::Key) -> &'static str {
    use egui::Key::*;
    match key {
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        ArrowUp => "ArrowUp",
        ArrowDown => "ArrowDown",
        Plus => "+",
        Minus => "-",
        Equals => "=",
        Comma => ",",
        Period => ".",
        Colon => ":",
        Semicolon => ";",
        Slash => "/",
        Backslash => "\\",
        Pipe => "|",
        Questionmark => "?",
        Exclamationmark => "!",
        OpenBracket => "[",
        CloseBracket => "]",
        OpenCurlyBracket => "{",
        CloseCurlyBracket => "}",
        Backtick => "`",
        Quote => "'",
        other => other.name(),
    }
}

impl PaintApp {
    /// Natively the clipboard shortcuts arrive as plain keys (`main` turns egui-winit's
    /// translation off), so Ctrl+C/X/V always reach the action map, even with an empty system
    /// clipboard. While a text field has the keyboard they become egui's `Copy`/`Cut`/`Paste`
    /// again (the paste text from the system clipboard), so text fields keep their clipboard. The
    /// web's events come from the browser: nothing to do there.
    pub fn text_clipboard_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if cfg!(target_arch = "wasm32")
            || !(ctx.text_edit_focused() || self.shell.text_input_focused())
        {
            return;
        }
        let mut events = Vec::with_capacity(raw.events.len());
        for e in std::mem::take(&mut raw.events) {
            let command = match &e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => text_clipboard_command(*key, *modifiers),
                _ => None,
            };
            match command {
                Some(egui::Event::Paste(_)) => {
                    // egui-winit: CRLF → LF, and no event for empty text.
                    let text = self.shell.clipboard.read_text().unwrap_or_default();
                    let text = text.replace("\r\n", "\n");
                    if !text.is_empty() {
                        events.push(egui::Event::Paste(text));
                    }
                }
                Some(command) => events.push(command),
                None => events.push(e),
            }
        }
        raw.events = events;
    }

    /// DocumentIO.handleDroppedFiles awaits each category before the image choice.
    fn advance_drop_batches(&mut self, ctx: &egui::Context) {
        if self
            .shell
            .drop_choice
            .as_ref()
            .is_some_and(|choice| !choice.finished())
        {
            return;
        }
        self.shell.drop_choice = None;
        let Some(batch) = self.shell.drop_batches.front_mut() else {
            return;
        };
        if let Some(page) = batch.active {
            // Give the manager at least one poll after an import starts. An invalid file can
            // settle without ever setting busy, but must not advance in the starting frame.
            if batch.started {
                batch.started = false;
                ctx.request_repaint();
                return;
            }
            let busy = match page {
                "shapes" | "assets" => self.asset_library.busy(),
                "plugins" => self.asset_library.plugins.busy(),
                "fonts" => self.fonts.importing(),
                _ => unreachable!("classified drop page"),
            };
            if busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(10));
                return;
            }
            batch.active = None;
        }
        if let Some(step) = batch.imports.front() {
            let ready = match step.page {
                "shapes" | "assets" => self.asset_library.ready() && !self.asset_library.busy(),
                "plugins" => {
                    let plugins = &self.asset_library.plugins;
                    plugins.ready() && !plugins.busy()
                }
                "fonts" => !self.fonts.importing(),
                _ => unreachable!("classified drop page"),
            };
            if !ready {
                ctx.request_repaint_after(std::time::Duration::from_millis(10));
                return;
            }
        }
        if let Some(step) = batch.imports.pop_front() {
            if !step.silent {
                self.app
                    .ui_requests
                    .push(pdn_core::action::UiRequest::OpenSettings(Some(
                        step.page.into(),
                    )));
            }
            match step.page {
                "shapes" | "assets" => self.asset_library.import(step.page, step.files),
                "plugins" => self.asset_library.plugins.import(step.files),
                "fonts" => self.fonts.import(step.files),
                _ => unreachable!("classified drop page"),
            }
            batch.active = Some(step.page);
            batch.started = true;
            ctx.request_repaint();
            return;
        }
        let completed = self.shell.drop_batches.pop_front().expect("front batch");
        if !completed.images.is_empty() {
            self.shell.drop_choice = Some(platform::files::show_drop_images(completed.images));
        }
        ctx.request_repaint();
    }
}

impl eframe::App for PaintApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(bridge) = self.native_launch.clone() {
            bridge.deliver(self, ctx);
        }
        #[cfg(target_os = "macos")]
        platform::launch::macos::settle_canceled_quit();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn on_exit(&mut self) {
        if let Some(bridge) = &self.native_launch {
            bridge.return_pending(
                self.incoming_files
                    .drain(..)
                    .filter_map(|f| f.path)
                    .collect(),
            );
            bridge.detach();
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.prefs.save(storage);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.text_clipboard_input(ctx, raw);
        #[cfg(target_os = "linux")]
        if let Some(dnd) = &self.wayland_dnd {
            dnd.feed(raw, ctx.zoom_factor());
        }
        #[cfg(target_arch = "wasm32")]
        self.web_pen_events.borrow_mut().append(
            raw,
            &mut self.shell.canvas_view.pen,
            self.clock.now_ms(),
            ctx.zoom_factor(),
        );
    }

    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        kit::text::begin_frame(&ctx);
        let initial_buttons = self.app.mouse_buttons;
        self.apply_display_settings(&ctx);
        let (m, buttons) = ui.input(|i| {
            let p = &i.pointer;
            let bit = |down: bool, b: pdn_core::tool::MouseButton| (down as u8) << b as u8;
            (
                i.modifiers,
                bit(p.primary_down(), pdn_core::tool::MouseButton::Left)
                    | bit(p.middle_down(), pdn_core::tool::MouseButton::Middle)
                    | bit(p.secondary_down(), pdn_core::tool::MouseButton::Right)
                    | bit(
                        p.button_down(egui::PointerButton::Extra1),
                        pdn_core::tool::MouseButton::X1,
                    )
                    | bit(
                        p.button_down(egui::PointerButton::Extra2),
                        pdn_core::tool::MouseButton::X2,
                    ),
            )
        });
        self.shell.canvas_view.pen.enabled = self.settings.get_bool("pen.pointerInput");
        self.shell
            .canvas_view
            .pen
            .observe(&ui.input(|i| i.events.clone()));
        self.app.tool_settings.pen_seen =
            self.shell.canvas_view.pen.state.pen_seen && self.shell.canvas_view.pen.enabled;
        let pdn = pdn_modifiers(m);
        self.app.set_modifier_keys(pdn.ctrl, pdn.shift, pdn.alt);
        // A key repeats alone, so the tools learn the other arrows held from here.
        self.app.arrow_keys_down = ui.input(|i| {
            use egui::Key::{ArrowDown, ArrowLeft, ArrowRight, ArrowUp};
            [ArrowLeft, ArrowRight, ArrowUp, ArrowDown].map(|k| i.key_down(k))
        });
        // Buttons held anywhere (a toolbar slider drag too): the transacted tools' settings
        // window and the generic Escape handler read them (painting-tools-2.md §7.4, §7.6).
        self.app.mouse_buttons = buttons;
        self.app.on_frame(self.clock.now_ms());
        if self.app.tool_ticking() {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                pdn_core::tool::transacted::SETTINGS_POLL_MS as u64,
            ));
        }
        // ModalDialogController: under an open dialog the app is inert — no shortcuts, and the
        // shell (forms, menus, canvas) sees no pointer, wheel or keys.
        platform::files::poll(
            &ctx,
            &mut self.app,
            &mut self.settings,
            &mut self.shell.settings_changed,
        );
        if self.shell.desktop {
            use shell::window_close::{self, Intent};
            let approved = matches!(window_close::state(&ctx).outcome, Some(Ok(_)));
            if ctx.input(|i| i.viewport().close_requested()) && !approved {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                window_close::request(&ctx, Intent::CloseRoot);
            }
            let launch_ready = self.incoming_files.is_empty();
            #[cfg(not(target_arch = "wasm32"))]
            let launch_ready = launch_ready
                && !self
                    .native_launch
                    .as_ref()
                    .is_some_and(|b| b.pending_paths());
            if launch_ready
                && !self.shell.dialogs.is_open()
                && !platform::files::busy()
                && window_close::begin(&ctx)
            {
                dialogs::flows::review_close(
                    &mut self.app,
                    &mut self.settings,
                    &mut self.shell.settings_changed,
                    &ctx,
                );
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            if std::mem::take(&mut self.shell.web_image_paste_event) {
                // The browser supplied this image. Consume its queued plain paste shortcut,
                // leaving text and modified paste shortcuts with their existing owners.
                ctx.input_mut(|i| {
                    i.events.retain(|event| !matches!(event,
                    egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. }
                    if (modifiers.ctrl || modifiers.command) && !modifiers.alt && !modifiers.shift
                ))
                });
            }
            self.shell.poll_web_paste(&ctx, &mut self.app);
            if self.web_close_requested && !self.shell.dialogs.is_open() && !platform::files::busy()
            {
                self.web_close_requested = false;
                if self.app.documents.iter().any(|w| w.dirty) {
                    dialogs::flows::exit_web(
                        &mut self.app,
                        &mut self.settings,
                        &mut self.shell.settings_changed,
                        &ctx,
                    );
                }
            }
        }
        let modal = self.shell.dialogs.is_open() || platform::files::busy();
        let font_drop = self.shell.dialogs.top_is("settings")
            && self.shell.settings_dialog.section() == "fonts";
        let asset_drop = self.shell.dialogs.top_is("settings")
            && matches!(self.shell.settings_dialog.section(), "shapes" | "assets");
        // A second raw drop on this choice joins the FIFO; other modal input stays masked.
        let drop_choice_top = self
            .shell
            .drop_choice
            .as_ref()
            .is_some_and(|choice| choice.awaiting_choice())
            && self.shell.dialogs.top_title_is("Drag and Drop");
        if !modal || font_drop || asset_drop || drop_choice_top {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            if !dropped.is_empty() {
                let files = dropped
                    .into_iter()
                    .map(|d| {
                        let name = d
                            .path()
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let mut f = platform::PickedFile::snapshot(name, vec![]);
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            f.path = Some(d.path().to_owned());
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            f.mime = d.web_file().map(|f| f.type_()).filter(|s| !s.is_empty());
                        }
                        f.dropped = Some(d);
                        f
                    })
                    .collect();
                self.shell
                    .drop_batches
                    .push_back(platform::files::dropped(files));
            }
        }
        if !self.fonts.importing() {
            let ready = self.shell.settings_dialog.is_open()
                && self.shell.settings_dialog.section() == "fonts";
            let mut files = vec![];
            self.shell.settings_imports.retain(|import| {
                if import.page == "fonts" && (import.silent || ready) {
                    files.extend(import.files.clone());
                    false
                } else {
                    true
                }
            });
            if !files.is_empty() {
                self.fonts.import(files);
            }
        }
        self.asset_library
            .poll(&ctx, &mut self.app, &mut self.prefs);
        if let Some(language) = self.asset_library.normalized_language.take() {
            self.settings.set("ui.language", language.into());
            self.shell.settings_changed = true;
        }
        if self.asset_library.ready()
            && !self.asset_library.busy()
            && self.shell.dialogs.top_is("settings")
        {
            let page = self.shell.settings_dialog.section();
            if matches!(page, "shapes" | "assets") {
                let mut files = vec![];
                self.shell.settings_imports.retain(|import| {
                    if import.page == page {
                        files.extend(import.files.clone());
                        false
                    } else {
                        true
                    }
                });
                if !files.is_empty() {
                    self.asset_library.import(page, files);
                }
            }
        }
        use pdn_core::fonts::FaceStyle;
        let family = self
            .app
            .tool_settings
            .get(pdn_core::tool::types::ToolType::Text, "fontFamily");
        if self.app.tool_type == Some(pdn_core::tool::types::ToolType::Text) {
            let bold = self
                .app
                .tool_settings
                .get(pdn_core::tool::types::ToolType::Text, "bold")
                .as_bool()
                .unwrap_or(false);
            let italic = self
                .app
                .tool_settings
                .get(pdn_core::tool::types::ToolType::Text, "italic")
                .as_bool()
                .unwrap_or(false);
            let style = match (bold, italic) {
                (true, true) => FaceStyle::BoldItalic,
                (true, false) => FaceStyle::Bold,
                (false, true) => FaceStyle::Italic,
                _ => FaceStyle::Regular,
            };
            self.fonts
                .ensure_family(family.as_str().unwrap_or("Calibri"), style);
            if let Some(delay) = self
                .fonts
                .retry_delay(family.as_str().unwrap_or("Calibri"), style)
            {
                ctx.request_repaint_after(delay);
            }
            let needs_color = self
                .app
                .with_tool(|tool, app| {
                    tool.as_any_mut()
                        .downcast_mut::<pdn_core::tool::text::TextTool>()
                        .is_some_and(|text| text.needs_color_font(app))
                })
                .unwrap_or(false);
            let scripts = self
                .app
                .with_tool(|tool, app| {
                    tool.as_any_mut()
                        .downcast_mut::<pdn_core::tool::text::TextTool>()
                        .map(|text| text.script_fallbacks(app))
                })
                .flatten()
                .unwrap_or_default();
            for family in needs_color
                .then_some("Noto Color Emoji")
                .into_iter()
                .chain(scripts)
            {
                self.fonts.ensure_family(family, FaceStyle::Regular);
                if let Some(delay) = self.fonts.retry_delay(family, FaceStyle::Regular) {
                    ctx.request_repaint_after(delay);
                }
            }
        }
        let ui_family = self.settings.get_str("ui.fontFamily");
        if pdn_core::fonts::BUNDLED
            .iter()
            .any(|(f, _)| f.eq_ignore_ascii_case(ui_family))
        {
            self.fonts.ensure_family(ui_family, FaceStyle::Regular);
        }
        let font_revision = self.fonts.revision;
        self.fonts.poll();
        if self.fonts.revision != font_revision {
            self.app
                .with_tool(|tool, app| tool.on_setting_changed(app, "fontFamily"));
        }
        self.fonts.sync(&ctx, &mut self.prefs, ui_family);
        if self.fonts.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(10));
        }
        self.advance_drop_batches(&ctx);
        let modal = modal || self.shell.dialogs.is_open() || platform::files::busy();
        let (shell, app, settings) = (&mut self.shell, &mut self.app, &mut self.settings);
        if modal {
            // The dialog takes the capture: a stroke held under it ends as a release (§8.11).
            shell.canvas_view.lose_capture(app);
            dialogs::without_input(&ctx, || {
                shell.show(ui, app, settings, &self.shortcuts, initial_buttons)
            });
        } else {
            self.shell.show(
                ui,
                &mut self.app,
                &mut self.settings,
                &self.shortcuts,
                initial_buttons,
            );
        }
        let mut cx = dialogs::DialogCx {
            app: &mut self.app,
            settings: &mut self.settings,
            prefs: &mut self.prefs,
            settings_changed: false,
            shortcuts: Some(&mut self.shortcuts),
            clipboard: Some(&mut self.shell.clipboard),
            settings_imports: Some(&mut self.shell.settings_imports),
            fonts: Some(&mut self.fonts),
            asset_library: Some(&mut self.asset_library),
            seat_focus: false,
        };
        self.shell.dialogs.show(ui, &mut cx);
        // On the app clock (a test's virtual clock moves only when it advances it).
        kit::tooltip::show(&ctx, self.clock.now_ms());
        let dialog_settings = cx.settings_changed;
        if !self.shell.dialogs.is_open()
            && !platform::files::busy()
            && !matches!(shell::window_close::state(&ctx).outcome, Some(Ok(_)))
            && !self.incoming_files.is_empty()
        {
            let files = self.incoming_files.drain(..).collect();
            platform::files::open_files(
                files,
                false,
                &mut self.app,
                &mut self.settings,
                &mut self.shell.settings_changed,
            );
        }
        // `setMainColor`/`setSecondaryColor`: `tools.primaryColor`/`secondaryColor` follow the
        // colours (whoever changed them).
        let mut colors_changed = false;
        for (key, c) in forms::colors::COLOR_SETTINGS
            .into_iter()
            .zip([self.app.primary_color, self.app.secondary_color])
        {
            let v = forms::colors::to_packed(c);
            if self.settings.get(key).as_i64() != Some(v) {
                self.settings.set(key, v.into());
                colors_changed = true;
            }
        }
        self.shell.forms.colors.persist(&mut self.prefs);
        if std::mem::take(&mut self.shell.settings_changed) || dialog_settings || colors_changed {
            self.prefs
                .set(pdn_core::settings::STORAGE_KEY, &self.settings.to_json());
        }
        persist_tool_settings(&mut self.app.tool_settings, &mut self.prefs);
        // Pack activation and dictionary preparation must survive an immediate reload; the
        // ordinary autosave interval is too late for the Settings reload action (Task55).
        let reload_language = self.shell.settings_dialog.take_reload_requested();
        if let Some(storage) = frame.storage_mut()
            && (self.asset_library.take_preferences_changed() || reload_language)
        {
            self.prefs.save(storage);
            storage.flush();
        }
        if reload_language {
            #[cfg(not(target_arch = "wasm32"))]
            {
                apply_language(&ctx, &self.prefs, &self.settings);
                self.shell.forms.refresh_titles();
                self.shell.settings_dialog.language_applied(&ctx);
                ctx.request_repaint();
            }
            #[cfg(target_arch = "wasm32")]
            if let Some(window) = web_sys::window()
                && let Err(error) = window.location().reload()
            {
                eprintln!("Could not reload the page: {error:?}");
            }
        }
        // The tool cursor goes to the OS (CSS on the web) once nothing else can change the cursor.
        let tool_cursor = shell::canvas_view::finish_cursor(&ctx);
        #[cfg(target_arch = "wasm32")]
        platform::web::set_canvas_cursor(tool_cursor.as_ref(), ctx.pixels_per_point());
        #[cfg(not(target_arch = "wasm32"))]
        let _ = tool_cursor;
        // Synthetic capture releases do not release a physically held button (PointerGone).
        // Keep the sampled frame-end state for polling and next frame's event-local replay.
        self.app.mouse_buttons = buttons;
        // A Playground live session: the others' changes go in, this person's go out, and the others are drawn.
        #[cfg(target_arch = "wasm32")]
        {
            live::frame(self, &ctx);
            live::draw(self, ui);
        }
    }
}

/// `ToolType.setSetting`'s localStorage writes: the tool settings written since the last call.
fn persist_tool_settings(
    tools: &mut pdn_core::tool::types::ToolSettings,
    prefs: &mut prefs::Prefs,
) {
    for (tool, key) in tools.dirty_keys() {
        let (k, v) = tools.storage_entry(&tool, &key);
        prefs.set(&k, &v);
    }
}

/// `Bundled` on the web or when `PAINTRS_BUNDLED_FONTS` is set, else `System` (spec §0.5).
fn default_policy() -> pdn_core::fonts::FontPolicy {
    use pdn_core::fonts::FontPolicy;
    if cfg!(target_arch = "wasm32") || std::env::var_os("PAINTRS_BUNDLED_FONTS").is_some() {
        FontPolicy::Bundled
    } else {
        FontPolicy::System
    }
}

/// Loads the fonts for the `ui.fontFamily` setting (spec §0.5, `ui-text-and-glyphs.md §2.2`):
/// "Segoe UI" and "Selawik" are the bundled Selawik (ruling R47: the free face, never the OS's
/// Segoe UI), another bundled family its `kit::fonts` face (Noto Sans when the build lacks
/// it), a generic its `resolve_family` face. Families: `Proportional` = [UI face, Noto Sans],
/// `Name("ui-bold")` = [UI bold face, Noto Sans], `Name("ui-semibold")` = [UI semibold face, Noto
/// Sans] (paintdotjs's weight 600), `Name("Calibri")` = [Carlito, Noto Sans], plus the
/// R131 appends Noto Sans Arabic and Hebrew as UI fallbacks after those existing faces, then
/// (R183 follow-up) every loaded lazy script face and Noto Color Emoji.
/// Text-tool faces remain installed by name (`kit::fonts::install`). Every UI face is tweaked by
/// `kit::fonts::pdn_hinting`. Called once per context.
pub fn install_fonts(ctx: &egui::Context, _policy: pdn_core::fonts::FontPolicy, ui_family: &str) {
    use egui::{FontData, FontFamily};
    use pdn_core::canvas::text::{NOTO_SANS, NOTO_SANS_ARABIC, NOTO_SANS_HEBREW, NOTO_SANS_MONO};
    use pdn_core::fonts::{self, FaceStyle, FontPolicy};
    use std::borrow::Cow;
    use std::sync::Arc;
    let mut defs = egui::FontDefinitions::empty();
    // Embedded once, in pdn-core (the canvas text registry uses the same bytes).
    defs.font_data.insert(
        "noto-sans".into(),
        Arc::new(FontData::from_static(NOTO_SANS)),
    );
    defs.font_data.insert(
        "noto-sans-mono".into(),
        Arc::new(FontData::from_static(NOTO_SANS_MONO)),
    );
    // R131: verified unmodified OFL faces add missing RTL glyphs after R47's existing faces.
    // R183 follow-up: then the lazy script faces and the colour emoji face, once registered
    // (`FontManager::sync` reinstalls the fonts whenever one loads).
    let lazy = fonts::SCRIPT_FALLBACKS
        .iter()
        .map(|(family, ..)| *family)
        .chain(["Noto Color Emoji"])
        .filter_map(|family| {
            let bytes = fonts::registered_face(family, FaceStyle::Regular)?;
            Some((
                format!("fallback:{family}"),
                FontData::from_blob(Arc::new(bytes), 0),
            ))
        });
    let mut fallbacks = vec![];
    for (name, data) in [
        (
            "noto-sans-arabic".to_string(),
            FontData::from_static(NOTO_SANS_ARABIC),
        ),
        (
            "noto-sans-hebrew".to_string(),
            FontData::from_static(NOTO_SANS_HEBREW),
        ),
    ]
    .into_iter()
    .chain(lazy)
    {
        let tweak = kit::fonts::pdn_hinting(data.bytes());
        defs.font_data
            .insert(name.clone(), Arc::new(data.tweak(tweak)));
        fallbacks.push(name);
    }
    defs.families.insert(
        FontFamily::Name("diagnostics-mono".into()),
        [vec!["noto-sans-mono".into()], fallbacks.clone()].concat(),
    );
    defs.families.insert(
        FontFamily::Monospace,
        [
            vec!["noto-sans-mono".into(), "noto-sans".into()],
            fallbacks.clone(),
        ]
        .concat(),
    );
    kit::fonts::install(&mut defs);
    // `wght` picks the weight of a variable face; static faces ignore it.
    let add = |defs: &mut egui::FontDefinitions, name: &str, bytes: Cow<'static, [u8]>, wght| {
        let tweak = egui::FontTweak {
            coords: egui::epaint::text::VariationCoords::new([(b"wght", wght)]),
            ..kit::fonts::pdn_hinting(&bytes)
        };
        let data = match bytes {
            Cow::Borrowed(b) => FontData::from_static(b),
            Cow::Owned(b) => FontData::from_owned(b),
        };
        defs.font_data
            .insert(name.into(), Arc::new(data.tweak(tweak)));
        name.to_string()
    };
    // Ruling R47 (user): the interface always uses the free bundled faces, Selawik for Segoe UI
    // and Carlito for Calibri, so no OS font dirs are walked here whatever the policy says.
    let candidates = fonts::system_font_files(FontPolicy::Bundled);
    let resolved = fonts::resolve_family(ui_family);
    let ui_policy = if ui_family.eq_ignore_ascii_case("Segoe UI") {
        Some(FontPolicy::Bundled)
    } else {
        resolved
            .eq_ignore_ascii_case("Selawik")
            .then_some(FontPolicy::Bundled)
    };
    let ui_bytes = |style| match ui_policy {
        Some(p) => fonts::ui_face_from(&candidates, p, style).bytes,
        // ponytail: a static bundled face has one weight here, so its bold and semibold draw
        // Regular (variable faces take `wght`); embed their Bold files if a non-default
        // ui.fontFamily matters.
        None => {
            if fonts::BUNDLED
                .iter()
                .any(|(f, _)| f.eq_ignore_ascii_case(resolved))
            {
                fonts::registered_face(resolved, style)
                    .map(|b| Cow::Owned(b.to_vec()))
                    .or_else(|| kit::fonts::face_bytes(resolved).map(Cow::Borrowed))
                    .unwrap_or(Cow::Borrowed(NOTO_SANS))
            } else {
                Cow::Borrowed(NOTO_SANS)
            }
        }
    };
    let regular = ui_bytes(FaceStyle::Regular);
    let measure_family = if resolved.eq_ignore_ascii_case("Selawik")
        || fonts::BUNDLED
            .iter()
            .any(|(f, _)| f.eq_ignore_ascii_case(resolved))
    {
        resolved
    } else {
        "Noto Sans"
    };
    kit::text::set_ui_face(ctx, measure_family, &regular);
    let segoe = add(
        &mut defs,
        "segoe-ui",
        Cow::Borrowed(fonts::SELAWIK_REGULAR),
        400.0,
    );
    let ui = add(&mut defs, "ui", regular, 400.0);
    let ui_medium = add(&mut defs, "ui-medium", ui_bytes(FaceStyle::Regular), 500.0);
    let ui_bold = add(&mut defs, "ui-bold", ui_bytes(FaceStyle::Bold), 700.0);
    let ui_semibold = add(
        &mut defs,
        "ui-semibold",
        ui_bytes(FaceStyle::Semibold),
        600.0,
    );
    let calibri = fonts::calibri_face_from(&candidates, FontPolicy::Bundled, FaceStyle::Regular);
    let calibri = add(&mut defs, "calibri", calibri.bytes, 400.0);
    for (family, first) in [
        (FontFamily::Proportional, ui),
        (FontFamily::Name("Segoe UI".into()), segoe),
        (FontFamily::Name("ui-bold".into()), ui_bold),
        (FontFamily::Name("ui-medium".into()), ui_medium),
        (FontFamily::Name("ui-semibold".into()), ui_semibold),
        (FontFamily::Name("Calibri".into()), calibri),
    ] {
        defs.families.insert(
            family,
            [vec![first, "noto-sans".into()], fallbacks.clone()].concat(),
        );
    }
    kit::fonts::install_document_previews(ctx, &mut defs);
    ctx.set_fonts(defs);
    ctx.all_styles_mut(|s| {
        // Fractional advances (`ui-text-and-glyphs.md §2.4` step 5); per-face tweaks decide the
        // hinting (`kit::fonts::pdn_hinting`).
        s.visuals.text_options.subpixel_binning = true;
        s.visuals.text_options.font_hinting = true;
        // Grayscale coverage → alpha. calibrate: B-T1 (Appendix B), Paint.NET's own curve. Until
        // then the references' Skia A8 preblend, measured: for white text it is coverage^(1/1.2)
        // (SK_GAMMA_EXPONENT 1.2), lighter for darker text; one global curve serves all colours.
        // 0.85 passes every text rect (mean text-rect diff 2.06, egui's dark default 5.70);
        // 1/1.2 fails the disabled "Acquire" in menu-file, 0.9 the disabled "…" in menu-image.
        s.visuals.text_options.color_transfer_function =
            egui::epaint::FontColorTransferFunction::Gamma(0.85);
    });
}

/// Paint.NET's modifiers from egui's: on a Mac, Cmd (egui's `command`) is Ctrl, as Mac users
/// press Cmd+Z, Cmd+S and Cmd+click where Paint.NET has Ctrl, and then no longer counts as Meta.
/// Elsewhere `command` is Ctrl itself, so nothing changes.
pub fn pdn_modifiers(mut m: egui::Modifiers) -> egui::Modifiers {
    if m.command {
        (m.ctrl, m.mac_cmd) = (true, false);
    }
    m
}

/// One key in the ordered canvas event stream. Widget ownership is checked by Shell.
fn dispatch_canvas_key(
    app: &mut pdn_core::app::AppWorkspace,
    shortcuts: &pdn_core::action::ShortcutRegistry,
    colors: &mut forms::colors::ColorsForm,
    strip: &mut shell::document_strip::DocumentStrip,
    key: &str,
    m: egui::Modifiers,
    captured: bool,
) {
    let m = pdn_modifiers(m);
    let now = app.now_ms;
    // Ctrl+Page is an outer form command; focused Ctrl+Tab reaches Text first (§10.11).
    if key != "Tab" && strip.navigate(app, key, m, captured) {
        return;
    }
    let shapes = matches!(
        app.tool_type,
        Some(pdn_core::tool::ToolType::Shapes | pdn_core::tool::ToolType::Line)
    );
    let toolbar_candidate = key.eq_ignore_ascii_case("c")
        || (shapes && matches!(key, "a" | "A" | "x" | "X" | "[" | "]" | "," | "." | "/"));
    // Only these toolbar characters need upstream global interception. Keep other
    // tool-key precedence unchanged, including Text and widget consumption above.
    let global_toolbar = toolbar_candidate
        && pdn_core::action::ACTIONS.iter().any(|action| {
            pdn_core::tool::ToolType::from_id(action.id).is_none()
                && pdn_core::action::is_executable(app, action.id)
                && shortcuts
                    .shortcut(action.id)
                    .is_some_and(|s| s.matches(key, m.shift, m.ctrl, m.alt, m.mac_cmd))
        });
    if shapes && global_toolbar {
        pdn_core::action::dispatch_key(app, shortcuts, key, m.shift, m.ctrl, m.alt, m.mac_cmd, now);
        return;
    }
    // egui supplies physical punctuation in Key events. Translate the Shapes toolbar
    // subset here; do not also dispatch Text events (which would cycle twice).
    let tool_key = if shapes {
        match (key, m.shift, m.ctrl) {
            ("[", true, _) => "{",
            ("]", true, _) => "}",
            (",", true, _) => "<",
            (".", true, _) => ">",
            ("/", true, _) => "?",
            ("[", false, true) => "\u{001b}",
            ("]", false, true) => "\u{001d}",
            _ => key,
        }
    } else {
        key
    };
    // `onKeyPress`: the active tool sees the remaining key first.
    let press = pdn_core::tool::KeyPress {
        key: tool_key.to_string(),
        shift: m.shift,
        ctrl: m.ctrl,
        alt: m.alt,
        meta: m.mac_cmd,
    };
    if app.on_key_press(&press) {
        return;
    }
    if strip.navigate(app, key, m, captured) {
        return;
    }
    // Native toolbar C toggles the selected palette slot. The active tool and
    // text/widget consumption above retain first refusal (Shapes suppresses gestures).
    if key.eq_ignore_ascii_case("c") && !global_toolbar {
        colors.select_secondary(!colors.secondary_selected());
        return;
    }
    pdn_core::action::dispatch_key(app, shortcuts, key, m.shift, m.ctrl, m.alt, m.mac_cmd, now);
}
