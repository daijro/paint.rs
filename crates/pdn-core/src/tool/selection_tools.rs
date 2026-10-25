//! The Rectangle, Ellipse and Lasso Select tools.
//! Port of: js/src/engine/document/tool/tool/{SelectionTool,RectangleSelectTool,EllipseSelectTool,
//! LassoSelectTool}.js (structure; the rules follow Paint.NET)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's Sutherland–Hodgman clip, too-quick clicks, ellipse flattening, fixed-size
//! defaults, Shift / ratio / fixed size and the live layer's tint.
//! Rulings P2: the ellipse flattener is four cubic arcs (κ = 0.5522847498) subdivided to a
//! flatness of 0.01 px with f32 vertices; fixed-size selections are in pixels.

use std::marker::PhantomData;

use super::{CursorRequest, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::document::workspace::DocumentWorkspace;
use crate::history::{HistoryMemento, MementoKind};
use crate::pixel::{IntRect, mask};
use crate::render::selection::SelectionRenderer;
use crate::render::{Renderer, RendererId};
use crate::selection::path::{GraphicsPath, clip_to_rect};
use crate::selection::{CombineMode, SelectionData};
use crate::util::geom::{Point, Rectangle};

/// Rectangle Select's draw mode (`selectionMode`); sizes in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RectMode {
    Normal,
    FixedRatio(f64, f64),
    FixedSize(f64, f64),
}

/// §2.5 step 6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhatToDo {
    Emit,
    Clear,
    Revert,
}

/// Screen points per document px: the zoom over the OS scale (upstream's `point_zoom`).
pub(super) fn points_per_px(app: &AppWorkspace) -> f64 {
    app.active_ws()
        .map_or(1.0, |ws| ws.view.zoom / ws.view.ppp)
        .max(0.01)
}

/// Selects `path` in `mode` as Lasso Select's release does (§2.5 steps 5–7 without the
/// press-time rules, Ruling R39), for the Extra selection tools: a Replace without area, or an
/// Intersect without area or disjoint from the selection, deselects ("Deselect", if there was a
/// selection); another mode without area, or an Exclude disjoint from it, changes nothing; else
/// one Selection history item named after `tool`.
pub(super) fn select_path(
    ws: &mut DocumentWorkspace,
    path: GraphicsPath,
    mode: CombineMode,
    tool: ToolType,
) {
    use CombineMode::*;
    use WhatToDo::*;
    let new = Some(path.get_bounds()).filter(|b| !b.is_empty());
    let old = ws.selection.create_path(true).get_bounds();
    let disjoint = |new: &Rectangle| Rectangle::intersect_of(&old, new).is_empty();
    let what = match (mode, new) {
        (Replace | Intersect, None) => Clear,
        (_, None) => Revert,
        (Intersect, Some(b)) if disjoint(&b) => Clear,
        (Exclude, Some(b)) if disjoint(&b) => Revert,
        _ => Emit,
    };
    let before = MementoKind::Selection(ws.selection.save());
    match what {
        Clear if !ws.selection.is_empty() => {
            let name = crate::i18n::t("deselectAction.name");
            let icon = "assets/icons/menu_edit_deselect_icon.png";
            ws.push_memento(HistoryMemento::new(&name, icon, before));
            ws.selection.reset();
        }
        Clear | Revert => {}
        Emit => {
            let name = crate::i18n::t(&format!("{}.name", tool.id()));
            ws.selection.set_continuation_path(path, mode);
            ws.push_memento(HistoryMemento::new(&name, &tool.icon_src(), before));
            ws.selection.commit_continuation();
        }
    }
}

