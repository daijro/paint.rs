//! Port of: js/src/engine/AppWorkspace.js, and the engine-side parts of js/src/engine/AppView.js
//! (the `on/off/fire` bus becomes the `AppEvent` queue; the rulers' ticks are
//! `render::ruler::ticks`).
//! Reference: paintdotjs @ b4e6fc5

use crate::document::text_layer;
use crate::document::workspace::DocumentWorkspace;
use crate::history::{HistoryMemento, MementoKind};
use crate::tool::pan_zoom::PanTool;
use crate::tool::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool};
use crate::util::{
    color::Color,
    geom::{Point, Rectangle},
};

/// painting-tools-1 §3.14: zero is real pressure; mouse, disabled and absent use None → 1.
pub fn get_pointer_pressure(
    pointer_type: crate::tool::PointerType,
    raw: Option<f64>,
    enabled: bool,
) -> Option<f64> {
    if pointer_type != crate::tool::PointerType::Pen || !enabled {
        return None;
    }
    raw.filter(|p| p.is_finite()).map(|p| p.clamp(0.0, 1.0))
}

/// One variant per JS `app.fire(...)` event. Payloads that are the document workspace in JS are
/// omitted: the UI re-reads the active workspace when it drains the queue.
#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    /// Dirty document rects; empty = the whole document.
    DocumentInvalidated {
        rects: Vec<Rectangle>,
    },
    UpdateViewport,
    RenderLayerRegion {
        layer: usize,
        rect: Rectangle,
    },
    LayersChanged,
    LayerPropertiesChanged,
    ActiveLayerChanged,
    DocumentChanged,
    UpdateSize {
        w: u32,
        h: u32,
    },
    Resize {
        w: f64,
        h: f64,
    },
    HistoryChanged,
    SelectionChanged,
    DirtyChanged,
    FileChanged,
    CreateDocument,
    CloseDocument,
    UpdateActiveDocument,
    ActiveToolUpdated,
    UpdateMeasurementUnit,
    GridVisibilityChanged,
    RulersVisibilityChanged,
    ToolSettingChanged {
        key: String,
    },
    ShortcutChanged,
    /// Base character C while a Text move consumes clipboard units (§10.8).
    ToggleColorSlot,
    ToolStatusChanged,
    SelectionClipboardChanged,
    WindowOpenStateChanged {
        form: String,
    },
    EffectApplied,
    MouseDown {
        x: f64,
        y: f64,
        button: i32,
    },
    MouseMove {
        x: f64,
        y: f64,
    },
    MouseUp {
        x: f64,
        y: f64,
        button: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementUnit {
    Pixel,
    Inch,
    Centimeter,
}

impl MeasurementUnit {
    pub const ALL: [MeasurementUnit; 3] = [Self::Pixel, Self::Inch, Self::Centimeter];

    /// The JS unit string (`"pixel"`, `"inch"`, `"centimeter"`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Pixel => "pixel",
            Self::Inch => "inch",
            Self::Centimeter => "centimeter",
        }
    }

    pub fn from_id(id: &str) -> Option<MeasurementUnit> {
        Self::ALL.into_iter().find(|u| u.id() == id)
    }
}

/// `toUnit` result: a number for pixels, a 2-decimal string otherwise.
#[derive(Clone, Debug, PartialEq)]
pub enum UnitValue {
    Pixels(f64),
    Formatted(String),
}

pub use crate::tool::types::ToolType;

/// What the next history step reaches (`AppWorkspace::step_once`).
#[derive(Clone, Copy)]
enum NextStep {
    /// A transacted tool's entry: its tool and layer.
    Tool(ToolType, usize),
    /// A brush-like tool's one-entry stroke: its tool and layer.
    Stroke(ToolType, usize),
    Other,
}

/// A document's Clone Stamp anchor (painting-tools-1.md §3.10): the source layer, and the point
/// (canvas px) Ctrl+click set, until the first stroke turns it into an offset (anchor − the
/// stroke's start centre) that every later stroke keeps. With Aligned off (S§4.3) every stroke
/// starts at `AppWorkspace::clone_sources`' point, which stays here, and the offset is the last
/// stroke's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CloneAnchor {
    pub layer: usize,
    pub absolute: Option<[f64; 2]>,
    pub offset: Option<[f64; 2]>,
}

pub struct AppWorkspace {
    pub documents: Vec<DocumentWorkspace>,
    pub active: Option<usize>,
    /// `activeTool`: the tools ported so far (`tool::create_tool`); None for the rest until their
    /// tasks.
    pub tool: Option<Box<dyn Tool>>,
    /// `activeTool.isActive()`: `tool` ran `on_activate` (with a document) and not yet
    /// `on_deactivate`.
    pub tool_active: bool,
    /// `activeTool.getType()`, also for the tools not ported yet.
    pub tool_type: Option<ToolType>,
    /// `AppView.panTool`: the middle-button pan.
    pub pan_tool: PanTool,
    /// `AppView.lastMouseX/Y`: the pointer in canvas points, from the canvas's device-aligned
    /// origin (the UI feeds pointer moves).
    pub last_mouse: (f64, f64),
    /// The pointer (canvas points) while it lies inside the visible canvas viewport, where Zoom
    /// In/Out anchor (`ui-drawn-graphics.md §2.10` "Zoom presets"); None = the viewport centre.
    pub zoom_anchor: Option<(f64, f64)>,
    /// Device px per point (the OS scale), kept current by the UI: new views start at it, so their
    /// first Fit to Window uses the real scale.
    pub pixels_per_point: f64,
    /// The app clock in ms (`performance.now()`): the UI sets it each frame (`on_frame`), test
    /// harnesses advance it.
    pub now_ms: f64,
    /// Bumped whenever `set_active_tool_from_type` installs a tool: `with_tool` restores the tool
    /// it took only while the generation is unchanged.
    pub tool_generation: u64,
    /// Generations of tools that were deactivated while `with_tool` had them out (a hook switched
    /// tools); `with_tool` runs their `on_deactivate` when the hook returns.
    pub deferred_deactivations: Vec<u64>,
    pub previous_tool: Option<ToolType>,
    /// Every tool type's settings (`ToolType.settings`, `selectionRenderingQuality`); the UI loads
    /// them from storage and persists `dirty_keys`.
    pub tool_settings: crate::tool::types::ToolSettings,
    /// AppView `gridVisible` / `rulersVisible` (restored from `workspace.showPixelGrid` /
    /// `workspace.showRulers` by the UI).
    pub grid_visible: bool,
    pub rulers_visible: bool,
    /// `ActionRegistry.lastToolSwitchAt` (ms): the start of the current tool-hotkey chord.
    pub last_tool_switch_at: f64,
    /// `BitmapEffectAction.lastEffect` (definition id), for Repeat: the last OK'd Effects-menu
    /// effect (adjustments-and-effect-framework.md §2.2 step 7).
    pub last_effect: Option<String>,
    /// Its OK'd values (§2.7).
    pub last_effect_values: Option<crate::effect::Values>,
    /// paintdotjs's `rememberedValues`: each effect's last OK'd values, for the session (§2.7).
    pub effect_tokens: std::collections::HashMap<String, crate::effect::Values>,
    /// The effect instance seeds (§2.6 step 5): `Some(s)` hands out s, s + 1, … (tests);
    /// `None` takes them from the clock.
    pub effect_seed: Option<u32>,
    /// `DocumentIO.internalSelectionPath` (Copy Selection).
    pub internal_selection_path: Option<crate::selection::path::GraphicsPath>,
    /// `DocumentIO.internalClipboard`: the last copy (selection-and-move.md §2.11).
    pub clipboard: crate::io::clipboard::InternalClipboard,
    /// Modifier keys held now (`AppView` key state); the UI and the test harness set them.
    pub control_key_down: bool,
    pub shift_key_down: bool,
    pub alt_key_down: bool,
    /// The mouse buttons held anywhere, one bit per `MouseButton` value: canvas presses and
    /// releases set and clear them, and the UI writes the pointer state each frame (a toolbar
    /// slider drag holds a button too; painting-tools-2.md §7.4 step 5, §7.6 note §).
    pub mouse_buttons: u8,
    /// The mouse buttons pressed on the canvas and not released yet, one bit per `MouseButton`
    /// value (the canvas capture, painting-tools-1.md §8.1 "Two captures"): the
    /// capture lasts from the first press until the last button is up, through a dead drag.
    pub canvas_buttons: u8,
    /// The canvas buttons the active tool instance saw pressed. Setting
    /// the tool aside clears it — Paint.NET re-creates the tool, whose count starts at 0 — so
    /// after an end-then-commit tool letters and the generic Escape work again while the button is
    /// still held (painting-tools-1.md §8.1 "Dead drag"), though Undo stays blocked.
    pub tool_buttons: u8,
    /// Each document's Clone Stamp anchor, by `Document::id` and the tool (§3.10 step 5: it
    /// survives tool switches; S§4.3: the Healing Brush keeps its own).
    pub clone_anchors: std::collections::HashMap<(u64, ToolType), CloneAnchor>,
    /// Each document's Clone Stamp source point, the last Ctrl+click or anchor drag (canvas px),
    /// kept after the anchor turns relative: where an unaligned stroke starts (S§4.3). Keyed as
    /// `clone_anchors`.
    pub clone_sources: std::collections::HashMap<(u64, ToolType), [f64; 2]>,
    /// UI work raised by keyboard-dispatched actions (`action::dispatch_key`); the UI drains it.
    pub ui_requests: Vec<crate::action::UiRequest>,
    /// Object Selection may use MobileSAM when its toolbar picks it (the desktop and web apps set
    /// it); harnesses leave it off, so their lookups stay on GrabCut.
    pub object_model: bool,
    pub measurement_unit: MeasurementUnit,
    /// Fixed gradient dither seed for reproducible tests; production uses RandomState.
    pub seed_override: Option<u32>,
    /// Optional device-unit drag metrics for Shapes input tests or host integration (R95).
    pub shape_drag_size_override: Option<[f64; 2]>,
    /// Imported custom shapes; persistence and import UI are supplied by Task 54.
    pub shape_catalog: crate::tool::xaml::ShapeCatalog,
    pub primary_color: Color,
    pub secondary_color: Color,
    pub events: Vec<AppEvent>,
    /// `AppView.getViewWidth/Height()`: the `#view` client size, kept current by the UI.
    pub view_width: f64,
    pub view_height: f64,
    /// The Colors window's palette, synced by the UI each frame; empty = the default palette
    /// (BB common.md §5.11 "Palette colours").
    pub current_palette: Vec<crate::pixel::Px>,
    /// The date effects see instead of today's (tests); `None` reads the clock.
    pub date_override: Option<crate::effect::rng::CivilDate>,
    /// Edit ▸ Fill Selection (Content-Aware)'s running fill (`tool::remove`). Meanwhile the tools
    /// take no press and no key; Esc, an undo and every commit-first command cancel it.
    pub content_fill: Option<crate::tool::remove::Removal>,
    /// Layers ▸ Perspective Warp's canvas mode (`tool::perspective_warp`). Meanwhile it takes the
    /// canvas presses (the middle-button pan still pans) and Enter / Esc, the tool takes none; a
    /// tool or image switch and every commit-first command commit it, an undo cancels it.
    pub perspective: Option<crate::tool::perspective_warp::PerspectiveWarp>,
    /// Move Selected Pixels shows a pointer drag's pixels through the canvas's display-resolution
    /// float (`render::float`) instead of re-rendering the layer each frame; the drag's end
    /// renders them in exactly. Set by a UI that draws `DocumentWorkspace::float`.
    pub float_moves: bool,
}

