//! Ported from PhotoCraft crates/ui-egui/src/retouch_ui.rs (`patch_drags_selection`, `patch_offset`, `finish_patch`) and crates/ui-egui/src/patch_preview.rs (the live preview), with crates/engine/src/retouch_cmds/patch.rs's `patch` and `preview` @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: Paint.NET's modifiers (a left drag inside the selection with no Ctrl, Alt or Shift patches; every other drag is Lasso Select's, with its combine modes, its entries named after the Patch tool with its icon, Ruling R87) instead of ⇧/⌥; the ants follow the drag through the selection's interim transform, which goes back on release; the preview is a pixel overlay (`Tint::pixels`) of the coarse heal over the repaired area instead of a recomposited document, with no zoom floor; both modes run as a background job (`extra::job`; upstream runs Normal at once) that lands as one "Patch" entry through Kernel B with no selection clip, Normal through the coverage and Content-Aware replacing the selected pixels (upstream's `replace`); Sample: Layer / Image in both modes (upstream's Sample All Layers is Content-Aware's only); no offset shows the hint in the status bar and past the cap the message, where upstream reports errors; Esc and undo cancel the job, and a commit-first command or a tool switch applies a held patch and lands a running one at once (Ruling R72); a text layer asks to rasterize when a patch drag starts.
//!
//! Photocraft phase spec: S§4.3 "Patch", "Resource limits"
//!
//! The Patch tool: select an area (Lasso Select's drag, with Paint.NET's combine modes, or with the
//! Extra Click points option point by point, `click_lasso`), then drag the selection. In Normal mode Source repairs the selection with the texture where it is dropped
//! and Destination repairs the dropped-on place with the selection's, both seamless-cloned
//! (`extra::patch`), with a live preview while dragging; Content-Aware fills the selection from
//! where it is dropped (`extra::patch::land`). The release runs it as a background job (busy
//! cursor, Esc cancels) that lands as one "Patch" entry.

use std::sync::Arc;

use super::base::ToolBase;
use super::click_lasso::ClickLasso;
use super::healing::Heal;
use super::selection_tools::{LassoShape, SelectionShape, SelectionTool, click_points};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::action::UiRequest;
use crate::app::{AppEvent, AppWorkspace};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::job::Job;
use crate::extra::patch::{Mode, Plan, Refusal, heal_region_coarse, land, plan, preview_step};
use crate::extra::sample::layer_or_image;
use crate::extra::shim::{from_f32, to_f32};
use crate::pixel::IntRect;
use crate::pixel::content::{ContentBlendMode, ContentLayer, render_content};
use crate::pixel::mask::{self, SelectionQuality};
use crate::render::nubs::Tint;
use crate::render::{Renderer, RendererId};
use crate::selection::matrix::Matrix;
use crate::util::geom::Point;

/// Cells of the preview's coarse solve (upstream's): a 128² solve keeps up with the pointer
/// whatever the selection's size.
const MAX_CELLS: u64 = 128 * 128;

/// The selection's clipping polygons.
type Polys = Arc<Vec<Vec<[f64; 2]>>>;

/// Ruling R87: Lasso Select's drag, its entries named after the Patch tool (with its icon), as
/// Paint.NET names a selection entry after the tool that made it. The combine mode reads through
/// to the selection tools' shared one.
struct PatchLasso;

impl SelectionShape for PatchLasso {
    const TOOL: ToolType = ToolType::Patch;
    const CURSORS: [&'static str; 4] = LassoShape::CURSORS;

    fn shape(trace: &[Point], app: &AppWorkspace) -> Vec<Point> {
        LassoShape::shape(trace, app)
    }
}

/// A patch being dragged: the press, the selection then (its polygons, its bounds on the canvas
/// and its interim transform), and the offset so far.
struct Drag {
    start: (f64, f64),
    polys: Polys,
    area: Rect,
    canvas: Rect,
    interim: Matrix,
    offset: (i32, i32),
}

/// A patch's job: the pixels over `rect` and the mask they land through with `mode`.
struct Running {
    job: Job<(Vec<u8>, Vec<u8>)>,
    rect: IntRect,
    mode: ContentBlendMode,
}

#[derive(Default)]
pub struct PatchTool {
    lasso: SelectionTool<PatchLasso>,
    /// Click points: the selection placed point by point.
    clicks: ClickLasso,
    /// The status bar's hint while Click points is on.
    status: Option<String>,
    drag: Option<Drag>,
    running: Option<Running>,
    preview: Option<RendererId>,
    /// The status bar's text after a patch let go where it was taken.
    hint: Option<&'static str>,
}

fn setting(app: &AppWorkspace, key: &str) -> serde_json::Value {
    app.tool_settings.get(ToolType::Patch, key)
}

fn quality(app: &AppWorkspace) -> SelectionQuality {
    SelectionQuality::from_value(&app.tool_settings.rendering_quality)
}

/// The toolbar's Patch mode and, in Normal, Source / Destination.
fn mode(app: &AppWorkspace) -> Mode {
    let get = |k| setting(app, k);
    match (get("patchMode").as_str(), get("patchSource").as_str()) {
        (Some("contentAware"), _) => Mode::ContentAware,
        (_, Some("destination")) => Mode::Destination,
        _ => Mode::Source,
    }
}

fn int_rect(r: Rect) -> IntRect {
    IntRect::new(r.x0, r.y0, r.width(), r.height())
}

/// The selection's coverage (0–255) over `r`, read `(dx, dy)` back.
fn coverage(polys: &Polys, quality: SelectionQuality, r: Rect, (dx, dy): (i32, i32)) -> Vec<u8> {
    mask::rasterize(polys, quality, int_rect(r.translate(-dx, -dy))).data
}

/// What the patch dragged by `d` reads, on the UI thread: its plan, and the pixels (the active
/// layer's, or the image's with Sample: Image) over the plan's rect and at its texture shift.
fn read(app: &AppWorkspace, d: &Drag, mode: Mode) -> Result<(Plan, ImageData, ImageData), Refusal> {
    let p = plan(d.area, d.canvas, d.offset, mode)?;
    let sample = setting(app, "sampleImage").as_bool().unwrap_or(false);
    let (sx, sy) = p.src_shift;
    let dst = layer_or_image(app, sample, p.rect);
    let src = layer_or_image(app, sample, p.rect.translate(sx, sy));
    Ok((p, dst, src))
}

/// Upstream `patch_preview`'s view of a Normal patch held at `d`'s offset: the coarse heal (at
/// most `MAX_CELLS` cells) over the repaired area, through the coverage. None for Content-Aware
/// (its result shows when it lands), with no offset, or past the cap.
// ponytail: it runs on the UI thread at each new offset, as upstream's; the reads, the coverage
// and the texture are O(rect), so a patch near the cap costs a few hundred ms per move. Move it
// to a job (dropping stale offsets) if big patches stutter.
fn preview(app: &AppWorkspace, d: &Drag) -> Option<Tint> {
    let mode = mode(app);
    if mode == Mode::ContentAware {
        return None;
    }
    let (p, dst, src) = read(app, d, mode).ok()?;
    let cov = coverage(&d.polys, quality(app), p.rect, p.dst_shift);
    let rect = int_rect(p.rect);
    let heal = Heal {
        rect,
        src: src.data,
        dst: dst.data,
        cov: cov.clone(),
    };
    let step = preview_step(p.rect, MAX_CELLS);
    let pixels = heal_region_coarse(heal, step, &Interrupt::NONE).ok()?;
    Some(Tint {
        rect,
        mask: Arc::new(cov),
        color: [0; 4],
        pixels: Some(Arc::new(pixels)),
    })
}

/// A patch's result lands as one "Patch" entry: Kernel B over `rect` with no selection clip
/// (Destination repairs outside the selection), the pixels through their mask with `mode`.
fn write(app: &mut AppWorkspace, rect: IntRect, mode: ContentBlendMode, out: (Vec<u8>, Vec<u8>)) {
    let Some(ws) = app.active_ws_mut() else {
        return;
    };
    let (pixels, mask) = out;
    let mut base = ToolBase::default();
    base.begin_bitmap_transaction(ws);
    let o = base.original();
    let layer = ContentLayer {
        content: &pixels,
        mask: Some(&mask),
    };
    let data = render_content(o.data(), o.width(), rect, &[layer], None, mode);
    base.put_transaction(ws, rect, data);
    let name = crate::i18n::t("patchTool.name");
    if let Some(m) = base.commit_changed_tiles(ws, &name, &ToolType::Patch.icon_src()) {
        ws.push_memento(m);
    }
}

impl PatchTool {
    /// Click points' hint for the status bar while it is on.
    fn sync_status(&mut self, app: &AppWorkspace) {
        let t = crate::i18n::t;
        self.status = click_points(app, ToolType::Patch).then(|| {
            crate::i18n::tf(
                "statusBar.context.help.text.format",
                &[&t("patchTool.name"), &t("patchTool.clickPointsHelpText")],
            )
        });
    }

