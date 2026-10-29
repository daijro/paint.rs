//! Photocraft phase spec: S§4.2, the Tier 3 dialogs; research R§B (root · Camera Raw Filter)
//!
//! Camera Raw: the develop pipeline (`extra::fx::camera_raw`) on the current layer's pixels, in
//! the Effect Config dialog with Basic, Tone, Curve, HSL, Grading and Detail tabs (an
//! `IndirectUi`, Ruling R11); the canvas is its live preview. Load Preset... sets the controls from
//! a Camera Raw `.xmp` develop preset (Lightroom's, LightCraft's). Raw-file decoding is not part of
//! it.

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::ui::{ControlUi, IndirectUi, Preset, TRACK_HUE, Tab, TabStrip};
use crate::effect::{
    Category, Control, EffectDef, EffectEnv, EffectKind, Values, flag, num, range,
};
use crate::extra::fx::camera_raw::{self, CameraRaw, xmp};
use crate::extra::interrupt::Interrupt;
use crate::extra::shim;
use serde_json::{Value, json};

/// ACR's Basic order, with upstream's "Light" group (Exposure … Blacks) kept together.
const BASIC: [&str; 8] = [
    "temperature",
    "tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
];
/// Upstream's Presence group (Dehaze with it, as ACR's Basic panel has it).
const TONE: [&str; 5] = ["texture", "clarity", "dehaze", "vibrance", "saturation"];
/// Upstream's Curve section: the parametric regions, then ACR's region splits; the point curves
/// (`pointCurves`, master, red, green, blue) come from presets, the check box turns them off.
const CURVE: [&str; 9] = [
    "curveHighlights",
    "curveLights",
    "curveDarks",
    "curveShadows",
    "curveShadowSplit",
    "curveMidtoneSplit",
    "curveHighlightSplit",
    "pointCurve",
    "pointCurves",
];
/// The Color Grading wheels' hue, saturation and luminance (shadows, midtones, highlights, global).
const GRADE: [[&str; 3]; 4] = [
    ["gradeShadowHue", "gradeShadowSat", "gradeShadowLum"],
    ["gradeMidtoneHue", "gradeMidtoneSat", "gradeMidtoneLum"],
    [
        "gradeHighlightHue",
        "gradeHighlightSat",
        "gradeHighlightLum",
    ],
    ["gradeGlobalHue", "gradeGlobalSat", "gradeGlobalLum"],
];
/// The Color Mixer's hue, saturation and luminance per band (red … magenta).
const HSL: [[&str; 8]; 3] = [
    [
        "hueRed",
        "hueOrange",
        "hueYellow",
        "hueGreen",
        "hueAqua",
        "hueBlue",
        "huePurple",
        "hueMagenta",
    ],
    [
        "satRed",
        "satOrange",
        "satYellow",
        "satGreen",
        "satAqua",
        "satBlue",
        "satPurple",
        "satMagenta",
    ],
    [
        "lumRed",
        "lumOrange",
        "lumYellow",
        "lumGreen",
        "lumAqua",
        "lumBlue",
        "lumPurple",
        "lumMagenta",
    ],
];
const BANDS: [&str; 8] = [
    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
];
const SHARPENING: [&str; 4] = [
    "sharpenAmount",
    "sharpenRadius",
    "sharpenDetail",
    "sharpenMasking",
];
const NOISE: [&str; 4] = [
    "noiseLuminance",
    "noiseLuminanceDetail",
    "noiseColor",
    "noiseColorDetail",
];

/// A slider drawn over colour stops (upstream's Temperature and Tint gradients).
const fn track(colors: &'static [u32]) -> ControlUi {
    ControlUi {
        track: Some(colors),
        ..ControlUi::DEFAULT
    }
}

