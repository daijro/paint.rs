//! Format detection and the single-image decoders (everything except `.pdn`, which is
//! `io::pdn`). Every decoder takes hostile input: sides are capped at `MAX_DECODE_SIDE` and the
//! decoded bytes at `MAX_DECODE_BYTES` before anything large is allocated, and corrupt or
//! truncated files return `CodecError`, never panic.
//! Ports: paintdotjs js/src/io/DocumentIO.js `getImageFormat`, `loadImage`;
//! src/portableImageCodec.js `decode` (`autoOrient`, depth 8, RGBA) (reference: paintdotjs @ b4e6fc5).
//! Paint.NET opens by extension; its WIC load takes frame 0, applies the EXIF orientation and reads
//! the DPI from the file.

use std::fmt;
use std::io::Cursor;

use image::ImageDecoder;

use crate::canvas::ImageData;

/// Hostile-input guard per side (§5 "Task 47": not a Paint.NET limit, Paint.NET decodes unbounded).
pub const MAX_DECODE_SIDE: u32 = 32768;
/// Hostile-input guard on decoded pixel bytes: a tiny compressed file can claim a huge image.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_DECODE_BYTES: u64 = 1 << 30;
#[cfg(target_arch = "wasm32")]
pub const MAX_DECODE_BYTES: u64 = 1 << 29;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// No codec for this format here (yet), with the message to show.
    Unsupported(String),
    /// Corrupt, truncated or over-limit input, or a failed encode.
    Invalid(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Unsupported(m) | CodecError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for CodecError {}

pub(crate) fn invalid(e: impl fmt::Display) -> CodecError {
    CodecError::Invalid(e.to_string())
}

/// paintdotjs's message for a format it has no codec for (`ImageEncoder.encode`, `portableImageCodec`).
pub(crate) fn unsupported(format: &str) -> CodecError {
    CodecError::Unsupported(format!("Unsupported image format: {format}"))
}

/// Rejects sides above `MAX_DECODE_SIDE` and images above `MAX_DECODE_BYTES` as RGBA8.
pub fn check_size(w: u32, h: u32) -> Result<usize, CodecError> {
    if w == 0 || h == 0 {
        return Err(invalid("empty image"));
    }
    let bytes = w as u64 * h as u64 * 4;
    if w > MAX_DECODE_SIDE || h > MAX_DECODE_SIDE || bytes > MAX_DECODE_BYTES {
        return Err(invalid(format!("image too large ({w} × {h})")));
    }
    Ok(bytes as usize) // ≤ MAX_DECODE_BYTES, so it fits usize on wasm32 too
}

/// `N` bytes at `at`, or `None` past the end (no offset arithmetic that could overflow).
pub(crate) fn bytes_at<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    b.get(at..)?.get(..N)?.try_into().ok()
}

#[derive(Clone, Debug)]
pub struct Decoded {
    pub image: ImageData,
    /// Dots per inch from pHYs, JFIF, the BMP header or the TIFF tags; `None` when absent.
    pub dpi: Option<f64>,
}

/// Frames a JPEG XL file may composite before keyframe 0 (layers saved as zero-duration frames):
/// a CPU bound against tiny files that chain many frames.
const MAX_JXL_FRAMES_BEFORE_KEYFRAME: usize = 64;

/// The format id (paintdotjs `SaveImageDialog.FORMATS` ids) of a file: by extension first
/// (Paint.NET picks the type by extension only, §2.1), then the MIME type (paintdotjs
/// `getImageFormat`), then the PNG/JPEG/GIF/BMP/TIFF/WebP/PDN3 magic. ICO is not supported (R20).
pub fn detect_format(file_name: &str, mime: Option<&str>, bytes: &[u8]) -> Option<&'static str> {
    let ext = file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase());
    let by_ext = match ext.as_deref() {
        Some("pdn") => Some("pdn"),
        Some("png") => Some("png"),
        Some("jpg" | "jpeg" | "jpe") => Some("jpeg"),
        Some("gif") => Some("gif"),
        Some("bmp") => Some("bmp"),
        Some("tif" | "tiff") => Some("tiff"),
        Some("tga") => Some("tga"),
        Some("webp") => Some("webp"),
        Some("dds") => Some("dds"),
        Some("jxl") => Some("jxl"),
        Some("avif") => Some("avif"),
        Some("heic" | "heif") => Some("heic"),
        Some("jxr" | "wdp" | "wmp") => Some("jxr"),
        Some("psd" | "psb") => Some("psd"),
        _ => None,
    };
    let by_mime = || match mime?.to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpeg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/bmp" => Some("bmp"),
        "image/avif" => Some("avif"),
        "image/heic" | "image/heif" => Some("heic"),
        "image/jxl" => Some("jxl"),
        "image/vnd-ms.dds" => Some("dds"),
        "image/tiff" => Some("tiff"),
        "image/x-tga" => Some("tga"),
        "image/vnd.ms-photo" => Some("jxr"),
        "image/vnd.adobe.photoshop" | "image/x-photoshop" => Some("psd"),
        _ => None,
    };
    by_ext.or_else(by_mime).or_else(|| magic(bytes))
}

