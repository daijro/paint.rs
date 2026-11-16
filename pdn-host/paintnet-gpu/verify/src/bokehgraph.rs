//! Oracle for the GENUINE `BokehBlurEffect` inner convolution on opend2d's d2d executor (== native
//! WARP). Assembles the EXACT decompiled transform graph (ref/pdn-wine
//! PaintDotNet.Windows.Framework.dll, `PaintDotNet.Direct2D1.Effects.BokehBlurEffect.Impl`):
//!
//!   input ─ Border(Mirror, Float32)
//!            ├─ VerticalConvolutionShader[reals_j]   ┐
//!            └─ VerticalConvolutionShader[imag_j]    ├─ HorizontalConvolutionAndAccumulate
//!                                                     ┘   Partials[z_j,w_j]  ─┐
//!            … one triple per component j …                                   ├ Composite(Plus,F32)
//!                                                                             ┘  → output
//!
//! The two shaders are genuine's ps_5_0 (ComputeSharp, `D2DOutputBuffer=Float32`); every node is
//! pinned Float32 (genuine sets Precision=Float32 on border/composite and the shaders bake it), so
//! the ONLY 8bpc quantisation is the input (the GPU wrapper's gammaEffect at the device-context
//! default 8bpc → the border dequantises to int/255). Alpha is Premultiplied → genuine's
//! convertInput/OutputAlpha are Passthrough, i.e. omitted here.
//!
//! Feed the 8bpc PREMULTIPLIED-linear node bytes the port dumps at its inner-convolution boundary
//! (PDNGPU_BOKEH=<prefix> → <prefix>.in8.bgra), render the composite to an R32G32B32A32_FLOAT target
//! (premultiplied; final output keeps exact f32), and dump RGBA f32 (LE) for a bit-exact compare vs
//! the port's dumped inner output (<prefix>.out.f32).
//!
//! Usage: `pdngpu-bokehgraph <radius> <quality> <in.premul8.bgra> <out.f32> <W> <H>`.
use d2d::abi::{D2D1_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32B32A32_FLOAT};
use d2d::bitmap::Bitmap;
use d2d::effects::{EffectContext, EffectImpl, Input, Props, TransformGraph};
use d2d::{float, Factory};
use std::rc::Rc;
use windows_core::{GUID, HRESULT};

const SHADER_DIR: &str = "pdn-host/paintnet-gpu/.build/shaders";
const VERT_ID: u128 = 0x7eb0_0001_0000_4000_8000_0000_7eb0_0001u128;
const HORZ_ID: u128 = 0x7eb0_0002_0000_4000_8000_0000_7eb0_0002u128;
const FLOAT32: u32 = 5; // D2D1_BUFFER_PRECISION_32BPC_FLOAT
const CHAN_ONE: u32 = 1; // D2D1_CHANNEL_DEPTH_1
const EXTEND_CLAMP: u32 = 0;
const EXTEND_MIRROR: u32 = 2;
const COMPOSITE_PLUS: u32 = 9; // D2D1_COMPOSITE_MODE_PLUS

// ---- genuine kernel tables (BokehBlurEffect.KernelScales / KernelComponents) -------------------
const KERNEL_SCALES: [f64; 10] = [1.4, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2, 1.2];

