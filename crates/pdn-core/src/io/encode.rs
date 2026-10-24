//! Save options and the encoders for JPEG and the bundled-plugin formats (Task 47), and Paint.NET's
//! 8-bit save pipeline that the PNG, BMP, TIFF, GIF and TGA writers share (Task 47b).
//! Ports: paintdotjs js/src/io/ImageEncoder.js `encode`, `encodeDds`, `encodeModern`
//! (reference: paintdotjs @ b4e6fc5).
//! Paint.NET's save options and defaults, bit depths, alpha flatten and indexed palette (through
//! `util::quantize`), JPEG flatten and quality.
//!
//! DDS, WebP, AVIF and JPEG XL are bundled Paint.NET plugins that cannot be analysed (spec §0.2),
//! so they keep paintdotjs's behaviour (R11): DDS is its uncompressed A8B8G8R8 writer byte for
//! byte, and quality q < 100 is lossy while q = 100 is lossless (Chromium's `toBlob("image/webp",
//! 1)` and ImageMagick's JPEG XL coder; AVIF has no lossless mode and just takes q).

use std::borrow::Cow;

use super::codecs::{CodecError, invalid, unsupported};
use super::{bmp, gif, png, tga, tiff};
use crate::canvas::ImageData;
use crate::pixel::Px;
use crate::util::quantize::{
    AlphaMixture, PaletteMap, analyze, flatten_alpha, map_indices, octree_palette, palette_table,
};

/// Bit depth token (§2.1). `Auto` picks the smallest lossless depth (Task 47b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BitDepth {
    #[default]
    Auto,
    B32,
    B24,
    B8,
    B4,
    B2,
    B1,
}

/// Quantization token (§2.1). Median Cut falls back to Octree (B-E7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quantizer {
    #[default]
    Octree,
    MedianCut,
}

/// One save's options. `Default` is Paint.NET's format-independent defaults with no quality.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveOptions {
    /// 0–100. `None`: JPEG uses 95; WebP, AVIF and JPEG XL use 100 (lossless where possible).
    pub quality: Option<u8>,
    /// JPEG chroma subsampling: "4:4:4", "4:4:0", "4:2:2" or "4:2:0"; `None` is "4:4:0".
    pub chroma: Option<String>,
    pub bit_depth: BitDepth,
    pub quantizer: Quantizer,
    /// Floyd–Steinberg level, 0–8.
    pub dither: u8,
    /// Transparency threshold for 8-bit with transparency, 0–255.
    pub alpha_threshold: u8,
    /// PNG Adam7. Not exposed (Task 47b): the PNG writer is always non-interlaced.
    pub interlaced: bool,
    /// TGA run-length encoding.
    pub rle: bool,
    /// Resolution written to PNG pHYs, the BMP header and the TIFF tags.
    pub dpi: f64,
}

impl Default for SaveOptions {
    fn default() -> Self {
        SaveOptions {
            quality: None,
            chroma: None,
            bit_depth: BitDepth::Auto,
            quantizer: Quantizer::Octree,
            dither: 7,
            alpha_threshold: 128,
            interlaced: false,
            rle: true,
            dpi: 96.0,
        }
    }
}

impl SaveOptions {
    /// The save dialog's defaults for a format: quality 95 for JPEG (with 4:4:0), 90 for HEIC,
    /// JPEG XR and the plugin lossy formats (§2.1; paintdotjs `SaveImageDialog.FORMATS`).
    pub fn default_for(format: &str) -> SaveOptions {
        let quality = match format {
            "jpeg" => Some(95),
            "heic" | "jxr" | "webp" | "avif" | "jxl" => Some(90),
            _ => None,
        };
        let chroma = (format == "jpeg").then(|| "4:4:0".to_string());
        SaveOptions {
            quality,
            chroma,
            ..SaveOptions::default()
        }
    }
}

/// A bit depth the 8-bit pipeline writes (§2.8 step 2). `B8A1` is 8-bit indexed with one
/// transparent entry, always the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    B1,
    B2,
    B4,
    B8,
    B8A1,
    B24,
    B32,
}

impl Depth {
    pub(crate) fn bits(self) -> u32 {
        match self {
            Depth::B1 => 1,
            Depth::B2 => 2,
            Depth::B4 => 4,
            Depth::B8 | Depth::B8A1 => 8,
            Depth::B24 => 24,
            Depth::B32 => 32,
        }
    }

    pub(crate) fn is_indexed(self) -> bool {
        self.bits() <= 8
    }
}

