//! Oracle for the GENUINE `PdnGlowEffect` graph (GlowGpuEffect) on opend2d's d2d executor (8bpc
//! premultiplied node buffers, == native WARP). Assembles the exact decompiled graph from d2d
//! built-ins plus genuine's compiled BrightnessContrast/FullContrast pixel shader:
//!
//!   Premultiply(src) ─┬─────────────────────────────────────────────────────────┐
//!                     └─ GaussianBlur{σ, Quality, Hard}                           │
//!                        → UnPremultiply → <BC shader>[8bpc input] → Clamp[0,1]   │
//!                        → Premultiply ─────────────────── Blend(Screen, bg=┘, fg) → UnPremultiply
//!
//! `WorkingSpace` = sRGB, so no gamma nodes (genuine's ConvertGamma are identity, mode 4). The
//! brightnessContrast sub-graph's ConvertAlpha(premul↔straight) == D2D Premultiply/UnPremultiply, and
//! its ClampEffect == an identity ColorMatrix with ClampOutput. σ = glow·1.25/3 (f32), and the
//! shader + constants are chosen exactly as PdnBrightnessContrastEffect.OnPrepareForRender.
//!
//! Renders the final UnPremultiply output to an R32G32B32A32_FLOAT target with the DEFAULT 8bpc
//! intermediate precision (so the blur + shader inputs quantise to 8bpc, but the final output keeps
//! its exact f32 — no round-ties-even store), and dumps straight RGBA f32 (LE) for a bit-exact
//! compare against the port's `classic_oracle glowEffect` prequant f32 (PDNGPU_F32).
//!
//! Usage: `pdngpu-glowgraph <radius> <brightness> <contrast> <in.straight.bgra> <out.f32> <W> <H>`.
use d2d::abi::{
    CLSID_D2D1Blend, CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, CLSID_D2D1Premultiply,
    CLSID_D2D1UnPremultiply, D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_STRAIGHT,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT,
};
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, PixelShaderEffect, Prop};
use d2d::{Factory, float};
use std::rc::Rc;
use windows_core::GUID;

