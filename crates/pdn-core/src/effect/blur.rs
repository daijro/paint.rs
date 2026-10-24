//! The 10 Blurs on Paint.NET's math.
//! Port of: js/src/engine/document/effect/BitmapEffectEngine.js (`EFFECTS` blurring entries: ids,
//! icons and control keys only).
//! Includes the Bokeh kernel components, .NET's `Random` and the P² estimator.

use super::sample::{
    circle_offsets, circle_rows, coverage, gamma_boost, gamma_unboost, gaussian_kernel, splat,
};
use super::surface::{Alpha, Edge, EffectSurface, Space, premultiply, unpremultiply};
use super::{
    Category, Control, EffectDef, EffectEnv, EffectKind, GPU_QUALITY, GpuFn, RenderInfo, Values,
    choice, flag, num, range,
};
use crate::pixel::gamma::{linear_to_srgb_f32, srgb_to_linear_f32};
use crate::render::display::for_bands;
use std::f64::consts::{PI, TAU};

const LINEAR: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Straight);
const LINEAR_PM: RenderInfo = RenderInfo::new(Space::Linear, Alpha::Premultiplied);
const SRGB: RenderInfo = RenderInfo::new(Space::Srgb, Alpha::Straight);

fn blur(
    id: &'static str,
    icon: &'static str,
    controls: Vec<Control>,
    info: RenderInfo,
    render: GpuFn,
) -> EffectDef {
    EffectDef {
        id,
        icon,
        category: Category::Effect("blurring"),
        controls,
        kind: EffectKind::Gpu(info, render),
        dialog: None,
        dialog_title: None,
        indirect: None,
    }
}

/// The Blurs submenu (§2.0 rows): paintdotjs ids, icons and keys; Paint.NET ranges (R19).
pub fn effects() -> Vec<EffectDef> {
    let gamma = || range("gamma", "Gamma boost", -0.99, 2.0, 0.0).step(0.01);
    let quality = |max: f64, default: f64| range("quality", "Quality", 1.0, max, default);
    let center =
        |key: &'static str, label: &'static str| range(key, label, -2.0, 2.0, 0.0).step(0.01);
    vec![
        blur(
            "blurEffect",
            "blur_effect.png",
            vec![
                range("radius", "Radius", 0.0, 300.0, 2.0),
                gamma(),
                quality(4.0, 4.0),
            ],
            LINEAR,
            gaussian_effect,
        ),
        blur(
            "bokehEffect",
            "bokeh_effect_icon.png",
            vec![
                range("radius", "Radius", 0.0, 300.0, 25.0),
                gamma(),
                quality(10.0, 3.0),
            ],
            LINEAR,
            bokeh_effect,
        ),
        blur(
            "fragmentEffect",
            "fragment_effect_icon.png",
            vec![
                range("fragments", "Fragment count", 2.0, 200.0, 4.0),
                range("distance", "Distance", 0.0, 400.0, 8.0),
                range("rotation", "Rotation", 0.0, 360.0, 0.0),
            ],
            LINEAR_PM,
            |s, v, _| {
                let o = fragment_offsets(
                    num(v, "fragments") as u32,
                    num(v, "distance"),
                    num(v, "rotation"),
                );
                coverage_normalised(s, |p| o.iter().map(move |o| [p[0] + o[0], p[1] + o[1]]))
            },
        ),
        blur(
            "medianBlurEffect",
            "median_effect_icon.png",
            vec![
                range("radius", "Radius", 0.0, 100.0, 10.0),
                range("percentile", "Percentile", 0.0, 100.0, 50.0),
                quality(9.0, 8.0),
            ],
            SRGB,
            |s, v, _| {
                let r = num(v, "radius").ceil();
                if r <= 0.0 {
                    return s.clone();
                }
                let p = num(v, "percentile") / 100.0;
                percentile_filter(s, r as u32, p, num(v, "quality") as u32, Edge::Clamp)
            },
        ),
        blur(
            "motionBlurEffect",
            "motion_blur_effect.png",
            vec![
                range("angle", "Angle", -180.0, 180.0, 25.0),
                range("distance", "Distance", 1.0, 500.0, 10.0),
                Control::Checkbox {
                    key: "centered",
                    label: "Centered",
                    default: true,
                },
                Control::Select {
                    key: "edge",
                    label: "Edge behavior",
                    default: "clamp",
                    choices: &[
                        ("clamp", "Clamp"),
                        ("mirror", "Mirror"),
                        ("wrap", "Wrap"),
                        ("transparent", "Transparent"),
                    ],
                },
            ],
            LINEAR_PM,
            motion_effect,
        ),
        blur(
            "radialBlurEffect",
            "radial_blur_effect.png",
            vec![
                range("angle", "Angle", 0.0, 360.0, 4.0),
                center("centerX", "Center X"),
                center("centerY", "Center Y"),
                quality(8.0, GPU_QUALITY),
            ],
            LINEAR_PM,
            radial_effect,
        ),
        blur(
            "sketchBlurEffect",
            "sketch_blur_effect_icon.png",
            vec![
                range("radius", "Radius", 0.0, 100.0, 25.0),
                range("percentile", "Percentile", 0.0, 100.0, 50.0),
                range("smoothness", "Smoothness", 1.0, 20.0, 3.0),
            ],
            SRGB,
            sketch_effect,
        ),
        blur(
            "squareBlurEffect",
            "square_blur_effect.png",
            vec![range("radius", "Radius", 0.0, 300.0, 6.0), gamma()],
            LINEAR,
            |s, v, _| {
                let r = num(v, "radius") as f32;
                if r <= 0.0 {
                    return s.clone();
                }
                boosted(s, num(v, "gamma"), |s| {
                    s.convolve_hv(&square_kernel(r), Edge::Mirror)
                })
            },
        ),
        blur(
            "surfaceBlurEffect",
            "surface_blur_effect_icon.png",
            vec![
                range("radius", "Radius", 1.0, 50.0, 6.0),
                range("threshold", "Threshold", 1.0, 100.0, 15.0),
            ],
            SRGB,
            surface_effect,
        ),
        blur(
            "zoomBlurEffect",
            "zoom_blur_effect.png",
            vec![
                range("distance", "Zoom", 0.25, 4.0, 1.25).step(0.01),
                range("focus", "Focus", 1.0, 4.0, 2.0).step(0.01),
                center("centerX", "Center X"),
                center("centerY", "Center Y"),
                quality(8.0, GPU_QUALITY),
            ],
            LINEAR_PM,
            zoom_effect,
        ),
    ]
}

/// acc += w·c per channel, in f32.
pub(super) fn madd(acc: &mut [f32; 4], w: f32, c: [f32; 4]) {
    for (a, c) in acc.iter_mut().zip(c) {
        *a += w * c;
    }
}

fn map_px(src: &EffectSurface, f: impl Fn([f32; 4]) -> [f32; 4] + Sync) -> EffectSurface {
    EffectSurface::from_fn(src.w, src.h, |x, y| f(src.at(x, y)))
}

/// F9 forward (γ = 1 + boost), premultiply, `blur`, unpremultiply, F9 inverse (§2.3 Gaussian
/// steps 1 and 6; Square and Bokeh the same).
fn boosted(
    src: &EffectSurface,
    boost: f64,
    blur: impl FnOnce(&EffectSurface) -> EffectSurface,
) -> EffectSurface {
    let g = (1.0 + boost) as f32;
    let pre = map_px(src, |p| premultiply(gamma_boost(p, g)));
    map_px(&blur(&pre), |p| gamma_unboost(unpremultiply(p), g))
}

/// §2.3 Gaussian steps 4–5: the HighQuality kernel, horizontal then vertical, on whatever `src`
/// holds (premultiplied for every caller). σ ≤ 0 returns the input.
pub fn gaussian(src: &EffectSurface, sigma: f64, edge: Edge) -> EffectSurface {
    if sigma <= 0.0 {
        return src.clone();
    }
    src.convolve_hv(&gaussian_kernel(sigma), edge)
}

/// §2.3 Gaussian: σ = min(radius/3, 100), Mirror (BorderMode Hard), in linear light.
fn gaussian_effect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let sigma = (num(v, "radius") / 3.0).min(100.0);
    if sigma <= 0.0 {
        return src.clone();
    }
    // approximation: D2D GaussianBlur for quality 1–3 (B-E2)
    boosted(src, num(v, "gamma"), |s| gaussian(s, sigma, Edge::Mirror))
}

/// §2.3 Square: w(i) = 1 − (|i| − min(|i|, r)) for i = −⌈r⌉..=⌈r⌉, each × f32(1/(1 + 2r)).
pub fn square_kernel(r: f32) -> Vec<f32> {
    let n = r.ceil() as i32;
    let k = (1.0 / (1.0 + 2.0 * r as f64)) as f32;
    (-n..=n)
        .map(|i| {
            let a = i.unsigned_abs() as f32;
            (1.0 - (a - a.min(r))) * k
        })
        .collect()
}