/// §2.5 step 2: the press's combine mode, Ctrl or Alt × the button, else `tool`'s setting.
pub(super) fn press_mode(app: &AppWorkspace, tool: ToolType, b: MouseButton) -> CombineMode {
    let configured = app.tool_settings.get(tool, "combineMode");
    let configured = CombineMode::from_value(configured.as_str().unwrap_or(""));
    match (b, app.control_key_down, app.alt_key_down) {
        (MouseButton::Left, true, _) => CombineMode::Union,
        (MouseButton::Left, _, true) => CombineMode::Exclude,
        (MouseButton::Right, true, _) => CombineMode::Xor,
        (MouseButton::Right, _, true) => CombineMode::Intersect,
        _ => configured,
    }
}

/// §2.5 step 5: `shape` Sutherland–Hodgman-clipped to the document, except for Exclude and Xor.
pub(super) fn clip_shape(shape: Vec<Point>, mode: CombineMode, (w, h): (u32, u32)) -> Vec<Point> {
    match mode {
        CombineMode::Xor | CombineMode::Exclude => shape,
        _ => clip_to_rect(&shape, &Rectangle::new(0.0, 0.0, w as f64, h as f64)),
    }
}

/// The workspace layer tints while a selection tool is active; a second, untinted layer shows
/// the selection being drawn (`canvas-rendering.md §2.1` step 1, §2.9 step 5). Its id.
pub(super) fn add_live_layer(app: &mut AppWorkspace) -> Option<RendererId> {
    let ws = app.active_ws_mut()?;
    ws.selection_renderer_mut().tinting = true;
    let live = SelectionRenderer {
        visible: false,
        path: Some(GraphicsPath::default()),
        ..Default::default()
    };
    Some(ws.surface_box.add_renderer(Renderer::Selection(live)))
}

/// Undoes [`add_live_layer`].
pub(super) fn remove_live_layer(app: &mut AppWorkspace, live: Option<RendererId>) {
    if let Some(ws) = app.active_ws_mut() {
        ws.selection_renderer_mut().tinting = false;
        if let Some(id) = live {
            ws.surface_box.remove_renderer(id);
        }
    }
}

/// §2.5 step 2: drawing in `mode` begins; the workspace outline hides and Replace clears the
/// selection (the live layer outlines the shape meanwhile).
pub(super) fn begin_live(ws: &mut DocumentWorkspace, mode: CombineMode) {
    ws.selection_renderer_mut().outline = false;
    if mode == CombineMode::Replace {
        ws.selection.reset();
    } else {
        ws.selection.reset_continuation();
    }
}

/// The continuation follows `polygon`; the live layer shows it (Replace) or `old` XOR it.
pub(super) fn show_live(
    ws: &mut DocumentWorkspace,
    live: Option<RendererId>,
    polygon: &[Point],
    mode: CombineMode,
    old: &GraphicsPath,
) {
    ws.selection.set_continuation_points(polygon, mode);
    let shape = GraphicsPath::from_points(polygon);
    let shown = if mode == CombineMode::Replace {
        shape
    } else {
        GraphicsPath::combine(old, CombineMode::Xor, &shape)
    };
    if let Some(Renderer::Selection(r)) = live.and_then(|id| ws.surface_box.renderer_mut(id)) {
        r.path = Some(shown);
        r.visible = true;
    }
}

/// Drawing ends: the live layer empties and hides, and the workspace outline shows again.
pub(super) fn end_live(ws: &mut DocumentWorkspace, live: Option<RendererId>) {
    if let Some(Renderer::Selection(r)) = live.and_then(|id| ws.surface_box.renderer_mut(id)) {
        r.visible = false;
        r.path = Some(GraphicsPath::default());
    }
    ws.selection_renderer_mut().outline = true;
}

