//! Photocraft phase spec: S§4.2, S§4.4; research R§B (Blurs)
//!
//! The Extra Shape, Field, Spin and Path Blur effects (`extra::fx::{shape_blur, gallery}`).
//! Centres, pins and the path anchor on the selection bounds, as upstream anchors on
//! `ctx.bounds`; the whole layer is read and only the bounds are written. Pins and path ends are
//! Pan rows, which only the IndirectUi path draws.
//!
//! Dropped upstream controls: none can be dead on an RGB document. Field Blur takes two pins
//! (upstream a pin list) because a single pin interpolates to one uniform blur, so its centre
//! could change nothing.

use crate::canvas::ImageData;
use crate::effect::bb::ui::IndirectUi;
use crate::effect::{
    Category, Control, EffectDef, EffectEnv, EffectKind, Values, choice, num, range,
};
use crate::extra::fx::gallery::{BlurPath, FieldPin, SpinPin};
use crate::extra::fx::shape_blur::BlurShape;
use crate::extra::{fx, interrupt::Interrupt, shim};

use super::blur::{bounds, finish, fraction, pan_control, pan_frac};

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

static GALLERY_UI: IndirectUi = IndirectUi::DEFAULT;

const SHAPES: &[(&str, &str)] = &[
    ("circle", "Circle"),
    ("ring", "Ring"),
    ("square", "Square"),
    ("diamond", "Diamond"),
    ("triangle", "Triangle"),
    ("hexagon", "Hexagon"),
    ("star", "Star"),
    ("heart", "Heart"),
    ("cross", "Cross"),
];

pub fn effects() -> Vec<EffectDef> {
    let blurs = |id: &'static str,
                 controls: Vec<Control>,
                 render: RenderFn,
                 indirect: Option<&'static IndirectUi>| {
        let m = super::manifest::manifest(id).expect("manifest row");
        EffectDef {
            id,
            icon: m.icon,
            category: Category::Effect("blurring"),
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(m.name),
            indirect,
        }
    };
    vec![
        blurs(
            "pcShapeBlur",
            vec![
                range("radius", "Radius", 0.0, 1000.0, 10.0),
                Control::Select {
                    key: "shape",
                    label: "Shape",
                    default: "circle",
                    choices: SHAPES,
                },
            ],
            shape_blur,
            None,
        ),
        blurs(
            "pcFieldBlur",
            vec![
                range("blur", "Blur", 0.0, 500.0, 15.0),
                pan_control("center", "Center", [-0.5, 0.0]),
                range("blur2", "Second Blur", 0.0, 500.0, 0.0),
                pan_control("center2", "Second Center", [0.5, 0.0]),
            ],
            field_blur,
            Some(&GALLERY_UI),
        ),
        blurs(
            "pcSpinBlur",
            vec![
                range("blurAngle", "Blur Angle", 0.0, 360.0, 15.0),
                pan_control("center", "Center", [0.0, 0.0]),
                fraction("radiusX", "Radius X", 0.01, 1.0, 0.3),
                fraction("radiusY", "Radius Y", 0.01, 1.0, 0.3),
                range("angle", "Angle", -180.0, 180.0, 0.0),
            ],
            spin_blur,
            Some(&GALLERY_UI),
        ),
        blurs(
            "pcPathBlur",
            vec![
                range("speed", "Speed", 0.0, 500.0, 50.0),
                range("taper", "Taper", 0.0, 100.0, 0.0),
                range("curve", "Curve", -100.0, 100.0, 0.0),
                pan_control("start", "Start", [-0.6, 0.0]),
                pan_control("end", "End", [0.6, 0.0]),
            ],
            path_blur,
            Some(&GALLERY_UI),
        ),
    ]
}

fn dims(src: &ImageData) -> (usize, usize) {
    (src.width as usize, src.height as usize)
}

fn shape_of(key: &str) -> BlurShape {
    match key {
        "ring" => BlurShape::Ring,
        "square" => BlurShape::Square,
        "diamond" => BlurShape::Diamond,
        "triangle" => BlurShape::Triangle,
        "hexagon" => BlurShape::Hexagon,
        "star" => BlurShape::Star,
        "heart" => BlurShape::Heart,
        "cross" => BlurShape::Cross,
        _ => BlurShape::Circle,
    }
}

