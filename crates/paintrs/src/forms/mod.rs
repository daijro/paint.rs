//! The floating forms: Tools, Colors, History and Layers, each in a `FloatingWindow` over the
//! editor, painted and hit-tested in Paint.NET's z-order (the last shown on top; a press in a form
//! raises it), translucent by Paint.NET's condition, reset one at a time with Ctrl+Shift+F5…F8.
//! Port of: js/src/interface/window/{Form,FormRegistry}.js, WebWindow.js's focus tracking
//! Reference: paintdotjs @ b4e6fc5
//! Implements Paint.NET's z-order, default descriptions, reset and translucency condition.
pub mod colors;
pub mod error;
pub mod history;
pub mod layers;
pub mod tools;

use egui::{Key, Pos2, Rect, Ui, Vec2};
use pdn_core::app::{AppEvent, AppWorkspace};
use pdn_core::settings::AppSettings;
use pdn_core::tool::types::ToolType;

use crate::kit::color::ColorLook;
use crate::kit::shadow::TexturePixels;
use crate::kit::thumbnails::ThumbnailMode;
use crate::kit::window::{FloatingWindow, Snap};
use crate::shell::canvas_view::DisplayPrefs;
use crate::theme::*;

pub use pdn_core::action::FormId;

/// Paint.NET's snap-manager order (`floating-windows.md` §2.3: Tools, History, Layers, Colors),
/// which is also its show order.
const ORDER: [FormId; 4] = [
    FormId::Tools,
    FormId::History,
    FormId::Layers,
    FormId::Colors,
];

/// The default corner of each form (§2.4 step 1; Ctrl+Shift+F5…F8 park there, §2.8).
fn default_snap(form: FormId) -> Snap {
    match form {
        FormId::Tools => Snap::corner(false, false),
        FormId::History => Snap::corner(true, false),
        FormId::Layers => Snap::corner(true, true),
        FormId::Colors => Snap::corner(false, true),
    }
}

/// `ui-drawn-graphics.md` Addendum: with translucent windows on, a form is translucent unless it
/// is forced opaque — its box misses the visible document rect (the image clipped to the
/// viewport; `doc` None = no document, which never forces it) — or the pointer is over it or a
/// press is held in it. Focus plays no part.
pub(crate) fn translucent(
    setting: bool,
    window: Rect,
    doc: Option<Rect>,
    hovered: bool,
    pressed: bool,
) -> bool {
    setting && !hovered && !pressed && doc.is_none_or(|d| window.intersect(d).is_positive())
}

pub struct Forms {
    /// In `ORDER`.
    pub windows: [FloatingWindow; 4],
    pub colors: colors::ColorsForm,
    pub history: history::HistoryForm,
    pub layers: layers::LayerForm,
    /// The z-order, bottom to top (indices into `windows`). Session-only, as in Paint.NET.
    // calibrate: B-W2 — the startup order (last shown on top: Colors, Layers, History, Tools from
    // the top) and the raise rule are inferred from Win32's owned-window rules (§2.2, OQ1).
    stack: [usize; 4],
    /// The window a held press began in (§2.2: it was raised).
    pressed: Option<usize>,
    /// `FormRegistry.register` ran (the saved states are restored on the first frame).
    registered: bool,
    /// Created, opened, closed or reset since the last save.
    toggled: [bool; 4],
}

impl Default for Forms {
    fn default() -> Self {
        let v = |(w, h): (f32, f32)| Vec2::new(w, h);
        let window = |form: FormId, size, min: Option<(f32, f32)>| {
            let snap = Some(default_snap(form));
            let mut w = FloatingWindow::new(form.id(), v(size), snap, min.map(v));
            w.title = pdn_core::i18n::t(&format!("{}.text", form.id()));
            w
        };
        Self {
            windows: [
                // `show_all` sizes it to the tools shown.
                window(
                    FormId::Tools,
                    tool_form_size(ToolType::shown(false).count()),
                    None,
                ),
                window(FormId::History, HISTORY_FORM_SIZE, Some(HISTORY_FORM_MIN)),
                window(FormId::Layers, LAYER_FORM_SIZE, Some(LAYER_FORM_MIN)),
                window(FormId::Colors, COLORS_FORM_SIZE, None),
            ],
            colors: Default::default(),
            history: Default::default(),
            layers: Default::default(),
            stack: [0, 1, 2, 3],
            pressed: None,
            registered: false,
            toggled: [false; 4],
        }
    }
}