/// §2.5 step 4 "Rectangle": L = min x, T = min y, R = 1 + max x, B = 1 + max y, each clamped to the
/// document; Shift makes a square of side min(w, h) and a fixed ratio fits W:H by the smaller of
/// w/W and h/H, both anchored at a's corner and growing toward b; a fixed size puts its top-left at
/// b, clamped to [0, doc − size]. Finally ∩ the document, rounded out to integer bounds.
pub fn rectangle_select_rect(
    a: Point,
    b: Point,
    doc: (u32, u32),
    mode: RectMode,
    shift: bool,
) -> Rectangle {
    let (dw, dh) = (doc.0 as f64, doc.1 as f64);
    // The low bound wins when the range is inverted (a size larger than the document).
    let clamp = |v: f64, lo: f64, hi: f64| v.min(hi).max(lo);
    let r = if let RectMode::FixedSize(w, h) = mode {
        let (w, h) = (w.max(0.0), h.max(0.0));
        Rectangle::new(clamp(b.x, 0.0, dw - w), clamp(b.y, 0.0, dh - h), w, h)
    } else {
        let (l, t) = (clamp(a.x.min(b.x), 0.0, dw), clamp(a.y.min(b.y), 0.0, dh));
        let (r, bt) = (
            clamp(1.0 + a.x.max(b.x), 0.0, dw),
            clamp(1.0 + a.y.max(b.y), 0.0, dh),
        );
        let (w, h) = (r - l, bt - t);
        let ratio = |v: f64| if v.is_finite() && v > 0.0 { v } else { 1.0 };
        let fit = match mode {
            RectMode::FixedRatio(rw, rh) => {
                let (rw, rh) = (ratio(rw), ratio(rh));
                let k = (w / rw).min(h / rh);
                Some((k * rw, k * rh))
            }
            _ if shift => Some((w.min(h), w.min(h))),
            _ => None,
        };
        match fit {
            None => Rectangle::absolute(l, t, r, bt),
            Some((fw, fh)) => Rectangle::new(
                if a.x <= b.x { l } else { r - fw },
                if a.y <= b.y { t } else { bt - fh },
                fw,
                fh,
            ),
        }
    };
    let r = Rectangle::intersect_of(&r, &Rectangle::new(0.0, 0.0, dw, dh));
    let i = IntRect::bound_of(&r);
    Rectangle::new(i.x as f64, i.y as f64, i.w as f64, i.h as f64)
}

/// The circle-approximation constant of each quarter-ellipse cubic.
const KAPPA: f64 = 0.5522847498;

/// Appends the end points of `c` subdivided until max(ux², vx²) + max(uy², vy²) ≤ 16·tol² (the
/// curve then lies within `tol` of its chords).
fn flatten(c: [[f64; 2]; 4], tol: f64, depth: u32, out: &mut Vec<[f64; 2]>) {
    let [p0, p1, p2, p3] = c;
    let u = [
        3.0 * p1[0] - 2.0 * p0[0] - p3[0],
        3.0 * p1[1] - 2.0 * p0[1] - p3[1],
    ];
    let v = [
        3.0 * p2[0] - p0[0] - 2.0 * p3[0],
        3.0 * p2[1] - p0[1] - 2.0 * p3[1],
    ];
    let flat = (u[0] * u[0]).max(v[0] * v[0]) + (u[1] * u[1]).max(v[1] * v[1]) <= 16.0 * tol * tol;
    if flat || depth == 0 {
        out.push(p3);
        return;
    }
    let mid = |a: [f64; 2], b: [f64; 2]| [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let (a, b, c2) = (mid(p0, p1), mid(p1, p2), mid(p2, p3));
    let (d, e) = (mid(a, b), mid(b, c2));
    let m = mid(d, e);
    flatten([p0, a, d, m], tol, depth - 1, out);
    flatten([m, e, c2, p3], tol, depth - 1, out);
}

/// §2.5 step 4 "Ellipse": the bounding box of a, b plus 1 on width and height, or with Shift the
/// circle centred at (a + b)/2 of radius |b − a|/2; flattened per the Rulings (four cubic arcs from
/// (cx + rx, cy), clockwise on screen, to 0.01 px), every vertex rounded to f32.
pub fn ellipse_select_polygon(a: Point, b: Point, shift: bool) -> Vec<Point> {
    let (cx, cy, rx, ry) = if shift {
        let r = (b.x - a.x).hypot(b.y - a.y) / 2.0;
        ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, r, r)
    } else {
        let (w, h) = ((a.x - b.x).abs() + 1.0, (a.y - b.y).abs() + 1.0);
        (
            a.x.min(b.x) + w / 2.0,
            a.y.min(b.y) + h / 2.0,
            w / 2.0,
            h / 2.0,
        )
    };
    if !(rx > 0.0 && ry > 0.0) || ![cx, cy, rx, ry].iter().all(|v| v.is_finite()) {
        return vec![];
    }
    // (cos, sin) at 0°, 90°, 180°, 270°.
    let q = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
    let mut pts = vec![[cx + rx, cy]];
    for i in 0..4 {
        let ((c0, s0), (c1, s1)) = (q[i], q[(i + 1) % 4]);
        let p0 = [cx + rx * c0, cy + ry * s0];
        let p3 = [cx + rx * c1, cy + ry * s1];
        // Tangent d/dθ (rx cos θ, ry sin θ) = (−rx sin θ, ry cos θ).
        let p1 = [p0[0] - KAPPA * rx * s0, p0[1] + KAPPA * ry * c0];
        let p2 = [p3[0] + KAPPA * rx * s1, p3[1] - KAPPA * ry * c1];
        flatten([p0, p1, p2, p3], 0.01, 16, &mut pts);
    }
    // The last arc ends on the start point.
    pts.pop();
    let mut out: Vec<Point> = pts
        .into_iter()
        .map(|[x, y]| Point::new(x as f32 as f64, y as f32 as f64))
        .collect();
    out.dedup();
    out
}

