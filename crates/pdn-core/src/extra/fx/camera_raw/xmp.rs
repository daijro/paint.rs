//! Ported from PhotoCraft crates/io/src/camera_raw_xmp.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: load only (Camera Raw's Load Preset...: `save`, `signed`, `escape` and the round-trip test dropped); grain, post-crop vignette and its style, which paint.rs's Camera Raw does not have, are ignored with the other unsupported settings; `validate_curve` and `MAX_CURVE_POINTS` moved here from crates/algo/src/camera_raw.rs (their doc says what they check, without upstream's note on stored Smart Filters); `Clarity`, the pre-2012 name LightCraft's example preset writes, is read as Clarity when the file has no `Clarity2012`; tests: `reads_a_photoshop_preset` finds GrainAmount ignored where upstream checks it, `load_applies_only_the_settings_in_the_file` and `malformed_input_is_an_error_and_leaves_settings_alone` unchanged, `reads_the_lightcraft_example_preset` added.
//!
//! Camera Raw settings files (`.xmp`), as Adobe Camera Raw, Lightroom and LightCraft write develop
//! presets: the `crs:` namespace (`http://ns.adobe.com/camera-raw-settings/1.0/`) on an
//! `x:xmpmeta/rdf:RDF/rdf:Description`, as attributes or as child elements.
//!
//! Loading applies only the settings the file holds, over the current ones, as Camera Raw's
//! Load Settings does; settings Camera Raw here has no control for (lens corrections, calibration,
//! geometry, defringe, vignette, grain, …) are ignored and named in the report.
//!
//! White balance: a Camera Raw Filter on a rendered image uses the relative
//! `IncrementalTemperature` / `IncrementalTint` (−100..=100), which are the Temperature and Tint
//! here. The absolute `Temperature` (kelvin) / `Tint` of raw-file settings have no counterpart and
//! are ignored.

use super::CameraRaw;

/// The Camera Raw settings namespace.
pub const CRS_NS: &str = "http://ns.adobe.com/camera-raw-settings/1.0/";
const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// Largest settings file read; real ones are a few kilobytes.
pub const MAX_XMP_BYTES: usize = 4 * 1024 * 1024;
/// Most points a Camera Raw point curve accepts.
pub const MAX_CURVE_POINTS: usize = 16;

/// Checks a point curve from a file: empty (linear) or 2..=16 finite points in 0..=255 with
/// inputs increasing by at least one level.
pub fn validate_curve(points: &[[f32; 2]]) -> Result<(), String> {
    if points.len() == 1 {
        return Err("a nonempty curve needs at least two points".into());
    }
    if points.len() > MAX_CURVE_POINTS {
        return Err(format!("at most {MAX_CURVE_POINTS} curve points"));
    }
    if !points
        .iter()
        .flatten()
        .all(|v| v.is_finite() && (0.0..=255.0).contains(v))
    {
        return Err("curve coordinates must be finite and in 0..=255".into());
    }
    if points.windows(2).any(|w| w[1][0] - w[0][0] < 1.0) {
        return Err("curve inputs must increase by at least one level".into());
    }
    Ok(())
}

type Field = fn(&mut CameraRaw) -> &mut f32;

