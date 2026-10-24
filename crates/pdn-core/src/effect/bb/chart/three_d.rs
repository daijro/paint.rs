//! BB spec: effects/render-chart-or-graph.md "3-D"; common.md §5.10
//!
//! The 3-D chart box (R194 Q50, `conventions::CHART_3D_OBLIQUE_RIGHT_ANGLE_V1`): the oblique
//! projection with right-angle axes, the box's uniform fit with its walls, and the visible shaded
//! faces of boxes, ribbons and slabs. Model x runs right, y up and z into the depth; screen y
//! runs down.
use crate::effect::bb::conventions::CHART_3D_OBLIQUE_RIGHT_ANGLE_V1 as VIEW;
use tiny_skia::{Path, PathBuilder};

/// `conventions::CHART_3D_OBLIQUE_RIGHT_ANGLE_V1`'s choices.
#[derive(Clone, Copy, Debug)]
pub struct Oblique {
    /// A model point's screen offset, before the uniform fit.
    pub project: fn([f64; 3]) -> [f64; 2],
    /// The shade of faces whose normal is mostly up, and of those mostly along x; front faces
    /// (mostly along z) keep their colour.
    pub top: f64,
    pub side: f64,
    /// Pie and Doughnut thickness, a fraction of the diameter.
    pub disc_thickness: f64,
}

/// "3-D": "an oblique 3-D box: tilted 30° (elevation) and turned 30° (rotation), with no
/// perspective; right-angle axes" (R194). x and y stay unrotated; depth goes where a 30° turn
/// then a 30° tilt take it: right by sin 30°, up by sin 30° cos 30° per unit.
pub fn project([x, y, z]: [f64; 3]) -> [f64; 2] {
    let (s, c) = 30f64.to_radians().sin_cos();
    [x + s * z, -y - s * c * z]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The unit model direction toward the viewer: the (oblique) projection's kernel, along which
/// points share a pixel, on the −z side. A point with a larger `dot` with it is nearer.
pub fn toward_viewer() -> [f64; 3] {
    let [a, b, c] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(VIEW.project);
    let (u, v) = ([a[0], b[0], c[0]], [a[1], b[1], c[1]]);
    let k = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let n = dot(k, k).sqrt() * if k[2] > 0.0 { -1.0 } else { 1.0 };
    k.map(|x| x / n)
}

/// `rgb` with every channel × `m`, rounded and clamped to a byte.
pub fn shade(rgb: u32, m: f64) -> u32 {
    [16, 8, 0].into_iter().fold(0, |out, at| {
        let c = ((rgb >> at & 255) as f64 * m).round().clamp(0.0, 255.0) as u32;
        out | c << at
    })
}

/// A planar face's shade from its normal's largest model component: up is a top, x a side,
/// z the front.
pub fn face_shade(n: [f64; 3]) -> f64 {
    let [x, y, z] = n.map(f64::abs);
    if y >= x && y >= z {
        VIEW.top
    } else if x >= z {
        VIEW.side
    } else {
        1.0
    }
}

/// A visible planar face: its model polygon and its shade.
#[derive(Clone, Debug)]
pub struct Face {
    pub pts: Vec<[f64; 3]>,
    pub shade: f64,
}

/// The corner quads of a hexahedron whose corners are a bottom quad and then the top quad
/// above it, in the same order.
pub const FRUSTUM: [[usize; 4]; 6] = [
    [0, 1, 2, 3],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

/// The faces of a convex solid (its corners and their quads) that face the viewer; degenerate
/// faces are dropped.
pub fn convex_faces(corners: &[[f64; 3]], quads: &[[usize; 4]]) -> Vec<Face> {
    let mean = |pts: &[[f64; 3]]| {
        let n = pts.len() as f64;
        let s = pts
            .iter()
            .fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
        s.map(|v| v / n)
    };
    let (mid, v) = (mean(corners), toward_viewer());
    quads
        .iter()
        .filter_map(|q| {
            let pts: Vec<[f64; 3]> = q.iter().map(|&i| corners[i]).collect();
            // Newell's normal, turned away from the solid's middle.
            let mut n = [0.0; 3];
            for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)) {
                n[0] += (a[1] - b[1]) * (a[2] + b[2]);
                n[1] += (a[2] - b[2]) * (a[0] + b[0]);
                n[2] += (a[0] - b[0]) * (a[1] + b[1]);
            }
            let c = mean(&pts);
            if dot(n, [c[0] - mid[0], c[1] - mid[1], c[2] - mid[2]]) < 0.0 {
                n = n.map(|x| -x);
            }
            let len = dot(n, n).sqrt();
            (len > 0.0 && dot(n, v) > 1e-9 * len).then(|| Face {
                pts,
                shade: face_shade(n),
            })
        })
        .collect()
}

