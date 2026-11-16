//! Like pdngpu-run but renders to an R32G32B32A32_FLOAT target (no 8bpc clamp), for accumulating
//! blurs whose premultiplied Σ exceeds [0,1] before the effect's unpremultiply. Dumps raw f32 RGBA.
//! Usage: `pdngpu-runf <shader.dxbc> <cbuffer.bin> <in.bgra> <out.f32> [W H]`.
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, PixelShaderEffect};
use d2d::{float, Factory};
use std::rc::Rc;
use windows_core::GUID;
fn main() {
    let a: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&a[1]).unwrap();
    let cbuffer = std::fs::read(&a[2]).unwrap();
    let in_bgra = std::fs::read(&a[3]).unwrap();
    let out = &a[4];
    let (w, h): (u32, u32) = (a.get(5).map_or(32, |s| s.parse().unwrap()), a.get(6).map_or(32, |s| s.parse().unwrap()));
    let rgba: Vec<u8> = in_bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let id = GUID::from_u128(0x5a11_00f7_0000_4000_8000_00005a1100f7u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_buffer_precision(5);
    dc.set_target_bitmap(float::float_target(w, h, 96.0, d2d::abi::DXGI_FORMAT(2), d2d::abi::D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect><Property name='DisplayName' type='string' value='PdnGpuRunF'/>\
<Property name='Author' type='string' value='opend2d'/><Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu runf'/><Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2) = (dxbc.clone(), cbuffer.clone());
    factory.register_effect_from_string(id, xml, Rc::new(move || Box::new(PixelShaderEffect { shader_id: id, dxbc: d2.clone(), cbuffer: c2.clone(), input_count: 1, precision: None }))).unwrap();
    let eff = dc.create_effect(id).unwrap();
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(w, h, &rgba))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    std::fs::write(out, &dc.take_target().unwrap().data).unwrap();
    eprintln!("ran genuine shader (float) -> {out}");
}