/// Where the forms are this frame and what they show over.
#[derive(Clone, Copy)]
pub struct FormsInput<'a> {
    /// `#view`'s box and client size: W (`floating-windows.md` §2.3).
    pub view: Rect,
    pub client: Vec2,
    /// The active image's screen rect (`updateImageOverlap`).
    pub image: Option<Rect>,
    /// Where the canvas overlays draw (`overlay_rects`).
    pub overlays: &'a [Rect],
    /// The canvas viewport: the visible document rect is the image clipped to it.
    pub viewport: Rect,
    /// The CPU pixels of textures under the window shadows.
    pub pixels: TexturePixels<'a>,
    /// The Colors form's palette menu is showing (its selector's `.open` look).
    pub palette_open: bool,
    /// The open popups' boxes (last frame's): a press on one is in no window.
    pub popups: &'a [Rect],
}

/// What the forms ask of the shell this frame.
#[derive(Default)]
pub struct FormsOutput {
    /// Actions to run (`runPerformAction`).
    pub actions: Vec<&'static str>,
    /// A Tools form button (`toolStripChooser.setSelectedId`).
    pub tool: Option<ToolType>,
    /// `windows.<id>` changed (the caller persists the settings).
    pub settings_changed: bool,
    /// The Colors form's palette selector (its drop menu's anchor) while the form shows.
    pub palette_selector: Option<Rect>,
    /// The palette selector was clicked.
    pub palette_clicked: bool,
}

fn index(form: FormId) -> usize {
    ORDER
        .iter()
        .position(|&f| f == form)
        .expect("every form is listed")
}

impl Forms {
    /// Native reload keeps form geometry, stacking, palette state and documents intact.
    pub fn refresh_titles(&mut self) {
        for window in &mut self.windows {
            window.title = pdn_core::i18n::t(&format!("{}.text", window.id));
        }
    }
    pub fn window(&self, form: FormId) -> &FloatingWindow {
        &self.windows[index(form)]
    }

    /// The visible forms bottom to top.
    pub fn z_order(&self) -> Vec<FormId> {
        let stack = self.stack.iter();
        stack
            .filter(|&&i| self.windows[i].visible)
            .map(|&i| ORDER[i])
            .collect()
    }

    /// The topmost visible form whose hit rect holds `p`, unless an open popup covers it.
    fn top_at(&self, p: Pos2, popups: &[Rect]) -> Option<usize> {
        if popups.iter().any(|r| r.contains(p)) {
            return None;
        }
        let w = &self.windows;
        self.stack
            .iter()
            .rev()
            .copied()
            .find(|&i| w[i].visible && w[i].hit_rect().contains(p))
    }

    /// §2.2: activation (a press, or showing a hidden form) brings a form to the top.
    fn raise(&mut self, i: usize) {
        let at = self.stack.iter().position(|&j| j == i).expect("stacked");
        self.stack[at..].rotate_left(1);
    }

    /// Where thumbnails are scaled.
    pub fn set_thumbnail_mode(&mut self, mode: ThumbnailMode) {
        self.layers.set_thumbnail_mode(mode);
    }

    /// `FormRegistry.register` for each form: the saved `windows.<id>` state, then the open ones
    /// are created (shown) in `ORDER`, so the stack starts in it.
    pub fn register(&mut self, settings: &AppSettings) {
        self.registered = true;
        for w in &mut self.windows {
            w.visible = w.restore(&settings.values["windows"][w.id]);
        }
        // ColorsForm: `expanded` from the saved state, then `updateWindowSize`.
        let colors = &settings.values["windows"][FormId::Colors.id()];
        self.colors.expanded = colors["expanded"].as_bool() == Some(true);
        self.windows[index(FormId::Colors)].size = self.colors.window_size();
        for (i, w) in self.windows.iter().enumerate() {
            // `create()` saves the state.
            self.toggled[i] = w.visible;
        }
    }

    /// `saveState` of window `i` into `windows.<id>` (merged into what is stored; paintdotjs's
    /// anchors give way to the description, `floating-windows.md` §5.4).
    fn persist(&self, i: usize, settings: &mut AppSettings) -> bool {
        let w = &self.windows[i];
        let path = format!("windows.{}", w.id);
        let mut merged = settings.get(&path).clone();
        if !merged.is_object() {
            merged = serde_json::json!({});
        }
        let state = w.state();
        let object = merged.as_object_mut().expect("an object");
        object.remove("anchorX");
        object.remove("anchorY");
        for (k, v) in state.as_object().expect("an object") {
            object.insert(k.clone(), v.clone());
        }
        if *settings.get(&path) == merged {
            return false;
        }
        settings.set(&path, merged);
        true
    }