/// What a selection tool draws: its shape and the per-tool rules.
pub trait SelectionShape: 'static {
    const TOOL: ToolType;
    /// The `_cursor`, `_mouse_down_cursor`, `_plus_cursor` and `_minus_cursor` images.
    const CURSORS: [&'static str; 4];
    /// The trace length from which the shape is shown.
    const MIN_POINTS: usize = 2;
    /// Rectangle Select's extra WhatToDo overrides (§2.5 step 6).
    const RECT_RULES: bool = false;
    /// The shape of the floored trace (never empty) in document coordinates.
    fn shape(trace: &[Point], app: &AppWorkspace) -> Vec<Point>;
    /// Rectangle Select in fixed-size mode: no "must move", no "too quick" (§2.5 steps 6–7).
    fn fixed_size(_app: &AppWorkspace) -> bool {
        false
    }
}

pub struct RectShape;
pub struct EllipseShape;
pub struct LassoShape;

/// Rectangle Select's `selectionMode`, `selectionWidth` and `selectionHeight` (pixels).
fn rect_mode(app: &AppWorkspace) -> RectMode {
    let t = ToolType::RectangleSelect;
    let num = |k: &str| app.tool_settings.get(t, k).as_f64().unwrap_or(0.0);
    match app.tool_settings.get(t, "selectionMode").as_str() {
        Some("fixedRatio") => RectMode::FixedRatio(num("selectionWidth"), num("selectionHeight")),
        Some("fixedSize") => RectMode::FixedSize(num("selectionWidth"), num("selectionHeight")),
        _ => RectMode::Normal,
    }
}

fn doc_size(app: &AppWorkspace) -> (u32, u32) {
    app.active_ws()
        .map_or((0, 0), |ws| (ws.document.width, ws.document.height))
}

fn ends(trace: &[Point]) -> (Point, Point) {
    (trace[0], trace[trace.len() - 1])
}

impl SelectionShape for RectShape {
    const TOOL: ToolType = ToolType::RectangleSelect;
    const CURSORS: [&'static str; 4] = [
        "rectangle_select_tool_cursor",
        "rectangle_select_tool_mouse_down_cursor",
        "rectangle_select_tool_plus_cursor",
        "rectangle_select_tool_minus_cursor",
    ];
    const MIN_POINTS: usize = 1;
    const RECT_RULES: bool = true;

