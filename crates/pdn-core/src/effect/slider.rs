//! Native effect-slider coordinates and exact replacement-control roles, adjustments-and-effect-framework §§7.4–8.
use super::{Control, EffectDef};

#[derive(Clone, Copy, Debug)]
pub struct SliderKeys {
    pub decimals: u32,
    pub small: f64,
    pub large: f64,
    pub exponential: bool,
}

impl SliderKeys {
    pub const INTEGER: Self = Self {
        decimals: 0,
        small: 1.0,
        large: 5.0,
        exponential: false,
    };

    /// Native integer coordinate, shared by keyboard and pointer input (§7.5).
    pub fn coordinate(self, value: f64, min: f64, max: f64) -> i64 {
        if !self.exponential {
            return (value * 10_f64.powi(self.decimals as i32)).round() as i64;
        }
        let mapped = signed_map(value, min, max, |v, a, b| {
            (((v - a) / (b - a)).abs().sqrt() * (b - a) + a)
                .max(a)
                .min(b)
        });
        (mapped * 10_f64.powi(self.decimals as i32 + 1)).trunc() as i64
    }

    /// Inverse native coordinate conversion; callers clamp to the property bounds.
    pub fn value(self, coordinate: i64, min: f64, max: f64) -> f64 {
        if !self.exponential {
            return coordinate as f64 / 10_f64.powi(self.decimals as i32);
        }
        let v = coordinate as f64 / 10_f64.powi(self.decimals as i32 + 1);
        signed_map(v, min, max, |v, a, b| {
            (((v - a) / (b - a)).powi(2) * (b - a) + a).max(a).min(b)
        })
    }

    /// Convert the configured absolute step, add it in slider coordinates, then clamp.
    pub fn stepped(self, value: f64, min: f64, max: f64, increase: bool, page: bool) -> f64 {
        let step = self.coordinate(if page { self.large } else { self.small }, min, max);
        if step == 0 {
            return value;
        }
        let at = self.coordinate(value, min, max);
        let next = if increase {
            at.saturating_add(step)
        } else {
            at.saturating_sub(step)
        };
        let next = next.clamp(
            self.coordinate(min, min, max),
            self.coordinate(max, min, max),
        );
        self.value(next, min, max).clamp(min, max)
    }
}

// Zero is special even for positive minima; signed intervals use each side's own extent (§7.5).
fn signed_map(value: f64, min: f64, max: f64, map: impl Fn(f64, f64, f64) -> f64) -> f64 {
    if value == 0.0 {
        0.0
    } else if min >= 0.0 {
        map(value, min, max)
    } else if max <= 0.0 {
        -map(-value, -min, -max)
    } else if value < 0.0 {
        -map(-value, 0.0, -min)
    } else {
        map(value, 0.0, max)
    }
}

/// The actual registered control kind, independent of labels, numeric precision or bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeRole {
    Slider,
    BuiltinAngle,
    PluginAngle,
}

/// R161: scalar AngleChooser substitutes keep their property pointer grids. Future plugin
/// registrations must declare their exact role here; vector/roll companions remain sliders.
pub fn range_role(effect: &str, key: &str) -> RangeRole {
    match (effect, key) {
        (
            "dentsEffect"
            | "dropShadowEffect"
            | "embossEffect"
            | "juliaFractalEffect"
            | "mandelbrotFractalEffect"
            | "motionBlurEffect"
            | "radialBlurEffect"
            | "reliefEffect"
            | "straightenEffect",
            "angle",
        )
        | ("fragmentEffect" | "tileEffect", "rotation") => RangeRole::BuiltinAngle,
        (
            "bbInnerShadowObject"
            | "bbInnerShadowSelection"
            | "bbDropShadowPlus"
            | "bbBevelObject"
            | "bbBevelSelection"
            | "bbEdgeDetectClassic"
            | "bbPolygonStars",
            "angle",
        ) => RangeRole::PluginAngle,
        _ => RangeRole::Slider,
    }
}

