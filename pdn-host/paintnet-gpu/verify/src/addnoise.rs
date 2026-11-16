//! Render the GENUINE Paint.NET GPU AddNoise pixel shader through crates/d2d's custom-effect
//! RESOURCE-TEXTURE path (`create_resource_texture` + `pixel_shader_transform` with a bound texture,
//! not the bare `PixelShaderEffect`), and dump the 32x32 premultiplied BGRA8 so it can be byte-compared
//! with the port's `add_noise` (classic_oracle) and a WARP capture. AddNoise samples a 1-D 16384-entry
//! Float32 noise LUT at register 1; that texture is why it was `runnable:false` through the bare path.
//!
//! Usage: `pdngpu-addnoise <AddNoise.dxbc> <out.bgra>` (32x32 opaque gradient input, default params).
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

const N: usize = 16384;

/// Genuine `PdnAddNoiseEffect.CreateLookupTable` (== the port's `noise_lut`): bisect lambda in [5,10]
/// so the running NormalCurve sum hits N, then fill the cumulative histogram with (j-N/2)*65536/N;
/// int -> f32 LE bytes (the resource texture is `BufferPrecision.Float32` after an int->float convert).
fn noise_lut() -> Vec<u8> {
    let normal = |x: f64, s: f64| s * (-x * x / 2.0).exp();
    let xf = |i: usize| 16.0 * (i as f64 - (N / 2) as f64) / N as f64;
    let (mut lo, mut hi, mut mid) = (5.0f64, 10.0f64, 50.0f64);
    while hi - lo > 1e-7 {
        mid = (lo + hi) * 0.5;
        let mut sum = 0.0;
        for i in 0..N {
            sum += normal(xf(i), mid);
            if sum > 1e6 {
                break;
            }
        }
        if sum > N as f64 {
            hi = mid;
        } else if sum < N as f64 {
            lo = mid;
        } else {
            break;
        }
    }
    let mut lut = vec![0i32; N];
    let (mut sum, mut prev) = (0.0f64, 0usize);
    for j in 0..N {
        sum += normal(xf(j), mid);
        let cur = (sum as usize).min(N);
        let val = ((j as i64 - (N / 2) as i64) * 65536 / N as i64) as i32;
        for k in prev..cur {
            lut[k] = val;
        }
        prev = cur;
    }
    // Sanity vs the port's pinned noise_lut values (add_noise_lut_is_pinned).
    assert_eq!((lut[0], lut[8192], lut[16383]), (-15736, 0, 23200), "noise LUT mismatch");
    lut.iter().flat_map(|&v| (v as f32).to_le_bytes()).collect()
}

/// The 32x32 opaque gradient (same pixels as `main.rs`): opaque so the effect's default premultiplied
/// alpha round-trip (ConvertAlpha UnPremultiply -> shader -> Premultiply) is a no-op, and the bare
/// shader output == the port's straight-sRGB add_noise. RGBA for `from_straight_rgba`.
fn gradient() -> Vec<u8> {
    let mut v = Vec::with_capacity(N);
    for y in 0..32u32 {
        for x in 0..32u32 {
            v.extend_from_slice(&[(8 * x).min(255) as u8, (8 * y).min(255) as u8, (4 * (x + y)).min(255) as u8, 255]);
        }
    }
    v
}

/// AddNoise as a custom effect: load the shader, create its 1-D Float32 noise LUT (point filter via
/// the default 0, clamp extend), and wire a draw transform that binds the LUT at register 1.
struct AddNoise {
    id: GUID,
    dxbc: Vec<u8>,
    cbuffer: Vec<u8>,
    lut: Vec<u8>,
}
impl EffectImpl for AddNoise {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(self.id, &self.dxbc)?;
        // SKIP_RESTEX=1 binds no texture (diagnostic: the coverage gate runs before the LUT read, so
        // this isolates whether the input-passthrough is the RNG gate vs the resource-texture binding).
        let textures = if std::env::var("SKIP_RESTEX").is_ok() {
            Vec::new()
        } else {
            vec![(1, ctx.create_resource_texture(1, &[N as u32], 5 /*32BPC_FLOAT*/, 1 /*CHANNEL_1*/, &[0 /*CLAMP*/], &self.lut)?)]
        };
        let t = ctx.pixel_shader_transform(self.id, self.cbuffer.clone(), 1, textures)?;
        graph.set_single_transform_node(t)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let dxbc = std::fs::read(&args[1]).expect("read AddNoise.dxbc");
    let out = &args[2];

    // Default params: intensity 0.64 -> dev=(64*64)/4=1024; saturation 1.0 -> sat=1*4096=4096;
    // coverage 1.0; seed = Seed(0) ^ SeedInstance(0) = 0. cbuffer order: dev,sat,coverage,seed.
    // Optional args override (diagnostics): <dev> <sat> <coverage> <seed>.
    let dev: i32 = args.get(3).map_or(1024, |s| s.parse().unwrap());
    let sat: i32 = args.get(4).map_or(4096, |s| s.parse().unwrap());
    let coverage: f32 = args.get(5).map_or(1.0, |s| s.parse().unwrap());
    let seed: u32 = args.get(6).map_or(0, |s| s.parse().unwrap());
    let mut cbuffer = Vec::new();
    cbuffer.extend_from_slice(&dev.to_le_bytes());
    cbuffer.extend_from_slice(&sat.to_le_bytes());
    cbuffer.extend_from_slice(&coverage.to_le_bytes());
    cbuffer.extend_from_slice(&seed.to_le_bytes());
    let lut = noise_lut();

    let id = GUID::from_u128(0x7e3a9f10_2b4c_4d5e_8f60_0000_0000_00ad);
    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(32, 32, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let xml = "<?xml version='1.0'?><Effect>\
<Property name='DisplayName' type='string' value='PdnAddNoise'/>\
<Property name='Author' type='string' value='opend2d'/>\
<Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='pdngpu addnoise'/>\
<Inputs><Input name='Source0'/></Inputs></Effect>";
    let (d2, c2, l2) = (dxbc.clone(), cbuffer.clone(), lut.clone());
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(AddNoise { id, dxbc: d2.clone(), cbuffer: c2.clone(), lut: l2.clone() })))
        .expect("register AddNoise effect");
    let eff = dc.create_effect(id).expect("create AddNoise effect (shader rejected by executor?)");
    eff.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(32, 32, &gradient()))));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine AddNoise -> {out} ({} bytes)", data.len());
}
