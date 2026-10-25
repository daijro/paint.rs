//! The selection canvas layer: the marching-ants outline and the selection tint.
//! Port of: js/src/engine/document/render/renderer/SelectionRenderer.js (its role only; the
//! pixels follow Paint.NET)
//! Reference: paintdotjs @ b4e6fc5
//! Also the second, untinted layer a selection tool draws, and the selection animation. Rulings
//! P2: tiny-skia analytic AA stands in for Direct2D per-primitive AA, ±1–2 at edges (B-C10
//! calibrates geometry and phase). The background mask render: a complex selection's masks build
//! on worker threads (`mask_builds`) while the old ones show.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex};

use tiny_skia::{FillRule, LineCap, LineJoin, Mask, PathBuilder, Stroke, StrokeDash, Transform};

use super::display::{DisplayTarget, RenderMode, blend_over, to_space};
use super::view::ViewTransform;
use crate::document::workspace::DocumentWorkspace;
use crate::pixel::IntRect;
use crate::pixel::mask::SelectionQuality;
use crate::selection::matrix::{Matrix, MatrixOrder};
use crate::selection::path::GraphicsPath;

/// One selection layer. `path: None` draws the workspace selection; `Some` draws that geometry
/// (a selection tool's live layer).
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionRenderer {
    pub visible: bool,
    /// The Highlight tint over the interior (§2.9 step 5).
    pub tinting: bool,
    pub outline: bool,
    pub path: Option<GraphicsPath>,
    /// The ants frame in 0..frames.
    pub frame: u32,
    pub frames: u32,
    pub quality: SelectionQuality,
    /// The system Highlight colour.
    pub highlight: [u8; 3],
}

impl Default for SelectionRenderer {
    fn default() -> Self {
        Self {
            visible: true,
            tinting: false,
            outline: true,
            path: None,
            frame: 0,
            frames: 8,
            quality: SelectionQuality::HighQuality,
            highlight: super::ruler::HIGHLIGHT,
        }
    }
}

/// A8 masks at device resolution over `rect` (row-major, `rect.w` wide).
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineMasks {
    pub rect: IntRect,
    /// The aliased even-odd fill.
    pub interior: Vec<u8>,
    pub solid: Vec<u8>,
    /// One dashed outline per frame.
    pub dashed: Vec<Vec<u8>>,
}

/// §2.9 step 1: the device bounds of the geometry (integer in Aliased mode), inflated by 1 px,
/// ∩ the viewport, as an integer rect.
fn mask_rect(polys: &[Vec<[f64; 2]>], t: &ViewTransform, quality: SelectionQuality) -> IntRect {
    let (mut l, mut tp, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in polys.iter().flatten() {
        let [x, y] = device(*p, t, quality);
        (l, tp, r, b) = (l.min(x), tp.min(y), r.max(x), b.max(y));
    }
    if !(l <= r && tp <= b) {
        return IntRect::default();
    }
    let edge = |v: f64| v.clamp(-1e9, 1e9) as i32;
    let (x0, y0) = (edge((l - 1.0).floor()), edge((tp - 1.0).floor()));
    let (x1, y1) = (edge((r + 1.0).ceil()), edge((b + 1.0).ceil()));
    IntRect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32).intersect(&IntRect::new(
        0,
        0,
        t.viewport[0],
        t.viewport[1],
    ))
}

/// Image → device px (§2.9 step 2); Aliased mode pixelates the geometry (rounded vertices).
fn device(p: [f64; 2], t: &ViewTransform, quality: SelectionQuality) -> [f64; 2] {
    let d = [p[0] * t.scale + t.origin[0], p[1] * t.scale + t.origin[1]];
    match quality {
        SelectionQuality::Aliased => d.map(f64::round),
        SelectionQuality::HighQuality => d,
    }
}

/// An outline piece: a whole closed polygon (`start` 0) or a visible run of one (open), with the
/// path length from the polygon's start to the run's first point.
struct Run {
    points: Vec<[f32; 2]>,
    closed: bool,
    start: f64,
}

/// Liang–Barsky: the parameter range of a → b inside [lo, hi]², if any.
fn clip_segment(a: [f64; 2], b: [f64; 2], lo: f64, hi: [f64; 2]) -> Option<(f64, f64)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a[0] - lo),
        (dx, hi[0] - a[0]),
        (-dy, a[1] - lo),
        (dy, hi[1] - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    (t0 <= t1).then_some((t0, t1))
}

/// Each polygon's outline in mask coordinates: closed when it lies within 2 px of the `w × h`
/// mask, else the pieces of it that do, each keeping its distance from the figure start so the
/// dashes keep their phase without building distant, invisible dashes. A piece starting at the figure start loses that one vertex's join.
fn outline_runs(
    polys: &[Vec<[f64; 2]>],
    to_local: impl Fn([f64; 2]) -> [f64; 2],
    w: u32,
    h: u32,
) -> Vec<Run> {
    let (lo, hi) = (-2.0, [w as f64 + 2.0, h as f64 + 2.0]);
    let f32p = |p: [f64; 2]| [p[0] as f32, p[1] as f32];
    let lerp =
        |a: [f64; 2], b: [f64; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let mut runs = vec![];
    for poly in polys {
        let mut pts: Vec<[f64; 2]> = poly.iter().map(|&p| to_local(p)).collect();
        pts.dedup();
        if pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        let n = pts.len();
        if n < 2 || pts.iter().flatten().any(|v| !v.is_finite()) {
            continue;
        }
        let edge = |i: usize| (pts[i], pts[(i + 1) % n]);
        if (0..n).all(|i| clip_segment(edge(i).0, edge(i).1, lo, hi) == Some((0.0, 1.0))) {
            let points = pts.iter().map(|&p| f32p(p)).collect();
            runs.push(Run {
                points,
                closed: true,
                start: 0.0,
            });
            continue;
        }
        let mut dist = 0.0;
        let mut run: Option<Run> = None;
        for i in 0..n {
            let (a, b) = edge(i);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            match clip_segment(a, b, lo, hi) {
                Some((t0, t1)) => {
                    if t0 > 0.0 {
                        runs.extend(run.take());
                    }
                    let start = dist + t0 * len;
                    let r = run.get_or_insert_with(|| Run {
                        points: vec![f32p(lerp(a, b, t0))],
                        closed: false,
                        start,
                    });
                    r.points.push(f32p(lerp(a, b, t1)));
                    if t1 < 1.0 {
                        runs.extend(run.take());
                    }
                }
                None => runs.extend(run.take()),
            }
            dist += len;
        }
        runs.extend(run);
    }
    runs
}

fn fill(mask: &mut Mask, pb: PathBuilder, rule: FillRule, aa: bool) {
    if let Some(path) = pb.finish() {
        mask.fill_path(&path, rule, aa, Transform::identity());
    }
}

fn run_path(r: &Run) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(r.points[0][0], r.points[0][1]);
    for p in &r.points[1..] {
        pb.line_to(p[0], p[1]);
    }
    if r.closed {
        pb.close();
    }
    pb.finish()
}

fn dash_style((len, shift): (f64, f64), start: f64) -> Option<StrokeDash> {
    StrokeDash::new(
        vec![(len / 2.0) as f32; 2],
        (len - shift + start).rem_euclid(len) as f32,
    )
}

/// A single contour too long for tiny-skia: walk its straight edges through the same on/off
/// intervals. Keep the first dash until the end so a closed contour joins across its start.
fn dash_long_run(r: &Run, (len, shift): (f64, f64), out: &mut PathBuilder) {
    let half = (len / 2.0) as f32 as f64;
    if half <= 0.0 || !half.is_finite() {
        return;
    }
    let phase = (len - shift + r.start).rem_euclid(len) as f32 as f64;
    let mut on = phase < half;
    let mut remaining = if on { half - phase } else { 2.0 * half - phase };
    let mut first = Vec::new();
    let mut holding = r.closed && on;
    let mut drawing = false;
    let mut last = None;
    let mut ended_on = false;
    let back = r
        .closed
        .then(|| [r.points[r.points.len() - 1], r.points[0]]);
    for [a, b] in r.points.windows(2).map(|p| [p[0], p[1]]).chain(back) {
        let length = ((b[0] as f64) - a[0] as f64).hypot((b[1] as f64) - a[1] as f64);
        let mut distance = 0.0;
        while distance < length {
            let end = (distance + remaining).min(length);
            ended_on = on;
            let at = |d: f64| {
                [
                    (a[0] as f64 + (b[0] as f64 - a[0] as f64) * (d / length)) as f32,
                    (a[1] as f64 + (b[1] as f64 - a[1] as f64) * (d / length)) as f32,
                ]
            };
            if on {
                let p = at(distance);
                let q = at(end);
                if holding {
                    if first.is_empty() {
                        first.push(p);
                    }
                    first.push(q);
                } else {
                    if !drawing {
                        out.move_to(p[0], p[1]);
                    }
                    out.line_to(q[0], q[1]);
                    last = Some(q);
                }
                drawing = true;
            }
            remaining -= end - distance;
            distance = end;
            if remaining <= 0.0 {
                on = !on;
                remaining = half;
                // Leave `drawing` true at the contour end: its dash can join the first one.
                if distance < length {
                    drawing = false;
                }
                if !on {
                    holding = false;
                }
            }
        }
        // At a vertex exactly on a dash boundary, the next edge begins a new interval.
        if !on {
            drawing = false;
        }
    }
    if !first.is_empty() {
        if !ended_on || last != Some(first[0]) || holding {
            out.move_to(first[0][0], first[0][1]);
        }
        for p in &first[1..] {
            out.line_to(p[0], p[1]);
        }
        if holding {
            out.close();
        }
    }
}

/// Partition only dashing, then concatenate the paths before stroking/filling. Separate fills
/// would change antialiasing where closed figures overlap. A failed bounded group is subdivided
/// down to a single contour, whose line dashes have no library count limit.
fn dashed_group(runs: &[&Run], pattern: (f64, f64), out: &mut PathBuilder) {
    if runs.is_empty() {
        return;
    }
    let mut pb = PathBuilder::new();
    for r in runs {
        if let Some(p) = run_path(r) {
            pb.push_path(&p);
        }
    }
    if let Some(p) = pb.finish()
        && let Some(style) = dash_style(pattern, runs[0].start)
        && let Some(dashed) = p.dash(&style, 1.0)
    {
        out.push_path(&dashed);
        return;
    }
    if runs.len() > 1 {
        let (a, b) = runs.split_at(runs.len() / 2);
        dashed_group(a, pattern, out);
        dashed_group(b, pattern, out);
    } else {
        dash_long_run(runs[0], pattern, out);
    }
}

/// Strokes runs at `width` device px, with flat caps and bevel joins. Closed figures share one
/// fill (union); clipped open runs retain their individual phase and fill ordering.
fn stroke_runs(mask: &mut Mask, runs: &[Run], aa: bool, dash: Option<(f64, f64)>, width: f32) {
    let stroke = Stroke {
        width,
        miter_limit: 10.0,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Bevel,
        dash: None,
    };
    let closed: Vec<&Run> = runs.iter().filter(|r| r.closed).collect();
    let groups = std::iter::once(closed).chain(runs.iter().filter(|r| !r.closed).map(|r| vec![r]));
    for group in groups {
        let mut pb = PathBuilder::new();
        match dash {
            Some(pattern) => dashed_group(&group, pattern, &mut pb),
            None => {
                for r in group {
                    if let Some(p) = run_path(r) {
                        pb.push_path(&p);
                    }
                }
            }
        }
        if let Some(p) = pb.finish()
            && let Some(outline) = p.stroke(&stroke, 1.0)
        {
            mask.fill_path(&outline, FillRule::Winding, aa, Transform::identity());
        }
    }
}

/// One layer's masks over `rect`, all built at once: the interior only for a tinting layer, the
/// solid outline, every dashed frame and the spans only for an outlined one.
struct LayerMasks {
    rect: IntRect,
    interior: Vec<u8>,
    /// The interior's runs of pixels, as one-row device rects sorted by row (tinting layers).
    interior_runs: Vec<IntRect>,
    solid: Vec<u8>,
    dashed: Vec<Vec<u8>>,
    /// The pixels an ants frame can change (`spans`).
    spans: Option<Vec<IntRect>>,
}

