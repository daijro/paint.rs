//! Oracle for the GENUINE `PdnPencilSketchEffect` graph (`PencilSketchGpuEffect`, `ColorContext` =
//! `WorkingSpace` = sRGB, `AlphaMode` = Premultiplied) on opend2d's d2d executor (8bpc premultiplied
//! node buffers, == WARP). Both ConvertAlpha nodes are `ConvertAlphaMode` 3 (identity-on-premul) so
//! they are omitted; a final UnPremultiply yields the straight f32 the port's
//! `prequant_f32(Srgb, Premultiplied)` compares to. Decompiled graph (PdnPencilSketchEffect.Impl):
//!
//!   src ─Premultiply─┬─ Grayscale ───────────────────────────────────────── ColorDodge.in0 (backdrop)
//!                    └─ GaussianBlur{σ, Quality, Hard}
//!                         └─[PdnBrightnessContrast, Premul, Srgb-gamma=identity:]
//!                              UnPremultiply → BC shader[8bpc simple in] → Clamp → Premultiply
//!                         → Invert(ColorMatrix PREMUL: unpremul, 1−rgb, premul) → Grayscale ── ColorDodge.in1 (source)
//!
//!   out = Blend(ColorDodge, in0=backdrop, in1=source) → UnPremultiply → float straight.
//!
//! σ = StandardDeviation.FromRadius(tip·1.5) = (tip·1.5)/3 (f32). PdnBrightnessContrast props:
//! Brightness = num2 + 0.01, Contrast = −(num2 + 0.01), with num2 = clamp((range/100) f32, ±0.2);
//! then num_b/num_c = 100·clamp(prop, ±1) and the shader + constants are chosen exactly as
//! PdnBrightnessContrastEffect.OnPrepareForRender (as in glowgraph.rs / inksketchgraph.rs).
//! WorkingSpace = sRGB ⇒ ConvertGamma nodes identity ⇒ omitted.
//!
//! Renders the final UnPremultiply to an R32G32B32A32_FLOAT target with DEFAULT 8bpc intermediates
//! (the blur bakes and the BC shader simple-input quantise to 8bpc; the rest of the simple-node chain
//! fuses in f32 and the final output keeps exact f32), dumping straight RGBA f32 (LE).
//!
//! Usage: `pdngpu-pencilsketchgraph <tip 1..20> <range -20..20> <in.straight.bgra> <out.f32> <W> <H>`.
use d2d::abi::{
    CLSID_D2D1Blend, CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, CLSID_D2D1Grayscale,
    CLSID_D2D1Invert, CLSID_D2D1Premultiply, CLSID_D2D1UnPremultiply, D2D1_ALPHA_MODE,
    D2D1_ALPHA_MODE_STRAIGHT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT,
};
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, PixelShaderEffect, Prop};
use d2d::{Factory, float};
use std::rc::Rc;
use windows_core::GUID;

