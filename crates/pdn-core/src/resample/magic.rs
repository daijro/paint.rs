//! Port of: PhotoSauce.MagicScaler 0.15.0 (MIT, © Clinton Ingram) — the interpolators, kernel map
//! and float convolution; R18. Not Paint.NET code. Source: https://github.com/saucecontrol/PhotoSauce
//! `src/MagicScaler/` at 0.15.0.
//! Copyright © Clinton Ingram and Contributors. SPDX-License-Identifier: MIT (full notice in NOTICE.md)
//! Ported: Core/Interpolators.cs, Core/ProcessImageSettings.cs + Utilities/SettingUtil.cs (default
//! interpolation), Magic/KernelMap.cs, Magic/MagicTransforms.cs (AddHighQualityScaler),
//! Magic/ConvolutionTransform.cs, Magic/Processors/ConvolversIntrinsic.cs (Convolver4ChanIntrinsic),
//! Core/LookupTables.cs, Magic/Processors/ConvertersLinear.cs + ConvertersFloat.cs (float → 8-bit).

use super::{Gamma, Kernel, PremulImage, size};
use std::f64::consts::PI;
use std::sync::LazyLock;

/// `CubicInterpolator(b, c)`'s polynomials, computed once (Interpolators.cs · GetValue).
#[derive(Clone, Copy)]
pub(crate) struct Cubic {
    p: [f64; 3],
    q: [f64; 4],
    /// Support 2 (else 1: b = c = 0).
    wide: bool,
}

/// Catmull-Rom: `Kernel::CatmullRom`'s interpolator.
pub(crate) const CATMULL_ROM: Cubic = Cubic::new(0.0, 0.5);

impl Cubic {
    const fn new(b: f64, c: f64) -> Cubic {
        Cubic {
            p: [
                (6.0 - 2.0 * b) / 6.0,
                (-18.0 + 12.0 * b + c * 6.0) / 6.0,
                (12.0 - 9.0 * b - c * 6.0) / 6.0,
            ],
            q: [
                (8.0 * b + c * 24.0) / 6.0,
                (-12.0 * b - c * 48.0) / 6.0,
                (6.0 * b + c * 30.0) / 6.0,
                (-b - c * 6.0) / 6.0,
            ],
            wide: !(b == 0.0 && c == 0.0),
        }
    }

    /// The weight at distance d ≥ 0.
    #[inline]
    pub(crate) fn value(&self, d: f64) -> f64 {
        let ([p0, p2, p3], [q0, q1, q2, q3]) = (self.p, self.q);
        if d < 1.0 {
            p0 + d * d * (p2 + d * p3)
        } else if self.wide && d < 2.0 {
            q0 + d * (q1 + d * (q2 + d * q3))
        } else {
            0.0
        }
    }
}

/// The `IInterpolator`s Paint.NET uses (resize modes, thumbnails, canvas mips).
#[derive(Clone, Copy)]
enum Interp {
    Box,
    Linear,
    Cubic(f64, f64),
    Lanczos3,
    Spline36,
    /// `QuadraticInterpolator(r = 1)`.
    Quadratic,
}

impl Interp {
    // Interpolators.cs · *Interpolator.Support
    fn support(self) -> f64 {
        match self {
            Interp::Box => 0.5,
            Interp::Linear => 1.0,
            Interp::Cubic(b, c) if b == 0.0 && c == 0.0 => 1.0,
            Interp::Cubic(..) => 2.0,
            Interp::Lanczos3 | Interp::Spline36 => 3.0,
            Interp::Quadratic => 1.5,
        }
    }

