//! Photocraft phase spec: S§4.3 "Remove", "Resource limits"
//!
//! The Remove tool: paint over or circle an object and it is filled from its surroundings by
//! PhotoCraft's non-local inpainting (`extra::remove`, upstream's `retouch_cmds/remove.rs`
//! pipeline). The stroke is Paint.NET's brush (Task 30's engine) and its coverage > 0 is the hole;
//! with "Remove after each stroke" off, strokes collect as a 50 % red tint until Enter (Esc drops
//! them). The fill runs as a background job (`extra::job`): the busy cursor shows, Esc cancels
//! it, and the result lands as one "Remove" entry, written only where the hole meets the
//! selection.

use std::sync::Arc;

use serde_json::Value;

use super::base::ToolBase;
use super::brush::{BrushSettings, BrushStroke, StrokeInput};
use super::drawing::{hair_width, read_brush, size_key};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::action::UiRequest;
use crate::app::AppWorkspace;
use crate::canvas::ImageData;
use crate::document::workspace::DocumentWorkspace;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Cancelled;
use crate::extra::job::Job;
use crate::extra::remove::{MAX_PIXELS, close_loops, dilate, remove_with, window_for};
use crate::extra::sample::layer_or_image;
use crate::extra::shim::{from_f32, to_f32};
use crate::pixel::IntRect;
use crate::pixel::mask::{self, SelectionQuality};
use crate::render::nubs::{BrushPreview, Tint};
use crate::render::{Renderer, RendererId};

/// The over-cap message (S§4.3).
pub const TOO_LARGE: &str = "The area is too large to remove.";

/// How far the hole grows beyond the stroke, in pixels (upstream's `GROW`).
const GROW: usize = 2;
/// The completion's seed, so a removal is reproducible (upstream's `SEED`).
const SEED: u64 = 0x0052_e30e;
/// The strokes' tint: red at 50 %.
const TINT: [u8; 4] = [255, 0, 0, 128];

/// Strokes as one mask: 0 or 255 over the canvas rect `rect`.
#[derive(Clone, Debug, Default)]
struct Strokes {
    rect: IntRect,
    mask: Vec<u8>,
}

impl Strokes {
    fn union(&self, o: &Strokes) -> Strokes {
        let rect = self.rect.union(&o.rect);
        let mut mask = vec![0; rect.w as usize * rect.h as usize];
        for s in [self, o] {
            let w = s.rect.w as usize;
            for (y, src) in s.mask.chunks_exact(w.max(1)).enumerate() {
                let row = (s.rect.y - rect.y) as usize + y;
                let at = row * rect.w as usize + (s.rect.x - rect.x) as usize;
                for (d, v) in mask[at..at + w].iter_mut().zip(src) {
                    *d |= v;
                }
            }
        }
        Strokes { rect, mask }
    }
}

/// The stroke's coverage > 0 over `area`, trimmed to its bounding box.
fn covered(stroke: &BrushStroke, area: IntRect) -> Strokes {
    if area.is_empty() {
        return Strokes::default();
    }
    let m = stroke.mask(area);
    let w = area.w as usize;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in m.chunks_exact(w).enumerate() {
        if let (Some(a), Some(b)) = (
            row.iter().position(|v| *v > 0),
            row.iter().rposition(|v| *v > 0),
        ) {
            (x0, x1, y0, y1) = (x0.min(a), x1.max(b + 1), y0.min(y), y + 1);
        }
    }
    if x0 == usize::MAX {
        return Strokes::default();
    }
    let rect = IntRect::new(
        area.x + x0 as i32,
        area.y + y0 as i32,
        (x1 - x0) as u32,
        (y1 - y0) as u32,
    );
    let mask = (y0..y1)
        .flat_map(|y| {
            m[y * w + x0..y * w + x1]
                .iter()
                .map(|v| if *v > 0 { 255 } else { 0 })
        })
        .collect();
    Strokes { rect, mask }
}

fn setting(app: &AppWorkspace, key: &str) -> Value {
    app.tool_settings.get(ToolType::Remove, key)
}