/// `crs:` scalar name, Camera Raw field, and the slider range values are clamped to.
#[rustfmt::skip]
const SCALARS: &[(&str, Field, f32, f32)] = &[
    ("IncrementalTemperature", |p| &mut p.temperature, -100.0, 100.0),
    ("IncrementalTint", |p| &mut p.tint, -100.0, 100.0),
    ("Exposure2012", |p| &mut p.exposure, -5.0, 5.0),
    ("Contrast2012", |p| &mut p.contrast, -100.0, 100.0),
    ("Highlights2012", |p| &mut p.highlights, -100.0, 100.0),
    ("Shadows2012", |p| &mut p.shadows, -100.0, 100.0),
    ("Whites2012", |p| &mut p.whites, -100.0, 100.0),
    ("Blacks2012", |p| &mut p.blacks, -100.0, 100.0),
    ("Texture", |p| &mut p.texture, -100.0, 100.0),
    ("Clarity2012", |p| &mut p.clarity, -100.0, 100.0),
    ("Clarity", |p| &mut p.clarity, -100.0, 100.0),
    ("Dehaze", |p| &mut p.dehaze, -100.0, 100.0),
    ("Vibrance", |p| &mut p.vibrance, -100.0, 100.0),
    ("Saturation", |p| &mut p.saturation, -100.0, 100.0),
    ("ParametricShadows", |p| &mut p.curve_shadows, -100.0, 100.0),
    ("ParametricDarks", |p| &mut p.curve_darks, -100.0, 100.0),
    ("ParametricLights", |p| &mut p.curve_lights, -100.0, 100.0),
    ("ParametricHighlights", |p| &mut p.curve_highlights, -100.0, 100.0),
    ("ParametricShadowSplit", |p| &mut p.curve_splits[0], 0.0, 100.0),
    ("ParametricMidtoneSplit", |p| &mut p.curve_splits[1], 0.0, 100.0),
    ("ParametricHighlightSplit", |p| &mut p.curve_splits[2], 0.0, 100.0),
    ("HueAdjustmentRed", |p| &mut p.hsl_hue[0], -100.0, 100.0),
    ("HueAdjustmentOrange", |p| &mut p.hsl_hue[1], -100.0, 100.0),
    ("HueAdjustmentYellow", |p| &mut p.hsl_hue[2], -100.0, 100.0),
    ("HueAdjustmentGreen", |p| &mut p.hsl_hue[3], -100.0, 100.0),
    ("HueAdjustmentAqua", |p| &mut p.hsl_hue[4], -100.0, 100.0),
    ("HueAdjustmentBlue", |p| &mut p.hsl_hue[5], -100.0, 100.0),
    ("HueAdjustmentPurple", |p| &mut p.hsl_hue[6], -100.0, 100.0),
    ("HueAdjustmentMagenta", |p| &mut p.hsl_hue[7], -100.0, 100.0),
    ("SaturationAdjustmentRed", |p| &mut p.hsl_sat[0], -100.0, 100.0),
    ("SaturationAdjustmentOrange", |p| &mut p.hsl_sat[1], -100.0, 100.0),
    ("SaturationAdjustmentYellow", |p| &mut p.hsl_sat[2], -100.0, 100.0),
    ("SaturationAdjustmentGreen", |p| &mut p.hsl_sat[3], -100.0, 100.0),
    ("SaturationAdjustmentAqua", |p| &mut p.hsl_sat[4], -100.0, 100.0),
    ("SaturationAdjustmentBlue", |p| &mut p.hsl_sat[5], -100.0, 100.0),
    ("SaturationAdjustmentPurple", |p| &mut p.hsl_sat[6], -100.0, 100.0),
    ("SaturationAdjustmentMagenta", |p| &mut p.hsl_sat[7], -100.0, 100.0),
    ("LuminanceAdjustmentRed", |p| &mut p.hsl_lum[0], -100.0, 100.0),
    ("LuminanceAdjustmentOrange", |p| &mut p.hsl_lum[1], -100.0, 100.0),
    ("LuminanceAdjustmentYellow", |p| &mut p.hsl_lum[2], -100.0, 100.0),
    ("LuminanceAdjustmentGreen", |p| &mut p.hsl_lum[3], -100.0, 100.0),
    ("LuminanceAdjustmentAqua", |p| &mut p.hsl_lum[4], -100.0, 100.0),
    ("LuminanceAdjustmentBlue", |p| &mut p.hsl_lum[5], -100.0, 100.0),
    ("LuminanceAdjustmentPurple", |p| &mut p.hsl_lum[6], -100.0, 100.0),
    ("LuminanceAdjustmentMagenta", |p| &mut p.hsl_lum[7], -100.0, 100.0),
    // Color Grading keeps the split-toning names for the shadow and highlight wheels' hue and
    // saturation, and the balance.
    ("SplitToningShadowHue", |p| &mut p.grade_shadows.hue, 0.0, 360.0),
    ("SplitToningShadowSaturation", |p| &mut p.grade_shadows.sat, 0.0, 100.0),
    ("ColorGradeShadowLum", |p| &mut p.grade_shadows.lum, -100.0, 100.0),
    ("ColorGradeMidtoneHue", |p| &mut p.grade_midtones.hue, 0.0, 360.0),
    ("ColorGradeMidtoneSat", |p| &mut p.grade_midtones.sat, 0.0, 100.0),
    ("ColorGradeMidtoneLum", |p| &mut p.grade_midtones.lum, -100.0, 100.0),
    ("SplitToningHighlightHue", |p| &mut p.grade_highlights.hue, 0.0, 360.0),
    ("SplitToningHighlightSaturation", |p| &mut p.grade_highlights.sat, 0.0, 100.0),
    ("ColorGradeHighlightLum", |p| &mut p.grade_highlights.lum, -100.0, 100.0),
    ("ColorGradeGlobalHue", |p| &mut p.grade_global.hue, 0.0, 360.0),
    ("ColorGradeGlobalSat", |p| &mut p.grade_global.sat, 0.0, 100.0),
    ("ColorGradeGlobalLum", |p| &mut p.grade_global.lum, -100.0, 100.0),
    ("ColorGradeBlending", |p| &mut p.grade_blending, 0.0, 100.0),
    ("SplitToningBalance", |p| &mut p.grade_balance, -100.0, 100.0),
    ("Sharpness", |p| &mut p.sharpen_amount, 0.0, 150.0),
    ("SharpenRadius", |p| &mut p.sharpen_radius, 0.5, 3.0),
    ("SharpenDetail", |p| &mut p.sharpen_detail, 0.0, 100.0),
    ("SharpenEdgeMasking", |p| &mut p.sharpen_masking, 0.0, 100.0),
    ("LuminanceSmoothing", |p| &mut p.noise_luminance, 0.0, 100.0),
    ("LuminanceNoiseReductionDetail", |p| &mut p.noise_luminance_detail, 0.0, 100.0),
    ("ColorNoiseReduction", |p| &mut p.noise_color, 0.0, 100.0),
    ("ColorNoiseReductionDetail", |p| &mut p.noise_color_detail, 0.0, 100.0),
];

