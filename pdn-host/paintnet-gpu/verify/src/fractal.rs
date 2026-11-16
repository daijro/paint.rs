//! Render a GENUINE Paint.NET GPU fractal pixel shader (Julia / Mandelbrot) through crates/d2d to an
//! R32G32B32A32_FLOAT target and dump raw f32 RGBA (premultiplied straight colour, one RGSS sample).
//! 0-input generator (D2D_INPUT_COUNT 0, D2D_REQUIRES_SCENE_POSITION), no resource textures.
//! Usage: `pdngpu-fractal <shader.dxbc> <cbuffer.bin> <out.f32> [W H]`.
use d2d::effects::{EffectContext, EffectImpl, Props, TransformGraph};
use d2d::{float, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

struct Frac {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
}
impl EffectImpl for Frac {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 0, vec![])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&a[1]).expect("read shader.dxbc");
    let cbuffer = std::fs::read(&a[2]).expect("read cbuffer.bin");
    let out = &a[3];
    let (w, h): (u32, u32) = (a.get(4).map_or(32, |s| s.parse().unwrap()), a.get(5).map_or(32, |s| s.parse().unwrap()));
    let id = GUID::from_u128(0xf7ac_1a10_0000_4000_8000_0000f7ac1a00u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_buffer_precision(5);
    dc.set_target_bitmap(float::float_target(w, h, 96.0, d2d::abi::DXGI_FORMAT(2), d2d::abi::D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect><Property name='DisplayName' type='string' value='PdnFractal'/>\
<Property name='Author' type='string' value='opend2d'/><Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu fractal'/><Inputs></Inputs></Effect>";
    let (d2, c2) = (dxbc.clone(), cbuffer.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(Frac { id, dxbc: d2.clone(), cbuffer: c2.clone() })))
        .expect("register Fractal effect");
    let eff = dc.create_effect(id).expect("create Fractal effect (shader rejected by executor?)");
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine fractal -> {out} ({} bytes)", data.len());
}
