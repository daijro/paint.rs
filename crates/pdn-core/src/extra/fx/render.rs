//! Ported from PhotoCraft crates/algo/src/render.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only Fibers, Lens Flare and Lighting Effects (Clouds are Paint.NET's own); kernels take a whole interleaved straight RGBA f32 layer (`w`, `h`) with the selection bounds instead of `Image`/`Ctx`, so the colour-mode, alpha-flag and 32-bit branches are gone (screen blend and `Texture: Alpha` assume an alpha channel); everything anchors on the bounds as upstream's `ctx.bounds` (the bump heights read neighbours from the whole layer) and pixels outside them are the source; rows run through `extra::par`, `Interrupt` is polled per row; `hash01` and `smoothstep` are `extra::fx::util`'s (upstream noise.rs and fxutil.rs); the tests are rebuilt over such layers with upstream's values.

use super::util::{hash01, smoothstep};
use crate::extra::geom::Rect;
use crate::extra::{interrupt::Interrupt, par};

/// `f(x, y, pixel)` for every pixel of `b` ∩ the layer (the rest is the source); `None` once
/// cancelled.
fn render_rows(
    buf: &[f32],
    (w, h): (usize, usize),
    b: Rect,
    int: &Interrupt,
    f: impl Fn(i32, i32, &mut [f32]) + Sync + Send,
) -> Option<Vec<f32>> {
    let b = b.intersect(&Rect::new(0, 0, w as i32, h as i32));
    let mut res = buf.to_vec();
    par::rows_mut(&mut res, w * 4, |y, row| {
        let y = y as i32;
        if y < b.y0 || y >= b.y1 || int.cancelled() {
            return;
        }
        for x in b.x0..b.x1 {
            f(x, y, &mut row[x as usize * 4..x as usize * 4 + 4]);
        }
    });
    (!int.cancelled()).then_some(res)
}

/// Smooth value noise in [0, 1).
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - ix as f32, y - iy as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let h = |a: i32, b: i32| hash01(a, b, 0, seed);
    let top = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * sx;
    let bot = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * sx;
    top + (bot - top) * sy
}

/// Fibers: vertical fibrous streaks between the foreground and background colours (RGB 0-1,
/// alpha becomes 1). Variance sets how quickly the colour varies across fibres (and how short
/// they are); strength stretches each fibre vertically. Anchored at the bounds' corner.
#[allow(clippy::too_many_arguments)]
pub fn fibers(
    buf: &[f32],
    size: (usize, usize),
    b: Rect,
    variance: f32,
    strength: f32,
    seed: u32,
    colours: ([f32; 3], [f32; 3]),
    int: &Interrupt,
) -> Option<Vec<f32>> {
    let var = variance.clamp(1.0, 64.0);
    let st = strength.clamp(1.0, 64.0);
    let (fg, bg) = colours;
    // Horizontal frequency rises with variance; vertical frequency falls with strength.
    let fx = 0.08 + var / 64.0 * 0.6;
    let fy = fx / (2.0 + st * 1.5) * (0.5 + var / 32.0);
    render_rows(buf, size, b, int, |x, y, px| {
        let (xf, yf) = ((x - b.x0) as f32, (y - b.y0) as f32);
        let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
        let (mut ax, mut ay) = (fx, fy);
        for o in 0..4u32 {
            sum += value_noise(xf * ax, yf * ay, seed.wrapping_add(o * 977)) * amp;
            norm += amp;
            amp *= 0.55;
            ax *= 2.0;
            ay *= 2.0;
        }
        // Fine per-column grain gives the hairline texture.
        let grain = value_noise(xf * 0.9, yf * fy * 0.25, seed ^ 0x5bd1) - 0.5;
        let t = ((sum / norm - 0.5) * (1.6 + var / 40.0) + 0.5 + grain * 0.25).clamp(0.0, 1.0);
        for k in 0..3 {
            px[k] = fg[k] + (bg[k] - fg[k]) * t;
        }
        px[3] = 1.0;
    })
}

