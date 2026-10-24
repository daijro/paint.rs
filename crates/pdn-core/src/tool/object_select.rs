//! Photocraft phase spec: S§4.3 "Object Selection"
//!
//! The Object Selection tool: PhotoCraft's GrabCut segmentation (`extra::segment`) behind
//! Paint.NET's selection combine modes and modifiers (Lasso Select's Ctrl/Alt × button).
//!
//! - A drag draws a box, as Rectangle Select's marquee; on release the object inside it is
//!   selected (`grabcut::object_in_rect`).
//! - Object Finder: while the pointer rests on the canvas with no button down, the object under
//!   it is looked up (`grabcut::object_at`, at most every 150 ms; the last 12 objects and 64
//!   misses kept while the sampled pixels stay the same) and shown as a 50 % blue fill with an
//!   opaque 1-px outline; a click without a drag selects it.
//! - Select Subject (the toolbar's action) selects the main subject (`subject::select_subject`).
//!
//! Every lookup runs as a background job (`extra::job`) on the active layer or, with Sample:
//! Image, the image. Its coverage ≥ 128 becomes a pixel-edged path, selected by Lasso Select's
//! release rules as one "Object Selection" history item. When the algorithm finds nothing, the
//! selection and history stay as they were and the status bar says so (upstream's `apply`).

use std::sync::Arc;

use serde_json::Value;

use super::selection_tools::{RectMode, points_per_px, rectangle_select_rect, select_path};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::canvas::ImageData;
use crate::extra::geom::Rect;
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::job::Job;
use crate::extra::sample::layer_or_image;
use crate::extra::segment::grabcut::{self, OBJECT_AT_WORK_PX};
use crate::extra::segment::{Region, Sampler, subject};
use crate::pixel::IntRect;
use crate::render::nubs::Tint;
use crate::render::selection::SelectionRenderer;
use crate::render::{Renderer, RendererId};
use crate::selection::CombineMode;
use crate::selection::path::{GraphicsPath, path_from_stencil};
use crate::util::bitvec2d::BitVector2D;
use crate::util::geom::{Point, Rectangle};

/// Upstream's working budget (pixels) for the box (`select.object` with a `rect`).
const RECT_WORK_PX: usize = 160_000;
/// The Object Finder looks up at most this often (ms).
const FINDER_MS: f64 = 150.0;
/// Objects the Object Finder remembers.
const FOUND: usize = 12;
/// Background cells it remembers, apart from the objects so a pass over the background does not
/// evict them.
const MISSES: usize = 64;
/// The Object Finder's point cells (document px): a point looked up answers for its cell.
const CELL: f64 = 4.0;
/// The hover highlight's colour; the tint's mask carries the alpha (fill 128, outline 255).
const HIGHLIGHT: [u8; 4] = [0, 0, 255, 255];
/// Status-bar hints.
const NOT_FOUND: &str = "No object found.";
const EMPTY_LAYER: &str = "The active layer has no pixels here.";
/// A press becomes a drag once the pointer is this many screen points away (Windows' default
/// drag rectangle).
const DRAG_POINTS: f64 = 4.0;
/// Rectangle Select's cursors (idle, dragging, Ctrl, Alt): the tool has none of its own.
const CURSORS: [&str; 4] = [
    "rectangle_select_tool_cursor",
    "rectangle_select_tool_mouse_down_cursor",
    "rectangle_select_tool_plus_cursor",
    "rectangle_select_tool_minus_cursor",
];

/// The sampled pixels, for the jobs: straight RGBA8 over the canvas.
struct Snapshot(ImageData);

impl Sampler for Snapshot {
    fn rgba(&self, r: Rect) -> Vec<[f32; 4]> {
        let (w, h) = (self.0.width as i32, self.0.height as i32);
        let px = self.0.data.as_chunks::<4>().0;
        let mut out = Vec::with_capacity(r.width() as usize * r.height() as usize);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                out.push(if (0..w).contains(&x) && (0..h).contains(&y) {
                    px[y as usize * w as usize + x as usize].map(|v| f32::from(v) / 255.0)
                } else {
                    [0.0; 4]
                });
            }
        }
        out
    }
}

impl Snapshot {
    /// Some pixel of `r` (clipped to the canvas) is not fully transparent.
    fn has_pixels(&self, r: Rect) -> bool {
        let (w, h) = (self.0.width as i32, self.0.height as i32);
        let (x0, x1) = (r.x0.max(0), r.x1.min(w));
        (r.y0.max(0)..r.y1.min(h)).any(|y| {
            let row = (y * w) as usize * 4;
            x0 < x1
                && self.0.data[row + x0 as usize * 4..row + x1 as usize * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] > 0)
        })
    }
}

/// A press: where, where the pointer is now, the combine mode it set, and whether it dragged.
struct Press {
    at: [f64; 2],
    to: [f64; 2],
    mode: CombineMode,
    dragged: bool,
}

/// A lookup to select: what it finds lands in `mode` in document `doc`.
struct Running {
    job: Job<Option<Region>>,
    mode: CombineMode,
    doc: u64,
}

