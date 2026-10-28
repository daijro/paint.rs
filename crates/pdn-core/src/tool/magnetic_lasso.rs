//! Ported from PhotoCraft crates/ui-egui/src/magnetic_lasso_ui.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: the state machine on paint.rs's `Tool` hooks; the first click's combine mode from Paint.NET's Ctrl/Alt × button (Lasso Select's) instead of ⇧/⌥; the tracer reads the active layer, its cache kept while the layer's `Surface::revision` holds; the close follows Lasso Select's release rules (Revert, Deselect) and is otherwise one Selection history item through the continuation; a double-click is two presses within 500 ms and 4 points; Ruling R231: a drag's release closes the border as Lasso Select's does (upstream's click-move-click mode starts from a click without a drag), the border shows as Lasso Select's selection does (the tinted continuation and its live layer's marching ants), and pointer moves trace a bounded number of steps a frame, the rest drawn straight until then (in place of upstream's batched-move `flush`); pen-pressure width, the closing mark, the fastening-point squares and the Precise cursor dropped.
//!
//! The Magnetic Lasso (S§4.3): a selection border that snaps to edges as the pointer moves.
//!
//! The first press places the start (snapped to an edge) and takes the combine mode from Lasso
//! Select's modifiers. Moving the pointer traces the border from the last fastening point along
//! the edges near the pointer's path (`extra::magnetic`), and points fasten by themselves as it
//! goes, more often at a higher Frequency. A drag's release closes the border along the edges and
//! selects it, as Lasso Select's release does (Ruling R231). A click without a drag keeps the
//! border open instead: moving the pointer, button up or down, goes on tracing; a click fastens a
//! point; Alt+click draws a straight segment and Alt+drag a freehand one (its release closes the
//! border, as every drag's does). Backspace or Delete removes the last fastening point. A click on
//! the start, a double-click or Enter closes the border along the edges (Alt: with a straight
//! segment); Esc cancels. With the Extra Click points option (`clickPoints`) a later drag's release
//! fastens a point instead of closing, as Lasso Select's Click points does (`click_lasso`). `[` and `]` change the detection width by 1. While it is drawn the
//! border shows as Lasso Select's selection does: tinted, with marching ants.

use std::collections::VecDeque;

use serde_json::Value;

use super::selection_tools::{
    add_live_layer, begin_live, click_points, clip_shape, end_live, points_per_px, press_mode,
    remove_live_layer, select_path, show_live,
};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::extra::geom::Rect;
use crate::extra::magnetic::{self, Fetch, Settings, Tracer};
use crate::render::RendererId;
use crate::selection::path::GraphicsPath;
use crate::selection::{CombineMode, SelectionData};
use crate::util::geom::Point;

/// Lasso Select's cursors (idle, drawing, Ctrl, Alt): the tool has none of its own.
const CURSORS: [&str; 4] = [
    "lasso_select_tool_cursor",
    "lasso_select_tool_mouse_down_cursor",
    "lasso_select_tool_plus_cursor",
    "lasso_select_tool_minus_cursor",
];
/// Windows' default double-click time (ms) and distance (screen points); a held button that goes
/// farther than that from its press drags.
const DOUBLE_CLICK: (f64, f64) = (500.0, 4.0);
/// Within this many screen points of the start, a click closes the border.
const CLOSE_RADIUS: f64 = 8.0;
/// Trace steps (each up to half the fastening spacing, ~23 screen points) a frame: a drag up to
/// ~90 points a frame keeps up; a faster one shows its tail straight until the tracing catches up.
// ponytail: a step-count budget, not a clock. A step costs ~1–7 ms at the default 10-px width on a
// dense 12 MP texture (release, 100 % and 20 %), but its trace grows with the width: ~16 ms at 64
// px, 30–250 ms at 256 px. Tracing on `extra::job`'s thread is the upgrade path for wide widths.
const STEPS_PER_FRAME: usize = 4;

/// A border being drawn (document px), and the layer that shows it.
#[derive(Default)]
pub struct MagneticLassoTool {
    /// The fastened border, from the first fastening point to the last; empty when none is drawn.
    path: Vec<[f64; 2]>,
    /// Indices into `path` of the fastening points (the first is 0).
    anchors: Vec<usize>,
    /// The live segment from the last fastening point to the edge near the pointer.
    live: Vec<[f64; 2]>,
    /// Where the pointer has been since the last fastening point: edges are looked for near it.
    trail: Vec<[f64; 2]>,
    /// Pointer positions not traced yet, walked into steps: drawn straight after the border until
    /// `catch_up` traces them, so the path shows from the first move (Ruling R231).
    pending: VecDeque<[f64; 2]>,
    /// The last pointer position.
    pointer: [f64; 2],
    /// Where the held button went down, which it is and whether Alt was down then, until its
    /// release.
    press: Option<([f64; 2], MouseButton, bool)>,
    /// The held button went farther than a click from its press: its release closes the border.
    dragged: bool,
    /// The held button placed the first fastening point.
    opening: bool,
    /// The combine mode the first press set.
    mode: CombineMode,
    /// The selection when the border began (the history memento's payload), and its path.
    before: SelectionData,
    old_path: GraphicsPath,
    /// An Alt+drag is drawing a freehand segment.
    freehand: bool,
    /// The traced surface's `revision` and its tracer (a pixel cache), kept while a border is
    /// drawn and its pixels stay the same (Ruling R38).
    tracer: Option<(u64, Tracer)>,
    /// The last press (time, point), for the double-click.
    last_press: Option<(f64, [f64; 2])>,
    /// Lasso Select's live layer, while active.
    layer: Option<RendererId>,
    /// Index into [`CURSORS`].
    cursor: usize,
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Detection settings from the toolbar.
fn settings(app: &AppWorkspace) -> Settings {
    let num = |k| app.tool_settings.get(ToolType::MagneticLasso, k).as_f64();
    let contrast = num("contrast").unwrap_or(10.0) / 100.0;
    Settings::new(num("width").unwrap_or(10.0), contrast as f32)
}

/// `p` kept inside the image, where the tracer keeps the border.
fn inside(app: &AppWorkspace, p: [f64; 2]) -> [f64; 2] {
    let (w, h) = app.active_ws().map_or((0.0, 0.0), |ws| {
        (ws.document.width as f64, ws.document.height as f64)
    });
    [p[0].clamp(0.0, w), p[1].clamp(0.0, h)]
}

/// How far the border runs before a point fastens by itself (document px):
/// (8 + (100 − Frequency)·0.9) screen points, 47 at the default 57.
fn spacing(app: &AppWorkspace) -> f64 {
    let f = app.tool_settings.get(ToolType::MagneticLasso, "frequency");
    let f = f.as_f64().filter(|f| f.is_finite()).unwrap_or(57.0);
    (8.0 + (100.0 - f.clamp(0.0, 100.0)) * 0.9) / points_per_px(app)
}

/// The polyline `p` up to `len` along it (ending at that point) and the point, when more than a
/// pixel of it lies beyond.
fn cut(p: &[[f64; 2]], len: f64) -> Option<(Vec<[f64; 2]>, [f64; 2])> {
    if magnetic::length(p) < len + 1.0 {
        return None;
    }
    let mut head = Vec::new();
    let mut acc = 0.0;
    for w in p.windows(2) {
        let (a, b) = (w[0], w[1]);
        head.push(a);
        let d = dist(a, b);
        if acc + d >= len && d > 0.0 {
            let t = (len - acc) / d;
            let point = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            head.push(point);
            return Some((head, point));
        }
        acc += d;
    }
    None
}

impl MagneticLassoTool {
    /// A border is being drawn.
    fn active(&self) -> bool {
        !self.path.is_empty()
    }