/// A lens-flare ghost: position along the flare→centre axis, radius and tint.
struct Ghost {
    t: f32,
    r: f32,
    tint: [f32; 3],
    k: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LensType {
    Zoom,
    Prime35,
    Prime105,
    MoviePrime,
}

/// Lens Flare: an additive (screen) glow, rays, halo ring and ghost discs along the line through
/// the bounds' centre. `centre` is a fraction of the bounds; brightness is 10-300 %.
pub fn lens_flare(
    buf: &[f32],
    size: (usize, usize),
    b: Rect,
    brightness: f32,
    centre: (f32, f32),
    lens: LensType,
    int: &Interrupt,
) -> Option<Vec<f32>> {
    let (bw, bh) = (b.width() as f32, b.height() as f32);
    let diag = bw.hypot(bh).max(1.0);
    let (cx, cy) = (b.x0 as f32 + centre.0 * bw, b.y0 as f32 + centre.1 * bh);
    let (mx, my) = (b.x0 as f32 + bw / 2.0, b.y0 as f32 + bh / 2.0);
    let bright = brightness.clamp(10.0, 300.0) / 100.0;
    let s = diag / 1000.0;
    let (core, glow, ring_r, ring_w, rays, streak) = match lens {
        LensType::Zoom => (14.0, 90.0, 0.0, 0.0, 0.5, 0.0),
        LensType::Prime35 => (12.0, 70.0, 90.0, 6.0, 0.3, 0.0),
        LensType::Prime105 => (22.0, 130.0, 0.0, 0.0, 0.8, 0.0),
        LensType::MoviePrime => (10.0, 60.0, 0.0, 0.0, 0.2, 1.0),
    };
    let ghost = |t, r, tint, k| Ghost { t, r, tint, k };
    let ghosts: Vec<Ghost> = match lens {
        LensType::Zoom => vec![
            ghost(0.35, 10.0, [0.6, 0.8, 1.0], 0.18),
            ghost(0.6, 26.0, [0.5, 1.0, 0.6], 0.10),
            ghost(0.85, 6.0, [1.0, 0.7, 0.4], 0.25),
            ghost(1.25, 40.0, [0.7, 0.5, 1.0], 0.08),
            ghost(1.5, 16.0, [1.0, 0.6, 0.5], 0.15),
            ghost(1.9, 70.0, [0.5, 0.7, 1.0], 0.06),
        ],
        LensType::Prime35 => vec![
            ghost(0.7, 18.0, [0.6, 1.0, 0.7], 0.12),
            ghost(1.4, 30.0, [1.0, 0.7, 0.5], 0.09),
        ],
        LensType::Prime105 => vec![
            ghost(0.5, 14.0, [1.0, 0.8, 0.5], 0.12),
            ghost(1.6, 50.0, [0.6, 0.7, 1.0], 0.07),
        ],
        LensType::MoviePrime => vec![
            ghost(0.4, 8.0, [0.5, 0.7, 1.0], 0.2),
            ghost(1.0, 20.0, [0.4, 0.6, 1.0], 0.12),
            ghost(1.6, 12.0, [0.5, 0.8, 1.0], 0.16),
        ],
    };
    render_rows(buf, size, b, int, |x, y, px| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (dx, dy) = (fx - cx, fy - cy);
        let d = (dx * dx + dy * dy).sqrt();
        let mut f = [0.0f32; 3];
        // Hot core and wide glow (warm white).
        let g = (-(d / (core * s)).powi(2)).exp() + 0.35 * (-d / (glow * s)).exp();
        for (k, w) in [1.0, 0.95, 0.85].iter().enumerate() {
            f[k] += g * w;
        }
        // Rays: angular modulation decaying with distance.
        if rays > 0.0 {
            let th = dy.atan2(dx);
            let m = ((th * 12.0).sin() * 0.5 + 0.5).powi(8)
                + ((th * 7.0 + 1.3).sin() * 0.5 + 0.5).powi(12) * 0.6;
            let rr = rays * m * (-d / (glow * 1.6 * s)).exp();
            for v in f.iter_mut() {
                *v += rr * 0.6;
            }
        }
        // Anamorphic horizontal streak (movie prime).
        if streak > 0.0 {
            let st = (-(dy.abs() / (2.5 * s))).exp() * (-(dx.abs() / (450.0 * s))).exp();
            f[0] += st * 0.5;
            f[1] += st * 0.7;
            f[2] += st;
        }
        // Halo ring with a slight chromatic spread.
        if ring_r > 0.0 {
            for (k, off) in [1.04f32, 1.0, 0.96].iter().enumerate() {
                let rd = (d - ring_r * s * off) / (ring_w * s);
                f[k] += 0.18 * (-rd * rd).exp();
            }
        }
        // Ghosts along the axis through the centre.
        for gh in &ghosts {
            let (gx, gy) = (cx + (mx - cx) * gh.t * 2.0, cy + (my - cy) * gh.t * 2.0);
            let gd = ((fx - gx).powi(2) + (fy - gy).powi(2)).sqrt() / (gh.r * s);
            if gd < 1.2 {
                // Soft-edged disc, slightly brighter at the rim.
                let e = smoothstep(1.0, 0.85, gd) * (0.6 + 0.4 * gd);
                for (fk, t) in f.iter_mut().zip(gh.tint) {
                    *fk += e * gh.k * t;
                }
            }
        }
        // Screen blend keeps the result below white.
        for k in 0..3 {
            let a = (f[k] * bright).max(0.0);
            px[k] += a.min(1.0) * (1.0 - px[k]).max(0.0);
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    Spot,
    Point,
    Infinite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureChannel {
    None,
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
}

#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub kind: LightKind,
    /// Position as fractions of the bounds; `z` is a fraction of the shorter side.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Spot: where the light points (fractions of the bounds).
    pub target_x: f32,
    pub target_y: f32,
    /// Infinite: direction the light comes from (degrees, 0 = right, counter-clockwise) and
    /// elevation.
    pub angle: f32,
    pub elevation: f32,
    pub color: [f32; 3],
    /// -100..100 (negative lights darken).
    pub intensity: f32,
    /// Spot cone half-angle in degrees.
    pub cone: f32,
    /// Spot hotspot, 0-100 % of the cone at full intensity.
    pub hotspot: f32,
    /// Point light reach as a fraction of the bounds' shorter side.
    pub radius: f32,
}

impl Default for Light {
    fn default() -> Self {
        Light {
            kind: LightKind::Spot,
            x: 0.25,
            y: 0.2,
            z: 0.6,
            target_x: 0.5,
            target_y: 0.55,
            angle: 135.0,
            elevation: 45.0,
            color: [1.0, 1.0, 1.0],
            intensity: 75.0,
            cone: 45.0,
            hotspot: 50.0,
            radius: 0.8,
        }
    }
}

pub struct LightingSpec<'a> {
    pub lights: &'a [Light],
    pub gloss: f32,
    pub metallic: f32,
    pub exposure: f32,
    pub ambience: f32,
    pub texture: TextureChannel,
    pub height: f32,
    pub white_is_high: bool,
}

/// Lighting Effects: Blinn-Phong shading of the layer as a surface (bumped by a texture channel)
/// under spot, point and infinite lights.
pub fn lighting(
    buf: &[f32],
    size: (usize, usize),
    b: Rect,
    spec: &LightingSpec,
    int: &Interrupt,
) -> Option<Vec<f32>> {
    let (bw, bh) = (b.width() as f32, b.height() as f32);
    let short = bw.min(bh).max(1.0);
    let hscale = spec.height.clamp(0.0, 100.0) / 100.0 * 0.06 * short;
    let gloss = (spec.gloss.clamp(-100.0, 100.0) + 100.0) / 200.0;
    let shininess = 2f32.powf(1.0 + gloss * 7.0);
    let spec_k = gloss * 0.7;
    let metal = (spec.metallic.clamp(-100.0, 100.0) + 100.0) / 200.0;
    let expo = 2f32.powf(spec.exposure.clamp(-100.0, 100.0) / 50.0);
    let ambient = spec.ambience.clamp(-100.0, 100.0) / 100.0 * 0.5;
    let (w, _) = size;
    // The height under (x, y) from the source, the position clamped to the layer.
    let tex = |x: i32, y: i32| -> f32 {
        let (xx, yy) = (x.clamp(0, w as i32 - 1), y.clamp(0, size.1 as i32 - 1));
        let p = &buf[(yy as usize * w + xx as usize) * 4..][..4];
        let v = match spec.texture {
            TextureChannel::None => 0.0,
            TextureChannel::Alpha => p[3],
            TextureChannel::Luminance => 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2],
            TextureChannel::Red => p[0],
            TextureChannel::Green => p[1],
            TextureChannel::Blue => p[2],
        };
        if spec.white_is_high { v } else { 1.0 - v }
    };
    render_rows(buf, size, b, int, |x, y, px| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let nrm = if spec.texture == TextureChannel::None || hscale <= 0.0 {
            [0.0, 0.0, 1.0]
        } else {
            let hx = (tex(x + 1, y) - tex(x - 1, y)) * 0.5 * hscale;
            let hy = (tex(x, y + 1) - tex(x, y - 1)) * 0.5 * hscale;
            norm3([-hx, -hy, 1.0])
        };
        let base = [px[0], px[1], px[2]];
        let mut lit = [ambient; 3];
        let mut spc = [0.0f32; 3];
        for l in spec.lights {
            let lpos = [b.x0 as f32 + l.x * bw, b.y0 as f32 + l.y * bh, l.z * short];
            let (ldir, atten) = match l.kind {
                LightKind::Infinite => {
                    let (s, co) = l.angle.to_radians().sin_cos();
                    let e = l.elevation.clamp(0.0, 90.0).to_radians();
                    (norm3([co * e.cos(), -s * e.cos(), e.sin()]), 1.0)
                }
                LightKind::Point => {
                    let v = [lpos[0] - fx, lpos[1] - fy, lpos[2]];
                    let dist = (v[0] * v[0] + v[1] * v[1]).sqrt();
                    let reach = (l.radius * short).max(1.0);
                    (norm3(v), (1.0 - dist / reach).clamp(0.0, 1.0).powi(2))
                }
                LightKind::Spot => {
                    let v = [lpos[0] - fx, lpos[1] - fy, lpos[2]];
                    let ld = norm3(v);
                    let tgt = [
                        b.x0 as f32 + l.target_x * bw,
                        b.y0 as f32 + l.target_y * bh,
                        0.0,
                    ];
                    let axis = norm3([tgt[0] - lpos[0], tgt[1] - lpos[1], tgt[2] - lpos[2]]);
                    let cosang = -dot3(ld, axis);
                    let cone = l.cone.clamp(1.0, 89.0);
                    let outer = cone.to_radians().cos();
                    let inner = (cone * (l.hotspot.clamp(0.0, 100.0) / 100.0))
                        .to_radians()
                        .cos();
                    (ld, smoothstep(outer, inner.max(outer + 1e-4), cosang))
                }
            };
            if atten <= 0.0 {
                continue;
            }
            let inten = l.intensity.clamp(-100.0, 100.0) / 50.0 * atten;
            let ndl = dot3(nrm, ldir).max(0.0);
            let hv = norm3([ldir[0], ldir[1], ldir[2] + 1.0]);
            let s = dot3(nrm, hv).max(0.0).powf(shininess) * spec_k;
            for k in 0..3 {
                lit[k] += ndl * inten * l.color[k];
                // Metallic highlights take the surface colour; plastic ones the light's.
                let sc = l.color[k] * (1.0 - metal) + base[k] * metal;
                spc[k] += s * inten.max(0.0) * sc;
            }
        }
        for k in 0..3 {
            px[k] = ((base[k] * lit[k] + spc[k]) * expo).max(0.0);
        }
    })
}