impl LayerMasks {
    /// Frame f's dashed outline moves the dashes f·L/F px toward the path end (tiny-skia phase
    /// (L − f·L/F) mod L, plus each run's distance from its figure start), L = 8u px. The masks
    /// render concurrently on native targets (each is one tiny-skia fill, the same pixels in any
    /// order); `None` once `live` turns false, which is checked before each mask.
    // ponytail: one tiny-skia AA fill per mask (single-threaded, ~100 B of edges per outline
    // vertex): a 4K salt-and-pepper selection (a Magic Wand on a noisy photo) takes ~6 s for its
    // solid outline alone, the floor of the whole build. Splitting one fill into row bands is
    // not pixel-exact (clipping moves tiny-skia's fixed-point edges); a dedicated 1-px outline
    // rasterizer is the upgrade path.
    fn new(
        polys: &[Vec<[f64; 2]>],
        t: &ViewTransform,
        quality: SelectionQuality,
        frames: u32,
        rect: IntRect,
        (tinting, outline): (bool, bool),
        live: &(dyn Fn() -> bool + Sync),
    ) -> Option<Self> {
        let aa = quality == SelectionQuality::HighQuality;
        let frames = frames.max(1) as usize;
        let local = |p: [f64; 2]| {
            let [x, y] = device(p, t, quality);
            [x - rect.x as f64, y - rect.y as f64]
        };
        let runs = match outline {
            true => outline_runs(polys, |p| local(p).map(|v| v + 0.5), rect.w, rect.h),
            false => vec![],
        };
        // u = the UI scale; L = 8u px.
        let len = 8.0 * t.ui_scale.max(f64::MIN_POSITIVE);
        // Mask 0 is the interior, 1 the solid outline, 2 + f dashed frame f.
        let wanted: Vec<usize> = (0..frames + 2)
            .filter(|&i| if i == 0 { tinting } else { outline })
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = fill_bytes(polys, &runs, len);
        let draw = |&i: &usize| {
            if !live() {
                return None;
            }
            // On the workers, the fill waits for room in `FILL_BYTES`, then looks again.
            #[cfg(not(target_arch = "wasm32"))]
            let _permit = match POOL.as_ref().and_then(|p| p.current_thread_index()) {
                Some(_) => {
                    let permit = FILLS.admit(bytes[i.min(2)], FILL_BYTES);
                    Some(live().then_some(permit)?)
                }
                None => None,
            };
            let Some(mut m) = Mask::new(rect.w, rect.h) else {
                return Some(vec![]);
            };
            match i {
                0 => {
                    let mut pb = PathBuilder::new();
                    for poly in polys.iter().filter(|p| p.len() > 2) {
                        let [x, y] = local(poly[0]);
                        pb.move_to(x as f32, y as f32);
                        for &p in &poly[1..] {
                            let [x, y] = local(p);
                            pb.line_to(x as f32, y as f32);
                        }
                        pb.close();
                    }
                    fill(&mut m, pb, FillRule::EvenOdd, false);
                }
                1 => stroke_runs(&mut m, &runs, aa, None, 1.0),
                _ => {
                    let shift = (i - 2) as f64 * len / frames as f64;
                    stroke_runs(&mut m, &runs, aa, Some((len, shift)), 1.0);
                }
            }
            Some(m.take())
        };
        #[cfg(not(target_arch = "wasm32"))]
        let built: Option<Vec<Vec<u8>>> = {
            use rayon::prelude::*;
            wanted.par_iter().map(draw).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let built: Option<Vec<Vec<u8>>> = wanted.iter().map(draw).collect();
        let mut built = built?.into_iter();
        let interior = if tinting { built.next()? } else { vec![] };
        let mut m = LayerMasks {
            rect,
            interior_runs: mask_runs(&interior, rect),
            interior,
            solid: if outline { built.next()? } else { vec![] },
            dashed: built.collect(),
            spans: None,
        };
        if outline {
            m.spans = m.find_spans();
        }
        Some(m)
    }

    /// Frame `f`'s dashed outline (the last one past the end).
    fn dashed(&self, f: u32) -> &[u8] {
        &self.dashed[(f as usize).min(self.dashed.len() - 1)]
    }

    /// The device pixels an ants frame can change: those the solid outline or any dashed frame
    /// covers (a dashed frame can reach past the solid outline: an aliased dash end, a
    /// self-crossing figure's winding), as one-row rects sorted by row then column, runs under
    /// `SPAN_GAP` px apart merged. `None` when they cost over half a refresh of the whole rect;
    /// the solid outline alone can decide that.
    fn find_spans(&self) -> Option<Vec<IntRect>> {
        let budget = self.rect.w as usize * self.rect.h as usize / 2;
        self.spans_within(&[&self.solid], budget)?;
        let all: Vec<&[u8]> = std::iter::once(&self.solid[..])
            .chain(self.dashed.iter().map(|d| &d[..]))
            .collect();
        self.spans_within(&all, budget)
    }

    /// The runs of pixels any of `masks` covers (see `find_spans`), or `None` past `budget`
    /// (pixels plus `SPAN_GAP` per run). Bands of rows are scanned concurrently on native
    /// targets.
    fn spans_within(&self, masks: &[&[u8]], budget: usize) -> Option<Vec<IntRect>> {
        let (w, h) = (self.rect.w as usize, self.rect.h as usize);
        let masks: Vec<&[u8]> = masks.iter().copied().filter(|m| !m.is_empty()).collect();
        // Rows j0..j0 + 64: their spans and cost.
        let band = |&j0: &usize| {
            let (mut out, mut cost, mut row) = (vec![], 0, vec![0u8; w]);
            for j in j0..h.min(j0 + 64) {
                row.fill(0);
                for m in &masks {
                    for (o, &v) in row.iter_mut().zip(&m[j * w..][..w]) {
                        *o |= v;
                    }
                }
                let mut x = 0;
                while let Some(a) = row[x..].iter().position(|&v| v != 0).map(|n| x + n) {
                    x = a;
                    loop {
                        x = row[x..].iter().position(|&v| v == 0).map_or(w, |n| x + n);
                        match row[x..w.min(x + SPAN_GAP)].iter().position(|&v| v != 0) {
                            Some(n) => x += n,
                            None => break,
                        }
                    }
                    cost += x - a + SPAN_GAP;
                    if cost > budget {
                        return None;
                    }
                    let (sx, sy) = (self.rect.x + a as i32, self.rect.y + j as i32);
                    out.push(IntRect::new(sx, sy, (x - a) as u32, 1));
                }
            }
            Some((out, cost))
        };
        let bands: Vec<usize> = (0..h).step_by(64).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let bands: Option<Vec<_>> = {
            use rayon::prelude::*;
            bands.par_iter().map(band).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let bands: Option<Vec<_>> = bands.iter().map(band).collect();
        let bands = bands?;
        let cost: usize = bands.iter().map(|b| b.1).sum();
        (cost <= budget).then(|| bands.into_iter().flat_map(|b| b.0).collect())
    }
}

/// Outline runs closer than this are refreshed as one span: a span costs about as much as this
/// many pixels.
const SPAN_GAP: usize = 16;

/// The runs of nonzero pixels of `mask` (row-major over `rect`) as one-row rects, sorted by row.
fn mask_runs(mask: &[u8], rect: IntRect) -> Vec<IntRect> {
    let mut out = vec![];
    let w = rect.w as usize;
    for (j, row) in mask.chunks_exact(w.max(1)).enumerate() {
        let mut x = 0;
        while let Some(a) = row[x..].iter().position(|&v| v != 0).map(|n| x + n) {
            x = row[a..].iter().position(|&v| v == 0).map_or(w, |n| a + n);
            out.push(IntRect::new(
                rect.x + a as i32,
                rect.y + j as i32,
                (x - a) as u32,
                1,
            ));
        }
    }
    out
}

/// The pixels of one-row rects `a` or `b` (each sorted by row, disjoint within it) but not both,
/// as one-row rects sorted by row.
pub(crate) fn xor_runs(a: &[IntRect], b: &[IntRect]) -> Vec<IntRect> {
    let (mut i, mut j, mut out) = (0, 0, vec![]);
    let mut edges = vec![];
    while i < a.len() || j < b.len() {
        let y = match (a.get(i), b.get(j)) {
            (Some(p), Some(q)) => p.y.min(q.y),
            (Some(p), None) => p.y,
            (None, Some(q)) => q.y,
            (None, None) => unreachable!(),
        };
        edges.clear();
        for (runs, k) in [(a, &mut i), (b, &mut j)] {
            while let Some(r) = runs.get(*k).filter(|r| r.y == y) {
                edges.extend([r.x, r.right()]);
                *k += 1;
            }
        }
        // Inside an odd number of runs (one of the two lists): between edges 2k and 2k + 1.
        edges.sort_unstable();
        for &[x0, x1] in edges.as_chunks::<2>().0.iter().filter(|e| e[0] < e[1]) {
            out.push(IntRect::new(x0, y, (x1 - x0) as u32, 1));
        }
    }
    out
}

/// What a tinting layer paints, split for listing what changed between two frames: its outline
/// (every pixel an outline frame paints) and its tint's runs, with the tint colour (the same
/// colour over the same base gives the same pixels, wherever both frames tint).
#[derive(Clone, Debug, PartialEq)]
pub struct TintParts {
    pub highlight: [u8; 3],
    pub outline: Vec<IntRect>,
    pub tint: Vec<IntRect>,
}

/// §2.9 steps 1–3: the interior (aliased even-odd fill), the solid outline and `frames` dashed
/// outlines of `polys` (image px) over the device rect `rect`. Geometry is scaled by `t.scale` and
/// offset by `t.origin` (Aliased: vertices rounded); the outline sits +0.5 px right and down.
/// Dashes are [4u, 4u] (L = 8u) from each figure's start; frame f moves them f·L/F px toward the
/// path end (tiny-skia phase (L − f·L/F) mod L). Antialiased in HighQuality only.
pub fn outline_masks(
    polys: &[Vec<[f64; 2]>],
    t: &ViewTransform,
    quality: SelectionQuality,
    frames: u32,
    rect: IntRect,
) -> OutlineMasks {
    let parts = (true, true);
    let m = LayerMasks::new(polys, t, quality, frames, rect, parts, &|| true).expect("live");
    OutlineMasks {
        rect,
        interior: m.interior,
        solid: m.solid,
        dashed: m.dashed,
    }
}

/// What a layer's masks are built from: the workspace selection's revision or a live path.
#[derive(Clone)]
enum Geometry {
    Selection(u64),
    Path(Arc<GraphicsPath>),
}

/// Everything a layer's masks depend on.
#[derive(Clone)]
struct Key {
    /// `Document::id`: another document's masks are never drawn meanwhile.
    doc: u64,
    geometry: Geometry,
    t: ViewTransform,
    quality: SelectionQuality,
    frames: u32,
    /// (tinting, outline).
    parts: (bool, bool),
    /// The selection's `placement` (none for a live path): how the masks move meanwhile.
    placement: (u64, Matrix),
    /// The selection's `paths` stamp (0 for a live path): equal while only its interim
    /// transform changed.
    paths: u64,
}

impl Key {
    /// `self` asks for the masks of `old`'s geometry moved by the selection's interim transform
    /// alone (the view and every other input the same): `old`'s masks drawn moved are its masks
    /// up to resampling.
    fn only_moved(&self, old: &Key) -> bool {
        matches!(
            (&self.geometry, &old.geometry),
            (Geometry::Selection(_), Geometry::Selection(_))
        ) && (
            self.doc,
            self.t,
            self.quality,
            self.frames,
            self.parts,
            self.paths,
        ) == (
            old.doc,
            old.t,
            old.quality,
            old.frames,
            old.parts,
            old.paths,
        )
    }
}

/// A finished build: its generation, key and masks (`None`: nothing to draw).
type Built = (u64, Key, Option<Arc<LayerMasks>>);

/// One layer's masks: the newest finished build, and the builds under way.
struct Slot {
    shown: Option<Built>,
    /// The newest build asked for: started, or `queued`.
    pending: Option<Key>,
    /// Its source while it waits for the worker build under way (one at a time per slot, as the
    /// effect previews: a moving selection never piles builds up).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    queued: Option<Source>,
    /// The newest build's generation: an older build stops before its next mask.
    generation: Arc<AtomicU64>,
    /// A worker build of this slot is under way.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    busy: Arc<AtomicBool>,
    /// Finished builds, from the workers.
    done: (mpsc::Sender<Built>, mpsc::Receiver<Built>),
    /// The worker build of `pending` held while a complex selection moves every frame (a Move
    /// drag): its source and heaviness. The shown masks follow meanwhile (`warp`); `mask_builds`
    /// starts it once a frame passes without a move.
    held: Option<(Source, bool)>,
    /// The selection moved since the last `mask_builds`.
    moving: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Slot {
            shown: None,
            pending: None,
            queued: None,
            generation: Arc::default(),
            busy: Arc::default(),
            done: mpsc::channel(),
            held: None,
            moving: false,
        }
    }
}

impl Slot {
    /// Shows `b` if it is newer than the shown build (a superseded build that still finished is
    /// newer masks to draw meanwhile, never the current ones: its key differs).
    fn take(&mut self, b: Built) -> bool {
        let newer = self.shown.as_ref().is_none_or(|s| b.0 > s.0);
        if newer {
            self.shown = Some(b);
        }
        newer
    }
}

/// The mask builds of this thread's canvas: per layer kind (the workspace layer, a tool's live
/// layer) its `Slot`; the waker of worker builds (`mask_builds`; `None`: every build runs in the
/// frame); the stamp of what is shown.
#[derive(Default)]
struct Builds {
    slots: [Slot; 2],
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    stamp: u64,
}

thread_local! {
    static BUILDS: RefCell<Builds> = RefCell::new(Builds::default());
}

/// The mask builds' own threads: never rayon's global pool, which the frame's parallel rows use.
#[cfg(not(target_arch = "wasm32"))]
static POOL: std::sync::LazyLock<Option<rayon::ThreadPool>> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(MASK_THREADS)
        .thread_name(|i| format!("paintrs-masks-{i}"))
        .build()
        .ok()
});

/// Mask fills at once over all worker builds: one takes up to ~1.5 GB of tiny-skia edges for a
/// 4K noise selection.
#[cfg(not(target_arch = "wasm32"))]
const MASK_THREADS: usize = 6;

/// A geometry with more vertices than this builds on the workers. One with fewer builds in the
/// frame (Select All at 4K: ~10 ms, its pixels rather than its fills), so a simple selection
/// shows at once and never waits behind a complex one's fills.
const LIGHT_POINTS: usize = 4096;

/// A geometry whose edges cross more viewport rows than this in all builds on the workers too: a
/// fill's cost per edge grows with its rows (every mask of a 4K full-height zigzag: 64 vertices
/// 45 ms, 4096 vertices 0.5 s; 4096 vertices 16 rows high: 21 ms).
const LIGHT_ROWS: f64 = 32768.0;

/// Whether `paths` (image px, drawn under `t`) are too much to build in the frame: over
/// `LIGHT_POINTS` vertices (counted only that far) or over `LIGHT_ROWS` viewport rows crossed by
/// edges (each edge clamped to the viewport: a rectangle zoomed far in stays light).
fn heavy<'a>(paths: impl IntoIterator<Item = &'a GraphicsPath>, t: &ViewTransform) -> bool {
    let h = t.viewport[1] as f64;
    let row = |p: &crate::util::geom::Point| (p.y * t.scale + t.origin[1]).clamp(0.0, h);
    geometry_is_heavy(paths, row)
}

fn geometry_is_heavy<'a>(
    paths: impl IntoIterator<Item = &'a GraphicsPath>,
    row: impl Fn(&crate::util::geom::Point) -> f64,
) -> bool {
    let (mut points, mut rows) = (0, 0.0);
    for p in paths.into_iter().flat_map(|p| &p.polygons) {
        points += p.len();
        if points > LIGHT_POINTS {
            return true;
        }
        let edges = p.iter().zip(p.iter().cycle().skip(1));
        rows += edges.map(|(a, b)| (row(b) - row(a)).abs()).sum::<f64>();
    }
    rows.is_nan() || rows > LIGHT_ROWS
}

#[derive(Clone, Copy, PartialEq)]
struct StatisticsKey {
    doc: u64,
    revision: u64,
    bounds: crate::util::geom::Rectangle,
}

type StatisticsSource = Box<dyn FnOnce() -> Option<crate::selection::SelectionStatistics> + Send>;
type StatisticsBuilt = (u64, StatisticsKey, crate::selection::SelectionStatistics);

/// One active request and one replacement, just like the mask slots. Completed geometry is
/// formatted by the caller so DPI, units, language and the live angle never become stale.
struct Statistics {
    requested: Option<StatisticsKey>,
    shown: Option<(StatisticsKey, crate::selection::SelectionStatistics)>,
    queued: Option<StatisticsSource>,
    generation: Arc<AtomicU64>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    busy: Arc<AtomicBool>,
    done: (
        mpsc::Sender<StatisticsBuilt>,
        mpsc::Receiver<StatisticsBuilt>,
    ),
}

impl Default for Statistics {
    fn default() -> Self {
        Self {
            requested: None,
            shown: None,
            queued: None,
            generation: Arc::default(),
            busy: Arc::default(),
            done: mpsc::channel(),
        }
    }
}

thread_local! {
    static STATISTICS: RefCell<Statistics> = RefCell::new(Statistics::default());
}