/// §2.8 step 2: PNG Auto = {1, 2, 4, 8, 8A1, 24, 32}; BMP and TIFF Auto = {1, 4, 8, 24, 32}; TGA
/// and JPEG XR Auto = {24, 32}; GIF and PNG 8-bit = {8, 8A1}. Any other explicit depth is the
/// single allowed one; a depth the format does not offer falls back to its Auto set.
pub fn allowed_depths(format: &str, opt: BitDepth) -> Vec<Depth> {
    use Depth::*;
    let auto = match format {
        "gif" => return vec![B8, B8A1],
        "png" => vec![B1, B2, B4, B8, B8A1, B24, B32],
        "bmp" | "tiff" => vec![B1, B4, B8, B24, B32],
        _ => vec![B24, B32],
    };
    let one = match opt {
        BitDepth::Auto => return auto,
        BitDepth::B8 if format == "png" => return vec![B8, B8A1],
        BitDepth::B32 => B32,
        BitDepth::B24 => B24,
        BitDepth::B8 => B8,
        BitDepth::B4 => B4,
        BitDepth::B2 => B2,
        BitDepth::B1 => B1,
    };
    if auto.contains(&one) { vec![one] } else { auto }
}

/// §2.8 steps 3–5. `trial(d)` is the byte length of the image encoded at depth `d`; it is called
/// only for images of at most 65536 pixels, and a tie goes to the non-indexed depth.
pub fn choose_bit_depth(
    format: &str,
    img: &ImageData,
    opt: &SaveOptions,
    trial: &mut dyn FnMut(Depth) -> usize,
) -> Depth {
    use Depth::*;
    let allowed = allowed_depths(format, opt.bit_depth);
    if let [one] = allowed[..] {
        return one;
    }
    if allowed == [B8, B8A1] {
        let clear = img
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .any(|&p| flatten_alpha(p, opt.alpha_threshold)[3] != 255);
        return if clear { B8A1 } else { B8 };
    }
    // The lossless set: 32 always; opaque adds 24 and 1/2/4/8 by palette size; opaque-or-
    // transparent with at most 256 colours (the transparent one included) adds 8A1.
    let a = analyze(&img.data);
    let n = a.palette.as_ref().map_or(usize::MAX, Vec::len);
    let opaque = a.alpha == AlphaMixture::AllOpaque;
    let lossless = |d: Depth| match d {
        B32 => true,
        B24 => opaque,
        B8A1 => a.alpha == AlphaMixture::AllOpaqueOrTransparent && n <= 256,
        _ => opaque && n <= 1 << d.bits(),
    };
    let candidates: Vec<Depth> = allowed.into_iter().filter(|&d| lossless(d)).collect();
    if let [one] = candidates[..] {
        return one;
    }
    let has = |d: Depth| candidates.contains(&d);
    if img.width as u64 * img.height as u64 <= 65536 {
        for (indexed, full) in [(B4, B24), (B8, B24), (B8A1, B32)] {
            if has(indexed) && has(full) {
                return if trial(indexed) < trial(full) {
                    indexed
                } else {
                    full
                };
            }
        }
    }
    [B1, B2, B4, B8, B8A1, B24, B32]
        .into_iter()
        .find(|&d| has(d))
        .unwrap_or(B32)
}

/// §2.8 steps 6–8 for an indexed depth: the alpha flatten (threshold for 8A1, else 0), the lossless
/// palette when it fits 2^bpp (transparent entry last), else the Octree with the last slot kept
/// for transparent; then the Floyd–Steinberg index map at `opt.dither`. Returns (palette, one
/// index per pixel). Median Cut falls back to the Octree (B-E7).
pub fn indexed(img: &ImageData, depth: Depth, opt: &SaveOptions) -> (Vec<Px>, Vec<u8>) {
    // ruling R34: §2.8 step 6's t = 0 for every depth but 8A1, not step 3's t = 1.
    let t = if depth == Depth::B8A1 {
        opt.alpha_threshold
    } else {
        0
    };
    let px: Vec<u8> = img
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&p| flatten_alpha(p, t))
        .collect();
    let max = 1 << depth.bits().min(8);
    let lossless = analyze(&px).palette.map(|p| palette_table(&p));
    let palette = match lossless {
        Some(table) if table.len() <= max => table,
        _ => {
            let clear = px.as_chunks::<4>().0.iter().any(|p| p[3] == 0);
            let mut pal: Vec<Px> = octree_palette(&px, max - clear as usize)
                .into_iter()
                .map(|[r, g, b]| [r, g, b, 255])
                .collect();
            if clear {
                pal.push([0; 4]);
            }
            pal
        }
    };
    let mut map = PaletteMap::new(&palette);
    let idx = map_indices(&px, img.width, img.height, &mut map, opt.dither);
    (palette, idx)
}

/// What a writer gets: top-down rows of one index (indexed depths), RGB (24) or RGBA (32) per pixel.
pub(crate) struct Raster<'a> {
    pub w: u32,
    pub h: u32,
    pub depth: Depth,
    pub palette: Vec<Px>,
    pub samples: Cow<'a, [u8]>,
}

