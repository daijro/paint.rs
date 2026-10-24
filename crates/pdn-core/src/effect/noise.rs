//! The 2 Noise effects on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` entries addNoiseEffect
//! and reduceNoiseEffect: ids, icons and control keys only).

use super::blur::sliding_histogram;
use super::rng::{next_float, pixel_seed};
use super::sample::circle_rows;
use super::surface::{Alpha, Edge, EffectSurface, Space, luma};
use super::{
    Category, Control, EffectDef, EffectEnv, EffectKind, GpuFn, RenderInfo, Values, num, range,
};
use std::sync::OnceLock;

const SRGB: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Straight);
const LUT_LEN: usize = 16384;

fn noise(id: &'static str, icon: &'static str, controls: Vec<Control>, render: GpuFn) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("noise"),
        controls,
        kind: EffectKind::Gpu(SRGB, render),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Noise submenu (§2.0 rows): paintdotjs ids, icons and labels; Paint.NET ranges (R19).
pub fn effects() -> Vec<EffectDef> {
    vec![
        noise(
            "addNoiseEffect",
            "add_noise_effect.png",
            vec![
                range("intensity", "Intensity", 0.0, 100.0, 64.0),
                range("saturation", "Color saturation", 0.0, 400.0, 100.0),
                range("coverage", "Coverage", 0.0, 100.0, 100.0),
                range("seed", "Seed", 0.0, 255.0, 0.0),
            ],
            add_noise,
        ),
        noise(
            "reduceNoiseEffect",
            "reduce_noise_effect.png",
            vec![
                range("radius", "Radius", 0.0, 50.0, 10.0),
                range("strength", "Strength", 0.0, 1.0, 0.4).step(0.01),
            ],
            reduce_noise,
        ),
    ]
}

/// λ·exp(−x²/2) at x = 16(i − 8192)/16384, in f64.
fn lut_term(lambda: f64, i: usize) -> f64 {
    let x = 16.0 * (i as f64 - 8192.0) / 16384.0;
    lambda * (-x * x / 2.0).exp()
}

/// §2.6 step 3.1: bisect λ ∈ [5, 10] until r − l ≤ 1e−7 so that Σ λ·exp(−x²/2) = 16384; the
/// result is the last midpoint evaluated. (The doc's 1e6 early abort never fires: Σ < 26000.)
pub fn noise_lambda() -> f64 {
    static LAMBDA: OnceLock<f64> = OnceLock::new();
    *LAMBDA.get_or_init(|| {
        let (mut l, mut r, mut m) = (5.0f64, 10.0f64, f64::NAN);
        while r - l > 1e-7 {
            m = (l + r) / 2.0;
            let s: f64 = (0..LUT_LEN).map(|i| lut_term(m, i)).sum();
            if s > LUT_LEN as f64 {
                r = m;
            } else if s < LUT_LEN as f64 {
                l = m;
            } else {
                break;
            }
        }
        m
    })
}

/// §2.6 step 3.2: when trunc of the running f64 sum rises from k₀ to k₁ at index j,
/// LUT[k₀..k₁] = (j − 8192)·65536/16384. Every term is positive, so k₁ ≥ k₀.
pub fn noise_lut() -> &'static [i32; LUT_LEN] {
    static LUT: OnceLock<[i32; LUT_LEN]> = OnceLock::new();
    LUT.get_or_init(|| {
        let lambda = noise_lambda();
        let mut t = [0; LUT_LEN];
        let (mut s, mut k0) = (0.0f64, 0usize);
        for j in 0..LUT_LEN {
            s += lut_term(lambda, j);
            let k1 = (s as usize).min(LUT_LEN);
            t[k0..k1].fill((j as i32 - 8192) * 65536 / 16384);
            k0 = k1;
        }
        t
    })
}

/// §2.6 Add Noise, sRGB straight, byte-exact for a fixed instance seed: per pixel the F8 stream
/// (seed = instance ⊕ Seed) draws the coverage test, then the R, G, B LUT indices.
fn add_noise(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    // ruling R43: §2.6 step 1 literally in f64; Intensity 58 gives 840 (100·0.58 = 57.99999999999999).
    let i = num(v, "intensity").clamp(0.0, 100.0) / 100.0;
    let dev = ((100.0 * i) * (100.0 * i) / 4.0) as i64;
    let sat = (4096.0 * (num(v, "saturation").clamp(0.0, 400.0) / 100.0)) as i32;
    let cov = (num(v, "coverage").clamp(0.0, 100.0) / 100.0) as f32;
    let seed = env.instance_seed ^ num(v, "seed").clamp(0.0, 255.0) as u32;
    let lut = noise_lut();
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let mut p = src.at(x, y);
        let mut st = pixel_seed(seed, x, y);
        if next_float(&mut st) > cov {
            return p;
        }
        let mut draw = || lut[(next_float(&mut st) * LUT_LEN as f32) as usize];
        let c1 = [draw(), draw(), draw()];
        let i = (4899 * c1[0] + 9618 * c1[1] + 1867 * c1[2]) >> 14;
        for (o, c) in p.iter_mut().zip(c1) {
            let c2 = i + (((c - i) * sat) >> 12);
            let n = (c2 as i64 * dev + 32768) >> 16;
            // No clamp here: the framework's 8-bit write clamps (§2.6 step 4.5).
            *o = (*o * 255.0 + n as f32) / 255.0;
        }
        p
    })
}

