//! f32 RGBA working surfaces: the effect input/output conversions, edge fetch, texel taps and the
//! separable convolution.
//! Paint.NET's GPU pixel path and quantizer, conversion order, edge extension, bilinear at texel
//! centres and the Gaussian Blur's H then V passes.

use crate::canvas::ImageData;
use crate::pixel::gamma::{SRGB8_TO_LINEAR, linear_to_srgb, quantize};
use crate::render::display::for_bands;

/// The working colour space: raw sRGB values (k/255) or linear light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Srgb,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alpha {
    Straight,
    Premultiplied,
}

/// F2 edge extension outside the layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Clamp,
    /// Duplicates the edge texel: −1 → 0, −2 → 1, W → W − 1.
    Mirror,
    Wrap,
    Transparent,
}

/// A W×H float RGBA image, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSurface {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[f32; 4]>,
}

pub fn premultiply(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// rgb/a; a ≤ 0 or NaN gives transparent black (§2.4 "Consequences").
pub fn unpremultiply(c: [f32; 4]) -> [f32; 4] {
    let a = c[3];
    if a.is_nan() || a <= 0.0 {
        return [0.0; 4];
    }
    [c[0] / a, c[1] / a, c[2] / a, a]
}

/// Y = 0.299r + 0.587g + 0.114b in f32, left to right, no FMA.
pub fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299_f32 * r + 0.587_f32 * g + 0.114_f32 * b
}

