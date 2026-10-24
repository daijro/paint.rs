//! The canvas view's pixels between frames: the base (view, grid) kept in a `DisplayTarget`, the
//! overlays painted over a copy of its rows on the way to the caller's upload pixels. A frame
//! redraws only what changed: the device pixels of the document rects invalidated since the last
//! one, and the old and new pixels of each overlay that changed, as one-row spans while they are
//! few (Task 27r). Every frame is byte-identical to a full refresh.
//! As in Paint.NET, invalidated rects are queued, unioned and clipped; only what they cover
//! re-renders.

use super::RendererId;
use super::display::{DisplayTarget, SPAN_GAP, SpanMask};
use super::float::{Float, FloatPainter, Levels};
use super::view::{self, CanvasStyle, MipChain, ViewTransform};
use crate::document::layer::BlendMode;
use crate::document::surface::Surface;
use crate::document::workspace::DocumentWorkspace;
use crate::pixel::IntRect;
use crate::util::geom::Rectangle;

/// Everything the base's pixels depend on (the transform carries the device viewport).
#[derive(Clone, Debug, PartialEq)]
struct BaseKey {
    /// The document shown (`Document::id`): another workspace at the same revision and view is
    /// another picture.
    doc: u64,
    t: ViewTransform,
    revision: u64,
    style: CanvasStyle,
    /// The grid renderer's visibility (View > Pixel grid).
    grid: bool,
}

/// Invalidated rects kept for one redraw at most; past that it redraws everything.
const MAX_DIRTY: usize = 1 << 18;

/// What a `render` rewrote in the caller's pixels.
#[derive(Debug, PartialEq)]
pub enum Upload {
    Nothing,
    All,
    Rect(IntRect),
    /// One-row rects sorted by row (`display::span_bands` makes upload rects of them).
    Spans(Vec<IntRect>),
}

#[derive(Default)]
pub struct CanvasPixels {
    /// The device-pixel base (`render_base`).
    target: Option<DisplayTarget>,
    key: Option<BaseKey>,
    /// Document rects `invalidate`d since the base was drawn, unless `everything` was, and how
    /// many invalidations brought them. Each bumps the composition revision once and queues its
    /// event in the workspace: an event still queued, or a revision that moved further, is an
    /// edit not seen yet (a full redraw).
    dirty: Vec<Rectangle>,
    everything: bool,
    invalidations: u64,
    /// (selection revision, `SurfaceBox` revision, mask-build stamp) the overlays were last drawn
    /// for, and the `SurfaceBox` ants revision.
    overlay_key: Option<(u64, u64, u64)>,
    overlay_ants: u64,
    /// The device rect they drew into.
    overlay_rect: Option<IntRect>,
    /// The (document, view) of `layers`, and per overlay drawn its id, key
    /// (`SurfaceBox::overlay_keys`) and paint rects.
    view: Option<(u64, ViewTransform)>,
    layers: Vec<(RendererId, [u64; 3], Option<Vec<IntRect>>)>,
    /// Set by the caller while a pan or zoom is in progress: a change of view alone is drawn as a
    /// draft (see `render_draft`) until a render without it settles the view exactly.
    pub draft: bool,
    /// The upload pixels show a draft at this transform, not `key`'s view.
    drafted: Option<ViewTransform>,
    /// The moving pixels drawn (`DocumentWorkspace::float`): their revision and device bounds;
    /// the composite of the layers under them with its levels (per lift and the layers' shown
    /// properties), its view (per view and style) and the float's own levels.
    float_rev: Option<u64>,
    float_rect: Option<IntRect>,
    under: Option<(UnderKey, Option<Surface>, MipChain)>,
    below: Option<((ViewTransform, CanvasStyle), DisplayTarget)>,
    levels: Option<Levels>,
    /// A zoom draft's half-resolution view; a pan draft's planes over translucent pixels (see
    /// `draft_translucent`) at their transform. Both go when a render settles the view.
    half: Option<DisplayTarget>,
    planes: Option<(ViewTransform, [Vec<[u8; 4]>; 2])>,
    /// Whether the composite of (document, revision) has no translucent pixel.
    opaque: Option<((u64, u64), bool)>,
}

/// A float's lift and every layer's (visible, opacity, blend mode): what the composite under it
/// depends on.
type UnderKey = (u64, Vec<(bool, u8, BlendMode)>);

impl BaseKey {
    fn same_view(&self, other: &BaseKey) -> bool {
        BaseKey {
            revision: other.revision,
            ..self.clone()
        } == *other
    }
}

/// `SpanMask` spans of `rects` within `area` while they cost under half of `budget` px.
fn sparse(rects: &[IntRect], area: IntRect, budget: u64) -> Option<Vec<IntRect>> {
    let mut mask = SpanMask::new(area);
    rects.iter().for_each(|r| mask.mark(*r));
    let (spans, cost) = mask.spans(SPAN_GAP);
    (cost * 2 < budget).then_some(spans)
}

/// Moves the `w × h` row-major `px` by (dx, dy) (|dx| < w, |dy| < h); the uncovered pixels keep
/// what they held.
fn shift<T: Copy>(px: &mut [T], w: usize, h: usize, dx: i32, dy: i32) {
    let (cols, len) = (dx.unsigned_abs() as usize, w - dx.unsigned_abs() as usize);
    let (from, to) = if dx > 0 { (0, cols) } else { (cols, 0) };
    let rows = dy.unsigned_abs() as usize..h;
    let mut copy = |y: usize| {
        let src = if dy > 0 { y - rows.start } else { y };
        let dst = if dy > 0 { y } else { y - rows.start };
        let (src, dst) = (src * w + from, dst * w + to);
        px.copy_within(src..src + len, dst);
    };
    // Each row before the one it lands on is read.
    if dy > 0 {
        rows.clone().rev().for_each(&mut copy);
    } else {
        for y in rows.clone() {
            copy(y);
        }
    }
}

/// `planes` (the view at `t` with every checkerboard square light, and every one dark, overlays
/// included) inside `r`, drawn through `target`; `drawn`: the target already holds the base at
/// `t` there.
fn fill_planes(
    ws: &DocumentWorkspace,
    target: &mut DisplayTarget,
    t: &ViewTransform,
    style: &CanvasStyle,
    r: IntRect,
    planes: &mut [Vec<[u8; 4]>; 2],
    drawn: bool,
) {
    let paint = ws.surface_box.overlay_painter(ws, t, style.mode);
    let [first, second] = planes;
    target.set_clip(Some(r));
    if !drawn {
        ws.surface_box.render_base_in(ws, target, t, style, None);
    }
    target.write_rgba8_over(r, None, first, |p| p, &paint);
    ws.surface_box.render_base_inverted_in(ws, target, t, style);
    target.write_rgba8_over(r, None, second, |p| p, &paint);
    target.set_clip(None);
    // The view and its inverse: swapped where the square is dark.
    let w = target.width as usize;
    for y in r.y..r.bottom() {
        for (run, lit) in square_runs(r.x..r.right(), y, t.ui_scale) {
            if !lit {
                let row = y as usize * w;
                let span = row + run.start..row + run.end;
                first[span.clone()].swap_with_slice(&mut second[span]);
            }
        }
    }
}

