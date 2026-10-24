//! Photocraft phase spec: S§4.2; research R§B (Noise)
//!
//! The Extra Noise effect: Dust & Scratches (`extra::fx::noise`). Anchored on the selection
//! bounds; reads the whole layer.

use crate::canvas::ImageData;
use crate::effect::{Category, EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::extra::geom::Rect;
use crate::extra::{fx, interrupt::Interrupt, shim};

pub fn effects() -> Vec<EffectDef> {
    let m = super::manifest::manifest("pcDustAndScratches").expect("manifest row");
    vec![EffectDef {
        id: "pcDustAndScratches",
        icon: m.icon,
        category: Category::Effect("noise"),
        controls: vec![
            range("radius", "Radius", 1.0, 100.0, 1.0),
            range("threshold", "Threshold", 0.0, 255.0, 0.0),
        ],
        kind: EffectKind::Cpu {
            render: dust_and_scratches,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: None,
    }]
}

fn dust_and_scratches(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let b = env.bounds;
    let out = fx::noise::median(
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32),
        num(v, "radius").clamp(1.0, 100.0) as f32,
        Some(num(v, "threshold").clamp(0.0, 255.0) as f32),
        &Interrupt(env.cancel),
    );
    out.map_or_else(
        || src.clone(),
        |buf| shim::from_f32(&buf, src.width, src.height),
    )
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run};
    use serde_json::json;

    /// A one-pixel bright speck on a flat field vanishes; a hard vertical edge survives (the
    /// median of a straight edge is one of its two sides, never a blend).
    #[test]
    fn dust_removes_isolated_speck_keeps_edges() {
        let speck = from_fn(21, 21, |x, y| {
            if (x, y) == (10, 10) {
                [255, 255, 255, 255]
            } else {
                [40, 80, 120, 255]
            }
        });
        let out = run("pcDustAndScratches", json!({"radius": 2}), &speck);
        assert_eq!(px(&out, 10, 10), [40, 80, 120, 255]);
        assert_eq!(px(&out, 3, 3), [40, 80, 120, 255]);

        let edge = from_fn(21, 21, |x, _| {
            if x < 10 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            }
        });
        let out = run("pcDustAndScratches", json!({"radius": 3}), &edge);
        // Edge replication keeps the straight edge, border columns included.
        assert_eq!(out.data, edge.data);
    }

    /// Threshold 255 never replaces anything.
    #[test]
    fn dust_threshold_gates_the_replacement() {
        let speck = from_fn(9, 9, |x, y| {
            if (x, y) == (4, 4) {
                [200, 200, 200, 255]
            } else {
                [100, 100, 100, 255]
            }
        });
        let out = run(
            "pcDustAndScratches",
            json!({"radius": 2, "threshold": 255}),
            &speck,
        );
        assert_eq!(out.data, speck.data);
        let out = run(
            "pcDustAndScratches",
            json!({"radius": 2, "threshold": 50}),
            &speck,
        );
        assert_eq!(px(&out, 4, 4), [100, 100, 100, 255]);
        // Under the threshold the speck stays.
        let out = run(
            "pcDustAndScratches",
            json!({"radius": 2, "threshold": 150}),
            &speck,
        );
        assert_eq!(px(&out, 4, 4), [200, 200, 200, 255]);
    }
}
