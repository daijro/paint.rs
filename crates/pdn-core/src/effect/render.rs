//! The 4 Render effects on Paint.NET's math, and SVG feTurbulence.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` entries cloudsEffect,
//! juliaFractalEffect, mandelbrotFractalEffect and turbulenceEffect: ids, icons and control keys
//! only).
//! Turbulence: W3C SVG 1.1 §15.24 / Filter Effects 1
//! `feTurbulence` reference code (`init`, `noise2`, `turbulence` with stitching).

use super::blur::madd;
use super::mix::{CHOICES, MixMode, mix_at};
use super::perlin::noise;
use super::sample::rgss_offsets;
use super::surface::{Alpha, EffectSurface, Space, premultiply, unpremultiply};
use super::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, GPU_QUALITY, GpuFn,
    RenderInfo, Values, choice, color_value, flag, num, range,
};
use std::array::from_fn;
use std::f64::consts::PI;

const SRGB: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Straight);
const SRGB_PM: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Premultiplied);
const LINEAR_PM: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);

fn render(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    f: GpuFn,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("render"),
        controls,
        kind: EffectKind::Gpu(info, f),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Render submenu (§1): paintdotjs ids, icons, keys, labels and steps; Paint.NET's ranges,
/// colours, Blend mode and the fractal controls (R19, §2.8–2.11).
pub fn effects() -> Vec<EffectDef> {
    let blend = |default| Control::Select {
        key: "blendMode",
        label: "Blend mode",
        default,
        choices: &CHOICES,
    };
    let quality = || range("quality", "Quality", 1.0, 8.0, GPU_QUALITY);
    let angle = || range("angle", "Angle", -180.0, 180.0, 0.0);
    let colour = |key, label, default| Control::Color {
        key,
        label,
        default,
    };
    vec![
        render(
            "cloudsEffect",
            "clouds_effect.png",
            vec![
                range("scale", "Scale", 2.0, 1000.0, 250.0).exponential(),
                range("roughness", "Roughness", 0.0, 1.0, 0.5).step(0.01),
                blend("over"),
                range("seed", "Seed", 0.0, 255.0, 0.0),
                colour("color1", "Color 1", ColorDefault::Primary),
                colour("color2", "Color 2", ColorDefault::Secondary),
            ],
            SRGB,
            clouds,
        ),
        render(
            "juliaFractalEffect",
            "julia_fractal_effect_icon.png",
            vec![
                range("factor", "Factor", 1.0, 10.0, 4.0),
                // paintdotjs's `zoom` steps by 1 over 1..100; Paint.NET's 0.1..50 [1] needs the
                // brief's 0.1.
                range("zoom", "Zoom", 0.1, 50.0, 1.0).step(0.1), // R53: step stays 0.1
                angle(),
                quality(),
                blend("overwrite"),
            ],
            LINEAR_PM,
            julia,
        ),
        render(
            "mandelbrotFractalEffect",
            "mandelbrot_fractal_effect_icon.png",
            vec![
                range("factor", "Factor", 1.0, 10.0, 1.0),
                range("zoom", "Zoom", 0.0, 100.0, 10.0),
                angle(),
                quality(),
                Control::Checkbox {
                    key: "invert",
                    label: "Invert colors",
                    default: false,
                },
                blend("overwrite"),
            ],
            LINEAR_PM,
            mandelbrot,
        ),
        render(
            "turbulenceEffect",
            "turbulence_effect_icon.png",
            vec![
                range("octaves", "Octaves", 1.0, 15.0, 4.0),
                range("period", "Period", 0.1, 1024.0, 100.0)
                    .step(0.1)
                    .exponential(),
                range("size", "Size", 1.0, 4096.0, 4096.0),
                Control::Select {
                    key: "noise",
                    label: "Noise",
                    default: "turbulence",
                    choices: &[("fractal", "Fractal sum"), ("turbulence", "Turbulence")],
                },
                range("seed", "Seed", 0.0, 255.0, 1.0),
                blend("over"),
            ],
            SRGB_PM,
            turbulence_effect,
        ),
    ]
}

/// §2.8 steps 2–4 and 5's t: dx = 2x − W, dy = 2y − H; up to 12 octaves while mult > 0.03 and
/// div > 0, each adding N(ix & 255, iy & 255, frac, (s + i) & 255)·mult at f = 65536 + d/div in
/// f32 (ix = trunc f), then div ≫= 1 and mult ·= power; t = (val + 1)/2.
pub fn clouds_value(x: u32, y: u32, w: u32, h: u32, scale: f64, power: f32, seed: u8) -> f32 {
    let (dx, dy) = (
        (2 * x as i64 - w as i64) as f32,
        (2 * y as i64 - h as i64) as f32,
    );
    let (mut val, mut mult, mut div) = (0.0f32, 1.0f32, scale.floor() as i64);
    for i in 0..12u8 {
        if mult <= 0.03 || div <= 0 {
            break;
        }
        let (fx, fy) = (65536.0 + dx / div as f32, 65536.0 + dy / div as f32);
        let (ix, iy) = (fx.trunc(), fy.trunc());
        let n = noise(
            ix as i32 & 255,
            iy as i32 & 255,
            fx - ix,
            fy - iy,
            seed.wrapping_add(i),
        );
        val += n * mult;
        div >>= 1;
        mult *= power;
    }
    (val + 1.0) / 2.0
}

/// §2.8 on straight sRGB: c = lerp(premul C1, premul C2, t) unclamped (unpremultiplied when
/// either colour is translucent), then mixed onto the layer.
fn clouds(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let scale = num(v, "scale").clamp(2.0, 1000.0);
    let power = num(v, "roughness").clamp(0.0, 1.0) as f32;
    let seed = ((num(v, "seed").clamp(0.0, 255.0) as u32 ^ env.instance_seed) & 255) as u8;
    let mode = MixMode::from_value(choice(v, "blendMode"));
    let [c1, c2] =
        ["color1", "color2"].map(|k| premultiply(color_value(v, k, env).map(|b| b as f32 / 255.0)));
    let translucent = c1[3] != 1.0 || c2[3] != 1.0;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let t = clouds_value(x, y, src.w, src.h, scale, power, seed);
        let c: [f32; 4] = from_fn(|i| c1[i] + t * (c2[i] - c1[i]));
        let c = if translucent { unpremultiply(c) } else { c };
        let layer = premultiply(src.at(x, y));
        unpremultiply(mix_at(mode, layer, premultiply(c), y * src.w + x))
    })
}

