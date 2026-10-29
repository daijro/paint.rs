//! The window shell: header strips, editor (canvas view, scrollbars, rulers), footer and the
//! drop menus over them.
//! Port of: css/layout.css, css/header.css (`#titleBar`, `#mainMenu`, `#documentStrip`,
//! `#settingsMenu`, `#commonMenu`, `#toolMenu`), css/footer.css,
//! js/src/interface/panel/{Panel,PanelRegistry}.js, js/src/engine/AppView.js
//! (`updateCanvasBounds`, `syncRulerVisibility`, the `#view` scroll listener)
//! Reference: paintdotjs @ b4e6fc5
//!
//! `PanelRegistry.initialize` order — MainMenu, DocumentStrip, CommonMenu, SettingsMenu,
//! ToolMenu, FooterMenu — is the paint order below, except the document strip: its `z-index:
//! 200` puts it (and its list popup) over the tool strip and the editor, under the floating forms
//! (`.window { z-index: 1000 }`), which lie under the tool-option popups (4000) and drop menus
//! (3000). Everything paints in one layer, the drop menus last (menu hit-testing and the CPU
//! shadow replay need that).
pub mod canvas_view;
pub mod common_menu;
pub mod document_strip;
pub mod footer;
pub mod main_menu;
pub mod settings_menu;
pub mod titlebar;
pub mod tool_menu;
pub mod window_close;

use egui::{Pos2, Rect, Ui, Vec2};
use pdn_core::action::{self, ActionOutcome, PasteKind, ShortcutRegistry, UiRequest};
use pdn_core::app::{AppEvent, AppWorkspace};
use pdn_core::i18n;
use pdn_core::io::clipboard::{self, PasteExpand};
use pdn_core::settings::AppSettings;

use crate::dialogs::task::{TaskChoice, TaskDialog, TaskDialogSpec};
use crate::kit::menu::{self, DropMenu, DropMenuState, MenuContext, MenuPick, MenuStyle};
use crate::kit::px;
use crate::kit::scroll::{self, Scrollbar};
use crate::theme::{self, *};

/// Rects of the shell's CSS boxes (DIPs = CSS px of the patched references), as Chromium lays
/// them out (fractional where text widths make them so; painters snap).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellLayout {
    pub title_bar: Rect,
    pub main_menu: Rect,
    pub document_strip: Rect,
    pub settings_menu: Rect,
    pub common_menu: Rect,
    pub tool_menu: Rect,
    pub editor: Rect,
    pub footer: Rect,
}

impl ShellLayout {
    /// `desktop` selects the Electron layout (`--window-top`, window controls), which Task 56
    /// ports. `main_menu_width` is the `#mainMenu`
    /// border box width (it fits its text items, see `main_menu::width`).
    pub fn compute(
        screen: Rect,
        desktop: bool,
        main_menu_width: f32,
        document_count: usize,
    ) -> ShellLayout {
        let top = if desktop { DESKTOP_TITLE_HEIGHT } else { 0.0 };
        let (x0, y0, w) = (screen.min.x, screen.min.y + top, screen.width());
        let title_h = TITLE_BAR_HEIGHT + 2.0 * TITLE_BAR_BORDER;
        let title_bar = Rect::from_min_size(Pos2::new(x0, y0), Vec2::new(w, title_h));
        let main_menu = Rect::from_min_size(
            Pos2::new(
                x0 + MAIN_MENU_MARGIN_LEFT,
                y0 + TITLE_BAR_BORDER + MAIN_MENU_MARGIN_TOP,
            ),
            Vec2::new(main_menu_width, MAIN_MENU_HEIGHT),
        );
        let settings_menu = {
            let width = settings_menu::width();
            let height = SETTINGS_ICON_ITEM + 2.0 * SETTINGS_MENU_PADDING;
            Rect::from_min_size(
                Pos2::new(
                    screen.max.x - SETTINGS_MENU_MARGIN_RIGHT - width,
                    y0 + TITLE_BAR_BORDER + SETTINGS_MENU_TOP,
                ),
                Vec2::new(width, height),
            )
        };
        let common_menu = Rect::from_min_size(
            Pos2::new(
                x0 + COMMON_MENU_MARGIN.0,
                title_bar.max.y + COMMON_MENU_MARGIN.1,
            ),
            Vec2::new(common_menu::width(), COMMON_MENU_HEIGHT),
        );
        // `ui-text-and-glyphs.md §7.4`: the strip spans the menu and toolbar rows, from the menu
        // row's top to `DOCUMENT_STRIP_CLEARANCE` above the toolbar row's bottom (Paint.NET's
        // toolbar layout), so it never reaches the tool-options row; its height is the item.
        let strip_top = y0 + TITLE_BAR_BORDER + DOCUMENT_STRIP_TOP - top;
        let item = common_menu.max.y - DOCUMENT_STRIP_CLEARANCE - strip_top;
        // DocumentStrip.updatePreferredWidth: ⌈count × item⌉ + the list button (item = the cell
        // and its border), 0 without documents; `flex: 0 1 auto; min-width: 0` lets it give way
        // to the settings menu (whose `margin-left: auto` is then 0).
        let strip_x = main_menu.max.x + DOCUMENT_STRIP_MARGIN_LEFT;
        let strip_w = match document_count {
            0 => 0.0,
            n => {
                (n as f32 * (item + 2.0 * DOCUMENT_ITEM_BORDER)).ceil() + DOCUMENT_STRIP_LIST_BUTTON
            }
        };
        let document_strip = Rect::from_min_size(
            Pos2::new(strip_x, strip_top),
            Vec2::new(strip_w.min((settings_menu.min.x - strip_x).max(0.0)), item),
        );
        let tool_menu = Rect::from_min_size(
            Pos2::new(x0 + TOOL_MENU_MARGIN_X, common_menu.max.y),
            Vec2::new(w - 2.0 * TOOL_MENU_MARGIN_X, TOOL_MENU_HEIGHT),
        );
        let header_bottom = tool_menu.max.y + HEADER_BORDER;
        let footer = Rect::from_min_max(Pos2::new(x0, screen.max.y - FOOTER_HEIGHT), screen.max);
        let editor = Rect::from_min_max(
            Pos2::new(x0, header_bottom),
            Pos2::new(screen.max.x, footer.min.y),
        );
        ShellLayout {
            title_bar,
            main_menu,
            document_strip,
            settings_menu,
            common_menu,
            tool_menu,
            editor,
            footer,
        }
    }
}

/// `#editor` children: `#view` (scroll container), its client area (the `#canvas` size), the
/// scrollbars, and the rulers (`#editor.rulers-visible #view { top/left: 17px }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorRects {
    pub view: Rect,
    /// `#canvas`: at the editor origin, sized to `#view`'s client area. quirk: with rulers on it
    /// is NOT inset by the rulers (it stays at 0,0 under them) while `#view` is.
    pub canvas: Rect,
    /// `#view`'s client area (inside the scrollbars).
    pub client: Rect,
    pub client_size: Vec2,
    pub scrollbars: bool,
}

impl EditorRects {
    pub fn compute(editor: Rect, rulers: bool, scrollbars: bool) -> EditorRects {
        let inset = if rulers { RULER_SIZE } else { 0.0 };
        let view = Rect::from_min_max(editor.min + Vec2::splat(inset), editor.max);
        let bar = if scrollbars { scroll::THICKNESS } else { 0.0 };
        let client_size = view.size() - Vec2::splat(bar);
        EditorRects {
            view,
            canvas: Rect::from_min_size(editor.min, client_size),
            client: Rect::from_min_size(view.min, client_size),
            client_size,
            scrollbars,
        }
    }

    pub fn vertical_bar(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.view.max.x - scroll::THICKNESS, self.view.min.y),
            Pos2::new(self.view.max.x, self.view.max.y - scroll::THICKNESS),
        )
    }

    pub fn horizontal_bar(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.view.min.x, self.view.max.y - scroll::THICKNESS),
            Pos2::new(self.view.max.x - scroll::THICKNESS, self.view.max.y),
        )
    }

    pub fn scroll_corner(&self) -> Rect {
        Rect::from_min_max(
            self.view.max - Vec2::splat(scroll::THICKNESS),
            self.view.max,
        )
    }
}

/// The drop menu that shows: a `#mainMenu` item (index into `main_menu::ITEMS`), Help, the
/// tool selector's, or a document's context menu (at a pointer position, with the document that
/// was active before it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenMenu {
    Main(usize),
    Help,
    ToolChooser,
    DocumentContext {
        doc: usize,
        at: Pos2,
        previous: Option<u64>,
    },
    /// The Colors form's palette menu (`SwatchItem`, a `SelectorMenuItem`).
    Palette,
    /// The status bar's unit selector (`MeasurementUnitSelectorItem`, drops up).
    Units,
}

/// `DropMenuItem` / `DropMenuPopup.activePopup` / `MainMenu.openDropMenu`: one popup at a time.
#[derive(Default)]
struct Menus {
    open: Option<OpenMenu>,
    state: DropMenuState,
    /// Opened during this frame: this frame's press doesn't close it.
    opened_now: bool,
}

impl Menus {
    fn open(&mut self, ctx: &egui::Context, which: OpenMenu) {
        if self.open == Some(which) {
            return;
        }
        self.close(ctx);
        self.open = Some(which);
        self.opened_now = true;
        ctx.request_repaint();
    }

    fn close(&mut self, ctx: &egui::Context) {
        if let Some(open) = self.open.take() {
            menu::forget(ctx, owner(open));
            self.state = DropMenuState::default();
            ctx.request_repaint();
        }
    }

    /// A click on a menu's own button: opens it, or closes it when it is the open one.
    fn toggle(&mut self, ctx: &egui::Context, which: OpenMenu) {
        if self.open == Some(which) {
            self.close(ctx);
        } else {
            self.open(ctx, which);
        }
    }
}

fn owner(open: OpenMenu) -> &'static str {
    match open {
        OpenMenu::Main(i) => main_menu::ITEMS[i],
        OpenMenu::Help => "menu.help",
        OpenMenu::ToolChooser => "toolStripChooser.chooseToolButton",
        OpenMenu::DocumentContext { .. } => "document-context",
        OpenMenu::Palette => "swatch",
        OpenMenu::Units => "measurementUnit",
    }
}

/// The tool chooser's entries: the shown tools in Tools-window order (S§4.1).
pub fn tool_chooser_entries() -> Vec<menu::MenuEntry> {
    pdn_core::tool::types::ToolType::shown(true)
        .map(menu::MenuEntry::Tool)
        .collect()
}