/// The PNG/JPEG/GIF/BMP/TIFF/WebP/PDN3/PSD signature at the start of `bytes`.
fn magic(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', 13, 10, 26, 10, ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("gif"),
        [b'B', b'M', ..] => Some("bmp"),
        [b'I', b'I', 42, 0, ..] | [b'M', b'M', 0, 42, ..] => Some("tiff"),
        [b'R', b'I', b'F', b'F', _, _, _, _, rest @ ..] if rest.starts_with(b"WEBP") => {
            Some("webp")
        }
        [b'P', b'D', b'N', b'3', ..] => Some("pdn"),
        [b'8', b'B', b'P', b'S', ..] => Some("psd"),
        _ => None,
    }
}

/// Decodes one image to straight RGBA8: frame 0, EXIF orientation applied (Paint.NET's
/// flip-rotator, paintdotjs `autoOrient`); metadata is not carried, so the tag is gone too.
/// The WIC types sniff the content (§2.1): a PNG/JPEG/GIF/BMP/TIFF file whose bytes are another
/// of those formats, or WebP, decodes as what it is. AVIF, HEIC and JPEG XR decode natively only
/// (`native`; the web build gets them from the browser, Task 48w); `.pdn` is layered
/// (`io::pdn::decode`), and so is PSD (`io::psd::decode`), which comes here as one picture.
pub fn decode(format: &str, bytes: &[u8]) -> Result<Decoded, CodecError> {
    decode_within(format, bytes, MAX_DECODE_BYTES)
}

/// `decode` with `budget` in place of `MAX_DECODE_BYTES` for the `image` decoders' allocations,
/// the palette-TIFF buffers and the native decoders' pictures (tests scale it down).
pub(crate) fn decode_within(
    format: &str,
    bytes: &[u8],
    budget: u64,
) -> Result<Decoded, CodecError> {
    use image::ImageFormat as F;
    const WIC: [&str; 5] = ["png", "jpeg", "gif", "bmp", "tiff"];
    let format = match magic(bytes) {
        Some(m) if WIC.contains(&format) && (WIC.contains(&m) || m == "webp") => m,
        _ => format,
    };
    if format == "tiff"
        && let Some(image) = super::tiff::decode_palette(bytes, budget)?
    {
        let dpi = tiff_dpi(bytes);
        return Ok(Decoded { image, dpi });
    }
    let image_format = match format {
        "png" => F::Png,
        "jpeg" => F::Jpeg,
        "gif" => F::Gif,
        "bmp" => F::Bmp,
        "tiff" => F::Tiff,
        "webp" => F::WebP,
        "tga" => {
            return Ok(Decoded {
                image: super::tga::decode(bytes)?,
                dpi: None,
            });
        }
        "dds" => return decode_dds(bytes).map(|image| Decoded { image, dpi: None }),
        "psd" => {
            let (image, dpi) = super::psd::decode_flat(bytes)?;
            return Ok(Decoded {
                image,
                dpi: Some(dpi),
            });
        }
        "jxl" => return decode_jxl(bytes).map(|image| Decoded { image, dpi: None }),
        #[cfg(not(target_arch = "wasm32"))]
        "avif" | "heic" | "jxr" => {
            let (image, dpi) = native::decode(format, bytes, budget)?;
            return Ok(Decoded { image, dpi });
        }
        _ => return Err(unsupported(format)),
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image_format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    // `image` caps a TIFF's intermediate buffer at max_alloc − image bytes: give it one strip of
    // headroom (R38 strips are at most 128 KiB). `total_bytes() > budget` below still holds the
    // image itself to the budget.
    limits.max_alloc = Some(if image_format == F::Tiff {
        budget + (1 << 20)
    } else {
        budget
    });
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(invalid)?;
    // `into_decoder` + `from_decoder` skip the `max_alloc` reservation that `ImageReader::decode`
    // makes, so check the header's size and the decoder's buffer before anything is allocated.
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    if decoder.total_bytes() > budget {
        return Err(invalid(format!("image too large ({w} × {h})")));
    }
    // ponytail: peak memory is still about 2× the budget (the decoder buffer plus the RGBA8
    // conversion or the orientation copy, and zune-jpeg's progressive planes); stream rows into
    // the RGBA buffer if that matters.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(invalid)?;
    img.apply_orientation(orientation);
    check_size(img.width(), img.height())?;
    let rgba = img.into_rgba8();
    let dpi = match format {
        "png" => png_dpi(bytes),
        "jpeg" => jfif_dpi(bytes),
        "bmp" => bmp_dpi(bytes),
        "tiff" => tiff_dpi(bytes),
        _ => None,
    };
    let (width, height) = rgba.dimensions();
    Ok(Decoded {
        image: ImageData {
            width,
            height,
            data: rgba.into_raw(),
        },
        dpi,
    })
}

/// pHYs in pixels per metre (unit 1), read before the first IDAT.
fn png_dpi(b: &[u8]) -> Option<f64> {
    let mut at = 8usize;
    loop {
        let [l0, l1, l2, l3, t0, t1, t2, t3] = bytes_at::<8>(b, at)?;
        match &[t0, t1, t2, t3] {
            b"pHYs" => {
                let d = bytes_at::<9>(b, at + 8)?;
                let x = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
                return (d[8] == 1 && x > 0).then_some(x as f64 * 0.0254);
            }
            b"IDAT" | b"IEND" => return None,
            _ => {
                at = at
                    .checked_add(12)?
                    .checked_add(u32::from_be_bytes([l0, l1, l2, l3]) as usize)?
            }
        }
    }
}

/// The JFIF APP0 density: units 1 = dots per inch, 2 = dots per cm.
fn jfif_dpi(b: &[u8]) -> Option<f64> {
    let mut at = 2usize;
    loop {
        let [ff, marker, l0, l1] = bytes_at::<4>(b, at)?;
        if ff != 0xFF || matches!(marker, 0xC0..=0xCF | 0xDA) {
            return None;
        }
        if marker == 0xE0 && bytes_at::<5>(b, at + 4)? == *b"JFIF\0" {
            let [units, x0, x1] = bytes_at::<3>(b, at + 11)?;
            let x = u16::from_be_bytes([x0, x1]) as f64;
            return match units {
                1 if x > 0.0 => Some(x),
                2 if x > 0.0 => Some(x * 2.54),
                _ => None,
            };
        }
        at = at.checked_add(2 + u16::from_be_bytes([l0, l1]) as usize)?;
    }
}

/// BITMAPINFOHEADER (and later) biXPelsPerMeter; the 12-byte core header has none.
fn bmp_dpi(b: &[u8]) -> Option<f64> {
    if u32::from_le_bytes(bytes_at(b, 14)?) < 40 {
        return None;
    }
    let x = i32::from_le_bytes(bytes_at(b, 38)?);
    (x > 0).then_some(x as f64 * 0.0254)
}

/// IFD0 XResolution (282) with ResolutionUnit (296; 2 = inch, the default, 3 = cm).
fn tiff_dpi(b: &[u8]) -> Option<f64> {
    let le = match bytes_at::<4>(b, 0)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        bytes_at(b, at).map(if le {
            u16::from_le_bytes
        } else {
            u16::from_be_bytes
        })
    };
    let u32_at = |at: usize| {
        bytes_at(b, at).map(if le {
            u32::from_le_bytes
        } else {
            u32::from_be_bytes
        })
    };
    let ifd = u32_at(4)? as usize;
    let (mut x, mut unit) = (None, 2);
    for i in 0..u16_at(ifd)? as usize {
        let entry = ifd.checked_add(2 + i * 12)?;
        match u16_at(entry)? {
            282 => {
                let at = u32_at(entry + 8)? as usize;
                let (num, den) = (u32_at(at)?, u32_at(at.checked_add(4)?)?);
                x = (num > 0 && den > 0).then(|| num as f64 / den as f64);
            }
            296 => unit = u16_at(entry + 8)?,
            _ => {}
        }
    }
    match unit {
        2 => x,
        3 => x.map(|x| x * 2.54),
        _ => None,
    }
}