/// Appendix B: (a, b, Z, W) per component, Q = 1..10, verbatim.
const BOKEH_COMPONENTS: [&[[f64; 4]]; 10] = [
    &[[0.862325, 1.624835, 0.767583, 1.862321]],
    &[
        [0.886528, 5.268909, 0.411259, -0.548794],
        [1.960518, 1.558213, 0.513282, 4.56111],
    ],
    &[
        [2.17649, 5.043495, 1.621035, -2.105439],
        [1.019306, 9.027613, -0.28086, -0.162882],
        [2.81511, 1.597273, -0.366471, 10.300301],
    ],
    &[
        [
            4.894547282763508,
            0.929376567571601,
            0.714407681087016,
            84.51265987988856,
        ],
        [
            3.666703743886918,
            4.336932586332215,
            2.801952481408896,
            -17.838314975955342,
        ],
        [
            2.626272224663539,
            8.135593719310219,
            -2.537157440802193,
            0.589049137537433,
        ],
        [
            1.290766926824529,
            12.34772134256371,
            0.010000000000002,
            0.227466315879772,
        ],
    ],
    &[
        [
            4.716883447930945,
            1.7358319178135824,
            -21.69215371604072,
            72.39689418968186,
        ],
        [
            4.617433689271565,
            5.182588501065085,
            35.32177160227391,
            -21.0988424879328,
        ],
        [
            4.142932558052223,
            8.360699330164213,
            -13.251867272261515,
            -3.711126526016351,
        ],
        [
            2.987295576935829,
            11.904916752786518,
            0.47666554573981107,
            1.9589101025300553,
        ],
        [
            1.5300037016315535,
            16.110869110006078,
            0.1415257483220136,
            -0.009987947127719764,
        ],
    ],
    &[
        [
            4.8250215031288795,
            1.863061689623832,
            -36.782352280221346,
            81.36352349123085,
        ],
        [
            4.770507936049109,
            5.703686744609449,
            53.4515131597662,
            0.010211273746183322,
        ],
        [
            5.036597150326645,
            9.380850085354819,
            -12.064235688807356,
            -32.14410659609454,
        ],
        [
            4.696499100343814,
            12.165429883418184,
            -5.184640171161125,
            12.868037804173614,
        ],
        [
            3.3515804060339978,
            15.658860435220012,
            1.589545666571149,
            -0.4191098554782176,
        ],
        [
            1.8025717988071053,
            19.850286335894605,
            -0.011516257341898792,
            -0.09291397508712977,
        ],
    ],
    &[
        [
            5.635755002716984,
            2.0161846499938942,
            -127.67050821204298,
            189.13366250400748,
        ],
        [
            6.2265180958586,
            6.010948636588568,
            255.34251414243556,
            37.55094949608352,
        ],
        [
            6.189230711552051,
            8.269383035533139,
            -132.2590521372958,
            -101.7059257653572,
        ],
        [
            4.972166727344845,
            12.050001393751478,
            -0.1843113559893084,
            27.06823846423038,
        ],
        [
            4.323578237784037,
            16.00101043380645,
            5.837168074459592,
            0.3359847314948253,
        ],
        [
            3.6920668221834534,
            19.726797144782385,
            0.010115759114852045,
            -1.091291088554394,
        ],
        [
            2.2295702188720004,
            23.527764286361837,
            -0.07655024461742256,
            0.01001768577317681,
        ],
    ],
    &[
        [
            6.6430131554059075,
            2.33925731610851,
            -665.7557728544768,
            445.83362839529286,
        ],
        [
            8.948432332999396,
            5.775418437190626,
            1130.5906034230607,
            15.626805026300797,
        ],
        [
            6.513143649767612,
            8.05507417830653,
            -419.50196449095665,
            -9.275778572724292,
        ],
        [
            6.245927989258722,
            12.863350894308521,
            -100.85574814870866,
            79.1599400003683,
        ],
        [
            6.713191682126933,
            17.072272272191718,
            36.65346659449611,
            118.71908139892597,
        ],
        [
            7.071814347005397,
            18.719212513078034,
            21.63902100281763,
            -77.52385953960055,
        ],
        [
            4.932882961391405,
            22.545463415981025,
            -1.9683109176118303,
            3.0163201264848736,
        ],
        [
            3.456372395841802,
            26.088356168016503,
            0.19835893874241894,
            0.08089803872063023,
        ],
    ],
    &[
        [
            7.393797857697906,
            2.4737002456790207,
            -1796.6881230069646,
            631.9043430000561,
        ],
        [
            13.246479495224113,
            6.216076882495199,
            3005.0995149934884,
            169.0878309991149,
        ],
        [
            7.303628653874887,
            7.783952969919921,
            -1058.5279460078423,
            459.6898389991668,
        ],
        [
            8.154742557454425,
            13.430399706116823,
            -1720.108330007715,
            810.6026949975844,
        ],
        [
            8.381657431347698,
            14.90360902110027,
            1568.5705749924186,
            285.01830799719926,
        ],
        [
            6.866935986644192,
            20.281841043506173,
            90.55436499314388,
            -59.610040004419275,
        ],
        [
            9.585395987559902,
            21.80265398520623,
            -93.26089100639886,
            -111.18596800373774,
        ],
        [
            5.4836869943565825,
            25.89243600015612,
            5.110650995956478,
            0.009999997374460896,
        ],
        [
            5.413819000655994,
            28.96548499880915,
            0.2499879943861626,
            -0.8591239990799346,
        ],
    ],
    &[
        [
            7.148185516924103,
            2.4927349464132558,
            -1242.6335320030207,
            435.1383849993189,
        ],
        [
            13.97011128857182,
            3.895575022534836,
            1392.3314399989267,
            638.7307769993176,
        ],
        [
            6.8989770619005375,
            7.476990508899397,
            -375.25432200238583,
            485.81362499651027,
        ],
        [
            7.0295872483302295,
            12.172942127710726,
            50.24416900119528,
            358.28113199585056,
        ],
        [
            7.602767849720905,
            15.65397904113909,
            251.22341800281518,
            -98.10971700247157,
        ],
        [
            6.624758031834333,
            18.75368396970372,
            -70.80337099593586,
            -33.03615600114058,
        ],
        [
            5.841447009473684,
            23.267768013476132,
            -13.118463995076524,
            11.5558290020063,
        ],
        [
            6.836643992578203,
            25.436927996489974,
            9.560691003252611,
            3.0842850032656344,
        ],
        [
            4.31348099995382,
            28.96548400078732,
            -0.5601639992634236,
            0.010000007078592587,
        ],
        [
            3.0536740000345017,
            32.57196899995972,
            0.009999996171068724,
            0.03756300474029547,
        ],
    ],
];

/// §2.3 Bokeh: the kernel length L = 2⌈r⌉ + 1.
pub fn bokeh_len(radius: f64) -> usize {
    2 * radius.ceil() as usize + 1
}

#[derive(Debug)]
struct BokehKernel {
    re: Vec<f32>,
    im: Vec<f32>,
    z: f32,
    w: f32,
}

/// §2.3 Bokeh steps 1–2: per component of quality `q` the complex 1-D kernel in f64 (n step
/// 1/1024, |n| ≤ r, linear split onto ⌊f⌋ and ⌈f⌉), then every kernel × 1/√T with
/// T = Σ_components Σⱼ Σₖ Z·Re(KⱼKₖ) + W·Im(KⱼKₖ), cast to f32.
fn bokeh_kernels(r: f64, q: usize) -> Vec<BokehKernel> {
    let c = r.ceil();
    let scale = if q == 1 { 1.4 } else { 1.2 };
    let parts: Vec<(Vec<[f64; 2]>, f64, f64)> = BOKEH_COMPONENTS[q - 1]
        .iter()
        .map(|&[a, b, z, w]| {
            let mut k = vec![[0.0; 2]; bokeh_len(r)];
            for i in 0..=2048 * c as usize {
                let n = -c + i as f64 / 1024.0;
                if n.abs() > r {
                    continue;
                }
                let u = n * scale / r;
                let e = (-a * (u * u)).exp();
                let v = [e * (b * (u * u)).cos(), e * (b * (u * u)).sin()];
                let f = n + c;
                let lo = f.floor();
                for (at, wt) in [(lo, 1.0 - (f - lo)), (lo + 1.0, f - lo)] {
                    if wt > 0.0 {
                        let t = &mut k[at as usize];
                        t[0] += v[0] * wt;
                        t[1] += v[1] * wt;
                    }
                }
            }
            (k, z, w)
        })
        .collect();
    let mut t = 0.0;
    for (k, z, w) in &parts {
        for a in k {
            for b in k {
                t += z * (a[0] * b[0] - a[1] * b[1]) + w * (a[0] * b[1] + a[1] * b[0]);
            }
        }
    }
    let s = 1.0 / t.sqrt();
    parts
        .into_iter()
        .map(|(k, z, w)| BokehKernel {
            re: k.iter().map(|v| (v[0] * s) as f32).collect(),
            im: k.iter().map(|v| (v[1] * s) as f32).collect(),
            z: z as f32,
            w: w as f32,
        })
        .collect()
}

fn bokeh_effect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    bokeh(
        src,
        num(v, "radius"),
        num(v, "quality") as usize,
        num(v, "gamma"),
    )
}

/// §2.3 Bokeh steps 3–4 with F9 on straight `src` (premultiplied around the blur), Mirror edges:
/// per component a vertical pass with Re K and Im K, a horizontal complex pass, partial =
/// Z·re + W·im; the partials summed in f32. Quality clamps to 1–10; radius ≤ 0 returns `src`.
pub fn bokeh(src: &EffectSurface, radius: f64, quality: usize, gamma: f64) -> EffectSurface {
    bokeh_with_edge(src, radius, quality, gamma, Edge::Mirror)
}