const SHADER_DIR: &str = "pdn-host/paintnet-gpu/.build/shaders";

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let radius: f32 = a[1].parse().expect("radius");
    let brightness: f32 = a[2].parse().expect("brightness");
    let contrast: f32 = a[3].parse().expect("contrast");
    let in_bgra = std::fs::read(&a[4]).expect("in.straight.bgra");
    let out = &a[5];
    let (w, h): (u32, u32) = (a[6].parse().unwrap(), a[7].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");

    // σ = StandardDeviation.FromRadius(glow·1.25) = glow·1.25/3 (GlowGpuEffect, f32).
    let glowf = radius.clamp(1.0, 20.0);
    let sigma = (glowf * 1.25_f32) / 3.0_f32;
    // GlowGpuEffect: int brightness/contrast → /100 (clamp ±1) → PdnBrightnessContrast ×100 (f32).
    let num_b = 100.0_f32 * (brightness / 100.0).clamp(-1.0, 1.0);
    let num_c = 100.0_f32 * (contrast / 100.0).clamp(-1.0, 1.0);
    let (multiply, num3) = if num_c < 0.0 {
        (num_c + 100.0, 100.0)
    } else if num_c > 0.0 {
        (100.0, 100.0 - num_c)
    } else {
        (1.0_f32, 1.0_f32)
    };
    // PdnBrightnessContrastEffect.OnPrepareForRender: shader selection + constant buffer (b0).
    let (shader_file, cbuffer): (&str, Vec<u8>) = if num3 == 0.0 {
        let bnorm = num_b / 255.0; // FullContrastShader(brightnessNorm)
        ("PdnBrightnessContrastEffect_FullContrastShader.dxbc", bnorm.to_le_bytes().to_vec())
    } else {
        let (pre_add, post_add) = if num3 == 100.0 { (0.0_f32, num_b / 255.0) } else { (num_b / 255.0, 0.0_f32) };
        let inv_div = 1.0_f32 / num3;
        let bytes: Vec<u8> = [pre_add, multiply, inv_div, post_add].iter().flat_map(|f| f.to_le_bytes()).collect();
        ("PdnBrightnessContrastEffect_BrightnessContrastShader.dxbc", bytes) // (preAdd, multiply, invDivide, postAdd)
    };
    let dxbc = std::fs::read(format!("{SHADER_DIR}/{shader_file}")).expect("read BC shader dxbc");

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    // FLOAT target, DEFAULT 8bpc intermediate precision: the blur + shader-input realises still
    // quantise to 8bpc; only the final output keeps f32 (no round-ties-even store).
    dc.set_target_bitmap(float::float_target(w, h, 96.0, DXGI_FORMAT_R32G32B32A32_FLOAT, D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    // Straight input: store the BGRA bytes verbatim (alpha STRAIGHT) so bitmap_texel yields straight
    // c/255 and PremultiplyEffect2 premultiplies once in f32 (genuine's straight SourceImage).
    let src = Bitmap {
        width: w,
        height: h,
        premul_bgra: in_bgra,
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alpha: D2D1_ALPHA_MODE_STRAIGHT,
        dpi: (96.0, 96.0),
    };

    let pre = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply");
    pre.set_input(0, Input::Bitmap(Rc::new(src)));

    let blur = dc.create_effect(CLSID_D2D1GaussianBlur).expect("GaussianBlur");
    blur.set_value(0, Prop::F32(sigma)).unwrap(); // StandardDeviation (px)
    blur.set_value(1, Prop::U32(2)).unwrap(); // Optimization = Quality
    blur.set_value(2, Prop::U32(1)).unwrap(); // BorderMode = Hard
    blur.set_input(0, Input::Effect(pre.clone()));

    let unp = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply");
    unp.set_input(0, Input::Effect(blur));

    // Genuine BrightnessContrast / FullContrast pixel shader (its b0 constants). The executor realises
    // this shader's SIMPLE input at the upstream 8bpc precision before sampling — exactly as native.
    let sid = GUID::from_u128(0x5a11_6104_0000_4000_8000_00005a116104u128);
    let (d2, c2) = (dxbc.clone(), cbuffer.clone());
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnGpuGlowBC'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='glow brightness/contrast'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    factory
        .register_effect_from_string(sid, xml, Rc::new(move || Box::new(PixelShaderEffect { shader_id: sid, dxbc: d2.clone(), cbuffer: c2.clone(), input_count: 1, precision: None })))
        .expect("register BC shader");
    let bc = dc.create_effect(sid).expect("create BC shader");
    bc.set_input(0, Input::Effect(unp));

    // ClampEffect == identity ColorMatrix (STRAIGHT) with ClampOutput: saturate rgba to [0,1].
    #[rustfmt::skip]
    let ident = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0, 0.0,0.0,0.0,0.0];
    let clamp = dc.create_effect(CLSID_D2D1ColorMatrix).expect("ColorMatrix (clamp)");
    clamp.set_value(0, Prop::M54(ident)).unwrap();
    clamp.set_value(1, Prop::U32(2)).unwrap(); // alpha mode = STRAIGHT
    clamp.set_value(2, Prop::Bool(true)).unwrap(); // ClampOutput
    clamp.set_input(0, Input::Effect(bc));

    let prec = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (output)");
    prec.set_input(0, Input::Effect(clamp));

    let blend = dc.create_effect(CLSID_D2D1Blend).expect("Blend");
    blend.set_value(0, Prop::U32(1)).unwrap(); // Mode = Screen
    blend.set_input(0, Input::Effect(pre)); // backdrop = Premultiply(src)
    blend.set_input(1, Input::Effect(prec)); // source = brightnessContrast node

    let unp_out = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (output)");
    unp_out.set_input(0, Input::Effect(blend));

    dc.draw_image_effect(&unp_out, [0.0, 0.0], 1, 0); // composite 0 → float path keeps exact f32
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    assert_eq!(data.len(), (w * h * 16) as usize, "float target size");
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine glow graph (σ={sigma}, b={num_b}, c={num_c}, {shader_file}) -> {out}");
}
