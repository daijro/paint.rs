//! Photocraft phase spec: S§4.2; research R§B (Object: Matting)
//!
//! The Extra Object effects: Defringe, Remove Black Matte, Remove White Matte and Color
//! Decontaminate (`extra::fx::matting`). They work on the layer's own alpha.

use crate::canvas::ImageData;
use crate::effect::{Category, EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::extra::interrupt::Interrupt;
use crate::extra::{fx, shim};

fn def(
    id: &'static str,
    icon: &'static str,
    controls: Vec<crate::effect::Control>,
    render: fn(&ImageData, &Values, &EffectEnv) -> ImageData,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("object"),
        controls,
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

pub fn effects() -> Vec<EffectDef> {
    vec![
        def(
            "pcDefringe",
            "defringe_effect.png",
            vec![range("width", "Width", 1.0, 200.0, 1.0)],
            defringe,
        ),
        def(
            "pcRemoveBlackMatte",
            "remove_black_matte_effect.png",
            vec![],
            |src, _, _| matte(src, false),
        ),
        def(
            "pcRemoveWhiteMatte",
            "remove_white_matte_effect.png",
            vec![],
            |src, _, _| matte(src, true),
        ),
        def(
            "pcColorDecontaminate",
            "color_decontaminate_effect.png",
            vec![range("amount", "Amount", 0.0, 100.0, 100.0)],
            decontaminate,
        ),
    ]
}

fn defringe(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut buf = shim::to_f32(src);
    let width = num(v, "width").clamp(1.0, 200.0).round() as usize;
    fx::matting::defringe(
        &mut buf,
        src.width as usize,
        src.height as usize,
        width,
        Interrupt(env.cancel),
    );
    shim::from_f32(&buf, src.width, src.height)
}

fn matte(src: &ImageData, white: bool) -> ImageData {
    let mut buf = shim::to_f32(src);
    fx::matting::remove_matte(&mut buf, white);
    shim::from_f32(&buf, src.width, src.height)
}

/// Upstream's decontaminate radius stays at its default of 4.
fn decontaminate(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let mut buf = shim::to_f32(src);
    fx::matting::decontaminate(
        &mut buf,
        src.width as usize,
        src.height as usize,
        4.0,
        num(v, "amount") as f32,
        Interrupt(env.cancel),
    );
    shim::from_f32(&buf, src.width, src.height)
}
