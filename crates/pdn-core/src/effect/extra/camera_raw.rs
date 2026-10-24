//! Photocraft phase spec: S§4.2, the Tier 3 dialogs; research R§B (root · Camera Raw Filter)
//!
//! Camera Raw: the develop pipeline (`extra::fx::camera_raw`) on the current layer's pixels, in
//! the Effect Config dialog with Basic, Tone, HSL and Detail tabs (an `IndirectUi`, Ruling R11);
//! the canvas is its live preview. Raw-file decoding is not part of it.

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::ui::{ControlUi, IndirectUi, Tab, TabStrip};
use crate::effect::{Category, Control, EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::extra::fx::camera_raw::{self, CameraRaw};
use crate::extra::interrupt::Interrupt;
use crate::extra::shim;

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
static CONTROL_UI: [(&str, ControlUi); 26] = {
    let mut c = [("", ControlUi::DEFAULT); 26];
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
                    caption: "HSL",
                    keys: &["hslTab"],
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
    transient_keys: &["tab", "hslTab", "detailTab"],
    controls: &CONTROL_UI,
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
    ];
    for keys in &HSL {
        controls.extend(keys.iter().zip(BANDS).map(|(k, l)| signed(k, l)));
    }
    controls.extend([
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

/// The dialog's values as develop settings. Exposure and the noise amounts are clamped to their
/// slider ranges, as upstream's command does: they become `2^exposure`, a filter radius and a
/// blur sigma, so a stored out-of-range value would overflow or mean unbounded work.
fn settings(v: &Values) -> CameraRaw {
    let f = |k: &str| num(v, k) as f32;
    let bands = |i: usize| std::array::from_fn(|b| f(HSL[i][b]));
    CameraRaw {
        temperature: f("temperature"),
        tint: f("tint"),
        exposure: f("exposure").clamp(-5.0, 5.0),
        contrast: f("contrast"),
        highlights: f("highlights"),
        shadows: f("shadows"),
        whites: f("whites"),
        blacks: f("blacks"),
        texture: f("texture"),
        clarity: f("clarity"),
        dehaze: f("dehaze"),
        vibrance: f("vibrance"),
        saturation: f("saturation"),
        hsl_hue: bands(0),
        hsl_sat: bands(1),
        hsl_lum: bands(2),
        sharpen_amount: f("sharpenAmount"),
        sharpen_radius: f("sharpenRadius"),
        sharpen_detail: f("sharpenDetail"),
        sharpen_masking: f("sharpenMasking"),
        noise_luminance: f("noiseLuminance").clamp(0.0, 100.0),
        noise_luminance_detail: f("noiseLuminanceDetail").clamp(0.0, 100.0),
        noise_color: f("noiseColor").clamp(0.0, 100.0),
        noise_color_detail: f("noiseColorDetail").clamp(0.0, 100.0),
    }
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
            ("sharpenAmount", 100.0),
            ("noiseLuminance", 60.0),
            ("noiseColor", 60.0),
        ] {
            let out = run("pcCameraRaw", json!({ k: val }), &busy);
            assert_ne!(out.data, busy.data, "{k} did nothing");
        }
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
        assert_eq!(captions, ["Basic", "Tone", "HSL", "Detail"]);
        let mut v = crate::effect::defaults(&def);
        assert_eq!(settings(&v), CameraRaw::default());
        v.insert("satAqua".into(), json!(-40));
        v.insert("exposure".into(), json!(300));
        let p = settings(&v);
        assert_eq!((p.hsl_sat[4], p.exposure), (-40.0, 5.0));
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