fn shape_blur(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = fx::shape_blur::shape_blur(
        &shim::to_f32(src),
        dims(src),
        bounds(env),
        num(v, "radius") as f32,
        shape_of(choice(v, "shape")),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn field_blur(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let pin = |centre, blur| {
        let (x, y) = pan_frac(v, centre);
        FieldPin {
            x,
            y,
            blur: num(v, blur) as f32,
        }
    };
    let out = fx::gallery::field(
        &shim::to_f32(src),
        dims(src),
        bounds(env),
        &[pin("center", "blur"), pin("center2", "blur2")],
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn spin_blur(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (x, y) = pan_frac(v, "center");
    let pin = SpinPin {
        x,
        y,
        radius_x: num(v, "radiusX").max(0.001) as f32,
        radius_y: num(v, "radiusY").max(0.001) as f32,
        angle: num(v, "angle").clamp(-360.0, 360.0) as f32,
        blur_angle: num(v, "blurAngle").clamp(0.0, 360.0) as f32,
    };
    let out = fx::gallery::spin(
        &shim::to_f32(src),
        dims(src),
        bounds(env),
        &[pin],
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

/// Segments the curve is flattened to.
const CURVE_SEGMENTS: usize = 16;

/// Start to End as a polyline of fractions of the bounds (`size` px). `curve` (-100..100) bows
/// it: the quadratic Bezier's control point sits `curve` percent of the chord length, in
/// pixels, to the side of the Start to End direction (the other side when negative).
// approximation: upstream draws free paths on canvas; this is a parameterised one-curve path.
fn path_points(
    start: (f32, f32),
    end: (f32, f32),
    curve: f32,
    (bw, bh): (f32, f32),
) -> Vec<[f32; 2]> {
    let k = curve.clamp(-100.0, 100.0) / 100.0;
    if k == 0.0 {
        return vec![[start.0, start.1], [end.0, end.1]];
    }
    // Everything below in pixels, so the offset is perpendicular to the chord on any bounds.
    let (s, e) = ((start.0 * bw, start.1 * bh), (end.0 * bw, end.1 * bh));
    let (dx, dy) = (e.0 - s.0, e.1 - s.1);
    let ctl = ((s.0 + e.0) / 2.0 + dy * k, (s.1 + e.1) / 2.0 - dx * k);
    (0..=CURVE_SEGMENTS)
        .map(|i| {
            let t = i as f32 / CURVE_SEGMENTS as f32;
            let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
            [
                (a * s.0 + b * ctl.0 + c * e.0) / bw.max(1.0),
                (a * s.1 + b * ctl.1 + c * e.1) / bh.max(1.0),
            ]
        })
        .collect()
}

fn path_blur(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let path = BlurPath {
        points: path_points(
            pan_frac(v, "start"),
            pan_frac(v, "end"),
            num(v, "curve") as f32,
            (env.bounds.w.max(1) as f32, env.bounds.h.max(1) as f32),
        ),
        speed: num(v, "speed").clamp(0.0, 500.0) as f32,
        taper: num(v, "taper").clamp(0.0, 100.0) as f32,
    };
    let out = fx::gallery::path(
        &shim::to_f32(src),
        dims(src),
        bounds(env),
        &[path],
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::test_util::{rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use serde_json::json;

    fn checker(x: u32, y: u32) -> [u8; 4] {
        let v = if (x + y).is_multiple_of(2) { 0 } else { 255 };
        [v, v, v, 255]
    }

    /// Mean absolute difference of the red channel over a box.
    fn diff(a: &ImageData, b: &ImageData, x0: u32, y0: u32, x1: u32, y1: u32) -> f32 {
        let mut s = 0.0;
        for (x, y) in (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))) {
            s += f32::from(px(a, x, y)[0].abs_diff(px(b, x, y)[0]));
        }
        s / ((x1 - x0) * (y1 - y0)) as f32
    }

    /// Radius 0 changes nothing, for every shape.
    #[test]
    fn shape_blur_radius_zero_is_identity() {
        let src = from_fn(24, 24, checker);
        for shape in super::SHAPES {
            let out = run("pcShapeBlur", json!({"radius": 0, "shape": shape.0}), &src);
            assert_eq!(out.data, src.data, "{}", shape.0);
        }
        let out = run("pcShapeBlur", json!({"radius": 4}), &src);
        let v = px(&out, 12, 12)[0];
        assert!(v.abs_diff(127) < 40, "{v}");
    }

    /// Shape Blur is a box-mean over the shape: a Square of radius 2 averages the 5x5
    /// neighbourhood, so an interior pixel of a checker is 12 or 13 of 25 bright.
    #[test]
    fn shape_blur_square_is_the_window_mean() {
        let src = from_fn(24, 24, checker);
        let out = run("pcShapeBlur", json!({"radius": 2, "shape": "square"}), &src);
        let bright = |x: u32, y: u32| (x + y) % 2 == 1;
        let n = (10..15)
            .flat_map(|y| (10..15).map(move |x| (x, y)))
            .filter(|&(x, y)| bright(x, y))
            .count();
        let want = (n as f32 * 255.0 / 25.0 + 0.5) as u8;
        assert_eq!(px(&out, 12, 12)[0], want);
    }

    /// A selection limits the writes to its bounds, the whole layer is read.
    #[test]
    fn shape_blur_writes_only_the_bounds_but_reads_outside() {
        let src = from_fn(32, 32, |x, _| {
            if x < 12 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let (mask, polys) = rect_selection(32, 32, 12, 8, 28, 24);
        let env = with_selection(32, 32, &mask, &polys);
        let out = run_env("pcShapeBlur", json!({"radius": 3}), &src, &env);
        for (x, y) in (0..32).flat_map(|y| (0..32).map(move |x| (x, y))) {
            if !(12..28).contains(&x) || !(8..24).contains(&y) {
                assert_eq!(px(&out, x, y), px(&src, x, y), "{x},{y}");
            }
        }
        // The white strip left of the bounds bleeds in.
        assert!(px(&out, 12, 16)[0] > 20);
    }

    /// One pin is a uniform blur; two equal pins give the same output, at any pin position.
    #[test]
    fn field_blur_single_pin_is_uniform_gaussian() {
        let src = from_fn(48, 48, checker);
        let a = run(
            "pcFieldBlur",
            json!({"blur": 8, "blur2": 8, "center": [-0.9, -0.9], "center2": [0.9, 0.9]}),
            &src,
        );
        let b = run(
            "pcFieldBlur",
            json!({"blur": 8, "blur2": 8, "center": [0.5, -0.2], "center2": [-0.3, 0.7]}),
            &src,
        );
        assert_eq!(a.data, b.data);
        let v = px(&a, 24, 24)[0];
        assert!(v.abs_diff(127) < 40, "{v}");
        // Blur 0 on both is the identity.
        let none = run("pcFieldBlur", json!({"blur": 0, "blur2": 0}), &src);
        assert_eq!(none.data, src.data);
    }

    /// With two pins the blur grows from the sharp pin to the blurred one.
    #[test]
    fn field_blur_two_pins_gradient() {
        let src = from_fn(64, 32, checker);
        let out = run(
            "pcFieldBlur",
            json!({"blur": 30, "center": [-1.0, 0.0], "blur2": 0, "center2": [1.0, 0.0]}),
            &src,
        );
        let left = diff(&out, &src, 0, 8, 8, 24);
        let right = diff(&out, &src, 56, 8, 64, 24);
        assert!(left > right + 40.0, "{left} {right}");
    }

    /// The centre is the sharpest point of a Spin Blur; the rim is blurred more.
    #[test]
    fn spin_blur_centre_is_sharpest() {
        let src = from_fn(64, 64, |x, y| {
            let v = if (x / 4 + y / 4).is_multiple_of(2) {
                0
            } else {
                255
            };
            [v, v, v, 255]
        });
        let out = run(
            "pcSpinBlur",
            json!({"blurAngle": 90, "radiusX": 0.45, "radiusY": 0.45}),
            &src,
        );
        let centre = diff(&out, &src, 30, 30, 34, 34);
        let rim = diff(&out, &src, 44, 30, 48, 34);
        assert!(centre < rim, "{centre} {rim}");
        // Outside the ellipse nothing changes; a zero arc changes nothing anywhere.
        assert_eq!(px(&out, 0, 0), px(&src, 0, 0));
        let flat = run("pcSpinBlur", json!({"blurAngle": 0}), &src);
        assert_eq!(flat.data, src.data);
    }

    /// Zero speed is the identity (and so is zero length from a full taper at the end).
    #[test]
    fn path_blur_zero_speed_is_identity() {
        let src = from_fn(32, 32, checker);
        let out = run("pcPathBlur", json!({"speed": 0}), &src);
        assert_eq!(out.data, src.data);
        // A horizontal path smooths a vertical-stripe image along x, but not along y.
        let stripes = from_fn(32, 32, |x, _| {
            let v = if x.is_multiple_of(2) { 0 } else { 255 };
            [v, v, v, 255]
        });
        let moved = run("pcPathBlur", json!({"speed": 12}), &stripes);
        assert!(px(&moved, 16, 16)[0].abs_diff(127) < 50);
        let rows = from_fn(32, 32, |_, y| {
            let v = if y.is_multiple_of(2) { 0 } else { 255 };
            [v, v, v, 255]
        });
        let kept = run("pcPathBlur", json!({"speed": 12}), &rows);
        assert_eq!(kept.data, rows.data);
    }

    /// Curve bows the path perpendicular to the chord by `curve`% of its length in pixels, on
    /// non-square bounds too; 0 stays straight.
    #[test]
    fn path_curve_bows_the_path() {
        let sq = (100.0, 100.0);
        let straight = super::path_points((0.2, 0.5), (0.8, 0.5), 0.0, sq);
        assert_eq!(straight.len(), 2);
        let bent = super::path_points((0.2, 0.5), (0.8, 0.5), 100.0, sq);
        let mid = bent[bent.len() / 2];
        assert!(
            (mid[0] - 0.5).abs() < 1e-4 && (mid[1] - 0.5).abs() > 0.1,
            "{mid:?}"
        );
        let other = super::path_points((0.2, 0.5), (0.8, 0.5), -100.0, sq);
        assert!((other[other.len() / 2][1] - 0.5) * (mid[1] - 0.5) < 0.0);
        // 200 x 50 bounds: the chord is 120 px, curve 20 % puts the control point 24 px off,
        // so the curve's midpoint (half of that) is 12 px = 0.24 of the height away.
        let wide = super::path_points((0.2, 0.5), (0.8, 0.5), 20.0, (200.0, 50.0));
        let m = wide[wide.len() / 2];
        assert!(
            (m[0] - 0.5).abs() < 1e-4 && ((m[1] - 0.5).abs() - 0.24).abs() < 1e-4,
            "{m:?}"
        );
        // A diagonal chord on tall bounds: the midpoint's offset is perpendicular to it in px.
        let (bw, bh) = (50.0, 200.0);
        let d = super::path_points((0.1, 0.1), (0.9, 0.5), 30.0, (bw, bh));
        let m = d[d.len() / 2];
        let chord = (0.8 * bw, 0.4 * bh);
        let off = (m[0] * bw - 0.5 * bw, m[1] * bh - 0.3 * bh);
        assert!(
            (off.0 * chord.0 + off.1 * chord.1).abs() < 1e-2,
            "not perpendicular {off:?}"
        );
        let len = off.0.hypot(off.1);
        assert!((len - 0.15 * chord.0.hypot(chord.1)).abs() < 1e-2, "{len}");
        // The bowed path's blur differs from the straight one's.
        let src = from_fn(48, 48, checker);
        let a = run("pcPathBlur", json!({"speed": 20, "curve": 0}), &src);
        let b = run("pcPathBlur", json!({"speed": 20, "curve": 80}), &src);
        assert_ne!(a.data, b.data);
    }

    /// A layer that repeats every 16 px in both axes, with the selection moved by one period
    /// diagonally, gives the same output moved by one period: a centre, pin or path anchored on
    /// the layer origin instead of the selection bounds would not move with the selection. The
    /// shift is in y as well, so both axes are pinned.
    #[test]
    fn centres_and_pins_anchor_on_the_selection_bounds() {
        let src = from_fn(96, 96, |x, y| {
            let (u, v) = (x % 16, y % 16);
            let r = (u * 16 + v * 3) as u8;
            let g = if (u / 4 + v / 4) % 2 == 0 { 0 } else { 255 };
            [r, g, 255 - r, 255]
        });
        let cases = [
            json!({"radius": 6}),
            json!({"blur": 6, "blur2": 0}),
            json!({"blurAngle": 60, "radiusX": 0.5, "radiusY": 0.4, "angle": 30, "center": [0.3, -0.2]}),
            json!({"speed": 12, "taper": 100, "curve": 40, "start": [-0.7, -0.5], "end": [0.6, 0.4]}),
        ];
        let ids = ["pcShapeBlur", "pcFieldBlur", "pcSpinBlur", "pcPathBlur"];
        for (id, v) in ids.into_iter().zip(cases) {
            let at = |l: u32| {
                let (mask, polys) = rect_selection(96, 96, l, l, l + 32, l + 32);
                let env = with_selection(96, 96, &mask, &polys);
                run_env(id, v.clone(), &src, &env)
            };
            let (a, b) = (at(16), at(32));
            for (x, y) in (16..48).flat_map(|y| (16..48).map(move |x| (x, y))) {
                let (p, q) = (px(&a, x, y), px(&b, x + 16, y + 16));
                assert!(
                    (0..4).all(|c| p[c].abs_diff(q[c]) <= 1),
                    "{id} {x},{y}: {p:?} {q:?}"
                );
            }
            assert_ne!(a.data, src.data, "{id} changed nothing");
        }
    }
}