/// The corners of the box [lo, hi] in `FRUSTUM` order.
pub fn corners(lo: [f64; 3], hi: [f64; 3]) -> [[f64; 3]; 8] {
    let floor = |y| {
        [
            [lo[0], y, lo[2]],
            [hi[0], y, lo[2]],
            [hi[0], y, hi[2]],
            [lo[0], y, hi[2]],
        ]
    };
    let ([a, b, c, d], [e, f, g, h]) = (floor(lo[1]), floor(hi[1]));
    [a, b, c, d, e, f, g, h]
}

/// The visible faces of the box [lo, hi].
pub fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Vec<Face> {
    convex_faces(&corners(lo, hi), &FRUSTUM)
}

fn farthest_first(faces: Vec<Face>) -> Vec<Face> {
    let v = toward_viewer();
    let mut keyed: Vec<(f64, Face)> = faces
        .into_iter()
        .map(|f| {
            (
                f.pts.iter().map(|&p| dot(p, v)).sum::<f64>() / f.pts.len() as f64,
                f,
            )
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    keyed.into_iter().map(|k| k.1).collect()
}

fn quad(a: [f64; 2], b: [f64; 2], [z0, z1]: [f64; 2]) -> Vec<[f64; 3]> {
    vec![
        [a[0], a[1], z0],
        [b[0], b[1], z0],
        [b[0], b[1], z1],
        [a[0], a[1], z1],
    ]
}

/// "Line types become ribbons": one quad per segment of `line` (model x, y) between depths
/// `z`, shaded by the side the viewer sees, farthest first.
pub fn ribbon(line: &[[f64; 2]], z: [f64; 2]) -> Vec<Face> {
    let v = toward_viewer();
    let faces = line.windows(2).filter(|w| w[0] != w[1]).map(|w| {
        let n = [w[0][1] - w[1][1], w[1][0] - w[0][0], 0.0];
        let n = if dot(n, v) < 0.0 { n.map(|x| -x) } else { n };
        Face {
            pts: quad(w[0], w[1], z),
            shade: face_shade(n),
        }
    });
    farthest_first(faces.collect())
}

/// "Areas become slabs": the outline of `upper` (the values) over `lower` (the baseline or the
/// second values, at the same x) between depths `z`: its faces that face the viewer, farthest
/// first, then its front.
pub fn slab(upper: &[[f64; 2]], lower: &[[f64; 2]], z: [f64; 2]) -> Vec<Face> {
    let v = toward_viewer();
    let n = upper.len().min(lower.len());
    let mut sides = Vec::new();
    let mut side = |a: [f64; 2], b: [f64; 2], normal: [f64; 3]| {
        if dot(normal, v) > 0.0 {
            sides.push(Face {
                pts: quad(a, b, z),
                shade: face_shade(normal),
            });
        }
    };
    for (edge, other) in [(upper, lower), (lower, upper)] {
        for i in 1..n {
            let (a, b) = (edge[i - 1], edge[i]);
            if a == b {
                continue;
            }
            // Outward: away from the opposite edge.
            let normal = [a[1] - b[1], b[0] - a[0], 0.0];
            let away = (a[1] + b[1]) - (other[i - 1][1] + other[i][1]);
            side(
                a,
                b,
                normal.map(|x| if normal[1] * away < 0.0 { -x } else { x }),
            );
        }
    }
    // The two ends, each facing away from its neighbour.
    for (i, j) in [(0, 1), (n.saturating_sub(1), n.saturating_sub(2))] {
        if n >= 2 {
            let out = if upper[i][0] < upper[j][0] { -1.0 } else { 1.0 };
            side(upper[i], lower[i], [out, 0.0, 0.0]);
        }
    }
    let mut faces = farthest_first(sides);
    let front = if v[2] < 0.0 { z[0] } else { z[1] };
    faces.push(Face {
        pts: upper[..n]
            .iter()
            .chain(lower[..n].iter().rev())
            .map(|p| [p[0], p[1], front])
            .collect(),
        shade: face_shade([0.0, 0.0, -1.0]),
    });
    faces
}

/// The projected bounds of model points.
pub fn bounds(pts: &[[f64; 3]]) -> ([f64; 2], [f64; 2]) {
    pts.iter().map(|&p| (VIEW.project)(p)).fold(
        ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]),
        |(lo, hi), q| {
            (
                [lo[0].min(q[0]), lo[1].min(q[1])],
                [hi[0].max(q[0]), hi[1].max(q[1])],
            )
        },
    )
}

