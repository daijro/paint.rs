//! The 9 Distort effects on Paint.NET's math, and the sample-map renderer.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` entries bulgeEffect,
//! crystalizeEffect, dentsEffect, frostedGlassEffect, morphologyEffect, pixelateEffect,
//! polarInversion, tileEffect and twistEffect: ids, icons and control keys only).
//! Includes Crystalize's P0/P1.

use super::blur::{box_fold, madd};
use super::perlin::noise_at;
use super::rng::{next_float, pcg, pixel_seed};
use super::sample::{edge_map, rgss_offsets};
use super::surface::{Alpha, Edge, EffectSurface, Space};
use super::{
    Category, Control, EffectDef, EffectEnv, EffectKind, GPU_QUALITY, GpuFn, RenderInfo, Values,
    choice, flag, num, range,
};
use crate::pixel::IntRect;
use std::f64::consts::{PI, TAU};

const LINEAR_PM: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);
const SRGB_PM: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
const EDGES: &[(&str, &str)] = &[
    ("clamp", "Clamp"),
    ("wrap", "Wrap"),
    ("reflect", "Reflect"),
    ("transparent", "Transparent"),
];

fn distort(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    render: GpuFn,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("distort"),
        controls,
        kind: EffectKind::Gpu(info, render),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Distort submenu (§2.0 rows): paintdotjs ids, icons and labels; Paint.NET ranges (R19).
pub fn effects() -> Vec<EffectDef> {
    let quality = |max: f64| range("quality", "Quality", 1.0, max, GPU_QUALITY);
    let pair = |key, label, lim: f64| range(key, label, -lim, lim, 0.0).step(0.01);
    let edge = |default| Control::Select {
        key: "edge",
        label: "Edge behavior",
        default,
        choices: EDGES,
    };
    let seed = || range("seed", "Seed", 0.0, 255.0, 0.0);
    vec![
        distort(
            "bulgeEffect",
            "bulge_effect.png",
            vec![
                // ruling R44: paintdotjs's percentage (−300..100 [45], step 1) rescaled to Paint.NET's
                // −3..1 keeps its step as 0.01.
                range("amount", "Amount", -3.0, 1.0, 0.45).step(0.01),
                pair("centerX", "Center X", 1.0),
                pair("centerY", "Center Y", 1.0),
                edge("clamp"),
                quality(8.0),
            ],
            LINEAR_PM,
            bulge,
        ),
        distort(
            "crystalizeEffect",
            "crystalize_effect_icon.png",
            vec![
                range("cell", "Cell size", 2.0, 250.0, 8.0),
                quality(5.0),
                seed(),
            ],
            LINEAR_PM,
            crystalize,
        ),
        distort(
            "dentsEffect",
            "dents_effect_icon.png",
            vec![
                range("scale", "Scale", 0.0, 200.0, 25.0),
                range("refraction", "Refraction", 0.0, 200.0, 50.0),
                range("detail", "Roughness", 0.0, 100.0, 10.0),
                range("turbulence", "Tension", 0.0, 100.0, 10.0),
                range("angle", "Angle", -180.0, 180.0, 0.0),
                quality(8.0),
                seed(),
            ],
            LINEAR_PM,
            dents,
        ),
        distort(
            "frostedGlassEffect",
            "frosted_glass_effect.png",
            vec![
                range("maxRadius", "Maximum scatter radius", 0.0, 500.0, 3.0),
                range("minRadius", "Minimum scatter radius", 0.0, 500.0, 0.0),
                range("diffusion", "Diffusion", 0.01, 3.0, 1.0).step(0.01),
                range("smoothness", "Smoothness", 1.0, 8.0, 2.0),
                seed(),
            ],
            LINEAR_PM,
            frosted_glass,
        ),
        distort(
            "morphologyEffect",
            "morphology_effect_icon.png",
            vec![
                range("width", "Width", 1.0, 100.0, 5.0),
                range("height", "Height", 1.0, 100.0, 5.0),
                Control::Checkbox {
                    key: "linked",
                    label: "Linked",
                    default: true,
                },
                Control::Select {
                    key: "mode",
                    label: "Operation",
                    default: "dilate",
                    // ruling R45: paintdotjs's choice order.
                    choices: &[("dilate", "Dilate"), ("erode", "Erode")],
                },
            ],
            SRGB_PM,
            morphology,
        ),
        distort(
            "pixelateEffect",
            "pixelate_effect.png",
            vec![
                range("cell", "Cell size", 1.0, 256.0, 2.0),
                pair("anchorX", "Anchor X", 1.0),
                pair("anchorY", "Anchor Y", 1.0),
                Control::Select {
                    key: "scaleDown",
                    label: "Scale down",
                    default: "multisampleBilinear",
                    choices: &[
                        ("anisotropic", "Anisotropic"),
                        ("bicubicHq", "Bicubic HQ"),
                        ("multisampleBilinear", "Multisample Bilinear"),
                        ("bicubic", "Bicubic"),
                        ("bilinear", "Bilinear"),
                        ("nearestNeighbor", "Nearest Neighbor"),
                    ],
                },
                Control::Select {
                    key: "scaleUp",
                    label: "Scale up",
                    default: "nearestNeighbor",
                    choices: &[
                        ("bicubic", "Bicubic"),
                        ("bilinear", "Bilinear"),
                        ("nearestNeighbor", "Nearest Neighbor"),
                    ],
                },
            ],
            LINEAR_PM,
            pixelate,
        ),
        distort(
            "polarInversion",
            "polar_inversion_effect.png",
            vec![
                range("amount", "Amount", -8.0, 8.0, 1.0),
                pair("offsetX", "Offset X", 2.0),
                pair("offsetY", "Offset Y", 2.0),
                edge("reflect"),
                quality(8.0),
            ],
            LINEAR_PM,
            polar_inversion,
        ),
        distort(
            "tileEffect",
            "tile_effect.png",
            vec![
                range("rotation", "Rotation", -180.0, 180.0, 30.0),
                range("tile", "Tile size", 1.0, 1600.0, 40.0),
                range("curvature", "Curvature", -200.0, 200.0, 8.0),
                edge("reflect"),
                quality(8.0),
            ],
            LINEAR_PM,
            tile_reflection,
        ),
        distort(
            "twistEffect",
            "twist_effect.png",
            vec![
                range("amount", "Amount", -200.0, 200.0, 30.0),
                range("size", "Size", 0.01, 2.0, 1.0).step(0.01),
                pair("centerX", "Center X", 2.0),
                pair("centerY", "Center Y", 2.0),
                quality(8.0),
            ],
            LINEAR_PM,
            twist,
        ),
    ]
}

/// A tap filter for `sample_map`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Bilinear,
    Point,
}

/// F4 with the F5 offsets of `quality` (0 gives the single offset (0, 0)): each pixel's samples
/// are `map(p + oₖ)` at its centre p = (x + ½, y + ½).
pub fn sample_map(
    src: &EffectSurface,
    quality: u32,
    edge: Edge,
    filter: Filter,
    map: impl Fn([f32; 2]) -> ([f32; 2], f32) + Sync,
) -> EffectSurface {
    let offsets = rgss_offsets(quality);
    render_maps(src, offsets.len(), edge, filter, |k, p| {
        map([p[0] + offsets[k][0], p[1] + offsets[k][1]])
    })
}

