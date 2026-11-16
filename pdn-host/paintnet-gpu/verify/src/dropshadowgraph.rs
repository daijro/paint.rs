//! Oracle for the GENUINE `PdnDropShadowEffect` graph (DropShadowGpuEffect, PaintDotNet.Effects.Gpu)
//! on opend2d's d2d executor (8bpc premultiplied node buffers, == native WARP). Assembles the exact
//! decompiled graph from d2d built-ins:
//!
//!   src8 ─┬───────────────────────────────────────────────────────────────┐
//!         └ Shadow{σ, color, Quality} → AffineTransform2D{translate(dx,dy), │
//!            Linear, Soft} ───────────── Composite(SourceOver, dst=affine, src=┘) → (float)
//!
//! The genuine graph wraps this in ConvertAlpha(Premultiply≡no-op on the premultiplied 8bpc source)
//! on input and output; AlphaMode=Premultiplied and the executor keeps premultiplied buffers, so both
//! ConvertAlpha nodes are identity and omitted. `shadowOnly` makes the AffineTransform the output node
//! (no Composite). The working space is LINEAR (genuine linearizes the colour via
//! `LinearColor.op_Implicit((SrgbColor)color)`), so this runner is FED the port's exact 8bpc
//! premultiplied-LINEAR source (`<prefix>.src8.bgra`) and the already-linear colour (`<prefix>.args`),
//! eliminating any duplicated sRGB/premul math. It renders the graph to an R32G32B32A32_FLOAT target
//! with the DEFAULT 8bpc intermediate precision (so Shadow/Affine realise at 8bpc, only the final
//! Composite keeps f32), then applies the SAME `unpremultiply` + `linear_to_srgb` the port's
//! `prequant_f32(Linear, Premultiplied)` applies (copied verbatim from pdn-core
//! `effect/surface.rs` + `pixel/gamma.rs`) and dumps straight sRGB RGBA f32 (LE) for a bit-exact
//! compare against `classic_oracle dropShadowEffect ... PDNGPU_F32`.
//!
//! Usage: `pdngpu-dropshadowgraph <prefix> <out.f32>`
//!   <prefix>.src8.bgra = 8bpc premultiplied-linear BGRA source (W*H*4 bytes)
//!   <prefix>.args      = "σ opt dx dy linR linG linB opacity shadowOnly W H"
use d2d::abi::{
    CLSID_D2D12DAffineTransform, CLSID_D2D1Composite, CLSID_D2D1Shadow, D2D1_ALPHA_MODE,
    D2D1_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT,
};
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, Prop};
use d2d::{Factory, float};
use std::rc::Rc;

