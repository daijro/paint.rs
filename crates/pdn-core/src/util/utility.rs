//! Port of: js/src/engine/util/Utility.js
//! Reference: paintdotjs @ b4e6fc5
//!
//! Matrix arguments are the JS `Matrix.elements` layout (`[[f64; 3]; 3]`); `selection::matrix`
//! (Task 24) owns the Matrix type.

use super::geom::{Point, Rectangle, Scanline, UtilityEdge};
use super::region::Region;
use crate::js::to_int32;

pub type MatrixElements = [[f64; 3]; 3];

pub const DEFAULT_SIMPLIFICATION_FACTOR: usize = 50;

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// paintdotjs's clip: every bounds edge inclusive.
pub fn sutherland_hodgman(bounds: &Rectangle, v: &[Point]) -> Vec<Point> {
    sutherland_hodgman_with(bounds, v, false)
}

/// Left, Right, Top, Bottom; `exclusive` = Paint.NET's inside tests x ≥ L, x < R, y ≥ T, y < B
/// (selection-and-move.md §2.2 step 4), else paintdotjs's inclusive ones.
pub(crate) fn sutherland_hodgman_with(
    bounds: &Rectangle,
    v: &[Point],
    exclusive: bool,
) -> Vec<Point> {
    let mut p = v.to_vec();
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        p = sutherland_hodgman_one_axis(bounds, edge, &p, exclusive);
    }
    p
}

fn sutherland_hodgman_one_axis(
    bounds: &Rectangle,
    edge: Edge,
    v: &[Point],
    exclusive: bool,
) -> Vec<Point> {
    let Some(&last) = v.last() else {
        return vec![];
    };
    let inside = |p: &Point| match (exclusive, edge) {
        (true, Edge::Right) => p.x < bounds.right(),
        (true, Edge::Bottom) => p.y < bounds.bottom(),
        _ => is_inside(bounds, edge, p),
    };
    let mut polygon = vec![];
    let mut s = last;
    for &p in v {
        let p_in = inside(&p);
        let s_in = inside(&s);
        if s_in && p_in {
            polygon.push(p);
        } else if s_in && !p_in {
            polygon.push(line_intercept(bounds, edge, &s, &p));
        } else if !s_in && p_in {
            polygon.push(line_intercept(bounds, edge, &s, &p));
            polygon.push(p);
        }
        s = p;
    }
    polygon
}

/// quirk: when the region is not simplified (`complexity == 0` or fewer rectangles than
/// `complexity`) JS gets a `Region` back, `simplified.length` is `undefined`, and nothing is
/// inflated.
pub fn simplify_and_inflate_region(
    region: &Region,
    complexity: usize,
    inflation_amount: f64,
) -> Vec<Rectangle> {
    let mut simplified = simplify_region(region, complexity);
    if complexity == 0 || region.size() < complexity {
        return simplified;
    }
    for r in &mut simplified {
        r.inflate(inflation_amount, inflation_amount);
    }
    simplified
}

/// JS returns `region.clone()` (a Region) when not simplifying; here its rectangles.
pub fn simplify_region(region: &Region, complexity: usize) -> Vec<Rectangle> {
    if complexity == 0 || region.size() < complexity {
        return region.rectangles.clone();
    }
    let size = region.size();
    (0..complexity)
        .map(|i| {
            let start_index = (i * size) / complexity;
            let length = size.min(((i + 1) * size) / complexity) - start_index;
            get_region_bounds(region, start_index, length)
        })
        .collect()
}

/// JS `splitRectangle(rect, rects)`: fills `rects` in place.
pub fn split_rectangle(rect: &Rectangle, rects: &mut [Rectangle]) {
    let height = rect.height;
    let n = rects.len() as f64;
    for (i, slot) in rects.iter_mut().enumerate() {
        let i = i as f64;
        *slot = Rectangle::new(
            rect.left(),
            rect.top() + ((height * i) / n),
            rect.right() - rect.left(),
            (height * (i + 1.0)) / n - (height * i) / n,
        );
    }
}