impl Default for AppWorkspace {
    fn default() -> Self {
        Self {
            documents: vec![],
            active: None,
            tool: None,
            tool_active: false,
            tool_type: None,
            pan_tool: PanTool::default(),
            last_mouse: (0.0, 0.0),
            zoom_anchor: None,
            pixels_per_point: 1.0,
            now_ms: 0.0,
            tool_generation: 0,
            deferred_deactivations: vec![],
            previous_tool: None,
            tool_settings: Default::default(),
            grid_visible: false,
            rulers_visible: false,
            // No chord yet: the first hotkey press always starts one.
            last_tool_switch_at: f64::NEG_INFINITY,
            last_effect: None,
            last_effect_values: None,
            effect_tokens: Default::default(),
            effect_seed: None,
            internal_selection_path: None,
            clipboard: Default::default(),
            control_key_down: false,
            shift_key_down: false,
            alt_key_down: false,
            mouse_buttons: 0,
            canvas_buttons: 0,
            tool_buttons: 0,
            clone_anchors: Default::default(),
            clone_sources: Default::default(),
            ui_requests: vec![],
            object_model: false,
            measurement_unit: MeasurementUnit::Pixel,
            seed_override: None,
            shape_drag_size_override: None,
            shape_catalog: Default::default(),
            primary_color: Color::BLACK,
            secondary_color: Color::WHITE,
            events: vec![],
            view_width: 0.0,
            view_height: 0.0,
            current_palette: vec![],
            date_override: None,
            content_fill: None,
            perspective: None,
            float_moves: false,
        }
    }
}

/// Integer status coordinates in the applied culture, without grouping.
pub fn format_integer(v: f64) -> String {
    format_decimal(v, 0, false)
}

/// Paint.NET's status-bar "F2" (`ui-drawn-graphics.md §2.10` "in / cm values"): two decimals,
/// rounded half away from zero on the exact binary value, no grouping; `-` only when the result
/// is not zero. Appendix B B-G6 calibrates ties and grouping.
pub fn format_fixed2(v: f64) -> String {
    format_decimal(v, 2, false)
}

/// .NET "N{decimals}" in the applied culture (ui-drawn-graphics.md §2.10),
/// rounded half away from zero on the exact binary value.
pub fn format_n(v: f64, decimals: u32) -> String {
    format_decimal(v, decimals, true)
}

fn format_decimal(v: f64, decimals: u32, grouping: bool) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    let d = decimals.min(20) as usize;
    // 60 decimals are exact enough: a double's distance from a tie is either 0 or far above
    // 1e-60 at any magnitude where the tie digits matter.
    let s = format!("{:.60}", v.abs());
    let (int, frac) = s.split_once('.').expect("fixed-point digits");
    let mut digits = format!("{int}{}", &frac[..d]).into_bytes();
    if frac.as_bytes()[d] >= b'5' {
        let carry = digits.iter().rposition(|&c| c != b'9');
        for c in &mut digits[carry.map_or(0, |i| i + 1)..] {
            *c = b'0';
        }
        match carry {
            Some(i) => digits[i] += 1,
            None => digits.insert(0, b'1'),
        }
    }
    let negative = v < 0.0 && digits.iter().any(|&c| c != b'0');
    let (int, frac) = digits.split_at(digits.len() - d);
    crate::number_format::format_parts(
        std::str::from_utf8(int).expect("ascii digits"),
        std::str::from_utf8(frac).expect("ascii digits"),
        negative,
        grouping,
        &crate::i18n::culture_code(),
    )
}

/// The status bar's selection text (`ToolInfoItem.getSelectionText`, `ui-drawn-graphics.md §2.10`
/// "Selection info"): the statistics' bounds by the coordinate rule (pixels: integers, else F2 in
/// the unit, both without grouping) with the unit abbreviation ("" for pixels), and the area in
/// "N" (0 decimals for pixels, 2 otherwise) scaled by the unit's pixel size squared. `None`
/// without a document or a selection. Once `mask_builds` registers a native UI waker, heavy
/// geometry is computed on its pool; while pending, format the current document's last completed
/// statistics (or return `None` before its first result). Headless calls and wasm stay synchronous.
pub fn selection_info_text(app: &AppWorkspace) -> Option<String> {
    let ws = app.active_ws()?;
    let stats = crate::render::selection::selection_statistics(ws)?;
    let doc = &ws.document;
    let unit = app.measurement_unit;
    let pixel = unit == MeasurementUnit::Pixel;
    let decimals = if pixel { 0 } else { 2 };
    let coordinate = |v: f64| format_decimal(doc.pixel_to_physical(v, unit.id()), decimals, false);
    let t = crate::i18n::t;
    let abbreviation = if pixel {
        String::new()
    } else {
        t(&format!("measurementUnit.{}.abbreviation", unit.id()))
    };
    let px = doc.pixel_to_physical(1.0, unit.id());
    let b = stats.bounds;
    let mut values = vec![
        coordinate(b.x),
        abbreviation.clone(),
        coordinate(b.y),
        abbreviation.clone(),
        coordinate(b.width),
        abbreviation.clone(),
        coordinate(b.height),
        abbreviation,
        format_n(stats.area * px * px, decimals),
        t(&format!("measurementUnit.{}.plural", unit.id())).to_lowercase(),
    ];
    // While moving: ". Angle: {a}°", N2 (Task 27).
    let format = match app.tool.as_ref().and_then(|t| t.status_angle()) {
        Some(angle) => {
            values.push(format_n(angle, 2));
            "statusBar.context.selectedArea.text.withAngle.format"
        }
        None => "statusBar.context.selectedArea.text.format",
    };
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    Some(crate::i18n::tf(format, &refs))
}