/// JPEG XL through `jxl-oxide` (formats-matrix.md §3.4): keyframe 0 with orientation applied, in
/// the file's colour space; grey is expanded to RGB and a missing alpha is 255.
fn decode_jxl(bytes: &[u8]) -> Result<ImageData, CodecError> {
    use jxl_oxide::{AllocTracker, JxlImage};
    // ponytail: jxl-oxide keeps f32 planes (16 B per RGBA pixel) under this budget, so JPEG XL
    // tops out near 67 MP natively; raise the tracker or render by region if that bites.
    let image = JxlImage::builder()
        .alloc_tracker(AllocTracker::with_limit(MAX_DECODE_BYTES as usize))
        .read(bytes)
        .map_err(invalid)?;
    let len = check_size(image.width(), image.height())?;
    let first = image
        .frame_by_keyframe(0)
        .ok_or(invalid("JPEG XL file has no frame"))?;
    if first.index() > MAX_JXL_FRAMES_BEFORE_KEYFRAME {
        return Err(invalid(
            "JPEG XL file has too many frames before its first keyframe",
        ));
    }
    if image.pixel_format().has_black() {
        return Err(CodecError::Unsupported(
            "CMYK JPEG XL images are not supported".into(),
        ));
    }
    let render = image.render_frame(0).map_err(invalid)?;
    let mut stream = render.stream();
    let channels = stream.channels() as usize;
    if channels == 0 {
        return Err(invalid("JPEG XL image without channels"));
    }
    let mut samples = vec![0u8; len / 4 * channels];
    stream.write_to_buffer(&mut samples);
    let data = samples
        .chunks_exact(channels)
        .flat_map(|s| match *s {
            [v] => [v, v, v, 255],
            [v, a] => [v, v, v, a],
            [r, g, b] => [r, g, b, 255],
            [r, g, b, a, ..] => [r, g, b, a],
            [] => unreachable!("channels > 0"),
        })
        .collect();
    Ok(ImageData {
        width: image.width(),
        height: image.height(),
        data,
    })
}

