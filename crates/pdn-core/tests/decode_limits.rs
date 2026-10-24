//! Headers that declare huge images must be refused before anything large is allocated
//! (Review Focus 5). A guard allocator refuses every single allocation above 256 MiB and counts
//! the refusals: a decoder that trusts the header either aborts this binary ("memory allocation
//! of N bytes failed") or trips the counter.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use pdn_core::io::codecs::{CodecError, decode};

const LIMIT: usize = 256 << 20;
static REFUSED: AtomicUsize = AtomicUsize::new(0);

struct Guard;

// SAFETY: forwards to the system allocator, or returns null (a refused allocation).
unsafe impl GlobalAlloc for Guard {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if l.size() > LIMIT {
            REFUSED.fetch_add(1, Ordering::SeqCst);
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if l.size() > LIMIT {
            REFUSED.fetch_add(1, Ordering::SeqCst);
            return std::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        if size > LIMIT {
            REFUSED.fetch_add(1, Ordering::SeqCst);
            return std::ptr::null_mut();
        }
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static GUARD: Guard = Guard;

/// Decodes within 2 s and without asking for more than 256 MiB at once.
fn bounded(format: &str, bytes: &[u8]) -> Result<pdn_core::io::codecs::Decoded, CodecError> {
    let t = Instant::now();
    let r = decode(format, bytes);
    assert_eq!(
        REFUSED.load(Ordering::SeqCst),
        0,
        "{format} asked for > 256 MiB"
    );
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "{format} took {:?}",
        t.elapsed()
    );
    r
}

fn refused(format: &str, bytes: &[u8]) {
    assert!(
        bounded(format, bytes).is_err(),
        "{format} decoded a hostile header"
    );
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

fn small(format: image::ImageFormat) -> Vec<u8> {
    let img =
        image::RgbaImage::from_raw(13, 9, (0..13 * 9 * 4).map(|i| i as u8).collect()).unwrap();
    let mut v = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut v), format)
        .unwrap();
    v
}

/// A valid PNG (correct CRCs) declaring 32768² RGBA16: 8 GiB of pixels.
#[test]
fn png_rgba16_32768() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = [0u8; 13];
    ihdr[..8].copy_from_slice(&[0, 0, 0x80, 0, 0, 0, 0x80, 0]);
    ihdr[8..10].copy_from_slice(&[16, 6]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(
        &mut png,
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&[0; 1024], 6),
    );
    png_chunk(&mut png, b"IEND", &[]);
    refused("png", &png);
}

/// A baseline JPEG whose SOF0 declares 32768² RGB (3 GiB), with an 8 × 8 scan.
#[test]
fn jpeg_sof0_32768() {
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&[128; 8 * 8 * 3], 8, 8, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let sof = jpeg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    jpeg[sof + 5..sof + 9].copy_from_slice(&[0x80, 0, 0x80, 0]);
    refused("jpeg", &jpeg);
}

/// A 32-bit BMP whose header says 32768² (4 GiB).
#[test]
fn bmp_32768() {
    let mut bmp = small(image::ImageFormat::Bmp);
    bmp[18..22].copy_from_slice(&32768i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&32768i32.to_le_bytes());
    refused("bmp", &bmp);
}

/// A GIF whose logical screen is 32768² (4 GiB as RGBA).
#[test]
fn gif_32768() {
    let mut gif = small(image::ImageFormat::Gif);
    gif[6..10].copy_from_slice(&[0, 0x80, 0, 0x80]);
    refused("gif", &gif);
}

/// A TIFF of 32768² 32-bit float RGBA (16 GiB as Rgba32F).
#[test]
fn tiff_rgba32f_32768() {
    // II*\0, IFD at 8 with 11 entries, then BitsPerSample and SampleFormat arrays and 64 data bytes.
    let entries: [(u16, u16, u32, u32); 11] = [
        (256, 4, 1, 32768),
        (257, 4, 1, 32768),
        (258, 3, 4, 146),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 162),
        (277, 3, 1, 4),
        (278, 4, 1, 32768),
        (279, 4, 1, 64),
        (338, 3, 1, 2),
        (339, 3, 4, 154),
    ];
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    tiff.extend(11u16.to_le_bytes());
    for (tag, kind, count, value) in entries {
        tiff.extend(tag.to_le_bytes());
        tiff.extend(kind.to_le_bytes());
        tiff.extend(count.to_le_bytes());
        tiff.extend(value.to_le_bytes());
    }
    tiff.extend([0; 4]);
    assert_eq!(tiff.len(), 146);
    tiff.extend([32, 0, 32, 0, 32, 0, 32, 0, 3, 0, 3, 0, 3, 0, 3, 0]);
    tiff.extend([0; 64]);
    refused("tiff", &tiff);
}