type Curve = fn(&mut CameraRaw) -> &mut Vec<[f32; 2]>;
const CURVES: &[(&str, Curve)] = &[
    ("ToneCurvePV2012", |p| &mut p.point_curve),
    ("ToneCurvePV2012Red", |p| &mut p.point_curve_red),
    ("ToneCurvePV2012Green", |p| &mut p.point_curve_green),
    ("ToneCurvePV2012Blue", |p| &mut p.point_curve_blue),
];

/// Bookkeeping fields Camera Raw writes that carry no setting.
const METADATA: &[&str] = &[
    "Version",
    "ProcessVersion",
    "HasSettings",
    "HasCrop",
    "WhiteBalance",
    "ToneCurveName2012",
    "CameraProfile",
    "PresetType",
    "UUID",
    "SupportsAmount",
    "SupportsColor",
    "SupportsMonochrome",
    "SupportsHighDynamicRange",
    "SupportsNormalDynamicRange",
    "SupportsSceneReferred",
    "SupportsOutputReferred",
    "CameraModelRestriction",
    "Copyright",
    "ContactInfo",
    "Name",
    "ShortName",
    "SortName",
    "Group",
    "Description",
];

/// What [`load`] applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadReport {
    /// How many settings were applied.
    pub applied: usize,
    /// `crs:` settings without a Camera Raw control (set to something other than their neutral
    /// value), or with an unusable value, in file order.
    pub ignored: Vec<String>,
}

fn number(s: &str) -> Option<f32> {
    let s = s.trim();
    let v: f32 = s.strip_prefix('+').unwrap_or(s).parse().ok()?;
    v.is_finite().then_some(v)
}