/// The status bar's image-size text (`ui-drawn-graphics.md §2.10` "Size": "{0} × {1}", U+00D7).
pub fn format_size(w: &str, h: &str) -> String {
    format!("{w} × {h}")
}

/// The status bar's zoom text (`ui-drawn-graphics.md §2.10` "Zoom text"): "{⌊100·r⌉}%", half away
/// from zero.
pub fn format_zoom(r: f64) -> String {
    format!("{}%", (100.0 * r).round())
}

impl AppWorkspace {
    pub fn fire(&mut self, event: AppEvent) {
        self.events.push(event);
    }

    /// Takes the queue, including events the workspaces' views raised since the last drain and
    /// one `SelectionChanged` per workspace whose selection changed meanwhile.
    pub fn drain_events(&mut self) -> Vec<AppEvent> {
        let mut out = std::mem::take(&mut self.events);
        for ws in &mut self.documents {
            if ws.selection.revision() != ws.selection_revision {
                ws.selection_revision = ws.selection.revision();
                ws.view.events.push(AppEvent::SelectionChanged);
            }
            out.append(&mut ws.view.events);
        }
        out
    }

    /// True while events wait for `drain_events` (the UI then schedules another frame).
    pub fn has_events(&self) -> bool {
        !self.events.is_empty()
            || self.documents.iter().any(|ws| {
                !ws.view.events.is_empty() || ws.selection.revision() != ws.selection_revision
            })
    }

    pub fn active_ws(&self) -> Option<&DocumentWorkspace> {
        self.documents.get(self.active?)
    }

    pub fn active_ws_mut(&mut self) -> Option<&mut DocumentWorkspace> {
        let i = self.active?;
        self.documents.get_mut(i)
    }

    pub fn create_blank_document_in_new_workspace(
        &mut self,
        width: u32,
        height: u32,
        resolution: f64,
        is_initial: bool,
    ) -> usize {
        if !is_initial
            && let Some(i) = self.documents.iter().position(|w| w.is_initial_workspace)
            && self.is_initial_workspace_untouched(i)
        {
            self.close_document_workspace(i);
        }

        let mut ws = DocumentWorkspace::new(width, height, resolution);
        ws.is_initial_workspace = is_initial;
        // JS DocumentView reads the view size from AppView; Rust keeps a copy per view.
        ws.view.view_width = self.view_width;
        ws.view.view_height = self.view_height;
        ws.view.ppp = self.pixels_per_point;
        ws.view.fit_viewport();

        // Default background layer (the insert makes it the active layer).
        ws.add_layer(crate::document::layer::Layer::create_background_layer(
            width, height,
        ));
        ws.set_active_layer(0);
        // JS then calls document.invalidate() again; add_layer already composited everything.

        self.documents.push(ws);
        let index = self.documents.len() - 1;
        self.set_active_document_workspace(Some(index));

        let ws = &mut self.documents[index];
        ws.history.clear_all();
        ws.history.push_new_memento(HistoryMemento::null(
            &crate::i18n::t("newImageAction.name"),
            "assets/icons/menu_file_new_icon.png",
        ));
        ws.set_dirty(false);

        self.fire(AppEvent::CreateDocument);
        index
    }

    /// `isInitialWorkspaceUntouched`; a pending bitmap transaction (a `ToolBase` holds the
    /// workspace scratch surface) is user work too.
    pub fn is_initial_workspace_untouched(&self, i: usize) -> bool {
        let Some(ws) = self.documents.get(i) else {
            return false;
        };
        !ws.dirty
            && ws.scratch_borrowed.is_none()
            && ws.history.redo.is_empty()
            && (ws.history.undo.is_empty()
                || (ws.history.undo.len() == 1
                    && matches!(ws.history.undo[0].kind, MementoKind::Null)))
    }

    /// `closeDocumentWorkspace(ws, confirmUnsaved)`: the unsaved-changes confirmation is the
    /// UI's job. Returns false for an unknown index.
    pub fn close_document_workspace(&mut self, i: usize) -> bool {
        if i >= self.documents.len() {
            return false;
        }
        let was_active = self.active == Some(i);
        if was_active {
            // Its image goes: nothing to commit to or restore.
            self.perspective = None;
            // Don't Save cancels before removal. Deactivation would commit; a reopened
            // transaction must also restore its toolbar backup (painting-tools-2 §7.6).
            self.with_transacted(|t, app| {
                t.cancel_gesture(app);
                t.set_aside(app);
            });
            self.tool_active = false;
            let fresh = self.tool_type.and_then(crate::tool::create_tool);
            let old = std::mem::replace(&mut self.tool, fresh);
            self.drop_tool(old);
            self.tool_generation += 1;
            self.pan_tool = PanTool::default();
            self.tool_buttons = 0;
            self.canvas_buttons = 0;
        }
        // Dropping a parked instance discards its image-owned pending work without running
        // a commit hook against the active image. Its workspace and scratch go away together.
        let parked = self.documents[i].parked_tool.take();
        self.drop_tool(parked);
        self.documents.remove(i);
        // Indices after `i` shifted down.
        match self.active {
            Some(a) if a > i => self.active = Some(a - 1),
            Some(a) if a == i => self.active = None,
            _ => {}
        }
        if was_active {
            let next = self.documents.len().checked_sub(1).map(|last| i.min(last));
            self.set_active_document_workspace(next);
        }
        self.fire(AppEvent::CloseDocument);
        true
    }

    /// A tool dropped with its image, uncommitted: a Text re-edit still puts back the shared
    /// settings it wrote (S§4.5), which outlive the image.
    fn drop_tool(&mut self, tool: Option<Box<dyn Tool>>) {
        if let Some(mut t) = tool
            && let Some(t) = t.as_any_mut().downcast_mut::<crate::tool::text::TextTool>()
        {
            t.restore_shared(self);
        }
    }

    /// Each image keeps its own tool instance while its transaction is open
    /// (painting-tools-2.md §7.6 "Switching to another image tab"): leaving parks it in its
    /// workspace, and returning resumes it, or commits it there when the global tool changed
    /// meanwhile. Other tool sessions end, and the same tool activates for the new document.
    pub fn set_active_document_workspace(&mut self, i: Option<usize>) {
        if i != self.active {
            self.release_canvas_capture();
            // A running Fill Selection (Content-Aware) is cancelled, as by a commit-first command;
            // Perspective Warp commits, as a tool switch commits Move.
            self.content_fill = None;
            crate::tool::perspective_warp::end(self, true);
        }
        let open = self.tool.as_mut().is_some_and(|t| {
            t.transacted().is_some_and(|t| t.state().is_open())
                || t.as_any_mut()
                    .downcast_ref::<crate::tool::text::TextTool>()
                    .is_some_and(|t| t.is_editing())
        });
        match self.active {
            Some(a) if self.tool_active && open && a < self.documents.len() => {
                self.documents[a].parked_tool = self.tool.take();
                self.tool_active = false;
                self.tool = self.tool_type.and_then(crate::tool::create_tool);
                self.tool_generation += 1;
            }
            _ => self.deactivate_tool(),
        }
        self.active = i;
        let (grid, rulers) = (self.grid_visible, self.rulers_visible);
        if let Some(ws) = self.active_ws_mut() {
            ws.set_grid_visible(grid);
            ws.set_rulers_visible(rulers);
        }
        match self.active_ws_mut().and_then(|ws| ws.parked_tool.take()) {
            Some(parked) if Some(parked.tool_type()) == self.tool_type => {
                self.tool = Some(parked);
                self.tool_active = true;
                self.tool_generation += 1;
            }
            Some(parked) => {
                // The return switches tools, which commits the parked transaction here.
                let current = self.tool.replace(parked);
                self.tool_active = true;
                self.deactivate_tool();
                self.tool = current;
                self.activate_tool();
            }
            // `setActiveTool(getActiveTool())`: the same tool, activated for the new document.
            None => self.activate_tool(),
        }
        self.fire(AppEvent::ActiveToolUpdated);
        self.fire(AppEvent::UpdateViewport);
        self.fire(AppEvent::UpdateActiveDocument);
    }