/// A paste error (`title` and `text` are i18n keys).
// ponytail: a choice-less task dialog stands in for Paint.NET's message box.
fn message(title: &str, text: &str) {
    TaskDialog::show(TaskDialogSpec {
        message_icon: None,
        title: i18n::t(title),
        icon: Some("menu_edit_paste_icon.png"),
        message: Some(i18n::t(text)),
        choices: vec![],
        cancel_value: "close",
        field: None,
        images: None,
        error_box: false,
    });
}

#[derive(Default)]
pub struct Shell {
    pub desktop: bool,
    pub canvas_view: canvas_view::CanvasView,
    /// The floating forms (Tools, Colors, History, Layers).
    pub forms: crate::forms::Forms,
    footer: footer::Footer,
    pub tool_menu: tool_menu::ToolMenu,
    pub document_strip: document_strip::DocumentStrip,
    /// Where thumbnails are scaled (Batch Export scales its own).
    thumbnail_mode: crate::kit::thumbnails::ThumbnailMode,
    menus: Menus,
    /// The app changed a setting (`AppSettingsStore.set`): the caller persists it.
    pub settings_changed: bool,
    /// `#editor.rulers-visible`: applied by `syncRulerVisibility` when a document activates.
    rulers_class: bool,
    /// `#view.scrollLeft/Top` as last written (the DOM scroll mirror).
    dom_scroll: Option<(f64, f64)>,
    /// The Colors form's palette selector this frame (the palette menu's anchor).
    palette_selector: Option<Rect>,
    /// The status bar's unit selector this frame (the units menu's anchor).
    unit_selector: Option<Rect>,
    started: bool,
    /// The modal dialogs over the app (`PaintApp` shows them after the shell).
    pub dialogs: crate::dialogs::DialogStack,
    pub settings_dialog: crate::dialogs::settings::SettingsHandle,
    pub settings_imports: std::collections::VecDeque<crate::platform::files::PendingImport>,
    /// Raw file drops advance one import category at a time across modal navigation.
    pub drop_batches: std::collections::VecDeque<crate::platform::files::DropBatch>,
    pub drop_choice: Option<crate::platform::files::DropImageChoice>,
    /// `ErrorForm`: over the forms, under the popups.
    pub error_form: crate::forms::error::ErrorForm,
    /// The system clipboard (none in tests: the in-app clipboard answers).
    pub clipboard: crate::platform::Clipboard,
    /// From File's last successful pick, for its next dialog and Repeat (R156, BB27).
    pub file_source: crate::dialogs::effect_config::FileSource,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_paste: Option<(crate::platform::ImageReadRequest, PasteKind, Option<u64>)>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_image_paste_event: bool,
}

impl Shell {
    fn editor_rects(&self, layout: &ShellLayout, app: &AppWorkspace) -> EditorRects {
        // AppView.updateCanvasBounds: `#view` scrolls unless the document is in Fit to Window.
        let scrollbars = !app.active_ws().is_some_and(|ws| ws.view.zoom_to_window);
        EditorRects::compute(layout.editor, self.rulers_class, scrollbars)
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        shortcuts: &ShortcutRegistry,
        initial_buttons: u8,
    ) {
        let ctx = ui.ctx().clone();
        let theme = theme::current(&ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("desktop-shell"), self.desktop));
        let screen = ctx.content_rect();
        let main_menu_width = main_menu::width(ui.painter());
        let layout =
            ShellLayout::compute(screen, self.desktop, main_menu_width, app.documents.len());

        // New views fit at the real scale (the canvas zoom is in device px).
        app.pixels_per_point = ctx.pixels_per_point() as f64;
        if !self.started {
            // main.js: the initial 800x600 document, fitted to the (ruler-less) view.
            self.started = true;
            let editor = self.editor_rects(&layout, app);
            app.view_width = editor.client_size.x as f64;
            app.view_height = editor.client_size.y as f64;
            // ToolSelector's constructor: `tools.defaultTool` (else the paintbrush).
            let tool =
                pdn_core::tool::types::ToolType::from_id(settings.get_str("tools.defaultTool"))
                    .unwrap_or(pdn_core::tool::types::ToolType::PaintBrush);
            app.set_active_tool_from_type(tool);
            app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
            // FormRegistry.initialize (main.js registers the forms at startup).
            self.forms.register(settings);
        }
        // The canvas zoom is in device px: a monitor change re-centres every view before layout.
        let ppp = ctx.pixels_per_point() as f64;
        let rescaled = app.active_ws().is_some_and(|ws| ws.view.ppp != ppp);
        for ws in &mut app.documents {
            ws.view.set_pixels_per_point(ppp);
        }
        if rescaled {
            // The re-laid-out `#view` reports its whole-px scroll position, and the document
            // position under the pointer changed.
            self.dom_scroll = None;
            self.dom_scroll_feedback(app);
            if self.footer.cursor_live() {
                let (x, y) = app.last_mouse;
                self.footer.on_event(&AppEvent::MouseMove { x, y }, app);
            }
        }
        // Plugin effects read the Colors window's palette (BB common.md §5.11), taken before
        // any effect session of this frame begins.
        let palette: Vec<pdn_core::pixel::Px> = self
            .forms
            .colors
            .palette
            .iter()
            .map(|c| [c.red, c.green, c.blue, c.alpha])
            .collect();
        if app.current_palette != palette {
            app.current_palette = palette;
        }
        self.handle_events(app, &layout, settings);
        for request in std::mem::take(&mut app.ui_requests) {
            self.handle_ui_request(&ctx, app, settings, request);
        }
        self.sync_view_size(app, &layout);
        self.dom_scroll_feedback(app);
        let blocked_document_keys = ctx.text_edit_focused() || self.text_input_focused();
        // The canvas takes the pointer first: everything painted later lies over it.
        let (over_canvas, strip_capture) =
            self.canvas_input(ui, &layout, app, settings, shortcuts, initial_buttons);
        // A tool's request (the Clone Stamp's error box on a release) is handled next frame; so are
        // the Magnetic Lasso's queued steps (its `on_frame` traces a few a frame), animations or
        // not. The other tools' ticks and Content-Aware Fill keep their 100-ms poll (`lib.rs`).
        let tracing = app.tool_type == Some(pdn_core::tool::types::ToolType::MagneticLasso)
            && app.tool.as_ref().is_some_and(|t| t.needs_ticks());
        if !app.ui_requests.is_empty() || tracing {
            ctx.request_repaint();
        }