/// F4 over n samples per pixel, `map(k, p)` giving sample k's position and weight at pixel centre
/// p: F3 maps the position, the tap (F2 padding in the same mode) is weighted by w·w′, groups of
/// up to 4 samples are summed, the group sums added, and the total multiplied by f32(1/n).
fn render_maps(
    src: &EffectSurface,
    n: usize,
    edge: Edge,
    filter: Filter,
    map: impl Fn(usize, [f32; 2]) -> ([f32; 2], f32) + Sync,
) -> EffectSurface {
    let size = [src.w as f32, src.h as f32];
    let inv = 1.0 / n as f32;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = [x as f32 + 0.5, y as f32 + 0.5];
        let mut total = [0.0f32; 4];
        for g in (0..n).step_by(4) {
            let mut group = [0.0; 4];
            for k in g..n.min(g + 4) {
                let (s, w) = map(k, p);
                let (s, w2) = edge_map(s, size, edge);
                let c = match filter {
                    Filter::Bilinear => src.bilinear(s[0], s[1], edge),
                    Filter::Point => src.point(s[0], s[1], edge),
                };
                madd(&mut group, w * w2, c);
            }
            madd(&mut total, 1.0, group);
        }
        total.map(|t| t * inv)
    })
}

fn quality(v: &Values, max: f64) -> u32 {
    num(v, "quality").clamp(1.0, max) as u32
}

/// The edge choice: reflect is F3's Mirror.
fn edge(v: &Values) -> Edge {
    match choice(v, "edge") {
        "wrap" => Edge::Wrap,
        "reflect" => Edge::Mirror,
        "transparent" => Edge::Transparent,
        _ => Edge::Clamp,
    }
}

/// (S.l + S.w(1 + o.x)/2, S.t + S.h(1 + o.y)/2) in f64, then f32: centre(S) for o = 0, and
/// centre(S) + o·(S.w/2, S.h/2) in general.
pub(super) fn anchor(s: IntRect, o: [f64; 2]) -> [f32; 2] {
    [
        (s.x as f64 + s.w as f64 * (1.0 + o[0]) / 2.0) as f32,
        (s.y as f64 + s.h as f64 * (1.0 + o[1]) / 2.0) as f32,
    ]
}

pub(super) fn offset(v: &Values, keys: [&str; 2], lim: f64) -> [f64; 2] {
    keys.map(|k| num(v, k).clamp(-lim, lim))
}

/// 1 − λ(1 − r/(D/2))².
pub fn bulge_scale(r_over_half_d: f32, strength: f32) -> f32 {
    let t = 1.0 - r_over_half_d;
    1.0 - strength * (t * t)
}

/// §2.5 Bulge: u = p − c, x = |u|·(1/(D/2)); inside the circle s = c + u·bulge_scale(x, λ), else p.
fn bulge(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let b = env.bounds;
    let c = anchor(b, offset(v, ["centerX", "centerY"], 1.0));
    let rho = (1.0 / (b.w.min(b.h) as f64 / 2.0)) as f32;
    let lambda = num(v, "amount").clamp(-3.0, 1.0) as f32;
    sample_map(src, quality(v, 8.0), edge(v), Filter::Bilinear, |p| {
        let u = [p[0] - c[0], p[1] - c[1]];
        let x = (u[0] * u[0] + u[1] * u[1]).sqrt() * rho;
        if 1.0 - x > 0.0 {
            let k = bulge_scale(x, lambda);
            ([c[0] + u[0] * k, c[1] + u[1] * k], 1.0)
        } else {
            (p, 1.0)
        }
    })
}

/// Appendix A's tables, decoded from the doc's hex at compile time.
const fn hex(s: &[u8; 512]) -> [u8; 256] {
    const fn nibble(c: u8) -> u8 {
        if c <= b'9' { c - b'0' } else { c - b'a' + 10 }
    }
    let mut out = [0; 256];
    let mut i = 0;
    while i < 256 {
        out[i] = nibble(s[2 * i]) << 4 | nibble(s[2 * i + 1]);
        i += 1;
    }
    out
}

const P0: [u8; 256] = hex(b"5bd55e28cb437cc83e33df35d06f1f1c896c25724a2eb1204775bb82bf512d78b2dcf158d864520fe6d45799f38d48c7aef2c5fb67b0212a223ce5608eff09ef05708bf783c946231611927ad91532b927eecacc8c3708faadbd349b7ff088e2c165938501534ce3a5a0de5026ab560a0c2c3b9d38446ed3669449bc9fb3a91e91a8af768facbedaa4ea1be954b8d6006a1a8755c0f4aa80fcb71d71078445f8f6b5c47703e0795c685aed40137e7ba1b63a4b6d184f639ef9197dd18a39ce129c2bec98f54dfe90a224dd59fd5f62733081c3cd2fd2cf973d1086db4ea7e10e96c6a3e769e49a3617b4c2eb41026b5d9504610642e8baa614290d74d70b313f");
const P1: [u8; 256] = hex(b"5730f8f09ab080be006ee5374ae2ea9fca2324fbb72d1b2abb64ba1745fa50e775398f7df3aed7610b4247790f8a6d5c0985ac218db22e2913cf383cf26b7e0520fdc4145973ab1a51c6ed7ce4a5193af6660dc2543267c70a70722c88a778a6cc465d12cb5f06a1d998dacdc1b54476b601489660bdbcf49dff08e904899ca4554eb1a94f0774a05b7ae8c35397035a2bf97f8669dd4916916883d24d22a256f53482840ca326bf9baf3debd6fcaa0ef7c0d0111d28418702ad31e694b45840de99d32733926295dfd1ecce9e633f8cd46fd893528b3be0effe6a5e712f1ee34bf118363e8edb776c25356543b3dcb981b81c15e1c8d54c7bee1f1090c9a8c5");

/// §2.5 Crystalize steps 2–4: t = p·(1/z); the strictly nearest feature point among cell
/// (trunc t) and its 8 neighbours, each tested in the doc's order only while its bound is below the
/// best squared distance; s = ½ + f·z (before the Clamp edge).
pub fn crystalize_position(p: [f32; 2], cell: f32, seed32: u32) -> [f32; 2] {
    let inv = 1.0 / cell;
    let t = [p[0] * inv, p[1] * inv];
    let (bi, bj) = (t[0].trunc() as i32, t[1].trunc() as i32);
    let feature = |i: i32, j: i32| {
        let h = ((seed32 ^ i as u32) & 255) as usize;
        let at = |table: &[u8; 256], k: u8| table[((k as u32 ^ j as u32) & 255) as usize] as f32;
        [
            i as f32 + at(&P1, P0[h]) / 255.0,
            j as f32 + at(&P0, P1[h]) / 255.0,
        ]
    };
    let d2 = |f: [f32; 2]| {
        let (dx, dy) = (t[0] - f[0], t[1] - f[1]);
        dx * dx + dy * dy
    };
    let mut nearest = feature(bi, bj);
    let mut best = d2(nearest);
    let (fx, fy) = (t[0] - bi as f32, t[1] - bj as f32);
    let (mx, my) = ((1.0 - fx) * (1.0 - fx), (1.0 - fy) * (1.0 - fy));
    let (x2, y2) = (fx * fx, fy * fy);
    for ((di, dj), bound) in [
        ((-1, 0), x2),
        ((0, -1), y2),
        ((1, 0), mx),
        ((0, 1), my),
        ((-1, -1), x2 + y2),
        ((1, 1), mx + my),
        ((-1, 1), x2 + my),
        ((1, -1), mx + y2),
    ] {
        if bound < best {
            let f = feature(bi + di, bj + dj);
            let d = d2(f);
            if d < best {
                (best, nearest) = (d, f);
            }
        }
    }
    [0.5 + nearest[0] * cell, 0.5 + nearest[1] * cell]
}