fn norm3(v: [f32; 3]) -> [f32; 3] {
    let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / m, v[1] / m, v[2] / m]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = Rect::new(0, 0, 40, 30);

    fn flat(v: f32) -> Vec<f32> {
        [v, v, v, 1.0].repeat(40 * 30)
    }

    fn at(buf: &[f32], x: usize, y: usize) -> [f32; 4] {
        let o = (y * 40 + x) * 4;
        [buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]
    }

    fn pattern() -> Vec<f32> {
        (0..30)
            .flat_map(|y| {
                (0..40).flat_map(move |x| {
                    [
                        ((x * 7 + y * 3) % 64) as f32 / 63.0,
                        ((x * x + y) % 50) as f32 / 49.0,
                        (y % 9) as f32 / 8.0,
                        1.0,
                    ]
                })
            })
            .collect()
    }

    fn spec(lights: &[Light], gloss: f32) -> LightingSpec<'_> {
        LightingSpec {
            lights,
            gloss,
            metallic: 0.0,
            exposure: 0.0,
            ambience: 0.0,
            texture: TextureChannel::None,
            height: 0.0,
            white_is_high: true,
        }
    }

    fn light_at(buf: &[f32], r: Rect, s: &LightingSpec) -> Vec<f32> {
        lighting(buf, (40, 30), r, s, &Interrupt::NONE).unwrap()
    }

    fn flare(buf: &[f32], r: Rect) -> Vec<f32> {
        lens_flare(
            buf,
            (40, 30),
            r,
            150.0,
            (0.25, 0.5),
            LensType::Zoom,
            &Interrupt::NONE,
        )
        .unwrap()
    }

    /// Upstream `lighting_point_light_falls_off`: same light, layer and margin.
    #[test]
    fn lighting_point_light_falls_off() {
        let point = Light {
            kind: LightKind::Point,
            x: 0.1,
            y: 0.5,
            z: 0.2,
            radius: 1.0,
            intensity: 50.0,
            ..Light::default()
        };
        let out = light_at(&flat(0.8), R, &spec(&[point], -100.0));
        assert!(at(&out, 4, 15)[0] > at(&out, 36, 15)[0] + 0.1);
        // A spot light does not reach outside its cone.
        let spot = Light {
            kind: LightKind::Spot,
            x: 0.5,
            y: 0.5,
            z: 0.3,
            target_x: 0.5,
            target_y: 0.5,
            cone: 20.0,
            ..Light::default()
        };
        let out = light_at(&flat(0.8), R, &spec(&[spot], 0.0));
        assert!(at(&out, 20, 15)[0] > 0.3 && at(&out, 1, 1)[0] < 1e-4);
    }

    /// Upstream `lens_flare_8_bit_result_is_the_screen_blend`, exactly: the amount is what the
    /// flare adds over black.
    #[test]
    fn lens_flare_8_bit_result_is_the_screen_blend() {
        let amount = flare(&flat(0.0), R);
        let src = pattern();
        let out = flare(&src, R);
        for i in 0..40 * 30 {
            for k in 0..3 {
                let (c, a) = (src[i * 4 + k], amount[i * 4 + k]);
                assert_eq!(
                    out[i * 4 + k],
                    c + a.min(1.0) * (1.0 - c).max(0.0),
                    "{i},{k}"
                );
            }
            assert_eq!(out[i * 4 + 3], src[i * 4 + 3]);
        }
    }

    /// A negative intensity darkens below the ambient-only layer.
    #[test]
    fn negative_intensity_darkens() {
        let amb = [Light {
            kind: LightKind::Infinite,
            intensity: 0.0,
            ..Light::default()
        }];
        let neg = [Light {
            intensity: -80.0,
            ..amb[0]
        }];
        let with = |l: &[Light]| {
            let mut s = spec(l, 0.0);
            s.ambience = 50.0;
            light_at(&flat(0.8), R, &s)
        };
        assert!(at(&with(&neg), 10, 10)[0] < at(&with(&amb), 10, 10)[0] - 0.1);
    }

    /// A slope facing the light is brighter than one facing away: the surface rises with x, so
    /// it faces left; a light from the left lights it more than one from the right.
    #[test]
    fn slope_facing_the_light_is_brighter() {
        let ramp: Vec<f32> = (0..30)
            .flat_map(|_| {
                (0..40).flat_map(|x| [x as f32 / 39.0, x as f32 / 39.0, x as f32 / 39.0, 1.0])
            })
            .collect();
        let from = |angle| {
            [Light {
                kind: LightKind::Infinite,
                angle,
                elevation: 45.0,
                intensity: 50.0,
                ..Light::default()
            }]
        };
        let with = |l: &[Light]| {
            let mut s = spec(l, 0.0);
            s.texture = TextureChannel::Luminance;
            s.height = 100.0;
            light_at(&ramp, R, &s)
        };
        let (left, right) = (with(&from(180.0)), with(&from(0.0)));
        assert!(at(&left, 20, 15)[0] > at(&right, 20, 15)[0] + 0.05 * at(&ramp, 20, 15)[0]);
    }

    /// The bump heights read the whole layer: with the bounds' scale matched (`hscale` follows
    /// the shorter side), every pixel of a sub-rect, edges included, equals the whole-layer render.
    #[test]
    fn bump_normals_read_across_the_bounds_edge() {
        let curve: Vec<f32> = (0..30)
            .flat_map(|_| {
                (0..40).flat_map(|x| {
                    let v = (x * x) as f32 / 1600.0;
                    [v, v, v, 1.0]
                })
            })
            .collect();
        let sub = Rect::new(8, 3, 32, 27);
        let l = [Light {
            kind: LightKind::Infinite,
            angle: 180.0,
            elevation: 45.0,
            intensity: 50.0,
            ..Light::default()
        }];
        let mut s = spec(&l, 0.0);
        s.texture = TextureChannel::Luminance;
        // Whole layer: short side 30; sub-rect: 24. Equal bump scale needs 30 * 40 = 24 * 50.
        s.height = 40.0;
        let whole = light_at(&curve, R, &s);
        s.height = 50.0;
        let part = light_at(&curve, sub, &s);
        for (x, y) in (3..27).flat_map(|y| (8..32).map(move |x| (x, y))) {
            let (p, q) = (at(&part, x, y), at(&whole, x, y));
            assert!(
                (0..4).all(|k| (p[k] - q[k]).abs() < 1e-5),
                "{x},{y}: {p:?} {q:?}"
            );
        }
    }

    /// Pixels outside the bounds are the source, for all three kernels.
    #[test]
    fn pixels_outside_the_bounds_are_the_source() {
        let src = pattern();
        let sub = Rect::new(10, 5, 30, 20);
        let l = [Light::default()];
        let outs = [
            fibers(
                &src,
                (40, 30),
                sub,
                16.0,
                4.0,
                3,
                ([1.0; 3], [0.0; 3]),
                &Interrupt::NONE,
            )
            .unwrap(),
            flare(&src, sub),
            light_at(&src, sub, &spec(&l, 0.0)),
        ];
        for out in outs {
            let mut changed = 0;
            for (x, y) in (0..30).flat_map(|y| (0..40).map(move |x| (x, y))) {
                let inside = (10..30).contains(&x) && (5..20).contains(&y);
                if inside {
                    changed += usize::from(at(&out, x, y) != at(&src, x, y));
                } else {
                    assert_eq!(at(&out, x, y), at(&src, x, y), "{x},{y}");
                }
            }
            assert!(changed > 0);
        }
    }
}