/// IEC 61966-2-1 encode, copied verbatim from pdn-core `pixel/gamma.rs::linear_to_srgb` (f64).
fn linear_to_srgb(l: f64) -> f64 {
    if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Copied verbatim from pdn-core `effect/surface.rs::unpremultiply`: a ≤ 0 or NaN → transparent black.
fn unpremultiply(c: [f32; 4]) -> [f32; 4] {
    let a = c[3];
    if a.is_nan() || a <= 0.0 {
        return [0.0; 4];
    }
    [c[0] / a, c[1] / a, c[2] / a, a]
}

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let prefix = &a[1];
    let out = &a[2];
    let args = std::fs::read_to_string(format!("{prefix}.args")).expect("read .args");
    let f: Vec<&str> = args.split_whitespace().collect();
    let sigma: f32 = f[0].parse().unwrap();
    let opt: u32 = f[1].parse().unwrap();
    let dx: f32 = f[2].parse().unwrap();
    let dy: f32 = f[3].parse().unwrap();
    let color = [f[4].parse().unwrap(), f[5].parse().unwrap(), f[6].parse().unwrap(), f[7].parse().unwrap()];
    let shadow_only: bool = f[8] != "0";
    let (w, h): (u32, u32) = (f[9].parse().unwrap(), f[10].parse().unwrap());
    let src8 = std::fs::read(format!("{prefix}.src8.bgra")).expect("read .src8.bgra");
    assert_eq!(src8.len(), (w * h * 4) as usize, "src8 size");
    // color[3] is opacity (the Shadow colour's straight alpha); color[0..3] already linearized.

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    // FLOAT target, DEFAULT 8bpc intermediate precision: Shadow+Affine realise at 8bpc; only the final
    // Composite (point-evaluable) keeps exact f32 (no round-ties-even store on the output).
    dc.set_target_bitmap(float::float_target(w, h, 96.0, DXGI_FORMAT_R32G32B32A32_FLOAT, D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    // The 8bpc premultiplied-LINEAR source node (ConvertAlpha(Premultiply) no-op on premultiplied data).
    let src = Rc::new(Bitmap {
        width: w,
        height: h,
        premul_bgra: src8,
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alpha: D2D1_ALPHA_MODE_PREMULTIPLIED,
        dpi: (96.0, 96.0),
    });

    // D2D Shadow (CBlur<1,1>): colorize-by-alpha premul(color)·inα, then always-SOFT Gaussian blur.
    let shadow = dc.create_effect(CLSID_D2D1Shadow).expect("Shadow");
    shadow.set_value(0, Prop::F32(sigma)).unwrap(); // BlurStandardDeviation (px)
    shadow.set_value(1, Prop::V4(color)).unwrap(); // Color (straight; effect premultiplies)
    shadow.set_value(2, Prop::U32(opt)).unwrap(); // Optimization = Quality (2)
    shadow.set_input(0, Input::Bitmap(src.clone()));

    // AffineTransform2D: pure translation by (dx,dy); InterpolationMode default LINEAR, BorderMode
    // default SOFT (the genuine graph sets only the matrix).
    let affine = dc.create_effect(CLSID_D2D12DAffineTransform).expect("AffineTransform2D");
    affine.set_value(2, Prop::M32([1.0, 0.0, 0.0, 1.0, dx, dy])).unwrap(); // TransformMatrix
    affine.set_input(0, Input::Effect(shadow));

    // Output node: the affine for shadowOnly, else source OVER the translated shadow.
    let output = if shadow_only {
        affine
    } else {
        let composite = dc.create_effect(CLSID_D2D1Composite).expect("Composite");
        composite.set_value(0, Prop::U32(0)).unwrap(); // Mode = SourceOver
        composite.set_input(0, Input::Effect(affine)); // destination = translated shadow
        composite.set_input(1, Input::Bitmap(src)); // source = the (no-op ConvertAlpha) source
        composite
    };

    dc.draw_image_effect(&output, [0.0, 0.0], 1, 0); // composite 0 → float path keeps exact f32
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data; // premultiplied LINEAR RGBA f32 LE
    assert_eq!(data.len(), (w * h * 16) as usize, "float target size");

    // Match the port's prequant_f32(Linear, Premultiplied): unpremultiply, then linear→sRGB on RGB.
    let mut sout = vec![0u8; data.len()];
    for i in 0..(w * h) as usize {
        let px: [f32; 4] = core::array::from_fn(|k| {
            let j = i * 16 + k * 4;
            f32::from_le_bytes([data[j], data[j + 1], data[j + 2], data[j + 3]])
        });
        let s = unpremultiply(px);
        let o = [
            linear_to_srgb(s[0] as f64) as f32,
            linear_to_srgb(s[1] as f64) as f32,
            linear_to_srgb(s[2] as f64) as f32,
            s[3],
        ];
        for (k, v) in o.iter().enumerate() {
            sout[i * 16 + k * 4..i * 16 + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(out, &sout).expect("write out.f32");
    eprintln!(
        "rendered genuine drop-shadow graph (σ={sigma}, opt={opt}, d=({dx},{dy}), shadowOnly={shadow_only}) -> {out}"
    );
}
