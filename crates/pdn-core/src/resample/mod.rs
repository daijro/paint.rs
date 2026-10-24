//! Paint.NET's resamplers and the
//! PhotoSauce.MagicScaler 0.15.0 port they delegate to (R18). Every float scaler works on
//! premultiplied f32 RGBA, in linear light when `Gamma::Linear`.
//! Mode ids: paintdotjs
//! `js/src/interface/window/dialog/ImageSizeDialog.js`.
pub mod magic;
pub mod pdn;

use crate::pixel::gamma::SRGB8_TO_LINEAR;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gamma {
    /// "Use gamma correction" on (the default): sRGB → linear before scaling.
    Linear,
    Companded,
}

/// Image ▸ Resize choices, with the paintdotjs ids (`ImageSizeDialog.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeMode {
    AdaptiveSharp,      // "adaptiveHighQuality"
    Bicubic,            // "cubic" — MagicScaler Catmull-Rom
    BicubicSmooth,      // "cubicSmooth" — MagicScaler B-spline
    Bilinear,           // "linear" — MagicScaler triangle
    BilinearLowQuality, // "linearLowQuality" — Paint.NET's own
    Lanczos3,           // "lanczos3"
    Fant,               // "fant" — Paint.NET's own
    NearestNeighbor,    // "nearestNeighbor" — Paint.NET's own, raw BGRA
}

impl ResizeMode {
    pub const ALL: [ResizeMode; 8] = [
        ResizeMode::AdaptiveSharp,
        ResizeMode::Bicubic,
        ResizeMode::BicubicSmooth,
        ResizeMode::Bilinear,
        ResizeMode::BilinearLowQuality,
        ResizeMode::Lanczos3,
        ResizeMode::Fant,
        ResizeMode::NearestNeighbor,
    ];

    pub fn value(self) -> &'static str {
        match self {
            ResizeMode::AdaptiveSharp => "adaptiveHighQuality",
            ResizeMode::Bicubic => "cubic",
            ResizeMode::BicubicSmooth => "cubicSmooth",
            ResizeMode::Bilinear => "linear",
            ResizeMode::BilinearLowQuality => "linearLowQuality",
            ResizeMode::Lanczos3 => "lanczos3",
            ResizeMode::Fant => "fant",
            ResizeMode::NearestNeighbor => "nearestNeighbor",
        }
    }

    pub fn from_value(v: &str) -> Option<ResizeMode> {
        Self::ALL.into_iter().find(|m| m.value() == v)
    }
}

/// MagicScaler interpolators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kernel {
    Linear,
    CatmullRom,
    CubicSmooth,
    Lanczos3,
    /// MagicScaler's default interpolation with hybrid scaling off ("Adaptive (Sharp)").
    Adaptive,
    /// MagicScaler `InterpolationSettings.Average` (box): the canvas mips when the size divides
    /// evenly (canvas-rendering.md §2.2 step 4(c)).
    Average,
}

/// Premultiplied f32 RGBA; channels in linear light or companded per `Gamma`.
#[derive(Clone, Debug, PartialEq)]
pub struct PremulImage {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[f32; 4]>,
}

/// MagicScaler `LookupTables.Alpha`: `f32(f64(i) / 255)`.
fn unit(b: u8) -> f32 {
    (b as f64 / 255.0) as f32
}

/// w·h·per, checked: `None` when it overflows usize (possible on 32-bit targets such as wasm32).
fn len(w: u32, h: u32, per: usize) -> Option<usize> {
    (w as usize).checked_mul(h as usize)?.checked_mul(per)
}

/// `len` for buffers that are known to fit; panics with a clear message otherwise. `resize` checks
/// its sizes first, so it never reaches the panic.
fn size(w: u32, h: u32, per: usize) -> usize {
    len(w, h, per).expect("resample: buffer size overflows usize")
}

