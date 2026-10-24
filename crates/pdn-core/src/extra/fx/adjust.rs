//! Ported from PhotoCraft crates/compose/src/adjust.rs (+ crates/color convert/blend helpers, crates/engine/src/adjust_params.rs `PHOTO_FILTERS`) @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: only Vibrance, Threshold, Gradient Map, Channel Mixer, Photo Filter and Selective Color, on an interleaved RGBA f32 buffer (alpha last) instead of `Buffer`/`Adjustment`; the 8-bit sRGB document paths only (no 16/32-bit, Lab or CMYK branches); the colour helpers (`srgb_to_linear`, `mat3_mul`, `lum`, `set_lum`) are inlined.

/// Pixels with alpha > 0 through `f` on their RGB (upstream `map_rgb`).
fn map_rgb(buf: &mut [f32], f: impl Fn([f32; 3]) -> [f32; 3]) {
    for p in buf.as_chunks_mut::<4>().0 {
        if p[3] <= 0.0 {
            continue;
        }
        let r = f([p[0], p[1], p[2]]);
        p[..3].copy_from_slice(&r);
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// W3C luminosity, the weights Photoshop's blend modes and Gradient Map use.
pub fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// ClipColor for an 8-bit document (`top` = 1): towards the grey, keeping the luminosity.
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        let d = l - n;
        for v in &mut out {
            *v = if l > 0.0 && d > 0.0 {
                l + (*v - l) * l / d
            } else {
                l
            };
        }
    }
    if x > 1.0 {
        let d = x - l;
        for v in &mut out {
            *v = if l < 1.0 && d > 0.0 {
                l + (*v - l) * (1.0 - l) / d
            } else {
                l
            };
        }
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

// ---- Vibrance ----

/// Weights of the grey Photoshop's Vibrance › Saturation desaturates to (R, G; blue has none).
const VIBRANCE_GREY: [f32; 2] = [0.288, 0.712];
/// Positive Vibrance's saturation boost, fitted to Photoshop's measurements.
const VIBRANCE_BOOST: [f32; 7] = [0.5001, 0.0887, 0.4374, 0.7573, -0.8113, 0.5154, 2.8026];

/// Vibrance and Saturation (the sliders / 100) on every pixel, in linear light, Vibrance first.
pub fn vibrance(buf: &mut [f32], v: f32, s: f32) {
    map_rgb(buf, |c| vibrance_px(c, v, s));
}

fn vibrance_px(c: [f32; 3], v: f32, s: f32) -> [f32; 3] {
    let mut l = c.map(|x| srgb_to_linear(x.max(0.0)));
    if v != 0.0 {
        l = vibrance_lin(l, v);
    }
    if s != 0.0 {
        let g = VIBRANCE_GREY[0] * l[0] + VIBRANCE_GREY[1] * l[1];
        l = l.map(|x| (g + (1.0 + s) * (x - g)).clamp(0.0, 1.0));
    }
    l.map(|x| linear_to_srgb(x.clamp(0.0, 1.0)))
}

/// The Vibrance slider (`v` in [-1, 1]) on a linear-light pixel: an HSV change that keeps the hue.
fn vibrance_lin(c: [f32; 3], v: f32) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx <= 0.0 || mx - mn <= 1e-9 {
        return c;
    }
    let t = 1.0 - mn / mx;
    let shape = t * (1.0 - t) * (2.0 - t + t * t) * mx * (1.0 - mx).max(0.0);
    let (t2, v2) = if v < 0.0 {
        let a = -v;
        (
            t * (1.0 - a / 4.0) * (1.0 - a + a * t * (1.0 + t) / 2.0),
            mx - a * shape,
        )
    } else {
        let [c0, c1, p, q, r0, ra, rt] = VIBRANCE_BOOST;
        let w = vibrance_skin(hsv_hue(c, mx, mn));
        let a = v * (1.0 - w) + 0.64 * v.powf(1.6) * w;
        let boost = (c0 * a + c1 * a * a)
            * t.powf(p)
            * (1.0 - t).powf(q)
            * ((r0 + ra * a + rt * (1.0 - t)) * (1.0 - mx).max(0.0)).exp();
        ((t * (1.0 + boost)).min(1.0), mx + v / 4.0 * shape)
    };
    let m2 = v2 * (1.0 - t2);
    let k = (v2 - m2) / (mx - mn);
    c.map(|x| m2 + (x - mn) * k)
}

/// How much positive Vibrance treats a hue (degrees) as skin.
fn vibrance_skin(h: f32) -> f32 {
    if h <= 30.0 {
        1.0
    } else if h < 45.0 {
        (45.0 - h) / 15.0
    } else if h < 300.0 {
        0.0
    } else {
        (h - 300.0) / 60.0
    }
}

fn hsv_hue(c: [f32; 3], mx: f32, mn: f32) -> f32 {
    let d = mx - mn;
    let h = if mx == c[0] {
        ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    h * 60.0
}

// ---- Threshold ----

/// White where the W3C luminosity sum (30 R + 59 G + 11 B over 8-bit levels) exceeds the level
/// (1–255), else black.
pub fn threshold(buf: &mut [f32], level: f32) {
    let t = level.round();
    map_rgb(buf, |c| {
        let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round());
        let white = 30.0 * r + 59.0 * g + 11.0 * b > 100.0 * t - 50.0;
        [if white { 1.0 } else { 0.0 }; 3]
    });
}

// ---- Gradient Map ----

/// Ordered-dither offset in -0.5..0.5 (4×4 Bayer matrix) for pixel (x, y).
fn bayer4(x: usize, y: usize) -> f32 {
    const M: [f32; 16] = [
        0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0,
    ];
    (M[(y & 3) * 4 + (x & 3)] + 0.5) / 16.0 - 0.5
}

fn gradient(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    match stops {
        [] => [t; 3],
        [s] => s.1,
        _ => {
            if t <= stops[0].0 {
                return stops[0].1;
            }
            for w in stops.windows(2) {
                if t <= w[1].0 {
                    let k = if w[1].0 > w[0].0 {
                        (t - w[0].0) / (w[1].0 - w[0].0)
                    } else {
                        0.0
                    };
                    return std::array::from_fn(|i| w[0].1[i] + (w[1].1[i] - w[0].1[i]) * k);
                }
            }
            stops[stops.len() - 1].1
        }
    }
}

/// Maps luminosity through `stops` (position, RGB), a w-pixel-wide buffer; optionally reversed and
/// ordered-dithered by half an 8-bit step.
pub fn gradient_map(
    buf: &mut [f32],
    w: usize,
    stops: &[(f32, [f32; 3])],
    reverse: bool,
    dither: bool,
) {
    for (i, p) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if p[3] <= 0.0 {
            continue;
        }
        let mut t = lum([p[0], p[1], p[2]]);
        if reverse {
            t = 1.0 - t;
        }
        let mut o = gradient(stops, t);
        if dither {
            let d = bayer4(i % w.max(1), i / w.max(1)) / 255.0;
            o = o.map(|v| (v + d).clamp(0.0, 1.0));
        }
        p[..3].copy_from_slice(&o);
    }
}

