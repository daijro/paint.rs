//! BB spec: effects/adjustments-black-and-white-plus.md;
//! effects/adjustments-color-balance.md; effects/adjustments-combined-adjustments.md;
//! effects/adjustments-hue-saturation-plus.md; effects/adjustments-transparency.md; common.md §5.9

use super::blocks::{
    bc_shift, colour_exact, colour_raw, d2d_blend, d2d_grayscale, d2d_sepia, hsl_adjust,
    rgb_to_hsl, swap_alpha, temp_tint_linear,
};
use super::color_value_alpha;
use super::conventions::{BW_FLOOD_COLOUR_MANAGED, GPU_CONTEXT, HUE_PREMULTIPLIED_INTERMEDIATES};
use super::manifest::manifest;
use super::ui::*;
use crate::effect::mix::mix_at;
use crate::effect::surface::{Alpha, EffectSurface, Space, luma, premultiply, unpremultiply};
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, GpuFn, RenderInfo, Values,
    choice, flag, num, range,
};
use crate::pixel::gamma::{linear_to_srgb_f32, srgb_to_linear_f32};
use std::sync::LazyLock;

static CYAN_RED: LazyLock<String> = LazyLock::new(|| format!("Cyan{}Red", " ".repeat(54)));
static MAGENTA_GREEN: LazyLock<String> =
    LazyLock::new(|| format!("Magenta{}Green", " ".repeat(44)));
static YELLOW_BLUE: LazyLock<String> = LazyLock::new(|| format!("Yellow{}Blue", " ".repeat(50)));
static TRANSPARENT_OPAQUE: LazyLock<String> =
    LazyLock::new(|| format!("Transparent{}Opaque", " ".repeat(36)));

const METHODS: [(&str, &str); 6] = [
    ("paintNet", "Paint.NET Method"),
    ("luminosity", "Luminosity Method"),
    ("average", "Average Method"),
    ("lightness", "Lightness Method"),
    ("maximum", "Maximum Method"),
    ("minimum", "Minimum Method"),
];
const BLENDS: [(&str, &str); 27] = [
    ("None", "None"),
    ("Multiply", "Multiply"),
    ("Screen", "Screen"),
    ("Darken", "Darken"),
    ("Lighten", "Lighten"),
    ("Dissolve", "Dissolve"),
    ("ColorBurn", "ColorBurn"),
    ("LinearBurn", "LinearBurn"),
    ("DarkerColor", "DarkerColor"),
    ("LighterColor", "LighterColor"),
    ("ColorDodge", "ColorDodge"),
    ("LinearDodge", "LinearDodge"),
    ("Overlay", "Overlay"),
    ("SoftLight", "SoftLight"),
    ("HardLight", "HardLight"),
    ("VividLight", "VividLight"),
    ("LinearLight", "LinearLight"),
    ("PinLight", "PinLight"),
    ("HardMix", "HardMix"),
    ("Difference", "Difference"),
    ("Exclusion", "Exclusion"),
    ("Hue", "Hue"),
    ("Saturation", "Saturation"),
    ("Color", "Color"),
    ("Luminosity", "Luminosity"),
    ("Subtract", "Subtract"),
    ("Division", "Division"),
];
const MAGENTA_GREEN_TRACK: [u32; 3] = [0xFF00FF, 0xFFFFFF, 0x008000];
const STRAIGHT_CONTEXT: RenderInfo = RenderInfo::new(GPU_CONTEXT.space, Alpha::Straight);

const fn track(colors: &'static [u32]) -> ControlUi {
    ControlUi {
        track: Some(colors),
        ..ControlUi::NO_HEADER
    }
}

static BW_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: "\u{a0}Conversion Algorithm\u{a0}",
                keys: &["method", "brightness", "contrast"],
            },
            Tab {
                caption: "\u{a0}Blend with Color\u{a0}",
                keys: &["blendMode", "blendColor", "swapOrder"],
            },
        ],
    }],
    controls: &[
        (
            "method",
            ControlUi {
                note: Note::Text(" "),
                ..ControlUi::NO_HEADER
            },
        ),
        ("brightness", track(&TRACK_BLACK_WHITE)),
        ("contrast", track(&TRACK_CONTRAST)),
        (
            "blendMode",
            ControlUi {
                caption: "Blend mode",
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "blendColor",
            ControlUi {
                alpha: true,
                reset: false,
                ..ControlUi::NO_HEADER
            },
        ),
        ("swapOrder", ControlUi::NO_HEADER),
    ],
    enabled: |v, key| {
        !matches!(key, "blendColor" | "swapOrder") || choice(v, "blendMode") != "None"
    },
    help: Help::Window(HelpWindow::Wide),
    ..IndirectUi::DEFAULT
};

static BALANCE_UI: IndirectUi = IndirectUi {
    controls: &[
        ("cyanRed", track(&[0x00FFFF, 0xFFFFFF, 0xFF0000])),
        ("magentaGreen", track(&MAGENTA_GREEN_TRACK)),
        ("yellowBlue", track(&[0xFFFF00, 0xFFFFFF, 0x0000FF])),
        ("brightness", track(&TRACK_BLACK_WHITE)),
        ("contrast", track(&TRACK_CONTRAST)),
    ],
    ..IndirectUi::DEFAULT
};

fn preview_enabled(v: &Values, key: &str) -> bool {
    matches!(key, "previewOriginal" | "tab") || !flag(v, "previewOriginal")
}

static COMBINED_UI: IndirectUi = IndirectUi {
    controls: &[
        ("brightness", track(&TRACK_BLACK_WHITE)),
        (
            "contrast",
            ControlUi {
                header_line: true,
                ..track(&TRACK_CONTRAST)
            },
        ),
        ("hue", track(&TRACK_HUE)),
        (
            "saturation",
            ControlUi {
                header_line: true,
                ..track(&TRACK_SATURATION)
            },
        ),
        (
            "lightness",
            ControlUi {
                header_line: true,
                ..track(&TRACK_BLACK_WHITE)
            },
        ),
        // approximation: track endpoint alpha ignored (adjustments-combined-adjustments.md control 6)
        (
            "temperature",
            ControlUi {
                decimals: Some(2),
                ..track(&[0x98BAFF, 0xFFFFFF, 0xFF890E])
            },
        ),
        (
            "tint",
            ControlUi {
                header_line: true,
                decimals: Some(2),
                ..track(&MAGENTA_GREEN_TRACK)
            },
        ),
        ("invert", ControlUi::NO_HEADER),
        ("previewOriginal", ControlUi::NO_HEADER),
    ],
    enabled: preview_enabled,
    ..IndirectUi::DEFAULT
};