/// §2.9 steps 1–2 and 7 for both fractals: per RGSS sample, uv = (2p − (W, H))/H turned by the
/// angle in polar form; `colour(uv′)` is straight, premultiplied, summed in order, × f32(1/n).
fn fractal(
    src: &EffectSurface,
    v: &Values,
    colour: impl Fn([f64; 2]) -> [f64; 4] + Sync,
) -> EffectSurface {
    let offsets = rgss_offsets(num(v, "quality").clamp(1.0, 8.0) as u32);
    let turn = num(v, "angle").clamp(-180.0, 180.0) * PI / 180.0;
    let (w, h) = (src.w as f64, src.h as f64);
    let inv = 1.0 / offsets.len() as f32;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let mut sum = [0.0f32; 4];
        for o in &offsets {
            let p = [x as f64 + 0.5 + o[0] as f64, y as f64 + 0.5 + o[1] as f64];
            let uv = [(2.0 * p[0] - w) / h, (2.0 * p[1] - h) / h];
            let (r, th) = (uv[0].hypot(uv[1]), uv[1].atan2(uv[0]) + turn);
            let c = colour([r * th.cos(), r * th.sin()]).map(|c| c.clamp(0.0, 1.0) as f32);
            madd(&mut sum, 1.0, premultiply(c));
        }
        sum.map(|s| s * inv)
    })
}

/// §2.9 step 8: the fractal mixed onto the layer in linear premultiplied light (Overwrite is the
/// fractal itself).
fn mix_layer(src: &EffectSurface, art: &EffectSurface, v: &Values) -> EffectSurface {
    let mode = MixMode::from_value(choice(v, "blendMode"));
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        mix_at(mode, src.at(x, y), art.at(x, y), y * src.w + x)
    })
}

