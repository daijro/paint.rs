//! Oracle for the GENUINE `PdnSketchBlurEffect` graph (SketchBlurGpuEffect) on opend2d's d2d
//! executor (== native WARP). Genuine's `PdnSketchBlurEffect : CustomEffect` wraps a single
//! `MedianApproximationEffect` (ref/pdn-wine PaintDotNet.Framework.dll ->
//! PaintDotNet.Direct2D1.Effects.MedianApproximationEffect). Its decompiled graph (AlphaMode=Straight,
//! EdgeMode=Mirror, document sRGB, RandomSeed=0, RandomOffset=0, SampleCountPercent=0.01):
//!
//!   input(straight sRGB) -> ConvertAlpha(Passthrough)     [identity, AlphaMode=Straight]
//!     -> Border(Mirror)                                    [8bpc]
//!     -> for j in 0..N:  P2QuantileEstimator(offsets_j, p) [8bpc]  -- GENUINE ps_5_0 DXBC
//!                        -> ColorManagement(sRGB->scRGB, straight) [8bpc]
//!     -> WeightedAverage: Composite(Plus, Float32) then Divide by N
//!     -> ColorManagement(scRGB->sRGB, straight) [Float32]
//!     -> ConvertAlpha(Passthrough) -> output
//!
//! The whole graph is STRAIGHT alpha (both ConvertAlpha are Passthrough, ColorManagement keeps alpha),
//! so no premultiply ever happens: every node operates on straight sRGB [0,1]. The ONLY Float32 node
//! is WeightedAverage's Composite; everything else is the device default 8bpc.
//!
//! Executor realisation (matches native's per-effect intermediate buffers):
//!  - STAGE 1 (per iteration j): the GENUINE P2 ps_5_0 DXBC run as a custom graph `Border(Mirror) ->
//!    P2 shader(offsets_j)` -> rendered to an 8bpc bitmap. P2's complex input samples the 8bpc border
//!    with WARP's fixed-point bilinear (executor models it op-for-op); `store` quantises the P2 output
//!    to 8bpc (round-ties-even) -- exactly the P2 node's 8bpc output buffer.
//!  - STAGE 2 (per j): ColorManagement(sRGB->scRGB, straight) applied with the executor's OWN verified
//!    `ColorTransform` (native CColorManagement, 1024-entry 16-bit s_srgbLut; `cms-*` byte-exact),
//!    then quantised to 8bpc -- the CM node's 8bpc output buffer. Done in-harness because it is
//!    bit-identical to the executor's CM node (same code), and the executor can't wire a
//!    ColorManagement into a mid-graph node.
//!  - STAGE 3: Composite(Plus) over the N 8bpc linear bitmaps as a Blend(Plus) chain pinned Float32
//!    (bokeh proved Blend(Plus) chain == native Composite(Plus)); rendered to an F32 target -> the
//!    exact f32 composite-sum.
//!  - STAGE 4 (Float32, no 8bpc): avg = sum / (float)N; then ColorManagement(scRGB->sRGB, straight)
//!    via the same verified `ColorTransform` -- bit-identical to the GPU divide + CM nodes.
//!
//! Dumps straight RGBA f32 (LE) for a bit-exact compare vs the port's `classic_oracle sketchBlurEffect`
//! prequant f32 (PDNGPU_F32).
//!
//! Usage: `pdngpu-sketchblurgraph <radius> <percentile_int 0-100> <smoothness> <in.straight.bgra> <out.f32> <W> <H>`.
use d2d::abi::{D2D1_ALPHA_MODE_STRAIGHT, D2D1_COLOR_SPACE_SCRGB, D2D1_COLOR_SPACE_SRGB, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT};
use d2d::bitmap::Bitmap;
use d2d::color_context::{ColorContext, ColorTransform};
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{bitmap_target, float, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

const SHADER_DIR: &str = "pdn-host/paintnet-gpu/.build/shaders";
const P2_ID: u128 = 0x5e70_0001_0000_4000_8000_0000_5e70_0001u128;
const FLOAT32: u32 = 5;
const CHAN_FOUR: u32 = 4;
const EXTEND_CLAMP: u32 = 0;
const EXTEND_MIRROR: u32 = 2;
const COMPOSITE_PLUS: u32 = 9;

// .NET's seeded System.Random (Knuth subtractive); same impl as crates/pdn-core blur.rs NetRandom.
struct NetRandom {
    a: [i32; 56],
    inext: usize,
    inextp: usize,
}
impl NetRandom {
    fn new(seed: i32) -> Self {
        let mut a = [0i32; 56];
        let m = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = 161_803_398i32.wrapping_sub(m);
        a[55] = mj;
        let mut mk = 1i32;
        for i in 1..55 {
            let idx = 21 * i % 55;
            a[idx] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 {
                mk = mk.wrapping_add(i32::MAX);
            }
            mj = a[idx];
        }
        for _ in 0..4 {
            for i in 1..56 {
                let v = a[i].wrapping_sub(a[1 + (i + 30) % 55]);
                a[i] = if v < 0 { v.wrapping_add(i32::MAX) } else { v };
            }
        }
        Self { a, inext: 0, inextp: 21 }
    }
    fn next_double(&mut self) -> f64 {
        self.inext = self.inext % 55 + 1;
        self.inextp = self.inextp % 55 + 1;
        let mut v = self.a[self.inext].wrapping_sub(self.a[self.inextp]);
        if v == i32::MAX {
            v -= 1;
        }
        if v < 0 {
            v = v.wrapping_add(i32::MAX);
        }
        self.a[self.inext] = v;
        v as f64 * (1.0 / i32::MAX as f64)
    }
}

/// MedianApproximationEffect: M = max(5, round(PI*(num*num))); num*num is f32, *PI in f64, ties-even.
fn sample_pool(num: f32) -> i32 {
    let val = (std::f64::consts::PI * (num * num) as f64).round_ties_even() as i32;
    val.max(5)
}
/// K = clamp(round((float)M * 0.01f), 5, M), ties-even.
fn sample_count(m: i32, sample_count_percent: f32) -> i32 {
    let k = (m as f32 * sample_count_percent).round_ties_even() as i32;
    k.clamp(5, m)
}

/// genuine's offsets for iteration j: K entries, offsets[k] = (Vector2Float)((rho cos phi, rho sin
/// phi) * num), rho = sqrt(NextDouble), phi = NextDouble*2PI; the remaining M-K draws drain the RNG
/// (irrelevant: each iteration reseeds Random(RandomSeed + j), RandomSeed=0, RandomOffset=0).
fn offsets(num: f32, j: i32, m: i32, k: i32) -> Vec<[f32; 2]> {
    let mut rng = NetRandom::new(j);
    let mut out = vec![[0.0f32; 2]; k as usize];
    let r = num as f64;
    for kk in 0..m {
        let rho = rng.next_double().sqrt();
        let phi = rng.next_double() * (std::f64::consts::PI * 2.0);
        if kk < k {
            out[kk as usize] = [(rho * phi.cos() * r) as f32, (rho * phi.sin() * r) as f32];
        }
    }
    out
}

/// Border(Mirror) -> genuine P2 ps_5_0 shader(offsets_j, p). Renders one iteration's median estimate.
struct P2Stage {
    dxbc: Vec<u8>,
    k: u32,
    offs_bytes: Vec<u8>, // K * Float4 (x,y,0,0) LE
    cb: Vec<u8>,         // p, pDiv2, pPlus1Div2, pGTE0pt5, samplingXYWH(int4)
}
impl EffectImpl for P2Stage {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(GUID::from_u128(P2_ID), &self.dxbc)?;
        let tex = ctx.create_resource_texture(1, &[self.k], FLOAT32, CHAN_FOUR, &[EXTEND_CLAMP], &self.offs_bytes)?;
        let border = graph.add_node(ctx.create_border_transform(EXTEND_MIRROR, EXTEND_MIRROR));
        graph.connect_to_effect_input(0, border, 0)?;
        // P2 input 0 is the (complex) source; resource texture at register t1.
        let p2 = graph.add_node(ctx.pixel_shader_transform(GUID::from_u128(P2_ID), self.cb.clone(), 1, vec![(1, tex)])?);
        graph.connect_node(border, p2, 0)?;
        graph.set_output_node(p2)
    }
}

