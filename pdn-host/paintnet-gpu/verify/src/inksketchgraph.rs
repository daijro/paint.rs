//! Oracle for the GENUINE `PdnInkSketchEffect` graph (InkSketchGpuEffect, `WorkingSpace` = sRGB,
//! `AlphaMode` = Premultiplied) on opend2d's d2d executor (8bpc premultiplied node buffers, == WARP).
//! Both ConvertAlpha nodes are `ConvertAlphaMode` 3 (identity-on-premul) so they are omitted; a final
//! UnPremultiply yields the straight f32 the port's `prequant_f32(Srgb, Premultiplied)` compares to.
//! Assembles the exact decompiled graph from d2d built-ins plus genuine's compiled `AdjustShader` and
//! the `PdnBrightnessContrast` shader that lives inside `PdnGlowEffect`:
//!
//!   Premultiply(src) ─┬─ ConvolveMatrix{5x5 INK_K, PreserveAlpha, Hard, ClampOutput}
//!                     │     → Grayscale → AdjustShader(threshold)[8bpc simple in] ──── darken.in0 (mask)
//!                     │
//!                     └─ [PdnGlow, AlphaMode=Premul ⇒ its premul/unpremul DISABLED]
//!                           GaussianBlur{σ=2.5, Quality, Hard} → UnPremultiply → BC[8bpc] → Clamp → Premultiply
//!                           → Blend(Screen, bg=Premultiply(src), fg=BC) ─────────────── darken.in1 (glow)
//!
//!   darken = Blend(Darken, in0=mask, in1=glow) → UnPremultiply → float straight.
//!
//! Glow fixed params: Glow=6 ⇒ σ = (6·1.25)/3 = 2.5 (f32); Brightness=Contrast=(0−(k−0.5))·2 where
//! k = clamp(color/100, 0, 1); then PdnBrightnessContrast's num_b/num_c = 100·clamp(prop, ±1) and the
//! shader + constants are chosen exactly as PdnBrightnessContrastEffect.OnPrepareForRender (as in
//! glowgraph.rs). threshold = clamp((ink/100) as f32, 0, 0.99). WorkingSpace = sRGB ⇒ no gamma nodes.
//!
//! Renders the final UnPremultiply to an R32G32B32A32_FLOAT target with DEFAULT 8bpc intermediates
//! (the convolve/blur bakes and both shader simple-inputs quantise to 8bpc; the final output keeps
//! exact f32), dumping straight RGBA f32 (LE).
//!
//! Usage: `pdngpu-inksketchgraph <ink 0..99> <color 0..100> <in.straight.bgra> <out.f32> <W> <H>`.
use d2d::abi::{
    CLSID_D2D1Blend, CLSID_D2D1ColorMatrix, CLSID_D2D1ConvolveMatrix, CLSID_D2D1GaussianBlur,
    CLSID_D2D1Grayscale, CLSID_D2D1Premultiply, CLSID_D2D1UnPremultiply, D2D1_ALPHA_MODE,
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
    let ink: f64 = a[1].parse().expect("ink");
    let color: f64 = a[2].parse().expect("color");
    let in_bgra = std::fs::read(&a[3]).expect("in.straight.bgra");
    let out = &a[4];
    let (w, h): (u32, u32) = (a[5].parse().unwrap(), a[6].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");

    // Wrapper: value/100 (f64→f32). PdnInkSketch.OnPrepareForRender: threshold = clamp(num, 0, 0.99),
    // Coloring num = clamp(num2, 0, 1), glow Brightness=Contrast = (0 − (num − 0.5))·2 (f32).
    let threshold = ((ink / 100.0) as f32).clamp(0.0, 0.99);
    let kf = ((color / 100.0) as f32).clamp(0.0, 1.0);
    let b_prop = (0.0_f32 - (kf - 0.5)) * 2.0_f32;
    // PdnGlow clamps to [-1,1], PdnBrightnessContrast then ×100 (clamp again).
    let num_b = 100.0_f32 * b_prop.clamp(-1.0, 1.0);
    let num_c = 100.0_f32 * b_prop.clamp(-1.0, 1.0);
    // σ = StandardDeviation.FromRadius(6·1.25) = (6·1.25)/3 (f32).
    let sigma = (6.0_f32 * 1.25_f32) / 3.0_f32;

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
    let adjust_dxbc = std::fs::read(format!("{SHADER_DIR}/PdnInkSketchEffect_AdjustShader.dxbc")).expect("read AdjustShader dxbc");

    // genuine convolutionMatrix (row-major, UNFLIPPED — the executor flips for convolution).
    #[rustfmt::skip]
    let kernel: Vec<f32> = vec![
        -1.0, -1.0, -5.0, -1.0, -1.0,
        -1.0, -1.0, -1.0, -1.0, -1.0,
        -1.0, -1.0, 30.0, -1.0, -1.0,
        -1.0, -1.0, -1.0, -1.0, -1.0,
        -1.0, -1.0, -1.0, -1.0, -1.0,
    ];

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

    // --- mask branch: ConvolveMatrix → Grayscale → AdjustShader ---
    let conv = dc.create_effect(CLSID_D2D1ConvolveMatrix).expect("ConvolveMatrix");
    conv.set_value(2, Prop::U32(5)).unwrap(); // KernelSizeX
    conv.set_value(3, Prop::U32(5)).unwrap(); // KernelSizeY
    conv.set_value(4, Prop::Floats(kernel.into())).unwrap(); // KernelMatrix (unflipped); divisor idx5=1, bias idx6=0 defaults
    conv.set_value(8, Prop::Bool(true)).unwrap(); // PreserveAlpha
    conv.set_value(9, Prop::U32(1)).unwrap(); // BorderMode.Hard (= clamp)
    conv.set_value(10, Prop::Bool(true)).unwrap(); // ClampOutput
    conv.set_input(0, Input::Effect(pre.clone()));

    let gray = dc.create_effect(CLSID_D2D1Grayscale).expect("Grayscale");
    gray.set_input(0, Input::Effect(conv));

    // Genuine AdjustShader: `input.a<=0 ? 0 : (input.r/input.a > threshold ? input.aaaa : (0,0,0,a))`.
    // D2DInputSimple(0) ⇒ the executor realises grayscale's PREMULTIPLIED output at 8bpc before sampling.
    let adj_sid = GUID::from_u128(0x5a11_1247_0000_4000_8000_00005a111247u128);
    let (ad, ac) = (adjust_dxbc.clone(), threshold.to_le_bytes().to_vec());
    factory
        .register_effect_from_string(
            adj_sid,
            "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnInkSketchAdjust'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='ink threshold'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>",
            Rc::new(move || Box::new(PixelShaderEffect { shader_id: adj_sid, dxbc: ad.clone(), cbuffer: ac.clone(), input_count: 1, precision: None })),
        )
        .expect("register adjust shader");
    let adj = dc.create_effect(adj_sid).expect("create adjust shader");
    adj.set_input(0, Input::Effect(gray));

    // --- glow branch (PdnGlow, AlphaMode=Premul ⇒ internal premul/unpremul DISABLED) ---
    let blur = dc.create_effect(CLSID_D2D1GaussianBlur).expect("GaussianBlur");
    blur.set_value(0, Prop::F32(sigma)).unwrap(); // StandardDeviation
    blur.set_value(1, Prop::U32(2)).unwrap(); // Optimization = Quality
    blur.set_value(2, Prop::U32(1)).unwrap(); // BorderMode = Hard
    blur.set_input(0, Input::Effect(pre.clone()));

    let unp = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (blur)");
    unp.set_input(0, Input::Effect(blur));

    let bc_sid = GUID::from_u128(0x5a11_1247_0000_4001_8000_00005a111247u128);
    let (d2, c2) = (bc_dxbc.clone(), bc_cbuffer.clone());
    factory
        .register_effect_from_string(
            bc_sid,
            "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnInkSketchGlowBC'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='glow brightness/contrast'/>\
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

    let prec = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (brightnessContrast)");
    prec.set_input(0, Input::Effect(clamp));

    let screen = dc.create_effect(CLSID_D2D1Blend).expect("Blend (Screen)");
    screen.set_value(0, Prop::U32(1)).unwrap(); // Mode = Screen
    screen.set_input(0, Input::Effect(pre.clone())); // backdrop = Premultiply(src)
    screen.set_input(1, Input::Effect(prec)); // source = brightnessContrast node

    // --- darken composite: in0 = adjust (mask), in1 = glow ---
    let darken = dc.create_effect(CLSID_D2D1Blend).expect("Blend (Darken)");
    darken.set_value(0, Prop::U32(2)).unwrap(); // Mode = Darken
    darken.set_input(0, Input::Effect(adj));
    darken.set_input(1, Input::Effect(screen));

    // convertOutputAlpha is mode-3 identity; the final UnPremultiply gives the straight f32 to compare.
    let unp_out = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (output)");
    unp_out.set_input(0, Input::Effect(darken));

    dc.draw_image_effect(&unp_out, [0.0, 0.0], 1, 0); // composite 0 → float path keeps exact f32
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    assert_eq!(data.len(), (w * h * 16) as usize, "float target size");
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine ink-sketch graph (t={threshold}, num_b={num_b}, num_c={num_c}, σ={sigma}, {bc_shader_file}) -> {out}");
}