/// Bind native slider metadata at the registry boundary; numeric increments stay independent.
/// §8's omitted actual sliders inherit integer defaults unless their double kind is listed.
pub(super) fn configure_builtin(def: &mut EffectDef) {
    for control in &mut def.controls {
        let Control::Range { key, keyboard, .. } = control else {
            continue;
        };
        let (p, small, large, exp) = match (def.id, *key) {
            ("blurEffect" | "bokehEffect" | "squareBlurEffect", "radius") => (1, 1.0, 5.0, true),
            ("blurEffect" | "bokehEffect" | "squareBlurEffect", "gamma") => (2, 0.05, 0.2, true),
            ("dropShadowEffect", "blur") => (1, 1.0, 5.0, true),
            ("dropShadowEffect", "distance") => (1, 1.0, 5.0, false),
            ("dropShadowEffect", "opacity") | ("cloudsEffect", "roughness") => {
                (2, 0.05, 0.25, false)
            }
            ("edgeDetectEffect", "strength") | ("frostedGlassEffect", "diffusion") => {
                (2, 0.05, 0.2, false)
            }
            ("edgeDetectEffect", "blurring") => (2, 0.25, 1.0, true),
            ("highlightsAndShadowsEffect", "radius") => (2, 0.25, 2.5, false),
            ("juliaFractalEffect" | "mandelbrotFractalEffect", "factor") => (2, 0.5, 2.0, false),
            ("motionBlurEffect", "distance") => (2, 0.05, 0.2, true),
            ("oilPaintingEffect", "granularity")
            | ("polarInversion", "amount")
            | ("rotateZoomEffect", "zoom")
            | ("vignetteEffect", "radius" | "strength") => (2, 0.05, 0.25, true),
            ("reduceNoiseEffect", "strength") => (2, 0.01, 0.1, false),
            ("twistEffect", "size") => (2, 0.05, 0.25, false),
            ("glowEffect", "radius")
            | ("pencilSketchEffect", "range")
            | ("radialBlurEffect" | "zoomBlurEffect", "quality")
            | ("softenPortraitEffect", "softness") => (1, 1.0, 5.0, false),
            ("dentsEffect", "scale" | "refraction" | "turbulence")
            | ("frostedGlassEffect", "maxRadius" | "minRadius")
            | ("sharpenEffect", "threshold")
            | ("tileEffect", "tile")
            | ("turbulenceEffect", "period") => (2, 1.0, 5.0, true),
            (
                "bulgeEffect" | "radialBlurEffect" | "twistEffect" | "vignetteEffect"
                | "zoomBlurEffect",
                "centerX" | "centerY",
            )
            | ("pixelateEffect", "anchorX" | "anchorY")
            | ("polarInversion", "offsetX" | "offsetY")
            | ("rotateZoomEffect", "panX" | "panY") => (2, 0.05, 0.25, false),
            ("rotateZoomEffect", "angle" | "rollDirection" | "rollAmount") => (2, 1.0, 5.0, false),
            ("addNoiseEffect", "coverage")
            | ("bulgeEffect", "amount")
            | ("dentsEffect", "detail")
            | ("juliaFractalEffect" | "mandelbrotFractalEffect", "zoom")
            | ("pencilSketchEffect", "tip")
            | ("sharpenEffect", "amount")
            | ("sketchBlurEffect", "radius")
            | ("tileEffect", "curvature")
            | ("zoomBlurEffect", "distance" | "focus") => (2, 1.0, 5.0, false),
            // Clouds.Scale and Twist.Amount are Int32 sliders. Their exponential assignments
            // target absent metadata and are ineffective (§8.3, scalar-kinds analyst correction).
            _ => continue,
        };
        *keyboard = SliderKeys {
            decimals: p,
            small,
            large,
            exponential: exp,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d59_native_coordinate_vectors_preserve_rounding_and_signed_extents() {
        let exp = SliderKeys {
            decimals: 2,
            exponential: true,
            ..SliderKeys::INTEGER
        };
        for (v, q) in [(0.0, 0), (4.0, 20000), (100.0, 100000)] {
            assert_eq!(exp.coordinate(v, 0.0, 100.0), q);
        }
        assert_eq!(exp.value(50000, 0.0, 100.0), 25.0);
        assert_eq!(exp.coordinate(1.0, 0.06, 16.0), 3930);
        assert_eq!(exp.coordinate(0.06, 0.06, 16.0), 60);
        assert_eq!(exp.coordinate(16.0, 0.06, 16.0), 16000);
        assert!((exp.value(8030, 0.06, 16.0) - 4.045).abs() < 1e-12);
        assert!((exp.value(3930, 0.06, 16.0) - 0.9995796737766627).abs() < 1e-12);
        for (q, want) in [
            (-8000, -8.0),
            (-4000, -2.0),
            (0, 0.0),
            (4000, 2.0),
            (8000, 8.0),
        ] {
            assert_eq!(exp.value(q, -8.0, 8.0), want);
        }
        assert_eq!(exp.coordinate(-0.25, -0.99, 2.0), -497);
        assert!((exp.value(-495, -0.99, 2.0) + 0.2475).abs() < 1e-12);
        let linear = SliderKeys {
            decimals: 2,
            ..SliderKeys::INTEGER
        };
        assert_eq!(linear.coordinate(0.125, -1.0, 1.0), 13);
        assert_eq!(linear.coordinate(-0.125, -1.0, 1.0), -13);
        assert_eq!(linear.value(-60, -1.0, 1.0), -0.6);
    }

    #[test]
    fn d59_registered_native_scalar_kind_inventory() {
        // Canonical §8: all22 active exponential and26 linear double identities.
        let exponential = [
            ("blurEffect", "radius", 1),
            ("bokehEffect", "radius", 1),
            ("squareBlurEffect", "radius", 1),
            ("blurEffect", "gamma", 2),
            ("bokehEffect", "gamma", 2),
            ("squareBlurEffect", "gamma", 2),
            ("dropShadowEffect", "blur", 1),
            ("edgeDetectEffect", "blurring", 2),
            ("motionBlurEffect", "distance", 2),
            ("oilPaintingEffect", "granularity", 2),
            ("polarInversion", "amount", 2),
            ("rotateZoomEffect", "zoom", 2),
            ("vignetteEffect", "radius", 2),
            ("vignetteEffect", "strength", 2),
            ("dentsEffect", "scale", 2),
            ("dentsEffect", "refraction", 2),
            ("dentsEffect", "turbulence", 2),
            ("frostedGlassEffect", "maxRadius", 2),
            ("frostedGlassEffect", "minRadius", 2),
            ("sharpenEffect", "threshold", 2),
            ("tileEffect", "tile", 2),
            ("turbulenceEffect", "period", 2),
        ];
        let linear = [
            ("addNoiseEffect", "coverage", 2),
            ("bulgeEffect", "amount", 2),
            ("dentsEffect", "detail", 2),
            ("juliaFractalEffect", "zoom", 2),
            ("mandelbrotFractalEffect", "zoom", 2),
            ("pencilSketchEffect", "tip", 2),
            ("sharpenEffect", "amount", 2),
            ("sketchBlurEffect", "radius", 2),
            ("tileEffect", "curvature", 2),
            ("zoomBlurEffect", "distance", 2),
            ("zoomBlurEffect", "focus", 2),
            ("cloudsEffect", "roughness", 2),
            ("dropShadowEffect", "distance", 1),
            ("dropShadowEffect", "opacity", 2),
            ("edgeDetectEffect", "strength", 2),
            ("frostedGlassEffect", "diffusion", 2),
            ("glowEffect", "radius", 1),
            ("highlightsAndShadowsEffect", "radius", 2),
            ("juliaFractalEffect", "factor", 2),
            ("mandelbrotFractalEffect", "factor", 2),
            ("pencilSketchEffect", "range", 1),
            ("radialBlurEffect", "quality", 1),
            ("zoomBlurEffect", "quality", 1),
            ("reduceNoiseEffect", "strength", 2),
            ("softenPortraitEffect", "softness", 1),
            ("twistEffect", "size", 2),
        ];
        for (cases, exp) in [(&exponential[..], true), (&linear[..], false)] {
            for &(id, key, p) in cases {
                let def = crate::effect::find(id).unwrap();
                let Control::Range {
                    keyboard, min, max, ..
                } = def.controls.iter().find(|c| c.key() == key).unwrap()
                else {
                    panic!("{id}/{key}")
                };
                assert_eq!(
                    (keyboard.decimals, keyboard.exponential),
                    (p, exp),
                    "{id}/{key}"
                );
                assert!(
                    keyboard.coordinate(*min, *min, *max) < keyboard.coordinate(*max, *min, *max)
                );
                assert!(
                    keyboard
                        .value(keyboard.coordinate(*min, *min, *max), *min, *max)
                        .is_finite()
                );
            }
        }
    }

    #[test]
    fn d59_registered_companion_and_integer_kinds() {
        for (id, keys) in [
            ("bulgeEffect", &["centerX", "centerY"][..]),
            ("radialBlurEffect", &["centerX", "centerY"][..]),
            ("twistEffect", &["centerX", "centerY"][..]),
            ("vignetteEffect", &["centerX", "centerY"][..]),
            ("zoomBlurEffect", &["centerX", "centerY"][..]),
            ("pixelateEffect", &["anchorX", "anchorY"][..]),
            ("polarInversion", &["offsetX", "offsetY"][..]),
            (
                "rotateZoomEffect",
                &["panX", "panY", "angle", "rollDirection", "rollAmount"][..],
            ),
        ] {
            let def = crate::effect::find(id).unwrap();
            for &key in keys {
                let Control::Range { keyboard, .. } =
                    def.controls.iter().find(|c| c.key() == key).unwrap()
                else {
                    panic!("companion")
                };
                assert_eq!(
                    (keyboard.decimals, keyboard.exponential),
                    (2, false),
                    "{id}/{key}"
                );
                assert_eq!(range_role(id, key), RangeRole::Slider);
            }
        }
        for (id, key) in [("cloudsEffect", "scale"), ("twistEffect", "amount")] {
            let def = crate::effect::find(id).unwrap();
            let Control::Range { keyboard, .. } =
                def.controls.iter().find(|c| c.key() == key).unwrap()
            else {
                panic!("integer")
            };
            assert_eq!((keyboard.decimals, keyboard.exponential), (0, false));
            assert_eq!((keyboard.small, keyboard.large), (1.0, 5.0));
        }
    }

    #[test]
    fn d59_exact_angle_roles_exclude_companions_hue_and_similar_names() {
        let mut builtin = 0;
        let mut plugin = 0;
        for def in crate::effect::all() {
            for c in def.controls {
                let role = range_role(def.id, c.key());
                if role == RangeRole::Slider {
                    continue;
                }
                let Control::Range {
                    min,
                    max,
                    step,
                    keyboard,
                    ..
                } = c
                else {
                    panic!("angle role must be a range")
                };
                assert_eq!(
                    step,
                    if def.id == "straightenEffect" {
                        0.1
                    } else {
                        1.0
                    }
                );
                assert!(!keyboard.exponential);
                match role {
                    RangeRole::BuiltinAngle => {
                        builtin += 1;
                        assert_eq!(
                            (keyboard.decimals, keyboard.small, keyboard.large),
                            (0, 1.0, 5.0)
                        );
                        assert_eq!(
                            max - min,
                            if def.id == "straightenEffect" {
                                90.0
                            } else {
                                360.0
                            }
                        );
                    }
                    RangeRole::PluginAngle => {
                        plugin += 1;
                        assert_eq!((keyboard.small, keyboard.large), (1.0, 5.0));
                        assert_eq!(
                            keyboard.decimals,
                            if def.id == "bbPolygonStars" { 2 } else { 3 }
                        );
                    }
                    RangeRole::Slider => unreachable!(),
                }
            }
        }
        assert_eq!((builtin, plugin), (11, 7));
        for (id, key) in [
            ("cloudsEffect", "scale"),
            ("twistEffect", "amount"),
            ("sharpenEffect", "amount"),
            ("rotateZoomEffect", "angle"),
            ("rotateZoomEffect", "rollDirection"),
            ("rotateZoomEffect", "rollAmount"),
            ("bbDropShadowPlus", "tiltDirection"),
            ("bbDropShadowPlus", "location"),
            ("hueAndSaturationAdjustment", "hue"),
            ("bbHueSaturationPlus", "hue"),
            ("bbCombinedAdjustments", "temperature"),
            ("futurePlugin", "angle"),
            ("dentsEffect", "rotation"),
            ("tileEffect", "angle"),
        ] {
            assert_eq!(range_role(id, key), RangeRole::Slider, "{id}/{key}");
        }
    }
}