        let painter = ui.painter().clone();
        let ppp = ctx.pixels_per_point();
        // header (css/layout.css): menu background, 1 px bottom border (whole device pixels).
        let header = px::snap_rect(
            Rect::from_min_max(screen.min, Pos2::new(screen.max.x, layout.editor.min.y)),
            ppp,
        );
        painter.rect_filled(header, 0.0, theme.menu_background);
        let hb = px::border_width(HEADER_BORDER, ppp);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(header.min.x, header.max.y - hb), header.max),
            0.0,
            theme.title_bar_border,
        );
        // #titleBar: title-bar fill (its top border has the same color), menu-colored bottom border.
        let title = px::snap_rect(layout.title_bar, ppp);
        painter.rect_filled(title, 0.0, theme.title_bar);
        let tb = px::border_width(TITLE_BAR_BORDER, ppp);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(title.min.x, title.max.y - tb), title.max),
            0.0,
            theme.menu_background,
        );
        if self.desktop {
            titlebar::show(ui, app, &layout, &theme);
        }
        let main_open = match self.menus.open {
            Some(OpenMenu::Main(i)) => Some(i),
            _ => None,
        };
        let main = main_menu::show(ui, layout.main_menu, main_open);
        if let Some(id) = common_menu::show(ui, layout.common_menu, app) {
            self.run_action(&ctx, app, settings, id);
        }
        let strip = settings_menu::show(
            ui,
            layout.settings_menu,
            settings,
            self.menus.open == Some(OpenMenu::Help),
        );
        // DropMenuItem: a click opens it, or closes it again; MainMenu.onDropMenuHovered: while a
        // main menu is open, entering another item opens that one instead. Not while a button is
        // down: a tap moves and presses in one frame, and its press must close the open menu (an
        // outside press) so its click opens the tapped one, not toggle it shut again.
        if let Some(i) = main.clicked {
            self.menus.toggle(&ctx, OpenMenu::Main(i));
        } else if let (Some(i), Some(_), false) =
            (main.hovered, main_open, ui.input(|i| i.pointer.any_down()))
        {
            self.menus.open(&ctx, OpenMenu::Main(i));
        }
        match strip.clicked {
            // HelpMenu (a DropMenuItem): a click opens it.
            Some("menu.help") => self.menus.toggle(&ctx, OpenMenu::Help),
            // The aux menu: Ctrl+Shift+click on a form's toggle resets the form
            // (`floating-windows.md` §2.8).
            Some(id) if ui.input(|i| i.modifiers.ctrl && i.modifiers.shift) => {
                match action::perform(app, id) {
                    ActionOutcome::Ui(UiRequest::ToggleForm(form)) => self.forms.reset(form, app),
                    ActionOutcome::Ui(request) => {
                        self.handle_ui_request(&ctx, app, settings, request)
                    }
                    ActionOutcome::Done | ActionOutcome::NotWired(_) => {}
                }
            }
            Some(id) => self.run_action(&ctx, app, settings, id),
            None => {}
        }
        // The drop menus and the tool popups are painted later over the options row: the wheel
        // over them (they do not scroll) is not a size box's.
        let covered = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
            self.menus.state.contains(p)
                || self.tool_menu.popup_rect().is_some_and(|r| r.contains(p))
        });
        let tools = self.tool_menu.show(
            ui,
            layout.tool_menu,
            app,
            self.menus.open == Some(OpenMenu::ToolChooser),
            !covered,
        );
        // ToolSelector (a DropMenuItem): a click opens it.
        if tools.selector_clicked {
            self.menus.toggle(&ctx, OpenMenu::ToolChooser);
        }

        // Pixels a tool changed during this frame's input (a brush stroke's frame) bumped the
        // composition revision after the drain: their rects join this render, or the next frame
        // would see the revision unchanged and never show them.
        if let Some(ws) = app.active_ws_mut() {
            let view = &mut self.canvas_view;
            ws.view.events.retain(|e| match e {
                AppEvent::DocumentInvalidated { rects } => {
                    view.invalidate(rects);
                    false
                }
                _ => true,
            });
        }
        self.show_editor(ui, &layout, app, settings, over_canvas);
        let units_open = self.menus.open == Some(OpenMenu::Units);
        let footer = self.footer.show(ui, layout.footer, app, units_open);
        self.unit_selector = Some(footer.unit_selector);
        if footer.open_units {
            self.menus.toggle(&ctx, OpenMenu::Units);
        }
        let strip_action = self.document_strip.show(
            ui,
            layout.document_strip,
            app,
            &theme,
            &crate::assets::Assets,
            settings,
        );
        if let Some(action) = strip_action {
            self.strip_action(&ctx, app, settings, action);
        }
        self.show_forms(ui, &layout, app, settings);
        self.tool_menu.show_popup(ui, app, &self.canvas_view);
        {
            let canvas_view = &self.canvas_view;
            let pixels = |id| {
                crate::kit::img::texture_pixels(&ctx, id).or_else(|| canvas_view.texture_pixels(id))
            };
            self.footer.show_zoom_menu(ui, layout.footer, app, &pixels);
        }
        let anchor = match self.menus.open {
            Some(OpenMenu::Main(i)) => main.rects[i],
            Some(OpenMenu::Help) => strip.help_rect,
            Some(OpenMenu::ToolChooser) => tools.selector_rect,
            Some(OpenMenu::DocumentContext { at, .. }) => Rect::from_min_size(at, Vec2::ZERO),
            Some(OpenMenu::Palette) => self.palette_selector.unwrap_or(Rect::NOTHING),
            Some(OpenMenu::Units) => self.unit_selector.unwrap_or(Rect::NOTHING),
            None => Rect::NOTHING,
        };
        // The palette menu closes with its form.
        if self.menus.open == Some(OpenMenu::Palette) && self.palette_selector.is_none() {
            self.menus.close(&ctx);
        }
        self.show_menus(ui, app, settings, shortcuts, anchor);
        // Text/numeric widgets get first refusal. A delivered document command has no text-
        // focus veto (§9.4); modal input has already been withheld by PaintApp.
        if blocked_document_keys {
            let events = ctx.input(|i| i.events.clone());
            let final_buttons = app.mouse_buttons;
            for (index, event) in events.iter().cloned().enumerate() {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    let previous = app.active_ws().map(|w| w.document.id);
                    app.mouse_buttons =
                        canvas_view::physical_buttons_at(&events, index, initial_buttons);
                    let handled = self.document_strip.navigate(
                        app,
                        crate::js_key(key),
                        modifiers,
                        strip_capture.at(&events, index),
                    );
                    app.mouse_buttons = final_buttons;
                    if handled {
                        ctx.input_mut(|i| {
                            i.consume_key(modifiers, key);
                        });
                        if previous != app.active_ws().map(|w| w.document.id) {
                            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("canvas-input")));
                            ctx.request_repaint();
                        }
                    }
                }
            }
        }
        if over_canvas {
            canvas_view::CanvasView::paint_cursor(&ctx, app);
        }
        // Events raised after this frame's drain (the scroll feedback, scrollbar drags) have
        // listeners (e.g. the zoom label): run them next frame rather than on the next input.
        if app.has_events() {
            ctx.request_repaint();
        }
    }

    /// The open drop menu (painted over everything) and the document click listener.
    fn show_menus(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        shortcuts: &ShortcutRegistry,
        anchor: Rect,
    ) {
        let ctx = ui.ctx().clone();
        let mut pick = None;
        if let Some(open) = self.menus.open {
            let entries = match open {
                OpenMenu::Main(i) => main_menu::entries(i, app),
                OpenMenu::Help => settings_menu::help_entries(),
                OpenMenu::ToolChooser => tool_chooser_entries(),
                OpenMenu::DocumentContext { doc, .. } => {
                    document_strip::context_menu_entries(app, doc)
                }
                OpenMenu::Palette => self.forms.colors.palette_menu_entries(),
                // MeasurementUnitSelectorItem: `DropEntry`s with the plural names, the selection
                // showing `tool_strip_checked` (an icon, not a checked entry).
                OpenMenu::Units => pdn_core::app::MeasurementUnit::ALL
                    .iter()
                    .map(|&u| {
                        let id = pdn_core::action::get(&format!("measurementUnit.{}", u.id()))
                            .expect("unit action")
                            .id;
                        let selected = u == self.footer.selected_unit();
                        menu::MenuEntry::Command {
                            id,
                            label: pdn_core::i18n::t(&format!("{id}.plural")),
                            icon: if selected {
                                "tool_strip_checked.png"
                            } else {
                                ""
                            },
                            enabled: true,
                        }
                    })
                    .collect(),
            };
            let menu = DropMenu {
                owner: owner(open),
                // MainMenu items and Help are DropMenuItems with "menu." ids.
                style: match open {
                    OpenMenu::ToolChooser => MenuStyle::ToolChooser,
                    OpenMenu::DocumentContext { .. } => MenuStyle::DocumentContext,
                    OpenMenu::Palette => MenuStyle::Palette,
                    OpenMenu::Units => MenuStyle::Plain,
                    _ => MenuStyle::Command,
                },
                anchor,
                entries,
                // DropMenuItem.isDropUpAt: the anchor reaches below the window; the unit
                // selector always drops up.
                drop_up: open == OpenMenu::Units || anchor.max.y > ctx.content_rect().max.y,
            };
            let canvas_view = &self.canvas_view;
            let pixels = |id| {
                crate::kit::img::texture_pixels(&ctx, id).or_else(|| canvas_view.texture_pixels(id))
            };
            let cx = MenuContext {
                app,
                shortcuts,
                pixels: &pixels,
            };
            let before = self.menus.state;
            pick = menu::show_drop_menu(ui, &menu, &mut self.menus.state, &cx);
            if self.menus.state != before {
                ctx.request_repaint();
            }
        }
        // A press (any button) off the menu, its submenu and its own button closes it; the title
        // bar's press too, which the window manager's move grab never turns into a click.
        let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()));
        let outside = press
            .flatten()
            .is_some_and(|p| !self.menus.state.contains(p) && !anchor.contains(p));
        // Escape closes every drop menu, its submenu with it.
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if let Some(OpenMenu::DocumentContext { doc, previous, .. }) = self.menus.open {
            // DocumentContextMenu: an entry closes the menu and runs; a `pointerdown` outside it
            // or Escape closes it.
            let state = self.menus.state;
            if let Some(MenuPick::Action(id)) = pick {
                self.menus.close(&ctx);
                self.document_context_pick(&ctx, app, settings, doc, previous, id);
            } else if escape || press.flatten().is_some_and(|p| !state.contains(p)) {
                self.menus.close(&ctx);
            }
            self.menus.opened_now = false;
            return;
        }
        match pick {
            // DropEntry.onPress: close the owner, then run the command.
            Some(MenuPick::Action(id)) if self.menus.open == Some(OpenMenu::Palette) => {
                self.menus.close(&ctx);
                #[cfg(target_arch = "wasm32")]
                if id == "colors.palette.openFolder" {
                    crate::platform::files::pick_palette_folder(
                        app,
                        settings,
                        &mut self.settings_changed,
                    );
                }
                self.forms.colors.palette_pick(id);
            }
            Some(MenuPick::Action(id)) => {
                let chooser = self.menus.open == Some(OpenMenu::ToolChooser);
                self.menus.close(&ctx);
                match pdn_core::tool::types::ToolType::from_id(id) {
                    // ToolSelector's entries: `setSelectedId` -> `setActiveToolFromType`, with or
                    // without a document (not `runPerformAction`).
                    Some(tool) if chooser => app.set_active_tool_from_type(tool),
                    _ => self.run_action(&ctx, app, settings, id),
                }
            }
            Some(MenuPick::Submenu) => {}
            None if escape || (outside && !self.menus.opened_now) => self.menus.close(&ctx),
            None => {}
        }
        self.menus.opened_now = false;
    }

    /// The document strip's request (DocumentStrip's ScrollList callbacks, DocumentItem's close
    /// button, `createDocumentItem`'s context-menu callback).
    fn strip_action(
        &mut self,
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        action: document_strip::StripAction,
    ) {
        use document_strip::StripAction;
        match action {
            StripAction::Activate(i) => {
                app.picked.clear();
                if app.active != Some(i) {
                    app.set_active_document_workspace(Some(i));
                }
            }
            StripAction::Pick { i, range } => {
                let id = |i: usize| app.documents[i].id;
                match (range, app.active) {
                    (true, Some(a)) => {
                        let (lo, hi) = (a.min(i), a.max(i));
                        app.picked = (lo..=hi).filter(|&k| k != a).map(id).collect();
                    }
                    _ if app.active == Some(i) => {}
                    _ => {
                        let id = id(i);
                        match app.picked.iter().position(|&p| p == id) {
                            Some(k) => {
                                app.picked.remove(k);
                            }
                            None => app.picked.push(id),
                        }
                    }
                }
            }
            StripAction::Close(i) => {
                crate::dialogs::flows::close_workspace(
                    app,
                    settings,
                    &mut self.settings_changed,
                    i,
                );
            }
            StripAction::Reorder { from, to } => {
                // itemSwapper: the workspaces swap places; the active one stays active.
                app.documents.swap(from, to);
                app.active = app.active.map(|a| match a {
                    a if a == from => to,
                    a if a == to => from,
                    a => a,
                });
            }
            StripAction::ContextMenu(i, at) => {
                let previous = app.active_ws().map(|ws| ws.id);
                if app.active != Some(i) {
                    app.set_active_document_workspace(Some(i));
                }
                self.menus.open(
                    ctx,
                    OpenMenu::DocumentContext {
                        doc: i,
                        at,
                        previous,
                    },
                );
            }
        }
    }

    /// A DocumentContextMenu entry for document `doc`: Save and Save As run for it
    /// (`runForWorkspace`); Close re-activates the previously active document.
    fn document_context_pick(
        &mut self,
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        doc: usize,
        previous: Option<u64>,
        id: &str,
    ) {
        if doc >= app.documents.len() {
            return;
        }
        match id {
            "menu.tab.copyPath" => {
                if let Some(path) = app.documents[doc].file_path.clone() {
                    ctx.copy_text(path);
                }
            }
            "menu.file.save" | "menu.file.saveAs" => {
                if app.active != Some(doc) {
                    app.set_active_document_workspace(Some(doc));
                }
                self.run_action(ctx, app, settings, id);
            }
            "menu.file.close" => {
                crate::dialogs::flows::close_then(
                    app,
                    settings,
                    &mut self.settings_changed,
                    doc,
                    previous,
                );
            }
            // `window.desktopFileActions.showItemInFolder`: the desktop build's (Task 56).
            _ => {}
        }
    }

    /// `runPerformAction` from a menu, button or key; the UI side of its outcome.
    fn run_action(
        &mut self,
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        id: &str,
    ) {
        match action::perform(app, id) {
            ActionOutcome::Ui(request) => self.handle_ui_request(ctx, app, settings, request),
            // The task that ports the feature wires the id.
            ActionOutcome::Done | ActionOutcome::NotWired(_) => {}
        }
    }

    fn handle_ui_request(
        &mut self,
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        request: UiRequest,
    ) {
        match request {
            UiRequest::OpenSettings(section) => self
                .settings_dialog
                .open(&mut self.dialogs, section.as_deref()),
            // ToggleFormItem / the Window menu's toggles.
            UiRequest::ToggleForm(form) => self.forms.toggle(form, app),
            UiRequest::ToggleColorAdd => self.forms.colors.toggle_add_mode(),
            UiRequest::HelpLink(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            // `window.close()`.
            UiRequest::Exit => {
                if self.desktop {
                    window_close::request(ctx, window_close::Intent::ExitProcess);
                } else {
                    crate::dialogs::flows::exit(app, settings, &mut self.settings_changed, ctx);
                }
            }
            UiRequest::OpenFile => crate::platform::files::pick_images(
                false,
                app,
                settings,
                &mut self.settings_changed,
            ),
            UiRequest::ImportFromFile => {
                crate::platform::files::pick_images(true, app, settings, &mut self.settings_changed)
            }
            UiRequest::Print => {
                crate::platform::files::print_active(app, settings, &mut self.settings_changed)
            }
            UiRequest::Save => {
                crate::dialogs::flows::save_active(
                    app,
                    settings,
                    &mut self.settings_changed,
                    false,
                );
            }
            UiRequest::SaveAs => {
                crate::dialogs::flows::save_active(app, settings, &mut self.settings_changed, true);
            }
            UiRequest::SaveAll => {
                crate::dialogs::flows::save_all(app, settings, &mut self.settings_changed);
            }
            UiRequest::BatchExport => {
                let ids = self.document_strip.batch_export_targets(app);
                if !ids.is_empty() {
                    let mode = self.thumbnail_mode;
                    let dialog =
                        crate::dialogs::batch_export::BatchExportDialog::new(app, &ids, mode);
                    self.dialogs.push(Box::new(dialog));
                }
            }
            UiRequest::Close => {
                if let Some(i) = app.active {
                    crate::dialogs::flows::close_workspace(
                        app,
                        settings,
                        &mut self.settings_changed,
                        i,
                    );
                }
            }
            // HelpMenu's "About" entry: `AboutDialog.open()`.
            UiRequest::About => self
                .dialogs
                .push(Box::new(crate::dialogs::about::AboutDialog::new(false))),
            UiRequest::ToggleHelpMenu => {
                if self.menus.open == Some(OpenMenu::Help) {
                    self.menus.close(ctx);
                } else {
                    self.menus.open(ctx, OpenMenu::Help);
                }
            }
            // Task 42: an effect session over the active layer, configured in its dialog.
            UiRequest::EffectDialog(id) => {
                let session = pdn_core::effect::find(&id)
                    .and_then(|def| pdn_core::effect::session::EffectSession::begin(app, def));
                if let Some(s) = session {
                    let values = s.initial_values(app);
                    self.dialogs
                        .push(crate::dialogs::effect_dialog(s, values, |dialog| {
                            dialog.attach_file_source(self.file_source.clone());
                            dialog.acquire_external(app, ctx, Some(&mut self.clipboard));
                        }));
                }
            }
            UiRequest::RepeatSeam | UiRequest::RepeatExternal | UiRequest::RepeatFonts => {
                // action::perform has already committed the active tool, as for EffectDialog.
                if let Some(def) = app.last_effect.as_deref().and_then(pdn_core::effect::find) {
                    let values = app.last_effect_values.clone().unwrap_or_default();
                    if let Some(session) = pdn_core::effect::session::EffectSession::begin(app, def)
                    {
                        let mut dialog =
                            crate::dialogs::effect_config::EffectConfigDialog::final_only(
                                session, values,
                            );
                        dialog.attach_file_source(self.file_source.clone());
                        dialog.acquire_external(app, ctx, Some(&mut self.clipboard));
                        self.dialogs.push(Box::new(dialog));
                    }
                }
            }
            // Task 44: File ▸ New (the active image's size, 800×600 without one), Image ▸
            // Resize / Canvas Size and Layer Properties.
            UiRequest::NewFileDialog => {
                let (w, h) = app
                    .active_ws()
                    .map_or((800, 600), |ws| (ws.document.width, ws.document.height));
                let image = if self.clipboard.is_system() {
                    self.clipboard.read_image().ok().flatten()
                } else {
                    app.clipboard
                        .item
                        .as_ref()
                        .map(|i| i.masked.as_ref().clone())
                };
                let (w, h) = image.map_or((w, h), |i| (i.width, i.height));
                self.dialogs
                    .push(Box::new(crate::dialogs::new_file::NewFileDialog::new(w, h)));
            }
            UiRequest::ResizeDialog | UiRequest::CanvasSizeDialog => {
                use crate::dialogs::image_size::{ImageSizeDialog, Mode};
                let Some(ws) = app.active_ws() else {
                    return;
                };
                let d = &ws.document;
                let mode = if request == UiRequest::ResizeDialog {
                    Mode::Resize
                } else {
                    Mode::Canvas
                };
                self.dialogs.push(Box::new(ImageSizeDialog::new(
                    mode,
                    (d.width, d.height, d.resolution),
                    d.layers.size(),
                )));
            }
            UiRequest::FadeDialog => self
                .dialogs
                .push(Box::new(crate::dialogs::fade::FadeDialog::default())),
            UiRequest::LayerPropertiesDialog => {
                if let Some(d) = crate::dialogs::layer_properties::LayerPropertiesDialog::new(app) {
                    self.dialogs.push(Box::new(d));
                }
            }
            // Task 28 (selection-and-move.md §2.11): the system clipboard gets the masked copy, or
            // the selection's JSON text.
            UiRequest::ClipboardImage => {
                if let Some(item) = &app.clipboard.item {
                    self.clipboard.write_image(&item.masked);
                }
            }
            UiRequest::ClipboardSelection => {
                if let Some(path) = &app.internal_selection_path {
                    let json = pdn_core::io::clipboard::selection_to_json(path);
                    self.clipboard.write_text(&json);
                }
            }
            UiRequest::Paste(kind) => {
                #[cfg(target_arch = "wasm32")]
                if self.clipboard.web_images {
                    self.start_web_paste(ctx, app, kind, None);
                    return;
                }
                self.paste(app, kind);
            }
            // Task 31 (painting-tools-1.md §3.10 step 2): CloneStampTool.js's
            // `showMissingAnchorWarning`, titled with the product name.
            UiRequest::CloneStampAnchorWarning => {
                // `white-space: pre-line`: each line's spaces collapse, the breaks stay.
                let text = i18n::t("cloneStampTool.error.triedToDrawWithoutAnchor.text");
                let lines: Vec<&str> = text.lines().map(str::trim).collect();
                TaskDialog::show(TaskDialogSpec {
                    message_icon: None,
                    title: crate::dialogs::about::PRODUCT.into(),
                    icon: None,
                    message: Some(lines.join("\n")),
                    choices: vec![TaskChoice {
                        value: "ok",
                        title: "OK".into(),
                        description: None,
                        icon: None,
                    }],
                    cancel_value: "ok",
                    field: None,
                    images: None,
                    error_box: true,
                });
            }
            // S§4.3: a tool's refusal (Remove over its pixel cap), in the same error box.
            UiRequest::ToolError(text) => {
                TaskDialog::show(TaskDialogSpec {
                    message_icon: None,
                    title: crate::dialogs::about::PRODUCT.into(),
                    icon: None,
                    message: Some(text),
                    choices: vec![TaskChoice {
                        value: "ok",
                        title: "OK".into(),
                        description: None,
                        icon: None,
                    }],
                    cancel_value: "ok",
                    field: None,
                    images: None,
                    error_box: true,
                });
            }
            // S§4.5: a pixel edit of a text layer asked first. OK rasterizes (its own history
            // item) and runs the command that asked again; Cancel drops the edit.
            UiRequest::RasterizeText { layer, then } => {
                if !pdn_core::document::text_layer::is_text(app, layer) {
                    return;
                }
                let choice = |value, title: &str| TaskChoice {
                    value,
                    title: title.into(),
                    ..Default::default()
                };
                let spec = TaskDialogSpec {
                    title: i18n::t("rasterizeTextLayerQuestion.title"),
                    message: Some(i18n::t("rasterizeTextLayerQuestion.text")),
                    choices: vec![choice("ok", "OK"), choice("cancel", "Cancel")],
                    cancel_value: "cancel",
                    ..Default::default()
                };
                TaskDialog::show_then(spec, move |value, cx| {
                    if value == "ok" {
                        pdn_core::document::text_layer::confirm(cx.app, layer, then);
                    }
                });
            }
            // Dialogs, files and the tool selector arrive with their tasks.
            _ => {}
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn start_web_paste(
        &mut self,
        ctx: &egui::Context,
        app: &AppWorkspace,
        kind: PasteKind,
        blob: Option<web_sys::Blob>,
    ) {
        let request = match blob {
            Some(blob) => self.clipboard.request_image_blob(ctx, blob),
            None if matches!(kind, PasteKind::Selection(_)) => {
                self.clipboard.request_selection(ctx)
            }
            None => self.clipboard.request_image(ctx),
        };
        self.web_paste = Some((request, kind, app.active_ws().map(|w| w.id)));
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn poll_web_paste(&mut self, ctx: &egui::Context, app: &mut AppWorkspace) {
        use crate::platform::ImageRead;
        self.clipboard.clear_completed_image_write();
        let Some((mut request, kind, document)) = self.web_paste.take() else {
            return;
        };
        if self.dialogs.is_open()
            || crate::platform::files::busy()
            || document != app.active_ws().map(|w| w.id)
        {
            return;
        }
        let selection_text = request.take_selection_text();
        match request.poll() {
            ImageRead::Pending => self.web_paste = Some((request, kind, document)),
            ImageRead::Canceled => {}
            ImageRead::Ready(Err(_)) => self.paste(app, kind), // denied browser access retains the internal fallback
            ImageRead::Ready(Ok(image)) => {
                let mut clipboard = crate::platform::Clipboard::in_memory();
                if let Some(text) = selection_text {
                    clipboard.write_text(&text);
                } else if let Some(image) = image {
                    clipboard.write_image(&image);
                }
                std::mem::swap(&mut self.clipboard, &mut clipboard);
                self.paste(app, kind);
                std::mem::swap(&mut self.clipboard, &mut clipboard);
                ctx.request_repaint();
            }
        }
    }

    /// `DocumentIO.pasteFromClipboard` / `pasteIntoNewLayer` / `pasteIntoNewImage` /
    /// `pasteSelectionOutline` (selection-and-move.md §2.11 steps 2–6). A system clipboard's
    /// answer is final (no image is an error, never the in-app item); without one (tests, the
    /// web) the in-app clipboard answers. A paste larger than the canvas asks Expand / Keep /
    /// Cancel; Paste into New Layer adds the layer first, and Cancel (or a failed paste) undoes it.
    fn paste(&mut self, app: &mut AppWorkspace, kind: PasteKind) {
        if kind == PasteKind::Paste
            && let Some(text) = self.clipboard.read_text()
            && app
                .with_tool(|tool, app| {
                    tool.as_any_mut()
                        .downcast_mut::<pdn_core::tool::text::TextTool>()
                        .is_some_and(|t| t.paste(app, &text))
                })
                .unwrap_or(false)
        {
            return;
        }
        if kind == PasteKind::Paste {
            app.commit_tool_first(|_| ());
        }
        let system = self.clipboard.is_system();
        if let PasteKind::Selection(mode) = kind {
            let geometry = if system {
                match self
                    .clipboard
                    .read_text()
                    .and_then(|t| clipboard::selection_from_json(&t))
                {
                    Some(g) => Ok(Some(g)),
                    None => self
                        .clipboard
                        .read_image()
                        .map(|image| Some(app.clipboard.read(Some(image?))?.geometry.clone())),
                }
            } else {
                Ok(app
                    .internal_selection_path
                    .clone()
                    .or_else(|| app.clipboard.item.as_ref().map(|i| i.geometry.clone())))
            };
            match geometry {
                Ok(Some(g)) => clipboard::paste_selection(app, g, mode),
                Ok(None) => message(
                    "menu.edit.pasteSelection.text",
                    "pasteSelectionAction.error.noSelection",
                ),
                Err(e) => {
                    eprintln!("paint.rs: reading the clipboard failed: {e}");
                    message(
                        "menu.edit.pasteSelection.text",
                        "pasteSelectionAction.error.transferFromClipboard",
                    );
                }
            }
            return;
        }
        let item = if system {
            match self.clipboard.read_image() {
                Ok(image) => image.and_then(|i| app.clipboard.read(Some(i)).cloned()),
                Err(e) => {
                    eprintln!("paint.rs: reading the clipboard image failed: {e}");
                    let title = match kind {
                        PasteKind::IntoNewImage => "menu.edit.pasteInToNewImage.text",
                        _ => "expandCanvasQuestion.title",
                    };
                    message(title, "pasteAction.error.transferFromClipboard");
                    return;
                }
            }
        } else {
            app.clipboard.item.clone()
        };
        let Some(item) = item else {
            match kind {
                PasteKind::IntoNewImage => message(
                    "menu.edit.pasteInToNewImage.text",
                    "pasteInToNewImageAction.error.noClipboardImage",
                ),
                _ => message("expandCanvasQuestion.title", "pasteAction.error.noImage"),
            }
            return;
        };
        if kind == PasteKind::IntoNewImage {
            clipboard::paste_into_new_image(app, &item);
            return;
        }
        let into_layer = kind == PasteKind::IntoNewLayer;
        if into_layer && !clipboard::add_paste_layer(app) {
            return;
        }
        let too_large = app.active_ws().is_some_and(|ws| {
            let doc = (ws.document.width, ws.document.height);
            clipboard::needs_expand(doc, item.paste_bounds())
        });
        // `None` = Cancel.
        let run = move |app: &mut AppWorkspace, expand: Option<PasteExpand>| {
            let floating = expand.is_some_and(|e| {
                clipboard::paste(app, &item, e) == clipboard::PasteOutcome::Floating
            });
            if into_layer && !floating {
                app.undo();
            }
        };
        if !too_large {
            run(app, Some(PasteExpand::Keep));
            return;
        }
        let choice = |value, key: &str, icon| TaskChoice {
            value,
            title: i18n::t(&format!("expandCanvasQuestion.{key}.actionText")),
            description: Some(i18n::t(&format!(
                "expandCanvasQuestion.{key}.explanationText"
            ))),
            icon: Some(icon),
        };
        let spec = TaskDialogSpec {
            message_icon: None,
            title: i18n::t("expandCanvasQuestion.title"),
            icon: Some("menu_edit_paste_icon.png"),
            message: Some(i18n::t("expandCanvasQuestion.introText")),
            choices: vec![
                choice("expand", "yesTB", "menu_image_canvas_size_icon.png"),
                choice("keep", "noTB", "menu_edit_paste_icon.png"),
                choice("cancel", "cancelTB", "menu_edit_undo_icon.png"),
            ],
            cancel_value: "cancel",
            field: None,
            images: None,
            error_box: false,
        };
        // ponytail: no preview thumbnail of the pasted image (paintdotjs's `.task-dialog-preview`)
        // until a scenario covers this dialog.
        TaskDialog::show_then(spec, move |value, cx| match value {
            "expand" => run(cx.app, Some(PasteExpand::Expand)),
            "keep" => run(cx.app, Some(PasteExpand::Keep)),
            _ => run(cx.app, None),
        });
    }

    /// `over_canvas`: the canvas owns the pointer this frame (over its visible part, or captured).
    fn show_editor(
        &mut self,
        ui: &mut Ui,
        layout: &ShellLayout,
        app: &mut AppWorkspace,
        settings: &AppSettings,
        over_canvas: bool,
    ) {
        let theme = theme::current(ui.ctx());
        let rects = self.editor_rects(layout, app);
        // html { background-color: var(--color-background) } shows through #editor.
        ui.painter()
            .rect_filled(layout.editor, 0.0, theme.background);
        // The rulers cover the rest of the canvas.
        let visible = rects.canvas.intersect(rects.client);
        let text_owner = !self.dialogs.is_open()
            && !self.text_input_focused()
            && !self
                .tool_menu
                .font_picker_owns(app, egui::Key::A, true, egui::Modifiers::NONE);
        self.canvas_view
            .show(ui, rects.canvas, visible, app, settings, text_owner);
        if rects.scrollbars {
            self.show_scrollbars(ui, &rects, app);
        }
        if self.rulers_class {
            // The mouse marker follows the pointer the status bar shows (the canvas's last
            // pointer sample) while the canvas owns the pointer.
            let origin = px::snap_rect(rects.canvas, ui.ctx().pixels_per_point()).min;
            let (x, y) = app.last_mouse;
            let pointer = over_canvas.then(|| origin + Vec2::new(x as f32, y as f32));
            self.canvas_view.show_rulers(
                ui,
                layout.editor,
                rects.client_size,
                rects.canvas,
                pointer,
                app,
            );
        }
    }

    fn show_scrollbars(&mut self, ui: &mut Ui, rects: &EditorRects, app: &mut AppWorkspace) {
        let theme = theme::current(ui.ctx());
        let (content, mut offset) = match app.active_ws() {
            Some(ws) => (
                ws.view.environment_size(),
                self.dom_scroll.unwrap_or_default(),
            ),
            None => ((0.0, 0.0), (0.0, 0.0)),
        };
        let (cw, ch) = (rects.client_size.x as f64, rects.client_size.y as f64);
        let v = Scrollbar::vertical(rects.vertical_bar()).show(ui, content.1, ch, &mut offset.1);
        let h =
            Scrollbar::horizontal(rects.horizontal_bar()).show(ui, content.0, cw, &mut offset.0);
        ui.painter()
            .rect_filled(rects.scroll_corner(), 0.0, theme.scrollbar_background);
        if (v || h)
            && let Some(ws) = app.active_ws_mut()
        {
            // The user moved `scrollLeft/Top` (whole px); the scroll listener feeds it back.
            let dom = (
                pdn_core::js::math_round(offset.0),
                pdn_core::js::math_round(offset.1),
            );
            self.dom_scroll = Some(dom);
            ws.view.set_view_position(dom.0, dom.1);
        }
    }

    /// Drains the engine's event queue (JS `app.on(...)` listeners).
    fn handle_events(
        &mut self,
        app: &mut AppWorkspace,
        layout: &ShellLayout,
        settings: &mut AppSettings,
    ) {
        for event in app.drain_events() {
            match &event {
                // setActiveDocumentWorkspace / setRulersVisible -> syncRulerVisibility ->
                // updateCanvasBounds(false)
                AppEvent::UpdateActiveDocument | AppEvent::RulersVisibilityChanged => {
                    self.rulers_class = app.rulers_visible;
                    let rects = self.editor_rects(layout, app);
                    let (w, h) = (rects.client_size.x as f64, rects.client_size.y as f64);
                    app.view_width = w;
                    app.view_height = h;
                    if let Some(ws) = app.active_ws_mut() {
                        ws.view.update_canvas_bounds(w, h, false);
                    }
                    if matches!(event, AppEvent::RulersVisibilityChanged) {
                        self.store(settings, "workspace.showRulers", app.rulers_visible.into());
                    }
                }
                AppEvent::GridVisibilityChanged => {
                    self.store(settings, "workspace.showPixelGrid", app.grid_visible.into());
                }
                AppEvent::UpdateMeasurementUnit => {
                    let unit = app.measurement_unit.id();
                    self.store(settings, "workspace.measurementUnit", unit.into());
                    if unit != "pixel" {
                        self.store(settings, "workspace.lastNonPixelUnit", unit.into());
                    }
                }
                AppEvent::ToggleColorSlot => {
                    let c = &mut self.forms.colors;
                    c.select_secondary(!c.secondary_selected());
                }
                AppEvent::DocumentInvalidated { rects } => self.canvas_view.invalidate(rects),
                // ToolMenu: `app:active_tool_updated` -> renderOptions.
                AppEvent::ActiveToolUpdated => self.tool_menu.render_options(),
                _ => {}
            }
            self.footer.on_event(&event, app);
        }
    }

    /// `AppSettingsStore.set`: the caller writes the settings back to storage.
    fn store(&mut self, settings: &mut AppSettings, path: &str, value: serde_json::Value) {
        settings.set(path, value);
        self.settings_changed = true;
    }

    /// `window resize` -> AppView.onResize: keep the active view at the `#view` client size.
    /// (A rulers toggle resizes without the shift instead: `handle_events` runs
    /// `syncRulerVisibility`'s `updateCanvasBounds(false)` first.)
    fn sync_view_size(&mut self, app: &mut AppWorkspace, layout: &ShellLayout) {
        let rects = self.editor_rects(layout, app);
        let (w, h) = (rects.client_size.x as f64, rects.client_size.y as f64);
        app.view_width = w;
        app.view_height = h;
        if let Some(ws) = app.active_ws_mut()
            && (ws.view.view_width, ws.view.view_height) != (w, h)
        {
            ws.view.on_resize(w, h);
        }
    }

    /// The DOM half of `AppView.setViewPosition` + the async `#view` scroll listener: the
    /// browser holds a clamped, whole-pixel `scrollLeft/Top`; when that value changes, the
    /// listener calls `documentWorkspace.setViewPosition(scrollLeft, scrollTop)` (not in Fit to
    /// Window). Run after the frame's events so listeners saw the pre-scroll viewport, as the
    /// footer's cursor position does in the reference (-275, not -274).
    fn dom_scroll_feedback(&mut self, app: &mut AppWorkspace) {
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        if ws.view.has_native_text_scroll() {
            self.dom_scroll = Some((ws.view.viewport_x, ws.view.viewport_y));
            return;
        }
        let dom = ws.view.dom_scroll_position();
        if self.dom_scroll == Some(dom) {
            return;
        }
        self.dom_scroll = Some(dom);
        if !ws.view.zoom_to_window {
            ws.view.set_view_position(dom.0, dom.1);
        }
    }
}

impl Shell {
    /// The floating forms over the editor and the strip (`#windowOverlay`), and what they ask.
    fn show_forms(
        &mut self,
        ui: &mut Ui,
        layout: &ShellLayout,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
    ) {
        let ctx = ui.ctx().clone();
        let rects = self.editor_rects(layout, app);
        let out = {
            let canvas_view = &self.canvas_view;
            let pixels = |id| {
                crate::kit::img::texture_pixels(&ctx, id).or_else(|| canvas_view.texture_pixels(id))
            };
            let popups: Vec<Rect> = [
                self.menus.state.popup.filter(|_| self.menus.open.is_some()),
                self.tool_menu.popup_rect(),
            ]
            .into_iter()
            .flatten()
            .collect();
            let overlays = crate::forms::overlay_rects(
                app,
                px::snap_rect(rects.canvas, ctx.pixels_per_point()).min,
                ctx.pixels_per_point(),
                canvas_view.draft_view(),
            );
            let input = crate::forms::FormsInput {
                view: rects.view,
                client: rects.client_size,
                image: crate::forms::image_rect(app, layout.editor.min),
                overlays: &overlays,
                // The rulers cover the rest of the canvas (`show_editor`).
                viewport: rects.canvas.intersect(rects.client),
                pixels: &pixels,
                palette_open: self.menus.open == Some(OpenMenu::Palette),
                popups: &popups,
            };
            let mut out = self.forms.show_all(ui, app, settings, input);
            // The error form, a `WebWindow` created after the forms, over them.
            let others: Vec<Rect> = self.forms.windows.iter().map(|w| w.rect()).collect();
            let documents = app.documents.len();
            let error = self
                .error_form
                .show(ui, input, &others, settings, documents);
            out.actions.extend(error);
            out
        };
        self.settings_changed |= out.settings_changed;
        self.palette_selector = out.palette_selector;
        // SwatchItem (a DropMenuItem): a click opens it.
        if out.palette_clicked {
            self.forms.colors.reload_file_palettes();
            self.menus.toggle(&ctx, OpenMenu::Palette);
        }
        if let Some(tool) = out.tool {
            // ToolForm: `toolStripChooser.setSelectedId`, with or without a document.
            app.set_active_tool_from_type(tool);
        }
        for id in out.actions {
            self.run_action(&ctx, app, settings, id);
        }
    }

    /// Whether a drop or context menu is open.
    pub fn menu_open(&self) -> bool {
        self.menus.open.is_some()
    }

    /// The status bar's zoom field has the keyboard: shortcuts wait (AppView skips INPUTs).
    pub fn text_input_focused(&self) -> bool {
        self.footer.owns_keys()
    }

    /// AppView's canvas listeners on the visible canvas (`canvas_view::CanvasView::input`).
    fn canvas_input(
        &mut self,
        ui: &mut Ui,
        layout: &ShellLayout,
        app: &mut AppWorkspace,
        settings: &AppSettings,
        shortcuts: &ShortcutRegistry,
        initial_buttons: u8,
    ) -> (bool, document_strip::CaptureSnapshot) {
        let capture = self.document_strip.capture_snapshot(ui, app);
        let events = ui.input(|i| i.events.clone());
        let rects = self.editor_rects(layout, app);
        let ppp = ui.ctx().pixels_per_point();
        let at = canvas_view::CanvasInput {
            // The rulers cover the rest of the canvas (`show_editor`).
            visible: rects
                .canvas
                .intersect(rects.client)
                .intersect(if self.desktop {
                    ui.ctx().content_rect().shrink(RESIZE_BORDER)
                } else {
                    Rect::EVERYTHING
                }),
            origin: px::snap_rect(rects.canvas, ppp).min,
            view: rects.view,
            auto_scroll: settings.get_bool("ui.autoScrollWhileDrawing"),
        };
        let blocked = ui.ctx().text_edit_focused() || self.text_input_focused();
        // Escape with a menu open closes the menu (`show_menus`), and the tool never sees it.
        let menu_open = self.menus.open.is_some();
        let tool_menu = &self.tool_menu;
        let colors = &mut self.forms.colors;
        let strip = &mut self.document_strip;
        let clipboard = &mut self.clipboard;
        let browser_paste = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Paste(s) if !s.is_empty()))
        });
        let key_ctx = ui.ctx().clone();
        let over = self
            .canvas_view
            .input(ui, at, app, initial_buttons, |index, event, app| {
                if blocked
                    || menu_open
                        && matches!(
                            event,
                            egui::Event::Key {
                                key: egui::Key::Escape,
                                ..
                            }
                        )
                {
                    return;
                }
                if let egui::Event::Key { modifiers: m, .. } = event {
                    let m = crate::pdn_modifiers(*m);
                    app.set_modifier_keys(m.ctrl, m.shift, m.alt);
                }
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers: m,
                    ..
                } = event
                {
                    let name = crate::js_key(*key);
                    let printable = name.chars().count() == 1 || *key == egui::Key::Space;
                    if tool_menu.font_picker_owns(app, *key, printable, *m) {
                        return;
                    }
                    let previous = app.active_ws().map(|w| w.document.id);
                    crate::dispatch_canvas_key(
                        app,
                        shortcuts,
                        colors,
                        strip,
                        name,
                        *m,
                        capture.at(&events, index),
                    );
                    // Text the Text tool copied or cut from its selection (Ctrl+C / Ctrl+X).
                    let copied = app.with_tool(|tool, _| {
                        tool.as_any_mut()
                            .downcast_mut::<pdn_core::tool::text::TextTool>()
                            .and_then(|t| t.take_copied())
                    });
                    if let Some(text) = copied.flatten() {
                        clipboard.write_text(&text);
                    }
                    if previous != app.active_ws().map(|w| w.document.id) {
                        key_ctx.memory_mut(|memory| {
                            memory.request_focus(egui::Id::unique("canvas-input"))
                        });
                    }
                    if matches!(
                        app.ui_requests.last(),
                        Some(UiRequest::Paste(PasteKind::Paste))
                    ) {
                        let text = if browser_paste {
                            None
                        } else {
                            clipboard.read_text()
                        };
                        let handled = app
                            .with_tool(|tool, app| {
                                tool.as_any_mut()
                                    .downcast_mut::<pdn_core::tool::text::TextTool>()
                                    .is_some_and(|t| {
                                        t.is_editing()
                                            && (browser_paste
                                                || text.as_deref().is_some_and(|s| t.paste(app, s)))
                                    })
                            })
                            .unwrap_or(false);
                        if handled {
                            app.ui_requests.pop();
                        }
                    }
                } else {
                    if tool_menu.font_picker_owns(app, egui::Key::A, true, egui::Modifiers::NONE) {
                        return;
                    }
                    app.with_tool(|tool, app| {
                        let Some(t) = tool
                            .as_any_mut()
                            .downcast_mut::<pdn_core::tool::text::TextTool>()
                        else {
                            return;
                        };
                        match event {
                            egui::Event::Text(s) => t.insert_text(app, s),
                            egui::Event::Paste(s) => {
                                t.paste(app, s);
                            }
                            egui::Event::Ime(egui::ImeEvent::Preedit {
                                text,
                                active_range_chars,
                            }) => t.ime_preedit_range(app, text, active_range_chars.clone()),
                            egui::Event::Ime(egui::ImeEvent::Commit(s)) => t.ime_commit(app, s),
                            egui::Event::Ime(egui::ImeEvent::DeleteSurrounding {
                                before_chars,
                                after_chars,
                            }) => t.ime_delete_surrounding(app, *before_chars, *after_chars),
                            _ => {}
                        }
                    });
                }
            });
        (over, capture)
    }

    /// Where the document strip, the Layers form and Batch Export scale thumbnails.
    pub fn set_thumbnail_mode(&mut self, mode: crate::kit::thumbnails::ThumbnailMode) {
        self.thumbnail_mode = mode;
        self.document_strip.set_thumbnail_mode(mode);
        self.forms.set_thumbnail_mode(mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_layout_reserves_native_title_band() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 830.0));
        let web = ShellLayout::compute(screen, false, 360.26563, 1);
        let desktop = ShellLayout::compute(screen, true, 360.26563, 1);
        assert_eq!(desktop.main_menu.min.y, web.main_menu.min.y + 30.0);
        assert_eq!(desktop.editor.min.y, web.editor.min.y + 30.0);
        assert_eq!(desktop.document_strip.min.y, web.document_strip.min.y);
        assert_eq!(
            desktop.document_strip.height(),
            web.document_strip.height() + 30.0
        );
    }

    #[test]
    fn layout_matches_reference_dom() {
        // shell-no-forms getBoundingClientRect dump (1280x800, one document, UI-scale and Task 19b
        // header patches).
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let l = ShellLayout::compute(screen, false, 360.26563, 1);
        let r = |r: Rect| [r.min.x, r.min.y, r.width(), r.height()];
        assert_eq!(r(l.title_bar), [0.0, 0.0, 1280.0, 30.0]);
        assert_eq!(r(l.main_menu), [3.0, 2.0, 360.26563, 27.0]);
        assert_eq!(r(l.settings_menu), [1125.0, 3.0, 153.0, 24.0]);
        assert_eq!(r(l.common_menu), [3.0, 31.0, 330.0, 26.0]);
        assert_eq!(r(l.tool_menu), [2.0, 57.0, 1276.0, 27.0]);
        assert_eq!(r(l.editor), [0.0, 85.0, 1280.0, 683.0]);
        assert_eq!(r(l.footer), [0.0, 768.0, 1280.0, 32.0]);
        let ds = r(l.document_strip);
        assert!((ds[0] - 387.26563).abs() < 1e-3 && ds[1..] == [2.0, 75.0, 54.0]);
        // updatePreferredWidth: no documents, no strip; many give way to the settings menu.
        let none = ShellLayout::compute(screen, false, 360.26563, 0);
        assert_eq!(none.document_strip.width(), 0.0);
        let many = ShellLayout::compute(screen, false, 360.26563, 20);
        assert_eq!(many.document_strip.max.x, many.settings_menu.min.x);
        let e = EditorRects::compute(l.editor, false, true);
        assert_eq!(e.client_size, Vec2::new(1265.0, 668.0));
        let e = EditorRects::compute(l.editor, true, true);
        assert_eq!(e.client_size, Vec2::new(1248.0, 651.0));
        assert_eq!(e.canvas.min, Pos2::new(0.0, 85.0));
        assert_eq!(r(e.horizontal_bar()), [17.0, 753.0, 1248.0, 15.0]);
        assert_eq!(r(e.vertical_bar()), [1265.0, 102.0, 15.0, 651.0]);
    }

    /// `ui-text-and-glyphs.md §7.3` (menu row 28 DIP; rnd(s) gaps; toolbar row 25.6 m@125 = 32 px;
    /// tool-options row 26.4 m@125 = 33 px) and §7.4 (the strip from the menu row's top to 1 px
    /// above the toolbar row's bottom, clearing the tool-options row), §8.5 (thumbnail max edge =
    /// item − 2·rnd(4s) − 2), as the painters snap them: device px at 100, 125 and 150 %.
    #[test]
    fn header_rows_are_paint_net_rows_at_each_scale() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let l = ShellLayout::compute(screen, false, 360.26563, 1);
        // (ppp, [menu row, gap, toolbar row, gap, tool-options row], item, clearance, thumb max)
        let cases = [
            (1.0, [28, 1, 26, 1, 26], 54, 2, 44),
            (1.25, [35, 1, 32, 1, 33], 67, 2, 55),
            (1.5, [42, 2, 39, 1, 39], 81, 3, 67),
        ];
        for (ppp, rows, item, clearance, thumb) in cases {
            let px = |v: f32| (v * ppp).round() as i32;
            let tab_top = px(l.main_menu.min.y);
            let title_bottom = px(l.title_bar.max.y);
            let common = px::snap_rect(l.common_menu, ppp);
            let tool = tool_menu::padding_box(l.tool_menu, ppp);
            let device = |v: f32| (v * ppp).round() as i32;
            let got = [
                title_bottom - tab_top,
                device(common.min.y) - title_bottom,
                device(common.height()),
                device(tool.min.y) - device(common.max.y),
                device(tool.height()),
            ];
            assert_eq!(got, rows, "rows at {ppp}");
            let strip = px::snap_rect(l.document_strip, ppp);
            assert_eq!(device(strip.min.y), tab_top, "strip top at {ppp}");
            assert_eq!(device(strip.height()), item, "item at {ppp}");
            assert_eq!(device(tool.min.y) - device(strip.max.y), clearance, "{ppp}");
            assert_eq!(document_strip::thumb_max_edge(l.document_strip, ppp), thumb);
        }
    }

    /// S§4.5: a command on a text layer asks "Rasterize text layer?". Cancel keeps the text layer
    /// and adds no history; OK rasterizes as its own item and runs the command again.
    #[test]
    fn rasterize_prompt_cancel_keeps_and_ok_runs_the_command() {
        use egui_kittest::kittest::Queryable;
        use pdn_core::document::text_layer::{RASTERIZE_NAME, TextSpec, is_text};
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| {
                crate::PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled)
            });
        h.run();
        let app = &mut h.state_mut().app;
        let settings = pdn_core::tool::text::TextSettings::from_tool_settings(
            &app.tool_settings,
            pdn_core::fonts::FontPolicy::Bundled,
        );
        let spec = TextSpec {
            text: "T".into(),
            settings,
            origin: (10.0, 10.0),
            color: [0, 0, 0, 255],
            dpi: 96.0,
        };
        let ws = app.active_ws_mut().unwrap();
        ws.document.layers.get_at_mut(0).unwrap().text = Some(spec);
        let items = ws.history.undo.len();
        let names = |h: &egui_kittest::Harness<'_, crate::PaintApp>| -> Vec<String> {
            let ws = h.state().app.active_ws().unwrap();
            ws.history.undo[items..]
                .iter()
                .map(|m| m.name.clone())
                .collect()
        };
        for (button, text_kept) in [("Cancel", true), ("OK", false)] {
            let app = &mut h.state_mut().app;
            assert_eq!(
                action::perform(app, "adjustment.invertColorsEffect"),
                ActionOutcome::Done
            );
            // The thumbnails keep frames coming for a while: a fixed number of frames.
            h.run_steps(4);
            assert!(h.state().shell.dialogs.is_open(), "{button}");
            h.get_by_label(button).click();
            h.run_steps(4);
            assert!(!h.state().shell.dialogs.is_open(), "{button}");
            assert_eq!(is_text(&h.state().app, 0), text_kept, "{button}");
        }
        assert_eq!(names(&h), [RASTERIZE_NAME, "Invert Colors"]);
    }
}

