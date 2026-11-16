//! Render a GENUINE Paint.NET coord-generating SampleMapShader (Bulge/Dents/PolarInversion/RotateZoom/
//! Spherize/TileReflection/Twist/Waves) through crates/d2d and dump the 32x32 R32G32B32A32_FLOAT coord
//! output for a WARP compare. These are the simple distorts: a float scene-position input
//! (D2DGetInput(0).xy), NO resource textures, and a float4(x,y,0,1) source-coordinate output — so they
//! need a float context + float target + a float input bitmap, but exercise only the executor's
//! arithmetic (sqrt/atan2/turbulence/matrices), not the res-tex path.
//!
//! Usage: `pdngpu-samplemap <SampleMap.dxbc> <cbuffer.bin> <out.f32>` (cbuffer = raw little-endian b0).
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{float, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

/// Scene positions (x+0.5, y+0.5, 0, 1) as the float input (pixel centres a SampleMap reads).
fn scene_positions() -> Vec<f32> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[x as f32 + 0.5, y as f32 + 0.5, 0.0, 1.0]);
        }
    }
    v
}

struct SampleMap {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
}
impl EffectImpl for SampleMap {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, Vec::new())?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read SampleMap.dxbc");
    let cbuffer = std::fs::read(&args[2]).expect("read cbuffer.bin");
    let out = &args[3];

    let id = GUID::from_u128(0x5a_11_0000_4000_8000_00005a110c00u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_buffer_precision(5); // 32FLOAT context
    dc.set_target_bitmap(float::float_target(32, 32, 96.0, d2d::abi::DXGI_FORMAT(2), d2d::abi::D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnSampleMap'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu samplemap'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2) = (dxbc.clone(), cbuffer.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(SampleMap { id, dxbc: d2.clone(), cbuffer: c2.clone() })))
        .expect("register SampleMap effect");
    let eff = dc.create_effect(id).expect("create SampleMap effect (shader rejected by executor?)");
    // args[4] (optional): a 32x32 RGBA f32 coord-map input file (for the SampleMapBorder shaders,
    // whose input is a coord map, not scene positions). Default: scene positions (coord-gen shaders).
    let input: Vec<f32> = match args.get(4) {
        Some(p) => std::fs::read(p).expect("read input.f32").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
        None => scene_positions(),
    };
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_f32_rgba(32, 32, &input))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine SampleMap -> {out} ({} bytes)", data.len());
}