/// §2.5 Crystalize, Clamp edges; seed32 = Seed·1566083941 + (instance & 255), wrapping.
fn crystalize(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let cell = num(v, "cell").clamp(2.0, 250.0) as f32;
    let seed = (num(v, "seed").clamp(0.0, 255.0) as u32)
        .wrapping_mul(1_566_083_941)
        .wrapping_add(env.instance_seed & 255);
    sample_map(src, quality(v, 5.0), Edge::Clamp, Filter::Bilinear, |p| {
        (crystalize_position(p, cell, seed), 1.0)
    })
}

/// §2.5 Dents steps 1–8, Mirror edges; the constants in f64, the per-sample math in f32.
fn dents(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let scale = num(v, "scale").clamp(0.0, 200.0);
    if scale == 0.0 {
        return src.clone();
    }
    let b = env.bounds;
    let c = anchor(b, [0.0; 2]);
    let sr = 400.0 / (b.w.min(b.h) as f64 / 2.0) / scale;
    let refr = (num(v, "refraction").clamp(0.0, 200.0) / 100.0 / sr) as f32;
    let theta = (TAU * num(v, "turbulence").clamp(0.0, 100.0) / 10.0) as f32;
    let detail = num(v, "detail").clamp(0.0, 100.0);
    let rough = (detail / 100.0) as f32;
    let mut det = 1.0 + detail / 10.0;
    let max_det = (sr.ln() / 0.5f64.ln()).floor();
    if det > max_det && max_det >= 1.0 {
        det = max_det;
    }
    let octaves = det.ceil() as u32;
    let (sin, cos) = num(v, "angle").clamp(-180.0, 180.0).to_radians().sin_cos();
    let (sin, cos) = (sin as f32, cos as f32);
    let seed = num(v, "seed").clamp(0.0, 255.0) as u32;
    let seed8 = seed.wrapping_add(env.instance_seed) as u8;
    let (sr, det) = (sr as f32, det as f32);
    sample_map(src, quality(v, 8.0), Edge::Mirror, Filter::Bilinear, |p| {
        let mut x = [(p[0] - c[0]) * sr, (p[1] - c[1]) * sr];
        let (mut total, mut f, mut amp, mut part) = (0.0f32, 1.0f32, 1.0f32, det);
        for _ in 0..octaves {
            x = [x[0] * cos - x[1] * sin, x[0] * sin + x[1] * cos];
            total += noise_at(x[0] * f, x[1] * f, seed8) * amp * part.min(1.0);
            amp *= rough;
            if amp < 0.001 {
                break;
            }
            f *= 2.0;
            part -= 1.0;
            x = [x[0] + 499.0, x[1] + 506.0];
        }
        let a = theta * total;
        ([p[0] + refr * -a.sin(), p[1] + refr * a.cos()], 1.0)
    })
}

/// §2.5 Frosted Glass step 1: n = [1, 2, 3, 5, 8, 13, 21, 34][q − 1].
pub fn frosted_sample_count(smoothness: u32) -> u32 {
    [1, 2, 3, 5, 8, 13, 21, 34][smoothness.clamp(1, 8) as usize - 1]
}

/// §2.5 Frosted Glass, Mirror edges, no RGSS: map k's F8 stream (seed Seed ⊕ (instance ⊕ R_k))
/// draws φ = next·2π, then ρ = |a + next·(b − a)|^(1/e); s = p + ρ(cos φ, sin φ); the n maps are
/// averaged as F4's samples.
fn frosted_glass(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let max = num(v, "maxRadius").clamp(0.0, 500.0);
    let min = num(v, "minRadius").clamp(0.0, max);
    let e = num(v, "diffusion").clamp(0.01, 3.0);
    let (a, b, inv_e) = (min.powf(e) as f32, max.powf(e) as f32, (1.0 / e) as f32);
    let n = frosted_sample_count(num(v, "smoothness").clamp(1.0, 8.0) as u32);
    let seed = num(v, "seed").clamp(0.0, 255.0) as u32;
    // ponytail: Paint.NET's R_k is Random.Shared (effects-1.md §2.5); a fixed distinct value per
    // map keeps the n maps distinct and the output deterministic.
    let seeds: Vec<u32> = (0..n)
        .map(|k| seed ^ (env.instance_seed ^ pcg(k)))
        .collect();
    let tau = TAU as f32;
    render_maps(src, n as usize, Edge::Mirror, Filter::Bilinear, |k, p| {
        let mut st = pixel_seed(seeds[k], p[0] as u32, p[1] as u32);
        let phi = next_float(&mut st) * tau;
        let rho = (a + next_float(&mut st) * (b - a)).abs().powf(inv_e);
        ([p[0] + rho * phi.cos(), p[1] + rho * phi.sin()], 1.0)
    })
}

/// B-E4 approximation of D2D Morphology on premultiplied sRGB.
fn morphology(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D Morphology; mode semantics as documented in Paint.NET's enum ("Erode:
    // maximum, Dilate: minimum"), centring and edges unverified (effects-1.md §2.5, §6 item 2; B-E4)
    let w = num(v, "width").clamp(1.0, 100.0) as i32;
    let h = if flag(v, "linked") {
        w
    } else {
        num(v, "height").clamp(1.0, 100.0) as i32
    };
    let f = if choice(v, "mode") == "erode" {
        f32::max
    } else {
        f32::min
    };
    box_fold(
        src,
        [-(w / 2), -(h / 2)],
        [w - 1 - w / 2, h - 1 - h / 2],
        Edge::Clamp,
        f,
    )
}

fn linear_weights(f: f32) -> [f32; 4] {
    [0.0, 1.0 - f, f, 0.0]
}

/// Catmull-Rom (a = −½) weights of texels −1..=2 for fraction f.
pub(super) fn catmull_rom(f: f32) -> [f32; 4] {
    [
        ((-0.5 * f + 1.0) * f - 0.5) * f,
        (1.5 * f - 2.5) * f * f + 1.0,
        ((-1.5 * f + 2.0) * f + 0.5) * f,
        (0.5 * f - 0.5) * f * f,
    ]
}

/// A separable 4-tap filter at scene position s over texels `tex(i, j)` (centres at i + ½):
/// `k(f)` weighs texels ⌊u⌋ − 1..=⌊u⌋ + 2 of u = s − ½; rows outer, columns inner.
pub(super) fn filter4(
    s: [f32; 2],
    k: fn(f32) -> [f32; 4],
    tex: impl Fn(i32, i32) -> [f32; 4],
) -> [f32; 4] {
    let (u, v) = (s[0] - 0.5, s[1] - 0.5);
    let (i0, j0) = (u.floor(), v.floor());
    let (wx, wy) = (k(u - i0), k(v - j0));
    let mut acc = [0.0; 4];
    for (j, wy) in (j0 as i32 - 1..).zip(wy) {
        for (i, wx) in (i0 as i32 - 1..).zip(wx) {
            madd(&mut acc, wx * wy, tex(i, j));
        }
    }
    acc
}

