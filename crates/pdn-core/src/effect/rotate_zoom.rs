//! Rotate/Zoom, the Layers-menu effect: Paint.NET's 3-D sample map.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`ROTATE_ZOOM`: id, action, icon,
//! dialog and control keys, labels and steps only).

use super::distort::{Filter, offset, sample_map};
use super::surface::{Alpha, Edge, EffectSurface, Space};
use super::{
    Category, Control, DialogLayout, EffectDef, EffectEnv, EffectKind, GPU_QUALITY, RenderInfo,
    Values, choice, num, range,
};
use std::f64::consts::PI;

/// paintdotjs's `ROTATE_ZOOM` keys, labels and steps; Paint.NET's quality range 1–8 [Q*], control
/// order and exponential zoom (R19, §2.16); choice order paintdotjs's (ruling R45).
pub fn definition() -> EffectDef {
    EffectDef {
        id: "rotateZoomEffect",
        icon: "menu_layers_rotate_zoom_icon.png",
        category: Category::Layers,
        controls: vec![
            range("angle", "Angle", -180.0, 180.0, 0.0).step(0.1),
            range("rollDirection", "Roll direction", -180.0, 180.0, 0.0).step(0.1),
            range("rollAmount", "Roll amount", 0.0, 90.0, 0.0).step(0.1),
            range("panX", "Pan X", -10.0, 10.0, 0.0).step(0.01),
            range("panY", "Pan Y", -10.0, 10.0, 0.0).step(0.01),
            range("zoom", "Zoom", 0.06, 16.0, 1.0)
                .step(0.01)
                .exponential(),
            range("quality", "Quality", 1.0, 8.0, GPU_QUALITY),
            Control::Select {
                key: "tiling",
                label: "Tiling",
                default: "transparent",
                choices: &[
                    ("transparent", "None"),
                    ("wrap", "Repeat"),
                    ("mirror", "Mirror"),
                ],
            },
            Control::Select {
                key: "sampling",
                label: "Sampling",
                default: "linear",
                choices: &[("linear", "Bilinear"), ("point", "Nearest Neighbor")],
            },
        ],
        kind: EffectKind::Gpu(
            RenderInfo::new(Space::Linear, Alpha::Premultiplied),
            rotate_zoom,
        ),
        dialog: Some(DialogLayout::RotateZoom),
        dialog_title: None,
        indirect: None,
    }
}

/// Appendix B B-E18 calibrates the 3-D roll and the quality quirk.
fn rotate_zoom(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    rotate_zoom_with(src, v)
}