impl Raster<'_> {
    /// §2.8 steps 6–8: 32-bit keeps the straight RGBA (the RGB of A = 0 too), 24-bit is
    /// flattened onto white (threshold 0), indexed depths go through `indexed`.
    pub(crate) fn new<'a>(img: &'a ImageData, depth: Depth, opt: &SaveOptions) -> Raster<'a> {
        let (palette, samples) = match depth {
            Depth::B32 => (vec![], Cow::Borrowed(&img.data[..])),
            Depth::B24 => (
                vec![],
                img.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|&p| {
                        // ruling R34: t = 0 (§2.8 step 6).
                        let [r, g, b, _] = flatten_alpha(p, 0);
                        [r, g, b]
                    })
                    .collect(),
            ),
            _ => {
                let (palette, idx) = indexed(img, depth, opt);
                (palette, Cow::Owned(idx))
            }
        };
        Raster {
            w: img.width,
            h: img.height,
            depth,
            palette,
            samples,
        }
    }

    /// Row `y`, with 1/2/4-bit indices packed MSB-first, as Paint.NET packs them.
    pub(crate) fn row(&self, y: u32) -> Vec<u8> {
        let bits = self.depth.bits();
        let width = self.w as usize * (bits as usize).div_ceil(8);
        let row = &self.samples[y as usize * width..][..width];
        if bits >= 8 {
            return row.to_vec();
        }
        let per = (8 / bits) as usize;
        row.chunks(per)
            .map(|c| {
                c.iter()
                    .enumerate()
                    .fold(0, |b, (i, &v)| b | v << (8 - bits as usize * (i + 1)))
            })
            .collect()
    }
}

/// The PNG/BMP/TIFF/GIF writer for `format` at `depth`.
fn write(
    img: &ImageData,
    format: &str,
    depth: Depth,
    opt: &SaveOptions,
) -> Result<Vec<u8>, CodecError> {
    let r = Raster::new(img, depth, opt);
    match format {
        "png" => Ok(png::write(&r, opt.dpi)),
        "bmp" => bmp::write(&r, opt.dpi),
        "tiff" => tiff::write(&r, opt.dpi),
        _ => gif::write(&r),
    }
}

/// §2.8: choose the depth (trial encodes use the Octree, threshold 1 and dither 0), then write.
/// Ruling R33: a depth Auto took from its lossless set is written like the trials (t = 1), so
/// Auto never loses colours (§2.8 step 5); a winning trial's bytes are reused as they are.
fn encode_8bit(img: &ImageData, format: &str, opt: &SaveOptions) -> Result<Vec<u8>, CodecError> {
    let trial_opt = SaveOptions {
        quantizer: Quantizer::Octree,
        alpha_threshold: 1,
        dither: 0,
        ..opt.clone()
    };
    let mut trials = Vec::new();
    let depth = choose_bit_depth(format, img, opt, &mut |d| {
        let bytes = write(img, format, d, &trial_opt);
        let len = bytes.as_ref().map_or(usize::MAX, Vec::len);
        trials.push((d, bytes));
        len
    });
    if let Some((_, bytes)) = trials.into_iter().find(|t| t.0 == depth) {
        return bytes;
    }
    let allowed = allowed_depths(format, opt.bit_depth);
    let auto = allowed.len() > 1 && allowed != [Depth::B8, Depth::B8A1];
    write(img, format, depth, if auto { &trial_opt } else { opt })
}

/// Encodes straight RGBA8 `img` as `format` (a paintdotjs format id). PNG, BMP, TIFF, GIF and TGA
/// go through Paint.NET's 8-bit pipeline (§2.8); HEIC and JPEG XR fail with paintdotjs's errors
/// (Task 48 keeps them).
pub fn encode(img: &ImageData, format: &str, options: &SaveOptions) -> Result<Vec<u8>, CodecError> {
    let len = (img.width as u64 * img.height as u64).checked_mul(4);
    if img.width == 0 || img.height == 0 || len != Some(img.data.len() as u64) {
        return Err(invalid("image buffer does not match its size"));
    }
    let quality = options.quality.unwrap_or(100);
    match format {
        "jpeg" => encode_jpeg(img, options),
        "dds" => Ok(encode_dds(img)),
        "webp" => encode_webp(img, quality),
        "avif" => encode_avif(img, quality),
        "jxl" => encode_jxl(img, quality),
        "png" | "bmp" | "tiff" | "gif" => encode_8bit(img, format, options),
        "tga" if img.width > 65535 || img.height > 65535 => {
            Err(invalid("TGA images are at most 65535 pixels per side"))
        }
        "tga" => Ok(tga::encode(img, options, tga::TgaClock::now())),
        // paintdotjs has no browser encoder for HEIC and its bundled ImageMagick fails with this;
        // JPEG XR gets the generic message below (both captured from `ImageEncoder.encode` in
        // paintdotjs @ b4e6fc5).
        "heic" => Err(CodecError::Unsupported(
            "NoEncodeDelegateForThisImageFormat `HEIC' @ error/constitute.c/WriteImage/1435".into(),
        )),
        _ => Err(unsupported(format)),
    }
}