// ---- Channel Mixer ----

/// Each output channel is `matrix[out]` = [R, G, B, constant] (fractions) applied to the source,
/// clamped; monochrome writes row 0 to all three.
pub fn channel_mixer(buf: &mut [f32], matrix: &[[f32; 4]; 3], monochrome: bool) {
    map_rgb(buf, |c| {
        let mix = |row: &[f32; 4]| {
            (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3]).clamp(0.0, 1.0)
        };
        if monochrome {
            [mix(&matrix[0]); 3]
        } else {
            [mix(&matrix[0]), mix(&matrix[1]), mix(&matrix[2])]
        }
    });
}

// ---- Photo Filter ----

/// Photo Filter presets `(id, label, colour)`.
pub const PHOTO_FILTERS: &[(&str, &str, [u8; 3])] = &[
    ("warming85", "Warming Filter (85)", [236, 138, 0]),
    ("warmingLBA", "Warming Filter (LBA)", [250, 150, 0]),
    ("warming81", "Warming Filter (81)", [235, 177, 19]),
    ("cooling80", "Cooling Filter (80)", [0, 109, 255]),
    ("coolingLBB", "Cooling Filter (LBB)", [0, 93, 255]),
    ("cooling82", "Cooling Filter (82)", [0, 181, 255]),
    ("red", "Red", [234, 26, 26]),
    ("orange", "Orange", [243, 132, 23]),
    ("yellow", "Yellow", [249, 227, 28]),
    ("green", "Green", [25, 201, 25]),
    ("cyan", "Cyan", [29, 203, 234]),
    ("blue", "Blue", [29, 53, 234]),
    ("violet", "Violet", [155, 29, 234]),
    ("magenta", "Magenta", [227, 24, 227]),
    ("sepia", "Sepia", [172, 122, 51]),
    ("deepRed", "Deep Red", [255, 0, 0]),
    ("deepBlue", "Deep Blue", [0, 34, 205]),
    ("deepEmerald", "Deep Emerald", [0, 140, 0]),
    ("deepYellow", "Deep Yellow", [255, 213, 0]),
    ("underwater", "Underwater", [0, 194, 177]),
];

