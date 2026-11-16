//! Render the GENUINE Paint.NET GPU ReduceNoise pixel shader through crates/d2d. Same disc-kernel
//! Float4 RLE resource texture at t1 as SurfaceBlur (and the same D2D_INPUT0_COMPLEX offset sampling),
//! but a different body (counts darker neighbours per channel) and cbuffer: `area` (total tap count,
//! float), `factor` (strength), then samplingRectXYWH. Dump the 32x32 BGRA8 for a WARP compare.
//!
//! Usage: `pdngpu-reducenoise <ReduceNoise.dxbc> <out.bgra> [radius] [factor]`.
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

/// Disc `dx²+dy² <= ((2R+1)²+2)/4`, one Float4 texel per row `(dxBegin, dy, dxLength, 0)` (ints
/// bit-cast to f32). Returns (texel count, total tap count, raw bytes). Identical to SurfaceBlur's.
fn disc_rle(radius: i32) -> (u32, u32, Vec<u8>) {
    let cutoff = ((radius * 2 + 1) * (radius * 2 + 1) + 2) / 4;
    let (mut data, mut n, mut taps) = (Vec::new(), 0u32, 0u32);
    for dy in -radius..=radius {
        let (mut dx_begin, mut dx_len) = (i32::MAX, 0i32);
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= cutoff {
                dx_begin = dx_begin.min(dx);
                dx_len += 1;
            }
        }
        data.extend_from_slice(&dx_begin.to_le_bytes());
        data.extend_from_slice(&dy.to_le_bytes());
        data.extend_from_slice(&dx_len.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        n += 1;
        taps += dx_len as u32;
    }
    (n, taps, data)
}

fn gradient() -> Vec<u8> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[(8 * x).min(255) as u8, (8 * y).min(255) as u8, (4 * (x + y)).min(255) as u8, 255]);
        }
    }
    v
}

struct ReduceNoise {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
    n: u32,
    rle: Vec<u8>,
}
impl EffectImpl for ReduceNoise {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        let tex = ctx.create_resource_texture(1, &[self.n], 5, 4, &[0], &self.rle)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, vec![(1, tex)])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read ReduceNoise.dxbc");
    let out = &args[2];
    let radius: i32 = args.get(3).map_or(3, |s| s.parse().unwrap());
    let factor: f32 = args.get(4).map_or(0.5, |s| s.parse().unwrap());
    let (n, taps, rle) = disc_rle(radius);

    // cbuffer: area@0 (= total tap count, float), factor@4, samplingRectXYWH(Int4)@16 -> 32 bytes.
    let mut cbuffer = vec![0u8; 32];
    cbuffer[0..4].copy_from_slice(&(taps as f32).to_le_bytes());
    cbuffer[4..8].copy_from_slice(&factor.to_le_bytes());
    for (i, v) in [-radius, -radius, 2 * radius + 1, 2 * radius + 1].iter().enumerate() {
        cbuffer[16 + i * 4..20 + i * 4].copy_from_slice(&v.to_le_bytes());
    }

    let id = GUID::from_u128(0x5edc_5edc_0000_4000_8000_00005edc5e00u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(32, 32, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnReduceNoise'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu reducenoise'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2, r2) = (dxbc.clone(), cbuffer.clone(), rle.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(ReduceNoise { id, dxbc: d2.clone(), cbuffer: c2.clone(), n, rle: r2.clone() })))
        .expect("register ReduceNoise effect");
    let eff = dc.create_effect(id).expect("create ReduceNoise effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(32, 32, &gradient()))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine ReduceNoise r={radius} area={taps} -> {out} ({} bytes)", data.len());
}
