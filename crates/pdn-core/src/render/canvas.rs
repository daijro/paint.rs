//! The canvas view's pixels between frames: the base (view, grid) kept in a `DisplayTarget`, the
//! overlays painted over a copy of its rows on the way to the caller's upload pixels. A frame
//! redraws only what changed: the device pixels of the document rects invalidated since the last
//! one, and the old and new pixels of each overlay that changed, as one-row spans while they are
//! few (Task 27r). Every frame is byte-identical to a full refresh.
//! As in Paint.NET, invalidated rects are queued, unioned and clipped; only what they cover
//! re-renders.

use super::display::{DisplayTarget, SPAN_GAP, SpanMask, span_bands};
use super::float::{Float, FloatPainter, Levels};
use super::selection::{self, TintParts};
use super::view::{self, CanvasStyle, MipChain, ViewTransform};
use super::{Renderer, RendererId};
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
    /// Nothing in the caller's pixels: a zoom draft, at a reduced resolution (`half_shown`).
    Half,
    /// A pan (a draft, or exact: see `render`): every pixel moved by `d` (device px; the
    /// uncovered ones keep what they held), then `rects` were drawn again. A display that can
    /// move its copy by `d` uploads only them.
    Panned {
        d: [i32; 2],
        rects: Vec<IntRect>,
    },
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
    /// (`SurfaceBox::overlay_keys`), paint rects and, for a tinting selection, its outline and tint
    /// runs apart.
    view: Option<(u64, ViewTransform)>,
    layers: Vec<Layer>,
    /// Set by the caller while a pan or zoom is in progress: a change of view alone is drawn as a
    /// draft (see `render_draft`) until a render without it settles the view exactly.
    pub draft: bool,
    /// The most pixels a zoom draft draws: it renders at 1/k of the view's resolution, k the
    /// least of 2, 3 and 4 that keeps it under this (the web renders on one thread). `None`: half
    /// resolution.
    pub draft_pixels: Option<usize>,
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
    /// A zoom draft's reduced view; a pan draft's planes over translucent pixels (see
    /// `draft_translucent`) at their transform. Both go when a render settles the view.
    half: Option<DisplayTarget>,
    planes: Option<(ViewTransform, [Vec<[u8; 4]>; 2])>,
    /// The zoom draft shown, at 1/k resolution (its size, k, its pixels): the upload pixels hold
    /// an older frame until a pan draft spreads it over them or the view settles.
    half_shown: Option<([u32; 2], u32, Vec<[u8; 4]>)>,
    /// Whether the composite of (document, revision) has no translucent pixel.
    opaque: Option<((u64, u64), bool)>,
    /// An exact pan moved the upload pixels but not the target's (it drew only the strips it
    /// uncovered): until the next full render, a write draws the base where it reads the target.
    stale: bool,
}

/// A zoom draft's size, k, and pixels (`CanvasPixels::half_shown`).
pub type HalfShown<'a> = ([u32; 2], u32, &'a [[u8; 4]]);