/// `"0, 0"` items of an `rdf:Seq` → curve points.
fn curve(node: roxmltree::Node<'_, '_>) -> Option<Vec<[f32; 2]>> {
    let seq = node.children().find(|c| {
        c.is_element() && c.tag_name().namespace() == Some(RDF_NS) && c.tag_name().name() == "Seq"
    })?;
    let mut points = Vec::new();
    for li in seq
        .children()
        .filter(|c| c.is_element() && c.tag_name().name() == "li")
    {
        let text = li.text().unwrap_or("");
        let (x, y) = text.split_once(',')?;
        points.push([number(x)?, number(y)?]);
    }
    validate_curve(&points).ok()?;
    // Camera Raw's linear curve is the identity's two end points; the empty one here.
    if points == [[0.0, 0.0], [255.0, 255.0]] {
        points.clear();
    }
    Some(points)
}

/// An unsupported setting at its neutral value changes nothing, so it isn't reported: zero,
/// `False`, and the few neutral values that aren't zero.
fn neutral(name: &str, value: &str) -> bool {
    let v = value.trim();
    if v.eq_ignore_ascii_case("false") || number(v) == Some(0.0) {
        return true;
    }
    match name {
        "GrainSeed" => true,
        "VignetteMidpoint" => number(v) == Some(50.0),
        "PerspectiveScale" => number(v) == Some(100.0),
        _ => name.starts_with("Defringe") && name.contains("Hue"),
    }
}

fn apply(params: &mut CameraRaw, report: &mut LoadReport, name: &str, value: &str) {
    if let Some((_, field, lo, hi)) = SCALARS.iter().find(|(n, ..)| *n == name) {
        match number(value) {
            Some(v) => {
                *field(params) = v.clamp(*lo, *hi);
                report.applied += 1;
            }
            None => report.ignored.push(name.to_string()),
        }
    } else if !METADATA.contains(&name) && !neutral(name, value) {
        report.ignored.push(name.to_string());
    }
}