/// The brush, sanitized as the brush tools', and the pressure toggle.
fn brush(app: &AppWorkspace) -> (BrushSettings, bool) {
    read_brush(|k| Some(setting(app, k)))
}

/// What a finished removal writes: the hole over the window and the filled window.
type Filled = (Vec<bool>, ImageData);

/// What the job found: nothing to change (or nothing around the hole to fill it from), or the
/// filled window.
enum Outcome {
    Nothing,
    Filled(Filled),
}

/// A removal in progress: the job filling the window `rect` (`None` when nothing would change),
/// and the document and layer it lands in.
struct Removal {
    job: Job<Outcome>,
    rect: IntRect,
    doc: u64,
    layer: usize,
}

impl Removal {
    /// Upstream's `plan`: the window around the strokes `s`, and on the UI thread only its
    /// selection clip and its pixels (the active layer's, or the image's with `sample_image`).
    /// The hole and the fill run in the job (upstream's `run_remove`). Over the cap the message
    /// instead; `None` when there is nothing to do.
    fn start(app: &mut AppWorkspace, s: Strokes, sample_image: bool) -> Option<Removal> {
        if u64::from(s.rect.w) * u64::from(s.rect.h) > MAX_PIXELS {
            app.ui_requests.push(UiRequest::ToolError(TOO_LARGE.into()));
            return None;
        }
        let ws = app.active_ws()?;
        let r = s.rect;
        let canvas = Rect::new(0, 0, ws.document.width as i32, ws.document.height as i32);
        let win = window_for(Rect::new(r.x, r.y, r.right(), r.bottom()), canvas);
        if win.is_empty() {
            return None;
        }
        let rect = IntRect::new(win.x0, win.y0, win.width(), win.height());
        let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
        // The selection's polygons are taken here; the job rasterizes them over the window.
        let polys = (!ws.selection.is_empty()).then(|| {
            let doc = IntRect::new(0, 0, ws.document.width, ws.document.height);
            ws.selection.clipping_polygons(doc)
        });
        let img = layer_or_image(app, sample_image, win);
        let job = Job::spawn(move |ctl| {
            let clip = polys.map(|p| mask::rasterize(&p, quality, rect).data);
            let Some(hole) = hole(&s, rect, clip) else {
                return Ok(Outcome::Nothing);
            };
            // A transparent layer has nothing to remove and nothing to fill from (upstream's
            // no-op).
            if !sample_image && img.data.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                return Ok(Outcome::Nothing);
            }
            let (w, h) = (rect.w as usize, rect.h as usize);
            let out = remove_with(w, h, 4, &to_f32(&img), &hole, SEED, ctl)?;
            Ok(Outcome::Filled((hole, from_f32(&out, w as u32, h as u32))))
        });
        Some(Removal {
            job,
            rect,
            doc: ws.document.id,
            layer: ws.active_layer,
        })
    }

    /// The filled pixels land where the hole is, as one entry `name` of the changed tiles; not
    /// when the active document or layer changed meanwhile.
    fn land(self, ws: &mut DocumentWorkspace, (hole, out): Filled, name: &str, icon: &str) {
        if (ws.document.id, ws.active_layer) != (self.doc, self.layer) {
            return;
        }
        let rect = self.rect;
        let mut base = ToolBase::default();
        base.begin_bitmap_transaction(ws);
        base.write_transaction(ws, rect, |_, layer| {
            let mut img = layer.get_image_data(rect.x, rect.y, rect.w, rect.h);
            let src = out.data.as_chunks::<4>().0;
            let px = img.data.as_chunks_mut::<4>().0.iter_mut().zip(src);
            for ((dst, src), m) in px.zip(&hole) {
                if *m {
                    *dst = *src;
                }
            }
            layer.put_image_data(&img, rect.x, rect.y);
            None
        });
        if let Some(m) = base.commit_changed_tiles(ws, name, icon) {
            ws.push_memento(m);
        }
    }
}