static HUE_UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: "Pixel Selection",
                keys: &[
                    "fromHue",
                    "toHue",
                    "grayPixels",
                    "fromSat",
                    "toSat",
                    "fromLight",
                    "toLight",
                ],
            },
            Tab {
                caption: "Pixel Adjustments",
                keys: &[
                    "brightness",
                    "contrast",
                    "hueShift",
                    "saturation",
                    "temperature",
                    "tint",
                ],
            },
        ],
    }],
    controls: &[
        (
            "fromHue",
            ControlUi {
                wrap: true,
                ..track(&TRACK_HUE)
            },
        ),
        (
            "toHue",
            ControlUi {
                wrap: true,
                ..track(&TRACK_HUE)
            },
        ),
        ("grayPixels", ControlUi::NO_HEADER),
        (
            "fromSat",
            ControlUi {
                header_line: true,
                wrap: true,
                ..track(&TRACK_SATURATION)
            },
        ),
        (
            "toSat",
            ControlUi {
                wrap: true,
                ..track(&TRACK_SATURATION)
            },
        ),
        (
            "fromLight",
            ControlUi {
                header_line: true,
                ..track(&TRACK_BLACK_WHITE)
            },
        ),
        ("toLight", track(&TRACK_BLACK_WHITE)),
        ("brightness", track(&TRACK_BLACK_WHITE)),
        (
            "contrast",
            ControlUi {
                note: Note::Text(" "),
                ..track(&TRACK_CONTRAST)
            },
        ),
        (
            "hueShift",
            ControlUi {
                header_line: true,
                wrap: true,
                ..track(&TRACK_HUE_CENTRED)
            },
        ),
        (
            "saturation",
            ControlUi {
                note: Note::Text(" "),
                ..track(&TRACK_SATURATION)
            },
        ),
        (
            "temperature",
            ControlUi {
                header_line: true,
                ..track(&[0x87CEEB, 0xFFFFFF, 0xFFA500])
            },
        ),
        ("tint", track(&MAGENTA_GREEN_TRACK)),
        ("transparency", track(&TRACK_BLACK_WHITE_BLACK)),
        ("previewOriginal", ControlUi::NO_HEADER),
    ],
    enabled: preview_enabled,
    ..IndirectUi::DEFAULT
};

static TRANSPARENCY_UI: IndirectUi = IndirectUi {
    controls: &[
        ("amount", track(&TRACK_BLACK_WHITE_BLACK)),
        ("ignoreTransparent", ControlUi::NO_HEADER),
        ("replaceTransparent", ControlUi::NO_HEADER),
    ],
    enabled: |v, key| key != "replaceTransparent" || !flag(v, "ignoreTransparent"),
    ..IndirectUi::DEFAULT
};

fn adjustment(
    id: &'static str,
    title: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    render: GpuFn,
    ui: &'static IndirectUi,
) -> EffectDef {
    EffectDef {
        id,
        icon: manifest(id).expect("registered adjustment").icon,
        category: Category::Adjustment,
        controls,
        kind: EffectKind::Gpu(info, render),
        dialog: None,
        dialog_title: Some(title),
        indirect: Some(ui),
    }
}

fn check(key: &'static str, label: &'static str) -> Control {
    Control::Checkbox {
        key,
        label,
        default: false,
    }
}

fn select(
    key: &'static str,
    default: &'static str,
    choices: &'static [(&'static str, &'static str)],
) -> Control {
    Control::Select {
        key,
        label: "",
        default,
        choices,
    }
}

fn tab() -> Control {
    Control::Value {
        key: "tab",
        default: || serde_json::json!(0),
    }
}

/// The five adjustments in the manifest's order.
pub fn effects() -> Vec<EffectDef> {
    vec![
        adjustment(
            "bbBlackAndWhitePlus",
            "Black and White+",
            vec![
                tab(),
                select("method", "paintNet", &METHODS),
                range("brightness", "Brightness", -100.0, 100.0, 0.0),
                range("contrast", "Contrast", -100.0, 100.0, 0.0),
                select("blendMode", "None", &BLENDS),
                Control::Color {
                    key: "blendColor",
                    label: "",
                    default: ColorDefault::Rgb(0xFF8C00),
                },
                check("swapOrder", "Swap order"),
            ],
            GPU_CONTEXT,
            black_and_white,
            &BW_UI,
        ),
        adjustment(
            "bbColorBalance",
            "Color Balance",
            vec![
                range("cyanRed", &CYAN_RED, -100.0, 100.0, 0.0),
                range("magentaGreen", &MAGENTA_GREEN, -100.0, 100.0, 0.0),
                range("yellowBlue", &YELLOW_BLUE, -100.0, 100.0, 0.0),
                range("brightness", "Brightness", -100.0, 100.0, 0.0),
                range("contrast", "Contrast", -100.0, 100.0, 0.0),
            ],
            GPU_CONTEXT,
            color_balance,
            &BALANCE_UI,
        ),
        adjustment(
            "bbCombinedAdjustments",
            "Combined Adjustments",
            vec![
                range("brightness", "Brightness / Contrast", -100.0, 100.0, 0.0),
                range("contrast", "", -100.0, 100.0, 0.0),
                range("hue", "Hue / Saturation / Lightness", -180.0, 180.0, 0.0),
                range("saturation", "", 0.0, 200.0, 100.0),
                range("lightness", "", -100.0, 100.0, 0.0),
                range("temperature", "Temperature / Tint", -1.0, 1.0, 0.0)
                    .slider_keys(2, 1.0, 5.0, false),
                range("tint", "", -1.0, 1.0, 0.0).slider_keys(2, 1.0, 5.0, false),
                select(
                    "conversion",
                    "color",
                    &[
                        ("color", "Color (no conversion)"),
                        ("sepia", "Sepia"),
                        ("blackAndWhite", "Black and White"),
                    ],
                ),
                check("invert", "Invert"),
                check("previewOriginal", "Preview original image"),
            ],
            GPU_CONTEXT,
            combined,
            &COMBINED_UI,
        ),
        adjustment(
            "bbHueSaturationPlus",
            "Hue / Saturation+",
            vec![
                tab(),
                range("fromHue", "From Hue", 0.0, 360.0, 0.0),
                range("toHue", "To Hue", 0.0, 360.0, 360.0),
                check("grayPixels", "Also select gray pixels"),
                range("fromSat", "From Saturation", 0.0, 100.0, 0.0),
                range("toSat", "To Saturation", 0.0, 100.0, 100.0),
                range("fromLight", "From Lightness", 0.0, 100.0, 0.0),
                range("toLight", "To Lightness", 0.0, 100.0, 100.0),
                range("brightness", "Brightness Adjustment", -100.0, 100.0, 0.0),
                range("contrast", "Contrast Adjustment", -100.0, 100.0, 0.0),
                range("hueShift", "Hue Adjustment", -180.0, 180.0, 0.0),
                range("saturation", "Saturation Adjustment", 0.0, 200.0, 100.0),
                range("temperature", "Temperature", -100.0, 100.0, 0.0),
                range("tint", "Tint", -100.0, 100.0, 0.0),
                range(
                    "transparency",
                    "Transparency Adjustment",
                    -100.0,
                    100.0,
                    0.0,
                ),
                check("previewOriginal", "Preview original image"),
            ],
            STRAIGHT_CONTEXT,
            hue_saturation,
            &HUE_UI,
        ),
        adjustment(
            "bbTransparency",
            "Transparency",
            vec![
                range("amount", &TRANSPARENT_OPAQUE, -100.0, 100.0, 0.0),
                check("ignoreTransparent", "Ignore transparent pixels"),
                check(
                    "replaceTransparent",
                    "Replace transparent pixels with secondary color values",
                ),
            ],
            STRAIGHT_CONTEXT,
            transparency,
            &TRANSPARENCY_UI,
        ),
    ]
}