/// Grey 128 to each Color Mixer band's hue (upstream's `hue_color(HSL_BANDS[k])`: red, orange,
/// yellow, green, aqua, blue, purple, magenta at full saturation and brightness).
const BAND_HUES: [u32; 8] = [
    0xFF0000, 0xFF8000, 0xFFFF00, 0x00FF00, 0x00FFFF, 0x0000FF, 0x8000FF, 0xFF00FF,
];
static BAND_STOPS: [[u32; 2]; 8] = {
    let mut s = [[0x808080; 2]; 8];
    let mut i = 0;
    while i < 8 {
        s[i][1] = BAND_HUES[i];
        i += 1;
    }
    s
};
/// A wheel's hue slider: the hue circle from red, wrapping (upstream's `hue_stops`).
const HUE: ControlUi = ControlUi {
    wrap: true,
    ..track(&TRACK_HUE)
};
static CONTROL_UI: [(&str, ControlUi); 30] = {
    let mut c = [("", ControlUi::DEFAULT); 30];
    c[0] = ("temperature", track(&[0x4678E6, 0xC8C8C8, 0xEBC83C]));
    c[1] = ("tint", track(&[0x46BE50, 0xC8C8C8, 0xD250C8]));
    let mut g = 0;
    while g < 3 {
        let mut b = 0;
        while b < 8 {
            c[2 + g * 8 + b] = (HSL[g][b], track(&BAND_STOPS[b]));
            b += 1;
        }
        g += 1;
    }
    let mut w = 0;
    while w < 4 {
        c[26 + w] = (GRADE[w][0], HUE);
        w += 1;
    }
    c
};

static CAMERA_RAW_UI: IndirectUi = IndirectUi {
    tabs: &[
        TabStrip {
            key: "tab",
            tabs: &[
                Tab {
                    caption: "Basic",
                    keys: &BASIC,
                },
                Tab {
                    caption: "Tone",
                    keys: &TONE,
                },
                Tab {
                    caption: "Curve",
                    keys: &CURVE,
                },
                Tab {
                    caption: "HSL",
                    keys: &["hslTab"],
                },
                Tab {
                    caption: "Grading",
                    keys: &["gradeTab", "gradeBlending", "gradeBalance"],
                },
                Tab {
                    caption: "Detail",
                    keys: &["detailTab"],
                },
            ],
        },
        TabStrip {
            key: "hslTab",
            tabs: &[
                Tab {
                    caption: "Hue",
                    keys: &HSL[0],
                },
                Tab {
                    caption: "Saturation",
                    keys: &HSL[1],
                },
                Tab {
                    caption: "Luminance",
                    keys: &HSL[2],
                },
            ],
        },
        TabStrip {
            key: "gradeTab",
            tabs: &[
                Tab {
                    caption: "Shadows",
                    keys: &GRADE[0],
                },
                Tab {
                    caption: "Midtones",
                    keys: &GRADE[1],
                },
                Tab {
                    caption: "Highlights",
                    keys: &GRADE[2],
                },
                Tab {
                    caption: "Global",
                    keys: &GRADE[3],
                },
            ],
        },
        TabStrip {
            key: "detailTab",
            tabs: &[
                Tab {
                    caption: "Sharpening",
                    keys: &SHARPENING,
                },
                Tab {
                    caption: "Noise Reduction",
                    keys: &NOISE,
                },
            ],
        },
    ],
    // ×1.1: room for Load Preset... in the footer.
    width: 1.1,
    transient_keys: &["tab", "hslTab", "gradeTab", "detailTab"],
    controls: &CONTROL_UI,
    // The point curves' check box only when there is a curve to turn off.
    enabled: |v, k| k != "pointCurve" || point_curves(v).iter().any(|c| !c.is_empty()),
    preset: Some(Preset {
        filter: "Camera Raw Presets (*.xmp)",
        extension: "xmp",
        max_bytes: xmp::MAX_XMP_BYTES,
        load: load_preset,
    }),
    ..IndirectUi::DEFAULT
};

fn tab(key: &'static str) -> Control {
    Control::Value {
        key,
        default: || serde_json::json!(0),
    }
}