/// Apply a Camera Raw settings file over `params`. Fails on input that isn't XMP or holds no
/// Camera Raw settings; never panics on malformed input.
pub fn load(params: &mut CameraRaw, bytes: &[u8]) -> Result<LoadReport, String> {
    if bytes.len() > MAX_XMP_BYTES {
        return Err("the settings file is too large".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "not a Camera Raw settings file (not UTF-8 text)".to_string())?;
    // Some writers put a byte order mark or an `<?xpacket?>` wrapper first; roxmltree takes both.
    let text = text.trim_start_matches('\u{feff}');
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| format!("not a Camera Raw settings file ({e})"))?;
    let mut next = params.clone();
    let mut report = LoadReport::default();
    let mut found = false;
    for desc in doc.descendants().filter(|n| {
        n.is_element()
            && n.tag_name().namespace() == Some(RDF_NS)
            && n.tag_name().name() == "Description"
    }) {
        // The pre-2012 Clarity only stands in for a missing Clarity2012.
        let has_2012 = desc.attribute((CRS_NS, "Clarity2012")).is_some()
            || desc
                .children()
                .any(|c| c.has_tag_name((CRS_NS, "Clarity2012")));
        let skip = |name: &str| name == "Clarity" && has_2012;
        for a in desc.attributes().filter(|a| a.namespace() == Some(CRS_NS)) {
            found = true;
            if !skip(a.name()) {
                apply(&mut next, &mut report, a.name(), a.value());
            }
        }
        for c in desc
            .children()
            .filter(|c| c.is_element() && c.tag_name().namespace() == Some(CRS_NS))
        {
            found = true;
            let name = c.tag_name().name();
            if let Some((_, field)) = CURVES.iter().find(|(n, _)| *n == name) {
                match curve(c) {
                    Some(points) => {
                        *field(&mut next) = points;
                        report.applied += 1;
                    }
                    None => report.ignored.push(name.to_string()),
                }
            } else if c.children().any(|g| g.is_element()) {
                // Structured settings (masks, looks, lens profiles…) have no counterpart.
                if !METADATA.contains(&name) {
                    report.ignored.push(name.to_string());
                }
            } else if !skip(name) {
                apply(&mut next, &mut report, name, c.text().unwrap_or(""));
            }
        }
    }
    if !found {
        return Err("no Camera Raw settings in the file".into());
    }
    *params = next;
    Ok(report)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::Wheel;
    use super::*;

    /// A hand-written settings file laid out as Photoshop 26's Camera Raw Filter saves one (the
    /// field set and order follow Camera Raw 18; the values are synthetic), plus element-form
    /// settings, which XMP allows and some writers use.
    const FIXTURE: &[u8] = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000 1.000000, 0000/00/00-00:00:00        ">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
   crs:Version="18.1.1" crs:ProcessVersion="15.4" crs:WhiteBalance="Custom"
   crs:IncrementalTemperature="0" crs:IncrementalTint="+10" crs:Exposure2012="+0.05"
   crs:Contrast2012="-5" crs:Highlights2012="-20" crs:Shadows2012="+10" crs:Whites2012="+35"
   crs:Blacks2012="0" crs:Texture="+30" crs:Clarity2012="+5" crs:Vibrance="+20" crs:Saturation="-5"
   crs:ParametricShadowSplit="25" crs:ParametricMidtoneSplit="50" crs:ParametricHighlightSplit="75"
   crs:Sharpness="15" crs:SharpenRadius="+1.0" crs:LuminanceSmoothing="5"
   crs:LuminanceNoiseReductionContrast="0" crs:HueAdjustmentGreen="+25" crs:SaturationAdjustmentGreen="+20"
   crs:ColorGradeBlending="50" crs:LensProfileEnable="0" crs:VignetteAmount="-20"
   crs:PerspectiveScale="100" crs:GrainAmount="10" crs:RedHue="-5" crs:GrainSeed="2834497962"
   crs:ToneCurveName2012="Linear" crs:CameraProfile="Default Color" crs:HasSettings="True" crs:HasCrop="False">
   <crs:Dehaze>+12</crs:Dehaze>
   <crs:ToneCurvePV2012>
    <rdf:Seq>
     <rdf:li>0, 0</rdf:li>
     <rdf:li>255, 255</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012>
   <crs:ToneCurvePV2012Red>
    <rdf:Seq>
     <rdf:li>0, 0</rdf:li>
     <rdf:li>128, 140</rdf:li>
     <rdf:li>255, 255</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012Red>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;

    #[test]
    fn reads_a_photoshop_preset() {
        let mut p = CameraRaw::default();
        let r = load(&mut p, FIXTURE).unwrap();
        assert_eq!(p.tint, 10.0);
        assert_eq!(p.exposure, 0.05);
        assert_eq!(
            (p.contrast, p.highlights, p.shadows, p.whites),
            (-5.0, -20.0, 10.0, 35.0)
        );
        assert_eq!(
            (p.texture, p.clarity, p.vibrance, p.saturation),
            (30.0, 5.0, 20.0, -5.0)
        );
        assert_eq!((p.hsl_hue[3], p.hsl_sat[3]), (25.0, 20.0));
        assert_eq!((p.sharpen_amount, p.noise_luminance), (15.0, 5.0));
        assert_eq!(p.grade_blending, 50.0);
        // The linear tone curve is the empty one here; an element-form setting is read too.
        assert!(p.point_curve.is_empty());
        assert_eq!(
            p.point_curve_red,
            vec![[0.0, 0.0], [128.0, 140.0], [255.0, 255.0]]
        );
        assert_eq!(p.dehaze, 12.0);
        // Lens vignetting, grain and calibration have no counterpart.
        assert_eq!(
            r.ignored,
            ["VignetteAmount", "GrainAmount", "RedHue"].map(String::from)
        );
    }

    /// The develop preset in the "LightCraft" section of the ArtCraft store's plug-in guide,
    /// verbatim: every setting lands on its control, the legacy `Clarity` included.
    #[test]
    fn reads_the_lightcraft_example_preset() {
        const PRESET: &[u8] = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
        crs:Version="16.0"
        crs:ProcessVersion="15.4"
        crs:Exposure2012="+0.35"
        crs:Contrast2012="+15"
        crs:Highlights2012="-25"
        crs:Shadows2012="+20"
        crs:Whites2012="+10"
        crs:Blacks2012="-12"
        crs:Texture="+10"
        crs:Clarity="+8"
        crs:Dehaze="+5"
        crs:Vibrance="+12"
        crs:Saturation="-5"
        crs:ParametricShadows="-8"
        crs:ParametricDarks="+5"
        crs:ParametricLights="+12"
        crs:ParametricHighlights="-15"
        crs:HueAdjustmentRed="+5"
        crs:HueAdjustmentOrange="-3"
        crs:SaturationAdjustmentOrange="+8"
        crs:LuminanceAdjustmentOrange="+10"
        crs:SplitToningShadowHue="215"
        crs:SplitToningShadowSaturation="18"
        crs:SplitToningHighlightHue="45"
        crs:SplitToningHighlightSaturation="14" />
  </rdf:RDF>
</x:xmpmeta>"#;
        let mut p = CameraRaw::default();
        let r = load(&mut p, PRESET).unwrap();
        assert_eq!(
            r,
            LoadReport {
                applied: 23,
                ignored: vec![]
            }
        );
        let mut want = CameraRaw {
            exposure: 0.35,
            contrast: 15.0,
            highlights: -25.0,
            shadows: 20.0,
            whites: 10.0,
            blacks: -12.0,
            texture: 10.0,
            clarity: 8.0,
            dehaze: 5.0,
            vibrance: 12.0,
            saturation: -5.0,
            curve_shadows: -8.0,
            curve_darks: 5.0,
            curve_lights: 12.0,
            curve_highlights: -15.0,
            grade_shadows: Wheel {
                hue: 215.0,
                sat: 18.0,
                lum: 0.0,
            },
            grade_highlights: Wheel {
                hue: 45.0,
                sat: 14.0,
                lum: 0.0,
            },
            ..CameraRaw::default()
        };
        want.hsl_hue[..2].copy_from_slice(&[5.0, -3.0]);
        want.hsl_sat[1] = 8.0;
        want.hsl_lum[1] = 10.0;
        assert_eq!(p, want);
        // Beside a Clarity2012, the pre-2012 Clarity is ignored, in either order.
        for xmp in [
            r#"crs:Clarity="+8" crs:Clarity2012="-20""#,
            r#"crs:Clarity2012="-20" crs:Clarity="+8""#,
        ] {
            let text = format!(
                r#"<rdf:RDF xmlns:rdf="{RDF_NS}"><rdf:Description xmlns:crs="{CRS_NS}" {xmp}/></rdf:RDF>"#
            );
            let mut p = CameraRaw::default();
            load(&mut p, text.as_bytes()).unwrap();
            assert_eq!(p.clarity, -20.0, "{xmp}");
        }
    }

    #[test]
    fn load_applies_only_the_settings_in_the_file() {
        let mut p = CameraRaw {
            contrast: 40.0,
            ..CameraRaw::default()
        };
        let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><crs:Exposure2012>-9</crs:Exposure2012>
            <crs:Clarity2012>abc</crs:Clarity2012></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let r = load(&mut p, xmp).unwrap();
        assert_eq!((p.exposure, p.contrast, p.clarity), (-5.0, 40.0, 0.0));
        assert_eq!(
            r,
            LoadReport {
                applied: 1,
                ignored: vec!["Clarity2012".into()]
            }
        );
    }

    #[test]
    fn malformed_input_is_an_error_and_leaves_settings_alone() {
        let before = CameraRaw {
            exposure: 0.5,
            ..CameraRaw::default()
        };
        for bad in [
            &b""[..],
            b"\xff\xfe\x00garbage",
            b"<x:xmpmeta><unclosed",
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>",
            b"<?xml version=\"1.0\"?><!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>",
        ] {
            let mut p = before.clone();
            assert!(load(&mut p, bad).is_err());
            assert_eq!(p, before);
        }
        // A broken curve or NaN is skipped, not fatal.
        let xmp = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description
            xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Contrast2012="NaN"><crs:ToneCurvePV2012><rdf:Seq>
            <rdf:li>10</rdf:li></rdf:Seq></crs:ToneCurvePV2012></rdf:Description></rdf:RDF>"#;
        let mut p = before.clone();
        let r = load(&mut p, xmp).unwrap();
        assert_eq!(p, before);
        assert_eq!(
            r.ignored,
            vec!["Contrast2012".to_string(), "ToneCurvePV2012".to_string()]
        );
    }
}