/// Upstream's `hole`: the strokes over the window `rect`, what they enclose and `GROW` more, ∩
/// the selection's `clip`. `None` when nothing would change or nothing is left around the hole to
/// fill it from.
fn hole(s: &Strokes, rect: IntRect, clip: Option<Vec<u8>>) -> Option<Vec<bool>> {
    let (r, w, h) = (s.rect, rect.w as usize, rect.h as usize);
    let mut hole = vec![false; w * h];
    for (y, src) in s.mask.chunks_exact(r.w as usize).enumerate() {
        let at = ((r.y - rect.y) as usize + y) * w + (r.x - rect.x) as usize;
        for (m, v) in hole[at..at + r.w as usize].iter_mut().zip(src) {
            *m = *v > 0;
        }
    }
    close_loops(&mut hole, w, h);
    let mut hole = dilate(&hole, w, h, GROW);
    for (m, c) in hole.iter_mut().zip(clip.into_iter().flatten()) {
        *m &= c > 0;
    }
    (hole.contains(&true) && hole.contains(&false)).then_some(hole)
}

#[derive(Default)]
pub struct RemoveTool {
    /// The stroke being drawn: its input, its coverage and the layer pixels it reached.
    stroke: Option<(StrokeInput, BrushStroke, IntRect)>,
    /// The strokes collected while "Remove after each stroke" is off.
    pending: Strokes,
    running: Option<Removal>,
    tint: Option<RendererId>,
    preview: Option<RendererId>,
    /// The pointer centre (canvas px) the brush outline follows.
    pointer: Option<[f64; 2]>,
}

impl RemoveTool {
    /// The tint shows the collected strokes and the one being drawn.
    // ponytail: the whole tint is rebuilt per pointer move (O(stroke bounds)); keep it
    // incremental if huge strokes stutter.
    fn show(&mut self, app: &mut AppWorkspace) {
        let shown = match &self.stroke {
            Some((_, stroke, area)) => self.pending.union(&covered(stroke, *area)),
            None => self.pending.clone(),
        };
        let tint = Tint {
            rect: shown.rect,
            mask: Arc::new(shown.mask),
            color: TINT,
            pixels: None,
        };
        if let (Some(id), Some(ws)) = (self.tint, app.active_ws_mut())
            && let Some(Renderer::Tint(t)) = ws.surface_box.renderer_mut(id)
        {
            *t = tint;
        }
    }

    /// The brush outline (radius Size/2) at the pointer while it is over the canvas or drawing.
    fn sync_preview(&mut self, app: &mut AppWorkspace) {
        let shown = self.pointer.is_some() && (self.stroke.is_some() || app.zoom_anchor.is_some());
        let p = BrushPreview {
            canvas: self.pointer.unwrap_or_default(),
            radius: brush(app).0.size / 2.0,
            pressure: None,
            opacity: if shown { 1.0 } else { 0.0 },
        };
        if let (Some(id), Some(ws)) = (self.preview, app.active_ws_mut())
            && !matches!(ws.surface_box.renderer(id), Some(Renderer::BrushPreview(c)) if *c == p)
            && let Some(Renderer::BrushPreview(r)) = ws.surface_box.renderer_mut(id)
        {
            *r = p;
        }
    }

    /// The pointer moved to `(x, y)` (its device pixel's top-left); the centre is pos + hw/2.
    fn track(&mut self, app: &AppWorkspace, x: f64, y: f64) {
        let hw = hair_width(app);
        self.pointer = Some([x + hw / 2.0, y + hw / 2.0]);
    }

    /// The pointer's samples join the stroke (every coalesced one; the move position when none).
    fn add(&mut self, app: &mut AppWorkspace, x: f64, y: f64, i: &PointerInput) {
        let Some((input, stroke, area)) = &mut self.stroke else {
            return;
        };
        if i.samples.is_empty() {
            input.push(x, y, None);
        }
        for s in &i.samples {
            input.push(s.x, s.y, s.pressure);
        }
        stroke.add(input);
        *area = area.union(&stroke.dirty());
        self.show(app);
    }