    /// `setActiveToolFromType` → `setActiveTool(type.create())`: the old tool deactivates, the new
    /// one activates when a document is open.
    pub fn set_active_tool_from_type(&mut self, tool_type: ToolType) {
        crate::tool::perspective_warp::end(self, true);
        if let Some(previous) = self.tool_type
            && previous != tool_type
        {
            self.previous_tool = Some(previous);
        }
        self.deactivate_tool();
        self.tool = crate::tool::create_tool(tool_type);
        self.tool_generation += 1;
        self.tool_type = Some(tool_type);
        self.activate_tool();
        self.fire(AppEvent::ActiveToolUpdated);
    }

    /// Runs `f` on the active tool, taken out of `self.tool` meanwhile. A deactivation `f`
    /// caused (a tool or document switch) runs when `f` returns, since the tool was out then. After
    /// a tool switch the new tool stays and the taken one is dropped; after a document switch the
    /// taken tool comes back and activates for the new document.
    pub fn with_tool<R>(&mut self, f: impl FnOnce(&mut dyn Tool, &mut Self) -> R) -> Option<R> {
        let mut tool = self.tool.take()?;
        let generation = self.tool_generation;
        let r = f(tool.as_mut(), self);
        let deactivated = match self
            .deferred_deactivations
            .iter()
            .position(|&g| g == generation)
        {
            // paintdotjs deactivates before the successor activates; the tool was out then.
            Some(i) => {
                self.deferred_deactivations.remove(i);
                tool.on_deactivate(self);
                true
            }
            None => false,
        };
        if generation == self.tool_generation && self.tool.is_none() {
            self.tool = Some(tool);
            if deactivated {
                self.activate_tool();
            }
        }
        Some(r)
    }

    /// `with_tool` for the activated tool's hooks only (paintdotjs adds the listeners in
    /// `onActivate`).
    fn tool_hook(&mut self, f: impl FnOnce(&mut dyn Tool, &mut Self)) {
        if self.tool_active {
            self.with_tool(f);
        }
    }

    pub(crate) fn deactivate_tool(&mut self) {
        if std::mem::take(&mut self.tool_active) {
            self.tool_buttons = 0;
            if self.tool.is_some() {
                self.with_tool(|t, app| t.on_deactivate(app));
            } else {
                // A `with_tool` up the stack has it out.
                self.deferred_deactivations.push(self.tool_generation);
            }
        }
    }

    /// The per-frame tick: sets the clock and runs the active tool's `on_frame`.
    /// A transacted tool's settings-window tick runs here (painting-tools-2.md §7.4 step 5), so
    /// no tool has to forward it.
    pub fn on_frame(&mut self, now_ms: f64) {
        self.now_ms = now_ms;
        crate::tool::remove::poll_content_fill(self);
        crate::tool::object_select::release_model(self);
        self.with_transacted(|t, app| t.poll(app));
        self.tool_hook(|t, app| t.on_frame(app));
    }

    /// `AppView` key state; the active tool's `on_modifier_keys_changed` runs when it changes.
    pub fn set_modifier_keys(&mut self, ctrl: bool, shift: bool, alt: bool) {
        let old = [
            self.control_key_down,
            self.shift_key_down,
            self.alt_key_down,
        ];
        [
            self.control_key_down,
            self.shift_key_down,
            self.alt_key_down,
        ] = [ctrl, shift, alt];
        if old != [ctrl, shift, alt] {
            self.tool_hook(|t, app| t.on_modifier_keys_changed(app));
        }
    }

    /// Runs `f` on the active document through the commit-first hook; the tool is inactive
    /// meanwhile, so no tool sees a selection change from outside (painting-tools-2.md §7.6).
    pub fn edit_selection<R>(&mut self, f: impl FnOnce(&mut DocumentWorkspace) -> R) -> Option<R> {
        self.commit_tool_first(|app| app.active_ws_mut().map(f))
    }