/// §2.9 steps 3–6: X = (u − v·a)/z, Y = (v + u·a)/z with a = H/W; z ← z² + 0.3125 + 0.03i while
/// n < 256 and |z|² < 10⁴; j = n − (2 − 2·ln 10⁴/ln|z|²), v = F·j/256 → (v − 1, v − 2, v − 3, v).
fn julia(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // GPU f32 escape counts may flip rare pixels: ±1 (effects-2.md §6 item 10)
    let f = num(v, "factor").clamp(1.0, 10.0);
    let z = num(v, "zoom").clamp(0.1, 50.0);
    let a = src.h as f64 / src.w as f64;
    let art = fractal(src, v, |[u, w]| {
        let (mut x, mut y) = ((u - w * a) / z, (w + u * a) / z);
        let mut n = 0;
        while n < 256 && x * x + y * y < 10000.0 {
            (x, y) = (x * x - y * y + 0.3125, 2.0 * x * y + 0.03);
            n += 1;
        }
        let j = n as f64 - (2.0 - 2.0 * 10000f64.ln() / (x * x + y * y).ln());
        let s = f * j / 256.0;
        [s - 1.0, s - 2.0, s - 3.0, s]
    });
    mix_layer(src, &art, v)
}

/// §2.10: c = uv′/M + (−0.7, −0.29), M = 1 + 20·zoom; z ← z² + c from 0 while n·F < 1024 and
/// |z|² < 10⁵; m = n − ln|z|²/ln 10⁵, v = (64 + F·m)/256 → (v − 3, v − 2, v − 1, v); Invert takes
/// 1 − rgb of the averaged straight colour.
fn mandelbrot(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // GPU f32 escape counts may flip rare pixels: ±1 (effects-2.md §6 item 10)
    let f = num(v, "factor").clamp(1.0, 10.0);
    let m = 1.0 + 20.0 * num(v, "zoom").clamp(0.0, 100.0);
    let mut art = fractal(src, v, |[u, w]| {
        let (cr, ci) = (u / m - 0.7, w / m - 0.29);
        let (mut x, mut y, mut n) = (0.0f64, 0.0f64, 0.0f64);
        while n * f < 1024.0 && x * x + y * y < 100000.0 {
            (x, y) = (x * x - y * y + cr, 2.0 * x * y + ci);
            n += 1.0;
        }
        let s = (64.0 + f * (n - (x * x + y * y).ln() / 100000f64.ln())) / 256.0;
        [s - 3.0, s - 2.0, s - 1.0, s]
    });
    if flag(v, "invert") {
        for p in &mut art.px {
            let [r, g, b, a] = unpremultiply(*p);
            *p = premultiply([1.0 - r, 1.0 - g, 1.0 - b, a]);
        }
    }
    mix_layer(src, &art, v)
}

/// §2.11: the stitched feTurbulence tile at pixel centres, repeated every Size px (D2D Tile),
/// straight colour premultiplied, mixed onto the layer.
fn turbulence_effect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    // approximation: D2D Turbulence = SVG feTurbulence (effects-2.md §6 item 2; B-E8)
    let octaves = num(v, "octaves").clamp(1.0, 15.0) as u32;
    let freq = 1.0 / num(v, "period").clamp(0.1, 1024.0);
    let size = num(v, "size").clamp(1.0, 4096.0) as u32;
    let fractal_sum = choice(v, "noise") == "fractal";
    let lattice = Lattice::new(num(v, "seed").clamp(0.0, 255.0) as i32);
    let tile = EffectSurface::from_fn(size.min(src.w), size.min(src.h), |x, y| {
        let p = [x as f64 + 0.5, y as f64 + 0.5];
        premultiply(lattice.turbulence(p, freq, octaves, fractal_sum, Some(size)))
    });
    let mode = MixMode::from_value(choice(v, "blendMode"));
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let t = tile.at(x % size, y % size);
        mix_at(mode, src.at(x, y), t, y * src.w + x)
    })
}