    /// Runs `f` with the active layer's tracer and its pixels (straight RGBA8, alpha at index 3).
    fn with_tracer<R>(
        &mut self,
        app: &AppWorkspace,
        f: impl FnOnce(&mut Tracer, &mut Fetch) -> R,
    ) -> Option<R> {
        let ws = app.active_ws()?;
        let surface = &ws.document.layers.get_at(ws.active_layer)?.surface;
        let w = surface.width() as usize;
        // Another layer, an undo or an effect: a new content stamp, so a fresh cache.
        if self
            .tracer
            .as_ref()
            .is_none_or(|(r, _)| *r != surface.revision())
        {
            let bounds = Rect::new(0, 0, surface.width() as i32, surface.height() as i32);
            self.tracer = Some((surface.revision(), Tracer::new(bounds)));
        }
        let data = surface.data();
        // The tracer asks only inside `bounds`; a short read would come back transparent.
        let mut fetch = |r: Rect| -> Vec<[u8; 4]> {
            let row = |y: i32| {
                let i = (y as usize * w + r.x0 as usize) * 4;
                let px = data.get(i..i + r.width() as usize * 4).unwrap_or(&[]);
                px.as_chunks::<4>().0.iter().copied()
            };
            (r.y0..r.y1).flat_map(row).collect()
        };
        let tracer = &mut self.tracer.as_mut()?.1;
        Some(f(tracer, &mut fetch))
    }

    fn snap(&mut self, app: &AppWorkspace, p: [f64; 2], s: Settings) -> [f64; 2] {
        self.with_tracer(app, |t, f| t.snap(f, p, s))
            .flatten()
            .unwrap_or(p)
    }

    fn trace(
        &mut self,
        app: &AppWorkspace,
        from: [f64; 2],
        to: [f64; 2],
        guide: &[[f64; 2]],
        s: Settings,
    ) -> Vec<[f64; 2]> {
        let t = self.with_tracer(app, |t, f| t.trace(f, from, to, guide, s));
        match t {
            Some(t) if !t.is_empty() => t,
            _ => vec![from, to],
        }
    }

    fn down(&mut self, app: &mut AppWorkspace, p: [f64; 2], b: MouseButton) {
        // A click acts on the border traced to here.
        self.catch_up(app, usize::MAX);
        let s = settings(app);
        let (alt, now) = (app.alt_key_down, app.now_ms);
        if !self.active() {
            // The first fastening point; Lasso Select's modifiers set the combine mode, and the
            // selection is drawn as Lasso Select's is.
            self.mode = press_mode(app, ToolType::MagneticLasso, b);
            let Some(ws) = app.active_ws_mut() else {
                return;
            };
            self.before = ws.selection.save();
            self.old_path = ws.selection.create_path(true);
            begin_live(ws, self.mode);
            self.tracer = None;
            let start = self.snap(app, p, s);
            let trail = vec![inside(app, p)];
            (self.path, self.anchors, self.trail) = (vec![start], vec![0], trail);
            self.last_press = Some((now, p));
            self.opening = true;
            return;
        }
        let (ms, near) = DOUBLE_CLICK;
        let double = self
            .last_press
            .is_some_and(|(t, q)| now - t <= ms && dist(p, q) * points_per_px(app) <= near);
        self.last_press = Some((now, p));
        if double || self.near_start(app, p) {
            self.close(app, alt, !double);
            return;
        }
        if alt {
            // Alt+click: a straight segment to here; dragging on draws freehand.
            if self.path.last().is_none_or(|q| dist(*q, p) > 1e-9) {
                self.path.push(p);
                self.anchors.push(self.path.len() - 1);
            }
            self.live.clear();
            self.trail = vec![inside(app, p)];
            self.freehand = true;
            return;
        }
        // A click fastens a point at the edge near the pointer.
        self.follow(app, p, s, false);
        self.fasten_live();
    }

    /// The pointer moved to `p`: its steps wait for `catch_up`.
    fn queue(&mut self, app: &AppWorkspace, p: [f64; 2]) {
        let last = std::mem::replace(&mut self.pointer, p);
        if last == p {
            return;
        }
        let step = (spacing(app) / 2.0).max(1.0);
        // A long jump is walked in steps, so points keep fastening along it.
        let n = ((dist(last, p) / step).ceil() as usize).clamp(1, 256);
        self.pending.extend((1..=n).map(|k| {
            let t = k as f64 / n as f64;
            [
                last[0] + (p[0] - last[0]) * t,
                last[1] + (p[1] - last[1]) * t,
            ]
        }));
    }