/// §2.2 step 5: L = f32(srgb_to_linear(c/255)) (the 256-entry table) for Linear, c/255 for
/// Companded; a = A/255; premultiply in f32. The Companded side is MagicScaler's
/// `FloatConverter.WideningImpl3A` (scalar path, `Magic/Processors/ConvertersFloat.cs`).
// calibrate: B-F3 — that converter's AVX2 path (rows ≥ 8 px) computes c·(1f/255), 1 ulp off
// the table for 126 of the 256 values; the brief's c/255 is kept until the capture decides.
pub(crate) fn widen([r, g, b, a]: [u8; 4], gamma: Gamma) -> [f32; 4] {
    let c = |v: u8| match gamma {
        Gamma::Linear => SRGB8_TO_LINEAR[v as usize],
        Gamma::Companded => unit(v),
    };
    let a = unit(a);
    [c(r) * a, c(g) * a, c(b) * a, a]
}

impl PremulImage {
    /// w×h transparent black (empty when either is 0).
    fn zeroed(w: u32, h: u32) -> PremulImage {
        PremulImage {
            w,
            h,
            px: vec![[0.0; 4]; size(w, h, 1)],
        }
    }

    /// Straight RGBA8 → premultiplied f32 (`widen`, §2.2 step 5).
    pub fn from_rgba8(src: &[u8], w: u32, h: u32, gamma: Gamma) -> PremulImage {
        let px = src[..size(w, h, 4)]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&p| widen(p, gamma))
            .collect();
        PremulImage { w, h, px }
    }

    /// The MagicScaler conversion back: un-premultiply (a < ½/255 → transparent black), linear →
    /// sRGB when Linear, quantize with MagicScaler's rounding (`magic::narrow`).
    pub fn to_rgba8(&self, gamma: Gamma) -> Vec<u8> {
        self.px
            .iter()
            .flat_map(|&p| magic::narrow(p, gamma))
            .collect()
    }
}

/// `resize`'s precondition: a `w × h` source of `src_len` bytes holds w·h·4 bytes and every
/// working buffer (at most max(w, dw)·max(h, dh) f32 pixels) is addressable.
pub(crate) fn accepts(src_len: usize, w: u32, h: u32, dw: u32, dh: u32) -> bool {
    let fits = len(w.max(dw), h.max(dh), 16).is_some_and(|b| b <= isize::MAX as usize);
    fits && len(w, h, 4).is_some_and(|n| src_len >= n)
}

/// Straight RGBA8 → straight RGBA8. Same size returns `src` unchanged for every mode.
///
/// Precondition: `src` holds w·h·4 bytes and every working buffer (at most
/// max(w, dw)·max(h, dh) f32 pixels) is addressable. Otherwise this returns an empty `Vec`
/// rather than panicking. Sizes that are addressable but exceed memory still abort on
/// allocation, as any Rust allocation does, so Image ▸ Resize (`io::ops::resize_image`) reserves
/// the output plus `scratch_bytes` fallibly first.
/// Every float mode streams: source rows are widened as the scaler asks for them and output rows
/// narrowed straight into the result. Bilinear (Low Quality) and Fant hold a few rows; the five
/// MagicScaler modes hold the dw×h horizontal pass.
pub fn resize(
    src: &[u8],
    w: u32,
    h: u32,
    dw: u32,
    dh: u32,
    mode: ResizeMode,
    gamma: Gamma,
) -> Vec<u8> {
    if (w, h) == (dw, dh) {
        return src.to_vec();
    }
    if !accepts(src.len(), w, h, dw, dh) {
        return Vec::new();
    }
    if w == 0 || h == 0 || dw == 0 || dh == 0 {
        return vec![0; size(dw, dh, 4)];
    }
    let kernel = match mode {
        ResizeMode::NearestNeighbor => return pdn::nearest_neighbor(src, w, h, dw, dh),
        ResizeMode::BilinearLowQuality | ResizeMode::Fant => None,
        ResizeMode::Bicubic => Some(Kernel::CatmullRom),
        ResizeMode::BicubicSmooth => Some(Kernel::CubicSmooth),
        ResizeMode::Bilinear => Some(Kernel::Linear),
        ResizeMode::Lanczos3 => Some(Kernel::Lanczos3),
        ResizeMode::AdaptiveSharp => Some(Kernel::Adaptive),
    };
    let row_bytes = w as usize * 4;
    let mut src_row = |y: usize, line: &mut [[f32; 4]]| {
        let row = src[y * row_bytes..][..row_bytes].as_chunks::<4>().0;
        for (p, &b) in line.iter_mut().zip(row) {
            *p = widen(b, gamma);
        }
    };
    let mut out = Vec::with_capacity(size(dw, dh, 4));
    let mut emit = |_: usize, line: &[[f32; 4]]| {
        out.extend(line.iter().flat_map(|&p| magic::narrow(p, gamma)))
    };
    let (s, d) = ((w, h), (dw, dh));
    match kernel {
        Some(k) => magic::scale_rows(
            (w as usize, h as usize),
            (dw as usize, dh as usize),
            k,
            src_row,
            emit,
        ),
        None if mode == ResizeMode::Fant => pdn::fant_rows(s, d, &mut src_row, &mut emit),
        None => pdn::linear_low_quality_rows(s, d, &mut src_row, &mut emit),
    }
    out
}