    /// ToggleFormItem: closes an open form, shows a closed one on top (§2.2); then
    /// `app:window_open_state_changed`. `show_all` saves the state.
    pub fn toggle(&mut self, form: FormId, app: &mut AppWorkspace) {
        let i = index(form);
        let open = !self.windows[i].visible;
        self.windows[i].visible = open;
        self.toggled[i] = true;
        if open {
            self.raise(i);
        }
        app.fire(AppEvent::WindowOpenStateChanged {
            form: form.id().to_string(),
        });
    }

    /// The aux menu's per-form reset (`floating-windows.md` §2.8, §5.5; Ctrl+Shift+F5…F8 or
    /// Ctrl+Shift+click on the form's toggle): History and Layers back to their default
    /// (= minimum) size, the default corner, shown. A visible form keeps its place in the stack;
    /// a hidden one is shown on top (§2.2).
    pub fn reset(&mut self, form: FormId, app: &mut AppWorkspace) {
        let i = index(form);
        let w = &mut self.windows[i];
        if let Some(min) = w.resizable {
            w.size = min;
        }
        w.park(default_snap(form));
        self.toggled[i] = true;
        if !self.windows[i].visible {
            self.toggle(form, app);
        }
    }

    /// Shows the open forms in `ui`'s layer, bottom to top, and applies their input.
    pub fn show_all(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        settings: &mut AppSettings,
        input: FormsInput,
    ) -> FormsOutput {
        if !self.registered {
            self.register(settings);
        }
        let mut out = FormsOutput::default();
        let ctx = ui.ctx().clone();
        let overlay = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        // W: `#view`'s client area (`floating-windows.md` §2.3, §5.2).
        let area = Rect::from_min_size(input.view.min, input.client);
        let now = ui.input(|i| i.time);
        // The aux menu's hot keys (§2.8), unless a text field has the keyboard (AppView's rule).
        if !ctx.text_edit_focused() {
            let keys = [
                (Key::F5, FormId::Tools),
                (Key::F6, FormId::History),
                (Key::F7, FormId::Layers),
                (Key::F8, FormId::Colors),
            ];
            for (key, form) in keys {
                let reset = ui.input(|i| {
                    i.events.iter().any(|e| {
                        matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
                            if *k == key && m.ctrl && m.shift && !m.alt)
                    })
                });
                if reset {
                    self.reset(form, app);
                }
            }
        }
        // §2.2: a press in a form raises it (popups and the canvas change nothing); it stays
        // "pressed" (opaque) while held.
        let (press, down, pointer) = ui.input(|i| {
            let p = &i.pointer;
            let press = p.any_pressed().then(|| p.interact_pos()).flatten();
            (press, p.any_down(), p.hover_pos())
        });
        if let Some(p) = press {
            self.pressed = self.top_at(p, input.popups);
            if let Some(i) = self.pressed {
                self.raise(i);
            }
        } else if !down {
            self.pressed = None;
        }
        let hovered = pointer.and_then(|p| self.top_at(p, input.popups));
        // S§4.1: the Tools form fits the tools it shows (a saved size never applies).
        let tools = ToolType::shown(true).count();
        self.windows[index(FormId::Tools)].size = tool_form_size(tools).into();
        let mut placed = false;
        for w in &mut self.windows {
            placed |= w.place(area, overlay, ppp);
        }
        let setting = settings.get_bool("ui.translucentWindows");
        let doc = input.image.map(|img| img.intersect(input.viewport));
        let display = DisplayPrefs::new(settings, &crate::theme::current(&ctx));
        let auto_select = settings.get_bool("ui.autoSelectVisibleLayer");
        if !self.windows[index(FormId::Layers)].visible {
            self.layers.release(&ctx);
        }
        for i in self.stack {
            if !self.windows[i].visible {
                continue;
            }
            let others: Vec<Rect> = (0..self.windows.len())
                .filter(|&j| j != i)
                .map(|j| self.windows[j].rect())
                .collect();
            let (windows, colors, history, layers) = (
                &mut self.windows,
                &mut self.colors,
                &mut self.history,
                &mut self.layers,
            );
            let w = &mut windows[i];
            (w.image, w.overlays) = (input.image, input.overlays.to_vec());
            let (over, held) = (hovered == Some(i), self.pressed == Some(i));
            w.translucent = translucent(setting, w.rect(), doc, over, held);
            let form = ORDER[i];
            let mut expand = false;
            let response = w.show(ui, area, overlay, &others, input.pixels, |ui| match form {
                FormId::Tools => out.tool = tools::show(ui, app),
                FormId::History => out.actions.extend(history.show(ui, app, now, display.mode)),
                FormId::Layers => {
                    let action = layers.show(ui, app, display, auto_select, now, input.pixels);
                    out.actions.extend(action)
                }
                FormId::Colors => {
                    let look = ColorLook {
                        mode: display.mode,
                        checker_brightness: display.checker_brightness,
                        dark: display.dark,
                        disabled: app.documents.is_empty(),
                    };
                    let o = colors.show(ui, app, look, input.palette_open);
                    out.palette_selector = Some(o.palette_selector);
                    out.palette_clicked = o.palette_clicked;
                    expand = o.toggle_expanded;
                }
            });
            if expand {
                // `setExpanded`: the saved state gains `expanded`; `updateWindowSize` resizes and
                // the next `place` re-parks (bottom-left: it grows up and right, §2.7).
                self.colors.expanded = !self.colors.expanded;
                let path = format!("windows.{}", FormId::Colors.id());
                let mut state = settings.get(&path).clone();
                if !state.is_object() {
                    state = serde_json::json!({});
                }
                state["expanded"] = self.colors.expanded.into();
                settings.set(&path, state);
                self.windows[i].size = self.colors.window_size();
                out.settings_changed = true;
                ctx.request_repaint();
            }
            if response.closed {
                app.fire(AppEvent::WindowOpenStateChanged {
                    form: form.id().to_string(),
                });
                out.settings_changed |= self.persist(i, settings);
            }
        }
        // `saveState`: `create`/`close`/reset (toggled), the document `mouseup` listeners (a drag
        // or resize ends) and a re-park after W changed.
        let released = ui.input(|i| i.pointer.any_released());
        for i in 0..self.windows.len() {
            let toggled = std::mem::take(&mut self.toggled[i]);
            if toggled || (self.windows[i].visible && (released || placed)) {
                out.settings_changed |= self.persist(i, settings);
            }
        }
        out
    }
}

