//! Render the GENUINE Paint.NET GPU Crystalize SampleMapShader through crates/d2d and dump the 32x32
//! R32G32B32A32_FLOAT output for a WARP compare. It's a coordinate generator (voronoi): reads a float
//! scene-position input (D2DGetInput(0).xy), two UNorm8 permute tables (p0@t1, p1@t2), and outputs
//! float4(cellSampleX, cellSampleY, 0, 1). It needs a FLOAT context + FLOAT target + a FLOAT input
//! bitmap — an 8bpc scene-position input would clamp the coordinates to [0,1].
//!
//! Usage: `pdngpu-crystalize <Crystalize.dxbc> <out.f32> [cellSize] [seed]`.
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, float, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

// Ken-Perlin-style permute tables from PdnCrystalizeSampleMap (byte[256] each); UNorm8 LUTs the shader
// reads via GetP0/GetP1 = (uint)(255 * pN[index]).
const P0: [u8; 256] = [
    91, 213, 94, 40, 203, 67, 124, 200, 62, 51, 223, 53, 208, 111, 31, 28, 137, 108, 37, 114, 74, 46,
    177, 32, 71, 117, 187, 130, 191, 81, 45, 120, 178, 220, 241, 88, 216, 100, 82, 15, 230, 212, 87,
    153, 243, 141, 72, 199, 174, 242, 197, 251, 103, 176, 33, 42, 34, 60, 229, 96, 142, 255, 9, 239,
    5, 112, 139, 247, 131, 201, 70, 35, 22, 17, 146, 122, 217, 21, 50, 185, 39, 238, 202, 204, 140,
    55, 8, 250, 173, 189, 52, 155, 127, 240, 136, 226, 193, 101, 147, 133, 1, 83, 76, 227, 165, 160,
    222, 80, 38, 171, 86, 10, 12, 44, 59, 157, 56, 68, 110, 211, 102, 148, 73, 188, 159, 179, 169, 30,
    145, 168, 175, 118, 143, 172, 190, 218, 164, 234, 27, 233, 84, 184, 214, 0, 106, 26, 135, 85, 192,
    244, 170, 128, 252, 183, 29, 113, 7, 132, 69, 248, 246, 181, 196, 119, 3, 224, 121, 92, 104, 90,
    237, 64, 19, 126, 123, 161, 182, 58, 75, 109, 24, 79, 99, 158, 249, 25, 125, 209, 138, 57, 206,
    18, 156, 43, 236, 152, 245, 77, 254, 144, 162, 36, 221, 89, 253, 95, 98, 115, 48, 129, 195, 205,
    47, 210, 207, 151, 61, 16, 134, 219, 78, 167, 225, 14, 150, 198, 163, 231, 105, 228, 154, 54, 23,
    180, 194, 235, 65, 2, 107, 93, 149, 4, 97, 6, 66, 232, 186, 166, 20, 41, 13, 116, 215, 11, 49, 63,
];
const P1: [u8; 256] = [
    87, 48, 248, 240, 154, 176, 128, 190, 0, 110, 229, 55, 74, 226, 234, 159, 202, 35, 36, 251, 183,
    45, 27, 42, 187, 100, 186, 23, 69, 250, 80, 231, 117, 57, 143, 125, 243, 174, 215, 97, 11, 66, 71,
    121, 15, 138, 109, 92, 9, 133, 172, 33, 141, 178, 46, 41, 19, 207, 56, 60, 242, 107, 126, 5, 32,
    253, 196, 20, 89, 115, 171, 26, 81, 198, 237, 124, 228, 165, 25, 58, 246, 102, 13, 194, 84, 50,
    103, 199, 10, 112, 114, 44, 136, 167, 120, 166, 204, 70, 93, 18, 203, 95, 6, 161, 217, 152, 218,
    205, 193, 181, 68, 118, 182, 1, 72, 150, 96, 189, 188, 244, 157, 255, 8, 233, 4, 137, 156, 164,
    85, 78, 177, 169, 79, 7, 116, 160, 91, 122, 232, 195, 83, 151, 3, 90, 43, 249, 127, 134, 105, 221,
    73, 22, 145, 104, 131, 210, 77, 34, 162, 86, 245, 52, 130, 132, 12, 163, 38, 191, 155, 175, 61,
    235, 214, 252, 170, 14, 247, 192, 208, 17, 29, 40, 65, 135, 2, 173, 49, 230, 148, 180, 88, 64, 222,
    153, 211, 39, 51, 146, 98, 149, 223, 209, 236, 206, 158, 99, 63, 140, 212, 111, 216, 147, 82, 139,
    59, 224, 239, 254, 106, 94, 113, 47, 30, 227, 75, 241, 24, 54, 62, 142, 219, 119, 108, 37, 53, 101,
    67, 179, 220, 185, 129, 184, 28, 21, 225, 200, 213, 76, 123, 238, 31, 16, 144, 201, 168, 197,
];

/// Scene-position image: pixel (x,y) = (x+0.5, y+0.5, 0, 1) — the pixel centres a SampleMap shader
/// reads as D2DGetScenePosition, bound as a FLOAT input so the coordinates survive.
fn scene_positions() -> Vec<f32> {
    let mut v = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[x as f32 + 0.5, y as f32 + 0.5, 0.0, 1.0]);
        }
    }
    v
}

struct Crystalize {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
}
impl EffectImpl for Crystalize {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        // Two 1-D UNorm8 (precision 1 / CHANNEL_1) permute tables, clamp, bound at registers 1 and 2.
        let p0 = ctx.create_resource_texture(1, &[256], 1, 1, &[0], &P0)?;
        let p1 = ctx.create_resource_texture(1, &[256], 1, 1, &[0], &P1)?;
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, vec![(1, p0), (2, p1)])?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read Crystalize.dxbc");
    let out = &args[2];
    let cell_size: f32 = args.get(3).map_or(2.0, |s| s.parse().unwrap());
    let seed: u32 = args.get(4).map_or(0, |s| s.parse().unwrap());

    // cbuffer: seed@0 (uint), cellSize@4, cellSizeR@8 -> 12 bytes.
    let mut cbuffer = vec![0u8; 12];
    cbuffer[0..4].copy_from_slice(&seed.to_le_bytes());
    cbuffer[4..8].copy_from_slice(&cell_size.to_le_bytes());
    cbuffer[8..12].copy_from_slice(&(1.0 / cell_size).to_le_bytes());

    let id = GUID::from_u128(0xc2a1_c2a1_0000_4000_8000_0000c2a10c00u128);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    // Float32 context + Float32 target: the coordinate intermediate/output must keep full precision.
    dc.set_buffer_precision(5);
    let _ = bitmap_target; // (8bpc target helper unused here)
    dc.set_target_bitmap(float::float_target(32, 32, 96.0, d2d::abi::DXGI_FORMAT(2), d2d::abi::D2D1_ALPHA_MODE(1)));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnCrystalize'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu crystalize'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2) = (dxbc.clone(), cbuffer.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(Crystalize { id, dxbc: d2.clone(), cbuffer: c2.clone() })))
        .expect("register Crystalize effect");
    let eff = dc.create_effect(id).expect("create Crystalize effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_f32_rgba(32, 32, &scene_positions()))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine Crystalize cellSize={cell_size} -> {out} ({} bytes)", data.len());
}