/// §2.16: pre = D, post = −D − A, t = asin(M/90); T(x, y) rolls by `pre`, tilts (sx/cos t, sz =
/// that·sin(−t), then sx/cos t again) and turns by `post`; per sample m = p − τ − (W, H)/2,
/// pos = dsdx·mₓ + dsdy·m_y, weight [pos_z ≥ −r], src = pos_xy·r/(z(r + pos_z)) + c, with
/// r = ½√(W² + H²) (the half diagonal).
pub fn rotate_zoom_with(src: &EffectSurface, v: &Values) -> EffectSurface {
    let deg = |k| num(v, k).clamp(-180.0, 180.0) * PI / 180.0;
    let (a, d) = (deg("angle"), deg("rollDirection"));
    let t = (num(v, "rollAmount").clamp(0.0, 90.0) / 90.0).asin();
    let pan = offset(v, ["panX", "panY"], 10.0);
    let z = num(v, "zoom").clamp(0.06, 16.0);
    // quirk(pdn): effects-2.md §2.16 step 1 — the roll amount M is never tested.
    let q = if a == 0.0 && d == 0.0 && pan == [0.0; 2] && z == 1.0 {
        1
    } else {
        num(v, "quality").clamp(1.0, 8.0) as u32
    };
    let (pre, post) = (d, -d - a);
    let map = |x: f64, y: f64| {
        let sx = (x * pre.cos() + y * pre.sin()) / t.cos();
        let sy = -x * pre.sin() + y * pre.cos();
        let sz = sx * (-t).sin();
        let sx = sx / t.cos();
        [
            sx * post.cos() + sy * post.sin(),
            -sx * post.sin() + sy * post.cos(),
            sz,
        ]
        .map(|c| c as f32)
    };
    let (dsdx, dsdy) = (map(1.0, 0.0), map(0.0, 1.0));
    let (w, h) = (src.w as f64, src.h as f64);
    let half_diag = (0.5 * (w * w + h * h).sqrt()) as f32;
    let c = [(w / 2.0) as f32, (h / 2.0) as f32];
    let shift = [(pan[0] * w / 2.0) as f32, (pan[1] * h / 2.0) as f32];
    let z = z as f32;
    let edge = match choice(v, "tiling") {
        "wrap" => Edge::Wrap,
        "mirror" => Edge::Mirror,
        _ => Edge::Transparent,
    };
    let filter = match choice(v, "sampling") {
        "point" => Filter::Point,
        _ => Filter::Bilinear,
    };
    sample_map(src, q, edge, filter, |p| {
        let m = [p[0] - shift[0] - c[0], p[1] - shift[1] - c[1]];
        let pos: [f32; 3] = std::array::from_fn(|i| dsdx[i] * m[0] + dsdy[i] * m[1]);
        let weight = if pos[2] >= -half_diag { 1.0 } else { 0.0 };
        // r/(z(r + pos_z)) first: exactly 1/z at pos_z = 0 for z = 1 or a power of two.
        let k = half_diag / (z * (half_diag + pos[2]));
        ([pos[0] * k + c[0], pos[1] * k + c[1]], weight)
    })
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run};
    use serde_json::json;

    fn img(w: u32, h: u32) -> crate::canvas::ImageData {
        from_fn(w, h, |x, y| {
            [(x * 6) as u8, (y * 9) as u8, ((x * y) % 255) as u8, 255]
        })
    }

    /// effects-2.md §5 vector 11.
    #[test]
    fn rotate_zoom_vectors() {
        let src = img(40, 30);
        assert_eq!(run("rotateZoomEffect", json!({}), &src).data, src.data);
        let turned = run(
            "rotateZoomEffect",
            json!({"angle": 180, "quality": 1}),
            &src,
        );
        assert_eq!(px(&turned, 0, 0), px(&src, 39, 29));
        assert_eq!(px(&turned, 13, 7), px(&src, 26, 22));
        let nn = run(
            "rotateZoomEffect",
            json!({"angle": 180, "quality": 2, "sampling": "point"}),
            &src,
        );
        // ruling R43: §2.16 step 4's transparent edge weighs the q = 2 samples that land past the
        // outer pixel centres (offsets up to 0.3 px), so only the interior is exact; the border
        // keeps the texel's colour at soft alpha (Python port: 206 at the corners, 230 at x = 0).
        // (0, 15) is a quantizer tie: α = 0.9·255 = 229.5 exactly, so GPU rounding could give 229
        // (doc §5 flags Julia's 182.48 the same way); the CPU round-half-up pin is 230.
        for y in 1..29 {
            for x in 1..39 {
                assert_eq!(px(&nn, x, y), px(&turned, x, y), "({x},{y})");
            }
        }
        assert_eq!(px(&nn, 0, 0), [234, 5, 111, 206]);
        assert_eq!(px(&nn, 39, 29), [0, 0, 0, 206]);
        assert_eq!(px(&nn, 0, 15), [234, 126, 36, 230]);
        let pan = run(
            "rotateZoomEffect",
            json!({"panX": 0.5, "quality": 1, "sampling": "point"}),
            &src,
        );
        assert_eq!(px(&pan, 9, 4)[3], 0);
        assert_eq!(px(&pan, 10, 4), px(&src, 0, 4));
        // z = 2, q = 1 on a 4-wide column ramp: column 0 samples u = 0.75 between texels 0 and 1 (linear light).
        let ramp = from_fn(4, 4, |x, _| {
            let v = [0u8, 255, 0, 0][x as usize];
            [v, v, v, 255]
        });
        let z = run("rotateZoomEffect", json!({"zoom": 2, "quality": 1}), &ramp);
        assert_eq!(px(&z, 0, 0)[0], crate::pixel::gamma::linear_to_srgb8(0.75));
    }

    #[test]
    fn rotate_zoom_is_a_layers_effect() {
        let d = crate::effect::rotate_zoom();
        assert_eq!(
            (d.category, d.action_id().as_str()),
            (crate::effect::Category::Layers, "menu.layers.rotateZoom")
        );
        assert!(crate::effect::find("rotateZoomEffect").is_some());
    }

    // ---- Vectors from an independent f64 Python port of §2.16 (F3 edge map, F2 taps). ----

    /// Roll: angle 10, direction 20, amount 30 (tilt t = asin(1/3), sx divided by cos t twice).
    /// A pure tilt keeps q = 1 (`quirk(pdn)`, effects-2.md §2.16 step 1: M is never tested), so
    /// q 2 equals q 1; direction 90 at q 3 supersamples.
    #[test]
    fn roll_and_the_quality_quirk() {
        let src = img(40, 30);
        let roll = run(
            "rotateZoomEffect",
            json!({"angle": 10, "rollDirection": 20, "rollAmount": 30, "quality": 1}),
            &src,
        );
        for ((x, y), want) in [
            ((5, 5), [47, 37, 32, 255]),
            ((20, 15), [120, 136, 48, 255]),
            ((35, 25), [0, 0, 0, 0]),
            ((2, 27), [11, 196, 40, 255]),
        ] {
            assert_eq!(px(&roll, x, y), want, "({x},{y})");
        }
        let tilt = |q: u32| {
            run(
                "rotateZoomEffect",
                json!({"rollAmount": 30, "quality": q}),
                &src,
            )
        };
        assert_eq!(tilt(2).data, tilt(1).data);
        assert_eq!(px(&tilt(1), 3, 15), [27, 134, 67, 255]);
        let turned = run(
            "rotateZoomEffect",
            json!({"rollAmount": 30, "rollDirection": 90, "quality": 3}),
            &src,
        );
        assert_eq!(px(&turned, 3, 15), [17, 136, 44, 255]);
        assert_eq!(px(&turned, 20, 2), [120, 23, 52, 255]);
    }

    /// Tiling at zoom 0.5, angle 30: None is transparent outside, Repeat wraps, Mirror reflects.
    /// Roll 80 with Repeat: past pos_z < −r, r the half diagonal (x ≥ 33 on row 15), the weight
    /// is 0, where wrapping alone would still find colour.
    #[test]
    fn tiling_and_the_horizon() {
        let src = img(40, 30);
        let at = |tiling: &str| {
            let out = run(
                "rotateZoomEffect",
                json!({"zoom": 0.5, "angle": 30, "quality": 1, "tiling": tiling}),
                &src,
            );
            [(1, 2), (38, 27), (20, 15)].map(|(x, y)| px(&out, x, y))
        };
        assert_eq!(
            at("transparent"),
            [[0, 0, 0, 0], [0, 0, 0, 0], [119, 143, 61, 255]]
        );
        assert_eq!(
            at("wrap"),
            [[53, 39, 36, 255], [230, 222, 194, 255], [119, 143, 61, 255]]
        );
        assert_eq!(
            at("mirror"),
            [[0, 222, 0, 255], [234, 39, 171, 255], [119, 143, 61, 255]]
        );
        let horizon = run(
            "rotateZoomEffect",
            json!({"rollAmount": 80, "quality": 1, "tiling": "wrap"}),
            &src,
        );
        assert_eq!(
            [20, 30, 32, 33, 39].map(|x| px(&horizon, x, 15)),
            [
                [132, 135, 75, 255],
                [59, 155, 169, 255],
                [86, 12, 19, 255],
                [0, 0, 0, 0],
                [0, 0, 0, 0]
            ]
        );
    }

    /// Pan (−0.25, 0.4) at zoom 1.5, angle −30, q 2.
    #[test]
    fn pan_and_zoom() {
        let out = run(
            "rotateZoomEffect",
            json!({"panX": -0.25, "panY": 0.4, "zoom": 1.5, "angle": -30}),
            &img(40, 30),
        );
        assert_eq!(
            [(4, 4), (30, 20), (39, 0)].map(|(x, y)| px(&out, x, y)),
            [[48, 76, 67, 255], [170, 81, 209, 255], [0, 0, 0, 0]]
        );
    }
}