    fn shape(trace: &[Point], app: &AppWorkspace) -> Vec<Point> {
        let (a, b) = ends(trace);
        let r = rectangle_select_rect(a, b, doc_size(app), rect_mode(app), app.shift_key_down);
        if r.is_empty() {
            return vec![];
        }
        GraphicsPath::from_rect(&r).polygons.remove(0)
    }

    fn fixed_size(app: &AppWorkspace) -> bool {
        matches!(rect_mode(app), RectMode::FixedSize(..))
    }
}

impl SelectionShape for EllipseShape {
    const TOOL: ToolType = ToolType::EllipseSelect;
    const CURSORS: [&'static str; 4] = [
        "ellipse_select_tool_cursor",
        "ellipse_select_tool_mouse_down_cursor",
        "ellipse_select_tool_plus_cursor",
        "ellipse_select_tool_minus_cursor",
    ];

    fn shape(trace: &[Point], app: &AppWorkspace) -> Vec<Point> {
        let (a, b) = ends(trace);
        ellipse_select_polygon(a, b, app.shift_key_down)
    }
}

impl SelectionShape for LassoShape {
    const TOOL: ToolType = ToolType::LassoSelect;
    const CURSORS: [&'static str; 4] = [
        "lasso_select_tool_cursor",
        "lasso_select_tool_mouse_down_cursor",
        "lasso_select_tool_plus_cursor",
        "lasso_select_tool_minus_cursor",
    ];

    /// The trace as it is (the polygon's closing edge is implicit).
    fn shape(trace: &[Point], _app: &AppWorkspace) -> Vec<Point> {
        trace.to_vec()
    }
}

pub type RectangleSelectTool = SelectionTool<RectShape>;
pub type EllipseSelectTool = SelectionTool<EllipseShape>;
pub type LassoSelectTool = SelectionTool<LassoShape>;

/// `SelectionTool` (§2.5): tracks a floored pointer trace and keeps the selection's continuation
/// on its shape; the release emits, clears or reverts it.
pub struct SelectionTool<S> {
    tracking: bool,
    has_moved: bool,
    was_not_empty: bool,
    /// Move-origin mode (a second button pressed while tracking): the last floored pointer.
    move_origin: Option<Point>,
    press_ms: f64,
    /// The selection at the press (the history memento's payload).
    before: SelectionData,
    mode: CombineMode,
    old_bounds: Rectangle,
    old_path: GraphicsPath,
    trace: Vec<Point>,
    /// The live (untinted) selection layer, while active.
    live: Option<RendererId>,
    cursor: &'static str,
    shape: PhantomData<S>,
}

impl<S: SelectionShape> Default for SelectionTool<S> {
    fn default() -> Self {
        Self {
            tracking: false,
            has_moved: false,
            was_not_empty: false,
            move_origin: None,
            press_ms: 0.0,
            before: SelectionData::default(),
            mode: CombineMode::Replace,
            old_bounds: Rectangle::default(),
            old_path: GraphicsPath::default(),
            trace: vec![],
            live: None,
            cursor: S::CURSORS[0],
            shape: PhantomData,
        }
    }
}

fn floor(x: f64, y: f64) -> Point {
    Point::new(x.floor(), y.floor())
}

/// Merged 1× scans: the pixels of pixelated geometry, whatever its vertices.
fn pixel_rows(p: &GraphicsPath) -> Vec<(i32, i32, i32)> {
    let mut out: Vec<(i32, i32, i32)> = vec![];
    for (y, x0, x1) in mask::scans(&p.as_polys(), 1.0) {
        match out.last_mut() {
            Some(last) if last.0 == y && last.2 == x0 => last.2 = x1,
            _ => out.push((y, x0, x1)),
        }
    }
    out
}

impl<S: SelectionShape> SelectionTool<S> {
    pub fn is_tracking(&self) -> bool {
        self.tracking
    }

    fn update_cursor(&mut self, app: &AppWorkspace) {
        let i = if self.tracking {
            1
        } else if app.control_key_down {
            2
        } else if app.alt_key_down {
            3
        } else {
            0
        };
        self.cursor = S::CURSORS[i];
    }

