//! Photocraft phase spec: S§4.2; research R§B (Photo)
//!
//! The Extra Photo effect Smart Sharpen (`extra::fx::sharpen`). Upstream's Smart Sharpen has
//! only Amount, Radius and Reduce Noise, so every control is a plain Range. PhotoCraft's Unsharp
//! Mask gave way to Tanel's Sharpen+ (`effect::forum::tanel`), an unsharp mask with more control.

use crate::canvas::ImageData;
use crate::effect::{Category, Control, EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::extra::{fx, interrupt::Interrupt, shim};

fn def(id: &'static str, controls: Vec<Control>, render: RenderFn) -> EffectDef {
    let m = super::manifest::manifest(id).expect("manifest row");
    EffectDef {
        id,
        icon: m.icon,
        category: Category::Effect("photo"),
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
    vec![def(
        "pcSmartSharpen",
        vec![
            range("amount", "Amount", 1.0, 500.0, 100.0),
            range("radius", "Radius", 0.1, 64.0, 1.0).slider_keys(1, 0.1, 1.0, false),
            range("reduceNoise", "Reduce Noise", 0.0, 100.0, 10.0),
        ],
        smart_sharpen,
    )]
}

/// Kernel on the whole layer; a cancelled run returns the source (the worker discards it).
fn run(
    src: &ImageData,
    env: &EffectEnv,
    f: impl FnOnce(&[f32], usize, usize, &Interrupt) -> Option<Vec<f32>>,
) -> ImageData {
    let buf = shim::to_f32(src);
    let ctl = Interrupt(env.cancel);
    match f(&buf, src.width as usize, src.height as usize, &ctl) {
        Some(out) => shim::from_f32(&out, src.width, src.height),
        None => src.clone(),
    }
}

fn smart_sharpen(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (a, r, n) = (num(v, "amount"), num(v, "radius"), num(v, "reduceNoise"));
    run(src, env, |b, w, h, c| {
        fx::sharpen::smart(b, w, h, a as f32, r as f32, n as f32, c)
    })
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, run, solid};
    use serde_json::json;

    fn edge() -> crate::canvas::ImageData {
        from_fn(16, 4, |x, _| {
            if x < 8 {
                [80, 80, 80, 255]
            } else {
                [180, 180, 180, 255]
            }
        })
    }

    #[test]
    fn smart_sharpen_flat_is_identity() {
        let img = solid(10, 6, [40, 90, 200, 255]);
        assert_eq!(run("pcSmartSharpen", json!({}), &img).data, img.data);
        assert_ne!(run("pcSmartSharpen", json!({}), &edge()).data, edge().data);
    }

    #[test]
    fn cancelled_sharpen_returns_source() {
        use crate::effect::{EffectEnv, apply, defaults, find};
        use std::sync::atomic::AtomicBool;
        let flag = AtomicBool::new(true);
        let mut env = EffectEnv::whole(16, 4);
        env.cancel = Some(&flag);
        let def = find("pcSmartSharpen").unwrap();
        assert_eq!(
            apply(&edge(), &def, &defaults(&def), &env).data,
            edge().data
        );
    }
}
