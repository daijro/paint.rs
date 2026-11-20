//! JPEG XR decode on every target (the web build too, so it previews and reopens what it saves).

use super::{CodecError, check_size, invalid, jxr_container};
use crate::canvas::ImageData;
use crate::pixel::gamma::{linear_to_srgb8, quantize};

/// JPEG XR (Ruling R51; formats-matrix.md §3.8) through openreadout-jpegxr, Microsoft's jxrlib
/// decoder ported without `unsafe`. That crate refuses alpha pixel formats, so an alpha plane
/// stored as its own codestream (the container's ALPHA_OFFSET: planar alpha, which WIC, and so
/// Paint.NET, writes for 32bppBGRA) is decoded here as a second bare codestream and merged; alpha
/// interleaved in the image codestream stays Unsupported. Read: 8-bit RGB, BGR and grey, and half
/// and float scRGB (linear, sRGB-encoded and clamped like WIC's converter), with straight or
/// premultiplied alpha. The DPI is WIDTH_RESOLUTION (§2.13: DPI > 0 is used).
pub(super) fn decode(bytes: &[u8], budget: u64) -> Result<(ImageData, Option<f64>), CodecError> {
    use openreadout_jpegxr::{Error as E, SampleType as S};
    let file = jxr_container(bytes).ok_or(invalid("truncated or corrupt JPEG XR file"))?;
    let max = usize::try_from(budget).unwrap_or(usize::MAX);
    let err = |e: E| match e {
        E::Unsupported(m) => CodecError::Unsupported(format!("JPEG XR: {m}")),
        e => invalid(e),
    };
    // The headers' sizes against the caps, before a pixel is decoded.
    let decode = |data: &[u8]| -> Result<openreadout_jpegxr::Image, CodecError> {
        let i = openreadout_jpegxr::probe(data).map_err(err)?;
        check_size(i.width, i.height)?;
        let len = i.width as u64 * i.height as u64 * i.channels as u64 * i.sample.bytes() as u64;
        if len > budget {
            return Err(invalid(format!(
                "image too large ({} × {})",
                i.width, i.height
            )));
        }
        openreadout_jpegxr::decode(data, max).map_err(err)
    };
    let refused = || CodecError::Unsupported("this JPEG XR pixel format is not supported".into());
    // The jxrlib GUIDs share their first 15 bytes; the 16th names the format.
    const GUID_PREFIX: [u8; 15] = [
        0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
    ];
    let format = match file.guid {
        Some(g) if g[..15] == GUID_PREFIX => Some(g[15]),
        _ => None,
    };
    // 32bppBGRA, 32bppPBGRA, 64bppRGBAHalf, 128bppRGBAFloat, 128bppPRGBAFloat.
    let planar = match format {
        Some(0x0F | 0x10) => Some(S::U8),
        Some(0x3A) => Some(S::F16),
        Some(0x19 | 0x1A) => Some(S::F32),
        Some(0x20..=0x2B) => return Err(refused()), // N-channel: not colour
        _ => None,
    };
    let premultiplied = matches!(format, Some(0x10 | 0x1A));
    let (colour, alpha) = match planar {
        Some(sample) => {
            if file.orientation != 0 {
                return Err(CodecError::Unsupported(
                    "JPEG XR: rotated or flipped orientation".into(),
                ));
            }
            let colour = decode(file.image(bytes))?;
            if colour.channels != 3 || colour.sample != sample {
                return Err(invalid("JPEG XR pixel format and codestream disagree"));
            }
            let alpha = match file.alpha(bytes) {
                Some(stream) => {
                    let a = decode(stream)?;
                    if a.channels != 1 || (a.width, a.height) != (colour.width, colour.height) {
                        return Err(invalid("JPEG XR alpha plane does not match the image"));
                    }
                    Some(a)
                }
                // ponytail: an alpha format without an alpha plane decodes opaque.
                None => None,
            };
            (colour, alpha)
        }
        None => (decode(bytes)?, None),
    };
    let reader = |s: S| -> Result<fn(&[u8]) -> f32, CodecError> {
        Ok(match s {
            S::U8 => |b| b[0] as f32 / 255.0,
            S::F16 => |b| half::f16::from_le_bytes([b[0], b[1]]).to_f32(),
            S::F32 => |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => return Err(refused()),
        })
    };
    let order = match (colour.channels, colour.bgr) {
        (3, true) => [2, 1, 0],
        (3, false) => [0, 1, 2],
        (1, _) => [0; 3],
        _ => return Err(refused()),
    };
    let (read, linear) = (reader(colour.sample)?, colour.sample != S::U8);
    let (sample, pixel) = (
        colour.sample.bytes(),
        colour.channels as usize * colour.sample.bytes(),
    );
    let alpha = match &alpha {
        Some(a) => Some((reader(a.sample)?, a.sample.bytes(), &a.data)),
        None => None,
    };
    let len = check_size(colour.width, colour.height)?;
    let mut data = Vec::with_capacity(len);
    for (i, px) in colour.data.chunks_exact(pixel).enumerate() {
        let a = alpha.map_or(1.0, |(read, size, plane)| read(&plane[i * size..]));
        let unmultiply = premultiplied && a > 0.0;
        let c = order.map(|c| {
            let v = read(&px[c * sample..]);
            let v = if unmultiply { v / a } else { v };
            if linear {
                linear_to_srgb8(v)
            } else {
                quantize(v)
            }
        });
        data.extend([c[0], c[1], c[2], quantize(a)]);
    }
    let image = ImageData {
        width: colour.width,
        height: colour.height,
        data,
    };
    Ok((image, file.dpi))
}