/// What the Object Finder knows at a point.
enum Known {
    Hit(Arc<Region>),
    /// Looked up before and found nothing.
    Miss,
    Unknown,
}

/// The Object Finder's lookups for one revision of the sampled pixels.
#[derive(Default)]
struct Finder {
    revision: u64,
    /// The objects found, oldest first.
    found: Vec<Arc<Region>>,
    /// The cells looked up that held no object, oldest first.
    misses: Vec<[i32; 2]>,
    /// The lookup running, with the revision and cell it answers for.
    job: Option<(u64, [i32; 2], Job<Option<Region>>)>,
    /// When the last lookup started (ms).
    last_ms: Option<f64>,
    /// The pointer rests where nothing was looked up yet.
    waiting: bool,
}

impl Finder {
    /// What is known at `p`: an object found covering it, else whether its cell was a miss.
    fn lookup(&self, p: [f64; 2]) -> Known {
        let (x, y) = (p[0].floor() as i32, p[1].floor() as i32);
        if let Some(r) = self.found.iter().find(|r| r.at(x, y) >= 0.5) {
            return Known::Hit(r.clone());
        }
        if self.misses.contains(&cell_of(p)) {
            Known::Miss
        } else {
            Known::Unknown
        }
    }

    /// A finished lookup joins what is known, if it read the current pixels.
    fn collect(&mut self) {
        let Some(result) = self.job.as_mut().and_then(|(_, _, j)| j.poll()) else {
            return;
        };
        let Some((revision, cell, _)) = self.job.take() else {
            return;
        };
        if revision != self.revision {
            return;
        }
        match result {
            Ok(Some(r)) => {
                if self.found.len() >= FOUND {
                    self.found.remove(0);
                }
                self.found.push(Arc::new(r));
            }
            Ok(None) => {
                if self.misses.len() >= MISSES {
                    self.misses.remove(0);
                }
                self.misses.push(cell);
            }
            Err(_) => {}
        }
    }
}

