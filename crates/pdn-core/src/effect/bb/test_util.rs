//! BB spec: common.md §1, §2, §6
//!
//! The BB test helpers (plan Conventions "Test helpers"): the effect from its dynamic defaults,
//! pixel asserts at the spec's tolerance, selections and layer lists.
use super::indirect_defaults;
use crate::canvas::ImageData;
use crate::effect::{EffectDef, EffectEnv, EffectKind, LayerInfo, RenderInfo, apply, find};
use crate::pixel::mask::Mask;
use crate::pixel::{IntRect, Px};
use crate::util::geom::Rectangle;
use serde_json::Value;

pub use crate::effect::test_util::{from_fn, px, solid};

/// common.md §1: ±2 levels per channel.
pub const TOL: u8 = 2;

/// Effect `id` over the whole layer (`EffectEnv::whole`), from its dynamic defaults, then
/// `overrides`.
pub fn bb(id: &str, overrides: Value, src: &ImageData) -> ImageData {
    bb_env(id, overrides, src, &EffectEnv::whole(src.width, src.height))
}

/// `bb` with a given env.
pub fn bb_env(id: &str, overrides: Value, src: &ImageData, env: &EffectEnv) -> ImageData {
    run(&def(id), overrides, src, env)
}

/// `bb` with the GPU effect's `RenderInfo` replaced by `info` (the other column of a gamma/linear
/// table: `conventions::ALT_GPU_CONTEXT`). Panics for CPU effects.
pub fn bb_ctx(id: &str, info: RenderInfo, overrides: Value, src: &ImageData) -> ImageData {
    let mut d = def(id);
    let EffectKind::Gpu(_, render) = d.kind else {
        panic!("{id} is a CPU effect");
    };
    d.kind = EffectKind::Gpu(info, render);
    run(&d, overrides, src, &EffectEnv::whole(src.width, src.height))
}

fn def(id: &str) -> EffectDef {
    find(id).unwrap_or_else(|| panic!("no effect {id}"))
}

fn run(def: &EffectDef, overrides: Value, src: &ImageData, env: &EffectEnv) -> ImageData {
    let mut v = indirect_defaults(def, env);
    if let Value::Object(o) = overrides {
        v.extend(o);
    }
    apply(src, def, &v, env)
}

/// Pixel (x, y) is `want` within ±`TOL` per channel.
#[track_caller]
pub fn assert_px(img: &ImageData, x: u32, y: u32, want: Px) {
    assert_px_tol(img, x, y, want, TOL);
}

/// Pixel (x, y) is `want` within ±`tol` per channel.
#[track_caller]
pub fn assert_px_tol(img: &ImageData, x: u32, y: u32, want: Px, tol: u8) {
    let got = px(img, x, y);
    assert!(
        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= tol),
        "pixel ({x}, {y}) is {got:?}, want {want:?} ±{tol}"
    );
}

/// The pixel-aligned rectangle [l, r) × [t, b) over a w×h layer: its mask (255 inside) and its
/// outline polygon.
pub fn rect_selection(
    w: u32,
    h: u32,
    l: u32,
    t: u32,
    r: u32,
    b: u32,
) -> (Mask, Vec<Vec<[f64; 2]>>) {
    let inside = |x, y| (l..r).contains(&x) && (t..b).contains(&y);
    let data = (0..h)
        .flat_map(|y| (0..w).map(move |x| if inside(x, y) { 255 } else { 0 }))
        .collect();
    let [l, t, r, b] = [l, t, r, b].map(f64::from);
    (
        Mask {
            rect: IntRect::new(0, 0, w, h),
            data,
        },
        vec![vec![[l, t], [r, t], [r, b], [l, b]]],
    )
}