/// (a, b, Z, W) per component, Q = 1..10 — verbatim from the decompiled table.
#[rustfmt::skip]
const KERNEL_COMPONENTS: [&[[f64; 4]]; 10] = [
    &[[0.862325, 1.624835, 0.767583, 1.862321]],
    &[[0.886528, 5.268909, 0.411259, -0.548794], [1.960518, 1.558213, 0.513282, 4.56111]],
    &[[2.17649, 5.043495, 1.621035, -2.105439], [1.019306, 9.027613, -0.28086, -0.162882], [2.81511, 1.597273, -0.366471, 10.300301]],
    &[[4.894547282763508, 0.929376567571601, 0.714407681087016, 84.51265987988856], [3.666703743886918, 4.336932586332215, 2.801952481408896, -17.838314975955342], [2.626272224663539, 8.135593719310219, -2.537157440802193, 0.589049137537433], [1.290766926824529, 12.34772134256371, 0.010000000000002, 0.227466315879772]],
    &[[4.716883447930945, 1.7358319178135824, -21.69215371604072, 72.39689418968186], [4.617433689271565, 5.182588501065085, 35.32177160227391, -21.0988424879328], [4.142932558052223, 8.360699330164213, -13.251867272261515, -3.711126526016351], [2.987295576935829, 11.904916752786518, 0.47666554573981107, 1.9589101025300553], [1.5300037016315535, 16.110869110006078, 0.1415257483220136, -0.009987947127719764]],
    &[[4.8250215031288795, 1.863061689623832, -36.782352280221346, 81.36352349123085], [4.770507936049109, 5.703686744609449, 53.4515131597662, 0.010211273746183322], [5.036597150326645, 9.380850085354819, -12.064235688807356, -32.14410659609454], [4.696499100343814, 12.165429883418184, -5.184640171161125, 12.868037804173614], [3.3515804060339978, 15.658860435220012, 1.589545666571149, -0.4191098554782176], [1.8025717988071053, 19.850286335894605, -0.011516257341898792, -0.09291397508712977]],
    &[[5.635755002716984, 2.0161846499938942, -127.67050821204298, 189.13366250400748], [6.2265180958586, 6.010948636588568, 255.34251414243556, 37.55094949608352], [6.189230711552051, 8.269383035533139, -132.2590521372958, -101.7059257653572], [4.972166727344845, 12.050001393751478, -0.1843113559893084, 27.06823846423038], [4.323578237784037, 16.00101043380645, 5.837168074459592, 0.3359847314948253], [3.6920668221834534, 19.726797144782385, 0.010115759114852045, -1.091291088554394], [2.2295702188720004, 23.527764286361837, -0.07655024461742256, 0.01001768577317681]],
    &[[6.6430131554059075, 2.33925731610851, -665.7557728544768, 445.83362839529286], [8.948432332999396, 5.775418437190626, 1130.5906034230607, 15.626805026300797], [6.513143649767612, 8.05507417830653, -419.50196449095665, -9.275778572724292], [6.245927989258722, 12.863350894308521, -100.85574814870866, 79.1599400003683], [6.713191682126933, 17.072272272191718, 36.65346659449611, 118.71908139892597], [7.071814347005397, 18.719212513078034, 21.63902100281763, -77.52385953960055], [4.932882961391405, 22.545463415981025, -1.9683109176118303, 3.0163201264848736], [3.456372395841802, 26.088356168016503, 0.19835893874241894, 0.08089803872063023]],
    &[[7.393797857697906, 2.4737002456790207, -1796.6881230069646, 631.9043430000561], [13.246479495224113, 6.216076882495199, 3005.0995149934884, 169.0878309991149], [7.303628653874887, 7.783952969919921, -1058.5279460078423, 459.6898389991668], [8.154742557454425, 13.430399706116823, -1720.108330007715, 810.6026949975844], [8.381657431347698, 14.90360902110027, 1568.5705749924186, 285.01830799719926], [6.866935986644192, 20.281841043506173, 90.55436499314388, -59.610040004419275], [9.585395987559902, 21.80265398520623, -93.26089100639886, -111.18596800373774], [5.4836869943565825, 25.89243600015612, 5.110650995956478, 0.009999997374460896], [5.413819000655994, 28.96548499880915, 0.2499879943861626, -0.8591239990799346]],
    &[[7.148185516924103, 2.4927349464132558, -1242.6335320030207, 435.1383849993189], [13.97011128857182, 3.895575022534836, 1392.3314399989267, 638.7307769993176], [6.8989770619005375, 7.476990508899397, -375.25432200238583, 485.81362499651027], [7.0295872483302295, 12.172942127710726, 50.24416900119528, 358.28113199585056], [7.602767849720905, 15.65397904113909, 251.22341800281518, -98.10971700247157], [6.624758031834333, 18.75368396970372, -70.80337099593586, -33.03615600114058], [5.841447009473684, 23.267768013476132, -13.118463995076524, 11.5558290020063], [6.836643992578203, 25.436927996489974, 9.560691003252611, 3.0842850032656344], [4.31348099995382, 28.96548400078732, -0.5601639992634236, 0.010000007078592587], [3.0536740000345017, 32.57196899995972, 0.009999996171068724, 0.03756300474029547]],
];