fn cell_of(p: [f64; 2]) -> [i32; 2] {
    [(p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32]
}

fn setting(app: &AppWorkspace, key: &str) -> Value {
    app.tool_settings.get(ToolType::ObjectSelection, key)
}

/// The toolbar's combine mode (the selection tools', Ruling R20).
fn configured_mode(app: &AppWorkspace) -> CombineMode {
    CombineMode::from_value(setting(app, "combineMode").as_str().unwrap_or(""))
}

/// The pixels with coverage ≥ 128 as a pixel-edged path (`path_from_stencil`, Magic Wand's).
fn region_path(r: &Region) -> GraphicsPath {
    let (w, h) = (r.bbox.width() as i32, r.bbox.height() as i32);
    let mut stencil = BitVector2D::new(w, h);
    for (b, &m) in stencil.bits_mut().iter_mut().zip(&r.mask) {
        *b = u8::from(m >= 128);
    }
    let mut path = path_from_stencil(&stencil, &Rectangle::new(0.0, 0.0, w as f64, h as f64));
    for p in path.polygons.iter_mut().flatten() {
        (p.x, p.y) = (p.x + r.bbox.x0 as f64, p.y + r.bbox.y0 as f64);
    }
    path
}

/// The highlight's mask over the region's box: coverage at half strength inside (alpha 128 at
/// full coverage), the pixels of coverage ≥ 0.5 on its edge opaque (upstream's `overlay_image`).
// ponytail: one document px wide, so thin at low zoom; scale it by the zoom if that reads badly.
fn outline_mask(r: &Region) -> Vec<u8> {
    let (w, h) = (r.bbox.width() as usize, r.bbox.height() as usize);
    let on = |x: usize, y: usize| r.mask.get(y * w + x).is_some_and(|&m| m >= 128);
    let mut out = Vec::with_capacity(r.mask.len());
    for y in 0..h {
        for x in 0..w {
            let edge = on(x, y)
                && (x == 0
                    || y == 0
                    || x + 1 == w
                    || y + 1 == h
                    || !on(x - 1, y)
                    || !on(x + 1, y)
                    || !on(x, y - 1)
                    || !on(x, y + 1));
            out.push(if edge {
                255
            } else {
                r.mask
                    .get(y * w + x)
                    .map_or(0, |&m| (m as u16).div_ceil(2) as u8)
            });
        }
    }
    out
}

#[derive(Default)]
pub struct ObjectSelectionTool {
    press: Option<Press>,
    running: Option<Running>,
    finder: Finder,
    /// The sampled surface's `revision` and its pixels, kept while they stay the same.
    snapshot: Option<(u64, Arc<Snapshot>)>,
    /// The pointer (document px), once it moved over the canvas.
    pointer: Option<[f64; 2]>,
    /// The object highlighted.
    shown: Option<Arc<Region>>,
    /// The box's layer and the highlight's, while active.
    marquee: Option<RendererId>,
    tint: Option<RendererId>,
    /// Index into [`CURSORS`].
    cursor: usize,
    /// The status bar's hint (`Tool::status_text`).
    hint: Option<&'static str>,
}

impl ObjectSelectionTool {
    /// The active layer's pixels, or the image's with Sample: Image, and their revision.
    fn sampled(&mut self, app: &AppWorkspace) -> Option<(u64, Arc<Snapshot>)> {
        let ws = app.active_ws()?;
        let image = setting(app, "sampleImage").as_bool().unwrap_or(false);
        let revision = if image {
            ws.view.composition.revision()
        } else {
            ws.document
                .layers
                .get_at(ws.active_layer)?
                .surface
                .revision()
        };
        if self.snapshot.as_ref().is_none_or(|(r, _)| *r != revision) {
            let (w, h) = (ws.document.width as i32, ws.document.height as i32);
            let pixels = layer_or_image(app, image, Rect::new(0, 0, w, h));
            self.snapshot = Some((revision, Arc::new(Snapshot(pixels))));
        }
        self.snapshot.clone()
    }

    /// With Sample: Layer, the layer has no pixels in `area`: says so and returns true.
    fn sampled_empty(&mut self, app: &AppWorkspace, area: Rect) -> bool {
        if setting(app, "sampleImage").as_bool().unwrap_or(false) {
            return false;
        }
        let empty = self.sampled(app).is_some_and(|(_, s)| !s.has_pixels(area));
        if empty {
            self.hint = Some(EMPTY_LAYER);
        }
        empty
    }

    /// The pixel under `p`.
    fn pixel_at(p: [f64; 2]) -> Rect {
        let (x, y) = (p[0].floor() as i32, p[1].floor() as i32);
        Rect::new(x, y, x + 1, y + 1)
    }

    /// Runs `find` over the sampled pixels as a job whose result is selected in `mode`.
    fn start(
        &mut self,
        app: &mut AppWorkspace,
        mode: CombineMode,
        find: impl FnOnce(&Snapshot, Rect, &Interrupt) -> Result<Option<Region>, Cancelled>
        + Send
        + 'static,
    ) {
        let (Some((_, pixels)), Some(doc)) =
            (self.sampled(app), app.active_ws().map(|ws| ws.document.id))
        else {
            return;
        };
        let canvas = Rect::new(0, 0, pixels.0.width as i32, pixels.0.height as i32);
        let job = Job::spawn(move |ctl| find(&pixels, canvas, ctl));
        self.running = Some(Running { job, mode, doc });
        self.hover(app);
        // On wasm the job already ran: it lands now.
        self.poll(app);
    }

    /// A finished job's object is selected, in the document it was found in.
    fn poll(&mut self, app: &mut AppWorkspace) {
        let Some(result) = self.running.as_mut().and_then(|r| r.job.poll()) else {
            return;
        };
        let Some(r) = self.running.take() else {
            return;
        };
        if let (Ok(region), Some(ws)) = (result, app.active_ws_mut())
            && ws.document.id == r.doc
        {
            // Nothing found leaves the selection and history alone (upstream's `apply`).
            match region {
                Some(region) => {
                    select_path(ws, region_path(&region), r.mode, ToolType::ObjectSelection)
                }
                None => self.hint = Some(NOT_FOUND),
            }
        }
    }

    /// Esc, a tool switch, an undo: a running job stops and selects nothing, a press is dropped.
    fn cancel(&mut self, app: &mut AppWorkspace) {
        if let Some(r) = self.running.take() {
            r.job.cancel();
        }
        self.press = None;
        self.hint = None;
        self.show_box(app);
    }

    /// The press's box (Rectangle Select's, in the document).
    fn drag_box(app: &AppWorkspace, p: &Press) -> Rectangle {
        let floor = |q: [f64; 2]| Point::new(q[0].floor(), q[1].floor());
        let doc = app
            .active_ws()
            .map_or((0, 0), |ws| (ws.document.width, ws.document.height));
        rectangle_select_rect(floor(p.at), floor(p.to), doc, RectMode::Normal, false)
    }

    /// The box being dragged, on the marquee layer.
    fn show_box(&self, app: &mut AppWorkspace) {
        let shape = self.press.as_ref().filter(|p| p.dragged);
        let path = shape.map(|p| GraphicsPath::from_rect(&Self::drag_box(app, p)));
        if let (Some(id), Some(ws)) = (self.marquee, app.active_ws_mut())
            && let Some(Renderer::Selection(r)) = ws.surface_box.renderer_mut(id)
        {
            r.visible = path.is_some();
            r.path = Some(path.unwrap_or_default());
        }
    }

    /// Object Finder: the object under the resting pointer, highlighted.
    fn hover(&mut self, app: &mut AppWorkspace) {
        let on = setting(app, "objectFinder").as_bool().unwrap_or(true);
        let idle = self.press.is_none() && self.running.is_none() && app.zoom_anchor.is_some();
        // A finished lookup is collected even while nothing is looked up, so the ticks stop.
        self.finder.collect();
        self.finder.waiting = false;
        let found = match self.pointer.filter(|_| on && idle) {
            Some(p) => {
                if self.hint == Some(EMPTY_LAYER) {
                    self.hint = None;
                }
                self.find(app, p)
            }
            None => {
                // No pointer, or the Finder is off: its empty-layer hint goes.
                if (!on || self.pointer.is_none()) && self.hint == Some(EMPTY_LAYER) {
                    self.hint = None;
                }
                None
            }
        };
        self.highlight(app, found);
    }

    /// The object known at `p`; when none is, a lookup starts (at most every 150 ms).
    fn find(&mut self, app: &AppWorkspace, p: [f64; 2]) -> Option<Arc<Region>> {
        let (revision, pixels) = self.sampled(app)?;
        let canvas = Rect::new(0, 0, pixels.0.width as i32, pixels.0.height as i32);
        // The grey margin outside the document has no layer to be empty.
        if !canvas.contains(p[0].floor() as i32, p[1].floor() as i32) {
            return None;
        }
        if self.sampled_empty(app, Self::pixel_at(p)) {
            return None;
        }
        let f = &mut self.finder;
        if f.revision != revision {
            f.revision = revision;
            f.found.clear();
            f.misses.clear();
        }
        match f.lookup(p) {
            Known::Hit(r) => return Some(r),
            Known::Miss => return None,
            Known::Unknown => {}
        }
        f.waiting = true;
        let due = f.last_ms.is_none_or(|t| app.now_ms - t >= FINDER_MS);
        if f.job.is_none() && due {
            let (x, y) = (p[0] as f32, p[1] as f32);
            let job = Job::spawn(move |_| {
                Ok(grabcut::object_at(
                    &*pixels,
                    canvas,
                    x,
                    y,
                    OBJECT_AT_WORK_PX,
                ))
            });
            f.job = Some((revision, cell_of(p), job));
            f.last_ms = Some(app.now_ms);
        }
        None
    }

    /// Tints `region` (nothing for `None`).
    fn highlight(&mut self, app: &mut AppWorkspace, region: Option<Arc<Region>>) {
        let same = match (&self.shown, &region) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };
        if same {
            return;
        }
        let tint = region.as_deref().map_or_else(Tint::default, |r| Tint {
            rect: IntRect::new(r.bbox.x0, r.bbox.y0, r.bbox.width(), r.bbox.height()),
            mask: Arc::new(outline_mask(r)),
            color: HIGHLIGHT,
            pixels: None,
        });
        self.shown = region;
        if let (Some(id), Some(ws)) = (self.tint, app.active_ws_mut())
            && let Some(Renderer::Tint(t)) = ws.surface_box.renderer_mut(id)
        {
            *t = tint;
        }
    }

    /// A press without a drag: the highlighted object, or the one there once found; with the
    /// Object Finder off, nothing (Rectangle Select's click). A click on a cached miss looks
    /// again (upstream's `object_at_point` reuses only found objects).
    fn click(&mut self, app: &mut AppWorkspace, p: [f64; 2], mode: CombineMode) {
        if !setting(app, "objectFinder").as_bool().unwrap_or(true) {
            if let Some(ws) = app.active_ws_mut() {
                select_path(ws, GraphicsPath::default(), mode, ToolType::ObjectSelection);
            }
            return;
        }
        if self.sampled_empty(app, Self::pixel_at(p)) {
            return;
        }
        let current = self.sampled(app).map(|(r, _)| r) == Some(self.finder.revision);
        match self.finder.lookup(p) {
            Known::Hit(r) if current => {
                if let Some(ws) = app.active_ws_mut() {
                    select_path(ws, region_path(&r), mode, ToolType::ObjectSelection);
                }
            }
            _ => {
                let (x, y) = (p[0] as f32, p[1] as f32);
                self.start(app, mode, move |s, canvas, _| {
                    Ok(grabcut::object_at(s, canvas, x, y, OBJECT_AT_WORK_PX))
                });
            }
        }
    }

    fn update_cursor(&mut self, app: &AppWorkspace) {
        self.cursor = if self.press.is_some() {
            1
        } else if app.control_key_down {
            2
        } else if app.alt_key_down {
            3
        } else {
            0
        };
    }
}