/// JS `getScans(vertices)` (24.8 fixed point edge walk).
pub fn get_scans(vertices: &[Point]) -> Vec<Scanline> {
    let length = vertices.len();
    let mut ymax = 0.0f64;

    // Build edge table
    let mut edge_table: Vec<UtilityEdge> = Vec::with_capacity(length);
    for i in 0..length {
        let mut top = vertices[i];
        let mut bottom = vertices[(i + 1) % length];
        if top.y > bottom.y {
            std::mem::swap(&mut top, &mut bottom);
        }
        let dy = bottom.y - top.y;
        if dy != 0.0 {
            edge_table.push(UtilityEdge::new(
                top.y,
                bottom.y,
                to_int32(top.x).wrapping_shl(8) as f64,
                to_int32(bottom.x - top.x).wrapping_shl(8) as f64 / dy,
            ));
            ymax = ymax.max(bottom.y);
        }
    }
    // Sort edge table by miny (the JS selection sorts, here O(n log n): spans depend only on the
    // x values, so the order among equal keys does not matter).
    edge_table.sort_by(|a, b| a.miny.total_cmp(&b.miny));
    let Some(first) = edge_table.first() else {
        return vec![];
    };

    // (JS first pre-counts the scanlines only to size the result array; not needed here.)
    let mut scans = vec![];
    let mut active: Vec<usize> = vec![];
    let mut yscan = first.miny;
    // Edges before `next` have miny < yscan: they were activated, or never will be.
    let mut next = 0;

    // Repeat until both the ET and AET are empty
    while yscan <= ymax {
        // Move any edges from the ET to the AET where yscan == miny
        while next < edge_table.len() && edge_table[next].miny <= yscan {
            if edge_table[next].miny == yscan {
                active.push(next);
            }
            next += 1;
        }

        // Sort the AET on x
        active.sort_by(|&a, &b| edge_table[a].x.total_cmp(&edge_table[b].x));

        // For each pair of entries in the AET, fill in pixels between their info
        for i in (0..active.len()).step_by(2) {
            let el = edge_table[active[i]];
            let er = edge_table[active[i + 1]];
            let startx = to_int32(el.x + 255.0) >> 8; // ceil(x)
            let endx = to_int32(er.x) >> 8; // floor(x)
            scans.push(Scanline::new(
                startx,
                to_int32(yscan),
                endx.wrapping_sub(startx),
            ));
        }

        yscan += 1.0;

        // Remove from the AET any edge where yscan == maxy
        active.retain(|&a| edge_table[a].maxy != yscan);

        // Update x for each entry in AET
        for &a in &active {
            edge_table[a].x += edge_table[a].dxdy;
        }
    }

    scans
}

pub fn get_region_bounds(region: &Region, start_index: usize, length: usize) -> Rectangle {
    if region.size() == 0 {
        return Rectangle::empty();
    }
    let rects = &region.rectangles;
    let first = &rects[start_index];
    let (mut left, mut top, mut right, mut bottom) =
        (first.left(), first.top(), first.right(), first.bottom());
    for rect in &rects[start_index + 1..start_index + length] {
        if rect.left() < left {
            left = rect.left();
        }
        if rect.top() < top {
            top = rect.top();
        }
        if rect.right() > right {
            right = rect.right();
        }
        if rect.bottom() > bottom {
            bottom = rect.bottom();
        }
    }
    Rectangle::new(left, top, right - left, bottom - top)
}

// `!(a < b)` mirrors JS NaN semantics (NaN is inside); do not rewrite as `a >= b`.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn is_inside(bounds: &Rectangle, edge: Edge, p: &Point) -> bool {
    match edge {
        Edge::Left => !(p.x < bounds.left()),
        Edge::Right => !(p.x > bounds.right()),
        Edge::Top => !(p.y < bounds.top()),
        Edge::Bottom => !(p.y > bounds.bottom()),
    }
}

fn line_intercept(bounds: &Rectangle, edge: Edge, a: &Point, b: &Point) -> Point {
    // JS compares object identity; value equality is equivalent here (see sutherland_hodgman).
    if a == b {
        return *a;
    }
    match edge {
        Edge::Bottom => {
            assert!(b.y != a.y, "No intercept found");
            Point::new(
                a.x + (((b.x - a.x) * (bounds.bottom() - a.y)) / (b.y - a.y)),
                bounds.bottom(),
            )
        }
        Edge::Left => {
            assert!(b.x != a.x, "No intercept found");
            Point::new(
                bounds.left(),
                a.y + (((b.y - a.y) * (bounds.left() - a.x)) / (b.x - a.x)),
            )
        }
        Edge::Right => {
            assert!(b.x != a.x, "No intercept found");
            Point::new(
                bounds.right(),
                a.y + (((b.y - a.y) * (bounds.right() - a.x)) / (b.x - a.x)),
            )
        }
        Edge::Top => {
            assert!(b.y != a.y, "No intercept found");
            Point::new(
                a.x + (((b.x - a.x) * (bounds.top() - a.y)) / (b.y - a.y)),
                bounds.top(),
            )
        }
    }
}