/// The uniform scale and offset that fit projected bounds [lo, hi] into `area`, centred; 0
/// when nothing fits.
pub fn fit((lo, hi): ([f64; 2], [f64; 2]), [l, t, r, b]: [f64; 4]) -> (f64, [f64; 2]) {
    let k = ((r - l) / (hi[0] - lo[0])).min((b - t) / (hi[1] - lo[1]));
    let k = if k.is_finite() && k > 0.0 { k } else { 0.0 };
    let o = [
        (l + r) / 2.0 - k * (lo[0] + hi[0]) / 2.0,
        (t + b) / 2.0 - k * (lo[1] + hi[1]) / 2.0,
    ];
    (k, o)
}

/// The 3-D chart box in a plot: a `size` model box (the 2-D plot's width and height, the series'
/// total depth) with its back wall and floor `wall` model units thick behind and below it, scaled
/// uniformly by `k` from `origin` so that the whole fits the plot, centred.
#[derive(Clone, Copy, Debug)]
pub struct Solid {
    pub k: f64,
    pub origin: [f64; 2],
    pub wall: f64,
    pub size: [f64; 3],
}

impl Solid {
    /// "Back and floor walls 7 px thick": `wall_px` screen pixels after the fit.
    pub fn fit([l, t, r, b]: [f64; 4], depth: f64, wall_px: f64) -> Self {
        let size = [r - l, b - t, depth];
        let extent = |wall: f64| {
            bounds(&corners(
                [0.0, -wall, 0.0],
                [size[0], size[1], depth + wall],
            ))
        };
        // The projected extent is linear in the wall: the bare box's, plus fixed pixels per wall.
        let ((lo0, hi0), (lo1, hi1)) = (extent(0.0), extent(1.0));
        let k = (0..2)
            .map(|i| {
                let span = hi0[i] - lo0[i];
                let per_wall = hi1[i] - lo1[i] - span;
                match span > 0.0 {
                    true => ((size[i] - wall_px * per_wall) / span).max(0.0),
                    false => f64::INFINITY,
                }
            })
            .fold(f64::INFINITY, f64::min);
        let k = if k.is_finite() { k } else { 0.0 };
        let wall = if k > 0.0 { wall_px / k } else { 0.0 };
        let (k, origin) = match k > 0.0 {
            true => fit(extent(wall), [l, t, r, b]),
            false => (0.0, [l, t]),
        };
        Solid {
            k,
            origin,
            wall,
            size,
        }
    }

    pub fn at(&self, p: [f64; 3]) -> [f64; 2] {
        let q = (VIEW.project)(p);
        [
            self.origin[0] + self.k * q[0],
            self.origin[1] + self.k * q[1],
        ]
    }
}

/// A closed polygon through screen points.
pub fn polygon(pts: &[[f64; 2]]) -> Option<Path> {
    let (first, rest) = pts.split_first()?;
    let mut pb = PathBuilder::new();
    pb.move_to(first[0] as f32, first[1] as f32);
    rest.iter()
        .for_each(|p| pb.line_to(p[0] as f32, p[1] as f32));
    pb.close();
    pb.finish()
}