/// Footer statistics: heavy geometry uses the mask workers when the canvas registered a waker.
/// Empty selections and document switches invalidate previous results before polling completions.
pub(crate) fn selection_statistics(
    ws: &DocumentWorkspace,
) -> Option<crate::selection::SelectionStatistics> {
    let key = StatisticsKey {
        doc: ws.document.id,
        revision: ws.selection.revision(),
        bounds: ws.document.get_bounds(),
    };
    STATISTICS.with_borrow_mut(|s| {
        if s.requested != Some(key) {
            s.generation.fetch_add(1, Ordering::Relaxed);
            s.requested = Some(key);
            s.queued = None;
            if s.shown.as_ref().is_some_and(|(old, _)| old.doc != key.doc) {
                s.shown = None;
            }
            if ws.selection.is_empty() {
                s.shown = None;
                return None;
            }
            let m = ws.selection.get_interim_transform().elements;
            let heavy = geometry_is_heavy(ws.selection.sources(), |p| {
                m[1][0] * p.x + m[1][1] * p.y + m[1][2]
            });
            let later = ws.selection.create_path_later();
            let generation = s.generation.clone();
            let g = generation.load(Ordering::Relaxed);
            let source: StatisticsSource = Box::new(move || {
                let path = later();
                #[cfg(not(target_arch = "wasm32"))]
                let _permit = POOL
                    .as_ref()
                    .and_then(|p| p.current_thread_index())
                    .map(|_| {
                        // Scan storage is bounded by edge-row crossings; geometry is counted here,
                        // on the worker, never while deciding whether the UI should offload it.
                        let mut bytes = 0usize;
                        for poly in &path.polygons {
                            bytes = bytes.saturating_add(poly.len().saturating_mul(96));
                            for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)) {
                                bytes = bytes.saturating_add(
                                    ((a.y - b.y).abs().ceil() as usize).saturating_mul(24),
                                );
                            }
                        }
                        FILLS.admit(bytes, FILL_BYTES)
                    });
                (generation.load(Ordering::Relaxed) == g)
                    .then(|| crate::selection::statistics_for_path(&path, &key.bounds))
            });
            #[cfg(not(target_arch = "wasm32"))]
            let worker = heavy && POOL.is_some() && BUILDS.with_borrow(|b| b.wake.is_some());
            #[cfg(target_arch = "wasm32")]
            let worker = {
                let _ = heavy;
                false
            };
            if worker {
                s.queued = Some(source);
            } else {
                s.shown = source().map(|stats| (key, stats));
            }
        }
        while let Ok((generation, completed, stats)) = s.done.1.try_recv() {
            if generation == s.generation.load(Ordering::Relaxed) && s.requested == Some(completed)
            {
                s.shown = Some((completed, stats));
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !s.busy.load(Ordering::Acquire)
            && let Some(source) = s.queued.take()
        {
            let wake = BUILDS
                .with_borrow(|b| b.wake.clone())
                .expect("registered worker waker");
            let (generation, busy, done) = (s.generation.clone(), s.busy.clone(), s.done.0.clone());
            let g = generation.load(Ordering::Relaxed);
            busy.store(true, Ordering::Release);
            POOL.as_ref().expect("worker pool").spawn(move || {
                if generation.load(Ordering::Relaxed) == g
                    && let Some(stats) = source()
                    && generation.load(Ordering::Relaxed) == g
                {
                    let _ = done.send((g, key, stats));
                }
                busy.store(false, Ordering::Release);
                // A replacement may be waiting even when this result was superseded.
                wake();
            });
        }
        s.shown.map(|(_, stats)| stats)
    })
}

/// The tiny-skia edges and paths the workers' fills may hold at once: each fill of a 4K noise
/// selection takes 0.4–2.2 GB (`fill_bytes`).
#[cfg(not(target_arch = "wasm32"))]
const FILL_BYTES: usize = 3 << 30;

/// The workers' fills: their bytes admitted (`fill_bytes` each) and their turn signal.
#[cfg(not(target_arch = "wasm32"))]
static FILLS: Gate = Gate {
    used: Mutex::new(0),
    turn: Condvar::new(),
};

/// Admits fills while their bytes fit a limit.
#[cfg(not(target_arch = "wasm32"))]
struct Gate {
    used: Mutex<usize>,
    turn: Condvar,
}

/// A fill's share of its gate, given back when dropped.
#[cfg(not(target_arch = "wasm32"))]
struct Permit<'a>(&'a Gate, usize);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.used.lock().unwrap_or_else(|e| e.into_inner()) -= self.1;
        self.0.turn.notify_all();
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Gate {
    /// Waits until `bytes` more fit within `limit` (at once when no fill is under way).
    fn admit(&self, bytes: usize, limit: usize) -> Permit<'_> {
        let mut held = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while *held > 0 && held.saturating_add(bytes) > limit {
            held = self.turn.wait(held).unwrap_or_else(|e| e.into_inner());
        }
        *held += bytes;
        Permit(self, bytes)
    }
}

/// `fill_bytes` per drawn vertex, the most measured (VmHWM over one fill, 4K noise 30 % and 50 %):
/// the interior's aliased fill 69–71 B, the solid outline's stroke 414–419 B, a dashed frame's
/// dashes and stroke 169–244 B. `fill_bytes` also budgets emitted dash endpoints on long edges.
#[cfg(not(target_arch = "wasm32"))]
const FILL_PER_VERTEX: usize = 75;
#[cfg(not(target_arch = "wasm32"))]
const SOLID_PER_POINT: usize = 420;
#[cfg(not(target_arch = "wasm32"))]
const DASHED_PER_POINT: usize = 250;

/// Every dash now draws, including dense selections. Reserve both source vertices and emitted
/// dash endpoints: long edges can contain many dashes even with very few source points.
#[cfg(not(target_arch = "wasm32"))]
fn fill_bytes(polys: &[Vec<[f64; 2]>], runs: &[Run], len: f64) -> [usize; 3] {
    let points: usize = runs.iter().map(|r| r.points.len()).sum();
    let dashes: f64 = runs.iter().map(|r| (run_length(r) / len).ceil()).sum();
    [
        FILL_PER_VERTEX.saturating_mul(polys.iter().map(Vec::len).sum()),
        SOLID_PER_POINT.saturating_mul(points),
        DASHED_PER_POINT
            .saturating_mul(points)
            .saturating_add((dashes as usize).saturating_mul(512)),
    ]
}

/// A run's length in device px (closed: back to its start).
#[cfg(not(target_arch = "wasm32"))]
fn run_length(r: &Run) -> f64 {
    let p = &r.points;
    let back = r.closed.then(|| [p[p.len() - 1], p[0]]);
    let edges = p.windows(2).map(|w| [w[0], w[1]]).chain(back);
    let edge = |[a, b]: [[f32; 2]; 2]| ((b[0] - a[0]) as f64).hypot((b[1] - a[1]) as f64);
    edges.map(edge).sum()
}

type Polys = Vec<Vec<[f64; 2]>>;

/// Makes a build's polygons (on the thread that builds it).
type Source = Box<dyn FnOnce() -> Polys + Send>;

/// §2.9 step 1 ("Mask render (background thread)") for this thread's canvas view, called once a
/// frame before the overlays: from now on its complex selections' masks build on worker threads
/// and each build calls `wake` as it ends. Shows the builds that finished since the last call; the
/// returned stamp changes whenever that changed what the overlays draw.
// ponytail: wasm32 has no threads here, so its masks always build in the frame (a 4K noise
// selection stalls it for seconds, as before Task 25q); wasm-bindgen-rayon is the upgrade path.
pub fn mask_builds(wake: Arc<dyn Fn() + Send + Sync>) -> u64 {
    BUILDS.with_borrow_mut(|b| {
        b.wake = Some(wake.clone());
        for kind in 0..b.slots.len() {
            let slot = &mut b.slots[kind];
            while let Ok(built) = slot.done.1.try_recv() {
                if slot.take(built) {
                    b.stamp += 1;
                }
            }
            // The build under way ended: the one waiting starts.
            #[cfg(not(target_arch = "wasm32"))]
            if !slot.busy.load(Ordering::Acquire)
                && let (Some(polys), Some(key)) = (slot.queued.take(), slot.pending.clone())
            {
                spawn(slot, key, polys, &wake);
            }
            // A held build waits while the selection still moves (another frame comes to see),
            // and starts once a whole frame passed without a move.
            let slot = &mut b.slots[kind];
            if std::mem::take(&mut slot.moving) {
                wake();
            } else if let (Some((polys, heavy)), Some(key)) =
                (slot.held.take(), slot.pending.clone())
                && start(b, kind, key, polys, heavy)
            {
                b.stamp += 1;
            }
        }
        b.stamp
    })
}

/// The masks for `key` from `polys` (`Some(None)`: nothing to draw); `None` once `live` turns
/// false.
fn build(
    polys: Polys,
    key: &Key,
    live: &(dyn Fn() -> bool + Sync),
) -> Option<Option<Arc<LayerMasks>>> {
    let rect = mask_rect(&polys, &key.t, key.quality);
    if rect.is_empty() {
        return Some(None);
    }
    let m = LayerMasks::new(
        &polys,
        &key.t,
        key.quality,
        key.frames,
        rect,
        key.parts,
        live,
    )?;
    Some(Some(Arc::new(m)))
}