    // Interpolators.cs · *Interpolator.GetValue (d ≥ 0)
    fn value(self, d: f64) -> f64 {
        match self {
            Interp::Box => {
                if d <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Interp::Linear => {
                if d < 1.0 {
                    1.0 - d
                } else {
                    0.0
                }
            }
            Interp::Cubic(b, c) => Cubic::new(b, c).value(d),
            Interp::Lanczos3 => {
                let (support, isupport) = (3.0, 1.0 / 3.0);
                if d <= 5e-10 {
                    1.0
                } else if d < support {
                    let d = d * PI;
                    support * d.sin() * (d * isupport).sin() / (d * d)
                } else {
                    0.0
                }
            }
            Interp::Spline36 => {
                if d < 1.0 {
                    ((13.0 / 11.0 * d - 453.0 / 209.0) * d - 3.0 / 209.0) * d + 1.0
                } else if d < 2.0 {
                    let d = d - 1.0;
                    ((-6.0 / 11.0 * d + 270.0 / 209.0) * d - 156.0 / 209.0) * d
                } else if d < 3.0 {
                    let d = d - 2.0;
                    ((1.0 / 11.0 * d - 45.0 / 209.0) * d + 26.0 / 209.0) * d
                } else {
                    0.0
                }
            }
            Interp::Quadratic => {
                let r = 1.0;
                let (r0, r1, r2, r3) =
                    (-2.0 * r, -2.0 * r - 0.5, 0.5 * (r + 1.0), 0.75 * (r + 1.0));
                if d < 0.5 {
                    d * d * r0 + r2
                } else if d < 1.5 {
                    d * d * r + d * r1 + r3
                } else {
                    0.0
                }
            }
        }
    }
}

/// Paint.NET's kernel → MagicScaler interpolator (document-ops-and-formats.md §2.2 step 4).
/// `Adaptive` is SettingUtil.cs · GetDefaultInterpolation(ProcessImageSettings.ScaleRatio /
/// HybridScaleRatio), the ratio being min(src/dst) per axis and HybridScaleRatio 1 (Off); see the
/// B-F3 calibrate note on `scale_rows` for the other possible entry point.
fn interp(kernel: Kernel, ratio: f64) -> Interp {
    match kernel {
        Kernel::Linear => Interp::Linear,
        Kernel::CatmullRom => Interp::Cubic(0.0, 0.5),
        Kernel::CubicSmooth => Interp::Cubic(1.0, 0.0),
        Kernel::Lanczos3 => Interp::Lanczos3,
        Kernel::Average => Interp::Box,
        Kernel::Adaptive if ratio == 1.0 => Interp::Linear,
        Kernel::Adaptive if ratio < 0.5 => Interp::Lanczos3,
        Kernel::Adaptive if ratio > 16.0 => Interp::Quadratic,
        Kernel::Adaptive if ratio > 4.0 => Interp::Cubic(0.0, 0.5),
        Kernel::Adaptive => Interp::Spline36,
    }
}

/// The interpolator's value at distance |x| (Interpolators.cs · GetValue).
///
/// `Adaptive` has no fixed interpolator: MagicScaler picks it from the scale ratio, which this
/// function does not know, so it evaluates Linear (the ratio-1 choice). Callers that need the real
/// Adaptive kernel must resolve it through `scale`.
pub fn kernel_weight(kernel: Kernel, x: f64) -> f64 {
    interp(kernel, 1.0).value(x.abs())
}

/// One axis: per output pixel the first source index and `samples` normalised f32 weights.
struct KernelMap {
    samples: usize,
    start: Vec<usize>,
    weights: Vec<f32>,
}

impl KernelMap {
    fn weights(&self, i: usize) -> &[f32] {
        &self.weights[i * self.samples..][..self.samples]
    }
}

// KernelMap.cs · fillWeights
fn fill_weights(kernel: &mut [f32], interp: Interp, start: i32, center: f64, scale: f64) {
    let mut sum = 0.0;
    for (i, k) in kernel.iter_mut().enumerate() {
        let weight = interp.value(((start as f64 - center + i as f64) * scale).abs());
        sum += weight;
        *k = weight as f32;
    }
    let kscale = (1.0 / sum) as f32;
    for k in kernel {
        *k *= kscale;
    }
}

// KernelMap.cs · clamp: fold the weights of taps outside [0, ipix) into the edge tap.
fn clamp(mut start: i32, ipix: i32, kernel: &mut [f32]) -> i32 {
    let len = kernel.len() as i32;
    if start + len > ipix {
        let (ns, last) = (ipix - len, len - 1);
        let offs = start - ns;
        let mut a = 0f32;
        let mut i = 0;
        while i <= offs && i < len {
            a += kernel[(last - i) as usize];
            i += 1;
        }
        start = ns;
        kernel[last as usize] = a;
        for i in (0..last).rev() {
            kernel[i as usize] = if i >= offs {
                kernel[(i - offs) as usize]
            } else {
                0.0
            };
        }
    }
    if start < 0 {
        let offs = -start;
        let mut a = 0f32;
        for i in 0..=offs {
            a += kernel[i as usize];
        }
        start = 0;
        kernel[0] = a;
        for i in 1..len {
            kernel[i as usize] = if i + offs < len {
                kernel[(i + offs) as usize]
            } else {
                0.0
            };
        }
    }
    start
}

// KernelMap.cs · KernelMap<float>.create / CreateResample (offset 0: not a point sampler; Blur 1
// for every interpolator here; 4 float channels → getPadding is 0, so klen = ksize).
fn kernel_map(isize: usize, osize: usize, interp: Interp) -> KernelMap {
    let blur = 1.0;
    let mut support = interp.support();
    let cscale = (osize as f64 / isize as f64).min(1.0) / blur;
    support /= cscale;
    // prevent artifacts from heavily negative-weighted edges when kernel is larger than input
    if isize < 3 && support * 2.0 > isize as f64 {
        support = isize as f64 / 2.0;
    }
    let ksize = (support * 2.0).ceil() as usize;
    let samples = ksize.min(isize);
    let mut map = KernelMap {
        samples,
        start: Vec::with_capacity(osize),
        weights: Vec::with_capacity(osize * samples),
    };
    let mut kernel = vec![0f32; ksize];
    let sinc = isize as f64 / osize as f64;
    let mut spoint = (isize as f64 - osize as f64) / (osize as f64 * 2.0);
    for _ in 0..osize {
        let mut start = (spoint + support) as i32 - ksize as i32 + 1;
        fill_weights(&mut kernel, interp, start, spoint, cscale);
        spoint += sinc;
        if start < 0 || start + ksize as i32 > isize as i32 {
            start = clamp(start, isize as i32, &mut kernel);
        }
        map.start.push(start as usize);
        map.weights.extend_from_slice(&kernel[..samples]);
    }
    map
}

/// ConvolversIntrinsic.cs · Convolver4ChanIntrinsic.ConvolveSourceLine / WriteDestLine on x64 with
/// AVX and FMA, the path Paint.NET runs on current hardware: fused multiply-adds into four partial
/// sums (taps ≡ 0, 1, 2, 3 mod 4) while ≥ 4 taps remain, pairwise sums, a 2-tap step, then the last
/// tap; under 4 taps the SSE path keeps two partial sums.
fn convolve(tap: impl Fn(usize) -> [f32; 4], w: &[f32]) -> [f32; 4] {
    let fma = |acc: [f32; 4], k: usize| {
        let p = tap(k);
        std::array::from_fn(|c| p[c].mul_add(w[k], acc[c]))
    };
    let add = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|c| a[c] + b[c]);
    let (n, mut k) = (w.len(), 0);
    let mut av0;
    if n >= 4 {
        let mut ax = [[0f32; 4]; 4];
        while n - k >= 4 {
            for (j, a) in ax.iter_mut().enumerate() {
                *a = fma(*a, k + j);
            }
            k += 4;
        }
        let (mut lo, mut hi) = (add(ax[0], ax[2]), add(ax[1], ax[3]));
        if n - k >= 2 {
            lo = fma(lo, k);
            hi = fma(hi, k + 1);
            k += 2;
        }
        av0 = add(lo, hi);
    } else {
        let (mut a0, mut a1) = ([0f32; 4], [0f32; 4]);
        while n - k >= 2 {
            a0 = fma(a0, k);
            a1 = fma(a1, k + 1);
            k += 2;
        }
        av0 = add(a0, a1);
    }
    if k < n {
        av0 = fma(av0, k);
    }
    av0
}