/// DDS through `ddsfile` (header) and `bcdec_rs` (BC1/BC2/BC3/BC7 blocks), plus uncompressed
/// bit-mask formats (formats-matrix.md §3.7). The first surface only. Sizes are computed here
/// with checked arithmetic: ddsfile's own size helpers overflow on hostile headers.
fn decode_dds(bytes: &[u8]) -> Result<ImageData, CodecError> {
    use ddsfile::{D3DFormat as D, Dds, DxgiFormat as X, PixelFormatFlags};
    type Block = fn(&[u8], &mut [u8], usize);
    let dds = Dds::read(bytes).map_err(invalid)?;
    let (w, h) = (dds.get_width(), dds.get_height());
    let len = check_size(w, h)?;
    let (wu, hu) = (w as usize, h as usize);
    let block: Option<(Block, usize)> = match (dds.get_dxgi_format(), dds.get_d3d_format()) {
        (Some(X::BC1_UNorm | X::BC1_UNorm_sRGB), _) | (_, Some(D::DXT1)) => {
            Some((bcdec_rs::bc1, 8))
        }
        (Some(X::BC2_UNorm | X::BC2_UNorm_sRGB), _) | (_, Some(D::DXT3)) => {
            Some((bcdec_rs::bc2, 16))
        }
        (Some(X::BC3_UNorm | X::BC3_UNorm_sRGB), _) | (_, Some(D::DXT5)) => {
            Some((bcdec_rs::bc3, 16))
        }
        (Some(X::BC7_UNorm | X::BC7_UNorm_sRGB), _) => Some((bcdec_rs::bc7, 16)),
        _ => None,
    };
    // The data must be present before the output is allocated.
    if let Some((decode_block, size)) = block {
        let (bw, bh) = (wu.div_ceil(4), hu.div_ceil(4));
        let src = dds
            .data
            .get(..bw * bh * size)
            .ok_or(invalid("truncated DDS data"))?;
        let mut out = vec![0u8; len];
        let mut tile = [0u8; 64];
        for (i, b) in src.chunks_exact(size).enumerate() {
            let (bx, by) = (i % bw * 4, i / bw * 4);
            decode_block(b, &mut tile, 16);
            let cols = (wu - bx).min(4) * 4;
            for y in 0..(hu - by).min(4) {
                out[((by + y) * wu + bx) * 4..][..cols].copy_from_slice(&tile[y * 16..][..cols]);
            }
        }
        return Ok(ImageData {
            width: w,
            height: h,
            data: out,
        });
    }
    // Uncompressed: bit masks from the legacy pixel format, or the DX10 8-bit RGBA/BGRA formats.
    let spf = &dds.header.spf;
    let (bits, [r, g, b, a]) = match dds.get_dxgi_format() {
        Some(X::R8G8B8A8_UNorm | X::R8G8B8A8_UNorm_sRGB) => {
            (32, [0xFF, 0xFF00, 0xFF_0000, 0xFF00_0000])
        }
        Some(X::B8G8R8A8_UNorm | X::B8G8R8A8_UNorm_sRGB) => {
            (32, [0xFF_0000, 0xFF00, 0xFF, 0xFF00_0000])
        }
        Some(X::B8G8R8X8_UNorm | X::B8G8R8X8_UNorm_sRGB) => (32, [0xFF_0000, 0xFF00, 0xFF, 0]),
        None if spf.flags.contains(PixelFormatFlags::RGB) => (
            spf.rgb_bit_count.unwrap_or(0),
            [
                spf.r_bit_mask,
                spf.g_bit_mask,
                spf.b_bit_mask,
                spf.a_bit_mask,
            ]
            .map(|m| m.unwrap_or(0)),
        ),
        _ => {
            return Err(CodecError::Unsupported(
                "this DDS pixel format is not supported".into(),
            ));
        }
    };
    let bpp = match bits {
        16 | 24 | 32 => bits as usize / 8,
        _ => {
            return Err(CodecError::Unsupported(
                "this DDS pixel format is not supported".into(),
            ));
        }
    };
    let src = dds
        .data
        .get(..wu * hu * bpp)
        .ok_or(invalid("truncated DDS data"))?;
    let mut out = vec![0u8; len];
    let channel = |v: u32, m: u32| match m {
        0 => None,
        _ => Some(((v & m) >> m.trailing_zeros()) as u64 * 255 / (m >> m.trailing_zeros()) as u64),
    };
    for (px, s) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.chunks_exact(bpp))
    {
        let mut le = [0u8; 4];
        le[..bpp].copy_from_slice(s);
        let v = u32::from_le_bytes(le);
        let c = |m| channel(v, m).unwrap_or(0) as u8;
        *px = [c(r), c(g), c(b), channel(v, a).map_or(255, |a| a as u8)];
    }
    Ok(ImageData {
        width: w,
        height: h,
        data: out,
    })
}

#[cfg(not(target_arch = "wasm32"))]
mod native;

/// Bounded JPEG XR metadata, shared with the async browser decoder.
pub fn jxr_dpi(bytes: &[u8]) -> Option<f64> {
    jxr_container(bytes)?.dpi
}
/// The container fields of a JPEG XR file (T.832 Annex A) that `jxr` needs.
struct JxrContainer {
    guid: Option<[u8; 16]>,
    orientation: u32,
    /// IMAGE_OFFSET/BYTE_COUNT and ALPHA_OFFSET/BYTE_COUNT (a count of 0: to the end).
    image: (usize, usize),
    alpha: (usize, usize),
    /// WIDTH_RESOLUTION, when it is a positive FLOAT.
    dpi: Option<f64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl JxrContainer {
    fn plane(bytes: &[u8], (at, len): (usize, usize)) -> &[u8] {
        let rest = &bytes[at.min(bytes.len())..];
        if len == 0 {
            rest
        } else {
            &rest[..len.min(rest.len())]
        }
    }

    fn image<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        Self::plane(bytes, self.image)
    }

