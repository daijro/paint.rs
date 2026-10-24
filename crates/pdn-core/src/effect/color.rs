//! Color ▸ Quantize on Paint.NET's shared 8-bit quantizer (`util::quantize`).
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS.quantizeEffect`: id,
//! icon and control keys only).

use super::{Category, Control, EffectDef, EffectEnv, EffectKind, Values, num, range};
use crate::canvas::ImageData;
use crate::pixel::Px;
use crate::util::quantize::{
    AlphaMixture, PaletteMap, analyze, flatten_alpha, map_indices, octree_palette, palette_table,
};

/// The Color submenu (§2.0 row): paintdotjs id, icon and labels; Paint.NET choices and ranges (R19).
pub fn effects() -> Vec<EffectDef> {
    vec![EffectDef {
        id: "quantizeEffect",
        icon: "quantize_effect_icon.png",
        category: Category::Effect("color"),
        controls: vec![
            Control::Select {
                key: "algorithm",
                label: "Algorithm",
                default: "octree",
                choices: &[("medianCut", "Median Cut"), ("octree", "Octree")],
            },
            range("colors", "Color count", 2.0, 256.0, 256.0),
            range("dithering", "Dithering level", 0.0, 8.0, 7.0),
            range("alphaThreshold", "Alpha threshold", 0.0, 255.0, 128.0),
        ],
        kind: EffectKind::Cpu {
            render: quantize,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: None,
        indirect: None,
    }]
}

/// §2.4 on the whole 8-bit layer: the lossless palette when it fits, else the Octree (with the
/// last slot reserved for #00000000 when transparency is on); the alpha flatten; the proximity map
/// and integer Floyd–Steinberg from row 0; indices expanded through the palette.
fn quantize(src: &ImageData, v: &Values, _: &EffectEnv) -> ImageData {
    if src.data.is_empty() {
        return src.clone();
    }
    let colors = num(v, "colors").clamp(2.0, 256.0) as usize;
    let dither = num(v, "dithering").clamp(0.0, 8.0) as u8;
    let threshold = num(v, "alphaThreshold") as u8; // saturating cast: the 0..255 clamp
    let analysis = analyze(&src.data);
    let transparency = threshold != 0 && analysis.alpha != AlphaMixture::AllOpaque;
    let t = if transparency { threshold } else { 0 };
    let pre: Vec<u8> = src
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&p| flatten_alpha(p, t))
        .collect();
    let palette: Vec<Px> = match analysis.palette {
        Some(lossless) if lossless.len() <= colors => palette_table(&lossless),
        _ => {
            // approximation: WIC Median Cut is unpublished (effects-1.md §6 item 7; B-E7)
            let mut p: Vec<Px> = octree_palette(&pre, colors - transparency as usize)
                .into_iter()
                .map(|[r, g, b]| [r, g, b, 255])
                .collect();
            if transparency {
                p.push([0; 4]);
            }
            p
        }
    };
    let mut map = PaletteMap::new(&palette);
    let indices = map_indices(&pre, src.width, src.height, &mut map, dither);
    ImageData {
        width: src.width,
        height: src.height,
        data: indices.iter().flat_map(|&i| palette[i as usize]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use crate::effect::test_util::{from_fn, px, run};
    use serde_json::json;

    /// effects-1.md §5 Task 40 Quantize.
    #[test]
    fn quantize_cases() {
        let few = from_fn(4, 2, |x, _| [(x * 60) as u8, 7, 9, 255]);
        assert_eq!(run("quantizeEffect", json!({}), &few).data, few.data);
        let alpha = from_fn(2, 1, |x, _| {
            if x == 0 {
                [100, 0, 0, 127]
            } else {
                [100, 0, 0, 128]
            }
        });
        let out = run(
            "quantizeEffect",
            json!({"colors": 2, "dithering": 0}),
            &alpha,
        );
        assert_eq!(
            (px(&out, 0, 0), px(&out, 1, 0)),
            ([0, 0, 0, 0], [177, 127, 127, 255])
        );
        let img = from_fn(6, 1, |x, _| match x {
            0..=2 => [10, 10, 10, 255],
            3 => [11, 10, 10, 255],
            _ => [250, 250, 250, 255],
        });
        let out = run("quantizeEffect", json!({"colors": 2, "dithering": 0}), &img);
        assert!((0..4).all(|x| px(&out, x, 0) == [10, 10, 10, 255]));
        assert_eq!(px(&out, 5, 0), [250, 250, 250, 255]);
    }

    // ---- Task 40 additions, derived by hand from effects-1.md §2.4 and
    // document-ops-and-formats.md §2.8–2.10. ----

    /// §2.4 steps 2–6 with threshold 0: transparency is off, so the A = 0 pixel is flattened to
    /// white even on the lossless path and maps to grey 100 (e = −155, a = (7e) ≫ 4 = −68). The
    /// black pixel's target is then 0 − ((−68·k) ≫ 3): 34 at k 4 (black), 68 at k 8 (grey 100).
    /// Threshold 1 keeps the transparent entry: identity, with A = 0 written as #00000000.
    #[test]
    fn quantize_threshold_zero_flattens_and_dithers() {
        let (black, grey) = ([0, 0, 0, 255], [100, 100, 100, 255]);
        let src = from_fn(3, 1, |x, _| [[9, 9, 9, 0], black, grey][x as usize]);
        let q = |v| run("quantizeEffect", v, &src).data;
        let at0 = |k: u8| q(json!({"alphaThreshold": 0, "dithering": k}));
        assert_eq!(at0(0), [grey, black, grey].concat());
        assert_eq!(at0(4), [grey, black, grey].concat());
        assert_eq!(at0(8), [grey, grey, grey].concat());
        assert_eq!(
            q(json!({"alphaThreshold": 1})),
            [[0; 4], black, grey].concat()
        );
        // §2.4 step 1: a lossless palette of exactly Colors entries is still used as is (the
        // Octree would add white). Dithering 0: at the default 7 the black pixel's target is 60.
        assert_eq!(
            q(json!({"alphaThreshold": 0, "colors": 3, "dithering": 0})),
            [grey, black, grey].concat()
        );
    }

    /// §2.4 steps 2–3: transparency reserves the last slot, so Colors 2 leaves one opaque colour,
    /// the linear mean of black and white (0.5 → sRGB 0.73536 → 187.5 → 188). Threshold 0 turns
    /// transparency off, flattens the A = 0 pixel to white and keeps both colours.
    #[test]
    fn quantize_reserves_the_transparent_slot() {
        let (black, white) = ([0, 0, 0, 255], [255, 255, 255, 255]);
        let src = from_fn(3, 1, |x, _| [black, white, [5, 5, 5, 0]][x as usize]);
        let q = |v| run("quantizeEffect", v, &src).data;
        let mean = [188, 188, 188, 255];
        assert_eq!(q(json!({"colors": 2})), [mean, mean, [0; 4]].concat());
        assert_eq!(
            q(json!({"colors": 2, "alphaThreshold": 0})),
            [black, white, white].concat()
        );
    }

    /// effects-1.md §6 item 7: WIC Median Cut is unpublished, so it runs the Octree (B-E7).
    #[test]
    fn quantize_median_cut_falls_back_to_octree() {
        let img = from_fn(16, 16, |x, y| {
            [(x * 16) as u8, (y * 16) as u8, ((x + y) * 8) as u8, 255]
        });
        let q = |a: &str| run("quantizeEffect", json!({"algorithm": a, "colors": 5}), &img).data;
        assert_eq!(q("medianCut"), q("octree"));
        assert_ne!(q("octree"), img.data);
    }

    /// Out-of-range values clamp to the control ranges. On this 512-colour image Colors 0 would
    /// underflow the transparent reserve and 999 would overflow the u8 indices. An empty layer
    /// (no palette entry at all) passes through.
    #[test]
    fn quantize_clamps_controls() {
        let empty = from_fn(0, 0, |_, _| [0; 4]);
        assert_eq!(run("quantizeEffect", json!({}), &empty).data, empty.data);
        let img = from_fn(32, 16, |x, y| {
            let a = if x == 0 { 200 } else { 255 };
            [(x * 8) as u8, (y * 16) as u8, (x * y) as u8, a]
        });
        let q = |v| run("quantizeEffect", v, &img).data;
        assert_eq!(q(json!({"colors": 0})), q(json!({"colors": 2})));
        assert_eq!(q(json!({"colors": 999})), q(json!({"colors": 256})));
        assert_eq!(q(json!({"dithering": 99})), q(json!({"dithering": 8})));
        assert_eq!(
            q(json!({"alphaThreshold": 999})),
            q(json!({"alphaThreshold": 255}))
        );
    }

    /// R19 controls (effects-1.md §2.0 Color row), paintdotjs labels and icon; a CPU effect that
    /// the framework clips to the selection.
    #[test]
    fn r19_controls() {
        use crate::effect::{Category, Control, EffectKind, defaults, find};
        let d = find("quantizeEffect").unwrap();
        assert_eq!(
            (d.category, d.icon),
            (Category::Effect("color"), "quantize_effect_icon.png")
        );
        assert!(matches!(
            d.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        let Control::Select {
            label,
            default,
            choices,
            ..
        } = &d.controls[0]
        else {
            panic!()
        };
        assert_eq!(
            (*label, *default, *choices),
            (
                "Algorithm",
                "octree",
                &[("medianCut", "Median Cut"), ("octree", "Octree")][..]
            )
        );
        let ranges: Vec<_> = d.controls[1..]
            .iter()
            .map(|c| match *c {
                Control::Range {
                    key,
                    label,
                    min,
                    max,
                    default,
                    step,
                    ..
                } => (key, label, min, max, default, step),
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            ranges,
            [
                ("colors", "Color count", 2.0, 256.0, 256.0, 1.0),
                ("dithering", "Dithering level", 0.0, 8.0, 7.0, 1.0),
                ("alphaThreshold", "Alpha threshold", 0.0, 255.0, 128.0, 1.0),
            ]
        );
        assert_eq!(defaults(&d)["algorithm"], "octree");
    }
}
