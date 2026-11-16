//! Render the GENUINE Paint.NET SampleMap APPLY shader (MultisampleInputEffect.Sample1InputShader,
//! from PaintDotNet.Windows.Direct2D1.Managed) through crates/d2d and dump the 32x32 BGRA8 for a WARP
//! compare. The apply gathers the source through a coordinate map:
//!   out = input1.w * D2DSampleInputAtPosition(0 /*source, linear*/, input1.xy /*coord map, point*/)
//! input1 is the coord map (a float image from a coord-gen SampleMap); input0 is the source. No cbuffer.
//!
//! Usage: `pdngpu-samplemap-apply <Sample1Input.dxbc> <out.bgra>`. A synthetic float coord map
//! (2x zoom about the centre, fractional ⇒ bilinear) stands in for the coord-gen output.
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

fn gradient() -> Vec<u8> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[(8 * x).min(255) as u8, (8 * y).min(255) as u8, (4 * (x + y)).min(255) as u8, 255]);
        }
    }
    v
}

/// Coord map (float): sample the source at a 2x zoom about (16,16) — fractional coords exercise the
/// source's bilinear tap. RGBA = (srcX, srcY, 0, alpha=1).
fn coord_map() -> Vec<f32> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let sx = 16.0 + (x as f32 + 0.5 - 16.0) * 0.5;
            let sy = 16.0 + (y as f32 + 0.5 - 16.0) * 0.5;
            v.extend_from_slice(&[sx, sy, 0.0, 1.0]);
        }
    }
    v
}

struct Apply {
    id: GUID,
    dxbc: Vec<u8>,
}
impl EffectImpl for Apply {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        // 2 inputs (input0 source complex/linear, input1 coord map simple/point), no cbuffer, no res-tex.
        let t = ctx.pixel_shader_transform(self.id, Vec::new(), 2, Vec::new())?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read Sample1Input.dxbc");
    let out = &args[2];

    // Optional external source (args[3], 32x32 BGRA8 straight) + coord map (args[4], 32x32 RGBA f32),
    // so the one apply bin serves the SampleMap-distort sweep (coord-gen output fed as the map).
    let src_rgba: Vec<u8> = match args.get(3) {
        Some(p) => std::fs::read(p).expect("read source.bgra").chunks_exact(4).flat_map(|b| [b[2], b[1], b[0], b[3]]).collect(),
        None => gradient(),
    };
    let map: Vec<f32> = match args.get(4) {
        Some(p) => std::fs::read(p).expect("read coordmap.f32").chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
        None => coord_map(),
    };

    let id = GUID::from_u128(0x5a11_a991_0000_4000_8000_00005a11a900u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(32, 32, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnSampleMapApply'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu samplemap apply'/>\
<Inputs><Input name='Source0'/><Input name='Map1'/></Inputs></Effect>";
    let d2 = dxbc.clone();
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(Apply { id, dxbc: d2.clone() })))
        .expect("register apply effect");
    let eff = dc.create_effect(id).expect("create apply effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(32, 32, &src_rgba))));
    eff.set_input(1, Input::Bitmap(Rc::new(Bitmap::from_f32_rgba(32, 32, &map))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine SampleMap apply -> {out} ({} bytes)", data.len());
}