/// A ±100 slider, neutral at 0.
fn signed(key: &'static str, label: &'static str) -> Control {
    range(key, label, -100.0, 100.0, 0.0)
}

pub fn effects() -> Vec<EffectDef> {
    let d = CameraRaw::default();
    let mut controls = vec![
        tab("tab"),
        tab("hslTab"),
        tab("gradeTab"),
        tab("detailTab"),
        signed("temperature", "Temperature"),
        signed("tint", "Tint"),
        range("exposure", "Exposure", -5.0, 5.0, 0.0)
            .step(0.01)
            .slider_keys(2, 0.01, 0.1, false),
        signed("contrast", "Contrast"),
        signed("highlights", "Highlights"),
        signed("shadows", "Shadows"),
        signed("whites", "Whites"),
        signed("blacks", "Blacks"),
        signed("texture", "Texture"),
        signed("clarity", "Clarity"),
        signed("dehaze", "Dehaze"),
        signed("vibrance", "Vibrance"),
        signed("saturation", "Saturation"),
        signed("curveHighlights", "Highlights"),
        signed("curveLights", "Lights"),
        signed("curveDarks", "Darks"),
        signed("curveShadows", "Shadows"),
        range(
            "curveShadowSplit",
            "Shadow Split",
            0.0,
            100.0,
            f64::from(d.curve_splits[0]),
        ),
        range(
            "curveMidtoneSplit",
            "Midtone Split",
            0.0,
            100.0,
            f64::from(d.curve_splits[1]),
        ),
        range(
            "curveHighlightSplit",
            "Highlight Split",
            0.0,
            100.0,
            f64::from(d.curve_splits[2]),
        ),
        Control::Checkbox {
            key: "pointCurve",
            label: "Point Curve",
            default: false,
        },
        Control::Value {
            key: "pointCurves",
            default: || json!([[], [], [], []]),
        },
    ];
    for keys in &HSL {
        controls.extend(keys.iter().zip(BANDS).map(|(k, l)| signed(k, l)));
    }
    for [hue, sat, lum] in GRADE {
        controls.extend([
            range(hue, "Hue", 0.0, 360.0, 0.0),
            range(sat, "Saturation", 0.0, 100.0, 0.0),
            signed(lum, "Luminance"),
        ]);
    }
    controls.extend([
        range(
            "gradeBlending",
            "Blending",
            0.0,
            100.0,
            f64::from(d.grade_blending),
        ),
        signed("gradeBalance", "Balance"),
        range("sharpenAmount", "Amount", 0.0, 150.0, 0.0),
        range(
            "sharpenRadius",
            "Radius",
            0.5,
            3.0,
            f64::from(d.sharpen_radius),
        )
        .step(0.1)
        .slider_keys(1, 0.1, 1.0, false),
        range(
            "sharpenDetail",
            "Detail",
            0.0,
            100.0,
            f64::from(d.sharpen_detail),
        ),
        range("sharpenMasking", "Masking", 0.0, 100.0, 0.0),
        range("noiseLuminance", "Luminance", 0.0, 100.0, 0.0),
        range(
            "noiseLuminanceDetail",
            "Luminance Detail",
            0.0,
            100.0,
            f64::from(d.noise_luminance_detail),
        ),
        range("noiseColor", "Color", 0.0, 100.0, 0.0),
        range(
            "noiseColorDetail",
            "Color Detail",
            0.0,
            100.0,
            f64::from(d.noise_color_detail),
        ),
    ]);
    let m = manifest("pcCameraRaw").expect("Camera Raw has a manifest row");
    vec![EffectDef {
        id: m.id,
        icon: m.icon,
        category: Category::Adjustment,
        controls,
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: Some(&CAMERA_RAW_UI),
    }]
}