/// Asks for slot `kind`'s build of `key` from `polys`, superseding the one under way: on a
/// worker when `heavy` and `mask_builds` set one up (native builds), at once or once the slot's
/// build under way ends; else here. True when it built here (the slot shows it).
fn start(b: &mut Builds, kind: usize, key: Key, polys: Source, heavy: bool) -> bool {
    let slot = &mut b.slots[kind];
    slot.generation.fetch_add(1, Ordering::Relaxed);
    slot.pending = Some(key.clone());
    (slot.queued, slot.held, slot.moving) = (None, None, false);
    #[cfg(not(target_arch = "wasm32"))]
    if let (true, Some(wake), Some(_)) = (heavy, &b.wake, POOL.as_ref()) {
        if slot.busy.load(Ordering::Acquire) {
            slot.queued = Some(polys);
        } else {
            spawn(slot, key, polys, wake);
        }
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = heavy;
    let g = slot.generation.load(Ordering::Relaxed);
    let masks = build(polys(), &key, &|| true).expect("live");
    slot.take((g, key, masks))
}

/// Starts `slot`'s worker build of `key` at the slot's generation.
#[cfg(not(target_arch = "wasm32"))]
fn spawn(slot: &Slot, key: Key, polys: Source, wake: &Arc<dyn Fn() + Send + Sync>) {
    let Some(pool) = POOL.as_ref() else {
        return;
    };
    let g = slot.generation.load(Ordering::Relaxed);
    let (done, generation, wake) = (slot.done.0.clone(), slot.generation.clone(), wake.clone());
    let busy = slot.busy.clone();
    busy.store(true, Ordering::Release);
    pool.spawn(move || {
        let live = || generation.load(Ordering::Relaxed) == g;
        if let Some(masks) = live().then(|| build(polys(), &key, &live)).flatten() {
            let _ = done.send((g, key, masks));
        }
        busy.store(false, Ordering::Release);
        // Also when it stopped early: one spare frame (and the build waiting starts).
        wake();
    });
}

impl SelectionRenderer {
    /// §2.4 "Selection animation" / §2.9 step 4: 8 frames in Hardware, 4 in Software, 1 with
    /// animations off.
    pub fn frames_for(mode: RenderMode, animations: bool) -> u32 {
        match (animations, mode) {
            (false, _) => 1,
            (true, RenderMode::Hardware) => 8,
            (true, RenderMode::Software) => 4,
        }
    }

    /// §2.9 step 4: counter = ⌊30·now_s⌋ mod 8 (one cycle every 0.26667 s); frame =
    /// counter·frames/8.
    pub fn frame_at(now_s: f64, frames: u32) -> u32 {
        let c = (30.0 * now_s).floor();
        let counter = if c.is_finite() {
            c.rem_euclid(8.0) as u32
        } else {
            0
        };
        counter * frames / 8
    }

    /// The masks this layer draws (the interior when tinting, the outlines when outlined) of its
    /// geometry under `t`, kept per document, geometry, transform, quality, frame count and those
    /// two flags; `None` when there is nothing to draw. While a build of them is under way
    /// (`mask_builds`), this layer's newest masks of the document, under the `warp` to the view
    /// and the selection's transform now (§2.9 step 1: "Meanwhile the old masks are drawn under
    /// the interim transform"), or `None` if there are none.
    fn masks(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<Drawn> {
        let parts = (self.tinting, self.outline);
        let frames = self.frames.max(1);
        let (doc, rev) = (ws.document.id, ws.selection.revision());
        let is = |k: &Key| {
            k.doc == doc
                && k.t == *t
                && k.quality == self.quality
                && k.frames == frames
                && k.parts == parts
                && match (&k.geometry, &self.path) {
                    (Geometry::Selection(r), None) => *r == rev,
                    (Geometry::Path(p), Some(q)) => **p == *q,
                    _ => false,
                }
        };
        let current = |s: &Slot| {
            let b = s.shown.as_ref().filter(|b| is(&b.1))?;
            Some(b.2.clone().map(|m| (m, None)))
        };
        let (placement, paths) = match &self.path {
            Some(_) => ((0, Matrix::identity()), 0),
            None => (ws.selection.placement(), ws.selection.paths()),
        };
        let kind = self.path.is_some() as usize;
        BUILDS.with_borrow_mut(|b| {
            if let Some(m) = current(&b.slots[kind]) {
                return m;
            }
            if !b.slots[kind].pending.as_ref().is_some_and(is) {
                let (geometry, polys, heavy): (_, Source, _) = match &self.path {
                    Some(p) => {
                        let heavy = heavy([p], t);
                        let p = Arc::new(p.clone());
                        let polys = p.clone();
                        (Geometry::Path(p), Box::new(move || polys.as_polys()), heavy)
                    }
                    None => {
                        let heavy = heavy(ws.selection.sources(), t);
                        let path = ws.selection.create_path_later();
                        let polys = move || path().as_polys();
                        (Geometry::Selection(rev), Box::new(polys), heavy)
                    }
                };
                let key = Key {
                    doc,
                    geometry,
                    t: *t,
                    quality: self.quality,
                    frames,
                    parts,
                    placement,
                    paths,
                };
                // Task 27p, ruling R77: while a complex selection only moves, and the canvas view
                // polls builds (`mask_builds`), its shown masks follow it and the worker rebuild
                // waits until it stops; a simple one rebuilds in the frame (crisp ants) unless it
                // only translates, from masks the viewport did not clip: then they are shown
                // shifted by whole device px (`snapped`), and rebuilt in the frame once it stops.
                let slot = &mut b.slots[kind];
                let follows = slot
                    .shown
                    .as_ref()
                    .is_some_and(|s| s.2.is_some() && key.only_moved(&s.1));
                let view = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
                let translates = |s: &Built| {
                    let r = s.2.as_ref()?.rect;
                    let unclipped = r.x > view.x
                        && r.y > view.y
                        && r.right() < view.right()
                        && r.bottom() < view.bottom();
                    let [a, b] = warp(&s.1, t, placement, r)?.inv;
                    (unclipped && [a[0], a[1], b[0], b[1]] == [1.0, 0.0, 0.0, 1.0]).then_some(())
                };
                let translated = slot.shown.as_ref().and_then(translates).is_some();
                if b.wake.is_some() && follows && (heavy || translated) {
                    slot.generation.fetch_add(1, Ordering::Relaxed);
                    slot.pending = Some(key);
                    slot.queued = None;
                    (slot.held, slot.moving) = (Some((polys, heavy)), true);
                } else {
                    let _ = start(b, kind, key, polys, heavy);
                    if let Some(m) = current(&b.slots[kind]) {
                        return m;
                    }
                }
            }
            let slot = &b.slots[kind];
            let shown = slot.shown.as_ref().filter(|b| b.1.doc == doc)?;
            let masks = shown.2.clone()?;
            let mut warp = warp(&shown.1, t, placement, masks.rect);
            if slot.held.as_ref().is_some_and(|(_, heavy)| !heavy) {
                warp = warp.map(|w| snapped(w, masks.rect, t));
            }
            Some((masks, warp))
        })
    }

    /// The device rect this layer draws into (∩ the viewport), if any.
    pub fn bounds(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<IntRect> {
        if !self.visible || !(self.tinting || self.outline) {
            return None;
        }
        self.masks(ws, t)
            .map(|(m, warp)| warp.map_or(m.rect, |w| w.bounds))
    }

    /// The device pixels an ants frame changes (`LayerMasks::find_spans`), none unless the outline
    /// shows; `None` when a refresh of the whole `bounds` is about as cheap.
    pub fn outline_spans(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<Vec<IntRect>> {
        if !self.visible || !self.outline {
            return Some(vec![]);
        }
        let view = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
        // Old masks drawn meanwhile may reach past a smaller viewport.
        let inside = |s: &Vec<IntRect>| {
            let s = s.iter().map(|s| s.intersect(&view));
            s.filter(|s| !s.is_empty()).collect()
        };
        // Shifted old masks: their spans shifted; otherwise warped, the whole of their bounds.
        match self.masks(ws, t) {
            None => Some(vec![]),
            Some((m, None)) => m.spans.as_ref().map(inside),
            Some((
                m,
                Some(Warp {
                    shift: Some((dx, dy)),
                    ..
                }),
            )) => m.spans.as_ref().map(|s| {
                let s = s.iter().map(|s| IntRect::new(s.x + dx, s.y + dy, s.w, s.h));
                inside(&s.collect())
            }),
            Some((_, Some(_))) => None,
        }
    }

    /// A tinting layer's outline and tint runs (see [`TintParts`]); `None` when it does not tint,
    /// or they are not listed (old masks drawn warped, or an outline too dense to list).
    pub fn tint_parts(&self, ws: &DocumentWorkspace, t: &ViewTransform) -> Option<TintParts> {
        if !self.visible || !self.tinting {
            return None;
        }
        let (m, warp) = self.masks(ws, t)?;
        let (dx, dy) = match warp {
            None => (0, 0),
            Some(Warp { shift, .. }) => shift?,
        };
        let outline = self.outline_spans(ws, t)?;
        // As `painter`: within the drawn rect ∩ the canvas.
        let drawn = warp.map_or(m.rect, |w| w.bounds);
        let area = drawn.intersect(&t.image_pixel_bounds(ws.document.width, ws.document.height));
        let tint = m
            .interior_runs
            .iter()
            .map(|r| IntRect::new(r.x + dx, r.y + dy, r.w, 1).intersect(&area))
            .filter(|r| !r.is_empty())
            .collect();
        Some(TintParts {
            highlight: self.highlight,
            outline,
            tint,
        })
    }

    /// §2.9 steps 5–6 (`Painter::paint`) into `target`, inside its clip.
    pub fn render(&self, ws: &DocumentWorkspace, target: &mut DisplayTarget, t: &ViewTransform) {
        if let Some(p) = self.painter(ws, t, target.mode) {
            target.paint_rows(|y, x0, row| p.paint(y, x0, row));
        }
    }

    /// What `render` draws in a `mode` target, ready for any rows; `None` when nothing shows.
    pub(crate) fn painter(
        &self,
        ws: &DocumentWorkspace,
        t: &ViewTransform,
        mode: RenderMode,
    ) -> Option<Painter> {
        if !self.visible || !(self.tinting || self.outline) {
            return None;
        }
        let (masks, warp) = self.masks(ws, t)?;
        let drawn = warp.map_or(masks.rect, |w| w.bounds);
        // Old masks drawn meanwhile may lack a part.
        let tint = (self.tinting && !masks.interior.is_empty()).then(|| {
            let [r, g, b] = self.highlight;
            let canvas = t.image_pixel_bounds(ws.document.width, ws.document.height);
            (to_space(mode, [r, g, b, 56]), drawn.intersect(&canvas))
        });
        let frame = (self.outline && !masks.solid.is_empty()).then_some(self.frame);
        Some(Painter {
            masks,
            warp,
            tint,
            frame,
            mode,
        })
    }
}

/// Masks to draw, and the warp of old masks drawn meanwhile.
type Drawn = (Arc<LayerMasks>, Option<Warp>);

/// Old masks drawn for another view or selection transform: each device pixel takes the old mask
/// pixel under its centre's preimage (nearest neighbour).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Warp {
    /// New device px → old device px: x′ = a·x + b·y + c (rows [a, b, c], [d, e, f]).
    inv: [[f64; 3]; 2],
    /// Where they land, ∩ the viewport.
    bounds: IntRect,
    /// A whole-pixel move (dx, dy): a scroll, or a Move drag at 100 %; drawn as shifted masks.
    shift: Option<(i32, i32)>,
}

/// The map from old masks of `old` over `rect` to the device px of `t` with the selection at
/// `placement`: M = D_new·T_new·T_old⁻¹·D_old⁻¹, D the view transform, T the selection's
/// `placement` transform (its interim transform after those committed) while its shape is the
/// same (else none: the old masks stay put). `None` when M is the identity (the same scale and origin, unmoved); empty bounds
/// when it is singular.
fn warp(old: &Key, t: &ViewTransform, (shape, m): (u64, Matrix), rect: IntRect) -> Option<Warp> {
    let moved = old.placement.0 == shape && old.placement.1 != m;
    // A viewport size change alone moves nothing.
    if (old.t.scale, old.t.origin) == (t.scale, t.origin) && !moved {
        return None;
    }
    let device = |t: &ViewTransform| Matrix {
        elements: [
            [t.scale, 0.0, t.origin[0]],
            [0.0, t.scale, t.origin[1]],
            [0.0, 0.0, 1.0],
        ],
    };
    let nothing = Warp {
        inv: [[0.0; 3]; 2],
        bounds: IntRect::default(),
        shift: None,
    };
    let mut map = device(&old.t);
    map = match map.invert() {
        Some(m) => m,
        None => return Some(nothing),
    };
    if moved {
        let Some(back) = old.placement.1.invert() else {
            return Some(nothing);
        };
        map.multiply(&back, MatrixOrder::Append);
        map.multiply(&m, MatrixOrder::Append);
    }
    map.multiply(&device(t), MatrixOrder::Append);
    let Some(inv) = map.invert() else {
        return Some(nothing);
    };
    let e = map.elements;
    let corners = [
        (rect.x, rect.y),
        (rect.right(), rect.y),
        (rect.x, rect.bottom()),
        (rect.right(), rect.bottom()),
    ];
    let (mut l, mut tp, mut r, mut b) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in corners.map(|(x, y)| (x as f64, y as f64)) {
        let (u, v) = (
            e[0][0] * x + e[0][1] * y + e[0][2],
            e[1][0] * x + e[1][1] * y + e[1][2],
        );
        (l, tp, r, b) = (l.min(u), tp.min(v), r.max(u), b.max(v));
    }
    let edge = |v: f64| v.clamp(-1e9, 1e9) as i32;
    let (x0, y0, x1, y1) = (
        edge(l.floor()),
        edge(tp.floor()),
        edge(r.ceil()),
        edge(b.ceil()),
    );
    let view = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
    let bounds = IntRect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32);
    let [r0, r1, _] = inv.elements;
    let whole = |v: f64| ((v - v.round()).abs() < 1e-9).then_some(v.round() as i32);
    let shift = match (r0, r1) {
        ([1.0, 0.0, c], [0.0, 1.0, f]) => whole(-c).zip(whole(-f)),
        _ => None,
    };
    Some(Warp {
        inv: [r0, r1],
        bounds: bounds.intersect(&view),
        shift,
    })
}

/// A translating `w` of masks over `rect` as a whole-device-px shift (the nearest): a simple
/// selection's masks while it moves, ≤ ½ px from where its rebuild puts them once it stops.
fn snapped(w: Warp, rect: IntRect, t: &ViewTransform) -> Warp {
    let (dx, dy) = (-w.inv[0][2].round() as i32, -w.inv[1][2].round() as i32);
    let view = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
    Warp {
        inv: [[1.0, 0.0, -dx as f64], [0.0, 1.0, -dy as f64]],
        bounds: IntRect::new(rect.x + dx, rect.y + dy, rect.w, rect.h).intersect(&view),
        shift: Some((dx, dy)),
    }
}

/// One selection layer's pixels for a refresh: its masks (and their warp while old), the tint
/// (premultiplied, target space) with its area, and the ants frame of the outline.
pub(crate) struct Painter {
    masks: Arc<LayerMasks>,
    warp: Option<Warp>,
    tint: Option<([f32; 4], IntRect)>,
    frame: Option<u32>,
    mode: RenderMode,
}

impl Painter {
    /// §2.9 steps 5–6 over device row `y`'s pixels `row`, which start at column `x0`: the tint
    /// (Highlight at α 56/255 × the interior mask, clipped to the canvas bounds), then the
    /// outline (not clipped, `outline_over`).
    pub(crate) fn paint(&self, y: i32, x0: i32, row: &mut [[f32; 4]]) {
        let (dx, dy) = match &self.warp {
            Some(w) if w.shift.is_none() => return self.paint_warped(w, y, x0, row),
            w => w.and_then(|w| w.shift).unwrap_or((0, 0)),
        };
        let (m, end) = (&self.masks, x0 + row.len() as i32);
        let rect = IntRect::new(m.rect.x + dx, m.rect.y + dy, m.rect.w, m.rect.h);
        if !(rect.y..rect.bottom()).contains(&y) {
            return;
        }
        let base = (y - rect.y) as usize * rect.w as usize;
        let cols = |a: IntRect| a.x.max(x0)..a.right().min(end);
        let mut over = |x: i32, s: [f32; 4]| blend_over(self.mode, &mut row[(x - x0) as usize], s);
        if let Some((tint, area)) = self.tint
            && (area.y..area.bottom()).contains(&y)
        {
            for x in cols(area) {
                let a = m.interior[base + (x - rect.x) as usize];
                if a != 0 {
                    over(x, tint.map(|c| c * a as f32 / 255.0));
                }
            }
        }
        if let Some(f) = self.frame {
            let dashed = m.dashed(f);
            for x in cols(rect) {
                let i = base + (x - rect.x) as usize;
                outline_over(self.mode, m, dashed, i, &mut row[(x - x0) as usize]);
            }
        }
    }

    /// `paint` for old masks under `w`: per pixel the tint, then the outline, of the old mask
    /// pixel under its centre's preimage.
    fn paint_warped(&self, w: &Warp, y: i32, x0: i32, row: &mut [[f32; 4]]) {
        let (m, end) = (&self.masks, x0 + row.len() as i32);
        let rect = m.rect;
        if !(w.bounds.y..w.bounds.bottom()).contains(&y) {
            return;
        }
        let [[a, b, c], [d, e, f]] = w.inv;
        let fy = y as f64 + 0.5;
        let dashed = self.frame.map(|f| m.dashed(f));
        let (l, r) = (rect.x as f64, rect.right() as f64);
        let (t, bt) = (rect.y as f64, rect.bottom() as f64);
        for x in w.bounds.x.max(x0)..w.bounds.right().min(end) {
            let fx = x as f64 + 0.5;
            let (sx, sy) = (a * fx + b * fy + c, d * fx + e * fy + f);
            // Inside the old rect (at or right of 0): truncation is the floor.
            if !(sx >= l && sx < r && sy >= t && sy < bt) {
                continue;
            }
            let (sx, sy) = (sx as i32, sy as i32);
            let i = (sy - rect.y) as usize * rect.w as usize + (sx - rect.x) as usize;
            let px = &mut row[(x - x0) as usize];
            if let Some((tint, area)) = self.tint
                && area.contains(x, y)
                && m.interior[i] != 0
            {
                let a = m.interior[i] as f32 / 255.0;
                blend_over(self.mode, px, tint.map(|c| c * a));
            }
            if let Some(dashed) = dashed {
                outline_over(self.mode, m, dashed, i, px);
            }
        }
    }
}

/// The outline at mask pixel `i` over `px`: source-over of [a_d, a_d, a_d, max(a_s, a_d)], white
/// dashes over black gaps.
fn outline_over(mode: RenderMode, m: &LayerMasks, dashed: &[u8], i: usize, px: &mut [f32; 4]) {
    let (s, d) = (m.solid[i], dashed[i]);
    if s | d != 0 {
        let d = d as f32 / 255.0;
        blend_over(mode, px, [d, d, d, (s as f32 / 255.0).max(d)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The runs one list alone covers: overlaps cancel, touching runs join, rows of one list only
    /// pass through.
    #[test]
    fn xor_runs_keep_what_one_list_alone_covers() {
        let r = |x, y, w| IntRect::new(x, y, w, 1);
        let a = [r(0, 0, 10), r(20, 0, 5), r(0, 1, 4), r(3, 3, 2)];
        let b = [r(5, 0, 10), r(0, 1, 4), r(4, 1, 2), r(7, 2, 1)];
        assert_eq!(
            xor_runs(&a, &b),
            [
                r(0, 0, 5),
                r(10, 0, 5),
                r(20, 0, 5),
                r(4, 1, 2),
                r(7, 2, 1),
                r(3, 3, 2)
            ]
        );
        assert_eq!(xor_runs(&a, &a), []);
        assert_eq!(
            mask_runs(&[0, 9, 9, 0, 9, 0, 0, 0], IntRect::new(5, 7, 4, 2)),
            [r(6, 7, 2), r(5, 8, 1)]
        );
    }

    /// A footer request must not scan heavy geometry while every mask worker is occupied.
    #[test]
    fn statistics_wait_for_workers_and_keep_the_previous_text() {
        let mut app = crate::app::AppWorkspace::default();
        app.documents.push(DocumentWorkspace::new(200, 150, 96.0));
        app.active = Some(0);
        let text = crate::app::selection_info_text;
        select(
            app.active_ws_mut().unwrap(),
            GraphicsPath::from_rect(&crate::util::geom::Rectangle::new(5.0, 6.0, 20.0, 10.0)),
        );
        let old = text(&app).unwrap();
        mask_builds(Arc::new(|| {}));
        let release = occupy_workers();
        select(app.active_ws_mut().unwrap(), ring(100.0, 75.0, 60.0, 50.0));
        let pending = text(&app);
        release.wait();
        assert_eq!(pending, Some(old), "heavy statistics ran in the UI call");
    }

    #[test]
    fn dense_dashes_match_a_bounded_visible_reference() {
        let rectangle = |left, right, y| Run {
            points: vec![[left, y], [right, y], [right, y + 16.0], [left, y + 16.0]],
            closed: true,
            start: 0.0,
        };
        for aa in [false, true] {
            for shift in [0.0, 1.0, 2.0, 3.5] {
                let render = |runs: &[Run]| {
                    let mut m = Mask::new(32, 32).unwrap();
                    stroke_runs(&mut m, runs, aa, Some((8.0, shift)), 1.0);
                    m.take()
                };
                // Each contour is below tiny-skia's limit; together they exceed 1 M dashes.
                // Reference dashes each bounded contour separately, then fills their union.
                let dense = [
                    rectangle(-1_000_000.0, 1_000_016.0, 5.5),
                    rectangle(-1_000_000.0, 1_000_016.0, 5.5),
                ];
                let bounded = [rectangle(-8.0, 40.0, 5.5), rectangle(-8.0, 40.0, 5.5)];
                let mut reference = PathBuilder::new();
                for r in &dense {
                    let mut p = PathBuilder::new();
                    p.move_to(r.points[0][0], r.points[0][1]);
                    for q in &r.points[1..] {
                        p.line_to(q[0], q[1]);
                    }
                    p.close();
                    let style =
                        StrokeDash::new(vec![4.0, 4.0], (8.0 - shift).rem_euclid(8.0) as f32)
                            .unwrap();
                    reference.push_path(&p.finish().unwrap().dash(&style, 1.0).unwrap());
                }
                let stroke = Stroke {
                    width: 1.0,
                    miter_limit: 10.0,
                    line_cap: LineCap::Butt,
                    line_join: LineJoin::Bevel,
                    dash: None,
                };
                let mut expected = Mask::new(32, 32).unwrap();
                expected.fill_path(
                    &reference.finish().unwrap().stroke(&stroke, 1.0).unwrap(),
                    FillRule::Winding,
                    aa,
                    Transform::identity(),
                );
                let expected = expected.take();
                assert!(expected.iter().any(|&a| a != 0));
                assert_eq!(render(&dense), expected, "closed union: {aa}, {shift}");
                let long = [Run {
                    points: vec![[-4_000_000.0, 8.5], [4_000_016.0, 8.5]],
                    closed: false,
                    start: 3.0,
                }];
                let short = [Run {
                    points: vec![[-8.0, 8.5], [40.0, 8.5]],
                    closed: false,
                    start: 3.0,
                }];
                assert_eq!(render(&long), render(&short), "long open: {aa}, {shift}");
                assert_eq!(
                    render(&[rectangle(-2_000_000.0, 2_000_016.0, 5.5)]),
                    render(&bounded[..1]),
                    "long closed: {aa}, {shift}"
                );
            }
        }
    }

    #[test]
    fn statistics_coalesce_reject_stale_results_and_clear_on_document_switch() {
        use crate::app::{AppWorkspace, MeasurementUnit, selection_info_text as text};
        let mut app = AppWorkspace::default();
        app.documents.push(DocumentWorkspace::new(200, 150, 96.0));
        app.active = Some(0);
        select(
            app.active_ws_mut().unwrap(),
            GraphicsPath::from_rect(&crate::util::geom::Rectangle::new(0.0, 0.0, 20.0, 10.0)),
        );
        let old = text(&app).unwrap();
        let (sent, received) = mpsc::channel();
        mask_builds(Arc::new(move || {
            let _ = sent.send(());
        }));
        let completed = || {
            received
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("worker repaint")
        };
        let release = occupy_workers();
        select(app.active_ws_mut().unwrap(), ring(100.0, 75.0, 60.0, 50.0));
        assert_eq!(text(&app), Some(old.clone()));
        for x in 1..=50 {
            let mut m = Matrix::identity();
            m.elements[0][2] = x as f64;
            app.active_ws_mut()
                .unwrap()
                .selection
                .set_interim_transform(m);
            assert_eq!(text(&app), Some(old.clone()));
        }
        STATISTICS.with_borrow(|s| {
            assert!(s.busy.load(Ordering::Acquire));
            assert!(
                s.queued.is_some(),
                "only the latest replacement is retained"
            );
        });
        release.wait();
        completed(); // The superseded active request ends without scanning.
        assert_eq!(text(&app), Some(old)); // Starts the one replacement.
        completed();
        let ws = app.active_ws().unwrap();
        assert_eq!(
            selection_statistics(ws),
            Some(ws.selection.get_statistics(&ws.document.get_bounds()))
        );
        let latest = text(&app).unwrap();

        // A worker really finishes B, but C supersedes it before the UI polls the result.
        select(app.active_ws_mut().unwrap(), ring(80.0, 60.0, 30.0, 20.0));
        assert_eq!(text(&app), Some(latest.clone()));
        completed();
        select(app.active_ws_mut().unwrap(), ring(70.0, 60.0, 20.0, 10.0));
        assert_eq!(text(&app), Some(latest));
        completed();
        let ws = app.active_ws().unwrap();
        assert_eq!(
            selection_statistics(ws),
            Some(ws.selection.get_statistics(&ws.document.get_bounds()))
        );

        let release = occupy_workers();
        select(app.active_ws_mut().unwrap(), ring(90.0, 70.0, 20.0, 10.0));
        assert!(text(&app).is_some());
        app.documents.push(DocumentWorkspace::new(200, 150, 96.0));
        app.active = Some(1);
        select(app.active_ws_mut().unwrap(), ring(100.0, 75.0, 60.0, 50.0));
        assert_eq!(text(&app), None, "another document's text must never show");
        app.active_ws_mut().unwrap().selection.reset();
        assert_eq!(text(&app), None, "deselect clears immediately");
        release.wait();
        completed();
        assert_eq!(text(&app), None, "a stale completion cannot undo deselect");
        STATISTICS.with_borrow(|s| assert!(s.queued.is_none()));

        select(
            app.active_ws_mut().unwrap(),
            GraphicsPath::from_rect(&crate::util::geom::Rectangle::new(0.0, 0.0, 20.0, 10.0)),
        );
        assert!(text(&app).unwrap().contains("Area: 200 pixels square"));
        app.active_ws_mut().unwrap().document.width = 5;
        assert!(
            text(&app).unwrap().contains("Area: 50 pixels square"),
            "document bounds key"
        );
        app.measurement_unit = MeasurementUnit::Inch;
        app.active_ws_mut().unwrap().document.resolution = 10.0;
        assert!(
            text(&app)
                .unwrap()
                .contains("size: 2.00in × 1.00in. Area: 0.50 inches square")
        );
        app.active_ws_mut().unwrap().document.resolution = 20.0;
        assert!(
            text(&app)
                .unwrap()
                .contains("size: 1.00in × 0.50in. Area: 0.13 inches square")
        );
    }

    /// Run explicitly in release: actual dense masks, all hardware/software/off frames, with
    /// a bounded reference containing one copy of each contour (odd repeats preserve fill).
    #[test]
    #[ignore = "release performance/memory measurement"]
    fn dense_masks_and_statistics_measurement() {
        use crate::app::{AppWorkspace, selection_info_text};
        use std::time::Instant;
        println!(
            "load {}",
            std::fs::read_to_string("/proc/loadavg")
                .unwrap_or_default()
                .trim()
        );
        println!("cpus {:?}", std::thread::available_parallelism());
        let mut app = AppWorkspace::default();
        app.documents.push(DocumentWorkspace::new(3840, 2160, 96.0));
        app.active = Some(0);
        let mut stencil = crate::util::bitvec2d::BitVector2D::new(3840, 2160);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for y in 0..2160 {
            for x in 0..3840 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                stencil.set(x, y, seed % 100 < 50);
            }
        }
        let path = crate::selection::path::path_from_stencil(
            &stencil,
            &crate::util::geom::Rectangle::new(0.0, 0.0, 3840.0, 2160.0),
        );
        println!(
            "statistics: {} contours, {} vertices",
            path.polygons.len(),
            path.polygons.iter().map(Vec::len).sum::<usize>()
        );
        select(app.active_ws_mut().unwrap(), path);
        let before = Instant::now();
        let exact_text = selection_info_text(&app).unwrap();
        println!(
            "prior synchronous UI call: {:.3} ms",
            before.elapsed().as_secs_f64() * 1000.0
        );
        let (sent, got) = mpsc::channel();
        mask_builds(Arc::new(move || {
            let _ = sent.send(());
        }));
        // A new revision with the same effective geometry, so exact output is independently known.
        let mut moved = Matrix::identity();
        moved.elements[0][2] = 1.0;
        app.active_ws_mut()
            .unwrap()
            .selection
            .set_interim_transform(moved);
        app.active_ws_mut()
            .unwrap()
            .selection
            .set_interim_transform(Matrix::identity());
        let before = Instant::now();
        assert_eq!(selection_info_text(&app), Some(exact_text.clone()));
        println!(
            "pending UI call: {:.3} ms",
            before.elapsed().as_secs_f64() * 1000.0
        );
        got.recv_timeout(std::time::Duration::from_secs(120))
            .expect("statistics repaint");
        assert_eq!(selection_info_text(&app), Some(exact_text));
        println!(
            "statistics complete: {:.3} ms",
            before.elapsed().as_secs_f64() * 1000.0
        );
        drop(app);

        let reference = vec![
            rect_poly(8.25, 6.5, 110.25, 90.5),
            rect_poly(20.5, 20.25, 80.5, 60.25),
            rect_poly(65.25, 30.5, 100.25, 75.5),
        ];
        let dense: Vec<_> = (0..20_001).flat_map(|_| reference.clone()).collect();
        let tt = t(1.25, [0.25, -1.5], [160, 120]);
        let rect = IntRect::new(0, 0, 160, 120);
        for quality in [SelectionQuality::HighQuality, SelectionQuality::Aliased] {
            let runs = outline_runs(
                &dense,
                |p| {
                    let d = device(p, &tt, quality);
                    [d[0] + 0.5, d[1] + 0.5]
                },
                160,
                120,
            );
            let length: f64 = runs.iter().filter(|r| r.closed).map(run_length).sum();
            assert!(
                length / 8.0 > 1e6,
                "actual closed group exceeds tiny-skia limit"
            );
            println!(
                "dense {quality:?}: {:.0} dashes, estimates {:?}",
                length / 8.0,
                fill_bytes(&dense, &runs, 8.0)
            );
            let before = Instant::now();
            let _ = std::fs::write("/proc/self/clear_refs", "5");
            let actual = POOL.as_ref().unwrap().install(|| {
                LayerMasks::new(&dense, &tt, quality, 8, rect, (true, true), &|| true).unwrap()
            });
            let expected =
                LayerMasks::new(&reference, &tt, quality, 8, rect, (true, true), &|| true).unwrap();
            assert_eq!(actual.interior, expected.interior);
            assert_eq!(actual.solid, expected.solid);
            for (mode, animations) in [(RenderMode::Software, true), (RenderMode::Hardware, false)]
            {
                let frames = SelectionRenderer::frames_for(mode, animations);
                let other = POOL.as_ref().unwrap().install(|| {
                    LayerMasks::new(&dense, &tt, quality, frames, rect, (false, true), &|| true)
                        .unwrap()
                });
                for f in 0..frames {
                    assert_eq!(
                        other.dashed[f as usize],
                        expected.dashed[(f * 8 / frames) as usize]
                    );
                }
            }
            assert_eq!(actual.dashed, expected.dashed);
            assert_ne!(actual.dashed[0], actual.dashed[1]);
            println!(
                "dense {quality:?} complete {:.3} s, {}",
                before.elapsed().as_secs_f64(),
                std::fs::read_to_string("/proc/self/status")
                    .unwrap()
                    .lines()
                    .find(|l| l.starts_with("VmHWM:"))
                    .unwrap_or("")
            );
        }
    }

    #[test]
    fn long_run_fallback_keeps_closed_joins_and_open_phase() {
        let paths = [
            // The outer loop returns to the start at the end of an on-dash; the final
            // short loop is entirely off. Equal endpoints must not create a closed join.
            vec![
                [5.5, 5.5],
                [13.5, 5.5],
                [13.5, 11.5],
                [5.5, 11.5],
                [5.5, 5.5],
                [6.0, 5.5],
                [6.0, 6.5],
                [5.5, 6.5],
            ],
            vec![[5.25, 6.5], [26.25, 6.5], [26.25, 18.5], [5.25, 18.5]],
            vec![
                [5.5, 5.5],
                [25.5, 5.5],
                [25.5, 25.5],
                [10.5, 25.5],
                [10.5, 10.5],
            ],
        ];
        for points in paths {
            for closed in [false, true] {
                for start in [0.0, 3.0, 17.25] {
                    let r = Run {
                        points: points.clone(),
                        closed,
                        start,
                    };
                    for shift in [0.0, 1.0, 2.0, 3.5, 5.0, 7.0] {
                        let pattern = (8.0, shift);
                        let mut fallback = PathBuilder::new();
                        dash_long_run(&r, pattern, &mut fallback);
                        let native = run_path(&r)
                            .unwrap()
                            .dash(&dash_style(pattern, start).unwrap(), 1.0)
                            .unwrap();
                        let stroke = Stroke {
                            width: 1.0,
                            miter_limit: 10.0,
                            line_cap: LineCap::Butt,
                            line_join: LineJoin::Bevel,
                            dash: None,
                        };
                        for aa in [false, true] {
                            let render = |p: &tiny_skia::Path| {
                                let mut m = Mask::new(32, 32).unwrap();
                                m.fill_path(
                                    &p.stroke(&stroke, 1.0).unwrap(),
                                    FillRule::Winding,
                                    aa,
                                    Transform::identity(),
                                );
                                m.take()
                            };
                            assert_eq!(
                                render(&fallback.clone().finish().unwrap()),
                                render(&native),
                                "closed {closed}, start {start}, shift {shift}, AA {aa}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn statistics_classification_uses_unclipped_transformed_rows() {
        use crate::app::{AppWorkspace, selection_info_text as text};
        let mut app = AppWorkspace::default();
        app.documents.push(DocumentWorkspace::new(20, 20, 96.0));
        app.active = Some(0);
        select(
            app.active_ws_mut().unwrap(),
            GraphicsPath::from_rect(&crate::util::geom::Rectangle::new(0.0, 0.0, 10.0, 2000.0)),
        );
        let mut m = Matrix::identity();
        m.elements[1][1] = 100.0;
        app.active_ws_mut()
            .unwrap()
            .selection
            .set_interim_transform(m);
        let (sent, got) = mpsc::channel();
        mask_builds(Arc::new(move || {
            let _ = sent.send(());
        }));
        let release = occupy_workers();
        assert_eq!(
            text(&app),
            None,
            "few vertices can still require many unclipped scans"
        );
        app.active_ws_mut().unwrap().selection.reset();
        select(
            app.active_ws_mut().unwrap(),
            GraphicsPath::from_rect(&crate::util::geom::Rectangle::new(1.0, 2.0, 3.0, 4.0)),
        );
        let expected =
            "Selection top left: 1, 2. Bounding rectangle size: 3 × 4. Area: 12 pixels square";
        assert_eq!(
            text(&app).as_deref(),
            Some(expected),
            "light geometry stays immediate with busy workers"
        );
        release.wait();
        got.recv_timeout(std::time::Duration::from_secs(30))
            .expect("cancelled request repaint");
        assert_eq!(text(&app).as_deref(), Some(expected));
    }

    fn t(scale: f64, origin: [f64; 2], viewport: [u32; 2]) -> ViewTransform {
        ViewTransform {
            scale,
            origin,
            viewport,
            ui_scale: 1.0,
        }
    }

    fn rect_poly(l: f64, tp: f64, r: f64, b: f64) -> Vec<[f64; 2]> {
        vec![[l, tp], [r, tp], [r, b], [l, b], [l, tp]]
    }

    /// §2.9 step 1: device bounds inflated by 1 px ∩ viewport; Aliased pixelates first.
    #[test]
    fn mask_rect_inflates_and_clips() {
        let p = [rect_poly(2.0, 3.0, 12.0, 8.0)];
        let tt = t(1.0, [0.0, 0.0], [20, 14]);
        assert_eq!(
            mask_rect(&p, &tt, SelectionQuality::HighQuality),
            IntRect::new(1, 2, 12, 7)
        );
        let tt = t(1.0, [0.4, 0.0], [20, 9]);
        assert_eq!(
            mask_rect(&p, &tt, SelectionQuality::HighQuality),
            IntRect::new(1, 2, 13, 7)
        );
        assert_eq!(
            mask_rect(&p, &tt, SelectionQuality::Aliased),
            IntRect::new(1, 2, 12, 7)
        );
        assert!(mask_rect(&[], &tt, SelectionQuality::Aliased).is_empty());
    }

    /// A polygon reaching far off the mask is stroked as its visible runs, each keeping its
    /// distance from the figure start: the dashes on screen match the whole-figure stroke.
    #[test]
    fn far_geometry_keeps_the_dash_phase() {
        let tt = t(1.0, [0.0, 0.0], [40, 12]);
        let rect = IntRect::new(0, 0, 40, 12);
        let near = [rect_poly(3.0, 2.0, 30.0, 9.0)];
        // The same top edge, but the figure continues 10⁶ px to the right and back.
        let far = [vec![[3.0, 2.0], [1e6, 2.0], [1e6, 9.0], [3.0, 9.0]]];
        let (a, b) = (
            outline_masks(&near, &tt, SelectionQuality::HighQuality, 8, rect),
            outline_masks(&far, &tt, SelectionQuality::HighQuality, 8, rect),
        );
        // Row 2 between the corners: the top edge, the same distance from the figure start.
        for f in 0..8 {
            assert_eq!(
                a.dashed[f][2 * 40 + 4..2 * 40 + 29],
                b.dashed[f][2 * 40 + 4..2 * 40 + 29],
                "frame {f}"
            );
            assert!(b.dashed[f][2 * 40 + 4..2 * 40 + 29].contains(&255));
        }
        // Two pieces, each clipped to within 2 px of the mask.
        let runs = outline_runs(&far, |v| v, 40, 12);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].points, [[3.0, 2.0], [42.0, 2.0]]);
        assert_eq!(runs[1].points[0], [42.0, 9.0]);
        // (10⁶ − 3) + 7 + (10⁶ − 42) along the figure.
        assert!(
            (runs[1].start - (2e6 - 38.0)).abs() < 1e-6,
            "{}",
            runs[1].start
        );
    }

    /// Geometries for the outline properties: a rectangle at fractional offsets, a 64-gon
    /// ellipse, an acute self-crossing lasso and a pixelated noise patch (a Magic Wand result).
    fn shapes() -> Vec<Vec<Vec<[f64; 2]>>> {
        let ellipse = (0..64)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 64.0;
                [40.0 + 31.3 * a.cos(), 30.0 + 21.7 * a.sin()]
            })
            .collect();
        let lasso = vec![
            [3.2, 2.1],
            [77.9, 9.3],
            [4.4, 11.0],
            [60.1, 55.7],
            [2.0, 57.5],
        ];
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut noise = vec![];
        for y in 0..24 {
            for x in 0..32 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                if seed.is_multiple_of(3) {
                    let (x, y) = (10.0 + x as f64, 8.0 + y as f64);
                    noise.push(rect_poly(x, y, x + 1.0, y + 1.0));
                }
            }
        }
        vec![
            vec![rect_poly(2.3, 3.7, 61.6, 40.2)],
            vec![ellipse],
            vec![lasso],
            noise,
        ]
    }

    /// The spans hold every pixel the solid outline or any dashed frame covers, sorted and
    /// disjoint, runs under `SPAN_GAP` apart merged; they fall back to `None` past the budget.
    #[test]
    fn spans_hold_every_pixel_an_ants_frame_draws() {
        let mut beyond_solid = 0;
        for polys in shapes() {
            for scale in [1.0, 0.37, 2.0, 4.6] {
                for quality in [SelectionQuality::HighQuality, SelectionQuality::Aliased] {
                    for (u, frames) in [(1.0, 8), (2.0, 8), (1.0, 4)] {
                        let tt = ViewTransform {
                            ui_scale: u,
                            ..t(scale, [1.3, 0.6], [200, 150])
                        };
                        let rect = mask_rect(&polys, &tt, quality);
                        let parts = (false, true);
                        let m =
                            LayerMasks::new(&polys, &tt, quality, frames, rect, parts, &|| true)
                                .unwrap();
                        let all: Vec<&[u8]> = std::iter::once(&m.solid[..])
                            .chain((0..frames).map(|f| m.dashed(f)))
                            .collect();
                        let spans = m.spans_within(&all, usize::MAX).unwrap();
                        let mut inside = vec![false; m.solid.len()];
                        for (k, s) in spans.iter().enumerate() {
                            assert_eq!(s.h, 1);
                            if let Some(n) = spans.get(k + 1) {
                                assert!((s.y, s.right() + SPAN_GAP as i32) <= (n.y, n.x));
                            }
                            let base = (s.y - rect.y) as usize * rect.w as usize;
                            for x in s.x..s.right() {
                                inside[base + (x - rect.x) as usize] = true;
                            }
                        }
                        for (i, &v) in all.iter().flat_map(|m| m.iter().enumerate()) {
                            assert!(v == 0 || inside[i], "s {scale} {quality:?} u {u}");
                        }
                        beyond_solid += all[1..]
                            .iter()
                            .flat_map(|d| d.iter().zip(&m.solid))
                            .filter(|&(&d, &s)| d != 0 && s == 0)
                            .count();
                        // Over budget: a full refresh instead.
                        let covered: usize = spans.iter().map(|s| s.w as usize).sum();
                        let cost = covered + SPAN_GAP * spans.len();
                        assert_eq!(m.spans_within(&all, cost), Some(spans.clone()));
                        assert_eq!(m.spans_within(&all, cost - 1), None);
                        // What the build keeps (`find_spans`): these within half the rect.
                        let half = rect.w as usize * rect.h as usize / 2;
                        assert_eq!(m.spans, (cost <= half).then_some(spans));
                    }
                }
            }
        }
        // The dashed frames do reach past the solid outline somewhere.
        assert!(beyond_solid > 0);
    }

    /// The interior is an aliased even-odd fill: a hole stays empty.
    #[test]
    fn interior_is_even_odd_and_aliased() {
        let tt = t(1.0, [0.0, 0.0], [10, 10]);
        let p = [rect_poly(1.0, 1.0, 9.0, 9.0), rect_poly(3.0, 3.0, 6.0, 6.0)];
        let m = outline_masks(
            &p,
            &tt,
            SelectionQuality::HighQuality,
            1,
            IntRect::new(0, 0, 10, 10),
        );
        let row4: Vec<u8> = m.interior[40..50].to_vec();
        assert_eq!(row4, [0, 255, 255, 0, 0, 0, 255, 255, 255, 0]);
    }

    /// `LayerMasks::new` stops once `live` turns false: before any mask, or between masks.
    #[test]
    fn a_superseded_build_stops_between_masks() {
        let polys = [rect_poly(2.3, 3.7, 61.6, 40.2)];
        let tt = t(1.0, [0.0, 0.0], [80, 60]);
        let rect = mask_rect(&polys, &tt, SelectionQuality::HighQuality);
        let build = |n: usize| {
            let calls = AtomicU64::new(0);
            let live = || calls.fetch_add(1, Ordering::Relaxed) < n as u64;
            LayerMasks::new(
                &polys,
                &tt,
                SelectionQuality::HighQuality,
                8,
                rect,
                (true, true),
                &live,
            )
        };
        // 10 masks: the interior, the solid outline, 8 dashed frames.
        assert!(build(10).is_some());
        assert!(build(9).is_none());
        assert!(build(0).is_none());
    }

    use crate::document::workspace::DocumentWorkspace;
    use crate::render::Renderer;
    use crate::render::display::DisplayTarget;
    use crate::selection::CombineMode;
    use crate::util::geom::{Point, Rectangle};
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    fn path(polys: Vec<Vec<[f64; 2]>>) -> GraphicsPath {
        let polys = polys
            .into_iter()
            .map(|p| p.into_iter().map(|[x, y]| Point::new(x, y)));
        GraphicsPath {
            polygons: polys.map(Iterator::collect).collect(),
        }
    }

    /// Over `LIGHT_POINTS` vertices with a dense outline: a 1-in-3 noise patch of unit squares
    /// from (x, y) (a Magic Wand result).
    fn noise(x: f64, y: f64) -> GraphicsPath {
        noise_cells(x, y, 64, 48)
    }

    /// `noise` over `cols × rows` cells.
    fn noise_cells(x: f64, y: f64, cols: usize, rows: usize) -> GraphicsPath {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut polys = vec![];
        for j in 0..rows {
            for i in 0..cols {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                if seed.is_multiple_of(3) {
                    let (x, y) = (x + i as f64, y + j as f64);
                    polys.push(rect_poly(x, y, x + 1.0, y + 1.0));
                }
            }
        }
        path(polys)
    }

    /// Over `LIGHT_POINTS` vertices with a thin outline: a 5000-gon ellipse.
    fn ring(cx: f64, cy: f64, rx: f64, ry: f64) -> GraphicsPath {
        let a = |i: usize| i as f64 * std::f64::consts::TAU / 5000.0;
        let p = (0..5000).map(|i| [cx + rx * a(i).cos(), cy + ry * a(i).sin()]);
        path(vec![p.collect()])
    }

    fn select(ws: &mut DocumentWorkspace, p: GraphicsPath) {
        ws.selection.set_continuation_path(p, CombineMode::Replace);
        ws.selection.commit_continuation();
    }

    /// The overlays over a fixed base, painted (`overlay_painter`) at every ants frame, RGBA8;
    /// and the ants spans.
    fn painted(
        ws: &mut DocumentWorkspace,
        t: &ViewTransform,
        mode: RenderMode,
    ) -> (Vec<Vec<u8>>, Option<Vec<IntRect>>) {
        let [w, h] = t.viewport;
        let all = IntRect::new(0, 0, w, h);
        let mut base = DisplayTarget::new(w, h, mode);
        base.fill(all, [40, 90, 200, 255]);
        base.fill(IntRect::new(20, 10, w / 2, h / 2), [250, 250, 245, 255]);
        let images = (0..SelectionRenderer::frames_for(mode, true)).map(|f| {
            ws.surface_box.set_ants_frame(f);
            let mut out = vec![0u8; (w * h * 4) as usize];
            let paint = ws.surface_box.overlay_painter(ws, t, mode);
            let px = out.as_chunks_mut().0;
            base.write_rgba8_over(all, None, px, |p: [u8; 4]| p, paint);
            out
        });
        let images = images.collect();
        (images, ws.surface_box.overlay_spans(ws, t))
    }

    /// Polls `mask_builds` as the canvas view does each frame until its stamp reaches `stamp`.
    fn landed(wake: &Arc<dyn Fn() + Send + Sync>, stamp: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let now = mask_builds(wake.clone());
            if now >= stamp {
                return now;
            }
            assert!(Instant::now() < deadline, "the masks never landed");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Task 25q requirement 6: complex selections' masks built on the workers give the
    /// synchronous build's pixels and spans, byte for byte, at every ants frame: a noise patch and
    /// a 5000-gon under a crossing live layer, tinted, at a fractional zoom, HighQuality and
    /// Aliased, in both modes. Nothing is drawn before the first masks land.
    #[test]
    fn worker_masks_match_the_synchronous_ones() {
        let t = ViewTransform {
            scale: 1.7,
            origin: [3.4, 2.2],
            viewport: [240, 170],
            ui_scale: 1.0,
        };
        let combos = [
            (RenderMode::Hardware, SelectionQuality::HighQuality),
            (RenderMode::Hardware, SelectionQuality::Aliased),
            (RenderMode::Software, SelectionQuality::HighQuality),
            (RenderMode::Software, SelectionQuality::Aliased),
        ];
        fn scene() -> DocumentWorkspace {
            let mut ws = DocumentWorkspace::new(120, 90, 96.0);
            let mut p = noise(10.3, 20.6);
            p.polygons.extend(ring(80.0, 45.0, 33.3, 30.1).polygons);
            select(&mut ws, p);
            ws.selection_renderer_mut().tinting = true;
            let live = Renderer::Selection(SelectionRenderer {
                path: Some(ring(60.0, 40.0, 50.0, 20.0)),
                ..Default::default()
            });
            ws.surface_box.add_renderer(live);
            ws
        }
        fn set(ws: &mut DocumentWorkspace, mode: RenderMode, quality: SelectionQuality) {
            for (_, r) in &mut ws.surface_box.renderers {
                if let Renderer::Selection(s) = r {
                    s.quality = quality;
                    s.frames = SelectionRenderer::frames_for(mode, true);
                }
            }
        }
        // Synchronous: a thread that never called `mask_builds`.
        let reference = std::thread::spawn(move || {
            let mut ws = scene();
            combos.map(|(mode, quality)| {
                set(&mut ws, mode, quality);
                painted(&mut ws, &t, mode)
            })
        });
        let reference = reference.join().unwrap();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut ws = scene();
        for (i, (mode, quality)) in combos.into_iter().enumerate() {
            set(&mut ws, mode, quality);
            let stamp = mask_builds(wake.clone());
            // Both layers' builds start here.
            let bounds = ws.surface_box.overlay_bounds(&ws, &t);
            if i == 0 {
                assert_eq!(bounds, None, "nothing older to draw meanwhile");
            }
            landed(&wake, stamp + 2);
            let (images, spans) = painted(&mut ws, &t, mode);
            assert_eq!(spans, reference[i].1, "{mode:?} {quality:?}");
            for (f, image) in images.iter().enumerate() {
                assert!(
                    *image == reference[i].0[f],
                    "{mode:?} {quality:?} frame {f}"
                );
            }
        }
    }

    /// §2.9 step 1 "Meanwhile the old masks are drawn": while a complex selection's masks build,
    /// the layer's previous masks of the same document show (any parts they lack left out, their
    /// spans kept inside a smaller viewport), another document's never. A simple selection builds
    /// in the frame; the complex build it supersedes stops before its masks, and a superseded
    /// build that finished anyway never replaces the newer masks.
    #[test]
    fn old_masks_show_until_the_new_ones_land() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let woke = |n: usize| {
            let deadline = Instant::now() + Duration::from_secs(60);
            while wakes.load(Ordering::SeqCst) < n {
                assert!(Instant::now() < deadline, "the build never ended");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        let stamp = mask_builds(wake.clone());
        let (big, small) = (
            t(1.0, [0.0, 0.0], [200, 150]),
            t(1.0, [0.0, 0.0], [160, 120]),
        );
        let bounds = |ws: &DocumentWorkspace, t| ws.surface_box.overlay_bounds(ws, t);
        let mut ws = DocumentWorkspace::new(200, 150, 96.0);
        // A, outlined and untinted.
        select(&mut ws, ring(50.0, 40.0, 30.0, 20.0));
        assert_eq!(bounds(&ws, &big), None);
        let stamp = landed(&wake, stamp + 1);
        let a = bounds(&ws, &big).expect("A");
        let (shown, _) = painted(&mut ws, &big, RenderMode::Hardware);
        // B, tinted: A meanwhile, untinted (it has no interior), until B lands.
        ws.selection_renderer_mut().tinting = true;
        select(&mut ws, ring(140.0, 100.0, 40.0, 30.0));
        assert_eq!(bounds(&ws, &big), Some(a));
        assert!(painted(&mut ws, &big, RenderMode::Hardware).0 == shown);
        let stamp = landed(&wake, stamp + 1);
        let b = bounds(&ws, &big).expect("B");
        assert_ne!(b, a);
        assert!(painted(&mut ws, &big, RenderMode::Hardware).0 != shown);
        // A smaller viewport: B meanwhile, its spans clipped to it.
        let view = IntRect::new(0, 0, 160, 120);
        let spans = ws.surface_box.overlay_spans(&ws, &small).expect("thin");
        assert!(!spans.is_empty());
        assert!(spans.iter().all(|s| s.intersect(&view) == *s));
        assert!(b.right() > 160 && b.bottom() > 120);
        let stamp = landed(&wake, stamp + 1);
        // Another document: none of the first one's masks meanwhile. Its tint alone (a selection
        // tool's drag), then outlined: the tint meanwhile, without the outline it lacks.
        let mut other = DocumentWorkspace::new(200, 150, 96.0);
        let layer = other.selection_renderer_mut();
        (layer.tinting, layer.outline) = (true, false);
        select(&mut other, ring(100.0, 75.0, 50.0, 40.0));
        assert_eq!(bounds(&other, &small), None);
        let stamp = landed(&wake, stamp + 1);
        let (tint, _) = painted(&mut other, &small, RenderMode::Hardware);
        other.selection_renderer_mut().outline = true;
        select(&mut other, ring(90.0, 70.0, 50.0, 40.0));
        assert!(painted(&mut other, &small, RenderMode::Hardware).0 == tint);
        let stamp = landed(&wake, stamp + 1);
        // D starts behind busy workers and D′ waits for it (one build at a time per layer); then
        // a simple selection E shows at once, drops D′, and D stops before its masks.
        let busy = occupy_workers;
        let queued = || BUILDS.with_borrow(|b| b.slots[0].queued.is_some());
        let release = busy();
        let ended = wakes.load(Ordering::SeqCst);
        select(&mut ws, ring(100.0, 75.0, 60.0, 50.0));
        bounds(&ws, &small);
        assert!(!queued());
        select(&mut ws, ring(100.0, 75.0, 50.0, 50.0));
        bounds(&ws, &small);
        assert!(queued(), "D′ waits for D");
        select(
            &mut ws,
            GraphicsPath::from_rect(&Rectangle::new(10.0, 10.0, 30.0, 20.0)),
        );
        let e = Some(IntRect::new(9, 9, 32, 22));
        assert_eq!(bounds(&ws, &small), e);
        assert!(!queued(), "E drops D′");
        release.wait();
        woke(ended + 1);
        let sent = BUILDS.with_borrow(|b| b.slots[0].done.1.try_recv().is_ok());
        assert!(!sent, "a superseded build made its masks");
        // D″ waits for D‴; once D‴ ends (superseded at once), the next frame starts D″.
        let release = busy();
        let ended = wakes.load(Ordering::SeqCst);
        select(&mut ws, ring(100.0, 75.0, 60.0, 50.0));
        bounds(&ws, &small);
        select(&mut ws, ring(80.0, 60.0, 30.0, 20.0));
        bounds(&ws, &small);
        assert!(queued());
        release.wait();
        woke(ended + 1);
        let stamp = landed(&wake, stamp + 1);
        assert_eq!(bounds(&ws, &small), Some(IntRect::new(49, 39, 62, 42)));
        // F finishes while G, a simple selection, supersedes it: F never shows.
        let ended = wakes.load(Ordering::SeqCst);
        select(&mut ws, ring(90.0, 70.0, 50.0, 40.0));
        bounds(&ws, &small);
        woke(ended + 1);
        select(
            &mut ws,
            GraphicsPath::from_rect(&Rectangle::new(20.0, 10.0, 30.0, 20.0)),
        );
        let g = Some(IntRect::new(19, 9, 32, 22));
        assert_eq!(bounds(&ws, &small), g);
        assert_eq!(mask_builds(wake.clone()), stamp);
        assert_eq!(bounds(&ws, &small), g);
    }

    /// Fix round 1 (M1, I3a): the light test counts vertices and the device rows the edges cross
    /// (a 4096-vertex full-height zigzag builds on the workers, unless it is drawn tiny), and in
    /// Replace mode only the continuation (a rectangle dragged over a complex selection stays in
    /// the frame).
    #[test]
    fn light_geometry_has_few_vertices_and_short_edges() {
        let at = |scale, origin| t(scale, origin, [3840, 2160]);
        let k4 = at(1.0, [0.0, 0.0]);
        let poly = |p: Vec<[f64; 2]>| path(vec![p]);
        let all = poly(rect_poly(0.0, 0.0, 3840.0, 2160.0));
        assert!(!heavy([&all], &k4));
        // Round 2 (N1): rows count inside the viewport only. A 4000 × 3000 photo's rectangle at
        // 600 % (18000 px tall), and a 1000 × 20000 image's at 100 %, stay light.
        let photo = poly(rect_poly(0.0, 0.0, 4000.0, 3000.0));
        assert!(!heavy([&photo], &at(6.0, [-5000.0, -4000.0])));
        assert!(!heavy([&poly(rect_poly(0.0, 0.0, 1000.0, 20000.0))], &k4));
        let dots = path(
            (0..819)
                .map(|i| rect_poly(i as f64, 0.0, i as f64 + 0.5, 0.5))
                .collect(),
        );
        assert!(
            !heavy([&dots], &k4) && heavy([&dots, &dots], &k4),
            "4095, 8190 vertices"
        );
        let zigzag: Vec<[f64; 2]> = (0..4096)
            .map(|i| [i as f64 * 0.9375, if i % 2 == 0 { 0.0 } else { 2160.0 }])
            .collect();
        let zigzag = poly(zigzag);
        assert!(heavy([&zigzag], &k4));
        assert!(
            !heavy([&zigzag], &at(0.003, [0.0, 0.0])),
            "drawn 6.5 rows high"
        );
        assert!(
            !heavy([&zigzag], &at(1.0, [0.0, 2154.0])),
            "6 rows of it in view"
        );
        let nan = poly(vec![[0.0, f64::NAN], [1.0, 1.0], [0.0, 1.0]]);
        assert!(heavy([&nan], &k4));
        let mut s = crate::selection::Selection::default();
        s.set_continuation_path(noise(0.0, 0.0), CombineMode::Replace);
        s.commit_continuation();
        assert!(heavy(s.sources(), &k4));
        s.set_continuation(&Rectangle::new(1.0, 1.0, 9.0, 9.0), CombineMode::Replace);
        assert!(!heavy(s.sources(), &k4));
        s.set_continuation(&Rectangle::new(1.0, 1.0, 9.0, 9.0), CombineMode::Union);
        assert!(heavy(s.sources(), &k4));
    }

    /// Fix round 1 (I1): a fill waits while the bytes under way and its own exceed the limit, and
    /// is admitted at once when nothing else is under way, however large.
    #[test]
    fn fills_wait_for_room_in_the_byte_gate() {
        let gate = Arc::new(Gate {
            used: Mutex::new(0),
            turn: Condvar::new(),
        });
        drop(gate.admit(5, 3));
        let first = gate.admit(2, 3);
        let (sent, got) = mpsc::channel();
        let g = gate.clone();
        let waiter = std::thread::spawn(move || {
            let p = g.admit(2, 3);
            sent.send(()).unwrap();
            drop(p);
        });
        assert!(
            got.recv_timeout(Duration::from_millis(200)).is_err(),
            "admitted over the limit"
        );
        let small = gate.admit(1, 3);
        drop((first, small));
        got.recv_timeout(Duration::from_secs(10))
            .expect("admitted once room was made");
        waiter.join().unwrap();
        assert_eq!(*gate.used.lock().unwrap(), 0);
    }

    /// Fix round 1 (I1): a worker fill admitted after waiting at the byte gate looks again, so a
    /// build superseded meanwhile stops there instead of filling. Builds in the frame never wait
    /// at the gate, even when it is full.
    #[test]
    fn a_fill_waiting_at_the_gate_stops_once_superseded() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = wakes.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        mask_builds(wake.clone());
        let tt = t(1.0, [0.0, 0.0], [200, 150]);
        let mut ws = DocumentWorkspace::new(200, 150, 96.0);
        // Two masks (the solid outline and one dashed frame), both reaching the full gate.
        ws.selection_renderer_mut().frames = 1;
        let full = FILLS.admit(FILL_BYTES, FILL_BYTES);
        let ended = wakes.load(Ordering::SeqCst);
        select(&mut ws, ring(100.0, 75.0, 60.0, 50.0));
        ws.surface_box.overlay_bounds(&ws, &tt);
        std::thread::sleep(Duration::from_millis(300));
        select(
            &mut ws,
            GraphicsPath::from_rect(&Rectangle::new(10.0, 10.0, 30.0, 20.0)),
        );
        let e = Some(IntRect::new(9, 9, 32, 22));
        assert_eq!(ws.surface_box.overlay_bounds(&ws, &tt), e);
        drop(full);
        let deadline = Instant::now() + Duration::from_secs(60);
        while wakes.load(Ordering::SeqCst) <= ended {
            assert!(Instant::now() < deadline, "the build never ended");
            std::thread::sleep(Duration::from_millis(1));
        }
        let sent = BUILDS.with_borrow(|b| b.slots[0].done.1.try_recv().is_ok());
        assert!(!sent, "a superseded fill went on after the gate");
    }

    #[test]
    fn dense_and_long_dashes_reserve_real_fill_memory() {
        let run = |closed| Run {
            points: vec![[0.0, 0.0], [5e6, 0.0]],
            closed,
            start: 0.0,
        };
        let closed = fill_bytes(&[], &[run(true)], 8.0);
        let open = fill_bytes(&[], &[run(false)], 8.0);
        assert!(
            closed[2] >= 1_250_000 * 512,
            "every dense dash needs memory"
        );
        assert!(
            open[2] >= 625_000 * 512,
            "long edges are not priced by vertices alone"
        );
        assert!(closed[2] > open[2]);
    }

    struct WorkerBlock {
        release: Arc<std::sync::Barrier>,
        _turn: std::sync::MutexGuard<'static, ()>,
    }

    impl WorkerBlock {
        fn wait(self) {}
    }

    impl Drop for WorkerBlock {
        fn drop(&mut self) {
            self.release.wait();
        }
    }

    /// Serialize pool blockers so concurrent tests cannot each hold half the worker threads.
    /// Dropping the guard also releases them after a failed assertion.
    fn occupy_workers() -> WorkerBlock {
        static TURN: Mutex<()> = Mutex::new(());
        let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let pool = POOL.as_ref().expect("the mask pool");
        let gate = || Arc::new(std::sync::Barrier::new(MASK_THREADS + 1));
        let (started, release) = (gate(), gate());
        for _ in 0..MASK_THREADS {
            let (started, release) = (started.clone(), release.clone());
            pool.spawn(move || {
                started.wait();
                release.wait();
            });
        }
        started.wait();
        WorkerBlock {
            release,
            _turn: turn,
        }
    }

    /// Round 2 (N1): a Replace rectangle over a 4000 × 3000 photo at 600 % (18000 device px
    /// tall, far past the viewport) builds in the frame: it shows at once with every worker busy.
    #[test]
    fn a_zoomed_in_rectangle_shows_in_the_frame() {
        mask_builds(Arc::new(|| {}));
        let mut ws = DocumentWorkspace::new(4000, 3000, 96.0);
        let tt = t(6.0, [-5000.0, -4000.0], [800, 600]);
        let release = occupy_workers();
        let r = Rectangle::new(100.0, 100.0, 3800.0, 2800.0);
        ws.selection.set_continuation(&r, CombineMode::Replace);
        let shown = ws.surface_box.overlay_bounds(&ws, &tt);
        release.wait();
        assert_eq!(shown, Some(IntRect::new(0, 0, 800, 600)));
    }

    /// The overlays at ants frame 0 over a uniform base, one RGBA8 pixel per entry.
    fn overlay(ws: &mut DocumentWorkspace, t: &ViewTransform) -> Vec<[u8; 4]> {
        overlay_at(ws, t, 0)
    }

    /// `overlay` at ants frame `frame`.
    fn overlay_at(ws: &mut DocumentWorkspace, t: &ViewTransform, frame: u32) -> Vec<[u8; 4]> {
        overlay_on(ws, t, frame, BASE)
    }

    /// `overlay_at` over a uniform `color` base.
    fn overlay_on(
        ws: &mut DocumentWorkspace,
        t: &ViewTransform,
        frame: u32,
        color: [u8; 4],
    ) -> Vec<[u8; 4]> {
        let [w, h] = t.viewport;
        let all = IntRect::new(0, 0, w, h);
        let mut base = DisplayTarget::new(w, h, RenderMode::Hardware);
        base.fill(all, color);
        ws.surface_box.set_ants_frame(frame);
        let mut out = vec![[0u8; 4]; (w * h) as usize];
        let paint = ws.surface_box.overlay_painter(ws, t, RenderMode::Hardware);
        base.write_rgba8_over(all, None, &mut out, |p: [u8; 4]| p, paint);
        out
    }

    const BASE: [u8; 4] = [40, 90, 200, 255];

    /// §2.9 step 1 under Task 27's moves (fix round 1, I2): while a complex selection's masks
    /// rebuild, the old ones follow the view and the selection's transform every frame. Whole
    /// pixel moves and scrolls draw them shifted byte for byte (at 100 % and 200 %); a quarter
    /// turn about a pixel corner maps pixel centres onto pixel centres, so the general
    /// nearest-neighbour warp is exact too. Shifted masks keep their spans, shifted (Task 27p);
    /// other warps give none (their whole bounds refresh). Committing the move, then adding to
    /// the selection, leaves them where they are.
    #[test]
    fn old_masks_follow_moves_and_scrolls() {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        // Round 2 (N2): the patch stays inside the 400-px view through every move and scroll at
        // 200 % (device 39..325 × 66..277), so no build clips it differently mid-test.
        let n = 400;
        let view = |scale, origin| t(scale, origin, [n, n]);
        let mut ws = DocumentWorkspace::new(n, n, 96.0);
        ws.selection_renderer_mut().tinting = true;
        let patch = noise_cells(20.0, 40.0, 128, 96);
        assert!(patch.polygons.iter().map(Vec::len).sum::<usize>() >= 20_000);
        let (n, side) = (n as i32, n as usize);
        // `image` with each pixel taken from `from(x, y)` (the base outside it).
        let remap = |image: &[[u8; 4]], from: &dyn Fn(i32, i32) -> (i32, i32)| -> Vec<[u8; 4]> {
            let px = |(x, y): (i32, i32)| {
                let inside = (0..n).contains(&x) && (0..n).contains(&y);
                if inside {
                    image[y as usize * side + x as usize]
                } else {
                    BASE
                }
            };
            (0..n * n).map(|i| px(from(i % n, i / n))).collect()
        };
        for scale in [1.0, 2.0] {
            let t0 = view(scale, [0.0, 0.0]);
            let stamp = mask_builds(wake.clone());
            select(&mut ws, patch.clone());
            ws.surface_box.overlay_bounds(&ws, &t0);
            let stamp = landed(&wake, stamp + 1);
            let still = overlay(&mut ws, &t0);
            let landed_at = ws.surface_box.overlay_bounds(&ws, &t0).unwrap();
            let shifted = |dx: i32, dy: i32| {
                let at = IntRect::new(landed_at.x + dx, landed_at.y + dy, landed_at.w, landed_at.h);
                Some(at.intersect(&IntRect::new(0, 0, n as u32, n as u32)))
            };
            // A Move drag, a frame per px, no build landing: shifted every frame.
            for i in 1..=6 {
                let mut m = Matrix::identity();
                m.translate(i as f64, -((i / 2) as f64));
                ws.selection.set_interim_transform(m);
                let d = (i * scale as i32, -(i / 2) * scale as i32);
                assert_eq!(ws.surface_box.overlay_bounds(&ws, &t0), shifted(d.0, d.1));
                assert_eq!(ws.surface_box.overlay_spans(&ws, &t0), None);
                let moved = remap(&still, &|x, y| (x - d.0, y - d.1));
                assert!(overlay(&mut ws, &t0) == moved, "scale {scale} move {i}");
            }
            // On, as the canvas view runs: builds land on the way, the ants follow every frame.
            for i in 7..=14 {
                let mut m = Matrix::identity();
                m.translate(i as f64, -((i / 2) as f64));
                ws.selection.set_interim_transform(m);
                mask_builds(wake.clone());
                let d = (i * scale as i32, -(i / 2) * scale as i32);
                assert_eq!(ws.surface_box.overlay_bounds(&ws, &t0), shifted(d.0, d.1));
                std::thread::sleep(Duration::from_millis(4));
            }
            // A scroll on top: attached. (The move ended at (14, −7) image px.)
            let d = (14 * scale as i32 - 5, -7 * scale as i32 + 4);
            let t1 = view(scale, [-5.0, 4.0]);
            assert_eq!(ws.surface_box.overlay_bounds(&ws, &t1), shifted(d.0, d.1));
            let scrolled = remap(&still, &|x, y| (x - d.0, y - d.1));
            assert!(overlay(&mut ws, &t1) == scrolled, "scale {scale} scroll");
            // Committed, then a rectangle added: they stay where they are.
            ws.selection.commit_interim_transform();
            let add = Rectangle::new(5.0, 5.0, 10.0, 10.0);
            ws.selection.set_continuation(&add, CombineMode::Union);
            assert_eq!(ws.surface_box.overlay_bounds(&ws, &t1), shifted(d.0, d.1));
            landed(&wake, stamp + 1);
            ws.selection.reset();
        }
        // A quarter turn about (124, 112): device px (x, y) comes from (y + 12, 236 − x − 1).
        let t0 = view(1.0, [0.0, 0.0]);
        let stamp = mask_builds(wake.clone());
        select(&mut ws, patch);
        ws.surface_box.overlay_bounds(&ws, &t0);
        landed(&wake, stamp + 1);
        let still = overlay(&mut ws, &t0);
        let mut m = Matrix::identity();
        m.rotate_at(90.0, Point::new(124.0, 112.0));
        ws.selection.set_interim_transform(m);
        let turned = remap(&still, &|x, y| (y + 12, 236 - x - 1));
        assert!(overlay(&mut ws, &t0) == turned, "quarter turn");
        assert_eq!(ws.surface_box.overlay_spans(&ws, &t0), None);
        // A thin outline has spans. Its old masks shifted keep them, shifted, and two ants
        // frames differ only there; moved by a fraction, none.
        let stamp = mask_builds(wake.clone());
        select(&mut ws, ring(128.0, 128.0, 60.0, 40.0));
        ws.surface_box.overlay_bounds(&ws, &t0);
        landed(&wake, stamp + 1);
        let spans = ws.surface_box.overlay_spans(&ws, &t0).expect("spans");
        assert!(!spans.is_empty());
        let mut m = Matrix::identity();
        m.translate(3.0, 0.0);
        ws.selection.set_interim_transform(m);
        let moved: Vec<IntRect> = spans
            .iter()
            .map(|s| IntRect::new(s.x + 3, s.y, s.w, s.h))
            .collect();
        assert_eq!(ws.surface_box.overlay_spans(&ws, &t0), Some(moved.clone()));
        let (a, b) = (overlay_at(&mut ws, &t0, 0), overlay_at(&mut ws, &t0, 3));
        assert!(a != b);
        for (i, (p, q)) in a.iter().zip(&b).enumerate() {
            let (x, y) = ((i % side) as i32, (i / side) as i32);
            assert!(
                p == q || moved.iter().any(|s| s.contains(x, y)),
                "({x}, {y})"
            );
        }
        m.translate(0.5, 0.0);
        ws.selection.set_interim_transform(m);
        assert_eq!(ws.surface_box.overlay_spans(&ws, &t0), None);
    }

    /// Task 27p (R77): while a complex selection only moves (its interim transform, every frame),
    /// its masks are not rebuilt: the shown ones follow it, and each `mask_builds` asks for
    /// another frame. Once a frame passes without a move, the next `mask_builds` starts the worker
    /// build, which lands with a new stamp: the same masks as a fresh build. Any other change
    /// rebuilds at once; without `mask_builds` nothing is held.
    #[test]
    fn a_moving_selection_rebuilds_once_it_stops() {
        let tt = t(1.0, [0.0, 0.0], [120, 90]);
        let patch = noise_cells(10.0, 8.0, 96, 72);
        let mut ws = DocumentWorkspace::new(120, 90, 96.0);
        select(&mut ws, patch.clone());
        assert!(heavy(ws.selection.sources(), &tt));
        let warp = |ws: &DocumentWorkspace| ws.selection_renderer().masks(ws, &tt).map(|(_, w)| w);
        assert!(warp(&ws).is_some_and(|w| w.is_none()));
        // No set-up on this thread yet: a move rebuilds in the frame.
        let mut m = Matrix::identity();
        m.translate(1.0, 0.0);
        ws.selection.set_interim_transform(m);
        assert!(warp(&ws).is_some_and(|w| w.is_none()));
        let b0 = ws.surface_box.overlay_bounds(&ws, &tt).unwrap();
        let wakes = Arc::new(AtomicU64::new(0));
        let w = wakes.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        });
        let stamp = mask_builds(wake.clone());
        // A frame per move: the selection moves, the overlays draw, the next frame polls.
        for i in 2..=5 {
            let mut m = Matrix::identity();
            m.translate(i as f64, 1.0);
            ws.selection.set_interim_transform(m);
            assert!(warp(&ws).is_some_and(|w| w.is_some_and(|w| w.shift == Some((i - 1, 1)))));
            let moved = IntRect::new(b0.x + i - 1, b0.y + 1, b0.w, b0.h);
            assert_eq!(ws.surface_box.overlay_bounds(&ws, &tt), Some(moved));
            assert_eq!(
                mask_builds(wake.clone()),
                stamp,
                "nothing lands while it moves"
            );
        }
        assert_eq!(wakes.load(Ordering::Relaxed), 4);
        // It stopped: a frame without a move, then the rebuild starts on the workers.
        let shifted = overlay(&mut ws, &tt);
        assert_eq!(mask_builds(wake.clone()), stamp);
        landed(&wake, stamp + 1);
        assert!(warp(&ws).is_some_and(|w| w.is_none()));
        assert!(
            overlay(&mut ws, &tt) == shifted,
            "whole pixels: the same pixels shifted"
        );
        let fresh = {
            let mut f = DocumentWorkspace::new(120, 90, 96.0);
            select(&mut f, patch);
            f.selection
                .set_interim_transform(ws.selection.get_interim_transform());
            let polys = f.selection.create_path(true).as_polys();
            let q = f.selection_renderer().quality;
            outline_masks(&polys, &tt, q, 8, mask_rect(&polys, &tt, q))
        };
        let (shown, _) = ws.selection_renderer().masks(&ws, &tt).unwrap();
        assert_eq!((&shown.solid, &shown.dashed), (&fresh.solid, &fresh.dashed));
        assert_eq!(mask_builds(wake.clone()), stamp + 1, "once");
        // A new shape (a Replace drag) is never held: its worker build starts at once.
        let other = noise_cells(12.0, 8.0, 96, 72);
        ws.selection
            .set_continuation_path(other, CombineMode::Replace);
        ws.surface_box.overlay_bounds(&ws, &tt);
        assert!(BUILDS.with_borrow(|b| b.slots[0].held.is_none()));
    }

    /// R77: a simple selection is held only while it translates. Rotated once a frame under
    /// `mask_builds`, its masks are rebuilt in each frame (no warp: crisp ants).
    #[test]
    fn a_moving_simple_selection_rebuilds_every_frame() {
        let tt = t(1.0, [0.0, 0.0], [120, 90]);
        let mut ws = DocumentWorkspace::new(120, 90, 96.0);
        let r = Rectangle::new(30.0, 20.0, 50.0, 40.0);
        ws.selection.set_continuation(&r, CombineMode::Replace);
        assert!(!heavy(ws.selection.sources(), &tt));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let stamp = mask_builds(wake.clone());
        ws.surface_box.overlay_bounds(&ws, &tt);
        for i in 1..=5 {
            let mut m = Matrix::identity();
            m.rotate_at(7.0 * i as f64, Point::new(55.0, 40.0));
            ws.selection.set_interim_transform(m);
            let (_, warp) = ws.selection_renderer().masks(&ws, &tt).unwrap();
            assert!(warp.is_none(), "frame {i}");
            assert_eq!(mask_builds(wake.clone()), stamp);
        }
    }

    /// A simple selection that only translates under `mask_builds` (a Move drag) shows its masks
    /// shifted by whole device px, rebuilt in the frame after it stops: by whole px, the same
    /// pixels as a rebuild each frame. Masks the viewport clipped are rebuilt at once.
    #[test]
    fn a_simple_selection_moved_by_whole_pixels_shifts_its_masks() {
        let tt = t(1.25, [3.0, 2.0], [160, 120]);
        let r = Rectangle::new(30.0, 20.0, 50.0, 40.0);
        let mut ws = DocumentWorkspace::new(120, 90, 96.0);
        ws.selection.set_continuation(&r, CombineMode::Replace);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let warp = |ws: &DocumentWorkspace| ws.selection_renderer().masks(ws, &tt).map(|(_, w)| w);
        let stamp = mask_builds(wake.clone());
        assert!(warp(&ws).is_some_and(|w| w.is_none()));
        // Whole device px: (dx, dy)·1.25.
        let to = |ws: &mut DocumentWorkspace, dx: f64, dy: f64| {
            let mut m = Matrix::identity();
            m.translate(dx, dy);
            ws.selection.set_interim_transform(m);
        };
        for i in 1..=3 {
            to(&mut ws, 4.0 * i as f64, -0.8 * i as f64);
            let shift = Some((5 * i, -i));
            assert!(
                warp(&ws).is_some_and(|w| w.is_some_and(|w| w.shift == shift)),
                "{i}"
            );
            // The rebuild's masks: the shown ones' bytes over their rect moved by the shift.
            let (shown, _) = ws.selection_renderer().masks(&ws, &tt).unwrap();
            let polys = ws.selection.create_path(true).as_polys();
            let q = ws.selection_renderer().quality;
            let fresh = outline_masks(&polys, &tt, q, 8, mask_rect(&polys, &tt, q));
            let (x, y) = (shown.rect.x + 5 * i, shown.rect.y - i);
            assert_eq!(fresh.rect, IntRect::new(x, y, shown.rect.w, shown.rect.h));
            assert!(
                (&shown.solid, &shown.dashed) == (&fresh.solid, &fresh.dashed),
                "{i}"
            );
            mask_builds(wake.clone());
        }
        // It stopped: rebuilt in the next frame.
        assert_eq!(mask_builds(wake.clone()), stamp + 1);
        assert!(warp(&ws).is_some_and(|w| w.is_none()));
        // Fractional device px: shifted by the nearest whole px meanwhile, rebuilt once it stops.
        to(&mut ws, 1.1, 0.0);
        let snapped =
            |w: &Warp| (w.shift, w.inv) == (Some((-14, 3)), [[1.0, 0.0, 14.0], [0.0, 1.0, -3.0]]);
        assert!(warp(&ws).is_some_and(|w| w.is_some_and(|w| snapped(&w))));
        mask_builds(wake.clone());
        assert_eq!(mask_builds(wake.clone()), stamp + 2);
        assert!(warp(&ws).is_some_and(|w| w.is_none()));
        // Masks the viewport clipped are rebuilt at once.
        to(&mut ws, 80.0, 0.0);
        assert!(warp(&ws).is_some_and(|w| w.is_some()));
        mask_builds(wake.clone());
        mask_builds(wake.clone());
        to(&mut ws, 84.0, 0.0);
        assert!(warp(&ws).is_some_and(|w| w.is_none()), "clipped");
    }

    /// Task 27p: a complex selection's held rebuild starts on the workers once it stops; a move
    /// before that build ran supersedes it (it ends without landing, freeing the workers), and the
    /// rebuild comes once the moves stop again.
    #[test]
    fn a_move_supersedes_the_rebuild_under_way() {
        let tt = t(1.0, [0.0, 0.0], [300, 300]);
        let mut ws = DocumentWorkspace::new(300, 300, 96.0);
        let ends = Arc::new(AtomicU64::new(0));
        let e = ends.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            e.fetch_add(1, Ordering::Relaxed);
        });
        let stamp = mask_builds(wake.clone());
        select(&mut ws, ring(150.0, 150.0, 80.0, 60.0));
        ws.surface_box.overlay_bounds(&ws, &tt);
        let stamp = landed(&wake, stamp + 1);
        let to = |ws: &mut DocumentWorkspace, dx: f64| {
            let mut m = Matrix::identity();
            m.translate(dx, 0.0);
            ws.selection.set_interim_transform(m);
            ws.surface_box.overlay_bounds(ws, &tt);
        };
        let release = occupy_workers();
        to(&mut ws, 1.0);
        mask_builds(wake.clone());
        // Stopped: the rebuild goes to the (busy) workers.
        assert_eq!(mask_builds(wake.clone()), stamp);
        to(&mut ws, 2.0);
        let before = ends.load(Ordering::Relaxed);
        release.wait();
        let deadline = Instant::now() + Duration::from_secs(60);
        while ends.load(Ordering::Relaxed) == before {
            assert!(Instant::now() < deadline, "the build never ended");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            mask_builds(wake.clone()),
            stamp,
            "superseded: nothing lands"
        );
        mask_builds(wake.clone());
        landed(&wake, stamp + 1);
        let (_, warp) = ws.selection_renderer().masks(&ws, &tt).unwrap();
        assert!(warp.is_none(), "the rebuild of the last move landed");
    }
}
