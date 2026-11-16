//! Render the GENUINE Paint.NET Dents SampleMapShader through crates/d2d and dump the 32x32
//! R32G32B32A32_FLOAT coord output for a WARP compare. Dents is a coord generator (Perlin bump
//! refraction): reads a float scene-position input (D2DGetInput(0).xy) + ONE permute Texture1D<Half>
//! (16BPC_FLOAT) at register t1 of the 512-entry PERM-doubled table, and a 56-byte cbuffer
//! (center, scaleR, theta, detail, turbulence, refractionScale, seed, rotateTx_0, rotateTx_1).
//!
//! Usage: `pdngpu-dents <Dents.dxbc> <cbuffer.bin(56)> <permute.f16(1024)> <out.f32>`.
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{float, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

fn scene_positions() -> Vec<f32> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[x as f32 + 0.5, y as f32 + 0.5, 0.0, 1.0]);
        }
    }
    v
}

struct Dents {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
    permute: Vec<u8>, // 512 f16 LE
}
impl EffectImpl for Dents {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        // One 1-D 16BPC_FLOAT (precision 4 / CHANNEL_1) permute table, clamp, bound at register 1.
        let pt = ctx.create_resource_texture(1, &[512], 4, 1, &[0], &self.permute)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, vec![(1, pt)])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read Dents.dxbc");
    let cbuffer = std::fs::read(&args[2]).expect("read cbuffer.bin");
    let permute = std::fs::read(&args[3]).expect("read permute.f16");
    let out = &args[4];
    assert_eq!(cbuffer.len(), 56, "dents cbuffer is 56 bytes");
    assert_eq!(permute.len(), 1024, "permute is 512 f16 (1024 bytes)");

    let id = GUID::from_u128(0xde75_de75_0000_4000_8000_0000de175000u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_buffer_precision(5);
    dc.set_target_bitmap(float::float_target(32, 32, 96.0, d2d::abi::DXGI_FORMAT(2), d2d::abi::D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnDents'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu dents'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2, p2) = (dxbc.clone(), cbuffer.clone(), permute.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(Dents { id, dxbc: d2.clone(), cbuffer: c2.clone(), permute: p2.clone() })))
        .expect("register Dents effect");
    let eff = dc.create_effect(id).expect("create Dents effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_f32_rgba(32, 32, &scene_positions()))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine Dents -> {out} ({} bytes)", data.len());
}