/// JPEG (§2.8 step 9): the alpha flatten (threshold 0) blends every pixel onto white with truncation
/// (step 6), then Bgr24 at `quality` (default 95) and the chroma subsampling (default 4:4:0).
pub fn encode_jpeg(img: &ImageData, options: &SaveOptions) -> Result<Vec<u8>, CodecError> {
    use jpeg_encoder::SamplingFactor as S;
    let sampling = match options.chroma.as_deref().unwrap_or("4:4:0") {
        "4:4:4" => S::R_4_4_4,
        "4:4:0" => S::R_4_4_0,
        "4:2:2" => S::R_4_2_2,
        "4:2:0" => S::R_4_2_0,
        other => return Err(invalid(format!("unknown chroma subsampling {other}"))),
    };
    let (Ok(w), Ok(h)) = (u16::try_from(img.width), u16::try_from(img.height)) else {
        return Err(invalid("JPEG images are at most 65535 pixels per side"));
    };
    let rgb: Vec<u8> = img
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&p| {
            let [r, g, b, _] = flatten_alpha(p, 0);
            [r, g, b]
        })
        .collect();
    let mut out = Vec::new();
    // jpeg-encoder clamps quality to 1..=100.
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, options.quality.unwrap_or(95));
    encoder.set_sampling_factor(sampling);
    encoder
        .encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .map_err(invalid)?;
    Ok(out)
}

/// paintdotjs's DDS writer (`ImageEncoder.encodeDds`), byte for byte: a 128-byte header for
/// uncompressed 32-bit A8B8G8R8 (masks R 0xFF, G 0xFF00, B 0xFF0000, A 0xFF000000), then the
/// straight RGBA rows top-down.
pub fn encode_dds(img: &ImageData) -> Vec<u8> {
    let mut header = [0u8; 128];
    header[..4].copy_from_slice(b"DDS ");
    for (at, v) in [
        (4, 124),    // header size
        (8, 0x100F), // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
        (12, img.height),
        (16, img.width),
        (20, img.width.wrapping_mul(4)), // pitch (DataView.setUint32 wraps)
        (76, 32),                        // pixel format size
        (80, 0x41),                      // ALPHAPIXELS | RGB
        (88, 32),                        // bits per pixel
        (92, 0xFF),
        (96, 0xFF00),
        (100, 0xFF_0000),
        (104, 0xFF00_0000),
        (108, 0x1000), // caps: TEXTURE
    ] {
        header[at..at + 4].copy_from_slice(&u32::to_le_bytes(v));
    }
    [&header[..], &img.data].concat()
}

/// WebP: lossless at q = 100 (`image-webp`); lossy otherwise through libwebp on native targets
/// (formats-matrix.md §3.9). The web build has no lossy encoder in Rust and stays lossless.
fn encode_webp(img: &ImageData, quality: u8) -> Result<Vec<u8>, CodecError> {
    #[cfg(not(target_arch = "wasm32"))]
    if quality < 100 {
        return webp::Encoder::from_rgba(&img.data, img.width, img.height)
            .encode_simple(false, quality as f32)
            .map(|m| m.to_vec())
            .map_err(|e| invalid(format!("WebP encode: {e:?}")));
    }
    let _ = quality;
    let mut out = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .encode(
            &img.data,
            img.width,
            img.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(invalid)?;
    Ok(out)
}

/// AVIF through `ravif` (formats-matrix.md §3.5) at colour and alpha quality q.
fn encode_avif(img: &ImageData, quality: u8) -> Result<Vec<u8>, CodecError> {
    let px: Vec<ravif::RGBA8> = img
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| ravif::RGBA8::new(r, g, b, a))
        .collect();
    let q = quality.min(100) as f32;
    ravif::Encoder::new()
        .with_quality(q)
        .with_alpha_quality(q)
        .encode_rgba(ravif::Img::new(
            &px[..],
            img.width as usize,
            img.height as usize,
        ))
        .map(|r| r.avif_file)
        .map_err(invalid)
}

/// JPEG XL (formats-matrix.md §3.4): lossless at q = 100 through `zune-jpegxl`; lossy at
/// libjxl's distance for q (`JxlEncoderDistanceFromQuality`, ImageMagick's mapping) through the
/// hand-written libjxl FFI on native targets, which also takes 1-pixel-wide or -high images that
/// zune-jpegxl refuses. The web build is lossless-only.
fn encode_jxl(img: &ImageData, quality: u8) -> Result<Vec<u8>, CodecError> {
    #[cfg(not(target_arch = "wasm32"))]
    if quality < 100 || img.width < 2 || img.height < 2 {
        return libjxl::encode(img, quality);
    }
    let _ = quality;
    use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
    let options = EncoderOptions::default()
        .set_width(img.width as usize)
        .set_height(img.height as usize)
        .set_colorspace(ColorSpace::RGBA)
        .set_depth(BitDepth::Eight);
    let mut out = Vec::new();
    zune_jpegxl::JxlSimpleEncoder::new(&img.data, options)
        .encode(&mut out)
        .map_err(|e| invalid(format!("JPEG XL encode: {e:?}")))?;
    Ok(out)
}

