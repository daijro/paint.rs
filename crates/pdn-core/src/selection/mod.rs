//! Port of: js/src/engine/document/selection/{Selection,Data,CombineMode}.js
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's selection data, effective and clipping-mask geometry, mask renderers, polygon model
//! and statistics. Rulings P2: Paint.NET's five combine modes (paintdotjs's COMPLEMENT dropped),
//! persisted as paintdotjs's strings.

pub mod matrix;
pub mod path;

use crate::pixel::IntRect;
use crate::pixel::mask::{self, Mask, SelectionQuality};
use crate::util::geom::{Point, Rectangle};
use crate::util::region::Region;
use matrix::{Matrix, MatrixOrder};
use path::GraphicsPath;
use std::sync::{Arc, Mutex};

/// How a new selection combines with the current one (§2.2 step 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CombineMode {
    Replace,
    Union,
    Exclude,
    Intersect,
    /// The continuation mode of a fresh or just-committed selection (§2.1).
    #[default]
    Xor,
}

impl CombineMode {
    pub fn value(self) -> &'static str {
        match self {
            CombineMode::Replace => "replace",
            CombineMode::Union => "union",
            CombineMode::Exclude => "exclude",
            CombineMode::Intersect => "intersect",
            CombineMode::Xor => "xor",
        }
    }

    /// Unknown values give Replace.
    pub fn from_value(v: &str) -> CombineMode {
        match v {
            "union" => CombineMode::Union,
            "exclude" => CombineMode::Exclude,
            "intersect" => CombineMode::Intersect,
            "xor" => CombineMode::Xor,
            _ => CombineMode::Replace,
        }
    }
}

/// SelectionData (§2.1); the selection memento's payload.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionData {
    pub base: GraphicsPath,
    pub continuation: GraphicsPath,
    pub mode: CombineMode,
    pub cumulative: Matrix,
    pub interim: Matrix,
}