/// The z×z box around s, each texel weighted by its overlap.
fn box_mean(src: &EffectSurface, s: [f32; 2], z: f32) -> [f32; 4] {
    let axis = |c: f32| {
        let (lo, hi) = (c - z / 2.0, c + z / 2.0);
        (lo.floor() as i32..hi.ceil() as i32)
            .map(move |k| (k, hi.min(k as f32 + 1.0) - lo.max(k as f32)))
    };
    let mut acc = [0.0; 4];
    for (j, wy) in axis(s[1]) {
        for (i, wx) in axis(s[0]) {
            madd(&mut acc, wx * wy, src.fetch(i, j, Edge::Clamp));
        }
    }
    acc.map(|a| a / (z * z))
}

/// §2.5 Pixelate, Clamp edges: the scale-down image D at q-texel centres (σ = c + (q − c)·z), then
/// the scale-up at q = c + (p − c)/z. D is rendered once over the q-texels the scale-up reads.
fn pixelate(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let z = num(v, "cell").clamp(1.0, 256.0) as f32;
    if z <= 1.0 || src.px.is_empty() {
        return src.clone();
    }
    let c = anchor(env.bounds, offset(v, ["anchorX", "anchorY"], 1.0));
    let down = choice(v, "scaleDown");
    let scale_down = |q: [f32; 2]| {
        let s = [c[0] + (q[0] - c[0]) * z, c[1] + (q[1] - c[1]) * z];
        match down {
            "bilinear" => src.bilinear(s[0], s[1], Edge::Clamp),
            "nearestNeighbor" => src.point(s[0], s[1], Edge::Clamp),
            // approximation: D2D 2DAffineTransform (B-E5)
            "bicubic" | "bicubicHq" => filter4(s, catmull_rom, |i, j| src.fetch(i, j, Edge::Clamp)),
            // approximation: D2D 2DAffineTransform (B-E5)
            "anisotropic" => box_mean(src, s, z),
            _ => {
                let (a, b) = (0.125 * z, 0.375 * z);
                let mut acc = [0.0; 4];
                for o in [[a, b], [-b, a], [-a, -b], [b, -a]] {
                    madd(
                        &mut acc,
                        0.25,
                        src.bilinear(s[0] + o[0], s[1] + o[1], Edge::Clamp),
                    );
                }
                acc
            }
        }
    };
    let q = |x: f32, c: f32| c + (x - c) / z;
    let lo = [q(0.5, c[0]), q(0.5, c[1])].map(|v| v.floor() as i32 - 2);
    let hi =
        [q(src.w as f32 - 0.5, c[0]), q(src.h as f32 - 0.5, c[1])].map(|v| v.floor() as i32 + 2);
    let grid = EffectSurface::from_fn(
        (hi[0] - lo[0] + 1) as u32,
        (hi[1] - lo[1] + 1) as u32,
        |i, j| {
            scale_down([
                (lo[0] + i as i32) as f32 + 0.5,
                (lo[1] + j as i32) as f32 + 0.5,
            ])
        },
    );
    let tex = |i: i32, j: i32| grid.fetch(i - lo[0], j - lo[1], Edge::Clamp);
    let up = choice(v, "scaleUp");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = [q(x as f32 + 0.5, c[0]), q(y as f32 + 0.5, c[1])];
        match up {
            "bilinear" => filter4(p, linear_weights, tex),
            // approximation: D2D 2DAffineTransform (B-E5)
            "bicubic" => filter4(p, catmull_rom, tex),
            _ => tex(p[0].floor() as i32, p[1].floor() as i32),
        }
    })
}

/// §2.5 Polar Inversion: d = p − c; s = c + d·lerp(1, R²·rcp(d·d), λ).
fn polar_inversion(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let b = env.bounds;
    let m = src.w.min(src.h) as f64;
    let lambda = (num(v, "amount").clamp(-8.0, 8.0) * b.w.min(b.h) as f64 / m) as f32;
    let c = anchor(b, offset(v, ["offsetX", "offsetY"], 2.0));
    let r2 = ((m / 2.0) * (m / 2.0)) as f32;
    sample_map(src, quality(v, 8.0), edge(v), Filter::Bilinear, |p| {
        let d = [p[0] - c[0], p[1] - c[1]];
        let k = 1.0 + lambda * (r2 * (1.0 / (d[0] * d[0] + d[1] * d[1])) - 1.0);
        let s = [c[0] + d[0] * k, c[1] + d[1] * k];
        // d = 0 (c on a sample position) gives ∞·0; the GPU's NaN tap is undefined, so keep p.
        let finite = s[0].is_finite() && s[1].is_finite();
        (if finite { s } else { p }, 1.0)
    })
}

/// §2.5 Tile Reflection: st = R(φ)(p − o), st′ = st + ι·tan(κ·st), s = o + R(φ)ᵀst′.
fn tile_reflection(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let o = [src.w as f32 / 2.0, src.h as f32 / 2.0];
    let phi = -num(v, "rotation").clamp(-180.0, 180.0).to_radians() as f32;
    let (sin, cos) = (phi.sin(), phi.cos());
    let kappa = (PI / num(v, "tile").clamp(1.0, 1600.0)) as f32;
    let k = num(v, "curvature").clamp(-200.0, 200.0);
    let iota = (k * k / 10.0 * k.signum()) as f32;
    sample_map(src, quality(v, 8.0), edge(v), Filter::Bilinear, |p| {
        let d = [p[0] - o[0], p[1] - o[1]];
        let st = [d[0] * cos + d[1] * sin, -d[0] * sin + d[1] * cos];
        let st = st.map(|t| t + iota * (t * kappa).tan());
        let back = [st[0] * cos - st[1] * sin, st[0] * sin + st[1] * cos];
        ([o[0] + back[0], o[1] + back[1]], 1.0)
    })
}

