//! Photocraft phase spec: S§4.2; research R§B.2 (Adjustments)
//!
//! The Extra adjustments: Vibrance, Threshold, Gradient Map, Channel Mixer, Photo Filter and
//! Selective Color (`extra::fx::adjust`).

use crate::canvas::ImageData;
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, choice, color_value,
    flag, num, range,
};
use crate::extra::{fx, shim};
use std::sync::LazyLock;

const FAMILIES: [(&str, &str); 9] = [
    ("reds", "Reds"),
    ("yellows", "Yellows"),
    ("greens", "Greens"),
    ("cyans", "Cyans"),
    ("blues", "Blues"),
    ("magentas", "Magentas"),
    ("whites", "Whites"),
    ("neutrals", "Neutrals"),
    ("blacks", "Blacks"),
];

/// The presets, then "Color" for the colour box.
static FILTERS: LazyLock<Vec<(&str, &str)>> = LazyLock::new(|| {
    fx::adjust::PHOTO_FILTERS
        .iter()
        .map(|f| (f.0, f.1))
        .chain([("custom", "Color")])
        .collect()
});

const MIXER_KEYS: [[&str; 4]; 3] = [
    ["rr", "rg", "rb", "rc"],
    ["gr", "gg", "gb", "gc"],
    ["br", "bg", "bb", "bc"],
];
const MIXER_LABELS: [[&str; 4]; 3] = [
    [
        "Red from Red",
        "Red from Green",
        "Red from Blue",
        "Red constant",
    ],
    [
        "Green from Red",
        "Green from Green",
        "Green from Blue",
        "Green constant",
    ],
    [
        "Blue from Red",
        "Blue from Green",
        "Blue from Blue",
        "Blue constant",
    ],
];

fn def(id: &'static str, controls: Vec<Control>, render: RenderFn) -> EffectDef {
    let m = super::manifest::manifest(id).expect("manifest row");
    EffectDef {
        id,
        icon: m.icon,
        category: Category::Adjustment,
        controls,
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: None,
    }
}

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

pub fn effects() -> Vec<EffectDef> {
    let mut mixer = Vec::new();
    for (o, row) in MIXER_KEYS.iter().enumerate() {
        for (i, key) in row.iter().enumerate() {
            let default = if i == o { 100.0 } else { 0.0 };
            mixer.push(range(key, MIXER_LABELS[o][i], -200.0, 200.0, default));
        }
    }
    mixer.push(Control::Checkbox {
        key: "monochrome",
        label: "Monochrome",
        default: false,
    });
    vec![
        def(
            "pcVibrance",
            vec![
                range("vibrance", "Vibrance", -100.0, 100.0, 0.0),
                range("saturation", "Saturation", -100.0, 100.0, 0.0),
            ],
            vibrance,
        ),
        def(
            "pcThreshold",
            vec![range("level", "Level", 1.0, 255.0, 128.0)],
            threshold,
        ),
        def(
            "pcGradientMap",
            vec![
                Control::Color {
                    key: "from",
                    label: "Start color",
                    default: ColorDefault::Primary,
                },
                Control::Color {
                    key: "to",
                    label: "End color",
                    default: ColorDefault::Secondary,
                },
                Control::Checkbox {
                    key: "reverse",
                    label: "Reverse",
                    default: false,
                },
                Control::Checkbox {
                    key: "dither",
                    label: "Dither",
                    default: false,
                },
            ],
            gradient_map,
        ),
        def("pcChannelMixer", mixer, channel_mixer),
        def(
            "pcPhotoFilter",
            vec![
                Control::Select {
                    key: "filter",
                    label: "Filter",
                    default: "warming85",
                    choices: FILTERS.as_slice(),
                },
                Control::Color {
                    key: "color",
                    label: "Color",
                    default: ColorDefault::Rgb(0xEC8A00),
                },
                range("density", "Density", 1.0, 100.0, 25.0),
                Control::Checkbox {
                    key: "preserveLuminosity",
                    label: "Preserve Luminosity",
                    default: true,
                },
            ],
            photo_filter,
        ),
        // ponytail: one colour family per run (36 sliders is no dialog); apply again for others.
        def(
            "pcSelectiveColor",
            vec![
                Control::Select {
                    key: "colors",
                    label: "Colors",
                    default: "reds",
                    choices: &FAMILIES,
                },
                range("cyan", "Cyan", -100.0, 100.0, 0.0),
                range("magenta", "Magenta", -100.0, 100.0, 0.0),
                range("yellow", "Yellow", -100.0, 100.0, 0.0),
                range("black", "Black", -100.0, 100.0, 0.0),
                Control::Select {
                    key: "method",
                    label: "Method",
                    default: "relative",
                    choices: &[("relative", "Relative"), ("absolute", "Absolute")],
                },
            ],
            selective_color,
        ),
        def("pcEqualize", vec![], equalize),
        def("pcAutoContrast", vec![], auto_contrast),
        def("pcAutoColor", vec![], auto_color),
    ]
}