impl EffectSurface {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![[0.0; 4]; w as usize * h as usize],
        }
    }

    /// Fills every pixel with `f(x, y)`; rows run in parallel on native targets.
    pub fn from_fn(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4] + Sync) -> Self {
        let mut s = Self::new(w, h);
        for_bands(&mut s.px, w as usize, 0..h as usize, 1, |y, row| {
            for (x, p) in row.iter_mut().enumerate() {
                *p = f(x as u32, y as u32);
            }
        });
        s
    }

    /// F1 input: k/255 (or the exact linear value of k), then premultiplied in the working space.
    pub fn from_rgba8(img: &ImageData, space: Space, alpha: Alpha) -> Self {
        let c = |k: u8| match space {
            Space::Srgb => k as f32 / 255.0,
            Space::Linear => SRGB8_TO_LINEAR[k as usize],
        };
        let px = img
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let v = [c(p[0]), c(p[1]), c(p[2]), p[3] as f32 / 255.0];
                match alpha {
                    Alpha::Straight => v,
                    Alpha::Premultiplied => premultiply(v),
                }
            })
            .collect();
        Self {
            w: img.width,
            h: img.height,
            px,
        }
    }

    /// F1 output: unpremultiply, linear → sRGB, then q() per channel (§2.4 "Quantize").
    pub fn to_rgba8(&self, space: Space, alpha: Alpha) -> ImageData {
        let mut out = ImageData::new(self.w, self.h);
        let w = self.w as usize;
        for_bands(&mut out.data, w * 4, 0..self.h as usize, 1, |y, row| {
            for (x, o) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let mut p = self.px[y * w + x];
                if alpha == Alpha::Premultiplied {
                    p = unpremultiply(p);
                }
                if space == Space::Linear {
                    for c in &mut p[..3] {
                        *c = linear_to_srgb(*c as f64) as f32;
                    }
                }
                *o = p.map(quantize);
            }
        });
        out
    }

    pub fn at(&self, x: u32, y: u32) -> [f32; 4] {
        self.px[y as usize * self.w as usize + x as usize]
    }

    /// F2: texel (x, y), extended past the layer by `edge`.
    pub fn fetch(&self, x: i32, y: i32, edge: Edge) -> [f32; 4] {
        let map = |i: i32, n: i32| match edge {
            Edge::Clamp => Some(i.clamp(0, n - 1)),
            Edge::Wrap => Some(i.rem_euclid(n)),
            Edge::Mirror => {
                let m = i.rem_euclid(2 * n);
                Some(if m < n { m } else { 2 * n - 1 - m })
            }
            Edge::Transparent => (0..n).contains(&i).then_some(i),
        };
        match (map(x, self.w as i32), map(y, self.h as i32)) {
            (Some(x), Some(y)) => self.at(x as u32, y as u32),
            _ => [0.0; 4],
        }
    }

    /// Bilinear tap at scene position (sx, sy), texel centres at i + ½; the four taps are summed
    /// in the order (i₀, j₀), (i₀+1, j₀), (i₀, j₀+1), (i₀+1, j₀+1).
    pub fn bilinear(&self, sx: f32, sy: f32, edge: Edge) -> [f32; 4] {
        let (u, v) = (sx - 0.5, sy - 0.5);
        let (i0, j0) = (u.floor(), v.floor());
        let (f, g) = (u - i0, v - j0);
        let (i, j) = (i0 as i32, j0 as i32);
        let taps = [
            (i, j, (1.0 - f) * (1.0 - g)),
            (i + 1, j, f * (1.0 - g)),
            (i, j + 1, (1.0 - f) * g),
            (i + 1, j + 1, f * g),
        ];
        let mut out = [0.0; 4];
        for (x, y, wt) in taps {
            for (o, c) in out.iter_mut().zip(self.fetch(x, y, edge)) {
                *o += wt * c;
            }
        }
        out
    }

    /// Nearest tap: texel (⌊sx⌋, ⌊sy⌋).
    pub fn point(&self, sx: f32, sy: f32, edge: Edge) -> [f32; 4] {
        self.fetch(sx.floor() as i32, sy.floor() as i32, edge)
    }

    /// Separable convolution with an odd `kernel` (R = (len − 1)/2): out(x) = Σⱼ wⱼ·in(x + j − R)
    /// accumulated in j order in f32, horizontal pass first, then vertical on its result.
    pub fn convolve_hv(&self, kernel: &[f32], edge: Edge) -> EffectSurface {
        let r = (kernel.len() as i32 - 1) / 2;
        let pass = |src: &EffectSurface, dx: i32, dy: i32| {
            EffectSurface::from_fn(src.w, src.h, |x, y| {
                let mut acc = [0.0f32; 4];
                for (j, &k) in kernel.iter().enumerate() {
                    let o = j as i32 - r;
                    let p = src.fetch(x as i32 + o * dx, y as i32 + o * dy, edge);
                    for (a, c) in acc.iter_mut().zip(p) {
                        *a += k * c;
                    }
                }
                acc
            })
        };
        pass(&pass(self, 1, 0), 0, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;

    fn all_pairs(alpha_from: u32) -> ImageData {
        let mut data = vec![];
        for a in alpha_from..=255u32 {
            for c in 0..=255u32 {
                data.extend([c as u8, (255 - c) as u8, (c / 2) as u8, a as u8]);
            }
        }
        ImageData {
            width: 256,
            height: 256 - alpha_from,
            data,
        }
    }

    /// §2.4: the in/out conversions round-trip every straight (c, a) through q().
    #[test]
    fn identity_round_trip_is_lossless() {
        for space in [Space::Srgb, Space::Linear] {
            for alpha in [Alpha::Straight, Alpha::Premultiplied] {
                let img = all_pairs(1);
                let back = EffectSurface::from_rgba8(&img, space, alpha).to_rgba8(space, alpha);
                assert!(back.data == img.data, "{space:?} {alpha:?}");
            }
            let img = all_pairs(0);
            assert!(
                EffectSurface::from_rgba8(&img, space, Alpha::Straight)
                    .to_rgba8(space, Alpha::Straight)
                    .data
                    == img.data
            );
        }
    }

    /// §2.4 "Consequences": premultiplied effects give RGB 0 where a = 0; straight ones keep it.
    #[test]
    fn premultiplied_zero_alpha_is_transparent_black() {
        let img = ImageData {
            width: 1,
            height: 1,
            data: vec![90, 180, 250, 0],
        };
        let p = EffectSurface::from_rgba8(&img, Space::Linear, Alpha::Premultiplied)
            .to_rgba8(Space::Linear, Alpha::Premultiplied);
        assert_eq!(p.data, [0, 0, 0, 0]);
        let s = EffectSurface::from_rgba8(&img, Space::Srgb, Alpha::Straight)
            .to_rgba8(Space::Srgb, Alpha::Straight);
        assert_eq!(s.data, [90, 180, 250, 0]);
    }

    /// effects-1.md F2: Mirror duplicates the edge texel (−1 → 0, −2 → 1, W → W − 1).
    #[test]
    fn edge_fetch() {
        let s = EffectSurface {
            w: 3,
            h: 1,
            px: vec![
                [0.1, 0.0, 0.0, 1.0],
                [0.2, 0.0, 0.0, 1.0],
                [0.3, 0.0, 0.0, 1.0],
            ],
        };
        let r = |x: i32, e: Edge| s.fetch(x, 0, e)[0];
        assert_eq!([r(-1, Edge::Clamp), r(3, Edge::Clamp)], [0.1, 0.3]);
        assert_eq!(
            [
                r(-1, Edge::Mirror),
                r(-2, Edge::Mirror),
                r(3, Edge::Mirror),
                r(4, Edge::Mirror)
            ],
            [0.1, 0.2, 0.3, 0.2]
        );
        assert_eq!([r(-1, Edge::Wrap), r(3, Edge::Wrap)], [0.3, 0.1]);
        assert_eq!(s.fetch(-1, 0, Edge::Transparent), [0.0; 4]);
    }

    /// Texel centres sit at i + ½ (effects-1.md notation; F4 step 3).
    #[test]
    fn bilinear_taps_texel_centres() {
        let s = EffectSurface {
            w: 2,
            h: 1,
            px: vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]],
        };
        assert_eq!(s.bilinear(0.5, 0.5, Edge::Clamp)[0], 0.0);
        assert_eq!(s.bilinear(1.0, 0.5, Edge::Clamp)[0], 0.5);
        assert_eq!(s.bilinear(1.25, 0.5, Edge::Clamp)[0], 0.75);
        assert_eq!(s.point(1.9, 0.5, Edge::Clamp)[0], 1.0);
    }

    #[test]
    fn luma_is_f32_left_to_right() {
        // adjustments-and-effect-framework.md notation Y; §5 Task 38 Black and White.
        let y = luma(200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0);
        assert_eq!(crate::pixel::gamma::quantize(y), 124);
    }

    /// Native rows run on rayon (spec §6.4); the output must not depend on the thread count.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_rows_equal_sequential() {
        let f = |x: u32, y: u32| [((x * 7 + y * 3) % 11) as f32 / 11.0, 0.5, 0.25, 1.0];
        let par = EffectSurface::from_fn(97, 61, f);
        let one = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let seq = one.install(|| EffectSurface::from_fn(97, 61, f));
        assert_eq!(par, seq);
    }

    /// effects-1.md §2.3 step 5: out(x) = Σⱼ wⱼ·in(x + j − R), H then V, through the F2 edge.
    #[test]
    fn convolve_hv_taps_and_edges() {
        let dot = |x: u32, y: u32| {
            EffectSurface::from_fn(3, 3, move |i, j| [((i, j) == (x, y)) as u8 as f32; 4])
        };
        let k = [0.25, 0.5, 0.25];
        let m = dot(0, 0).convolve_hv(&k, Edge::Mirror);
        let r = |x, y| m.at(x, y)[0];
        assert_eq!(
            [r(0, 0), r(1, 0), r(1, 1), r(2, 2)],
            [0.5625, 0.1875, 0.0625, 0.0]
        );
        let t = dot(0, 0).convolve_hv(&k, Edge::Transparent);
        assert_eq!(t.at(0, 0)[0], 0.25);
        let shifted = dot(1, 1).convolve_hv(&[0.0, 0.0, 1.0], Edge::Clamp);
        assert_eq!((shifted.at(0, 0)[0], shifted.at(1, 1)[0]), (1.0, 0.0));
    }
}