fn render_p2_8bpc(factory: &Factory, src: &Bitmap, dxbc: &[u8], offs: &[[f32; 2]], cb: &[u8], w: u32, h: u32, j: i32) -> Vec<u8> {
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(w, h, 96.0)); // 8bpc premul BGRA8 target (default precision)
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));
    let offs_bytes: Vec<u8> = offs.iter().flat_map(|o| [o[0], o[1], 0.0f32, 0.0f32]).flat_map(|f| f.to_le_bytes()).collect();
    // UNIQUE GUID per iteration: registration is GUID-keyed, so a shared one would reuse the first
    // iteration's captured offsets closure for every iteration.
    let id = GUID::from_u128(0x5e70_9001_0000_4000_8000_0000_0000_0000u128 + j as u128);
    let (d, ob, cbb, k) = (dxbc.to_vec(), offs_bytes.clone(), cb.to_vec(), offs.len() as u32);
    let xml = "<?xml version='1.0'?><Effect><Property name='DisplayName' type='string' value='PdnSketchP2'/>\
<Property name='Author' type='string' value='opend2d'/><Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='P2 median iteration'/><Inputs><Input name='Source0'/></Inputs></Effect>";
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(P2Stage { dxbc: d.clone(), k, offs_bytes: ob.clone(), cb: cbb.clone() })))
        .expect("register P2Stage");
    let eff = dc.create_effect(id).expect("create P2Stage (shader rejected?)");
    eff.set_input(0, Input::Bitmap(Rc::new(src.clone())));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    dc.take_target().unwrap().data // premul BGRA8 == straight bytes (no premul node)
}