/// A palette TIFF (Task 47b's writer, 8-bit) re-headed to 32768²: 1 GiB of indices, 4 GiB as RGBA.
#[test]
fn tiff_palette_32768() {
    let img = pdn_core::canvas::ImageData {
        width: 4,
        height: 2,
        data: (0..8u8).flat_map(|i| [i * 9, i, 0, 255]).collect(),
    };
    let o = pdn_core::io::encode::SaveOptions {
        bit_depth: pdn_core::io::encode::BitDepth::B8,
        ..Default::default()
    };
    let mut tiff = pdn_core::io::encode::encode(&img, "tiff", &o).unwrap();
    let n = u16::from_le_bytes([tiff[8], tiff[9]]) as usize;
    for e in (0..n).map(|k| 10 + 12 * k) {
        if matches!(u16::from_le_bytes([tiff[e], tiff[e + 1]]), 256 | 257 | 278) {
            tiff[e + 8..e + 12].copy_from_slice(&32768u32.to_le_bytes());
        }
    }
    refused("tiff", &tiff);
}

/// DDS headers: 16384² (1 GiB, inside the budget) with no data, and a 4294967295-high one.
#[test]
fn dds_huge_headers() {
    let img = pdn_core::canvas::ImageData {
        width: 2,
        height: 2,
        data: vec![7; 16],
    };
    let mut dds = pdn_core::io::encode::encode_dds(&img);
    dds[12..20].copy_from_slice(&[0, 0x40, 0, 0, 0, 0x40, 0, 0]);
    refused("dds", &dds);
    dds[84..88].copy_from_slice(b"DXT1"); // the same as BC1 blocks
    dds[80] = 4;
    refused("dds", &dds);
    dds[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    refused("dds", &dds);
}

/// TGA: 32768² RLE (4 GiB) and 16384² raw (1 GiB) with a few body bytes.
#[test]
fn tga_huge_headers() {
    let header = |kind: u8, side: u16| {
        let mut v = vec![0u8, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        v.extend(side.to_le_bytes());
        v.extend(side.to_le_bytes());
        v.extend([32, 0, 0xFF, 1, 2, 3, 4]);
        v
    };
    refused("tga", &header(10, 32768));
    refused("tga", &header(10, 16384));
    refused("tga", &header(2, 16384));
}

#[cfg(not(target_arch = "wasm32"))]
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/").to_owned() + name)
        .unwrap()
}

/// Task 48: every truncation of the native-decoder fixtures is refused, and every single-bit flip
/// either decodes or is refused, always within the time and allocation bounds.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_fixtures_truncated_and_bit_flipped() {
    let mut cases = vec![
        ("heic", "a.heic"),
        ("heic", "heic-alpha.heic"),
        ("jxr", "a.jxr"),
        ("jxr", "a-bgra32.jxr"),
        ("jxr", "a-rgba64half.jxr"),
    ];
    if cfg!(feature = "avif-dav1d") {
        cases.extend([
            ("avif", "a.avif"),
            ("avif", "avif-bt709-limited-alpha.avif"),
            ("avif", "avif-clap-irot.avif"),
            ("avif", "avif-grid-2x1.avif"),
        ]);
    }
    for (f, name) in cases {
        let good = fixture(name);
        bounded(f, &good).unwrap_or_else(|e| panic!("{name}: {e}"));
        for cut in 0..good.len() {
            assert!(bounded(f, &good[..cut]).is_err(), "{name} cut at {cut}");
        }
        for bit in 0..good.len() * 8 {
            let mut b = good.clone();
            b[bit / 8] ^= 1 << (bit % 8);
            let _ = bounded(f, &b);
        }
    }
}

