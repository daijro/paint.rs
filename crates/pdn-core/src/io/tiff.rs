//! TIFF writer for the 8-bit pipeline: hand-written TIFF 6.0 baseline, little-endian,
//! uncompressed strips of about 8 KB (ruling R38). RGB (2) or palette (3) with a 16-bit
//! ColorMap; 32-bit adds ExtraSamples 2 (unassociated alpha).
//! Also the palette-TIFF read path that `codecs::decode` uses, since `image` refuses those.
// compression: WIC "DontCare" default unverified (B-F4); the tag set and strip size are
// unverified too (document-ops-and-formats.md §6 item 4).

use std::io::Cursor;

use image::metadata::Orientation;

use super::codecs::{CodecError, bytes_at, check_size, invalid};
use super::encode::Raster;
use crate::canvas::ImageData;

const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

pub(crate) fn write(r: &Raster, dpi: f64) -> Result<Vec<u8>, CodecError> {
    let bits = r.depth.bits();
    let indexed = r.depth.is_indexed();
    let spp: u16 = match bits {
        24 => 3,
        32 => 4,
        _ => 1,
    };
    let bps = if indexed { bits as u16 } else { 8 };
    let row_bytes = (r.w as u64 * bits as u64).div_ceil(8);
    // ruling R38: strips of about 8 KB (TIFF 6.0's recommendation), not the brief's one strip:
    // `image` caps a strip at budget − image bytes. WIC's strip size is unverified (B-F4).
    let rows = (8192 / row_bytes).max(1);
    let strips = (r.h as u64).div_ceil(rows);
    let strip_len = row_bytes * r.h as u64;
    if strip_len + 8 * strips > u32::MAX as u64 {
        return Err(invalid("image too large for TIFF"));
    }
    fn longs(v: impl Iterator<Item = u64>) -> Vec<u8> {
        v.flat_map(|x| (x as u32).to_le_bytes()).collect()
    }
    let counts = longs((0..strips).map(|k| (r.h as u64 - k * rows).min(rows) * row_bytes));
    let shorts = |v: &[u16]| v.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>();
    let res = [
        ((dpi * 10000.0).round() as u32).to_le_bytes(),
        10000u32.to_le_bytes(),
    ]
    .concat();
    // (tag, type, count, value bytes), ascending by tag; StripOffsets (273) is filled in below.
    let mut tags: Vec<(u16, u16, u32, Vec<u8>)> = vec![
        (256, LONG, 1, r.w.to_le_bytes().to_vec()),
        (257, LONG, 1, r.h.to_le_bytes().to_vec()),
        (258, SHORT, spp as u32, shorts(&vec![bps; spp as usize])),
        (259, SHORT, 1, shorts(&[1])),
        (262, SHORT, 1, shorts(&[if indexed { 3 } else { 2 }])),
        (273, LONG, strips as u32, vec![0; 4 * strips as usize]),
        (277, SHORT, 1, shorts(&[spp])),
        (278, LONG, 1, (rows as u32).to_le_bytes().to_vec()),
        (279, LONG, strips as u32, counts),
        (282, RATIONAL, 1, res.clone()),
        (283, RATIONAL, 1, res),
        (296, SHORT, 1, shorts(&[2])),
    ];
    if indexed {
        let n = 1 << bits;
        let channel =
            |c: usize| (0..n).map(move |i| r.palette.get(i).map_or(0, |p| p[c] as u16 * 257));
        let map: Vec<u16> = (0..3).flat_map(channel).collect();
        tags.push((320, SHORT, map.len() as u32, shorts(&map)));
    }
    if bits == 32 {
        tags.push((338, SHORT, 1, shorts(&[2])));
    }
    // Header, IFD, the values longer than 4 bytes (each at an even offset), then the strips.
    let ifd_end = 8 + 2 + 12 * tags.len() + 4;
    let extra: usize = tags
        .iter()
        .filter(|t| t.3.len() > 4)
        .map(|t| t.3.len().next_multiple_of(2))
        .sum();
    let strip_at = ifd_end + extra;
    let total = strip_at as u64 + strip_len;
    if total > u32::MAX as u64 {
        return Err(invalid("image too large for TIFF"));
    }
    tags[5].3 = longs((0..strips).map(|k| strip_at as u64 + k * rows * row_bytes));
    let mut out = Vec::with_capacity(total as usize);
    out.extend(b"II\x2a\x00\x08\x00\x00\x00");
    out.extend((tags.len() as u16).to_le_bytes());
    let mut values = Vec::new();
    for (tag, kind, count, v) in &tags {
        out.extend(tag.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(count.to_le_bytes());
        if v.len() <= 4 {
            out.extend(v);
            out.extend(vec![0; 4 - v.len()]);
        } else {
            out.extend(((ifd_end + values.len()) as u32).to_le_bytes());
            values.extend(v);
            values.resize(values.len().next_multiple_of(2), 0);
        }
    }
    out.extend([0; 4]); // no next IFD
    out.extend(values);
    for y in 0..r.h {
        out.extend(r.row(y));
    }
    Ok(out)
}

/// A palette TIFF (IFD0 PhotometricInterpretation 3), which `image` refuses: a copy re-tagged
/// BlackIsZero (1) goes through `tiff`, which returns the packed indices unscaled, and the
/// ColorMap maps them (c >> 8 undoes the writer's c·257; A = 255). The Orientation tag applies
/// as on `image`'s TIFF path (§2.13). `Ok(None)` when IFD0 is not a palette image (or not
/// classic TIFF; BigTIFF is left to `image`).
pub(crate) fn decode_palette(b: &[u8], budget: u64) -> Result<Option<ImageData>, CodecError> {
    let bad = |m: &str| Err(invalid(format!("TIFF palette: {m}")));
    let le = match bytes_at::<4>(b, 0) {
        Some([b'I', b'I', 42, 0]) => true,
        Some([b'M', b'M', 0, 42]) => false,
        _ => return Ok(None),
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
    // IFD0's entries must lie inside the file: offset + 2 + 12·count ≤ length.
    let Some(ifd) = u32_at(4).map(|v| v as usize) else {
        return Ok(None);
    };
    let Some(n) = u16_at(ifd).map(usize::from) else {
        return Ok(None);
    };
    let first = ifd
        .checked_add(2)
        .filter(|f| f.checked_add(12 * n).is_some_and(|e| e <= b.len()));
    let Some(first) = first else {
        return Ok(None);
    };
    // (type, count, the inline value of a one-element SHORT or LONG) per tag of interest.
    let mut photometric = None;
    let (mut spp, mut bps, mut fill, mut extra, mut map) = (Some(1), Some(1), Some(1), false, None);
    for e in (0..n).map(|k| first + 12 * k) {
        let (Some(tag), Some(kind), Some(count)) = (u16_at(e), u16_at(e + 2), u32_at(e + 4)) else {
            return Ok(None);
        };
        let value = match (kind, count) {
            (3, 1) => u16_at(e + 8).map(u32::from),
            (4, 1) => u32_at(e + 8),
            _ => None,
        };
        match tag {
            262 => photometric = Some((value, kind, e + 8)),
            277 => spp = value,
            258 => bps = value,
            266 => fill = value,
            338 => extra = true,
            320 => map = Some((kind, count)),
            _ => {}
        }
    }
    let Some((Some(3), kind, at)) = photometric else {
        return Ok(None);
    };
    if spp != Some(1) || extra || fill != Some(1) {
        return bad("only one sample per pixel, no extra samples, fill order 1");
    }
    let bps = match bps {
        Some(v @ (1 | 2 | 4 | 8)) => v,
        _ => return bad("bits per sample must be 1, 2, 4 or 8"),
    };
    let n = 1usize << bps;
    if map != Some((3, 3 * n as u32)) {
        return bad("the ColorMap must be 3 · 2^bps SHORTs");
    }
    let mut copy = b.to_vec();
    let grey: &[u8] = match (kind, le) {
        (3, true) => &1u16.to_le_bytes(),
        (3, false) => &1u16.to_be_bytes(),
        (_, true) => &1u32.to_le_bytes(),
        (_, false) => &1u32.to_be_bytes(),
    };
    copy[at..at + grey.len()].copy_from_slice(grey);
    let mut limits = ::tiff::decoder::Limits::default();
    limits.decoding_buffer_size = budget as usize;
    limits.intermediate_buffer_size = budget as usize; // a whole strip may be read at once
    let mut dec = ::tiff::decoder::Decoder::new(Cursor::new(copy))
        .map_err(invalid)?
        .with_limits(limits);
    let (w, h) = dec.dimensions().map_err(invalid)?;
    check_size(w, h)?; // the RGBA budget, before `read_image` allocates the indices
    let map = dec
        .get_tag_u16_vec(::tiff::tags::Tag::ColorMap)
        .map_err(invalid)?;
    if map.len() != 3 * n {
        return bad("short ColorMap");
    }
    let orientation = dec
        .find_tag_unsigned::<u16>(::tiff::tags::Tag::Orientation)
        .ok()
        .flatten()
        .and_then(|v| Orientation::from_exif(v.min(255) as u8))
        .unwrap_or(Orientation::NoTransforms);
    let ::tiff::decoder::DecodingResult::U8(idx) = dec.read_image().map_err(invalid)? else {
        return bad("unexpected sample type");
    };
    let (w, h, bits) = (w as usize, h as usize, bps as usize);
    let stride = (w * bits).div_ceil(8);
    if idx.len() < stride * h {
        return bad("short strip");
    }
    let lut: Vec<[u8; 4]> = (0..n)
        .map(|i| [map[i], map[n + i], map[2 * n + i], 0xFFFF].map(|c| (c >> 8) as u8))
        .collect();
    let mut out = vec![[0u8; 4]; w * h];
    for (row, dst) in idx.chunks_exact(stride).zip(out.chunks_exact_mut(w)) {
        for (x, d) in dst.iter_mut().enumerate() {
            let at = x * bits;
            *d = lut[(row[at / 8] >> (8 - bits - at % 8)) as usize & (n - 1)];
        }
    }
    let rgba = image::RgbaImage::from_raw(w as u32, h as u32, out.into_flattened())
        .ok_or_else(|| invalid("TIFF"))?;
    let mut img = image::DynamicImage::ImageRgba8(rgba);
    img.apply_orientation(orientation);
    let (width, height) = (img.width(), img.height());
    Ok(Some(ImageData {
        width,
        height,
        data: img.into_rgba8().into_raw(),
    }))
}

#[cfg(test)]
mod tests {
    use super::super::codecs::decode;
    use super::super::encode::{BitDepth, SaveOptions, encode};
    use crate::canvas::ImageData;
    use std::collections::BTreeMap;

    /// tag → (type, count, value or offset field) from IFD0 of a little-endian TIFF.
    fn ifd(b: &[u8]) -> BTreeMap<u16, (u16, u32, u32)> {
        assert_eq!(&b[..4], b"II\x2a\x00");
        let le16 = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let le32 = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let at = le32(4) as usize;
        let n = le16(at) as usize;
        assert_eq!(le32(at + 2 + 12 * n), 0, "one IFD");
        (0..n)
            .map(|k| {
                let e = at + 2 + 12 * k;
                (le16(e), (le16(e + 2), le32(e + 4), le32(e + 8)))
            })
            .collect()
    }

    fn tiff(img: &ImageData, bit_depth: BitDepth) -> Vec<u8> {
        let o = SaveOptions {
            bit_depth,
            ..SaveOptions::default_for("tiff")
        };
        encode(img, "tiff", &o).unwrap()
    }

    /// 32-bit RGBA: the baseline tag set, ExtraSamples 2, 96/1 dpi as a rational, the strip.
    #[test]
    fn rgba_tags_and_strip() {
        let img = ImageData {
            width: 2,
            height: 1,
            data: vec![1, 2, 3, 0, 4, 5, 6, 255],
        };
        let b = tiff(&img, BitDepth::B32);
        let t = ifd(&b);
        let tags: Vec<u16> = t.keys().copied().collect();
        assert_eq!(
            tags,
            [
                256, 257, 258, 259, 262, 273, 277, 278, 279, 282, 283, 296, 338
            ]
        );
        assert_eq!(t[&256], (4, 1, 2));
        assert_eq!(t[&257], (4, 1, 1));
        assert_eq!((t[&258].0, t[&258].1), (3, 4));
        let bps = t[&258].2 as usize;
        assert_eq!(&b[bps..bps + 8], [8, 0, 8, 0, 8, 0, 8, 0]);
        assert_eq!(
            [t[&259], t[&262], t[&277], t[&296], t[&338]],
            [(3, 1, 1), (3, 1, 2), (3, 1, 4), (3, 1, 2), (3, 1, 2)]
        );
        assert_eq!(t[&278], (4, 1, 1024)); // ruling R38: max(1, 8192 / 8) rows per strip
        assert_eq!(t[&279], (4, 1, 8));
        let res = t[&282].2 as usize;
        assert_eq!(&b[res..res + 8], [0x00, 0xA6, 0x0E, 0, 0x10, 0x27, 0, 0]); // 960000 / 10000
        assert!(
            [258, 282, 283].iter().all(|k| t[k].2.is_multiple_of(2)),
            "word-aligned values"
        );
        let strip = t[&273].2 as usize;
        assert_eq!(&b[strip..], img.data);
        assert_eq!(decode("tiff", &b).unwrap().image.data, img.data);
    }

    /// 4-bit palette: Photometric 3, a 3·16-entry ColorMap of c·257 (reds, greens, blues), rows
    /// packed MSB-first, each starting on a byte.
    #[test]
    fn palette_colour_map() {
        let img = ImageData {
            width: 3,
            height: 2,
            data: [[255, 0, 0, 255], [0, 128, 0, 255], [0, 0, 1, 255]]
                .concat()
                .repeat(2),
        };
        let b = tiff(&img, BitDepth::B4);
        let t = ifd(&b);
        assert_eq!(
            [t[&258], t[&262], t[&277]],
            [(3, 1, 4), (3, 1, 3), (3, 1, 1)]
        );
        assert!(!t.contains_key(&338));
        assert_eq!((t[&320].0, t[&320].1), (3, 48));
        let at = t[&320].2 as usize;
        let map: Vec<u16> = b[at..at + 96]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let mut want = vec![0u16; 48];
        want[0] = 65535; // red of entry 0
        want[16 + 1] = 128 * 257; // green of entry 1
        want[32 + 2] = 257; // blue of entry 2
        assert_eq!(map, want);
        let strip = t[&273].2 as usize;
        assert_eq!([t[&278], t[&279]], [(4, 1, 4096), (4, 1, 4)]);
        assert_eq!(&b[strip..], [0x01, 0x20, 0x01, 0x20]);
        let rgb = tiff(&img, BitDepth::B24);
        assert_eq!(decode("tiff", &rgb).unwrap().image.data, img.data);
    }

    /// Overwrites the count (`field` 4) or value (`field` 8) of IFD0 entry `tag`, little-endian.
    fn patch(b: &mut [u8], tag: u16, field: usize, v: u32) {
        let at = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
        let n = u16::from_le_bytes([b[at], b[at + 1]]) as usize;
        let e = (0..n)
            .map(|k| at + 2 + 12 * k)
            .find(|&e| u16::from_le_bytes([b[e], b[e + 1]]) == tag)
            .unwrap();
        b[e + field..e + field + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn colours(n: u32, w: u32, h: u32) -> ImageData {
        let px: Vec<[u8; 4]> = (0..w * h)
            .map(|i| [(i % n * 37) as u8, (i % n) as u8, 255 - (i % n) as u8, 255])
            .collect();
        ImageData {
            width: w,
            height: h,
            data: px.concat(),
        }
    }

    /// Palette TIFFs open again: Auto 3 colours at 300×300 is 4-bit; explicit 1-bit (a 9-wide
    /// row is 2 bytes) and 8-bit; the dpi survives.
    #[test]
    fn palette_tiffs_round_trip() {
        let auto = colours(3, 300, 300);
        let b = tiff(&auto, BitDepth::Auto);
        assert_eq!((ifd(&b)[&258].2, ifd(&b)[&262].2), (4, 3));
        for (img, b) in [
            (auto.clone(), b),
            (colours(2, 9, 3), tiff(&colours(2, 9, 3), BitDepth::B1)),
            (
                colours(200, 20, 10),
                tiff(&colours(200, 20, 10), BitDepth::B8),
            ),
        ] {
            let d = decode("tiff", &b).unwrap();
            assert_eq!(d.image.data, img.data, "{}×{}", img.width, img.height);
            assert_eq!(d.dpi, Some(96.0));
        }
    }

    /// A big-endian (MM) 2×1 1-bit palette TIFF built by hand: the SHORT Photometric sits in
    /// the first two bytes of its value field, and the ColorMap is big-endian too.
    #[test]
    fn big_endian_palette() {
        let entries: [(u16, u16, u32, u32); 10] = [
            (256, 4, 1, 2),
            (257, 4, 1, 1),
            (258, 3, 1, 1 << 16),
            (259, 3, 1, 1 << 16),
            (262, 3, 1, 3 << 16),
            (273, 4, 1, 146),
            (277, 3, 1, 1 << 16),
            (278, 4, 1, 1),
            (279, 4, 1, 1),
            (320, 3, 6, 134),
        ];
        let mut b = b"MM\0*\0\0\0\x08\0\x0a".to_vec();
        for (tag, kind, count, value) in entries {
            b.extend(tag.to_be_bytes());
            b.extend(kind.to_be_bytes());
            b.extend(count.to_be_bytes());
            b.extend(value.to_be_bytes());
        }
        b.extend([0; 4]);
        for c in [0xFFFFu16, 0, 0, 0x8080, 0x1234, 0xFFFF] {
            b.extend(c.to_be_bytes());
        }
        b.push(0b1000_0000); // indices 1, 0
        assert_eq!(b.len(), 147);
        let d = decode("tiff", &b).unwrap().image;
        assert_eq!((d.width, d.height), (2, 1));
        assert_eq!(d.data, [0, 0x80, 0xFF, 255, 0xFF, 0, 0x12, 255]);
    }

    /// A little-endian TIFF from IFD0 entries (tag, type, count, value) and the bytes after it.
    fn hand(entries: &[(u16, u16, u32, u32)], tail: &[u8]) -> Vec<u8> {
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        b.extend((entries.len() as u16).to_le_bytes());
        for &(tag, kind, count, value) in entries {
            b.extend(tag.to_le_bytes());
            b.extend(kind.to_le_bytes());
            b.extend(count.to_le_bytes());
            b.extend(value.to_le_bytes());
        }
        b.extend([0; 4]);
        b.extend(tail);
        b
    }

    /// `count` LONGs at the offset in entry `tag`.
    fn longs(b: &[u8], t: &BTreeMap<u16, (u16, u32, u32)>, tag: u16) -> Vec<u32> {
        let (_, count, at) = t[&tag];
        let at = at as usize;
        (0..count as usize)
            .map(|k| u32::from_le_bytes(b[at + 4 * k..at + 4 * k + 4].try_into().unwrap()))
            .collect()
    }

    /// Ruling R38: 8192-byte strips. 100 RGB pixels are 300 bytes a row: 27 rows a strip, so 60
    /// rows are strips of 27, 27 and 6, laid out back to back.
    #[test]
    fn strips_with_a_partial_last_one() {
        let img = ImageData {
            width: 100,
            height: 60,
            data: crate::testing::fixture_rgba(100, 60, 2),
        };
        let b = tiff(&img, BitDepth::B24);
        let t = ifd(&b);
        assert_eq!(t[&278], (4, 1, 27));
        assert_eq!((t[&273].1, t[&279].1), (3, 3));
        assert_eq!(longs(&b, &t, 279), [8100, 8100, 1800]);
        let at = longs(&b, &t, 273);
        assert_eq!([at[1] - at[0], at[2] - at[1]], [8100, 8100]);
        assert_eq!(at[2] as usize + 1800, b.len());
        assert_eq!(decode("tiff", &b).unwrap().image.data, img.data);
    }

    /// Rows of 8192 bytes or more get one strip each, at 32-bit and on the palette path.
    #[test]
    fn one_row_per_strip() {
        let img = ImageData {
            width: 2049,
            height: 3,
            data: crate::testing::fixture_rgba(2049, 3, 5),
        };
        let b = tiff(&img, BitDepth::B32);
        let t = ifd(&b);
        assert_eq!(t[&278], (4, 1, 1));
        assert_eq!(longs(&b, &t, 279), [8196; 3]);
        assert_eq!(decode("tiff", &b).unwrap().image.data, img.data);
        let wide = colours(200, 8192, 2);
        let b = tiff(&wide, BitDepth::B8);
        let t = ifd(&b);
        assert_eq!((t[&278], t[&273].1), ((4, 1, 1), 2));
        assert_eq!(decode("tiff", &b).unwrap().image.data, wide.data);
    }

    /// Ruling R38: a 24/32-bit TIFF above half the decode budget reopens, because `image` caps
    /// its intermediate buffer at budget − image bytes and no strip comes near that.
    #[test]
    fn above_half_the_budget_reopens() {
        let budget = 1 << 20;
        for (w, h, d) in [(512, 300, BitDepth::B32), (600, 400, BitDepth::B24)] {
            let img = colours(50, w, h);
            let b = tiff(&img, d);
            let back = super::super::codecs::decode_within("tiff", &b, budget).unwrap();
            assert_eq!(back.image.data, img.data, "{d:?}");
        }
    }

    /// A 32-bit TIFF of exactly the budget reopens: `image`'s intermediate buffer gets one strip
    /// of headroom above it.
    #[test]
    fn exactly_the_budget_reopens() {
        let img = colours(50, 512, 512);
        let b = tiff(&img, BitDepth::B32);
        let back = super::super::codecs::decode_within("tiff", &b, 1 << 20).unwrap();
        assert_eq!(back.image.data, img.data);
    }

    /// One 8-bit palette strip above `tiff`'s default 128 MiB intermediate limit (16384 × 8193).
    #[test]
    #[ignore = "~1 GB peak: one strip above tiff's 128 MiB default; run with --ignored"]
    fn one_huge_palette_strip() {
        let (w, h) = (16384u32, 8193u32);
        let n = (w * h) as usize;
        let mut map = vec![0u16; 768];
        map[1] = 65535; // entry 1 is red
        let tail: Vec<u8> = map.iter().flat_map(|c| c.to_le_bytes()).collect();
        let mut b = hand(
            &[
                (256, 4, 1, w),
                (257, 4, 1, h),
                (258, 3, 1, 8),
                (259, 3, 1, 1),
                (262, 3, 1, 3),
                (273, 4, 1, 134 + 1536),
                (277, 3, 1, 1),
                (278, 4, 1, h),
                (279, 4, 1, n as u32),
                (320, 3, 768, 134),
            ],
            &tail,
        );
        b.resize(134 + 1536 + n, 0); // the strip, built in place
        b[134 + 1536] = 1;
        b[134 + 1536 + n - 1] = 1;
        let d = decode("tiff", &b).unwrap().image;
        assert_eq!((d.width, d.height), (w, h));
        assert_eq!(&d.data[..8], [255, 0, 0, 255, 0, 0, 0, 255]);
        assert_eq!(&d.data[d.data.len() - 4..], [255, 0, 0, 255]);
    }

    /// §2.13: Orientation 6 (rotate 90° clockwise) applies on the palette path as on `image`'s:
    /// the 2×1 row [A B] becomes a 1×2 column, A on top.
    #[test]
    fn palette_orientation() {
        let mut tail: Vec<u8> = [0xFFFFu16, 0, 0, 0x8080, 0x1234, 0xFFFF]
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        tail.push(0b1000_0000); // indices 1, 0
        let b = hand(
            &[
                (256, 4, 1, 2),
                (257, 4, 1, 1),
                (258, 3, 1, 1),
                (259, 3, 1, 1),
                (262, 3, 1, 3),
                (273, 4, 1, 158),
                (274, 3, 1, 6),
                (277, 3, 1, 1),
                (278, 4, 1, 1),
                (279, 4, 1, 1),
                (320, 3, 6, 146),
            ],
            &tail,
        );
        let d = decode("tiff", &b).unwrap().image;
        assert_eq!((d.width, d.height), (1, 2));
        assert_eq!(d.data, [0, 0x80, 0xFF, 255, 0xFF, 0, 0x12, 255]);
    }

    fn four_bit() -> Vec<u8> {
        tiff(&colours(3, 5, 3), BitDepth::B4)
    }

    #[test]
    fn palette_refuses_a_short_colour_map() {
        let mut b = four_bit();
        patch(&mut b, 320, 4, 47);
        assert_eq!(
            decode("tiff", &b).unwrap_err().to_string(),
            "TIFF palette: the ColorMap must be 3 · 2^bps SHORTs"
        );
    }

    #[test]
    fn palette_refuses_a_strip_past_eof() {
        let mut b = four_bit();
        let len = b.len() as u32;
        patch(&mut b, 273, 8, len - 2);
        assert!(decode("tiff", &b).is_err());
    }

    #[test]
    fn palette_refuses_16_bit() {
        let mut b = four_bit();
        patch(&mut b, 258, 8, 16);
        assert_eq!(
            decode("tiff", &b).unwrap_err().to_string(),
            "TIFF palette: bits per sample must be 1, 2, 4 or 8"
        );
    }

    #[test]
    fn palette_refuses_a_truncated_strip() {
        let mut b = four_bit();
        b.truncate(b.len() - 1);
        assert!(decode("tiff", &b).is_err());
    }
}