/// The working memory `resize` holds besides `src` and its output, in bytes: the MagicScaler
/// modes' dw×h f32 horizontal pass; the streamed Paint.NET scalers' few rows count as 0. `None`
/// when it overflows usize.
pub fn scratch_bytes(mode: ResizeMode, h: u32, dw: u32) -> Option<usize> {
    match mode {
        ResizeMode::NearestNeighbor | ResizeMode::BilinearLowQuality | ResizeMode::Fant => Some(0),
        _ => len(dw, h, 16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_row(v: &[u8]) -> Vec<u8> {
        v.iter().flat_map(|&g| [g, g, g, 255]).collect()
    }
    fn grays(rgba: &[u8]) -> Vec<u8> {
        rgba.chunks(4).map(|p| p[0]).collect()
    }

    #[test]
    fn nearest_neighbor_picks_pixel_centres() {
        let src: Vec<u8> = (0..23u8).flat_map(|i| [i, 0, 0, 255]).collect();
        let out = resize(
            &src,
            23,
            1,
            11,
            1,
            ResizeMode::NearestNeighbor,
            Gamma::Linear,
        );
        assert_eq!(grays(&out), [1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21]);
        let up = resize(
            &src,
            23,
            1,
            41,
            1,
            ResizeMode::NearestNeighbor,
            Gamma::Linear,
        );
        let g = grays(&up);
        assert_eq!((g[0], g[1], g[2], g[40]), (0, 0, 1, 22));
    }

    #[test]
    fn bilinear_low_quality_extrapolates_on_upsize() {
        // quirk(pdn): document-ops-and-formats.md §2.2 step 7.
        let c = Gamma::Companded;
        assert_eq!(
            grays(&resize(
                &gray_row(&[200, 0]),
                2,
                1,
                4,
                1,
                ResizeMode::BilinearLowQuality,
                c
            )),
            [250, 150, 50, 0]
        );
        assert_eq!(
            grays(&resize(
                &gray_row(&[0, 200]),
                2,
                1,
                4,
                1,
                ResizeMode::BilinearLowQuality,
                c
            )),
            [0, 50, 150, 200]
        );
        assert_eq!(
            grays(&resize(
                &gray_row(&[0, 100, 200, 250]),
                4,
                1,
                2,
                1,
                ResizeMode::BilinearLowQuality,
                c
            )),
            [50, 225]
        );
        // Gamma on: floats [220.85, 175.82, 106.09, 0]; ±1 until Appendix B B-F3 pins the rounding.
        let lin = grays(&resize(
            &gray_row(&[200, 0]),
            2,
            1,
            4,
            1,
            ResizeMode::BilinearLowQuality,
            Gamma::Linear,
        ));
        for (got, want) in lin.iter().zip([221u8, 176, 106, 0]) {
            assert!(got.abs_diff(want) <= 1, "{lin:?}");
        }
    }

    #[test]
    fn fant_box_averages() {
        let out = resize(
            &gray_row(&[0, 90, 180]),
            3,
            1,
            2,
            1,
            ResizeMode::Fant,
            Gamma::Companded,
        );
        assert_eq!(grays(&out), [30, 150]);
    }

    #[test]
    fn same_size_is_identity_for_every_mode() {
        let src = crate::testing_fixture();
        for mode in ResizeMode::ALL {
            assert_eq!(
                resize(&src, 7, 5, 7, 5, mode, Gamma::Linear),
                src,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn uniform_colour_stays_uniform() {
        let src: Vec<u8> = std::iter::repeat_n([40u8, 120, 200, 255], 16)
            .flatten()
            .collect();
        for mode in ResizeMode::ALL {
            let out = resize(&src, 4, 4, 2, 2, mode, Gamma::Linear);
            assert!(
                out.chunks(4).all(|p| p == [40, 120, 200, 255]),
                "{mode:?} {out:?}"
            );
        }
    }

    /// ui-drawn-graphics.md §5 Task 19: a black/white pair averages in linear light (≈ 188, not 128).
    #[test]
    fn catmull_rom_downscale_averages_in_linear_light() {
        let src = [0u8, 0, 0, 255, 255, 255, 255, 255];
        let out = resize(&src, 2, 1, 1, 1, ResizeMode::Bicubic, Gamma::Linear);
        assert!(out[0].abs_diff(188) <= 1, "{out:?}");
        let flat = resize(&src, 2, 1, 1, 1, ResizeMode::Bicubic, Gamma::Companded);
        assert!(flat[0].abs_diff(128) <= 1, "{flat:?}");
    }

    #[test]
    fn kernels() {
        // Catmull-Rom (B 0, C ½), B-spline (B 1, C 0), triangle, Lanczos 3.
        let k = |kern, x: f64| magic::kernel_weight(kern, x);
        assert_eq!(
            (k(Kernel::CatmullRom, 0.0), k(Kernel::CatmullRom, 1.0)),
            (1.0, 0.0)
        );
        assert!((k(Kernel::CatmullRom, 0.5) - 0.5625).abs() < 1e-12);
        assert!((k(Kernel::CatmullRom, 1.5) + 0.0625).abs() < 1e-12);
        assert!((k(Kernel::CubicSmooth, 0.0) - 2.0 / 3.0).abs() < 1e-12);
        assert!((k(Kernel::CubicSmooth, 1.0) - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(k(Kernel::Linear, 0.5), 0.5);
        assert!((k(Kernel::Lanczos3, 0.5) - 0.607927).abs() < 1e-6);
        assert_eq!(k(Kernel::Lanczos3, 3.0), 0.0);
    }

    /// SettingUtil.GetDefaultInterpolation on min(src/dst): ratio 1 (one axis kept) → Linear,
    /// < 0.5 → Lanczos 3.
    #[test]
    fn adaptive_picks_by_scale_ratio() {
        let src = crate::testing_fixture();
        let r = |dw, dh, m| resize(&src, 7, 5, dw, dh, m, Gamma::Linear);
        assert_eq!(
            r(3, 5, ResizeMode::AdaptiveSharp),
            r(3, 5, ResizeMode::Bilinear)
        );
        assert_eq!(
            r(15, 11, ResizeMode::AdaptiveSharp),
            r(15, 11, ResizeMode::Lanczos3)
        );
        assert_ne!(
            r(15, 11, ResizeMode::Bilinear),
            r(15, 11, ResizeMode::Lanczos3)
        );
    }

    /// Exact f32 output of MagicScaler 0.15.0's own KernelMap / Convolver4ChanIntrinsic routines run
    /// on AVX + FMA hardware (a local harness, not committed): 9 taps per output pixel exercise the
    /// fused-multiply-add grouping and the edge folding.
    #[test]
    fn magic_scale_matches_magicscaler_bits() {
        let src: Vec<u8> = (0..9u32)
            .flat_map(|i| {
                [
                    (i * 37) as u8,
                    (i * 91) as u8,
                    (i * 13) as u8,
                    (i * 53 + 17) as u8,
                ]
            })
            .collect();
        let img = PremulImage::from_rgba8(&src, 9, 1, Gamma::Linear);
        let bits = |k| {
            magic::scale(&img, 2, 1, k)
                .px
                .concat()
                .iter()
                .map(|f| f.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            bits(Kernel::CatmullRom),
            [
                0x3d877ee4, 0x3d7986be, 0x3b994bcb, 0x3ecd3ada, 0x3dd23f0a, 0x3e2e11a8, 0x3d3bebbd,
                0x3f044d7e
            ]
        );
        assert_eq!(
            bits(Kernel::Lanczos3),
            [
                0x3d90c35c, 0x3d54064f, 0x3b283836, 0x3ecb768a, 0x3ddb6c50, 0x3e2eedc4, 0x3d3d55d7,
                0x3f07e214
            ]
        );
        assert_eq!(
            magic::scale(&img, 2, 1, Kernel::Lanczos3).to_rgba8(Gamma::Linear),
            [0x75, 0x65, 0x13, 0x65, 0x7c, 0x9a, 0x53, 0x87]
        );
    }

    /// MagicScaler's Average (box) kernel, the canvas mips' scaler, against the same harness: exact
    /// f32 bits for 4×2 → 2×1 and FNV-1a over the f32 bits for larger pairs (linear light).
    #[test]
    fn average_matches_magicscaler_bits() {
        let fixture = |w: u32, h: u32| -> Vec<u8> {
            (0..w * h)
                .flat_map(|i| {
                    [
                        (i * 37) as u8,
                        (i * 91) as u8,
                        (i * 13) as u8,
                        (i * 53 + 17) as u8,
                    ]
                })
                .collect()
        };
        let bits = |w, h, dw, dh| {
            let img = PremulImage::from_rgba8(&fixture(w, h), w, h, Gamma::Linear);
            let px = magic::scale(&img, dw, dh, Kernel::Average).px.concat();
            px.iter().map(|f| f.to_bits()).collect::<Vec<_>>()
        };
        let fnv = |v: Vec<u32>| {
            v.into_iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            })
        };
        assert_eq!(
            bits(4, 2, 2, 1),
            [
                0x3da41713, 0x3d62f072, 0x3c18ea86, 0x3eababac, 0x3dbd312c, 0x3dae65ba, 0x3cc62104,
                0x3f000000
            ]
        );
        assert_eq!(fnv(bits(8, 6, 4, 3)), 0xb1e6281335e84cb4);
        assert_eq!(fnv(bits(9, 7, 5, 4)), 0x0ec537ceb058cb85);
        assert_eq!(fnv(bits(23, 17, 12, 9)), 0x655f8191f742094f);
        assert_eq!(fnv(bits(23, 17, 46, 34)), 0xfc5591072cfd878d);
        assert_eq!(magic::kernel_weight(Kernel::Average, 0.5), 1.0);
        assert_eq!(magic::kernel_weight(Kernel::Average, 0.6), 0.0);
    }

    fn one_px() -> PremulImage {
        PremulImage {
            w: 1,
            h: 1,
            px: vec![[0.5, 0.25, 0.125, 1.0]],
        }
    }
    fn empty(w: u32, h: u32) -> PremulImage {
        PremulImage { w, h, px: vec![] }
    }

    #[test]
    fn magic_scale_zero_dimensions() {
        assert_eq!(
            magic::scale(&one_px(), 0, 3, Kernel::CatmullRom),
            empty(0, 3)
        );
        assert_eq!(
            magic::scale(&empty(0, 4), 2, 1, Kernel::Lanczos3).px,
            [[0.0; 4]; 2]
        );
    }

    #[test]
    fn linear_low_quality_zero_dimensions() {
        assert_eq!(pdn::linear_low_quality(&one_px(), 2, 0), empty(2, 0));
        assert_eq!(
            pdn::linear_low_quality(&empty(0, 5), 2, 1).px,
            [[0.0; 4]; 2]
        );
    }

    #[test]
    fn box_average_zero_dimensions() {
        assert_eq!(pdn::box_average(&one_px(), 0, 0), empty(0, 0));
        assert_eq!(pdn::box_average(&empty(3, 0), 1, 2).px, [[0.0; 4]; 2]);
    }

    #[test]
    fn nearest_neighbor_zero_dimensions() {
        assert_eq!(pdn::nearest_neighbor(&[1, 2, 3, 4], 1, 1, 0, 2), [0u8; 0]);
        assert_eq!(pdn::nearest_neighbor(&[], 0, 3, 1, 2), [0u8; 8]);
    }

    /// The documented precondition: oversize or short input returns an empty Vec, never a panic.
    #[test]
    fn resize_rejects_oversize_without_panicking() {
        let m = ResizeMode::Bicubic;
        assert!(resize(&[], u32::MAX, u32::MAX, 1, 1, m, Gamma::Linear).is_empty());
        assert!(resize(&[0; 12], 2, 2, 1, 1, m, Gamma::Linear).is_empty());
    }

    /// Task 43 streams Bilinear (Low Quality) and Fant row by row: FNV-1a of their output, taken
    /// from the whole-image f32 implementation before the change, for every axis combination
    /// (both shrink, both grow, each mixed order, one axis kept) with partial alpha.
    #[test]
    fn llq_and_fant_streaming_is_bit_identical() {
        let fnv = |v: Vec<u8>| {
            v.into_iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            })
        };
        let src = crate::testing::fixture_rgba(23, 17, 3);
        let sizes = [
            (11, 8),
            (41, 30),
            (11, 30),
            (41, 9),
            (23, 9),
            (50, 17),
            (7, 17),
        ];
        let mut got = vec![];
        for mode in [ResizeMode::BilinearLowQuality, ResizeMode::Fant] {
            for gamma in [Gamma::Linear, Gamma::Companded] {
                for (dw, dh) in sizes {
                    got.push(fnv(resize(&src, 23, 17, dw, dh, mode, gamma)));
                }
            }
        }
        let one = |dw, dh, m| fnv(resize(&[9, 80, 200, 77], 1, 1, dw, dh, m, Gamma::Linear));
        got.push(one(3, 2, ResizeMode::BilinearLowQuality));
        got.push(one(3, 2, ResizeMode::Fant));
        assert_eq!(
            got,
            [
                0xe5f38c59d4345031,
                0xfd967290c40f6b67,
                0x525a35d60b8389b8,
                0x476939643e02840c,
                0x2e291999034f5e8a,
                0xf43eda6dd1a1d3c2,
                0xff844e93d13301c6,
                0xefd98757df7ff9c9,
                0x1628ab64257ea27,
                0xd0714aa1e1870289,
                0xa166e771c2b10753,
                0xba56dea3ea007b07,
                0x2c107aab305c77bd,
                0x69ccfb4cb3e92d8,
                0x3971b1b76b159da1,
                0xfd967290c40f6b67,
                0x94b2b782bcdedfcf,
                0xca4af48a788cb711,
                0x894779cc1b1ad06e,
                0xf43eda6dd1a1d3c2,
                0x3ec93b591e097a5b,
                0x4616af2ec7b5bea4,
                0x1628ab64257ea27,
                0x47f50c9232db948,
                0x79a0b04be6657ea0,
                0x6b95d8a8a9a635ac,
                0x2c107aab305c77bd,
                0x95ebb645d9fdea4c,
                0xfeeaac546b7dd889,
                0xfeeaac546b7dd889
            ]
        );
    }

    #[test]
    fn fully_transparent_input_stays_transparent_black() {
        // Coloured α = 0 pixels contribute nothing in premultiplied space; un-premultiplying a = 0
        // gives transparent black for every float scaler.
        let src: Vec<u8> = (0..16u8)
            .flat_map(|i| [i * 15, 255 - i * 15, 77, 0])
            .collect();
        for mode in [
            ResizeMode::Bicubic,
            ResizeMode::Bilinear,
            ResizeMode::Lanczos3,
            ResizeMode::Fant,
            ResizeMode::BilinearLowQuality,
        ] {
            let out = resize(&src, 4, 4, 3, 2, mode, Gamma::Linear);
            assert!(out.iter().all(|&b| b == 0), "{mode:?} {out:?}");
        }
    }
}