/// The effective geometry (§2.1): Combine(base, mode, continuation) (the continuation alone for
/// Replace), then `interim`; always `bounded`.
fn effective(
    base: &GraphicsPath,
    mode: CombineMode,
    continuation: &GraphicsPath,
    interim: Option<&Matrix>,
) -> GraphicsPath {
    let p = GraphicsPath::combine(base, mode, continuation);
    match interim {
        Some(m) if !m.is_identity() => p.transformed(m).bounded(),
        _ => p,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelectionStatistics {
    pub bounds: Rectangle,
    pub area: f64,
}

/// Exact scan bounds/area, shared by the synchronous API and footer worker.
pub(crate) fn statistics_for_path(
    path: &GraphicsPath,
    doc_bounds: &Rectangle,
) -> SelectionStatistics {
    let (mut bounds, mut area) = (None, 0.0);
    for (y, x0, x1) in mask::scans(&path.as_polys(), 1.0) {
        let scan = Rectangle::new(x0 as f64, y as f64, (x1 - x0) as f64, 1.0);
        bounds = Some(bounds.map_or(scan, |b: Rectangle| Rectangle::union(&b, &scan)));
        let c = Rectangle::intersect_of(&scan, doc_bounds);
        area += c.width * c.height;
    }
    let bounds = bounds.unwrap_or_else(|| {
        let g = Rectangle::intersect_of(&path.get_bounds(), doc_bounds);
        Rectangle::new(g.x.floor(), g.y.floor(), 0.0, 0.0)
    });
    SelectionStatistics { bounds, area }
}

/// paintdotjs's `Selection.clipRectangle` (`createRegion`).
const CLIP_RECTANGLE: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 65535.0,
    height: 65535.0,
};

fn next_revision() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

type MaskCache = Option<(u64, SelectionQuality, IntRect, Arc<Mask>)>;

/// The document selection (SelectionData, §2.1). Every mutator returns whether the data changed
/// (judged from what it touches, without copying the data); a change takes a new `revision`, which
/// keys the clipping-mask cache and `AppEvent::SelectionChanged`.
#[derive(Debug, Default)]
pub struct Selection {
    /// Shared with the mask builds (`render::selection`), which read them on a worker: a mutator
    /// replaces a path, never copies one.
    base: Arc<GraphicsPath>,
    continuation: Arc<GraphicsPath>,
    mode: CombineMode,
    cumulative: Matrix,
    interim: Matrix,
    revision: u64,
    /// Renewed by `reset` and `restore` only: while it holds, every base the selection had is the
    /// first one moved by `moved` (every interim transform committed since), so old ants can
    /// follow it (`placement`).
    shape: u64,
    moved: Matrix,
    /// Renewed by every change but a new interim transform: while it holds, only the interim
    /// transform moved the geometry (masks of it can be drawn moved, `render::selection`).
    paths: u64,
    mask: Mutex<MaskCache>,
}

impl Selection {
    /// A new revision when `changed`.
    fn bump(&mut self, changed: bool) -> bool {
        if changed {
            self.revision = next_revision();
            self.paths = self.revision;
        }
        changed
    }

    /// A new revision and shape when `changed`: the data was replaced, not moved.
    fn replaced(&mut self, changed: bool) -> bool {
        if changed {
            (self.shape, self.moved) = (next_revision(), Matrix::identity());
        }
        self.bump(changed)
    }

    /// A process-wide stamp, renewed on every change.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// (shape stamp, moved·interim): where the first base of the shape is now. Masks built for
    /// one placement are moved to another of the same shape meanwhile (§2.9 step 1).
    pub(crate) fn placement(&self) -> (u64, Matrix) {
        let mut m = self.moved;
        m.multiply(&self.interim, MatrixOrder::Append);
        (self.shape, m)
    }

    pub fn reset(&mut self) -> bool {
        // Cheap: the polygon lists differ by length unless both are empty.
        let changed = !self.base.polygons.is_empty()
            || !self.continuation.polygons.is_empty()
            || self.mode != CombineMode::Xor
            || !self.cumulative.is_identity()
            || !self.interim.is_identity();
        (self.base, self.continuation) = (Arc::default(), Arc::default());
        (self.mode, self.cumulative, self.interim) = Default::default();
        self.replaced(changed)
    }

    /// Commits the interim transform: base and continuation transformed, cumulative ←
    /// cumulative·interim, interim ← I.
    fn commit_interim(&mut self) {
        if !self.interim.is_identity() {
            self.base = Arc::new(self.base.transformed(&self.interim));
            self.continuation = Arc::new(self.continuation.transformed(&self.interim));
            self.cumulative.multiply(&self.interim, MatrixOrder::Append);
            self.moved.multiply(&self.interim, MatrixOrder::Append);
            self.interim = Matrix::identity();
        }
    }

    /// Sets the continuation: commits the interim transform and resets the cumulative one first.
    fn put_continuation(&mut self, path: GraphicsPath, mode: CombineMode) {
        self.commit_interim();
        self.cumulative = Matrix::identity();
        self.mode = mode;
        self.continuation = Arc::new(path);
    }

    /// Resets the continuation: back to the base. The mode returns to Xor, as a commit of the
    /// continuation and a fresh selection leave it: an empty Intersect continuation would empty
    /// the effective geometry (§2.2 step 2), so a selection tool's Revert (§2.5 step 7) would
    /// deselect.
    // ruling R48: Revert = reset the continuation → mode Xor, no history entry.
    // calibrate: B-S8 — Alt+Right-drag a rectangle enclosing a rectangular selection: expect no
    // change and no history item.
    pub fn reset_continuation(&mut self) -> bool {
        let changed = !self.continuation.polygons.is_empty()
            || !self.interim.is_identity()
            || !self.cumulative.is_identity()
            || self.mode != CombineMode::Xor;
        self.put_continuation(GraphicsPath::default(), CombineMode::Xor);
        self.bump(changed)
    }

    pub fn set_continuation(&mut self, r: &Rectangle, mode: CombineMode) -> bool {
        self.set_continuation_path(GraphicsPath::from_rect(r), mode)
    }

    pub fn set_continuation_points(&mut self, points: &[Point], mode: CombineMode) -> bool {
        self.set_continuation_path(GraphicsPath::from_points(points), mode)
    }

    pub fn set_continuation_path(&mut self, path: GraphicsPath, mode: CombineMode) -> bool {
        let changed = !self.interim.is_identity()
            || !self.cumulative.is_identity()
            || self.mode != mode
            || *self.continuation != path;
        self.put_continuation(path, mode);
        self.bump(changed)
    }

    /// base ← effective geometry; continuation cleared; mode ← Xor. A no-op (false) with an empty
    /// continuation, mode Xor and no interim transform.
    pub fn commit_continuation(&mut self) -> bool {
        if self.continuation.polygons.is_empty()
            && self.mode == CombineMode::Xor
            && self.interim.is_identity()
        {
            return false;
        }
        self.base = Arc::new(self.create_path(true));
        self.continuation = Arc::default();
        self.mode = CombineMode::Xor;
        self.bump(true)
    }

    pub fn commit_interim_transform(&mut self) -> bool {
        let changed = !self.interim.is_identity();
        self.commit_interim();
        self.bump(changed)
    }

    pub fn set_interim_transform(&mut self, m: Matrix) -> bool {
        let (changed, paths) = (self.interim != m, self.paths);
        self.interim = m;
        self.bump(changed);
        self.paths = paths;
        changed
    }

    /// Changes only when something but the interim transform changes (`set_interim_transform`
    /// keeps it).
    pub(crate) fn paths(&self) -> u64 {
        self.paths
    }

    pub fn get_interim_transform(&self) -> Matrix {
        self.interim
    }

    pub fn create_path(&self, apply_interim: bool) -> GraphicsPath {
        let interim = apply_interim.then_some(&self.interim);
        effective(&self.base, self.mode, &self.continuation, interim)
    }

    /// `create_path(true)` to run later, on another thread: it holds the paths (shared, not
    /// copied).
    pub(crate) fn create_path_later(&self) -> impl FnOnce() -> GraphicsPath + Send + 'static {
        let (base, continuation) = (self.base.clone(), self.continuation.clone());
        let (mode, interim) = (self.mode, self.interim);
        move || effective(&base, mode, &continuation, Some(&interim))
    }

    /// The paths the effective geometry comes from: the continuation alone for Replace.
    pub(crate) fn sources(&self) -> impl Iterator<Item = &GraphicsPath> {
        let base = (self.mode != CombineMode::Replace).then_some(&*self.base);
        base.into_iter().chain([&*self.continuation])
    }

    pub fn save(&self) -> SelectionData {
        SelectionData {
            base: (*self.base).clone(),
            continuation: (*self.continuation).clone(),
            mode: self.mode,
            cumulative: self.cumulative,
            interim: self.interim,
        }
    }

    pub fn restore(&mut self, data: SelectionData) -> bool {
        let changed = *self.base != data.base
            || *self.continuation != data.continuation
            || (self.mode, self.cumulative, self.interim)
                != (data.mode, data.cumulative, data.interim);
        self.base = Arc::new(data.base);
        self.continuation = Arc::new(data.continuation);
        (self.mode, self.cumulative, self.interim) = (data.mode, data.cumulative, data.interim);
        self.replaced(changed)
    }

    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.continuation.is_empty()
    }

    /// The effective geometry's vertex bounds.
    pub fn get_bounds(&self) -> Rectangle {
        self.create_path(true).get_bounds()
    }

    /// §4 "Selection statistics": bounds of the unclipped §2.3 scans, area of the scans clipped
    /// to the document.
    pub fn get_statistics(&self, doc_bounds: &Rectangle) -> SelectionStatistics {
        statistics_for_path(&self.create_path(true), doc_bounds)
    }

    /// paintdotjs `createRegion`: each polygon's bounds ∩ the clip rectangle; the clip rectangle
    /// when empty.
    pub fn create_region(&self) -> Region {
        if self.is_empty() {
            return Region::from_rectangle(CLIP_RECTANGLE);
        }
        let path = self.create_path(true);
        let mut region = Region::from_rectangles(
            path.polygons
                .iter()
                .filter_map(|p| path::bounds_of(p.iter()))
                .collect(),
        );
        region.intersect_rectangle(&CLIP_RECTANGLE);
        region
    }

    /// §2.1 clipping-mask geometry: the effective geometry with each polygon not already inside
    /// `doc` Sutherland–Hodgman-clipped to it; the whole `doc` when the selection is empty.
    pub fn clipping_polygons(&self, doc: IntRect) -> Vec<Vec<[f64; 2]>> {
        let r = Rectangle::new(doc.x as f64, doc.y as f64, doc.w as f64, doc.h as f64);
        let path = if self.is_empty() {
            GraphicsPath::from_rect(&r)
        } else {
            self.create_path(true)
        };
        path.polygons
            .iter()
            .filter_map(|p| {
                let b = path::bounds_of(p.iter())?;
                let inside = b.left() >= r.left()
                    && b.top() >= r.top()
                    && b.right() <= r.right()
                    && b.bottom() <= r.bottom();
                let c = if inside {
                    p.clone()
                } else {
                    path::clip_to_rect(p, &r)
                };
                (!c.is_empty()).then(|| c.iter().map(|v| [v.x, v.y]).collect())
            })
            .collect()
    }

    /// The clipping mask over `doc` (§2.3 "Mask renderers"): opaque when empty, else the
    /// rasterized clipping polygons; cached for the last (revision, quality, doc).
    // ponytail: one whole-document A8 mask (64 MB at 8000²); Paint.NET renders 128-px tiles
    // lazily (§2.3 "Tiling") — tile it when large documents need it.
    pub fn clipping_mask(&self, quality: SelectionQuality, doc: IntRect) -> Arc<Mask> {
        let mut cache = self.mask.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((rev, q, r, m)) = &*cache
            && (*rev, *q, *r) == (self.revision, quality, doc)
        {
            return m.clone();
        }
        let m = Arc::new(self.clipping_mask_in(quality, doc, doc));
        *cache = Some((self.revision, quality, doc, m.clone()));
        m
    }

    /// `clipping_mask(quality, doc)` over `rect` alone, uncached: the same pixels there (a move
    /// frame needs only its own rect).
    pub fn clipping_mask_in(&self, quality: SelectionQuality, doc: IntRect, rect: IntRect) -> Mask {
        if self.is_empty() {
            Mask::opaque(rect)
        } else {
            mask::rasterize(&self.clipping_polygons(doc), quality, rect)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle {
        Rectangle::new(x, y, w, h)
    }

    /// §2.1: setting the continuation commits the interim transform and resets the cumulative
    /// one; committing it stores the effective geometry and resets the mode to Xor.
    #[test]
    fn continuation_and_transforms() {
        let mut s = Selection::default();
        assert!(s.is_empty() && s.save().mode == CombineMode::Xor);
        assert!(!s.reset());
        let r0 = s.revision();
        assert!(s.set_continuation(&rect(0.0, 0.0, 4.0, 4.0), CombineMode::Replace));
        assert_ne!(s.revision(), r0);
        assert!(s.commit_continuation());
        assert_eq!(s.save().mode, CombineMode::Xor);
        let mut t = Matrix::identity();
        t.translate(2.0, 1.0);
        s.set_interim_transform(t);
        assert_eq!(s.get_bounds(), rect(2.0, 1.0, 4.0, 4.0));
        assert_eq!(s.create_path(false).get_bounds(), rect(0.0, 0.0, 4.0, 4.0));
        s.set_continuation(&rect(10.0, 10.0, 1.0, 1.0), CombineMode::Union);
        let d = s.save();
        assert!(d.interim.is_identity() && d.cumulative.is_identity());
        assert_eq!(d.base.get_bounds(), rect(2.0, 1.0, 4.0, 4.0));
        s.reset_continuation();
        assert_eq!(s.get_bounds(), rect(2.0, 1.0, 4.0, 4.0));
        // Ruling R48: back to the base with mode Xor, so an Intersect reset keeps the base.
        assert_eq!(s.save().mode, CombineMode::Xor);
        s.set_continuation_path(GraphicsPath::default(), CombineMode::Intersect);
        assert!(s.reset_continuation());
        assert_eq!(s.get_bounds(), rect(2.0, 1.0, 4.0, 4.0));
        assert!(!s.reset_continuation());
        s.set_interim_transform(t);
        assert!(s.commit_interim_transform());
        assert_eq!(s.save().cumulative, t);
        assert!(!s.commit_interim_transform());
        // Committing the continuation under an interim transform keeps both transforms.
        let mut t2 = Matrix::identity();
        t2.scale(2.0, 2.0);
        s.set_interim_transform(t2);
        assert!(s.commit_continuation());
        let d = s.save();
        assert_eq!((d.interim, d.cumulative), (t2, t));
        assert_eq!(d.base.get_bounds(), rect(8.0, 4.0, 8.0, 8.0));
        let saved = s.save();
        s.reset();
        assert!(s.is_empty());
        assert!(s.restore(saved.clone()));
        assert_eq!(s.save(), saved);
        assert_eq!(CombineMode::from_value("complement"), CombineMode::Replace);
        assert_eq!(
            CombineMode::from_value(CombineMode::Exclude.value()),
            CombineMode::Exclude
        );
    }

    /// §2.3 "Selection empty → opaque"; the mask is cached per revision.
    #[test]
    fn clipping_mask_cache_and_empty() {
        let mut s = Selection::default();
        let doc = IntRect::new(0, 0, 4, 2);
        assert!(
            s.clipping_mask(SelectionQuality::HighQuality, doc)
                .data
                .iter()
                .all(|&v| v == 255)
        );
        assert_eq!(s.create_region().rectangles, [CLIP_RECTANGLE]);
        s.set_continuation(&rect(1.0, 0.0, 2.0, 2.0), CombineMode::Replace);
        let a = s.clipping_mask(SelectionQuality::HighQuality, doc);
        assert!(Arc::ptr_eq(
            &a,
            &s.clipping_mask(SelectionQuality::HighQuality, doc)
        ));
        assert_eq!(a.data, [0, 255, 255, 0, 0, 255, 255, 0]);
        assert_eq!(s.create_region().rectangles, [rect(1.0, 0.0, 2.0, 2.0)]);
        // An empty effective geometry with a pending continuation selects nothing.
        s.set_continuation(&rect(9.0, 9.0, 1.0, 1.0), CombineMode::Intersect);
        assert!(
            s.clipping_mask(SelectionQuality::Aliased, doc)
                .data
                .iter()
                .all(|&v| v == 0)
        );
        let st = s.get_statistics(&rect(0.0, 0.0, 4.0, 2.0));
        assert_eq!((st.bounds, st.area), (Rectangle::default(), 0.0));
    }
}