/// Runs `f` over the layer as straight RGBA f32.
fn through(src: &ImageData, f: impl FnOnce(&mut [f32])) -> ImageData {
    let mut buf = shim::to_f32(src);
    f(&mut buf);
    shim::from_f32(&buf, src.width, src.height)
}

fn vibrance(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    through(src, |b| {
        fx::adjust::vibrance(
            b,
            num(v, "vibrance") as f32 / 100.0,
            num(v, "saturation") as f32 / 100.0,
        )
    })
}

fn threshold(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    through(src, |b| fx::adjust::threshold(b, num(v, "level") as f32))
}

fn rgb(p: [u8; 4]) -> [f32; 3] {
    [p[0], p[1], p[2]].map(|c| f32::from(c) / 255.0)
}

fn gradient_map(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let stops = [
        (0.0, rgb(color_value(v, "from", env))),
        (1.0, rgb(color_value(v, "to", env))),
    ];
    through(src, |b| {
        fx::adjust::gradient_map(
            b,
            src.width as usize,
            &stops,
            flag(v, "reverse"),
            flag(v, "dither"),
        )
    })
}

fn channel_mixer(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let mut m: [[f32; 4]; 3] =
        std::array::from_fn(|o| std::array::from_fn(|i| num(v, MIXER_KEYS[o][i]) as f32 / 100.0));
    let mono = flag(v, "monochrome");
    // Photoshop's grey mix, while the Red row is still the untouched identity.
    if mono && m[0] == [1.0, 0.0, 0.0, 0.0] {
        m[0] = [0.4, 0.4, 0.2, 0.0];
    }
    through(src, |b| fx::adjust::channel_mixer(b, &m, mono))
}

fn photo_filter(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let color = match fx::adjust::PHOTO_FILTERS
        .iter()
        .find(|f| f.0 == choice(v, "filter"))
    {
        Some(f) => f.2.map(|c| f32::from(c) / 255.0),
        None => rgb(color_value(v, "color", env)),
    };
    through(src, |b| {
        fx::adjust::photo_filter(
            b,
            color,
            num(v, "density") as f32 / 100.0,
            flag(v, "preserveLuminosity"),
        )
    })
}

fn selective_color(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let mut adj = [[0.0f32; 4]; 9];
    if let Some(row) = FAMILIES.iter().position(|f| f.0 == choice(v, "colors")) {
        adj[row] = ["cyan", "magenta", "yellow", "black"].map(|k| num(v, k) as f32);
    }
    through(src, |b| {
        fx::adjust::selective_color(b, choice(v, "method") != "absolute", &adj)
    })
}

/// Histograms over the selection: the pixels inside `env.bounds` the mask selects.
fn auto(
    src: &ImageData,
    env: &EffectEnv,
    f: fn(&[[u64; 256]; 3]) -> [fx::auto::Levels; 3],
) -> ImageData {
    through(src, |b| {
        let (w, b0) = (src.width as usize, env.bounds);
        let inc: Vec<bool> = shim::mask_bool(env)
            .into_iter()
            .enumerate()
            .map(|(i, k)| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                k && x >= b0.x && y >= b0.y && x < b0.x + b0.w as i32 && y < b0.y + b0.h as i32
            })
            .collect();
        fx::auto::apply_levels(b, &f(&fx::auto::histograms(b, &inc)));
    })
}