/// A minimal hand-written FFI over libjxl 0.12 (BSD-3-Clause), linked statically by build.rs
/// through `jpegxl-src`. The declarations follow libjxl's public headers `jxl/encode.h`,
/// `jxl/codestream_header.h`, `jxl/color_encoding.h` and `jxl/types.h`.
#[cfg(not(target_arch = "wasm32"))]
mod libjxl {
    use std::ffi::{c_int, c_void};
    use std::ptr;

    use super::{CodecError, ImageData};

    /// `JxlBasicInfo` (`JXL_BOOL` and the enums are C `int`).
    #[repr(C)]
    struct BasicInfo {
        have_container: c_int,
        xsize: u32,
        ysize: u32,
        bits_per_sample: u32,
        exponent_bits_per_sample: u32,
        intensity_target: f32,
        min_nits: f32,
        relative_to_max_display: c_int,
        linear_below: f32,
        uses_original_profile: c_int,
        have_preview: c_int,
        have_animation: c_int,
        orientation: c_int,
        num_color_channels: u32,
        num_extra_channels: u32,
        alpha_bits: u32,
        alpha_exponent_bits: u32,
        alpha_premultiplied: c_int,
        preview: [u32; 2],
        animation: [u32; 4],
        intrinsic_xsize: u32,
        intrinsic_ysize: u32,
        padding: [u8; 100],
    }

    /// `JxlPixelFormat`.
    #[repr(C)]
    struct PixelFormat {
        num_channels: u32,
        data_type: c_int,
        endianness: c_int,
        align: usize,
    }

    /// `JxlColorEncoding` (104 bytes, 8-aligned), only filled by `JxlColorEncodingSetToSRGB`.
    #[repr(C, align(8))]
    struct ColorEncoding([u8; 128]);

    enum Encoder {}
    enum FrameSettings {}

    unsafe extern "C" {
        fn JxlEncoderCreate(memory_manager: *const c_void) -> *mut Encoder;
        fn JxlEncoderDestroy(enc: *mut Encoder);
        fn JxlEncoderInitBasicInfo(info: *mut BasicInfo);
        fn JxlEncoderSetBasicInfo(enc: *mut Encoder, info: *const BasicInfo) -> c_int;
        fn JxlColorEncodingSetToSRGB(color: *mut ColorEncoding, is_gray: c_int);
        fn JxlEncoderSetColorEncoding(enc: *mut Encoder, color: *const ColorEncoding) -> c_int;
        fn JxlEncoderFrameSettingsCreate(
            enc: *mut Encoder,
            source: *const FrameSettings,
        ) -> *mut FrameSettings;
        fn JxlEncoderSetFrameDistance(settings: *mut FrameSettings, distance: f32) -> c_int;
        fn JxlEncoderSetFrameLossless(settings: *mut FrameSettings, lossless: c_int) -> c_int;
        fn JxlEncoderDistanceFromQuality(quality: f32) -> f32;
        fn JxlEncoderAddImageFrame(
            settings: *const FrameSettings,
            format: *const PixelFormat,
            buffer: *const c_void,
            size: usize,
        ) -> c_int;
        fn JxlEncoderCloseInput(enc: *mut Encoder);
        fn JxlEncoderProcessOutput(
            enc: *mut Encoder,
            next_out: *mut *mut u8,
            avail_out: *mut usize,
        ) -> c_int;
    }

    const SUCCESS: c_int = 0;
    const NEED_MORE_OUTPUT: c_int = 2;
    const TYPE_UINT8: c_int = 2;