#[cfg(test)]
mod bb26_tests {
    use super::*;
    use crate::platform::TestImageReads;
    use pdn_core::testing::{Ev, drive_tool, fresh_doc, layer_bytes};
    use pdn_core::tool::{MouseButton, ToolType};
    /// R155: action commit-first survives pending external Repeat Cancel through the actual shell handler.
    #[test]
    fn bb26_repeat_actual_shell_starts_read_in_invocation_and_cancel_keeps_tool_commit() {
        let probe = TestImageReads::install(true);
        let mut app = fresh_doc(10, 10, None);
        let def = pdn_core::effect::find("bbPasteAlpha").unwrap();
        let values = pdn_core::effect::defaults(&def);
        app.last_effect = Some(def.id.into());
        app.last_effect_values = Some(values.clone());
        drive_tool(
            &mut app,
            ToolType::MagicWand,
            &[
                Ev::Down(1., 1., MouseButton::Left),
                Ev::Up(1., 1., MouseButton::Left),
            ],
        );
        let prior = app.active_ws().unwrap().history.undo.len();
        let ActionOutcome::Ui(request) = action::perform(&mut app, "menu.effects.repeat") else {
            panic!()
        };
        assert_eq!(request, UiRequest::RepeatExternal);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), prior + 1);
        let source = layer_bytes(&app, None);
        let tokens = app.effect_tokens.clone();
        let dirty = app.active_ws().unwrap().dirty;
        let mut shell = Shell {
            clipboard: crate::platform::Clipboard::in_memory(),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut settings = AppSettings::load(None);
        shell.handle_ui_request(&ctx, &mut app, &mut settings, request);
        assert_eq!(probe.reads.get(), 1, "read starts before any painted frame");
        assert!(shell.dialogs.is_open());
        let mut prefs = crate::prefs::Prefs::default();
        for escape in [false, true] {
            let events = if escape {
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                vec![]
            };
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    shell.dialogs.show(
                        ui,
                        &mut crate::dialogs::DialogCx {
                            app: &mut app,
                            settings: &mut settings,
                            prefs: &mut prefs,
                            settings_changed: false,
                            shortcuts: None,
                            clipboard: Some(&mut shell.clipboard),
                            settings_imports: None,
                            fonts: None,
                            asset_library: None,
                            seat_focus: false,
                        },
                    );
                },
            );
            out.textures_delta.clear();
        }
        assert!(!shell.dialogs.is_open());
        probe.completions.borrow()[0].complete(Ok(Some(pdn_core::canvas::ImageData {
            width: 1,
            height: 1,
            data: vec![255; 4],
        })));
        assert_eq!(layer_bytes(&app, None), source);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), prior + 1);
        assert_eq!(app.active_ws().unwrap().dirty, dirty);
        assert_eq!(app.effect_tokens, tokens);
        assert_eq!(app.last_effect.as_deref(), Some("bbPasteAlpha"));
        assert_eq!(app.last_effect_values, Some(values));
        assert_eq!(probe.reads.get(), 1);
        TestImageReads::clear();
    }
}

