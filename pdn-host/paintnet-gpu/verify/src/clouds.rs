//! Render the GENUINE Paint.NET GPU Clouds pixel shader through crates/d2d's custom-effect
//! RESOURCE-TEXTURE path and dump the 32x32 BGRA8 for byte-comparison with a WARP capture.
//! Clouds is a 0-input GENERATOR that reads a 512-entry UNorm8 doubled Ken-Perlin permutation
//! table at register t0 (`permuteLookupTexture[i] * 255`), unlike AddNoise's Float32 LUT at t1.
//!
//! Usage: `pdngpu-clouds <Clouds.dxbc> <out.bgra>` (32x32, scale=64 -> 7 octaves, black->white).
use d2d::effects::{EffectContext, EffectImpl, Props, TransformGraph};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

/// Ken Perlin's reference permutation (SHA-256 == PDN's RVA table `3682C3D0...`), doubled to 512 so
/// every index the noise algorithm forms (ix+seed, a+1, ... all <= 511) stays in range.
const PERM: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142,
    8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117,
    35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71,
    134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41,
    55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89,
    18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226,
    250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182,
    189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43,
    172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97,
    228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107,
    49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138,
    236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180,
];

fn perm512() -> Vec<u8> {
    PERM.iter().chain(PERM.iter()).copied().collect()
}

/// Clouds cbuffer: size@0 (float2), colorFrom@16 (float4), colorTo@32 (float4), scale@48, power@52,
/// seed@56 (int) -> 64 bytes (confirmed from the ComputeSharp ConstantBuffer FieldOffsets).
fn cbuffer(scale: f32, power: f32, seed: i32) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0..8].copy_from_slice(&[32.0f32.to_le_bytes(), 32.0f32.to_le_bytes()].concat()); // size
    b[16..32].copy_from_slice(&[0.0f32, 0.0, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()); // colorFrom
    b[32..48].copy_from_slice(&[1.0f32, 1.0, 1.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()); // colorTo
    b[48..52].copy_from_slice(&scale.to_le_bytes());
    b[52..56].copy_from_slice(&power.to_le_bytes());
    b[56..60].copy_from_slice(&seed.to_le_bytes());
    b
}

struct Clouds {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
    perm: Vec<u8>,
}
impl EffectImpl for Clouds {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        // 512-entry 1-D UNorm8 (8BPC_UNORM=1, CHANNEL_1=1), clamp extend, bound at register 0.
        let tex = ctx.create_resource_texture(1, &[512], 1, 1, &[0], &self.perm)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 0, vec![(0, tex)])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read Clouds.dxbc");
    let out = &args[2];
    // Optional diagnostics: <scale> <power> <seed>. scale=0 => div=0 => the octave loop never runs,
    // so the output is a constant lerp(colorFrom,colorTo,0.5) (isolates cbuffer/color path from noise).
    let scale: f32 = args.get(3).map_or(64.0, |s| s.parse().unwrap());
    let power: f32 = args.get(4).map_or(0.6, |s| s.parse().unwrap());
    let seed: i32 = args.get(5).map_or(0, |s| s.parse().unwrap());

    let id = GUID::from_u128(0xc10d5c10_0000_4000_8000_000000c10d00u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(32, 32, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnClouds'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu clouds'/>\
<Inputs></Inputs></Effect>";
    let (d2, c2, p2) = (dxbc.clone(), cbuffer(scale, power, seed), perm512());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(Clouds { id, dxbc: d2.clone(), cbuffer: c2.clone(), perm: p2.clone() })))
        .expect("register Clouds effect");
    let eff = dc.create_effect(id).expect("create Clouds effect (shader rejected by executor?)");
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine Clouds -> {out} ({} bytes)", data.len());
}