    fn append(&mut self, x: f64, y: f64) {
        let p = floor(x, y);
        if self.trace.last() != Some(&p) {
            self.trace.push(p);
        }
    }

    /// §2.5 steps 4–5: the shape, Sutherland–Hodgman-clipped to the document except for Exclude
    /// and Xor.
    fn polygon(&self, app: &AppWorkspace) -> Vec<Point> {
        if self.trace.is_empty() {
            return vec![];
        }
        clip_shape(S::shape(&self.trace, app), self.mode, doc_size(app))
    }

    /// The continuation follows the shape; the live layer shows it (Replace) or base XOR it.
    fn render(&mut self, app: &mut AppWorkspace) {
        if self.trace.len() < S::MIN_POINTS {
            return;
        }
        let polygon = self.polygon(app);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        show_live(ws, self.live, &polygon, self.mode, &self.old_path);
    }

    /// §2.5 step 6, with the overrides that turn a planned Emit into something else.
    fn what_to_do(&self, app: &AppWorkspace, polygon: &[Point]) -> WhatToDo {
        use CombineMode::*;
        use WhatToDo::*;
        let has_moved = self.has_moved && self.trace.len() > 1;
        let fixed = S::fixed_size(app);
        let must_move = !fixed;
        let too_quick = !fixed && app.now_ms - self.press_ms <= 50.0;
        let new = crate::selection::path::bounds_of(polygon.iter());
        let clipped = new.is_none_or(|b| b.is_empty());
        let planned = match self.mode {
            Replace if (has_moved || !must_move) && !too_quick && !clipped => Emit,
            Replace => Clear,
            Intersect if clipped => Clear,
            _ if (!has_moved && must_move) || clipped => Revert,
            _ => Emit,
        };
        let Some(new) = new.filter(|_| planned == Emit) else {
            return planned;
        };
        let old = self.old_bounds;
        let disjoint = Rectangle::intersect_of(&old, &new).is_empty();
        let contains = new.left() <= old.left()
            && new.top() <= old.top()
            && new.right() >= old.right()
            && new.bottom() >= old.bottom();
        // The fixed-size toggle: the same rectangle again deselects.
        let same = || {
            let p = GraphicsPath::from_points(polygon);
            self.old_path.is_pixelated() && pixel_rows(&self.old_path) == pixel_rows(&p)
        };
        match self.mode {
            Intersect if disjoint => Clear,
            Exclude if disjoint => Revert,
            _ if !S::RECT_RULES => Emit,
            Replace if fixed && same() => Clear,
            Intersect if contains => Revert,
            Exclude if contains => Clear,
            _ => Emit,
        }
    }

    /// `done`: §2.5 steps 6–7 on release (or deactivation while tracking).
    fn done(&mut self, app: &mut AppWorkspace) {
        if !self.tracking {
            return;
        }
        let polygon = self.polygon(app);
        let what = self.what_to_do(app, &polygon);
        self.tracking = false;
        self.move_origin = None;
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let memento = |name: &str, icon: &str| {
            HistoryMemento::new(name, icon, MementoKind::Selection(self.before.clone()))
        };
        match what {
            WhatToDo::Clear => {
                if self.was_not_empty {
                    ws.push_memento(memento(
                        &crate::i18n::t("deselectAction.name"),
                        "assets/icons/menu_edit_deselect_icon.png",
                    ));
                }
                ws.selection.reset();
            }
            WhatToDo::Emit => {
                let name = crate::i18n::t(&format!("{}.name", S::TOOL.id()));
                ws.push_memento(memento(&name, &S::TOOL.icon_src()));
                ws.selection.commit_continuation();
            }
            // Ruling R48: back to the base, no history entry.
            WhatToDo::Revert => {
                ws.selection.reset_continuation();
            }
        }
        end_live(ws, self.live);
    }
}

