//! BB spec: effects/render-flames.md; effects/render-polygon-stars.md; common.md §1–5

use super::blocks::hsl_adjust;
use super::conventions::{
    FLAMES_CLAMP_BEFORE_SATURATION, FLAMES_GPU_CONTEXT, FLAMES_SWIZZLE_ALPHA,
    POLYGON_DDA_UNWRAPPED_TARGET,
};
use super::manifest::manifest;
use super::raster::{Cap, Join, Pen, polygon_path, stroke_coverage};
use super::ui::{ControlUi, Help, HelpWindow, IndirectUi, Note};
use crate::canvas::ImageData;
use crate::document::layer::BlendMode;
use crate::effect::mix::{MixMode, mix};
use crate::effect::render::clouds_value;
use crate::effect::surface::{Alpha, EffectSurface, luma, premultiply, unpremultiply};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, choice, color_value, flag,
    num, range,
};
use crate::pixel::gamma::quantize;
use crate::pixel::ops::compose;

const HINT: &str = "HINT: Start by drawing a gradient from black to white before running this effect. Try different gradient shapes to vary the flames.";
static FLAMES_UI: IndirectUi = IndirectUi {
    controls: &[
        ("scale", ControlUi::NO_HEADER),
        (
            "roughness",
            ControlUi {
                decimals: Some(3),
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "seed",
            ControlUi {
                note: Note::Text(HINT),
                ..ControlUi::NO_HEADER
            },
        ),
    ],
    ..IndirectUi::DEFAULT
};
static POLYGON_UI: IndirectUi = IndirectUi {
    controls: &[
        (
            "depth",
            ControlUi {
                decimals: Some(2),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "size",
            ControlUi {
                decimals: Some(2),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "angle",
            ControlUi {
                decimals: Some(2),
                wrap: true,
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "color",
            ControlUi {
                reset: false,
                ..ControlUi::NO_HEADER
            },
        ),
    ],
    help: Help::Window(HelpWindow::Older),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let flames = manifest("bbFlames").unwrap();
    let polygon = manifest("bbPolygonStars").unwrap();
    vec![
        EffectDef {
            id: flames.id,
            icon: flames.icon,
            category: flames.category,
            controls: vec![
                range("scale", "Scale", 2.0, 1000.0, 250.0),
                range("roughness", "Roughness", 0.0, 1.0, 0.5)
                    .step(0.01)
                    .slider_keys(3, 0.05, 0.25, false),
                Control::Select {
                    key: "style",
                    label: "",
                    default: "normal",
                    choices: &[
                        ("normal", "Normal Flames"),
                        ("transparent", "Transparent Flames"),
                        ("fog", "Fog/Smoke"),
                        ("smoke", "Transparent Smoke/Steam"),
                    ],
                },
                range("seed", "Randomize", 0.0, 255.0, 0.0),
            ],
            kind: EffectKind::Gpu(FLAMES_GPU_CONTEXT, flames_render),
            dialog: None,
            dialog_title: Some("Render Flames"),
            indirect: Some(&FLAMES_UI),
        },
        EffectDef {
            id: polygon.id,
            icon: polygon.icon,
            category: polygon.category,
            controls: vec![
                range("vertices", "Verticies", 3.0, 100.0, 5.0),
                range("depth", "Star depth", 0.0, 1.0, 0.0)
                    .step(0.01)
                    .slider_keys(2, 0.05, 0.25, false),
                range("size", "Size/Orientation", 0.01, 1.0, 0.9)
                    .step(0.01)
                    .slider_keys(2, 0.05, 0.25, false),
                // R107: native numeric p2/increment1; R14 replacement slider small1/large5.
                range("angle", "", -180.0, 180.0, 90.0).slider_keys(2, 1.0, 5.0, false),
                range("lineWidth", "Line width", 1.0, 100.0, 2.0),
                Control::Color {
                    key: "color",
                    label: "",
                    default: ColorDefault::Primary,
                },
                Control::Checkbox {
                    key: "antiAlias",
                    label: "Anti-Aliased",
                    default: true,
                },
            ],
            kind: EffectKind::Cpu {
                render: polygon_render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some("Polygon/Stars"),
            indirect: Some(&POLYGON_UI),
        },
    ]
}

/// Unclamped Direct2D flame colour matrix in the effective working space.
pub fn flame_rgb(v: f32) -> [f32; 3] {
    [v, 2.2173 * v - 1.2172977, 5.0 * v - 4.0]
}

fn flames_render(src: &EffectSurface, values: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.expect("GPU render context");
    let scale = num(values, "scale").clamp(2.0, 1000.0);
    let roughness = num(values, "roughness").clamp(0.0, 1.0) as f32;
    // quirk(bb): render-flames.md Noise / R136: ordinal days 1 and 257 collide after masking.
    let seed =
        ((num(values, "seed").clamp(0.0, 255.0) as u32 ^ env.today.day_of_year as u32) & 255) as u8;
    let style = choice(values, "style");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let u = match info.input {
            Alpha::Straight => src.at(x, y),
            Alpha::Premultiplied => unpremultiply(src.at(x, y)),
        };
        let g = luma(u[0], u[1], u[2]);
        // approximation: deterministic port Clouds, full-canvas coordinates (render-flames.md Noise parity).
        let t = clouds_value(x, y, env.width, env.height, scale, roughness, seed);
        let o = mix(
            MixMode::Overlay,
            premultiply([g, g, g, u[3]]),
            [t, t, t, 1.0],
        );
        let v = o[0];
        let p = match style {
            "fog" => o,
            "smoke" => premultiply([1.0, 1.0, 1.0, v]),
            "transparent" => {
                let c = flame_rgb(v);
                // spec-question: Q40 Transparent Flames clamp before saturation
                let c = if FLAMES_CLAMP_BEFORE_SATURATION {
                    c.map(|v| v.clamp(0.0, 1.0))
                } else {
                    c
                };
                let [r, g, b] = hsl_adjust(c, 0.0, 150.0, 0.0);
                // spec-question: Q39 Transparent Flames swizzle alpha mode
                match FLAMES_SWIZZLE_ALPHA {
                    Alpha::Premultiplied => [r, g, b, v],
                    Alpha::Straight => premultiply([r, g, b, v]),
                }
            }
            _ => {
                let [r, g, b] = flame_rgb(v);
                [r, g, b, o[3]]
            }
        };
        match info.output {
            Alpha::Straight => unpremultiply(p),
            Alpha::Premultiplied => p,
        }
    })
}

/// Discrete star step; shared factors produce compound figures, with all N segments drawn.
pub fn star_step(n: u32, depth: f64) -> u32 {
    (1 + (depth * (n as f64 - 0.5) / 2.0).floor() as u32).min((n - 1) / 2)
}

fn polygon_vertex(c: [f64; 2], r: f64, n: u32, angle_deg: f64, j: u32) -> [f64; 2] {
    let theta = ((angle_deg - 360.0 * j as f64 / n as f64) * std::f64::consts::PI) / 180.0;
    [c[0] + r * theta.cos(), c[1] - r * theta.sin()]
}

pub fn polygon_vertices(c: [f64; 2], r: f64, n: u32, angle_deg: f64) -> Vec<[f64; 2]> {
    (0..n)
        .map(|i| polygon_vertex(c, r, n, angle_deg, i))
        .collect()
}

/// Inclusive, truncated-endpoint major-axis DDA, y on ties and half-up minor rounding.
pub fn dda(a: [f64; 2], b: [f64; 2]) -> Vec<(i32, i32)> {
    let (mut a, mut b) = (a.map(|v| v as i32), b.map(|v| v as i32));
    let major = usize::from((b[0] - a[0]).abs() <= (b[1] - a[1]).abs());
    if a[major] > b[major] {
        std::mem::swap(&mut a, &mut b);
    }
    let count = b[major] - a[major];
    if count == 0 {
        return vec![(a[0], a[1])];
    }
    let minor = 1 - major;
    let slope = (b[minor] - a[minor]) as f64 / count as f64;
    let mut at = a[minor] as f64;
    (a[major]..=b[major])
        .map(|m| {
            let q = (at + 0.5).floor() as i32;
            at += slope;
            if major == 0 { (m, q) } else { (q, m) }
        })
        .collect()
}

fn polygon_render(src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    let n = num(values, "vertices").clamp(3.0, 100.0) as u32;
    let k = star_step(n, num(values, "depth").clamp(0.0, 1.0));
    let b = env.bounds;
    let c = [b.x as f64 + (b.w / 2) as f64, b.y as f64 + (b.h / 2) as f64];
    let r = (b.w / 2).min(b.h / 2) as f64 * num(values, "size").clamp(0.01, 1.0);
    let angle = num(values, "angle").clamp(-180.0, 180.0);
    let width = num(values, "lineWidth").clamp(1.0, 100.0) as f32;
    let aa = flag(values, "antiAlias");
    let mut color = color_value(values, "color", env);
    color[3] = 255;
    let mut out = src.clone();
    let vertices = polygon_vertices(c, r, n, angle);
    for i in 0..n {
        if width == 1.0 && !aa {
            // spec-question: Q41 Polygon thin-DDA closing endpoint evaluation
            let j = if POLYGON_DDA_UNWRAPPED_TARGET {
                i + k
            } else {
                (i + k) % n
            };
            let target = polygon_vertex(c, r, n, angle, j);
            for (x, y) in dda(vertices[i as usize], target) {
                // quirk(bb): render-polygon-stars.md Notes: safely skip off-canvas DDA writes.
                if x >= 0 && y >= 0 && x < src.width as i32 && y < src.height as i32 {
                    let at = (y as usize * src.width as usize + x as usize) * 4;
                    out.data[at..at + 4].copy_from_slice(&color);
                }
            }
        } else {
            let segment = vec![vertices[i as usize], vertices[((i + k) % n) as usize]];
            let Some(path) = polygon_path(&[segment], [0.5, 0.5], false) else {
                continue;
            };
            let pen = Pen {
                width,
                cap: Cap::Round,
                join: Join::Round,
                miter_limit: 10.0,
                dash: None,
            };
            // approximation: shared tiny-skia coverage stands in for GDI+ (render-polygon-stars.md Drawing).
            let coverage = stroke_coverage(&path, &pen, src.width, src.height, aa);
            for (p, a) in out.data.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
                if a > 0.0 {
                    let mut line = color;
                    line[3] = quantize(a);
                    *p = compose(BlendMode::Normal, *p, line, 255);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::test_util::*;
    use crate::effect::surface::{Alpha, EffectSurface, Space};
    use crate::effect::{EffectEnv, EffectKind, RenderInfo};
    use crate::pixel::gamma::{SRGB8_TO_LINEAR, linear_to_srgb8, quantize};
    use serde_json::json;

    const FLAMES: &str = "bbFlames";
    const POLYGON: &str = "bbPolygonStars";
    const STYLES: [&str; 4] = ["normal", "transparent", "fog", "smoke"];
    const CONTEXTS: [RenderInfo; 2] = [
        RenderInfo::new(Space::Srgb, Alpha::Premultiplied),
        RenderInfo::new(Space::Linear, Alpha::Premultiplied),
    ];

    fn encode(v: f32, space: Space) -> u8 {
        match space {
            Space::Srgb => quantize(v),
            Space::Linear => linear_to_srgb8(v.max(0.0)),
        }
    }
    fn decode(v: u8, space: Space) -> f32 {
        match space {
            Space::Srgb => v as f32 / 255.0,
            Space::Linear => SRGB8_TO_LINEAR[v as usize],
        }
    }
    fn overlay(g: f32, t: f32) -> f32 {
        if g <= 0.5 {
            2.0 * g * t
        } else {
            1.0 - 2.0 * (1.0 - g) * (1.0 - t)
        }
    }
    fn noise(x: u32, y: u32, w: u32, h: u32, env: &EffectEnv) -> f32 {
        crate::effect::render::clouds_value(
            x,
            y,
            w,
            h,
            30.0,
            0.5,
            ((3 ^ env.today.day_of_year) & 255) as u8,
        )
    }
    fn gradient() -> crate::canvas::ImageData {
        from_fn(8, 256, |_, y| [y as u8, y as u8, y as u8, 255])
    }
    fn near(a: [u8; 4], b: [u8; 4]) {
        assert!(
            a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= TOL),
            "{a:?} != {b:?}"
        );
    }

    /// render-flames.md Expected results 1–2: white/black invariants for all styles, seeds and date edges.
    #[test]
    fn flames_white_black() {
        for info in CONTEXTS {
            for (scale, roughness) in [(2, 0.0), (40, 0.5), (1000, 1.0)] {
                for seed in [0, 7, 255] {
                    for day in [1, 257, 366] {
                        let mut env = EffectEnv::whole(8, 8);
                        env.today.day_of_year = day;
                        for style in STYLES {
                            let mut def = crate::effect::find(FLAMES).unwrap();
                            let EffectKind::Gpu(_, f) = def.kind else {
                                panic!("GPU")
                            };
                            def.kind = EffectKind::Gpu(info, f);
                            let mut values = crate::effect::bb::indirect_defaults(&def, &env);
                            values.extend(json!({"style":style,"seed":seed,"scale":scale,"roughness":roughness}).as_object().unwrap().clone());
                            let white =
                                crate::effect::apply(&solid(8, 8, [255; 4]), &def, &values, &env);
                            for p in white.data.as_chunks::<4>().0 {
                                near(*p, [255; 4]);
                            }
                            let black = crate::effect::apply(
                                &solid(8, 8, [0, 0, 0, 255]),
                                &def,
                                &values,
                                &env,
                            );
                            for p in black.data.as_chunks::<4>().0 {
                                if matches!(style, "transparent" | "smoke") {
                                    assert_eq!(p[3], 0);
                                } else {
                                    near(*p, [0, 0, 0, 255]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// render-flames.md Expected results 3: both conditional ramp tables through the actual GPU callback.
    #[test]
    fn flames_ramp_and_effective_context() {
        for (g, gamma, linear) in [
            (0, [0, 0, 0], [0, 0, 0]),
            (128, [128, 0, 0], [128, 0, 0]),
            (160, [160, 44, 0], [160, 0, 0]),
            (192, [192, 115, 0], [192, 0, 0]),
            (204, [204, 142, 0], [204, 98, 0]),
            (224, [224, 186, 100], [224, 176, 0]),
            (240, [240, 222, 180], [240, 220, 161]),
            (255, [255; 3], [255; 3]),
        ] {
            for (info, want) in CONTEXTS.into_iter().zip([gamma, linear]) {
                let rgb = flame_rgb(decode(g, info.space)).map(|v| encode(v, info.space));
                near(
                    [rgb[0], rgb[1], rgb[2], 255],
                    [want[0], want[1], want[2], 255],
                );
                // Lattice centre has noise exactly .5, leaving the heat map unchanged.
                let out = bb_ctx(
                    FLAMES,
                    info,
                    json!({"scale":30}),
                    &solid(2, 2, [g, g, g, 255]),
                );
                assert_px(&out, 1, 1, [want[0], want[1], want[2], 255]);
            }
        }
        // R142: the specified linear matrix crosses zero between encoded 195 and 196.
        for (g, want) in [(195, [195, 0, 0, 255]), (196, [196, 19, 0, 255])] {
            let out = bb_ctx(FLAMES, CONTEXTS[1], json!({}), &solid(2, 2, [g, g, g, 255]));
            assert_px(&out, 1, 1, want);
        }
        let default = bb(FLAMES, json!({}), &solid(2, 2, [224, 224, 224, 255]));
        assert_px(&default, 1, 1, [224, 186, 100, 255]);
        for (info, green, blue) in [(CONTEXTS[0], 140, 204), (CONTEXTS[1], 195, 231)] {
            for g in 0..=255 {
                let out = bb_ctx(FLAMES, info, json!({}), &solid(2, 2, [g, g, g, 255]));
                if g <= green {
                    assert_eq!(px(&out, 1, 1)[1], 0, "green {g}");
                }
                if g <= blue {
                    assert_eq!(px(&out, 1, 1)[2], 0, "blue {g}");
                }
            }
        }
    }

    /// render-flames.md Expected results 3–5: every gradient pixel and unquantized smoke opacity oracle.
    #[test]
    fn flames_gradient_smoke_and_noise() {
        let src = gradient();
        for info in CONTEXTS {
            let v = json!({"scale":30,"seed":3});
            let fog = bb_ctx(
                FLAMES,
                info,
                json!({"style":"fog","scale":30,"seed":3}),
                &src,
            );
            let normal = bb_ctx(FLAMES, info, v, &src);
            let smoke = bb_ctx(
                FLAMES,
                info,
                json!({"style":"smoke","scale":30,"seed":3}),
                &src,
            );
            let env = EffectEnv::whole(8, 256);
            for y in 0..256 {
                for x in 0..8 {
                    let field = overlay(decode(y as u8, info.space), noise(x, y, 8, 256, &env));
                    let f = px(&fog, x, y);
                    let n = px(&normal, x, y);
                    let s = px(&smoke, x, y);
                    let gray = encode(field, info.space);
                    near(f, [gray, gray, gray, 255]);
                    assert!(f[0].abs_diff(n[0]) <= TOL);
                    let ramp = [field, 2.2173 * field - 1.2172977, 5.0 * field - 4.0]
                        .map(|v| encode(v, info.space));
                    near(n, [ramp[0], ramp[1], ramp[2], 255]);
                    assert!(s[3].abs_diff(quantize(field)) <= TOL);
                    assert!(s[3].abs_diff(quantize(decode(f[0], info.space))) <= TOL);
                    if s[3] > 0 {
                        assert_eq!(&s[..3], &[255; 3]);
                    }
                    if y == 0 {
                        near(f, [0, 0, 0, 255]);
                    }
                    if y == 255 {
                        near(f, [255; 4]);
                    }
                }
            }
            assert!(
                fog.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[0] > 40 && p[0] < 215)
            );
        }
        let linear = bb_ctx(
            FLAMES,
            CONTEXTS[1],
            json!({"style":"smoke"}),
            &solid(2, 2, [188, 188, 188, 255]),
        );
        assert_px(&linear, 1, 1, [255, 255, 255, 128]);
    }

    /// render-flames.md Expected results 5: R136 exact-half identity and separate byte128/188 quantization.
    #[test]
    fn flames_exact_half_and_byte_pipeline() {
        for info in CONTEXTS {
            let mut env = EffectEnv::whole(64, 64);
            env.render_info = Some(info);
            let def = crate::effect::find(FLAMES).unwrap();
            let mut v = crate::effect::bb::indirect_defaults(&def, &env);
            v.extend(
                json!({"style":"fog","scale":30,"seed":3})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            let EffectKind::Gpu(_, render) = def.kind else {
                panic!("GPU")
            };
            let out = render(
                &EffectSurface::from_fn(64, 64, |_, _| [0.5, 0.5, 0.5, 1.0]),
                &v,
                &env,
            );
            let g = if info.space == Space::Srgb { 128 } else { 188 };
            let bytes = bb_ctx(
                FLAMES,
                info,
                json!({"style":"fog","scale":30,"seed":3}),
                &solid(64, 64, [g, g, g, 255]),
            );
            let mut varied = false;
            for y in 0..64 {
                for x in 0..64 {
                    let t = noise(x, y, 64, 64, &env);
                    assert!((out.at(x, y)[0] - t).abs() < 0.000001);
                    let want = encode(overlay(decode(g, info.space), t), info.space);
                    assert_px(&bytes, x, y, [want, want, want, 255]);
                    varied |= (t - 0.5).abs() > 0.05;
                }
            }
            assert!(varied);
        }
    }

    /// render-flames.md Expected results 6: UTC snapshots, masked day collision and canvas-coordinate noise.
    #[test]
    fn flames_determinism_date_and_selection() {
        let src = gradient();
        let v = json!({"style":"fog","seed":3,"scale":30});
        let mut env = EffectEnv::whole(8, 256);
        env.today.day_of_year = 1;
        let first = bb_env(FLAMES, v.clone(), &src, &env);
        assert_eq!(first, bb_env(FLAMES, v.clone(), &src, &env));
        env.today.day_of_year = 257;
        assert_eq!(first, bb_env(FLAMES, v.clone(), &src, &env));
        env.today.day_of_year = 2;
        assert_ne!(first, bb_env(FLAMES, v.clone(), &src, &env));
        env.today.day_of_year = 1;
        assert_ne!(
            first,
            bb_env(
                FLAMES,
                json!({"style":"fog","seed":4,"scale":30}),
                &src,
                &env
            )
        );
        env.instance_seed = u32::MAX;
        env.primary = [250, 0, 0, 20];
        env.secondary = [0, 0, 250, 10];
        env.palette = &[];
        assert_eq!(first, bb_env(FLAMES, v.clone(), &src, &env));
        let (mask, polys) = rect_selection(8, 256, 2, 20, 7, 220);
        let selected = EffectEnv {
            mask: Some(&mask),
            selection: &polys,
            bounds: crate::pixel::IntRect::new(2, 20, 5, 200),
            ..env
        };
        let clipped = bb_env(FLAMES, v, &src, &selected);
        for y in 0..256 {
            for x in 0..8 {
                assert_eq!(
                    px(&clipped, x, y),
                    if (2..7).contains(&x) && (20..220).contains(&y) {
                        px(&first, x, y)
                    } else {
                        px(&src, x, y)
                    }
                );
            }
        }
    }

    /// render-flames.md Behaviour / R88: coloured and partial-alpha heat maps, all effective alpha modes.
    #[test]
    fn flames_grayscale_alpha_and_transparent_conventions() {
        for space in [Space::Srgb, Space::Linear] {
            for input in [Alpha::Straight, Alpha::Premultiplied] {
                for output in [Alpha::Straight, Alpha::Premultiplied] {
                    let info = RenderInfo {
                        space,
                        input,
                        output,
                    };
                    let src = from_fn(4, 4, |x, y| [40 + 30 * x as u8, 80, 200, (y * 85) as u8]);
                    for style in STYLES {
                        let out = bb_ctx(
                            FLAMES,
                            info,
                            json!({"style":style,"scale":30,"seed":3}),
                            &src,
                        );
                        for y in 0..4 {
                            for x in 0..4 {
                                let p = px(&src, x, y);
                                let a = p[3] as f32 / 255.0;
                                let gray = 0.299 * decode(p[0], space)
                                    + 0.587 * decode(p[1], space)
                                    + 0.114 * decode(p[2], space);
                                let t = noise(x, y, 4, 4, &EffectEnv::whole(4, 4));
                                let v = t * (1.0 - a) + a * overlay(gray, t);
                                let ramp = [v, 2.2173 * v - 1.2172977, 5.0 * v - 4.0];
                                let (rgb, alpha) = match style {
                                    "fog" => ([v; 3], 1.0),
                                    "normal" => (ramp, 1.0),
                                    "smoke" => ([1.0; 3], v),
                                    _ => {
                                        let c = ramp.map(|c| c.clamp(0.0, 1.0));
                                        let lo = c.into_iter().fold(f32::INFINITY, f32::min);
                                        let hi = c.into_iter().fold(f32::NEG_INFINITY, f32::max);
                                        let l = (lo + hi) / 2.0;
                                        (c.map(|c| (l + 2.5 * (c - l)) / v), v)
                                    }
                                };
                                let c = rgb.map(|c| encode(c, space));
                                near(px(&out, x, y), [c[0], c[1], c[2], quantize(alpha)]);
                            }
                        }
                    }
                }
            }
        }
    }

    fn thin(n: u32, angle: f64) -> serde_json::Value {
        json!({"vertices":n,"depth":0,"size":1.0,"angle":angle,"lineWidth":1,"antiAlias":false,"color":"#000000"})
    }
    fn exact_rows(w: u32, h: u32, rows: &[Vec<u32>]) -> crate::canvas::ImageData {
        from_fn(w, h, |x, y| {
            if rows.get(y as usize).is_some_and(|r| r.contains(&x)) {
                [0, 0, 0, 255]
            } else {
                [0; 4]
            }
        })
    }

    /// render-polygon-stars.md Expected results 1–2, amended R139: complete native 21/48 RGBA masks.
    #[test]
    fn polygon_complete_thin_masks() {
        let diamond = vec![
            vec![4, 5],
            vec![3, 6],
            vec![2, 7],
            vec![2, 8],
            vec![1, 9],
            vec![0, 10],
            vec![1, 9],
            vec![2, 8],
            vec![3, 7],
            vec![4, 6],
            vec![5],
        ];
        assert_eq!(
            bb(POLYGON, thin(4, 90.0), &solid(11, 11, [0; 4])),
            exact_rows(11, 11, &diamond)
        );
        let mut triangle = vec![
            vec![9, 10],
            vec![8, 11],
            vec![8, 11],
            vec![7, 12],
            vec![7, 12],
            vec![6, 13],
            vec![6, 13],
            vec![5, 14],
            vec![5, 14],
            vec![4, 15],
            vec![4, 15],
            vec![3, 16],
            vec![3, 16],
            vec![2, 17],
            vec![2, 17],
        ];
        triangle.push((1..=18).collect());
        assert_eq!(
            bb(POLYGON, thin(3, 90.0), &solid(21, 21, [0; 4])),
            exact_rows(21, 21, &triangle)
        );
    }

    /// render-polygon-stars.md Behaviour: exact DDA ties, endpoint order, truncation and half-up rounding.
    #[test]
    fn polygon_dda_rules() {
        for (a, b, want) in [
            (
                [0.9, 0.9],
                [4.9, 2.9],
                vec![(0, 0), (1, 1), (2, 1), (3, 2), (4, 2)],
            ),
            (
                [0.0, 0.0],
                [2.0, 4.0],
                vec![(0, 0), (1, 1), (1, 2), (2, 3), (2, 4)],
            ),
            ([2.0, 0.0], [0.0, 2.0], vec![(2, 0), (1, 1), (0, 2)]),
            ([-1.9, -1.9], [-1.1, -1.1], vec![(-1, -1)]),
            (
                [-2.0, -1.0],
                [2.0, 1.0],
                vec![(-2, -1), (-1, 0), (0, 0), (1, 1), (2, 1)],
            ),
        ] {
            assert_eq!(dda(a, b), want);
            assert_eq!(dda(b, a), want);
        }
    }

    /// render-polygon-stars.md Expected results 3: all vertex centres, outline-only geometry, AA and black top.
    #[test]
    fn polygon_default_pentagon() {
        let vertices = polygon_vertices([100.0, 50.0], 45.0, 5, 90.0);
        for (got, want) in vertices.iter().zip([
            [100.0, 5.0],
            [142.80, 36.09],
            [126.45, 86.41],
            [73.55, 86.41],
            [57.20, 36.09],
        ]) {
            assert!((got[0] - want[0]).abs() < 0.01 && (got[1] - want[1]).abs() < 0.01);
        }
        let out = bb(
            POLYGON,
            json!({"color":"#000000"}),
            &solid(200, 100, [255; 4]),
        );
        assert_eq!(px(&out, 100, 5), [0, 0, 0, 255]);
        assert_eq!(px(&out, 100, 50), [255; 4]);
        assert_eq!(px(&out, 100, 95), [255; 4]);
        assert!(
            out.data
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] > 20 && p[0] < 235)
        );
        assert!(px(&out, 100, 86)[0] < 80);
        // Independent published centres, distance from each finite segment, clear exterior/interior.
        let points = [
            [100.0, 5.0],
            [142.80, 36.09],
            [126.45, 86.41],
            [73.55, 86.41],
            [57.20, 36.09],
        ];
        for y in 0..100 {
            for x in 0..200 {
                let d = (0..5)
                    .map(|i| distance([x as f64, y as f64], points[i], points[(i + 1) % 5]))
                    .fold(f64::INFINITY, f64::min);
                if d > 1.6 {
                    assert_eq!(px(&out, x, y), [255; 4], "{x},{y} distance {d}");
                }
            }
        }
    }
    fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
        let d = [b[0] - a[0], b[1] - a[1]];
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]))
            .clamp(0.0, 1.0);
        (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
    }
    fn painted_near(img: &crate::canvas::ImageData, x: f64, y: f64) -> bool {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let x = x.round() as i32 + dx;
                let y = y.round() as i32 + dy;
                x >= 0
                    && y >= 0
                    && x < img.width as i32
                    && y < img.height as i32
                    && px(img, x as u32, y as u32)[0] < 80
            })
        })
    }

    /// render-polygon-stars.md Expected results 4: every tabulated depth interval and rendered compound cycles.
    #[test]
    fn polygon_star_steps_and_rendered_segments() {
        for (n, thresholds) in [
            (3, vec![]),
            (4, vec![]),
            (5, vec![45]),
            (6, vec![37]),
            (7, vec![31, 62]),
            (8, vec![27, 54]),
            (9, vec![24, 48, 71]),
            (10, vec![22, 43, 64]),
            (12, vec![18, 35, 53, 70]),
        ] {
            for hundredth in 0..=100 {
                assert_eq!(
                    star_step(n, hundredth as f64 / 100.0),
                    1 + thresholds.iter().filter(|&&t| hundredth >= t).count() as u32,
                    "N{n} depth{hundredth}"
                );
            }
        }
        let pent = [
            [100.0, 5.0],
            [142.7975, 36.0942],
            [126.4503, 86.4058],
            [73.5497, 86.4058],
            [57.2025, 36.0942],
        ];
        let oct = [
            [100.0, 5.0],
            [131.8198, 18.1802],
            [145.0, 50.0],
            [131.8198, 81.8198],
            [100.0, 95.0],
            [68.1802, 81.8198],
            [55.0, 50.0],
            [68.1802, 18.1802],
        ];
        let mut images = Vec::new();
        for (points, d, k) in [
            (&pent[..], 0.44, 1),
            (&pent[..], 0.45, 2),
            (&oct[..], 0.5, 2),
            (&oct[..], 1.0, 3),
        ] {
            let out = bb(
                POLYGON,
                json!({"vertices":points.len(),"depth":d,"color":"#000000"}),
                &solid(200, 100, [255; 4]),
            );
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + k) % points.len()];
                assert!(
                    painted_near(&out, (a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0),
                    "N{} d{d} segment{i}",
                    points.len()
                );
            }
            assert_eq!(px(&out, 100, 50), [255; 4]);
            images.push(out);
        }
        assert_ne!(images[0], images[1]);
        assert_ne!(images[2], images[3]);
    }

    /// render-polygon-stars.md Expected results 5: actual thin rotations and axis-aligned square.
    #[test]
    fn polygon_rotations() {
        for (angle, tip, empty) in [
            (90.0, (10, 0), (10, 20)),
            (-90.0, (10, 20), (10, 0)),
            (0.0, (20, 10), (0, 10)),
        ] {
            let out = bb(POLYGON, thin(3, angle), &solid(21, 21, [0; 4]));
            assert_eq!(px(&out, tip.0, tip.1), [0, 0, 0, 255]);
            assert_eq!(px(&out, empty.0, empty.1), [0; 4]);
            assert_eq!(px(&out, 10, 10), [0; 4]);
        }
        let out = bb(POLYGON, thin(4, 45.0), &solid(21, 21, [0; 4]));
        for y in 0..21 {
            for x in 0..21 {
                let on = ((x == 2 || x == 17) && (2..=17).contains(&y))
                    || ((y == 2 || y == 17) && (2..=17).contains(&x));
                assert_eq!(
                    px(&out, x, y),
                    if on { [0, 0, 0, 255] } else { [0; 4] },
                    "{x},{y}"
                );
            }
        }
        let p = polygon_vertices([0.0; 2], 10.0, 4, 45.0);
        assert!(((p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]) - 10.0 * 2f64.sqrt()).abs() < 1e-10);
    }

    /// render-polygon-stars.md Expected results 6: selected centre/radius, stored outside RGBA and fractional clipping once.
    #[test]
    fn polygon_selection_bounds_and_clip() {
        let src = solid(100, 100, [210, 170, 90, 120]);
        let (mask, polys) = rect_selection(100, 100, 20, 10, 60, 30);
        let env = with_selection(100, 100, &mask, &polys);
        let out = bb_env(POLYGON, json!({"color":"#000000"}), &src, &env);
        assert_eq!(px(&out, 40, 11), [0, 0, 0, 255]);
        assert_eq!(px(&out, 40, 20), px(&src, 40, 20));
        for y in 0..100 {
            for x in 0..100 {
                if !(20..60).contains(&x) || !(10..30).contains(&y) {
                    assert_eq!(px(&out, x, y), px(&src, x, y));
                }
            }
        }
        let (mut mask, polys) = ellipse_selection(40, 40, [20.0, 20.0], [16.0, 10.0]);
        for a in &mut mask.data {
            if *a > 0 {
                *a = 128;
            }
        }
        let env = with_selection(40, 40, &mask, &polys);
        let src = solid(40, 40, [90, 140, 210, 64]);
        let def = crate::effect::find(POLYGON).unwrap();
        let v = crate::effect::bb::indirect_defaults(&def, &env);
        let EffectKind::Cpu { render, .. } = def.kind else {
            panic!("CPU")
        };
        let raw = render(&src, &v, &env);
        let out = crate::effect::apply(&src, &def, &v, &env);
        assert_ne!(raw, out);
        for y in 0..40 {
            for x in 0..40 {
                let want = crate::pixel::ops::blend(
                    px(&src, x, y),
                    px(&raw, x, y),
                    mask.get(x as i32, y as i32),
                );
                assert_eq!(px(&out, x, y), want);
            }
        }
        for (w, h) in [(1, 1), (1, 7), (8, 8), (9, 8)] {
            let src = solid(w, h, [1, 2, 3, 0]);
            let out = bb(POLYGON, thin(4, 0.0), &src);
            assert_eq!(out.data.len(), src.data.len());
            assert_eq!(out, bb(POLYGON, thin(4, 0.0), &src));
        }
    }

    /// render-polygon-stars.md drawing: AA/width branches, round caps, opaque colour and source-over.
    #[test]
    fn polygon_strokes_and_opaque_color() {
        for (width, aa) in [(1, true), (2, false), (8, true)] {
            let v = json!({"vertices":3,"size":0.6,"lineWidth":width,"antiAlias":aa,"color":"#12345620"});
            let src = solid(41, 41, [7, 8, 9, 0]);
            let out = bb(POLYGON, v.clone(), &src);
            assert!(out.data.as_chunks::<4>().0.contains(&[18, 52, 86, 255]));
            assert_eq!(px(&out, 0, 0), [7, 8, 9, 0]);
            assert_eq!(px(&out, 20, 20), [7, 8, 9, 0]);
            for p in out.data.as_chunks::<4>().0.iter().filter(|p| p[3] > 0) {
                assert_eq!(&p[..3], &[18, 52, 86]);
            }
            if aa {
                assert!(
                    out.data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[3] > 0 && p[3] < 255)
                );
            } else {
                assert!(
                    out.data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[3] == 0 || p[3] == 255)
                );
            }
            if width == 8 {
                assert!(px(&out, 20, 5)[3] > 0);
                assert_eq!(px(&out, 20, 3)[3], 0);
            }
            let mut env = EffectEnv::whole(41, 41);
            env.instance_seed = 55;
            env.primary = [18, 52, 86, 1];
            let mut v = v;
            v["color"] = json!("primary");
            assert_eq!(out, bb_env(POLYGON, v, &src, &env));
        }
    }
    /// render-flames.md and render-polygon-stars.md Controls: complete registered dialog contract.
    #[test]
    fn control_metadata() {
        use crate::effect::bb::ui::{Help, HelpWindow, Note};
        use crate::effect::{Category, ColorDefault, Control};
        for (id, title, icon, keys, labels, older) in [
            (
                FLAMES,
                "Render Flames",
                "burninate.png",
                vec!["scale", "roughness", "style", "seed"],
                vec!["Scale", "Roughness", "", "Randomize"],
                false,
            ),
            (
                POLYGON,
                "Polygon/Stars",
                "polygon.png",
                vec![
                    "vertices",
                    "depth",
                    "size",
                    "angle",
                    "lineWidth",
                    "color",
                    "antiAlias",
                ],
                vec![
                    "Verticies",
                    "Star depth",
                    "Size/Orientation",
                    "",
                    "Line width",
                    "",
                    "Anti-Aliased",
                ],
                true,
            ),
        ] {
            let def = crate::effect::find(id).unwrap();
            assert_eq!(def.dialog_title, Some(title));
            assert_eq!(def.icon, icon);
            assert_eq!(def.category, Category::Effect("render"));
            assert_eq!(
                def.controls.iter().map(Control::key).collect::<Vec<_>>(),
                keys
            );
            assert_eq!(
                def.controls
                    .iter()
                    .map(|c| match c {
                        Control::Range { label, .. }
                        | Control::Select { label, .. }
                        | Control::Color { label, .. }
                        | Control::Checkbox { label, .. } => *label,
                        _ => panic!("unexpected control"),
                    })
                    .collect::<Vec<_>>(),
                labels
            );
            let ui = def.indirect.unwrap();
            assert_eq!(ui.width, 1.0);
            assert!(ui.tabs.is_empty());
            assert!(
                matches!(ui.help,Help::Window(w) if w==if older {HelpWindow::Older} else {HelpWindow::Standard})
            );
            for key in keys {
                assert!((ui.enabled)(&crate::effect::defaults(&def), key));
            }
        }
        let f = crate::effect::find(FLAMES).unwrap();
        let p = crate::effect::find(POLYGON).unwrap();
        for (def, key, label, min, max, default, step, precision, small, large) in [
            (&f, "scale", "Scale", 2.0, 1000.0, 250.0, 1.0, 0, 1.0, 5.0),
            (
                &f,
                "roughness",
                "Roughness",
                0.0,
                1.0,
                0.5,
                0.01,
                3,
                0.05,
                0.25,
            ),
            (&f, "seed", "Randomize", 0.0, 255.0, 0.0, 1.0, 0, 1.0, 5.0),
            (
                &p,
                "vertices",
                "Verticies",
                3.0,
                100.0,
                5.0,
                1.0,
                0,
                1.0,
                5.0,
            ),
            (
                &p,
                "depth",
                "Star depth",
                0.0,
                1.0,
                0.0,
                0.01,
                2,
                0.05,
                0.25,
            ),
            (
                &p,
                "size",
                "Size/Orientation",
                0.01,
                1.0,
                0.9,
                0.01,
                2,
                0.05,
                0.25,
            ),
            (&p, "angle", "", -180.0, 180.0, 90.0, 1.0, 2, 1.0, 5.0),
            (
                &p,
                "lineWidth",
                "Line width",
                1.0,
                100.0,
                2.0,
                1.0,
                0,
                1.0,
                5.0,
            ),
        ] {
            let Control::Range {
                label: l,
                min: a,
                max: b,
                default: d,
                step: s,
                keyboard,
                exponential,
                ..
            } = def.controls.iter().find(|c| c.key() == key).unwrap()
            else {
                panic!("range {key}")
            };
            assert_eq!((*l, *a, *b, *d, *s), (label, min, max, default, step));
            assert!(!exponential && !keyboard.exponential);
            assert_eq!(
                (keyboard.decimals, keyboard.small, keyboard.large),
                (precision, small, large)
            );
        }
        let fui = f.indirect.unwrap();
        for key in ["scale", "roughness", "seed"] {
            assert!(!fui.control(key).header_line);
        }
        assert_eq!(fui.control("roughness").decimals, Some(3));
        assert!(matches!(
            fui.control("seed").note,
            Note::Text(
                "HINT: Start by drawing a gradient from black to white before running this effect. Try different gradient shapes to vary the flames."
            )
        ));
        assert!(
            matches!(&f.controls[2],Control::Select {label:"",default:"normal",choices,..} if *choices==[("normal","Normal Flames"),("transparent","Transparent Flames"),("fog","Fog/Smoke"),("smoke","Transparent Smoke/Steam")])
        );
        let pui = p.indirect.unwrap();
        assert!(pui.control("angle").wrap);
        assert_eq!(pui.control("angle").decimals, Some(2));
        assert!(!pui.control("color").alpha && !pui.control("color").reset);
        assert!(matches!(
            p.controls[5],
            Control::Color {
                label: "",
                default: ColorDefault::Primary,
                ..
            }
        ));
        assert!(matches!(
            p.controls[6],
            Control::Checkbox {
                label: "Anti-Aliased",
                default: true,
                ..
            }
        ));
    }

    /// render-polygon-stars.md Expected results 1–2 / R139: wrapped 20/47 alternate-convention diagnostics.
    #[test]
    fn polygon_wrapped_diagnostics() {
        for (n, w, count) in [(4, 11, 20), (3, 21, 47)] {
            let v = polygon_vertices([(w / 2) as f64; 2], (w / 2) as f64, n, 90.0);
            let mut out = solid(w, w, [0; 4]);
            for i in 0..n {
                for (x, y) in dda(v[i as usize], v[((i + 1) % n) as usize]) {
                    if x >= 0 && y >= 0 && x < w as i32 && y < w as i32 {
                        let at = (y as usize * w as usize + x as usize) * 4;
                        out.data[at..at + 4].copy_from_slice(&[0, 0, 0, 255]);
                    }
                }
            }
            assert_eq!(
                out.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] == 255)
                    .count(),
                count
            );
            if n == 4 {
                for y in 0..w {
                    for x in 0..w {
                        assert_eq!(
                            px(&out, x, y)[3] == 255,
                            (x as i32 - 5).abs() + (y as i32 - 5).abs() == 5
                        );
                    }
                }
            }
            assert_ne!(out, bb(POLYGON, thin(n, 90.0), &solid(w, w, [0; 4])));
        }
    }
    /// render-polygon-stars.md Notes / drawing: skip exclusive-edge writes and compose partial coverage over source.
    #[test]
    fn polygon_off_canvas_skips_and_partial_source_over() {
        let src = solid(10, 10, [17, 23, 41, 0]);
        let out = bb(POLYGON, thin(4, 0.0), &src);
        assert_eq!(px(&out, 9, 5), px(&src, 9, 5));
        assert_eq!(px(&out, 5, 9), px(&src, 5, 9));
        assert_eq!(px(&out, 9, 6), [0, 0, 0, 255]);
        let values = json!({"vertices":4,"angle":45,"size":0.6,"lineWidth":1,"color":"#123456","antiAlias":true});
        let coverage = bb(POLYGON, values.clone(), &solid(41, 41, [0; 4]));
        let out = bb(POLYGON, values, &solid(41, 41, [210, 170, 90, 128]));
        let q = px(&coverage, 28, 20);
        assert!(q[3] > 0 && q[3] < 255);
        let want = crate::pixel::ops::compose(
            crate::document::layer::BlendMode::Normal,
            [210, 170, 90, 128],
            q,
            255,
        );
        assert_eq!(px(&out, 28, 20), want);
    }
}