#[cfg(test)]
mod bb27_tests {
    use super::*;
    use crate::dialogs::effect_config::FileAttachment;
    use crate::platform::{Files, PickedFile};
    use pdn_core::testing::{fresh_doc, layer_bytes};
    /// R156: actual Repeat snapshots Shell's committed attachment and acquires it in the
    /// invocation, without a chooser or a typed read; a parent Cancel keeps the attachment.
    #[test]
    fn bb27_repeat_actual_shell_uses_committed_attachment() {
        let files = Files::in_memory();
        files.clone().install();
        let mut app = fresh_doc(8, 8, None);
        let def = pdn_core::effect::find("bbFillFromFile").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("file".into(), serde_json::json!("pick.png"));
        app.last_effect = Some(def.id.into());
        app.last_effect_values = Some(values);
        let png = pdn_core::io::encode::encode(
            &pdn_core::canvas::ImageData {
                width: 1,
                height: 1,
                data: vec![255, 0, 0, 255],
            },
            "png",
            &pdn_core::io::encode::SaveOptions::default_for("png"),
        )
        .unwrap();
        let pick = PickedFile::snapshot("pick.png".into(), png);
        let mut shell = Shell::default();
        *shell.file_source.borrow_mut() = Some(FileAttachment {
            value: "pick.png".into(),
            file: pick.clone(),
        });
        let ActionOutcome::Ui(request) = action::perform(&mut app, "menu.effects.repeat") else {
            panic!()
        };
        assert_eq!(request, UiRequest::RepeatExternal);
        let before = (layer_bytes(&app, None), app.effect_tokens.clone());
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut settings = AppSettings::load(None);
        shell.handle_ui_request(&ctx, &mut app, &mut settings, request);
        assert!(shell.dialogs.is_open());
        assert_eq!(*files.probe.log.borrow(), ["decode"]);
        assert_eq!(std::rc::Rc::strong_count(&shell.file_source), 2);
        let mut prefs = crate::prefs::Prefs::default();
        for events in [
            vec![],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    shell.dialogs.show(
                        ui,
                        &mut crate::dialogs::DialogCx {
                            app: &mut app,
                            settings: &mut settings,
                            prefs: &mut prefs,
                            settings_changed: false,
                            shortcuts: None,
                            clipboard: Some(&mut shell.clipboard),
                            settings_imports: None,
                            fonts: None,
                            asset_library: None,
                            seat_focus: false,
                        },
                    );
                },
            );
            out.textures_delta.clear();
        }
        assert!(!shell.dialogs.is_open());
        assert_eq!((layer_bytes(&app, None), app.effect_tokens.clone()), before);
        assert_eq!(
            shell.file_source.borrow().as_ref().map(|a| a.file.id),
            Some(pick.id)
        );
        assert_eq!(std::rc::Rc::strong_count(&shell.file_source), 1);
        assert!(!files.probe.log.borrow().contains(&"choose"));
    }
}