impl Tool for ObjectSelectionTool {
    fn tool_type(&self) -> ToolType {
        ToolType::ObjectSelection
    }

    /// The workspace layer tints while a selection tool is active; a marquee layer shows the box
    /// and a tint the object found.
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        self.update_cursor(app);
        if let Some(ws) = app.active_ws_mut() {
            ws.selection_renderer_mut().tinting = true;
            let marquee = SelectionRenderer {
                visible: false,
                path: Some(GraphicsPath::default()),
                ..Default::default()
            };
            self.marquee = Some(ws.surface_box.add_renderer(Renderer::Selection(marquee)));
            self.tint = Some(ws.surface_box.add_renderer(Renderer::Tint(Tint::default())));
        }
    }

    /// Every commit-first command (a tool, layer or document change …) stops a running job and
    /// drops the press.
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        self.cancel(app);
        self.shown = None;
        if let Some(ws) = app.active_ws_mut() {
            ws.selection_renderer_mut().tinting = false;
            for id in [self.marquee.take(), self.tint.take()]
                .into_iter()
                .flatten()
            {
                ws.surface_box.remove_renderer(id);
            }
        }
    }

    /// Lasso Select's modifiers set the combine mode; presses while a job runs are swallowed.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if self.running.is_some() || self.press.is_some() {
            return true;
        }
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        let mode = match (b, app.control_key_down, app.alt_key_down) {
            (MouseButton::Left, true, _) => CombineMode::Union,
            (MouseButton::Left, _, true) => CombineMode::Exclude,
            (MouseButton::Right, true, _) => CombineMode::Xor,
            (MouseButton::Right, _, true) => CombineMode::Intersect,
            _ => configured_mode(app),
        };
        self.hint = None;
        self.press = Some(Press {
            at: [x, y],
            to: [x, y],
            mode,
            dragged: false,
        });
        self.pointer = Some([x, y]);
        self.hover(app);
        self.update_cursor(app);
        true
    }

    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        self.pointer = Some([x, y]);
        let reach = DRAG_POINTS / points_per_px(app);
        if let Some(p) = &mut self.press {
            p.to = [x, y];
            p.dragged |= (x - p.at[0]).hypot(y - p.at[1]) >= reach;
            self.show_box(app);
        } else {
            self.hover(app);
        }
        self.press.is_some()
    }

    /// The release ends the press: a drag selects the object in its box, a click the one there.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        _b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if self.press.is_none() {
            return false;
        }
        self.on_mouse_move(app, x, y, i);
        let Some(p) = self.press.take() else {
            return false;
        };
        self.show_box(app);
        if p.dragged {
            let b = Self::drag_box(app, &p);
            let r = Rect::new(
                b.left() as i32,
                b.top() as i32,
                b.right() as i32,
                b.bottom() as i32,
            );
            if !self.sampled_empty(app, r) {
                self.start(app, p.mode, move |s, canvas, _| {
                    Ok(grabcut::object_in_rect(s, r, canvas, RECT_WORK_PX))
                });
            }
        } else {
            self.click(app, p.at, p.mode);
        }
        self.hover(app);
        self.update_cursor(app);
        true
    }

    /// Esc stops a running job or drops the box being dragged.
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        if key.key != "Escape" || (self.running.is_none() && self.press.is_none()) {
            return false;
        }
        self.cancel(app);
        self.update_cursor(app);
        true
    }

    /// Select Subject: the main subject, in the toolbar's combine mode.
    fn on_action(&mut self, app: &mut AppWorkspace, key: &str) {
        if key != "selectSubject" || self.running.is_some() || self.press.is_some() {
            return;
        }
        self.hint = None;
        let whole = app
            .active_ws()
            .map(|ws| Rect::new(0, 0, ws.document.width as i32, ws.document.height as i32));
        if whole.is_none_or(|r| self.sampled_empty(app, r)) {
            return;
        }
        let mode = configured_mode(app);
        // ponytail: upstream's 110 k working pixels, not the 4 MP cap; raise it if subjects come
        // out too coarse on large photos.
        self.start(app, mode, |s, canvas, ctl| {
            subject::select_subject(s, canvas, subject::WORK_PX, ctl)
        });
    }

    fn on_executing_history_memento(&mut self, app: &mut AppWorkspace) {
        self.cancel(app);
    }

    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        self.update_cursor(app);
    }

    /// A finished job lands; the highlight follows the pointer, the pixels and the settings.
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.poll(app);
        self.hover(app);
    }

    fn status_text(&self) -> Option<&str> {
        self.hint
    }

    fn needs_ticks(&self) -> bool {
        self.running.is_some() || self.finder.job.is_some() || self.finder.waiting
    }

    fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(CURSORS[self.cursor])
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppWorkspace;
    use crate::render::Renderer;
    use crate::selection::CombineMode;
    use crate::testing::{Ev, drive_tool, fresh_doc, selection_mask};
    use crate::tool::MouseButton::Left;
    use crate::util::geom::Rectangle;
    use serde_json::json;

    const T: ToolType = ToolType::ObjectSelection;
    const DISC: f64 = std::f64::consts::PI * 20.0 * 20.0;

    /// A red disc (radius 20, centre (100, 75)) on a flat blue 200×150 layer, 100 % zoom.
    fn disc_doc() -> AppWorkspace {
        let mut app = fresh_doc(200, 150, None);
        let ws = app.active_ws_mut().unwrap();
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        let px = layer.surface.data_mut().as_chunks_mut::<4>().0;
        for (i, p) in px.iter_mut().enumerate() {
            let (dx, dy) = (
                (i % 200) as f64 + 0.5 - 100.0,
                (i / 200) as f64 + 0.5 - 75.0,
            );
            *p = if dx.hypot(dy) <= 20.0 {
                [220, 50, 40, 255]
            } else {
                [60, 110, 180, 255]
            };
        }
        ws.invalidate(None);
        app
    }

    /// Runs frames until the tool's jobs and lookups are done.
    fn settle(app: &mut AppWorkspace) {
        let start = std::time::Instant::now();
        while app.tool_ticking() {
            assert!(start.elapsed().as_secs() < 60, "the job never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.on_frame(app.now_ms + 16.0);
        }
    }

    /// The selected area (coverage-weighted pixels).
    fn area(app: &AppWorkspace) -> f64 {
        let mask = selection_mask(app, 200, 150);
        mask.iter().map(|&v| f64::from(v) / 255.0).sum()
    }

    /// The pixel `(x, y)` is selected.
    fn selected(app: &AppWorkspace, x: usize, y: usize) -> bool {
        selection_mask(app, 200, 150)[y * 200 + x] >= 128
    }

    fn undo_names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.history.undo[0].name, "New Image");
        ws.history.undo[1..]
            .iter()
            .map(|m| m.name.clone())
            .collect()
    }

    fn drag(app: &mut AppWorkspace, from: (f64, f64), to: (f64, f64), mods: &'static str) {
        let events = [
            Ev::DownMods(from.0, from.1, Left, mods),
            Ev::Move((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0),
            Ev::Move(to.0, to.1),
            Ev::Up(to.0, to.1, Left),
        ];
        drive_tool(app, T, &events);
        settle(app);
    }

    /// The hover highlight shows.
    fn highlighted(app: &AppWorkspace) -> bool {
        let ws = app.active_ws().unwrap();
        ws.surface_box
            .renderers
            .iter()
            .any(|(_, r)| matches!(r, Renderer::Tint(t) if !t.rect.is_empty()))
    }

    /// S§4.3: a box dragged around the disc selects it.
    #[test]
    fn rect_drag_selects_the_disc() {
        let mut app = disc_doc();
        drag(&mut app, (70.0, 45.0), (130.0, 105.0), "");
        let a = area(&app);
        assert!((a - DISC).abs() <= 0.05 * DISC, "{a} vs {DISC}");
        assert!(selected(&app, 100, 75) && !selected(&app, 75, 50));
    }

    /// S§4.3 Object Finder: resting over the disc highlights it; a click selects it at once.
    #[test]
    fn click_selects_the_hovered_object() {
        let mut app = disc_doc();
        app.zoom_anchor = Some((100.0, 75.0));
        drive_tool(&mut app, T, &[Ev::Move(100.0, 75.0)]);
        settle(&mut app);
        assert!(highlighted(&app));
        drive_tool(
            &mut app,
            T,
            &[Ev::Down(100.0, 75.0, Left), Ev::Up(100.0, 75.0, Left)],
        );
        // The highlighted object needs no second lookup.
        assert!(!app.tool_busy());
        let a = area(&app);
        assert!((a - DISC).abs() <= 0.05 * DISC, "{a} vs {DISC}");
        assert_eq!(undo_names(&app), ["Object Selection"]);
        // The background highlights nothing, and a click there finds nothing (R53): the
        // selection and history stay, and the status bar says so.
        let kept = app.active_ws().unwrap().selection.save();
        drive_tool(&mut app, T, &[Ev::Move(10.0, 10.0)]);
        settle(&mut app);
        assert!(!highlighted(&app));
        click(&mut app, (10.0, 10.0));
        assert_eq!(app.active_ws().unwrap().selection.save(), kept);
        assert_eq!(undo_names(&app), ["Object Selection"]);
        assert_eq!(hint(&mut app).as_deref(), Some("No object found."));
    }

    /// A click with the Object Finder off deselects, like Paint.NET's selection click.
    #[test]
    fn click_without_the_finder_deselects() {
        let mut app = disc_doc();
        app.update_tool_setting(T, "objectFinder", json!(false));
        drag(&mut app, (70.0, 45.0), (130.0, 105.0), "");
        click(&mut app, (10.0, 10.0));
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert_eq!(undo_names(&app), ["Object Selection", "Deselect"]);
    }

    fn click(app: &mut AppWorkspace, at: (f64, f64)) {
        drive_tool(
            app,
            T,
            &[Ev::Down(at.0, at.1, Left), Ev::Up(at.0, at.1, Left)],
        );
        settle(app);
    }

    fn hint(app: &mut AppWorkspace) -> Option<String> {
        app.with_tool(|t, _| t.status_text().map(str::to_owned))
            .flatten()
    }

    /// A flat blue opaque layer: nothing to find.
    fn flat_doc() -> AppWorkspace {
        let mut app = fresh_doc(200, 150, None);
        let ws = app.active_ws_mut().unwrap();
        let px = ws.document.layers.get_at_mut(0).unwrap().surface.data_mut();
        px.as_chunks_mut::<4>().0.fill([60, 110, 180, 255]);
        ws.invalidate(None);
        app
    }

    /// A 10×10 selection in the corner, to see that a failed search keeps it.
    fn select_corner(app: &mut AppWorkspace) -> Vec<u8> {
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(5.0, 5.0, 10.0, 10.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        selection_mask(app, 200, 150)
    }

    /// R53: Select Subject finding nothing keeps the selection and history.
    #[test]
    fn select_subject_nothing_found_keeps_the_selection() {
        let mut app = flat_doc();
        let before = select_corner(&mut app);
        drive_tool(&mut app, T, &[]);
        app.with_tool(|t, app| t.on_action(app, "selectSubject"));
        settle(&mut app);
        assert_eq!(selection_mask(&app, 200, 150), before);
        assert_eq!(undo_names(&app), Vec::<String>::new());
        assert_eq!(hint(&mut app).as_deref(), Some("No object found."));
    }

    /// R53: a box around nothing keeps the selection and history, in every mode.
    #[test]
    fn empty_box_keeps_the_selection() {
        let mut app = flat_doc();
        let before = select_corner(&mut app);
        drag(&mut app, (60.0, 40.0), (140.0, 110.0), "");
        assert_eq!(selection_mask(&app, 200, 150), before);
        assert_eq!(undo_names(&app), Vec::<String>::new());
        assert_eq!(hint(&mut app).as_deref(), Some("No object found."));
    }

    /// Sample: Layer on a layer without pixels there says so and changes nothing.
    #[test]
    fn empty_layer_says_so() {
        let mut app = fresh_doc(200, 150, None);
        let ws = app.active_ws_mut().unwrap();
        ws.document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .data_mut()
            .fill(0);
        ws.invalidate(None);
        let before = select_corner(&mut app);
        drag(&mut app, (60.0, 40.0), (140.0, 110.0), "");
        assert_eq!(
            hint(&mut app).as_deref(),
            Some("The active layer has no pixels here.")
        );
        click(&mut app, (100.0, 75.0));
        assert_eq!(
            hint(&mut app).as_deref(),
            Some("The active layer has no pixels here.")
        );
        assert!(!app.tool_busy());
        app.with_tool(|t, app| t.on_action(app, "selectSubject"));
        assert!(!app.tool_busy());
        assert_eq!(selection_mask(&app, 200, 150), before);
        assert_eq!(undo_names(&app), Vec::<String>::new());
    }

    /// Ruling R20: the selection tools' Union, set on the toolbar, adds the object to the
    /// selection.
    #[test]
    fn union_mode_adds_to_existing_selection() {
        let mut app = disc_doc();
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(5.0, 5.0, 10.0, 10.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        app.update_tool_setting(ToolType::LassoSelect, "combineMode", json!("union"));
        drag(&mut app, (70.0, 45.0), (130.0, 105.0), "");
        let a = area(&app);
        assert!((a - DISC - 100.0).abs() <= 0.05 * DISC, "{a}");
        assert!(selected(&app, 7, 7) && selected(&app, 100, 75));
    }

    /// Review Focus 3: one "Object Selection" history item; undo restores the selection.
    #[test]
    fn object_selection_is_one_history_item() {
        let mut app = disc_doc();
        let before = app.active_ws().unwrap().selection.save();
        drag(&mut app, (70.0, 45.0), (130.0, 105.0), "");
        assert_eq!(undo_names(&app), ["Object Selection"]);
        assert_ne!(app.active_ws().unwrap().selection.save(), before);
        assert!(app.undo());
        assert_eq!(app.active_ws().unwrap().selection.save(), before);
    }

    /// S§4.3: Select Subject selects the disc in the middle of the image.
    #[test]
    fn select_subject_selects_the_centre_object() {
        let mut app = disc_doc();
        drive_tool(&mut app, T, &[]);
        app.with_tool(|t, app| t.on_action(app, "selectSubject"));
        settle(&mut app);
        assert!(selected(&app, 100, 75) && !selected(&app, 5, 5));
        let a = area(&app);
        assert!((a - DISC).abs() <= 0.1 * DISC, "{a} vs {DISC}");
        assert_eq!(undo_names(&app), ["Object Selection"]);
    }

    /// The hover cache follows the pixels: once they change, what the pointer rests on is looked
    /// up again.
    #[test]
    fn hover_follows_changed_pixels() {
        let mut app = disc_doc();
        app.zoom_anchor = Some((40.0, 40.0));
        drive_tool(&mut app, T, &[Ev::Move(40.0, 40.0)]);
        settle(&mut app);
        assert!(!highlighted(&app));
        // A red square under the pointer.
        let ws = app.active_ws_mut().unwrap();
        let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        for y in 30..50 {
            for x in 30..50 {
                s.data_mut()[(y * 200 + x) * 4..][..3].copy_from_slice(&[220, 50, 40]);
            }
        }
        ws.invalidate(None);
        drive_tool(&mut app, T, &[Ev::Wait(200.0), Ev::Move(40.0, 40.0)]);
        settle(&mut app);
        assert!(highlighted(&app));
    }

    /// Hovering the margin outside an opaque layer shows no empty-layer hint.
    #[test]
    fn hover_outside_the_canvas_shows_no_hint() {
        let mut app = flat_doc();
        app.zoom_anchor = Some((-30.0, -30.0));
        drive_tool(&mut app, T, &[Ev::Move(-30.0, -30.0)]);
        settle(&mut app);
        assert_eq!(hint(&mut app), None);
    }

    /// The Object Finder's tint is a 50 % fill with an opaque 1-px outline (R54).
    #[test]
    fn highlight_is_a_fill_with_an_outline() {
        let mut app = disc_doc();
        app.zoom_anchor = Some((100.0, 75.0));
        drive_tool(&mut app, T, &[Ev::Move(100.0, 75.0)]);
        settle(&mut app);
        let ws = app.active_ws().unwrap();
        let t = ws
            .surface_box
            .renderers
            .iter()
            .find_map(|(_, r)| match r {
                Renderer::Tint(t) if !t.rect.is_empty() => Some(t.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(t.color, [0, 0, 255, 255]);
        let w = t.rect.w as usize;
        let at = |x: i32, y: i32| t.mask[(y - t.rect.y) as usize * w + (x - t.rect.x) as usize];
        assert_eq!(at(100, 75), 128, "fill at 50 %");
        assert_eq!(at(100, 55).max(at(100, 56)), 255, "outline at the top");
        assert_eq!(at(80, 75).max(at(81, 75)), 255, "outline at the side");
    }

    fn square(x: i32) -> Region {
        Region {
            bbox: Rect::new(x, 0, x + 4, 4),
            mask: vec![255; 16],
        }
    }

    /// Up to 12 objects and 64 misses, in separate stores: misses never evict objects.
    #[test]
    fn finder_keeps_objects_and_misses_apart() {
        let mut f = Finder::default();
        for i in 0..80 {
            f.job = Some((0, [i, 100], Job::spawn(|_| Ok(None))));
            while f.job.is_some() {
                f.collect();
            }
        }
        for i in 0..20 {
            f.job = Some((0, [i, 0], Job::spawn(move |_| Ok(Some(square(i * 4))))));
            while f.job.is_some() {
                f.collect();
            }
        }
        assert_eq!((f.found.len(), f.misses.len()), (12, 64));
        // The oldest objects went; the newest stay, and the misses survived the objects.
        assert!(matches!(f.lookup([1.0, 1.0]), Known::Unknown));
        assert!(matches!(f.lookup([77.0, 1.0]), Known::Hit(_)));
        assert!(matches!(f.lookup([79.0 * 4.0, 400.0]), Known::Miss));
        assert!(matches!(f.lookup([0.0, 400.0]), Known::Unknown));
    }

    /// A click on a cached miss looks again, as upstream reuses only found objects: the pixels
    /// under a cached "background" cell changed without a new revision.
    #[test]
    fn click_recomputes_a_cached_miss() {
        let mut app = disc_doc();
        drive_tool(&mut app, T, &[]);
        // The disc's cell is wrongly remembered as background.
        let rev = app.with_tool(|t, app| {
            let t = t
                .as_any_mut()
                .downcast_mut::<ObjectSelectionTool>()
                .unwrap();
            let rev = t.sampled(app).unwrap().0;
            t.finder.revision = rev;
            t.finder.misses.push(cell_of([100.0, 75.0]));
            rev
        });
        assert!(rev.is_some());
        click(&mut app, (100.0, 75.0));
        let a = area(&app);
        assert!((a - DISC).abs() <= 0.05 * DISC, "{a} vs {DISC}");
    }

    /// A job that finishes only once `gate` is set, then finds the square (80..120, 55..95).
    #[cfg(not(target_arch = "wasm32"))]
    fn start_gated(app: &mut AppWorkspace) -> Arc<std::sync::atomic::AtomicBool> {
        use std::sync::atomic::Ordering::Relaxed;
        let gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let g = gate.clone();
        drive_tool(app, T, &[]);
        app.with_tool(|t, app| {
            let t = t
                .as_any_mut()
                .downcast_mut::<ObjectSelectionTool>()
                .unwrap();
            t.start(app, CombineMode::Replace, move |_, _, ctl| {
                while !g.load(Relaxed) {
                    ctl.check()?;
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Ok(Some(Region {
                    bbox: Rect::new(80, 55, 120, 95),
                    mask: vec![255; 1600],
                }))
            });
        });
        assert!(app.tool_busy());
        gate
    }

    /// Esc drops a running job: nothing is selected, nothing in the history.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn escape_drops_the_job() {
        let mut app = disc_doc();
        let gate = start_gated(&mut app);
        drive_tool(&mut app, T, &[Ev::Key("Escape")]);
        assert!(!app.tool_busy());
        gate.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(50));
        app.on_frame(app.now_ms + 16.0);
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert_eq!(undo_names(&app), Vec::<String>::new());
    }

    /// Undo cancels a running job.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn undo_cancels_the_job() {
        let mut app = disc_doc();
        drag(&mut app, (70.0, 45.0), (130.0, 105.0), "");
        let gate = start_gated(&mut app);
        assert!(app.undo());
        assert!(!app.tool_busy());
        gate.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(50));
        app.on_frame(app.now_ms + 16.0);
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert_eq!(app.active_ws().unwrap().history.undo.len(), 1);
    }

    /// A result for another document is dropped (the document-id guard).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn stale_document_result_is_dropped() {
        let mut app = disc_doc();
        let gate = start_gated(&mut app);
        app.with_tool(|t, _| {
            let t = t
                .as_any_mut()
                .downcast_mut::<ObjectSelectionTool>()
                .unwrap();
            t.running.as_mut().unwrap().doc += 1;
        });
        gate.store(true, std::sync::atomic::Ordering::Relaxed);
        settle(&mut app);
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert_eq!(undo_names(&app), Vec::<String>::new());
    }
}
