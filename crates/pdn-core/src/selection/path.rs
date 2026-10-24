//! Port of: js/src/engine/document/selection/{GraphicsPath,VertexList,Polygon}.js
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's polygon list (even-odd, no hole flags), its shortcuts, XOR fast path and
//! Sutherland–Hodgman clip, and stencil → rectilinear integer polygons.
//!
//! `VertexList { hole }` is gone: a path is a flat polygon list filled even-odd across all
//! polygons, each with an implicit closing edge. i_overlay (not GPC, Global Constraints) does the
//! booleans; its outer and inner contours both become plain polygons, which even-odd fills to the
//! same region (§4 "Boolean engine").

use super::CombineMode;
use super::matrix::Matrix;
use crate::util::bitvec2d::BitVector2D;
use crate::util::geom::{Point, Rectangle};
use i_overlay::core::{fill_rule::FillRule, overlay_rule::OverlayRule};
use i_overlay::float::single::SingleFloatOverlay;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphicsPath {
    pub polygons: Vec<Vec<Point>>,
}

pub(crate) fn bounds_of<'a>(points: impl Iterator<Item = &'a Point>) -> Option<Rectangle> {
    let mut it = points.peekable();
    it.peek()?;
    let (mut l, mut t, mut r, mut b) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in it {
        (l, t, r, b) = (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y));
    }
    Some(Rectangle::absolute(l, t, r, b))
}

impl GraphicsPath {
    /// §2.1: the 5-point outline (L,T),(R,T),(R,B),(L,B),(L,T).
    pub fn from_rect(r: &Rectangle) -> GraphicsPath {
        let (l, t, rr, b) = (r.left(), r.top(), r.right(), r.bottom());
        GraphicsPath {
            polygons: vec![vec![
                Point::new(l, t),
                Point::new(rr, t),
                Point::new(rr, b),
                Point::new(l, b),
                Point::new(l, t),
            ]],
        }
    }

