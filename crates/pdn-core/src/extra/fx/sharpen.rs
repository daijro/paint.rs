//! Ported from PhotoCraft crates/algo/src/sharpen.rs and sharpen/exact.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: kernels work on a whole interleaved straight RGBA f32 layer (`w`, `h`) instead of `Image`/`Rect`/`Ctx`, with zero extension past the edges; one tap-major sequential Gaussian (`blur`, the same tap order as upstream's `conv_sep`/`exact::run`) replaces both them and `sharpen/fft.rs` (no rustfft); rows run through `extra::par`; `Interrupt` is polled per row; the reference, fft and fft_bench modules are dropped; above σ 32 the blur is a three-box cascade (Ruling R88: O(1) a pixel whatever the radius) where upstream took its FFT path, with zero outside the layer as the direct kernel has, so the two meet at σ 32 without a jump at the edges; the unit tests are new.

use super::util::gauss_box_radii;
use crate::extra::interrupt::Interrupt;
use crate::extra::par::rows_mut;

/// Above this σ the blur is the three-box cascade (Ruling R88); up to it the direct kernel, so
/// small radii keep their exact output.
const BOX_SIGMA: f32 = 32.0;

fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    if sigma < 0.05 {
        return vec![1.0];
    }
    let r = (sigma * 3.0).ceil() as i32;
    let k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let s: f32 = k.iter().sum();
    k.into_iter().map(|v| v / s).collect()
}

/// Gaussian blur of the layer, premultiplied internally; straight RGBA back. `None` once
/// cancelled.
fn blur(buf: &[f32], w: usize, h: usize, sigma: f32, ctl: &Interrupt) -> Option<Vec<f32>> {
    let mut p = buf.to_vec();
    for px in p.as_chunks_mut::<4>().0 {
        let a = px[3];
        px[..3].iter_mut().for_each(|v| *v *= a);
    }
    let mut res = if sigma > BOX_SIGMA {
        cascade(&mut p, w, h, sigma, ctl).then_some(p)?
    } else {
        direct(&p, w, h, sigma, ctl)?
    };
    for px in res.as_chunks_mut::<4>().0 {
        let a = px[3];
        px[..3]
            .iter_mut()
            .for_each(|v| *v = if a > 1e-7 { *v / a } else { 0.0 });
    }
    Some(res)
}

/// Running-sum box blur (radius `r`) of `line` in place, zero beyond its ends, through
/// `scratch`. f64 sums, as the Blur Gallery's clamped passes.
fn box_zero(line: &mut [f32], r: usize, scratch: &mut Vec<f32>) {
    let n = line.len();
    // scratch[k] = line[k - r - 1], zero around it.
    scratch.clear();
    scratch.resize(r + 1, 0.0);
    scratch.extend_from_slice(line);
    scratch.resize(n + 2 * r + 2, 0.0);
    let norm = 1.0 / (2 * r + 1) as f64;
    let mut acc: f64 = scratch[1..2 * r + 2].iter().map(|&v| f64::from(v)).sum();
    for (x, v) in line.iter_mut().enumerate() {
        *v = (acc * norm) as f32;
        acc += f64::from(scratch[x + 2 * r + 2]) - f64::from(scratch[x + 1]);
    }
}

/// The three box passes of one channel line, zero outside it. They run in a copy padded with
/// zeros past the first pass's reach (and the last's), so what spreads out of the line between
/// passes is kept: the result is the cascade's exact convolution of the zero-extended line.
fn line_zero(
    line: &mut [f32],
    radii: [usize; 3],
    pad: usize,
    buf: &mut Vec<f32>,
    s: &mut Vec<f32>,
) {
    buf.clear();
    buf.resize(pad, 0.0);
    buf.extend_from_slice(line);
    buf.resize(line.len() + 2 * pad, 0.0);
    for r in radii {
        box_zero(buf, r, s);
    }
    line.copy_from_slice(&buf[pad..pad + line.len()]);
}

/// Ruling R88's three-box Gaussian of the premultiplied layer `p` (O(1) a pixel), reading zero
/// outside the layer as `direct` does, so un-premultiplying normalises over the layer the same
/// way. Rows in parallel, columns serial; `false` once cancelled.
fn cascade(p: &mut [f32], w: usize, h: usize, sigma: f32, ctl: &Interrupt) -> bool {
    let radii = gauss_box_radii(sigma);
    // Past the first pass's reach a pass reads true zeros; the last reads within its radius.
    let pad = radii[0].max(radii[2]) + 1;
    rows_mut(p, w * 4, |_, row| {
        if ctl.cancelled() {
            return;
        }
        let (mut line, mut buf, mut s) = (vec![0.0; w], vec![], vec![]);
        for c in 0..4 {
            for (x, v) in line.iter_mut().enumerate() {
                *v = row[x * 4 + c];
            }
            line_zero(&mut line, radii, pad, &mut buf, &mut s);
            for (x, v) in line.iter().enumerate() {
                row[x * 4 + c] = *v;
            }
        }
    });
    let (mut line, mut buf, mut s) = (vec![0.0; h], vec![], vec![]);
    for x in 0..w {
        if ctl.cancelled() {
            return false;
        }
        for c in 0..4 {
            for (y, v) in line.iter_mut().enumerate() {
                *v = p[(y * w + x) * 4 + c];
            }
            line_zero(&mut line, radii, pad, &mut buf, &mut s);
            for (y, v) in line.iter().enumerate() {
                p[(y * w + x) * 4 + c] = *v;
            }
        }
    }
    !ctl.cancelled()
}