/// genuine `CreateComplex1DKernel` (f64): per sample n in [-ceil(r), ceil(r)] step 1/1024, linear
/// split onto floor/ceil(n+c), value exp(-a·u²)·(cos,sin)(b·u²), u = n·scale·(1/r).
fn complex_1d_kernel(radius: f64, kernel_size: usize, scale: f64, a: f64, b: f64) -> Vec<[f64; 2]> {
    let mut k = vec![[0.0f64; 2]; kernel_size];
    let c = radius.ceil();
    let inv_r = 1.0 / radius;
    let step = 1.0 / 1024.0;
    let mut n = -c;
    while n <= c {
        if !(n < -radius) && !(n > radius) {
            let f = n + c;
            let lo = f.floor() as usize;
            let hi = f.ceil() as usize;
            let w_lo = 1.0 - (f - f.floor());
            let w_hi = 1.0 - w_lo;
            let u = n * scale * inv_r;
            let u2 = u * u;
            let e = (-a * u2).exp();
            let v = [e * (b * u2).cos(), e * (b * u2).sin()];
            if w_lo > 0.0 {
                k[lo][0] += v[0] * w_lo;
                k[lo][1] += v[1] * w_lo;
            }
            if w_hi > 0.0 {
                k[hi][0] += v[0] * w_hi;
                k[hi][1] += v[1] * w_hi;
            }
        }
        n += step;
    }
    k
}

/// genuine `CreateKernelDataFloat32`: f64 kernels, normalised by 1/√T, cast to f32. Returns the
/// reals/imaginaries texels (f32) and the raw (z, w) per component, plus the kernel size.
fn kernel_data(radius: f64, components: usize) -> (Vec<(Vec<f32>, Vec<f32>)>, Vec<(f32, f32)>, usize) {
    let size = radius.ceil() as usize * 2 + 1;
    let params = KERNEL_COMPONENTS[components - 1];
    let scale = KERNEL_SCALES[components - 1];
    let mut kernels: Vec<Vec<[f64; 2]>> = params.iter().map(|p| complex_1d_kernel(radius, size, scale, p[0], p[1])).collect();
    // NormalizeKernels: T = Σ_i Σ_j Σ_k [Z_i(re_j·re_k − im_j·im_k) + W_i(re_j·im_k + im_j·re_k)].
    let mut t = 0.0f64;
    for (i, kern) in kernels.iter().enumerate() {
        let (z, w) = (params[i][2], params[i][3]);
        for j in kern {
            for k in kern {
                t += z * (j[0] * k[0] - j[1] * k[1]) + w * (j[0] * k[1] + j[1] * k[0]);
            }
        }
    }
    let s = 1.0 / t.sqrt();
    for kern in &mut kernels {
        for v in kern.iter_mut() {
            v[0] *= s;
            v[1] *= s;
        }
    }
    let texels = kernels.iter().map(|kern| (kern.iter().map(|v| v[0] as f32).collect(), kern.iter().map(|v| v[1] as f32).collect())).collect();
    let zw = params.iter().map(|p| (p[2] as f32, p[3] as f32)).collect();
    (texels, zw, size)
}