    /// Upstream's `patch_drags_selection`: a left press with no modifiers inside the selection
    /// grabs the patch.
    fn grab(app: &AppWorkspace, x: f64, y: f64, b: MouseButton) -> Option<Drag> {
        let mods = app.control_key_down || app.alt_key_down || app.shift_key_down;
        if b != MouseButton::Left || mods {
            return None;
        }
        let ws = app.active_ws().filter(|ws| !ws.selection.is_empty())?;
        let (w, h) = (ws.document.width, ws.document.height);
        let polys = Arc::new(ws.selection.clipping_polygons(IntRect::new(0, 0, w, h)));
        let at = IntRect::new(x.floor() as i32, y.floor() as i32, 1, 1);
        if mask::rasterize(&polys, quality(app), at).data[0] == 0 {
            return None;
        }
        let canvas = Rect::new(0, 0, w as i32, h as i32);
        let b = IntRect::bound_of(&ws.selection.get_bounds());
        Some(Drag {
            start: (x, y),
            polys,
            area: Rect::new(b.x, b.y, b.right(), b.bottom()).intersect(&canvas),
            canvas,
            interim: ws.selection.get_interim_transform(),
            offset: (0, 0),
        })
    }

    /// Upstream's `patch_offset`: the whole-pixel drag to `(x, y)`, limited so the patch stays
    /// on the canvas. The ants follow it, and the preview.
    fn follow(&mut self, app: &mut AppWorkspace, x: f64, y: f64) {
        let Some(d) = self.drag.as_mut() else {
            return;
        };
        let (c, a) = (d.canvas, d.area);
        let shift = |v: f64, lo: i32, hi: i32| {
            let v = if v.is_finite() {
                v.round().clamp(-1e7, 1e7) as i32
            } else {
                0
            };
            if lo > hi { 0 } else { v.clamp(lo, hi) }
        };
        let offset = (
            shift(x - d.start.0, c.x0 - a.x0, c.x1 - a.x1),
            shift(y - d.start.1, c.y0 - a.y0, c.y1 - a.y1),
        );
        if offset == d.offset {
            return;
        }
        d.offset = offset;
        let mut m = d.interim;
        m.translate(f64::from(offset.0), f64::from(offset.1));
        if let Some(ws) = app.active_ws_mut() {
            ws.selection.set_interim_transform(m);
        }
        self.sync_preview(app);
    }

    fn sync_preview(&mut self, app: &mut AppWorkspace) {
        let tint = self.drag.as_ref().and_then(|d| preview(app, d));
        self.show(app, tint.unwrap_or_default());
    }

    fn show(&mut self, app: &mut AppWorkspace, tint: Tint) {
        if let (Some(id), Some(ws)) = (self.preview, app.active_ws_mut())
            && let Some(Renderer::Tint(t)) = ws.surface_box.renderer_mut(id)
        {
            *t = tint;
        }
    }

    /// The held patch, let go: the selection back where it was. The preview stays until the
    /// patch lands (no flicker in between) or is dropped.
    fn take_drag(&mut self, app: &mut AppWorkspace) -> Option<Drag> {
        let d = self.drag.take()?;
        if let Some(ws) = app.active_ws_mut() {
            ws.selection.set_interim_transform(d.interim);
        }
        Some(d)
    }

    fn set_hint(&mut self, app: &mut AppWorkspace, hint: Option<&'static str>) {
        if self.hint != hint {
            self.hint = hint;
            app.fire(AppEvent::ToolStatusChanged);
        }
    }