/// Bokeh on straight input/output, with explicit off-canvas extension. Premultiplication occurs
/// exactly once inside `boosted`; callers holding premultiplied data must convert before entry.
pub fn bokeh_with_edge(
    src: &EffectSurface,
    radius: f64,
    quality: usize,
    gamma: f64,
    edge: Edge,
) -> EffectSurface {
    if radius <= 0.0 {
        return src.clone();
    }
    let kernels = bokeh_kernels(radius, quality.clamp(1, 10));
    boosted(src, gamma, |s| {
        let mut sum = EffectSurface::new(s.w, s.h);
        for k in &kernels {
            let (vr, vi) = (vertical(s, &k.re, edge), vertical(s, &k.im, edge));
            let half = (k.re.len() / 2) as i32;
            let part = EffectSurface::from_fn(s.w, s.h, |x, y| {
                let (mut re, mut im) = ([0.0f32; 4], [0.0f32; 4]);
                for (i, (&kr, &ki)) in (-half..).zip(k.re.iter().zip(&k.im)) {
                    let a = vr.fetch(x as i32 + i, y as i32, edge);
                    let b = vi.fetch(x as i32 + i, y as i32, edge);
                    for (((re, im), a), b) in re.iter_mut().zip(&mut im).zip(a).zip(b) {
                        *re += kr * a - ki * b;
                        *im += kr * b + ki * a;
                    }
                }
                std::array::from_fn(|c| k.z * re[c] + k.w * im[c])
            });
            for (o, p) in sum.px.iter_mut().zip(part.px) {
                madd(o, 1.0, p);
            }
        }
        sum
    })
}

/// out(x, y) = Σᵢ k[i]·in(x, y + i − ⌊L/2⌋) in i order, f32, with the requested border (Bokeh step 3.1).
fn vertical(src: &EffectSurface, k: &[f32], edge: Edge) -> EffectSurface {
    let half = (k.len() / 2) as i32;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let mut acc = [0.0; 4];
        for (i, &w) in (-half..).zip(k) {
            madd(&mut acc, w, src.fetch(x as i32, y as i32 + i, edge));
        }
        acc
    })
}

/// F10 over bilinear Transparent taps: Σ sample(sᵢ) / max(1, Σ coverage(sᵢ)), the sᵢ given by
/// `taps(p)` at each pixel's scene position p = (x + ½, y + ½).
fn coverage_normalised<I: Iterator<Item = [f32; 2]>>(
    src: &EffectSurface,
    taps: impl Fn([f32; 2]) -> I + Sync,
) -> EffectSurface {
    let size = [src.w as f32, src.h as f32];
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let (mut sum, mut cov) = ([0.0; 4], 0.0f32);
        for s in taps([x as f32 + 0.5, y as f32 + 0.5]) {
            madd(&mut sum, 1.0, src.bilinear(s[0], s[1], Edge::Transparent));
            cov += coverage(s, size);
        }
        sum.map(|c| c / cov.max(1.0))
    })
}

/// §2.3 Fragment: θ = radians(rotation − 90); oᵢ = d·(sin(θ + 2πi/n), cos(θ + 2πi/n)).
pub fn fragment_offsets(n: u32, distance: f64, rotation_deg: f64) -> Vec<[f32; 2]> {
    let theta = (rotation_deg - 90.0).to_radians();
    (0..n)
        .map(|i| {
            let a = theta + TAU * i as f64 / n as f64;
            [(distance * a.sin()) as f32, (distance * a.cos()) as f32]
        })
        .collect()
}

/// §2.3 Motion step 3: (t_min, R).
fn motion_range(distance: f64, centered: bool) -> (f64, f64) {
    if centered {
        (-distance / 2.0, distance / 2.0)
    } else {
        (0.0, distance)
    }
}

/// tₛ = ⌊t_min⌋ + k/8 while tₛ ≤ ⌈R⌉.
fn motion_ts(t_min: f64, r: f64) -> impl Iterator<Item = f64> {
    let (start, end) = (t_min.floor(), r.ceil());
    (0..)
        .map(move |k| start + k as f64 / 8.0)
        .take_while(move |&t| t <= end)
}

pub fn motion_sample_count(distance: f64, centered: bool) -> usize {
    let (t_min, r) = motion_range(distance, centered);
    motion_ts(t_min, r).count()
}

/// §2.3 Motion steps 2–5: samples at tₛ·dir (dir = unit(−d·cos θ, d·sin θ)) weighted
/// Gauss(|tₛ|, R/3), F7-splatted; returns the taps and f32(1/Σw) of the unsplatted weights.
/// distance ≤ 0 gives NaN weights (σ = 0): callers pass the image through instead.
pub fn motion_kernel(distance: f64, angle_deg: f64, centered: bool) -> (Vec<([i32; 2], f64)>, f32) {
    let theta = angle_deg.to_radians();
    let (dx, dy) = (-distance * theta.cos(), distance * theta.sin());
    let len = dx.hypot(dy);
    let dir = [dx / len, dy / len];
    let (t_min, r) = motion_range(distance, centered);
    let sigma = r / 3.0;
    let gauss = |t: f64| (-(t * t) / (2.0 * sigma * sigma)).exp() / (sigma * (2.0 * PI).sqrt());
    let samples: Vec<([f64; 2], f64)> = motion_ts(t_min, r)
        .map(|t| ([t * dir[0], t * dir[1]], gauss(t.abs())))
        .collect();
    let total: f64 = samples.iter().map(|s| s.1).sum();
    (splat(&samples), (1.0 / total) as f32)
}

fn motion_effect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let edge = match choice(v, "edge") {
        "mirror" => Edge::Mirror,
        "wrap" => Edge::Wrap,
        "transparent" => Edge::Transparent,
        _ => Edge::Clamp,
    };
    motion_blur(
        src,
        num(v, "distance"),
        num(v, "angle"),
        flag(v, "centered"),
        edge,
    )
}

/// §2.3 Motion step 6 on premultiplied `src`: Σ over the (y, x)-sorted taps of w·in(p + tap)
/// through `edge`, × 1/Σw. Distance ≤ 0 returns `src`.
pub fn motion_blur(
    src: &EffectSurface,
    distance: f64,
    angle_deg: f64,
    centered: bool,
    edge: Edge,
) -> EffectSurface {
    if distance <= 0.0 {
        return src.clone();
    }
    let (taps, inv) = motion_kernel(distance, angle_deg, centered);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let mut acc = [0.0; 4];
        for &([tx, ty], w) in &taps {
            madd(
                &mut acc,
                w as f32,
                src.fetch(x as i32 + tx, y as i32 + ty, edge),
            );
        }
        acc.map(|a| a * inv)
    })
}

/// §2.3 Radial/Zoom: N = trunc(2^(q + 5)).
pub fn sample_count(quality: f64) -> u32 {
    (quality + 5.0).exp2() as u32
}

/// §2.3 Radial/Zoom centre (W/2, H/2) + (W/2, H/2)·(centerX, centerY) on the document.
fn center(v: &Values, env: &EffectEnv) -> [f32; 2] {
    let (hw, hh) = (env.width as f64 / 2.0, env.height as f64 / 2.0);
    [
        (hw + hw * num(v, "centerX")) as f32,
        (hh + hh * num(v, "centerY")) as f32,
    ]
}

/// §2.3 Radial: θᵢ = lerp(−A/2, A/2, i·f32(1/(N − 1))); taps at c + R(−θᵢ)(p − c).
fn radial_effect(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let n = sample_count(num(v, "quality").clamp(1.0, 8.0)) as usize;
    let half = (num(v, "angle").to_radians() / 2.0) as f32;
    let step = (1.0 / (n - 1) as f64) as f32;
    let turns: Vec<[f32; 2]> = (0..n)
        .map(|i| {
            let t = -half + (half + half) * (i as f32 * step);
            [t.cos(), t.sin()]
        })
        .collect();
    let c = center(v, env);
    coverage_normalised(src, |p| {
        let d = [p[0] - c[0], p[1] - c[1]];
        turns
            .iter()
            .map(move |&[co, si]| [c[0] + co * d[0] + si * d[1], c[1] - si * d[0] + co * d[1]])
    })
}

/// §2.3 Zoom: μ = e^(e + 1)·(e + 1).
pub fn zoom_weight_multiplier(focus: f64) -> f64 {
    focus.powf(focus + 1.0) * (focus + 1.0)
}