/// Row `y`'s columns `xs` in runs within one checkerboard square (the view's at UI scale `u`),
/// each with whether its square is light.
fn square_runs(
    xs: std::ops::Range<i32>,
    y: i32,
    u: f64,
) -> impl Iterator<Item = (std::ops::Range<usize>, bool)> {
    let side = super::checker::checker_square(u, true);
    let s = side as i32;
    let mut x = xs.start;
    std::iter::from_fn(move || {
        (x < xs.end).then(|| {
            let end = ((x.div_euclid(s) + 1) * s).min(xs.end);
            let run = (
                x as usize..end as usize,
                super::checker::checker_is_light(x, y, side),
            );
            x = end;
            run
        })
    })
}

fn area(r: IntRect) -> u64 {
    r.w as u64 * r.h as u64
}

impl CanvasPixels {
    /// The upload pixels show a draft (`draft`), not the settled view.
    pub fn drafted(&self) -> bool {
        self.drafted.is_some()
    }

    /// `DocumentInvalidated { rects }` (empty = the whole document).
    pub fn invalidate(&mut self, rects: &[Rectangle]) {
        self.invalidations += 1;
        if rects.is_empty() || self.dirty.len() + rects.len() > MAX_DIRTY {
            self.everything = true;
            self.dirty.clear();
        } else if !self.everything {
            self.dirty.extend_from_slice(rects);
        }
    }

    /// Brings the caller's upload pixels (`out`, asked for only when they change: the viewport's
    /// pixels row-major, through `conv`) to `ws` under `t` and `style`, `stamp` being this frame's
    /// `selection::mask_builds`. An empty `CanvasPixels` draws everything.
    pub fn render<'a, T: Copy + Send + 'a>(
        &mut self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        style: &CanvasStyle,
        stamp: u64,
        out: impl FnOnce() -> &'a mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
    ) -> Upload {
        let [w, h] = t.viewport.map(|v| v.max(1));
        let all = IntRect::new(0, 0, w, h);
        let out = || {
            let out = out();
            debug_assert_eq!(out.len(), w as usize * h as usize);
            out
        };
        let key = BaseKey {
            doc: ws.document.id,
            t: *t,
            revision: ws.view.composition_revision,
            style: *style,
            grid: ws.grid_visible,
        };
        if self.draft
            && self.view_only(ws, &key)
            && let Some(redo) = self.draft_overlays(ws, &key, stamp)
        {
            return self.render_draft(ws, &key, &redo, out(), conv);
        }
        if self.drafted.is_some() {
            // The view settles: the drafts' buffers go.
            (self.half, self.planes) = (None, None);
        }
        // The device px whose base changed: their bounds, and their spans when listed.
        let base =
            (self.key.as_ref() != Some(&key) || self.target.is_none() || self.drafted.is_some())
                .then(|| self.render_base(ws, key, all));
        let overlay_key = (ws.selection.revision(), ws.surface_box.revision(), stamp);
        let ants = ws.surface_box.ants_revision();
        let ants_moved = self.overlay_ants != ants;
        self.overlay_ants = ants;
        // The moving pixels: their old and new device bounds when they moved.
        let float = ws.float.as_deref();
        match float {
            Some(f) => self.prepare_float(ws, f, t, style),
            None => (self.under, self.below, self.levels) = (None, None, None),
        }
        let float_rect = float.map(|f| f.device_bounds(t, ws.document.width, ws.document.height));
        let float_moved = self.float_rev != float.map(|f| f.rev);
        let floated = [self.float_rect, float_rect].map(|r| r.filter(|_| float_moved));
        self.float_rev = float.map(|f| f.rev);
        self.float_rect = float_rect;
        let content = base.is_some() || self.overlay_key != Some(overlay_key) || float_moved;
        if !content && !ants_moved {
            return Upload::Nothing;
        }
        let mode = self.target.as_ref().expect("drawn above").mode;
        // An ants frame alone changes only the outline pixels (unless they are too many).
        if !content && let Some(spans) = ws.surface_box.overlay_spans(ws, t) {
            let target = self.target.as_ref().expect("drawn above");
            let paint = self.painter(ws, t, mode, float);
            target.write_rgba8_over(all, Some(&spans), out(), conv, paint);
            return Upload::Spans(spans);
        }
        let drawn = ws
            .surface_box
            .overlay_bounds(ws, t)
            .map(|r| r.intersect(&all));
        let refresh = [base.as_ref().map(|b| b.0), self.overlay_rect, drawn]
            .into_iter()
            .chain(floated)
            .flatten()
            .fold(IntRect::default(), |a, b| a.union(&b))
            .intersect(&all);
        // What changed, listed: the base's, each changed overlay's old and new pixels, and the
        // outline spans if the ants moved.
        let mut pieces = self.overlay_changes(ws, t, stamp);
        let mut add = |rects: Option<&[IntRect]>| match (&mut pieces, rects) {
            (Some(p), Some(r)) => p.extend_from_slice(r),
            (p, _) => *p = None,
        };
        if let Some((bounds, spans)) = &base {
            add(Some(
                spans.as_deref().unwrap_or(std::slice::from_ref(bounds)),
            ));
        }
        if ants_moved {
            add(ws.surface_box.overlay_spans(ws, t).as_deref());
        }
        for r in floated.into_iter().flatten() {
            add(Some(&[r.intersect(&all)]));
        }
        let spans = pieces.and_then(|p| sparse(&p, all, area(refresh)));
        self.overlay_rect = drawn;
        self.overlay_key = Some(overlay_key);
        let target = self.target.as_ref().expect("drawn above");
        let paint = self.painter(ws, t, mode, float);
        match spans {
            Some(spans) if spans.is_empty() => Upload::Nothing,
            Some(spans) => {
                target.write_rgba8_over(all, Some(&spans), out(), conv, paint);
                Upload::Spans(spans)
            }
            None if refresh.is_empty() => Upload::Nothing,
            None => {
                target.write_rgba8_over(refresh, None, out(), conv, paint);
                match refresh == all {
                    true => Upload::All,
                    false => Upload::Rect(refresh),
                }
            }
        }
    }

