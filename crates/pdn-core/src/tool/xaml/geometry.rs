use super::{MAX_SEGMENTS, Result, invalid};
use crate::{
    canvas::{
        FillRule, Path2D,
        path::{AnalyticArc, PathFlags, Verb},
    },
    selection::{CombineMode, path::GraphicsPath},
    util::geom::{Point, Rectangle},
};
use std::f64::consts::{PI, TAU};
pub(super) const IDENTITY: [f64; 6] = [1., 0., 0., 1., 0., 0.];
#[derive(Clone)]
pub(super) struct Geometry {
    pub path: Path2D,
    pub rule: FillRule,
    pub infinite: bool,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            path: Path2D::new(),
            rule: FillRule::EvenOdd,
            infinite: false,
        }
    }
}
pub(super) fn mapped(p: &Path2D, m: [f64; 6]) -> Path2D {
    let mut out = Path2D::new();
    out.push_mapped(p, &m);
    out
}
pub(super) fn compose(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    // a, then b
    [
        b[0] * a[0] + b[2] * a[1],
        b[1] * a[0] + b[3] * a[1],
        b[0] * a[2] + b[2] * a[3],
        b[1] * a[2] + b[3] * a[3],
        b[0] * a[4] + b[2] * a[5] + b[4],
        b[1] * a[4] + b[3] * a[5] + b[5],
    ]
}
pub(super) fn filled(p: &Path2D) -> Path2D {
    p.fill_path()
}
fn pt(x: f64, y: f64) -> [f64; 2] {
    [x, y]
}
fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn curve(q: &[[f64; 2]], t: f64) -> [f64; 2] {
    let mut p = q.to_vec();
    for n in (1..p.len()).rev() {
        for i in 0..n {
            p[i] = lerp(p[i], p[i + 1], t)
        }
    }
    p[0]
}