/// feTurbulence at `(x, y)` for base frequency `freq` on both axes, stitched to a `stitch`-px
/// square tile at the origin: per channel the octave sum of noise2 (fractal sum) or |noise2|
/// (turbulence), mapped (sum + 1)/2 for fractal sum, clamped to [0, 1]; straight RGBA. Runs
/// `init(seed)` per call; the effect builds the lattice once.
pub fn turbulence(
    x: f64,
    y: f64,
    freq: f64,
    octaves: u32,
    seed: i32,
    fractal_sum: bool,
    stitch: Option<u32>,
) -> [f32; 4] {
    Lattice::new(seed).turbulence([x, y], freq, octaves, fractal_sum, stitch)
}

const BSIZE: usize = 256;
const PERLIN_N: f64 = 4096.0;

/// The reference code's `uLatticeSelector` and `fGradient` after `init(seed)`.
struct Lattice {
    sel: Vec<usize>,
    grad: Vec<[[f64; 2]; 2 * BSIZE + 2]>,
}

/// The reference Park–Miller step: a = 16807, m = 2³¹ − 1 (Schrage).
fn park_miller(s: i64) -> i64 {
    let r = 16807 * (s % 127773) - 2836 * (s / 127773);
    if r <= 0 { r + 2147483647 } else { r }
}

impl Lattice {
    /// `init`: `setup_seed`; per channel k and i < 256 two draws (r mod 512 − 256)/256,
    /// normalised; the selector shuffled for i = 255..1 with j = r mod 256; both copied up.
    fn new(seed: i32) -> Self {
        let m = 2147483647i64;
        let mut s = seed as i64;
        if s <= 0 {
            s = -(s % (m - 1)) + 1;
        }
        s = s.min(m - 1);
        let mut draw = || {
            s = park_miller(s);
            s
        };
        let mut sel: Vec<usize> = (0..2 * BSIZE + 2).collect();
        let mut grad = vec![[[0.0f64; 2]; 2 * BSIZE + 2]; 4];
        for g in &mut grad {
            for v in &mut g[..BSIZE] {
                *v = [0, 1]
                    .map(|_| ((draw() % (2 * BSIZE as i64)) - BSIZE as i64) as f64 / BSIZE as f64);
                let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
                *v = [v[0] / n, v[1] / n];
            }
        }
        for i in (1..BSIZE).rev() {
            let j = (draw() % BSIZE as i64) as usize;
            sel.swap(i, j);
        }
        for i in 0..BSIZE + 2 {
            sel[BSIZE + i] = sel[i];
            for g in &mut grad {
                g[BSIZE + i] = g[i];
            }
        }
        Self { sel, grad }
    }

    /// `noise2` for channel `k`; with stitching, lattice indices at or past `wrap` step back by
    /// `size` before the & 255 mask (Filter Effects 1's order; SVG 1.1's text masks first, which
    /// makes the stitch test unreachable).
    fn noise2(&self, k: usize, v: [f64; 2], st: Option<&Stitch>) -> f64 {
        let axis = |c: f64, a: usize| {
            let t = c + PERLIN_N;
            let mut b = [t as i64, t as i64 + 1];
            if let Some(st) = st {
                for b in &mut b {
                    if *b >= st.wrap[a] {
                        *b -= st.size[a];
                    }
                }
            }
            let r0 = t - (t as i64) as f64;
            (b.map(|b| (b & 255) as usize), r0, r0 - 1.0)
        };
        let ([bx0, bx1], rx0, rx1) = axis(v[0], 0);
        let ([by0, by1], ry0, ry1) = axis(v[1], 1);
        let (i, j) = (self.sel[bx0], self.sel[bx1]);
        let g = &self.grad[k];
        let dot = |b: usize, x: f64, y: f64| x * g[b][0] + y * g[b][1];
        let s_curve = |t: f64| t * t * (3.0 - 2.0 * t);
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let (sx, sy) = (s_curve(rx0), s_curve(ry0));
        let a = lerp(
            sx,
            dot(self.sel[i + by0], rx0, ry0),
            dot(self.sel[j + by0], rx1, ry0),
        );
        let b = lerp(
            sx,
            dot(self.sel[i + by1], rx0, ry1),
            dot(self.sel[j + by1], rx1, ry1),
        );
        lerp(sy, a, b)
    }