/// Every slider's key with its develop setting.
fn sliders(p: &mut CameraRaw) -> Vec<(&'static str, &mut f32)> {
    let mut s = vec![
        ("temperature", &mut p.temperature),
        ("tint", &mut p.tint),
        ("exposure", &mut p.exposure),
        ("contrast", &mut p.contrast),
        ("highlights", &mut p.highlights),
        ("shadows", &mut p.shadows),
        ("whites", &mut p.whites),
        ("blacks", &mut p.blacks),
        ("texture", &mut p.texture),
        ("clarity", &mut p.clarity),
        ("dehaze", &mut p.dehaze),
        ("vibrance", &mut p.vibrance),
        ("saturation", &mut p.saturation),
        ("curveHighlights", &mut p.curve_highlights),
        ("curveLights", &mut p.curve_lights),
        ("curveDarks", &mut p.curve_darks),
        ("curveShadows", &mut p.curve_shadows),
        ("gradeBlending", &mut p.grade_blending),
        ("gradeBalance", &mut p.grade_balance),
        ("sharpenAmount", &mut p.sharpen_amount),
        ("sharpenRadius", &mut p.sharpen_radius),
        ("sharpenDetail", &mut p.sharpen_detail),
        ("sharpenMasking", &mut p.sharpen_masking),
        ("noiseLuminance", &mut p.noise_luminance),
        ("noiseLuminanceDetail", &mut p.noise_luminance_detail),
        ("noiseColor", &mut p.noise_color),
        ("noiseColorDetail", &mut p.noise_color_detail),
    ];
    s.extend(CURVE[4..7].iter().copied().zip(&mut p.curve_splits));
    for (keys, bands) in HSL
        .iter()
        .zip([&mut p.hsl_hue, &mut p.hsl_sat, &mut p.hsl_lum])
    {
        s.extend(keys.iter().copied().zip(bands));
    }
    let wheels = [
        &mut p.grade_shadows,
        &mut p.grade_midtones,
        &mut p.grade_highlights,
        &mut p.grade_global,
    ];
    for (keys, w) in GRADE.iter().zip(wheels) {
        s.extend(
            keys.iter()
                .copied()
                .zip([&mut w.hue, &mut w.sat, &mut w.lum]),
        );
    }
    s
}

/// The point curves (master, red, green, blue) as `pointCurves` holds them: `[input, output]`
/// pairs in 0..=255, empty for a linear curve or a malformed value.
fn point_curves(v: &Values) -> [Vec<[f32; 2]>; 4] {
    std::array::from_fn(|i| {
        let pair = |p: &Value| Some([p.get(0)?.as_f64()? as f32, p.get(1)?.as_f64()? as f32]);
        let points = v.get("pointCurves").and_then(|c| c.get(i)?.as_array());
        points.into_iter().flatten().filter_map(pair).collect()
    })
}

/// The dialog's values as develop settings. Exposure and the noise amounts are clamped to their
/// slider ranges, as upstream's command does: they become `2^exposure`, a filter radius and a
/// blur sigma, so a stored out-of-range value would overflow or mean unbounded work. The point
/// curves apply while their check box is on.
fn settings(v: &Values) -> CameraRaw {
    let mut p = CameraRaw::default();
    for (k, f) in sliders(&mut p) {
        *f = num(v, k) as f32;
    }
    p.exposure = p.exposure.clamp(-5.0, 5.0);
    for n in [
        &mut p.noise_luminance,
        &mut p.noise_luminance_detail,
        &mut p.noise_color,
        &mut p.noise_color_detail,
    ] {
        *n = n.clamp(0.0, 100.0);
    }
    if flag(v, "pointCurve") {
        [
            p.point_curve,
            p.point_curve_red,
            p.point_curve_green,
            p.point_curve_blue,
        ] = point_curves(v);
    }
    p
}

