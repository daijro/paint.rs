//! Port of: js/src/engine/document/render/{SurfaceBox,SurfaceBoxRenderer}.js
//! Reference: paintdotjs @ b4e6fc5
//! The canvas layers are in Paint.NET's order.
//!
//! The JS renderer classes become variants of `Renderer` (the nub renderers are `Handles`, the
//! `BrushPreviewRenderer` is `BrushPreview`). `dispose()` is not ported: nothing in the engine calls it. The JS
//! base renderer is `view::render_view` (always drawn first). The base (view and grid) and the
//! overlays (every other renderer) render separately, so the canvas view can keep the base and
//! redraw only the overlays for an ants frame.

pub mod canvas;
pub mod checker;
pub mod color_ui;
pub mod display;
pub mod float;
pub mod grid;
pub mod nubs;
pub mod ruler;
pub mod selection;
pub mod thumbnail;
pub mod view;

use crate::document::workspace::DocumentWorkspace;
use crate::pixel::IntRect;
use display::{DisplayTarget, RenderMode};
use view::{CanvasStyle, ViewTransform};

pub type RendererId = usize;

pub enum Renderer {
    Grid(grid::GridRenderer),
    Selection(selection::SelectionRenderer),
    /// A tool's handles (Task 26; `canvas-rendering.md §2.10`), drawn in list order.
    Handles(Vec<nubs::Handle>),
    /// The brush tools' outline (Task 30; `painting-tools-1.md §3.14` step 5).
    BrushPreview(nubs::BrushPreview),
    /// The Color Picker's sample square (Task 31; `painting-tools-1.md §3.12` step 6).
    PickerPreview(nubs::PickerPreview),
    /// A tool's tint over canvas pixels (the Remove tool's strokes; S§4.3).
    Tint(nubs::Tint),
}