/// §2.3 Zoom, Clamp edges (coverage 1): sᵢ = lerp(m, M, i/(N − 1)), wᵢ = μ·|(sᵢ − sub)/den|^e,
/// taps at c + (p − c)/sᵢ; Σ w·sample / max(1, Σw).
fn zoom_effect(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let k = num(v, "distance");
    if k == 1.0 {
        return src.clone();
    }
    let e = num(v, "focus");
    let n = sample_count(num(v, "quality").clamp(1.0, 8.0)) as usize;
    let (m, big) = (k.min(1.0), k.max(1.0));
    let (sub, den) = if m >= 1.0 {
        (big, -(big - m))
    } else {
        (m, big - m)
    };
    let den = if den == 0.0 { 1.0 } else { den };
    let mu = zoom_weight_multiplier(e) as f32;
    let (m, big, sub, den, e) = (m as f32, big as f32, sub as f32, den as f32, e as f32);
    let taps: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let s = m + (big - m) * (i as f32 / (n - 1) as f32);
            (s, mu * ((s - sub) / den).abs().powf(e))
        })
        .collect();
    let norm = taps.iter().map(|t| t.1).sum::<f32>().max(1.0);
    let c = center(v, env);
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let d = [x as f32 + 0.5 - c[0], y as f32 + 0.5 - c[1]];
        let mut sum = [0.0; 4];
        for &(s, w) in &taps {
            let p = src.bilinear(c[0] + d[0] / s, c[1] + d[1] / s, Edge::Clamp);
            madd(&mut sum, w, p);
        }
        sum.map(|a| a / norm)
    })
}

/// HLSL f32tof16 taken as round-toward-zero (§6 item 4): clear the low 13 mantissa bits; x ≥ 0.
pub fn f16_floor(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & !0x1fff)
}

/// `f16_floor(x)` when exact, else one half-precision ulp above it.
pub fn f16_ceil(x: f32) -> f32 {
    let b = x.to_bits();
    if b & 0x1fff == 0 {
        x
    } else {
        f32::from_bits((b & !0x1fff) + 0x2000)
    }
}

/// Separable min or max over the offsets `lo..=hi` per axis ([x, y]), x then y, through `edge`.
pub(super) fn box_fold(
    src: &EffectSurface,
    lo: [i32; 2],
    hi: [i32; 2],
    edge: Edge,
    f: fn(f32, f32) -> f32,
) -> EffectSurface {
    let pass = |s: &EffectSurface, a: usize| {
        EffectSurface::from_fn(s.w, s.h, |x, y| {
            let at = |o: i32| {
                let mut q = [x as i32, y as i32];
                q[a] += o;
                s.fetch(q[0], q[1], edge)
            };
            let mut acc = at(lo[a]);
            for o in lo[a] + 1..=hi[a] {
                let t = at(o);
                acc = std::array::from_fn(|c| f(acc[c], t[c]));
            }
            acc
        })
    };
    pass(&pass(src, 0), 1)
}

/// §2.3 Median, per channel including alpha, over the F6 circle of `radius` with point taps:
/// p ≤ 0 → min, p ≥ 1 → max; otherwise τ = f32(A·p) with A·p in f64 (R32), the f16
/// bracket (square box when bits > 6, else the circle) and `bits` bisection steps; the result is
/// the last pivot.
pub fn percentile_filter(
    src: &EffectSurface,
    radius: u32,
    percentile: f64,
    bits: u32,
    edge: Edge,
) -> EffectSurface {
    percentile_impl(src, radius, percentile, bits, edge, is_8bit(src))
}

/// Every value is exactly k/255 for an integer k (F1 input of an sRGB effect).
fn is_8bit(src: &EffectSurface) -> bool {
    src.px.iter().flatten().all(|&v| {
        let k = (v * 255.0).round();
        (0.0..=255.0).contains(&k) && k / 255.0 == v
    })
}

/// Per row, a 256-bin histogram per channel of the 8-bit values under the F6 circle `rows`
/// (taps through `edge`), slid one pixel at a time; `f(x, y, hist)` gives each output pixel.
pub(super) fn sliding_histogram(
    src: &EffectSurface,
    rows: &[(i32, i32, u32)],
    edge: Edge,
    f: impl Fn(u32, u32, &[[u32; 256]; 4]) -> [f32; 4] + Sync,
) -> EffectSurface {
    debug_assert!(is_8bit(src));
    let mut out = EffectSurface::new(src.w, src.h);
    for_bands(
        &mut out.px,
        src.w as usize,
        0..src.h as usize,
        1,
        |y, row| {
            let y = y as i32;
            let put = |hist: &mut [[u32; 256]; 4], x: i32, dy: i32, add: bool| {
                for (h, v) in hist.iter_mut().zip(src.fetch(x, y + dy, edge)) {
                    let n = &mut h[(v * 255.0).round() as usize];
                    *n = if add { *n + 1 } else { *n - 1 };
                }
            };
            let mut hist = [[0u32; 256]; 4];
            for &(dy, x0, n) in rows {
                for dx in x0..x0 + n as i32 {
                    put(&mut hist, dx, dy, true);
                }
            }
            for (x, o) in row.iter_mut().enumerate() {
                *o = f(x as u32, y as u32, &hist);
                for &(dy, x0, n) in rows {
                    put(&mut hist, x as i32 + x0, dy, false);
                    put(&mut hist, x as i32 + x0 + n as i32, dy, true);
                }
            }
        },
    );
    out
}

/// `sliding`: 8-bit input (every value k/255) keeps a 256-bin window histogram per row, O(r) per
/// pixel; it decides τ ≤ #{taps ≤ pivot} through the order statistic, which gives the same
/// pivots. Otherwise each pixel counts its taps every step, as the shader does, O(r²·bits).
fn percentile_impl(
    src: &EffectSurface,
    radius: u32,
    percentile: f64,
    bits: u32,
    edge: Edge,
    sliding: bool,
) -> EffectSurface {
    let rows = circle_rows(radius);
    let tau = (rows.iter().map(|r| r.2).sum::<u32>() as f64 * percentile) as f32;
    let square = (bits > 6 && percentile > 0.0 && percentile < 1.0).then(|| {
        let r = radius as i32;
        (
            box_fold(src, [-r; 2], [r; 2], edge, f32::min),
            box_fold(src, [-r; 2], [r; 2], edge, f32::max),
        )
    });
    let pick = |x: u32, y: u32, c: usize, circle: (f32, f32), reaches: &dyn Fn(f32) -> bool| {
        if percentile <= 0.0 {
            return circle.0;
        }
        if percentile >= 1.0 {
            return circle.1;
        }
        let (min, max) = square
            .as_ref()
            .map_or(circle, |(lo, hi)| (lo.at(x, y)[c], hi.at(x, y)[c]));
        let (mut lo, mut hi) = (f16_floor(min), f16_ceil(max));
        let mut pivot = (lo + hi) / 2.0;
        for _ in 0..bits {
            if reaches(pivot) {
                hi = pivot;
            } else {
                lo = pivot;
            }
            pivot = (lo + hi) / 2.0;
        }
        pivot
    };
    if !sliding {
        let taps = circle_offsets(radius);
        return EffectSurface::from_fn(src.w, src.h, |x, y| {
            let s: Vec<[f32; 4]> = taps
                .iter()
                .map(|&(dx, dy)| src.fetch(x as i32 + dx, y as i32 + dy, edge))
                .collect();
            std::array::from_fn(|c| {
                let circle = s
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), t| {
                        (a.min(t[c]), b.max(t[c]))
                    });
                let count = |pivot: f32| s.iter().filter(|t| t[c] <= pivot).count() as f32;
                pick(x, y, c, circle, &|pivot| tau <= count(pivot))
            })
        });
    }
    sliding_histogram(src, &rows, edge, |x, y, hist| {
        std::array::from_fn(|c| {
            let (h, val) = (&hist[c], |b: usize| b as f32 / 255.0);
            let first = h.iter().position(|&n| n > 0).unwrap_or(0);
            let last = h.iter().rposition(|&n| n > 0).unwrap_or(0);
            let mut seen = 0;
            let kth = h
                .iter()
                .position(|&n| {
                    seen += n;
                    tau <= seen as f32
                })
                .map_or(f32::INFINITY, val);
            pick(x, y, c, (val(first), val(last)), &|p| kth <= p)
        })
    })
}

/// §2.3 Sketch step 1: M = max(5, rne(πr²)), K = clamp(rne(f32(M)·0.01), 5, M).
pub fn sketch_counts(radius: f64) -> (usize, usize) {
    let m = ((PI * (radius * radius)).round_ties_even() as usize).max(5);
    let k = ((m as f32 * 0.01).round_ties_even() as usize).clamp(5, m);
    (m, k)
}

/// §2.3 Sketch step 2 for iteration j: f32(ρ cos φ·r, ρ sin φ·r) with ρ = √next, φ = next·2π
/// from `Random(j)`. Only the first K of the M draws are kept and every iteration reseeds, so the
/// discarded draws are skipped.
fn sketch_offsets(r: f64, j: i32, k: usize) -> Vec<[f32; 2]> {
    let mut rng = NetRandom::new(j);
    (0..k)
        .map(|_| {
            let rho = rng.next_double().sqrt();
            let phi = rng.next_double() * TAU;
            [(rho * phi.cos() * r) as f32, (rho * phi.sin() * r) as f32]
        })
        .collect()
}