    /// `turbulence` per channel, with the tile at (0, 0): the stitch snaps each frequency to
    /// ⌊S·f⌋/S or ⌈S·f⌉/S (whichever ratio is nearer 1), then doubles the wrap every octave.
    fn turbulence(
        &self,
        p: [f64; 2],
        freq: f64,
        octaves: u32,
        fractal_sum: bool,
        stitch: Option<u32>,
    ) -> [f32; 4] {
        let mut f = freq;
        let base = stitch.map(|s| {
            let s = s as f64;
            if f != 0.0 {
                let (lo, hi) = ((s * f).floor() / s, (s * f).ceil() / s);
                f = if f / lo < hi / f { lo } else { hi };
            }
            let n = (s * f + 0.5) as i64;
            Stitch {
                size: [n; 2],
                wrap: [PERLIN_N as i64 + n; 2],
            }
        });
        from_fn(|k| {
            let mut st = base;
            let (mut sum, mut v, mut ratio) = (0.0, [p[0] * f, p[1] * f], 1.0);
            for _ in 0..octaves {
                let n = self.noise2(k, v, st.as_ref());
                sum += if fractal_sum { n } else { n.abs() } / ratio;
                v = v.map(|c| c * 2.0);
                ratio *= 2.0;
                if let Some(st) = &mut st {
                    st.size = st.size.map(|s| s * 2);
                    st.wrap = st.wrap.map(|w| 2 * w - PERLIN_N as i64);
                }
            }
            let c = if fractal_sum { (sum + 1.0) / 2.0 } else { sum };
            c.clamp(0.0, 1.0) as f32
        })
    }
}

/// The reference `StitchInfo` per axis.
#[derive(Clone, Copy)]
struct Stitch {
    size: [i64; 2],
    wrap: [i64; 2],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectEnv;
    use crate::effect::test_util::{px, run, run_env, solid};
    use serde_json::json;

