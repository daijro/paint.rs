//! Photocraft phase spec: S§4.2; research R§B.2 (Adjustments)
//!
//! The Extra tone adjustments: Black and White Mixer, HDR Toning, Match Color and Color Lookup
//! (`extra::fx::{tone, lut}`). Color Lookup applies a built-in look or a `.cube`/`.3dl`/`.look`
//! file, read by its dialog as Fill From File reads its image (R155's external input).

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::channels::layer_pixels;
use crate::effect::bb::index;
use crate::effect::bb::ui::{External, IndirectUi};
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, choice, color_value,
    flag, num, range,
};
use crate::extra::fx::lut::{self, BUILTIN};
use crate::extra::fx::tone::{self, HdrToning, MatchColor};
use crate::extra::shim;

/// Photoshop's Black & White defaults: reds, yellows, greens, cyans, blues, magentas.
const BW_DEFAULTS: [f64; 6] = [40.0, 60.0, 40.0, 60.0, 20.0, 80.0];
const BW_KEYS: [&str; 6] = ["reds", "yellows", "greens", "cyans", "blues", "magentas"];
const BW_LABELS: [&str; 6] = ["Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas"];

fn decimal(c: Control, step: f64) -> Control {
    c.step(step).slider_keys(1, step, step * 10.0, false)
}

/// Match Color's dialog draws its Source row (the generic layout skips `LayerSelect`) and starts
/// on the layer below the active one, or above it at the bottom.
static MATCH_UI: IndirectUi = IndirectUi {
    defaults: |v, env| {
        let source = if env.layer_index > 0 {
            env.layer_index - 1
        } else {
            1.min(env.layers.len().saturating_sub(1))
        };
        v.insert("source".into(), source.into());
    },
    ..IndirectUi::DEFAULT
};

/// Color Lookup's dialog draws its File row (the generic layout skips it); only Source: File
/// reads that file, and each source's own row is the enabled one.
static LUT_UI: IndirectUi = IndirectUi {
    external_for: |v| {
        if choice(v, "source") == "file" {
            External::Lut("file")
        } else {
            External::None
        }
    },
    enabled: |v, key| match key {
        "look" => choice(v, "source") != "file",
        "file" => choice(v, "source") == "file",
        _ => true,
    },
    ..IndirectUi::DEFAULT
};