/// §2.3 Sketch, sRGB straight, Mirror bilinear taps: per iteration the P² estimate of each
/// channel over the K taps; RGB estimates averaged in linear light (alpha as is), × f32(1/J).
fn sketch_effect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let r = num(v, "radius");
    if r <= 0.0 {
        return src.clone();
    }
    let p = (num(v, "percentile") / 100.0) as f32;
    let iters = (num(v, "smoothness") as i32).max(1);
    let k = sketch_counts(r).1;
    let offsets: Vec<Vec<[f32; 2]>> = (0..iters).map(|j| sketch_offsets(r, j, k)).collect();
    let inv = (1.0 / iters as f64) as f32;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let mut sum = [0.0f32; 4];
        for offs in &offsets {
            let taps: Vec<[f32; 4]> = offs
                .iter()
                .map(|o| src.bilinear(px + o[0], py + o[1], Edge::Mirror))
                .collect();
            for (c, s) in sum.iter_mut().enumerate() {
                let e = p2(taps.iter().map(|t| t[c]), p);
                *s += if c < 3 { srgb_to_linear_f32(e) } else { e };
            }
        }
        std::array::from_fn(|c| {
            let a = sum[c] * inv;
            if c < 3 { linear_to_srgb_f32(a) } else { a }
        })
    })
}