    /// Takes the pointer's waiting steps, tracing up to `budget` of them.
    fn catch_up(&mut self, app: &AppWorkspace, mut budget: usize) {
        let s = settings(app);
        while budget > 0
            && let Some(q) = self.pending.pop_front()
        {
            if !self.freehand {
                self.follow(app, q, s, true);
                budget -= 1;
                continue;
            }
            // Alt+drag: the pointer's path is the border.
            if self.path.last().is_none_or(|l| dist(*l, q) >= 0.5) {
                self.path.push(q);
            }
            self.trail = vec![inside(app, q)];
        }
    }

    /// Adds the pointer position `q` to the trail, keeping its points a few pixels apart.
    fn extend_trail(&mut self, q: [f64; 2], s: Settings) {
        let gap = (s.width / 4.0).clamp(1.0, 8.0);
        let t = &mut self.trail;
        match t.len() {
            n if n >= 2 && dist(t[n - 2], q) < gap => t[n - 1] = q,
            _ => t.push(q),
        }
    }

    /// The pointer is at `q`: traces the live segment to the edge near it and, with `fasten`,
    /// fastens points behind the pointer while the live segment is long enough.
    fn follow(&mut self, app: &AppWorkspace, q: [f64; 2], s: Settings, fasten: bool) {
        // Off the image the tracer reads the trail clamped onto its edge; a trail left outside
        // no longer matched the border, so each fastening traced back along the edge again.
        let q = inside(app, q);
        self.extend_trail(q, s);
        let target = self.snap(app, q, s);
        let every = spacing(app);
        for _ in 0..64 {
            let Some(&from) = self.path.last() else {
                return;
            };
            let guide = self.trail.clone();
            let live = self.trace(app, from, target, &guide, s);
            // The part of the border `spacing` behind its start no longer changes as the pointer
            // moves: fasten it.
            let Some((head, point)) = cut(&live, every).filter(|_| fasten) else {
                self.live = live;
                return;
            };
            self.path.extend(head.into_iter().skip(1));
            self.anchors.push(self.path.len() - 1);
            // Edges are looked for along the pointer's path from where it passed the new point.
            let near = self
                .trail
                .iter()
                .enumerate()
                .min_by(|a, b| dist(*a.1, point).total_cmp(&dist(*b.1, point)))
                .map_or(0, |(i, _)| i);
            let rest = self.trail.iter().skip(near + 1).copied();
            self.trail = std::iter::once(point).chain(rest).collect();
        }
    }

    /// Fastens the live segment: its end becomes a fastening point.
    fn fasten_live(&mut self) {
        let before = self.path.len();
        for q in std::mem::take(&mut self.live).into_iter().skip(1) {
            if self.path.last().is_none_or(|l| dist(*l, q) > 1e-9) {
                self.path.push(q);
            }
        }
        if self.path.len() > before {
            self.anchors.push(self.path.len() - 1);
        }
        self.trail = self.path.last().copied().into_iter().collect();
    }

    /// An Alt+drag ends: where it stopped is a fastening point.
    fn end_freehand(&mut self) {
        if std::mem::take(&mut self.freehand)
            && self
                .anchors
                .last()
                .is_some_and(|&a| a + 1 < self.path.len())
        {
            self.anchors.push(self.path.len() - 1);
        }
    }

    /// `p` is over the first point of a border long enough to close.
    fn near_start(&self, app: &AppWorkspace, p: [f64; 2]) -> bool {
        let r = CLOSE_RADIUS / points_per_px(app);
        self.path.first().is_some_and(|&first| dist(first, p) <= r)
            && magnetic::length(&self.path) + magnetic::length(&self.live) > 2.0 * r
    }

    /// Closes the border and selects it. From the start (`at_start`) the last fastening point
    /// joins the first along the edges near the pointer's path; otherwise the live segment
    /// fastens first. `straight` joins them with a straight segment.
    fn close(&mut self, app: &mut AppWorkspace, straight: bool, at_start: bool) {
        self.catch_up(app, usize::MAX);
        self.end_freehand();
        let guide = if at_start {
            self.live.clear();
            std::mem::take(&mut self.trail)
        } else {
            self.fasten_live();
            vec![]
        };
        if let (false, Some(&first), Some(&last)) = (straight, self.path.first(), self.path.last())
        {
            let mut closing = self.trace(app, last, first, &guide, settings(app));
            // The border ends where it starts.
            closing.pop();
            self.path.extend(closing.into_iter().skip(1));
        }
        self.commit(app);
    }

    /// Selects the border in the first press's mode by Lasso Select's release rules
    /// ([`select_path`]) from the selection it began with, clipped to the image but for Exclude
    /// and Xor.
    fn commit(&mut self, app: &mut AppWorkspace) {
        let polygon = self.polygon(app);
        let mode = self.mode;
        self.cancel(app);
        if let Some(ws) = app.active_ws_mut() {
            let path = GraphicsPath::from_points(&polygon);
            select_path(ws, path, mode, ToolType::MagneticLasso);
        }
    }

    /// Drops the border: the selection is back as it began and the live layer empties.
    fn cancel(&mut self, app: &mut AppWorkspace) {
        let (layer, cursor) = (self.layer, self.cursor);
        let old = std::mem::take(self);
        (self.layer, self.cursor) = (layer, cursor);
        if let (true, Some(ws)) = (old.active(), app.active_ws_mut()) {
            ws.selection.restore(old.before);
            end_live(ws, layer);
        }
    }

    /// Removes the last fastening point and the border back to the one before it; with only the
    /// first point left, cancels.
    fn remove_last_point(&mut self, app: &mut AppWorkspace) {
        self.end_freehand();
        if self.anchors.len() <= 1 {
            self.cancel(app);
            return;
        }
        self.anchors.pop();
        let keep = self.anchors.last().map_or(1, |&a| a + 1);
        self.path.truncate(keep);
        self.live.clear();
        self.trail = self.path.last().copied().into_iter().collect();
        // The live segment runs to the pointer again; it fastens only when the pointer moves on,
        // so Backspace can keep removing points.
        self.follow(app, self.pointer, settings(app), false);
    }

    /// The border through the live segment and the untraced steps, closed back to the start,
    /// clipped as Lasso Select's polygon is.
    fn polygon(&self, app: &AppWorkspace) -> Vec<Point> {
        let points = self.path.iter().chain(self.live.iter().skip(1));
        let shape = (points.chain(&self.pending))
            .map(|q| Point::new(q[0], q[1]))
            .collect();
        let doc = app
            .active_ws()
            .map_or((0, 0), |ws| (ws.document.width, ws.document.height));
        clip_shape(shape, self.mode, doc)
    }

