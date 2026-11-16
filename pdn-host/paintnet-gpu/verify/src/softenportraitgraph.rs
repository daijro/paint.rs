//! Oracle for the GENUINE `PdnSoftenPortraitEffect` graph (SoftenPortraitGpuEffect, WorkingSpace =
//! sRGB, straight in/out) on opend2d's d2d executor (8bpc premultiplied node buffers, == native
//! WARP). Assembles the exact decompiled graph from d2d built-ins plus genuine's compiled
//! BrightnessContrast/FullContrast shader and the HlslBinaryOperator `MultiplyInputValue` shader:
//!
//!   Premultiply(src) ─┬─ Grayscale → UnPremultiply → [× value2 (warmth)] → Premultiply ─┐
//!                     │                                                                  Mix(Overlay) → UnPremultiply
//!                     └─ GaussianBlur{σ,Quality,Hard} → UnPremultiply → <BC>[8bpc] → Clamp → Premultiply ─┘
//!
//! AlphaMode = Straight (set by the wrapper), so the input/output Premultiply/UnPremultiply are
//! enabled. `MixMode.BlendOverlay` (1011) == D2D Blend mode 11 (Overlay) on premultiplied inputs
//! (dst = tint branch, src = brightness/contrast branch). The grayscale tint multiplies the straight
//! grey by `value2 = (1+w, 1, 1−w, 1)`; when warmth == 0 that is a passthrough (no shader). The BC
//! sub-graph is identical to Glow's (UnPremultiply → BC shader[8bpc simple input] → Clamp[0,1] →
//! Premultiply); `num_b = 100·clamp(lighting/100, −1, 1)`, `num_c = 100·clamp(−lighting/200, −1, 1)`.
//! σ = ((softness·3)·1.25)/3 (f32). WorkingSpace = sRGB so no gamma nodes.
//!
//! Renders the final UnPremultiply to an R32G32B32A32_FLOAT target with DEFAULT 8bpc intermediates
//! (so the blur + both shader simple-inputs quantise to 8bpc, the final output keeps exact f32) and
//! dumps straight RGBA f32 (LE) for a bit-exact compare vs the port's `classic_oracle
//! softenPortraitEffect` prequant f32 (PDNGPU_F32).
//!
//! Usage: `pdngpu-softenportraitgraph <softness> <lighting> <warmth> <in.straight.bgra> <out.f32> <W> <H>`.
use d2d::abi::{
    CLSID_D2D1Blend, CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, CLSID_D2D1Grayscale,
    CLSID_D2D1Premultiply, CLSID_D2D1UnPremultiply, D2D1_ALPHA_MODE, D2D1_ALPHA_MODE_STRAIGHT,
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
    let softness: f64 = a[1].parse().expect("softness");
    let lighting: i32 = a[2].parse().expect("lighting");
    let warmth: i32 = a[3].parse().expect("warmth");
    let in_bgra = std::fs::read(&a[4]).expect("in.straight.bgra");
    let out = &a[5];
    let (w, h): (u32, u32) = (a[6].parse().unwrap(), a[7].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");

    // σ = StandardDeviation.FromRadius(softness·3·1.25) = ((s·3)·1.25)/3 (PdnSoftenPortraitEffect, f32).
    let s = (softness.clamp(0.0, 10.0)) as f32;
    let num4 = (s * 3.0_f32) * 1.25_f32;
    let sigma = num4 / 3.0_f32;

    // PdnBrightnessContrast params: Brightness = clamp(lighting/100, −0.2, 0.2), Contrast = −B/2;
    // then num_b = 100·clamp(B, −1, 1), num_c = 100·clamp(C, −1, 1) (PdnBrightnessContrast prepare).
    let lighting_f = (lighting as f64 / 100.0) as f32;
    let num2 = lighting_f.clamp(-0.2, 0.2);
    let brightness = num2;
    let contrast = (0.0_f32 - num2) / 2.0_f32;
    let num_b = 100.0_f32 * brightness.clamp(-1.0, 1.0);
    let num_c = 100.0_f32 * contrast.clamp(-1.0, 1.0);
    let (multiply, num3) = if num_c < 0.0 {
        (num_c + 100.0, 100.0)
    } else if num_c > 0.0 {
        (100.0, 100.0 - num_c)
    } else {
        (1.0_f32, 1.0_f32)
    };
    let (bc_shader_file, bc_cbuffer): (&str, Vec<u8>) = if num3 == 0.0 {
        let bnorm = num_b / 255.0;
        ("PdnBrightnessContrastEffect_FullContrastShader.dxbc", bnorm.to_le_bytes().to_vec())
    } else {
        let (pre_add, post_add) = if num3 == 100.0 { (0.0_f32, num_b / 255.0) } else { (num_b / 255.0, 0.0_f32) };
        let inv_div = 1.0_f32 / num3;
        let bytes: Vec<u8> = [pre_add, multiply, inv_div, post_add].iter().flat_map(|f| f.to_le_bytes()).collect();
        ("PdnBrightnessContrastEffect_BrightnessContrastShader.dxbc", bytes)
    };
    let bc_dxbc = std::fs::read(format!("{SHADER_DIR}/{bc_shader_file}")).expect("read BC shader dxbc");

    // Warmth: value2 = (1+w, 1, 1−w, 1); w == 0 ⇒ value2 == One ⇒ HlslBinaryOperator passthrough.
    let w_warm = (warmth as f64 / 100.0) as f32;
    let w_warm = w_warm.clamp(0.0, 0.2);
    let num6 = 1.0_f32 + w_warm;
    let num7 = 1.0_f32 - w_warm;
    let passthrough = num6 == 1.0 && num7 == 1.0;

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(float::float_target(w, h, 96.0, DXGI_FORMAT_R32G32B32A32_FLOAT, D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    let src = Bitmap {
        width: w,
        height: h,
        premul_bgra: in_bgra,
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alpha: D2D1_ALPHA_MODE_STRAIGHT,
        dpi: (96.0, 96.0),
    };

    // premultiplyInput (enabled: AlphaMode = Straight).
    let pre = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (input)");
    pre.set_input(0, Input::Bitmap(Rc::new(src)));

    // --- tint branch: Grayscale → UnPremultiply → [× value2] → Premultiply ---
    let gray = dc.create_effect(CLSID_D2D1Grayscale).expect("Grayscale");
    gray.set_input(0, Input::Effect(pre.clone()));
    let ungray = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (grayscale)");
    ungray.set_input(0, Input::Effect(gray));

    let rb = if passthrough {
        ungray
    } else {
        // HlslBinaryOperatorTransform MultiplyInputValueShader: `return D2DGetInput(0) * value2;`
        // D2DInputSimple(0) ⇒ the executor realises the straight grey input at 8bpc before the mul.
        let mul_dxbc = std::fs::read(format!("{SHADER_DIR}/HlslBinaryOperatorTransform_MultiplyInputValueShader.dxbc"))
            .expect("read multiply shader dxbc");
        let cbuffer: Vec<u8> = [num6, 1.0_f32, num7, 1.0_f32].iter().flat_map(|f| f.to_le_bytes()).collect();
        let sid = GUID::from_u128(0x4ab2_4acc_11f7_bbcf_463b_9cc95ebf1168u128);
        let (d, c) = (mul_dxbc.clone(), cbuffer.clone());
        factory
            .register_effect_from_string(
                sid,
                "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='HlslMultiplyInputValue'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='input * value2'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>",
                Rc::new(move || Box::new(PixelShaderEffect { shader_id: sid, dxbc: d.clone(), cbuffer: c.clone(), input_count: 1, precision: None })),
            )
            .expect("register multiply shader");
        let m = dc.create_effect(sid).expect("create multiply shader");
        m.set_input(0, Input::Effect(ungray));
        m
    };
    let prb = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (redBlueAdjust)");
    prb.set_input(0, Input::Effect(rb));

    // --- lit branch: GaussianBlur → UnPremultiply → BC shader → Clamp → Premultiply ---
    let blur = dc.create_effect(CLSID_D2D1GaussianBlur).expect("GaussianBlur");
    blur.set_value(0, Prop::F32(sigma)).unwrap(); // StandardDeviation (px)
    blur.set_value(1, Prop::U32(2)).unwrap(); // Optimization = Quality
    blur.set_value(2, Prop::U32(1)).unwrap(); // BorderMode = Hard
    blur.set_input(0, Input::Effect(pre.clone()));

    let unp = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (blur)");
    unp.set_input(0, Input::Effect(blur));

    let bc_sid = GUID::from_u128(0x5a11_6104_0000_4001_8000_00005a116104u128);
    let (d2, c2) = (bc_dxbc.clone(), bc_cbuffer.clone());
    factory
        .register_effect_from_string(
            bc_sid,
            "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnSoftenPortraitBC'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='soften lighting brightness/contrast'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>",
            Rc::new(move || Box::new(PixelShaderEffect { shader_id: bc_sid, dxbc: d2.clone(), cbuffer: c2.clone(), input_count: 1, precision: None })),
        )
        .expect("register BC shader");
    let bc = dc.create_effect(bc_sid).expect("create BC shader");
    bc.set_input(0, Input::Effect(unp));

    // ClampEffect == identity ColorMatrix (STRAIGHT) with ClampOutput: saturate rgba to [0,1].
    #[rustfmt::skip]
    let ident = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0, 0.0,0.0,0.0,0.0];
    let clamp = dc.create_effect(CLSID_D2D1ColorMatrix).expect("ColorMatrix (clamp)");
    clamp.set_value(0, Prop::M54(ident)).unwrap();
    clamp.set_value(1, Prop::U32(2)).unwrap(); // alpha mode = STRAIGHT
    clamp.set_value(2, Prop::Bool(true)).unwrap(); // ClampOutput
    clamp.set_input(0, Input::Effect(bc));

    let prec = dc.create_effect(CLSID_D2D1Premultiply).expect("Premultiply (brightnessContrast)");
    prec.set_input(0, Input::Effect(clamp));

    // Mix = BlendOverlay (11), dst = tint (input 0), src = brightnessContrast (input 1), premultiplied.
    let blend = dc.create_effect(CLSID_D2D1Blend).expect("Blend");
    blend.set_value(0, Prop::U32(11)).unwrap(); // Mode = Overlay
    blend.set_input(0, Input::Effect(prb));
    blend.set_input(1, Input::Effect(prec));

    // unPremultiplyOutput (enabled: AlphaMode = Straight).
    let unp_out = dc.create_effect(CLSID_D2D1UnPremultiply).expect("UnPremultiply (output)");
    unp_out.set_input(0, Input::Effect(blend));

    dc.draw_image_effect(&unp_out, [0.0, 0.0], 1, 0); // composite 0 → float path keeps exact f32
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    assert_eq!(data.len(), (w * h * 16) as usize, "float target size");
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!(
        "rendered genuine soften-portrait graph (σ={sigma}, num_b={num_b}, num_c={num_c}, warmth={w_warm}, passthrough={passthrough}, {bc_shader_file}) -> {out}"
    );
}