    /// painting-tools-2.md §7.6 "Commit first": the active tool is deactivated (its gestures end
    /// and a Dirty transaction commits), `f` runs, and the tool is re-activated in Idle. Every
    /// command that changes pixels, layers, the document or the selection runs through it, plus
    /// Cut, Paste, Save and Print (`action::perform`); nested calls are plain calls.
    pub fn commit_tool_first<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.content_fill = None;
        crate::tool::perspective_warp::end(self, true);
        let active = self.tool_active;
        self.deactivate_tool();
        let r = f(self);
        // `f` may have activated a tool itself (a document or tool switch).
        if active && !self.tool_active {
            self.activate_tool();
        }
        r
    }

    /// An active-layer change: a deactivate and re-activate of the tool around it (§7.8).
    pub fn set_active_layer(&mut self, index: usize) {
        let Some((active, n)) = self
            .active_ws()
            .map(|ws| (ws.active_layer, ws.document.layers.size()))
        else {
            return;
        };
        if active != index {
            self.commit_tool_first(|app| {
                if let Some(ws) = app.active_ws_mut() {
                    // Committing emptied editable text deletes its layer, the active one (S§4.5,
                    // R76): the layers above it moved down.
                    let removed = ws.document.layers.size() < n && index > active;
                    ws.set_active_layer(if removed { index - 1 } else { index });
                }
            });
        }
    }

    /// `ToolMenu.updateSetting`: `type.setSetting`, `app:tool_setting_changed`, then the active
    /// tool's `onSettingChanged` when it is that tool.
    /// A pick of the value already set changes nothing (no event, no settings window).
    pub fn update_tool_setting(&mut self, tool: ToolType, key: &str, value: serde_json::Value) {
        let old = self.tool_settings.get(tool, key);
        // Numbers compare by value (the toolbar writes 50, not 50.0, but either may arrive).
        if old == value || old.as_f64().is_some_and(|o| Some(o) == value.as_f64()) {
            return;
        }
        self.tool_settings.set(tool, key, value);
        self.fire(AppEvent::ToolSettingChanged { key: key.into() });
        if self.tool_active && self.tool_type == Some(tool) {
            self.with_tool(|t, app| t.on_setting_changed(app, key));
        }
    }

    /// Gradient's per-gesture non-negative int31 (painting-tools-2 §2.3).
    pub fn random_seed(&mut self) -> u32 {
        use std::hash::BuildHasher;
        self.seed_override.unwrap_or_else(|| {
            (std::collections::hash_map::RandomState::new().hash_one(0u8) as u32) & 0x7fff_ffff
        })
    }

    /// Palette edits notify the active tool once; record write-back bypasses this method.
    pub fn set_colors(&mut self, primary: Color, secondary: Color) {
        if (self.primary_color, self.secondary_color) == (primary, secondary) {
            return;
        }
        (self.primary_color, self.secondary_color) = (primary, secondary);
        if self.tool_active {
            self.with_tool(|t, app| t.on_colors_changed(app));
        }
    }

    pub fn swap_colors(&mut self) {
        self.set_colors(self.secondary_color, self.primary_color);
    }

    /// The active tool's settings window is open, or a timer of its own runs (a nudge burst): the
    /// UI keeps frames coming for its ticks.
    pub fn tool_ticking(&mut self) -> bool {
        self.content_fill.is_some()
            || self
                .tool
                .as_mut()
                .is_some_and(|t| t.needs_ticks() || t.transacted().is_some_and(|t| t.ticking()))
    }

    /// The active tool's background job runs (`Tool::busy`), or Fill Selection (Content-Aware)'s:
    /// the canvas shows the busy cursor.
    pub fn tool_busy(&self) -> bool {
        self.content_fill.is_some() || self.tool.as_ref().is_some_and(|t| t.busy())
    }

    fn activate_tool(&mut self) {
        if self.active.is_some() && self.tool.is_some() {
            self.tool_active = true;
            self.with_tool(|t, app| t.on_activate(app));
        }
    }

    /// What the canvas shows as the pointer: the middle-button pan's hand while it drags, else
    /// the active tool's request (`AppView.temporaryPanCursorState` restores it on release).
    pub fn cursor(&self) -> CursorRequest {
        if self.pan_tool.is_tracking() {
            return self.pan_tool.cursor();
        }
        if self.perspective.is_some() {
            return crate::tool::perspective_warp::cursor(self);
        }
        self.tool
            .as_ref()
            .map_or(CursorRequest::Css("default"), |t| t.cursor())
    }

    /// `toToolDocumentPosition` for the active tool and the samples of `input` (canvas points →
    /// document coordinates of their device pixels; the samples stay continuous).
    fn tool_position(&self, x: f64, y: f64, input: &PointerInput) -> Option<(Point, PointerInput)> {
        let ws = self.active_ws()?;
        let continuous = self
            .tool
            .as_ref()
            .is_some_and(|t| t.uses_continuous_pointer_coordinates());
        let p = ws.view.to_document_position(Point::new(x, y), continuous);
        let mut input = input.clone();
        for s in &mut input.samples {
            let q = ws.view.to_document_position(Point::new(s.x, s.y), true);
            (s.x, s.y) = (q.x, q.y);
        }
        Some((p, input))
    }

    /// Ctrl+Z, Ctrl+Y and Edit ▸ Undo/Redo are ignored while this holds (painting-tools-1.md §8.2,
    /// ruling R81): some canvas button is still down.
    pub fn canvas_captured(&self) -> bool {
        self.canvas_buttons != 0
    }

    /// Ruling R82 (painting-tools-1.md §8.10): switching images mid-drag first gives the active
    /// tool a release of each canvas button still held, at the last pointer position with no new
    /// point, as a capture loss would: an open stroke commits in its own image (one entry). The
    /// buttons stay captured until they are really up.
    fn release_canvas_capture(&mut self) {
        let empty = PointerInput {
            pointer_type: crate::tool::PointerType::Mouse,
            samples: vec![],
        };
        let (x, y) = self.last_mouse;
        for b in MouseButton::ALL {
            if self.canvas_buttons & (1 << b as u8) == 0 {
                continue;
            }
            let Some((p, input)) = self.tool_position(x, y, &empty) else {
                return;
            };
            if self.pan_tool.is_tracking() {
                let mut pan = std::mem::take(&mut self.pan_tool);
                pan.on_mouse_up(self, p.x, p.y, b, &input);
                self.pan_tool = pan;
            } else {
                self.with_tool(|t, app| t.on_mouse_up(app, p.x, p.y, b, &input));
            }
        }
    }

    /// painting-tools-1.md §8.7: only a first Middle press pans. Other presses reach the
    /// active gesture; subsequent buttons must not take a stroke over for panning.
    pub fn on_mouse_down(&mut self, x: f64, y: f64, b: MouseButton, input: &PointerInput) -> bool {
        let first = self.tool_buttons == 0;
        self.mouse_buttons |= 1 << b as u8;
        self.canvas_buttons |= 1 << b as u8;
        self.tool_buttons |= 1 << b as u8;
        let Some((p, input)) = self.tool_position(x, y, input) else {
            return false;
        };
        if self.pan_tool.is_tracking() || (first && b == MouseButton::Middle) {
            let mut pan = std::mem::take(&mut self.pan_tool);
            let handled = pan.on_mouse_down(self, p.x, p.y, b, &input);
            self.pan_tool = pan;
            return handled;
        }
        if self.perspective.is_some() {
            if b == MouseButton::Left
                && let Some(p) = self.perspective_point(x, y)
            {
                crate::tool::perspective_warp::on_mouse_down(self, p);
            }
            return true;
        }
        if self.content_fill.is_some() {
            return true;
        }
        // S§4.5: a press that would start a pixel edit of a text layer asks to rasterize it
        // first; the tool never sees it.
        let ctrl = self.control_key_down;
        if self
            .tool_type
            .is_some_and(|t| text_layer::press_edits_pixels(t, b, ctrl, &self.tool_settings))
            && !text_layer::ensure_active_raster(self)
        {
            return true;
        }
        self.with_tool(|t, app| t.on_mouse_down(app, p.x, p.y, b, &input))
            .unwrap_or(false)
    }

    /// `AppView.onMouseMove`: records `last_mouse`, then drives the pan while it tracks, else the
    /// active tool.
    pub fn on_mouse_move(&mut self, x: f64, y: f64, input: &PointerInput) -> bool {
        self.last_mouse = (x, y);
        let Some((p, input)) = self.tool_position(x, y, input) else {
            return false;
        };
        if self.pan_tool.is_tracking() {
            let mut pan = std::mem::take(&mut self.pan_tool);
            let handled = pan.on_mouse_move(self, p.x, p.y, &input);
            self.pan_tool = pan;
            return handled;
        }
        if self.perspective.is_some() {
            if let Some(p) = self.perspective_point(x, y) {
                crate::tool::perspective_warp::on_mouse_move(self, p);
            }
            return true;
        }
        self.with_tool(|t, app| t.on_mouse_move(app, p.x, p.y, &input))
            .unwrap_or(false)
    }

    /// `AppView.onMouseUp`: a tracking pan takes any button's release.
    pub fn on_mouse_up(&mut self, x: f64, y: f64, b: MouseButton, input: &PointerInput) -> bool {
        self.mouse_buttons &= !(1 << b as u8);
        self.canvas_buttons &= !(1 << b as u8);
        self.tool_buttons &= !(1 << b as u8);
        let Some((p, input)) = self.tool_position(x, y, input) else {
            return false;
        };
        if self.pan_tool.is_tracking() {
            let mut pan = std::mem::take(&mut self.pan_tool);
            let handled = pan.on_mouse_up(self, p.x, p.y, b, &input);
            self.pan_tool = pan;
            return handled;
        }
        if self.perspective.is_some() {
            if b == MouseButton::Left
                && let Some(p) = self.perspective_point(x, y)
            {
                crate::tool::perspective_warp::on_mouse_up(self, p);
            }
            return true;
        }
        self.with_tool(|t, app| t.on_mouse_up(app, p.x, p.y, b, &input))
            .unwrap_or(false)
    }

    /// Canvas point (x, y) in continuous layer coordinates, for Perspective Warp's corners.
    fn perspective_point(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let p = self
            .active_ws()?
            .view
            .to_document_position(Point::new(x, y), true);
        Some([p.x, p.y])
    }

    /// `AppWorkspace.onDocumentKeyPress`: the active tool sees keys first (true = consumed; the
    /// shortcut is then not dispatched).
    pub fn on_key_press(&mut self, key: &KeyPress) -> bool {
        if self.active.is_none() {
            return false;
        }
        // Perspective Warp: Enter commits, Esc cancels; the tool takes no key and the rest reach
        // the shortcuts (a tool switch or a commit-first command commits first).
        if self.perspective.is_some() {
            let plain = !(key.ctrl || key.alt || key.meta);
            let commit = match key.key.as_str() {
                "Enter" if plain => true,
                "Escape" if plain => false,
                _ => return false,
            };
            crate::tool::perspective_warp::end(self, commit);
            return true;
        }
        // A running Fill Selection (Content-Aware): Esc cancels it, and the tool takes no key.
        if self.content_fill.is_some() {
            if key.key == "Escape" {
                self.content_fill = None;
                return true;
            }
        } else if self
            .with_tool(|t, app| t.on_key_press(app, key))
            .unwrap_or(false)
        {
            return true;
        }
        if key.key.eq_ignore_ascii_case("x") && !(key.ctrl || key.alt || key.meta) {
            self.swap_colors();
            return true;
        }
        // The generic tool key handler (painting-tools-2.md §7.6 note §): Escape,
        // with or without Shift/Ctrl/Alt, deselects through the commit-first path, but only with
        // a non-empty selection and no button pressed on the tool (`tool_buttons`).
        if key.key == "Escape"
            && !key.meta
            && self.tool_buttons == 0
            && self.active_ws().is_some_and(|ws| !ws.selection.is_empty())
        {
            self.edit_selection(|ws| ws.execute_function(crate::history::functions::deselect));
            return true;
        }
        false
    }

    /// AppView `isGridVisible`.
    pub fn is_grid_visible(&self) -> bool {
        self.active.is_some() && self.grid_visible
    }

    /// AppView `isRulersVisible`.
    pub fn is_rulers_visible(&self) -> bool {
        self.active.is_some() && self.rulers_visible
    }

    /// AppView `setGridVisible` (the UI persists `workspace.showPixelGrid` on the event).
    pub fn set_grid_visible(&mut self, visible: bool) {
        let Some(ws) = self.active_ws_mut() else {
            return;
        };
        ws.set_grid_visible(visible);
        self.grid_visible = visible;
        self.fire(AppEvent::GridVisibilityChanged);
    }

    /// AppView `setRulersVisible` (the UI persists `workspace.showRulers` and runs
    /// `syncRulerVisibility` on the event).
    pub fn set_rulers_visible(&mut self, visible: bool) {
        let Some(ws) = self.active_ws_mut() else {
            return;
        };
        ws.set_rulers_visible(visible);
        self.rulers_visible = visible;
        self.fire(AppEvent::RulersVisibilityChanged);
    }

    /// `setMeasurementUnit` (the UI persists `workspace.measurementUnit`/`lastNonPixelUnit`;
    /// the footer re-reads the cursor position on the event, as `document:mousemove` does).
    pub fn set_measurement_unit(&mut self, unit: MeasurementUnit) {
        self.measurement_unit = unit;
        self.fire(AppEvent::UpdateMeasurementUnit);
    }

    pub fn to_unit(&self, pixels: f64) -> UnitValue {
        let unit = match self.measurement_unit {
            MeasurementUnit::Pixel => return UnitValue::Pixels(pixels),
            MeasurementUnit::Inch => "inch",
            MeasurementUnit::Centimeter => "centimeter",
        };
        let value = match self.active_ws() {
            Some(ws) => ws.document.pixel_to_physical(pixels, unit),
            None => pixels,
        };
        UnitValue::Formatted(format_fixed2(value))
    }

    /// One history step, ignored while a canvas button is held (ruling R81: the Edit menu
    /// checks the canvas capture, painting-tools-1.md §8.2 and painting-tools-2.md §7.5
    /// case 4 as corrected; every caller goes through here, the History panel too). Otherwise a
    /// transacted tool's gesture is cancelled first (by the core, so no tool has to forward
    /// it), then the step runs between the tool's
    /// `on_executing_history_memento` and `on_executed_history_memento`; a selection the step
    /// changed gets `on_selection_changing`
    /// and `on_selection_changed` after it (the selection is restored inside the step; paintdotjs's
    /// tools ignore `changing` while a memento executes).
    fn history_step(&mut self, forward: bool) -> bool {
        let Some(before) = self.active_ws().map(|ws| ws.selection.revision()) else {
            return false;
        };
        if self.canvas_captured() {
            return false;
        }
        // An undo while Perspective Warp runs cancels it, and steps nothing else.
        if self.perspective.is_some() {
            crate::tool::perspective_warp::end(self, false);
            return true;
        }
        // Ruling R29: an undo cancels a running fill, as it does a running removal.
        self.content_fill = None;
        self.with_transacted(|t, app| t.cancel_gesture(app));
        self.tool_hook(|t, app| t.on_executing_history_memento(app));
        let done = self.step_once(forward);
        if self
            .active_ws()
            .is_some_and(|ws| ws.selection.revision() != before)
        {
            self.tool_hook(|t, app| t.on_selection_changing(app));
            self.tool_hook(|t, app| t.on_selection_changed(app));
        }
        self.tool_hook(|t, app| t.on_executed_history_memento(app));
        done
    }

    /// The memento the next undo (or redo) steps: its id and its kind.
    fn next_step(&self, forward: bool) -> Option<(u64, NextStep)> {
        let h = &self.active_ws()?.history;
        let m = if forward {
            h.redo.front()?
        } else if h.can_undo() {
            h.undo.last()?
        } else {
            return None;
        };
        let next = match &m.kind {
            MementoKind::Tool(e) => NextStep::Tool(e.tool, e.layer),
            MementoKind::Stroke { tool, layer, .. } => NextStep::Stroke(*tool, *layer),
            _ => NextStep::Other,
        };
        Some((m.id, next))
    }

    /// §7.2/§7.5: a tool entry switches to its tool, then its layer, then the tool steps it. A
    /// non-tool entry is stepped by the document; its undo sets a Dirty transaction aside around
    /// the step (§7.5 case 5), its redo leaves the transaction alone. A stroke entry
    /// (painting-tools-1.md §8.8) is a non-tool entry whose undo then makes its layer active (the
    /// tool stays) and whose redo first switches to its tool and layer.
    fn step_once(&mut self, forward: bool) -> bool {
        let Some((id, next)) = self.next_step(forward) else {
            return false;
        };
        let (tool, layer) = match next {
            NextStep::Tool(tool, layer) => (tool, layer),
            NextStep::Stroke(tool, layer) if forward => {
                if self.tool_type != Some(tool) {
                    self.set_active_tool_from_type(tool);
                }
                self.set_active_layer(layer);
                // Switching could only push an entry if something was left open.
                if self.next_step(forward).map(|n| n.0) != Some(id) {
                    return false;
                }
                return self.active_ws_mut().is_some_and(|ws| ws.redo());
            }
            other => {
                let aside = if forward {
                    None
                } else {
                    self.with_transacted(|t, app| t.set_aside(app)).flatten()
                };
                let done = self
                    .active_ws_mut()
                    .is_some_and(|ws| if forward { ws.redo() } else { ws.undo() });
                if let Some(r) = aside {
                    self.with_transacted(|t, app| t.restore(app, r));
                }
                if let (NextStep::Stroke(_, layer), true) = (other, done) {
                    self.set_active_layer(layer);
                }
                return done;
            }
        };
        if self.tool_type != Some(tool) {
            self.set_active_tool_from_type(tool);
        }
        self.set_active_layer(layer);
        // Switching could only push an entry if something was left open; never step past it.
        if self.next_step(forward).map(|n| n.0) != Some(id)
            || !self.tool_active
            || self.tool.as_mut().and_then(|t| t.transacted()).is_none()
        {
            return false;
        }
        let Some(ws) = self.active_ws_mut() else {
            return false;
        };
        let m = if forward {
            ws.history.redo.pop_front()
        } else {
            ws.history.undo.pop()
        };
        let Some(HistoryMemento {
            id,
            name,
            image,
            kind: MementoKind::Tool(entry),
        }) = m
        else {
            unreachable!("next_step saw a tool entry");
        };
        let mut slot = Some(entry.step);
        let stepped = self.with_transacted(|t, app| t.step_entry(app, slot.take().unwrap()));
        // A refused step (its precondition does not hold) goes back where it came from.
        let (step, done) = match stepped {
            Some(Ok(step)) => (step, true),
            Some(Err(step)) => (step, false),
            None => (slot.take().expect("the step"), false),
        };
        let m = HistoryMemento {
            id,
            name,
            image,
            kind: MementoKind::Tool(crate::tool::transacted::ToolEntry { step, ..entry }),
        };
        let Some(ws) = self.active_ws_mut() else {
            return false;
        };
        if forward == done {
            ws.history.undo.push(m);
        } else {
            ws.history.redo.push_front(m);
        }
        if done {
            ws.after_history_change();
        }
        done
    }

    /// Runs `f` on the active tool's transacted core, if it has one.
    fn with_transacted<R>(
        &mut self,
        f: impl FnOnce(&mut dyn crate::tool::transacted::DynTransacted, &mut Self) -> R,
    ) -> Option<R> {
        if !self.tool_active {
            return None;
        }
        self.with_tool(|t, app| t.transacted().map(|x| f(x, app)))
            .flatten()
    }

    /// A new effect instance's seed (§2.6 step 5): the next pinned one, else the clock's.
    pub fn next_instance_seed(&mut self) -> u32 {
        match &mut self.effect_seed {
            Some(s) => {
                let seed = *s;
                *s = s.wrapping_add(1);
                seed
            }
            None => crate::effect::rng::clock_seed(),
        }
    }

    /// Undo on the active document; false when nothing was undone.
    pub fn undo(&mut self) -> bool {
        self.history_step(false)
    }

    pub fn redo(&mut self) -> bool {
        self.history_step(true)
    }

    /// `HistoryStack.stepTo(id)` on the active document, one step at a time; false when no step
    /// ran.
    pub fn step_to(&mut self, id: u64) -> bool {
        let Some(h) = self.active_ws().map(|ws| &ws.history) else {
            return false;
        };
        let top = |app: &Self| {
            app.active_ws()
                .and_then(|ws| ws.history.undo.last())
                .is_some_and(|m| m.id != id)
        };
        let mut stepped = false;
        if let Some(i) = h.undo.iter().position(|m| m.id == id) {
            if i + 1 == h.undo.len() {
                return h.undo.len() > 1 && self.undo();
            }
            while top(self) && self.undo() {
                stepped = true;
            }
        } else if h.redo.iter().any(|m| m.id == id) {
            while top(self) && self.redo() {
                stepped = true;
            }
        }
        stepped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::geom::Point;

    fn app_with_doc(w: u32, h: u32, view: (f64, f64)) -> AppWorkspace {
        let mut app = AppWorkspace::default();
        let i = app.create_blank_document_in_new_workspace(w, h, 96.0, true);
        let ws = &mut app.documents[i];
        ws.view.view_width = view.0;
        ws.view.view_height = view.1;
        ws.view.fit_viewport();
        app
    }

    #[test]
    fn fit_viewport_is_paint_net_fit() {
        // canvas-rendering.md §2.5 step 9: min(1, (Vw − 9u)/w, (Vh − 9u)/h).
        let app = app_with_doc(800, 600, (1249.0, 415.0));
        let z = app.active_ws().unwrap().view.zoom;
        assert_eq!(z, (415.0 - 9.0) / 600.0);
    }

    #[test]
    fn set_view_position_clamps_like_dom_scroll() {
        let mut app = app_with_doc(800, 600, (1000.0, 600.0));
        let ws = app.active_ws_mut().unwrap();
        ws.view.set_view_position(-50.0, 1e9);
        let (ew, eh) = ws.view.environment_size();
        assert_eq!(ws.view.viewport_x, 0.0);
        assert_eq!(ws.view.viewport_y, eh - 600.0);
        assert!(ew >= 1000.0);
    }

    #[test]
    fn document_screen_roundtrip() {
        let mut app = app_with_doc(800, 600, (1000.0, 700.0));
        let ws = app.active_ws_mut().unwrap();
        // An integer zoom snaps the origin to a device px: whole document px map back exactly.
        ws.view.set_zoom(2.0, 500.3, 350.0);
        let p = ws.view.to_screen_position(Point::new(123.0, 45.0));
        assert_eq!(
            ws.view.to_document_position(p, true),
            Point::new(123.0, 45.0)
        );
        // A pointer maps its device pixel's top-left corner: anywhere inside document px
        // (123, 45) floors to it.
        ws.view.set_zoom(2.5, 500.3, 350.0);
        let p = ws.view.to_screen_position(Point::new(123.9, 45.9));
        let floored = ws.view.to_document_position(p, false);
        assert_eq!((floored.x, floored.y), (123.0, 45.0));
        let q = ws.view.to_document_position(p, true);
        assert!(q.x <= 123.9 && 123.9 - q.x < 1.0 / 2.5);
    }

    /// Carried from Task 17e: pointers map through the snapped transform the canvas is drawn
    /// with, so the footer and the ruler marker agree at a fractional scale.
    #[test]
    fn pointer_maps_through_the_snapped_view() {
        let mut app = app_with_doc(800, 600, (1001.0, 700.0));
        let v = &mut app.active_ws_mut().unwrap().view;
        v.set_pixels_per_point(1.25);
        v.set_zoom_to_window(true);
        let t = v.view_transform();
        assert_ne!(t.origin[0], v.get_render_bounds().x * 1.25);
        // Device px origin + 10 is image px ⌊10/s⌋; the unsnapped mapping would say otherwise.
        let d = t.origin[0] + 10.0;
        let p = v.to_document_position(Point::new(d / 1.25, d / 1.25), false);
        assert_eq!(p.x, (10.0 / t.scale).floor());
    }

    #[test]
    fn huge_document_zoom_extremes() {
        // Review Focus #3
        let mut app = app_with_doc(8000, 8000, (1249.0, 615.0));
        let ws = app.active_ws_mut().unwrap();
        for z in [0.01, 0.5, 1.0, 37.5, 100.0, 1000.0] {
            ws.view.set_zoom(z, 600.0, 300.0);
            assert!(ws.view.zoom >= 0.01 && ws.view.zoom <= 100.0);
            let b = ws.view.get_render_bounds();
            assert!(b.width.is_finite() && b.height.is_finite());
            let (ew, eh) = ws.view.environment_size();
            ws.view.set_view_position(ew, eh);
            assert!(ws.view.viewport_x <= ew && ws.view.viewport_y <= eh);
        }
        assert_eq!(ws.view.composition.width(), 8000);
    }

    #[test]
    fn create_blank_document_has_white_background_and_null_memento() {
        let app = app_with_doc(64, 48, (800.0, 600.0));
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.document.layers.size(), 1);
        assert!(
            ws.document
                .layers
                .get_at(0)
                .unwrap()
                .properties
                .is_background
        );
        assert_eq!(ws.history.undo.len(), 1);
        assert!(!ws.dirty);
        assert_eq!(ws.view.composition.pixel(10, 10), [255, 255, 255, 255]);
    }

    #[test]
    fn layer_ops_fix_up_active_layer_and_recomposite() {
        use crate::history::functions;
        let mut app = app_with_doc(16, 16, (800.0, 600.0));
        let ws = app.active_ws_mut().unwrap();
        let rev = ws.view.composition_revision;
        assert_eq!(
            ws.execute_function(functions::add_new_blank_layer),
            crate::history::FunctionResult::Success
        );
        assert_eq!((ws.document.layers.size(), ws.active_layer), (2, 1));
        assert!(ws.dirty && ws.view.composition_revision > rev);
        ws.execute_function(functions::add_new_blank_layer);
        assert_eq!(ws.active_layer, 2);
        // JS onLayerRemoving: deleting layer 2 activates its lower neighbour.
        ws.execute_function(functions::delete_layer);
        assert_eq!(ws.active_layer, 1);
        // Undo of the delete re-inserts at 2 and activates it (onLayerInserted).
        assert!(ws.undo());
        assert_eq!((ws.document.layers.size(), ws.active_layer), (3, 2));
        // Moving the active layer: the JS reference follows the swapped layer.
        ws.execute_function(|t| functions::swap_layer(t, 2, 1));
        assert_eq!(ws.active_layer, 1);
        assert!(
            app.drain_events()
                .iter()
                .any(|e| matches!(e, AppEvent::DocumentInvalidated { .. }))
        );
    }

    #[test]
    fn document_state_undo_restores_view_and_captures_current() {
        use crate::document::Document;
        use crate::history::{DocumentStateMemento, HistoryMemento, MementoKind};
        let mut app = app_with_doc(32, 32, (800.0, 600.0));
        let ws = app.active_ws_mut().unwrap();
        ws.view.set_zoom_center(3.0);
        let before = (ws.view.zoom, ws.view.viewport_x);
        // A DocumentState memento of a 16x16 document, as a resize would record it.
        let mut small = Document::new(16, 16, 96.0);
        small.add_layer(crate::document::layer::Layer::create_background_layer(
            16, 16,
        ));
        let memento = HistoryMemento::new(
            "Resize",
            "",
            MementoKind::DocumentState(Box::new(DocumentStateMemento {
                document: small,
                active_layer: 0,
                selection: Default::default(),
                zoom: 0.5,
                zoom_to_window: false,
                viewport_x: 7.0,
                viewport_y: 9.0,
            })),
        );
        ws.history.push_new_memento(memento);
        // Pretend the document was replaced after the memento was made: undo restores it.
        assert!(ws.undo());
        assert_eq!((ws.document.width, ws.view.composition.width()), (16, 16));
        assert_eq!(ws.view.zoom, 0.5);
        match &ws.history.redo.front().unwrap().kind {
            MementoKind::DocumentState(m) => {
                assert_eq!((m.zoom, m.viewport_x), before);
                assert_eq!(m.document.width, 32);
            }
            _ => panic!(),
        }
        assert!(ws.redo());
        assert_eq!((ws.document.width, ws.view.zoom), (32, before.0));
    }

    #[test]
    fn to_unit_and_close_workspace() {
        let mut app = app_with_doc(96, 96, (800.0, 600.0));
        assert_eq!(app.to_unit(96.0), UnitValue::Pixels(96.0));
        app.measurement_unit = MeasurementUnit::Inch;
        assert_eq!(app.to_unit(96.0), UnitValue::Formatted("1.00".into()));
        // A non-initial new document replaces the untouched initial one.
        let i = app.create_blank_document_in_new_workspace(8, 8, 96.0, false);
        assert_eq!((app.documents.len(), i, app.active), (1, 0, Some(0)));
        assert!(app.close_document_workspace(0));
        assert!(app.active_ws().is_none());
    }

    #[test]
    fn bitmap_undo_recomposites_only_the_chunk() {
        use crate::history::{BitmapMemento, HistoryMemento, MementoKind};
        use crate::util::region::Region;
        let mut app = app_with_doc(4000, 4000, (800.0, 600.0));
        let ws = app.active_ws_mut().unwrap();
        let rect = Rectangle::new(10.0, 10.0, 20.0, 20.0);
        let chunk =
            BitmapMemento::capture(&ws.document, 0, Some(&Region::from_rectangles(vec![rect])));
        ws.push_memento(HistoryMemento::new("Draw", "", MementoKind::Bitmap(chunk)));
        // Mark a pixel outside the chunk in the composition; a full re-composite would erase it.
        ws.view
            .composition
            .fill_rect(crate::pixel::IntRect::new(100, 100, 1, 1), [255, 0, 0, 255]);
        ws.view.events.clear();
        assert!(ws.undo());
        assert_eq!(ws.view.composition.pixel(100, 100), [255, 0, 0, 255]);
        let rects: Vec<_> = ws
            .view
            .events
            .iter()
            .filter_map(|e| match e {
                AppEvent::DocumentInvalidated { rects } => Some(rects.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(rects, vec![vec![rect]]);
    }

    #[test]
    fn fit_to_window_centres_and_keeps_raw_viewport() {
        let mut app = app_with_doc(800, 600, (1249.0, 615.0));
        let ws = app.active_ws_mut().unwrap();
        ws.view.set_zoom_to_window(true);
        let (vw, vh) = (1249.0, 615.0);
        let b = ws.view.get_render_bounds();
        assert!((b.x - (vw - b.width) / 2.0).abs() < 1e-9);
        assert!((b.y - (vh - b.height) / 2.0).abs() < 1e-9);
        // Leaving Fit to Window keeps the document where it was.
        ws.view.set_zoom_to_window(false);
        let a = ws.view.get_render_bounds();
        assert!((a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6);
    }

    #[test]
    fn swapped_document_keeps_restored_active_layer() {
        use crate::document::{Document, layer::Layer};
        use crate::history::{DocumentStateMemento, HistoryMemento, MementoKind};
        let mut app = app_with_doc(32, 32, (800.0, 600.0));
        let ws = app.active_ws_mut().unwrap();
        let mut doc = Document::new(32, 32, 96.0);
        doc.add_layer(Layer::create_background_layer(32, 32));
        doc.add_layer(Layer::create_layer(32, 32, "b"));
        doc.add_layer(Layer::create_layer(32, 32, "c"));
        ws.history.push_new_memento(HistoryMemento::new(
            "Flatten",
            "",
            MementoKind::DocumentState(Box::new(DocumentStateMemento {
                document: doc,
                active_layer: 1,
                selection: Default::default(),
                zoom: 1.0,
                zoom_to_window: false,
                viewport_x: 0.0,
                viewport_y: 0.0,
            })),
        ));
        assert!(ws.undo());
        assert_eq!((ws.document.layers.size(), ws.active_layer), (3, 1));
        assert!(ws.redo());
        assert_eq!((ws.document.layers.size(), ws.active_layer), (1, 0));
    }

    #[test]
    fn initial_document_fits_the_app_view_and_scroll_snaps_half_up() {
        // #view client 1265x443 at ppp 1.25 (Fit uses the app's real scale) -> zoom
        // (443·1.25 − 9·1.25)/600, centered at 632.5/221.5; Chromium's scrollLeft/Top read back
        // 633/222.
        let mut app = AppWorkspace {
            view_width: 1265.0,
            view_height: 443.0,
            pixels_per_point: 1.25,
            ..Default::default()
        };
        let i = app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
        let v = &mut app.documents[i].view;
        assert_eq!(v.zoom, (443.0 * 1.25 - 9.0 * 1.25) / 600.0);
        assert_eq!((v.viewport_x, v.viewport_y), (632.5, 221.5));
        assert_eq!(v.dom_scroll_position(), (633.0, 222.0));
        assert!(app.has_events());
        app.drain_events();
        assert!(!app.has_events());
        let v = &mut app.documents[i].view;
        let (x, y) = v.dom_scroll_position();
        v.set_view_position(x, y);
        let render_w = 800.0 * v.zoom / 1.25;
        assert_eq!(v.get_render_bounds().x, 1265.0 - render_w / 2.0 - 633.0);
        v.set_view_position(-3.0, 1e9);
        let (_, env_h) = v.environment_size();
        assert_eq!(v.dom_scroll_position(), (0.0, env_h - 443.0));
    }

    #[test]
    fn first_update_canvas_bounds_shifts_by_zero() {
        use crate::document::view::DocumentView;
        let mut v = DocumentView::new(100, 100);
        v.update_canvas_bounds(1000.0, 700.0, true);
        assert_eq!(v.viewport_x, 0.0);
        v.update_canvas_bounds(1200.0, 700.0, true);
        assert_eq!(v.viewport_x, 100.0);
    }

    /// ui-drawn-graphics.md §2.10 and §3 "Status, 96 dpi": two decimals, ties away from zero on
    /// the exact value, no grouping; sizes with U+00D7.
    #[test]
    fn status_number_formats() {
        assert_eq!(format_fixed2(12.0 / 96.0), "0.13");
        assert_eq!(format_fixed2(60.0 / 96.0), "0.63");
        assert_eq!(format_fixed2(96.0 * 2.54 / 96.0), "2.54");
        assert_eq!(format_fixed2(2.54 / 96.0), "0.03");
        assert_eq!(format_fixed2(1234.5), "1234.50");
        assert_eq!(format_fixed2(-0.125), "-0.13");
        assert_eq!(format_fixed2(0.0049), "0.00");
        assert_eq!(format_size("800", "600"), "800 × 600");
        // The pixel-to-physical conversion multiplies first: 120·2.54/96 = 3.175 is a tie that
        // rounds up (120/96·2.54 lands below it, "3.17").
        let mut app = app_with_doc(8, 8, (100.0, 100.0));
        app.measurement_unit = MeasurementUnit::Centimeter;
        assert_eq!(app.to_unit(120.0), UnitValue::Formatted("3.18".into()));
    }

    /// A tool that switches tools from inside its own hook still gets `on_deactivate` (deferred
    /// until the hook returns); the new tool stays. Text is now installed as well as Pan.
    #[test]
    fn tool_switch_from_a_hook_deactivates_the_old_tool() {
        use crate::tool::{PointerInput, PointerType, Tool};
        use std::{cell::Cell, rc::Rc};
        struct Switcher(Rc<Cell<u32>>, ToolType);
        impl Tool for Switcher {
            fn tool_type(&self) -> ToolType {
                ToolType::PaintBrush
            }
            fn on_deactivate(&mut self, _: &mut AppWorkspace) {
                self.0.set(self.0.get() + 1);
            }
            fn on_mouse_down(
                &mut self,
                app: &mut AppWorkspace,
                _: f64,
                _: f64,
                _: crate::tool::MouseButton,
                _: &PointerInput,
            ) -> bool {
                app.set_active_tool_from_type(self.1);
                true
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        for target in [ToolType::Pan, ToolType::Text] {
            let mut app = app_with_doc(8, 8, (100.0, 100.0));
            let count = Rc::new(Cell::new(0));
            app.tool = Some(Box::new(Switcher(count.clone(), target)));
            app.tool_type = Some(ToolType::PaintBrush);
            app.set_active_document_workspace(Some(0));
            assert!(app.on_mouse_down(1.0, 1.0, crate::tool::MouseButton::Left, &input));
            assert_eq!(count.get(), 1);
            assert_eq!(app.tool.as_ref().map(|t| t.tool_type()), Some(target));
            assert_eq!(app.tool_type, Some(target));
            assert!(app.deferred_deactivations.is_empty());
        }
    }

    /// ui-drawn-graphics.md §2.10 "Zoom text": ⌊100·ratio⌉ %.
    #[test]
    fn zoom_text_rounds_half_away() {
        assert_eq!(
            [1.0, 2.0 / 3.0, 1.2544, 1.0 / 1.12, 0.01, 1.5].map(format_zoom),
            ["100%", "67%", "125%", "89%", "1%", "150%"]
        );
    }
}