/// The exact separable Gaussian of the premultiplied layer `p`, tap-major; `None` once cancelled.
fn direct(p: &[f32], w: usize, h: usize, sigma: f32, ctl: &Interrupt) -> Option<Vec<f32>> {
    let k = gaussian_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let stride = w * 4;
    let mut tmp = vec![0.0f32; p.len()];
    rows_mut(&mut tmp, stride, |y, dst| {
        if ctl.cancelled() {
            return;
        }
        let row = &p[y * stride..(y + 1) * stride];
        for (i, wt) in k.iter().enumerate() {
            let dx = i as isize - r;
            let (lo, hi) = (
                (-dx).max(0) as usize,
                (w as isize - dx.max(0)).max(0) as usize,
            );
            for x in lo..hi.max(lo) {
                let s = (x as isize + dx) as usize * 4;
                for c in 0..4 {
                    dst[x * 4 + c] += row[s + c] * wt;
                }
            }
        }
    });
    let mut res = vec![0.0f32; p.len()];
    rows_mut(&mut res, stride, |y, dst| {
        if ctl.cancelled() {
            return;
        }
        for (i, wt) in k.iter().enumerate() {
            let sy = y as isize + i as isize - r;
            if sy < 0 || sy >= h as isize {
                continue;
            }
            let s = &tmp[sy as usize * stride..(sy as usize + 1) * stride];
            for (d, v) in dst.iter_mut().zip(s) {
                *d += v * wt;
            }
        }
    });
    if ctl.cancelled() {
        return None;
    }
    Some(res)
}

/// Colour channels through `f(original, blurred)`; transparent pixels and alpha are kept.
fn with_blur(
    buf: &[f32],
    w: usize,
    h: usize,
    radius: f32,
    ctl: &Interrupt,
    f: impl Fn(f32, f32) -> f32,
) -> Option<Vec<f32>> {
    let blurred = blur(buf, w, h, radius, ctl)?;
    let mut out = buf.to_vec();
    for (px, bl) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(blurred.as_chunks::<4>().0)
    {
        if px[3] <= 0.0 {
            continue;
        }
        for c in 0..3 {
            px[c] = f(px[c], bl[c]);
        }
    }
    Some(out)
}

/// Unsharp Mask: amount in percent, radius (Gaussian sigma) in px, threshold 0–255.
pub fn unsharp(
    buf: &[f32],
    w: usize,
    h: usize,
    amount: f32,
    radius: f32,
    threshold: f32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let (k, t) = (amount / 100.0, threshold / 255.0);
    with_blur(buf, w, h, radius, ctl, |o, b| {
        let d = o - b;
        if d.abs() + 1e-6 >= t { o + k * d } else { o }
    })
}

/// Smart Sharpen (upstream's basic form): amount %, radius, reduce-noise %.
pub fn smart(
    buf: &[f32],
    w: usize,
    h: usize,
    amount: f32,
    radius: f32,
    reduce_noise: f32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let k = amount / 100.0;
    let nr = (reduce_noise / 100.0).clamp(0.0, 1.0);
    with_blur(buf, w, h, radius, ctl, |o, b| {
        let d = o - b;
        // Suppress small (noise-level) differences.
        o + k * d * (1.0 - nr * (-(d.abs() * 40.0)).exp())
    })
}