    /// Removes the collected strokes; the tint stays while the job runs. While a removal runs
    /// they stay collected instead (Enter removes them after it): it is never replaced.
    fn run(&mut self, app: &mut AppWorkspace) {
        if self.running.is_none() {
            let sample_image = setting(app, "sampleImage").as_bool().unwrap_or(false);
            let strokes = std::mem::take(&mut self.pending);
            self.running = Removal::start(app, strokes, sample_image);
            if self.running.is_some() {
                // On wasm the job already ran: it lands now.
                return self.poll(app);
            }
        }
        self.show(app);
    }

    /// A finished job lands as one "Remove" entry.
    fn poll(&mut self, app: &mut AppWorkspace) {
        let Some(result) = self.running.as_mut().and_then(|r| r.job.poll()) else {
            return;
        };
        if let Some(r) = self.running.take() {
            self.finish(app, r, result);
        }
    }

    /// A stopped job's fill lands as one "Remove" entry.
    fn finish(&mut self, app: &mut AppWorkspace, r: Removal, result: Result<Outcome, Cancelled>) {
        self.show(app);
        if let (Ok(Outcome::Filled(filled)), Some(ws)) = (result, app.active_ws_mut()) {
            let name = crate::i18n::t("removeTool.name");
            r.land(ws, filled, &name, &ToolType::Remove.icon_src());
        }
    }

    /// R89: a running removal is waited for and lands; then the collected strokes (a held one
    /// joins them) are removed at once, waited for too. Nothing is dropped (past the cap, the
    /// message).
    fn land_now(&mut self, app: &mut AppWorkspace) {
        if let Some((_, stroke, area)) = self.stroke.take() {
            self.pending = self.pending.union(&covered(&stroke, area));
        }
        if let Some(mut r) = self.running.take() {
            let result = r.job.wait();
            self.finish(app, r, result);
        }
        if !self.pending.rect.is_empty() {
            self.run(app);
            if let Some(mut r) = self.running.take() {
                let result = r.job.wait();
                self.finish(app, r, result);
            }
        }
    }

    /// Esc, an undo: a running job stops and changes nothing; the stroke and the collected
    /// strokes are dropped.
    fn cancel(&mut self, app: &mut AppWorkspace) {
        if let Some(r) = self.running.take() {
            r.job.cancel();
        }
        self.stroke = None;
        self.pending = Strokes::default();
        self.show(app);
    }
}

impl Tool for RemoveTool {
    fn tool_type(&self) -> ToolType {
        ToolType::Remove
    }

