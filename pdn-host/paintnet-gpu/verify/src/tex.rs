//! Generic runner for a GENUINE Paint.NET pixel shader that reads ONE 1-D Float32x4 resource texture
//! at register t1 (e.g. MotionBlur's sampleInfos, SurfaceBlur's disc RLE). Renders an 8bpc BGRA
//! target. Usage: `pdngpu-tex <shader.dxbc> <cbuffer.bin> <tex.f32> <in.bgra> <out.bgra> [W H]`.
//! `tex.f32` is a flat little-endian f32 stream; its length / 16 is the texel count.
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

struct Tex {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
    n: u32,
    tex: Vec<u8>,
}
impl EffectImpl for Tex {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        let t1 = ctx.create_resource_texture(1, &[self.n], 5, 4, &[0], &self.tex)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, vec![(1, t1)])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&a[1]).expect("dxbc");
    let cbuffer = std::fs::read(&a[2]).expect("cbuffer");
    let tex = std::fs::read(&a[3]).expect("tex.f32");
    let in_bgra = std::fs::read(&a[4]).expect("in.bgra");
    let out = &a[5];
    let (w, h): (u32, u32) = (a.get(6).map_or(32, |s| s.parse().unwrap()), a.get(7).map_or(32, |s| s.parse().unwrap()));
    let n = (tex.len() / 16) as u32;
    let rgba: Vec<u8> = in_bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let id = GUID::from_u128(0x7e_000001_0000_4000_8000_0000_7e000001u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(w, h, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect><Property name='DisplayName' type='string' value='PdnTex'/>\
<Property name='Author' type='string' value='opend2d'/><Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu tex'/><Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2, t2) = (dxbc.clone(), cbuffer.clone(), tex.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(Tex { id, dxbc: d2.clone(), cbuffer: c2.clone(), n, tex: t2.clone() })))
        .expect("register Tex effect");
    let eff = dc.create_effect(id).expect("create Tex effect (shader rejected?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(w, h, &rgba))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine tex-shader -> {out} ({} bytes, {n} texels)", data.len());
}