fn auto_contrast(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
    auto(src, env, fx::auto::auto_contrast_levels)
}

fn auto_color(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
    auto(src, env, fx::auto::auto_color_levels)
}

fn equalize(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
    let flags = shim::mask_bool(env);
    let w = src.width as usize;
    let b0 = env.bounds;
    through(src, |b| {
        let inc: Vec<bool> = flags
            .iter()
            .enumerate()
            .map(|(i, &k)| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                k && x >= b0.x && y >= b0.y && x < b0.x + b0.w as i32 && y < b0.y + b0.h as i32
            })
            .collect();
        let h = fx::auto::histograms(b, &inc);
        fx::auto::equalize(b, &h);
    })
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, solid};
    use crate::effect::{EffectEnv, apply, defaults, find};
    use serde_json::json;

    fn run(
        id: &str,
        img: &crate::canvas::ImageData,
        patch: serde_json::Value,
    ) -> crate::canvas::ImageData {
        let def = find(id).unwrap();
        let mut v = defaults(&def);
        for (k, x) in patch.as_object().unwrap() {
            v.insert(k.clone(), x.clone());
        }
        apply(img, &def, &v, &EffectEnv::whole(img.width, img.height))
    }

    fn sat(p: [u8; 4]) -> f32 {
        let mx = f32::from(p[0].max(p[1]).max(p[2]));
        let mn = f32::from(p[0].min(p[1]).min(p[2]));
        if mx == 0.0 { 0.0 } else { (mx - mn) / mx }
    }

    fn luma(p: [u8; 4]) -> f32 {
        0.3 * f32::from(p[0]) + 0.59 * f32::from(p[1]) + 0.11 * f32::from(p[2])
    }

    #[test]
    fn vibrance_boosts_muted_more_than_saturated() {
        let img = from_fn(2, 1, |x, _| {
            if x == 0 {
                [140, 120, 110, 255]
            } else {
                [250, 20, 20, 255]
            }
        });
        let out = run("pcVibrance", &img, json!({"vibrance": 50.0}));
        let gain = |x| sat(px(&out, x, 0)) - sat(px(&img, x, 0));
        assert!(gain(0) > gain(1), "{} {}", gain(0), gain(1));
        assert_eq!(run("pcVibrance", &img, json!({})).data, img.data);
    }

    #[test]
    fn threshold_outputs_black_or_white() {
        let img = from_fn(16, 16, |x, y| {
            [(x * 16) as u8, (y * 16) as u8, ((x + y) * 8) as u8, 255]
        });
        let out = run("pcThreshold", &img, json!({"level": 100.0}));
        for (x, y) in (0..16).flat_map(|y| (0..16).map(move |x| (x, y))) {
            let p = px(&out, x, y);
            assert!(p[..3].iter().all(|&c| c == p[0]) && (p[0] == 0 || p[0] == 255));
            assert_eq!(p[0] == 255, luma(px(&img, x, y)) > 99.5, "({x},{y})");
        }
    }

    #[test]
    fn gradient_map_endpoints() {
        let img = from_fn(2, 1, |x, _| {
            if x == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let def = find("pcGradientMap").unwrap();
        let mut env = EffectEnv::whole(2, 1);
        env.primary = [10, 20, 30, 255];
        env.secondary = [200, 150, 100, 255];
        let out = apply(&img, &def, &defaults(&def), &env);
        assert_eq!(px(&out, 0, 0), [10, 20, 30, 255]);
        assert_eq!(px(&out, 1, 0), [200, 150, 100, 255]);
    }

    #[test]
    fn channel_mixer_identity_and_swap() {
        let img = from_fn(4, 4, |x, y| [(x * 60) as u8, (y * 60) as u8, 77, 200]);
        assert_eq!(run("pcChannelMixer", &img, json!({})).data, img.data);
        let out = run("pcChannelMixer", &img, json!({"rr": 0.0, "rg": 100.0}));
        for (x, y) in (0..4).flat_map(|y| (0..4).map(move |x| (x, y))) {
            let (s, o) = (px(&img, x, y), px(&out, x, y));
            assert_eq!(o, [s[1], s[1], s[2], s[3]]);
        }
    }

    #[test]
    fn photo_filter_preserves_luminosity() {
        let img = from_fn(4, 4, |x, y| [(x * 70) as u8, (y * 70) as u8, 120, 255]);
        let out = run(
            "pcPhotoFilter",
            &img,
            json!({"filter": "deepBlue", "density": 60.0}),
        );
        assert_ne!(out.data, img.data);
        for (x, y) in (0..4).flat_map(|y| (0..4).map(move |x| (x, y))) {
            let d = (luma(px(&out, x, y)) - luma(px(&img, x, y))).abs();
            assert!(d <= 1.0, "({x},{y}) {d}");
        }
    }

    #[test]
    fn selective_color_touches_only_its_family() {
        let img = from_fn(2, 1, |x, _| {
            if x == 0 {
                [200, 40, 40, 255]
            } else {
                [40, 40, 200, 255]
            }
        });
        let out = run(
            "pcSelectiveColor",
            &img,
            json!({"colors": "reds", "cyan": -100.0}),
        );
        assert_ne!(px(&out, 0, 0), px(&img, 0, 0));
        assert_eq!(px(&out, 1, 0), px(&img, 1, 0));
        let _ = solid(1, 1, [0; 4]);
    }

    fn hue(p: [u8; 4]) -> f32 {
        let [r, g, b] = [p[0], p[1], p[2]].map(f32::from);
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let d = mx - mn;
        let h = if mx == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h * 60.0
    }

    #[test]
    fn equalize_flattens_histogram() {
        let img = from_fn(8, 8, |x, _| {
            let v = if x < 4 { 90 } else { 160 };
            [v, v, v, 255]
        });
        let out = run("pcEqualize", &img, json!({}));
        for x in 0..8 {
            let want = if x < 4 { 0 } else { 255 };
            assert_eq!(px(&out, x, 3), [want, want, want, 255]);
        }
    }

    #[test]
    fn auto_contrast_keeps_hue() {
        let img = from_fn(32, 4, |x, _| {
            let t = x as f32 / 31.0;
            [
                (120.0 + 60.0 * t) as u8,
                (80.0 + 40.0 * t) as u8,
                (40.0 + 20.0 * t) as u8,
                255,
            ]
        });
        let out = run("pcAutoContrast", &img, json!({}));
        assert!(px(&out, 31, 0)[0] > px(&img, 31, 0)[0] + 20, "no stretch");
        for x in 4..28 {
            let d = (hue(px(&out, x, 0)) - hue(px(&img, x, 0))).abs();
            assert!(d <= 2.0, "x={x} {d}");
        }
    }

    #[test]
    fn auto_color_neutralises_cast() {
        let img = from_fn(32, 1, |x, _| {
            let g = 20.0 + x as f32 * 7.0;
            [(g * 0.8) as u8, g as u8, (g * 1.1).min(255.0) as u8, 255]
        });
        let o = px(&run("pcAutoColor", &img, json!({})), 16, 0);
        let (mx, mn) = (o[0].max(o[1]).max(o[2]), o[0].min(o[1]).min(o[2]));
        assert!(mx - mn <= 3, "{o:?}");
    }

    #[test]
    fn auto_adjust_histogram_follows_selection() {
        use crate::pixel::IntRect;
        let img = from_fn(8, 1, |x, _| {
            let v = if x < 4 { 100 } else { 10 };
            [v, v, v, 255]
        });
        let def = find("pcEqualize").unwrap();
        let mut env = EffectEnv::whole(8, 1);
        env.bounds = IntRect::new(0, 0, 4, 1);
        let out = apply(&img, &def, &defaults(&def), &env);
        // Only the 100s were measured: a single value keeps its identity map.
        assert_eq!(px(&out, 0, 0), [100, 100, 100, 255]);
    }
}