/// §2.5 Twist, Clamp edges: t = sat(1 − ρ·(1/λ)·(1/R)), φ = atan2(u) + t³·τ·0.01,
/// s = c + ρ(cos φ, sin φ).
fn twist(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let b = env.bounds;
    let m = src.w.min(src.h) as f64;
    let a = -num(v, "amount").clamp(-200.0, 200.0) / 100.0;
    let lambda = num(v, "size").clamp(0.01, 2.0) * b.w.min(b.h) as f64 / m;
    let c = anchor(b, offset(v, ["centerX", "centerY"], 2.0));
    let tau = ((100.0 * a) * (100.0 * a) * a.signum()) as f32;
    // ruling R43: the doc's literal ρ·(1/λ)·(1/R), not the brief's ρ/(λR).
    let (inv_l, inv_r) = ((1.0 / lambda) as f32, (1.0 / (m / 2.0)) as f32);
    sample_map(src, quality(v, 8.0), Edge::Clamp, Filter::Bilinear, |p| {
        let u = [p[0] - c[0], p[1] - c[1]];
        let rho = (u[0] * u[0] + u[1] * u[1]).sqrt();
        let t = (1.0 - rho * inv_l * inv_r).clamp(0.0, 1.0);
        let phi = u[1].atan2(u[0]) + t * t * t * tau * 0.01;
        ([c[0] + rho * phi.cos(), c[1] + rho * phi.sin()], 1.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::test_util::{from_fn, px, run, run_env, solid};
    use crate::effect::{EffectEnv, find};
    use crate::pixel::IntRect;
    use serde_json::json;

    fn fixture() -> crate::canvas::ImageData {
        from_fn(16, 12, |x, y| {
            [(x * 15) as u8, (y * 20) as u8, ((x + y) * 7) as u8, 255]
        })
    }

    #[test]
    fn crystalize_feature_points() {
        let s =
            |p: [f32; 2], seed: u32| crystalize_position(p, 8.0, seed.wrapping_mul(1_566_083_941));
        let close =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        assert!(close(s([0.5, 0.5], 0), [-0.53529, -0.31569]));
        assert!(close(s([20.5, 13.5], 0), [20.76667, 16.90784]));
        assert!(close(s([20.5, 13.5], 1), [17.25294, 16.21765]));
    }

    #[test]
    fn bulge_and_counts() {
        assert!((bulge_scale(0.5, 0.45) - 0.8875).abs() < 1e-6);
        assert_eq!(
            (1..=8).map(frosted_sample_count).collect::<Vec<_>>(),
            [1, 2, 3, 5, 8, 13, 21, 34]
        );
    }

    /// Neutral parameters at quality 1 sample every pixel at its own centre.
    #[test]
    fn neutral_maps_are_identity() {
        let img = fixture();
        for (id, v) in [
            ("polarInversion", json!({"amount": 0, "quality": 1})),
            ("tileEffect", json!({"curvature": 0, "quality": 1})),
            ("twistEffect", json!({"amount": 0, "quality": 1})),
            ("bulgeEffect", json!({"amount": 0, "quality": 1})),
            ("dentsEffect", json!({"scale": 0})),
            ("frostedGlassEffect", json!({"maxRadius": 0})),
            ("pixelateEffect", json!({"cell": 1})),
            ("morphologyEffect", json!({"width": 1, "height": 1})),
        ] {
            let out = run(id, v, &img);
            assert!(
                out.data
                    .iter()
                    .zip(&img.data)
                    .all(|(a, b)| a.abs_diff(*b) <= 1),
                "{id}"
            );
        }
    }

    /// Pixelate z = 2, MS4 down, NN up: 2×2 cells anchored at the selection centre.
    #[test]
    fn pixelate_cells() {
        let img = from_fn(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let out = run("pixelateEffect", json!({}), &img);
        assert_eq!(px(&out, 0, 0), px(&out, 1, 1));
        assert_eq!(px(&out, 2, 0), px(&out, 3, 1));
        assert_eq!(px(&out, 0, 2), px(&out, 1, 3));
    }

    #[test]
    fn distort_menu() {
        let ids: Vec<&str> = crate::effect::effects()
            .iter()
            .filter(|e| e.category == crate::effect::Category::Effect("distort"))
            .map(|e| e.id)
            .collect();
        assert_eq!(ids.len(), 9);
        assert!(
            ids.contains(&"frostedGlassEffect")
                && ids.contains(&"morphologyEffect")
                && !ids.contains(&"turbulenceEffect")
        );
    }

    // ---- Vectors from an independent f32 port of effects-1.md §2.1 F1–F5, F8 and §2.5 (numpy f32,
    // libm's cosf/sinf/tanf/atan2f/powf; session scratchpad gen.py, not committed). Every unrounded
    // 8-bit value is ≥ 0.001 from a rounding boundary. ----

    /// 9×7: a different ramp per channel, α falling right and down, and a transparent pixel at
    /// (4, 3) whose hidden RGB a straight-alpha tap would leak.
    fn scene() -> ImageData {
        from_fn(9, 7, |x, y| {
            let a = if (x, y) == (4, 3) {
                0
            } else {
                255 - 10 * x - 7 * y
            };
            [
                (x * 28) as u8,
                (y * 40) as u8,
                (x * y * 13 % 256) as u8,
                a as u8,
            ]
        })
    }

    /// The selection bounds S = (2, 1) 6×5 inside the 9×7 layer: centre(S) = (5, 3.5) ≠ the layer's.
    fn env(instance_seed: u32) -> EffectEnv<'static> {
        EffectEnv {
            bounds: IntRect::new(2, 1, 6, 5),
            instance_seed,
            ..EffectEnv::whole(9, 7)
        }
    }

    /// (Σ bytes, Σ (i + 1)·byte): any changed byte changes the pair.
    fn digest(img: &ImageData) -> (u64, u64) {
        let b = img.data.iter().map(|&v| v as u64);
        (
            b.clone().sum(),
            b.enumerate().map(|(i, v)| (i as u64 + 1) * v).sum(),
        )
    }

    /// F4 directly: RGSS offsets added before the map, F3 per mode with F2 padding in the same mode,
    /// the map's weight times the Transparent weight, point taps, the 1/n mean, and the groups of 4.
    #[test]
    fn sample_map_renderer() {
        let src = EffectSurface::from_fn(5, 4, |x, y| [x as f32 * 0.25, y as f32 / 3.0, 0.5, 1.0]);
        let at = |s: &EffectSurface| [(0, 0), (4, 0), (2, 3), (4, 3)].map(|(x, y)| s.at(x, y));
        let shift = |w: f32| move |p: [f32; 2]| ([p[0] - 1.7, p[1] + 0.4], w);
        let wrap = sample_map(&src, 2, Edge::Wrap, Filter::Bilinear, shift(1.0));
        assert_eq!(
            at(&wrap),
            [
                [0.824_999_9, 0.133_333_34, 0.5, 1.0],
                [0.575, 0.133_333_34, 0.5, 1.0],
                [0.074_999_99, 0.599_999_96, 0.5, 1.0],
                [0.575, 0.599_999_96, 0.5, 1.0],
            ]
        );
        let transparent = sample_map(&src, 2, Edge::Transparent, Filter::Bilinear, shift(0.5));
        assert_eq!(
            at(&transparent),
            [
                [0.0; 4],
                [0.287_5, 0.066_666_67, 0.25, 0.5],
                [0.022_499_993, 0.299_999_98, 0.149_999_99, 0.299_999_98],
                [0.172_499_98, 0.299_999_98, 0.149_999_99, 0.299_999_98],
            ]
        );
        let point = sample_map(&src, 3, Edge::Mirror, Filter::Point, |p| {
            ([p[0] + 3.2, p[1] - 2.6], 1.0)
        });
        assert_eq!(
            at(&point),
            [
                [0.805_555_6, 0.518_518_57, 0.5, 1.0],
                [0.472_222_24, 0.518_518_57, 0.5, 1.0],
                [0.972_222_2, 0.148_148_15, 0.5, 1.0],
                [0.472_222_24, 0.148_148_15, 0.5, 1.0],
            ]
        );
        // Step 4's grouping (quality 3: 4 + 4 + 1 samples): a 2²⁴ weight on the first sample swallows
        // the 0.5-sized terms of its own group, but the other groups are summed apart and survive;
        // one flat sum loses all eight (932 067.56, …).
        let one = EffectSurface::from_fn(1, 1, |_, _| [0.5, 0.25, 1.0, 0.75]);
        let heavy = sample_map(&one, 3, Edge::Clamp, Filter::Bilinear, |p| {
            (
                p,
                if p[1].fract() < 0.15 {
                    16_777_216.0
                } else {
                    1.0
                },
            )
        });
        assert_eq!(
            heavy.at(0, 0),
            [932_067.8, 466_033.9, 1_864_135.6, 1_398_102.1]
        );
    }

    /// Every sample-map effect and Pixelate's own modes on `scene()` with S ≠ the layer:
    /// (id, values, instance seed, digest, pixel (0, 0), pixel (4, 3)).
    #[test]
    fn distort_vectors() {
        let pix = |down: &str, up: &str| json!({"cell": 3, "anchorX": 0.4, "anchorY": -0.25, "scaleDown": down, "scaleUp": up});
        let bulge = |edge: &str| json!({"amount": -3, "centerX": 0.9, "centerY": 0.9, "edge": edge, "quality": 2});
        let polar = |edge: &str| json!({"amount": 0.45, "offsetX": 0.25, "offsetY": -0.5, "edge": edge, "quality": 2});
        #[rustfmt::skip]
        let cases = [
            ("bulgeEffect", json!({"amount": 0.75, "centerX": 0.3, "centerY": -0.2, "quality": 2}), 0, (32576, 4605776), [4, 7, 0, 253], [124, 118, 164, 79]),
            ("bulgeEffect", bulge("clamp"), 0, (32239, 4553640), [4, 7, 0, 253], [114, 124, 159, 72]),
            ("bulgeEffect", bulge("wrap"), 0, (32744, 4432835), [66, 78, 6, 241], [114, 124, 159, 72]),
            ("bulgeEffect", bulge("reflect"), 0, (32251, 4556349), [4, 7, 0, 253], [114, 124, 159, 72]),
            ("bulgeEffect", bulge("transparent"), 0, (31229, 4385784), [4, 8, 0, 204], [114, 124, 159, 72]),
            ("crystalizeEffect", json!({"cell": 3, "quality": 2, "seed": 5}), 3, (33069, 4558071), [75, 53, 46, 220], [171, 163, 71, 166]),
            // seed8 = (9 + 300) mod 256 = 53; det = 3.5 gives 4 octaves, the last at half weight.
            ("dentsEffect", json!({"scale": 10, "refraction": 80, "detail": 25, "turbulence": 30, "angle": 50, "quality": 2, "seed": 9}), 300, (32406, 4586125), [4, 7, 0, 253], [114, 123, 159, 74]),
            ("frostedGlassEffect", json!({"maxRadius": 4, "minRadius": 1, "diffusion": 1.5, "smoothness": 3, "seed": 7}), 0x1234, (33403, 4515995), [29, 44, 8, 240], [131, 127, 87, 193]),
            ("polarInversion", polar("reflect"), 0, (33215, 4688633), [30, 20, 4, 243], [80, 168, 154, 197]),
            ("polarInversion", polar("wrap"), 0, (34358, 4678414), [30, 20, 4, 243], [80, 168, 154, 197]),
            ("tileEffect", json!({"rotation": 25, "tile": 6, "curvature": 3, "quality": 2}), 0, (33669, 4510489), [37, 90, 56, 202], [114, 124, 157, 102]),
            ("tileEffect", json!({"rotation": -50, "tile": 4, "curvature": -6, "edge": "transparent", "quality": 1}), 0, (8075, 1183442), [149, 0, 0, 75], [0, 0, 0, 0]),
            ("twistEffect", json!({"amount": 60, "size": 0.8, "centerX": 0.2, "centerY": -0.1, "quality": 3}), 0, (32572, 4605726), [4, 8, 0, 253], [149, 131, 158, 165]),
            ("pixelateEffect", pix("multisampleBilinear", "nearestNeighbor"), 0, (33057, 4596888), [27, 62, 16, 239], [103, 177, 123, 178]),
            ("pixelateEffect", pix("bilinear", "nearestNeighbor"), 0, (32865, 4564107), [20, 53, 11, 240], [102, 171, 177, 189]),
            ("pixelateEffect", pix("nearestNeighbor", "nearestNeighbor"), 0, (32058, 4431204), [28, 40, 13, 238], [112, 160, 208, 187]),
            ("pixelateEffect", pix("multisampleBilinear", "bilinear"), 0, (33556, 4639033), [23, 47, 8, 244], [117, 140, 106, 182]),
        ];
        for (id, v, inst, want, p00, p43) in cases {
            let out = run_env(id, v.clone(), &scene(), &env(inst));
            assert_eq!(
                (digest(&out), px(&out, 0, 0), px(&out, 4, 3)),
                (want, p00, p43),
                "{id} {v} {inst}"
            );
        }
    }

    /// §2.5 Crystalize step 3 (z = 8, seed 0): a point whose nearest feature lies in each of the
    /// base cell and its 8 neighbours, so no neighbour is skipped or pruned by a wrong bound.
    #[test]
    fn crystalize_searches_every_neighbour() {
        #[rustfmt::skip]
        let cases = [
            ((0, 0), [16.5, 8.5], [21.707_844, 9.786_275]),
            ((-1, 0), [32.5, 8.5], [26.570_59, 8.625_49]),
            ((0, -1), [8.5, 8.5], [12.860_785, 5.958_823_7]),
            ((1, 0), [71.5, 8.5], [77.896_08, 8.813_725]),
            ((0, 1), [16.5, 12.5], [20.766_666, 16.907_843]),
            ((-1, -1), [67.5, 8.5], [63.496_08, 6.962_745]),
            ((1, 1), [191.5, 13.5], [196.107_85, 17.284_313]),
            ((-1, 1), [120.5, 12.5], [120.343_14, 17.221_57]),
            ((1, -1), [39.5, 8.5], [41.127_453, 6.209_804]),
        ];
        for (cell, p, s) in cases {
            assert_eq!(crystalize_position(p, 8.0, 0), s, "{cell:?}");
        }
    }

    /// §2.5 Dents steps 2–3: sR = 400/(L/2)/NS = 400/20/200 = 0.1 caps det = 6 at
    /// maxDet = ⌊ln 0.1/ln 0.5⌋ = 3 octaves (same independent port; these pixels are ≥ 0.06 from a
    /// rounding boundary).
    #[test]
    fn dents_detail_caps_at_max_det() {
        let img = from_fn(40, 40, |x, y| {
            [(x * 6) as u8, (y * 6) as u8, (x * y % 256) as u8, 255]
        });
        let v =
            json!({"scale": 200, "refraction": 200, "detail": 50, "turbulence": 50, "quality": 1});
        let out = run("dentsEffect", v, &img);
        let want = [
            ((0, 0), [92, 71, 182, 255]),
            ((9, 0), [54, 33, 49, 255]),
            ((15, 0), [205, 34, 196, 255]),
            ((21, 30), [30, 108, 90, 255]),
            ((37, 12), [232, 40, 132, 255]),
        ];
        for ((x, y), p) in want {
            assert_eq!(px(&out, x, y), p, "({x}, {y})");
        }
    }

    /// B-E5 smoke: the Direct2D scale modes keep a flat image flat and pass cell 1 through.
    #[test]
    fn pixelate_d2d_modes_smoke() {
        let flat = solid(7, 5, [90, 140, 200, 180]);
        let img = fixture();
        for down in ["anisotropic", "bicubicHq", "bicubic"] {
            for up in ["bicubic", "bilinear", "nearestNeighbor"] {
                let v = json!({"cell": 3, "anchorX": 0.3, "scaleDown": down, "scaleUp": up});
                let out = run("pixelateEffect", v.clone(), &flat);
                assert!(
                    out.data
                        .iter()
                        .zip(&flat.data)
                        .all(|(a, b)| a.abs_diff(*b) <= 1),
                    "{v}"
                );
                let v = json!({"cell": 1, "scaleDown": down, "scaleUp": up});
                assert_eq!(run("pixelateEffect", v, &img).data, img.data);
            }
        }
        // Anisotropic is the z×z box mean: a 2-px checkerboard at z = 2 averages to grey.
        let checker = from_fn(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let out = run(
            "pixelateEffect",
            json!({"scaleDown": "anisotropic"}),
            &checker,
        );
        assert!(
            out.data.chunks(4).all(|p| p == [188, 188, 188, 255]),
            "{:?}",
            out.data
        );
    }

    /// B-E4 approximation: per premultiplied sRGB channel, Erode = maximum and Dilate = minimum over
    /// the offsets −⌊W/2⌋..W − 1 − ⌊W/2⌋ (and H), Clamp edges; `linked` makes H follow W.
    #[test]
    fn morphology_rectangle_and_modes() {
        let dot = from_fn(9, 7, |x, y| {
            if (x, y) == (4, 3) {
                [200, 100, 50, 255]
            } else {
                [0, 0, 0, 255]
            }
        });
        let lit = |img: &ImageData| {
            (0..7u32)
                .flat_map(|y| (0..9u32).map(move |x| (x, y)))
                .filter(|&(x, y)| px(img, x, y)[0] > 0)
                .collect::<Vec<_>>()
        };
        let rect = |xs: std::ops::RangeInclusive<u32>, ys: std::ops::RangeInclusive<u32>| {
            ys.flat_map(|y| xs.clone().map(move |x| (x, y)))
                .collect::<Vec<_>>()
        };
        let erode = run(
            "morphologyEffect",
            json!({"mode": "erode", "width": 4, "height": 3, "linked": false}),
            &dot,
        );
        assert_eq!(lit(&erode), rect(3..=6, 2..=4));
        assert_eq!(px(&erode, 6, 4), [200, 100, 50, 255]);
        let linked = run(
            "morphologyEffect",
            json!({"mode": "erode", "width": 4, "height": 1}),
            &dot,
        );
        assert_eq!(lit(&linked), rect(3..=6, 2..=5));
        assert!(lit(&run("morphologyEffect", json!({}), &dot)).is_empty());
        let hole = from_fn(9, 7, |x, y| {
            if (x, y) == (4, 3) {
                [0, 0, 0, 255]
            } else {
                [200, 100, 50, 255]
            }
        });
        let dilate = run(
            "morphologyEffect",
            json!({"width": 2, "height": 3, "linked": false}),
            &hole,
        );
        let dark: Vec<_> = rect(0..=8, 0..=6)
            .into_iter()
            .filter(|&(x, y)| px(&dilate, x, y)[0] == 0)
            .collect();
        assert_eq!(dark, rect(4..=5, 2..=4));
        // Clamp edges: a flat layer stays flat under the minimum (Transparent would darken the rim).
        let flat = solid(5, 4, [200, 100, 50, 255]);
        assert_eq!(
            run("morphologyEffect", json!({"width": 5}), &flat).data,
            flat.data
        );
        // Premultiplied sRGB: max((255, 0, 0, α 0.2), (0, 0, 255, α 1)) = (0.2, 0, 1, 1) → (51, 0, 255).
        let pair = ImageData {
            width: 2,
            height: 1,
            data: vec![255, 0, 0, 51, 0, 0, 255, 255],
        };
        let out = run(
            "morphologyEffect",
            json!({"mode": "erode", "width": 2}),
            &pair,
        );
        assert_eq!(px(&out, 1, 0), [51, 0, 255, 255]);
    }

    /// R19 (effects-1.md §2.0 Distort rows): paintdotjs keys, labels and icons; Paint.NET ranges.
    #[test]
    fn r19_controls() {
        use crate::effect::{Control, EffectKind, GPU_QUALITY};
        let def = |id: &str| find(id).unwrap();
        let ranges = |id: &str| {
            def(id)
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Range {
                        key,
                        label,
                        min,
                        max,
                        default,
                        step,
                        ..
                    } => Some((*key, *label, *min, *max, *default, *step)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let keys = |id: &str| def(id).controls.iter().map(|c| c.key()).collect::<Vec<_>>();
        let q = GPU_QUALITY;
        assert_eq!(
            keys("bulgeEffect"),
            ["amount", "centerX", "centerY", "edge", "quality"]
        );
        assert_eq!(
            ranges("bulgeEffect"),
            [
                ("amount", "Amount", -3.0, 1.0, 0.45, 0.01),
                ("centerX", "Center X", -1.0, 1.0, 0.0, 0.01),
                ("centerY", "Center Y", -1.0, 1.0, 0.0, 0.01),
                ("quality", "Quality", 1.0, 8.0, q, 1.0),
            ]
        );
        assert_eq!(
            ranges("crystalizeEffect"),
            [
                ("cell", "Cell size", 2.0, 250.0, 8.0, 1.0),
                ("quality", "Quality", 1.0, 5.0, q, 1.0),
                ("seed", "Seed", 0.0, 255.0, 0.0, 1.0)
            ]
        );
        assert_eq!(
            ranges("dentsEffect"),
            [
                ("scale", "Scale", 0.0, 200.0, 25.0, 1.0),
                ("refraction", "Refraction", 0.0, 200.0, 50.0, 1.0),
                ("detail", "Roughness", 0.0, 100.0, 10.0, 1.0),
                ("turbulence", "Tension", 0.0, 100.0, 10.0, 1.0),
                ("angle", "Angle", -180.0, 180.0, 0.0, 1.0),
                ("quality", "Quality", 1.0, 8.0, q, 1.0),
                ("seed", "Seed", 0.0, 255.0, 0.0, 1.0),
            ]
        );
        assert_eq!(
            ranges("frostedGlassEffect"),
            [
                ("maxRadius", "Maximum scatter radius", 0.0, 500.0, 3.0, 1.0),
                ("minRadius", "Minimum scatter radius", 0.0, 500.0, 0.0, 1.0),
                ("diffusion", "Diffusion", 0.01, 3.0, 1.0, 0.01),
                ("smoothness", "Smoothness", 1.0, 8.0, 2.0, 1.0),
                ("seed", "Seed", 0.0, 255.0, 0.0, 1.0),
            ]
        );
        assert_eq!(
            keys("morphologyEffect"),
            ["width", "height", "linked", "mode"]
        );
        assert_eq!(
            ranges("morphologyEffect"),
            [
                ("width", "Width", 1.0, 100.0, 5.0, 1.0),
                ("height", "Height", 1.0, 100.0, 5.0, 1.0)
            ]
        );
        assert_eq!(
            keys("pixelateEffect"),
            ["cell", "anchorX", "anchorY", "scaleDown", "scaleUp"]
        );
        assert_eq!(
            ranges("pixelateEffect"),
            [
                ("cell", "Cell size", 1.0, 256.0, 2.0, 1.0),
                ("anchorX", "Anchor X", -1.0, 1.0, 0.0, 0.01),
                ("anchorY", "Anchor Y", -1.0, 1.0, 0.0, 0.01)
            ]
        );
        assert_eq!(
            keys("polarInversion"),
            ["amount", "offsetX", "offsetY", "edge", "quality"]
        );
        assert_eq!(
            ranges("polarInversion"),
            [
                ("amount", "Amount", -8.0, 8.0, 1.0, 1.0),
                ("offsetX", "Offset X", -2.0, 2.0, 0.0, 0.01),
                ("offsetY", "Offset Y", -2.0, 2.0, 0.0, 0.01),
                ("quality", "Quality", 1.0, 8.0, q, 1.0),
            ]
        );
        assert_eq!(
            keys("tileEffect"),
            ["rotation", "tile", "curvature", "edge", "quality"]
        );
        assert_eq!(
            ranges("tileEffect"),
            [
                ("rotation", "Rotation", -180.0, 180.0, 30.0, 1.0),
                ("tile", "Tile size", 1.0, 1600.0, 40.0, 1.0),
                ("curvature", "Curvature", -200.0, 200.0, 8.0, 1.0),
                ("quality", "Quality", 1.0, 8.0, q, 1.0),
            ]
        );
        assert_eq!(
            ranges("twistEffect"),
            [
                ("amount", "Amount", -200.0, 200.0, 30.0, 1.0),
                ("size", "Size", 0.01, 2.0, 1.0, 0.01),
                ("centerX", "Center X", -2.0, 2.0, 0.0, 0.01),
                ("centerY", "Center Y", -2.0, 2.0, 0.0, 0.01),
                ("quality", "Quality", 1.0, 8.0, q, 1.0),
            ]
        );
        let select = |id: &str, k: &str| {
            def(id)
                .controls
                .into_iter()
                .find_map(|c| match c {
                    Control::Select {
                        key,
                        label,
                        default,
                        choices,
                    } if key == k => Some((label, default, choices.to_vec())),
                    _ => None,
                })
                .unwrap()
        };
        let edges = vec![
            ("clamp", "Clamp"),
            ("wrap", "Wrap"),
            ("reflect", "Reflect"),
            ("transparent", "Transparent"),
        ];
        assert_eq!(
            select("bulgeEffect", "edge"),
            ("Edge behavior", "clamp", edges.clone())
        );
        assert_eq!(
            select("polarInversion", "edge"),
            ("Edge behavior", "reflect", edges.clone())
        );
        assert_eq!(
            select("tileEffect", "edge"),
            ("Edge behavior", "reflect", edges)
        );
        assert_eq!(
            select("morphologyEffect", "mode"),
            (
                "Operation",
                "dilate",
                vec![("dilate", "Dilate"), ("erode", "Erode")]
            )
        );
        assert_eq!(
            select("pixelateEffect", "scaleDown"),
            (
                "Scale down",
                "multisampleBilinear",
                vec![
                    ("anisotropic", "Anisotropic"),
                    ("bicubicHq", "Bicubic HQ"),
                    ("multisampleBilinear", "Multisample Bilinear"),
                    ("bicubic", "Bicubic"),
                    ("bilinear", "Bilinear"),
                    ("nearestNeighbor", "Nearest Neighbor"),
                ]
            )
        );
        assert_eq!(
            select("pixelateEffect", "scaleUp"),
            (
                "Scale up",
                "nearestNeighbor",
                vec![
                    ("bicubic", "Bicubic"),
                    ("bilinear", "Bilinear"),
                    ("nearestNeighbor", "Nearest Neighbor")
                ]
            )
        );
        assert!(matches!(
            def("morphologyEffect").controls[2],
            Control::Checkbox {
                key: "linked",
                label: "Linked",
                default: true
            }
        ));
        for (id, icon) in [
            ("bulgeEffect", "bulge_effect.png"),
            ("crystalizeEffect", "crystalize_effect_icon.png"),
            ("dentsEffect", "dents_effect_icon.png"),
            ("frostedGlassEffect", "frosted_glass_effect.png"),
            ("morphologyEffect", "morphology_effect_icon.png"),
            ("pixelateEffect", "pixelate_effect.png"),
            ("polarInversion", "polar_inversion_effect.png"),
            ("tileEffect", "tile_effect.png"),
            ("twistEffect", "twist_effect.png"),
        ] {
            let d = def(id);
            let EffectKind::Gpu(info, _) = d.kind else {
                panic!("{id}")
            };
            let space = if id == "morphologyEffect" {
                Space::Srgb
            } else {
                Space::Linear
            };
            assert_eq!(
                (d.icon, info),
                (icon, RenderInfo::new(space, Alpha::Premultiplied)),
                "{id}"
            );
        }
    }

    /// Out-of-range values clamp to the §2.0 ranges instead of panicking or dividing by zero; the
    /// 300-px layer makes the upper cell and width limits visible.
    #[test]
    fn controls_clamp() {
        let img = fixture();
        let wide = from_fn(300, 2, |x, y| {
            [(x % 256) as u8, (x / 2) as u8, (90 * y) as u8, 255]
        });
        #[rustfmt::skip]
        let cases = [
            ("crystalizeEffect", &img, json!({"cell": 0, "quality": 0}), json!({"cell": 2, "quality": 1})),
            ("crystalizeEffect", &wide, json!({"cell": 900, "quality": 9}), json!({"cell": 250, "quality": 5})),
            ("frostedGlassEffect", &img, json!({"smoothness": 0, "diffusion": 0}), json!({"smoothness": 1, "diffusion": 0.01})),
            ("frostedGlassEffect", &img, json!({"smoothness": 99, "maxRadius": 900, "minRadius": 950}), json!({"smoothness": 8, "maxRadius": 500, "minRadius": 500})),
            ("frostedGlassEffect", &img, json!({"maxRadius": 2, "minRadius": 5}), json!({"maxRadius": 2, "minRadius": 2})),
            ("pixelateEffect", &img, json!({"cell": 0}), json!({"cell": 1})),
            ("pixelateEffect", &wide, json!({"cell": 999, "anchorX": 9, "anchorY": -9}), json!({"cell": 256, "anchorX": 1, "anchorY": -1})),
            ("tileEffect", &img, json!({"tile": 0, "quality": 99, "rotation": 500}), json!({"tile": 1, "quality": 8, "rotation": 180})),
            ("twistEffect", &img, json!({"size": 0, "amount": 1e6}), json!({"size": 0.01, "amount": 200})),
            ("twistEffect", &img, json!({"size": 50}), json!({"size": 2})),
            ("morphologyEffect", &img, json!({"width": 0, "height": -3, "linked": false}), json!({"width": 1, "height": 1, "linked": false})),
            ("morphologyEffect", &wide, json!({"width": 500}), json!({"width": 100})),
            ("bulgeEffect", &img, json!({"amount": -1e9, "quality": -1, "centerX": 7}), json!({"amount": -3, "quality": 1, "centerX": 1})),
            ("polarInversion", &img, json!({"amount": 1e9, "offsetX": -5}), json!({"amount": 8, "offsetX": -2})),
            ("dentsEffect", &img, json!({"scale": 1e4, "detail": 400, "seed": 300}), json!({"scale": 200, "detail": 100, "seed": 255})),
            ("dentsEffect", &img, json!({"refraction": -1}), json!({"refraction": 0})),
        ];
        for (id, src, a, b) in cases {
            assert_eq!(
                run(id, a.clone(), src).data,
                run(id, b, src).data,
                "{id} {a}"
            );
        }
        // An empty layer renders nothing (Pixelate's scale-down grid must not tap it).
        let empty = ImageData {
            width: 0,
            height: 3,
            data: vec![],
        };
        for d in crate::effect::effects() {
            if d.category == crate::effect::Category::Effect("distort") {
                assert!(run(d.id, json!({}), &empty).data.is_empty(), "{}", d.id);
            }
        }
        // A centre exactly on a pixel centre (odd layer, λ = 0) must not turn that pixel to NaN.
        let odd = from_fn(5, 3, |x, y| [(x * 50) as u8, (y * 90) as u8, 30, 255]);
        assert_eq!(
            run("polarInversion", json!({"amount": 0, "quality": 1}), &odd).data,
            odd.data
        );
    }
}