/// A face's screen polygon through `at`.
pub fn path(face: &Face, at: impl Fn([f64; 3]) -> [f64; 2]) -> Option<Path> {
    polygon(&face.pts.iter().map(|&p| at(p)).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 2], b: [f64; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12,
            "{a:?} vs {b:?}"
        );
    }

    /// R194 (Q50): an oblique box with right-angle axes, x right, y up and z depth: x and y stay
    /// unrotated, the depth axis goes to the screen vector a 30° elevation and a 30° rotation
    /// give it; no perspective.
    #[test]
    fn projection_basis_and_view() {
        let (s, c) = (0.5, 3f64.sqrt() / 2.0);
        close(project([1.0, 0.0, 0.0]), [1.0, 0.0]);
        close(project([0.0, 1.0, 0.0]), [0.0, -1.0]);
        close(project([0.0, 0.0, 1.0]), [s, -s * c]);
        // Linear, no perspective: depth never scales a shape.
        let (a, b) = ([3.0, -2.0, 5.0], [-1.5, 4.0, 0.25]);
        let sum = project([a[0] + b[0], a[1] + b[1], a[2] + b[2]]);
        let (pa, pb) = (project(a), project(b));
        close(sum, [pa[0] + pb[0], pa[1] + pb[1]]);
        close(
            project([7.0 * a[0], 7.0 * a[1], 7.0 * a[2]]),
            [7.0 * pa[0], 7.0 * pa[1]],
        );
        // Right-angle axes: any face at one depth is its own axis-aligned rectangle, moved by
        // the depth vector.
        let face =
            [[2.0, 3.0], [7.0, 3.0], [7.0, 9.0], [2.0, 9.0]].map(|p| project([p[0], p[1], 4.0]));
        assert!(face[0][1] == face[1][1] && face[1][0] == face[2][0]);
        assert!(face[2][1] == face[3][1] && face[3][0] == face[0][0]);
        assert_eq!(
            (face[1][0] - face[0][0], face[0][1] - face[3][1]),
            (5.0, 6.0)
        );
        close(
            [face[0][0] - 2.0, face[0][1] + 3.0],
            [4.0 * s, -4.0 * s * c],
        );
        // The view direction projects to a point; the front, top and right faces are seen.
        let v = toward_viewer();
        let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
        close(project(v), [0.0, 0.0]);
        assert!((dot(v, v) - 1.0).abs() < 1e-12);
        assert!(v[0] > 0.0 && v[1] > 0.0 && v[2] < 0.0);
    }

    /// Faces shade ×1.2 when mostly up, ×0.8 when mostly sideways, else ×1, clamped; a box
    /// shows exactly its front, top and right faces.
    #[test]
    fn faces_shade_and_cull() {
        assert_eq!(shade(0x804020, 1.2), 0x9A4D26);
        assert_eq!(shade(0x804020, 0.8), 0x66331A);
        assert_eq!(shade(0xFF8000, 1.2), 0xFF9A00, "clamped");
        assert_eq!(face_shade([0.0, 1.0, 0.0]), 1.2);
        assert_eq!(face_shade([0.9, 0.3, -0.3]), 0.8);
        assert_eq!(face_shade([0.3, 0.2, -0.9]), 1.0);
        let faces = cuboid([0.0, 0.0, 0.0], [2.0, 3.0, 4.0]);
        assert_eq!(faces.len(), 3);
        let on = |f: &Face, axis: usize, v: f64| f.pts.iter().all(|p| p[axis] == v);
        let front = faces.iter().find(|f| on(f, 2, 0.0)).unwrap();
        let top = faces.iter().find(|f| on(f, 1, 3.0)).unwrap();
        let right = faces.iter().find(|f| on(f, 0, 2.0)).unwrap();
        assert_eq!((front.shade, top.shade, right.shade), (1.0, 1.2, 0.8));
        // A steep square frustum (a pyramid segment): its front and right slopes, and its top.
        let (lo, hi) = (2.0, 1.5);
        let corners = [
            [-lo, 0.0, -lo],
            [lo, 0.0, -lo],
            [lo, 0.0, lo],
            [-lo, 0.0, lo],
            [-hi, 1.0, -hi],
            [hi, 1.0, -hi],
            [hi, 1.0, hi],
            [-hi, 1.0, hi],
        ];
        let faces = convex_faces(&corners, &FRUSTUM);
        assert_eq!(faces.len(), 3);
        let centre = |f: &Face| {
            let n = f.pts.len() as f64;
            let s = f
                .pts
                .iter()
                .fold([0.0; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
            s.map(|v| v / n)
        };
        let c: Vec<_> = faces.iter().map(centre).collect();
        assert!(c.iter().any(|p| p[2] < -1.0) && c.iter().any(|p| p[0] > 1.0));
        assert!(c.iter().any(|p| p[1] == 1.0), "the top square");
        assert!(
            c.iter().all(|p| p[0] > -1.0 && p[2] < 1.0),
            "no back or left face"
        );
    }

    /// A line's ribbon: one quad per segment across the depth, shaded by the side the viewer
    /// sees, farthest first.
    #[test]
    fn ribbons_face_the_viewer() {
        let line = [[0.0, 10.0], [20.0, 0.0], [21.0, 10.0], [21.0, 20.0]];
        let faces = ribbon(&line, [3.0, 7.0]);
        assert_eq!(faces.len(), 3);
        for f in &faces {
            assert!(f.pts.iter().any(|p| p[2] == 3.0) && f.pts.iter().any(|p| p[2] == 7.0));
        }
        let shade_of = |a: [f64; 2]| faces.iter().find(|f| f.pts[0][..2] == a).unwrap().shade;
        assert_eq!(shade_of([0.0, 10.0]), 1.2, "a gentle fall shows its top");
        assert_eq!(shade_of([20.0, 0.0]), 0.8, "a steep rise shows its side");
        assert_eq!(shade_of([21.0, 10.0]), 0.8, "a vertical step");
        let depth = |f: &Face| {
            let v = toward_viewer();
            f.pts
                .iter()
                .map(|p| p[0] * v[0] + p[1] * v[1] + p[2] * v[2])
                .sum::<f64>()
        };
        assert!(faces.windows(2).all(|w| depth(&w[0]) <= depth(&w[1])));
        assert!(ribbon(&[[1.0, 1.0], [1.0, 1.0]], [0.0, 1.0]).is_empty());
    }

    /// An area's slab: the visible outline faces farthest first, the front last.
    #[test]
    fn slabs_show_their_outline_then_front() {
        let up = [[0.0, 5.0], [10.0, 8.0], [20.0, 2.0]];
        let base = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
        let faces = slab(&up, &base, [0.0, 4.0]);
        // Two top facets and the right end; not the left end or the floor.
        assert_eq!(faces.len(), 4);
        let front = faces.last().unwrap();
        assert!(front.pts.iter().all(|p| p[2] == 0.0) && front.shade == 1.0);
        assert_eq!(front.pts.len(), 6);
        assert!(faces[..3].iter().all(|f| f.pts.iter().all(|p| p[0] >= 0.0)));
        assert!(
            faces[..3]
                .iter()
                .any(|f| f.pts.iter().all(|p| p[0] == 20.0))
        );
        assert!(!faces[..3].iter().any(|f| f.pts.iter().all(|p| p[0] == 0.0)));
        assert!(!faces[..3].iter().any(|f| f.pts.iter().all(|p| p[1] == 0.0)));
        // Below the baseline the top of the slab is the baseline itself.
        let down = [[0.0, -5.0], [10.0, -3.0]];
        let faces = slab(&down, &[[0.0, 0.0], [10.0, 0.0]], [0.0, 4.0]);
        assert!(
            faces[..faces.len() - 1]
                .iter()
                .any(|f| f.pts.iter().all(|p| p[1] == 0.0))
        );
        assert!(
            !faces[..faces.len() - 1]
                .iter()
                .any(|f| f.pts.iter().any(|p| p[1] == -5.0) && f.pts.iter().any(|p| p[1] == -3.0))
        );
    }

    /// The box with its walls fits the plot uniformly, centred, the walls 7 screen pixels.
    #[test]
    fn the_box_fits_with_seven_pixel_walls() {
        let plot = [10.0, 20.0, 410.0, 320.0];
        let s = Solid::fit(plot, 60.0, 7.0);
        assert!((s.k * s.wall - 7.0).abs() < 1e-9);
        assert_eq!(s.size, [400.0, 300.0, 60.0]);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for x in [0.0, 400.0] {
            for y in [-s.wall, 300.0] {
                for z in [0.0, 60.0 + s.wall] {
                    let p = s.at([x, y, z]);
                    lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                    hi = [hi[0].max(p[0]), hi[1].max(p[1])];
                }
            }
        }
        assert!(lo[0] >= 10.0 - 1e-9 && hi[0] <= 410.0 + 1e-9);
        assert!(lo[1] >= 20.0 - 1e-9 && hi[1] <= 320.0 + 1e-9);
        assert!((hi[0] - lo[0] - 400.0).abs() < 1e-9 || (hi[1] - lo[1] - 300.0).abs() < 1e-9);
        assert!(((lo[0] + hi[0]) / 2.0 - 210.0).abs() < 1e-9);
        assert!(((lo[1] + hi[1]) / 2.0 - 170.0).abs() < 1e-9);
        // An exhausted plot draws nothing.
        assert_eq!(Solid::fit([0.0, 0.0, 5.0, 5.0], 10.0, 7.0).k, 0.0);
    }
}