impl Renderer {
    pub fn visible(&self) -> bool {
        match self {
            Renderer::Grid(r) => r.visible,
            Renderer::Selection(r) => r.visible,
            Renderer::Handles(h) => !h.is_empty(),
            Renderer::BrushPreview(p) => p.opacity > 0.0,
            Renderer::PickerPreview(p) => p.visible,
            Renderer::Tint(t) => !t.rect.is_empty(),
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        match self {
            Renderer::Grid(r) => r.visible = visible,
            Renderer::Selection(r) => r.visible = visible,
            Renderer::PickerPreview(p) => p.visible = visible,
            // Shown while it holds handles / has opacity.
            Renderer::Handles(_) | Renderer::BrushPreview(_) | Renderer::Tint(_) => {}
        }
    }

    /// Part of the base (drawn with the document view).
    fn is_base(&self) -> bool {
        matches!(self, Renderer::Grid(_))
    }

    /// Rects holding every device pixel an overlay paints (`None`: too many to list, or a tint).
    /// These cover all painted pixels, not only the ants-dependent ones (`overlay_spans`).
    fn paint_rects(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<Vec<IntRect>> {
        match self {
            Renderer::Selection(s) if s.visible && s.tinting => {
                s.tint_parts(ws, t).map(|p| [p.outline, p.tint].concat())
            }
            Renderer::Selection(s) => s.outline_spans(ws, t),
            Renderer::Handles(h) => Some(h.iter().filter_map(|h| nubs::device_box(t, h)).collect()),
            Renderer::BrushPreview(p) => Some(p.device_box(t).into_iter().collect()),
            Renderer::PickerPreview(p) => Some(p.device_box(t).into_iter().collect()),
            Renderer::Tint(p) => Some(p.device_box(t).into_iter().collect()),
            Renderer::Grid(_) => Some(vec![]),
        }
    }

    /// The device rect an overlay draws into.
    fn bounds(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<IntRect> {
        match self {
            Renderer::Grid(_) => None,
            Renderer::Selection(r) => r.bounds(ws, t),
            Renderer::Handles(h) => h
                .iter()
                .filter_map(|h| nubs::device_box(t, h))
                .reduce(|a, b| a.union(&b)),
            Renderer::BrushPreview(p) => p.device_box(t),
            Renderer::PickerPreview(p) => p.device_box(t),
            Renderer::Tint(p) => p.device_box(t),
        }
    }
}

#[derive(Default)]
pub struct SurfaceBox {
    pub renderers: Vec<(RendererId, Renderer)>,
    next_id: RendererId,
    /// Bumped by every renderer list change and every `renderer_mut`.
    revision: u64,
    /// Per renderer, the `revision` of its last add or `renderer_mut`.
    stamps: Vec<(RendererId, u64)>,
    /// Bumped by `set_ants_frame` (a frame-only change).
    ants_revision: u64,
}

impl SurfaceBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// The surface is always the workspace's composition (`setSurface(view.composition)`):
    /// `render_base`, then `render_overlays`.
    pub fn render(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
        style: &CanvasStyle,
    ) {
        self.render_base(ws, target, t, style);
        self.render_overlays(ws, target, t);
    }

    /// The view (surround, shadow, checkerboard, document), then the visible grid renderers.
    pub fn render_base(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
        style: &CanvasStyle,
    ) {
        self.render_base_in(ws, target, t, style, None);
    }

    /// `render_base` inside just `spans` (one-row rects sorted by row, none overlapping) ∩ the
    /// target's clip, or the whole clip (`None`).
    pub fn render_base_in(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
        style: &CanvasStyle,
        spans: Option<&[IntRect]>,
    ) {
        view::render_view_in(
            &ws.view.composition,
            ws.view.composition_revision,
            &mut ws.view_mips(),
            t,
            style,
            target,
            spans,
        );
        self.render_grid_in(ws, target, t, spans);
    }

    /// `render_base_in` of the whole clip with the checkerboard's colours swapped.
    pub fn render_base_inverted_in(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
        style: &CanvasStyle,
    ) {
        view::render_view_inverted_in(
            &ws.view.composition,
            ws.view.composition_revision,
            &mut ws.view_mips(),
            t,
            style,
            target,
        );
        self.render_grid_in(ws, target, t, None);
    }

    /// The grid, if shown (the part of the base over the view), inside the target's clip.
    pub fn render_grid_in(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
        spans: Option<&[IntRect]>,
    ) {
        for (_, r) in &self.renderers {
            if let Renderer::Grid(g) = r
                && g.visible
            {
                g.render_in(ws, target, t, spans);
            }
        }
    }

    /// Every other visible renderer, in insertion order (selection layers, tool layers), inside
    /// the target's clip.
    pub fn render_overlays(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
    ) {
        target.paint_rows(self.overlay_painter(ws, t, target.mode));
    }

    /// The overlays as a row painter for a `mode` target (`DisplayTarget::paint_rows`,
    /// `write_rgba8_over`): each visible one composited in order over a device row's pixels.
    pub fn overlay_painter(
        &self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        mode: RenderMode,
    ) -> impl Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send + use<> {
        type Paint = Box<dyn Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send>;
        let mut painters: Vec<Paint> = vec![];
        for (_, r) in self.renderers.iter().filter(|(_, r)| r.visible()) {
            match r {
                Renderer::Selection(s) => {
                    if let Some(p) = s.painter(ws, t, mode) {
                        painters.push(Box::new(move |y, x0, row| p.paint(y, x0, row)));
                    }
                }
                Renderer::Handles(hs) => {
                    for p in hs
                        .iter()
                        .filter_map(|h| nubs::HandlePainter::new(t, h, mode))
                    {
                        painters.push(Box::new(move |y, x0, row| p.paint(y, x0, row)));
                    }
                }
                Renderer::BrushPreview(b) => {
                    if let Some(p) = nubs::BrushPreviewPainter::new(t, b, mode) {
                        painters.push(Box::new(move |y, x0, row| p.paint(y, x0, row)));
                    }
                }
                Renderer::PickerPreview(p) => {
                    if let Some(p) = p.painter(t, mode) {
                        painters.push(Box::new(p));
                    }
                }
                Renderer::Tint(p) => {
                    if let Some(p) = p.painter(t, mode) {
                        painters.push(Box::new(p));
                    }
                }
                Renderer::Grid(_) => {}
            }
        }
        move |y, x0, row| {
            for p in &painters {
                p(y, x0, row);
            }
        }
    }

    /// The device pixels a `set_ants_frame` change can touch: the outlined selection layers'
    /// outline spans, one-row rects sorted by row (two layers' may overlap). `None` when they are
    /// so many that refreshing the whole `overlay_bounds` is about as cheap.
    pub fn overlay_spans(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<Vec<IntRect>> {
        // Handles do not change with the ants frame.
        let layers = self.renderers.iter().map(|(_, r)| match r {
            Renderer::Selection(s) => s.outline_spans(ws, t),
            Renderer::Grid(_)
            | Renderer::Handles(_)
            | Renderer::BrushPreview(_)
            | Renderer::PickerPreview(_)
            | Renderer::Tint(_) => Some(vec![]),
        });
        let mut spans: Vec<IntRect> = layers.collect::<Option<Vec<_>>>()?.concat();
        spans.sort_unstable_by_key(|s| (s.y, s.x));
        Some(spans)
    }

    /// Every selection layer's ants frame, without bumping `revision`: only `overlay_spans`
    /// change. Bumps `ants_revision` when a frame changed.
    pub fn set_ants_frame(&mut self, frame: u32) {
        for (_, r) in &mut self.renderers {
            if let Renderer::Selection(s) = r
                && s.frame != frame
            {
                s.frame = frame;
                self.ants_revision += 1;
            }
        }
    }

    /// Changes whenever `set_ants_frame` changed a frame.
    pub fn ants_revision(&self) -> u64 {
        self.ants_revision
    }

    /// The union of the device rects the overlays draw into (`None` = nothing).
    pub fn overlay_bounds(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<IntRect> {
        self.renderers
            .iter()
            .filter(|(_, r)| r.visible() && !r.is_base())
            .filter_map(|(_, r)| r.bounds(ws, t))
            .reduce(|a, b| a.union(&b))
    }

    /// Per overlay (every renderer but the base ones), its id and what its pixels depend on
    /// besides the view and the ants frame: its last change (`renderer_mut`) and, for a selection
    /// layer, the selection's revision and the mask-build `stamp` (`selection::mask_builds`).
    pub fn overlay_keys(&self, ws: &DocumentWorkspace, stamp: u64) -> Vec<(RendererId, [u64; 3])> {
        let changed = |id| {
            self.stamps
                .iter()
                .find(|(i, _)| *i == id)
                .map_or(0, |s| s.1)
        };
        let layers = self.renderers.iter().filter(|(_, r)| !r.is_base());
        layers
            .map(|(id, r)| {
                let key = match r {
                    Renderer::Selection(_) => [changed(*id), ws.selection.revision(), stamp],
                    _ => [changed(*id), 0, 0],
                };
                (*id, key)
            })
            .collect()
    }

    /// Rects holding every device pixel overlay `id` paints (none if it is hidden or gone);
    /// `None` when they are not listed (too many, or a tint).
    pub fn overlay_paint_rects(
        &self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        id: RendererId,
    ) -> Option<Vec<IntRect>> {
        match self.renderer(id) {
            Some(r) if r.visible() => r.paint_rects(ws, t),
            _ => Some(vec![]),
        }
    }

    /// Overlay `id`'s outline and tint runs when it is a tinting selection layer
    /// (`SelectionRenderer::tint_parts`).
    pub fn overlay_tint_parts(
        &self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        id: RendererId,
    ) -> Option<selection::TintParts> {
        match self.renderer(id) {
            Some(Renderer::Selection(s)) => s.tint_parts(ws, t),
            _ => None,
        }
    }

    /// Changes whenever a renderer may have changed (list edits, `renderer_mut`).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn add_renderer(&mut self, renderer: Renderer) -> RendererId {
        let id = self.next_id;
        self.next_id += 1;
        self.revision += 1;
        self.stamps.push((id, self.revision));
        self.renderers.push((id, renderer));
        id
    }

    pub fn remove_renderer(&mut self, id: RendererId) {
        self.revision += 1;
        self.renderers.retain(|(i, _)| *i != id);
        self.stamps.retain(|(i, _)| *i != id);
    }

    pub fn renderer_mut(&mut self, id: RendererId) -> Option<&mut Renderer> {
        self.revision += 1;
        if let Some(s) = self.stamps.iter_mut().find(|(i, _)| *i == id) {
            s.1 = self.revision;
        }
        self.renderers
            .iter_mut()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| r)
    }

    pub fn renderer(&self, id: RendererId) -> Option<&Renderer> {
        self.renderers
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, r)| r)
    }
}