/// §2.6 Reduce Noise, sRGB straight, F2 Mirror taps over the F6 circle (A taps): per channel
/// ν = #{s < v}/A and out = v + (ν − v)·t with t = −0.2·strength·(1 − 0.75·Y(v)); α kept; radius
/// 0 passes through. The input is 8-bit (F1), so the ranks come from Task 39's sliding
/// histogram, O(r + 256) per pixel instead of O(r²).
fn reduce_noise(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let radius = num(v, "radius").clamp(0.0, 50.0) as u32;
    if radius == 0 {
        return src.clone();
    }
    let rows = circle_rows(radius);
    let taps = rows.iter().map(|r| r.2).sum::<u32>() as f32;
    let f = (-0.2 * num(v, "strength").clamp(0.0, 1.0)) as f32;
    sliding_histogram(src, &rows, Edge::Mirror, |x, y, hist| {
        let mut p = src.at(x, y);
        let t = f * (1.0 - 0.75 * luma(p[0], p[1], p[2]));
        for (o, h) in p[..3].iter_mut().zip(hist) {
            let below: u32 = h[..(*o * 255.0).round() as usize].iter().sum();
            *o += (below as f32 / taps - *o) * t;
        }
        p
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::test_util::{from_fn, px, run, run_env, solid};
    use serde_json::json;

    /// effects-1.md §2.6 / §5 Task 40 Add Noise: exact for instance seed 0.
    #[test]
    fn add_noise_lut_and_pixel() {
        assert_eq!(noise_lambda(), 6.383076533675194);
        let lut = noise_lut();
        assert_eq!((lut[0], lut[8192], lut[16383]), (-15736, 0, 23200));
        let out = run(
            "addNoiseEffect",
            json!({}),
            &solid(2, 2, [128, 128, 128, 255]),
        );
        assert_eq!(px(&out, 0, 0), [255, 147, 42, 255]);
        assert_eq!(
            run(
                "addNoiseEffect",
                json!({"coverage": 0}),
                &solid(2, 2, [128, 128, 128, 255])
            )
            .data,
            solid(2, 2, [128, 128, 128, 255]).data
        );
    }

    #[test]
    fn reduce_noise_brightens_flat_grey() {
        assert_eq!(
            px(
                &run(
                    "reduceNoiseEffect",
                    json!({}),
                    &solid(25, 25, [128, 128, 128, 255])
                ),
                12,
                12
            ),
            [134, 134, 134, 255]
        );
    }

    // ---- Vectors from an independent port of effects-1.md §2.6, F2, F6 and F8 (numpy f32 for the
    // shader arithmetic). It reproduces λ, the three LUT entries and pixel (0, 0) above exactly. ----

    /// §2.6 step 3.2 over the whole table: more entries and Σ (k + 1)·LUT[k]. LUT[16383] is
    /// filled only where the running sum reaches 16384 (x ≈ 5.66), far past LUT[16382] (x ≈ 3.84).
    #[test]
    fn add_noise_lut_is_pinned() {
        let lut = noise_lut();
        assert_eq!(
            (lut[100], lut[4096], lut[12288], lut[16000], lut[16382]),
            (-10252, -2760, 2764, 8144, 15736)
        );
        let weighted: i64 = lut
            .iter()
            .enumerate()
            .map(|(k, &v)| (k as i64 + 1) * v as i64)
            .sum();
        assert_eq!(weighted, 310_403_501_056);
    }

    fn sample() -> ImageData {
        let p = [
            [20, 100, 230, 255],
            [128, 128, 128, 77],
            [0, 0, 0, 255],
            [255, 255, 255, 255],
        ];
        from_fn(2, 2, |x, y| p[(y * 2 + x) as usize])
    }

    /// §2.6 step 4 per channel at seed 0. At (0, 0) i = 2568, so saturation 0 adds n = 40 to every
    /// channel; saturation 200 gives g's c2·dev term −2.5, floored to −3 by the arithmetic shift.
    /// Intensity 37 gives dev = trunc(1369/4) = 342. α is untouched (77).
    #[test]
    fn add_noise_channels_saturation_and_intensity() {
        let src = sample();
        let out = run("addNoiseEffect", json!({}), &src);
        assert_eq!(
            out.data,
            [
                [151, 119, 144, 255],
                [170, 124, 61, 77],
                [101, 12, 0, 255],
                [196, 255, 255, 255]
            ]
            .concat()
        );
        let at00 = |v| px(&run("addNoiseEffect", v, &src), 0, 0);
        assert_eq!(at00(json!({"saturation": 0})), [60, 140, 255, 255]);
        assert_eq!(at00(json!({"saturation": 200})), [241, 97, 17, 255]);
        let out = run(
            "addNoiseEffect",
            json!({"saturation": 33, "intensity": 37}),
            &src,
        );
        assert_eq!(
            out.data,
            [
                [43, 111, 229, 255],
                [133, 128, 121, 77],
                [18, 8, 0, 255],
                [255, 255, 255, 255]
            ]
            .concat()
        );
    }

    /// §2.6 step 4.1: the first draw at (0, 0) is 0.11249119 (F8), so coverage 11 % leaves the
    /// pixel and 12 % noises it. Step 2: the shader seed is instance ⊕ Seed (5 ⊕ 3 = 6 ⊕ 0).
    #[test]
    fn add_noise_coverage_and_seeds() {
        let src = sample();
        let at00 = |v| px(&run("addNoiseEffect", v, &src), 0, 0);
        assert_eq!(at00(json!({"coverage": 11})), [20, 100, 230, 255]);
        assert_eq!(at00(json!({"coverage": 12})), [151, 119, 144, 255]);
        let mut env = EffectEnv::whole(2, 2);
        env.instance_seed = 5;
        let out = run_env("addNoiseEffect", json!({"seed": 3}), &src, &env);
        assert_eq!(
            out.data,
            [
                [82, 0, 253, 255],
                [195, 213, 92, 77],
                [52, 0, 0, 255],
                [255, 255, 255, 255]
            ]
            .concat()
        );
        env.instance_seed = 6;
        assert_eq!(
            run_env("addNoiseEffect", json!({}), &src, &env).data,
            out.data
        );
        assert_eq!(
            run("addNoiseEffect", json!({"seed": 7}), &src).data,
            [
                [110, 39, 255, 255],
                [0, 189, 151, 77],
                [0, 97, 0, 255],
                [255, 252, 207, 255]
            ]
            .concat()
        );
    }

    /// §2.6 step 4.3 rounding: i = S ≫ 14 floors (at saturation 0, pixel (12, 0) has S < 0 and
    /// c2 = i, so truncation would give 126), and sat = trunc(4096·0.33) = 1351 (1352 would give
    /// 198 at (1, 1) of seed 1, intensity 100).
    #[test]
    fn add_noise_floors_luma_and_truncates_saturation() {
        let row = solid(13, 1, [128, 128, 128, 255]);
        let out = run(
            "addNoiseEffect",
            json!({"intensity": 100, "saturation": 0}),
            &row,
        );
        assert_eq!(px(&out, 12, 0), [125, 125, 125, 255]);
        let out = run(
            "addNoiseEffect",
            json!({"intensity": 100, "saturation": 33, "seed": 1}),
            &solid(2, 2, [128, 128, 128, 255]),
        );
        assert_eq!(
            out.data,
            [
                [255, 227, 225, 255],
                [84, 203, 176, 255],
                [62, 48, 82, 255],
                [224, 255, 199, 255]
            ]
            .concat()
        );
    }

    /// Ruling R43: dev = trunc((100·I)²/4) with I = Intensity/100 in f64, so Intensity 58 gives 840,
    /// not 58²/4 = 841; at (2, 0) that is g 68 (841 would give 67).
    #[test]
    fn add_noise_dev_follows_the_f64_formula() {
        let out = run(
            "addNoiseEffect",
            json!({"intensity": 58}),
            &solid(3, 1, [128, 128, 128, 255]),
        );
        assert_eq!(px(&out, 2, 0), [53, 68, 62, 255]);
    }

    /// Out-of-range values clamp to the control ranges (an unclamped saturation overflows i32).
    #[test]
    fn add_noise_clamps_controls() {
        let src = sample();
        let q = |v| run("addNoiseEffect", v, &src).data;
        assert_eq!(q(json!({"saturation": 1e9})), q(json!({"saturation": 400})));
        assert_eq!(q(json!({"intensity": 1000})), q(json!({"intensity": 100})));
        assert_eq!(q(json!({"seed": 300})), q(json!({"seed": 255})));
    }

    fn varied() -> ImageData {
        from_fn(6, 5, |x, y| {
            [
                ((x * 40 + y * 7) % 256) as u8,
                ((y * 50 + x * x * 9) % 256) as u8,
                (((x ^ y) * 37) % 256) as u8,
                if (x + y) % 3 == 0 { 128 } else { 255 },
            ]
        })
    }

    /// §2.6 Reduce Noise at radius 2 (21 taps), strength 1: per-channel strict ranks, t from the
    /// centre's luma, α kept, F2 Mirror taps (Clamp would change 16 of the 30 pixels). Every
    /// unrounded value is at least 0.022 from a rounding boundary.
    #[test]
    fn reduce_noise_ranks_per_channel_with_mirror_edges() {
        let out = run(
            "reduceNoiseEffect",
            json!({"radius": 2, "strength": 1}),
            &varied(),
        );
        let want: [[u8; 4]; 30] = [
            [0, 0, 0, 128],
            [29, 2, 28, 255],
            [77, 34, 68, 255],
            [123, 82, 111, 128],
            [167, 145, 152, 255],
            [205, 227, 187, 255],
            [0, 42, 28, 255],
            [34, 50, 0, 255],
            [82, 83, 102, 128],
            [128, 131, 71, 255],
            [171, 195, 185, 255],
            [211, 22, 159, 128],
            [9, 100, 68, 255],
            [44, 109, 102, 128],
            [90, 136, 0, 255],
            [135, 181, 32, 255],
            [178, 244, 222, 128],
            [218, 72, 0, 255],
            [17, 157, 111, 128],
            [53, 164, 73, 255],
            [99, 189, 31, 255],
            [143, 233, 0, 128],
            [190, 38, 0, 255],
            [225, 119, 224, 255],
            [23, 208, 150, 255],
            [61, 211, 183, 255],
            [106, 236, 221, 128],
            [150, 29, 0, 255],
            [195, 92, 0, 255],
            [230, 168, 26, 128],
        ];
        assert_eq!(out.data, want.concat());
    }

    /// Radius 0 passes through; strength 0 is the identity; out-of-range values clamp.
    #[test]
    fn reduce_noise_radius_zero_and_clamping() {
        let src = varied();
        let q = |v| run("reduceNoiseEffect", v, &src).data;
        assert_eq!(q(json!({"radius": 0})), src.data);
        assert_eq!(q(json!({"strength": -1})), src.data);
        assert_eq!(
            q(json!({"radius": 2, "strength": 7})),
            q(json!({"radius": 2, "strength": 1}))
        );
        // A 128-px gradient row: radius 99 would reach farther than 50 (88 pixels differ).
        let row = from_fn(128, 1, |x, _| {
            [(2 * x) as u8, (255 - 2 * x) as u8, x as u8, 255]
        });
        let q = |r| run("reduceNoiseEffect", json!({ "radius": r }), &row).data;
        assert_eq!(q(99), q(50));
    }

    /// R19 controls (effects-1.md §2.0 Noise rows), paintdotjs labels and icons; sRGB straight.
    #[test]
    fn r19_controls() {
        let ranges = |id: &str| {
            let d = crate::effect::find(id).unwrap();
            assert_eq!(d.category, Category::Effect("noise"));
            assert!(matches!(
                d.kind,
                EffectKind::Gpu(
                    RenderInfo {
                        space: Space::Srgb,
                        input: Alpha::Straight,
                        output: Alpha::Straight
                    },
                    _
                )
            ));
            let r: Vec<_> = d
                .controls
                .iter()
                .map(|c| match *c {
                    Control::Range {
                        key,
                        label,
                        min,
                        max,
                        default,
                        step,
                        ..
                    } => (key, label, min, max, default, step),
                    _ => panic!("{id}"),
                })
                .collect();
            (d.icon, r)
        };
        assert_eq!(
            ranges("addNoiseEffect"),
            (
                "add_noise_effect.png",
                vec![
                    ("intensity", "Intensity", 0.0, 100.0, 64.0, 1.0),
                    ("saturation", "Color saturation", 0.0, 400.0, 100.0, 1.0),
                    ("coverage", "Coverage", 0.0, 100.0, 100.0, 1.0),
                    ("seed", "Seed", 0.0, 255.0, 0.0, 1.0),
                ]
            )
        );
        assert_eq!(
            ranges("reduceNoiseEffect"),
            (
                "reduce_noise_effect.png",
                vec![
                    ("radius", "Radius", 0.0, 50.0, 10.0, 1.0),
                    ("strength", "Strength", 0.0, 1.0, 0.4, 0.01),
                ]
            )
        );
        let ids: Vec<_> = effects().iter().map(|d| d.id).collect();
        assert_eq!(ids, ["addNoiseEffect", "reduceNoiseEffect"]);
    }
}