/// The screen rects where the active document's canvas overlays actually draw (ruling R67): the
/// selection outline's device spans, one device px wider (its whole layer rect when it has too
/// many), each tool handle's box (which already holds its anti-aliasing) and the brush outline's and
/// picker square's boxes. `canvas_min` is the canvas box's device-snapped top-left.
pub fn overlay_rects(app: &AppWorkspace, canvas_min: Pos2, ppp: f32) -> Vec<Rect> {
    use pdn_core::render::{Renderer, nubs};
    let Some(ws) = app.active_ws() else {
        return vec![];
    };
    let t = ws.view.view_transform();
    ws.surface_box
        .renderers
        .iter()
        .flat_map(|(_, r)| match r {
            // Each span grown by one device px: the reference's (paintdotjs's) anti-aliased
            // outline lies up to a pixel off Paint.NET's (`canvas-rendering.md §2.9` step 2).
            Renderer::Selection(s) => s
                .outline_spans(ws, &t)
                .map(|spans| {
                    spans
                        .iter()
                        .map(|r| pdn_core::pixel::IntRect::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2))
                        .collect()
                })
                .unwrap_or_else(|| s.bounds(ws, &t).into_iter().collect()),
            Renderer::Handles(h) => h.iter().filter_map(|h| nubs::device_box(&t, h)).collect(),
            Renderer::BrushPreview(p) => p.device_box(&t).into_iter().collect(),
            Renderer::PickerPreview(p) => p.device_box(&t).into_iter().collect(),
            Renderer::Tint(p) => p.device_box(&t).into_iter().collect(),
            Renderer::Grid(_) => vec![],
        })
        .map(|r| {
            Rect::from_min_size(
                canvas_min + Vec2::new(r.x as f32, r.y as f32) / ppp,
                Vec2::new(r.w as f32, r.h as f32) / ppp,
            )
        })
        .collect()
}