/// High Pass: detail around mid grey.
pub fn high_pass(
    buf: &[f32],
    w: usize,
    h: usize,
    radius: f32,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    with_blur(buf, w, h, radius, ctl, |o, b| 0.5 + (o - b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    const N: &Interrupt = &Interrupt::NONE;

    fn flat(w: usize, h: usize, v: f32) -> Vec<f32> {
        [v, v, v, 1.0].repeat(w * h)
    }

    /// Blurring a flat opaque layer leaves its colour, edges included (alpha is in the blur).
    #[test]
    fn blur_of_flat_is_flat() {
        let out = blur(&flat(9, 7, 0.4), 9, 7, 2.0, N).unwrap();
        assert!(
            out.as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3].iter().all(|v| (v - 0.4).abs() < 1e-5))
        );
    }

    /// The bright side of a step gets brighter, the dark side darker.
    #[test]
    fn unsharp_overshoots_both_sides_of_an_edge() {
        let step: Vec<f32> = (0..16)
            .flat_map(|x| {
                let v = if x < 8 { 0.3 } else { 0.7 };
                [v, v, v, 1.0]
            })
            .collect();
        let out = unsharp(&step, 16, 1, 100.0, 1.5, 0.0, N).unwrap();
        assert!(out[7 * 4] < 0.3 && out[8 * 4] > 0.7);
        assert_eq!(out[3], 1.0);
    }

    #[test]
    fn cancelled_returns_none() {
        let flag = AtomicBool::new(true);
        assert!(high_pass(&flat(4, 4, 0.5), 4, 4, 1.0, &Interrupt(Some(&flag))).is_none());
    }

    /// A `w` × `h` layer of varied colour, alpha 0.25 to 1.
    fn noise(w: usize, h: usize) -> Vec<f32> {
        let mut s = 0x9e37_79b9_u64;
        let mut r = move || (crate::extra::rng::splitmix64(&mut s) >> 40) as f32 / 16_777_216.0;
        (0..w * h)
            .flat_map(|_| [r(), r(), r(), 0.25 + 0.75 * r()])
            .collect()
    }

    /// Every bit of `v`, folded (FNV-1a over the f32 bit patterns).
    fn digest(v: &[f32]) -> u64 {
        v.iter().fold(0xcbf2_9ce4_8422_2325, |h, x| {
            (h ^ u64::from(x.to_bits())).wrapping_mul(0x100_0000_01b3)
        })
    }

    /// Ruling R88: up to σ 32 the direct kernel runs as before, bit for bit (digests taken
    /// before the cascade was added).
    #[test]
    fn small_radii_are_unchanged() {
        let src = noise(40, 30);
        let got = [
            digest(&unsharp(&src, 40, 30, 150.0, 5.0, 0.0, N).unwrap()),
            digest(&unsharp(&src, 40, 30, 80.0, 32.0, 10.0, N).unwrap()),
            digest(&high_pass(&src, 40, 30, 32.0, N).unwrap()),
        ];
        assert_eq!(
            got,
            [
                5_758_631_492_380_064_091,
                12_335_686_524_503_257_090,
                15_856_426_332_455_453_439
            ]
        );
    }

    /// N1: the blur does not jump where it switches to the cascade. Along every edge of a layer
    /// with 1 level/px ramps (and a 20 px dark border), σ 32 (direct) and σ 32.5 (cascade) give
    /// colours within 3 levels; a cascade clamped at the edges, where the direct kernel reads
    /// zero, differs by about 7. (The blurred alpha is not compared: the sharpeners keep the
    /// layer's.)
    #[test]
    fn blur_is_continuous_across_the_cascade_switch() {
        let n = 160;
        let src: Vec<f32> = (0..n * n)
            .flat_map(|i| {
                let (x, y) = ((i % n) as f32, (i / n) as f32);
                let b = if x < 20.0 { 0.0 } else { x / 255.0 };
                [x / 255.0, y / 255.0, b, 1.0]
            })
            .collect();
        let a = blur(&src, n, n, 32.0, N).unwrap();
        let b = blur(&src, n, n, 32.5, N).unwrap();
        let worst = a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .flat_map(|(p, q)| (0..3).map(move |c| (p[c] - q[c]).abs() * 255.0))
            .fold(0.0f32, f32::max);
        assert!(worst <= 3.0, "{worst:.2} levels apart");
    }

    /// Ruling R88: above σ 32 the blur is the three-box cascade with zero outside the layer: at
    /// σ 40 it matches tone's cascade run over planes padded with zeros past its reach (to float
    /// rounding), and radius 1000 at 512 × 512, run serially as on wasm, stays well inside a
    /// generous bound.
    #[test]
    fn huge_radius_is_the_box_cascade_and_bounded() {
        let big = noise(512, 512);
        let serial = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        serial.install(|| unsharp(&big, 512, 512, 100.0, 1000.0, 0.0, N).unwrap());
        let secs = start.elapsed().as_secs_f64();
        assert!(secs < 4.0, "radius 1000 at 512 x 512 took {secs:.1} s");
        let (w, h, sigma) = (48, 40, 40.0);
        let src = noise(w, h);
        let pad: usize = gauss_box_radii(sigma).iter().sum();
        let (pw, ph) = (w + 2 * pad, h + 2 * pad);
        let planes: Vec<Vec<f32>> = (0..4)
            .map(|c| {
                let mut plane = vec![0.0; pw * ph];
                for y in 0..h {
                    for x in 0..w {
                        let p = &src[(y * w + x) * 4..][..4];
                        plane[(y + pad) * pw + x + pad] = if c < 3 { p[c] * p[3] } else { p[3] };
                    }
                }
                crate::extra::fx::tone::gauss_blur(&mut plane, pw, ph, sigma);
                plane
            })
            .collect();
        let out = high_pass(&src, w, h, sigma, N).unwrap();
        for y in 0..h {
            for x in 0..w {
                let (i, j) = (y * w + x, (y + pad) * pw + x + pad);
                for c in 0..3 {
                    let want = 0.5 + (src[i * 4 + c] - planes[c][j] / planes[3][j]);
                    let got = out[i * 4 + c];
                    assert!(
                        (got - want).abs() < 1e-5,
                        "{x},{y} channel {c}: {got} {want}"
                    );
                }
            }
        }
    }
}