    fn close(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    /// effects-2.md §5 vector 5 (64×64, black → white, instance 0; ±1 for GPU float).
    #[test]
    fn clouds_vectors() {
        let layer = solid(64, 64, [0, 0, 0, 0]);
        let v = json!({"color1": "#000000", "color2": "#ffffff"});
        let out = run("cloudsEffect", v.clone(), &layer);
        for ((x, y), want) in [((0, 0), 84u8), ((10, 20), 109), ((63, 5), 183)] {
            assert!(
                close(px(&out, x, y), [want, want, want, 255]),
                "({x},{y}) {:?}",
                px(&out, x, y)
            );
        }
        let mut v1 = v.clone();
        v1["seed"] = json!(1);
        assert!(close(
            px(&run("cloudsEffect", v1, &layer), 0, 0),
            [152, 152, 152, 255]
        ));
    }

    /// Vectors 6–7: fractals at quality 1, Overwrite.
    #[test]
    fn fractal_vectors() {
        let layer = solid(64, 64, [9, 9, 9, 255]);
        let j = run("juliaFractalEffect", json!({"quality": 1}), &layer);
        for ((x, y), want) in [
            ((0, 0), [0, 0, 0, 11]),
            ((32, 32), [0, 0, 0, 159]),
            ((27, 12), [182, 0, 0, 255]),
            ((21, 21), [136, 0, 0, 255]),
        ] {
            assert!(
                close(px(&j, x, y), want),
                "julia ({x},{y}) {:?}",
                px(&j, x, y)
            );
        }
        let m = run("mandelbrotFractalEffect", json!({"quality": 1}), &layer);
        for ((x, y), want) in [
            ((0, 0), [0, 0, 0, 143]),
            ((16, 40), [0, 0, 0, 200]),
            ((12, 0), [0, 0, 170, 255]),
            ((32, 32), [255, 255, 255, 255]),
        ] {
            assert!(
                close(px(&m, x, y), want),
                "mandelbrot ({x},{y}) {:?}",
                px(&m, x, y)
            );
        }
        let inv = run(
            "mandelbrotFractalEffect",
            json!({"quality": 1, "invert": true}),
            &layer,
        );
        assert!(close(px(&inv, 32, 32), [0, 0, 0, 255]));
        // q = 2 boundary pixel (α < 1, rgb > 0): Invert unpremultiplies first, then 1 − rgb.
        // Independent f64 port of §2.10: premul average at (42, 27) (0, 0, .224, .849) →
        // straight b .264 → 1 − b = .736 → sRGB 223 (stable under 1e−5 offset jitter; α 216.5 → 217).
        let inv2 = run(
            "mandelbrotFractalEffect",
            json!({"quality": 2, "invert": true}),
            &layer,
        );
        assert!(
            close(px(&inv2, 42, 27), [255, 255, 223, 217]),
            "{:?}",
            px(&inv2, 42, 27)
        );
    }

    /// feTurbulence with stitching tiles with period Size (B-E8 calibrates the values).
    #[test]
    fn turbulence_tiles() {
        let out = run(
            "turbulenceEffect",
            json!({"size": 16, "period": 8, "blendMode": "overwrite"}),
            &solid(40, 8, [0, 0, 0, 0]),
        );
        for y in 0..8 {
            for x in 0..24 {
                assert_eq!(px(&out, x, y), px(&out, x + 16, y), "({x},{y})");
            }
        }
        let other = run(
            "turbulenceEffect",
            json!({"size": 16, "period": 8, "seed": 2, "blendMode": "overwrite"}),
            &solid(40, 8, [0, 0, 0, 0]),
        );
        assert_ne!(out.data, other.data);
    }

    // ---- Vectors from an independent Python port of §2.8–2.10 (f32 Clouds, f64 fractals). ----

    fn all_close(img: &crate::canvas::ImageData, cases: &[((u32, u32), [u8; 4])], what: &str) {
        for &((x, y), want) in cases {
            assert!(
                close(px(img, x, y), want),
                "{what} ({x},{y}) {:?}",
                px(img, x, y)
            );
        }
    }

    /// §2.8 step 1: s = (seed ⊕ instance) & 255, so instance 0x101 with seed 0 is seed 1, and
    /// instance 1 cancels seed 1. A 70×40 layer at scale 30 (dx = 2x − W, dy = 2y − H: swapping W
    /// and H moves (33, 17)); roughness 0.8 and seed 7 (8 octaves at (s + i) & 255).
    #[test]
    fn clouds_seed_lattice_and_roughness() {
        let layer = solid(64, 64, [0, 0, 0, 0]);
        let v = json!({"color1": "#000000", "color2": "#ffffff"});
        let mut env = EffectEnv::whole(64, 64);
        env.instance_seed = 0x101;
        assert!(close(
            px(&run_env("cloudsEffect", v.clone(), &layer, &env), 0, 0),
            [152, 152, 152, 255]
        ));
        env.instance_seed = 1;
        let mut v1 = v.clone();
        v1["seed"] = json!(1);
        assert!(close(
            px(&run_env("cloudsEffect", v1, &layer, &env), 0, 0),
            [84, 84, 84, 255]
        ));
        let mut v30 = v.clone();
        v30["scale"] = json!(30);
        let wide = run("cloudsEffect", v30, &solid(70, 40, [0, 0, 0, 0]));
        all_close(
            &wide,
            &[
                ((0, 0), [129, 129, 129, 255]),
                ((69, 39), [129, 129, 129, 255]),
                ((33, 17), [106, 106, 106, 255]),
            ],
            "70×40",
        );
        let mut rough = v.clone();
        rough["roughness"] = json!(0.8);
        rough["seed"] = json!(7);
        all_close(
            &run("cloudsEffect", rough, &layer),
            &[
                ((5, 5), [110, 110, 110, 255]),
                ((50, 9), [100, 100, 100, 255]),
            ],
            "roughness",
        );
    }

    /// §2.8 step 5: the lerp is not clamped (t = −0.456 at (41, 30), scale 60, roughness 1, seed
    /// 3: #404040 → #c0c0c0 gives 6, a clamped t gives 64); translucent colours are lerped
    /// premultiplied, then unpremultiplied (primary (255, 0, 0, 128), secondary blue, t = 0.4268).
    #[test]
    fn clouds_colours() {
        let layer = solid(64, 64, [0, 0, 0, 0]);
        let out = run(
            "cloudsEffect",
            json!({"scale": 60, "roughness": 1, "seed": 3, "color1": "#404040", "color2": "#c0c0c0"}),
            &layer,
        );
        assert!(
            close(px(&out, 41, 30), [6, 6, 6, 255]),
            "{:?}",
            px(&out, 41, 30)
        );
        let mut env = EffectEnv::whole(64, 64);
        env.primary = [255, 0, 0, 128];
        env.secondary = [0, 0, 255, 255];
        let out = run_env("cloudsEffect", json!({}), &layer, &env);
        assert!(
            close(px(&out, 10, 20), [103, 0, 152, 182]),
            "{:?}",
            px(&out, 10, 20)
        );
    }

    /// §2.8 step 6: the layer is the destination (t = 0.4268 at (10, 20)): Under keeps an opaque
    /// layer; Multiply on opaque (200, 100, 50) → (85, 43, 21); Blend2 Additive on (200, 100, 50,
    /// 128) → (209, 159, 134, 255).
    #[test]
    fn clouds_mix_with_the_layer() {
        let v = |m: &str| json!({"color1": "#000000", "color2": "#ffffff", "blendMode": m});
        let out = run(
            "cloudsEffect",
            v("under"),
            &solid(64, 64, [200, 100, 50, 255]),
        );
        assert_eq!(px(&out, 10, 20), [200, 100, 50, 255]);
        let out = run(
            "cloudsEffect",
            v("multiply"),
            &solid(64, 64, [200, 100, 50, 255]),
        );
        assert!(
            close(px(&out, 10, 20), [85, 43, 21, 255]),
            "{:?}",
            px(&out, 10, 20)
        );
        let out = run(
            "cloudsEffect",
            v("additive"),
            &solid(64, 64, [200, 100, 50, 128]),
        );
        assert!(
            close(px(&out, 10, 20), [209, 159, 134, 255]),
            "{:?}",
            px(&out, 10, 20)
        );
    }

    /// §2.9: Factor, Zoom, Angle; RGSS at q = 2; the aspect term on a non-square layer.
    #[test]
    fn julia_parameters() {
        let run_j = |v, w, h| run("juliaFractalEffect", v, &solid(w, h, [9, 9, 9, 255]));
        all_close(
            &run_j(
                json!({"quality": 1, "factor": 7, "zoom": 2.5, "angle": 30}),
                64,
                64,
            ),
            &[
                ((27, 12), [0, 0, 0, 90]),
                ((40, 50), [0, 0, 0, 95]),
                ((10, 33), [105, 0, 0, 255]),
            ],
            "F7 z2.5 a30",
        );
        all_close(
            &run_j(json!({"quality": 2}), 64, 64),
            &[
                ((27, 12), [160, 61, 0, 183]),
                ((21, 21), [29, 0, 0, 198]),
                ((0, 0), [0, 0, 0, 11]),
            ],
            "q2",
        );
        all_close(
            &run_j(json!({"quality": 1}), 80, 50),
            &[
                ((30, 10), [0, 0, 0, 54]),
                ((60, 40), [0, 0, 0, 22]),
                ((45, 25), [0, 0, 0, 158]),
            ],
            "80×50",
        );
    }

    /// §2.9 step 8: other modes mix the fractal over the layer in linear premultiplied light.
    #[test]
    fn julia_mixes_in_linear_light() {
        let layer = solid(64, 64, [200, 150, 100, 255]);
        let m = run(
            "juliaFractalEffect",
            json!({"quality": 1, "blendMode": "multiply"}),
            &layer,
        );
        let o = run(
            "juliaFractalEffect",
            json!({"quality": 1, "blendMode": "over"}),
            &layer,
        );
        all_close(
            &m,
            &[((27, 12), [142, 0, 0, 255]), ((0, 0), [196, 147, 98, 255])],
            "multiply",
        );
        all_close(
            &o,
            &[((27, 12), [182, 0, 0, 255]), ((32, 32), [129, 95, 62, 255])],
            "over",
        );
    }

    /// §2.10: Factor, Zoom, Angle, no aspect term.
    #[test]
    fn mandelbrot_parameters() {
        let run_m = |v, w, h| run("mandelbrotFractalEffect", v, &solid(w, h, [9, 9, 9, 255]));
        all_close(
            &run_m(
                json!({"quality": 1, "factor": 3, "zoom": 4, "angle": -60}),
                64,
                64,
            ),
            &[
                ((12, 0), [0, 0, 0, 189]),
                ((40, 20), [0, 0, 195, 255]),
                ((5, 60), [255, 255, 255, 255]),
            ],
            "F3 z4 a−60",
        );
        all_close(
            &run_m(json!({"quality": 1}), 80, 50),
            &[
                ((12, 0), [0, 0, 0, 176]),
                ((40, 20), [0, 0, 0, 235]),
                ((70, 45), [255, 255, 255, 255]),
            ],
            "80×50",
        );
    }

    // ---- Vectors from an independent f64 Python port of the SVG 1.1 feTurbulence code. ----

    /// `init` (Park–Miller lattice and gradients), `noise2`, fractal sum vs turbulence, the
    /// stitch frequency snap and wrap (16-px tile: x = 15.5 is near x = −0.5, not x = 31.5),
    /// seeds ≤ 0 through `setup_seed`.
    #[test]
    fn turbulence_matches_the_reference_code() {
        for ((x, y, f, o, s, fr, st), want) in [
            (
                (10.5, 20.5, 0.01, 4, 1, false, Some(4096)),
                [0.307_065_6, 0.150_215_36, 0.149_671_27, 0.274_991_63],
            ),
            (
                (10.5, 20.5, 0.01, 4, 1, true, Some(4096)),
                [0.624_715_45, 0.425_514_34, 0.427_548_92, 0.519_192_3],
            ),
            (
                (3.25, 7.75, 0.05, 3, 0, true, None),
                [0.600_581_7, 0.370_978_33, 0.438_270_93, 0.441_734_43],
            ),
            (
                (100.5, 0.5, 0.125, 1, 2, false, Some(16)),
                [0.025_630_496, 0.371_735_24, 0.108_505_22, 0.222_988_95],
            ),
            (
                (15.5, 3.5, 0.125, 4, 1, false, Some(16)),
                [0.372_265_25, 0.113_065_8, 0.152_056_7, 0.253_771_48],
            ),
            (
                (31.5, 3.5, 0.125, 4, 1, false, Some(16)),
                [0.329_004_23, 0.425_704_48, 0.154_212_18, 0.488_663_05],
            ),
            (
                (-0.5, 3.5, 0.125, 4, 1, false, Some(16)),
                [0.367_512_7, 0.089_488_976, 0.158_192_29, 0.251_871_2],
            ),
            (
                (5.5, 9.5, 0.3, 2, -7, true, Some(10)),
                [0.611_404_96, 0.605_963_95, 0.518_735_35, 0.511_518_5],
            ),
        ] {
            let got = turbulence(x, y, f, o, s, fr, st);
            assert!(
                got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6),
                "({x}, {y}, {f}, {o}, {s}, {fr}, {st:?}): {got:?}"
            );
        }
    }

    /// §2.11: the tile at pixel centres (x mod Size + ½, y mod Size + ½) as straight colour, Over a
    /// transparent layer; fractal sum at period 30, 2 octaves, seed 9.
    #[test]
    fn turbulence_effect_pixels() {
        let out = run(
            "turbulenceEffect",
            json!({"size": 16, "period": 8}),
            &solid(20, 20, [0, 0, 0, 0]),
        );
        all_close(
            &out,
            &[
                ((3, 4), [42, 111, 55, 111]),
                ((15, 15), [56, 10, 49, 45]),
                ((19, 2), [60, 152, 58, 93]),
            ],
            "turbulence",
        );
        let out = run(
            "turbulenceEffect",
            json!({"period": 30, "octaves": 2, "seed": 9, "noise": "fractal"}),
            &solid(10, 10, [0, 0, 0, 0]),
        );
        all_close(&out, &[((7, 2), [71, 126, 161, 58])], "fractal");
    }
}
