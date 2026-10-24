//! Photocraft phase spec: S§4.2; research R§B (Photo: Lens Correction; Adaptive Wide Angle deferred, R71)
//!
//! The Extra Photo effect Lens Correction (`extra::fx::lens`). The optical centre and radius are
//! those of `env.bounds` (the selection bounds ∩ the layer); the source is read from the whole
//! layer, and only `env.bounds` is written.

use crate::canvas::ImageData;
use crate::effect::{
    Category, Control, EffectDef, EffectEnv, EffectKind, Values, choice, flag, num, range,
};
use crate::extra::fx::lens::{self, EdgeMode, LensCorrection};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::shim;

const EDGES: &[(&str, &str)] = &[
    ("transparency", "Transparency"),
    ("edgeExtension", "Edge Extension"),
    ("black", "Black"),
    ("white", "White"),
];

const PROFILES: &[(&str, &str)] = &[("none", "None"), ("generic", "Generic Lens")];

pub fn effects() -> Vec<EffectDef> {
    let def = |id, icon, controls, render| EffectDef {
        id,
        icon,
        category: Category::Effect("photo"),
        controls,
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: None,
        indirect: None,
    };
    vec![def(
        "pcLensCorrection",
        "lens_correction_effect.png",
        vec![
            Control::Select {
                key: "profile",
                label: "Profile",
                default: "none",
                choices: PROFILES,
            },
            range(
                "focalLength",
                "Focal Length (35 mm equivalent)",
                12.0,
                200.0,
                50.0,
            ),
            Control::Checkbox {
                key: "correctDistortion",
                label: "Correct Distortion",
                default: true,
            },
            Control::Checkbox {
                key: "correctVignette",
                label: "Correct Vignette",
                default: true,
            },
            Control::Checkbox {
                key: "correctCA",
                label: "Correct Chromatic Aberration",
                default: true,
            },
            range("distortion", "Remove Distortion", -100.0, 100.0, 0.0),
            range("redCyan", "Fix Red/Cyan Fringe", -100.0, 100.0, 0.0),
            range("blueYellow", "Fix Blue/Yellow Fringe", -100.0, 100.0, 0.0),
            range("vignetteAmount", "Vignette Amount", -100.0, 100.0, 0.0),
            range("vignetteMidpoint", "Vignette Midpoint", 0.0, 100.0, 50.0),
            range("vertical", "Vertical Perspective", -100.0, 100.0, 0.0),
            range("horizontal", "Horizontal Perspective", -100.0, 100.0, 0.0),
            range("angle", "Angle", -180.0, 180.0, 0.0),
            range("scale", "Scale", 50.0, 150.0, 100.0),
            Control::Checkbox {
                key: "autoScale",
                label: "Auto Scale Image",
                default: false,
            },
            Control::Select {
                key: "edge",
                label: "Edge",
                default: "transparency",
                choices: EDGES,
            },
        ],
        lens_correction,
    )]
}

fn frame(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.right(), b.bottom())
}

/// The settings a dialog's values describe (engine `lens_params`, minus EXIF and Straighten).
fn params(v: &Values, frame: Rect, layer: Rect) -> LensCorrection {
    let mut lc = LensCorrection {
        correct_distortion: flag(v, "correctDistortion"),
        correct_vignette: flag(v, "correctVignette"),
        correct_ca: flag(v, "correctCA"),
        distortion: num(v, "distortion").clamp(-100.0, 100.0),
        red_cyan: num(v, "redCyan").clamp(-100.0, 100.0),
        blue_yellow: num(v, "blueYellow").clamp(-100.0, 100.0),
        vignette_amount: num(v, "vignetteAmount").clamp(-100.0, 100.0),
        vignette_midpoint: num(v, "vignetteMidpoint").clamp(0.0, 100.0),
        vertical: num(v, "vertical").clamp(-100.0, 100.0),
        horizontal: num(v, "horizontal").clamp(-100.0, 100.0),
        angle: num(v, "angle"),
        scale: num(v, "scale").clamp(50.0, 150.0),
        ..Default::default()
    };
    lc.edge = match choice(v, "edge") {
        "edgeExtension" => EdgeMode::Extension,
        "black" => EdgeMode::Color([0.0, 0.0, 0.0, 1.0]),
        "white" => EdgeMode::Color([1.0, 1.0, 1.0, 1.0]),
        _ => EdgeMode::Transparency,
    };
    if choice(v, "profile") == "generic" {
        lc.profile = Some(lens::generic_profile(num(v, "focalLength")));
    }
    if flag(v, "autoScale") {
        lc.scale = lens::auto_scale(&lc, frame, layer);
    }
    lc
}