    fn alpha<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        (self.alpha.0 != 0).then(|| Self::plane(bytes, self.alpha))
    }
}

/// Reads the IFD, or `None` when it or a plane it points at runs past the end of the file: jxrlib
/// reads a truncated plane as zeros, so a cut file must not reach the decoder.
fn jxr_container(b: &[u8]) -> Option<JxrContainer> {
    if !b.starts_with(&[b'I', b'I', 0xBC]) {
        return None;
    }
    let ifd = u32::from_le_bytes(bytes_at(b, 4)?) as usize;
    let mut c = JxrContainer {
        guid: None,
        orientation: 0,
        image: (0, 0),
        alpha: (0, 0),
        dpi: None,
    };
    for i in 0..u16::from_le_bytes(bytes_at(b, ifd)?) as usize {
        let e: [u8; 12] = bytes_at(b, ifd.checked_add(2 + 12 * i)?)?;
        let kind = u16::from_le_bytes([e[2], e[3]]);
        let v = match kind {
            1 => e[8] as u32,                             // BYTE
            3 => u16::from_le_bytes([e[8], e[9]]) as u32, // SHORT
            _ => u32::from_le_bytes([e[8], e[9], e[10], e[11]]),
        };
        match u16::from_le_bytes([e[0], e[1]]) {
            0xBC01 => c.guid = Some(bytes_at(b, v as usize)?),
            0xBC02 => c.orientation = v,
            0xBC82 if kind == 11 => {
                let dpi = f32::from_bits(v) as f64;
                c.dpi = (dpi > 0.0 && dpi.is_finite()).then_some(dpi);
            }
            0xBCC0 => c.image.0 = v as usize,
            0xBCC1 => c.image.1 = v as usize,
            0xBCC2 => c.alpha.0 = v as usize,
            0xBCC3 => c.alpha.1 = v as usize,
            _ => {}
        }
    }
    let fits = |(at, len): (usize, usize)| at.checked_add(len).is_some_and(|end| end <= b.len());
    (fits(c.image) && fits(c.alpha)).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_order_and_aliases() {
        assert_eq!(
            detect_format("a.Tif", Some("image/png"), b"GIF89a"),
            Some("tiff")
        );
        assert_eq!(detect_format("a.heif", None, &[]), Some("heic"));
        assert_eq!(detect_format("a.wdp", None, &[]), Some("jxr"));
        assert_eq!(
            detect_format("a", Some("IMAGE/X-TGA"), b"\x89PNG\r\n\x1a\n"),
            Some("tga")
        );
        assert_eq!(
            detect_format("a", None, b"RIFF\0\0\0\0WEBPVP8L"),
            Some("webp")
        );
        assert_eq!(detect_format("a", None, b"MM\0*"), Some("tiff"));
        assert_eq!(detect_format("a", None, b"BM"), Some("bmp"));
        assert_eq!(
            detect_format("a", None, &[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("jpeg")
        );
        assert_eq!(detect_format("a", None, b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(detect_format("a.PSD", None, &[]), Some("psd"));
        assert_eq!(detect_format("a.psb", None, &[]), Some("psd"));
        assert_eq!(
            detect_format("a", Some("image/vnd.adobe.photoshop"), &[]),
            Some("psd")
        );
        assert_eq!(detect_format("a", None, b"8BPS\0\x01"), Some("psd"));
    }

    #[test]
    fn dpi_readers() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 9]);
        png.extend(b"pHYs");
        png.extend(3779u32.to_be_bytes());
        png.extend(3779u32.to_be_bytes());
        png.extend([1, 0, 0, 0, 0]);
        assert_eq!(png_dpi(&png), Some(3779.0 * 0.0254));
        png[24] = 0; // unit unknown
        assert_eq!(png_dpi(&png), None);

        let jfif = [
            0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 2, 0, 100, 0, 100,
        ];
        assert_eq!(jfif_dpi(&jfif), Some(254.0));

        let mut bmp = vec![0u8; 54];
        bmp[14] = 40;
        bmp[38..42].copy_from_slice(&2835i32.to_le_bytes());
        assert_eq!(bmp_dpi(&bmp), Some(2835.0 * 0.0254));

        // II*\0, IFD at 8: XResolution → 300/1 at offset 38, ResolutionUnit 3 (cm).
        let mut tiff = b"II*\0\x08\0\0\0\x02\0".to_vec();
        tiff.extend([26, 1, 5, 0, 1, 0, 0, 0, 38, 0, 0, 0]);
        tiff.extend([40, 1, 3, 0, 1, 0, 0, 0, 3, 0, 0, 0]);
        tiff.extend([0, 0, 0, 0]);
        tiff.extend(300u32.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
        assert_eq!(tiff_dpi(&tiff), Some(762.0));
        // Hostile offsets are just absent, never a panic.
        tiff[30] = 0xFF;
        tiff[31] = 0xFF;
        tiff[32] = 0xFF;
        tiff[33] = 0xFF;
        assert_eq!(tiff_dpi(&tiff), None);
    }

    /// §2.13: EXIF orientation is applied (6 = rotate 90° clockwise) and the JFIF density is the DPI.
    #[test]
    fn jpeg_orientation_and_density() {
        let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0".to_vec();
        exif.extend([0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0]);
        let mut jpeg = Vec::new();
        let mut e = jpeg_encoder::Encoder::new(&mut jpeg, 100);
        e.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
        e.set_density(jpeg_encoder::Density::Inch { x: 300, y: 300 });
        e.add_app_segment(1, &exif).unwrap();
        e.encode(&[255, 0, 0, 0, 0, 255], 2, 1, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        let d = decode("jpeg", &jpeg).unwrap();
        assert_eq!((d.image.width, d.image.height, d.dpi), (1, 2, Some(300.0)));
        let (top, bottom) = (&d.image.data[..4], &d.image.data[4..]);
        assert!(
            top[0] > 200 && top[2] < 60 && bottom[2] > 200 && bottom[0] < 60,
            "{:?}",
            d.image.data
        );
    }

    /// A valid 13 × 9 file per decoder.
    fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
        use super::super::encode::{SaveOptions, encode};
        use image::{DynamicImage, ImageFormat, RgbaImage};
        let rgba = crate::testing::fixture_rgba(13, 9, 4);
        let img = ImageData {
            width: 13,
            height: 9,
            data: rgba.clone(),
        };
        let dynamic = DynamicImage::ImageRgba8(RgbaImage::from_raw(13, 9, rgba.clone()).unwrap());
        let mut out: Vec<(&str, Vec<u8>)> = [
            ("png", ImageFormat::Png),
            ("gif", ImageFormat::Gif),
            ("bmp", ImageFormat::Bmp),
            ("tiff", ImageFormat::Tiff),
        ]
        .into_iter()
        .map(|(f, format)| {
            let mut v = Vec::new();
            dynamic.write_to(&mut Cursor::new(&mut v), format).unwrap();
            (f, v)
        })
        .collect();
        for f in ["jpeg", "webp", "dds", "jxl"] {
            out.push((f, encode(&img, f, &SaveOptions::default()).unwrap()));
        }
        let lossy = SaveOptions {
            quality: Some(50),
            ..SaveOptions::default()
        };
        out.push(("jxl", encode(&img, "jxl", &lossy).unwrap()));
        out.push(("webp", encode(&img, "webp", &lossy).unwrap()));
        // ravif: 8-bit 4:4:4 with an alpha item.
        #[cfg(all(feature = "avif-dav1d", not(target_arch = "wasm32")))]
        out.push(("avif", encode(&img, "avif", &lossy).unwrap()));
        // TGA: one raw RLE packet of 117 BGR pixels, top-down.
        let mut tga = vec![
            0, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 9, 0, 24, 0x20, 116,
        ];
        tga.extend(rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0]]));
        out.push(("tga", tga));
        out
    }

    /// Review Focus 5: every truncation and a corrupted byte at every offset returns Ok or Err.
    #[test]
    fn hostile_inputs_never_panic() {
        for (f, good) in fixtures() {
            let d = decode(f, &good).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert_eq!((d.image.width, d.image.height), (13, 9), "{f}");
            for cut in 0..good.len() {
                let _ = decode(f, &good[..cut]);
            }
            for at in 0..good.len() {
                let mut b = good.clone();
                b[at] ^= 0xA5;
                let _ = decode(f, &b);
            }
        }
    }

    #[test]
    fn size_caps() {
        assert_eq!(check_size(32768, 8192), Ok(1 << 30));
        assert!(check_size(32769, 1).is_err() && check_size(32768, 8193).is_err());
        assert!(check_size(0, 1).is_err());
    }

    /// §2.1: the WIC types sniff the content, so a mislabeled file opens as what it is, and a
    /// WIC-detected WebP goes to the WebP decoder. TGA and the plugin types do not sniff.
    #[test]
    fn wic_types_sniff_the_content() {
        let fx = fixtures();
        let get = |f: &str| fx.iter().find(|x| x.0 == f).unwrap().1.clone();
        for (label, real) in [
            ("jpeg", "png"),
            ("jpeg", "webp"),
            ("png", "gif"),
            ("bmp", "tiff"),
            ("tiff", "jpeg"),
            ("gif", "bmp"),
        ] {
            let d = decode(label, &get(real)).unwrap_or_else(|e| panic!("{real} as .{label}: {e}"));
            assert_eq!(
                (d.image.width, d.image.height),
                (13, 9),
                "{real} as .{label}"
            );
        }
        assert!(decode("tga", &get("png")).is_err());
        assert!(decode("webp", &get("png")).is_err());
        assert!(decode("png", &get("dds")).is_err());
    }

    /// DDS: a BC1 block over a 3 × 2 image (partial block), and legacy A8R8G8B8 masks.
    #[test]
    fn dds_block_and_mask_formats() {
        let mut bc1 = [0u8; 128];
        bc1[..4].copy_from_slice(b"DDS ");
        for (at, v) in [
            (4, 124u32),
            (8, 0x1007),
            (12, 2),
            (16, 3),
            (76, 32),
            (80, 4),
            (108, 0x1000),
        ] {
            bc1[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        bc1[84..88].copy_from_slice(b"DXT1");
        let mut file = bc1.to_vec();
        file.extend([0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0]); // colour 0 = red (565), every index 0
        assert_eq!(
            decode("dds", &file).unwrap().image.data,
            [255u8, 0, 0, 255].repeat(6)
        );
        assert!(decode("dds", &file[..file.len() - 1]).is_err());

        let img = ImageData {
            width: 1,
            height: 1,
            data: vec![10, 20, 30, 40],
        };
        let mut argb = super::super::encode::encode_dds(&img);
        argb[92..96].copy_from_slice(&0xFF_0000u32.to_le_bytes());
        argb[100..104].copy_from_slice(&0xFFu32.to_le_bytes());
        assert_eq!(decode("dds", &argb).unwrap().image.data, [30, 20, 10, 40]);
    }

    /// 70 zero-duration frames (cjxl 0.12 from a 70-frame GIF with delay 0) put keyframe 0 at
    /// index 69, past `MAX_JXL_FRAMES_BEFORE_KEYFRAME`.
    #[test]
    fn jxl_frames_before_the_first_keyframe_are_bounded() {
        let bytes = include_bytes!("../../../../tests/fixtures/jxl-70-zero-duration-frames.jxl");
        let e = decode("jxl", bytes).unwrap_err();
        assert!(e.to_string().contains("too many frames"), "{e}");
    }

    /// Task 48: dav1d's frame size limit and the JPEG XR output buffers follow the budget (the
    /// HEVC SPS check is `decode_limits.rs`'s `heic_sps_32768x16384`, under its allocation guard).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_decoders_budget_their_pictures() {
        let fx = |n: &str| {
            std::fs::read(
                concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/").to_owned() + n,
            )
            .unwrap()
        };
        let edge = |format: &str, file: &[u8], ok: u64| {
            assert!(decode_within(format, file, ok).is_ok(), "{format} at {ok}");
            let e = decode_within(format, file, ok - 1).unwrap_err();
            assert!(
                e.to_string().contains("too large"),
                "{format} at {}: {e}",
                ok - 1
            );
        };
        #[cfg(feature = "avif-dav1d")]
        {
            // dav1d may hold budget / 2 of pictures: 12-bit 4:2:0 is 3 bytes a pixel, and 8-bit
            // 4:2:0 with film grain 1.5 twice over; both 117 pixels × 6. 8-bit 4:4:4 lossless
            // with alpha: 3 bytes a pixel, then the 8-bit 4:0:0 alpha item (1, under the RGBA
            // canvas's 4).
            edge("avif", &fx("a.avif"), 13 * 9 * 6);
            edge("avif", &fx("avif-film-grain.avif"), 13 * 9 * 6);
            edge("avif", &fx("avif-lossless-alpha.avif"), 13 * 9 * 6);
        }
        // 64bppRGBAHalf: a 48bppRGBHalf colour codestream (6 bytes a pixel) and a half alpha plane.
        edge("jxr", &fx("a-rgba64half.jxr"), 13 * 9 * 6);
    }

    /// AV1 §5.5: the sequence header fields that size dav1d's pictures, folded to the worst case
    /// over an item's sequence headers.
    #[cfg(all(feature = "avif-dav1d", not(target_arch = "wasm32")))]
    #[test]
    fn av1_sequence_header_fields() {
        use super::native::{Av1Sequence, av1_sequence};
        // An OBU of `kind` with a one-byte size: (value, bit count) fields MSB-first.
        let obu = |kind: u8, fields: &[(u64, u32)]| {
            let bits: Vec<u8> = fields
                .iter()
                .flat_map(|&(v, n)| (0..n).rev().map(move |i| (v >> i & 1) as u8))
                .collect();
            let payload: Vec<u8> = bits
                .chunks(8)
                .map(|c| c.iter().enumerate().fold(0, |b, (i, &x)| b | x << (7 - i)))
                .collect();
            [vec![kind << 3 | 2, payload.len() as u8], payload].concat()
        };
        let seq = |w, h, cost4, film_grain| {
            Some(Av1Sequence {
                max_width: w,
                max_height: h,
                cost4,
                film_grain,
            })
        };
        // Reduced still-picture headers ending in color_config and film_grain_params_present:
        // profile 0 8-bit 4:2:0, profile 0 10-bit 4:0:0, profile 1 sRGB identity (4:4:4 with no
        // color_range bit).
        let reduced = |profile: u64, colour: &[(u64, u32)]| {
            let mut f = vec![(profile, 3), (1, 1), (1, 1), (0, 5)];
            f.extend([(3, 4), (3, 4), (12, 4), (8, 4), (0, 3), (0, 3)]);
            f.extend(colour);
            obu(1, &f)
        };
        let yuv420 = reduced(0, &[(0, 1), (0, 1), (0, 1), (0, 1), (0, 2), (0, 1), (0, 1)]);
        assert_eq!(av1_sequence(&yuv420), seq(13, 9, 6, false));
        let mono10 = reduced(0, &[(1, 1), (1, 1), (0, 1), (0, 1), (0, 1)]);
        assert_eq!(av1_sequence(&mono10), seq(13, 9, 8, false));
        let identity = reduced(
            1,
            &[(0, 1), (1, 1), (1, 8), (13, 8), (0, 8), (0, 1), (0, 1)],
        );
        assert_eq!(av1_sequence(&identity), seq(13, 9, 12, false));
        // A full header: timing info with a uvlc picture interval, a decoder model with 10-bit
        // delays, two operating points (one with a tier bit and both optional blocks), frame ids,
        // order hints and screen content tools, then profile 2 12-bit 4:4:4 BT.2020 with grain.
        let full = obu(
            1,
            &[
                (2, 3),
                (0, 1),
                (0, 1),
                (1, 1),
                (0, 32),
                (0, 32),
                (1, 1),
                (0b00101, 5),
                (1, 1),
                (9, 5),
                (0, 32),
                (0, 10),
                (1, 1),
                (1, 5),
                (0, 12),
                (9, 5),
                (1, 1),
                (1, 1),
                (0, 21),
                (1, 1),
                (7, 4),
                (0, 12),
                (2, 5),
                (0, 1),
                (0, 1),
                (11, 4),
                (11, 4),
                (3999, 12),
                (2999, 12),
                (1, 1),
                (0, 7),
                (0, 3),
                (0, 4),
                (1, 1),
                (0, 2),
                (0, 1),
                (1, 1),
                (0, 1),
                (0, 1),
                (6, 3),
                (0, 3),
                (1, 1),
                (1, 1),
                (0, 1),
                (1, 1),
                (9, 8),
                (16, 8),
                (9, 8),
                (1, 1),
                (0, 1),
                (0, 1),
                (1, 1),
            ],
        );
        assert_eq!(av1_sequence(&full), seq(4000, 3000, 24, true));
        // A temporal delimiter, then two headers: the larger of each wins.
        let both = [obu(2, &[]), yuv420.clone(), mono10.clone()].concat();
        assert_eq!(av1_sequence(&both), seq(13, 9, 8, false));
        let mut extended = yuv420.clone();
        extended[0] |= 4; // obu_extension_flag: one more header byte
        extended.insert(1, 0);
        assert_eq!(av1_sequence(&extended), seq(13, 9, 6, false));
        assert_eq!(av1_sequence(&obu(2, &[])), None);
        for cut in 0..full.len() {
            assert_eq!(av1_sequence(&full[..cut]), None, "cut at {cut}");
        }
    }

    /// H.265 §7.3.2.2: the picture size behind sub-layer profile/level blocks and emulation
    /// prevention bytes; other NAL units and layers are not SPSs.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn hevc_sps_picture_size() {
        use super::native::hevc_sps_size;
        // (value, bits) MSB-first; `None` bits = ue(v).
        let pack = |fields: &[(u64, Option<u32>)]| {
            let mut bits = Vec::new();
            for &(v, n) in fields {
                let (v, n) = n.map_or_else(
                    || {
                        let lz = 63 - (v + 1).leading_zeros();
                        bits.extend(std::iter::repeat_n(0, lz as usize));
                        (v + 1, lz + 1)
                    },
                    |n| (v, n),
                );
                bits.extend((0..n).rev().map(|i| (v >> i & 1) as u8));
            }
            bits.push(1); // rbsp_stop_one_bit
            let rbsp: Vec<u8> = bits
                .chunks(8)
                .map(|c| c.iter().enumerate().fold(0, |b, (i, &x)| b | x << (7 - i)))
                .collect();
            let mut nal = vec![33 << 1, 1];
            let mut zeros = 0;
            for b in rbsp {
                if zeros >= 2 && b <= 3 {
                    nal.push(3);
                    zeros = 0;
                }
                zeros = if b == 0 { zeros + 1 } else { 0 };
                nal.push(b);
            }
            nal
        };
        let ue = |v: u64| (v, None);
        let u = |v: u64, n: u32| (v, Some(n));
        // Three sub-layers: the first with profile and level, the second with level only; a
        // zero-filled profile puts emulation prevention bytes in the stream.
        let sps = pack(&[
            u(0, 4),
            u(2, 3),
            u(1, 1),
            u(0, 64),
            u(0, 32), // general profile_tier_level (96 bits)
            u(0b11, 2),
            u(0b01, 2),
            u(0, 12), // sub-layer flags, alignment to 8 entries
            u(0, 64),
            u(0, 24),
            u(0, 8), // sub-layer 0 profile (88) and level (8)
            u(0, 8), // sub-layer 1 level
            ue(0),
            ue(3),
            u(0, 1), // sps id, chroma_format_idc 3, separate_colour_plane_flag
            ue(40000),
            ue(13),
        ]);
        assert!(sps.windows(3).any(|w| w == [0, 0, 3]), "{sps:?}");
        assert_eq!(hevc_sps_size(&sps), Ok(Some((40000, 13))));
        let mut pps = sps.clone();
        pps[0] = 34 << 1;
        assert_eq!(hevc_sps_size(&pps), Ok(None));
        let mut layer1 = sps.clone();
        layer1[1] = 1 << 3 | 1;
        assert_eq!(hevc_sps_size(&layer1), Ok(None));
        assert!(hevc_sps_size(&sps[..sps.len() - 3]).is_err());
    }
}