fn black_and_white(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let (brightness, contrast) = (num(v, "brightness") as f32, num(v, "contrast") as f32);
    let mode = BLENDS
        .iter()
        .position(|(key, _)| *key == choice(v, "blendMode"))
        .unwrap_or(0);
    let color = color_value_alpha(v, "blendColor", env);
    // spec-question: Q3 Black and White+ flood colour conversion
    let flood = if BW_FLOOD_COLOUR_MANAGED {
        colour_exact(color, env.render_info.expect("GPU render context"))
    } else {
        colour_raw(color)
    };
    let method = choice(v, "method");
    let swap = flag(v, "swapOrder");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = src.at(x, y);
        let u = unpremultiply(p);
        let [r, g, b] = bc_shift([u[0], u[1], u[2]], brightness, contrast);
        let gray = match method {
            "luminosity" => 0.2126 * r + 0.7152 * g + 0.0722 * b,
            "average" => (r + g + b) / 3.0,
            "lightness" => (r.max(g).max(b) + r.min(g).min(b)) / 2.0,
            "maximum" => r.max(g).max(b),
            "minimum" => r.min(g).min(b),
            _ => luma(r, g, b),
        };
        let g = premultiply([gray, gray, gray, u[3]]);
        if mode == 0 {
            return g;
        }
        let (dst, top) = if swap { (g, flood) } else { (flood, g) };
        swap_alpha(mix_at(d2d_blend(mode - 1), dst, top, y * src.w + x), p[3])
    })
}

fn color_balance(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let (brightness, contrast) = (num(v, "brightness") as f32, num(v, "contrast") as f32);
    // quirk(bb): adjustments-color-balance.md Notes (255 vs 100 divisors)
    let [red, green, blue] =
        ["cyanRed", "magentaGreen", "yellowBlue"].map(|k| num(v, k) as f32 / 255.0);
    let offset = [
        red - green / 2.0 - blue / 2.0,
        green - red / 2.0 - blue / 2.0,
        blue - red / 2.0 - green / 2.0,
    ];
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let u = unpremultiply(src.at(x, y));
        let rgb = bc_shift([u[0], u[1], u[2]], brightness, contrast);
        premultiply([
            rgb[0] + offset[0],
            rgb[1] + offset[1],
            rgb[2] + offset[2],
            u[3],
        ])
    })
}

// approximation: shared D2D TemperatureTint gains (adjustments-combined-adjustments.md Behaviour 4)
fn temperature(rgb: [f32; 3], t: f32, n: f32, space: Space) -> [f32; 3] {
    if t == 0.0 && n == 0.0 {
        return rgb;
    }
    let linear = if space == Space::Srgb {
        rgb.map(srgb_to_linear_f32)
    } else {
        rgb
    };
    let adjusted = temp_tint_linear(linear, t, n);
    if space == Space::Srgb {
        adjusted.map(linear_to_srgb_f32)
    } else {
        adjusted
    }
}

fn combined(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    if flag(v, "previewOriginal") {
        return src.clone();
    }
    let [brightness, contrast, hue, saturation, lightness, t, n] = [
        "brightness",
        "contrast",
        "hue",
        "saturation",
        "lightness",
        "temperature",
        "tint",
    ]
    .map(|k| num(v, k) as f32);
    let space = env.render_info.expect("GPU render context").space;
    let conversion = choice(v, "conversion");
    let invert = flag(v, "invert");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let u = unpremultiply(src.at(x, y));
        let rgb = [u[0], u[1], u[2]];
        let rgb = match conversion {
            "sepia" => d2d_sepia(rgb),
            "blackAndWhite" => d2d_grayscale(rgb),
            _ => rgb,
        };
        let rgb = bc_shift(rgb, brightness, contrast);
        let rgb = hsl_adjust(rgb, hue, saturation, lightness);
        let rgb = temperature(rgb, t, n, space);
        let [r, g, b] = if invert { rgb.map(|c| 1.0 - c) } else { rgb };
        premultiply([r, g, b, u[3]])
    })
}

fn sorted_window(v: &Values, from: &str, to: &str) -> [f32; 2] {
    let (f, t) = (num(v, from) as f32 / 100.0, num(v, to) as f32 / 100.0);
    [f.min(t), f.max(t)]
}

fn adjusted_alpha(alpha: f32, amount: f32) -> f32 {
    if amount > 0.0 {
        alpha + (1.0 - alpha) * amount
    } else {
        alpha * (1.0 + amount)
    }
}