/// The 13 × 9 ravif AVIF (opaque: one item) with its reduced still-picture sequence header
/// re-packed to declare `w` × `h` (AV1 §5.5.1: frame_width/height_bits_minus_1 = 15 and
/// max_frame_width/height_minus_1); the OBU, `iloc` extent and `mdat` sizes follow. The frame
/// header takes its size from these fields, so dav1d would size the frame from them too.
#[cfg(all(feature = "avif-dav1d", not(target_arch = "wasm32")))]
fn reheadered_avif(w: u32, h: u32) -> Vec<u8> {
    let img = pdn_core::canvas::ImageData {
        width: 13,
        height: 9,
        data: [200, 200, 200, 255].repeat(13 * 9),
    };
    let avif = pdn_core::io::encode::encode(&img, "avif", &Default::default()).unwrap();
    let meta = heic_rs::meta::parse(&avif).unwrap();
    let item = meta.item_data(&avif, meta.primary).unwrap().to_vec();
    let at = avif.windows(item.len()).position(|w| w == item).unwrap();
    let mut obu = at + if avif[at] == 0x12 { 2 } else { 0 }; // after a temporal delimiter
    assert_eq!(avif[obu], 0x0A, "a sequence header OBU");
    let size = avif[obu + 1] as usize;
    obu += 2;
    let bits: Vec<u32> = (0..size * 8)
        .map(|i| (avif[obu + i / 8] >> (7 - i % 8) & 1) as u32)
        .collect();
    let field = |from: usize, n: usize| bits[from..from + n].iter().fold(0, |v, b| v << 1 | b);
    assert_eq!(bits[4], 1, "reduced still-picture header");
    let (wb, hb) = (field(10, 4) as usize + 1, field(14, 4) as usize + 1);
    let mut repacked = bits[..10].to_vec();
    for (v, n) in [(15, 4), (15, 4), (w - 1, 16), (h - 1, 16)] {
        repacked.extend((0..n).rev().map(|i| v >> i & 1));
    }
    repacked.extend(&bits[18 + wb + hb..]);
    assert_eq!(repacked.len() % 8, 0);
    let payload: Vec<u8> = repacked
        .chunks(8)
        .map(|c| c.iter().fold(0, |v, &b| v << 1 | b as u8))
        .collect();
    let grow = (payload.len() - size) as u32;
    let mut out = avif[..obu - 1].to_vec();
    out.push(payload.len() as u8);
    out.extend(&payload);
    out.extend(&avif[obu + size..]);
    // mdat is last: only its size and the item's iloc extent length change.
    let be = |v: &[u8]| u32::from_be_bytes(v.try_into().unwrap());
    let mdat = out.windows(4).position(|w| w == b"mdat").unwrap() - 4;
    let n = be(&out[mdat..mdat + 4]) + grow;
    out[mdat..mdat + 4].copy_from_slice(&n.to_be_bytes());
    let extent = [(at as u32).to_be_bytes(), (item.len() as u32).to_be_bytes()].concat();
    let e = out.windows(8).position(|w| w == extent).unwrap() + 4;
    out[e..e + 4].copy_from_slice(&(item.len() as u32 + grow).to_be_bytes());
    out
}

/// 16385 × 16384, one row over the 2^28-pixel budget, and 32769 × 16, one column over
/// `MAX_DECODE_SIDE`: both refused from the sequence header, before dav1d sizes a frame.
#[cfg(all(feature = "avif-dav1d", not(target_arch = "wasm32")))]
#[test]
fn avif_sequence_header_too_large() {
    for (w, h) in [(16385, 16384), (32769, 16)] {
        let e = bounded("avif", &reheadered_avif(w, h)).unwrap_err();
        assert!(e.to_string().contains("too large"), "{w} × {h}: {e}");
    }
}

/// Review finding (Ruling R51): `a.jxr` (24bppBGR) re-labelled 8bppGray, the pixel-format GUID's
/// last byte (file offset 0x17) 0x0C → 0x08, made jpegxr-pure-rs write the RGB codestream's 351
/// bytes into a 117-byte buffer, a heap overflow ASan reports. It must be refused or decode to the
/// same image; run under ASan, never a corrupting write.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn jxr_gray_guid_over_rgb_codestream() {
    let good = fixture("a.jxr");
    let mut jxr = good.clone();
    assert_eq!(jxr[0x17], 0x0C);
    jxr[0x17] = 0x08;
    if let Ok(d) = bounded("jxr", &jxr) {
        assert_eq!(d.image.data, decode("jxr", &good).unwrap().image.data);
    }
}

/// HEIC whose `ispe` declares 40000² (6.4 GB as RGBA).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn heic_ispe_40000() {
    let mut heic = fixture("a.heic");
    let at = heic.windows(4).position(|w| w == b"ispe").unwrap() + 8;
    heic[at..at + 8].copy_from_slice(&[0, 0, 0x9C, 0x40, 0, 0, 0x9C, 0x40]);
    refused("heic", &heic);
}

/// HEIC whose HEVC SPS declares 32768 × 16384 (2 GiB as RGBA) under the fixture's 64 × 64 `ispe`:
/// heic-rs checks the `ispe` but allocates the SPS's picture (1 GiB of luma samples). Written from
/// `a.heic` by re-packing pic_width/height_in_luma_samples and moving the box sizes and the `iloc`
/// base offset by the 5 bytes the SPS grew.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn heic_sps_32768x16384() {
    refused("heic", &fixture("heic-sps-32768x16384.heic"));
}

/// JPEG XR declaring 65536² in the codestream's short header is refused; 40000² in the
/// container's IFD is not what the decoder sizes its picture from, so that file still opens.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn jxr_huge_headers() {
    let good = fixture("a.jxr");
    let mut jxr = good.clone();
    let at = jxr.windows(8).position(|w| w == b"WMPHOTO\0").unwrap();
    assert_eq!(jxr[at + 10] >> 7, 1, "short header");
    jxr[at + 12..at + 16].fill(0xFF); // WIDTH_MINUS1, HEIGHT_MINUS1
    refused("jxr", &jxr);
    let mut jxr = good;
    for tag in [0xBC80u16, 0xBC81] {
        let e = jxr.windows(2).position(|w| w == tag.to_le_bytes()).unwrap();
        jxr[e + 8..e + 12].copy_from_slice(&40000u32.to_le_bytes());
    }
    let d = bounded("jxr", &jxr).unwrap();
    assert_eq!((d.image.width, d.image.height), (13, 9));
}
