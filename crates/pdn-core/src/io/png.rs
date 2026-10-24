//! PNG writer for the 8-bit pipeline, hand-written per ISO/IEC 15948.
//! Paint.NET's PNG: the chunk order of a metadata-free WIC PNG is IHDR, sRGB (0), gAMA (45455), pHYs, IDAT (zlib
//! header 78 5E), IEND. No Software tEXt (Ruling (P3) 8).
// WIC container details unverified (document-ops-and-formats.md §6 item 4; B-F4): where WIC puts
// PLTE/tRNS, its row filters and IDAT splitting. Here PLTE and tRNS follow pHYs, every row has
// filter 0 and there is one IDAT.

use super::encode::{Depth, Raster};
use super::pdn::crc32;

/// The largest chunk length PNG allows (2^31 − 1).
const MAX_CHUNK: usize = 0x7FFF_FFFF;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    assert!(data.len() <= MAX_CHUNK, "PNG chunk too long");
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

pub(crate) fn write(r: &Raster, dpi: f64) -> Vec<u8> {
    let (bits, colour_type) = match r.depth {
        Depth::B24 => (8, 2),
        Depth::B32 => (8, 6),
        d => (d.bits() as u8, 3),
    };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let ihdr = [
        &r.w.to_be_bytes()[..],
        &r.h.to_be_bytes(),
        &[bits, colour_type, 0, 0, 0],
    ];
    chunk(&mut out, b"IHDR", &ihdr.concat());
    chunk(&mut out, b"sRGB", &[0]);
    chunk(&mut out, b"gAMA", &45455u32.to_be_bytes());
    let ppm = ((dpi / 0.0254) as u32).to_be_bytes();
    chunk(&mut out, b"pHYs", &[&ppm[..], &ppm, &[1]].concat());
    if colour_type == 3 {
        let plte: Vec<u8> = r.palette.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        chunk(&mut out, b"PLTE", &plte);
        if r.palette.iter().any(|p| p[3] == 0) {
            let alpha: Vec<u8> = r.palette.iter().map(|p| p[3]).collect();
            chunk(&mut out, b"tRNS", &alpha);
        }
    }
    let raw: Vec<u8> = (0..r.h)
        .flat_map(|y| std::iter::once(0).chain(r.row(y)))
        .collect();
    for part in miniz_oxide::deflate::compress_to_vec_zlib(&raw, 3).chunks(MAX_CHUNK) {
        chunk(&mut out, b"IDAT", part);
    }
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::super::codecs::decode;
    use super::super::encode::{BitDepth, SaveOptions, encode};
    use super::crc32;
    use crate::canvas::ImageData;

    /// (type, data) of every chunk; checks each CRC.
    fn chunks(b: &[u8]) -> Vec<(String, Vec<u8>)> {
        assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n");
        let mut out = vec![];
        let mut at = 8;
        while at < b.len() {
            let len = u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            let body = &b[at + 4..at + 8 + len];
            let crc = u32::from_be_bytes(b[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(body));
            out.push((
                String::from_utf8(body[..4].to_vec()).unwrap(),
                body[4..].to_vec(),
            ));
            at += 12 + len;
        }
        out
    }

    fn opts(bit_depth: BitDepth) -> SaveOptions {
        SaveOptions {
            bit_depth,
            ..SaveOptions::default_for("png")
        }
    }

    /// 2-bit: IHDR, the lossless palette in first-seen order, MSB-first packing behind filter 0,
    /// zlib header 78 5E, and the chunk order with PLTE after pHYs.
    #[test]
    fn indexed_layout() {
        let (r, b, g) = ([255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]);
        let img = ImageData {
            width: 5,
            height: 2,
            data: [r, b, r, g, b, g, g, r, r, b].concat(),
        };
        let bytes = encode(&img, "png", &opts(BitDepth::B2)).unwrap();
        let c = chunks(&bytes);
        let kinds: Vec<&str> = c.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(
            kinds,
            ["IHDR", "sRGB", "gAMA", "pHYs", "PLTE", "IDAT", "IEND"]
        );
        assert_eq!(c[0].1, [0, 0, 0, 5, 0, 0, 0, 2, 2, 3, 0, 0, 0]);
        assert_eq!(c[1].1, [0]);
        assert_eq!(c[2].1, 45455u32.to_be_bytes());
        assert_eq!(c[3].1, [0, 0, 0x0E, 0xC3, 0, 0, 0x0E, 0xC3, 1]);
        assert_eq!(c[4].1, [255, 0, 0, 0, 0, 255, 0, 255, 0]);
        assert_eq!(&c[5].1[..2], [0x78, 0x5E]);
        // Indices [0 1 0 2 1] and [2 2 0 0 1], two bits each from the high end.
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&c[5].1).unwrap();
        assert_eq!(
            raw,
            [0, 0b0001_0010, 0b0100_0000, 0, 0b1010_0000, 0b0100_0000]
        );
        assert_eq!(decode("png", &bytes).unwrap().image.data, img.data);
    }

    /// 8A1: PLTE with the transparent entry last as black and a tRNS whose last entry is 0.
    #[test]
    fn transparency_is_the_last_entry() {
        let img = ImageData {
            width: 4,
            height: 1,
            data: [[9, 9, 9, 0], [1, 2, 3, 255], [0, 0, 0, 0], [4, 5, 6, 255]].concat(),
        };
        let bytes = encode(&img, "png", &opts(BitDepth::B8)).unwrap();
        let c = chunks(&bytes);
        assert_eq!(c[0].1[8..10], [8, 3]);
        assert_eq!(
            (c[4].0.as_str(), &c[4].1[..]),
            ("PLTE", &[1, 2, 3, 4, 5, 6, 0, 0, 0][..])
        );
        assert_eq!((c[5].0.as_str(), &c[5].1[..]), ("tRNS", &[255, 255, 0][..]));
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&c[6].1).unwrap();
        assert_eq!(raw, [0, 2, 0, 2, 1]);
        assert_eq!(
            decode("png", &bytes).unwrap().image.data,
            [[0, 0, 0, 0], [1, 2, 3, 255], [0, 0, 0, 0], [4, 5, 6, 255]].concat()
        );
    }

    /// 24-bit is RGB flattened onto white (A = 0 → white); 32-bit is RGBA with the hidden RGB
    /// kept; pHYs follows the dpi.
    #[test]
    fn truecolour_rows_and_dpi() {
        let img = ImageData {
            width: 2,
            height: 1,
            data: vec![10, 20, 30, 0, 100, 0, 0, 128],
        };
        let raw = |d, dpi| {
            let o = SaveOptions { dpi, ..opts(d) };
            let c = chunks(&encode(&img, "png", &o).unwrap());
            let idat = &c.iter().find(|c| c.0 == "IDAT").unwrap().1;
            (
                c[0].1[8..10].to_vec(),
                c[3].1[..4].to_vec(),
                miniz_oxide::inflate::decompress_to_vec_zlib(idat).unwrap(),
            )
        };
        let (ihdr, phys, rgb) = raw(BitDepth::B24, 300.0);
        assert_eq!((ihdr, phys), (vec![8, 2], 11811u32.to_be_bytes().to_vec()));
        assert_eq!(rgb, [0, 255, 255, 255, 177, 127, 127]);
        let (ihdr, _, rgba) = raw(BitDepth::B32, 96.0);
        assert_eq!(ihdr, [8, 6]);
        assert_eq!(rgba, [&[0][..], &img.data].concat());
    }
}