fn hue_saturation(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    if flag(v, "previewOriginal") {
        return src.clone();
    }
    let (from, to) = (
        num(v, "fromHue") as f32 / 360.0,
        num(v, "toHue") as f32 / 360.0,
    );
    let [s1, s2] = sorted_window(v, "fromSat", "toSat");
    let [l1, l2] = sorted_window(v, "fromLight", "toLight");
    let gray_pixels = flag(v, "grayPixels");
    let [contrast, hue, saturation, lightness] =
        ["contrast", "hueShift", "saturation", "brightness"].map(|k| num(v, k) as f32);
    let [t, n, alpha] = ["temperature", "tint", "transparency"].map(|k| num(v, k) as f32 / 100.0);
    let space = env.render_info.expect("GPU render context").space;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let p = src.at(x, y);
        let [r, g, b] = [p[0], p[1], p[2]].map(|c| c.clamp(0.0, 1.0));
        // spec-question: Q26 Hue/Saturation+ zero-alpha intermediate colour
        let zero_intermediates = HUE_PREMULTIPLIED_INTERMEDIATES && p[3] == 0.0;
        let a = if zero_intermediates {
            [0.0; 3]
        } else {
            [r, g, b]
        };
        let [h, s, l] = rgb_to_hsl(a[0], a[1], a[2]).map(|c| c.clamp(0.0, 1.0));
        let gray = (r - g).abs() < 0.0039 && (g - b).abs() < 0.0039 && (r - b).abs() < 0.0039;
        // quirk(bb): adjustments-hue-saturation-plus.md Behaviour (strict hue endpoints; gray skips saturation)
        let hue_ok = if from <= to {
            from < h && h < to
        } else {
            h > from || h < to
        };
        let chosen = if gray {
            gray_pixels
        } else {
            hue_ok && s1 <= s && s <= s2
        };
        if !chosen || l < l1 || l > l2 {
            return p;
        }
        let rgb = bc_shift([p[0], p[1], p[2]], 0.0, contrast);
        let rgb = hsl_adjust(rgb, hue, saturation, lightness);
        let [r, g, b] = if zero_intermediates {
            [0.0; 3]
        } else {
            temperature(rgb, t, n, space)
        };
        [r, g, b, adjusted_alpha(p[3], alpha)]
    })
}