fn def(id: &'static str, controls: Vec<Control>, render: crate::effect::CpuFn) -> EffectDef {
    let m = manifest(id).expect("tone effect has a manifest row");
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

pub fn effects() -> Vec<EffectDef> {
    let mut bw: Vec<Control> = (0..6)
        .map(|i| range(BW_KEYS[i], BW_LABELS[i], -200.0, 300.0, BW_DEFAULTS[i]))
        .collect();
    bw.push(Control::Checkbox {
        key: "tint",
        label: "Tint",
        default: false,
    });
    // Photoshop's default tint is a light sepia.
    bw.push(Control::Color {
        key: "tintColor",
        label: "Tint color",
        default: ColorDefault::Rgb(0xE1D3B3),
    });
    let d = HdrToning::default();
    vec![
        def("pcBlackAndWhiteMixer", bw, black_and_white),
        def(
            "pcHdrToning",
            vec![
                range("radius", "Radius", 1.0, 500.0, f64::from(d.radius)),
                decimal(
                    range("strength", "Strength", 0.1, 4.0, f64::from(d.strength)),
                    0.1,
                ),
                decimal(range("gamma", "Gamma", 0.1, 2.0, f64::from(d.gamma)), 0.1),
                decimal(
                    range("exposure", "Exposure", -5.0, 5.0, f64::from(d.exposure)),
                    0.1,
                ),
                range("detail", "Detail", -100.0, 300.0, f64::from(d.detail)),
                range("shadow", "Shadow", -100.0, 100.0, f64::from(d.shadow)),
                range(
                    "highlight",
                    "Highlight",
                    -100.0,
                    100.0,
                    f64::from(d.highlight),
                ),
                range("vibrance", "Vibrance", -100.0, 100.0, f64::from(d.vibrance)),
                range(
                    "saturation",
                    "Saturation",
                    -100.0,
                    100.0,
                    f64::from(d.saturation),
                ),
            ],
            hdr_toning,
        ),
        EffectDef {
            indirect: Some(&MATCH_UI),
            ..def(
                "pcMatchColor",
                vec![
                    Control::LayerSelect {
                        key: "source",
                        label: "Source",
                    },
                    range("luminance", "Luminance", 1.0, 200.0, 100.0),
                    range("intensity", "Color Intensity", 1.0, 200.0, 100.0),
                    range("fade", "Fade", 0.0, 100.0, 0.0),
                    Control::Checkbox {
                        key: "neutralize",
                        label: "Neutralize",
                        default: false,
                    },
                ],
                match_color,
            )
        },
        EffectDef {
            indirect: Some(&LUT_UI),
            ..def(
                "pcColorLookup",
                vec![
                    Control::Select {
                        key: "source",
                        label: "Source",
                        default: "builtin",
                        choices: &[("builtin", "Built-in"), ("file", "File")],
                    },
                    Control::Select {
                        key: "look",
                        label: "Look",
                        default: "warm",
                        choices: &LOOKS,
                    },
                    Control::File {
                        key: "file",
                        label: "Look file",
                        filters: &lut::EXTENSIONS,
                        all_files: false,
                    },
                    Control::Select {
                        key: "interpolation",
                        label: "Interpolation",
                        default: "trilinear",
                        choices: &[("trilinear", "Trilinear"), ("tetrahedral", "Tetrahedral")],
                    },
                    Control::Checkbox {
                        key: "dither",
                        label: "Dither",
                        default: false,
                    },
                ],
                color_lookup,
            )
        },
    ]
}

/// `lut::BUILTIN` as a `Select`'s choices.
static LOOKS: [(&str, &str); 8] = {
    let mut out = [("", ""); 8];
    let mut i = 0;
    while i < 8 {
        out[i] = BUILTIN[i];
        i += 1;
    }
    out
};

fn black_and_white(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let weights = std::array::from_fn(|i| num(v, BW_KEYS[i]) as f32);
    let t = color_value(v, "tintColor", env);
    let tint = flag(v, "tint").then(|| std::array::from_fn(|i| f32::from(t[i]) / 255.0));
    let mut buf = shim::to_f32(src);
    tone::black_white(&mut buf, &weights, tint);
    shim::from_f32(&buf, src.width, src.height)
}

fn hdr_toning(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    let p = HdrToning {
        radius: num(v, "radius") as f32,
        strength: num(v, "strength") as f32,
        gamma: num(v, "gamma") as f32,
        exposure: num(v, "exposure") as f32,
        detail: num(v, "detail") as f32,
        shadow: num(v, "shadow") as f32,
        highlight: num(v, "highlight") as f32,
        vibrance: num(v, "vibrance") as f32,
        saturation: num(v, "saturation") as f32,
    };
    let mut buf = shim::to_f32(src);
    tone::hdr_toning(&mut buf, src.width as usize, src.height as usize, &p);
    shim::from_f32(&buf, src.width, src.height)
}

fn match_color(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let o = MatchColor {
        luminance: num(v, "luminance") as f32,
        intensity: num(v, "intensity") as f32,
        fade: num(v, "fade") as f32,
        neutralize: flag(v, "neutralize"),
    };
    let mut buf = shim::to_f32(src);
    let Some(target) = tone::lab_stats(&buf) else {
        return src.clone();
    };
    // No layer list (or an empty source layer): the image matches its own statistics.
    let source =
        layer_pixels(env, index(v, "source")).and_then(|l| tone::lab_stats(&shim::to_f32(l)));
    tone::match_color(&mut buf, &target, source.as_ref(), &o);
    shim::from_f32(&buf, src.width, src.height)
}

fn color_lookup(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let builtin;
    let table = if choice(v, "source") == "file" {
        // Nothing read (an empty path, or a failed read): the layer as it is.
        env.lut
    } else {
        builtin = lut::builtin(choice(v, "look"));
        builtin.as_ref()
    };
    let Some(table) = table else {
        return src.clone();
    };
    let mut buf = shim::to_f32(src);
    table.apply(
        &mut buf,
        src.width as usize,
        choice(v, "interpolation") == "tetrahedral",
        flag(v, "dither"),
    );
    shim::from_f32(&buf, src.width, src.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::LayerInfo;
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use serde_json::json;
    use std::sync::Arc;

    /// Raising Reds brightens a red pixel and leaves a blue one alone.
    #[test]
    fn bw_mixer_reds_slider_brightens_red_only() {
        let src = from_fn(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let base = run("pcBlackAndWhiteMixer", json!({}), &src);
        let more = run("pcBlackAndWhiteMixer", json!({"reds": 100}), &src);
        assert!(px(&more, 0, 0)[0] > px(&base, 0, 0)[0]);
        assert_eq!(px(&more, 1, 0), px(&base, 1, 0));
        // Greys: r = g = b, alpha kept.
        let p = px(&base, 0, 0);
        assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 255);
        // Defaults: red -> 40%.
        assert_eq!(p[0], 102);
    }

    #[test]
    fn bw_mixer_tint_colours_the_grey() {
        let src = from_fn(1, 1, |_, _| [128, 128, 128, 255]);
        let p = px(
            &run("pcBlackAndWhiteMixer", json!({"tint": true}), &src),
            0,
            0,
        );
        assert!(p[0] > p[2], "{p:?}");
    }

    #[test]
    fn hdr_toning_is_deterministic() {
        let src = from_fn(24, 16, |x, y| [(x * 10) as u8, (y * 15) as u8, 90, 255]);
        let a = run("pcHdrToning", json!({}), &src);
        assert_eq!(a.data, run("pcHdrToning", json!({}), &src).data);
        assert_ne!(a.data, src.data);
    }

    /// Matching a layer to itself changes nothing (±1 level of Lab round trip).
    #[test]
    fn match_color_to_self_is_identity() {
        let src = from_fn(16, 16, |x, y| [(x * 16) as u8, (y * 16) as u8, 120, 255]);
        let ls = [LayerInfo {
            name: "Layer 1".into(),
            pixels: Arc::new(src.clone()),
        }];
        let mut env = EffectEnv::whole(16, 16);
        env.layers = &ls;
        let out = run_env("pcMatchColor", json!({"source": 0}), &src, &env);
        assert!(
            out.data
                .iter()
                .zip(&src.data)
                .all(|(a, b)| a.abs_diff(*b) <= 1),
            "max diff {}",
            out.data
                .iter()
                .zip(&src.data)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap()
        );
    }

    /// The default source is another layer than the active one (below it, or above at the bottom).
    #[test]
    fn match_color_default_source_is_not_the_active_layer() {
        use crate::effect::bb::indirect_defaults;
        let def = crate::effect::find("pcMatchColor").unwrap();
        let img = from_fn(1, 1, |_, _| [1, 2, 3, 255]);
        let ls: Vec<LayerInfo> = (0..3)
            .map(|i| LayerInfo {
                name: format!("L{i}"),
                pixels: Arc::new(img.clone()),
            })
            .collect();
        for active in 0..3 {
            let mut env = EffectEnv::whole(1, 1);
            env.layers = &ls;
            env.layer_index = active;
            let src = num(&indirect_defaults(&def, &env), "source") as usize;
            assert_ne!(src, active, "active {active}");
            assert!(src < 3);
        }
    }

    #[test]
    fn match_color_moves_toward_a_source_layer() {
        let src = from_fn(8, 8, |x, _| [40, 60, (100 + x * 4) as u8, 255]);
        let other = from_fn(8, 8, |x, _| [(200 + x) as u8, 120, 40, 255]);
        let ls = [LayerInfo {
            name: "Other".into(),
            pixels: Arc::new(other),
        }];
        let mut env = EffectEnv::whole(8, 8);
        env.layers = &ls;
        let out = run_env("pcMatchColor", json!({"source": 0}), &src, &env);
        assert!(px(&out, 4, 4)[0] > px(&src, 4, 4)[0] + 50);
    }

    #[test]
    fn color_lookup_applies_a_builtin_look() {
        let src = from_fn(2, 1, |x, _| [x as u8 * 255, 128, 40, 200]);
        let out = run("pcColorLookup", json!({"look": "monoContrast"}), &src);
        let p = px(&out, 0, 0);
        assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 200, "{p:?}");
        let tet = run(
            "pcColorLookup",
            json!({"look": "monoContrast", "interpolation": "tetrahedral", "dither": true}),
            &src,
        );
        assert_eq!(tet.data.len(), src.data.len());
        assert_ne!(out.data, src.data);
    }

    /// I1: with Source: File the table the dialog read (`env.lut`) is applied, alpha kept; before
    /// any read the layer stays as it is, and the built-in source ignores the file.
    #[test]
    fn color_lookup_applies_the_file_table() {
        let src = from_fn(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 64, 255]
            } else {
                [10, 200, 30, 128]
            }
        });
        let inverse = lut::LutFile::from_fn("Inverse", 2, |c| c.map(|v| 1.0 - v));
        let file = json!({"source": "file"});
        let mut env = EffectEnv::whole(2, 1);
        assert_eq!(
            run_env("pcColorLookup", file.clone(), &src, &env).data,
            src.data
        );
        env.lut = Some(&inverse);
        let out = run_env("pcColorLookup", file, &src, &env);
        assert_eq!(out.data, [0, 255, 191, 255, 245, 55, 225, 128]);
        assert_eq!(
            run_env("pcColorLookup", json!({}), &src, &env).data,
            run("pcColorLookup", json!({}), &src).data
        );
    }

    /// I1: the source is a row of the dialog, and only File reads a file (R155's external input).
    #[test]
    fn color_lookup_reads_a_file_only_from_the_file_source() {
        use crate::effect::bb::ui::External;
        let def = crate::effect::find("pcColorLookup").unwrap();
        let ui = def.indirect.expect("Color Lookup has an IndirectUi");
        let mut v = crate::effect::defaults(&def);
        assert_eq!((ui.external_for)(&v), External::None);
        assert!((ui.enabled)(&v, "look") && !(ui.enabled)(&v, "file"));
        v.insert("source".into(), json!("file"));
        assert_eq!((ui.external_for)(&v), External::Lut("file"));
        assert!(!(ui.enabled)(&v, "look") && (ui.enabled)(&v, "file"));
    }
}