#[cfg(test)]
mod bb29_tests {
    use super::*;
    use crate::platform::fonts::{FontManager, FontStore};
    use pdn_core::testing::{fresh_doc, layer_bytes};

    /// R157: a cold-face Meme Repeat routes to the final-only owner through the actual shell
    /// handler; it resolves this invocation's face (Bundled: Anton after the failed Impact
    /// attempt) and commits exactly one history entry with the remembered values.
    #[test]
    fn bb29_cold_face_repeat_commits_through_the_final_only_owner() {
        let _impact = crate::dialogs::effect_config::bb29_tests::IMPACT
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut app = fresh_doc(60, 40, None);
        let def = pdn_core::effect::find("bbMemeMaker").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.remove("face");
        values.remove("tab");
        values.insert("top".into(), serde_json::json!("A\r\nB"));
        app.last_effect = Some(def.id.into());
        app.last_effect_values = Some(values.clone());
        let ActionOutcome::Ui(request) = action::perform(&mut app, "menu.effects.repeat") else {
            panic!("a cold face cannot repeat synchronously")
        };
        assert_eq!(request, UiRequest::RepeatFonts);
        let (prior, source) = (
            app.active_ws().unwrap().history.undo.len(),
            layer_bytes(&app, None),
        );
        let mut shell = Shell::default();
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let mut settings = AppSettings::load(None);
        shell.handle_ui_request(&ctx, &mut app, &mut settings, request);
        assert!(shell.dialogs.is_open());
        let mut fonts = FontManager::new(FontStore::memory(), pdn_core::fonts::FontPolicy::Bundled);
        let mut prefs = crate::prefs::Prefs::default();
        for _ in 0..3000 {
            fonts.poll();
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(1280., 800.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    shell.dialogs.show(
                        ui,
                        &mut crate::dialogs::DialogCx {
                            app: &mut app,
                            settings: &mut settings,
                            prefs: &mut prefs,
                            settings_changed: false,
                            shortcuts: None,
                            clipboard: None,
                            settings_imports: None,
                            fonts: Some(&mut fonts),
                            asset_library: None,
                            seat_focus: false,
                        },
                    );
                },
            );
            out.textures_delta.clear();
            if !shell.dialogs.is_open() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!shell.dialogs.is_open());
        assert_eq!(app.active_ws().unwrap().history.undo.len(), prior + 1);
        assert_ne!(layer_bytes(&app, None), source);
        assert_eq!(app.last_effect_values, Some(values.clone()));
        assert_eq!(app.effect_tokens["bbMemeMaker"], values);
    }
}