/// A 64-sided ellipse with antialiased coverage from the shared even-odd rasterizer.
pub fn ellipse_selection(
    w: u32,
    h: u32,
    centre: [f64; 2],
    radii: [f64; 2],
) -> (Mask, Vec<Vec<[f64; 2]>>) {
    let polygon = (0..64)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 64.0;
            [
                centre[0] + radii[0] * a.cos(),
                centre[1] + radii[1] * a.sin(),
            ]
        })
        .collect();
    let polygons = vec![polygon];
    let path = super::raster::polygon_path(&polygons, [0.0, 0.0], true).unwrap();
    let data = super::raster::fill_coverage(&path, w, h, true)
        .into_iter()
        .map(|a| (a * 255.0).round() as u8)
        .collect();
    (
        Mask {
            rect: IntRect::new(0, 0, w, h),
            data,
        },
        polygons,
    )
}

/// The whole w×h layer with `mask`, the polygons' integer bounds ∩ the layer, and `polys`.
pub fn with_selection<'a>(
    w: u32,
    h: u32,
    mask: &'a Mask,
    polys: &'a [Vec<[f64; 2]>],
) -> EffectEnv<'a> {
    let (mut l, mut t) = (f64::INFINITY, f64::INFINITY);
    let (mut r, mut b) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &[x, y] in polys.iter().flatten() {
        (l, t, r, b) = (l.min(x), t.min(y), r.max(x), b.max(y));
    }
    let bounds = IntRect::bound_of(&Rectangle::new(l, t, r - l, b - t));
    EffectEnv {
        mask: Some(mask),
        bounds: bounds.intersect(&IntRect::new(0, 0, w, h)),
        selection: polys,
        ..EffectEnv::whole(w, h)
    }
}

/// The env of a layer-reading effect run on `layers[index]` (bottom first), whole layer.
pub fn layers_env(layers: &[LayerInfo], index: usize) -> EffectEnv<'_> {
    let p = &layers[index].pixels;
    EffectEnv {
        layers,
        layer_index: index,
        ..EffectEnv::whole(p.width, p.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::conventions::ALT_GPU_CONTEXT;
    use serde_json::json;
    use std::sync::Arc;

    /// The helpers on built-in effects: overrides apply, the context swap reaches the render,
    /// selections and layer lists land in the env.
    #[test]
    fn helpers() {
        let img = solid(2, 1, [10, 20, 30, 255]);
        assert_px(
            &bb("invertColorsEffect", json!({}), &img),
            1,
            0,
            [245, 235, 225, 255],
        );
        // The overrides replace the defaults (radius 2 would blur the edge).
        let edge = from_fn(4, 1, |x, _| if x < 2 { [0, 0, 0, 255] } else { [255; 4] });
        assert_eq!(
            bb("blurEffect", json!({"radius": 0}), &edge).data,
            edge.data
        );
        assert_px_tol(&edge, 2, 0, [253, 255, 255, 255], 2);
        // A linear-light blur of black | white is brighter at the seam than an sRGB one.
        let lin = bb("blurEffect", json!({"radius": 2}), &edge);
        let srgb = bb_ctx("blurEffect", ALT_GPU_CONTEXT, json!({"radius": 2}), &edge);
        assert!(px(&lin, 1, 0)[0] > px(&srgb, 1, 0)[0]);
        let (mask, polys) = rect_selection(5, 4, 1, 2, 3, 4);
        assert_eq!(
            (mask.get(0, 2), mask.get(1, 2), mask.get(3, 3)),
            (0, 255, 0)
        );
        let env = with_selection(5, 4, &mask, &polys);
        assert_eq!(env.bounds, IntRect::new(1, 2, 2, 2));
        assert_eq!(env.selection[0][2], [3.0, 4.0]);
        let layers = [3, 7].map(|w| LayerInfo {
            name: format!("L{w}"),
            pixels: Arc::new(solid(w, 2, [0; 4])),
        });
        let env = layers_env(&layers, 1);
        assert_eq!((env.width, env.layer_index, env.layers.len()), (7, 1, 2));
    }

    #[test]
    #[should_panic(expected = "pixel (0, 0) is [10, 20, 30, 255], want [13, 20, 30, 255] ±2")]
    fn assert_px_rejects_three_levels() {
        assert_px(&solid(1, 1, [10, 20, 30, 255]), 0, 0, [13, 20, 30, 255]);
    }
}
