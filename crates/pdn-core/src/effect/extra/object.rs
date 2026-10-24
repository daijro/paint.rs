//! Photocraft phase spec: S§4.2; research R§B (Object)
//!
//! The Extra Object effect: Color to Alpha (`extra::fx::color_to_alpha`).

use crate::canvas::ImageData;
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, color_value,
};
use crate::extra::{fx, shim};

pub fn effects() -> Vec<EffectDef> {
    let m = super::manifest::manifest("pcColorToAlpha").expect("manifest row");
    vec![EffectDef {
        id: "pcColorToAlpha",
        icon: m.icon,
        category: Category::Effect("object"),
        controls: vec![Control::Color {
            key: "color",
            label: "Color",
            default: ColorDefault::Secondary,
        }],
        kind: EffectKind::Cpu {
            render: color_to_alpha,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: None,
    }]
}

fn color_to_alpha(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let c = color_value(v, "color", env);
    let key = [c[0], c[1], c[2]].map(|b| f32::from(b) / 255.0);
    let out = fx::color_to_alpha::color_to_alpha(&shim::to_f32(src), key, 0.0, 1.0);
    shim::from_f32(&out, src.width, src.height)
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run};
    use serde_json::json;

    /// A pure-colour pixel goes fully transparent; a 50 % mix of black over white becomes black
    /// at alpha about 128; an unrelated opaque colour stays.
    #[test]
    fn color_to_alpha_removes_the_colour() {
        let src = from_fn(3, 1, |x, _| match x {
            0 => [255, 255, 255, 255],
            1 => [128, 128, 128, 255],
            _ => [0, 0, 0, 255],
        });
        let out = run("pcColorToAlpha", json!({"color": "#ffffff"}), &src);
        assert_eq!(px(&out, 0, 0)[3], 0);
        let mix = px(&out, 1, 0);
        assert!(mix[3].abs_diff(128) <= 2, "{mix:?}");
        assert!(mix[..3].iter().all(|&c| c <= 1), "{mix:?}");
        assert_eq!(px(&out, 2, 0), [0, 0, 0, 255]);
    }

    /// The colour defaults to the secondary colour (white in `EffectEnv::whole`).
    #[test]
    fn color_defaults_to_secondary() {
        let src = from_fn(1, 1, |_, _| [255, 255, 255, 255]);
        let out = run("pcColorToAlpha", json!({}), &src);
        assert_eq!(px(&out, 0, 0)[3], 0);
    }
}