    /// One polygon (none for no points).
    pub fn from_points(points: &[Point]) -> GraphicsPath {
        GraphicsPath {
            polygons: if points.is_empty() {
                vec![]
            } else {
                vec![points.to_vec()]
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.polygons.iter().all(Vec::is_empty)
    }

    /// The vertex bounding box; (0, 0, 0, 0) when empty.
    pub fn get_bounds(&self) -> Rectangle {
        bounds_of(self.polygons.iter().flatten()).unwrap_or_default()
    }

    pub fn transformed(&self, m: &Matrix) -> GraphicsPath {
        let mut out = self.clone();
        for p in &mut out.polygons {
            m.transform_points(p);
        }
        out
    }

    /// §2.1 pixelated: rectilinear with integer vertices (empty counts).
    pub fn is_pixelated(&self) -> bool {
        crate::pixel::mask::is_pixelated(&self.as_polys())
    }

    pub fn as_polys(&self) -> Vec<Vec<[f64; 2]>> {
        self.polygons
            .iter()
            .map(|p| p.iter().map(|v| [v.x, v.y]).collect())
            .collect()
    }

    /// The geometry the booleans and the mask can take: polygons with a non-finite point and
    /// empty ones are dropped, and polygons reaching past ±2^17 are Sutherland–Hodgman-clipped to
    /// that box, which keeps every even-odd region inside it (documents stay within 65535 px).
    /// i_overlay panics on non-finite input and its integer grid loses small shapes next to huge
    /// coordinates.
    pub(crate) fn bounded(&self) -> GraphicsPath {
        const LIMIT: f64 = 131072.0;
        let r = Rectangle::new(-LIMIT, -LIMIT, 2.0 * LIMIT, 2.0 * LIMIT);
        let finite = |p: &[Point]| p.iter().all(|v| v.x.is_finite() && v.y.is_finite());
        let polygons = self
            .polygons
            .iter()
            .filter(|p| finite(p))
            .filter_map(|p| {
                let b = bounds_of(p.iter())?;
                let inside = b.left() >= -LIMIT
                    && b.top() >= -LIMIT
                    && b.right() <= LIMIT
                    && b.bottom() <= LIMIT;
                let c = if inside {
                    p.clone()
                } else {
                    clip_to_rect(p, &r)
                };
                // The clip's intercepts can overflow for coordinates near f64::MAX.
                (!c.is_empty() && finite(&c)).then_some(c)
            })
            .collect();
        GraphicsPath { polygons }
    }

    fn from_polys(polys: impl IntoIterator<Item = Vec<[f64; 2]>>) -> GraphicsPath {
        GraphicsPath {
            polygons: polys
                .into_iter()
                .filter(|p| !p.is_empty())
                .map(|p| p.into_iter().map(|[x, y]| Point::new(x, y)).collect())
                .collect(),
        }
    }

    /// `a` combined with `b` (§2.2) after `bounded`. Replace gives `b`.
    pub fn combine(a: &GraphicsPath, mode: CombineMode, b: &GraphicsPath) -> GraphicsPath {
        if mode == CombineMode::Replace {
            return b.bounded();
        }
        let (a, b) = (a.bounded(), b.bounded());
        let rule = match mode {
            CombineMode::Replace => unreachable!("returned above"),
            CombineMode::Union => OverlayRule::Union,
            CombineMode::Exclude => OverlayRule::Difference,
            CombineMode::Intersect => OverlayRule::Intersect,
            CombineMode::Xor => OverlayRule::Xor,
        };
        // Step 2: the empty-operand shortcuts.
        match (a.is_empty(), b.is_empty()) {
            (true, true) => return GraphicsPath::default(),
            (true, false) => {
                return match mode {
                    CombineMode::Union | CombineMode::Xor => b,
                    _ => GraphicsPath::default(),
                };
            }
            (false, true) => {
                return match mode {
                    CombineMode::Intersect => GraphicsPath::default(),
                    _ => a,
                };
            }
            (false, false) => {}
        }
        // Step 3: XOR with a rectangle that strictly contains `a`'s bounds.
        let r = b.get_bounds();
        let outline = GraphicsPath::from_rect(&r);
        let ab = a.get_bounds();
        if mode == CombineMode::Xor
            && b == outline
            && r.left() < ab.left()
            && r.top() < ab.top()
            && ab.right() < r.right()
            && ab.bottom() < r.bottom()
        {
            let (same, mut out): (Vec<_>, Vec<_>) = a
                .polygons
                .into_iter()
                .partition(|p| *p == outline.polygons[0]);
            if same.len() % 2 == 0 {
                out.extend(outline.polygons);
            }
            return GraphicsPath { polygons: out };
        }
        let shapes = a.as_polys().overlay(&b.as_polys(), rule, FillRule::EvenOdd);
        GraphicsPath::from_polys(shapes.into_iter().flatten())
    }
}

/// `pathFromStencil`: the Paint.NET 3.36 island tracer (Rulings P2). Each island's outline is
/// walked with the pixels on its left, then the pixels inside it are inverted, so holes come out
/// as further islands: an even-odd list of rectilinear integer polygons (§2.7 step 10).
pub fn path_from_stencil(stencil: &BitVector2D, bounds: &Rectangle) -> GraphicsPath {
    let mut stencil = stencil.clone();
    let (l, t) = (bounds.left() as i32, bounds.top() as i32);
    let (r, b) = (bounds.right() as i32, bounds.bottom() as i32);
    let mut ret = GraphicsPath::default();
    if stencil.is_empty() || l >= r || t >= b {
        return ret;
    }
    let in_bounds = |(x, y): (i32, i32)| x >= l && x < r && y >= t && y < b;
    let mut start = (l, t);
    loop {
        // The next island: the first set pixel in row-major order from the last start.
        while !stencil.get(start.0, start.1) {
            start.0 += 1;
            if start.0 >= r {
                (start.0, start.1) = (l, start.1 + 1);
                if start.1 >= b {
                    return ret;
                }
            }
        }
        let mut pts = vec![];
        let (mut last, mut curr) = ((start.0, start.1 + 1), start);
        loop {
            let (dx, dy) = (curr.0 - last.0, curr.1 - last.1);
            // The source algorithm's integer divisions (truncating).
            let left = (
                (dx + dy + 2) / 2 + curr.0 - 1,
                (dy - dx + 2) / 2 + curr.1 - 1,
            );
            let right = (
                (dx - dy + 2) / 2 + curr.0 - 1,
                (dy + dx + 2) / 2 + curr.1 - 1,
            );
            let set = |p: (i32, i32)| in_bounds(p) && stencil.get(p.0, p.1);
            let next = if set(left) {
                (curr.0 + dy, curr.1 - dx) // turn left
            } else if set(right) {
                (curr.0 + dx, curr.1 + dy) // straight
            } else {
                (curr.0 - dy, curr.1 + dx) // turn right
            };
            if (next.0 - curr.0).signum() != dx.signum()
                || (next.1 - curr.1).signum() != dy.signum()
            {
                pts.push(Point::new(curr.0 as f64, curr.1 as f64));
            }
            (last, curr) = (curr, next);
            if next == start {
                break;
            }
        }
        for scan in crate::util::utility::get_scans(&pts) {
            stencil.invert_scanline(&scan);
        }
        ret.polygons.push(pts);
    }
}

/// Sutherland–Hodgman against `r`: Left, Right, Top, Bottom; inside x ≥ L, x < R, y ≥ T, y < B;
/// linear intercepts in f64 (§2.2 step 4).
pub fn clip_to_rect(poly: &[Point], r: &Rectangle) -> Vec<Point> {
    crate::util::utility::sutherland_hodgman_with(r, poly, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(l: f64, t: f64, s: f64) -> GraphicsPath {
        GraphicsPath::from_rect(&Rectangle::new(l, t, s, s))
    }

    /// i_overlay keeps integer vertices exact, so booleans of pixelated rectangles stay pixelated;
    /// a hole comes back as a plain polygon that even-odd fills as a hole.
    #[test]
    fn booleans_of_rects() {
        let (a, b) = (sq(0.0, 0.0, 4.0), sq(2.0, 2.0, 4.0));
        let u = GraphicsPath::combine(&a, CombineMode::Union, &b);
        assert!(u.is_pixelated());
        assert_eq!(u.get_bounds(), Rectangle::new(0.0, 0.0, 6.0, 6.0));
        let i = GraphicsPath::combine(&a, CombineMode::Intersect, &b);
        assert_eq!(i.get_bounds(), Rectangle::new(2.0, 2.0, 2.0, 2.0));
        let hole =
            GraphicsPath::combine(&sq(0.0, 0.0, 6.0), CombineMode::Exclude, &sq(2.0, 2.0, 2.0));
        assert_eq!(hole.polygons.len(), 2);
        let m = crate::pixel::mask::rasterize(
            &hole.as_polys(),
            crate::pixel::mask::SelectionQuality::Aliased,
            crate::pixel::IntRect::new(0, 0, 6, 6),
        );
        assert_eq!(&m.data[18..24], &[255, 255, 0, 0, 255, 255]);
        assert_eq!(GraphicsPath::combine(&a, CombineMode::Replace, &b), b);
    }

    /// §2.2 step 4: right and bottom exclusive; intercepts land on the edges.
    #[test]
    fn clip_is_exclusive_on_right_and_bottom() {
        let r = Rectangle::new(0.0, 0.0, 10.0, 10.0);
        let tri = [
            Point::new(5.0, -5.0),
            Point::new(15.0, 5.0),
            Point::new(5.0, 15.0),
        ];
        let c = clip_to_rect(&tri, &r);
        assert!(
            c.iter()
                .all(|p| p.x >= 5.0 && p.x <= 10.0 && p.y >= 0.0 && p.y <= 10.0)
        );
        assert!(c.contains(&Point::new(10.0, 0.0)) && c.contains(&Point::new(10.0, 10.0)));
        // A vertex on the right edge is outside (x < R), but the intercept puts it back.
        let on_edge = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 5.0),
        ];
        assert_eq!(
            clip_to_rect(&on_edge, &r),
            [
                Point::new(10.0, 5.0),
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0)
            ]
        );
        assert!(clip_to_rect(&[Point::new(20.0, 20.0)], &r).is_empty());
    }

    #[test]
    fn stencil_hole_traces_two_polygons() {
        let mut bv = BitVector2D::new(5, 5);
        bv.set_unchecked_rect(&Rectangle::new(0.0, 0.0, 5.0, 5.0), true);
        bv.set(2, 2, false);
        let p = path_from_stencil(&bv, &Rectangle::new(0.0, 0.0, 5.0, 5.0));
        assert_eq!(p.polygons.len(), 2);
        assert!(p.is_pixelated());
        assert_eq!(p.get_bounds(), Rectangle::new(0.0, 0.0, 5.0, 5.0));
        assert!(
            path_from_stencil(&BitVector2D::new(3, 3), &Rectangle::new(0.0, 0.0, 3.0, 3.0))
                .is_empty()
        );
    }
}