    /// Shows the border as Lasso Select shows its selection, from two points on.
    fn show(&self, app: &mut AppWorkspace) {
        let n = self.path.len() + self.live.len().saturating_sub(1) + self.pending.len();
        if n < 2 {
            return;
        }
        let polygon = self.polygon(app);
        if let Some(ws) = app.active_ws_mut() {
            show_live(ws, self.layer, &polygon, self.mode, &self.old_path);
        }
    }

    fn update_cursor(&mut self, app: &AppWorkspace) {
        self.cursor = if self.active() {
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

impl Tool for MagneticLassoTool {
    fn tool_type(&self) -> ToolType {
        ToolType::MagneticLasso
    }

    /// Lasso Select's layers ([`add_live_layer`]).
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        self.update_cursor(app);
        self.layer = add_live_layer(app);
    }

    /// A tool or document switch, or a command that commits the tool first (a selection command),
    /// drops the border.
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        self.cancel(app);
        remove_live_layer(app, self.layer.take());
    }

    /// An undo or redo drops the border first, so it steps the selection the border began with.
    fn on_executing_history_memento(&mut self, app: &mut AppWorkspace) {
        self.cancel(app);
        self.update_cursor(app);
    }

    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        // A second button while one is held changes nothing.
        if self.press.is_none() {
            let press = Some(([x, y], b, app.alt_key_down));
            (self.press, self.dragged, self.pointer) = (press, false, [x, y]);
            self.down(app, [x, y], b);
            self.show(app);
            self.update_cursor(app);
        }
        true
    }

    /// The step waits for the next frame (`on_frame`); the border shows meanwhile, the step drawn
    /// straight.
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        if !self.active() {
            return false;
        }
        let far = DOUBLE_CLICK.1 / points_per_px(app);
        self.dragged |= self.press.is_some_and(|(q, _, _)| dist(q, [x, y]) > far);
        self.queue(app, [x, y]);
        self.show(app);
        true
    }

