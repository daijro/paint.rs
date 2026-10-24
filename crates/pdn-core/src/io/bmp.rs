//! BMP writer for the 8-bit pipeline (hand-written; BITMAPINFOHEADER / BITMAPV5HEADER).
//! Paint.NET's 32-bit BMP uses the V5 header (`EnableV5Header32bppBGRA`); bottom-up, as WIC is
//! inferred to write it.
// WIC container details unverified (document-ops-and-formats.md §6 item 4; B-F4): the V5 colour
// space fields (LCS_sRGB, LCS_GM_IMAGES here), the palette entry count and the pixels-per-metre
// rounding.

use super::codecs::{CodecError, invalid};
use super::encode::{Depth, Raster};

pub(crate) fn write(r: &Raster, dpi: f64) -> Result<Vec<u8>, CodecError> {
    let bits = r.depth.bits();
    let stride = (r.w as u64 * bits as u64).div_ceil(32) * 4;
    let image_size = stride * r.h as u64;
    let (header, colours) = match r.depth {
        Depth::B32 => (124, 0),
        Depth::B24 => (40, 0),
        _ => (40, 1u32 << bits),
    };
    let offset = 14 + header + 4 * colours;
    let file_size = offset as u64 + image_size;
    if r.w > i32::MAX as u32 || r.h > i32::MAX as u32 || file_size > u32::MAX as u64 {
        return Err(invalid("image too large for BMP"));
    }
    let ppm = (dpi / 0.0254) as u32;
    let mut out = Vec::with_capacity(file_size as usize);
    out.extend(b"BM");
    for v in [file_size as u32, 0, offset, header, r.w, r.h] {
        out.extend(v.to_le_bytes());
    }
    out.extend(1u16.to_le_bytes());
    out.extend((bits as u16).to_le_bytes());
    let compression = if bits == 32 { 3 } else { 0 }; // BI_BITFIELDS : BI_RGB
    for v in [compression, image_size as u32, ppm, ppm, colours, 0] {
        out.extend(v.to_le_bytes());
    }
    if bits == 32 {
        // Masks R, G, B, A; CSType LCS_sRGB; 36 bytes of endpoints and 12 of gamma (unused);
        // intent LCS_GM_IMAGES; no profile; reserved.
        for v in [0x00FF_0000u32, 0xFF00, 0xFF, 0xFF00_0000, 0x7352_4742] {
            out.extend(v.to_le_bytes());
        }
        out.extend([0; 48]);
        for v in [4u32, 0, 0, 0] {
            out.extend(v.to_le_bytes());
        }
    }
    for i in 0..colours as usize {
        let [red, green, blue, _] = r.palette.get(i).copied().unwrap_or_default();
        out.extend([blue, green, red, 0]);
    }
    for y in (0..r.h).rev() {
        let mut row = r.row(y);
        match bits {
            24 => row
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .for_each(|p| p.swap(0, 2)),
            32 => row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|p| p.swap(0, 2)),
            _ => {}
        }
        row.resize(stride as usize, 0);
        out.extend(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::codecs::decode;
    use super::super::encode::{BitDepth, SaveOptions, encode};
    use crate::canvas::ImageData;

    fn bmp(img: &ImageData, bit_depth: BitDepth) -> Vec<u8> {
        let o = SaveOptions {
            bit_depth,
            ..SaveOptions::default_for("bmp")
        };
        encode(img, "bmp", &o).unwrap()
    }

    fn le32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// 32-bit: file header, the 124-byte V5 header with BI_BITFIELDS masks, then bottom-up BGRA
    /// rows that keep the colour of A = 0.
    #[test]
    fn v5_header_at_32_bit() {
        let img = ImageData {
            width: 1,
            height: 2,
            data: vec![10, 20, 30, 0, 40, 50, 60, 128],
        };
        let b = bmp(&img, BitDepth::B32);
        assert_eq!(b.len(), 138 + 8);
        assert_eq!(&b[..2], b"BM");
        assert_eq!([le32(&b, 2), le32(&b, 6), le32(&b, 10)], [146, 0, 138]);
        let fields: Vec<u32> = (0..10).map(|i| le32(&b, 14 + 4 * i)).collect();
        assert_eq!(&b[26..28], [1, 0]); // planes
        assert_eq!(&b[28..30], [32, 0]);
        assert_eq!(fields[0], 124);
        assert_eq!([fields[1], fields[2]], [1, 2]);
        assert_eq!(&fields[4..], [3, 8, 3779, 3779, 0, 0]);
        let masks: Vec<u32> = (0..4).map(|i| le32(&b, 54 + 4 * i)).collect();
        assert_eq!(masks, [0x00FF_0000, 0xFF00, 0xFF, 0xFF00_0000]);
        assert_eq!(&b[70..74], b"BGRs");
        assert_eq!(&b[138..], [60, 50, 40, 128, 30, 20, 10, 0]);
        assert_eq!(decode("bmp", &b).unwrap().image.data, img.data);
    }

    /// 24-bit and 4-bit: the 40-byte header, a 2^bpp-entry BGRX palette, rows bottom-up with
    /// MSB-first indices, padded to 4 bytes.
    #[test]
    fn info_header_palette_and_padding() {
        let (r, g, b) = ([255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]);
        let img = ImageData {
            width: 3,
            height: 2,
            data: [r, g, b, b, b, r].concat(),
        };
        let rgb = bmp(&img, BitDepth::B24);
        assert_eq!(
            [le32(&rgb, 10), le32(&rgb, 14), le32(&rgb, 30)],
            [54, 40, 0]
        );
        assert_eq!(&rgb[28..30], [24, 0]);
        assert_eq!([le32(&rgb, 34), le32(&rgb, 46)], [24, 0]);
        assert_eq!(
            &rgb[54..],
            [
                255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 0, 0
            ]
        );
        let four = bmp(&img, BitDepth::B4);
        assert_eq!([le32(&four, 10), le32(&four, 46)], [54 + 64, 16]);
        assert_eq!(&four[28..30], [4, 0]);
        assert_eq!(&four[54..66], [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0]);
        assert!(four[66..118].iter().all(|&v| v == 0));
        // Row 1 (stored first) is [b b r] = [2 2 0], row 0 is [r g b] = [0 1 2].
        assert_eq!(&four[118..], [0x22, 0x00, 0, 0, 0x01, 0x20, 0, 0]);
        for d in [
            BitDepth::B1,
            BitDepth::B4,
            BitDepth::B8,
            BitDepth::B24,
            BitDepth::B32,
        ] {
            let two = ImageData {
                width: 9,
                height: 3,
                data: [r, b].concat().repeat(14)[..108].to_vec(),
            };
            let bytes = bmp(&two, d);
            assert_eq!(decode("bmp", &bytes).unwrap().image.data, two.data, "{d:?}");
        }
    }
}