type Layer = (
    RendererId,
    [u64; 3],
    Option<Vec<IntRect>>,
    Option<TintParts>,
);

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
    /// The transform a draft shows (up to ½ px off the view's), while one shows.
    pub fn draft_view(&self) -> Option<ViewTransform> {
        self.drafted
    }

    /// The upload pixels show a draft (`draft`), not the settled view.
    pub fn drafted(&self) -> bool {
        self.drafted.is_some()
    }

    /// After `Upload::Half` (until another upload): the zoom draft shown, at 1/k resolution (its
    /// size, k, and its pixels row-major), each pixel to be shown as a k × k block.
    pub fn half_shown(&self) -> Option<HalfShown<'_>> {
        self.half_shown
            .as_ref()
            .map(|(size, k, px)| (*size, *k, &px[..]))
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
        // A pan by whole device px over an opaque image, from an exact view: the pixels move and
        // only the strips it uncovers are drawn, exactly (no draft, no settle).
        let exact = self.drafted.is_none()
            && self.view_only(ws, &key)
            && self.pan_of(&key).is_some_and(|(t, _)| t == key.t)
            && self.composite_opaque(ws);
        if (exact || (self.draft && self.view_only(ws, &key)))
            && let Some(redo) = self.draft_overlays(ws, &key, stamp, exact)
        {
            return self.render_draft(ws, &key, &redo, exact, out(), conv);
        }
        if self.drafted.is_some() {
            // The view settles: the drafts' buffers go.
            (self.half, self.planes, self.half_shown) = (None, None, None);
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
            self.fresh_base(ws, t, style, all, Some(&spans));
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
        match &spans {
            Some(spans) => self.fresh_base(ws, t, style, all, Some(spans)),
            None => self.fresh_base(ws, t, style, refresh, None),
        }
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

    /// Draws the base into the target over `rect` (∩ `spans`) before a write reads it there, while
    /// the target lags the upload pixels (`stale`); all of it, for good, past half of it.
    fn fresh_base(
        &mut self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        style: &CanvasStyle,
        rect: IntRect,
        spans: Option<&[IntRect]>,
    ) {
        let Some(target) = self.target.as_mut().filter(|_| self.stale) else {
            return;
        };
        let whole = IntRect::new(0, 0, target.width, target.height);
        let n = spans.map_or(area(rect), |s| {
            s.iter().map(|r| area(r.intersect(&rect))).sum()
        });
        if n == 0 {
            return;
        }
        let (rect, spans) = match n * 2 >= area(whole) {
            true => {
                self.stale = false;
                (whole, None)
            }
            false => (rect, spans),
        };
        target.set_clip(Some(rect));
        ws.surface_box.render_base_in(ws, target, t, style, spans);
        target.set_clip(None);
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
        exact: bool,
    ) -> Option<Vec<IntRect>> {
        let keys = ws.surface_box.overlay_keys(ws, stamp);
        let Some((t, _)) = self.pan_of(key) else {
            // Drawn at half resolution: their pixels are not listed.
            self.layers = keys
                .into_iter()
                .map(|(id, k)| (id, k, None, None))
                .collect();
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
        let moved_parts = |p: &Option<TintParts>| {
            let p = p.as_ref()?;
            Some(TintParts {
                outline: moved(&Some(p.outline.clone()))?,
                tint: moved(&Some(p.tint.clone()))?,
                ..*p
            })
        };
        let mut old = std::mem::take(&mut self.layers);
        let (mut layers, mut redo) = (vec![], vec![]);
        for (id, k) in keys {
            let was = old
                .iter()
                .position(|l| l.0 == id)
                .map(|i| old.swap_remove(i));
            let (rects, parts) = match was {
                // Listed again at `t`: the old lists stop at the old view's edge, and miss what
                // the strips the pan uncovered draw now (review I-1).
                Some((_, wk, rects, parts)) if wk == k => {
                    let parts_now = ws.surface_box.overlay_tint_parts(ws, &t, id);
                    let now = match &parts_now {
                        Some(p) => Some([&p.outline[..], &p.tint[..]].concat()),
                        None => ws.surface_box.overlay_paint_rects(ws, &t, id),
                    };
                    // An exact pan draws a selection's outline again, old and new: where the view
                    // clipped it, its first corner is a cap, not the join it is once in view (an
                    // outline too dense to list is left as it moved).
                    if exact && matches!(ws.surface_box.renderer(id), Some(Renderer::Selection(_)))
                    {
                        let outline = |p: Option<TintParts>, r: Option<Vec<IntRect>>| {
                            p.map(|p| p.outline).or(r).unwrap_or_default()
                        };
                        redo.extend(outline(moved_parts(&parts), moved(&rects)));
                        redo.extend(outline(parts_now.clone(), now.clone()));
                    }
                    (now, parts_now)
                }
                was => {
                    // As `overlay_changes`: a tinting selection's tint where both frames tint
                    // shows the same pixels (the base under it moved with them).
                    let parts = ws.surface_box.overlay_tint_parts(ws, &t, id);
                    let now = match &parts {
                        Some(p) => Some([&p.outline[..], &p.tint[..]].concat()),
                        None => ws.surface_box.overlay_paint_rects(ws, &t, id),
                    };
                    match (was.as_ref().and_then(|w| moved_parts(&w.3)), &parts) {
                        (Some(old), Some(new)) if old.highlight == new.highlight => {
                            redo.extend(old.outline);
                            redo.extend_from_slice(&new.outline);
                            redo.extend(selection::xor_runs(&old.tint, &new.tint));
                        }
                        _ => {
                            redo.extend(was.map_or(Some(vec![]), |w| moved(&w.2))?);
                            redo.extend(now.clone()?);
                        }
                    }
                    (now, parts)
                }
            };
            layers.push((id, k, rects, parts));
        }
        for (_, _, gone, _) in &old {
            redo.extend(moved(gone)?);
        }
        self.layers = layers;
        self.view = Some((ws.document.id, t));
        Some(redo)
    }

    /// A pan or zoom frame, drawn cheaply; the next render without `draft` draws the view exactly.
    /// `exact` (a whole-pixel pan over an opaque image from an exact view): the strips are drawn
    /// in the view's own style, so the frame is the view at its transform and needs no settling.
    /// A pan by whole device px (rounded) moves the pixels shown and draws only the strips it
    /// uncovers, at the shown transform moved by that much (≤ ½ px off until the view settles),
    /// plus `redo` (the overlays that changed); any other change of view draws everything at half
    /// resolution (or less, see `draft_pixels`) with one Linear tap (`render_half`). The overlays
    /// are drawn with the pixels they lie on; the ants hold still meanwhile.
    fn render_draft<T: Copy + Send>(
        &mut self,
        ws: &DocumentWorkspace,
        key: &BaseKey,
        redo: &[IntRect],
        exact: bool,
        out: &mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
    ) -> Upload {
        let shown = self.shown();
        let [w, h] = key.t.viewport.map(|v| v.max(1));
        let style = CanvasStyle {
            draft: !exact,
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
            self.render_half(ws, &key.t, &style);
            self.drafted = Some(key.t);
            return Upload::Half;
        };
        let all = IntRect::new(0, 0, w, h);
        // The overlays' rects (a tinted selection's outline and tint runs: thousands of one-row
        // rects) as spans, drawn in one pass and uploaded in row bands (a band of more than four
        // rects as one); past half of the view, the whole view, strips included.
        let spans = match redo.is_empty() {
            true => Some(vec![]),
            false => sparse(redo, all, area(all)),
        };
        let (drawn, bands) = match &spans {
            Some(spans) => {
                let bands = span_bands(spans, 32);
                let bands = (bands.chunk_by(|a, b| a.y == b.y)).flat_map(|band| match band.len() {
                    ..=4 => band.to_vec(),
                    _ => vec![band.iter().fold(band[0], |u, r| u.union(r))],
                });
                (strips, bands.collect())
            }
            None => (vec![all], vec![]),
        };
        let drawn: Vec<IntRect> = drawn.into_iter().filter(|r| !r.is_empty()).collect();
        // What goes up (and what the translucent planes draw again).
        let regions: Vec<IntRect> = drawn.iter().copied().chain(bands).collect();
        // The checkerboard is fixed to the viewport: where the image is translucent it does not
        // move with the pixels.
        if !self.composite_opaque(ws) {
            // (It writes all of the upload pixels.)
            self.half_shown = None;
            self.draft_translucent(ws, shown, t, d, &regions, &style, out, conv);
            self.drafted = Some(t);
            return Upload::All;
        }
        // A zoom draft shown at a reduced resolution first fills the upload pixels it left as
        // they were.
        if let Some(([hw, _], k, small)) = self.half_shown.take() {
            let (w, hw, k) = (w as usize, hw as usize, k as usize);
            super::display::for_bands(out, w, 0..h as usize, 32, |y0, band| {
                for (j, row) in band.chunks_mut(w).enumerate() {
                    let src = &small[(y0 + j) / k * hw..][..hw];
                    for (x, o) in row.iter_mut().enumerate() {
                        *o = conv(src[x / k]);
                    }
                }
            });
        }
        shift(out, w as usize, h as usize, d[0] as i32, d[1] as i32);
        let target = match self.target.take() {
            Some(target) if (target.width, target.height) == (w, h) => target,
            _ => DisplayTarget::new(w, h, style.mode),
        };
        let target = self.target.insert(target);
        let paint = ws.surface_box.overlay_painter(ws, &t, style.mode);
        // The overlays' changes in one pass (in parallel row bands natively).
        if let Some(spans) = spans.filter(|s| !s.is_empty()) {
            let bounds = spans.iter().fold(spans[0], |u, r| u.union(r));
            target.set_clip(Some(bounds));
            ws.surface_box
                .render_base_in(ws, target, &t, &style, Some(&spans));
            target.set_clip(None);
            target.write_rgba8_over(all, Some(&spans), out, &conv, &paint);
        }
        for &r in &drawn {
            target.set_clip(Some(r));
            ws.surface_box.render_base_in(ws, target, &t, &style, None);
            target.set_clip(None);
            target.write_rgba8_over(r, None, out, &conv, &paint);
        }
        if exact {
            // What a full render at `t` shows (the ants hold their frame until the next render).
            // The overlays drew into the old rect moved, and into the strips now.
            let [dx, dy] = d.map(|v| v as i32);
            let moved = self
                .overlay_rect
                .map(|r| IntRect::new(r.x + dx, r.y + dy, r.w, r.h));
            let now = ws.surface_box.overlay_bounds(ws, &t);
            self.overlay_rect = [moved, now]
                .into_iter()
                .flatten()
                .reduce(|a, b| a.union(&b))
                .map(|r| r.intersect(&all));
            (self.key, self.stale) = (Some(key.clone()), true);
        } else {
            self.drafted = Some(t);
        }
        Upload::Panned {
            d: [d[0] as i32, d[1] as i32],
            rects: regions,
        }
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
            // the squares (unless an exact pan left it behind).
            _ if self.drafted.is_none() && !self.stale => {
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

    /// A zoom's draft: the view at 1/k resolution (1/k the scale, origin and UI scale; k from
    /// `draft_pixels`, else 2), each pixel to be shown as a k × k block (`half_shown`; the
    /// caller's display scales it, so a zoom uploads 1/k² of the pixels and spreads none of them).
    fn render_half(&mut self, ws: &DocumentWorkspace, t: &ViewTransform, style: &CanvasStyle) {
        let [w, h] = t.viewport.map(|v| v.max(1));
        let size = |k: u32| (w.div_ceil(k), h.div_ceil(k));
        let k = match self.draft_pixels {
            Some(most) => (2..4)
                .find(|&k| (size(k).0 as usize) * (size(k).1 as usize) <= most)
                .unwrap_or(4),
            None => 2,
        };
        let (hw, hh) = size(k);
        let half = ViewTransform {
            scale: t.scale / k as f64,
            origin: t.origin.map(|o| o / k as f64),
            viewport: [hw, hh],
            ui_scale: t.ui_scale / k as f64,
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
        let mut small = self
            .half_shown
            .take()
            .map(|(_, _, px)| px)
            .unwrap_or_default();
        small.resize(hw as usize * hh as usize, [0; 4]);
        let paint = ws.surface_box.overlay_painter(ws, &half, style.mode);
        target.write_rgba8_over(IntRect::new(0, 0, hw, hh), None, &mut small, |p| p, paint);
        self.half_shown = Some(([hw, hh], k, small));
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
        // A full render draws the target again (a partial one only where it changed).
        self.stale &= partial.is_some();
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
            let (rects, parts) = match was {
                Some((_, k, rects, parts)) if k == key => (rects, parts),
                was => {
                    let parts = ws.surface_box.overlay_tint_parts(ws, t, id);
                    let now = match &parts {
                        Some(p) => Some([&p.outline[..], &p.tint[..]].concat()),
                        None => ws.surface_box.overlay_paint_rects(ws, t, id),
                    };
                    match (was, &parts) {
                        // The same tint where both frames tint shows the same pixels: only the
                        // outlines and the runs either frame alone tints change.
                        (Some((_, _, _, Some(old))), Some(new))
                            if old.highlight == new.highlight =>
                        {
                            add(&Some(old.outline));
                            add(&Some(new.outline.clone()));
                            add(&Some(selection::xor_runs(&old.tint, &new.tint)));
                        }
                        (was, _) => {
                            add(&was.map_or(Some(vec![]), |w| w.2));
                            add(&now);
                        }
                    }
                    (now, parts)
                }
            };
            self.layers.push((id, key, rects, parts));
        }
        for (_, _, gone, _) in &old {
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
                Upload::Half | Upload::Panned { .. } => unreachable!("drafts are not checked"),
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

    /// A Rectangle Select drag tints its selection: every frame equals a full refresh, and once
    /// the rectangle is drawn a frame redraws only the outlines and the runs one frame alone tints,
    /// growing, shrinking or crossing over, not the rectangles' bounds.
    #[test]
    fn tinted_selection_drags_redraw_only_what_changed() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            for zoom in [1.0, 1.7] {
                let mut app = scene(zoom, true, GraphicsPath::default());
                let ws = app.active_ws().unwrap();
                let t = ws.view.view_transform();
                let at = |x: f64, y: f64| (x * t.scale + t.origin[0], y * t.scale + t.origin[1]);
                app.set_active_tool_from_type(ToolType::RectangleSelect);
                let (x, y) = at(30.0, 20.0);
                app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
                let corners = [
                    (200.0, 150.0),
                    (204.0, 152.0),
                    (180.0, 151.0),
                    (20.0, 10.0),
                    (60.0, 40.0),
                ];
                let mut boxes = vec![];
                let up = frames(&mut app, mode, corners.len(), |app, i| {
                    let (x, y) = at(corners[i].0, corners[i].1);
                    app.on_mouse_move(x, y, &input(x, y));
                    let ws = app.active_ws().unwrap();
                    assert!(
                        ws.surface_box
                            .renderers
                            .iter()
                            .any(|(_, r)| matches!(r, Renderer::Selection(s) if s.tinting))
                    );
                    boxes.push(ws.surface_box.overlay_bounds(ws, &ws.view.view_transform()));
                });
                // From the second corner on, the bounds of both rectangles would be over 100 000
                // device px at 100 %.
                for (i, (&px, b)) in up[2..4].iter().zip(&boxes[1..3]).enumerate() {
                    let b = b.expect("drawn");
                    assert!(
                        px * 4 < area(b),
                        "{mode:?} zoom {zoom} frame {i}: {px} of {b:?}"
                    );
                }
            }
        }
    }

    /// A Move Selection drag while the view scrolls (the wheel, auto-scroll): each pan draft
    /// redraws the outlines and the runs one frame alone tints, in row bands (a tinted selection's
    /// outline and tint runs were thousands of one-row regions, each drawn and uploaded on its
    /// own), and shows what a full render at its transform shows.
    #[test]
    fn pan_drafts_redraw_a_moving_tint_in_bands() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            let mut app = AppWorkspace {
                view_width: 320.0,
                view_height: 240.0,
                ..Default::default()
            };
            app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
            let ws = app.active_ws_mut().unwrap();
            (ws.view.ppp, ws.view.zoom_to_window, ws.view.zoom) = (1.0, false, 1.0);
            ws.view.center_view();
            // Over two thirds of the view, its top left corner inside.
            let r = Rectangle::new(60.0, 40.0, 700.0, 500.0);
            ws.selection
                .set_continuation_path(GraphicsPath::from_rect(&r), CombineMode::Replace);
            ws.selection.commit_continuation();
            let mut check = Check::new(mode);
            check.frame(&mut app, 0);
            check.canvas.draft = true;
            app.set_active_tool_from_type(ToolType::MoveSelection);
            let at = |app: &AppWorkspace, x: f64, y: f64| {
                let t = app.active_ws().unwrap().view.view_transform();
                (x * t.scale + t.origin[0], y * t.scale + t.origin[1])
            };
            let (x, y) = at(&app, 200.0, 150.0);
            app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
            for i in 0..4 {
                // Far enough that the old and new outlines' bands don't merge over what changed.
                let (x, y) = at(&app, 240.0 + 40.0 * i as f64, 152.0 + i as f64);
                app.on_mouse_move(x, y, &input(x, y));
                let v = &mut app.active_ws_mut().unwrap().view;
                v.set_view_position(v.viewport_x + 6.0, v.viewport_y + 4.0);
                app.now_ms += 16.0;
                let now = app.now_ms;
                app.on_frame(now);
                for e in app.drain_events() {
                    if let AppEvent::DocumentInvalidated { rects } = e {
                        check.canvas.invalidate(&rects);
                    }
                }
                let ws = app.active_ws().unwrap();
                assert!(ws.selection_renderer().tinting, "Move Selection tints");
                let t = ws.view.view_transform();
                let (style, out) = (check.style, &mut check.out);
                let up = check
                    .canvas
                    .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                let Upload::Panned { rects, .. } = up else {
                    panic!("{mode:?} frame {i}: {up:?}");
                };
                // Two strips, then at most a few rects per 32-row band; once both frames tint (the
                // press starts it), under half of the view.
                let bands = t.viewport[1].div_ceil(32) as usize;
                assert!(
                    rects.len() <= 2 + 3 * bands,
                    "{mode:?} {i}: {}",
                    rects.len()
                );
                let drawn: u64 = rects.iter().map(|r| area(*r)).sum();
                let view = (t.viewport[0] * t.viewport[1]) as u64;
                assert!(
                    i == 0 || drawn * 2 < view,
                    "{mode:?} {i}: {drawn} of {view} px"
                );
                // (Whole-pixel pans over an opaque image are exact: no draft.)
                let shown = check.canvas.drafted.unwrap_or(t);
                let mut fresh = vec![[0u8; 4]; check.out.len()];
                CanvasPixels::default().render(ws, &shown, &style, 0, || &mut fresh[..], |p| p);
                let stale = check.out.iter().zip(&fresh).filter(|(a, b)| a != b).count();
                assert_eq!(stale, 0, "{mode:?} frame {i}");
            }
        }
    }

    /// The same drag in every frame's pixels: a selection with all four edges in view moving far
    /// enough that its bands don't merge over what changed (the old outline, inside the new tint,
    /// must be drawn again), then a jump that redraws the whole view instead; over a patterned
    /// opaque image at 100 % and 170 % and over a translucent one (the draft's planes); and a grid
    /// of many small parts (a band of more than four rects goes up as one). Every frame shows what
    /// a full render at its transform shows, texel by texel in the view's style or (where the
    /// draft drew them) a draft's.
    #[test]
    fn pan_drafts_redraw_tints_exactly() {
        for (mode, zoom, translucent, grid) in [
            (RenderMode::Hardware, 1.0, false, false),
            (RenderMode::Software, 1.0, false, false),
            (RenderMode::Hardware, 1.7, false, false),
            (RenderMode::Hardware, 1.0, true, false),
            (RenderMode::Hardware, 1.0, false, true),
        ] {
            let mut app = AppWorkspace {
                view_width: 640.0,
                view_height: 480.0,
                ..Default::default()
            };
            app.create_blank_document_in_new_workspace(1200, 900, 96.0, true);
            let ws = app.active_ws_mut().unwrap();
            let px = ws.document.layers.get_at_mut(0).unwrap().surface.data_mut();
            for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = (i % 1200, i / 1200);
                let a = if translucent { 60 + (x + y) % 150 } else { 255 };
                *p = [(x * 7) as u8, (y * 5) as u8, (x ^ y) as u8, a as u8];
            }
            ws.invalidate(None);
            (ws.view.ppp, ws.view.zoom_to_window, ws.view.zoom) = (1.0, false, zoom);
            ws.view.center_view();
            let t = ws.view.view_transform();
            // Image px under device px (x, y) now.
            let img = |x: f64, y: f64| ((x - t.origin[0]) / t.scale, (y - t.origin[1]) / t.scale);
            let (x0, y0) = img(60.0, 50.0);
            let path = match grid {
                // 5 × 5 cells of 20 × 12 px, 110 × 80 px apart.
                true => GraphicsPath {
                    polygons: (0..25)
                        .map(|k| {
                            let (x, y) = (x0 + 110.0 * (k % 5) as f64, y0 + 80.0 * (k / 5) as f64);
                            let c = [(x, y), (x + 20.0, y), (x + 20.0, y + 12.0), (x, y + 12.0)];
                            c.map(|(x, y)| Point::new(x, y)).to_vec()
                        })
                        .collect(),
                },
                false => {
                    GraphicsPath::from_rect(&Rectangle::new(x0, y0, 500.0 / zoom, 360.0 / zoom))
                }
            };
            ws.selection
                .set_continuation_path(path, CombineMode::Replace);
            ws.selection.commit_continuation();
            let mut check = Check::new(mode);
            check.frame(&mut app, 0);
            check.canvas.draft = true;
            app.set_active_tool_from_type(ToolType::MoveSelection);
            let at = |app: &AppWorkspace, x: f64, y: f64| {
                let t = app.active_ws().unwrap().view.view_transform();
                (x * t.scale + t.origin[0], y * t.scale + t.origin[1])
            };
            let (px, py) = img(200.0, 150.0);
            let (x, y) = at(&app, px, py);
            app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
            let mut whole = 0;
            for (i, dx) in [40.0, 80.0, 120.0, 520.0].into_iter().enumerate() {
                let (x, y) = at(&app, px + dx / zoom, py + (1 + i) as f64 / zoom);
                app.on_mouse_move(x, y, &input(x, y));
                let v = &mut app.active_ws_mut().unwrap().view;
                v.set_view_position(v.viewport_x + 6.0, v.viewport_y + 4.0);
                app.now_ms += 16.0;
                let now = app.now_ms;
                app.on_frame(now);
                for e in app.drain_events() {
                    if let AppEvent::DocumentInvalidated { rects } = e {
                        check.canvas.invalidate(&rects);
                    }
                }
                let ws = app.active_ws().unwrap();
                let t = ws.view.view_transform();
                let (style, out) = (check.style, &mut check.out);
                let up = check
                    .canvas
                    .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                let bands = t.viewport[1].div_ceil(32) as usize;
                match up {
                    Upload::Panned { rects, .. } => {
                        // The strips, at most four rects a 32-row band, or the whole view alone.
                        let all = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
                        whole += rects.contains(&all) as usize;
                        assert!(rects.len() <= 2 + 4 * bands, "{i}: {}", rects.len());
                        let banded = rects.get(2..).unwrap_or_default();
                        for band in banded.chunk_by(|a, b| a.y == b.y) {
                            assert!(band.len() <= 4, "{i}: {band:?}");
                        }
                        assert!(!rects.contains(&all) || rects.len() == 1, "{rects:?}");
                    }
                    Upload::All => assert!(translucent, "{up:?}"),
                    up => panic!("{mode:?} {zoom} frame {i}: {up:?}"),
                }
                let shown = check.canvas.drafted.unwrap_or(t);
                let fresh = |style: CanvasStyle| {
                    let mut px = vec![[0u8; 4]; check.out.len()];
                    CanvasPixels::default().render(ws, &shown, &style, 0, || &mut px[..], |p| p);
                    px
                };
                let draft = CanvasStyle {
                    draft: true,
                    ..style
                };
                let (exact, drafted) = (fresh(style), fresh(draft));
                let stale = (0..exact.len())
                    .filter(|&k| check.out[k] != exact[k] && check.out[k] != drafted[k])
                    .count();
                assert_eq!(stale, 0, "{mode:?} {zoom} {translucent} frame {i}");
            }
            assert!(
                translucent || grid || whole > 0,
                "{mode:?} {zoom}: never the whole view"
            );
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
                let zooms = v.zoom != zoom;
                v.zoom = zoom;
                v.set_view_position(v.viewport_x + dx, v.viewport_y + dy);
                let ws = app.active_ws().unwrap();
                let t = ws.view.view_transform();
                let (style, out) = (check.style, &mut check.out);
                out.resize((t.viewport[0] * t.viewport[1]) as usize, [0; 4]);
                let up = check
                    .canvas
                    .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                // A zoom draft leaves the upload pixels: it shows at half resolution. (This image
                // is translucent: its pans draw all of them.)
                let want = if zooms { Upload::Half } else { Upload::All };
                assert_eq!(up, want, "{mode:?} ({dx}, {dy}) at {zoom}");
                assert_eq!(check.canvas.half_shown().is_some(), zooms);
                assert!(check.canvas.drafted());
            }
            check.canvas.draft = false;
            assert!(check.frame(&mut app, 0) > 0);
            assert!(!check.canvas.drafted());
        }
    }

    /// Under a pixel budget a zoom draft renders at 1/k resolution, k the least of 2, 3 and 4 that
    /// fits it (else 4); a pan right after it spreads each of its pixels over k × k upload pixels
    /// and moves them; the view settles to a full refresh.
    #[test]
    fn budgeted_zoom_drafts_spread_their_pixels() {
        for (budget, want) in [(None, 2), (Some(9000), 3), (Some(1), 4)] {
            // An opaque image: the outlines of a sparse selection make the pixels differ.
            let mut app = scene(1.7, false, cells(20.0, 20.0, 260, 160, 600));
            let mut check = Check::new(RenderMode::Hardware);
            check.frame(&mut app, 0);
            (check.canvas.draft, check.canvas.draft_pixels) = (true, budget);
            let render = |app: &mut AppWorkspace, check: &mut Check| {
                let ws = app.active_ws().unwrap();
                let t = ws.view.view_transform();
                let (style, out) = (check.style, &mut check.out);
                let up = check
                    .canvas
                    .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                (t, up)
            };
            app.active_ws_mut().unwrap().view.zoom = 1.3;
            let (t, up) = render(&mut app, &mut check);
            assert_eq!(up, Upload::Half);
            let (size, k, small) = check.canvas.half_shown().expect("a zoom draft");
            let [w, h] = t.viewport;
            assert_eq!(
                (k, size),
                (want, [w.div_ceil(k), h.div_ceil(k)]),
                "{budget:?}"
            );
            let (small, hw, k) = (small.to_vec(), size[0] as usize, k as usize);
            assert!(small.iter().any(|p| *p != small[0]));
            let v = &mut app.active_ws_mut().unwrap().view;
            v.set_view_position(v.viewport_x - 5.0, v.viewport_y - 3.0);
            let Upload::Panned { d, rects } = render(&mut app, &mut check).1 else {
                panic!("a pan draft");
            };
            assert!(check.canvas.half_shown().is_none());
            let (w, h) = (w as i32, h as i32);
            for (y, x) in (0..h).flat_map(|y| (0..w).map(move |x| (y, x))) {
                let (sx, sy) = (x - d[0], y - d[1]);
                if rects.iter().any(|r| r.contains(x, y)) || sx < 0 || sy < 0 || sx >= w || sy >= h
                {
                    continue;
                }
                let want = small[sy as usize / k * hw + sx as usize / k];
                assert_eq!(check.out[(y * w + x) as usize], want, "{k}: ({x}, {y})");
            }
            check.canvas.draft = false;
            assert!(check.frame(&mut app, 0) > 0);
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
                    let before = check.out.clone();
                    let half_before = check.canvas.half_shown().is_some();
                    let (style, out) = (check.style, &mut check.out);
                    let up = check
                        .canvas
                        .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                    // Over an opaque image the pixels move: a display that moves its copy by
                    // `d` needs only `rects` again (after a zoom draft its copy is not the pixels).
                    match up {
                        Upload::Panned { d, rects } if !half_before => {
                            let [w, h] = t.viewport.map(|v| v as usize);
                            let mut moved = before;
                            shift(&mut moved, w, h, d[0], d[1]);
                            let redrawn = |i: usize| {
                                let (x, y) = ((i % w) as i32, (i / w) as i32);
                                rects.iter().any(|r| r.contains(x, y))
                            };
                            let stale = (0..w * h)
                                .filter(|&i| !redrawn(i) && moved[i] != check.out[i])
                                .count();
                            assert_eq!(stale, 0, "{mode:?} {noise} {change} {zoom} step {i}");
                        }
                        Upload::Panned { .. } => {}
                        up => assert!(noise || (zoom && change && i == 0), "{up:?}"),
                    }
                    // Over an opaque image from an exact view a pan is exact, no draft. A changed
                    // overlay drawn at half resolution is not listed: that frame renders exactly.
                    let drafted = check.canvas.drafted;
                    let exact = !noise && !zoom;
                    assert_eq!(drafted.is_some(), !exact && !(zoom && change && i == 0));
                    let shown = drafted.unwrap_or(t);
                    let mut fresh = vec![[0u8; 4]; size];
                    CanvasPixels::default().render(ws, &shown, &style, 0, || &mut fresh[..], |p| p);
                    let stale = check.out.iter().zip(&fresh).filter(|(a, b)| a != b).count();
                    assert_eq!(stale, 0, "{mode:?} {noise} {change} {zoom} step {i}");
                }
            }
        }
    }

    /// A pan by whole device px over an opaque image, in a gesture or not, moves the pixels and
    /// draws only the strips it uncovers, exactly: no draft and no settle. The target lags the
    /// pixels meanwhile; the frames after (an ants step, a selection change, a brush dab, another
    /// pan, a zoom) each equal a full refresh.
    #[test]
    fn exact_pans_need_no_settle() {
        for mode in [RenderMode::Hardware, RenderMode::Software] {
            // Zoomed out the settled view samples more finely than a draft. After the pans the
            // selection is deselected, or replaced by another.
            for (zoom, draft, deselect, ppp) in [
                (1.0, false, false, 1.0),
                (1.0, true, true, 1.0),
                (0.6, true, false, 1.0),
                (0.6, false, true, 1.0),
                (1.0, false, false, 1.25),
                (0.6, true, true, 1.25),
            ] {
                // An opaque image with a pattern (a stale pixel shows), larger than the view.
                let mut app = AppWorkspace {
                    view_width: 320.0,
                    view_height: 240.0,
                    ..Default::default()
                };
                app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
                let ws = app.active_ws_mut().unwrap();
                let px = ws.document.layers.get_at_mut(0).unwrap().surface.data_mut();
                for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let (x, y) = (i % 800, i / 800);
                    *p = [(x * 7) as u8, (y * 5) as u8, (x ^ y) as u8, 255];
                }
                ws.invalidate(None);
                (ws.view.ppp, ws.view.zoom_to_window) = (ppp, false);
                ws.view.zoom = zoom;
                ws.view.center_view();
                // Two rects the view clips: the strips the pans uncover draw more of their outlines,
                // and the pans bring the second one wholly into view (its first corner becomes a
                // join, review M-1).
                let path = GraphicsPath {
                    polygons: [(300.0, 250.0, 400.0, 80.0), (520.0, 400.0, 30.0, 30.0)]
                        .map(|(x, y, w, h)| GraphicsPath::from_rect(&Rectangle::new(x, y, w, h)))
                        .into_iter()
                        .flat_map(|p| p.polygons)
                        .collect(),
                };
                ws.selection
                    .set_continuation_path(path, CombineMode::Replace);
                ws.selection.commit_continuation();
                let mut check = Check::new(mode);
                check.frame(&mut app, 0);
                check.canvas.draft = draft;
                // (Whole device px at 1.25 too.)
                let pan = |app: &mut AppWorkspace, check: &mut Check, dx: f64, dy: f64| {
                    let v = &mut app.active_ws_mut().unwrap().view;
                    v.set_view_position(v.viewport_x + dx, v.viewport_y + dy);
                    let ws = app.active_ws().unwrap();
                    let t = ws.view.view_transform();
                    let size = (t.viewport[0] * t.viewport[1]) as usize;
                    let (style, out) = (check.style, &mut check.out);
                    let up = check
                        .canvas
                        .render(ws, &t, &style, 0, || &mut out[..], |p| p);
                    let Upload::Panned { rects, .. } = up else {
                        panic!("{mode:?} {zoom}: {up:?}");
                    };
                    let drawn: u64 = rects.iter().map(|r| area(*r)).sum();
                    assert!(drawn * 2 < size as u64, "{drawn} of {size}");
                    assert!(!check.canvas.drafted());
                    let mut fresh = vec![[0u8; 4]; size];
                    CanvasPixels::default().render(ws, &t, &style, 0, || &mut fresh[..], |p| p);
                    assert!(check.out == fresh, "{mode:?} {zoom} ({dx}, {dy})");
                };
                for (dx, dy) in [(32.0, 20.0), (24.0, 28.0), (20.0, 16.0)] {
                    pan(&mut app, &mut check, dx, dy);
                }
                check.canvas.draft = false;
                // Nothing to settle.
                assert_eq!(check.frame(&mut app, 0), 0);
                let ws = app.active_ws_mut().unwrap();
                ws.surface_box.set_ants_frame(3);
                assert!(check.frame(&mut app, 0) > 0);
                // The ants' spans alone drew the base under them, not all of it.
                assert!(check.canvas.stale);
                // Review I-1: the old outline goes everywhere it was drawn, the strips included.
                let ws = app.active_ws_mut().unwrap();
                if deselect {
                    ws.selection.reset();
                } else {
                    let r = Rectangle::new(380.0, 280.0, 60.0, 40.0);
                    let path = GraphicsPath::from_rect(&r);
                    ws.selection
                        .set_continuation_path(path, CombineMode::Replace);
                }
                ws.selection.commit_continuation();
                assert!(check.frame(&mut app, 0) > 0);
                // Exact still, the small one's outline too dense to list.
                pan(&mut app, &mut check, -24.0, -20.0);
                // A dab in view, then a zoom.
                app.set_active_tool_from_type(ToolType::PaintBrush);
                app.on_mouse_down(150.0, 110.0, MouseButton::Left, &input(150.0, 110.0));
                app.on_mouse_up(159.0, 110.0, MouseButton::Left, &input(159.0, 110.0));
                check.frame(&mut app, 0);
                let ws = app.active_ws_mut().unwrap();
                ws.view.zoom = zoom * 1.25;
                check.frame(&mut app, 0);
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