/// R248: with a selection, the arrows reach Move Selected Pixels and Move Selection through the
/// canvas's key path after the tool is picked from the Tools window.
#[cfg(test)]
mod arrow_nudge_tests {
    use egui::{Key, Modifiers};
    use egui_kittest::Harness;
    use pdn_core::action::FormId;
    use pdn_core::pixel::{IntRect, Px};
    use pdn_core::selection::CombineMode;
    use pdn_core::tool::ToolType;
    use pdn_core::tool::move_tools::NUDGE_COMMIT_MS;
    use pdn_core::util::geom::Rectangle;
    use std::ops::Range;

    const RED: Px = [255, 0, 0, 255];
    const WHITE: Px = [255; 4];
    const CLEAR: Px = [0; 4];

    /// The app on its white image with a selected 4×4 red square at (10, 10), `tool` picked by a
    /// click on its Tools window button (Rectangle Select active before).
    fn picked(tool: ToolType) -> Harness<'static, crate::PaintApp> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_max_steps(16)
            .build_eframe(|cc| {
                let mut app =
                    crate::PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
                app.clock = crate::Clock::Virtual(0.0);
                app
            });
        h.run_steps(2);
        let app = &mut h.state_mut().app;
        let ws = app.active_ws_mut().unwrap();
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        layer.surface.fill_rect(IntRect::new(10, 10, 4, 4), RED);
        ws.selection
            .set_continuation(&Rectangle::new(10.0, 10.0, 4.0, 4.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        ws.invalidate(None);
        app.set_active_tool_from_type(ToolType::RectangleSelect);
        h.run_steps(2);
        let window = h.state().shell.forms.window(FormId::Tools).rect();
        let at = crate::forms::tools::button_rect(window, tool).center();
        h.hover_at(at);
        h.run_steps(1);
        h.drag_at(at);
        h.run_steps(1);
        h.drop_at(at);
        h.run_steps(2);
        assert_eq!(h.state().app.tool_type, Some(tool), "the button click");
        h
    }

    fn arrow(h: &mut Harness<'_, crate::PaintApp>, m: Modifiers) {
        h.key_press_modifiers(m, Key::ArrowRight);
        h.run_steps(2);
    }

    /// Past the burst's idle commit.
    fn idle(h: &mut Harness<'_, crate::PaintApp>) {
        h.state_mut().advance_clock(NUDGE_COMMIT_MS as u32 + 1);
        h.run_steps(2);
    }

    fn row(h: &Harness<'_, crate::PaintApp>) -> Vec<Px> {
        let ws = h.state().app.active_ws().unwrap();
        let s = &ws.document.layers.get_at(0).unwrap().surface;
        (0..40).map(|x| s.pixel(x, 11)).collect()
    }

    /// Row 11's pixels: red at `red`, cleared at `clear`, white elsewhere.
    fn expect(red: Range<u32>, clear: Range<u32>) -> Vec<Px> {
        (0..40)
            .map(|x| match x {
                _ if red.contains(&x) => RED,
                _ if clear.contains(&x) => CLEAR,
                _ => WHITE,
            })
            .collect()
    }

    fn selection_x(h: &Harness<'_, crate::PaintApp>) -> f64 {
        let ws = h.state().app.active_ws().unwrap();
        ws.selection.get_bounds().x
    }

    fn undo_len(h: &Harness<'_, crate::PaintApp>) -> usize {
        h.state().app.active_ws().unwrap().history.undo.len()
    }

    /// An arrow moves the pixels 1 px; Ctrl+arrow 10 px.
    #[test]
    fn move_selected_pixels_nudges_1_and_10_px() {
        let mut h = picked(ToolType::Move);
        arrow(&mut h, Modifiers::NONE);
        assert_eq!(row(&h), expect(11..15, 10..11));
        arrow(&mut h, Modifiers::CTRL);
        assert_eq!(row(&h), expect(21..25, 10..14));
    }

    /// The outline moves 1 px, then 10 px with Ctrl; the pixels stay.
    #[test]
    fn move_selection_nudges_the_outline_1_and_10_px() {
        let mut h = picked(ToolType::MoveSelection);
        let before = row(&h);
        arrow(&mut h, Modifiers::NONE);
        idle(&mut h);
        assert_eq!(selection_x(&h), 11.0);
        arrow(&mut h, Modifiers::CTRL);
        idle(&mut h);
        assert_eq!(selection_x(&h), 21.0);
        assert_eq!(row(&h), before);
    }

    /// R227, R253: Shift+arrows stamp the pixels where the burst started and where each step
    /// lands, so the start keeps its copy. Shift let go between the taps does not split the burst.
    #[test]
    fn shift_arrows_leave_the_trail_in_the_pixels() {
        let mut h = picked(ToolType::Move);
        let before = undo_len(&h);
        for _ in 0..3 {
            arrow(&mut h, Modifiers::SHIFT);
        }
        idle(&mut h);
        assert_eq!(row(&h), expect(10..17, 0..0));
        assert_eq!(undo_len(&h), before + 1);
    }

    /// ArrowRight held for `steps` key-downs: the press, then the auto-repeats, then the release.
    fn hold(h: &mut Harness<'_, crate::PaintApp>, steps: usize) {
        for i in 0..steps {
            h.event(egui::Event::Key {
                key: Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: i > 0,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.key_up(Key::ArrowRight);
        h.run_steps(1);
    }

    /// A held arrow repeats, 1 px a key-down; the burst is one history entry, committed once the
    /// keys go idle, and the next burst another.
    #[test]
    fn a_held_arrow_repeats_and_its_burst_is_one_history_entry() {
        for tool in [ToolType::Move, ToolType::MoveSelection] {
            let mut h = picked(tool);
            let before = undo_len(&h);
            hold(&mut h, 3);
            idle(&mut h);
            assert_eq!(undo_len(&h), before + 1, "{tool:?}");
            assert_eq!(selection_x(&h), 13.0, "{tool:?}");
            if tool == ToolType::Move {
                assert_eq!(row(&h), expect(13..17, 10..13));
            }
            hold(&mut h, 1);
            idle(&mut h);
            assert_eq!(undo_len(&h), before + 2, "{tool:?}");
            assert_eq!(selection_x(&h), 14.0, "{tool:?}");
        }
    }

    /// Right held while Down repeats (only the last key pressed repeats): each key-down moves by
    /// both, so the burst goes diagonally, as one history entry.
    #[test]
    fn two_held_arrows_nudge_diagonally() {
        for tool in [ToolType::Move, ToolType::MoveSelection] {
            let mut h = picked(tool);
            let before = undo_len(&h);
            let down = |h: &mut Harness<'_, crate::PaintApp>, key, repeat| {
                h.event(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat,
                    modifiers: Modifiers::NONE,
                });
                h.run_steps(1);
            };
            down(&mut h, Key::ArrowRight, false);
            down(&mut h, Key::ArrowDown, false);
            down(&mut h, Key::ArrowDown, true);
            h.key_up(Key::ArrowDown);
            h.key_up(Key::ArrowRight);
            h.run_steps(1);
            idle(&mut h);
            let b = h.state().app.active_ws().unwrap().selection.get_bounds();
            assert_eq!((b.x, b.y), (13.0, 12.0), "{tool:?}");
            assert_eq!(undo_len(&h), before + 1, "{tool:?}");
        }
    }
}