    /// Destroys the encoder (and its frame settings) on every exit path.
    struct Owned(*mut Encoder);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: created by JxlEncoderCreate and destroyed once.
            unsafe { JxlEncoderDestroy(self.0) }
        }
    }

    /// RGBA8 sRGB → JPEG XL codestream; q ≥ 100 is lossless.
    pub(super) fn encode(img: &ImageData, quality: u8) -> Result<Vec<u8>, CodecError> {
        let fail = |what: &str| CodecError::Invalid(format!("JPEG XL encode: {what}"));
        let ok = |status: c_int, what: &str| {
            if status == SUCCESS {
                Ok(())
            } else {
                Err(fail(what))
            }
        };
        let lossless = quality >= 100;
        // SAFETY: every pointer passed is valid for the call (the encoder is owned and non-null,
        // the structs live on this stack, the pixel buffer is w·h·4 bytes as `encode` checked),
        // and the output loop never lets libjxl write past `out`'s length.
        unsafe {
            let enc = JxlEncoderCreate(ptr::null());
            if enc.is_null() {
                return Err(fail("out of memory"));
            }
            let _owned = Owned(enc);
            let mut info = std::mem::MaybeUninit::<BasicInfo>::zeroed();
            JxlEncoderInitBasicInfo(info.as_mut_ptr());
            let mut info = info.assume_init();
            info.xsize = img.width;
            info.ysize = img.height;
            info.bits_per_sample = 8;
            info.exponent_bits_per_sample = 0;
            info.num_color_channels = 3;
            info.num_extra_channels = 1;
            info.alpha_bits = 8;
            info.alpha_exponent_bits = 0;
            info.uses_original_profile = lossless as c_int;
            ok(JxlEncoderSetBasicInfo(enc, &info), "basic info")?;
            let mut color = ColorEncoding([0; 128]);
            JxlColorEncodingSetToSRGB(&mut color, 0);
            ok(JxlEncoderSetColorEncoding(enc, &color), "colour encoding")?;
            let settings = JxlEncoderFrameSettingsCreate(enc, ptr::null());
            if settings.is_null() {
                return Err(fail("frame settings"));
            }
            if lossless {
                ok(JxlEncoderSetFrameLossless(settings, 1), "lossless")?;
            } else {
                let distance = JxlEncoderDistanceFromQuality(quality as f32);
                ok(JxlEncoderSetFrameDistance(settings, distance), "distance")?;
            }
            let format = PixelFormat {
                num_channels: 4,
                data_type: TYPE_UINT8,
                endianness: 0,
                align: 0,
            };
            let pixels = img.data.as_ptr().cast();
            ok(
                JxlEncoderAddImageFrame(settings, &format, pixels, img.data.len()),
                "frame",
            )?;
            JxlEncoderCloseInput(enc);
            let mut out = vec![0u8; 4096];
            let mut used = 0;
            loop {
                let mut next = out.as_mut_ptr().add(used);
                let mut avail = out.len() - used;
                let status = JxlEncoderProcessOutput(enc, &mut next, &mut avail);
                used = out.len() - avail;
                match status {
                    SUCCESS => {
                        out.truncate(used);
                        return Ok(out);
                    }
                    NEED_MORE_OUTPUT => out.resize(out.len() * 2, 0),
                    _ => return Err(fail("output")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let jpeg = SaveOptions::default_for("jpeg");
        assert_eq!(
            (jpeg.quality, jpeg.chroma.as_deref()),
            (Some(95), Some("4:4:0"))
        );
        for f in ["heic", "jxr", "webp", "avif", "jxl"] {
            assert_eq!(SaveOptions::default_for(f).quality, Some(90), "{f}");
        }
        let png = SaveOptions::default_for("png");
        assert_eq!(png, SaveOptions::default());
        assert_eq!(
            (
                png.bit_depth,
                png.quantizer,
                png.dither,
                png.alpha_threshold,
                png.interlaced,
                png.rle
            ),
            (BitDepth::Auto, Quantizer::Octree, 7, 128, false, true)
        );
    }

    fn image(w: u32, h: u32, px: &[Px]) -> ImageData {
        ImageData {
            width: w,
            height: h,
            data: px.concat(),
        }
    }

    /// §2.8 step 2: each token's allowed set; PNG 8-bit is {8, 8A1}; GIF ignores the token; a
    /// depth the format lacks falls back to its Auto set.
    #[test]
    fn allowed_depth_tokens() {
        use Depth::*;
        let png_auto = [B1, B2, B4, B8, B8A1, B24, B32];
        assert_eq!(allowed_depths("png", BitDepth::Auto), png_auto);
        for f in ["bmp", "tiff"] {
            assert_eq!(
                allowed_depths(f, BitDepth::Auto),
                [B1, B4, B8, B24, B32],
                "{f}"
            );
            assert_eq!(allowed_depths(f, BitDepth::B8), [B8], "{f}");
            assert_eq!(
                allowed_depths(f, BitDepth::B2),
                [B1, B4, B8, B24, B32],
                "{f}"
            );
        }
        for f in ["tga", "jxr"] {
            assert_eq!(allowed_depths(f, BitDepth::Auto), [B24, B32], "{f}");
        }
        assert_eq!(allowed_depths("tga", BitDepth::B8), [B24, B32]);
        assert_eq!(allowed_depths("tga", BitDepth::B32), [B32]);
        assert_eq!(allowed_depths("png", BitDepth::B8), [B8, B8A1]);
        assert_eq!(allowed_depths("png", BitDepth::B2), [B2]);
        assert_eq!(allowed_depths("png", BitDepth::B24), [B24]);
        assert_eq!(allowed_depths("gif", BitDepth::B24), [B8, B8A1]);
    }

    /// §2.8 step 5: rules 2–4 run in order (4 vs 24 before 8 vs 24); 8 vs 24 above 16 colours;
    /// 8A1 vs 32 for opaque-or-transparent; above 65536 pixels BMP skips 2-bit.
    #[test]
    fn auto_rules_in_order() {
        use Depth::*;
        let o = SaveOptions::default();
        let three = image(
            64,
            64,
            &[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]].repeat(1366)[..4096],
        );
        let mut prefer8 = |d: Depth| if d == B8 { 50 } else { 100 };
        assert_eq!(choose_bit_depth("png", &three, &o, &mut prefer8), B24);
        let many: Vec<Px> = (0..4096u32).map(|i| [(i % 20) as u8, 0, 0, 255]).collect();
        let many = image(64, 64, &many);
        assert_eq!(choose_bit_depth("png", &many, &o, &mut prefer8), B8);
        assert_eq!(choose_bit_depth("png", &many, &o, &mut |_| 7), B24);
        let holes = image(64, 64, &[[1, 2, 3, 255], [0; 4]].repeat(2048));
        let mut prefer8a1 = |d: Depth| if d == B8A1 { 50 } else { 100 };
        assert_eq!(choose_bit_depth("png", &holes, &o, &mut prefer8a1), B8A1);
        assert_eq!(choose_bit_depth("png", &holes, &o, &mut |_| 7), B32);
        let big = image(
            300,
            300,
            &[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]].repeat(30000),
        );
        let mut never = |_: Depth| -> usize { unreachable!() };
        assert_eq!(choose_bit_depth("bmp", &big, &o, &mut never), B4);
        assert_eq!(choose_bit_depth("png", &big, &o, &mut never), B2);
        let gif = image(1, 2, &[[1, 2, 3, 127], [1, 2, 3, 128]]);
        assert_eq!(choose_bit_depth("gif", &gif, &o, &mut never), B8A1);
        let zero = SaveOptions {
            alpha_threshold: 0,
            ..o
        };
        assert_eq!(choose_bit_depth("gif", &gif, &zero, &mut never), B8);
    }

    /// The trial lengths are real encodes: Auto equals the shorter explicit encode (tie → 24).
    #[test]
    fn auto_keeps_the_smaller_encode() {
        let three = image(
            64,
            64,
            &[[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]].repeat(1366)[..4096],
        );
        let fixture = ImageData {
            width: 13,
            height: 9,
            data: crate::testing::fixture_rgba(13, 9, 4),
        };
        for (img, low) in [(&three, BitDepth::B4), (&fixture, BitDepth::B8)] {
            for f in ["png", "bmp", "tiff"] {
                let at = |bit_depth| {
                    let o = SaveOptions {
                        bit_depth,
                        ..SaveOptions::default_for(f)
                    };
                    encode(img, f, &o).unwrap()
                };
                let (indexed, full) = (at(low), at(BitDepth::B24));
                let best = if indexed.len() < full.len() {
                    indexed
                } else {
                    full
                };
                assert_eq!(at(BitDepth::Auto), best, "{f} {low:?}");
            }
        }
    }

    /// §5 "Octree, PNG 1-bit, dither 0": 5 black, 3 white, 1 red → [(255, 225, 225), black];
    /// white → 0, black and red → 1.
    #[test]
    fn octree_palette_and_indices() {
        let (k, w, r) = ([0, 0, 0, 255], [255, 255, 255, 255], [255, 0, 0, 255]);
        let img = image(9, 1, &[k, k, k, w, k, w, k, w, r]);
        let o = SaveOptions {
            bit_depth: BitDepth::B1,
            dither: 0,
            ..SaveOptions::default_for("png")
        };
        let (palette, idx) = indexed(&img, Depth::B1, &o);
        assert_eq!(palette, [[255, 225, 225, 255], [0, 0, 0, 255]]);
        assert_eq!(idx, [1, 1, 1, 0, 1, 0, 1, 0, 1]);
        let back = super::super::codecs::decode("png", &encode(&img, "png", &o).unwrap()).unwrap();
        let p = [255, 225, 225, 255];
        assert_eq!(back.image.data, [k, k, k, p, k, p, k, p, k].concat());
    }

    /// 8A1 over 255 colours: the threshold makes A = 100 transparent and blends A = 200 onto
    /// white; the Octree gets 255 slots and [0, 0, 0, 0] is entry 255.
    #[test]
    fn transparent_slot_is_reserved_last() {
        let mut px: Vec<Px> = (0..300u32)
            .map(|i| [i as u8, (i / 256) as u8 * 100, 7, 255])
            .collect();
        px.extend([[50, 60, 70, 100]; 10]);
        px.extend([[0, 0, 0, 200]; 10]);
        let img = image(32, 10, &px);
        let o = SaveOptions {
            dither: 0,
            ..SaveOptions::default_for("gif")
        };
        let (palette, idx) = indexed(&img, Depth::B8A1, &o);
        let processed: Vec<u8> = px.iter().flat_map(|&p| flatten_alpha(p, 128)).collect();
        assert_eq!(&processed[300 * 4..301 * 4], [0, 0, 0, 0]);
        assert_eq!(&processed[310 * 4..311 * 4], [55, 55, 55, 255]);
        let octree: Vec<Px> = octree_palette(&processed, 255)
            .into_iter()
            .map(|[r, g, b]| [r, g, b, 255])
            .collect();
        assert_eq!(palette.len(), 256);
        assert_eq!(palette[..255], octree);
        assert_eq!(palette[255], [0; 4]);
        assert!(idx[300..310].iter().all(|&i| i == 255));
        assert!(idx[..300].iter().chain(&idx[310..]).all(|&i| i < 255));
    }

    /// The dither level reaches the Floyd–Steinberg map (a grey ramp at 1 bit).
    #[test]
    fn dither_level_reaches_the_index_map() {
        let ramp: Vec<Px> = (0..64u32)
            .map(|i| [(i * 4) as u8, (i * 4) as u8, (i * 4) as u8, 255])
            .collect();
        let img = image(64, 1, &ramp);
        let at = |dither| {
            indexed(
                &img,
                Depth::B1,
                &SaveOptions {
                    dither,
                    ..SaveOptions::default()
                },
            )
        };
        let (palette, plain) = at(0);
        let (_, dithered) = at(7);
        let mut map = PaletteMap::new(&palette);
        assert_eq!(dithered, map_indices(&img.data, 64, 1, &mut map, 7));
        assert_ne!(dithered, plain);
    }

    /// Ruling R33: Auto never loses colours (§2.8 step 5), even at alpha threshold 0: the 8A1
    /// it picks is written like the trials, at t = 1, so the transparent entry and tRNS stay.
    #[test]
    fn auto_keeps_transparency_at_threshold_zero() {
        let o = SaveOptions {
            alpha_threshold: 0,
            ..SaveOptions::default_for("png")
        };
        for n in [64u32, 300] {
            let img = image(n, n, &[[9, 9, 9, 255], [0; 4]].repeat((n * n / 2) as usize));
            let bytes = encode(&img, "png", &o).unwrap();
            let back = super::super::codecs::decode("png", &bytes).unwrap().image;
            assert_eq!(back.data, img.data, "{n}");
            if n == 300 {
                assert!(bytes.windows(4).any(|w| w == b"tRNS"));
            }
        }
    }

    /// §2.8 step 6 through the JPEG path: (100, A = 128) flattens to 177 on white.
    #[test]
    fn jpeg_flattens_with_process_color() {
        let img = ImageData {
            width: 8,
            height: 8,
            data: [100u8, 100, 100, 128].repeat(64),
        };
        let o = SaveOptions {
            quality: Some(100),
            chroma: Some("4:4:4".into()),
            ..SaveOptions::default()
        };
        let back = super::super::codecs::decode("jpeg", &encode(&img, "jpeg", &o).unwrap())
            .unwrap()
            .image;
        assert!(
            back.data
                .chunks(4)
                .all(|p| p[0].abs_diff(177) <= 1 && p[3] == 255),
            "{:?}",
            &back.data[..4]
        );
    }

    #[test]
    fn errors() {
        let img = ImageData {
            width: 2,
            height: 2,
            data: vec![0; 16],
        };
        let o = SaveOptions::default();
        let msg = |f: &str| encode(&img, f, &o).unwrap_err().to_string();
        assert_eq!(msg("jxr"), "Unsupported image format: jxr");
        assert!(msg("heic").starts_with("NoEncodeDelegateForThisImageFormat `HEIC'"));
        assert_eq!(msg("ico"), "Unsupported image format: ico");
        let bad_len = ImageData {
            width: 3,
            height: 2,
            data: vec![0; 16],
        };
        assert!(encode(&bad_len, "dds", &o).is_err());
        let chroma = SaveOptions {
            chroma: Some("4:1:1".into()),
            ..o.clone()
        };
        assert!(encode(&img, "jpeg", &chroma).is_err());
        let wide = ImageData {
            width: 70_000,
            height: 1,
            data: vec![0; 280_000],
        };
        assert!(encode(&wide, "jpeg", &o).is_err());
    }

    /// Lossy WebP and JPEG XL keep the size and stay close; q = 100 is exact; 1-px JXL works.
    #[test]
    fn plugin_quality_semantics() {
        let img = ImageData {
            width: 16,
            height: 8,
            data: crate::testing_fixture().repeat(4)[..512].to_vec(),
        };
        let opaque = ImageData {
            data: img
                .data
                .chunks(4)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            ..img
        };
        for f in ["webp", "jxl"] {
            let exact = encode(
                &opaque,
                f,
                &SaveOptions {
                    quality: Some(100),
                    ..SaveOptions::default()
                },
            )
            .unwrap();
            assert_eq!(
                super::super::codecs::decode(f, &exact).unwrap().image.data,
                opaque.data,
                "{f}"
            );
            let lossy = encode(&opaque, f, &SaveOptions::default_for(f)).unwrap();
            let back = super::super::codecs::decode(f, &lossy).unwrap().image;
            assert_eq!((back.width, back.height), (16, 8), "{f}");
        }
        let one = ImageData {
            width: 1,
            height: 1,
            data: vec![9, 8, 7, 255],
        };
        let bytes = encode(&one, "jxl", &SaveOptions::default()).unwrap();
        assert_eq!(
            super::super::codecs::decode("jxl", &bytes)
                .unwrap()
                .image
                .data,
            one.data
        );
    }
}