/// MagicScaler's separable resample (ConvolutionTransform.cs, MagicTransforms.cs ·
/// AddHighQualityScaler): horizontal pass over each source row, then the vertical pass, f32
/// premultiplied. An axis that keeps its size gets MagicScaler's point sampler, whose map is the
/// identity, so that pass is skipped (exact). `src_row(y, line)` fills source row y (w pixels);
/// `emit(y, line)` receives output row y (ow pixels) in order. Only the ow×h horizontal buffer is
/// held whole. Sizes must be non-zero.
// calibrate: B-F3 — this follows MagicScaler's ProcessImageSettings pipeline with HybridScaleMode
// Off. That pipeline sharpens after the resample by default (Sharpen = true →
// MagicTransforms.AddUnsharpMask with SettingUtil.GetDefaultUnsharpMask(ratio)); whether Paint.NET
// disables it is unconfirmed, so no unsharp mask is applied. If Paint.NET instead calls
// Experimental.TransformFactory.CreateScaleTransform: (1) Adaptive's ratio becomes min(dst/src)
// rather than min(src/dst); (2) no unsharp mask is applied (as here); (3) a kept axis is convolved
// with the same interpolator instead of the point sampler, which changes Bicubic (Smooth)
// (B-spline weights 1/6, 2/3, 1/6 at integer offsets) where the others stay (near) identity.
pub(crate) fn scale_rows(
    size: (usize, usize),
    out: (usize, usize),
    kernel: Kernel,
    src_row: impl FnMut(usize, &mut [[f32; 4]]),
    emit: impl FnMut(usize, &[[f32; 4]]),
) {
    RowScaler::new(size, out, kernel).step(usize::MAX, src_row, emit);
}