pub fn path_bounds(path: &Path2D) -> Rectangle {
    let hollow = path.flags().iter().any(|f| !f.filled || !f.stroked);
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut current = [0.; 2];
    let mut start = current;
    let mut add = |p: [f64; 2]| {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    };
    for (v, flags) in path.verbs().iter().zip(path.flags()) {
        let mut points = Vec::new();
        let end = match *v {
            Verb::Move(x, y) => {
                current = pt(x, y);
                start = current;
                continue;
            }
            Verb::Line(x, y) => {
                points.extend([current, pt(x, y)]);
                pt(x, y)
            }
            Verb::Close => {
                points.extend([current, start]);
                start
            }
            Verb::Quad(x, y, ex, ey) => {
                let q = [current, pt(x, y), pt(ex, ey)];
                points.extend([q[0], q[2]]);
                for axis in 0..2 {
                    let d = q[0][axis] - 2. * q[1][axis] + q[2][axis];
                    if d != 0. {
                        let t = (q[0][axis] - q[1][axis]) / d;
                        if t > 0. && t < 1. {
                            points.push(curve(&q, t));
                        }
                    }
                }
                q[2]
            }
            Verb::Cubic(x, y, u, v, ex, ey) => {
                let q = [current, pt(x, y), pt(u, v), pt(ex, ey)];
                points.extend([q[0], q[3]]);
                for axis in 0..2 {
                    let a = -q[0][axis] + 3. * q[1][axis] - 3. * q[2][axis] + q[3][axis];
                    let b = 2. * (q[0][axis] - 2. * q[1][axis] + q[2][axis]);
                    let c = q[1][axis] - q[0][axis];
                    let roots = if a == 0. {
                        vec![-c / b]
                    } else {
                        let d = b * b - 4. * a * c;
                        if d >= 0. {
                            let r = d.sqrt();
                            vec![(-b + r) / (2. * a), (-b - r) / (2. * a)]
                        } else {
                            vec![]
                        }
                    };
                    for t in roots {
                        if t > 0. && t < 1. {
                            points.push(curve(&q, t));
                        }
                    }
                }
                q[3]
            }
            Verb::Arc(a) => {
                points.extend([a.point(a.start), a.point(a.start + a.sweep)]);
                for axis in 0..2 {
                    let t = a.v[axis].atan2(a.u[axis]);
                    for t in [t, t + PI] {
                        let offset = if a.sweep >= 0. {
                            (t - a.start).rem_euclid(TAU)
                        } else {
                            (a.start - t).rem_euclid(TAU)
                        };
                        if offset <= a.sweep.abs() + 1e-12 {
                            points.push(a.point(t));
                        }
                    }
                }
                a.point(a.start + a.sweep)
            }
        };
        if !hollow || flags.stroked {
            for point in points {
                add(point)
            }
        }
        current = end;
    }
    if !lo[0].is_finite() {
        Rectangle::default()
    } else {
        Rectangle::new(lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1])
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn endpoint_arc(
    p: &mut Path2D,
    start: [f64; 2],
    end: [f64; 2],
    radii: [f64; 2],
    rotation: f64,
    large: bool,
    clockwise: bool,
) -> Result<()> {
    if start == end {
        return Ok(());
    }
    let [mut rx, mut ry] = radii.map(f64::abs);
    if rx == 0. || ry == 0. {
        p.line_to(end[0], end[1]);
        return Ok(());
    }
    let (s, c) = rotation.to_radians().sin_cos();
    let d = [(start[0] - end[0]) / 2., (start[1] - end[1]) / 2.];
    let x = c * d[0] + s * d[1];
    let y = -s * d[0] + c * d[1];
    let lambda = (x / rx).powi(2) + (y / ry).powi(2);
    if lambda > 1. {
        let k = lambda.sqrt();
        rx *= k;
        ry *= k;
    }
    let xn = x / rx;
    let yn = y / ry;
    let d = xn * xn + yn * yn;
    let sign = if large == clockwise { -1. } else { 1. };
    let k = sign * ((1. - d) / d).max(0.).sqrt();
    let cx = k * rx * yn;
    let cy = -k * ry * xn;
    let center = [
        c * cx - s * cy + (start[0] + end[0]) / 2.,
        s * cx + c * cy + (start[1] + end[1]) / 2.,
    ];
    let a = [(x - cx) / rx, (y - cy) / ry];
    let b = [(-x - cx) / rx, (-y - cy) / ry];
    let t = a[1].atan2(a[0]);
    let mut sweep = (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]);
    if clockwise && sweep < 0. {
        sweep += TAU
    } else if !clockwise && sweep > 0. {
        sweep -= TAU
    }
    if !center
        .iter()
        .chain([rx, ry, t, sweep].iter())
        .all(|v| v.is_finite())
    {
        return Err(invalid("arc overflow"));
    }
    p.analytic_arc(AnalyticArc {
        center,
        u: [rx * c, rx * s],
        v: [-ry * s, ry * c],
        start: t,
        sweep,
    });
    Ok(())
}
fn line(out: &mut Path2D, p: [f64; 2], flags: PathFlags) -> Result<()> {
    if out.verbs().len() >= MAX_SEGMENTS {
        return Err(invalid("geometry subdivision limit"));
    }
    let i = out.verbs().len();
    out.line_to(p[0], p[1]);
    out.annotate_from(i, flags);
    Ok(())
}
fn subdivide(
    q: &[[f64; 2]],
    tol: f64,
    depth: u32,
    out: &mut Path2D,
    flags: PathFlags,
) -> Result<()> {
    // Distance to the chord segment bounds a Bezier's convex hull, including collinear reversals.
    let a = q[0];
    let b = q[q.len() - 1];
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = d[0] * d[0] + d[1] * d[1];
    let err = q
        .iter()
        .map(|&p| {
            let t = if len == 0. {
                0.
            } else {
                ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len
            };
            distance(p, lerp(a, b, t.clamp(0., 1.)))
        })
        .fold(0., f64::max);
    if err <= tol {
        return line(out, b, flags);
    }
    if depth >= 32 {
        return Err(invalid("geometry tolerance exceeds subdivision budget"));
    }
    let mut work = q.to_vec();
    let mut left = vec![a];
    let mut right = vec![b];
    for n in (1..q.len()).rev() {
        for i in 0..n {
            work[i] = lerp(work[i], work[i + 1], 0.5)
        }
        left.push(work[0]);
        right.push(work[n - 1]);
    }
    right.reverse();
    subdivide(&left, tol, depth + 1, out, flags)?;
    subdivide(&right, tol, depth + 1, out, flags)
}
pub(super) fn flatten(p: &Path2D, tol: f64, allow_curves: bool) -> Result<Path2D> {
    if !tol.is_finite() || tol <= 0. {
        return Err(invalid("invalid flattening tolerance"));
    }
    let mut out = Path2D::new();
    let mut current = [0.; 2];
    let mut start = current;
    for (v, &f) in p.verbs().iter().zip(p.flags()) {
        let i = out.verbs().len();
        match *v {
            Verb::Move(x, y) => {
                out.move_to(x, y);
                current = pt(x, y);
                start = current;
            }
            Verb::Line(x, y) => {
                line(&mut out, pt(x, y), f)?;
                current = pt(x, y);
            }
            Verb::Close => {
                out.close_path();
                current = start;
            }
            Verb::Quad(x, y, ex, ey) => {
                let end = pt(ex, ey);
                if allow_curves {
                    let a = lerp(current, pt(x, y), 2. / 3.);
                    let b = lerp(end, pt(x, y), 2. / 3.);
                    out.bezier_curve_to(a[0], a[1], b[0], b[1], ex, ey);
                } else {
                    subdivide(&[current, pt(x, y), end], tol, 0, &mut out, f)?
                }
                current = end;
            }
            Verb::Cubic(x, y, u, v, ex, ey) => {
                if allow_curves {
                    out.bezier_curve_to(x, y, u, v, ex, ey);
                } else {
                    subdivide(
                        &[current, pt(x, y), pt(u, v), pt(ex, ey)],
                        tol,
                        0,
                        &mut out,
                        f,
                    )?
                }
                current = pt(ex, ey);
            }
            Verb::Arc(a) => {
                let radius = distance(a.u, [0., 0.]) + distance(a.v, [0., 0.]);
                let step = if radius == 0. {
                    TAU
                } else {
                    (8. * tol / radius).sqrt().min(PI / 2.)
                };
                let count = (a.sweep.abs() / step).ceil().max(1.);
                if !count.is_finite() || count > MAX_SEGMENTS as f64 {
                    return Err(invalid("arc tolerance exceeds subdivision budget"));
                }
                for j in 1..=count as usize {
                    line(&mut out, a.point(a.start + a.sweep * j as f64 / count), f)?;
                }
                current = a.point(a.start + a.sweep);
            }
        }
        out.annotate_from(i, f);
        if out.verbs().len() > MAX_SEGMENTS {
            return Err(invalid("geometry subdivision limit"));
        }
    }
    Ok(out)
}
fn polygons(p: &Path2D, tol: f64) -> Result<Vec<Vec<[f64; 2]>>> {
    let p = flatten(&filled(p), tol, false)?;
    let mut polys = Vec::new();
    let mut points = Vec::new();
    for v in p.verbs() {
        match *v {
            Verb::Move(x, y) => {
                if !points.is_empty() {
                    polys.push(std::mem::take(&mut points))
                }
                points.push(pt(x, y));
            }
            Verb::Line(x, y) => points.push(pt(x, y)),
            Verb::Close => {}
            _ => unreachable!(),
        }
    }
    if !points.is_empty() {
        polys.push(points)
    }
    Ok(polys)
}
fn poly_path(polys: impl IntoIterator<Item = Vec<[f64; 2]>>) -> Path2D {
    let mut p = Path2D::new();
    for pts in polys {
        if let Some(a) = pts.first() {
            p.move_to(a[0], a[1]);
            for b in &pts[1..] {
                p.line_to(b[0], b[1]);
            }
            p.close_path();
        }
    }
    p
}
// Bound both input and potential intersection work before entering i_overlay. These are
// product resource limits, not Paint.NET geometry limits; no tolerance is silently relaxed.
fn boolean_budget(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>]) -> Result<()> {
    let count: usize = a.iter().chain(b).map(Vec::len).sum();
    if count > 4096 {
        return Err(invalid("boolean edge limit (4096)"));
    }
    let mut boxes = Vec::with_capacity(count);
    for poly in a.iter().chain(b) {
        for (i, &p) in poly.iter().enumerate() {
            let q = poly[(i + 1) % poly.len()];
            boxes.push([
                p[0].min(q[0]),
                p[1].min(q[1]),
                p[0].max(q[0]),
                p[1].max(q[1]),
            ]);
        }
    }
    let mut pairs = 0;
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            if a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] {
                pairs += 1;
                if pairs > 65536 {
                    return Err(invalid("boolean intersection-work limit (65536)"));
                }
            }
        }
    }
    Ok(())
}
pub(super) fn combine(a: &Geometry, b: &Geometry, mode: CombineMode, tol: f64) -> Result<Path2D> {
    use i_overlay::{
        core::{fill_rule::FillRule as Rule, overlay_rule::OverlayRule},
        float::single::SingleFloatOverlay,
    };
    let mut aa = polygons(&a.path, tol)?;
    let mut bb = polygons(&b.path, tol)?;
    boolean_budget(&aa, &bb)?;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in aa.iter().chain(&bb).flatten() {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    if !lo[0].is_finite() {
        return Ok(Path2D::new());
    }
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let scale = if span == 0. { 1. } else { 65536. / span };
    if !scale.is_finite() {
        return Err(invalid("boolean normalization overflow"));
    }
    for p in aa.iter_mut().chain(&mut bb).flatten() {
        p[0] = (p[0] - lo[0]) * scale;
        p[1] = (p[1] - lo[1]) * scale;
    }
    let normalize = |polys: Vec<Vec<[f64; 2]>>, rule: FillRule| -> GraphicsPath {
        let empty: Vec<Vec<[f64; 2]>> = vec![];
        let normalized = polys.overlay(
            &empty,
            OverlayRule::Union,
            if rule == FillRule::EvenOdd {
                Rule::EvenOdd
            } else {
                Rule::NonZero
            },
        );
        GraphicsPath {
            polygons: normalized
                .into_iter()
                .flatten()
                .map(|p| p.into_iter().map(|[x, y]| Point::new(x, y)).collect())
                .collect(),
        }
    };
    let aa = normalize(aa, a.rule);
    let bb = normalize(bb, b.rule);
    boolean_budget(&aa.as_polys(), &bb.as_polys())?;
    let c = GraphicsPath::combine(&aa, mode, &bb);
    if c.polygons.iter().map(Vec::len).sum::<usize>() > MAX_SEGMENTS {
        return Err(invalid("boolean result limit"));
    }
    Ok(poly_path(c.polygons.into_iter().map(|p| {
        p.into_iter()
            .map(|p| [p.x / scale + lo[0], p.y / scale + lo[1]])
            .collect()
    })))
}
pub(super) fn outline(g: &Geometry, tol: f64) -> Result<Path2D> {
    combine(g, &Geometry::default(), CombineMode::Union, tol)
}

#[derive(Clone)]
pub(super) struct StrokeStyle {
    pub start: u8,
    pub end: u8,
    pub dash_cap: u8,
    pub join: u8,
    pub miter: f64,
    pub dashes: Vec<f64>,
    pub offset: f64,
}
impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            dash_cap: 1,
            join: 0,
            miter: 10.,
            dashes: vec![],
            offset: 0.,
        }
    }
}
impl StrokeStyle {
    pub fn toolbar(width: f64, dash: super::DashStyle) -> Self {
        let mut s = Self::default();
        if let Some((d, o)) = super::super::shapes::dash_pattern(dash, width) {
            s.dashes = d.iter().map(|v| f64::from(*v) / width).collect();
            s.offset = f64::from(o) / width;
            s.dash_cap = 0;
        }
        s
    }
}
#[derive(Default)]
struct Figure {
    points: Vec<[f64; 2]>,
    flags: Vec<PathFlags>,
    closed: bool,
}
fn figures(p: &Path2D, tol: f64) -> Result<Vec<Figure>> {
    let p = flatten(p, tol, false)?;
    let mut result = Vec::new();
    let mut f = Figure::default();
    for (v, &flags) in p.verbs().iter().zip(p.flags()) {
        match *v {
            Verb::Move(x, y) => {
                if !f.points.is_empty() {
                    result.push(std::mem::take(&mut f))
                }
                f.points.push(pt(x, y));
            }
            Verb::Line(x, y) => {
                f.points.push(pt(x, y));
                f.flags.push(flags);
            }
            Verb::Close => {
                if let Some(&start) = f.points.first() {
                    f.points.push(start);
                    f.flags.push(flags);
                    f.closed = true
                }
            }
            _ => unreachable!(),
        }
    }
    if !f.points.is_empty() {
        result.push(f)
    }
    Ok(result)
}
pub(super) fn stroke_polylines(p: &Path2D, tol: f64) -> Result<Vec<Vec<[f64; 2]>>> {
    let mut out = vec![];
    for f in figures(p, tol)? {
        let mut run = vec![];
        for (i, flags) in f.flags.iter().enumerate() {
            if flags.stroked {
                if run.is_empty() {
                    run.push(f.points[i]);
                }
                run.push(f.points[i + 1]);
            } else if !run.is_empty() {
                out.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            out.push(run)
        }
    }
    Ok(out)
}
fn from_skia(p: &tiny_skia::Path) -> Path2D {
    let mut out = Path2D::new();
    for s in p.segments() {
        use tiny_skia::PathSegment::*;
        match s {
            MoveTo(p) => out.move_to(p.x as f64, p.y as f64),
            LineTo(p) => out.line_to(p.x as f64, p.y as f64),
            QuadTo(a, b) => out.quadratic_curve_to(a.x as f64, a.y as f64, b.x as f64, b.y as f64),
            CubicTo(a, b, c) => out.bezier_curve_to(
                a.x as f64, a.y as f64, b.x as f64, b.y as f64, c.x as f64, c.y as f64,
            ),
            Close => out.close_path(),
        }
    }
    out
}
fn cap(out: &mut Path2D, p: [f64; 2], other: [f64; 2], cap: u8, width: f64) {
    let radius = width / 2.;
    let len = distance(p, other);
    if cap == 0 || len == 0. {
        return;
    }
    let d = [
        (p[0] - other[0]) / len * radius,
        (p[1] - other[1]) / len * radius,
    ];
    let n = [-d[1], d[0]];
    let mut c = Path2D::new();
    if cap == 2 {
        c.ellipse(p[0], p[1], radius, radius, 0., 0., TAU, false);
        c.close_path();
    } else {
        c.move_to(p[0] + n[0], p[1] + n[1]);
        if cap == 1 {
            c.line_to(p[0] + n[0] + d[0], p[1] + n[1] + d[1]);
            c.line_to(p[0] - n[0] + d[0], p[1] - n[1] + d[1]);
        } else {
            c.line_to(p[0] + d[0], p[1] + d[1]);
        }
        c.line_to(p[0] - n[0], p[1] - n[1]);
        c.close_path();
    }
    out.push_mapped(&c, &IDENTITY);
}
fn stroke_run(
    out: &mut Path2D,
    pts: &[[f64; 2]],
    closed: bool,
    width: f64,
    style: &StrokeStyle,
    start: u8,
    end: u8,
) -> Result<()> {
    if pts.len() < 2 {
        return Ok(());
    }
    if pts.iter().flatten().any(|v| !(*v as f32).is_finite()) {
        return Err(invalid("stroke coordinate overflow"));
    }
    let mut p = Path2D::new();
    p.move_to(pts[0][0], pts[0][1]);
    for q in &pts[1..] {
        p.line_to(q[0], q[1]);
    }
    if closed {
        p.close_path()
    }
    if let Some(path) = p.to_skia() {
        let stroke = tiny_skia::Stroke {
            width: width as f32,
            miter_limit: style.miter as f32,
            line_join: match style.join {
                1 => tiny_skia::LineJoin::Bevel,
                2 => tiny_skia::LineJoin::Round,
                _ => tiny_skia::LineJoin::MiterClip,
            },
            ..Default::default()
        };
        if let Some(s) = path.stroke(&stroke, 4.) {
            out.push_mapped(&from_skia(&s), &IDENTITY)
        }
    }
    if !closed {
        cap(out, pts[0], pts[1], start, width);
        cap(out, pts[pts.len() - 1], pts[pts.len() - 2], end, width)
    }
    if out.verbs().len() > MAX_SEGMENTS {
        return Err(invalid("stroke geometry limit"));
    }
    Ok(())
}
// Split forced round joins after dashing, so neither dash phase nor gaps change.
fn stroke_joined(
    out: &mut Path2D,
    pts: &[[f64; 2]],
    smooth: &[bool],
    closed: bool,
    width: f64,
    style: &StrokeStyle,
    caps: [u8; 2],
) -> Result<()> {
    if pts.len() < 2 {
        return Ok(());
    }
    let first = smooth
        .iter()
        .take(pts.len() - usize::from(closed))
        .position(|v| *v);
    if first.is_none() {
        return stroke_run(out, pts, closed, width, style, caps[0], caps[1]);
    }
    let mut points = pts.to_vec();
    let mut joins = smooth.to_vec();
    if closed {
        points.pop();
        joins.pop();
        points.rotate_left(first.unwrap());
        joins.rotate_left(first.unwrap());
        points.push(points[0]);
        joins.push(joins[0]);
        cap(out, points[0], points[1], 2, width);
    }
    let mut start = 0;
    for i in 1..points.len() - 1 {
        if joins[i] {
            stroke_run(
                out,
                &points[start..=i],
                false,
                width,
                style,
                if start == 0 && !closed { caps[0] } else { 0 },
                0,
            )?;
            cap(out, points[i], points[i + 1], 2, width);
            start = i;
        }
    }
    stroke_run(
        out,
        &points[start..],
        false,
        width,
        style,
        if start == 0 && !closed { caps[0] } else { 0 },
        if closed { 0 } else { caps[1] },
    )
}
fn dashed(
    out: &mut Path2D,
    pts: &[[f64; 2]],
    smooth: &[bool],
    closed: bool,
    width: f64,
    s: &StrokeStyle,
) -> Result<()> {
    if s.dashes.is_empty() {
        return stroke_joined(out, pts, smooth, closed, width, s, [s.start, s.end]);
    }
    let pattern: Vec<_> = s.dashes.iter().map(|v| v * width).collect();
    let total: f64 = pattern.iter().sum();
    let mut offset = (s.offset * width).rem_euclid(total);
    let mut index = 0;
    while offset >= pattern[index] {
        offset -= pattern[index];
        index = (index + 1) % pattern.len();
    }
    let mut remaining = pattern[index] - offset;
    let mut run = Vec::new();
    let mut joins = Vec::new();
    let mut count = 0;
    let mut at_start = true;
    for (segment, pair) in pts.windows(2).enumerate() {
        let len = distance(pair[0], pair[1]);
        if len == 0. {
            continue;
        }
        let mut along = 0.;
        while along < len {
            count += 1;
            if count > MAX_SEGMENTS {
                return Err(invalid("dash subdivision limit"));
            }
            let step = remaining.min(len - along);
            let a = lerp(pair[0], pair[1], along / len);
            let reaches_end = step == len - along;
            let b = if reaches_end {
                pair[1]
            } else {
                lerp(pair[0], pair[1], (along + step) / len)
            };
            if index % 2 == 0 {
                if step == 0. && s.dash_cap != 0 {
                    let delta = [
                        (pair[1][0] - pair[0][0]) / len,
                        (pair[1][1] - pair[0][1]) / len,
                    ];
                    cap(
                        out,
                        a,
                        [a[0] - delta[0], a[1] - delta[1]],
                        s.dash_cap,
                        width,
                    );
                    cap(
                        out,
                        a,
                        [a[0] + delta[0], a[1] + delta[1]],
                        s.dash_cap,
                        width,
                    );
                }
                if run.is_empty() {
                    run.push(a);
                    joins.push(along == 0. && smooth[segment]);
                }
                run.push(b);
                joins.push(reaches_end && smooth[segment + 1]);
            }
            along += step;
            remaining -= step;
            if remaining <= f64::EPSILON * total {
                if !run.is_empty() {
                    let sc = if at_start && !closed {
                        s.start
                    } else {
                        s.dash_cap
                    };
                    stroke_joined(out, &run, &joins, false, width, s, [sc, s.dash_cap])?;
                    run.clear();
                    joins.clear();
                }
                at_start = false;
                index = (index + 1) % pattern.len();
                remaining = pattern[index];
            }
        }
    }
    if !run.is_empty() {
        stroke_joined(
            out,
            &run,
            &joins,
            false,
            width,
            s,
            [
                if at_start && !closed {
                    s.start
                } else {
                    s.dash_cap
                },
                if closed { s.dash_cap } else { s.end },
            ],
        )?;
    }
    Ok(())
}
pub(super) fn widen(
    p: &Path2D,
    width: f64,
    s: &StrokeStyle,
    tol: f64,
    transform: [f64; 6],
) -> Result<Path2D> {
    if width <= 0. {
        return Ok(Path2D::new());
    }
    if !width.is_finite() || width > f32::MAX as f64 {
        return Err(invalid("stroke thickness"));
    }
    let norm = (transform[0].hypot(transform[1]) + transform[2].hypot(transform[3])).max(1.);
    let local_tol = tol / norm;
    let mut out = Path2D::new();
    for f in figures(p, local_tol)? {
        let mut smooth: Vec<_> = f.flags.iter().map(|flag| flag.smooth).collect();
        smooth.push(f.closed && smooth.first().copied().unwrap_or(false));
        if f.flags.iter().all(|v| v.stroked) {
            dashed(&mut out, &f.points, &smooth, f.closed, width, s)?;
            continue;
        }
        let mut runs: Vec<(Vec<[f64; 2]>, Vec<bool>)> = Vec::new();
        let mut run = Vec::new();
        let mut joins = Vec::new();
        for (i, flag) in f.flags.iter().enumerate() {
            if !flag.stroked && !run.is_empty() {
                runs.push((std::mem::take(&mut run), std::mem::take(&mut joins)));
            }
            if flag.stroked {
                if run.is_empty() {
                    run.push(f.points[i]);
                    joins.push(smooth[i]);
                }
                run.push(f.points[i + 1]);
                joins.push(smooth[i + 1]);
            }
        }
        if !run.is_empty() {
            runs.push((run, joins));
        }
        if f.closed
            && runs.len() > 1
            && f.flags.first().is_some_and(|v| v.stroked)
            && f.flags.last().is_some_and(|v| v.stroked)
        {
            let first = runs.remove(0);
            let last = runs.last_mut().unwrap();
            last.0.extend_from_slice(&first.0[1..]);
            last.1.extend_from_slice(&first.1[1..]);
        }
        for (run, joins) in runs {
            dashed(&mut out, &run, &joins, false, width, s)?;
        }
    }
    // R93: tiny-skia's clipped-miter/f32 footprint is an accepted native approximation.
    // Normalize the union so opposite cap/stroker winding cannot cancel under Nonzero.
    let normalized = outline(
        &Geometry {
            path: out,
            rule: FillRule::NonZero,
            infinite: false,
        },
        local_tol,
    )?;
    Ok(mapped(&normalized, transform))
}