pub fn points_to_constrained_rectangle(a: Point, b: Point) -> Rectangle {
    let mut rect = points_to_rectangle(a, b);
    let min_wh = rect.width.min(rect.height);
    rect.width = min_wh;
    rect.height = min_wh;
    if rect.y != a.y {
        rect.y = a.y - min_wh;
    }
    if rect.x != a.x {
        rect.x = a.x - min_wh;
    }
    rect
}

fn vectors(m: &MatrixElements, v: Point) -> Point {
    Point::new(m[0][0] * v.x + m[0][1] * v.y, m[1][0] * v.x + m[1][1] * v.y)
}

pub fn get_angle_of_transform(matrix: &MatrixElements) -> f64 {
    let p = vectors(matrix, Point::new(1.0, 0.0));
    to_degrees(p.y.atan2(p.x))
}

pub fn is_transform_flipped(matrix: &MatrixElements) -> bool {
    let pt_xt = transform_one_vector(matrix, Point::new(1.0, 0.0));
    let mut angle_x = pt_xt.y.atan2(pt_xt.x) * (180.0 / std::f64::consts::PI);
    let pt_yt = transform_one_vector(matrix, Point::new(0.0, 1.0));
    let mut angle_y = (pt_yt.y.atan2(pt_yt.x) * (180.0 / std::f64::consts::PI)) - 90.0;
    while angle_x < 0.0 {
        angle_x += 360.0;
    }
    while angle_y < 0.0 {
        angle_y += 360.0;
    }
    let angle_delta = (angle_x - angle_y).abs();
    angle_delta > 1.0 && angle_delta < 359.0
}

pub fn points_to_rectangle(a: Point, b: Point) -> Rectangle {
    Rectangle::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (a.x - b.x).abs() + 1.0,
        (a.y - b.y).abs() + 1.0,
    )
}

pub fn rectangle_from_center(center: Point, half_size: f64) -> Rectangle {
    let mut ret = Rectangle::new(center.x, center.y, 0.0, 0.0);
    ret.inflate(half_size, half_size);
    ret
}

pub fn transform_one_point(matrix: &MatrixElements, point: Point) -> Point {
    Point::new(
        matrix[0][0] * point.x + matrix[0][1] * point.y + matrix[0][2],
        matrix[1][0] * point.x + matrix[1][1] * point.y + matrix[1][2],
    )
}

pub fn transform_one_vector(matrix: &MatrixElements, vector: Point) -> Point {
    vectors(matrix, vector)
}

pub fn normalize_vector2(vec: Point) -> Point {
    let magnitude = magnitude(vec);
    if magnitude == 0.0 {
        Point::new(0.0, 0.0)
    } else {
        Point::new(vec.x / magnitude, vec.y / magnitude)
    }
}

/// JS `getProjection`: returns `(yhat, yhat_len, z)`.
pub fn get_projection(point_y: Point, point_u: Point) -> (Point, f64, Point) {
    if point_u.x == 0.0 && point_u.y == 0.0 {
        return (Point::new(0.0, 0.0), 0.0, Point::new(0.0, 0.0));
    }
    let y_dot_u = dot_product(point_y, point_u);
    let u_dot_u = dot_product(point_u, point_u);
    let yhat_len = y_dot_u / u_dot_u;
    let yhat = multiply_vector(point_u, yhat_len);
    let z = subtract_vectors(point_y, yhat);
    (yhat, yhat_len, z)
}

pub fn dot_product(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

pub fn multiply_vector(point: Point, scalar: f64) -> Point {
    Point::new(point.x * scalar, point.y * scalar)
}

pub fn subtract_vectors(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

pub fn distance(a: Point, b: Point) -> f64 {
    magnitude(Point::new(a.x - b.x, a.y - b.y))
}

pub fn magnitude(point: Point) -> f64 {
    (point.x.powi(2) + point.y.powi(2)).sqrt()
}

/// `Math.min(Math.max(value, min), max)`.
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

pub fn to_radians(degrees: f64) -> f64 {
    degrees * (std::f64::consts::PI / 180.0)
}

pub fn to_degrees(radians: f64) -> f64 {
    radians * (180.0 / std::f64::consts::PI)
}