/// `scale_rows` as a resumable job (the web build scales thumbnails a few rows per frame): the
/// source rows go through the horizontal pass, then the output rows come out of the vertical
/// pass, in order; `step` runs at most `rows` of those row passes. The same code runs in the same
/// order however the work is split, so every split gives `scale_rows`'s output.
pub(crate) struct RowScaler {
    size: (usize, usize),
    out: (usize, usize),
    /// The axes' maps; `None` keeps that axis (MagicScaler's identity point sampler).
    hmap: Option<KernelMap>,
    vmap: Option<KernelMap>,
    /// The ow×h horizontal-pass buffer and one working line.
    horiz: Vec<[f32; 4]>,
    line: Vec<[f32; 4]>,
    src_y: usize,
    out_y: usize,
}

impl RowScaler {
    /// Sizes must be non-zero.
    pub(crate) fn new((w, h): (usize, usize), (ow, oh): (usize, usize), kernel: Kernel) -> Self {
        let interp = interp(kernel, (w as f64 / ow as f64).min(h as f64 / oh as f64));
        RowScaler {
            size: (w, h),
            out: (ow, oh),
            hmap: (ow != w).then(|| kernel_map(w, ow, interp)),
            vmap: (oh != h).then(|| kernel_map(h, oh, interp)),
            horiz: vec![[0f32; 4]; size(ow as u32, h as u32, 1)],
            line: vec![[0f32; 4]; w.max(ow)],
            src_y: 0,
            out_y: 0,
        }
    }

    /// Runs at most `rows` row passes: `src_row(y, line)` fills source row y (w pixels),
    /// `emit(y, line)` receives output row y (ow pixels). True once every output row is out.
    pub(crate) fn step(
        &mut self,
        mut rows: usize,
        mut src_row: impl FnMut(usize, &mut [[f32; 4]]),
        mut emit: impl FnMut(usize, &[[f32; 4]]),
    ) -> bool {
        let ((w, h), (ow, oh)) = (self.size, self.out);
        let RowScaler {
            hmap,
            vmap,
            horiz,
            line,
            src_y,
            out_y,
            ..
        } = self;
        while rows > 0 && *src_y < h {
            let out = &mut horiz[*src_y * ow..][..ow];
            match hmap {
                None => src_row(*src_y, out),
                Some(m) => {
                    let line = &mut line[..w];
                    src_row(*src_y, line);
                    for (x, o) in out.iter_mut().enumerate() {
                        *o = convolve(|k| line[m.start[x] + k], m.weights(x));
                    }
                }
            }
            *src_y += 1;
            rows -= 1;
        }
        while rows > 0 && *src_y == h && *out_y < oh {
            let y = *out_y;
            match vmap {
                None => emit(y, &horiz[y * ow..][..ow]),
                Some(m) => {
                    let line = &mut line[..ow];
                    let s = m.start[y];
                    for (x, o) in line.iter_mut().enumerate() {
                        *o = convolve(|k| horiz[(s + k) * ow + x], m.weights(y));
                    }
                    emit(y, line);
                }
            }
            *out_y += 1;
            rows -= 1;
        }
        *out_y == oh
    }
}