/// Blend(Plus) chain over the N 8bpc linear bitmaps, pinned Float32 -> exact f32 composite-sum.
struct PlusChain {
    bitmaps: Vec<Rc<Bitmap>>,
}
impl EffectImpl for PlusChain {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        // Each bitmap enters via a Source node (bitmap bound directly through a border-less path is
        // not available; use effect input 0 for the first and extra inputs for the rest).
        // Simpler: bind all bitmaps as effect inputs and chain Blend(Plus).
        let n = self.bitmaps.len();
        if n == 1 {
            return graph.set_passthrough_graph(0);
        }
        // input i -> a bounds-free pass; chain Plus.
        let mut acc = {
            let b = graph.add_node(ctx.create_blend_transform(COMPOSITE_PLUS));
            graph.set_output_buffer(b, FLOAT32)?;
            graph.connect_to_effect_input(0, b, 0)?;
            graph.connect_to_effect_input(1, b, 1)?;
            b
        };
        for i in 2..n {
            let b = graph.add_node(ctx.create_blend_transform(COMPOSITE_PLUS));
            graph.set_output_buffer(b, FLOAT32)?;
            graph.connect_node(acc, b, 0)?;
            graph.connect_to_effect_input(i as u32, b, 1)?;
            acc = b;
        }
        graph.set_output_node(acc)
    }
}

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let radius: f32 = a[1].parse().expect("radius");
    let percentile_int: i32 = a[2].parse().expect("percentile 0-100");
    let smoothness: i32 = a[3].parse().expect("smoothness");
    let in_bgra = std::fs::read(&a[4]).expect("in.straight.bgra");
    let out = &a[5];
    let (w, h): (u32, u32) = (a[6].parse().unwrap(), a[7].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");

    let num = radius.clamp(0.0, 100.0);
    let p = (percentile_int as f32 / 100.0).clamp(0.0, 1.0);
    let n = smoothness.max(1);
    let sample_count_percent = 0.01f32;

    let factory = Factory::new(false);

    // Straight input: store BGRA bytes verbatim (alpha STRAIGHT), bitmap_texel yields straight c/255.
    let src = Bitmap { width: w, height: h, premul_bgra: in_bgra.clone(), format: DXGI_FORMAT_B8G8R8A8_UNORM, alpha: D2D1_ALPHA_MODE_STRAIGHT, dpi: (96.0, 96.0) };

    if num == 0.0 {
        // Passthrough: output = straight input as f32.
        let mut f = Vec::with_capacity((w * h * 4) as usize);
        for px in in_bgra.chunks_exact(4) {
            f.push(px[2] as f32 / 255.0);
            f.push(px[1] as f32 / 255.0);
            f.push(px[0] as f32 / 255.0);
            f.push(px[3] as f32 / 255.0);
        }
        std::fs::write(out, f.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
        eprintln!("sketch passthrough (radius 0) -> {out}");
        return;
    }

    let dxbc = std::fs::read(format!("{SHADER_DIR}/P2QuantileEstimatorEffect.dxbc")).expect("P2 dxbc (place genuine 58a072f4 under .build/shaders as P2QuantileEstimatorEffect.dxbc)");
    let m = sample_pool(num);
    let k = sample_count(m, sample_count_percent);

    // sRGB <-> scRGB (linear) straight-alpha transforms (native CColorManagement).
    let srgb = ColorContext::from_color_space(D2D1_COLOR_SPACE_SRGB);
    let scrgb = ColorContext::from_color_space(D2D1_COLOR_SPACE_SCRGB);
    let fwd = ColorTransform::new(&srgb, &scrgb, true).expect("sRGB->scRGB");
    let bwd = ColorTransform::new(&scrgb, &srgb, true).expect("scRGB->sRGB");

    // P2 cbuffer: p, pDiv2, pPlus1Div2, pGTE0pt5(bool as u32), samplingXYWH(int4).
    let make_cb = |offs: &[[f32; 2]]| -> Vec<u8> {
        let (mut minx, mut miny, mut maxx, mut maxy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for o in offs {
            minx = minx.min(o[0]);
            miny = miny.min(o[1]);
            maxx = maxx.max(o[0]);
            maxy = maxy.max(o[1]);
        }
        let (x, y) = (minx.floor() as i32, miny.floor() as i32);
        let (ww, hh) = (maxx.ceil() as i32 - x, maxy.ceil() as i32 - y);
        let mut cb = Vec::new();
        cb.extend(p.to_le_bytes());
        cb.extend((p / 2.0f32).to_le_bytes());
        cb.extend(((p + 1.0f32) / 2.0f32).to_le_bytes());
        cb.extend((if p >= 0.5 { 1u32 } else { 0u32 }).to_le_bytes());
        cb.extend((x).to_le_bytes());
        cb.extend((y).to_le_bytes());
        cb.extend((ww).to_le_bytes());
        cb.extend((hh).to_le_bytes());
        cb
    };

    // STAGE 1 + 2: per iteration, render P2 -> 8bpc, then CM(fwd) + quantise8 -> linear 8bpc bitmap.
    let q8 = |v: f32| (v * 255.0).round_ties_even().clamp(0.0, 255.0);
    let mut lin_bitmaps: Vec<Rc<Bitmap>> = Vec::with_capacity(n as usize);
    let dump_p2: Option<i32> = std::env::var("SKETCH_DUMP_P2J").ok().and_then(|s| s.parse().ok());
    for j in 0..n {
        let offs = offsets(num, j, m, k);
        let cb = make_cb(&offs);
        let p2 = render_p2_8bpc(&factory, &src, &dxbc, &offs, &cb, w, h, j); // premul(=straight) BGRA8
        if dump_p2 == Some(j) {
            let mut f = Vec::new();
            for px in p2.chunks_exact(4) {
                f.push(px[2] as f32 / 255.0);
                f.push(px[1] as f32 / 255.0);
                f.push(px[0] as f32 / 255.0);
                f.push(px[3] as f32 / 255.0);
            }
            std::fs::write(out, f.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
            eprintln!("dumped P2 iteration {j} (8bpc) -> {out}");
            return;
        }
        // CM sRGB->scRGB straight on each straight pixel, then 8bpc store.
        let mut lin = vec![0u8; (w * h * 4) as usize];
        for (i, px) in p2.chunks_exact(4).enumerate() {
            // premul_bgra stores B,G,R,A; straight values c/255.
            let s = [px[2] as f32 / 255.0, px[1] as f32 / 255.0, px[0] as f32 / 255.0, px[3] as f32 / 255.0];
            let t = fwd.apply(s); // [sample(FWD,r),sample(FWD,g),sample(FWD,b),a]
            lin[i * 4] = q8(t[2]) as u8; // B
            lin[i * 4 + 1] = q8(t[1]) as u8; // G
            lin[i * 4 + 2] = q8(t[0]) as u8; // R
            lin[i * 4 + 3] = q8(t[3]) as u8; // A
        }
        lin_bitmaps.push(Rc::new(Bitmap { width: w, height: h, premul_bgra: lin, format: DXGI_FORMAT_B8G8R8A8_UNORM, alpha: D2D1_ALPHA_MODE_STRAIGHT, dpi: (96.0, 96.0) }));
    }

    // STAGE 3: Composite(Plus) over the N linear 8bpc bitmaps, Float32 -> f32 sum.
    let sum: Vec<f32> = {
        let device = factory.create_device();
        let mut dc = device.create_device_context();
        dc.set_dpi(96.0, 96.0);
        dc.set_target_bitmap(float::float_target(w, h, 96.0, DXGI_FORMAT_R32G32B32A32_FLOAT, D2D1_ALPHA_MODE_STRAIGHT));
        dc.begin_draw();
        dc.clear(Some([0.0; 4]));
        let bitmaps = lin_bitmaps.clone();
        let nin = bitmaps.len() as u32;
        let id = GUID::from_u128(0x5e70_9003_0000_4000_8000_0000_5e70_9003u128);
        let inputs_xml: String = (0..nin).map(|i| format!("<Input name='Source{i}'/>")).collect();
        let xml = format!(
            "<?xml version='1.0'?><Effect><Property name='DisplayName' type='string' value='PdnSketchPlus'/>\
<Property name='Author' type='string' value='opend2d'/><Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='weighted average plus'/><Inputs>{inputs_xml}</Inputs></Effect>"
        );
        let bm2 = bitmaps.clone();
        factory.register_effect_from_string(id, &xml, Rc::new(move || Box::new(PlusChain { bitmaps: bm2.clone() }))).expect("register PlusChain");
        let eff = dc.create_effect(id).expect("create PlusChain");
        for (i, b) in bitmaps.iter().enumerate() {
            eff.set_input(i as u32, Input::Bitmap(b.clone()));
        }
        dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
        let _ = dc.end_draw();
        let data = dc.take_target().unwrap().data;
        data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
    };

    // STAGE 4 (Float32, no 8bpc): avg = sum / (float)N; then CM scRGB->sRGB straight.
    let nf = n as f32;
    let mut f = Vec::with_capacity((w * h * 4) as usize);
    for px in sum.chunks_exact(4) {
        // sum target is RGBA f32 (float_target RGBA order).
        let avg = [px[0] / nf, px[1] / nf, px[2] / nf, px[3] / nf];
        let o = bwd.apply(avg); // [sample(BWD,r),sample(BWD,g),sample(BWD,b),a]
        f.extend_from_slice(&o);
    }
    std::fs::write(out, f.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
    eprintln!("rendered genuine sketch graph (r={num}, p={p}, N={n}, M={m}, K={k}) -> {out}");
}