    /// The rows' painter over the base: the moving pixels, then the overlays.
    fn painter<'s>(
        &'s self,
        ws: &'s DocumentWorkspace,
        t: &ViewTransform,
        mode: super::display::RenderMode,
        float: Option<&'s Float>,
    ) -> impl Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send + 's {
        let overlays = ws.surface_box.overlay_painter(ws, t, mode);
        let doc = &ws.document;
        let shown = |f: &Float| {
            doc.layers
                .get_at(f.layer)
                .is_some_and(|l| l.properties.visible)
        };
        let float = match (float.filter(|f| shown(f)), &self.levels, &self.below) {
            (Some(f), Some(levels), Some((_, below))) => {
                let opacity = doc
                    .layers
                    .get_at(f.layer)
                    .map_or(255, |l| l.properties.opacity);
                let size = (doc.width, doc.height);
                Some(FloatPainter::new(
                    f,
                    t,
                    size,
                    opacity as f32 / 255.0,
                    levels,
                    below,
                ))
            }
            _ => None,
        };
        move |y, x0, row: &mut [[f32; 4]]| {
            if let Some(f) = &float {
                f.paint(y, x0, row);
            }
            overlays(y, x0, row);
        }
    }

    /// The view of the layers under `f` (for its lift, the view and the style) and the levels it
    /// samples under `t`.
    fn prepare_float(
        &mut self,
        ws: &DocumentWorkspace,
        f: &Float,
        t: &ViewTransform,
        style: &CanvasStyle,
    ) {
        let doc = &ws.document;
        let props = doc.layers.iter().map(|l| {
            let p = &l.properties;
            (p.visible, p.opacity, p.blend_mode)
        });
        let under: UnderKey = (f.src.id, props.collect());
        if self.under.as_ref().is_none_or(|(k, ..)| *k != under) {
            let composite = doc.composite_under(f.layer);
            self.under = Some((under, composite, MipChain::default()));
            self.below = None;
        }
        // A new view samples the composite again (it and its levels are kept).
        let key = (*t, *style);
        if self.below.as_ref().is_none_or(|(k, _)| *k != key) {
            let [w, h] = t.viewport.map(|v| v.max(1));
            let mut below = match self.below.take() {
                Some((_, b)) if (b.width, b.height, b.mode) == (w, h, style.mode) => b,
                _ => DisplayTarget::new(w, h, style.mode),
            };
            below.set_clip(None);
            match self.under.as_mut().expect("set above") {
                (_, Some(c), mips) => view::render_view_in(c, 0, mips, t, style, &mut below, None),
                (_, None, _) => view::render_empty_in(doc.width, doc.height, t, style, &mut below),
            }
            self.below = Some((key, below));
        }
        self.levels = Some(Levels::up_to(
            self.levels.take(),
            &f.src,
            style.mode,
            f.level(t),
        ));
    }

    /// Only the view changed since the base was drawn (or since the last draft): the same
    /// document, revision, style and grid, and no edit waiting to be drawn.
    fn view_only(&self, ws: &DocumentWorkspace, key: &BaseKey) -> bool {
        ws.float.is_none()
            && self.target.is_some()
            && self.dirty.is_empty()
            && !self.everything
            && self.invalidations == 0
            && self.key.as_ref().is_some_and(|old| {
                (old.doc, old.revision, old.style, old.grid)
                    == (key.doc, key.revision, key.style, key.grid)
                    && (old.t != key.t || self.drafted.is_some())
            })
            && !ws
                .view
                .events
                .iter()
                .any(|e| matches!(e, crate::app::AppEvent::DocumentInvalidated { .. }))
    }

    /// The transform shown now (a draft's, or the base's).
    fn shown(&self) -> ViewTransform {
        self.drafted
            .or(self.key.as_ref().map(|k| k.t))
            .expect("view_only: drawn before")
    }

    /// A pan draft's transform and whole device px offset from the shown one (`None`: a zoom).
    fn pan_of(&self, key: &BaseKey) -> Option<(ViewTransform, [f64; 2])> {
        let shown = self.shown();
        let [w, h] = key.t.viewport.map(|v| v.max(1));
        let pan = (shown.scale, shown.viewport, shown.ui_scale)
            == (key.t.scale, key.t.viewport, key.t.ui_scale);
        let d = [0, 1].map(|i| (key.t.origin[i] - shown.origin[i]).round());
        let t = ViewTransform {
            origin: [shown.origin[0] + d[0], shown.origin[1] + d[1]],
            ..shown
        };
        (pan && d[0].abs() < w as f64 && d[1].abs() < h as f64).then_some((t, d))
    }

    /// The device px a draft draws again for the overlays that changed since they were drawn (a
    /// preview following the pointer, a selection being dragged while the view scrolls): their
    /// old pixels, which moved with the rest, and their new ones. `None` when they cannot be
    /// listed: the frame renders exactly instead. A zoom draft draws every overlay anyway.
    fn draft_overlays(
        &mut self,
        ws: &DocumentWorkspace,
        key: &BaseKey,
        stamp: u64,
    ) -> Option<Vec<IntRect>> {
        let keys = ws.surface_box.overlay_keys(ws, stamp);
        let Some((t, _)) = self.pan_of(key) else {
            // Drawn at half resolution: their pixels are not listed.
            self.layers = keys.into_iter().map(|(id, k)| (id, k, None)).collect();
            self.view = Some((ws.document.id, key.t));
            return Some(vec![]);
        };
        // The rects were listed at `self.view`'s transform: they move with the pixels.
        let offset = match self.view {
            Some((doc, v))
                if doc == ws.document.id
                    && (v.scale, v.viewport, v.ui_scale) == (t.scale, t.viewport, t.ui_scale) =>
            {
                Some([0, 1].map(|i| (t.origin[i] - v.origin[i]).round() as i32))
            }
            _ => None,
        };
        let moved = |rects: &Option<Vec<IntRect>>| match (rects, offset) {
            (Some(r), Some([dx, dy])) => Some(
                r.iter()
                    .map(|r| IntRect::new(r.x + dx, r.y + dy, r.w, r.h))
                    .collect(),
            ),
            _ => None,
        };
        let mut old = std::mem::take(&mut self.layers);
        let (mut layers, mut redo) = (vec![], vec![]);
        for (id, k) in keys {
            let was = old
                .iter()
                .position(|l| l.0 == id)
                .map(|i| old.swap_remove(i));
            let rects = match was {
                Some((_, wk, rects)) if wk == k => moved(&rects),
                was => {
                    let now = ws.surface_box.overlay_paint_rects(ws, &t, id);
                    let before = was.map_or(Some(vec![]), |w| moved(&w.2));
                    redo.extend(before?);
                    redo.extend(now.clone()?);
                    now
                }
            };
            layers.push((id, k, rects));
        }
        for (_, _, gone) in &old {
            redo.extend(moved(gone)?);
        }
        self.layers = layers;
        self.view = Some((ws.document.id, t));
        Some(redo)
    }

    /// A pan or zoom frame, drawn cheaply; the next render without `draft` draws the view exactly.
    /// A pan by whole device px (rounded) moves the pixels shown and draws only the strips it
    /// uncovers, at the shown transform moved by that much (≤ ½ px off until the view settles),
    /// plus `redo` (the overlays that changed); any other change of view draws everything at half
    /// resolution with one Linear tap (`render_half`). The overlays are drawn with the pixels
    /// they lie on; the ants hold still meanwhile.
    fn render_draft<T: Copy + Send>(
        &mut self,
        ws: &DocumentWorkspace,
        key: &BaseKey,
        redo: &[IntRect],
        out: &mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
    ) -> Upload {
        let shown = self.shown();
        let [w, h] = key.t.viewport.map(|v| v.max(1));
        let style = CanvasStyle {
            draft: true,
            ..key.style
        };
        let (t, d, strips) = if let Some((t, d)) = self.pan_of(key) {
            if d == [0.0, 0.0] && redo.is_empty() {
                return Upload::Nothing;
            }
            let (dx, dy) = (d[0] as i32, d[1] as i32);
            // The columns, then the rest of the rows, that the move uncovered.
            let cols = match dx {
                0 => IntRect::default(),
                dx if dx > 0 => IntRect::new(0, 0, dx as u32, h),
                dx => IntRect::new(w as i32 + dx, 0, (-dx) as u32, h),
            };
            let (rx, rw) = (dx.max(0), w as i32 - dx.abs());
            let rows = match dy {
                0 => IntRect::default(),
                dy if dy > 0 => IntRect::new(rx, 0, rw as u32, dy as u32),
                dy => IntRect::new(rx, h as i32 + dy, rw as u32, (-dy) as u32),
            };
            (t, d, vec![cols, rows])
        } else {
            self.render_half(ws, &key.t, &style, out, conv);
            self.drafted = Some(key.t);
            return Upload::All;
        };
        let all = IntRect::new(0, 0, w, h);
        let redo = redo.iter().map(|r| r.intersect(&all));
        let regions: Vec<IntRect> = strips
            .into_iter()
            .chain(redo)
            .filter(|r| !r.is_empty())
            .collect();
        // The checkerboard is fixed to the viewport: where the image is translucent it does not
        // move with the pixels.
        if !self.composite_opaque(ws) {
            self.draft_translucent(ws, shown, t, d, &regions, &style, out, conv);
            self.drafted = Some(t);
            return Upload::All;
        }
        shift(out, w as usize, h as usize, d[0] as i32, d[1] as i32);
        let target = match self.target.take() {
            Some(target) if (target.width, target.height) == (w, h) => target,
            _ => DisplayTarget::new(w, h, style.mode),
        };
        let target = self.target.insert(target);
        let paint = ws.surface_box.overlay_painter(ws, &t, style.mode);
        for &r in &regions {
            target.set_clip(Some(r));
            ws.surface_box.render_base_in(ws, target, &t, &style, None);
            target.set_clip(None);
            target.write_rgba8_over(r, None, out, &conv, &paint);
        }
        self.drafted = Some(t);
        Upload::All
    }

    /// A pan draft over a translucent image. Two planes hold the view (overlays included) with
    /// every checkerboard square light, and every one dark; they move with the image (only the
    /// uncovered strips and `regions` are drawn again), and each device px shows the plane of the
    /// square fixed to the viewport there, as a full render at `t` does.
    #[allow(clippy::too_many_arguments)]
    fn draft_translucent<T: Copy + Send>(
        &mut self,
        ws: &DocumentWorkspace,
        shown: ViewTransform,
        t: ViewTransform,
        d: [f64; 2],
        regions: &[IntRect],
        style: &CanvasStyle,
        out: &mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
    ) {
        let [w, h] = t.viewport.map(|v| v.max(1));
        let (wu, hu, all) = (w as usize, h as usize, IntRect::new(0, 0, w, h));
        let mut target = match self.target.take() {
            Some(target) if (target.width, target.height) == (w, h) => target,
            _ => DisplayTarget::new(w, h, style.mode),
        };
        let moved = |mut planes: [Vec<[u8; 4]>; 2]| {
            for p in &mut planes {
                shift(p, wu, hu, d[0] as i32, d[1] as i32);
            }
            planes
        };
        let new = || [vec![[0u8; 4]; wu * hu], vec![[0u8; 4]; wu * hu]];
        let (mut planes, regions) = match self.planes.take() {
            Some((at, planes)) if at == shown => (moved(planes), regions.to_vec()),
            // The base still shows `shown` (in its own style): it is the first plane's half of
            // the squares.
            _ if self.drafted.is_none() => {
                let mut planes = new();
                let base = self.key.as_ref().expect("view_only: drawn before").style;
                fill_planes(ws, &mut target, &shown, &base, all, &mut planes, true);
                (moved(planes), regions.to_vec())
            }
            _ => {
                let mut planes = new();
                fill_planes(ws, &mut target, &t, style, all, &mut planes, false);
                (planes, vec![])
            }
        };
        for r in regions {
            fill_planes(ws, &mut target, &t, style, r, &mut planes, false);
        }
        let [light, dark] = &planes;
        super::display::for_bands(out, wu, 0..hu, 32, |y0, band| {
            for (j, row) in band.chunks_mut(wu).enumerate() {
                let y = y0 + j;
                let planes = [&light[y * wu..][..wu], &dark[y * wu..][..wu]];
                for (run, lit) in square_runs(0..w as i32, y as i32, t.ui_scale) {
                    let src = &planes[!lit as usize][run.clone()];
                    for (o, p) in row[run].iter_mut().zip(src) {
                        *o = conv(*p);
                    }
                }
            }
        });
        self.target = Some(target);
        self.planes = Some((t, planes));
    }

    /// No translucent pixel in the composite (cached per document and revision).
    fn composite_opaque(&mut self, ws: &DocumentWorkspace) -> bool {
        let key = (ws.document.id, ws.view.composition_revision);
        match self.opaque {
            Some((k, opaque)) if k == key => opaque,
            _ => {
                let data = ws.view.composition.data();
                let opaque = data.iter().skip(3).step_by(4).all(|&a| a == 255);
                self.opaque = Some((key, opaque));
                opaque
            }
        }
    }

    /// A zoom's draft: the view at half resolution (half the scale, origin and UI scale), each
    /// pixel shown as a 2 × 2 block.
    fn render_half<T: Copy + Send>(
        &mut self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        style: &CanvasStyle,
        out: &mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
    ) {
        let [w, h] = t.viewport.map(|v| v.max(1));
        let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
        let half = ViewTransform {
            scale: t.scale / 2.0,
            origin: t.origin.map(|o| o / 2.0),
            viewport: [hw, hh],
            ui_scale: t.ui_scale / 2.0,
        };
        let target = match self.half.take() {
            Some(target) if (target.width, target.height, target.mode) == (hw, hh, style.mode) => {
                target
            }
            _ => DisplayTarget::new(hw, hh, style.mode),
        };
        let target = self.half.insert(target);
        ws.surface_box
            .render_base_in(ws, target, &half, style, None);
        let mut small = vec![[0u8; 4]; hw as usize * hh as usize];
        let paint = ws.surface_box.overlay_painter(ws, &half, style.mode);
        target.write_rgba8_over(IntRect::new(0, 0, hw, hh), None, &mut small, |p| p, paint);
        let (w, hw) = (w as usize, hw as usize);
        super::display::for_bands(out, w, 0..h as usize, 32, |y0, band| {
            for (j, row) in band.chunks_mut(w).enumerate() {
                let src = &small[(y0 + j) / 2 * hw..][..hw];
                for (x, o) in row.iter_mut().enumerate() {
                    *o = conv(src[x / 2]);
                }
            }
        });
    }

    /// Draws the base where it changed: just the invalidated document rects' device pixels in an
    /// unchanged view once every edit's rects came in, else all of it. Returns their bounds, and
    /// their spans when they are few enough.
    fn render_base(
        &mut self,
        ws: &DocumentWorkspace,
        key: BaseKey,
        all: IntRect,
    ) -> (IntRect, Option<Vec<IntRect>>) {
        let t = key.t;
        let partial =
            match &self.key {
                Some(old)
                    if old.same_view(&key)
                        && self.drafted.is_none()
                        && !self.everything
                        && !self.dirty.is_empty()
                        && key.revision <= old.revision + self.invalidations
                        && !ws.view.events.iter().any(|e| {
                            matches!(e, crate::app::AppEvent::DocumentInvalidated { .. })
                        }) =>
                {
                    let rects: Vec<IntRect> = self
                        .dirty
                        .iter()
                        .map(|d| t.dirty_rect([d.x, d.y, d.right(), d.bottom()]))
                        .filter(|r| !r.is_empty())
                        .collect();
                    let bounds = rects.iter().fold(IntRect::default(), |a, b| a.union(b));
                    let spans = (rects.len() > 1)
                        .then(|| sparse(&rects, bounds, area(bounds)))
                        .flatten();
                    Some((bounds, spans))
                }
                _ => None,
            };
        let [w, h] = [all.w, all.h];
        let target = match self.target.take() {
            Some(t) if (t.width, t.height, t.mode) == (w, h, key.style.mode) => t,
            _ => DisplayTarget::new(w, h, key.style.mode),
        };
        let target = self.target.insert(target);
        let (bounds, spans) = partial.unwrap_or((all, None));
        target.set_clip(Some(bounds));
        ws.surface_box
            .render_base_in(ws, target, &t, &key.style, spans.as_deref());
        target.set_clip(None);
        self.key = Some(key);
        self.drafted = None;
        self.dirty.clear();
        (self.everything, self.invalidations) = (false, 0);
        (bounds, spans)
    }

    /// The rects each overlay that changed since the last draw painted then and paints now, and
    /// those of the overlays gone; `None` when some are not listed (or the view changed). Keeps
    /// each overlay's paint rects for the next frame.
    fn overlay_changes(
        &mut self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        stamp: u64,
    ) -> Option<Vec<IntRect>> {
        let (view, mut old) = (Some((ws.document.id, *t)), std::mem::take(&mut self.layers));
        if self.view != view {
            old.clear();
        }
        let mut pieces = (self.view == view).then(Vec::new);
        let mut add = |rects: &Option<Vec<IntRect>>| match (&mut pieces, rects) {
            (Some(p), Some(r)) => p.extend_from_slice(r),
            (p, _) => *p = None,
        };
        for (id, key) in ws.surface_box.overlay_keys(ws, stamp) {
            let was = old
                .iter()
                .position(|l| l.0 == id)
                .map(|i| old.swap_remove(i));
            let rects = match was {
                Some((_, k, rects)) if k == key => rects,
                was => {
                    let now = ws.surface_box.overlay_paint_rects(ws, t, id);
                    add(&was.map_or(Some(vec![]), |w| w.2));
                    add(&now);
                    now
                }
            };
            self.layers.push((id, key, rects));
        }
        for (_, _, gone) in &old {
            add(gone);
        }
        self.view = view;
        pieces
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppEvent, AppWorkspace};
    use crate::render::Renderer;
    use crate::render::display::RenderMode;
    use crate::render::nubs::{BrushPreview, Handle, HandleShape};
    use crate::render::selection::mask_builds;
    use crate::selection::CombineMode;
    use crate::selection::path::GraphicsPath;
    use crate::tool::types::ToolType;
    use crate::tool::{MouseButton, PointerInput, PointerSample, PointerType};
    use crate::util::geom::Point;
    use std::sync::Arc;

    fn input(x: f64, y: f64) -> PointerInput {
        PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![PointerSample {
                x,
                y,
                pressure: None,
                time_ms: 0.0,
            }],
        }
    }

    /// Unit squares at about one in `of` cells of `w × h` from (x, y): a sparse wand selection.
    fn cells(x: f64, y: f64, w: usize, h: usize, of: u64) -> GraphicsPath {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut path = GraphicsPath::default();
        for j in 0..h {
            for i in 0..w {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                if seed.is_multiple_of(of) {
                    let (x, y) = (x + i as f64, y + j as f64);
                    let square = [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];
                    path.polygons
                        .push(square.iter().map(|&(x, y)| Point::new(x, y)).collect());
                }
            }
        }
        path
    }

    /// A 300 × 200 document (noise, or white) in a 320 × 240 view at `zoom` (grid on), `path`
    /// selected.
    fn scene(zoom: f64, noise: bool, path: GraphicsPath) -> AppWorkspace {
        let mut app = crate::testing::fresh_doc(300, 200, noise.then_some(7));
        (app.view_width, app.view_height) = (320.0, 240.0);
        let ws = app.active_ws_mut().unwrap();
        (ws.view.view_width, ws.view.view_height) = (320.0, 240.0);
        ws.view.zoom = zoom;
        ws.view.center_view();
        ws.set_grid_visible(true);
        ws.selection
            .set_continuation_path(path, CombineMode::Replace);
        ws.selection.commit_continuation();
        app
    }

    /// A canvas drawn from frame to frame, each frame checked against one drawn from scratch.
    struct Check {
        canvas: CanvasPixels,
        style: CanvasStyle,
        out: Vec<[u8; 4]>,
    }

    impl Check {
        fn new(mode: RenderMode) -> Self {
            Check {
                canvas: CanvasPixels::default(),
                style: CanvasStyle::for_scheme(false, true, None, 0.75, mode),
                out: vec![],
            }
        }

        /// A frame: the invalidations, then the render; asserts it is byte-identical to a full
        /// refresh and returns the px it rewrote.
        fn frame(&mut self, app: &mut AppWorkspace, stamp: u64) -> u64 {
            for e in app.drain_events() {
                if let AppEvent::DocumentInvalidated { rects } = e {
                    self.canvas.invalidate(&rects);
                }
            }
            let ws = app.active_ws().unwrap();
            let t = ws.view.view_transform();
            let size = (t.viewport[0] * t.viewport[1]) as usize;
            self.out.resize(size, [0; 4]);
            let out = &mut self.out;
            let upload = self
                .canvas
                .render(ws, &t, &self.style, stamp, || &mut out[..], |p| p);
            let mut fresh = vec![[0u8; 4]; size];
            let full = CanvasPixels::default().render(
                ws,
                &t,
                &self.style,
                stamp,
                || &mut fresh[..],
                |p| p,
            );
            assert_eq!(full, Upload::All);
            let stale = self.out.iter().zip(&fresh).filter(|(a, b)| a != b).count();
            assert_eq!(stale, 0, "{:?}: stale pixels", self.style.mode);
            match upload {
                Upload::Nothing => 0,
                Upload::All => size as u64,
                Upload::Rect(r) => area(r),
                Upload::Spans(s) => s.iter().map(|r| area(*r)).sum(),
            }
        }
    }

    /// `step(app, i)`, `on_frame` and a checked frame, for i in 0..n, after a first frame.
    fn frames(
        app: &mut AppWorkspace,
        mode: RenderMode,
        n: usize,
        mut step: impl FnMut(&mut AppWorkspace, usize),
    ) -> Vec<u64> {
        let mut check = Check::new(mode);
        let mut uploaded = vec![check.frame(app, 0)];
        for i in 0..n {
            app.now_ms += 16.0;
            step(app, i);
            let now = app.now_ms;
            app.on_frame(now);
            uploaded.push(check.frame(app, 0));
        }
        uploaded
    }

    /// Task 27r: every frame of drags over a sparse selection (cells and a rectangle) — Move
    /// Selected Pixels translating (whole-pixel at 100 %) and rotating, Move Selection (tinted), a
    /// Paintbrush stroke — at 30 % (mips), 100 %, a fractional zoom and 300 % (the grid), in both
    /// modes, equals a full refresh; the sparse move's frames redraw only part of the view. At 30 %
    /// and 170 % Move Selected Pixels drags through the float, whose frames equal a full refresh
    /// drawing the same float.
    #[test]
    fn move_drag_frames_match_a_full_refresh() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for zoom in [0.3, 1.0, 1.7, 3.0] {
                // Sparse cells, and a rectangle whose tint shows inside its outline.
                let mut selection = cells(20.0, 20.0, 260, 160, 600);
                let rect = [(110.0, 95.0), (134.0, 95.0), (134.0, 111.0), (110.0, 111.0)];
                selection
                    .polygons
                    .push(rect.map(|(x, y)| Point::new(x, y)).to_vec());
                let mut app = scene(zoom, true, selection);
                // The display-resolution float (`render::float`) every other zoom.
                app.float_moves = zoom != 1.0 && zoom != 3.0;
                let ws = app.active_ws().unwrap();
                let t = ws.view.view_transform();
                let at = |x: f64, y: f64| (x * t.scale + t.origin[0], y * t.scale + t.origin[1]);
                let viewport = (t.viewport[0] * t.viewport[1]) as u64;
                for (tool, button) in [
                    (ToolType::Move, MouseButton::Left),
                    (ToolType::Move, MouseButton::Right),
                    (ToolType::MoveSelection, MouseButton::Left),
                    (ToolType::PaintBrush, MouseButton::Left),
                ] {
                    app.set_active_tool_from_type(tool);
                    let (x, y) = at(100.0, 80.0);
                    app.on_mouse_down(x, y, button, &input(x, y));
                    let up = frames(&mut app, mode, 6, |app, i| {
                        // The ants step too (`CanvasView`'s `sync_selection_layers`).
                        let ws = app.active_ws_mut().unwrap();
                        ws.surface_box.set_ants_frame(i as u32 % 8);
                        // One px a frame, then a jump past the outline's width.
                        let dx = if i >= 4 { 12.0 } else { 0.0 };
                        let (x, y) = at(101.0 + i as f64 + dx, 80.0 + (i / 2) as f64);
                        match i {
                            5 => app.on_mouse_up(x, y, button, &input(x, y)),
                            _ => app.on_mouse_move(x, y, &input(x, y)),
                        };
                    });
                    if tool == ToolType::Move && button == MouseButton::Left && !app.float_moves {
                        // The first move vacates the cells; each later frame redraws few px.
                        assert!(
                            up[2..].iter().all(|&a| a * 2 < viewport),
                            "{mode:?} zoom {zoom}: {up:?} of {viewport}"
                        );
                    }
                }
            }
        }
    }

    /// Pan and zoom drafts (`CanvasPixels::draft`): a pan by whole device px draws only the strips
    /// it uncovers, a zoom everything; the first render without `draft` equals a full refresh.
    #[test]
    fn drafts_settle_to_a_full_refresh() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            let mut app = scene(1.7, true, cells(20.0, 20.0, 260, 160, 600));
            let mut check = Check::new(mode);
            check.frame(&mut app, 0);
            check.canvas.draft = true;
            let steps: [(f64, f64, f64); 5] = [
                (5.0, 0.0, 1.7),
                (0.0, -7.0, 1.7),
                (3.0, 2.5, 1.7),
                (0.0, 0.0, 1.3),
                (4.0, 0.0, 1.3),
            ];
            for (dx, dy, zoom) in steps {
                let ws = app.active_ws_mut().unwrap();
                let v = &mut ws.view;
                v.zoom = zoom;
                v.set_view_position(v.viewport_x + dx, v.viewport_y + dy);
                let ws = app.active_ws().unwrap();
                let t = ws.view.view_transform();
                let (style, out) = (check.style, &mut check.out);
                out.resize((t.viewport[0] * t.viewport[1]) as usize, [0; 4]);
                let up = check
                    .canvas
                    .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                assert_eq!(up, Upload::All, "{mode:?} ({dx}, {dy}) at {zoom}");
                assert!(check.canvas.drafted());
            }
            check.canvas.draft = false;
            assert!(check.frame(&mut app, 0) > 0);
            assert!(!check.canvas.drafted());
        }
    }

    /// A pan draft by whole device px shows what a full render at its transform shows: over an
    /// opaque image (the pixels shift), over a translucent one (I2: the checkerboard stays fixed to
    /// the viewport under them, frame after frame), and when the selection changed meanwhile (I4:
    /// the old outline's pixels and the new one's are drawn again), also right after a zoom
    /// draft.
    #[test]
    fn pan_drafts_show_a_full_render_at_their_transform() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for (noise, change, zoom) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, false, true),
                (true, true, true),
            ] {
                let mut app = scene(1.0, noise, cells(20.0, 20.0, 260, 160, 600));
                let mut check = Check::new(mode);
                check.frame(&mut app, 0);
                check.canvas.draft = true;
                // Zoom drafts (half resolution), back to 100 % (where a draft's Linear tap is
                // the settled view's).
                for zoom in [1.25, 1.0].into_iter().filter(|_| zoom) {
                    app.active_ws_mut().unwrap().view.zoom = zoom;
                    let ws = app.active_ws().unwrap();
                    let t = ws.view.view_transform();
                    check
                        .out
                        .resize((t.viewport[0] * t.viewport[1]) as usize, [0; 4]);
                    let (style, out) = (check.style, &mut check.out);
                    check
                        .canvas
                        .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                }
                for (i, (dx, dy)) in [(6.0, 3.0), (-11.0, 4.0), (2.0, -9.0)]
                    .into_iter()
                    .enumerate()
                {
                    let ws = app.active_ws_mut().unwrap();
                    if change {
                        let r = Rectangle::new(40.0 + i as f64, 30.0, 150.0, 100.0);
                        let path = GraphicsPath::from_rect(&r);
                        ws.selection
                            .set_continuation_path(path, CombineMode::Replace);
                        ws.selection.commit_continuation();
                    }
                    let v = &mut ws.view;
                    v.set_view_position(v.viewport_x + dx, v.viewport_y + dy);
                    for e in app.drain_events() {
                        if let AppEvent::DocumentInvalidated { rects } = e {
                            check.canvas.invalidate(&rects);
                        }
                    }
                    let ws = app.active_ws().unwrap();
                    let t = ws.view.view_transform();
                    let size = (t.viewport[0] * t.viewport[1]) as usize;
                    check.out.resize(size, [0; 4]);
                    let (style, out) = (check.style, &mut check.out);
                    check
                        .canvas
                        .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                    // A changed overlay drawn at half resolution is not listed: that frame
                    // renders exactly.
                    let drafted = check.canvas.drafted;
                    assert_eq!(drafted.is_some(), !(zoom && change && i == 0));
                    let shown = drafted.unwrap_or(t);
                    let mut fresh = vec![[0u8; 4]; size];
                    CanvasPixels::default().render(ws, &shown, &style, 0, || &mut fresh[..], |p| p);
                    let stale = check.out.iter().zip(&fresh).filter(|(a, b)| a != b).count();
                    assert_eq!(stale, 0, "{mode:?} {noise} {change} {zoom} step {i}");
                }
            }
        }
    }

    /// I3, M4: during a float drag a change of view samples the composite of the layers under
    /// the float again but keeps it (and its levels); a lower layer shown or hidden composites it
    /// again. Every frame equals a full refresh drawing the same float.
    #[test]
    fn float_view_changes_keep_the_composite_under_it() {
        use crate::document::layer::Layer;
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            let r = Rectangle::new(40.0, 30.0, 120.0, 80.0);
            let mut app = scene(0.6, true, GraphicsPath::from_rect(&r));
            app.float_moves = true;
            let ws = app.active_ws_mut().unwrap();
            ws.add_layer(Layer::create_layer(300, 200, "Top"));
            ws.active_layer = 1;
            let top = &mut ws.document.layers.get_at_mut(1).unwrap().surface;
            top.fill_rect(IntRect::new(40, 30, 120, 80), [200, 40, 40, 180]);
            ws.invalidate(None);
            app.set_active_tool_from_type(ToolType::Move);
            let t = app.active_ws().unwrap().view.view_transform();
            let at = |x: f64, y: f64| (x * t.scale + t.origin[0], y * t.scale + t.origin[1]);
            let (x, y) = at(100.0, 80.0);
            app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
            let mut check = Check::new(mode);
            for i in 1..3 {
                let (x, y) = at(100.0 + 3.0 * i as f64, 80.0 + i as f64);
                app.on_mouse_move(x, y, &input(x, y));
                app.now_ms += 16.0;
                let now = app.now_ms;
                app.on_frame(now);
                check.frame(&mut app, 0);
            }
            assert!(app.active_ws().unwrap().float.is_some());
            let under = |c: &CanvasPixels| {
                c.under
                    .as_ref()
                    .and_then(|u| u.1.as_ref())
                    .map(|s| s.data().as_ptr())
            };
            let kept = under(&check.canvas);
            assert!(kept.is_some());
            for _ in 0..2 {
                let v = &mut app.active_ws_mut().unwrap().view;
                v.set_view_position(v.viewport_x + 7.0, v.viewport_y + 3.0);
                check.frame(&mut app, 0);
                assert_eq!(under(&check.canvas), kept, "{mode:?}: composited again");
            }
            let ws = app.active_ws_mut().unwrap();
            ws.document.layers.get_at_mut(0).unwrap().properties.visible = false;
            ws.invalidate(None);
            check.frame(&mut app, 0);
            assert_eq!(
                under(&check.canvas),
                None,
                "{mode:?}: nothing shown under it"
            );
        }
    }

    /// Edits whose invalidation reaches the canvas a frame late (Task 30's last-frame bug: each
    /// frame's drain brings the last frame's rects while its own edit's are still queued), one of
    /// the whole document after a rect's, and one drained elsewhere: every frame equals a full
    /// refresh.
    #[test]
    fn edits_not_announced_yet_redraw_everything() {
        let mut app = scene(1.0, true, GraphicsPath::default());
        let mut check = Check::new(RenderMode::Hardware);
        check.frame(&mut app, 0);
        let mut late = vec![];
        let edit = |app: &mut AppWorkspace, r: IntRect, whole: bool| {
            let ws = app.active_ws_mut().unwrap();
            let layer = ws.document.layers.get_at_mut(0).unwrap();
            layer.surface.fill_rect(r, [9, 99, 199, 255]);
            let rect = Rectangle::new(r.x as f64, r.y as f64, r.w as f64, r.h as f64);
            ws.invalidate((!whole).then(|| vec![rect]));
        };
        for i in 0..4 {
            late.drain(..)
                .for_each(|rects: Vec<Rectangle>| check.canvas.invalidate(&rects));
            edit(
                &mut app,
                IntRect::new(10 + 40 * i, 10 + 20 * i, 5, 4),
                false,
            );
            let ws = app.active_ws().unwrap();
            let t = ws.view.view_transform();
            let (style, out) = (check.style, &mut check.out);
            let _ = check
                .canvas
                .render(ws, &t, &style, 0, || &mut out[..], |p| p);
            let mut fresh = vec![[0u8; 4]; check.out.len()];
            let _ = CanvasPixels::default().render(ws, &t, &style, 0, || &mut fresh[..], |p| p);
            assert!(check.out == fresh, "frame {i}: stale pixels");
            for e in app.drain_events() {
                if let AppEvent::DocumentInvalidated { rects } = e {
                    late.push(rects);
                }
            }
        }
        late.drain(..)
            .for_each(|rects| check.canvas.invalidate(&rects));
        edit(&mut app, IntRect::new(30, 30, 2, 2), false);
        edit(&mut app, IntRect::new(250, 20, 9, 9), true);
        check.frame(&mut app, 0);
        // An invalidation drained that never reached the canvas, then one that did.
        edit(&mut app, IntRect::new(60, 40, 3, 3), false);
        app.drain_events();
        edit(&mut app, IntRect::new(90, 40, 3, 3), false);
        check.frame(&mut app, 0);
    }

    /// Only a moved handle or brush outline changes: its old and new boxes are redrawn, not the
    /// selection's.
    #[test]
    fn a_moved_handle_redraws_just_its_boxes() {
        let mut app = scene(1.0, true, cells(20.0, 20.0, 260, 160, 600));
        let handle = |x: f64| Handle {
            canvas: [x, 50.0],
            shape: HandleShape::Square,
            opacity: 1.0,
            hover: 0.0,
        };
        let id = app
            .active_ws_mut()
            .unwrap()
            .surface_box
            .add_renderer(Renderer::Handles(vec![handle(60.0)]));
        let brush = |x: f64| BrushPreview {
            canvas: [x, 120.0],
            radius: 6.0,
            pressure: None,
            opacity: 0.5,
        };
        let outline = app
            .active_ws_mut()
            .unwrap()
            .surface_box
            .add_renderer(Renderer::BrushPreview(brush(60.0)));
        let up = frames(&mut app, RenderMode::Hardware, 6, |app, i| {
            let ws = app.active_ws_mut().unwrap();
            match ws.surface_box.renderer_mut([id, outline][i % 2]) {
                Some(Renderer::Handles(h)) => *h = vec![handle(90.0 + 30.0 * i as f64)],
                Some(Renderer::BrushPreview(b)) => *b = brush(90.0 + 30.0 * i as f64),
                _ => unreachable!(),
            }
        });
        assert!(up[1..].iter().all(|&a| 0 < a && a < 2_000), "{up:?}");
    }

    /// Task 31/27r integration: a picker's old pixels clear after moves, hiding, and removal.
    #[test]
    fn picker_preview_frames_match_a_full_refresh() {
        use crate::render::nubs::PickerPreview;
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for zoom in [0.3, 1.0, 1.7] {
                let mut app = scene(zoom, true, GraphicsPath::default());
                let id =
                    app.active_ws_mut()
                        .unwrap()
                        .surface_box
                        .add_renderer(Renderer::PickerPreview(PickerPreview {
                            cell: [40, 60],
                            size: 11,
                            visible: true,
                        }));
                let mut check = Check::new(mode);
                check.frame(&mut app, 0);
                for (cell, visible) in [([100, 60], true), ([100, 60], false), ([160, 100], true)] {
                    let Some(Renderer::PickerPreview(p)) =
                        app.active_ws_mut().unwrap().surface_box.renderer_mut(id)
                    else {
                        unreachable!()
                    };
                    p.cell = cell;
                    p.visible = visible;
                    assert!(check.frame(&mut app, 0) > 0);
                }
                app.active_ws_mut().unwrap().surface_box.remove_renderer(id);
                assert!(check.frame(&mut app, 0) > 0);
            }
        }
    }

    /// The 4K drag's case, small: a complex selection's (a 5000-gon ring's) masks are held
    /// while it moves over a white image (drawn shifted), then rebuilt on the workers; every frame
    /// equals a full refresh.
    #[test]
    fn held_masks_frames_match_a_full_refresh() {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let a = |i: usize| i as f64 * std::f64::consts::TAU / 5000.0;
        let ring =
            (0..5000).map(|i| Point::new(150.0 + 120.0 * a(i).cos(), 100.0 + 80.0 * a(i).sin()));
        let mut path = GraphicsPath::default();
        path.polygons.push(ring.collect());
        let mut app = scene(1.0, false, path);
        app.set_active_tool_from_type(ToolType::Move);
        let mut check = Check::new(RenderMode::Hardware);
        // Frames until a build lands (the stamp moves past `stamp`), each checked.
        let until_landed = |app: &mut AppWorkspace, check: &mut Check, stamp: u64| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let now = mask_builds(wake.clone());
                check.frame(app, now);
                if now != stamp {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the masks never landed"
                );
            }
        };
        let stamp = mask_builds(wake.clone());
        check.frame(&mut app, stamp);
        until_landed(&mut app, &mut check, stamp);
        let stamp = mask_builds(wake.clone());
        let t = app.active_ws().unwrap().view.view_transform();
        let at = |x: f64, y: f64| (x + t.origin[0], y + t.origin[1]);
        // Off the rotation anchor's nub at the centre.
        let (x, y) = at(100.0, 100.0);
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        let mut up = vec![];
        for i in 0..6 {
            app.now_ms += 16.0;
            let (x, y) = at(101.0 + i as f64, 100.0);
            match i {
                5 => app.on_mouse_up(x, y, MouseButton::Left, &input(x, y)),
                _ => app.on_mouse_move(x, y, &input(x, y)),
            };
            let now = app.now_ms;
            app.on_frame(now);
            up.push(check.frame(&mut app, mask_builds(wake.clone())));
        }
        assert!(
            up[2..].iter().all(|&a| a > 0 && a * 2 < 320 * 240),
            "{up:?}"
        );
        // The rebuild after the move lands.
        until_landed(&mut app, &mut check, stamp);
    }
}