    /// Upstream's `finish_patch`: the patch let go at `d`'s offset starts as a job. No offset
    /// shows the hint instead, and past the cap the message.
    fn start(&mut self, app: &mut AppWorkspace, d: Drag) {
        let (p, dst, src) = match read(app, &d, mode(app)) {
            Ok(r) => r,
            Err(Refusal::NoOffset) => return self.set_hint(app, Some(Refusal::NoOffset.message())),
            Err(Refusal::Empty) => return,
            Err(r) => {
                return app
                    .ui_requests
                    .push(UiRequest::ToolError(r.message().into()));
            }
        };
        let level = |k: &str, lo: f64, hi: f64| {
            let v = setting(app, k).as_f64().unwrap_or(lo);
            v.round().clamp(lo, hi) as u8
        };
        let (structure, color) = (
            level("structure", 1.0, 7.0),
            level("colorAmount", 0.0, 10.0),
        );
        let (rect, polys, quality) = (int_rect(p.rect), d.polys, quality(app));
        let job = Job::spawn(move |ctl| {
            let cov = coverage(&polys, quality, p.rect, p.dst_shift);
            if p.mode != Mode::ContentAware {
                let heal = Heal {
                    rect,
                    src: src.data,
                    dst: dst.data,
                    cov: cov.clone(),
                };
                return Ok((heal_region_coarse(heal, 1, ctl)?, cov));
            }
            // A soft selection counts wherever it is above zero; the band does the blending.
            let selected: Vec<bool> = cov.iter().map(|&c| c > 0).collect();
            let (w, h) = (rect.w as usize, rect.h as usize);
            let out = land(
                w,
                h,
                &to_f32(&dst),
                &to_f32(&src),
                &selected,
                structure,
                color,
                ctl,
            )?;
            let mask = selected.iter().map(|&s| u8::from(s) * 255).collect();
            Ok((from_f32(&out, rect.w, rect.h).data, mask))
        });
        let mode = match p.mode {
            Mode::ContentAware => ContentBlendMode::Overwrite,
            _ => ContentBlendMode::Layer(BlendMode::Normal),
        };
        self.running = Some(Running { job, rect, mode });
        // On wasm the job already ran: it lands now.
        if cfg!(target_arch = "wasm32") {
            self.poll(app);
        }
    }

    /// A finished job lands, then its preview goes.
    fn poll(&mut self, app: &mut AppWorkspace) {
        let Some(result) = self.running.as_mut().and_then(|r| r.job.poll()) else {
            return;
        };
        if let (Some(r), Ok(out)) = (self.running.take(), result) {
            write(app, r.rect, r.mode, out);
        }
        self.show(app, Tint::default());
    }

    /// Esc, an undo: a held patch is let go and a running job stops; nothing changes.
    fn cancel(&mut self, app: &mut AppWorkspace) {
        self.running = None;
        self.take_drag(app);
        self.show(app, Tint::default());
    }
}

impl Tool for PatchTool {
    fn tool_type(&self) -> ToolType {
        ToolType::Patch
    }

    /// Lasso Select's layers, and the preview's (empty).
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        self.lasso.on_activate(app);
        self.sync_status(app);
        if let Some(ws) = app.active_ws_mut() {
            self.preview = Some(ws.surface_box.add_renderer(Renderer::Tint(Tint::default())));
        }
    }

