//! Rotate/Zoom, the Layers-menu effect: Paint.NET's 3-D sample map.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`ROTATE_ZOOM`: id, action, icon,
//! dialog and control keys, labels and steps only).

use super::distort::{Filter, sample_map};
use super::surface::{Alpha, Edge, EffectSurface, Space};
use super::{
    Category, Control, DialogLayout, EffectDef, EffectEnv, EffectKind, GPU_QUALITY, RenderInfo,
    Values, choice, num, range,
};

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
            RenderInfo::new(Space::Srgb, Alpha::Premultiplied),
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
    // Genuine (PdnRotateZoomSampleMap) builds the 3-D basis entirely in f32 with MathF, so mirror its
    // op sequence through crt (ucrtbase-exact sinf/cosf/asinf) — f64-then-cast shifts the last bit (and
    // diverges hugely at rollAmount=90, where tilt=π/2 and cos t→0).
    let pi = std::f32::consts::PI;
    let clampf = |k, lo, hi| num(v, k).clamp(lo, hi) as f32;
    let angle = clampf("angle", -180.0, 180.0);
    let roll_dir = clampf("rollDirection", -180.0, 180.0);
    let roll_amt = clampf("rollAmount", 0.0, 90.0);
    let (pan_x, pan_y) = (clampf("panX", -10.0, 10.0), clampf("panY", -10.0, 10.0));
    let z = clampf("zoom", 0.06, 16.0);
    // quirk(pdn): effects-2.md §2.16 step 1 — the roll amount M is never tested.
    let q = if angle == 0.0 && roll_dir == 0.0 && pan_x == 0.0 && pan_y == 0.0 && z == 1.0 {
        1
    } else {
        num(v, "quality").clamp(1.0, 8.0) as u32
    };
    // pre = D, post = −D − A, tilt = asin(M/90), all f32; NaN pre folds to a no-roll identity.
    let (mut pre, mut num6) = (roll_dir * pi / 180.0, roll_amt);
    if pre.is_nan() {
        pre = 0.0;
        num6 = 0.0;
    }
    let post = -pre - angle * pi / 180.0;
    let tilt = crt::asinf(num6 / 90.0);
    // genuine Transform(x, y): roll by `pre`, tilt (÷cos t, sz = ·sin(−t), ÷cos t again), turn by `post`.
    let transform = |x: f32, y: f32| -> [f32; 3] {
        let (c1, s1) = (crt::cosf(pre), crt::sinf(pre));
        let (c2, s2) = (crt::cosf(-tilt), crt::sinf(-tilt));
        let (c3, s3) = (crt::cosf(post), crt::sinf(post));
        let mut sx = (x * c1 + y * s1) / c2;
        let sy = -x * s1 + y * c1;
        let sz = sx * s2;
        sx /= c2;
        [sx * c3 + sy * s3, -sx * s3 + sy * c3, sz]
    };
    // startPos = T(0,0); dsd* are the basis deltas (the subtraction normalises ±0 as genuine does).
    let start = transform(0.0, 0.0);
    let (t10, t01) = (transform(1.0, 0.0), transform(0.0, 1.0));
    let dsdx: [f32; 3] = std::array::from_fn(|i| t10[i] - start[i]);
    let dsdy: [f32; 3] = std::array::from_fn(|i| t01[i] - start[i]);
    let (w, h) = (src.w as f32, src.h as f32);
    let half_diag = 0.5 * (w * w + h * h).sqrt(); // ½√(W²+H²); f32::sqrt == MathF.Sqrt
    let c = [w / 2.0, h / 2.0];
    let shift = [pan_x * w / 2.0, pan_y * h / 2.0];
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
        let pos: [f32; 3] = std::array::from_fn(|i| start[i] + dsdx[i] * m[0] + dsdy[i] * m[1]);
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
        // The interior point (q=2) and bilinear (q=1) samples agree to ≤1: genuine builds the 3-D
        // basis with MathF, and `sinf(−π_f32) ≈ 8.7e-8` leaves the 180° map a few e-6 px off pixel-
        // exact, so a coord that crosses a Q8 boundary blends ≤1/256 of a neighbour (an f32 fact, not
        // an f64 one — the old f64 port's near-zero `sin(−π)` made these identical).
        for y in 1..29 {
            for x in 1..39 {
                let (a, b) = (px(&nn, x, y), px(&turned, x, y));
                for ch in 0..4 {
                    assert!(
                        (a[ch] as i16 - b[ch] as i16).abs() <= 1,
                        "({x},{y}) ch{ch}: {a:?} vs {b:?}"
                    );
                }
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
        // z = 2, q = 1 on a 4-wide column ramp: column 0 samples u = 0.75 between texels 0 and 1 in
        // sRGB via WARP's Q8 fixed-point (distorts sample the 8bpc sRGB source), ⌊255·0.75⌋-ish → 191.
        let ramp = from_fn(4, 4, |x, _| {
            let v = [0u8, 255, 0, 0][x as usize];
            [v, v, v, 255]
        });
        let z = run("rotateZoomEffect", json!({"zoom": 2, "quality": 1}), &ramp);
        assert_eq!(px(&z, 0, 0)[0], 191);
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
            ((5, 5), [46, 36, 31, 255]),
            ((20, 15), [119, 136, 47, 255]),
            ((35, 25), [0, 0, 0, 0]),
            ((2, 27), [10, 195, 39, 255]),
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
        assert_eq!(px(&tilt(1), 3, 15), [26, 134, 66, 255]);
        let turned = run(
            "rotateZoomEffect",
            json!({"rollAmount": 30, "rollDirection": 90, "quality": 3}),
            &src,
        );
        assert_eq!(px(&turned, 3, 15), [17, 135, 43, 255]);
        assert_eq!(px(&turned, 20, 2), [119, 22, 50, 255]);
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
            [[0, 0, 0, 0], [0, 0, 0, 0], [119, 142, 60, 255]]
        );
        assert_eq!(
            at("wrap"),
            [[10, 39, 7, 255], [224, 221, 188, 255], [119, 142, 60, 255]]
        );
        assert_eq!(
            at("mirror"),
            [[0, 221, 0, 255], [234, 39, 169, 255], [119, 142, 60, 255]]
        );
        let horizon = run(
            "rotateZoomEffect",
            json!({"rollAmount": 80, "quality": 1, "tiling": "wrap"}),
            &src,
        );
        assert_eq!(
            [20, 30, 32, 33, 39].map(|x| px(&horizon, x, 15)),
            [
                [131, 135, 75, 255],
                [58, 154, 168, 255],
                [85, 11, 17, 255],
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
            [[47, 76, 67, 255], [169, 81, 170, 255], [0, 0, 0, 0]]
        );
    }
}