/// .NET's seeded `System.Random` (Knuth's subtractive generator, Appendix C); C#'s unchecked
/// int arithmetic wraps.
pub struct NetRandom {
    a: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl NetRandom {
    pub fn new(seed: i32) -> Self {
        let mut a = [0i32; 56];
        let m = if seed == i32::MIN {
            i32::MAX
        } else {
            seed.abs()
        };
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
        Self {
            a,
            inext: 0,
            inextp: 21,
        }
    }

    pub fn next_double(&mut self) -> f64 {
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

/// Appendix C's P² estimate (marker q₂) of the p-quantile of `samples` (at least 5), in order;
/// p is clamped to [0, 1].
pub fn p2_quantile(samples: &[f32], p: f32) -> f32 {
    p2(samples.iter().copied(), p)
}

fn p2(mut xs: impl Iterator<Item = f32>, p: f32) -> f32 {
    // p > 1.125 would put a marker at sorted index 5.
    let p = p.clamp(0.0, 1.0);
    let mut s: [f32; 5] = std::array::from_fn(|_| xs.next().expect("P² needs 5 samples"));
    s.sort_by(f32::total_cmp);
    let mut n = [
        0.0,
        (2.0 * p).round_ties_even(),
        (4.0 * p).round_ties_even(),
        (2.0 + 2.0 * p).round_ties_even(),
        4.0f32,
    ];
    let mut q = [
        s[0],
        s[n[1] as usize],
        s[n[2] as usize],
        s[n[3] as usize],
        s[4],
    ];
    let want = [p / 2.0, p, (1.0 + p) / 2.0, 1.0];
    let order = if p >= 0.5 { [1, 2, 3] } else { [3, 2, 1] };
    let mut count = 5.0f32;
    for x in xs {
        let mut below = x < q[0];
        if below {
            q[0] = x;
        }
        for (qi, ni) in q[1..4].iter().zip(&mut n[1..4]) {
            below |= x < *qi;
            *ni += f32::from(u8::from(below));
        }
        if !(below || x < q[4]) {
            q[4] = x;
        }
        n[4] += 1.0;
        for i in order {
            p2_adjust(&mut q, &mut n, i, count * want[i - 1]);
        }
        count += 1.0;
    }
    q[2]
}

/// Appendix C step 3: move marker i one position toward `target`, parabolic if it stays between
/// its neighbours, else linear.
fn p2_adjust(q: &mut [f32; 5], n: &mut [f32; 5], i: usize, target: f32) {
    let d = target - n[i];
    let moves = (d >= 1.0 && n[i + 1] - n[i] > 1.0) || (d <= -1.0 && n[i - 1] - n[i] < -1.0);
    if !moves {
        return;
    }
    let s = d.signum();
    let a = (n[i] - n[i - 1] + s) * (q[i + 1] - q[i]) / (n[i + 1] - n[i]);
    let b = (n[i + 1] - n[i] - s) * (q[i] - q[i - 1]) / (n[i] - n[i - 1]);
    let qp = q[i] + s / (n[i + 1] - n[i - 1]) * (a + b);
    q[i] = if q[i - 1] < qp && qp < q[i + 1] {
        qp
    } else {
        let j = if s > 0.0 { i + 1 } else { i - 1 };
        q[i] + s * (q[j] - q[i]) / (n[j] - n[i])
    };
    n[i] += s;
}

/// §2.3 Surface: λ = f32(0.96/T), T = threshold/100; w = max(0, 1 − |s − v|·λ).
pub fn surface_weight(v: f32, s: f32, threshold: f64) -> f32 {
    surface_w(v, s, surface_slope(threshold))
}

fn surface_slope(threshold: f64) -> f32 {
    (0.96 / (threshold / 100.0)) as f32
}

fn surface_w(v: f32, s: f32, lambda: f32) -> f32 {
    (1.0 - (s - v).abs() * lambda).max(0.0)
}

/// §2.3 Surface, sRGB straight, Mirror edges: per channel Σws/Σw over the F6 circle of
/// ⌈radius⌉. The input is 8-bit (F1), so a sliding histogram sums per value instead of per tap:
/// Σ_b n_b·w(v, b/255)·b/255 over the bins with w > 0 (|b/255 − v| < 1/λ), O(r + 255/λ) per
/// pixel. Only the order of the f32 sum differs from the shader's tap order (§5: ±1).
fn surface_effect(src: &EffectSurface, v: &Values, _: &EffectEnv) -> EffectSurface {
    let rows = circle_rows(num(v, "radius").ceil() as u32);
    let lambda = surface_slope(num(v, "threshold"));
    let val = |b: usize| b as f32 / 255.0;
    // Per centre bin: the first bin it weighs above 0, then (w, w·s) for the contiguous run.
    let table: Vec<(usize, Vec<[f32; 2]>)> = (0..256)
        .map(|c| {
            let run: Vec<(usize, f32)> = (0..256)
                .map(|b| (b, surface_w(val(c), val(b), lambda)))
                .filter(|t| t.1 > 0.0)
                .collect();
            let first = run.first().map_or(c, |t| t.0);
            (first, run.iter().map(|&(b, w)| [w, w * val(b)]).collect())
        })
        .collect();
    sliding_histogram(src, &rows, Edge::Mirror, |x, y, hist| {
        let c = src.at(x, y);
        std::array::from_fn(|k| {
            let (first, weights) = &table[(c[k] * 255.0).round() as usize];
            let (mut w, mut ws) = (0.0f32, 0.0f32);
            for (&n, &[t, ts]) in hist[k][*first..].iter().zip(weights) {
                w += n as f32 * t;
                ws += n as f32 * ts;
            }
            if w > 0.0 { ws / w } else { c[k] }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::test_util::{from_fn, px, run, solid};
    use serde_json::json;

    fn near(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
    }
    fn flat_is_identity(id: &str, values: serde_json::Value, tol: u8) {
        for p in [[128u8, 64, 200, 255], [30, 90, 10, 160]] {
            let out = run(id, values.clone(), &solid(40, 30, p));
            for y in 0..30 {
                for x in 0..40 {
                    assert!(
                        near(px(&out, x, y), p, tol),
                        "{id} {p:?} at ({x},{y}): {:?}",
                        px(&out, x, y)
                    );
                }
            }
        }
    }
    fn dot() -> ImageData {
        from_fn(9, 9, |x, y| {
            if (x, y) == (4, 4) {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
    }

    /// effects-1.md §5 Task 39: Q4, γ = 1, radius 2 → centre linear 0.508209² → sRGB 139.
    #[test]
    fn gaussian_white_dot() {
        let out = run("blurEffect", json!({"radius": 2, "quality": 4}), &dot());
        assert_eq!(px(&out, 4, 4), [139, 139, 139, 255]);
        assert_eq!(
            run("blurEffect", json!({"radius": 0}), &dot()).data,
            dot().data
        );
    }

    #[test]
    fn flat_images_stay_flat() {
        flat_is_identity("blurEffect", json!({"radius": 6, "gamma": 1.0}), 1);
        flat_is_identity("squareBlurEffect", json!({}), 1);
        flat_is_identity("bokehEffect", json!({"radius": 5}), 1);
        flat_is_identity("motionBlurEffect", json!({}), 1);
        flat_is_identity("radialBlurEffect", json!({"angle": 30}), 2);
        flat_is_identity("zoomBlurEffect", json!({}), 2);
        flat_is_identity("fragmentEffect", json!({}), 2);
        flat_is_identity("surfaceBlurEffect", json!({}), 0);
        flat_is_identity("sketchBlurEffect", json!({"radius": 4}), 1);
        for q in 1..=9 {
            flat_is_identity("medianBlurEffect", json!({"radius": 3, "quality": q}), 1);
        }
    }

    #[test]
    fn kernel_shapes() {
        let k = square_kernel(2.5);
        let want = [0.5f32, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5].map(|w| w * (1.0 / 6.0f32));
        assert!(
            k.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-7),
            "{k:?}"
        );
        let (taps, inv) = motion_kernel(10.0, 90.0, true);
        let sum: f64 = taps.iter().map(|t| t.1).sum::<f64>() * inv as f64;
        assert!((sum - 1.0).abs() < 1e-6);
        assert_eq!(motion_sample_count(10.0, true), 81);
        assert_eq!(sample_count(2.0), 128);
        assert_eq!(zoom_weight_multiplier(2.0), 24.0);
        assert_eq!(bokeh_len(25.0), 51);
        let f = fragment_offsets(4, 8.0, 0.0);
        let want = [[-8.0, 0.0], [0.0, 8.0], [8.0, 0.0], [0.0, -8.0]];
        assert!(
            f.iter()
                .zip(want)
                .all(|(a, b)| (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5),
            "{f:?}"
        );
    }

    /// Surface Blur weight: λ = 0.96/T; v 0.5, s 0.6, T 0.15 → w 0.36.
    #[test]
    fn surface_weight() {
        assert!((super::surface_weight(0.5, 0.6, 15.0) - 0.36).abs() < 1e-6);
    }

    /// Median Blur: f16 bracket, bisection; a 3×3 binary field goes to the majority.
    #[test]
    fn median_bisection() {
        assert_eq!(
            (f16_floor(1.0 / 3.0), f16_ceil(1.0 / 3.0)),
            (0.333_251_95, 0.333_496_1)
        );
        assert_eq!((f16_floor(0.5), f16_ceil(0.5)), (0.5, 0.5));
        let field = |ones: &[(u32, u32)]| {
            from_fn(3, 3, |x, y| {
                if ones.contains(&(x, y)) {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                }
            })
        };
        let five = field(&[(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)]);
        assert_eq!(
            px(&run("medianBlurEffect", json!({"radius": 1}), &five), 1, 1)[0],
            255
        );
        let four = field(&[(0, 0), (1, 0), (2, 0), (0, 1)]);
        assert_eq!(
            px(&run("medianBlurEffect", json!({"radius": 1}), &four), 1, 1)[0],
            0
        );
    }

    /// Appendix C: .NET Random(0) and the Sketch Blur sample counts.
    #[test]
    fn sketch_blur_randomness_is_deterministic() {
        assert_eq!(NetRandom::new(0).next_double(), 0.726_243_269_967_959_8);
        assert_eq!(sketch_counts(25.0), (1963, 20));
        let v = [0.1f32, 0.9, 0.5, 0.3, 0.7, 0.2, 0.8, 0.4, 0.6];
        assert!((p2_quantile(&v, 0.5) - 0.5).abs() < 0.11);
    }

    #[test]
    fn r19_controls() {
        let g = crate::effect::find("blurEffect").unwrap();
        let d = crate::effect::defaults(&g);
        assert_eq!(
            (d["radius"].as_f64(), d["quality"].as_f64()),
            (Some(2.0), Some(4.0))
        );
        let crate::effect::Control::Range { max, .. } = &g.controls[0] else {
            panic!()
        };
        assert_eq!(*max, 300.0);
        let ids: Vec<&str> = crate::effect::effects()
            .iter()
            .filter(|e| e.category == crate::effect::Category::Effect("blurring"))
            .map(|e| e.id)
            .collect();
        assert_eq!(ids.len(), 10);
        assert!(!ids.contains(&"unfocusEffect") && !ids.contains(&"reduceNoiseEffect"));
    }

    // ---- Task 39 additions: vectors re-derived from effects-1.md §2.3 / App. B–C. ----

    use crate::pixel::gamma::{linear_to_srgb8, quantize};

    /// A w×h test pattern with distinct values per channel.
    fn pattern(w: u32, h: u32) -> ImageData {
        from_fn(w, h, |x, y| {
            [
                ((x * 37 + y * 11) % 256) as u8,
                ((x * 5 + y * 53) % 256) as u8,
                (255 - (x * 29) % 256) as u8,
                (255 - (y * 17) % 128) as u8,
            ]
        })
    }

    /// §2.3 Gaussian steps 1–6, from the doc's w₀ = 0.508208994 (centre tap) and w₁:
    /// - white column over 50 %-alpha black, γ = 2 (gamma 1): colour w₀, α = w₀ + (1 − w₀)·a with
    ///   a = 128/255, c = √(w₀/α) → 233.70, α → 192.54 (without the boost: 214);
    /// - the same over 50 %-alpha grey 128 (linear g), γ = 1: (w₀ + (1 − w₀)·g·a)/α → 223.76
    ///   (blurring straight colour: 233 with the unpremultiply, 206 without);
    /// - Mirror edges (F2): a white column at x = 0 gives w₀ + w₁ → 222.62 (Clamp: 225).
    #[test]
    fn gaussian_premultiplies_boosts_and_mirrors() {
        let img = from_fn(9, 9, |x, _| if x == 4 { [255; 4] } else { [0, 0, 0, 128] });
        let out = run("blurEffect", json!({"radius": 2, "gamma": 1.0}), &img);
        assert_eq!(px(&out, 4, 4), [234, 234, 234, 193]);
        let img = from_fn(9, 9, |x, _| {
            if x == 4 {
                [255; 4]
            } else {
                [128, 128, 128, 128]
            }
        });
        let out = run("blurEffect", json!({"radius": 2}), &img);
        assert_eq!(px(&out, 4, 4), [224, 224, 224, 193]);
        let img = from_fn(9, 9, |x, _| if x == 0 { [255; 4] } else { [0, 0, 0, 255] });
        assert_eq!(
            px(&run("blurEffect", json!({"radius": 2}), &img), 0, 4),
            [223, 223, 223, 255]
        );
    }

    /// §2.3 Square, r = 2: taps ⅕ each, Mirror edges: x = −2 → 1 (black), −1 → 0 (white) → 2/5
    /// linear → 169.62. Clamp would give 3/5 → 203.
    #[test]
    fn square_blur_mirrors_edges() {
        let img = from_fn(7, 7, |x, _| if x == 0 { [255; 4] } else { [0, 0, 0, 255] });
        assert_eq!(
            px(&run("squareBlurEffect", json!({"radius": 2}), &img), 0, 3),
            [170, 170, 170, 255]
        );
    }

    /// BB15 transparent borders must lose alpha at the canvas; the built-in wrapper still mirrors.
    #[test]
    fn bokeh_explicit_edge_and_alpha_contract() {
        // Binary-exact ratios isolate alpha conversion from Q10 complex-sum cancellation.
        let src = EffectSurface::from_fn(9, 7, |_, _| [0.5, 0.25, 0.125, 0.25]);
        let soft = bokeh_with_edge(&src, 3.0, 10, 0.0, Edge::Transparent);
        let hard = bokeh(&src, 3.0, 10, 0.0);
        assert_eq!(hard, bokeh_with_edge(&src, 3.0, 10, 0.0, Edge::Mirror));
        let p = soft.at(0, 0);
        assert!(p[3] > 0.0 && p[3] < 0.2, "{p:?}");
        for (got, want) in p[..3].iter().zip([0.5, 0.25, 0.125]) {
            assert!((got - want).abs() < 1e-5, "{p:?}: {got} != {want}");
        }
        assert!((hard.at(0, 0)[3] - 0.25).abs() < 1e-5);
        assert_eq!(bokeh_with_edge(&src, 0.0, 10, 0.0, Edge::Transparent), src);
    }

    /// §2.3 Bokeh + App. B, re-derived in Python (f64 kernel, 1/√T, f32 cast): Q1 r 2.5 and r 3 taps; every
    /// Q's flat response is 1 (§5 "±1e−4"); a Q3 r 3 dot spreads as Σ Z·Re(KₓK_y) + W·Im(KₓK_y).
    #[test]
    fn bokeh_kernels_and_dot() {
        // r 2.5: |n| > r is skipped and u = n·s/r uses r itself, not ⌈r⌉.
        let k = bokeh_kernels(2.5, 1);
        let re = [
            -0.0073934523,
            -0.0084326845,
            0.17633957,
            0.27458334,
            0.17633957,
            -0.0084326845,
            -0.0073934523,
        ];
        let im = [
            0.0031275041,
            0.07911523,
            0.09544991,
            0.02188594,
            0.09544991,
            0.07911523,
            0.0031275041,
        ];
        for i in 0..7 {
            assert!(
                (k[0].re[i] - re[i]).abs() < 1e-7 && (k[0].im[i] - im[i]).abs() < 1e-7,
                "{i}: {:?}",
                k[0]
            );
        }
        let k = bokeh_kernels(3.0, 1);
        let re = [
            -0.022127422,
            0.027580505,
            0.17598517,
            0.23273619,
            0.17598517,
            0.027580505,
            -0.022127422,
        ];
        let im = [
            0.01888574,
            0.09583529,
            0.067344956,
            0.013135722,
            0.067344956,
            0.09583529,
            0.01888574,
        ];
        assert_eq!(k.len(), 1);
        for i in 0..7 {
            assert!(
                (k[0].re[i] - re[i]).abs() < 1e-7 && (k[0].im[i] - im[i]).abs() < 1e-7,
                "{i}: {:?}",
                k[0]
            );
        }
        for q in 1..=10 {
            for r in [3.0, 7.5, 25.0] {
                let ks = bokeh_kernels(r, q);
                assert_eq!((ks.len(), ks[0].re.len()), (q, bokeh_len(r)));
                let dc: f64 = ks
                    .iter()
                    .map(|k| {
                        let (a, b) = (
                            k.re.iter().map(|&v| v as f64).sum::<f64>(),
                            k.im.iter().map(|&v| v as f64).sum::<f64>(),
                        );
                        k.z as f64 * (a * a - b * b) + k.w as f64 * (2.0 * a * b)
                    })
                    .sum();
                assert!((dc - 1.0).abs() < 1e-4, "Q{q} r{r}: {dc}");
            }
        }
        let img = from_fn(15, 15, |x, y| {
            if (x, y) == (7, 7) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        });
        let out = run("bokehEffect", json!({"radius": 3}), &img);
        for ((a, b), want) in [
            ((0, 0), 58),
            ((1, 0), 58),
            ((2, 1), 53),
            ((3, 0), 28),
            ((2, 2), 35),
            ((3, 3), 0),
        ] {
            let p = px(&out, 7 + a, 7 + b);
            assert!(near(p, [want, want, want, 255], 1), "({a},{b}): {p:?}");
        }
    }

    /// The 1-D re-derivation of §2.3 Motion steps 2–5: tap k gets Σₛ G(|tₛ|)·max(0, 1 − |tₛ − k|).
    fn motion_weights(ts: &[f64], sigma: f64, k: f64) -> f64 {
        let g = |t: f64| {
            (-t * t / (2.0 * sigma * sigma)).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())
        };
        ts.iter()
            .map(|&t| g(t.abs()) * (1.0 - (t - k).abs()).max(0.0))
            .sum()
    }

    /// Angle 0 points the kernel to −x (dx = −d·cos θ); not centered, t ∈ [0, d], σ = d/3;
    /// centered, t ∈ [−d/2, d/2], σ = d/6.
    #[test]
    fn motion_kernel_taps() {
        let ts: Vec<f64> = (0..=16).map(|s| s as f64 / 8.0).collect();
        let (taps, inv) = motion_kernel(2.0, 0.0, false);
        assert_eq!(
            taps.iter().map(|t| t.0).collect::<Vec<_>>(),
            [[-2, 0], [-1, 0], [0, 0]]
        );
        for (t, k) in taps.iter().zip([2.0, 1.0, 0.0]) {
            assert!(
                (t.1 - motion_weights(&ts, 2.0 / 3.0, k)).abs() < 1e-12,
                "{taps:?}"
            );
        }
        let g = |t: f64| {
            (-t * t / (2.0 * 4.0 / 9.0)).exp() / (2.0 / 3.0 * (2.0 * std::f64::consts::PI).sqrt())
        };
        let want = 1.0 / ts.iter().map(|&t| g(t)).sum::<f64>();
        assert!((inv as f64 / want - 1.0).abs() < 1e-7, "{inv} {want}");
        assert_eq!(motion_sample_count(2.0, false), 17);
        let ts: Vec<f64> = (-16..=16).map(|s| s as f64 / 8.0).collect();
        let (taps, _) = motion_kernel(4.0, 0.0, true);
        assert_eq!(
            taps.iter().map(|t| t.0).collect::<Vec<_>>(),
            [[-2, 0], [-1, 0], [0, 0], [1, 0], [2, 0]]
        );
        for (t, k) in taps.iter().zip([-2.0, -1.0, 0.0, 1.0, 2.0]) {
            assert!(
                (t.1 - motion_weights(&ts, 2.0 / 3.0, k)).abs() < 1e-12,
                "{taps:?}"
            );
        }
    }

    /// F2 through Motion's `edge` control: a white pixel at x = 0, kernel taps x − 2, x − 1, x.
    #[test]
    fn motion_edge_modes() {
        let ts: Vec<f64> = (0..=16).map(|s| s as f64 / 8.0).collect();
        let w: Vec<f64> = (0..3)
            .map(|k| motion_weights(&ts, 2.0 / 3.0, k as f64))
            .collect();
        let total: f64 = w.iter().sum();
        let img = from_fn(6, 1, |x, _| if x == 0 { [255; 4] } else { [0, 0, 0, 255] });
        let at0 = |edge: &str| {
            px(
                &run(
                    "motionBlurEffect",
                    json!({"angle": 0, "distance": 2, "centered": false, "edge": edge}),
                    &img,
                ),
                0,
                0,
            )
        };
        let grey = |l: f64| linear_to_srgb8(l as f32);
        assert_eq!(at0("clamp"), [255; 4]);
        let m = grey((w[0] + w[1]) / total);
        assert!(
            near(at0("mirror"), [m, m, m, 255], 1),
            "{:?}",
            at0("mirror")
        );
        let v = grey(w[0] / total);
        assert!(near(at0("wrap"), [v, v, v, 255], 1), "{:?}", at0("wrap"));
        assert!(
            near(
                at0("transparent"),
                [255, 255, 255, quantize((w[0] / total) as f32)],
                1
            ),
            "{:?}",
            at0("transparent")
        );
    }

    /// F10: two horizontal fragments ±2; at x = 4 one tap is the white column, the other is off
    /// the layer (coverage 0), so the white is not diluted.
    #[test]
    fn fragment_taps_and_coverage() {
        let img = from_fn(5, 5, |x, _| if x == 2 { [255; 4] } else { [0, 0, 0, 255] });
        let out = run(
            "fragmentEffect",
            json!({"fragments": 2, "distance": 2}),
            &img,
        );
        assert_eq!(
            [px(&out, 4, 2), px(&out, 0, 2), px(&out, 2, 2)],
            [[255; 4], [255; 4], [0, 0, 0, 255]]
        );
    }

    /// §2.3 Radial, re-derived in Python (f32, ideal bilinear, F10): q 1 (N 64), angle 30 about
    /// the centre of a 20×20 black | white step at x = 10. Sweeping ±A instead of ±A/2 gives
    /// 216/152/175; degrees used as radians give 188/187/188.
    #[test]
    fn radial_blur_vector() {
        let img = from_fn(
            20,
            20,
            |x, _| if x >= 10 { [255; 4] } else { [0, 0, 0, 255] },
        );
        let out = run("radialBlurEffect", json!({"angle": 30, "quality": 1}), &img);
        for ((x, y), want) in [((11, 2), 240), ((8, 17), 101), ((9, 3), 161)] {
            let p = px(&out, x, y);
            assert!(near(p, [want, want, want, 255], 2), "({x},{y}): {p:?}");
        }
    }

    /// Gaussian quality 1–3 is Direct2D's GaussianBlur, approximated by the HQ kernel until
    /// capture B-E2: smoke only. Radius 0 is the identity; a step edge softens monotonically.
    #[test]
    fn gaussian_low_quality_smoke() {
        let img = from_fn(16, 4, |x, _| if x >= 8 { [255; 4] } else { [0, 0, 0, 255] });
        for q in 1..=3 {
            let same = run("blurEffect", json!({"radius": 0, "quality": q}), &img);
            assert_eq!(same.data, img.data);
            let out = run("blurEffect", json!({"radius": 4, "quality": q}), &img);
            let row: Vec<u8> = (0..16).map(|x| px(&out, x, 1)[0]).collect();
            let rising = row.windows(2).all(|w| w[0] <= w[1]);
            assert!(
                rising && row[7] > 0 && row[8] < 255 && row[0] == 0,
                "Q{q}: {row:?}"
            );
        }
    }

    /// §2.3 Radial: c = (W/2, H/2)·(1 + offset) on the document; the pixel at c never moves.
    #[test]
    fn radial_blur_turns_about_the_centre() {
        let img = pattern(10, 10);
        let fixed = run(
            "radialBlurEffect",
            json!({"angle": 90, "centerX": -0.9, "centerY": 0.3}),
            &img,
        );
        assert_eq!(px(&fixed, 0, 6), px(&img, 0, 6));
        let moved = run("radialBlurEffect", json!({"angle": 90}), &img);
        assert_ne!(px(&moved, 0, 6), px(&img, 0, 6));
    }

    /// §2.3 Zoom on a black→white step at x = 15 (row through c), re-derived in Python (f64):
    /// k = 2 samples c + (p − c)/s, s ∈ [1, 2], weights 24·(2 − s)²; k = ½ reaches outward.
    #[test]
    fn zoom_blur_direction_and_weights() {
        let img = from_fn(
            20,
            3,
            |x, _| if x >= 15 { [255; 4] } else { [0, 0, 0, 255] },
        );
        let row = |k: f64| {
            let out = run("zoomBlurEffect", json!({"distance": k}), &img);
            [13, 14, 15].map(|x| px(&out, x, 1)[0])
        };
        let [a, b, c] = row(2.0);
        assert!(a == 0 && b == 0 && c.abs_diff(144) <= 2, "{:?}", [a, b, c]);
        let [a, b, c] = row(0.5);
        assert!(
            a.abs_diff(77) <= 2 && b.abs_diff(195) <= 2 && c == 255,
            "{:?}",
            [a, b, c]
        );
    }

    /// §2.3 Median: Fast (B > 6) brackets over the square, Accurate over the circle; p ≥ 1 is the
    /// circle max; per channel including alpha; Clamp edges.
    #[test]
    fn median_bracket_extremes_and_edges() {
        // r 2 at (2, 2): the corner (0, 0) is in the square, not the circle (8 > 6).
        let img = from_fn(5, 5, |x, y| {
            if (x, y) == (0, 0) {
                [255, 0, 255, 0]
            } else {
                [0, 0, 255, 255]
            }
        });
        let at = |q: u32, p: u32| {
            px(
                &run(
                    "medianBlurEffect",
                    json!({"radius": 2, "quality": q, "percentile": p}),
                    &img,
                ),
                2,
                2,
            )
        };
        // Fast: R [0, 1] → 7 halvings toward 0 → 2⁻⁸ → 1; A [0, 1] → toward 1 → 254.
        assert_eq!(at(7, 50), [1, 0, 255, 254]);
        assert_eq!(at(6, 50), [0, 0, 255, 255]);
        assert_eq!(at(8, 100), [0, 0, 255, 255]);
        assert_eq!(at(8, 0), [0, 0, 255, 255]);
        // Row 0 255 255 0 0, r 2 at x = 0: Clamp sees 13 zeros of 21 → 0 (Mirror: 10 → 255).
        let row = from_fn(5, 1, |x, _| {
            if x == 1 || x == 2 {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        });
        assert_eq!(
            px(&run("medianBlurEffect", json!({"radius": 2}), &row), 0, 0)[0],
            0
        );
    }

    /// The 8-bit sliding-histogram path gives the per-pixel counting path's pivots exactly.
    #[test]
    fn median_paths_agree() {
        let src = EffectSurface::from_rgba8(&pattern(13, 9), Space::Srgb, Alpha::Straight);
        for edge in [Edge::Clamp, Edge::Mirror, Edge::Wrap, Edge::Transparent] {
            for (r, p, bits) in [
                (1, 0.5, 8),
                (2, 0.0, 3),
                (3, 1.0, 9),
                (3, 0.3, 6),
                (4, 0.77, 7),
                (2, 0.01, 1),
                // τ = A/3 is exactly 3 (r 1) and 7 (r 2): the τ ≤ count tie.
                (1, 1.0 / 3.0, 8),
                (2, 1.0 / 3.0, 7),
            ] {
                let a = percentile_impl(&src, r, p, bits, edge, false);
                assert_eq!(
                    a,
                    percentile_impl(&src, r, p, bits, edge, true),
                    "{edge:?} r{r} p{p} B{bits}"
                );
            }
        }
    }

    /// R32: τ = f32(A·p) with A·p in f64. r 14 (A 665), p 0.6 gives τ = 399 exactly; f32·f32
    /// gives 399.00003. With 399 zeros and 266 ones in the circle, count(pivot) = 399 ≥ τ → 0.
    #[test]
    fn median_tau_is_formed_in_f64() {
        let taps = circle_offsets(14);
        assert_eq!(taps.len(), 665);
        let ones = &taps[399..];
        let img = from_fn(29, 29, |x, y| {
            if ones.contains(&(x as i32 - 14, y as i32 - 14)) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            }
        });
        let out = run(
            "medianBlurEffect",
            json!({"radius": 14, "percentile": 60}),
            &img,
        );
        assert_eq!(px(&out, 14, 14), [0, 0, 0, 255]);
    }

    /// The histogram Surface Blur against the shader's per-pixel, tap-order f32 sum (§5: ±1).
    #[test]
    fn surface_blur_matches_direct_sum() {
        let img = from_fn(23, 17, |x, y| {
            crate::effect::rng::pcg(x + 23 * y).to_le_bytes()
        });
        let src = EffectSurface::from_rgba8(&img, Space::Srgb, Alpha::Straight);
        for (r, t) in [(1, 15.0), (3, 1.0), (6, 15.0), (5, 100.0)] {
            let out = run(
                "surfaceBlurEffect",
                json!({"radius": r, "threshold": t}),
                &img,
            );
            let (taps, lambda) = (circle_offsets(r), surface_slope(t));
            for (x, y) in (0..17).flat_map(|y| (0..23).map(move |x| (x, y))) {
                let c = src.at(x, y);
                let (mut ws, mut w) = ([0.0f32; 4], [0.0f32; 4]);
                for &(dx, dy) in &taps {
                    let s = src.fetch(x as i32 + dx, y as i32 + dy, Edge::Mirror);
                    for (((ws, w), s), v) in ws.iter_mut().zip(&mut w).zip(s).zip(c) {
                        let t = surface_w(v, s, lambda);
                        *ws += t * s;
                        *w += t;
                    }
                }
                let want = std::array::from_fn(|k| quantize(ws[k] / w[k]));
                assert!(
                    near(px(&out, x, y), want, 1),
                    "r{r} T{t} ({x},{y}): {:?} {want:?}",
                    px(&out, x, y)
                );
            }
        }
    }

    /// §2.3 Surface, T 0.15 (λ 6.4), straight per channel: R 128 among 140s → w 0.69882 →
    /// 138.18; G 60 among 200s and A 1 among 200/255 are cut off (w 0); B flat. Mirror at x = 0
    /// on 100 120 100 100 100 (r 2): 8 taps of 120 → 104.69 (Clamp: 5 → 102.69).
    #[test]
    fn surface_blur_per_channel_and_mirror() {
        let img = from_fn(3, 3, |x, y| {
            if (x, y) == (1, 1) {
                [128, 60, 200, 255]
            } else {
                [140, 200, 200, 200]
            }
        });
        assert_eq!(
            px(&run("surfaceBlurEffect", json!({"radius": 1}), &img), 1, 1),
            [138, 60, 200, 255]
        );
        let row = from_fn(5, 1, |x, _| {
            if x == 1 {
                [120, 120, 120, 255]
            } else {
                [100, 100, 100, 255]
            }
        });
        assert_eq!(
            px(&run("surfaceBlurEffect", json!({"radius": 2}), &row), 0, 0),
            [105, 105, 105, 255]
        );
    }

    /// Appendix C P², re-derived in Python (f32): init positions rne(2p)…, adjust order by p.
    #[test]
    fn p2_quantile_vectors() {
        assert_eq!(
            p2_quantile(&[0.1, 0.9, 0.5, 0.3, 0.7, 0.2, 0.8, 0.4, 0.6], 0.5),
            0.5
        );
        let w = [
            0.05f32, 0.6, 0.15, 0.95, 0.3, 0.7, 0.1, 0.45, 0.85, 0.2, 0.55, 0.4, 0.65,
        ];
        for (p, want) in [
            (0.1, 0.12133333),
            (0.3, 0.21333334),
            (0.5, 0.45000002),
            (0.9, 0.84166664),
        ] {
            assert!(
                (p2_quantile(&w, p) - want).abs() < 1e-6,
                "{p}: {}",
                p2_quantile(&w, p)
            );
        }
        // Here markers 3 and 2 move in one step; ascending order (wrong for p < ½) gives 0.6758.
        let w = [
            0.04, 0.96, 0.77, 0.05, 0.01, 0.96, 0.83, 0.78, 0.48, 0.81, 0.74,
        ];
        assert!((p2_quantile(&w, 0.3) - 0.5065).abs() < 1e-6);
        // p beyond [0, 1] (Sketch's JSON percentile > 112.5 indexed sorted[5]) is clamped.
        assert_eq!(p2_quantile(&w, 2.0), p2_quantile(&w, 1.0));
    }

    /// Appendix C `Random(seed)`: four NextDouble() per seed, from .NET 10 (`new Random(s)`),
    /// including the |int.MinValue| and negative-mj paths; Sketch offset 0 of iteration 0, r 25.
    #[test]
    fn net_random_matches_dotnet() {
        for (seed, want) in [
            (
                0,
                [
                    0.7262432699679598,
                    0.8173253595909687,
                    0.7680226893946634,
                    0.5581611914365372,
                ],
            ),
            (
                1,
                [
                    0.24866858415709278,
                    0.11074397718102856,
                    0.46701067987224587,
                    0.7716041220219825,
                ],
            ),
            (
                19,
                [
                    0.6523242395614852,
                    0.39227909380210524,
                    0.04879450846872968,
                    0.6135768725599986,
                ],
            ),
            (
                i32::MAX,
                [
                    0.7262432699679598,
                    0.8173253595909687,
                    0.7680226921886312,
                    0.5581611914365372,
                ],
            ),
            (
                i32::MIN,
                [
                    0.7262432699679598,
                    0.8173253595909687,
                    0.7680226921886312,
                    0.5581611914365372,
                ],
            ),
            (
                -5,
                [
                    0.33836984091362443,
                    0.2844184475412678,
                    0.2629626417825756,
                    0.6253758443637638,
                ],
            ),
        ] {
            let mut r = NetRandom::new(seed);
            assert_eq!(want.map(|_| r.next_double()), want, "seed {seed}");
        }
        let o = sketch_offsets(25.0, 0, 20)[0];
        assert!(
            (o[0] - 8.745992).abs() < 1e-5 && (o[1] + 19.427034).abs() < 1e-5,
            "{o:?}"
        );
    }

    /// §2.3 Sketch Blur end to end against an independent Python port (App. C Random and P², F2
    /// Mirror bilinear, linear-light average): r 20 (M 1257, K 13), p 0.3, J 2.
    #[test]
    fn sketch_blur_matches_reference() {
        assert_eq!(sketch_counts(20.0), (1257, 13));
        let out = run(
            "sketchBlurEffect",
            json!({"radius": 20, "percentile": 30, "smoothness": 2}),
            &pattern(8, 8),
        );
        for ((x, y), want) in [
            ((3, 4), [96, 73, 126, 166]),
            ((0, 0), [74, 70, 105, 178]),
            ((7, 2), [59, 87, 92, 176]),
        ] {
            assert!(
                near(px(&out, x, y), want, 1),
                "({x},{y}): {:?}",
                px(&out, x, y)
            );
        }
    }
}