    /// R72: a held patch applies at its offset and a running one lands, at once, as the one entry
    /// its job would have made (past the cap, the message).
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        if let Some(d) = self.take_drag(app) {
            self.start(app, d);
        }
        if let Some(Running {
            mut job,
            rect,
            mode,
        }) = self.running.take()
            && let Ok(out) = job.wait()
        {
            write(app, rect, mode, out);
        }
        self.hint = None;
        self.clicks.cancel(app, self.lasso.live_layer());
        self.lasso.on_deactivate(app);
        if let (Some(id), Some(ws)) = (self.preview.take(), app.active_ws_mut()) {
            ws.surface_box.remove_renderer(id);
        }
    }

    /// A press while a patch is held or runs is swallowed, and one while Click points places a
    /// selection is its. A plain left press inside the selection grabs the patch (a text layer
    /// asks to rasterize first, S§4.5); any other press is Lasso Select's, or with Click points
    /// the first point.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if self.drag.is_some() || self.running.is_some() {
            return true;
        }
        self.set_hint(app, None);
        let (layer, button) = (
            self.lasso.live_layer(),
            matches!(b, MouseButton::Left | MouseButton::Right),
        );
        if self.clicks.active() {
            if button && !self.clicks.pressed() {
                self.clicks.down(app, ToolType::Patch, x, y, b, layer);
            }
            return true;
        }
        if !self.lasso.is_tracking()
            && let Some(d) = Self::grab(app, x, y, b)
        {
            if crate::document::text_layer::ensure_active_raster(app) {
                self.drag = Some(d);
            }
            return true;
        }
        if button && !self.lasso.is_tracking() && click_points(app, ToolType::Patch) {
            self.clicks.down(app, ToolType::Patch, x, y, b, layer);
            return true;
        }
        self.lasso.on_mouse_down(app, x, y, b, i)
    }

    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, i: &PointerInput) -> bool {
        if self.drag.is_none() {
            if self.clicks.active() {
                return self.clicks.moved(app, x, y, i, self.lasso.live_layer());
            }
            return self.lasso.on_mouse_move(app, x, y, i);
        }
        self.follow(app, x, y);
        true
    }

    /// The release lets the patch go where it was dragged.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if self.drag.is_none() {
            if self.clicks.pressed() {
                return self.clicks.up(app, x, y, i, self.lasso.live_layer());
            }
            return self.lasso.on_mouse_up(app, x, y, b, i);
        }
        self.follow(app, x, y);
        if let Some(d) = self.take_drag(app) {
            self.start(app, d);
        }
        // Refused: no job lands, so the preview goes now.
        if self.running.is_none() {
            self.show(app, Tint::default());
        }
        true
    }

    /// Click points' keys while it places a selection; else Escape lets a held patch go, or stops a
    /// running one.
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        if self.clicks.key(app, key, self.lasso.live_layer()) {
            return true;
        }
        if key.key != "Escape" || (self.drag.is_none() && self.running.is_none()) {
            return false;
        }
        self.cancel(app);
        true
    }

    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        self.lasso.on_modifier_keys_changed(app);
    }

    /// An undo or redo drops a selection being placed and stops a running patch.
    fn on_executing_history_memento(&mut self, app: &mut AppWorkspace) {
        self.clicks.cancel(app, self.lasso.live_layer());
        self.cancel(app);
    }

    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if key == "clickPoints" {
            self.clicks.cancel(app, self.lasso.live_layer());
            self.sync_status(app);
        }
    }

    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.poll(app);
    }

    fn needs_ticks(&self) -> bool {
        self.running.is_some()
    }

    fn busy(&self) -> bool {
        self.running.is_some()
    }

    fn status_text(&self) -> Option<&str> {
        self.hint.or(self.status.as_deref())
    }

    fn cursor(&self) -> CursorRequest {
        if self.clicks.active() {
            return CursorRequest::Image(LassoShape::CURSORS[1]);
        }
        self.lasso.cursor()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::action::UiRequest;
    use crate::app::AppWorkspace;
    use crate::document::layer::{BlendMode, Layer};
    use crate::extra::interrupt::Interrupt;
    use crate::extra::patch::{Mode, Refusal, blemished, plan, texture};
    use crate::pixel::IntRect;
    use crate::pixel::content::{ContentBlendMode, ContentLayer, render_content};
    use crate::render::Renderer;
    use crate::selection::CombineMode;
    use crate::testing::{Ev, drive_tool, fresh_doc, layer_bytes, selection_mask};
    use crate::tool::MouseButton::Left;
    use crate::tool::ToolType;
    use crate::tool::healing::Heal;
    use crate::tool::selection_tools::ellipse_select_polygon;
    use crate::tool::types::{ToolSettings, option_definitions_with_fonts};
    use crate::tool::{PointerInput, PointerType};
    use crate::util::geom::{Point, Rectangle};
    use serde_json::json;

    const T: ToolType = ToolType::Patch;

    /// Runs frames until the patch has landed.
    fn finish(app: &mut AppWorkspace) {
        let start = std::time::Instant::now();
        while app.tool_busy() {
            assert!(start.elapsed().as_secs() < 300, "the patch never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.on_frame(app.now_ms + 16.0);
        }
    }

    fn undo_names(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    /// Click points: clicks place the patch's selection and a click on the first point selects
    /// it, one entry named after the Patch tool; a press inside it then grabs the patch.
    #[test]
    fn click_points_place_the_selection() {
        let mut app = fresh_doc(80, 60, None);
        app.tool_settings.set(T, "clickPoints", true.into());
        let corners = [
            (10.0, 10.0),
            (40.0, 10.0),
            (40.0, 30.0),
            (10.0, 30.0),
            (10.0, 10.0),
        ];
        let events: Vec<Ev> = corners
            .into_iter()
            .flat_map(|(x, y)| [Ev::Down(x, y, Left), Ev::Up(x, y, Left), Ev::Wait(600.0)])
            .collect();
        let n = undo_names(&app).len();
        drive_tool(&mut app, T, &events);
        assert_eq!(undo_names(&app)[n..], ["Patch"]);
        let bounds = app
            .active_ws()
            .unwrap()
            .selection
            .create_path(true)
            .get_bounds();
        assert_eq!(bounds, Rectangle::new(10.0, 10.0, 30.0, 20.0));
        drive_tool(&mut app, T, &[Ev::Down(20.0, 20.0, Left)]);
        let mut grabbed = false;
        crate::testing::with_tool::<super::PatchTool>(&mut app, |t, _| grabbed = t.drag.is_some());
        assert!(grabbed);
    }

    /// The bottom layer as `f(x, y)`.
    fn paint(app: &mut AppWorkspace, f: impl Fn(i32, i32) -> [u8; 4]) {
        let ws = app.active_ws_mut().unwrap();
        let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        let w = s.width() as usize;
        for (i, p) in s.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *p = f((i % w) as i32, (i / w) as i32);
        }
        ws.invalidate(None);
    }

    fn px(app: &AppWorkspace, x: i32, y: i32) -> [u8; 4] {
        let ws = app.active_ws().unwrap();
        ws.document
            .layers
            .get_at(ws.active_layer)
            .unwrap()
            .surface
            .pixel(x as u32, y as u32)
    }

    fn select(app: &mut AppWorkspace, x: f64, y: f64, w: f64, h: f64) {
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(x, y, w, h), CombineMode::Replace);
        ws.selection.commit_continuation();
    }

    fn set(app: &mut AppWorkspace, settings: &[(&str, serde_json::Value)]) {
        for (k, v) in settings {
            app.tool_settings.set(T, k, v.clone());
        }
    }

    /// A patch drag from `a` to `b`, until it lands.
    fn drag(app: &mut AppWorkspace, a: (f64, f64), b: (f64, f64)) {
        drive_tool(
            app,
            T,
            &[
                Ev::Down(a.0, a.1, Left),
                Ev::Move(b.0, b.1),
                Ev::Up(b.0, b.1, Left),
            ],
        );
        finish(app);
    }

    /// The release of a held drag, with no frame after it: the job is started, not polled.
    fn release(app: &mut AppWorkspace, x: f64, y: f64) {
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        app.on_mouse_up(x, y, Left, &input);
    }

    /// Every pixel of `(x0, y0, x1, y1)` is within `t` of the clean texture, per channel.
    fn assert_repaired(app: &AppWorkspace, r: (i32, i32, i32, i32), t: f32, what: &str) {
        for y in r.1..r.3 {
            for x in r.0..r.2 {
                let (got, want) = (px(app, x, y), texture(x, y));
                for c in 0..4 {
                    let d = (f32::from(got[c]) - f32::from(want[c])).abs() / 255.0;
                    assert!(
                        d < t,
                        "{what} at {x},{y} channel {c}: {got:?} want {want:?}"
                    );
                }
            }
        }
    }

    /// Upstream's `patch_source_repairs_the_selection_with_texture_from_the_drag_target` (8-bit):
    /// a red blemish, selected, dragged (-30, 2) onto clean texture. Every repaired pixel is within
    /// upstream's 0.07 of the clean texture (its gradient comes from the membrane, its noise from
    /// the source), and each channel's mean over the selection within 2 levels of it; outside the
    /// selection nothing changed, the sampled area and a far pixel included. One entry, whose undo
    /// brings the blemish back bit for bit.
    #[test]
    fn patch_source_repairs_selection_from_offset() {
        let mut app = fresh_doc(96, 48, None);
        paint(&mut app, blemished(40));
        select(&mut app, 36.0, 16.0, 16.0, 16.0);
        let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
        drag(&mut app, (44.0, 24.0), (14.0, 26.0));
        assert_repaired(&app, (36, 16, 52, 32), 0.07, "source");
        for c in 0..3 {
            let mean = |f: &dyn Fn(i32, i32) -> [u8; 4]| {
                let pts = (16..32).flat_map(|y| (36..52).map(move |x| (x, y)));
                pts.map(|(x, y)| f64::from(f(x, y)[c])).sum::<f64>() / 256.0
            };
            let (got, want) = (mean(&|x, y| px(&app, x, y)), mean(&texture));
            assert!((got - want).abs() < 2.0, "channel {c}: {got} vs {want}");
        }
        for (x, y) in [(35, 20), (52, 25), (10, 20), (44, 40), (90, 45)] {
            assert_eq!(px(&app, x, y), texture(x, y), "untouched {x},{y}");
        }
        assert_eq!(undo_names(&app)[n..], ["Patch"]);
        assert!(app.undo());
        assert!(
            layer_bytes(&app, None) == before,
            "undo brings the blemish back"
        );
    }

    /// Upstream's `patch_destination_repairs_the_drag_target_from_the_selection`: clean texture
    /// selected and dragged onto the blemish repairs it; the selected pixels stay as they were.
    #[test]
    fn patch_destination_copies_selection_to_offset() {
        let mut app = fresh_doc(96, 48, None);
        paint(&mut app, blemished(40));
        set(&mut app, &[("patchSource", json!("destination"))]);
        select(&mut app, 6.0, 16.0, 16.0, 16.0);
        drag(&mut app, (14.0, 24.0), (44.0, 24.0));
        assert_repaired(&app, (36, 16, 52, 32), 0.07, "destination");
        for (x, y) in [(10, 20), (6, 16), (21, 31), (90, 45)] {
            assert_eq!(px(&app, x, y), texture(x, y), "untouched {x},{y}");
        }
        assert_eq!(undo_names(&app)[1..], ["Patch"]);
    }

    /// Upstream's `patch_content_aware_copies_the_dragged_to_content_into_the_selection` (8-bit,
    /// Structure 7, Color 0): the selection's middle is the dragged-to content within upstream's
    /// one level; outside the selection nothing changed. One "Patch" entry, whose undo brings the
    /// blemish back bit for bit.
    #[test]
    fn patch_content_aware_keeps_core_texture() {
        let mut app = fresh_doc(128, 64, None);
        paint(&mut app, blemished(40));
        set(
            &mut app,
            &[
                ("patchMode", json!("contentAware")),
                ("structure", json!(7)),
                ("colorAmount", json!(0)),
            ],
        );
        select(&mut app, 32.0, 12.0, 24.0, 24.0);
        let before = layer_bytes(&app, None);
        drag(&mut app, (44.0, 24.0), (14.0, 26.0));
        for (x, y) in [(42, 22), (44, 24), (46, 26)] {
            let (got, want) = (px(&app, x, y), texture(x - 30, y + 2));
            for c in 0..4 {
                assert!(
                    got[c].abs_diff(want[c]) <= 1,
                    "middle copied at {x},{y}: {got:?} vs {want:?}"
                );
            }
        }
        for (x, y) in [(31, 20), (56, 25), (12, 20), (44, 40), (120, 60)] {
            assert_eq!(px(&app, x, y), texture(x, y), "untouched {x},{y}");
        }
        assert_eq!(undo_names(&app)[1..], ["Patch"]);
        assert!(app.undo());
        assert!(
            layer_bytes(&app, None) == before,
            "undo brings the blemish back"
        );
    }

    /// Upstream's `patch_at_the_canvas_edge_stays_opaque_and_matches`: a blemish at the left edge
    /// is repaired without pulling in the empty pixels beyond it.
    #[test]
    fn patch_at_the_canvas_edge_stays_opaque_and_matches() {
        let mut app = fresh_doc(64, 48, None);
        paint(&mut app, blemished(0));
        select(&mut app, 0.0, 16.0, 12.0, 16.0);
        drag(&mut app, (4.0, 24.0), (34.0, 24.0));
        assert_repaired(&app, (0, 16, 12, 32), 0.07, "edge");
    }

    /// A press and release inside the selection on the same pixel (no offset) shows the hint in
    /// the status bar and changes nothing: no entry, no job, no message.
    #[test]
    fn patch_zero_offset_changes_nothing() {
        let mut app = fresh_doc(96, 48, None);
        paint(&mut app, blemished(40));
        select(&mut app, 36.0, 16.0, 16.0, 16.0);
        let before = layer_bytes(&app, None);
        drag(&mut app, (44.0, 24.0), (44.4, 24.6));
        let hint = Refusal::NoOffset.message();
        assert_eq!(hint, "Drag the selection to the area to sample from.");
        assert_eq!(app.tool.as_ref().unwrap().status_text(), Some(hint));
        assert!(app.ui_requests.is_empty() && !app.tool_busy());
        assert_eq!(undo_names(&app), ["New Image"]);
        assert!(layer_bytes(&app, None) == before);
        // The next press clears it.
        drive_tool(&mut app, T, &[Ev::Down(44.0, 24.0, Left)]);
        assert_eq!(app.tool.as_ref().unwrap().status_text(), None);
    }

    /// Review Focus 2: the heal is written through the selection's coverage. A soft-edged
    /// (antialiased) ellipse over a flat area, patched from vertical stripes: the layer is the
    /// pre-patch pixels plus the heal through the coverage (Normal), bit for bit, so its edge
    /// pixels stay partly blended and nothing outside the coverage changes. Upstream's
    /// `patch_follows_a_feathered_selection_and_a_mask_target`: a blemish well inside an ellipse
    /// is repaired, a corner outside it untouched.
    #[test]
    fn patch_respects_selection_coverage() {
        let ellipse = |app: &mut AppWorkspace, x: f64, y: f64| {
            let poly =
                ellipse_select_polygon(Point::new(x, y), Point::new(x + 15.0, y + 15.0), false);
            let ws = app.active_ws_mut().unwrap();
            ws.selection
                .set_continuation_points(&poly, CombineMode::Replace);
            ws.selection.commit_continuation();
        };
        let stripes = |x: i32, y: i32| {
            if x < 40 {
                if x % 6 < 3 {
                    [40, 160, 220, 255]
                } else {
                    [200, 60, 90, 255]
                }
            } else {
                [120, 140, 100 + (y % 2) as u8, 255]
            }
        };
        let mut app = fresh_doc(96, 48, None);
        paint(&mut app, stripes);
        ellipse(&mut app, 56.0, 16.0);
        let before = layer_bytes(&app, None);
        let mask = selection_mask(&app, 96, 48);
        // The tool's heal, rebuilt: its plan's rect, the texture at the offset, the coverage.
        let p = plan(
            crate::extra::geom::Rect::new(56, 16, 72, 32),
            crate::extra::geom::Rect::new(0, 0, 96, 48),
            (-40, 0),
            Mode::Source,
        )
        .unwrap();
        let r = IntRect::new(p.rect.x0, p.rect.y0, p.rect.width(), p.rect.height());
        let crop = |b: &[u8], n: usize, dx: i32| -> Vec<u8> {
            (r.y..r.bottom())
                .flat_map(|y| {
                    let i = (y as usize * 96 + (r.x + dx) as usize) * n;
                    b[i..i + r.w as usize * n].to_vec()
                })
                .collect()
        };
        let cov = crop(&mask, 1, 0);
        let heal = Heal {
            rect: r,
            src: crop(&before, 4, -40),
            dst: crop(&before, 4, 0),
            cov: cov.clone(),
        };
        let healed = heal.run(&Interrupt::NONE).unwrap();
        let layer = ContentLayer {
            content: &healed,
            mask: Some(&cov),
        };
        let normal = ContentBlendMode::Layer(BlendMode::Normal);
        let want = render_content(&before, 96, r, &[layer], None, normal);
        drag(&mut app, (64.0, 24.0), (24.0, 24.0));
        let after = layer_bytes(&app, None);
        assert_eq!(crop(&after, 4, 0), want, "the heal through the coverage");
        let mut partial = 0;
        for (i, m) in mask.iter().enumerate() {
            let (o, e) = (&before[i * 4..][..4], &after[i * 4..][..4]);
            if *m == 0 {
                assert_eq!(o, e, "outside the coverage at {i}");
            }
            let (x, y) = ((i % 96) as i32, (i / 96) as i32);
            if (1..255).contains(m) && r.contains(x, y) {
                let k = ((y - r.y) as usize * r.w as usize + (x - r.x) as usize) * 4;
                let h = &healed[k..k + 4];
                partial += usize::from(
                    (0..3).any(|c| e[c].abs_diff(o[c]) >= 6 && e[c].abs_diff(h[c]) >= 6),
                );
            }
        }
        assert!(partial > 0, "no edge pixel is partly blended");
        // Upstream's feathered case.
        let mut app = fresh_doc(96, 48, None);
        paint(&mut app, blemished(40));
        ellipse(&mut app, 36.0, 16.0);
        drag(&mut app, (44.0, 24.0), (14.0, 24.0));
        assert_repaired(&app, (42, 22, 46, 26), 0.07, "ellipse");
        assert_eq!(
            px(&app, 36, 16),
            texture(36, 16),
            "corner outside the ellipse"
        );
    }

    /// Review Focus 3: a Normal and a Content-Aware patch are each exactly one "Patch" entry;
    /// undo restores the prior pixels bit for bit and redo the patched ones.
    #[test]
    fn patch_is_one_history_item() {
        for mode in ["normal", "contentAware"] {
            let mut app = fresh_doc(96, 48, None);
            paint(&mut app, blemished(40));
            set(&mut app, &[("patchMode", json!(mode))]);
            select(&mut app, 36.0, 16.0, 16.0, 16.0);
            let before = layer_bytes(&app, None);
            drag(&mut app, (44.0, 24.0), (14.0, 26.0));
            let patched = layer_bytes(&app, None);
            assert!(patched != before, "{mode}: nothing patched");
            assert_eq!(undo_names(&app)[1..], ["Patch"], "{mode}");
            assert!(app.undo());
            assert!(layer_bytes(&app, None) == before, "{mode}: undo");
            assert!(app.redo());
            assert!(layer_bytes(&app, None) == patched, "{mode}: redo");
        }
    }

    /// Review Focus 1: past the cap, judged on the rect the patch works on, every path shows the
    /// message and changes nothing (no entry, no job, the bytes as they were): a Normal release
    /// whose 4094² selection fits but whose solve rect does not, a Content-Aware release whose
    /// 3500² selection fits but whose window does not, and a tool switch mid-drag, which would
    /// otherwise apply the patch. The over-cap drag shows no preview.
    #[test]
    fn patch_over_cap_shows_message() {
        for end in ["release", "contentAware", "switch"] {
            let mut app = fresh_doc(4200, 4200, None);
            if end == "contentAware" {
                set(&mut app, &[("patchMode", json!("contentAware"))]);
                select(&mut app, 350.0, 350.0, 3500.0, 3500.0);
            } else {
                select(&mut app, 50.0, 50.0, 4094.0, 4094.0);
            }
            let (before, n) = (layer_bytes(&app, None), undo_names(&app).len());
            drive_tool(
                &mut app,
                T,
                &[Ev::Down(400.0, 400.0, Left), Ev::Move(410.0, 400.0)],
            );
            assert!(preview(&app).is_none(), "{end}: a preview past the cap");
            match end {
                "switch" => app.set_active_tool_from_type(ToolType::PaintBrush),
                _ => drive_tool(&mut app, T, &[Ev::Up(410.0, 400.0, Left)]),
            }
            let msg = UiRequest::ToolError(Refusal::TooLarge.message().into());
            assert_eq!(app.ui_requests, [msg], "{end}");
            assert!(!app.tool_busy(), "{end}");
            assert_eq!(undo_names(&app).len(), n, "{end}");
            assert!(
                layer_bytes(&app, None) == before,
                "{end}: the bytes changed"
            );
        }
    }

    /// S§4.3 "Resource limits" and R72: the patch runs as a job (busy cursor) after the release.
    /// Escape stops it, and undo stops it and steps back the entry before: no entry of its own
    /// and the bytes as they were. A tool switch, a layer switch and a document switch and back
    /// land it as the one entry the finished job makes, on the patched layer and document (the
    /// other document is untouched meanwhile).
    #[test]
    fn patch_job_lifecycle() {
        let run = |end: &str| {
            let mut app = fresh_doc(128, 128, Some(2));
            let ws = app.active_ws_mut().unwrap();
            ws.add_layer(Layer::create_layer(128, 128, "Top"));
            ws.set_active_layer(0);
            select(&mut app, 40.0, 40.0, 30.0, 30.0);
            if end == "undo" {
                // Something to undo that changes no pixel.
                app.edit_selection(|ws| ws.execute_function(crate::history::functions::select_all));
                select(&mut app, 40.0, 40.0, 30.0, 30.0);
            }
            let (before, n) = (layer_bytes(&app, Some(0)), undo_names(&app).len());
            drive_tool(
                &mut app,
                T,
                &[Ev::Down(50.0, 50.0, Left), Ev::Move(90.0, 70.0)],
            );
            release(&mut app, 90.0, 70.0);
            assert!(app.tool_busy(), "{end}");
            match end {
                "Escape" => drive_tool(&mut app, T, &[Ev::Key("Escape")]),
                "undo" => assert!(app.undo()),
                "tool" => app.set_active_tool_from_type(ToolType::PaintBrush),
                "layer" => app.set_active_layer(1),
                "document" => {
                    let other = app.create_blank_document_in_new_workspace(32, 32, 96.0, true);
                    assert_eq!(app.active, Some(other));
                    for _ in 0..20 {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        app.on_frame(app.now_ms + 16.0);
                    }
                    assert_eq!(undo_names(&app).len(), 1, "an entry in the other document");
                    assert!(layer_bytes(&app, None).iter().all(|&v| v == 255));
                    app.set_active_document_workspace(Some(0));
                    finish(&mut app);
                }
                _ => finish(&mut app),
            }
            assert!(!app.tool_busy(), "{end}");
            assert!(preview(&app).is_none(), "{end}: the preview stays");
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                app.on_frame(app.now_ms + 16.0);
            }
            let names = undo_names(&app);
            let n = if end == "undo" { n - 1 } else { n };
            (before, layer_bytes(&app, Some(0)), names[n..].to_vec())
        };
        for end in ["Escape", "undo"] {
            let (before, after, names) = run(end);
            assert!(after == before && names.is_empty(), "{end}: {names:?}");
        }
        let (before, patched, names) = run("wait");
        assert!(patched != before && names == ["Patch"]);
        for end in ["tool", "layer", "document"] {
            let (_, after, names) = run(end);
            assert!(after == patched, "{end}: not the job's patch");
            assert_eq!(names, ["Patch"], "{end}");
        }
    }

    /// Upstream's `patch_tool_lassoes_then_drags_the_patch`, with Paint.NET's modifiers: outside
    /// any selection the drag is a lasso (Ruling R87: a "Patch" entry with Patch's icon, as
    /// Paint.NET names a selection entry after its tool; no pixel changed); a Ctrl-drag
    /// inside the selection adds to it (Paint.NET's Union) rather than patching; a plain drag
    /// inside it patches from where it is dropped, the offset limited so the patch stays on the
    /// canvas, and the ants follow it.
    #[test]
    fn patch_lasso_then_drag() {
        let mut app = fresh_doc(100, 60, None);
        paint(&mut app, |x, y| {
            if (68..74).contains(&x) && (28..32).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [255; 4]
            }
        });
        let red = |app: &AppWorkspace| px(app, 71, 30)[1] < 128;
        let before = layer_bytes(&app, None);
        drive_tool(
            &mut app,
            T,
            &[
                Ev::Down(60.0, 20.0, Left),
                Ev::Move(84.0, 20.0),
                Ev::Move(84.0, 40.0),
                Ev::Move(60.0, 40.0),
                Ev::Wait(60.0),
                Ev::Up(60.0, 40.0, Left),
            ],
        );
        assert!(
            !app.active_ws().unwrap().selection.is_empty(),
            "lasso made a selection"
        );
        assert_eq!(undo_names(&app)[1..], ["Patch"]);
        let ws = app.active_ws().unwrap();
        let entry = ws.history.undo.last().unwrap();
        assert_eq!(entry.image, T.icon_src());
        assert!(matches!(
            entry.kind,
            crate::history::MementoKind::Selection(_)
        ));
        assert!(
            red(&app) && layer_bytes(&app, None) == before,
            "the lasso alone changes no pixels"
        );
        let bounds = |app: &AppWorkspace| app.active_ws().unwrap().selection.get_bounds();
        let lassoed = bounds(&app);
        drive_tool(
            &mut app,
            T,
            &[
                Ev::DownMods(70.0, 30.0, Left, "Ctrl"),
                Ev::Move(95.0, 30.0),
                Ev::Move(95.0, 50.0),
                Ev::Wait(60.0),
                Ev::Up(70.0, 50.0, Left),
            ],
        );
        app.set_modifier_keys(false, false, false);
        assert!(
            bounds(&app).right() > lassoed.right(),
            "Ctrl-drag adds to the selection"
        );
        assert!(red(&app) && !app.tool_busy(), "Ctrl-drag patched");
        assert_eq!(undo_names(&app)[1..], ["Patch", "Patch"]);
        // Back to the lasso; the offset is limited to the canvas, and the ants follow it.
        assert!(app.undo());
        drive_tool(
            &mut app,
            T,
            &[Ev::Down(70.0, 30.0, Left), Ev::Move(-200.0, 30.0)],
        );
        let moved = bounds(&app);
        assert_eq!(
            (moved.left(), moved.top()),
            (lassoed.left() - 60.0, lassoed.top())
        );
        drive_tool(
            &mut app,
            T,
            &[Ev::Move(30.0, 30.0), Ev::Up(30.0, 30.0, Left)],
        );
        finish(&mut app);
        assert_eq!(bounds(&app), lassoed, "the selection stays where it was");
        let p = px(&app, 71, 30);
        assert!(
            p.iter().all(|&v| v > 240),
            "blemish patched with the white background: {p:?}"
        );
    }

    /// The live preview (Normal) is an overlay: while the patch is held the layer is unchanged and
    /// a pixel overlay shows the heal at the offset over the selection; it stays after the release
    /// until the patch lands, which removes it.
    /// Content-Aware shows none (its job's result shows when it lands).
    #[test]
    fn patch_preview_is_an_overlay() {
        for mode in ["normal", "contentAware"] {
            let mut app = fresh_doc(96, 48, None);
            paint(&mut app, blemished(40));
            set(&mut app, &[("patchMode", json!(mode))]);
            select(&mut app, 36.0, 16.0, 16.0, 16.0);
            let before = layer_bytes(&app, None);
            drive_tool(
                &mut app,
                T,
                &[Ev::Down(44.0, 24.0, Left), Ev::Move(14.0, 26.0)],
            );
            assert!(
                layer_bytes(&app, None) == before,
                "{mode}: the layer changed while held"
            );
            let shown = preview(&app);
            if mode == "normal" {
                let (rect, mask, pixels) = shown.expect("no preview");
                assert!(rect.contains(36, 16) && rect.contains(51, 31), "{rect:?}");
                let i = ((24 - rect.y) as usize * rect.w as usize + (44 - rect.x) as usize) * 4;
                assert_eq!(mask[i / 4], 255);
                assert!(
                    pixels[i + 1] > 100,
                    "the preview at the blemish is not repaired"
                );
            } else {
                assert!(shown.is_none(), "a Content-Aware preview");
            }
            // Review M2: released, the preview stays until the job lands (no flicker between).
            release(&mut app, 14.0, 26.0);
            assert!(app.tool_busy(), "{mode}");
            assert_eq!(
                preview(&app).is_some(),
                mode == "normal",
                "{mode}: after the release"
            );
            assert!(
                layer_bytes(&app, None) == before,
                "{mode}: landed before a frame"
            );
            finish(&mut app);
            assert!(layer_bytes(&app, None) != before, "{mode}: nothing landed");
            assert!(preview(&app).is_none(), "{mode}: the preview stays");
        }
    }

    /// The preview overlay: its rect, coverage and pixels, when it shows.
    fn preview(app: &AppWorkspace) -> Option<(IntRect, Vec<u8>, Vec<u8>)> {
        let ws = app.active_ws().unwrap();
        ws.surface_box.renderers.iter().find_map(|(_, r)| match r {
            Renderer::Tint(t) if !t.rect.is_empty() => {
                let pixels = t.pixels.as_ref()?;
                Some((t.rect, t.mask.to_vec(), pixels.to_vec()))
            }
            _ => None,
        })
    }

    /// S§4.3 Sample: Image: on an empty layer above the texture, Image patches what is seen (the
    /// selection becomes the repaired texture); Layer patches the empty layer itself (no change,
    /// no entry).
    #[test]
    fn patch_sample_image_heals_onto_an_empty_layer() {
        for image in [false, true] {
            let mut app = fresh_doc(96, 48, None);
            paint(&mut app, blemished(40));
            let ws = app.active_ws_mut().unwrap();
            ws.add_layer(Layer::create_layer(96, 48, "Top"));
            ws.set_active_layer(1);
            set(&mut app, &[("sampleImage", json!(image))]);
            select(&mut app, 36.0, 16.0, 16.0, 16.0);
            drag(&mut app, (44.0, 24.0), (14.0, 26.0));
            if image {
                assert_repaired(&app, (36, 16, 52, 32), 0.07, "image");
                assert_eq!(px(&app, 10, 10), [0; 4], "outside the selection");
                assert_eq!(undo_names(&app)[1..], ["Patch"]);
            } else {
                assert!(layer_bytes(&app, None).iter().all(|&v| v == 0));
                assert_eq!(undo_names(&app), ["New Image"]);
            }
        }
    }

    /// S§4.1, S§4.3: Paint.NET's combine row and quality (no plus), then the Extra options (the
    /// plus): Patch mode, then Source / Destination in Normal or Structure and Color in
    /// Content-Aware, then Sample, and Click points last (`click_lasso`). The defaults are S§4.3's;
    /// the combine mode is the selection tools' shared one.
    #[test]
    fn patch_toolbar_and_defaults() {
        let keys = |settings: &ToolSettings| -> Vec<(&str, bool)> {
            let defs = option_definitions_with_fonts(T, settings, &[]);
            defs.iter().map(|d| (d.key, d.extra)).collect()
        };
        let mut s = ToolSettings::default();
        assert_eq!(
            keys(&s),
            [
                ("combineMode", false),
                ("patchMode", true),
                ("patchSource", true),
                ("sampleImage", true),
                ("renderingQuality", false),
                ("clickPoints", true),
            ]
        );
        let defaults = |s: &ToolSettings| {
            [
                "patchMode",
                "patchSource",
                "structure",
                "colorAmount",
                "sampleImage",
            ]
            .map(|k| s.get(T, k))
        };
        let want = [
            json!("normal"),
            json!("source"),
            json!(4),
            json!(0),
            json!(false),
        ];
        assert_eq!(defaults(&s), want);
        s.set(T, "patchMode", json!("contentAware"));
        assert_eq!(
            keys(&s),
            [
                ("combineMode", false),
                ("patchMode", true),
                ("structure", true),
                ("colorAmount", true),
                ("sampleImage", true),
                ("renderingQuality", false),
                ("clickPoints", true),
            ]
        );
        let defs = option_definitions_with_fonts(T, &s, &[]);
        let range = |k: &str| {
            let d = defs.iter().find(|d| d.key == k).unwrap();
            (d.min, d.max)
        };
        assert_eq!(range("structure"), (Some(1.0), Some(7.0)));
        assert_eq!(range("colorAmount"), (Some(0.0), Some(10.0)));
        let mut app = fresh_doc(8, 8, None);
        app.update_tool_setting(T, "combineMode", json!("union"));
        let shared = app.tool_settings.get(ToolType::LassoSelect, "combineMode");
        assert_eq!(shared, json!("union"));
    }
}