const SHADER_DIR: &str = "pdn-host/paintnet-gpu/.build/shaders";

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let tip: f64 = a[1].parse().expect("tip");
    let range: f64 = a[2].parse().expect("range");
    let in_bgra = std::fs::read(&a[3]).expect("in.straight.bgra");
    let out = &a[4];
    let (w, h): (u32, u32) = (a[5].parse().unwrap(), a[6].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");

    // Wrapper: PencilTipSize = (float)value (clamp 1..20); ColorRange = (float)(value/100) (clamp ±0.2).
    let tip_f = (tip as f32).clamp(1.0, 20.0);
    let num2 = ((range / 100.0) as f32).clamp(-0.2, 0.2);
    // PdnPencilSketch.OnPrepareForRender: BrightnessContrast Brightness = num2+0.01, Contrast = −(num2+0.01).
    let b_prop = num2 + 0.01_f32;
    let c_prop = 0.0_f32 - (num2 + 0.01_f32);
    // PdnBrightnessContrast then ×100 (clamp ±1).
    let num_b = 100.0_f32 * b_prop.clamp(-1.0, 1.0);
    let num_c = 100.0_f32 * c_prop.clamp(-1.0, 1.0);
    // σ = StandardDeviation.FromRadius(tip·1.5) = (tip·1.5)/3 (f32).
    let sigma = (tip_f * 1.5_f32) / 3.0_f32;

    // PdnBrightnessContrastEffect.OnPrepareForRender: shader selection + constant buffer (b0).
    let (multiply, num3) = if num_c < 0.0 {
        (num_c + 100.0, 100.0)
    } else if num_c > 0.0 {
        (100.0, 100.0 - num_c)
    } else {
        (1.0_f32, 1.0_f32)
    };
    let (bc_shader_file, bc_cbuffer): (&str, Vec<u8>) = if num3 == 0.0 {
        let bnorm = num_b / 255.0; // FullContrastShader(brightnessNorm)
        ("PdnBrightnessContrastEffect_FullContrastShader.dxbc", bnorm.to_le_bytes().to_vec())
    } else {
        let (pre_add, post_add) = if num3 == 100.0 { (0.0_f32, num_b / 255.0) } else { (num_b / 255.0, 0.0_f32) };
        let inv_div = 1.0_f32 / num3;
        let bytes: Vec<u8> = [pre_add, multiply, inv_div, post_add].iter().flat_map(|f| f.to_le_bytes()).collect();
        ("PdnBrightnessContrastEffect_BrightnessContrastShader.dxbc", bytes) // (preAdd, multiply, invDivide, postAdd)
    };
    let bc_dxbc = std::fs::read(format!("{SHADER_DIR}/{bc_shader_file}")).expect("read BC shader dxbc");

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(float::float_target(w, h, 96.0, DXGI_FORMAT_R32G32B32A32_FLOAT, D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    // Straight input: store the BGRA bytes verbatim (alpha STRAIGHT) so bitmap_texel yields c/255 and
    // Premultiply premultiplies once in f32 (genuine's premultiplied SourceImage via ConvertAlpha id).
    let src = Bitmap {
        width: w,
        height: h,
        premul_bgra: in_bgra,
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alpha: D2D1_ALPHA_MODE_STRAIGHT,
        dpi: (96.0, 96.0),
    };
    let pre = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (input)");
    pre.set_input(0, Input::Bitmap(Rc::new(src)));

    // --- backdrop branch: Grayscale(src) → ColorDodge.in0 ---
    let gray0 = dc.create_effect(CLSID_D2D1Grayscale).expect("Grayscale (input)");
    gray0.set_input(0, Input::Effect(pre.clone()));

    // --- source branch: GaussianBlur → PdnBrightnessContrast → Invert → Grayscale ---
    let blur = dc.create_effect(CLSID_D2D1GaussianBlur).expect("GaussianBlur");
    blur.set_value(0, Prop::F32(sigma)).unwrap(); // StandardDeviation
    blur.set_value(1, Prop::U32(2)).unwrap(); // Optimization = Quality
    blur.set_value(2, Prop::U32(1)).unwrap(); // BorderMode = Hard
    blur.set_input(0, Input::Effect(pre.clone()));

    // PdnBrightnessContrast internals (AlphaMode=Premul ⇒ convertInputAlpha=UnPremultiply,
    // convertOutputAlpha=Premultiply; InputGamma=Srgb=WorkingSpace ⇒ ConvertGamma identity, omitted).
    let unp = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (BC input)");
    unp.set_input(0, Input::Effect(blur));

    let bc_sid = GUID::from_u128(0x5a11_1247_0000_4002_8000_00005a111247u128);
    let (d2, c2) = (bc_dxbc.clone(), bc_cbuffer.clone());
    factory
        .register_effect_from_string(
            bc_sid,
            "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnPencilSketchBC'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='brightness/contrast'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>",
            Rc::new(move || Box::new(PixelShaderEffect { shader_id: bc_sid, dxbc: d2.clone(), cbuffer: c2.clone(), input_count: 1, precision: None })),
        )
        .expect("register BC shader");
    let bc = dc.create_effect(bc_sid).expect("create BC shader");
    bc.set_input(0, Input::Effect(unp));

    // ClampEffect == identity ColorMatrix (STRAIGHT) with ClampOutput.
    #[rustfmt::skip]
    let ident = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0, 0.0,0.0,0.0,0.0];
    let clamp = dc.create_effect(CLSID_D2D1ColorMatrix).expect("ColorMatrix (clamp)");
    clamp.set_value(0, Prop::M54(ident)).unwrap();
    clamp.set_value(1, Prop::U32(2)).unwrap(); // alpha mode = STRAIGHT
    clamp.set_value(2, Prop::Bool(true)).unwrap(); // ClampOutput
    clamp.set_input(0, Input::Effect(bc));

    let bcpre = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (BC output)");
    bcpre.set_input(0, Input::Effect(clamp));

    // Invert: D2D ColorMatrix PREMULTIPLIED (unpremul-or-transparent-black, out.rgb = 1−rgb, re-premul).
    let inv = dc.create_effect(CLSID_D2D1Invert).expect("Invert");
    inv.set_input(0, Input::Effect(bcpre));

    let gray1 = dc.create_effect(CLSID_D2D1Grayscale).expect("Grayscale (source)");
    gray1.set_input(0, Input::Effect(inv));

    // --- ColorDodge composite: in0 = backdrop (grayscale src), in1 = source (grayscale invert) ---
    let dodge = dc.create_effect(CLSID_D2D1Blend).expect("Blend (ColorDodge)");
    dodge.set_value(0, Prop::U32(9)).unwrap(); // Mode = ColorDodge (D2D1_BLEND_MODE 9)
    dodge.set_input(0, Input::Effect(gray0));
    dodge.set_input(1, Input::Effect(gray1));

    // convertOutputAlpha is mode-3 identity; the final UnPremultiply gives the straight f32 to compare.
    let unp_out = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (output)");
    unp_out.set_input(0, Input::Effect(dodge));

    dc.draw_image_effect(&unp_out, [0.0, 0.0], 1, 0); // composite 0 → float path keeps exact f32
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    assert_eq!(data.len(), (w * h * 16) as usize, "float target size");
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine pencil-sketch graph (σ={sigma}, num_b={num_b}, num_c={num_c}, {bc_shader_file}) -> {out}");
}