const D50: [f32; 3] = [0.964_22, 1.0, 0.825_21];
const SRGB_TO_XYZ_D50: [[f32; 3]; 3] = [
    [0.436_074, 0.385_064, 0.143_080],
    [0.222_504, 0.716_878, 0.060_618],
    [0.013_932, 0.097_104, 0.714_173],
];
const XYZ_D50_TO_SRGB: [[f32; 3]; 3] = [
    [3.133_856, -1.616_867, -0.490_615],
    [-0.978_768, 1.916_142, 0.033_454],
    [0.071_945, -0.228_991, 1.405_243],
];

fn mat3_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

/// Photo Filter's linear-light RGB matrix: the pixel's D50 XYZ, relative to white, times the filter
/// colour's, mixed in by `density`, folded into one matrix.
fn photo_filter_matrix(color: [f32; 3], density: f32) -> [[f32; 3]; 3] {
    let d = if density.is_finite() {
        density.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let xyz = mat3_mul(
        &SRGB_TO_XYZ_D50,
        color.map(|v| srgb_to_linear(v.clamp(0.0, 1.0))),
    );
    let s: [f32; 3] = std::array::from_fn(|i| 1.0 - d + d * xyz[i] / D50[i]);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3)
                .map(|k| XYZ_D50_TO_SRGB[r][k] * s[k] * SRGB_TO_XYZ_D50[k][c])
                .sum()
        })
    })
}

/// Tints toward `color` (0–1 RGB) by `density` (0–1); with `preserve_luminosity` the result is
/// moved back to the source luminosity like the Luminosity blend mode.
pub fn photo_filter(buf: &mut [f32], color: [f32; 3], density: f32, preserve_luminosity: bool) {
    let m = photo_filter_matrix(color, density);
    map_rgb(buf, |c| {
        let f =
            mat3_mul(&m, c.map(|v| srgb_to_linear(v.max(0.0)))).map(|v| linear_to_srgb(v.max(0.0)));
        let f = if preserve_luminosity {
            set_lum(f, lum(c))
        } else {
            f
        };
        f.map(|v| v.clamp(0.0, 1.0))
    });
}

// ---- Selective Color ----

/// How much a colour belongs to each range: reds, yellows, greens, cyans, blues, magentas,
/// whites, neutrals, blacks.
fn selective_color_weights(c: [f32; 3]) -> [f32; 9] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let mid = c[0] + c[1] + c[2] - max - min;
    let top = |i: usize| if c[i] >= max { max - mid } else { 0.0 };
    let bottom = |i: usize| if c[i] <= min { mid - min } else { 0.0 };
    [
        top(0),
        bottom(2),
        top(1),
        bottom(0),
        top(2),
        bottom(1),
        ((min - 0.5) * 2.0).max(0.0),
        (1.0 - (max - 0.5).abs() - (min - 0.5).abs()).clamp(0.0, 1.0),
        ((0.5 - max) * 2.0).max(0.0),
    ]
}

/// Photoshop Selective Color: per range [C, M, Y, K] percent shifts on the inks (1 − R, G, B).
pub fn selective_color(buf: &mut [f32], relative: bool, adj: &[[f32; 4]; 9]) {
    map_rgb(buf, |c| {
        let w = selective_color_weights(c);
        let mut delta = [0.0f32; 3];
        for (r, wr) in w.iter().enumerate() {
            if *wr <= 0.0 {
                continue;
            }
            let a = adj[r].map(|v| v / 100.0);
            for i in 0..3 {
                let ink = 1.0 - c[i];
                let d = a[i] + a[3];
                let d = if relative {
                    d * ink
                } else if d > 0.0 {
                    d * (2.0 * (1.0 - ink)).min(1.0)
                } else {
                    d * (2.0 * ink).min(1.0)
                };
                delta[i] += d * wr;
            }
        }
        std::array::from_fn(|i| (c[i] - delta[i]).clamp(0.0, 1.0))
    });
}