impl<S: SelectionShape> Tool for SelectionTool<S> {
    fn tool_type(&self) -> ToolType {
        S::TOOL
    }

    fn live_gesture(&self, app: &AppWorkspace) -> Option<super::LiveGesture> {
        if !self.tracking || self.trace.len() < S::MIN_POINTS.max(1) {
            return None;
        }
        let shape = S::shape(&self.trace, app);
        let points = shape.iter().map(|p| [p.x, p.y]);
        if S::TOOL == ToolType::LassoSelect {
            return Some(super::LiveGesture::Lasso(points.collect()));
        }
        let rect = points.fold(None, |r: Option<[f64; 4]>, [x, y]| {
            Some(r.map_or([x, y, x, y], |r| {
                [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)]
            }))
        })?;
        Some(super::LiveGesture::Band {
            rect,
            ellipse: S::TOOL == ToolType::EllipseSelect,
        })
    }

    /// [`add_live_layer`].
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        self.update_cursor(app);
        self.live = add_live_layer(app);
    }

    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        self.done(app);
        remove_live_layer(app, self.live.take());
    }

    /// §2.5 steps 1–2.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if self.tracking {
            self.move_origin = Some(floor(x, y));
            self.update_cursor(app);
            return true;
        }
        if !matches!(b, MouseButton::Left | MouseButton::Right) {
            return false;
        }
        let mode = press_mode(app, S::TOOL, b);
        let Some(ws) = app.active_ws_mut() else {
            return false;
        };
        self.tracking = true;
        self.has_moved = false;
        self.trace = vec![floor(x, y)];
        self.before = ws.selection.save();
        self.was_not_empty = !ws.selection.is_empty();
        self.old_path = ws.selection.create_path(true);
        self.old_bounds = self.old_path.get_bounds();
        self.mode = mode;
        begin_live(ws, mode);
        self.press_ms = app.now_ms;
        self.render(app);
        self.update_cursor(app);
        true
    }

    /// §2.5 step 3.
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, i: &PointerInput) -> bool {
        if let Some(last) = self.move_origin {
            let p = floor(x, y);
            let (dx, dy) = (p.x - last.x, p.y - last.y);
            for q in &mut self.trace {
                (q.x, q.y) = (q.x + dx, q.y + dy);
            }
            self.move_origin = Some(p);
            self.render(app);
        } else if self.tracking {
            for s in &i.samples {
                self.append(s.x, s.y);
            }
            self.append(x, y);
            self.has_moved = true;
            self.render(app);
        }
        self.update_cursor(app);
        self.tracking
    }

    /// The release point is a last move; it ends move-origin mode, else the selection.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        _b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        let was = self.tracking;
        if was {
            self.on_mouse_move(app, x, y, i);
        }
        if self.move_origin.take().is_none() {
            self.done(app);
        }
        self.update_cursor(app);
        was
    }

    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        if self.tracking {
            self.render(app);
        }
        self.update_cursor(app);
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(self.cursor)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;

    /// Ruling R49: the status-bar area of a 201×151 ellipse (`select-ellipse-drag`) from the
    /// κ-cubic flattening (Rulings P2); paintdotjs's flattener gives 23,839, exact arcs ~23,843.
    // calibrate: B-B1 — Paint.NET's area for this box: 23,851 ⇒ κ-cubic arcs; ~23,843 ⇒ exact arcs.
    #[test]
    fn ellipse_area_pins_the_flattening() {
        let poly = ellipse_select_polygon(Point::new(10.0, 20.0), Point::new(210.0, 170.0), false);
        let mut s = Selection::default();
        s.set_continuation_points(&poly, CombineMode::Replace);
        let st = s.get_statistics(&Rectangle::new(0.0, 0.0, 400.0, 300.0));
        assert_eq!(st.bounds, Rectangle::new(10.0, 20.0, 201.0, 151.0));
        assert_eq!(crate::app::format_n(st.area, 0), "23,851");
    }
}