fn transparency(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let amount = num(v, "amount") as f32 / 100.0;
    if amount == 0.0 {
        return src.clone();
    }
    let ignore = flag(v, "ignoreTransparent");
    let replace = flag(v, "replaceTransparent");
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let mut p = src.at(x, y);
        if p[3] == 0.0 {
            if ignore {
                return p;
            }
            if replace {
                // quirk(bb): adjustments-transparency.md "Replace" 0–255 into 0–1
                for (c, secondary) in p[..3].iter_mut().zip(env.secondary) {
                    *c = if secondary == 0 { 0.0 } else { 1.0 };
                }
            }
        }
        p[3] = adjusted_alpha(p[3], amount);
        p
    })
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::conventions::ALT_GPU_CONTEXT;
    use crate::effect::bb::test_util::*;
    use crate::effect::surface::Alpha;
    use crate::effect::{EffectKind, RenderInfo, find};
    use serde_json::{Value, json};

    const BW: &str = "bbBlackAndWhitePlus";
    const BALANCE: &str = "bbColorBalance";
    const COMBINED: &str = "bbCombinedAdjustments";
    const HUE: &str = "bbHueSaturationPlus";
    const TRANSPARENCY: &str = "bbTransparency";
    const WHITE: [u8; 4] = [255; 4];
    const STRIP: [[u8; 4]; 5] = [
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 128, 0, 255],
        [128, 128, 128, 255],
        [255, 0, 0, 255],
    ];

    fn one(id: &str, v: Value, p: [u8; 4]) -> [u8; 4] {
        px(&bb(id, v, &solid(1, 1, p)), 0, 0)
    }

    // The alternate column changes space, never these shaders' required alpha modes (R88).
    fn gamma(id: &str, v: Value, p: [u8; 4]) -> [u8; 4] {
        let EffectKind::Gpu(mut info, _) = find(id).unwrap().kind else {
            panic!("GPU effect")
        };
        info.space = ALT_GPU_CONTEXT.space;
        px(&bb_ctx(id, info, v, &solid(1, 1, p)), 0, 0)
    }

    #[track_caller]
    fn near(got: [u8; 4], want: [u8; 4]) {
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= TOL),
            "{got:?} vs {want:?}"
        );
    }

    fn strip() -> crate::canvas::ImageData {
        from_fn(5, 1, |x, _| STRIP[x as usize])
    }
    fn alphas(v: Value) -> Vec<u8> {
        let out = bb(HUE, v, &strip());
        (0..5).map(|x| px(&out, x, 0)[3]).collect()
    }

    /// adjustments-black-and-white-plus.md Expected results 1.
    #[test]
    fn bw_neutral_passthrough() {
        let img = from_fn(4, 4, |x, y| {
            if (x + y) % 2 == 0 {
                [128, 128, 128, 255]
            } else {
                [64, 64, 64, 128]
            }
        });
        for method in [
            "paintNet",
            "luminosity",
            "average",
            "lightness",
            "maximum",
            "minimum",
        ] {
            assert_eq!(bb(BW, json!({"method":method}), &img).data, img.data);
        }
    }

    /// adjustments-black-and-white-plus.md Expected results 2.
    #[test]
    fn bw_extrema_in_both_spaces() {
        for render in [one, gamma] {
            near(
                render(BW, json!({"method":"maximum"}), [200, 50, 100, 255]),
                [200, 200, 200, 255],
            );
            near(
                render(BW, json!({"method":"minimum"}), [200, 50, 100, 255]),
                [50, 50, 50, 255],
            );
        }
    }

    /// adjustments-black-and-white-plus.md Expected results 3.
    #[test]
    fn bw_weighted_methods_in_both_spaces() {
        for (render, gray, red, luminosity) in [
            (one as fn(_, _, _) -> _, 125, 149, 110),
            (gamma, 101, 76, 86),
        ] {
            near(
                render(BW, json!({}), [200, 50, 100, 255]),
                [gray, gray, gray, 255],
            );
            near(
                render(BW, json!({}), [255, 0, 0, 255]),
                [red, red, red, 255],
            );
            near(
                render(BW, json!({"method":"luminosity"}), [200, 50, 100, 255]),
                [luminosity, luminosity, luminosity, 255],
            );
        }
        near(
            gamma(BW, json!({"method":"average"}), [200, 50, 100, 255]),
            [117, 117, 117, 255],
        );
        near(
            gamma(BW, json!({"method":"lightness"}), [200, 50, 100, 255]),
            [125, 125, 125, 255],
        );
    }

    /// adjustments-black-and-white-plus.md Expected results 4.
    #[test]
    fn bw_multiply_converts_flood_to_effect_space() {
        for render in [one, gamma] {
            for swap in [false, true] {
                near(
                    render(BW, json!({"blendMode":"Multiply","swapOrder":swap}), WHITE),
                    [255, 140, 0, 255],
                );
                near(
                    render(
                        BW,
                        json!({"blendMode":"Multiply","blendColor":"#4080C0","swapOrder":swap}),
                        WHITE,
                    ),
                    [64, 128, 192, 255],
                );
            }
        }
    }

    /// adjustments-black-and-white-plus.md Expected results 5.
    #[test]
    fn bw_blend_keeps_source_alpha() {
        for render in [one, gamma] {
            near(
                render(
                    BW,
                    json!({"blendMode":"Screen","blendColor":"#000000"}),
                    [255, 255, 255, 128],
                ),
                [255, 255, 255, 128],
            );
            for swap in [false, true] {
                assert_eq!(
                    render(
                        BW,
                        json!({"blendMode":"Multiply","blendColor":"#4080C040","swapOrder":swap}),
                        [180, 70, 20, 77]
                    )[3],
                    77
                );
            }
        }
    }

    /// adjustments-color-balance.md Expected results 1.
    #[test]
    fn balance_identity() {
        let img = from_fn(5, 3, |x, y| [(x * 60) as u8, (y * 120) as u8, 77, 128]);
        assert_eq!(bb(BALANCE, json!({}), &img).data, img.data);
    }

    /// adjustments-color-balance.md Expected results 2.
    #[test]
    fn balance_red_on_gray() {
        near(
            one(BALANCE, json!({"cyanRed":100}), [128, 128, 128, 255]),
            [205, 38, 38, 255],
        );
        near(
            gamma(BALANCE, json!({"cyanRed":100}), [128, 128, 128, 255]),
            [228, 78, 78, 255],
        );
    }

    /// adjustments-color-balance.md Expected results 3.
    #[test]
    fn balance_tints_black() {
        near(
            one(BALANCE, json!({"cyanRed":100}), [0, 0, 0, 255]),
            [168, 0, 0, 255],
        );
        near(
            gamma(BALANCE, json!({"cyanRed":100}), [0, 0, 0, 255]),
            [100, 0, 0, 255],
        );
    }

    /// adjustments-color-balance.md Expected results 4.
    #[test]
    fn balance_tints_white() {
        near(
            one(BALANCE, json!({"cyanRed":100}), WHITE),
            [255, 232, 232, 255],
        );
        near(
            gamma(BALANCE, json!({"cyanRed":100}), WHITE),
            [255, 205, 205, 255],
        );
    }

    /// adjustments-color-balance.md Expected results 5.
    #[test]
    fn balance_equal_axes_cancel() {
        for render in [one, gamma] {
            for amount in [-100, 100] {
                near(
                    render(
                        BALANCE,
                        json!({"cyanRed":amount,"magentaGreen":amount,"yellowBlue":amount}),
                        [90, 140, 200, 64],
                    ),
                    [90, 140, 200, 64],
                );
            }
        }
    }

    /// adjustments-color-balance.md Expected results 6.
    #[test]
    fn balance_yellow_keeps_alpha() {
        near(
            one(BALANCE, json!({"yellowBlue":-100}), [128, 128, 128, 64]),
            [172, 172, 0, 64],
        );
        near(
            gamma(BALANCE, json!({"yellowBlue":-100}), [128, 128, 128, 64]),
            [178, 178, 28, 64],
        );
    }

    /// adjustments-combined-adjustments.md Expected results 1.
    #[test]
    fn combined_identity() {
        let img = from_fn(5, 3, |x, y| [(x * 60) as u8, (y * 120) as u8, 77, 128]);
        assert_eq!(bb(COMBINED, json!({}), &img).data, img.data);
    }

    /// adjustments-combined-adjustments.md Expected results 2.
    #[test]
    fn combined_preview_bypasses_every_stage() {
        let c = [200, 50, 100, 77];
        for render in [one, gamma] {
            near(
                render(
                    COMBINED,
                    json!({"conversion":"sepia","brightness":90,"contrast":50,"hue":40,"saturation":0,"lightness":-100,"temperature":1,"tint":-1,"invert":true,"previewOriginal":true}),
                    c,
                ),
                c,
            );
        }
    }

    /// adjustments-combined-adjustments.md Expected results 3.
    #[test]
    fn combined_lightness_extremes_keep_alpha() {
        for render in [one, gamma] {
            for alpha in [77, 255] {
                near(
                    render(COMBINED, json!({"lightness":100}), [200, 50, 100, alpha]),
                    [255, 255, 255, alpha],
                );
                near(
                    render(COMBINED, json!({"lightness":-100}), [200, 50, 100, alpha]),
                    [0, 0, 0, alpha],
                );
            }
        }
    }

    /// adjustments-combined-adjustments.md Expected results 4.
    #[test]
    fn combined_hue_half_turn() {
        for render in [one, gamma] {
            for hue in [-180, 180] {
                near(
                    render(COMBINED, json!({"hue":hue}), [255, 0, 0, 255]),
                    [0, 255, 255, 255],
                );
            }
        }
    }

    /// adjustments-combined-adjustments.md Expected results 5.
    #[test]
    fn combined_invert_in_both_spaces() {
        near(
            one(COMBINED, json!({"invert":true}), [10, 20, 30, 255]),
            [255, 254, 254, 255],
        );
        near(
            gamma(COMBINED, json!({"invert":true}), [10, 20, 30, 255]),
            [245, 235, 225, 255],
        );
        near(
            one(COMBINED, json!({"invert":true}), [128, 128, 128, 255]),
            [229, 229, 229, 255],
        );
        near(
            gamma(COMBINED, json!({"invert":true}), [128, 128, 128, 255]),
            [127, 127, 127, 255],
        );
    }

    /// adjustments-combined-adjustments.md Expected results 6.
    #[test]
    fn combined_zero_saturation_is_neutral() {
        // Reuse paint.rs's HSL block; the spec's ~76 applies to the classic BT.601 op.
        for render in [one, gamma] {
            let p = render(COMBINED, json!({"saturation":0}), [255, 0, 0, 255]);
            assert!(
                p[0].abs_diff(p[1]) <= 1 && p[1].abs_diff(p[2]) <= 1,
                "{p:?}"
            );
        }
    }

    /// adjustments-hue-saturation-plus.md Expected results 1.
    #[test]
    fn hue_identity() {
        assert_eq!(bb(HUE, json!({}), &strip()).data, strip().data);
    }

    /// adjustments-hue-saturation-plus.md Expected results 2.
    #[test]
    fn hue_default_range_excludes_red_and_gray() {
        assert_eq!(alphas(json!({"transparency":-100})), [0, 0, 0, 255, 255]);
    }

    /// adjustments-hue-saturation-plus.md Expected results 3.
    #[test]
    fn hue_gray_opt_in() {
        assert_eq!(
            alphas(json!({"transparency":-100,"grayPixels":true})),
            [0, 0, 0, 0, 255]
        );
    }

    /// adjustments-hue-saturation-plus.md Expected results 4.
    #[test]
    fn hue_wraps_selection() {
        assert_eq!(
            alphas(json!({"transparency":-100,"fromHue":300,"toHue":60})),
            [255, 255, 0, 255, 0]
        );
    }

    /// adjustments-hue-saturation-plus.md Expected results 5.
    #[test]
    fn hue_shift_affects_only_chosen_pixels() {
        let out = bb(HUE, json!({"hueShift":120}), &strip());
        assert_px(&out, 0, 0, [0, 0, 255, 255]);
        assert_px(&out, 4, 0, STRIP[4]);
    }

    /// adjustments-hue-saturation-plus.md Expected results 6.
    #[test]
    fn hue_alpha_toward_opaque() {
        for render in [one, gamma] {
            near(
                render(HUE, json!({"transparency":50}), [0, 255, 0, 128]),
                [0, 255, 0, 191],
            );
            near(
                render(HUE, json!({"transparency":100}), [0, 255, 0, 128]),
                [0, 255, 0, 255],
            );
        }
    }

    /// adjustments-hue-saturation-plus.md Expected results 7.
    #[test]
    fn hue_brightness_is_lightness() {
        for render in [one, gamma] {
            near(
                render(HUE, json!({"brightness":100}), [0, 255, 0, 255]),
                WHITE,
            );
        }
    }

    /// adjustments-transparency.md Expected results 1.
    #[test]
    fn transparency_more_opaque() {
        for render in [one, gamma] {
            near(
                render(TRANSPARENCY, json!({"amount":40}), [255, 0, 0, 200]),
                [255, 0, 0, 222],
            );
        }
    }

    /// adjustments-transparency.md Expected results 2.
    #[test]
    fn transparency_extremes_and_scaling() {
        for render in [one, gamma] {
            near(
                render(TRANSPARENCY, json!({"amount":-25}), [255, 0, 0, 200]),
                [255, 0, 0, 150],
            );
            near(
                render(TRANSPARENCY, json!({"amount":-100}), [255, 0, 0, 200]),
                [255, 0, 0, 0],
            );
            for a in [100, 255] {
                near(
                    render(TRANSPARENCY, json!({"amount":100}), [0, 255, 0, a]),
                    [0, 255, 0, 255],
                );
            }
        }
    }

    /// adjustments-transparency.md Expected results 3.
    #[test]
    fn transparency_hidden_color_and_ignore() {
        for render in [one, gamma] {
            near(
                render(TRANSPARENCY, json!({"amount":40}), [64, 128, 192, 0]),
                [64, 128, 192, 102],
            );
            assert_eq!(
                render(
                    TRANSPARENCY,
                    json!({"amount":40,"ignoreTransparent":true}),
                    [64, 128, 192, 0]
                ),
                [64, 128, 192, 0]
            );
            near(
                render(
                    TRANSPARENCY,
                    json!({"amount":40,"ignoreTransparent":true}),
                    [255, 0, 0, 200],
                ),
                [255, 0, 0, 222],
            );
            near(
                render(
                    TRANSPARENCY,
                    json!({"amount":100,"ignoreTransparent":true}),
                    [20, 80, 100, 1],
                ),
                [20, 80, 100, 255],
            );
        }
    }

    /// adjustments-transparency.md Expected results 4.
    #[test]
    fn transparency_secondary_channels_saturate_and_ignore_its_alpha() {
        for sec in [[255, 0, 0, 255], [128, 128, 0, 255], [64, 128, 192, 0]] {
            let mut env = crate::effect::EffectEnv::whole(1, 1);
            env.secondary = sec;
            let out = bb_env(
                TRANSPARENCY,
                json!({"amount":50,"replaceTransparent":true}),
                &solid(1, 1, [9, 17, 28, 0]),
                &env,
            );
            let rgb = if sec[1] == 0 {
                [255, 0, 0, 128]
            } else if sec[2] == 0 {
                [255, 255, 0, 128]
            } else {
                [255, 255, 255, 128]
            };
            assert_px(&out, 0, 0, rgb);
            let out = bb_env(
                TRANSPARENCY,
                json!({"amount":-50,"replaceTransparent":true}),
                &solid(1, 1, [9, 17, 28, 0]),
                &env,
            );
            assert_px(&out, 0, 0, [rgb[0], rgb[1], rgb[2], 0]);
            let out = bb_env(
                TRANSPARENCY,
                json!({"amount":50,"replaceTransparent":true,"ignoreTransparent":true}),
                &solid(1, 1, [9, 17, 28, 0]),
                &env,
            );
            assert_eq!(px(&out, 0, 0), [9, 17, 28, 0]);
        }
    }

    /// adjustments-transparency.md Expected results 5.
    #[test]
    fn transparency_zero_is_bit_identical() {
        let img = from_fn(3, 2, |x, y| {
            [(x * 70) as u8, 9, (y * 90) as u8, (x * 100 + y * 20) as u8]
        });
        for ignore in [false, true] {
            for replace in [false, true] {
                assert_eq!(
                    bb(
                        TRANSPARENCY,
                        json!({"ignoreTransparent":ignore,"replaceTransparent":replace}),
                        &img
                    )
                    .data,
                    img.data
                );
            }
        }
    }

    /// adjustments-hue-saturation-plus.md Behaviour: Straight alpha in both spaces (R88).
    #[test]
    fn straight_contract_keeps_hidden_rgb() {
        for id in [HUE, TRANSPARENCY] {
            let EffectKind::Gpu(info, _) = find(id).unwrap().kind else {
                panic!("GPU effect")
            };
            assert_eq!(
                (info.input, info.output),
                (Alpha::Straight, Alpha::Straight)
            );
            for render in [one, gamma] {
                assert_eq!(render(id, json!({}), [41, 89, 137, 0]), [41, 89, 137, 0]);
            }
        }
        let out = bb_ctx(
            HUE,
            RenderInfo::new(ALT_GPU_CONTEXT.space, Alpha::Straight),
            json!({"transparency":50}),
            &solid(1, 1, [0, 128, 0, 0]),
        );
        assert_eq!(px(&out, 0, 0), [0, 128, 0, 0]);
    }

    /// adjustments-hue-saturation-plus.md Behaviour B and C.3: zero-alpha intermediates (R90, Q26).
    #[test]
    fn hue_zero_alpha_intermediates_follow_explicit_topology() {
        for render in [one, gamma] {
            let green = [0, 128, 0, 0];
            let gray = [128, 128, 128, 0];
            // H=0 fails the default strict hue window. Gray detection still uses source RGB.
            for values in [
                json!({"transparency":50}),
                json!({"transparency":50,"grayPixels":true}),
            ] {
                assert_eq!(render(HUE, values, green), green);
            }
            // Wrapping selects H=0. Both neutral and nonneutral adjusted chains lose hidden RGB.
            for values in [
                json!({"fromHue":300,"toHue":60,"transparency":50}),
                json!({"fromHue":300,"toHue":60,"transparency":50,"brightness":100,"temperature":100}),
            ] {
                near(render(HUE, values, green), [0, 0, 0, 128]);
            }
            // Analysis S=0, independently of the original green's saturation.
            assert_eq!(
                render(
                    HUE,
                    json!({"fromHue":300,"toHue":60,"fromSat":1,"transparency":50}),
                    green
                ),
                green
            );
            near(
                render(
                    HUE,
                    json!({"grayPixels":true,"fromSat":100,"transparency":50}),
                    gray,
                ),
                [0, 0, 0, 128],
            );
            // Gray bypasses saturation, but the zero analysis L fails a positive lower bound.
            assert_eq!(
                render(
                    HUE,
                    json!({"grayPixels":true,"fromLight":1,"transparency":50}),
                    gray
                ),
                gray
            );
            assert_eq!(
                render(
                    HUE,
                    json!({"fromHue":300,"toHue":60,"transparency":50,"brightness":100,"temperature":100,"previewOriginal":true}),
                    green
                ),
                green
            );
            // Alpha 1/255 must retain normal analysis and adjustment colour.
            near(
                render(HUE, json!({"transparency":50}), [0, 128, 0, 1]),
                [0, 128, 0, 128],
            );
            near(
                render(
                    HUE,
                    json!({"transparency":50,"brightness":100}),
                    [0, 128, 0, 1],
                ),
                [255, 255, 255, 128],
            );
        }
    }

    /// adjustments-hue-saturation-plus.md Behaviour: clamp analysis H/S/L before inclusive windows.
    #[test]
    fn hue_clamps_analysis_saturation_at_the_default_upper_bound() {
        for render in [one, gamma] {
            near(
                render(HUE, json!({"transparency":-100}), [0, 128, 0, 255]),
                [0, 128, 0, 0],
            );
        }
    }

    /// adjustments-black-and-white-plus.md Behaviour: destination/source order and pixel-seeded Dissolve.
    #[test]
    fn bw_blend_order_and_spatial_dissolve() {
        near(
            gamma(
                BW,
                json!({"blendMode":"Overlay","blendColor":"#C08020"}),
                [64, 64, 64, 255],
            ),
            [161, 65, 16, 255],
        );
        near(
            gamma(
                BW,
                json!({"blendMode":"Overlay","blendColor":"#C08020","swapOrder":true}),
                [64, 64, 64, 255],
            ),
            [96, 64, 16, 255],
        );
        let img = solid(32, 2, [128, 128, 128, 128]);
        let values = json!({"blendMode":"Dissolve"});
        let out = bb(BW, values.clone(), &img);
        assert_eq!(out.data, bb(BW, values, &img).data);
        let colors: std::collections::BTreeSet<_> =
            out.data.as_chunks::<4>().0.iter().copied().collect();
        assert!(colors.len() > 1, "Dissolve must vary between pixels");
        assert!(out.data.as_chunks::<4>().0.iter().all(|p| p[3] == 128));
    }

    /// adjustments-combined-adjustments.md Behaviour: conversion precedes B/C, HSL, temperature and invert.
    #[test]
    fn combined_stage_order_and_linear_temperature() {
        near(
            gamma(
                COMBINED,
                json!({"conversion":"blackAndWhite","brightness":40,"lightness":50,"invert":true}),
                [255, 0, 0, 255],
            ),
            [69, 69, 69, 255],
        );
        near(
            gamma(
                COMBINED,
                json!({"conversion":"sepia","invert":true}),
                [255, 0, 0, 255],
            ),
            [77, 211, 220, 255],
        );
        for render in [one, gamma] {
            near(
                render(COMBINED, json!({"temperature":1}), [128, 128, 128, 77]),
                [149, 128, 109, 77],
            );
            near(
                render(
                    HUE,
                    json!({"temperature":100,"grayPixels":true}),
                    [128, 128, 128, 77],
                ),
                [149, 128, 109, 77],
            );
            near(
                render(COMBINED, json!({"tint":-1}), [128, 128, 128, 77]),
                [139, 109, 139, 77],
            );
            near(
                render(
                    HUE,
                    json!({"tint":-100,"grayPixels":true}),
                    [128, 128, 128, 77],
                ),
                [139, 109, 139, 77],
            );
        }
        near(
            one(
                HUE,
                json!({"contrast":100,"brightness":50}),
                [0, 0, 255, 77],
            ),
            [188, 188, 188, 77],
        );
        near(
            gamma(
                HUE,
                json!({"contrast":100,"brightness":50}),
                [0, 0, 255, 77],
            ),
            [128, 128, 128, 77],
        );
    }

    /// adjustments-hue-saturation-plus.md Behaviour: strict hue endpoints, sorted inclusive S/L and gray bypass.
    #[test]
    fn hue_selection_boundaries() {
        for render in [one, gamma] {
            for values in [
                json!({"fromHue":120}),
                json!({"toHue":120}),
                json!({"fromHue":120,"toHue":120}),
            ] {
                let mut v = values;
                v["transparency"] = json!(-100);
                assert_eq!(render(HUE, v, [0, 255, 0, 255]), [0, 255, 0, 255]);
            }
            for values in [
                json!({"fromSat":100,"toSat":100}),
                json!({"fromLight":50,"toLight":50}),
                json!({"fromLight":75,"toLight":25}),
                json!({"fromSat":100,"toSat":0}),
            ] {
                let mut v = values;
                v["transparency"] = json!(-100);
                assert_eq!(render(HUE, v, [0, 255, 0, 255]), [0, 255, 0, 0]);
            }
            assert_eq!(
                render(
                    HUE,
                    json!({"transparency":-100,"fromSat":0,"toSat":0}),
                    [0, 255, 0, 255]
                ),
                [0, 255, 0, 255]
            );
            assert_eq!(
                render(
                    HUE,
                    json!({"grayPixels":true,"transparency":-100,"fromSat":100,"toSat":100}),
                    [128, 128, 128, 255]
                )[3],
                0
            );
            assert_eq!(
                render(
                    HUE,
                    json!({"grayPixels":true,"transparency":-100,"fromLight":75}),
                    [128, 128, 128, 255]
                )[3],
                255
            );
        }
        assert_eq!(
            one(HUE, json!({"transparency":-100}), [10, 11, 10, 255]),
            [10, 11, 10, 255]
        );
        assert_eq!(
            gamma(HUE, json!({"transparency":-100}), [10, 11, 10, 255]),
            [10, 11, 10, 0]
        );
        assert_eq!(
            gamma(HUE, json!({"transparency":-100}), [128, 128, 129, 255])[3],
            0
        );
        near(
            gamma(
                HUE,
                json!({"contrast":100,"brightness":-100,"hueShift":120,"transparency":-100,"previewOriginal":true}),
                [20, 90, 40, 77],
            ),
            [20, 90, 40, 77],
        );
    }

    /// common.md §2: each registered adjustment uses the framework's normal selection clipping.
    #[test]
    fn selection_keeps_pixels_outside_its_bounds() {
        let img = solid(3, 1, [20, 90, 40, 255]);
        let (mask, polys) = rect_selection(3, 1, 1, 0, 2, 1);
        let env = with_selection(3, 1, &mask, &polys);
        for (id, values) in [
            (BW, json!({})),
            (BALANCE, json!({"cyanRed":100})),
            (COMBINED, json!({"invert":true})),
            (HUE, json!({"transparency":-100})),
            (TRANSPARENCY, json!({"amount":-100})),
        ] {
            let out = bb_env(id, values, &img, &env);
            assert_eq!(px(&out, 0, 0), [20, 90, 40, 255]);
            assert_eq!(px(&out, 2, 0), [20, 90, 40, 255]);
            assert_ne!(px(&out, 1, 0), [20, 90, 40, 255]);
        }
    }

    /// adjustments-black-and-white-plus.md Behaviour 1–2; adjustments-color-balance.md Behaviour 1–3.
    #[test]
    fn brightness_precedes_gray_conversion_and_color_balance() {
        near(
            gamma(BW, json!({"brightness":40}), [255, 0, 0, 255]),
            [104, 104, 104, 255],
        );
        near(
            gamma(
                BALANCE,
                json!({"brightness":40,"cyanRed":100}),
                [0, 0, 0, 255],
            ),
            [140, 0, 0, 255],
        );
        near(
            gamma(
                TRANSPARENCY,
                json!({"amount":50,"replaceTransparent":true}),
                [41, 89, 137, 0],
            ),
            [255, 255, 255, 128],
        );
    }

    /// common.md §3; all five adjustment control tables: exact labels, tabs, tracks and enable rules.
    #[test]
    fn dialog_contracts() {
        use crate::effect::bb::ui::{Help, HelpWindow, Note, TRACK_CONTRAST, TRACK_HUE_CENTRED};
        use crate::effect::{Control, with};
        for (id, title, n) in [
            (BW, "Black and White+", 7),
            (BALANCE, "Color Balance", 5),
            (COMBINED, "Combined Adjustments", 10),
            (HUE, "Hue / Saturation+", 16),
            (TRANSPARENCY, "Transparency", 3),
        ] {
            let d = find(id).unwrap();
            let ui = d.indirect.unwrap();
            assert_eq!(d.dialog_title, Some(title));
            assert_eq!(d.controls.len(), n);
            assert!(matches!(ui.help, Help::Window(_)));
        }
        let d = find(BW).unwrap();
        let ui = d.indirect.unwrap();
        assert_eq!(
            ui.tabs[0].tabs[0].caption,
            "\u{a0}Conversion Algorithm\u{a0}"
        );
        assert_eq!(ui.tabs[0].tabs[1].caption, "\u{a0}Blend with Color\u{a0}");
        assert!(matches!(ui.help, Help::Window(HelpWindow::Wide)));
        assert!(matches!(ui.control("method").note, Note::Text(" ")));
        assert_eq!(
            ui.control("contrast").track,
            Some(TRACK_CONTRAST.as_slice())
        );
        assert!(!ui.control("brightness").header_line);
        assert_eq!(ui.control("blendMode").caption, "Blend mode");
        assert!(ui.control("blendColor").alpha && !ui.control("blendColor").reset);
        for mode in ["None", "Multiply"] {
            let v = with(&d, &json!({"blendMode":mode}));
            for k in ["blendColor", "swapOrder"] {
                assert_eq!((ui.enabled)(&v, k), mode != "None");
            }
        }
        let d = find(BALANCE).unwrap();
        for (k, label) in [
            ("cyanRed", format!("Cyan{}Red", " ".repeat(54))),
            ("magentaGreen", format!("Magenta{}Green", " ".repeat(44))),
            ("yellowBlue", format!("Yellow{}Blue", " ".repeat(50))),
        ] {
            assert!(
                matches!(d.controls.iter().find(|c|c.key()==k),Some(Control::Range{label:got,..}) if *got==label)
            );
        }
        for id in [COMBINED, HUE] {
            let d = find(id).unwrap();
            let ui = d.indirect.unwrap();
            for preview in [false, true] {
                let v = with(&d, &json!({"previewOriginal":preview}));
                for c in &d.controls {
                    let k = c.key();
                    assert_eq!(
                        (ui.enabled)(&v, k),
                        !preview || k == "previewOriginal" || k == "tab",
                        "{id}:{k}"
                    );
                }
            }
        }
        let d = find(HUE).unwrap();
        let ui = d.indirect.unwrap();
        assert_eq!(ui.tabs[0].tabs[0].caption, "Pixel Selection");
        assert_eq!(ui.tabs[0].tabs[1].caption, "Pixel Adjustments");
        assert!(
            ui.tabs[0]
                .tabs
                .iter()
                .all(|t| !t.keys.contains(&"transparency") && !t.keys.contains(&"previewOriginal"))
        );
        assert_eq!(
            ui.control("hueShift").track,
            Some(TRACK_HUE_CENTRED.as_slice())
        );
        for k in ["fromHue", "toHue", "fromSat", "toSat", "hueShift"] {
            assert!(ui.control(k).wrap);
        }
        for k in ["fromSat", "fromLight", "hueShift", "temperature"] {
            assert!(ui.control(k).header_line);
        }
        let d = find(TRANSPARENCY).unwrap();
        let ui = d.indirect.unwrap();
        assert!(
            matches!(&d.controls[0],Control::Range{label,..} if *label==format!("Transparent{}Opaque"," ".repeat(36)))
        );
        assert!(!(ui.enabled)(
            &with(&d, &json!({"ignoreTransparent":true})),
            "replaceTransparent"
        ));
        assert!((ui.enabled)(&with(&d, &json!({})), "replaceTransparent"));
    }
}