    /// The pressed button's release is a last move. After a drag it closes the border, as Lasso
    /// Select's does (Ruling R231), straight with Alt unless Alt was already down at its press;
    /// after a click the border stays open. Another button's release changes nothing.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        let Some((_, _, alt_at_press)) = self.press.filter(|&(_, pressed, _)| pressed == b) else {
            return false;
        };
        self.on_mouse_move(app, x, y, i);
        self.press = None;
        let opening = std::mem::take(&mut self.opening);
        if std::mem::take(&mut self.dragged) && self.active() {
            if click_points(app, ToolType::MagneticLasso) && !opening {
                // Click points: the drag's end is a fastening point; the border stays open.
                self.catch_up(app, usize::MAX);
                if self.freehand {
                    self.end_freehand();
                } else {
                    self.fasten_live();
                }
            } else {
                let straight = app.alt_key_down && !alt_at_press;
                self.close(app, straight, false);
            }
        } else {
            self.catch_up(app, usize::MAX);
            self.end_freehand();
        }
        self.show(app);
        self.update_cursor(app);
        true
    }

    /// The pointer's waiting steps trace, a bounded number a frame.
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        if !self.pending.is_empty() {
            self.catch_up(app, STEPS_PER_FRAME);
            self.show(app);
        }
    }

    fn needs_ticks(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Enter closes the border along the edges (Alt+Enter straight), Esc cancels it, Backspace
    /// and Delete remove the last fastening point; `[` and `]` change the width by 1.
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        if key.ctrl || key.meta {
            return false;
        }
        match key.key.as_str() {
            "[" | "]" => {
                let step = if key.key == "[" { -1.0 } else { 1.0 };
                let w = (settings(app).width.round() + step).clamp(1.0, 256.0);
                app.update_tool_setting(ToolType::MagneticLasso, "width", Value::from(w as i64));
            }
            _ if !self.active() => return false,
            "Enter" => self.close(app, key.alt, false),
            "Escape" => self.cancel(app),
            "Backspace" | "Delete" => {
                self.catch_up(app, usize::MAX);
                self.remove_last_point(app);
            }
            _ => return false,
        }
        self.show(app);
        self.update_cursor(app);
        true
    }

    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        self.update_cursor(app);
    }

    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
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
    use crate::render::selection::SelectionRenderer;
    use crate::selection::CombineMode;
    use crate::testing::{Ev, drive_tool, fresh_doc, with_tool};
    use crate::util::geom::Rectangle;
    use MouseButton::Left;

    /// The white square's edges, on a black layer.
    const SQUARE: (f64, f64, f64, f64) = (50.0, 40.0, 130.0, 110.0);

    /// A white 80×70 square on an opaque black 200×150 layer, 100 % zoom.
    fn square_doc() -> AppWorkspace {
        let mut app = fresh_doc(200, 150, None);
        paint_square(&mut app, SQUARE);
        app
    }

    /// The layer black with a white `(x0, y0, x1, y1)` square.
    fn paint_square(app: &mut AppWorkspace, (x0, y0, x1, y1): (f64, f64, f64, f64)) {
        let ws = app.active_ws_mut().unwrap();
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        for (i, px) in layer
            .surface
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let (x, y) = ((i % 200) as f64, (i / 200) as f64);
            let v = if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                255
            } else {
                0
            };
            *px = [v, v, v, 255];
        }
        ws.invalidate(None);
    }

    /// The first click 3 px outside the top-left corner, then clicks outside the other three.
    fn around() -> Vec<Ev> {
        [(52.0, 37.0), (133.0, 37.0), (133.0, 113.0), (47.0, 113.0)]
            .into_iter()
            .flat_map(|(x, y)| [Ev::Down(x, y, Left), Ev::Up(x, y, Left)])
            .collect()
    }

    /// The history items after the document's "New Image".
    fn undo_names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.history.undo[0].name, "New Image");
        ws.history.undo[1..]
            .iter()
            .map(|m| m.name.clone())
            .collect()
    }

    /// Click points: a later drag's release fastens a point and the border stays open until it
    /// is closed.
    #[test]
    fn click_points_keep_the_border_open_after_drags() {
        let mut app = square_doc();
        app.tool_settings
            .set(ToolType::MagneticLasso, "clickPoints", true.into());
        let events = [
            Ev::Down(52.0, 37.0, Left),
            Ev::Up(52.0, 37.0, Left),
            Ev::Down(90.0, 37.0, Left),
            Ev::Move(110.0, 37.0),
            Ev::Move(133.0, 37.0),
            Ev::Up(133.0, 37.0, Left),
        ];
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        let (anchors, _) = border(&mut app);
        assert!(anchors.len() >= 3, "{anchors:?}");
        assert!(undo_names(&app).is_empty(), "the border is still open");
        drive_tool(&mut app, ToolType::MagneticLasso, &[Ev::Key("Enter")]);
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
    }

    /// (fastening points, border points) of the border being drawn.
    fn border(app: &mut AppWorkspace) -> (Vec<usize>, Vec<[f64; 2]>) {
        let mut out = Default::default();
        with_tool::<MagneticLassoTool>(app, |t, _| {
            out = (t.anchors.clone(), t.path.clone());
        });
        out
    }

    /// A selection to keep or replace: Rect(5, 5, 20, 20), with no history.
    fn select_rect(app: &mut AppWorkspace) {
        let ws = app.active_ws_mut().unwrap();
        let r = Rectangle::new(5.0, 5.0, 20.0, 20.0);
        ws.selection.set_continuation(&r, CombineMode::Replace);
        ws.selection.commit_continuation();
    }

    /// S§4.3: clicks outside three corners and Enter select the square along its edges.
    #[test]
    fn lasso_traces_a_square_edge() {
        let mut app = square_doc();
        let mut events = around();
        events.push(Ev::Key("Enter"));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        let path = app.active_ws().unwrap().selection.create_path(true);
        let (x0, y0, x1, y1) = SQUARE;
        let points: Vec<_> = path.polygons.iter().flatten().collect();
        assert!(points.len() >= 4, "{path:?}");
        for p in points {
            let inside =
                (x0 - 1.5..=x1 + 1.5).contains(&p.x) && (y0 - 1.5..=y1 + 1.5).contains(&p.y);
            let near = [p.x - x0, p.x - x1, p.y - y0, p.y - y1]
                .iter()
                .any(|d| d.abs() <= 1.5);
            assert!(inside && near, "{p:?} is off the square's edge");
        }
        let b = path.get_bounds();
        let got = [b.left(), b.top(), b.right(), b.bottom()];
        for (g, w) in got.iter().zip([x0, y0, x1, y1]) {
            assert!((g - w).abs() <= 1.5, "bounds {got:?}");
        }
        let mask = crate::testing::selection_mask(&app, 200, 150);
        let area: f64 = mask.iter().map(|&v| f64::from(v) / 255.0).sum();
        assert!(
            (area - 5600.0).abs() < 120.0,
            "the square's 80 × 70: {area}"
        );
    }

    /// The pointer (button up) moves along `pts` in 2 px steps.
    fn hover(pts: &[[f64; 2]]) -> Vec<Ev> {
        let mut out = vec![];
        for w in pts.windows(2) {
            let n = (dist(w[0], w[1]) / 2.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                let t = k as f64 / n as f64;
                let (dx, dy) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
                out.push(Ev::Move(w[0][0] + dx * t, w[0][1] + dy * t));
            }
        }
        out
    }

    /// Upstream's `tracing_around_a_shape_selects_it`: moving round the square fastens points on
    /// its edges by themselves, nothing is selected meanwhile, and a click on the start closes.
    /// A double-click closes too.
    #[test]
    fn hovering_fastens_points_and_clicks_close() {
        let mut app = square_doc();
        let ring = [
            [52.0, 37.0],
            [133.0, 37.0],
            [133.0, 113.0],
            [47.0, 113.0],
            [47.0, 37.0],
        ];
        let mut events = vec![Ev::Down(52.0, 37.0, Left), Ev::Up(52.0, 37.0, Left)];
        events.extend(hover(&ring));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        let (anchors, path) = border(&mut app);
        // About every 47 px at Frequency 57.
        assert!(anchors.len() >= 5, "{anchors:?}");
        let (x0, y0, x1, y1) = SQUARE;
        for p in &path {
            let near = [p[0] - x0, p[0] - x1, p[1] - y0, p[1] - y1];
            assert!(near.iter().any(|d| d.abs() <= 1.0), "{p:?} is off the edge");
        }
        assert!(undo_names(&app).is_empty());
        // The border shows on Lasso Select's live layer while it is drawn.
        let shown = |app: &AppWorkspace| !live_points(app).is_empty();
        assert!(shown(&app));
        let mut events = hover(&[[47.0, 37.0], [52.0, 37.0]]);
        events.extend([Ev::Down(52.0, 37.0, Left), Ev::Up(52.0, 37.0, Left)]);
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!(border(&mut app).1.is_empty() && !shown(&app));
        let mask = crate::testing::selection_mask(&app, 200, 150);
        let area: f64 = mask.iter().map(|&v| f64::from(v) / 255.0).sum();
        assert!(
            (area - 5600.0).abs() < 120.0,
            "the square's 80 × 70: {area}"
        );
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
        // Over the top and right edges, then a double-click: the rest closes along the edges.
        let mut events = vec![Ev::Down(52.0, 37.0, Left), Ev::Up(52.0, 37.0, Left)];
        events.extend(hover(&ring[..3]));
        let (x, y) = (133.0, 113.0);
        events.extend([
            Ev::Down(x, y, Left),
            Ev::Up(x, y, Left),
            Ev::Down(x, y, Left),
        ]);
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!(border(&mut app).1.is_empty());
        assert_eq!(undo_names(&app), ["Magnetic Lasso", "Magnetic Lasso"]);
    }

    /// Ruling R38: the tracer's pixels follow the layer's content, so after the pixels change
    /// mid-border (an undo, an effect) the next trace follows the new edges.
    #[test]
    fn the_tracer_follows_changed_pixels() {
        let mut app = square_doc();
        let click = |x, y| [Ev::Down(x, y, Left), Ev::Up(x, y, Left)];
        drive_tool(&mut app, ToolType::MagneticLasso, &click(52.0, 37.0));
        // The square's top edge moves down from y = 40 to y = 46.
        paint_square(&mut app, (50.0, 46.0, 130.0, 110.0));
        drive_tool(&mut app, ToolType::MagneticLasso, &click(133.0, 37.0));
        let (_, path) = border(&mut app);
        // Where the border crosses x = 90.
        let y = path.windows(2).find_map(|w| {
            let ([ax, ay], [bx, by]) = (w[0], w[1]);
            (ax.min(bx) <= 90.0 && 90.0 <= ax.max(bx) && ax != bx)
                .then(|| ay + (by - ay) * (90.0 - ax) / (bx - ax))
        });
        assert!(
            y.is_some_and(|y| (y - 46.0).abs() <= 1.5),
            "{y:?} in {path:?}"
        );
    }

    /// Ruling R39, Lasso Select's overrides: an Exclude border disjoint from the selection
    /// reverts, with no history item.
    #[test]
    fn exclude_disjoint_reverts_without_history() {
        let mut app = square_doc();
        select_rect(&mut app);
        let before = app.active_ws().unwrap().selection.save();
        let mut events = around();
        events[0] = Ev::DownMods(52.0, 37.0, Left, "Alt");
        events.push(Ev::Key("Enter"));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!(border(&mut app).1.is_empty());
        assert_eq!(app.active_ws().unwrap().selection.save(), before);
        assert!(undo_names(&app).is_empty());
    }

    /// Ruling R39: a Replace or Intersect border off the canvas (its points stay on the image's
    /// edge, so it has no area there) deselects, recording "Deselect", as Lasso Select does.
    #[test]
    fn off_canvas_replace_and_intersect_deselect() {
        for (b, m) in [(Left, ""), (MouseButton::Right, "Alt")] {
            let mut app = square_doc();
            select_rect(&mut app);
            let events = [
                Ev::DownMods(-20.0, 50.0, b, m),
                Ev::Up(-20.0, 50.0, b),
                Ev::Down(-20.0, 100.0, Left),
                Ev::Up(-20.0, 100.0, Left),
                Ev::Key("Enter"),
            ];
            drive_tool(&mut app, ToolType::MagneticLasso, &events);
            assert!(border(&mut app).1.is_empty(), "{b:?}+{m}");
            assert!(app.active_ws().unwrap().selection.is_empty(), "{b:?}+{m}");
            assert_eq!(undo_names(&app), ["Deselect"], "{b:?}+{m}");
        }
    }

    /// Backspace and Delete take the last fastening point and the border back to the one before;
    /// with only the first left, the border cancels.
    #[test]
    fn backspace_removes_the_last_point() {
        let mut app = square_doc();
        // The start and two clicks: three fastening points.
        drive_tool(&mut app, ToolType::MagneticLasso, &around()[..6]);
        let (anchors, path) = border(&mut app);
        assert_eq!(anchors.len(), 3, "{anchors:?}");
        drive_tool(&mut app, ToolType::MagneticLasso, &[Ev::Key("Backspace")]);
        let (after, rest) = border(&mut app);
        assert_eq!(after, anchors[..2]);
        assert_eq!(rest, path[..anchors[1] + 1]);
        drive_tool(&mut app, ToolType::MagneticLasso, &[Ev::Key("Delete")]);
        assert_eq!(border(&mut app).0, [0]);
        drive_tool(&mut app, ToolType::MagneticLasso, &[Ev::Key("Backspace")]);
        assert!(border(&mut app).1.is_empty());
        assert!(undo_names(&app).is_empty());
        assert!(app.active_ws().unwrap().selection.is_empty());
    }

    /// Esc drops the border and leaves the selection and the history as they were.
    #[test]
    fn esc_cancels_without_history() {
        let mut app = square_doc();
        select_rect(&mut app);
        let before = app.active_ws().unwrap().selection.save();
        drive_tool(&mut app, ToolType::MagneticLasso, &around());
        assert!(!border(&mut app).1.is_empty());
        drive_tool(&mut app, ToolType::MagneticLasso, &[Ev::Key("Escape")]);
        assert!(border(&mut app).1.is_empty());
        assert_eq!(app.active_ws().unwrap().selection.save(), before);
        assert!(undo_names(&app).is_empty());
    }

    /// The close is one Selection history item, "Magnetic Lasso"; undo restores the selection.
    #[test]
    fn magnetic_lasso_is_one_history_item() {
        let mut app = square_doc();
        select_rect(&mut app);
        let before = app.active_ws().unwrap().selection.save();
        let mut events = around();
        events.push(Ev::Key("Enter"));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
        assert_ne!(app.active_ws().unwrap().selection.save(), before);
        crate::action::perform(&mut app, "menu.edit.undo");
        assert_eq!(app.active_ws().unwrap().selection.save(), before);
    }

    /// The square's ring, 3 px outside its corners, back to the start.
    const RING: [[f64; 2]; 5] = [
        [52.0, 37.0],
        [133.0, 37.0],
        [133.0, 113.0],
        [47.0, 113.0],
        [52.0, 37.0],
    ];

    /// A left-button drag along `pts` in 2 px steps; `release` lets go at the end.
    fn drag(pts: &[[f64; 2]], release: bool) -> Vec<Ev> {
        let [x, y] = pts[0];
        let mut out = vec![Ev::Down(x, y, Left)];
        out.extend(hover(pts));
        if release {
            let [x, y] = pts[pts.len() - 1];
            out.push(Ev::Up(x, y, Left));
        }
        out
    }

    /// Each selection layer as the renderer reads it, its geometry reduced to "has a path".
    fn looks(app: &AppWorkspace) -> Vec<SelectionRenderer> {
        let ws = app.active_ws().unwrap();
        let layers = ws.surface_box.renderers.iter();
        layers
            .filter_map(|(_, r)| match r {
                Renderer::Selection(s) => Some(SelectionRenderer {
                    path: s.path.as_ref().map(|_| GraphicsPath::default()),
                    ..s.clone()
                }),
                _ => None,
            })
            .collect()
    }

    /// The live layers' (a tool's) geometry: the points of each visible one.
    fn live_points(app: &AppWorkspace) -> Vec<usize> {
        let ws = app.active_ws().unwrap();
        let layers = ws.surface_box.renderers.iter();
        layers
            .filter_map(|(_, r)| match r {
                Renderer::Selection(s) if s.visible => s.path.as_ref(),
                _ => None,
            })
            .map(|p| p.polygons.iter().map(Vec::len).sum())
            .collect()
    }

    fn area(app: &AppWorkspace) -> f64 {
        let mask = crate::testing::selection_mask(app, 200, 150);
        mask.iter().map(|&v| f64::from(v) / 255.0).sum()
    }

    /// Ruling R231: a drag round the square and a release select it along its edges, as one
    /// "Magnetic Lasso" history item, and leave no border behind.
    #[test]
    fn a_drag_selects_on_release() {
        let mut app = square_doc();
        drive_tool(&mut app, ToolType::MagneticLasso, &drag(&RING, true));
        assert!(border(&mut app).1.is_empty(), "the border lingers");
        with_tool::<MagneticLassoTool>(&mut app, |t, _| {
            assert!(t.live.is_empty() && t.pending.is_empty() && t.press.is_none());
        });
        assert!((area(&app) - 5600.0).abs() < 120.0, "{}", area(&app));
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
        assert!(live_points(&app).is_empty(), "a live layer still shows");
        // A second drag is a new border: one more item, nothing left over.
        drive_tool(&mut app, ToolType::MagneticLasso, &drag(&RING, true));
        assert!(border(&mut app).1.is_empty());
        assert_eq!(undo_names(&app), ["Magnetic Lasso", "Magnetic Lasso"]);
    }

    /// Ruling R231: the path shows from the first move, before any tracing (the raw pointer),
    /// and keeps showing while the button is held.
    #[test]
    fn the_path_shows_from_the_first_move() {
        let mut app = square_doc();
        drive_tool(
            &mut app,
            ToolType::MagneticLasso,
            &[Ev::Down(52.0, 37.0, Left)],
        );
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        app.on_mouse_move(60.0, 37.0, &input);
        assert!(
            live_points(&app).iter().any(|&n| n >= 2),
            "{:?}",
            live_points(&app)
        );
        drive_tool(&mut app, ToolType::MagneticLasso, &hover(&RING[..3]));
        assert!(live_points(&app).iter().any(|&n| n >= 3));
        // The continuation is the border: the tint fills it.
        assert!(!app.active_ws().unwrap().selection.is_empty());
    }

    /// Ruling R231: drawing looks as Lasso Select's does: the same layers (the tinted workspace
    /// selection without its outline, the live layer's marching ants), so the same pixels.
    #[test]
    fn drawing_looks_like_lasso_select() {
        let mut lasso = square_doc();
        drive_tool(&mut lasso, ToolType::LassoSelect, &drag(&RING[..3], false));
        let mut magnetic = square_doc();
        drive_tool(
            &mut magnetic,
            ToolType::MagneticLasso,
            &drag(&RING[..3], false),
        );
        assert_eq!(looks(&magnetic), looks(&lasso));
        // Released, both show only the workspace selection, tinted and outlined.
        drive_tool(
            &mut lasso,
            ToolType::LassoSelect,
            &[Ev::Up(133.0, 113.0, Left)],
        );
        drive_tool(
            &mut magnetic,
            ToolType::MagneticLasso,
            &[Ev::Up(133.0, 113.0, Left)],
        );
        assert_eq!(looks(&magnetic), looks(&lasso));
    }

    /// Ruling R231: Esc mid-drag cancels; the release after it selects nothing.
    #[test]
    fn esc_cancels_a_drag() {
        let mut app = square_doc();
        select_rect(&mut app);
        let before = app.active_ws().unwrap().selection.save();
        let mut events = drag(&RING[..3], false);
        events.extend([Ev::Key("Escape"), Ev::Up(133.0, 113.0, Left)]);
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!(border(&mut app).1.is_empty());
        assert_eq!(app.active_ws().unwrap().selection.save(), before);
        assert!(undo_names(&app).is_empty());
        assert!(live_points(&app).is_empty());
    }

    /// Ruling R231: a tool switch (or a selection command) mid-border leaves the selection as it
    /// was and no outline of the border behind.
    #[test]
    fn a_switch_mid_border_leaves_no_outline() {
        let mut app = square_doc();
        select_rect(&mut app);
        let before = app.active_ws().unwrap().selection.save();
        drive_tool(&mut app, ToolType::MagneticLasso, &drag(&RING[..3], false));
        app.set_active_tool_from_type(ToolType::RectangleSelect);
        assert_eq!(app.active_ws().unwrap().selection.save(), before);
        assert!(live_points(&app).is_empty());
        assert!(app.active_ws().unwrap().selection_renderer().outline);
        // Click mode, then Select All: only "Select All".
        let mut events = vec![Ev::Down(52.0, 37.0, Left), Ev::Up(52.0, 37.0, Left)];
        events.extend(hover(&RING[..3]));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        crate::action::perform(&mut app, "menu.edit.selectAll");
        assert!(border(&mut app).1.is_empty());
        assert_eq!(undo_names(&app), ["Select All"]);
        assert!(live_points(&app).is_empty());
    }

    /// Ruling R231: Lasso Select's modifiers: a Ctrl+drag adds to the selection.
    #[test]
    fn ctrl_drag_adds() {
        let mut app = square_doc();
        select_rect(&mut app);
        let mut events = drag(&RING, true);
        events[0] = Ev::DownMods(52.0, 37.0, Left, "Ctrl");
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!((area(&app) - 6000.0).abs() < 120.0, "{}", area(&app));
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
    }

    /// Review M2: only the button that started a drag ends it. A right click mid-drag (its
    /// press and release) leaves the border open; the left release then selects.
    #[test]
    fn another_buttons_release_does_not_end_a_drag() {
        let mut app = square_doc();
        let mut events = drag(&RING[..3], false);
        events.extend([
            Ev::Down(133.0, 113.0, MouseButton::Right),
            Ev::Up(133.0, 113.0, MouseButton::Right),
        ]);
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!(
            !border(&mut app).1.is_empty(),
            "the right release ended the drag"
        );
        assert!(undo_names(&app).is_empty());
        let mut events = hover(&RING[2..]);
        events.push(Ev::Up(52.0, 37.0, Left));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert!(border(&mut app).1.is_empty());
        assert!((area(&app) - 5600.0).abs() < 120.0, "{}", area(&app));
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
    }

    /// Review M3: Alt held from the press (Exclude) to the release closes along the edges, not
    /// straight. Released 10 px right of the bottom-left corner, the close runs along the bottom
    /// and left edges; a straight close would cut the corner (350 px less).
    #[test]
    fn alt_drag_closes_along_the_edges() {
        let mut app = square_doc();
        {
            let ws = app.active_ws_mut().unwrap();
            let r = Rectangle::new(0.0, 0.0, 200.0, 150.0);
            ws.selection.set_continuation(&r, CombineMode::Replace);
            ws.selection.commit_continuation();
        }
        let ring = [[52.0, 37.0], [133.0, 37.0], [133.0, 113.0], [60.0, 113.0]];
        let mut events = drag(&ring, true);
        events[0] = Ev::DownMods(52.0, 37.0, Left, "Alt");
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
        let excluded = 30000.0 - area(&app);
        assert!((excluded - 5600.0).abs() < 120.0, "excluded {excluded}");
    }

    /// Review M4: Alt is read per press. In click mode an Alt-click starts the border (Exclude);
    /// a later press without Alt is dragged, Alt goes down during the drag and is held at the
    /// release: the close is straight and cuts the bottom-left corner (350 px less).
    #[test]
    fn alt_pressed_during_a_later_drag_closes_straight() {
        let mut app = square_doc();
        {
            let ws = app.active_ws_mut().unwrap();
            let r = Rectangle::new(0.0, 0.0, 200.0, 150.0);
            ws.selection.set_continuation(&r, CombineMode::Replace);
            ws.selection.commit_continuation();
        }
        let mut events = vec![
            Ev::DownMods(52.0, 37.0, Left, "Alt"),
            Ev::Up(52.0, 37.0, Left),
        ];
        events.extend(hover(&[[52.0, 37.0], [133.0, 37.0], [133.0, 113.0]]));
        events.extend(drag(&[[133.0, 113.0], [96.0, 113.0]], false));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        app.set_modifier_keys(false, false, true);
        let mut events = hover(&[[96.0, 113.0], [60.0, 113.0]]);
        events.push(Ev::Up(60.0, 113.0, Left));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        assert_eq!(undo_names(&app), ["Magnetic Lasso"]);
        let excluded = 30000.0 - area(&app);
        assert!((excluded - 5250.0).abs() < 120.0, "excluded {excluded}");
    }

    /// Ruling R231: tracing is bounded a frame. A long move waits, drawn straight, and traces
    /// `STEPS_PER_FRAME` steps a frame, the UI ticking meanwhile; the release traces the rest.
    #[test]
    fn tracing_is_bounded_a_frame() {
        let mut app = square_doc();
        drive_tool(
            &mut app,
            ToolType::MagneticLasso,
            &[Ev::Down(52.0, 37.0, Left)],
        );
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        // Down the right side and back along the bottom: ~11 steps of 23.5 px.
        app.on_mouse_move(133.0, 37.0, &input);
        app.on_mouse_move(133.0, 113.0, &input);
        app.on_mouse_move(47.0, 113.0, &input);
        let waiting = |app: &mut AppWorkspace| {
            let mut n = 0;
            with_tool::<MagneticLassoTool>(app, |t, _| n = t.pending.len());
            n
        };
        let n = waiting(&mut app);
        assert!(n > STEPS_PER_FRAME, "{n}");
        // Drawn straight meanwhile: the start and the three corners at least.
        assert!(live_points(&app).iter().any(|&p| p >= 4));
        assert!(app.tool_ticking());
        app.on_frame(app.now_ms + 16.0);
        assert_eq!(waiting(&mut app), n - STEPS_PER_FRAME);
        app.on_mouse_up(52.0, 37.0, Left, &input);
        assert!((area(&app) - 5600.0).abs() < 120.0, "{}", area(&app));
        assert!(!app.tool_ticking());
    }

    /// An undo mid-border drops the border first, then steps the selection it began with.
    #[test]
    fn undo_mid_border_cancels_it() {
        let mut app = square_doc();
        drive_tool(&mut app, ToolType::MagneticLasso, &drag(&RING, true));
        let mut events = vec![Ev::Down(52.0, 37.0, Left), Ev::Up(52.0, 37.0, Left)];
        events.extend(hover(&RING[..3]));
        drive_tool(&mut app, ToolType::MagneticLasso, &events);
        crate::action::perform(&mut app, "menu.edit.undo");
        assert!(border(&mut app).1.is_empty());
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert!(undo_names(&app).is_empty());
        assert!(live_points(&app).is_empty());
        assert!(app.active_ws().unwrap().selection_renderer().outline);
    }

    /// A drag far off the image, zoomed out: the border follows the image's edge and stays about
    /// as long as the pointer's path there. It once went back and forth along the edge, every
    /// pass fastening another point (thousands of points, and a stall, a move).
    #[test]
    fn a_drag_off_the_image_follows_its_edge() {
        let mut app = crate::testing::fresh_doc(1000, 750, Some(7));
        app.active_ws_mut().unwrap().view.zoom = 0.2;
        let input = PointerInput {
            pointer_type: super::super::PointerType::Mouse,
            samples: vec![],
        };
        drive_tool(&mut app, ToolType::MagneticLasso, &[]);
        // Down the right of the image, 2500 px out, in 100-px moves.
        let points = (0..=6).map(|k| [3500.0, 100.0 * k as f64]);
        for (k, [x, y]) in points.enumerate() {
            with_tool::<MagneticLassoTool>(&mut app, |t, app| {
                if k == 0 {
                    t.on_mouse_down(app, x, y, Left, &input);
                } else {
                    t.on_mouse_move(app, x, y, &input);
                }
                t.on_frame(app);
            });
        }
        let (anchors, path) = border(&mut app);
        let long = magnetic::length(&path);
        assert!(
            long < 2000.0,
            "{long} px, {} points, {} anchors",
            path.len(),
            anchors.len()
        );
    }
}