/// Load Preset...: a Camera Raw `.xmp` develop preset's settings over the dialog's values (the
/// ones Camera Raw has no control for are ignored). A preset that changes the point curves turns
/// their check box on, or off when it makes them all linear.
fn load_preset(bytes: &[u8], v: &mut Values) -> Result<(), String> {
    let mut p = settings(v);
    let before = point_curves(v);
    [
        p.point_curve,
        p.point_curve_red,
        p.point_curve_green,
        p.point_curve_blue,
    ] = before.clone();
    xmp::load(&mut p, bytes)?;
    let after = [
        std::mem::take(&mut p.point_curve),
        std::mem::take(&mut p.point_curve_red),
        std::mem::take(&mut p.point_curve_green),
        std::mem::take(&mut p.point_curve_blue),
    ];
    for (k, f) in sliders(&mut p) {
        // The f32's shortest decimal: +0.35 stays 0.35, not 0.3499999940395355.
        v.insert(k.into(), json!(f.to_string().parse::<f64>().unwrap_or(0.0)));
    }
    if after != before {
        v.insert(
            "pointCurve".into(),
            json!(after.iter().any(|c| !c.is_empty())),
        );
        v.insert("pointCurves".into(), json!(after));
    }
    Ok(())
}

fn render(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut buf = shim::to_f32(src);
    let (w, h) = (src.width as usize, src.height as usize);
    let px = buf.as_chunks_mut::<4>().0;
    match camera_raw::develop(px, w, h, &settings(v), Interrupt(env.cancel)) {
        Ok(()) => shim::from_f32(&buf, src.width, src.height),
        Err(_) => src.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, run};
    use crate::pixel::gamma::{linear_to_srgb, srgb_to_linear};
    use serde_json::json;

    fn ramp() -> ImageData {
        from_fn(16, 16, |x, y| {
            [(x * 16) as u8, (y * 16) as u8, (x * 8 + y * 7) as u8, 200]
        })
    }

    /// Every control at its default develops nothing, exactly; so do the controls that only
    /// shape a stage whose amount is zero (sharpen Radius and Detail, the noise Detail sliders);
    /// and each amount that does something changes the picture (a stub that ignores
    /// `is_identity`, treating every setting as neutral, fails the second half).
    #[test]
    fn camera_raw_defaults_are_identity() {
        let src = ramp();
        assert_eq!(run("pcCameraRaw", json!({}), &src).data, src.data);
        let inert = json!({
            "sharpenRadius": 2.5, "sharpenDetail": 90, "sharpenMasking": 50,
            "noiseLuminanceDetail": 10, "noiseColorDetail": 90,
            "curveShadowSplit": 10, "curveMidtoneSplit": 70, "curveHighlightSplit": 90,
            "gradeShadowHue": 200, "gradeBlending": 80, "gradeBalance": -40,
            "pointCurves": [[[0, 40], [255, 255]], [], [], []],
        });
        assert_eq!(run("pcCameraRaw", inert, &src).data, src.data);
        let busy = from_fn(16, 16, |x, y| {
            let v = ((x * 37 + y * 91) % 200 + 20) as u8;
            [v, 255 - v, ((x ^ y) * 15) as u8, 200]
        });
        for (k, val) in [
            ("exposure", 1.0),
            ("temperature", 40.0),
            ("contrast", 50.0),
            ("shadows", 60.0),
            ("vibrance", 50.0),
            ("saturation", 50.0),
            ("hueRed", 50.0),
            ("curveShadows", 60.0),
            ("gradeShadowSat", 60.0),
            ("gradeGlobalLum", 40.0),
            ("sharpenAmount", 100.0),
            ("noiseLuminance", 60.0),
            ("noiseColor", 60.0),
        ] {
            let out = run("pcCameraRaw", json!({ k: val }), &busy);
            assert_ne!(out.data, busy.data, "{k} did nothing");
        }
        let curve = json!({"pointCurve": true, "pointCurves": [[], [], [], [[0, 60], [255, 255]]]});
        assert_ne!(run("pcCameraRaw", curve, &busy).data, busy.data);
    }

    /// Exposure +1 is one stop: twice the linear light, alpha kept (±1 level of 8-bit rounding).
    #[test]
    fn camera_raw_exposure_plus_one_doubles_linear() {
        let src = from_fn(16, 1, |x, _| {
            let v = (x * 8) as u8;
            [v, v / 2, v / 3, 255 - x as u8]
        });
        let out = run("pcCameraRaw", json!({"exposure": 1.0}), &src);
        for x in 0..16 {
            let (a, b) = (px(&src, x, 0), px(&out, x, 0));
            for c in 0..3 {
                let doubled = 2.0 * srgb_to_linear(f64::from(a[c]) / 255.0);
                let want = (linear_to_srgb(doubled) * 255.0).round() as i32;
                assert!((i32::from(b[c]) - want).abs() <= 1, "{x} {c}: {b:?} {want}");
            }
            assert_eq!(b[3], a[3]);
        }
    }

    /// Every control sits on a page of the four tabs (R11), and the dialog's values reach the
    /// develop settings, clamped.
    #[test]
    fn camera_raw_tabs_cover_every_control() {
        let def = crate::effect::find("pcCameraRaw").unwrap();
        let ui = def.indirect.unwrap();
        let paged: Vec<&str> = ui
            .tabs
            .iter()
            .flat_map(|s| {
                std::iter::once(s.key).chain(s.tabs.iter().flat_map(|t| t.keys.iter().copied()))
            })
            .collect();
        let keys: Vec<&str> = def.controls.iter().map(|c| c.key()).collect();
        for k in &keys {
            assert!(paged.contains(k), "{k} is on no page");
        }
        let named = ui.controls.iter().map(|(k, _)| k).chain(ui.transient_keys);
        for k in paged.iter().chain(named) {
            assert!(keys.contains(k), "{k} names no control");
        }
        let captions: Vec<_> = ui.tabs[0].tabs.iter().map(|t| t.caption).collect();
        assert_eq!(
            captions,
            ["Basic", "Tone", "Curve", "HSL", "Grading", "Detail"]
        );
        let mut v = crate::effect::defaults(&def);
        assert_eq!(settings(&v), CameraRaw::default());
        v.insert("satAqua".into(), json!(-40));
        v.insert("exposure".into(), json!(300));
        let p = settings(&v);
        assert_eq!((p.hsl_sat[4], p.exposure), (-40.0, 5.0));
    }

    /// Load Preset... sets the controls a preset holds and keeps the rest; a point curve in it
    /// turns the curves on, a linear one off; a file that is not a Camera Raw preset is refused
    /// with a message and changes nothing.
    #[test]
    fn camera_raw_loads_a_develop_preset() {
        let def = crate::effect::find("pcCameraRaw").unwrap();
        let load = def.indirect.unwrap().preset.unwrap().load;
        let mut v = crate::effect::defaults(&def);
        v.insert("sharpenAmount".into(), json!(40));
        let xmp = |attrs: &str, body: &str| {
            format!(
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="{}" {attrs}>{body}</rdf:Description></rdf:RDF></x:xmpmeta>"#,
                xmp::CRS_NS
            )
        };
        let preset = xmp(
            r#"crs:Exposure2012="+0.35" crs:Clarity="+8" crs:ParametricLights="+12" crs:SaturationAdjustmentOrange="+8" crs:SplitToningShadowHue="215" crs:SplitToningShadowSaturation="18" crs:LensProfileEnable="1""#,
            "<crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 20</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>",
        );
        load(preset.as_bytes(), &mut v).unwrap();
        let f = |k: &str| num(&v, k) as f32;
        assert_eq!(v["exposure"], json!(0.35));
        assert_eq!(
            [f("clarity"), f("curveLights"), f("satOrange")],
            [8.0, 12.0, 8.0]
        );
        assert_eq!([f("gradeShadowHue"), f("gradeShadowSat")], [215.0, 18.0]);
        assert_eq!(f("sharpenAmount"), 40.0, "a setting the file lacks is kept");
        assert!(flag(&v, "pointCurve"));
        assert_eq!(settings(&v).point_curve, vec![[0.0, 20.0], [255.0, 255.0]]);
        // The check box is enabled while there is a curve to turn off.
        let enabled = def.indirect.unwrap().enabled;
        assert!(enabled(&v, "pointCurve"));
        let linear = xmp(
            "",
            "<crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>",
        );
        load(linear.as_bytes(), &mut v).unwrap();
        assert!(!flag(&v, "pointCurve") && !enabled(&v, "pointCurve"));
        assert_eq!(num(&v, "exposure") as f32, 0.35);
        let kept = v.clone();
        for bad in [
            &b"\x89PNG\r\n"[..],
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>",
        ] {
            assert!(load(bad, &mut v).is_err());
            assert_eq!(v, kept);
        }
    }

    /// Every stage at once on degenerate layers: no panic, alpha kept.
    #[test]
    fn camera_raw_every_stage_on_tiny_layers() {
        let def = crate::effect::find("pcCameraRaw").unwrap();
        let mut v = crate::effect::defaults(&def);
        for c in &def.controls {
            if let Control::Range { key, max, .. } = c {
                v.insert((*key).into(), json!(max * 0.6));
            }
        }
        for (w, h) in [(1, 1), (1, 7), (7, 1), (3, 2)] {
            let src = from_fn(w, h, |x, y| [(x * 30) as u8, (y * 30) as u8, 90, 128]);
            let out = crate::effect::apply(&src, &def, &v, &EffectEnv::whole(w, h));
            assert!(
                out.data.iter().skip(3).step_by(4).all(|a| *a == 128),
                "{w}x{h}"
            );
        }
    }

    /// Band sliders run from grey 128 to the band's hue; the Basic page holds ACR's eight.
    #[test]
    fn camera_raw_hsl_gradients_and_basic_order() {
        let ui = crate::effect::find("pcCameraRaw")
            .unwrap()
            .indirect
            .unwrap();
        let track = |k: &str| ui.controls.iter().find(|(n, _)| *n == k).unwrap().1.track;
        assert_eq!(track("hueRed"), Some(&[0x808080, 0xFF0000][..]));
        assert_eq!(track("lumMagenta"), Some(&[0x808080, 0xFF00FF][..]));
        assert_eq!(track("satAqua"), Some(&[0x808080, 0x00FFFF][..]));
        assert_eq!(
            ui.tabs[0].tabs[0].keys[..3],
            ["temperature", "tint", "exposure"]
        );
        assert_eq!(ui.tabs[0].tabs[0].keys.len(), 8);
    }

    /// A cancel set before `detail` returns early: the develop reports it and the layer is kept.
    #[test]
    fn camera_raw_detail_polls_cancel() {
        let mut px = vec![[0.4f32, 0.5, 0.6, 1.0]; 64];
        let p = CameraRaw {
            noise_luminance: 50.0,
            noise_color: 50.0,
            sharpen_amount: 50.0,
            ..CameraRaw::default()
        };
        let flag = std::sync::atomic::AtomicBool::new(true);
        let stop = Interrupt(Some(&flag));
        assert!(camera_raw::develop(&mut px, 8, 8, &p, stop).is_err());
        assert!(px.iter().all(|q| *q == [0.4, 0.5, 0.6, 1.0]));
    }

    /// A cancelled develop leaves the layer untouched.
    #[test]
    fn camera_raw_stops_when_cancelled() {
        let src = ramp();
        let def = crate::effect::find("pcCameraRaw").unwrap();
        let mut v = crate::effect::defaults(&def);
        v.insert("shadows".into(), json!(80));
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut env = EffectEnv::whole(16, 16);
        env.cancel = Some(&flag);
        assert_eq!(render(&src, &v, &env).data, src.data);
    }
}