struct BokehGraph {
    vert: Vec<u8>,
    horz: Vec<u8>,
    size: usize,
    texels: Vec<(Vec<f32>, Vec<f32>)>,
    zw: Vec<(f32, f32)>,
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

impl EffectImpl for BokehGraph {
    fn initialize(&self, _p: &Props, ctx: &EffectContext, graph: &mut TransformGraph) -> Result<(), HRESULT> {
        ctx.load_pixel_shader(GUID::from_u128(VERT_ID), &self.vert)?;
        ctx.load_pixel_shader(GUID::from_u128(HORZ_ID), &self.horz)?;
        let q = self.texels.len();
        // kernelLength is read with integer ops (ishr/ige) → store its bit pattern as i32.
        let vert_cb: Vec<u8> = (self.size as i32).to_le_bytes().iter().chain(&[0u8; 12]).copied().collect();

        let border = graph.add_node(ctx.create_border_transform(EXTEND_MIRROR, EXTEND_MIRROR));
        graph.set_output_buffer(border, FLOAT32)?;
        graph.connect_to_effect_input(0, border, 0)?;

        let mut horiz_nodes = Vec::with_capacity(q);
        for j in 0..q {
            let (re, im) = &self.texels[j];
            let tex_re = ctx.create_resource_texture(1, &[self.size as u32], FLOAT32, CHAN_ONE, &[EXTEND_CLAMP], &f32s(re))?;
            let tex_im = ctx.create_resource_texture(1, &[self.size as u32], FLOAT32, CHAN_ONE, &[EXTEND_CLAMP], &f32s(im))?;
            let v_re = graph.add_node(ctx.pixel_shader_transform(GUID::from_u128(VERT_ID), vert_cb.clone(), 1, vec![(1, tex_re.clone())])?);
            let v_im = graph.add_node(ctx.pixel_shader_transform(GUID::from_u128(VERT_ID), vert_cb.clone(), 1, vec![(1, tex_im.clone())])?);
            graph.set_output_buffer(v_re, FLOAT32)?;
            graph.set_output_buffer(v_im, FLOAT32)?;
            let (z, w) = self.zw[j];
            let horz_cb: Vec<u8> = (self.size as i32).to_le_bytes().iter().chain(&z.to_le_bytes()).chain(&w.to_le_bytes()).chain(&[0u8; 4]).copied().collect();
            let horz = graph.add_node(ctx.pixel_shader_transform(GUID::from_u128(HORZ_ID), horz_cb, 2, vec![(2, tex_re), (3, tex_im)])?);
            graph.set_output_buffer(horz, FLOAT32)?;
            graph.connect_node(border, v_re, 0)?;
            graph.connect_node(border, v_im, 0)?;
            graph.connect_node(v_re, horz, 0)?;
            graph.connect_node(v_im, horz, 1)?;
            horiz_nodes.push(horz);
        }

        // CompositeEffect(Plus) over the components — chained pairwise (opend2d Blend is 2-input;
        // Plus is associative addition). q==1 wires the single horizontal straight to the output.
        let out = if q == 1 {
            horiz_nodes[0]
        } else {
            let mut acc = horiz_nodes[0];
            for &h in &horiz_nodes[1..] {
                let blend = graph.add_node(ctx.create_blend_transform(COMPOSITE_PLUS));
                graph.set_output_buffer(blend, FLOAT32)?;
                graph.connect_node(acc, blend, 0)?; // dst
                graph.connect_node(h, blend, 1)?; // src
                acc = blend;
            }
            acc
        };
        graph.set_output_node(out)
    }
}

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let radius: f64 = a[1].parse().expect("radius");
    let quality: usize = a[2].parse().expect("quality");
    let in_bgra = std::fs::read(&a[3]).expect("in.premul8.bgra");
    let out = &a[4];
    let (w, h): (u32, u32) = (a[5].parse().unwrap(), a[6].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");
    let q = quality.clamp(1, 10);

    let (texels, zw, size) = kernel_data(radius, q);
    let vert = std::fs::read(format!("{SHADER_DIR}/BokehBlurEffect_VerticalConvolutionShader.dxbc")).expect("vertical dxbc");
    let horz = std::fs::read(format!("{SHADER_DIR}/BokehBlurEffect_HorizontalConvolutionAndAccumulatePartialsShader.dxbc")).expect("horizontal dxbc");

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    // Genuine sets Float32 on every inner node (border/shaders/composite), so the ONLY 8bpc step is
    // the 8bpc bitmap input (realized at its own format precision). The executor models intermediate
    // precision via the device-context buffer precision (SetOutputBuffer is a no-op, like WARP), so
    // pin it to 32BPC_FLOAT: border→shader→composite stay f32; the bitmap input stays 8bpc.
    dc.set_buffer_precision(FLOAT32);
    dc.set_target_bitmap(float::float_target(w, h, 96.0, DXGI_FORMAT_R32G32B32A32_FLOAT, D2D1_ALPHA_MODE_PREMULTIPLIED));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    // The 8bpc premultiplied-linear node bytes, verbatim (border dequantises to int/255).
    let bm = Bitmap { width: w, height: h, premul_bgra: in_bgra, format: DXGI_FORMAT_B8G8R8A8_UNORM, alpha: D2D1_ALPHA_MODE_PREMULTIPLIED, dpi: (96.0, 96.0) };

    let id = GUID::from_u128(0x7eb0_9999_0000_4000_8000_0000_7eb0_9999u128);
    let (vc, hc, tx, zwc) = (vert.clone(), horz.clone(), texels.clone(), zw.clone());
    let xml = "<?xml version='1.0'?><Effect><Property name='DisplayName' type='string' value='PdnBokehGraph'/>\
<Property name='Author' type='string' value='opend2d'/><Property name='Category' type='string' value='Test'/>\
<Property name='Description' type='string' value='bokeh inner convolution'/><Inputs><Input name='Source0'/></Inputs></Effect>";
    factory
        .register_effect_from_string(id, xml, Rc::new(move || Box::new(BokehGraph { vert: vc.clone(), horz: hc.clone(), size, texels: tx.clone(), zw: zwc.clone() })))
        .expect("register BokehGraph effect");
    let eff = dc.create_effect(id).expect("create BokehGraph effect (shader rejected?)");
    eff.set_input(0, Input::Bitmap(Rc::new(bm)));
    dc.draw_image_effect(&eff, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    assert_eq!(data.len(), (w * h * 16) as usize, "float target size");
    std::fs::write(out, &data).expect("write out.f32");
    eprintln!("rendered genuine bokeh graph (r={radius}, q={q}, size={size}) -> {out} ({} bytes)", data.len());
}