/// The screen rect of the active document's image (`getRenderBounds()` in the editor).
pub fn image_rect(app: &AppWorkspace, editor_min: Pos2) -> Option<Rect> {
    let b = app.active_ws()?.view.get_render_bounds();
    Some(Rect::from_min_size(
        editor_min + Vec2::new(b.x as f32, b.y as f32),
        Vec2::new(b.width as f32, b.height as f32),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
    }

    /// The forms placed in a layout (W, overlay) at 100 %.
    fn placed(layout: (Rect, Rect)) -> Forms {
        let mut forms = Forms::default();
        for w in &mut forms.windows {
            w.place(layout.0, layout.1, 1.0);
        }
        forms
    }

    /// 1024×632 (`floating-windows.md` §5 layout B on today's shell).
    fn b() -> (Rect, Rect) {
        (r(0.0, 85.0, 1009.0, 500.0), r(0.0, 0.0, 1024.0, 632.0))
    }

    /// Z1–Z3 (`floating-windows.md` §2.2, §5.1): the startup stack is the show order (Colors on
    /// top); a press hits the topmost form under it and raises that one; the canvas and popups
    /// change nothing.
    #[test]
    fn z1_z3_presses_hit_and_raise_the_topmost_form() {
        use FormId::*;
        let mut forms = placed(b());
        assert_eq!(forms.z_order(), [Tools, History, Layers, Colors]);
        // Tools (y 91–424) and Colors (y 262–579) overlap at x 6–72.
        let both = pos2(40.0, 300.0);
        assert_eq!(forms.top_at(both, &[]), Some(index(Colors)));
        let tools = pos2(40.0, 150.0);
        assert_eq!(forms.top_at(tools, &[]), Some(index(Tools)));
        forms.raise(index(Tools));
        assert_eq!(forms.z_order(), [History, Layers, Colors, Tools]);
        assert_eq!(forms.top_at(both, &[]), Some(index(Tools)));
        // The canvas, and an open popup over a form.
        assert_eq!(forms.top_at(pos2(500.0, 300.0), &[]), None);
        assert_eq!(forms.top_at(both, &[r(0.0, 250.0, 100.0, 100.0)]), None);
        // History (y 91–323) under Layers (y 301–579).
        assert_eq!(forms.top_at(pos2(900.0, 310.0), &[]), Some(index(Layers)));
    }

    /// Z4: hiding and showing a form puts it on top. Z5: Ctrl+Shift+F6 on a visible, resized
    /// History returns its default size and corner and keeps the stack; on a hidden one it shows
    /// it on top.
    #[test]
    fn z4_z5_toggles_and_resets() {
        use FormId::*;
        let mut app = AppWorkspace::default();
        let mut forms = placed(b());
        forms.toggle(Layers, &mut app);
        assert_eq!(forms.z_order(), [Tools, History, Colors]);
        forms.toggle(Layers, &mut app);
        assert_eq!(forms.z_order(), [Tools, History, Colors, Layers]);
        let a = (r(0.0, 85.0, 1265.0, 668.0), r(0.0, 0.0, 1280.0, 800.0));
        let history = index(History);
        forms.windows[history].size = Vec2::new(260.0, 300.0);
        forms.windows[history].set_position(pos2(300.0, 300.0), a.1, 1.0);
        forms.windows[history].snap = None;
        forms.reset(History, &mut app);
        forms.windows[history].place(a.0, a.1, 1.0);
        let w = &forms.windows[history];
        assert_eq!(
            (w.rect().min, w.size),
            (pos2(1057.0, 91.0), Vec2::new(200.0, 230.0))
        );
        assert_eq!(forms.z_order(), [Tools, History, Colors, Layers]);
        forms.toggle(History, &mut app);
        forms.reset(History, &mut app);
        assert_eq!(forms.z_order(), [Tools, Colors, Layers, History]);
    }

    /// `ui-drawn-graphics.md` Addendum: translucent only with the setting on, over the visible
    /// document (strictly overlapping; with no document it is never forced opaque), with the
    /// pointer elsewhere and no press held in it.
    #[test]
    fn the_translucency_condition_is_paint_net_s() {
        let window = r(10.0, 400.0, 272.0, 317.0);
        let doc = Some(r(232.0, 119.0, 800.0, 600.0));
        assert!(translucent(true, window, doc, false, false));
        assert!(!translucent(false, window, doc, false, false));
        assert!(!translucent(true, window, doc, true, false), "hovered");
        assert!(!translucent(true, window, doc, false, true), "pressed");
        // Clear of the visible document (touching its edge counts as clear): forced opaque.
        let clear = Some(r(282.0, 119.0, 800.0, 600.0));
        assert!(!translucent(true, window, clear, false, false));
        // No document: never forced opaque.
        assert!(translucent(true, window, None, false, false));
        assert!(!translucent(true, window, None, true, false));
        // A document scrolled out of the viewport (an empty visible rect).
        let gone = Some(r(232.0, 119.0, 800.0, 600.0).intersect(r(0.0, 0.0, 100.0, 50.0)));
        assert!(!translucent(true, window, gone, false, false));
    }
}
