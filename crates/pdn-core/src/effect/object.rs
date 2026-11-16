//! Drop Shadow, the Object submenu's one effect, on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` entry dropShadowEffect:
//! id, icon and control keys only).

use super::graph::{
    CLSID_D2D1Composite, CLSID_D2D1Shadow, CLSID_D2D12DAffineTransform, Graph, Prop, QUALITY,
};
use super::surface::{Alpha, EffectSurface, Space, premultiply, unpremultiply};
use super::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, RenderInfo, Values,
    color_value, flag, num, range,
};
use crate::pixel::gamma::SRGB8_TO_LINEAR;
use std::f64::consts::PI;

/// The Object submenu (§1): paintdotjs's id, icon, keys, labels and steps; Paint.NET's ranges and
/// Color control (R19, §2.1).
pub fn effects() -> Vec<EffectDef> {
    vec![EffectDef {
        id: "dropShadowEffect",
        icon: "drop_shadow_effect.png",
        category: Category::Effect("object"),
        controls: vec![
            // Paint.NET's slider is exponential (§2.1).
            range("blur", "Blur radius", 0.0, 100.0, 10.0).exponential(),
            range("distance", "Distance", 0.0, 100.0, 10.0),
            range("angle", "Angle", -180.0, 180.0, -45.0),
            range("opacity", "Opacity", 0.0, 1.0, 0.75).step(0.01),
            Control::Color {
                key: "color",
                label: "Color",
                default: ColorDefault::Rgb(0x000000),
            },
            Control::Checkbox {
                key: "shadowOnly",
                label: "Shadow only",
                default: false,
            },
        ],
        kind: EffectKind::Gpu(
            RenderInfo::new(Space::Linear, Alpha::Premultiplied),
            drop_shadow,
        ),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }]
}

/// §2.1 step 4: (cos φ·d, sin φ·d) with φ = −angle·π/180; the default −45° gives (+7.07, +7.07),
/// down and right.
pub fn shadow_offset(distance: f64, angle_deg: f64) -> [f64; 2] {
    let phi = -angle_deg * PI / 180.0;
    [phi.cos() * distance, phi.sin() * distance]
}

/// Genuine `PdnDropShadowEffect` (decompiled, PaintDotNet.Effects.Gpu): the GPU graph
///
///   src → ConvertAlpha(Premul≡no-op) ─┬───────────────────────────────────────────────┐
///                                      └ Shadow{σ, color, Quality} → AffineTransform2D  │
///                                        {translate(dx,dy), Linear, Soft} → Composite(SourceOver, ┘)
///                                      → ConvertAlpha(Premul≡no-op)  [shadowOnly: output the affine]
///
/// in a LINEAR working space (the color is linearized: `LinearColor.op_Implicit((SrgbColor)color)`),
/// every intermediate at the 8bpc-premultiplied device default precision. σ = `StandardDeviation
/// .FromRadius(blur)` = blur/3 (f32); the offset is `Translation(cos(−angle)·d, sin(−angle)·d)` with
/// the cos/sin in f64 then the matrix cast to f32. Opacity is the shadow color's alpha.
fn drop_shadow(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let sigma = (num(v, "blur").clamp(0.0, 100.0) as f32) / 3.0_f32; // FromRadius, f32
    let dist = num(v, "distance").clamp(0.0, 100.0);
    let phi = -num(v, "angle").clamp(-180.0, 180.0) * PI / 180.0;
    let (dx, dy) = ((phi.cos() * dist) as f32, (phi.sin() * dist) as f32); // Matrix3x2Float
    let opacity = num(v, "opacity").clamp(0.0, 1.0) as f32;
    let c = color_value(v, "color", env);
    let lin = |k: u8| SRGB8_TO_LINEAR[k as usize];
    let color_lin = [lin(c[0]), lin(c[1]), lin(c[2])];
    let colour = premultiply([color_lin[0], color_lin[1], color_lin[2], opacity]);
    drop_shadow_with(
        src,
        sigma as f64,
        [dx as f64, dy as f64],
        colour,
        flag(v, "shadowOnly"),
    )
}

