//! Generic single-input genuine-shader runner for the port GPU-effect oracle sweep. Runs one built-in
//! Paint.NET pixel shader (its DXBC) through crates/d2d's effect graph over a caller-supplied input
//! image and writes the premultiplied BGRA8 result. Because the campaign proved crates/d2d byte-exact
//! vs WARP vs genuine for these shaders, this result is the genuine oracle for the effect on that input
//! (on an OPAQUE input the effect's default UnPremultiply→shader→Premultiply wrap is a no-op, so the
//! bare shader == the full effect). Compare it to the paint.rs port via `pdn-core`'s `classic_oracle`.
//!
//! Usage: `pdngpu-run <shader.dxbc> <cbuffer.bin> <in.bgra> <out.bgra> [W H]` (default 32x32).
//! cbuffer.bin = raw little-endian constant-buffer bytes (int/float mixed, exactly as D2D sees b0).
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, PixelShaderEffect};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::GUID;

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&a[1]).expect("read shader.dxbc");
    let cbuffer = std::fs::read(&a[2]).expect("read cbuffer.bin");
    let in_bgra = std::fs::read(&a[3]).expect("read in.bgra");
    let out = &a[4];
    let (w, h): (u32, u32) = (a.get(5).map_or(32, |s| s.parse().unwrap()), a.get(6).map_or(32, |s| s.parse().unwrap()));
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "in.bgra size != w*h*4");
    // BGRA8 (straight) -> RGBA for from_straight_rgba (opaque input => premul == straight).
    let rgba: Vec<u8> = in_bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();

    let id = GUID::from_u128(0x5a11_0072_0000_4000_8000_00005a110072u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(w, h, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnGpuRun'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu run'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2) = (dxbc.clone(), cbuffer.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(PixelShaderEffect { shader_id: id, dxbc: d2.clone(), cbuffer: c2.clone(), input_count: 1, precision: None })))
        .expect("register shader effect");
    let eff = dc.create_effect(id).expect("create effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(w, h, &rgba))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("ran genuine shader -> {out} ({} bytes)", data.len());
}