#[cfg(test)]
mod tests {
    use super::selection::SelectionRenderer;
    use super::*;
    use crate::pixel::mask::SelectionQuality;
    use crate::selection::CombineMode;
    use crate::selection::path::GraphicsPath;
    use crate::tool::ToolType;
    use crate::util::geom::{Point, Rectangle};

    /// An ants frame alone (`set_ants_frame`) leaves `revision` alone; a painted write of just
    /// `overlay_spans` then gives the pixels a full painted write gives at that frame, from any
    /// earlier one, and both match each overlay drawn into the target in turn: a rectangle and a
    /// crossing lasso at a fractional zoom, tinted, under an overlapping live layer and tool
    /// handles (one off the view) and a brush outline, HighQuality and Aliased, in both modes.
    #[test]
    fn span_refresh_matches_a_full_refresh() {
        let mut app = crate::testing::fresh_doc(120, 90, None);
        app.set_active_tool_from_type(ToolType::RectangleSelect);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(4.0, 5.0, 50.0, 30.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let lasso = [
            (60.2, 8.1),
            (110.7, 70.3),
            (70.4, 80.0),
            (100.1, 12.7),
            (58.0, 60.5),
        ];
        let lasso: Vec<Point> = lasso.iter().map(|&(x, y)| Point::new(x, y)).collect();
        ws.selection
            .set_continuation_points(&lasso, CombineMode::Union);
        ws.selection.commit_continuation();
        // A tool's live layer over part of it: the two layers' spans overlap.
        let live = GraphicsPath::from_rect(&Rectangle::new(30.0, 20.0, 50.0, 40.0));
        ws.surface_box
            .add_renderer(Renderer::Selection(SelectionRenderer {
                path: Some(live),
                ..Default::default()
            }));
        // Tool handles over the outlines: painted last, inside the spans too.
        let handle = |canvas, shape, opacity| nubs::Handle {
            canvas,
            shape,
            opacity,
            hover: 0.3,
        };
        ws.surface_box.add_renderer(Renderer::Handles(vec![
            handle([4.0, 5.0], nubs::HandleShape::Square, 1.0),
            handle([54.0, 20.0], nubs::HandleShape::Circle, 0.6),
            handle([80.0, 60.0], nubs::HandleShape::Compass, 1.0),
            handle([139.0, 99.0], nubs::HandleShape::Square, 1.0),
        ]));
        // A brush outline across the lasso, half faded.
        ws.surface_box
            .add_renderer(Renderer::BrushPreview(nubs::BrushPreview {
                canvas: [75.5, 40.5],
                radius: 12.0,
                pressure: None,
                opacity: 0.5,
            }));
        // The Color Picker's square across an outline.
        ws.surface_box
            .add_renderer(Renderer::PickerPreview(nubs::PickerPreview {
                cell: [57, 30],
                size: 3,
                visible: true,
            }));
        let t = ViewTransform {
            scale: 1.7,
            origin: [3.4, 2.2],
            viewport: [240, 170],
            ui_scale: 1.0,
        };
        let all = IntRect::new(0, 0, 240, 170);
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for quality in [SelectionQuality::HighQuality, SelectionQuality::Aliased] {
                let ws = app.active_ws_mut().unwrap();
                for (_, r) in &mut ws.surface_box.renderers {
                    if let Renderer::Selection(s) = r {
                        s.quality = quality;
                        s.frames = SelectionRenderer::frames_for(mode, true);
                    }
                }
                let base = || {
                    let mut base = DisplayTarget::new(240, 170, mode);
                    base.fill(all, [40, 90, 200, 255]);
                    base.fill(IntRect::new(30, 20, 150, 100), [250, 250, 245, 255]);
                    base.fill(IntRect::new(90, 0, 40, 170), [10, 10, 10, 128]);
                    base
                };
                let target = base();
                let write =
                    |ws: &DocumentWorkspace, spans: Option<&[IntRect]>, out: &mut Vec<u8>| {
                        let paint = ws.surface_box.overlay_painter(ws, &t, mode);
                        let px = out.as_chunks_mut().0;
                        target.write_rgba8_over(all, spans, px, |p: [u8; 4]| p, paint);
                    };
                let full = |ws: &DocumentWorkspace| {
                    let mut out = vec![0; 240 * 170 * 4];
                    write(ws, None, &mut out);
                    // The same as drawing each layer into the target in turn.
                    let mut drawn = base();
                    for (_, r) in &ws.surface_box.renderers {
                        match r {
                            Renderer::Selection(s) => s.render(ws, &mut drawn, &t),
                            Renderer::Handles(h) => {
                                h.iter().for_each(|h| nubs::draw_handle(&mut drawn, &t, h))
                            }
                            Renderer::BrushPreview(b) => {
                                if let Some(p) = nubs::BrushPreviewPainter::new(&t, b, mode) {
                                    drawn.paint_rows(|y, x0, row| p.paint(y, x0, row));
                                }
                            }
                            Renderer::PickerPreview(p) => {
                                if let Some(p) = p.painter(&t, mode) {
                                    drawn.paint_rows(p);
                                }
                            }
                            Renderer::Tint(p) => {
                                if let Some(p) = p.painter(&t, mode) {
                                    drawn.paint_rows(p);
                                }
                            }
                            Renderer::Grid(_) => {}
                        }
                    }
                    assert!(drawn.to_rgba8() == out);
                    out
                };
                let mut image = full(app.active_ws().unwrap());
                for f in 1..12 {
                    let ws = app.active_ws_mut().unwrap();
                    let (rev, ants) = (ws.surface_box.revision(), ws.surface_box.ants_revision());
                    ws.surface_box.set_ants_frame(f % 8);
                    assert_eq!(ws.surface_box.revision(), rev);
                    assert_ne!(ws.surface_box.ants_revision(), ants);
                    let ws = app.active_ws().unwrap();
                    let spans = ws.surface_box.overlay_spans(ws, &t).expect("sparse");
                    write(ws, Some(&spans), &mut image);
                    assert!(image == full(ws), "{mode:?} {quality:?} frame {f}");
                }
            }
        }
    }
}
