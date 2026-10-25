//! Port of: js/src/engine/document/tool/tool/{PreviewShapeTool,LineTool,ShapesTool}.js
//! and history/memento/Shape{Draw,Edit,Commit}HistoryMemento.js (structure and point data).
//! Reference: paintdotjs @ b4e6fc5
//! R58 uses the shared transaction core; R89 requires explicit tracer closure.

use crate::canvas::{FillRule, Path2D};
use crate::pixel::IntRect;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Triangle,
    /// Appendix B B-P14 calibrates its pixels.
    RightTriangle,
    Diamond,
    Trapezoid,
    Parallelogram,
    Pentagon,
    Hexagon,
    Heptagon,
    Octagon,
    Star3,
    Star4,
    Star5,
    Star6,
    BlockArrow,
    NotchedArrow,
    PentagonArrow,
    ChevronArrow,
    CheckMark,
    Multiply,
    Heart,
    LightningBolt,
    Gear,
    RectangularCallout,
    RoundedCallout,
    EllipticalCallout,
    CloudCallout,
}
impl ShapeKind {
    /// Native catalogue order, also used by the existing grouped gallery.
    pub const ALL: [Self; 29] = [
        Self::Rectangle,
        Self::RoundedRectangle,
        Self::Ellipse,
        Self::Diamond,
        Self::Trapezoid,
        Self::Parallelogram,
        Self::Triangle,
        Self::RightTriangle,
        Self::Pentagon,
        Self::Hexagon,
        Self::Heptagon,
        Self::Octagon,
        Self::Star3,
        Self::Star4,
        Self::Star5,
        Self::Star6,
        Self::BlockArrow,
        Self::NotchedArrow,
        Self::PentagonArrow,
        Self::ChevronArrow,
        Self::RectangularCallout,
        Self::RoundedCallout,
        Self::EllipticalCallout,
        Self::CloudCallout,
        Self::LightningBolt,
        Self::CheckMark,
        Self::Multiply,
        Self::Gear,
        Self::Heart,
    ];
    pub fn from_value(v: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|k| k.value() == v)
            .unwrap_or(Self::Rectangle)
    }
    pub fn value(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::RoundedRectangle => "roundedRectangle",
            Self::Ellipse => "ellipse",
            Self::Triangle => "triangle",
            Self::RightTriangle => "rightTriangle",
            Self::Diamond => "diamond",
            Self::Trapezoid => "trapezoid",
            Self::Parallelogram => "parallelogram",
            Self::Pentagon => "pentagon",
            Self::Hexagon => "hexagon",
            Self::Heptagon => "heptagon",
            Self::Octagon => "octagon",
            Self::Star3 => "star3",
            Self::Star4 => "star4",
            Self::Star5 => "star5",
            Self::Star6 => "star6",
            Self::BlockArrow => "blockArrow",
            Self::NotchedArrow => "notchedArrow",
            Self::PentagonArrow => "pentagonArrow",
            Self::ChevronArrow => "chevronArrow",
            Self::CheckMark => "checkMark",
            Self::Multiply => "multiply",
            Self::Heart => "heart",
            Self::LightningBolt => "lightningBolt",
            Self::Gear => "gear",
            Self::RectangularCallout => "rectangularCallout",
            Self::RoundedCallout => "roundedCallout",
            Self::EllipticalCallout => "ellipticalCallout",
            Self::CloudCallout => "cloudCallout",
        }
    }
    pub fn aspect_ratio(self) -> f64 {
        if matches!(
            self,
            Self::Triangle
                | Self::Pentagon
                | Self::Hexagon
                | Self::Heptagon
                | Self::Octagon
                | Self::Star3
                | Self::Star4
                | Self::Star5
                | Self::Star6
        ) {
            let [l, t, r, b] = guide_bounds(self);
            (r - l) / (b - t)
        } else {
            1.0
        }
    }
    fn name(self) -> String {
        let key = match self {
            Self::Triangle => "isoscelesTriangle".into(),
            Self::RoundedCallout => "roundedRectangularCallout".into(),
            Self::Star3 | Self::Star4 | Self::Star5 | Self::Star6 => {
                format!("{}Point", self.value())
            }
            _ => self.value().into(),
        };
        crate::i18n::t(&format!("{key}Shape.name"))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawType {
    Outline,
    Interior,
    Both,
}
impl DrawType {
    pub fn from_value(v: &str) -> Self {
        match v {
            "fill" => Self::Interior,
            "both" => Self::Both,
            _ => Self::Outline,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashStyle {
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}
impl DashStyle {
    pub fn from_value(v: &str) -> Self {
        match v {
            "dash" => Self::Dash,
            "dot" => Self::Dot,
            "dashDot" => Self::DashDot,
            "dashDotDot" => Self::DashDotDot,
            _ => Self::Solid,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapStyle {
    Flat,
    Arrow,
    ArrowFilled,
    Rounded,
}
impl CapStyle {
    pub fn from_value(v: &str) -> Self {
        match v {
            "arrow" => Self::Arrow,
            "filledArrow" => Self::ArrowFilled,
            "round" => Self::Rounded,
            _ => Self::Flat,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveType {
    Straight,
    Spline,
    Bezier,
}
impl CurveType {
    pub fn from_value(v: &str) -> Self {
        match v {
            "straight" => Self::Straight,
            "bezier" => Self::Bezier,
            _ => Self::Spline,
        }
    }
}

pub fn snap_rect_points(mut start: [f64; 2], mut end: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    for i in 0..2 {
        (start[i], end[i]) = if start[i] <= end[i] {
            (start[i].floor(), end[i].ceil())
        } else {
            (start[i].ceil(), end[i].floor())
        };
        if start[i] == end[i] {
            end[i] += 1.0;
        }
    }
    (start, end)
}
pub fn snap_line_point(p: [f64; 2]) -> [f64; 2] {
    p.map(|v| v.floor() + 0.5)
}
fn poly(points: &[[f64; 2]], closed: bool) -> Path2D {
    let mut p = Path2D::new();
    if let Some(q) = points.first() {
        p.move_to(q[0], q[1]);
        for q in &points[1..] {
            p.line_to(q[0], q[1]);
        }
        if closed {
            p.close_path();
        }
    }
    p
}
fn ellipse(r: [f64; 4]) -> Path2D {
    let mut p = Path2D::new();
    p.ellipse(
        r[0] + r[2] / 2.0,
        r[1] + r[3] / 2.0,
        r[2] / 2.0,
        r[3] / 2.0,
        0.0,
        0.0,
        TAU,
        false,
    );
    p.close_path();
    p
}
fn rect_path(r: [f64; 4], radius: Option<f64>) -> Path2D {
    let mut p = Path2D::new();
    if let Some(v) = radius {
        p.round_rect(r[0], r[1], r[2], r[3], v.max(0.0).min(r[2].min(r[3]) / 2.0));
    } else {
        p.rect(r[0], r[1], r[2], r[3]);
    }
    p
}

pub fn guide(kind: ShapeKind) -> (Path2D, FillRule) {
    use ShapeKind::*;
    let p = match kind {
        Rectangle | RoundedRectangle => rect_path([0.0, 0.0, 1.0, 1.0], None),
        Ellipse => ellipse([0.0, 0.0, 1.0, 1.0]),
        Triangle => poly(
            &[[0.5, 0.0], [0.0, 0.75f64.sqrt()], [1.0, 0.75f64.sqrt()]],
            true,
        ),
        RightTriangle => poly(&[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0]], true),
        Diamond => poly(&[[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]], true),
        Trapezoid => poly(&[[0.25, 0.0], [0.75, 0.0], [1.0, 1.0], [0.0, 1.0]], true),
        Parallelogram => poly(&[[1.0, 0.0], [0.75, 1.0], [0.0, 1.0], [0.25, 0.0]], true),
        Pentagon | Hexagon | Heptagon | Octagon | Star3 | Star4 | Star5 | Star6 => {
            let (n, ratio) = match kind {
                Pentagon => (5, None),
                Hexagon => (6, None),
                Heptagon => (7, None),
                Octagon => (8, None),
                Star3 => (3, Some(0.2)),
                Star4 => (4, Some(0.25)),
                Star5 => (5, Some(0.381966)),
                _ => (6, Some(0.57735)),
            };
            let count = if ratio.is_some() { n * 2 } else { n };
            let points: Vec<_> = (0..count)
                .map(|i| {
                    let a = TAU * i as f64 / count as f64 - PI / 2.0
                        + if kind == Octagon { PI / 8.0 } else { 0.0 };
                    let r = if i % 2 == 1 {
                        ratio.unwrap_or(1.0)
                    } else {
                        1.0
                    };
                    [1.0 + r * a.cos(), 1.0 + r * a.sin()]
                })
                .collect();
            poly(&points, true)
        }
        BlockArrow => poly(
            &[
                [1.0, 0.5],
                [0.5, 0.0],
                [0.5, 0.25],
                [0.0, 0.25],
                [0.0, 0.75],
                [0.5, 0.75],
                [0.5, 1.0],
            ],
            true,
        ),
        NotchedArrow => poly(
            &[
                [1.0, 0.5],
                [0.5, 0.0],
                [0.5, 0.25],
                [0.0, 0.25],
                [0.25, 0.5],
                [0.0, 0.75],
                [0.5, 0.75],
                [0.5, 1.0],
            ],
            true,
        ),
        PentagonArrow => poly(
            &[[1.0, 0.5], [0.5, 0.0], [0.0, 0.0], [0.0, 1.0], [0.5, 1.0]],
            true,
        ),
        ChevronArrow => poly(
            &[
                [0.0, 0.0],
                [1.0 / 3.0, 0.5],
                [0.0, 1.0],
                [2.0 / 3.0, 1.0],
                [1.0, 0.5],
                [2.0 / 3.0, 0.0],
            ],
            true,
        ),
        CheckMark => poly(
            &[
                [0.85, 0.0],
                [1.0, 0.15],
                [0.375, 1.0],
                [0.0, 0.625],
                [0.15, 0.475],
                [0.352, 0.677],
            ],
            true,
        ),
        Multiply => poly(
            &[
                [0.15, 0.0],
                [0.5, 0.35],
                [0.85, 0.0],
                [1.0, 0.15],
                [0.65, 0.5],
                [1.0, 0.85],
                [0.85, 1.0],
                [0.5, 0.65],
                [0.15, 1.0],
                [0.0, 0.85],
                [0.35, 0.5],
                [0.0, 0.15],
            ],
            true,
        ),
        LightningBolt => poly(
            &[
                [29.965, 0.5047],
                [0.5047, 18.514],
                [26.9396, 39.3102],
                [17.968, 45.4352],
                [43.0051, 64.8427],
                [35.3201, 69.5556],
                [75.6158, 100.505],
                [51.855, 60.1204],
                [58.149, 56.0927],
                [39.2442, 32.3176],
                [44.0417, 28.4428],
            ],
            true,
        ),
        RectangularCallout => poly(
            &[
                [0.0, 0.0],
                [100.0, 0.0],
                [100.0, 75.0],
                [25.0, 75.0],
                [0.0, 100.0],
                [12.5, 75.0],
                [0.0, 75.0],
            ],
            true,
        ),
        Heart => {
            let mut p = Path2D::new();
            p.move_to(11.5, 20.5834);
            for [a, b, c, d, e, f] in [
                [10.3333, 16.25, 13.6667, 12.9167, 16.5, 12.4167],
                [19.3334, 11.9167, 21.6667, 14.25, 24.0, 16.5833],
                [26.3334, 14.25, 28.6667, 11.9167, 31.5, 12.4167],
                [34.3333, 12.9167, 37.6666, 16.25, 36.5, 20.5833],
                [35.3333, 24.9167, 29.6667, 30.25, 24.0, 35.5833],
                [18.3334, 30.25, 12.6667, 24.9167, 11.5, 20.5834],
            ] {
                p.bezier_curve_to(a, b, c, d, e, f);
            }
            p.close_path();
            p
        }
        EllipticalCallout => {
            let mut p = Path2D::new();
            p.move_to(0.195, 44.743);
            p.bezier_curve_to(0.195, 20.032, 22.342, 0.0, 49.902, 0.0);
            p.bezier_curve_to(77.658, 0.0, 99.805, 20.032, 99.805, 44.743);
            p.bezier_curve_to(100.0, 69.453, 77.463, 89.485, 49.902, 89.485);
            p.bezier_curve_to(42.201, 89.485, 34.538, 87.841, 27.901, 84.913);
            p.line_to(0.0, 100.0);
            p.line_to(14.441, 76.045);
            p.bezier_curve_to(5.628, 67.976, 0.0, 56.928, 0.195, 44.743);
            p.close_path();
            p
        }
        Gear => gear(),
        RoundedCallout => rounded_callout(),
        CloudCallout => cloud_callout(),
    };
    (
        p,
        if matches!(kind, Gear | RoundedCallout) {
            FillRule::NonZero
        } else {
            FillRule::EvenOdd
        },
    )
}
pub fn guide_bounds(kind: ShapeKind) -> [f64; 4] {
    bounds(&guide(kind).0)
}
fn bounds(p: &Path2D) -> [f64; 4] {
    p.to_skia()
        .and_then(|p| p.compute_tight_bounds())
        .map(|r| {
            [
                r.left() as f64,
                r.top() as f64,
                r.right() as f64,
                r.bottom() as f64,
            ]
        })
        .unwrap_or([0.0; 4])
}
pub fn fit_to_rect(path: &Path2D, b: [f64; 4], r: [f64; 4]) -> Path2D {
    let scale = |n: f64, d: f64| {
        let s = n / d;
        if s.is_finite() { s } else { 1.0 }
    };
    let (sx, sy) = (scale(r[2], b[2] - b[0]), scale(r[3], b[3] - b[1]));
    mapped(
        path,
        &[sx, 0.0, 0.0, sy, r[0] - sx * b[0], r[1] - sy * b[1]],
    )
}
fn mapped(p: &Path2D, m: &[f64; 6]) -> Path2D {
    let mut out = Path2D::new();
    out.push_mapped(p, m);
    out
}

#[derive(Clone, Debug)]
pub struct StrokeSpec {
    pub width: f64,
    pub round_join: bool,
    pub round_start: bool,
    pub round_end: bool,
    pub dash: Option<(Vec<f32>, f32)>,
}
impl StrokeSpec {
    fn new(width: f64, dash: DashStyle) -> Self {
        Self {
            width,
            round_join: false,
            round_start: false,
            round_end: false,
            dash: dash_pattern(dash, width),
        }
    }
}
pub fn dash_pattern(d: DashStyle, w: f64) -> Option<(Vec<f32>, f32)> {
    let p: &[f64] = match d {
        DashStyle::Solid => return None,
        DashStyle::Dash => &[3.0, 1.0],
        DashStyle::Dot => &[1.0, 1.0],
        DashStyle::DashDot => &[3.0, 1.0, 1.0, 1.0],
        DashStyle::DashDotDot => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    };
    Some((p.iter().map(|v| (v * w) as f32).collect(), (0.5 * w) as f32))
}
#[derive(Default)]
pub struct ShapeRenderData {
    pub interior: Option<(Path2D, FillRule)>,
    pub outline: Option<(Path2D, FillRule)>,
    pub outline_stroke: Option<(Path2D, StrokeSpec)>,
    pub tracer: Option<Vec<Vec<[f64; 2]>>>,
}

pub fn build_shape(
    kind: ShapeKind,
    r: [f64; 4],
    width: f64,
    draw: DrawType,
    dash: DashStyle,
    radius: f64,
) -> ShapeRenderData {
    let mut d = ShapeRenderData::default();
    if r[2] <= 0.0 || r[3] <= 0.0 {
        return d;
    }
    let (g, rule) = guide(kind);
    let p = fit_to_rect(&g, bounds(&g), r);
    let rounded = kind == ShapeKind::RoundedRectangle;
    let rectangle = kind == ShapeKind::Rectangle || rounded;
    let tolerance = if matches!(
        kind,
        ShapeKind::RoundedRectangle | ShapeKind::Ellipse | ShapeKind::Gear
    ) {
        1e-4
    } else {
        0.25
    };
    let inset = |w: f64| [r[0] + w, r[1] + w, r[2] - 2.0 * w, r[3] - 2.0 * w];
    let make_rect = |r| rect_path(r, rounded.then_some(radius));
    let path = |r| {
        let p = make_rect(r);
        if rounded { flattened(&p, tolerance) } else { p }
    };
    if draw != DrawType::Outline {
        let inside = if rectangle {
            let rr = if draw == DrawType::Both {
                inset(if rounded { width / 2.0 } else { width })
            } else {
                r
            };
            (rr[2] >= 0.0 && rr[3] >= 0.0).then(|| path(rr))
        } else {
            Some(if tolerance == 1e-4 {
                flattened(&p, tolerance)
            } else {
                p.clone()
            })
        };
        d.interior = inside.map(|p| (p, rule));
    }
    if draw != DrawType::Interior {
        let rr = inset(width / 2.0);
        if rectangle && (rr[2] < 0.0 || rr[3] < 0.0) {
            d.outline = Some((path(r), rule));
        } else {
            let outline = if rectangle {
                path(rr)
            } else if tolerance == 1e-4 {
                flattened(&p, tolerance)
            } else {
                p
            };
            d.tracer = Some(flatten(&outline, 0.25));
            let spec = StrokeSpec::new(width, dash);
            if kind == ShapeKind::Ellipse {
                d.outline = Some((widen(&outline, &spec), FillRule::NonZero));
            } else {
                d.outline_stroke = Some((outline, spec));
            }
        }
    }
    d
}

/// Natural C² cubic interpolation; the two-knot case is the straight cubic (§2.4.4).
pub fn natural_spline(q: &[[f64; 2]]) -> Vec<[[f64; 2]; 3]> {
    if q.len() < 2 {
        return vec![];
    }
    let n = q.len() - 1;
    let mut first = vec![[0.0; 2]; n];
    if n == 1 {
        for k in 0..2 {
            first[0][k] = (2.0 * q[0][k] + q[1][k]) / 3.0;
        }
        return vec![[
            first[0],
            [2.0 * first[0][0] - q[0][0], 2.0 * first[0][1] - q[0][1]],
            q[1],
        ]];
    }
    for k in 0..2 {
        let mut b = vec![0.0; n];
        let mut rhs = vec![0.0; n];
        b[0] = 2.0;
        rhs[0] = q[0][k] + 2.0 * q[1][k];
        for i in 1..n {
            let last = i == n - 1;
            b[i] = if last { 3.5 } else { 4.0 } - 1.0 / b[i - 1];
            rhs[i] = (if last {
                (8.0 * q[i][k] + q[i + 1][k]) / 2.0
            } else {
                4.0 * q[i][k] + 2.0 * q[i + 1][k]
            }) - rhs[i - 1] / b[i - 1];
        }
        first[n - 1][k] = rhs[n - 1] / b[n - 1];
        for i in (0..n - 1).rev() {
            first[i][k] = (rhs[i] - first[i + 1][k]) / b[i];
        }
    }
    (0..n)
        .map(|i| {
            let second = if i == n - 1 {
                [(q[n][0] + first[i][0]) / 2.0, (q[n][1] + first[i][1]) / 2.0]
            } else {
                [
                    2.0 * q[i + 1][0] - first[i + 1][0],
                    2.0 * q[i + 1][1] - first[i + 1][1],
                ]
            };
            [first[i], second, q[i + 1]]
        })
        .collect()
}
pub fn cap_trim(c: CapStyle, w: f64) -> f64 {
    match c {
        CapStyle::Arrow => 0.5 * w,
        CapStyle::ArrowFilled => 1.5 * w,
        _ => 0.0,
    }
}
pub fn arrow_geometry(end: [f64; 2], angle: f64, w: f64, filled: bool) -> Vec<[f64; 2]> {
    let x = if filled { -5.0 * w } else { -2.5 * w };
    [[x, 2.5 * w], [0.0, 0.0], [x, -2.5 * w]]
        .map(|[x, y]| {
            [
                end[0] + x * angle.cos() - y * angle.sin(),
                end[1] + x * angle.sin() + y * angle.cos(),
            ]
        })
        .to_vec()
}

// Fixed geometry from painting-tools-2.md Appendix A.
fn gear() -> Path2D {
    let mut p = Path2D::new();
    p.move_to(37.6289, 0.0);
    p.line_to(49.6289, -0.000976563);
    p.line_to(52.8506, 10.8311);
    p.bezier_curve_to(55.3838, 11.5439, 57.7979, 12.542, 60.0557, 13.79);
    p.line_to(70.0654, 8.37012);
    p.line_to(78.5518, 16.8545);
    p.line_to(73.1934, 26.749);
    p.bezier_curve_to(74.4844, 29.0117, 75.5234, 31.4365, 76.2725, 33.9863);
    p.line_to(87.251, 37.252);
    p.line_to(87.251, 49.252);
    p.line_to(76.4746, 52.457);
    p.bezier_curve_to(75.7588, 55.1113, 74.7295, 57.6377, 73.4297, 59.9932);
    p.line_to(78.8838, 70.0645);
    p.line_to(70.3994, 78.5518);
    p.line_to(60.4404, 73.1582);
    p.bezier_curve_to(58.0811, 74.5029, 55.5439, 75.5732, 52.875, 76.3252);
    p.line_to(49.624, 87.2549);
    p.line_to(37.624, 87.2549);
    p.line_to(34.373, 76.3232);
    p.bezier_curve_to(31.7061, 75.5713, 29.1729, 74.501, 26.8145, 73.1572);
    p.line_to(16.8574, 78.5488);
    p.line_to(8.37207, 70.0635);
    p.line_to(13.8262, 59.9912);
    p.bezier_curve_to(12.5264, 57.6357, 11.498, 55.1113, 10.7822, 52.458);
    p.line_to(0.0, 49.251);
    p.line_to(0.0, 37.251);
    p.line_to(10.9854, 33.9834);
    p.bezier_curve_to(11.7334, 31.4365, 12.7715, 29.0137, 14.0615, 26.7529);
    p.line_to(8.70313, 16.8564);
    p.line_to(17.1885, 8.37012);
    p.line_to(27.1982, 13.791);
    p.bezier_curve_to(29.4561, 12.542, 31.8721, 11.543, 34.4072, 10.8311);
    p.close_path();
    p.move_to(43.62644869, 59.92630997);
    p.bezier_curve_to(
        52.62744869,
        59.92630997,
        59.92434869,
        52.62740997,
        59.92434869,
        43.62740997,
    );
    p.bezier_curve_to(
        59.92434869,
        34.62650997,
        52.62744869,
        27.32760997,
        43.62644869,
        27.32760997,
    );
    p.bezier_curve_to(
        34.62644869,
        27.32760997,
        27.32664869,
        34.62650997,
        27.32664869,
        43.62740997,
    );
    p.bezier_curve_to(
        27.32664869,
        52.62740997,
        34.62644869,
        59.92630997,
        43.62644869,
        59.92630997,
    );
    p.close_path();
    p
}

// Fixed geometry from painting-tools-2.md Appendix A.
fn rounded_callout() -> Path2D {
    let mut p = Path2D::new();
    p.move_to(743.813, 569.0);
    p.bezier_curve_to(739.498, 569.0, 736.0, 565.502, 736.0, 561.188);
    p.line_to(736.0, 501.813);
    p.bezier_curve_to(736.0, 497.498, 739.498, 494.0, 743.813, 494.0);
    p.line_to(828.188, 494.0);
    p.bezier_curve_to(832.502, 494.0, 836.0, 497.498, 836.0, 501.813);
    p.line_to(836.0, 561.188);
    p.bezier_curve_to(836.0, 565.502, 832.502, 569.0, 828.188, 569.0);
    p.line_to(764.152, 569.0);
    p.line_to(736.0, 594.0);
    p.line_to(746.704, 569.0);
    p.line_to(743.813, 569.0);
    p.close_path();
    p
}

// Fixed geometry from painting-tools-2.md Appendix A.
fn cloud_callout() -> Path2D {
    let mut p = Path2D::new();
    p.move_to(29.8114, 6.86053);
    p.bezier_curve_to(32.6865, 6.86053, 35.3553, 7.63245, 37.561, 8.95203);
    p.bezier_curve_to(38.8107, 5.08148, 43.0224, 2.23468, 48.0257, 2.23468);
    p.bezier_curve_to(51.3842, 2.23468, 54.386, 3.51746, 56.3747, 5.53149);
    p.bezier_curve_to(57.7646, 2.53638, 60.4959, 0.5, 63.638, 0.5);
    p.bezier_curve_to(66.7857, 0.5, 69.5212, 2.5437, 70.9087, 5.54773);
    p.bezier_curve_to(72.5835, 2.72839, 76.078, 0.789063, 80.1176, 0.789063);
    p.bezier_curve_to(85.786, 0.789063, 90.3812, 4.6076, 90.3812, 9.31799);
    p.line_to(90.3795, 9.47687);
    p.bezier_curve_to(95.0489, 9.78705, 98.7656, 15.0986, 98.7656, 21.6055);
    p.bezier_curve_to(98.7656, 22.9968, 98.5956, 24.3336, 98.2828, 25.5785);
    p.bezier_curve_to(99.6871, 28.1501, 100.5, 31.2003, 100.5, 34.4711);
    p.bezier_curve_to(100.5, 42.6901, 95.3656, 49.5161, 88.6192, 50.8566);
    p.line_to(88.6465, 51.6735);
    p.bezier_curve_to(88.6465, 58.6992, 82.6275, 64.3947, 75.2026, 64.3947);
    p.bezier_curve_to(72.9284, 64.3947, 70.7861, 63.8603, 68.9082, 62.9171);
    p.bezier_curve_to(68.1762, 68.1365, 62.4068, 72.2007, 55.3982, 72.2007);
    p.bezier_curve_to(49.8579, 72.2007, 45.092, 69.6611, 42.9766, 66.0188);
    p.bezier_curve_to(40.5288, 67.3732, 37.6553, 68.1531, 34.5818, 68.1531);
    p.bezier_curve_to(27.846, 68.1531, 22.0709, 64.407, 19.6314, 59.0776);
    p.bezier_curve_to(19.1799, 59.152, 18.7175, 59.1906, 18.2467, 59.1906);
    p.bezier_curve_to(13.217, 59.1906, 9.13957, 54.7896, 9.13957, 49.3606);
    p.bezier_curve_to(9.13957, 46.6221, 10.1771, 44.1451, 11.851, 42.3626);
    p.bezier_curve_to(9.22155, 40.8277, 7.40488, 37.5473, 7.40488, 33.7484);
    p.bezier_curve_to(7.40488, 28.4791, 10.8998, 24.2075, 15.211, 24.2075);
    p.bezier_curve_to(16.1401, 24.2075, 17.0314, 24.4059, 17.8578, 24.7699);
    p.bezier_curve_to(16.7251, 23.0251, 16.0784, 21.008, 16.0784, 18.8588);
    p.bezier_curve_to(16.0784, 12.2324, 22.2268, 6.86053, 29.8114, 6.86053);
    p.close_path();
    p.move_to(19.9099, 64.5927);
    p.bezier_curve_to(24.1911, 64.4238, 27.7611, 67.2875, 27.8812, 70.9274);
    p.bezier_curve_to(28.0014, 74.5673, 24.6274, 77.6326, 20.3451, 77.774);
    p.bezier_curve_to(16.0628, 77.9154, 12.4939, 75.0793, 12.3738, 71.4393);
    p.bezier_curve_to(12.2536, 67.7994, 15.2289, 64.7773, 19.9099, 64.5927);
    p.close_path();
    p.move_to(11.0507, 75.5271);
    p.bezier_curve_to(15.2486, 75.5865, 18.6535, 77.7501, 18.6535, 80.4922);
    p.bezier_curve_to(18.6535, 83.2344, 15.2496, 85.4573, 11.0507, 85.4573);
    p.bezier_curve_to(6.85179, 85.4573, 3.44791, 83.2344, 3.44791, 80.4922);
    p.bezier_curve_to(3.44791, 77.7501, 6.60393, 75.4642, 11.0507, 75.5271);
    p.close_path();
    p.move_to(4.06865, 83.7512);
    p.bezier_curve_to(6.03956, 83.756, 7.6373, 85.0016, 7.6373, 86.5441);
    p.bezier_curve_to(7.6373, 88.0865, 6.03956, 89.3369, 4.06865, 89.3369);
    p.bezier_curve_to(2.09774, 89.3369, 0.49999, 88.0865, 0.49999, 86.5441);
    p.bezier_curve_to(0.49999, 85.0016, 1.94781, 83.7461, 4.06865, 83.7512);
    p.close_path();
    p
}

fn from_skia(p: &tiny_skia::Path) -> Path2D {
    use tiny_skia::PathSegment::*;
    let mut out = Path2D::new();
    for s in p.segments() {
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
fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn cubic_flatten(q: [[f64; 2]; 4], tol: f64, depth: u32, out: &mut Vec<[f64; 2]>) {
    // A control polygon inside this narrow chord strip bounds the whole cubic.
    let chord = distance(q[0], q[3]);
    let error = if chord > 0.0 {
        q[1..3]
            .iter()
            .map(|p| {
                ((p[0] - q[0][0]) * (q[3][1] - q[0][1]) - (p[1] - q[0][1]) * (q[3][0] - q[0][0]))
                    .abs()
                    / chord
            })
            .fold(0.0, f64::max)
    } else {
        distance(q[0], q[1]).max(distance(q[0], q[2]))
    };
    let polygon = distance(q[0], q[1]) + distance(q[1], q[2]) + distance(q[2], q[3]);
    if depth >= 24 || (error <= tol && polygon - chord <= tol) {
        out.push(q[3]);
        return;
    }
    let (a, b, c) = (
        midpoint(q[0], q[1]),
        midpoint(q[1], q[2]),
        midpoint(q[2], q[3]),
    );
    let (d, e) = (midpoint(a, b), midpoint(b, c));
    let f = midpoint(d, e);
    cubic_flatten([q[0], a, d, f], tol, depth + 1, out);
    cubic_flatten([f, e, c, q[3]], tol, depth + 1, out);
}
fn flatten(path: &Path2D, tolerance: f64) -> Vec<Vec<[f64; 2]>> {
    use tiny_skia::PathSegment::*;
    let Some(path) = path.to_skia() else {
        return vec![];
    };
    let mut polys = vec![];
    let mut poly = vec![];
    let point = |p: tiny_skia::Point| [p.x as f64, p.y as f64];
    for seg in path.segments() {
        match seg {
            MoveTo(p) => {
                if !poly.is_empty() {
                    polys.push(std::mem::take(&mut poly));
                }
                poly.push(point(p));
            }
            LineTo(p) => poly.push(point(p)),
            QuadTo(a, b) => {
                let s = *poly.last().unwrap();
                let (a, b) = (point(a), point(b));
                let c1 = [(s[0] + 2.0 * a[0]) / 3.0, (s[1] + 2.0 * a[1]) / 3.0];
                let c2 = [(b[0] + 2.0 * a[0]) / 3.0, (b[1] + 2.0 * a[1]) / 3.0];
                cubic_flatten([s, c1, c2, b], tolerance, 0, &mut poly);
            }
            CubicTo(a, b, c) => {
                let s = *poly.last().unwrap();
                cubic_flatten([s, point(a), point(b), point(c)], tolerance, 0, &mut poly);
            }
            Close => {
                if let Some(&p) = poly.first() {
                    poly.push(p);
                }
            }
        }
    }
    if !poly.is_empty() {
        polys.push(poly);
    }
    for p in &mut polys {
        p.dedup();
    }
    polys
}
fn flattened(p: &Path2D, tol: f64) -> Path2D {
    let mut out = Path2D::new();
    for points in flatten(p, tol) {
        let closed = points.len() > 1 && points.first() == points.last();
        out.push_mapped(&poly(&points, closed), &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
    out
}
fn widen(p: &Path2D, s: &StrokeSpec) -> Path2D {
    let Some(mut path) = p.to_skia() else {
        return Path2D::new();
    };
    if let Some((array, phase)) = &s.dash {
        let Some(dash) = tiny_skia::StrokeDash::new(array.clone(), *phase) else {
            return Path2D::new();
        };
        let Some(dashed) = path.dash(&dash, 4.0) else {
            return Path2D::new();
        };
        path = dashed;
    }
    let stroke = tiny_skia::Stroke {
        width: s.width as f32,
        miter_limit: 10.0,
        line_join: if s.round_join {
            tiny_skia::LineJoin::Round
        } else {
            tiny_skia::LineJoin::MiterClip
        },
        line_cap: tiny_skia::LineCap::Butt,
        ..Default::default()
    };
    let mut out = path
        .stroke(&stroke, 4.0)
        .as_ref()
        .map(from_skia)
        .unwrap_or_default();
    // Per-end round caps, never round the internal ends of square-capped dashes.
    let figures = flatten(p, 0.1);
    for figure in figures {
        if figure.first() == figure.last() {
            continue;
        }
        for (enabled, point) in [
            (s.round_start, figure.first()),
            (s.round_end, figure.last()),
        ] {
            if enabled && let Some(&[x, y]) = point {
                let circle = ellipse([x - s.width / 2.0, y - s.width / 2.0, s.width, s.width]);
                out = union_paths(&out, &circle);
            }
        }
    }
    out
}
fn union_paths(a: &Path2D, b: &Path2D) -> Path2D {
    use i_overlay::{
        core::{fill_rule::FillRule as Rule, overlay_rule::OverlayRule},
        float::single::SingleFloatOverlay,
    };
    let shapes = flatten(a, 0.1).overlay(&flatten(b, 0.1), OverlayRule::Union, Rule::NonZero);
    let mut out = Path2D::new();
    for figure in shapes.into_iter().flatten() {
        out.push_mapped(&poly(&figure, true), &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn build_line(
    knots: [[f64; 2]; 4],
    curve: CurveType,
    edited: bool,
    width: f64,
    start: CapStyle,
    end: CapStyle,
    dash: DashStyle,
) -> ShapeRenderData {
    let mut d = ShapeRenderData::default();
    if knots.iter().all(|p| *p == knots[0]) {
        return d;
    }
    let mut p = Path2D::new();
    p.move_to(knots[0][0], knots[0][1]);
    if !edited {
        p.line_to(knots[3][0], knots[3][1]);
    } else {
        match curve {
            CurveType::Straight => {
                for q in &knots[1..] {
                    p.line_to(q[0], q[1]);
                }
            }
            CurveType::Bezier => p.bezier_curve_to(
                knots[1][0],
                knots[1][1],
                knots[2][0],
                knots[2][1],
                knots[3][0],
                knots[3][1],
            ),
            CurveType::Spline => {
                for [a, b, c] in natural_spline(&knots) {
                    p.bezier_curve_to(a[0], a[1], b[0], b[1], c[0], c[1]);
                }
            }
        }
    }
    let points = flatten(&p, 0.1).into_iter().next().unwrap_or_default();
    if points.len() < 2 {
        return d;
    }
    // End derivatives, falling back to the first/last distinct control for collapsed tangents.
    let (sdir, edir) = if !edited || curve == CurveType::Straight {
        (points[1], points[points.len() - 2])
    } else if curve == CurveType::Bezier {
        (
            knots[1..].iter().copied().find(|q| *q != knots[0]).unwrap(),
            knots[..3]
                .iter()
                .rev()
                .copied()
                .find(|q| *q != knots[3])
                .unwrap(),
        )
    } else {
        let c = natural_spline(&knots);
        (c[0][0], c[c.len() - 1][1])
    };
    let sa = (knots[0][1] - sdir[1]).atan2(knots[0][0] - sdir[0]);
    let ea = (knots[3][1] - edir[1]).atan2(knots[3][0] - edir[0]);
    let extend = |p: [f64; 2], a: f64| [p[0] + 0.4999 * a.cos(), p[1] + 0.4999 * a.sin()];
    let (s, e) = (extend(knots[0], sa), extend(knots[3], ea));
    let mut extended = Path2D::new();
    extended.move_to(s[0], s[1]);
    extended.line_to(knots[0][0], knots[0][1]);
    extended.push_mapped_continuing(&p, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    extended.line_to(e[0], e[1]);
    if matches!(start, CapStyle::Flat | CapStyle::Rounded)
        && matches!(end, CapStyle::Flat | CapStyle::Rounded)
    {
        d.tracer = Some(flatten(&extended, 0.25));
    }
    if start == CapStyle::Flat && end == CapStyle::Flat && dash == DashStyle::Solid {
        d.outline_stroke = Some((extended, StrokeSpec::new(width, dash)));
        return d;
    }
    let line = flatten(&extended, 0.1)
        .into_iter()
        .next()
        .unwrap_or_default();
    let mut lengths = vec![0.0];
    for pair in line.windows(2) {
        lengths.push(lengths.last().unwrap() + distance(pair[0], pair[1]));
    }
    let (lo, hi) = (
        cap_trim(start, width),
        lengths.last().copied().unwrap_or(0.0) - cap_trim(end, width),
    );
    let mut outline = Path2D::new();
    if hi > lo {
        let mut trimmed = vec![];
        for (i, pair) in line.windows(2).enumerate() {
            let (a, b) = (lengths[i], lengths[i + 1]);
            if b <= lo || a >= hi || a == b {
                continue;
            }
            for t in [
                ((lo - a) / (b - a)).clamp(0.0, 1.0),
                ((hi - a) / (b - a)).clamp(0.0, 1.0),
            ] {
                let q = [
                    pair[0][0] + (pair[1][0] - pair[0][0]) * t,
                    pair[0][1] + (pair[1][1] - pair[0][1]) * t,
                ];
                if trimmed.last() != Some(&q) {
                    trimmed.push(q);
                }
            }
        }
        let spec = StrokeSpec {
            round_join: true,
            round_start: start == CapStyle::Rounded,
            round_end: end == CapStyle::Rounded,
            ..StrokeSpec::new(width, dash)
        };
        outline = widen(&poly(&trimmed, false), &spec);
    }
    for (cap, p, a) in [(start, s, sa), (end, e, ea)] {
        if matches!(cap, CapStyle::Arrow | CapStyle::ArrowFilled) {
            let arrow = poly(
                &arrow_geometry(p, a, width, cap == CapStyle::ArrowFilled),
                cap == CapStyle::ArrowFilled,
            );
            let arrow = if cap == CapStyle::Arrow {
                widen(&arrow, &StrokeSpec::new(width, DashStyle::Solid))
            } else {
                arrow
            };
            outline = union_paths(&outline, &arrow);
        }
    }
    d.outline = Some((outline, FillRule::NonZero));
    d
}

/// The approved Direct2D approximation: tiny-skia coverage at 4× and rounded box reduction (B-P6).
pub fn coverage_mask(
    path: &Path2D,
    rule: FillRule,
    stroke: Option<&StrokeSpec>,
    antialias: bool,
    rect: IntRect,
) -> Vec<u8> {
    let mut out = vec![0; rect.w as usize * rect.h as usize];
    if rect.is_empty() {
        return out;
    }
    let widened;
    let path = if let Some(s) = stroke {
        widened = widen(path, s);
        &widened
    } else {
        path
    };
    let Some(p) = path.to_skia() else {
        return out;
    };
    let factor = if antialias { 4 } else { 1 };
    let Some(mut mask) = tiny_skia::Mask::new(rect.w * factor, rect.h * factor) else {
        return out;
    };
    let f = factor as f32;
    let transform =
        tiny_skia::Transform::from_row(f, 0.0, 0.0, f, -(rect.x as f32) * f, -(rect.y as f32) * f);
    let rule = if stroke.is_some() || rule == FillRule::NonZero {
        tiny_skia::FillRule::Winding
    } else {
        tiny_skia::FillRule::EvenOdd
    };
    mask.fill_path(&p, rule, antialias, transform);
    if !antialias {
        return mask.data().to_vec();
    }
    let w = rect.w as usize;
    for y in 0..rect.h as usize {
        for x in 0..w {
            let mut sum = 0u32;
            for dy in 0..4 {
                for dx in 0..4 {
                    sum += mask.data()[(y * 4 + dy) * w * 4 + x * 4 + dx] as u32;
                }
            }
            out[y * w + x] = ((sum + 8) / 16) as u8;
        }
    }
    out
}
/// Closed figures must repeat their first vertex. Open figures stay open (R89).
/// Phase only advances for segments intersecting this render tile (§2.4.5 quirk).
pub fn trace_1px(polys: &[Vec<[f64; 2]>], dash: DashStyle, rect: IntRect) -> Vec<u8> {
    let (pattern, mut index): (&[bool], usize) = match dash {
        DashStyle::Solid => (&[true], 0),
        DashStyle::Dash => (&[true, true, true, false], 1),
        DashStyle::Dot => (&[true, false], 0),
        DashStyle::DashDot => (&[true, true, true, false, true, false], 1),
        DashStyle::DashDotDot => (&[true, true, true, false, true, false, true, false], 1),
    };
    let mut out = vec![0; rect.w as usize * rect.h as usize];
    let mut remainder = 0.0;
    for points in polys {
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = dx.abs().max(dy.abs());
            if len == 0.0
                || !len.is_finite()
                || a[0].max(b[0]) < rect.x as f64
                || a[1].max(b[1]) < rect.y as f64
                || a[0].min(b[0]) >= rect.right() as f64
                || a[1].min(b[1]) >= rect.bottom() as f64
            {
                continue;
            }
            let mut u = remainder;
            while u < len {
                let (x, y) = (
                    (a[0] + dx / len * u).floor() as i64,
                    (a[1] + dy / len * u).floor() as i64,
                );
                if pattern[index]
                    && x >= rect.x as i64
                    && y >= rect.y as i64
                    && x < rect.right() as i64
                    && y < rect.bottom() as i64
                {
                    out[(y - rect.y as i64) as usize * rect.w as usize
                        + (x - rect.x as i64) as usize] = 255;
                }
                index = (index + 1) % pattern.len();
                u += 1.0;
            }
            remainder = u - len;
        }
    }
    out
}

use super::base::ToolBase;
use super::fill::FillBrush;
use super::move_tools::{MoveHit, NUDGE_COMMIT_MS, TransformHandles, constrain_angle};
use super::transacted::{self, DynTransacted, Edit, State, Transacted, Transaction};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::history::HistoryMemento;
use crate::pixel::{
    Px,
    content::{ContentBlendMode, ContentLayer},
    mask::SelectionQuality,
};
use crate::render::{
    Renderer, RendererId,
    nubs::{Handle, HandleShape},
};
use crate::selection::matrix::{Matrix, MatrixOrder};
use crate::util::{
    color::Color,
    geom::{Point, Rectangle},
};
use serde_json::Value;

const SHAPE_SETTINGS: [&str; 9] = [
    "shape",
    "width",
    "drawType",
    "dash",
    "radius",
    "fillStyle",
    "antialias",
    "blendMode",
    "renderingQuality",
];
const LINE_SETTINGS: [&str; 9] = [
    "width",
    "dash",
    "startCap",
    "endCap",
    "fillStyle",
    "curveType",
    "antialias",
    "blendMode",
    "renderingQuality",
];

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeState {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub knots: [[f64; 2]; 4],
    pub transform: Matrix,
    /// Explicit anchor in canvas coordinates (§8.5), not base coordinates.
    pub anchor: Option<[f64; 2]>,
    pub settings: Vec<(&'static str, Value)>,
    /// Retain the chosen geometry in history even if the catalogue is later updated.
    pub custom_shape: Option<std::sync::Arc<super::xaml::ParsedShape>>,
    pub colors: [Color; 2],
    pub button: MouseButton,
}
impl ShapeState {
    fn text(&self, key: &str) -> &str {
        self.settings
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.as_str())
            .unwrap_or("")
    }
    fn number(&self, key: &str, fallback: f64) -> f64 {
        self.settings
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.as_f64())
            .filter(|v| v.is_finite())
            .unwrap_or(fallback)
    }
    fn aa(&self) -> bool {
        self.settings
            .iter()
            .find(|(k, _)| *k == "antialias")
            .is_none_or(|(_, v)| *v != false)
    }
    fn shape(&self) -> ShapeKind {
        ShapeKind::from_value(self.text("shape"))
    }
    fn bounds(&self) -> Rectangle {
        Rectangle::absolute(
            self.start[0].min(self.end[0]),
            self.start[1].min(self.end[1]),
            self.start[0].max(self.end[0]),
            self.start[1].max(self.end[1]),
        )
    }
    fn center(&self) -> [f64; 2] {
        self.anchor
            .unwrap_or_else(|| transform_point(&self.transform, midpoint(self.start, self.end)))
    }
    fn initial_knots(&self) -> [[f64; 2]; 4] {
        std::array::from_fn(|i| {
            [
                self.start[0] + (self.end[0] - self.start[0]) * i as f64 / 3.0,
                self.start[1] + (self.end[1] - self.start[1]) * i as f64 / 3.0,
            ]
        })
    }
}
fn transform_point(m: &Matrix, p: [f64; 2]) -> [f64; 2] {
    let mut q = [Point::new(p[0], p[1])];
    m.transform_points(&mut q);
    [q[0].x, q[0].y]
}
fn transform_vector(m: &Matrix, p: [f64; 2]) -> [f64; 2] {
    let mut q = [Point::new(p[0], p[1])];
    m.transform_vectors(&mut q);
    [q[0].x, q[0].y]
}
fn scales(m: &Matrix) -> [f64; 2] {
    let a = transform_vector(m, [1.0, 0.0]);
    let b = transform_vector(m, [0.0, 1.0]);
    [a[0].hypot(a[1]), b[0].hypot(b[1])]
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ShapeHit {
    Transform(MoveHit),
    Node(usize),
    Reset,
}
#[derive(Clone, Copy, Debug)]
struct ResetGeometry {
    center: [f64; 2],
    angle: f64,
    size: f64,
}
impl ResetGeometry {
    fn new(center: [f64; 2], anchor: [f64; 2], padded_width: f64, size: f64) -> Self {
        let delta = [anchor[0] - center[0], anchor[1] - center[1]];
        let length = delta[0].hypot(delta[1]);
        let direction = if length > 0.0 {
            [delta[0] / length, delta[1] / length]
        } else {
            [0.0; 2]
        };
        Self {
            center: [
                anchor[0] + 2.0 * padded_width * direction[0],
                anchor[1] + 2.0 * padded_width * direction[1],
            ],
            angle: if length > 0.0 {
                delta[1].atan2(delta[0])
            } else {
                0.0
            },
            size,
        }
    }
    fn contains(self, p: [f64; 2]) -> bool {
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (p[0] - self.center[0], p[1] - self.center[1]);
        let (x, y) = (dx * cos + dy * sin, -dx * sin + dy * cos);
        let half = self.size / 2.0;
        x >= -half && y >= -half && x < half && y < half
    }
}

// R95: this portable stand-in is not a measured Windows default. Device units, not DIP.
#[cfg(not(windows))]
const PORTABLE_DRAG_SIZE: [f64; 2] = [4.0, 4.0];

fn platform_drag_size() -> [f64; 2] {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDRAG, SM_CYDRAG};
        // SAFETY: these scalar system-metric queries accept no pointers or owned resources.
        unsafe {
            [
                GetSystemMetrics(SM_CXDRAG) as f64,
                GetSystemMetrics(SM_CYDRAG) as f64,
            ]
        }
    }
    #[cfg(not(windows))]
    {
        PORTABLE_DRAG_SIZE
    }
}

#[derive(Clone)]
enum Live {
    Draw {
        press: [f64; 2],
        last: [f64; 2],
        button: MouseButton,
        begun: bool,
    },
    Pointer {
        press: [f64; 2],
        last: [f64; 2],
        button: MouseButton,
        hit: ShapeHit,
        from: Box<ShapeState>,
    },
    Nudge(f64),
    Reset {
        inside: bool,
    },
}
#[derive(Default)]
pub struct PreviewShapeTool<const LINE: bool = false> {
    tx: Transaction<ShapeState>,
    base: ToolBase,
    control: TransformHandles,
    live: Option<Live>,
    handles: Option<RendererId>,
    shift: bool,
    cursor: Option<CursorRequest>,
    reset_hover: bool,
    render_error: Option<super::xaml::XamlError>,
}
pub type ShapesTool = PreviewShapeTool<false>;
pub type LineTool = PreviewShapeTool<true>;

/// §8.3: proposed displayed size, nearest-even dimension snapping, then parity addition.
fn scale_factor(raw: f64, base: f64, old_scale: f64, step: f64) -> f64 {
    let start = base * old_scale;
    let magnitude = if base == 0.0 || start == 0.0 {
        1.0
    } else {
        let size = (raw.abs() * old_scale / step).round_ties_even() * step
            + if step == 2.0 {
                start.round_ties_even().rem_euclid(2.0)
            } else {
                0.0
            };
        size / start
    };
    magnitude.max(1e-7) * if raw <= 0.0 { -1.0 } else { 1.0 }
}
fn transform_edit(
    from: &ShapeState,
    hit: MoveHit,
    press: [f64; 2],
    p: [f64; 2],
    shift: bool,
    alt: bool,
) -> ShapeState {
    let mut r = from.clone();
    let delta = [p[0] - press[0], p[1] - press[1]];
    match hit {
        MoveHit::Translate | MoveHit::Background => {
            r.transform.translate(delta[0].trunc(), delta[1].trunc())
        }
        MoveHit::Anchor => {
            let c = from.center();
            r.anchor = Some([c[0] + delta[0], c[1] + delta[1]]);
        }
        MoveHit::Rotate => {
            let c = from.center();
            let angle = ((p[1] - c[1]).atan2(p[0] - c[0])
                - (press[1] - c[1]).atan2(press[0] - c[0]))
            .to_degrees();
            let axis = transform_vector(&from.transform, [1.0, 0.0]);
            let before = axis[1].atan2(axis[0]).to_degrees();
            let angle = if shift {
                constrain_angle(before + angle) - before
            } else {
                angle
            };
            r.transform.rotate_at(angle, Point::new(c[0], c[1]));
        }
        MoveHit::Scale(edge) => {
            let Some(inv) = from.transform.invert() else {
                return r;
            };
            let d = transform_vector(&inv, delta);
            let bb = from.bounds();
            let (ex, ey) = edge.dir();
            let (hx, hy) = (ex != 0.0, ey != 0.0);
            let old = scales(&from.transform);
            let mul = if alt { 2.0 } else { 1.0 };
            let mut raw = [bb.width + mul * ex * d[0], bb.height + mul * ey * d[1]];
            if shift && bb.width != 0.0 && bb.height != 0.0 {
                let sx = if hx {
                    raw[0].abs() * old[0] / bb.width
                } else {
                    f64::INFINITY
                };
                let sy = if hy {
                    raw[1].abs() * old[1] / bb.height
                } else {
                    f64::INFINITY
                };
                let s = sx.min(sy);
                raw = [
                    s * bb.width / old[0] * if hx && raw[0] <= 0.0 { -1.0 } else { 1.0 },
                    s * bb.height / old[1] * if hy && raw[1] <= 0.0 { -1.0 } else { 1.0 },
                ];
            }
            let step = |dragged: bool| {
                if (alt && dragged) || (shift && !dragged) {
                    2.0
                } else {
                    1.0
                }
            };
            let sx = if hx || shift {
                scale_factor(raw[0], bb.width, old[0], step(hx))
            } else {
                1.0
            };
            let sy = if hy || shift {
                scale_factor(raw[1], bb.height, old[1], step(hy))
            } else {
                1.0
            };
            let anchor = |lo: f64, len: f64, edge: f64| {
                if alt || edge == 0.0 {
                    lo + len / 2.0
                } else if edge > 0.0 {
                    lo
                } else {
                    lo + len
                }
            };
            let (ax, ay) = (anchor(bb.x, bb.width, ex), anchor(bb.y, bb.height, ey));
            let mut edit = Matrix::identity();
            edit.translate(-ax, -ay);
            edit.scale(sx, sy);
            edit.translate(ax, ay);
            edit.multiply(&from.transform, MatrixOrder::Append);
            r.transform = edit;
        }
        MoveHit::None => {}
    }
    r
}

impl<const LINE: bool> PreviewShapeTool<LINE> {
    pub fn state(&self) -> State {
        self.tx.state()
    }
    pub fn rendering_error(&self) -> Option<&super::xaml::XamlError> {
        self.render_error.as_ref()
    }
    pub fn context(&self) -> Option<&ShapeState> {
        self.tx.record()
    }
    fn keys() -> &'static [&'static str] {
        if LINE {
            &LINE_SETTINGS
        } else {
            &SHAPE_SETTINGS
        }
    }
    fn toolbar(app: &AppWorkspace) -> Vec<(&'static str, Value)> {
        Self::keys()
            .iter()
            .map(|&k| (k, app.tool_settings.get(Self::TOOL, k)))
            .collect()
    }
    fn display_name(r: &ShapeState) -> String {
        if LINE {
            crate::i18n::t("lineTool.name")
        } else {
            r.custom_shape
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| r.shape().name())
        }
    }
    fn named(key: &str, r: &ShapeState) -> String {
        crate::i18n::t(key).replace("{0}", &Self::display_name(r))
    }
    fn draw_record(
        &self,
        app: &AppWorkspace,
        press: [f64; 2],
        last: [f64; 2],
        button: MouseButton,
    ) -> ShapeState {
        let mut r = self
            .tx
            .record()
            .filter(|_| self.tx.state() == State::Drawing)
            .cloned()
            .unwrap_or_else(|| ShapeState {
                start: press,
                end: last,
                knots: [[0.0; 2]; 4],
                transform: Matrix::identity(),
                anchor: None,
                settings: Self::toolbar(app),
                custom_shape: app
                    .shape_catalog
                    .selected(
                        app.tool_settings
                            .get(Self::TOOL, "shape")
                            .as_str()
                            .unwrap_or(""),
                    )
                    .cloned()
                    .map(std::sync::Arc::new),
                colors: [app.primary_color, app.secondary_color],
                button,
            });
        let (mut s, mut e) = if LINE {
            (snap_line_point(press), snap_line_point(last))
        } else {
            snap_rect_points(press, last)
        };
        if app.shift_key_down {
            let (dx, dy) = (e[0] - s[0], e[1] - s[1]);
            if LINE {
                let step = (12.0 * dy.atan2(dx) / PI).round();
                let a = step * PI / 12.0;
                let mut len = dx.hypot(dy);
                if (step as i32) % 6 == 0 {
                    len = len.round();
                } else if (step as i32) % 3 == 0 {
                    len = (len / 2.0f64.sqrt()).round() * 2.0f64.sqrt();
                }
                e = [s[0] + len * a.cos(), s[1] + len * a.sin()];
            } else {
                let ar = r
                    .custom_shape
                    .as_ref()
                    .map(|s| s.aspect_ratio())
                    .unwrap_or_else(|| r.shape().aspect_ratio());
                let h = dy.abs().min(dx.abs() / ar);
                let w = h * ar;
                e = [s[0] + w * dx.signum(), s[1] + h * dy.signum()];
            }
        }
        if app.alt_key_down {
            s = [2.0 * s[0] - e[0], 2.0 * s[1] - e[1]];
        }
        (r.start, r.end) = (s, e);
        r.knots = r.initial_knots();
        r
    }
    fn render_data(r: &ShapeState) -> Result<ShapeRenderData, super::xaml::XamlError> {
        let width = r.number("width", 2.0).clamp(1.0, 2000.0);
        let dash = DashStyle::from_value(r.text("dash"));
        let mut d = if LINE {
            let knots = r.knots.map(|p| transform_point(&r.transform, p));
            build_line(
                knots,
                CurveType::from_value(r.text("curveType")),
                r.knots != r.initial_knots(),
                width,
                CapStyle::from_value(r.text("startCap")),
                CapStyle::from_value(r.text("endCap")),
                dash,
            )
        } else {
            // §8.4: fit at scaled size, then retain only orientation/reflection in the transform.
            let bb = r.bounds();
            let [sx, sy] = scales(&r.transform);
            let rect = [bb.x * sx, bb.y * sy, bb.width * sx, bb.height * sy];
            let draw = DrawType::from_value(r.text("drawType"));
            let mut d = if let Some(custom) = &r.custom_shape {
                custom.render_data(rect, width, draw, dash)?
            } else if r.text("shape").starts_with("custom.") {
                return Err(super::xaml::XamlError::BadAttribute(
                    "custom shape is not registered".into(),
                ));
            } else {
                build_shape(r.shape(), rect, width, draw, dash, r.number("radius", 10.0))
            };
            let mut m = Matrix::identity();
            m.scale(1.0 / sx, 1.0 / sy);
            m.multiply(&r.transform, MatrixOrder::Append);
            let map = m.to_canvas();
            for pair in [&mut d.interior, &mut d.outline].into_iter().flatten() {
                pair.0 = mapped(&pair.0, &map);
            }
            if let Some((p, _)) = &mut d.outline_stroke {
                *p = mapped(p, &map);
            }
            if let Some(polys) = &mut d.tracer {
                for p in polys.iter_mut().flatten() {
                    *p = transform_point(&m, *p);
                }
            }
            d
        };
        if r.aa() || width != 1.0 {
            d.tracer = None;
        }
        Ok(d)
    }
    fn anchor_visible(&self) -> bool {
        !LINE
            && matches!(self.tx.state(), State::Dirty | State::Editing)
            && !matches!(
                self.live,
                Some(
                    Live::Nudge(_)
                        | Live::Pointer {
                            hit: ShapeHit::Transform(
                                MoveHit::Translate | MoveHit::Background | MoveHit::Scale(_)
                            ),
                            ..
                        }
                )
            )
    }
    fn reset_geometry(&self, app: &AppWorkspace) -> Option<ResetGeometry> {
        if !self.anchor_visible() {
            return None;
        }
        let r = self.tx.record()?;
        let anchor = r.anchor?;
        let view = app.active_ws()?.view.view_transform();
        let all = self.control.handles(
            &r.transform,
            &r.bounds(),
            1.0 / view.scale,
            app.now_ms / 1000.0,
        );
        let width = all
            .iter()
            .skip(1)
            .take(8)
            .map(|h| crate::render::nubs::canvas_width(&view, h))
            .reduce(f64::min)?;
        Some(ResetGeometry::new(
            transform_point(&r.transform, midpoint(r.start, r.end)),
            anchor,
            width + 8.0 / view.scale,
            20.0 * view.ui_scale / view.scale,
        ))
    }
    fn reset_anchor(&mut self, app: &mut AppWorkspace) {
        let Some(mut r) = self.tx.record().cloned().filter(|r| r.anchor.is_some()) else {
            return;
        };
        transacted::end(self, app); // finish a settings edit before the separate anchor edit
        transacted::begin_edit(self, Edit::Pointer);
        r.anchor = None;
        transacted::update(self, app, r);
        transacted::end(self, app);
    }
    fn handle_list(&mut self, app: &AppWorkspace) -> Vec<Handle> {
        let Some(r) = self
            .tx
            .record()
            .filter(|_| self.tx.state() != State::Drawing)
        else {
            return vec![];
        };
        let Some(ws) = app.active_ws() else {
            return vec![];
        };
        let view = ws.view.view_transform();
        let hw = 1.0 / view.scale;
        self.control.ui_scale = view.ui_scale;
        // The shared Move control stores base coordinates. Convert only its presentation copy;
        // the shape record keeps the explicit anchor fixed in canvas coordinates (§8.5).
        self.control.anchor = r.anchor.and_then(|p| {
            r.transform.invert().map(|m| {
                let q = transform_point(&m, p);
                Point::new(q[0], q[1])
            })
        });
        let all = self
            .control
            .handles(&r.transform, &r.bounds(), hw, app.now_ms / 1000.0);
        if LINE {
            let mut list: Vec<_> = r
                .knots
                .iter()
                .map(|&p| Handle {
                    canvas: transform_point(&r.transform, p),
                    shape: HandleShape::Circle,
                    opacity: 1.0,
                    hover: 0.0,
                })
                .collect();
            if let Some(h) = all.last() {
                list.push(*h);
            }
            list
        } else {
            let mut list = all;
            if !self.anchor_visible() {
                list.remove(0);
            }
            if let Some(reset) = self.reset_geometry(app) {
                use crate::render::nubs::ResetState;
                let state = match self.live {
                    Some(Live::Reset { inside: true }) => ResetState::Pressed,
                    Some(Live::Reset { inside: false }) => ResetState::Normal,
                    _ if self.reset_hover => ResetState::Hot,
                    _ => ResetState::Normal,
                };
                // Reset is below the anchor/scale/compass siblings and does not pulse.
                list.insert(
                    0,
                    Handle {
                        canvas: reset.center,
                        shape: HandleShape::Reset {
                            angle: reset.angle,
                            state,
                        },
                        opacity: 1.0,
                        hover: 0.0,
                    },
                );
            }
            list
        }
    }
    fn sync_handles(&mut self, app: &mut AppWorkspace) {
        let list = self.handle_list(app);
        if let (Some(id), Some(ws)) = (self.handles, app.active_ws_mut()) {
            if matches!(ws.surface_box.renderer(id),Some(Renderer::Handles(h)) if *h==list) {
                return;
            }
            if let Some(Renderer::Handles(h)) = ws.surface_box.renderer_mut(id) {
                *h = list;
            }
        }
    }
    fn hit(&mut self, app: &AppWorkspace, p: [f64; 2]) -> Option<ShapeHit> {
        let list = self.handle_list(app);
        let r = self.tx.record()?;
        let view = app.active_ws()?.view.view_transform();
        let reach = 12.0 * view.ui_scale / view.scale;
        if LINE {
            let nearest = list
                .iter()
                .take(4)
                .enumerate()
                .map(|(i, h)| (distance(p, h.canvas), i))
                .filter(|(d, _)| *d <= reach)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, i)) = nearest {
                return Some(ShapeHit::Node(i));
            }
            return list
                .last()
                .filter(|h| {
                    (p[0] - h.canvas[0]).abs() <= 15.0 * view.ui_scale / view.scale
                        && (p[1] - h.canvas[1]).abs() <= 15.0 * view.ui_scale / view.scale
                })
                .map(|_| ShapeHit::Transform(MoveHit::Translate));
        }
        let hit = self.control.hit_test_with_anchor(
            Point::new(p[0], p[1]),
            &r.transform,
            &r.bounds(),
            1.0 / view.scale,
            self.anchor_visible(),
        );
        if !matches!(
            hit,
            MoveHit::Anchor | MoveHit::Scale(_) | MoveHit::Translate
        ) && self.reset_geometry(app).is_some_and(|g| g.contains(p))
        {
            return Some(ShapeHit::Reset);
        }
        if hit == MoveHit::Background {
            let local = transform_point(&r.transform.invert()?, p);
            let bb = r.bounds();
            if local[0] < bb.left()
                || local[0] > bb.right()
                || local[1] < bb.top()
                || local[1] > bb.bottom()
            {
                return None;
            }
        }
        (hit != MoveHit::None).then_some(ShapeHit::Transform(hit))
    }
    fn move_pointer(&mut self, app: &mut AppWorkspace, p: [f64; 2]) {
        let Some(live) = self.live.clone() else {
            return;
        };
        match live {
            Live::Draw {
                press,
                button,
                begun,
                ..
            } => {
                if !begun {
                    let size = app
                        .shape_drag_size_override
                        .unwrap_or_else(platform_drag_size);
                    let scale = app.active_ws().unwrap().view.view_transform().scale;
                    if (p[0] - press[0]).abs() * scale < size[0]
                        && (p[1] - press[1]).abs() * scale < size[1]
                    {
                        return;
                    }
                    // Dirty background crossing only begins the drag, at the original press.
                    // Subsequent movement (including a release-relayed move) updates geometry.
                    if button == MouseButton::Right {
                        let from = Box::new(self.tx.record().unwrap().clone());
                        transacted::begin_edit(self, Edit::Pointer);
                        self.live = Some(Live::Pointer {
                            press,
                            last: press,
                            button,
                            hit: ShapeHit::Transform(MoveHit::Rotate),
                            from,
                        });
                    } else {
                        transacted::commit(self, app);
                        let r = self.draw_record(app, press, press, button);
                        self.live = Some(Live::Draw {
                            press,
                            last: press,
                            button,
                            begun: true,
                        });
                        transacted::begin_drawing(self, app, r, None);
                    }
                } else {
                    self.live = Some(Live::Draw {
                        press,
                        last: p,
                        button,
                        begun: true,
                    });
                    let r = self.draw_record(app, press, p, button);
                    transacted::update(self, app, r);
                }
            }
            Live::Pointer {
                press,
                button,
                hit,
                from,
                ..
            } => {
                let r = match hit {
                    ShapeHit::Transform(hit) => {
                        transform_edit(&from, hit, press, p, app.shift_key_down, app.alt_key_down)
                    }
                    ShapeHit::Reset => return,
                    ShapeHit::Node(i) => {
                        let mut r = (*from).clone();
                        if let Some(inv) = from.transform.invert() {
                            let delta = transform_vector(&inv, [p[0] - press[0], p[1] - press[1]]);
                            r.knots[i] = [from.knots[i][0] + delta[0], from.knots[i][1] + delta[1]];
                        }
                        r
                    }
                };
                self.live = Some(Live::Pointer {
                    press,
                    last: p,
                    button,
                    hit,
                    from,
                });
                transacted::update(self, app, r);
            }
            Live::Reset { .. } => {
                self.live = Some(Live::Reset {
                    inside: self.reset_geometry(app).is_some_and(|g| g.contains(p)),
                });
            }
            Live::Nudge(_) => {}
        }
    }
    fn end_nudge(&mut self, app: &mut AppWorkspace) {
        if matches!(self.live, Some(Live::Nudge(_))) {
            self.live = None;
            transacted::end(self, app);
        }
    }
}

impl<const LINE: bool> Transacted for PreviewShapeTool<LINE> {
    type Record = ShapeState;
    const TOOL: ToolType = if LINE {
        ToolType::Line
    } else {
        ToolType::Shapes
    };
    fn transaction(&mut self) -> &mut Transaction<ShapeState> {
        &mut self.tx
    }
    fn names(r: &ShapeState) -> [String; 3] {
        [
            "shapesTool.endDrawing.historyMementoName.format",
            "shapesTool.changed.generic.historyMementoName.format",
            "shapesTool.commitChanges.historyMementoName.format",
        ]
        .map(|k| Self::named(k, r))
    }
    fn drawing_icon(r: &ShapeState) -> String {
        if LINE {
            ToolType::Line.icon_src()
        } else {
            format!("shape:{}", r.text("shape"))
        }
    }
    fn settings(r: &ShapeState) -> Vec<(&'static str, Value)> {
        r.settings.clone()
    }
    fn colors(r: &ShapeState) -> Option<[Color; 2]> {
        Some(r.colors)
    }
    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &ShapeState) -> ShapeState {
        ShapeState {
            settings: Self::toolbar(app),
            custom_shape: if app.tool_settings.get(Self::TOOL, "shape").as_str()
                == Some(r.text("shape"))
            {
                r.custom_shape.clone()
            } else {
                app.shape_catalog
                    .selected(
                        app.tool_settings
                            .get(Self::TOOL, "shape")
                            .as_str()
                            .unwrap_or(""),
                    )
                    .cloned()
                    .map(std::sync::Arc::new)
            },
            colors: [app.primary_color, app.secondary_color],
            ..r.clone()
        }
    }
    fn changed(a: &ShapeState, b: &ShapeState) -> Vec<String> {
        let mut names = vec![];
        if a.colors != b.colors {
            names.push(crate::i18n::t(
                "transactedTool.changed.primaryColorOrSecondaryColor.historyMementoName",
            ));
        }
        if a.start != b.start || a.end != b.end || a.transform != b.transform {
            names.push(Self::named(
                "shapesTool.changed.transform.historyMementoName.format",
                b,
            ));
        }
        if a.anchor != b.anchor {
            names.push(crate::i18n::t(
                "shapesTool.changed.rotationAnchorOffset.historyMementoName",
            ));
        }
        // A changed shape property participates in the fold, but has no specialized label.
        if a.knots != b.knots {
            names.push(Self::names(b)[1].clone());
        }
        for ((key, av), (_, bv)) in a.settings.iter().zip(&b.settings) {
            if av == bv {
                continue;
            }
            let name = match *key {
                "shape" => "shapesTool.changed.shapeType.historyMementoName.format",
                "width" => "transactedTool.changed.brush.width.historyMementoName",
                "drawType" => "shapesTool.changed.shapeDrawType.historyMementoName",
                "dash" => "transactedTool.changed.pen.dashStyle.historyMementoName",
                "radius" => "transactedTool.changed.radius.historyMementoName",
                "fillStyle" => "transactedTool.changed.brushFillTypeHatchStyle.historyMementoName",
                "antialias" => "transactedTool.changed.antialiasing.historyMementoName",
                "blendMode" => "transactedTool.changed.blendMode.historyMementoName",
                "renderingQuality" => {
                    "transactedTool.changed.selectionRenderingQuality.historyMementoName"
                }
                "curveType" => "shapesTool.changed.curveType.historyMementoName",
                "startCap" => "transactedTool.changed.pen.startCap.historyMementoName",
                "endCap" => "transactedTool.changed.pen.endCap.historyMementoName",
                _ => continue,
            };
            names.push(Self::named(name, b));
        }
        names
    }
    fn show(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            self.base.cancel_bitmap_transaction(ws);
            if let Some(r) = self.tx.record() {
                self.base.begin_bitmap_transaction(ws);
                let d = match Self::render_data(r) {
                    Ok(data) => {
                        self.render_error = None;
                        data
                    }
                    Err(error) => {
                        self.render_error = Some(error);
                        ShapeRenderData::default()
                    }
                };
                let bounds = self.base.original().int_bounds();
                let mut affected = IntRect::default();
                // Keep tile origins fixed at multiples of 512 (the tracer phase depends on
                // them), but don't allocate or composite empty tiles outside the geometry.
                for (path, padding) in d
                    .interior
                    .iter()
                    .map(|(p, _)| (p, 1.0))
                    .chain(d.outline.iter().map(|(p, _)| (p, 1.0)))
                    .chain(
                        d.outline_stroke
                            .iter()
                            .map(|(p, s)| (p, 5.0 * s.width + 1.0)),
                    )
                {
                    let [x0, y0, x1, y1] = crate::tool::shapes::bounds(path);
                    let left = (x0 - padding).max(0.0).min(bounds.w as f64).floor();
                    let top = (y0 - padding).max(0.0).min(bounds.h as f64).floor();
                    let right = (x1 + padding).max(left).min(bounds.w as f64).ceil();
                    let bottom = (y1 + padding).max(top).min(bounds.h as f64).ceil();
                    affected = affected.union(&IntRect::new(
                        left as i32,
                        top as i32,
                        (right - left) as u32,
                        (bottom - top) as u32,
                    ));
                }
                let fg = r.colors[usize::from(r.button == MouseButton::Right)];
                let bg = r.colors[usize::from(r.button != MouseButton::Right)];
                let px = |c: Color| -> Px { [c.red, c.green, c.blue, c.alpha] };
                let both = !LINE && DrawType::from_value(r.text("drawType")) == DrawType::Both;
                let fill = FillBrush::from_style(
                    r.text("fillStyle"),
                    px(if both { bg } else { fg }),
                    px(if both { fg } else { bg }),
                );
                let pen = FillBrush::from_style(r.text("fillStyle"), px(fg), px(bg));
                // Tile before allocating supersampled masks; the tracer's phase is tile-local.
                for y in ((affected.y as u32 / 512 * 512)..affected.bottom() as u32).step_by(512) {
                    for x in ((affected.x as u32 / 512 * 512)..affected.right() as u32).step_by(512)
                    {
                        let tile = IntRect::new(
                            x as i32,
                            y as i32,
                            (bounds.w - x).min(512),
                            (bounds.h - y).min(512),
                        );
                        let mut owned = vec![];
                        if let Some((p, rule)) = &d.interior {
                            owned.push((
                                fill.render(tile),
                                coverage_mask(p, *rule, None, r.aa(), tile),
                            ));
                        }
                        let outline = if let Some(polys) = &d.tracer {
                            Some(trace_1px(
                                polys,
                                DashStyle::from_value(r.text("dash")),
                                tile,
                            ))
                        } else if let Some((p, s)) = &d.outline_stroke {
                            Some(coverage_mask(p, FillRule::NonZero, Some(s), r.aa(), tile))
                        } else {
                            d.outline
                                .as_ref()
                                .map(|(p, rule)| coverage_mask(p, *rule, None, r.aa(), tile))
                        };
                        if let Some(mask) = outline {
                            owned.push((pen.render(tile), mask));
                        }
                        let content: Vec<_> = owned
                            .iter()
                            .map(|(c, m)| ContentLayer {
                                content: c,
                                mask: Some(m),
                            })
                            .collect();
                        self.base.render_transaction(
                            ws,
                            tile,
                            &content,
                            ContentBlendMode::from_value(r.text("blendMode")),
                            SelectionQuality::from_value(r.text("renderingQuality")),
                        );
                    }
                }
            }
        }
        self.sync_handles(app);
    }
    fn commit_inner(&mut self, app: &mut AppWorkspace) -> Option<HistoryMemento> {
        self.base.commit_changed_tiles(
            app.active_ws_mut()?,
            &Self::names(self.tx.record()?)[2],
            &Self::TOOL.icon_src(),
        )
    }
}
impl<const LINE: bool> Tool for PreviewShapeTool<LINE> {
    fn tool_type(&self) -> ToolType {
        Self::TOOL
    }
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        transacted::activate(self);
        self.shift = app.shift_key_down;
        if let Some(ws) = app.active_ws_mut() {
            self.handles = Some(ws.surface_box.add_renderer(Renderer::Handles(vec![])));
        }
    }
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        transacted::deactivate(self, app);
        self.live = None;
        if let (Some(id), Some(ws)) = (self.handles.take(), app.active_ws_mut()) {
            ws.surface_box.remove_renderer(id);
        }
    }
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _: &PointerInput,
    ) -> bool {
        if matches!(self.live, Some(Live::Reset { .. })) {
            return true;
        }
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        if matches!(self.live, Some(Live::Draw { .. } | Live::Pointer { .. })) {
            return true;
        }
        let p = [x, y];
        let hit = self.hit(app, p);
        self.end_nudge(app);
        if hit == Some(ShapeHit::Reset) {
            self.live = Some(Live::Reset { inside: true });
        } else if let Some(mut hit) = hit {
            if b == MouseButton::Right && matches!(hit, ShapeHit::Transform(_)) {
                hit = ShapeHit::Transform(MoveHit::Rotate);
            }
            transacted::end(self, app);
            transacted::begin_edit(self, Edit::Pointer);
            self.live = Some(Live::Pointer {
                press: p,
                last: p,
                button: b,
                hit,
                from: Box::new(self.tx.record().unwrap().clone()),
            });
        } else {
            transacted::end(self, app);
            // Idle canvas input disallows clicks and starts Drawing on press. Only the
            // Dirty transform background waits for the OS click-versus-drag threshold.
            let begun = self.tx.state() == State::Idle;
            self.live = Some(Live::Draw {
                press: p,
                last: p,
                button: b,
                begun,
            });
            if begun {
                let r = self.draw_record(app, p, p, b);
                transacted::begin_drawing(self, app, r, None);
            }
        }
        true
    }
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _: &PointerInput) -> bool {
        if self.live.is_some() && !matches!(self.live, Some(Live::Nudge(_))) {
            self.move_pointer(app, [x, y]);
            self.cursor = Some(CursorRequest::Image("hand_closed_cursor"));
            true
        } else {
            let hit = self.hit(app, [x, y]);
            self.reset_hover = hit == Some(ShapeHit::Reset);
            if let Some(ShapeHit::Transform(h)) = hit {
                self.control.set_hover(h, app.now_ms / 1000.0);
            } else {
                self.control.set_hover(MoveHit::None, app.now_ms / 1000.0);
            }
            self.cursor = hit.map(|_| CursorRequest::Image("hand_open_cursor"));
            false
        }
    }
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _: &PointerInput,
    ) -> bool {
        let Some(live) = self.live.clone() else {
            return false;
        };
        if matches!(live, Live::Reset { .. }) {
            let activate = self.reset_geometry(app).is_some_and(|g| g.contains([x, y]));
            self.live = None;
            self.reset_hover = false;
            if activate {
                self.reset_anchor(app);
            }
            self.cursor = None;
            return true; // first relayed release, regardless of its button
        }
        let owner = match live {
            Live::Draw { button, .. } | Live::Pointer { button, .. } => button,
            Live::Nudge(_) | Live::Reset { .. } => return false,
        };
        if b != owner {
            return true;
        }
        self.move_pointer(app, [x, y]);
        let live = self.live.take();
        match live {
            Some(Live::Draw { begun: false, .. }) => {
                if self.tx.state() == State::Dirty {
                    transacted::commit(self, app);
                }
            }
            Some(Live::Draw { .. })
                if LINE && self.tx.record().is_some_and(|r| r.start == r.end) =>
            {
                transacted::cancel_gesture(self, app)
            }
            _ => transacted::end(self, app),
        }
        self.cursor = None;
        true
    }
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        if key.key == "Escape"
            && !(key.ctrl || key.shift || key.alt || key.meta)
            && matches!(
                self.live,
                Some(Live::Pointer {
                    hit: ShapeHit::Transform(_),
                    ..
                })
            )
        {
            self.live = None;
            transacted::cancel_gesture(self, app);
        }
        if key.key == "Enter" || key.key == "Escape" {
            let handled = transacted::key_press(self, app, key);
            if handled && !matches!(self.tx.edit(), Some(Edit::Settings(_))) {
                self.live = None;
            }
            return handled;
        }
        // §8.8: these are delivered characters; the UI translates punctuation once and
        // lets registered global shortcuts intercept before reaching this mapping.
        let size = match key.key.as_str() {
            "[" => Some(-1.0),
            "]" => Some(1.0),
            "\u{001b}" if key.ctrl => Some(-5.0),
            "\u{001d}" if key.ctrl => Some(5.0),
            _ => None,
        };
        let cycle = match key.key.as_str() {
            "," if LINE => Some(("startCap", &["flat", "arrow", "filledArrow", "round"][..])),
            "/" if LINE => Some(("endCap", &["flat", "arrow", "filledArrow", "round"][..])),
            "." => Some((
                "dash",
                &["solid", "dash", "dot", "dashDot", "dashDotDot"][..],
            )),
            _ => None,
        };
        let shape = key.key.eq_ignore_ascii_case("a");
        let swap = key.key.eq_ignore_ascii_case("x");
        let active_color = key.key.eq_ignore_ascii_case("c");
        if size.is_some() || cycle.is_some() || shape || swap || active_color {
            if self.live.is_some() || matches!(self.tx.state(), State::Drawing | State::Editing) {
                return true;
            }
            if active_color {
                return false;
            } // UI selection only; the palette record is unchanged.
            if swap {
                app.swap_colors();
            } else if let Some(step) = size {
                let width = app
                    .tool_settings
                    .get(Self::TOOL, "width")
                    .as_f64()
                    .unwrap_or(2.0);
                let width = (((width + step) * 100.0).round() / 100.0).clamp(1.0, 2000.0);
                let value = if width.fract() == 0.0 {
                    Value::from(width as i64)
                } else {
                    Value::from(width)
                };
                app.update_tool_setting(Self::TOOL, "width", value);
            } else if let Some((name, values)) = cycle {
                let current = app.tool_settings.get(Self::TOOL, name);
                let i = values
                    .iter()
                    .position(|v| Some(*v) == current.as_str())
                    .unwrap_or(0);
                app.update_tool_setting(Self::TOOL, name, values[(i + 1) % values.len()].into());
            } else if shape {
                if LINE {
                    return true;
                }
                let current = app.tool_settings.get(Self::TOOL, "shape");
                let values = app.shape_catalog.values();
                let i = values
                    .iter()
                    .position(|v| Some(v.as_str()) == current.as_str())
                    .unwrap_or(0);
                let n = values.len();
                let next = (i + if key.shift { n - 1 } else { 1 }) % n;
                app.update_tool_setting(Self::TOOL, "shape", values[next].clone().into());
            }
            // AppWorkspace temporarily removes this Tool while dispatching the key, so the
            // ordinary toolbar/color setter cannot relay its notification back into us.
            transacted::setting_changed(self, app);
            return true;
        }
        if app.mouse_buttons != 0 {
            return false;
        }
        let Some(delta) = super::arrow_nudge(app, &key.key) else {
            return false;
        };
        if !self.tx.is_open() {
            return false;
        }
        if delta == (0.0, 0.0) {
            return true;
        }
        if !matches!(self.live, Some(Live::Nudge(_))) {
            transacted::end(self, app);
            transacted::begin_edit(self, Edit::Pointer);
        }
        let mut r = self.tx.record().unwrap().clone();
        let step = if key.ctrl { 10.0 } else { 1.0 };
        r.transform.translate(delta.0 * step, delta.1 * step);
        transacted::update(self, app, r);
        self.live = Some(Live::Nudge(app.now_ms));
        true
    }
    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        let shift_changed = self.shift != app.shift_key_down;
        self.shift = app.shift_key_down;
        let point = match &self.live {
            Some(Live::Draw {
                last, begun: true, ..
            }) => Some(*last),
            Some(Live::Pointer { last, .. }) if shift_changed => Some(*last),
            _ => None,
        };
        if let Some(p) = point {
            self.move_pointer(app, p);
        }
    }
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if Self::keys().contains(&key) {
            self.end_nudge(app);
            transacted::setting_changed(self, app);
        }
    }
    fn on_colors_changed(&mut self, app: &mut AppWorkspace) {
        self.end_nudge(app);
        transacted::setting_changed(self, app);
    }
    fn on_executing_history_memento(&mut self, _: &mut AppWorkspace) {
        self.live = None;
    }
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        if matches!(self.live,Some(Live::Nudge(t)) if app.now_ms-t>=NUDGE_COMMIT_MS) {
            self.end_nudge(app);
        }
        self.sync_handles(app);
    }
    fn needs_ticks(&self) -> bool {
        matches!(self.live, Some(Live::Nudge(_)))
    }
    fn commit_pending(&mut self, app: &mut AppWorkspace) -> bool {
        transacted::finish_button(self, app)
    }
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }
    fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
        Some(self)
    }
    fn cursor(&self) -> CursorRequest {
        self.cursor.unwrap_or(CursorRequest::Css("crosshair"))
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::super::move_tools::Edge;
    use super::*;
    #[test]
    fn gallery_and_history_share_native_order_and_resource_labels() {
        let definitions =
            super::super::types::option_definitions(ToolType::Shapes, &Default::default());
        let gallery = definitions.iter().find(|d| d.key == "shape").unwrap();
        assert_eq!(gallery.values.len(), 29);
        for (kind, (id, label, _)) in ShapeKind::ALL.iter().zip(&gallery.values) {
            assert_eq!(kind.value(), *id);
            assert_eq!(kind.name(), *label);
        }
        assert_eq!(ShapeKind::ALL.last(), Some(&ShapeKind::Heart));
        assert_eq!(guide_bounds(ShapeKind::RightTriangle), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(guide(ShapeKind::RightTriangle).1, FillRule::EvenOdd);
    }

    #[test]
    fn collinear_curve_reversals_survive_line_tracing() {
        let knots = [[20.5, 20.5], [100.5, 20.5], [-59.5, 20.5], [21.5, 20.5]];
        let data = build_line(
            knots,
            CurveType::Bezier,
            true,
            1.0,
            CapStyle::Flat,
            CapStyle::Flat,
            DashStyle::Solid,
        );
        let mask = trace_1px(
            data.tracer.as_ref().unwrap(),
            DashStyle::Solid,
            IntRect::new(0, 0, 64, 40),
        );
        // The Bezier reaches x>40 and x<1 despite endpoints only one pixel apart.
        assert_eq!(mask[20 * 64 + 40], 255);
        assert_eq!(mask[20 * 64 + 1], 255);
    }

    #[test]
    fn tracer_phase_skips_nonintersecting_segments_and_line_extends_ends() {
        let polys = vec![
            vec![[0.5, 1.5], [3.5, 1.5]],
            vec![[513.5, 1.5], [519.5, 1.5]],
        ];
        let mask = trace_1px(&polys, DashStyle::Dash, IntRect::new(512, 0, 10, 4));
        assert_eq!(&mask[11..17], &[255, 255, 0, 255, 255, 255]);
        let knots = [[2.5, 2.5], [4.5, 2.5], [6.5, 2.5], [8.5, 2.5]];
        let d = build_line(
            knots,
            CurveType::Straight,
            false,
            1.0,
            CapStyle::Flat,
            CapStyle::Rounded,
            DashStyle::Solid,
        );
        let mask = trace_1px(
            d.tracer.as_ref().unwrap(),
            DashStyle::Solid,
            IntRect::new(0, 0, 12, 5),
        );
        assert_eq!(
            &mask[24..36],
            &[0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0]
        );
    }

    fn state(w: f64, h: f64) -> ShapeState {
        ShapeState {
            start: [0.0, 0.0],
            end: [w, h],
            knots: [[0.0; 2]; 4],
            transform: Matrix::identity(),
            anchor: None,
            settings: vec![],
            custom_shape: None,
            colors: [Color::BLACK, Color::WHITE],
            button: MouseButton::Left,
        }
    }
    fn close(a: [f64; 2], b: [f64; 2]) {
        assert!(distance(a, b) < 1e-8, "{a:?} != {b:?}");
    }
    /// §8.3.5: ordinary and parity-biased nearest-even sizes, edge/corner Shift, flips.
    #[test]
    fn scale_dimension_vectors() {
        for (dx, shift, alt, w, h) in [
            (0.5, false, false, 10.0, 6.0),
            (1.5, false, false, 12.0, 6.0),
            (-1.5, false, false, 8.0, 6.0),
            (0.25, false, true, 10.0, 6.0),
            (0.5, false, true, 12.0, 6.0),
            (-0.5, false, true, 8.0, 6.0),
            (5.0, true, false, 15.0, 8.0),
        ] {
            let r = transform_edit(
                &state(10.0, 6.0),
                MoveHit::Scale(Edge::Right),
                [10.0, 3.0],
                [10.0 + dx, 3.0],
                shift,
                alt,
            );
            let a = transform_point(&r.transform, r.start);
            let b = transform_point(&r.transform, r.end);
            close([b[0] - a[0], b[1] - a[1]], [w, h]);
        }
        for (w, dx, want) in [(5.0, 0.25, 7.0), (7.0, 0.0, 9.0)] {
            let r = transform_edit(
                &state(w, 6.0),
                MoveHit::Scale(Edge::Right),
                [w, 3.0],
                [w + dx, 3.25],
                false,
                true,
            );
            let s = scales(&r.transform);
            assert!((s[0] * w - want).abs() < 1e-10);
        }
        let r = transform_edit(
            &state(10.0, 6.0),
            MoveHit::Scale(Edge::BottomRight),
            [10.0, 6.0],
            [15.0, 7.2],
            true,
            false,
        );
        close(transform_point(&r.transform, r.end), [12.0, 7.0]);
        let r = transform_edit(
            &state(10.0, 6.0),
            MoveHit::Scale(Edge::Right),
            [10.0, 3.0],
            [-2.0, 3.0],
            false,
            false,
        );
        close(transform_point(&r.transform, r.end), [-2.0, 6.0]);
        let r = transform_edit(
            &state(10.0, 6.0),
            MoveHit::Scale(Edge::Right),
            [10.0, 3.0],
            [0.0, 3.0],
            false,
            false,
        );
        close(transform_point(&r.transform, r.end), [-0.000001, 6.0]);
    }
    /// §8.2–8.5: transforms use canvas movement but local scaling; explicit anchors stay fixed.
    #[test]
    fn transformed_axes_and_anchor_vectors() {
        let mut r = state(10.0, 6.0);
        r.transform.scale(2.0, 1.0);
        let r = transform_edit(
            &r,
            MoveHit::Scale(Edge::Right),
            [20.0, 3.0],
            [22.0, 3.0],
            true,
            false,
        );
        close(scales(&r.transform), [2.2, 14.0 / 6.0]);
        let mut r = state(10.0, 6.0);
        r.transform.rotate_at(90.0, Point::new(0.0, 0.0));
        let scaled = transform_edit(
            &r,
            MoveHit::Scale(Edge::Right),
            [-3.0, 10.0],
            [-3.0, 12.5],
            false,
            false,
        );
        close(
            transform_point(&scaled.transform, [10.0, 3.0]),
            [-3.0, 12.0],
        );
        let moved = transform_edit(&r, MoveHit::Translate, [0.9, 0.0], [2.8, -1.5], true, true);
        close(transform_point(&moved.transform, [0.0, 0.0]), [1.0, -1.0]);
        let anchor = transform_edit(&r, MoveHit::Anchor, [-3.0, 5.0], [-2.75, 4.5], true, true);
        close(anchor.anchor.unwrap(), [-2.75, 4.5]);
        let moved = transform_edit(
            &anchor,
            MoveHit::Translate,
            [0.0, 0.0],
            [2.0, -1.0],
            false,
            false,
        );
        assert_eq!(moved.anchor, anchor.anchor);
        for (a, b) in [
            (7.5, 15.0),
            (-7.5, 0.0),
            (22.5, 30.0),
            (-22.5, -15.0),
            (172.5, 180.0),
            (-172.5, -165.0),
        ] {
            assert_eq!(constrain_angle(a), b);
        }
    }
    /// Every built-in has finite geometry and an observable fill/outline under both AA modes.
    #[test]
    fn all_builtins_render() {
        for kind in ShapeKind::ALL {
            let d = build_shape(
                kind,
                [12.0, 12.0, 40.0, 40.0],
                3.0,
                DrawType::Both,
                DashStyle::Dash,
                10.0,
            );
            let (p, rule) = d.interior.unwrap();
            for aa in [false, true] {
                assert!(
                    coverage_mask(&p, rule, None, aa, IntRect::new(0, 0, 64, 64)).contains(&255),
                    "{}",
                    kind.value()
                );
            }
            assert!(d.outline.is_some() || d.outline_stroke.is_some());
        }
    }
    /// §8.4: rebuilding after scale keeps the brush's absolute width, even nonuniformly.
    #[test]
    fn scaled_shape_keeps_outline_width() {
        let mut r = state(20.0, 20.0);
        r.start = [10.0, 10.0];
        r.end = [30.0, 30.0];
        r.settings = vec![("width", 2.into()), ("antialias", false.into())];
        r.transform.scale(2.0, 1.0);
        let d = ShapesTool::render_data(&r).unwrap();
        let (p, s) = d.outline_stroke.unwrap();
        let m = coverage_mask(
            &p,
            FillRule::NonZero,
            Some(&s),
            false,
            IntRect::new(0, 0, 80, 40),
        );
        assert_eq!(&m[20 * 80 + 19..20 * 80 + 24], &[0, 255, 255, 0, 0]);
    }
    /// Two knots remain a straight cubic; spline endpoints have zero second derivative.
    #[test]
    fn spline_two_knots_and_natural_end_derivatives() {
        let c = natural_spline(&[[0.0, 0.0], [9.0, 6.0]]);
        assert_eq!(c, vec![[[3.0, 2.0], [6.0, 4.0], [9.0, 6.0]]]);
        let q = [
            [1.0, 4.0],
            [8.0, -3.0],
            [20.0, 7.0],
            [30.0, 9.0],
            [32.0, 1.0],
        ];
        let c = natural_spline(&q);
        for k in 0..2 {
            assert!((q[0][k] - 2.0 * c[0][0][k] + c[0][1][k]).abs() < 1e-10);
            let last = c.last().unwrap();
            assert!((last[0][k] - 2.0 * last[1][k] + last[2][k]).abs() < 1e-10);
        }
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;
    use crate::testing::{Ev, drive_tool, fresh_doc, with_tool};

    #[test]
    fn reset_layout_and_rotated_half_open_hit_vectors() {
        let horizontal = ResetGeometry::new([50.0, 30.0], [80.0, 30.0], 12.0, 20.0);
        assert_eq!(horizontal.center, [104.0, 30.0]);
        assert!(horizontal.contains([94.0, 20.0]));
        assert!(!horizontal.contains([114.0, 30.0]));
        assert!(!horizontal.contains([104.0, 40.0]));
        let diagonal = ResetGeometry::new([50.0, 30.0], [80.0, 70.0], 12.0, 20.0);
        assert!((diagonal.center[0] - 94.4).abs() < 1e-12);
        assert!((diagonal.center[1] - 89.2).abs() < 1e-12);
        assert!((diagonal.angle - 0.8f64.atan2(0.6)).abs() < 1e-12);
        assert!(diagonal.contains([diagonal.center[0] - 1.8, diagonal.center[1] + 12.6]));
        assert!(!diagonal.contains([diagonal.center[0] + 6.6, diagonal.center[1] + 8.8]));
        let centered = ResetGeometry::new([50.0, 30.0], [50.0, 30.0], 12.0, 15.0);
        assert_eq!(centered.center, [50.0, 30.0]);
        assert_eq!(centered.angle, 0.0);
        assert!(centered.contains([42.5, 22.5]));
        assert!(!centered.contains([57.5, 30.0]));
    }

    #[test]
    fn reset_null_noop_and_measured_geometry_at_zoom_and_ui_scale() {
        use MouseButton::Left;
        let mut app = fresh_doc(260, 240, None);
        drive_tool(
            &mut app,
            ToolType::Shapes,
            &[
                Ev::Down(40.0, 40.0, Left),
                Ev::Move(140.0, 100.0),
                Ev::Up(140.0, 100.0, Left),
            ],
        );
        let n = app.active_ws().unwrap().history.undo.len();
        with_tool::<ShapesTool>(&mut app, |t, app| t.reset_anchor(app));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        drive_tool(
            &mut app,
            ToolType::Shapes,
            &[
                Ev::Down(90.0, 70.0, Left),
                Ev::Move(90.0, 140.0),
                Ev::Up(90.0, 140.0, Left),
            ],
        );
        let view = &mut app.active_ws_mut().unwrap().view;
        view.zoom = 2.0;
        view.ppp = 1.5;
        with_tool::<ShapesTool>(&mut app, |t, app| {
            let reset = t.reset_geometry(app).unwrap();
            assert_eq!(reset.size, 15.0);
            // 8.5 DIP measured circle width ×1.5/2, plus eight hair widths, twice.
            assert_eq!(reset.center, [90.0, 160.75]);
        });
    }
}