    /// Adds the tint and the brush outline, both hidden.
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            let outline = BrushPreview {
                canvas: [0.0; 2],
                radius: 0.0,
                pressure: None,
                opacity: 0.0,
            };
            self.tint = Some(ws.surface_box.add_renderer(Renderer::Tint(Tint::default())));
            self.preview = Some(ws.surface_box.add_renderer(Renderer::BrushPreview(outline)));
        }
    }

    /// Every commit-first command (a tool, layer or document change, Save …) lands a running
    /// removal and removes the collected strokes, at once (R89).
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        self.land_now(app);
        if let Some(ws) = app.active_ws_mut() {
            for id in [self.tint.take(), self.preview.take()]
                .into_iter()
                .flatten()
            {
                ws.surface_box.remove_renderer(id);
            }
        }
    }

    /// Any non-Middle press starts a stroke; presses while a removal runs are swallowed.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if self.running.is_some() || self.stroke.is_some() {
            return true;
        }
        let Some(ws) = app.active_ws() else {
            return false;
        };
        if b == MouseButton::Middle {
            return false;
        }
        let (w, h) = (ws.document.width, ws.document.height);
        let (settings, pressure) = brush(app);
        let input = StrokeInput::new(hair_width(app), pressure);
        let stroke = BrushStroke::new(w, h, settings);
        self.stroke = Some((input, stroke, IntRect::default()));
        self.track(app, x, y);
        self.add(app, x, y, i);
        self.sync_preview(app);
        true
    }

    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, i: &PointerInput) -> bool {
        self.track(app, x, y);
        self.add(app, x, y, i);
        self.sync_preview(app);
        self.stroke.is_some()
    }

    /// The first release ends the stroke: it joins the collected ones, and with "Remove after
    /// each stroke" on they are removed at once.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        _b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        let Some((_, stroke, area)) = self.stroke.take() else {
            return false;
        };
        self.pending = self.pending.union(&covered(&stroke, area));
        if setting(app, "removeEachStroke").as_bool().unwrap_or(true) {
            self.run(app);
        } else {
            self.show(app);
        }
        self.sync_preview(app);
        true
    }

    /// Enter (not while drawing) removes the collected strokes; Esc cancels a running removal,
    /// else drops the strokes; `[` `]` change the size as the brush tools' do.
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        let drawing = self.stroke.is_some();
        if let Some(changed) = size_key(app, ToolType::Remove, key, drawing) {
            if changed {
                self.sync_preview(app);
            }
            return true;
        }
        let busy = self.running.is_some();
        let collected = !self.pending.rect.is_empty();
        match key.key.as_str() {
            "Enter" if !busy && !drawing && collected => self.run(app),
            "Escape" if busy || drawing || collected => self.cancel(app),
            _ => return false,
        }
        true
    }

    /// An undo or redo drops the strokes and cancels a running removal.
    fn on_executing_history_memento(&mut self, app: &mut AppWorkspace) {
        self.cancel(app);
    }

    /// A finished removal lands; the outline hides once the pointer left the canvas.
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.poll(app);
        self.sync_preview(app);
    }

    fn needs_ticks(&self) -> bool {
        self.running.is_some()
    }

    fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Css("crosshair")
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::UiRequest;
    use crate::app::AppWorkspace;
    use crate::render::Renderer;
    use crate::selection::CombineMode;
    use crate::testing::{Ev, drive_tool, fresh_doc, layer_bytes};
    use crate::tool::MouseButton::Left;
    use crate::tool::types::{ToolSettings, option_definitions_with_fonts};
    use crate::util::geom::Rectangle;
    use serde_json::json;

    const T: ToolType = ToolType::Remove;

    /// Runs frames until the tool's job is done.
    fn finish(app: &mut AppWorkspace) {
        let start = std::time::Instant::now();
        while app.tool_busy() {
            assert!(start.elapsed().as_secs() < 120, "the job never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.on_frame(app.now_ms + 16.0);
        }
    }

    fn undo_names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    fn stroke(app: &mut AppWorkspace, from: (f64, f64), to: (f64, f64)) {
        drive_tool(
            app,
            T,
            &[
                Ev::Down(from.0, from.1, Left),
                Ev::Move(to.0, to.1),
                Ev::Up(to.0, to.1, Left),
            ],
        );
        finish(app);
    }

    /// The collected strokes' tint shows.
    fn tinted(app: &AppWorkspace) -> bool {
        let ws = app.active_ws().unwrap();
        ws.surface_box
            .renderers
            .iter()
            .any(|(_, r)| matches!(r, Renderer::Tint(t) if !t.rect.is_empty()))
    }

    /// A press and release over a dot on a flat white layer fills it with white.
    #[test]
    fn remove_fills_a_dot_on_a_flat_layer() {
        let mut app = fresh_doc(64, 64, None);
        let ws = app.active_ws_mut().unwrap();
        let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        assert!(s.data().iter().all(|&v| v == 255));
        for y in 29..35 {
            for x in 29..35 {
                s.data_mut()[(y * 64 + x) * 4..][..3].fill(0);
            }
        }
        ws.invalidate(None);
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(12));
        drive_tool(
            &mut app,
            T,
            &[Ev::Down(31.0, 31.0, Left), Ev::Up(31.0, 31.0, Left)],
        );
        finish(&mut app);
        let px = layer_bytes(&app, None);
        assert!(
            px.chunks(4)
                .all(|p| p[..3].iter().all(|&v| v >= 250) && p[3] == 255),
            "the dot is still there"
        );
        assert!(!tinted(&app));
    }

    /// Review Focus 2: a selection over the stroke's left half leaves its right half as it was.
    #[test]
    fn remove_writes_only_inside_selection() {
        let mut app = fresh_doc(64, 64, Some(2));
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 32.0, 64.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let before = layer_bytes(&app, None);
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(10));
        stroke(&mut app, (12.0, 30.0), (52.0, 30.0));
        let after = layer_bytes(&app, None);
        let half = |b: &[u8], right: bool| -> Vec<u8> {
            b.chunks(4 * 32)
                .enumerate()
                .filter(|(i, _)| (i % 2 == 1) == right)
                .flat_map(|(_, c)| c.to_vec())
                .collect()
        };
        assert_eq!(half(&before, true), half(&after, true));
        assert_ne!(half(&before, false), half(&after, false));
    }

    /// Review Focus 3: one removal is one "Remove" entry, and undo restores the bytes exactly.
    #[test]
    fn remove_is_one_history_item() {
        let mut app = fresh_doc(48, 48, Some(2));
        let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(8));
        stroke(&mut app, (10.0, 20.0), (30.0, 20.0));
        assert_ne!(layer_bytes(&app, None), before);
        let names = undo_names(&app);
        assert_eq!(names.len(), n + 1);
        assert_eq!(names.last().map(String::as_str), Some("Remove"));
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, None), before);
    }

    /// "Remove after each stroke" off: strokes collect as a tint; Enter removes them as one
    /// entry, Esc drops them.
    #[test]
    fn remove_collects_strokes_until_enter() {
        let mut app = fresh_doc(48, 48, Some(2));
        let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "removeEachStroke", json!(false));
        app.update_tool_setting(T, "width", json!(6));
        stroke(&mut app, (10.0, 10.0), (30.0, 10.0));
        stroke(&mut app, (10.0, 30.0), (30.0, 30.0));
        assert_eq!(layer_bytes(&app, None), before);
        assert_eq!(undo_names(&app).len(), n);
        assert!(tinted(&app));
        drive_tool(&mut app, T, &[Ev::Key("Enter")]);
        finish(&mut app);
        let removed = layer_bytes(&app, None);
        assert_ne!(removed, before);
        let names = undo_names(&app);
        assert_eq!(names.len(), n + 1);
        assert_eq!(names.last().map(String::as_str), Some("Remove"));
        assert!(!tinted(&app));
        stroke(&mut app, (10.0, 20.0), (30.0, 20.0));
        stroke(&mut app, (20.0, 5.0), (20.0, 40.0));
        assert!(tinted(&app));
        drive_tool(&mut app, T, &[Ev::Key("Escape")]);
        finish(&mut app);
        assert!(!tinted(&app));
        assert_eq!(layer_bytes(&app, None), removed);
        assert_eq!(undo_names(&app).len(), n + 1);
    }

    /// Review Focus 1: a stroke whose bounding box passes `MAX_PIXELS` shows the message and
    /// changes nothing.
    #[test]
    fn remove_over_cap_shows_message() {
        let mut app = fresh_doc(4100, 4100, None);
        let n = undo_names(&app).len();
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(20));
        stroke(&mut app, (0.0, 0.0), (4099.0, 4099.0));
        assert!(
            app.ui_requests
                .contains(&UiRequest::ToolError(TOO_LARGE.into()))
        );
        assert!(!app.tool_busy());
        assert_eq!(undo_names(&app).len(), n);
        assert!(layer_bytes(&app, None).iter().all(|&v| v == 255));
        assert!(!tinted(&app));
    }

    /// S§4.1: the reused brush controls carry no plus (pressure shows once a pen was seen);
    /// Sample and "Remove after each stroke" do.
    #[test]
    fn remove_toolbar_marks_only_its_own_options() {
        let keys = |settings: &ToolSettings| -> Vec<(&str, bool)> {
            let defs = option_definitions_with_fonts(T, settings, &[]);
            defs.iter().map(|d| (d.key, d.extra)).collect()
        };
        let mut want = vec![
            ("width", false),
            ("hardness", false),
            ("antialias", false),
            ("sampleImage", true),
            ("removeEachStroke", true),
            ("renderingQuality", false),
        ];
        assert_eq!(keys(&ToolSettings::default()), want);
        let mut pen = ToolSettings::default();
        pen.pen_seen = true;
        want.insert(1, ("pressure", false));
        assert_eq!(keys(&pen), want);
    }

    /// R28: cancelling a running removal (Esc, an undo) adds no entry and changes no pixel, also
    /// once frames run after it. (A tool switch lands it instead, R89.)
    #[test]
    fn remove_cancel_leaves_no_trace() {
        for case in ["Escape", "undo"] {
            // Big enough that the job is still running when the cancel comes.
            let mut app = fresh_doc(512, 512, Some(2));
            if case == "undo" {
                // Something to undo that changes no pixel.
                app.edit_selection(|ws| ws.execute_function(crate::history::functions::select_all));
            }
            let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
            drive_tool(&mut app, T, &[]);
            app.update_tool_setting(T, "width", json!(200));
            drive_tool(
                &mut app,
                T,
                &[Ev::Down(60.0, 256.0, Left), Ev::Move(452.0, 256.0)],
            );
            // The release starts the job; no frame runs before the cancel.
            let input = PointerInput {
                pointer_type: crate::tool::PointerType::Mouse,
                samples: vec![],
            };
            app.on_mouse_up(452.0, 256.0, Left, &input);
            assert!(app.tool_busy(), "{case}");
            let mut running = false;
            crate::testing::with_tool::<RemoveTool>(&mut app, |t, _| {
                running = t.running.as_ref().is_some_and(|r| r.job.is_running());
            });
            assert!(running, "{case}: the job is not running");
            match case {
                "Escape" => drive_tool(&mut app, T, &[Ev::Key("Escape")]),
                _ => assert!(app.undo()),
            }
            assert!(!app.tool_busy(), "{case}");
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                app.on_frame(app.now_ms + 16.0);
            }
            let n = if case == "undo" { n - 1 } else { n };
            assert_eq!(undo_names(&app).len(), n, "{case}");
            assert_eq!(layer_bytes(&app, None), before, "{case}");
        }
    }

    /// R89: a commit-first command waits for a running removal and lands it, and removes the
    /// collected strokes ("Remove after each stroke" off) instead of dropping them: a tool switch
    /// or Save lands one "Remove" entry.
    #[test]
    fn remove_commit_first_lands_the_removal() {
        for case in ["running", "collected, tool switch", "collected, Save"] {
            let mut app = fresh_doc(48, 48, Some(2));
            let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
            drive_tool(&mut app, T, &[]);
            app.update_tool_setting(T, "width", json!(6));
            if case == "running" {
                drive_tool(
                    &mut app,
                    T,
                    &[Ev::Down(10.0, 10.0, Left), Ev::Move(30.0, 10.0)],
                );
                // The release starts the job; no frame runs before the switch.
                let input = PointerInput {
                    pointer_type: crate::tool::PointerType::Mouse,
                    samples: vec![],
                };
                app.on_mouse_up(30.0, 10.0, Left, &input);
                assert!(app.tool_busy());
            } else {
                app.update_tool_setting(T, "removeEachStroke", json!(false));
                stroke(&mut app, (10.0, 10.0), (30.0, 10.0));
                assert!(tinted(&app));
            }
            if case == "collected, Save" {
                crate::action::perform(&mut app, "menu.file.save");
            } else {
                app.set_active_tool_from_type(ToolType::PaintBrush);
            }
            assert!(!app.tool_busy(), "{case}");
            assert_ne!(layer_bytes(&app, None), before, "{case}");
            assert_eq!(undo_names(&app)[n..], ["Remove"], "{case}");
        }
    }

    /// R28: on an empty layer above a textured one, Sample: Layer has nothing to fill from and
    /// changes nothing; Sample: Image fills the hole, and only the hole, from the composite.
    #[test]
    fn remove_sample_image_fills_an_empty_layer() {
        let mut app = fresh_doc(64, 64, Some(2));
        let ws = app.active_ws_mut().unwrap();
        // A flat known colour field below, so the filled colour can be told from its alpha.
        const FIELD: [u8; 4] = [200, 40, 40, 255];
        let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        for p in s.data_mut().as_chunks_mut::<4>().0 {
            *p = FIELD;
        }
        ws.invalidate(None);
        ws.add_layer(crate::document::layer::Layer::create_layer(64, 64, "Top"));
        ws.set_active_layer(1);
        let (bottom, n) = (layer_bytes(&app, Some(0)), undo_names(&app).len());
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(10));
        stroke(&mut app, (20.0, 32.0), (44.0, 32.0));
        assert!(layer_bytes(&app, None).iter().all(|&v| v == 0));
        assert_eq!(undo_names(&app).len(), n);
        app.update_tool_setting(T, "sampleImage", json!(true));
        stroke(&mut app, (20.0, 32.0), (44.0, 32.0));
        let top = layer_bytes(&app, None);
        for x in 20..=44 {
            let p = &top[(32 * 64 + x) * 4..][..3];
            assert!(
                p.iter().zip(FIELD).all(|(a, b)| a.abs_diff(b) <= 8),
                "the hole at x={x} is {p:?}, not the bottom layer's colour"
            );
        }
        let alpha: Vec<u8> = top.chunks(4).map(|p| p[3]).collect();
        assert!(
            (20..=44).all(|x| alpha[32 * 64 + x] == 255),
            "the hole is not filled"
        );
        for (x, y) in [(0, 0), (63, 63), (32, 10), (32, 54), (5, 32), (58, 32)] {
            assert_eq!(alpha[y * 64 + x], 0, "({x}, {y}) is outside the hole");
        }
        assert_eq!(layer_bytes(&app, Some(0)), bottom);
        let names = undo_names(&app);
        assert_eq!(names.len(), n + 1);
        assert_eq!(names.last().map(String::as_str), Some("Remove"));
    }

    /// `[` `]` resize the brush (Ctrl ×5) when idle and are swallowed unchanged while drawing,
    /// as the brush tools' are.
    #[test]
    fn remove_brackets_resize_unless_drawing() {
        let mut app = fresh_doc(32, 32, None);
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(10));
        let key = |k: &str, ctrl| KeyPress {
            key: k.into(),
            shift: false,
            ctrl,
            alt: false,
            meta: false,
        };
        assert!(app.on_key_press(&key("]", false)));
        assert!(app.on_key_press(&key("[", true)));
        assert_eq!(setting(&app, "width"), json!(6));
        drive_tool(&mut app, T, &[Ev::Down(10.0, 10.0, Left)]);
        assert!(app.on_key_press(&key("]", false)));
        assert_eq!(setting(&app, "width"), json!(6));
    }

    /// R28: Enter while drawing neither removes the collected strokes nor lets the release
    /// replace that removal: both strokes go in one removal.
    #[test]
    fn remove_enter_mid_stroke_keeps_the_strokes() {
        let mut app = fresh_doc(64, 64, None);
        let ws = app.active_ws_mut().unwrap();
        let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        for (cx, cy) in [(16, 16), (48, 48)] {
            for y in cy - 2..cy + 2 {
                for x in cx - 2..cx + 2 {
                    s.data_mut()[(y * 64 + x) * 4..][..3].fill(0);
                }
            }
        }
        ws.invalidate(None);
        let n = undo_names(&app).len();
        drive_tool(&mut app, T, &[]);
        app.update_tool_setting(T, "width", json!(10));
        app.update_tool_setting(T, "removeEachStroke", json!(false));
        stroke(&mut app, (16.0, 16.0), (16.0, 16.0));
        app.update_tool_setting(T, "removeEachStroke", json!(true));
        drive_tool(
            &mut app,
            T,
            &[
                Ev::Down(48.0, 48.0, Left),
                Ev::Key("Enter"),
                Ev::Up(48.0, 48.0, Left),
            ],
        );
        finish(&mut app);
        let px = layer_bytes(&app, None);
        assert!(
            px.chunks(4).all(|p| p[..3].iter().all(|&v| v >= 250)),
            "a dot is still there"
        );
        assert_eq!(undo_names(&app).len(), n + 1);
    }
}