/// Drop Shadow's graph on premultiplied `src`: d2d's Shadow of colour `colour` (premultiplied in
/// the working space) at `sigma` (Quality), translated by `offset`, with `src` composited over it
/// unless `shadow_only`.
pub fn drop_shadow_with(
    src: &EffectSurface,
    sigma: f64,
    offset: [f64; 2],
    colour: [f32; 4],
    shadow_only: bool,
) -> EffectSurface {
    let (g, src) = Graph::new(src, Alpha::Premultiplied);
    // Shadow.Color is straight; the effect premultiplies it by the input alpha.
    let shadow = [
        (0, Prop::F32(sigma as f32)),
        (1, Prop::V4(unpremultiply(colour))),
        (2, Prop::U32(QUALITY)),
    ];
    let shadow = g.fx(CLSID_D2D1Shadow, &shadow, [src.clone()]);
    let [dx, dy] = offset.map(|o| o as f32);
    let moved = [(2, Prop::M32([1.0, 0.0, 0.0, 1.0, dx, dy]))];
    let moved = g.fx(CLSID_D2D12DAffineTransform, &moved, [shadow]);
    let out = if shadow_only {
        moved
    } else {
        // SourceOver: the source (input 1) over the translated shadow (input 0).
        g.fx(CLSID_D2D1Composite, &[(0, Prop::U32(0))], [moved, src])
    };
    g.finish(&out, Alpha::Premultiplied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, run, solid};
    use serde_json::json;

    /// effects-2.md §5 vector 4: one white α128 pixel, radius 0, distance 0 → α176, RGB 222 (paintdotjs: 255).
    #[test]
    fn drop_shadow_composites_in_linear_light() {
        let out = run(
            "dropShadowEffect",
            json!({"blur": 0, "distance": 0}),
            &solid(1, 1, [255, 255, 255, 128]),
        );
        assert_eq!(px(&out, 0, 0), [222, 222, 222, 176]);
        let o = shadow_offset(10.0, -45.0);
        assert!((o[0] - 7.0710678).abs() < 1e-6 && (o[1] - 7.0710678).abs() < 1e-6);
        let square = from_fn(5, 5, |_, _| [255, 255, 255, 255]);
        let shadow = run(
            "dropShadowEffect",
            json!({"blur": 0, "distance": 0, "shadowOnly": true}),
            &square,
        );
        assert_eq!(px(&shadow, 2, 2), [0, 0, 0, 191]);
        let red = run(
            "dropShadowEffect",
            json!({"blur": 0, "distance": 0, "shadowOnly": true, "color": "#ff0000"}),
            &square,
        );
        assert_eq!(px(&red, 2, 2), [255, 0, 0, 191]);
    }

    // ---- Vectors from an independent f64 Python port of §2.1 (Soft border = unbounded zero padding). ----

    /// Step 1 linearizes the colour (`LinearColor.op_Implicit((SrgbColor)color)`): grey #808080
    /// (linear 0.2158) under white α128 composites to ~230 through the 8bpc premultiplied nodes;
    /// using the sRGB value 0.502 as if it were linear gives 239.20. Byte-exact vs the genuine graph.
    #[test]
    fn drop_shadow_colour_is_linearized() {
        let out = run(
            "dropShadowEffect",
            json!({"blur": 0, "distance": 0, "color": "#808080"}),
            &solid(1, 1, [255, 255, 255, 128]),
        );
        assert_eq!(px(&out, 0, 0), [229, 229, 229, 176]);
    }

    /// Step 4: the shadow at p is the translated blurred source at p − (cos φ·d, sin φ·d), φ = −angle
    /// (the AffineTransform2D, Linear+Soft): angle 90 moves it up (row 0 for a dot at row 2), angle 0
    /// right; d = 0.5 splits the dot's 0.75 over two columns (the Q8 resampler gives 95 each).
    #[test]
    fn drop_shadow_offset_direction() {
        let dot = from_fn(
            5,
            5,
            |x, y| {
                if (x, y) == (2, 2) { [255; 4] } else { [0; 4] }
            },
        );
        let up = run(
            "dropShadowEffect",
            json!({"blur": 0, "distance": 2, "angle": 90, "shadowOnly": true}),
            &dot,
        );
        assert_eq!(
            [px(&up, 2, 0), px(&up, 2, 4), px(&up, 2, 2)],
            [[0, 0, 0, 191], [0; 4], [0; 4]]
        );
        let half = run(
            "dropShadowEffect",
            json!({"blur": 0, "distance": 0.5, "angle": 0, "shadowOnly": true}),
            &dot,
        );
        assert_eq!(
            [px(&half, 1, 2), px(&half, 2, 2), px(&half, 3, 2)],
            [[0; 4], [0, 0, 0, 96], [0, 0, 0, 96]]
        );
    }

    /// The D2D Shadow blur's Soft (transparent) border grows past the layer: an opaque column at
    /// x = 0 of a 9×5 layer, radius 6 (σ 2, Quality), distance 3 to the right. The halo the affine
    /// translation brings back in matches the genuine graph, in float (a blur clipped to the layer
    /// gives 0 at x = 0..2; σ = radius, not radius/3, would peak far lower).
    #[test]
    fn drop_shadow_blur_grows_past_the_layer() {
        let col = from_fn(9, 5, |x, _| if x == 0 { [255; 4] } else { [0; 4] });
        let out = run(
            "dropShadowEffect",
            json!({"blur": 6, "distance": 3, "angle": 0, "shadowOnly": true}),
            &col,
        );
        let alpha: Vec<u8> = (0..5).map(|x| px(&out, x, 2)[3]).collect();
        assert_eq!(alpha, [10, 19, 27, 31, 27]);
        // Soft pads with transparent black: an opaque 9×9 layer, radius 6, distance 0 fades at its
        // corner, edge and centre (a Mirror/clamp border would fade much less).
        let opaque = run(
            "dropShadowEffect",
            json!({"blur": 6, "distance": 0, "shadowOnly": true}),
            &solid(9, 9, [255; 4]),
        );
        assert_eq!(
            [(0, 0), (4, 0), (4, 4)].map(|(x, y)| px(&opaque, x, y)[3]),
            [69, 113, 185]
        );
    }

    /// R19 controls (paintdotjs's keys, labels and steps; Paint.NET's ranges and Color), and
    /// out-of-range values are clamped (radius 1e9 would otherwise build a 2e10-tap kernel).
    #[test]
    fn drop_shadow_controls_and_clamping() {
        let d = crate::effect::find("dropShadowEffect").unwrap();
        assert_eq!(d.category, crate::effect::Category::Effect("object"));
        assert_eq!(d.icon, "drop_shadow_effect.png");
        let ranges: Vec<_> = d
            .controls
            .iter()
            .filter_map(|c| match c {
                Control::Range {
                    key,
                    label,
                    min,
                    max,
                    default,
                    step,
                    ..
                } => Some((*key, *label, *min, *max, *default, *step)),
                _ => None,
            })
            .collect();
        assert_eq!(
            ranges,
            [
                ("blur", "Blur radius", 0.0, 100.0, 10.0, 1.0),
                ("distance", "Distance", 0.0, 100.0, 10.0, 1.0),
                ("angle", "Angle", -180.0, 180.0, -45.0, 1.0),
                ("opacity", "Opacity", 0.0, 1.0, 0.75, 0.01),
            ]
        );
        let v = crate::effect::defaults(&d);
        assert_eq!(
            (&v["color"], &v["shadowOnly"]),
            (&json!("#000000"), &json!(false))
        );
        let img = solid(4, 3, [90, 140, 30, 200]);
        for o in [
            json!({"blur": 1e9, "distance": 1e9, "opacity": 9}),
            json!({"blur": -5, "distance": -5, "opacity": -1, "angle": 1e9}),
        ] {
            assert_eq!(run("dropShadowEffect", o, &img).data.len(), 48);
        }
        let opaque = run(
            "dropShadowEffect",
            json!({"opacity": 9, "blur": 0, "distance": 0, "shadowOnly": true}),
            &img,
        );
        assert_eq!(px(&opaque, 0, 0), [0, 0, 0, 200]);
        let dot = from_fn(
            5,
            5,
            |x, y| if (x, y) == (2, 2) { [255; 4] } else { [0; 4] },
        );
        let angle = |a: f64| {
            run(
                "dropShadowEffect",
                json!({"blur": 0, "distance": 2, "angle": a, "shadowOnly": true}),
                &dot,
            )
            .data
        };
        assert_eq!(angle(1e9), angle(180.0));
    }
}