/// `scale_rows` over a whole `PremulImage`. Any zero dimension gives an empty or transparent
/// result.
pub fn scale(img: &PremulImage, dw: u32, dh: u32, kernel: Kernel) -> PremulImage {
    if img.w == 0 || img.h == 0 || dw == 0 || dh == 0 {
        return PremulImage::zeroed(dw, dh);
    }
    let w = img.w as usize;
    let mut px = Vec::with_capacity(size(dw, dh, 1));
    scale_rows(
        (w, img.h as usize),
        (dw as usize, dh as usize),
        kernel,
        |y, line| line.copy_from_slice(&img.px[y * w..][..w]),
        |_, line| px.extend_from_slice(line),
    );
    PremulImage { w: dw, h: dh, px }
}

const UQ15_ONE: usize = 1 << 15;

/// LookupTables.cs · gammaTable (1023-step float sRGB encode) → MakeUQ15Gamma: linear UQ15
/// (0..=32768) → sRGB byte.
static SRGB_GAMMA_UQ15: LazyLock<Vec<u8>> = LazyLock::new(|| {
    const SCALE: usize = 1023;
    let gt: Vec<f32> = (0..SCALE + 2)
        .map(|i| {
            let d = i.min(SCALE) as f64 / SCALE as f64; // Fixup repeats the last valid entry
            let d = if d <= 0.04045 / 12.92 {
                d * 12.92
            } else {
                1.055 * d.powf(1.0 / 2.4) - 0.055
            };
            d as f32
        })
        .collect();
    (0..=UQ15_ONE)
        .map(|i| {
            let pos = i as f64 / UQ15_ONE as f64 * SCALE as f64;
            let idx = pos as usize;
            let (l, h) = (gt[idx] as f64, gt[idx + 1] as f64);
            let val = (h - l) * (pos - idx as f64) + l;
            ((val * 255.0 + 0.5) as i32).clamp(0, 255) as u8
        })
        .collect()
});

/// One premultiplied f32 pixel → straight 8-bit: ConvertersLinear.cs ·
/// ConverterFromLinear<float, byte>.Converter3A (Linear, through `SRGB_GAMMA_UQ15`) and
/// ConvertersFloat.cs · FloatConverter.NarrowingImpl3A (Companded), scalar paths.
// calibrate: B-F3 — the AVX2 paths Paint.NET takes for rows ≥ 8 px multiply by `rcpps` (an
// approximate reciprocal) and round half-to-even; the scalar paths divide exactly and truncate
// x + 0.5. Kept at the brief's ±1 until the capture decides.
pub(crate) fn narrow(p: [f32; 4], gamma: Gamma) -> [u8; 4] {
    const FMAX: f32 = 255.0;
    const FROUND: f32 = 0.5;
    let a = p[3];
    if a < FROUND / FMAX {
        return [0; 4];
    }
    let byte = |v: f32| ((v + FROUND) as i32).clamp(0, 255) as u8;
    let o3 = byte(a * FMAX);
    let c = |v: f32| match gamma {
        Gamma::Linear => {
            let i = ((v * (UQ15_ONE as f32 / a) + FROUND) as i32).clamp(0, UQ15_ONE as i32);
            SRGB_GAMMA_UQ15[i as usize]
        }
        Gamma::Companded => byte(v * (FMAX / a)),
    };
    [c(p[0]), c(p[1]), c(p[2]), o3]
}