fn lens_correction(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let frame = frame(env);
    let layer = Rect::new(0, 0, src.width as i32, src.height as i32);
    let out = lens::correct(
        &shim::to_f32(src),
        src.width as usize,
        frame,
        &params(v, frame, layer),
        Interrupt(env.cancel),
    );
    shim::from_f32(&out, src.width, src.height)
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::test_util::{rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px};
    use crate::effect::{EffectEnv, apply, defaults, find};

    fn gradient(w: u32, h: u32) -> crate::canvas::ImageData {
        from_fn(w, h, |x, y| {
            [(x * 4) as u8, (y * 4) as u8, 100, 255 - (x % 5) as u8]
        })
    }

    #[test]
    fn lens_correction_zero_is_identity() {
        let def = find("pcLensCorrection").unwrap();
        let src = gradient(48, 32);
        let out = apply(&src, &def, &defaults(&def), &EffectEnv::whole(48, 32));
        assert_eq!(out.data, src.data);
    }

    /// Remove Distortion changes the picture and leaves the centre where it was.
    #[test]
    fn sliders_move_pixels() {
        let src = gradient(48, 32);
        let env = EffectEnv::whole(48, 32);
        let lens = find("pcLensCorrection").unwrap();
        let mut v = defaults(&lens);
        v.insert("distortion".into(), 60.0.into());
        v.insert("vignetteAmount".into(), (-100.0).into());
        let out = apply(&src, &lens, &v, &env);
        assert_ne!(out.data, src.data);
        assert!(px(&out, 24, 16)[3] > 250);
    }

    /// An opaque layer with an off-centre selection: the optical centre is the selection's, so
    /// its centre pixel is a fixed point of a symmetric distortion; the layer centre is not.
    /// The selection's corner samples layer pixels just outside it, so it stays opaque
    /// (clamping to the selection would clear it).
    #[test]
    fn anchors_on_selection_bounds() {
        let (w, h) = (64u32, 48u32);
        let src = from_fn(w, h, |x, y| [(x * 4) as u8, (y * 5) as u8, 90, 255]);
        let lens = find("pcLensCorrection").unwrap();
        let mut v = defaults(&lens);
        v.insert("distortion".into(), (-80.0).into());
        let (mask, polys) = rect_selection(w, h, 36, 24, 60, 44);
        let env = with_selection(w, h, &mask, &polys);
        let out = apply(&src, &lens, &v, &env);
        // Selection centre (48, 34) maps to itself.
        assert_eq!(px(&out, 48, 34), px(&src, 48, 34));
        // Corner pixels keep their alpha.
        assert_eq!(px(&out, 36, 24)[3], 255);
        assert_eq!(px(&out, 59, 43)[3], 255);
        // The effect did change pixels inside, and none outside.
        assert_ne!(px(&out, 40, 26), px(&src, 40, 26));
        assert_eq!(px(&out, 2, 2), px(&src, 2, 2));
        // A layer-centre anchor would leave (32, 24) fixed instead; it is outside the selection
        // and untouched either way, so check the selection centre moved under that anchor.
        let whole = apply(&src, &lens, &v, &EffectEnv::whole(w, h));
        assert_ne!(px(&whole, 48, 34), px(&src, 48, 34));
    }
}
