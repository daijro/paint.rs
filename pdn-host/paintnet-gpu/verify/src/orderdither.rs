//! Render the GENUINE Paint.NET GPU OrderedDither pixel shader through crates/d2d with its thresholds
//! matrix bound: a 2-D single-channel Float32 resource texture at register t1 (NxN, N=matrixMask+1),
//! indexed by (floor(SCENE_POSITION) & matrixMask). The shader has no decompiled C#, so the matrix was
//! RE'd from the DXBC disasm:
//!   out.rgb = saturate(floor(in.rgb*levelsMinus1 + thresholds[pos&mask]*strength + 0.5) * invLevelsMinus1)
//! WARP vs executor agree for ANY valid thresholds matrix (both use the one we bind), so we bind a
//! standard 8x8 Bayer matrix / 64. It reads SCENE_POSITION (a varying, like AddNoise), not a float
//! input, so BGRA8 I/O + the res-tex path suffice.
//!
//! Usage: `pdngpu-orderdither <OrderedDither.dxbc> <out.bgra> [levelsMinus1] [strength]`.
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

/// Standard recursive 8x8 Bayer matrix (values 0..63). thresholds texel (x,y) = BAYER8[y][x] / 64.
fn bayer8() -> Vec<f32> {
    // Recursive doubling from the 2x2 base [[0,2],[3,1]].
    let base = [[0u32, 2], [3, 1]];
    let mut m: Vec<Vec<u32>> = vec![vec![0, 2], vec![3, 1]];
    while m.len() < 8 {
        let n = m.len();
        let mut out = vec![vec![0u32; n * 2]; n * 2];
        for y in 0..n * 2 {
            for x in 0..n * 2 {
                out[y][x] = 4 * m[y % n][x % n] + base[y / n][x / n];
            }
        }
        m = out;
    }
    // row-major x-fastest: data[y*8 + x]
    let mut data = Vec::with_capacity(64);
    for y in 0..8 {
        for x in 0..8 {
            data.push(m[y][x] as f32 / 64.0);
        }
    }
    data
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

struct OrderDither {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
    thresholds: Vec<u8>,
}
impl EffectImpl for OrderDither {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        // 2-D 8x8 Float32 single-channel (5/CHANNEL_1), clamp both axes, point, bound at register 1.
        let tex = ctx.create_resource_texture(2, &[8, 8], 5, 1, &[0, 0], &self.thresholds)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, vec![(1, tex)])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read OrderedDither.dxbc");
    let out = &args[2];
    let levels_minus1: f32 = args.get(3).map_or(3.0, |s| s.parse().unwrap());
    let strength: f32 = args.get(4).map_or(1.0, |s| s.parse().unwrap());

    // cbuffer: matrixMask(int)@0, strength@4, levelsMinus1@8, invLevelsMinus1@12 -> 16 bytes.
    let mut cbuffer = vec![0u8; 16];
    cbuffer[0..4].copy_from_slice(&7i32.to_le_bytes()); // 8x8 matrix -> mask 7
    cbuffer[4..8].copy_from_slice(&strength.to_le_bytes());
    cbuffer[8..12].copy_from_slice(&levels_minus1.to_le_bytes());
    cbuffer[12..16].copy_from_slice(&(1.0 / levels_minus1).to_le_bytes());
    let thresholds: Vec<u8> = bayer8().iter().flat_map(|v| v.to_le_bytes()).collect();

    let id = GUID::from_u128(0x0d17_0d17_0000_4000_8000_00000d170d00u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(32, 32, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnOrderedDither'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu orderdither'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2, t2) = (dxbc.clone(), cbuffer.clone(), thresholds.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(OrderDither { id, dxbc: d2.clone(), cbuffer: c2.clone(), thresholds: t2.clone() })))
        .expect("register OrderedDither effect");
    let eff = dc.create_effect(id).expect("create OrderedDither effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(32, 32, &gradient()))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine OrderedDither L={levels_minus1} -> {out} ({} bytes)", data.len());
}
