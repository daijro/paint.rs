//! TGA: Paint.NET's own reader (Task 47) and writer (Task 47b).

use super::codecs::{CodecError, bytes_at, check_size};
use super::encode::{Depth, SaveOptions, choose_bit_depth};
use crate::canvas::ImageData;
use crate::util::quantize::flatten_alpha;

/// The UTC date and time stamped into the extension area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TgaClock {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

impl TgaClock {
    /// The current UTC time (`Date.now()` on the web).
    pub fn now() -> TgaClock {
        #[cfg(not(target_arch = "wasm32"))]
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        #[cfg(target_arch = "wasm32")]
        let secs = (js_sys::Date::now() / 1000.0).floor() as i64;
        TgaClock::from_unix(secs)
    }

    /// Proleptic Gregorian civil time of a Unix timestamp (Howard Hinnant's `civil_from_days`).
    fn from_unix(secs: i64) -> TgaClock {
        let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        TgaClock {
            year: (yoe + era * 400 + (month <= 2) as i64) as u16,
            month: month as u16,
            day: (doy - (153 * mp + 2) / 5 + 1) as u16,
            hour: (rem / 3600) as u16,
            minute: (rem / 60 % 60) as u16,
            second: (rem % 60) as u16,
        }
    }
}

/// One row's packets (§2.12 Write step 4): at most 128 pixels each,
/// never crossing rows.
struct Packets<'a> {
    out: &'a mut Vec<u8>,
    bpp: usize,
    px: Vec<[u8; 4]>,
    rle: bool,
}

impl Packets<'_> {
    fn flush(&mut self) {
        if !self.px.is_empty() {
            self.out
                .push(if self.rle { 0x80 } else { 0 } + (self.px.len() - 1) as u8);
            let shown = if self.rle {
                &self.px[..1]
            } else {
                &self.px[..]
            };
            for p in shown {
                self.out.extend_from_slice(&p[..self.bpp]);
            }
            self.px.clear();
        }
    }

    fn push(&mut self, p: [u8; 4]) {
        let n = self.px.len();
        let last = self.px.last().copied();
        if n == 0 {
            self.rle = false;
            self.px.push(p);
        } else if n == 1 {
            self.rle = Some(p) == last;
            self.px.push(p);
        } else if n == 128 || (self.rle && Some(p) != last) {
            self.flush();
            self.push(p);
        } else if self.rle || Some(p) != last {
            self.px.push(p);
        } else {
            // A raw packet ending in this pixel: the pair becomes the start of a run.
            self.px.pop();
            self.flush();
            self.push(p);
            self.push(p);
        }
    }
}

/// Paint.NET's TGA writer (§2.12 Write), byte for byte: Auto is 24-bit when every pixel is opaque,
/// else 32-bit; rows bottom-up; RLE per `opts.rle`; the 495-byte TGA 2.0 extension area stamped
/// with `now` and the 26-byte footer. Panics if a side is 0 or above 65535 (`io::encode::encode`
/// rejects those first).
pub fn encode(img: &ImageData, opts: &SaveOptions, now: TgaClock) -> Vec<u8> {
    let depth = choose_bit_depth("tga", img, opts, &mut |_| 0);
    let wide = depth == Depth::B32;
    let side = |v: u32| u16::try_from(v).expect("TGA sides are at most 65535");
    let (w, h) = (side(img.width), side(img.height));
    let bpp = if wide { 4 } else { 3 };
    let mut out = vec![0u8; 18];
    out[2] = if opts.rle { 10 } else { 2 };
    out[12..14].copy_from_slice(&w.to_le_bytes());
    out[14..16].copy_from_slice(&h.to_le_bytes());
    out[16] = bpp as u8 * 8;
    out[17] = if wide { 8 } else { 0 }; // 8 alpha bits, bottom-left origin
    let rows = img.data.chunks_exact(w as usize * 4).rev();
    for row in rows {
        // ruling R34: 24-bit flattens with t = 0 (§2.8 step 6).
        let pixels = row.as_chunks::<4>().0.iter().map(|&p| {
            let [r, g, b, a] = if wide { p } else { flatten_alpha(p, 0) };
            [b, g, r, a]
        });
        if opts.rle {
            let mut packets = Packets {
                out: &mut out,
                bpp,
                px: Vec::with_capacity(128),
                rle: false,
            };
            pixels.for_each(|p| packets.push(p));
            packets.flush();
        } else {
            pixels.for_each(|p| out.extend_from_slice(&p[..bpp]));
        }
    }
    let at = out.len() as u32;
    let mut area = [0u8; 495];
    let mut put16 = |i: usize, v: u16| area[i..i + 2].copy_from_slice(&v.to_le_bytes());
    put16(0, 495);
    for (k, v) in [
        now.month, now.day, now.year, now.hour, now.minute, now.second,
    ]
    .into_iter()
    .enumerate()
    {
        put16(367 + 2 * k, v);
    }
    put16(474, 1); // pixel aspect ratio 1/1
    put16(476, 1);
    put16(478, 1); // gamma 1/1
    put16(480, 1);
    area[426..442].copy_from_slice(b"Paint.NET 5.1.12"); // software ID, zero-padded to 41
    area[469] = b' '; // version letter
    area[494] = if wide { 3 } else { 0 }; // attributes: straight alpha
    out.extend(area);
    out.extend(at.to_le_bytes());
    out.extend([0; 4]);
    out.extend(b"TRUEVISION-XFILE.\0");
    out
}

fn bad(m: &str) -> CodecError {
    CodecError::Invalid(format!("TGA: {m}"))
}

/// One straight-RGBA pixel from a 15/16/24/32-bit BGR(A) value. 16-bit colour map entries are
/// expanded by c·255/31; direct 16-bit pixels by c·8 with no bit replication (§2.12 Read).
fn color(s: &[u8], bits: u8, alpha: bool, map_entry: bool) -> [u8; 4] {
    match *s {
        [lo, hi] => {
            let w = u16::from_le_bytes([lo, hi]);
            let a = if alpha && w & 0x8000 == 0 { 0 } else { 255 };
            if map_entry {
                let c = |v: u16| ((v & 31) * 255 / 31) as u8;
                [c(w >> 10), c(w >> 5), c(w), a]
            } else {
                [
                    ((w >> 7) & 0xF8) as u8,
                    ((w >> 2) & 0xF8) as u8,
                    ((w & 31) * 8) as u8,
                    a,
                ]
            }
        }
        [b, g, r] => [r, g, b, 255],
        [b, g, r, a] if bits == 32 => [r, g, b, if alpha { a } else { 255 }],
        _ => [0, 0, 0, 255],
    }
}

/// (attributes type, key colour bytes B, G, R, A) of a TGA 2.0 extension area, if the footer has one.
fn extension(b: &[u8]) -> Option<(u8, [u8; 4])> {
    let footer = b.get(b.len().checked_sub(26)?..)?;
    if &footer[8..] != b"TRUEVISION-XFILE.\0" {
        return None;
    }
    let at = u32::from_le_bytes(bytes_at(footer, 0)?) as usize;
    let area = b.get(at..)?.get(..495).filter(|_| at != 0)?;
    Some((area[494], bytes_at(area, 470)?))
}

/// Decodes types 1/2/3 (raw) and 9/10/11 (RLE) at depths 8/15/16/24/32 (§2.12 Read).
pub fn decode(bytes: &[u8]) -> Result<ImageData, CodecError> {
    let h: [u8; 18] = bytes_at(bytes, 0).ok_or(bad("truncated header"))?;
    let u16_at = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
    let (kind, depth, desc) = (h[2], h[16], h[17]);
    let (map_first, map_len, map_bits) = (u16_at(3) as usize, u16_at(5) as usize, h[7]);
    let (w, ht) = (u16_at(12) as u32, u16_at(14) as u32);
    let (mapped, mono) = (matches!(kind, 1 | 9), matches!(kind, 3 | 11));
    if !matches!(kind, 1 | 2 | 3 | 9 | 10 | 11) {
        return Err(bad("unsupported image type"));
    }
    if w == 0 || ht == 0 || !matches!(depth, 8 | 15 | 16 | 24 | 32) {
        return Err(bad("bad size or depth"));
    }
    if mapped
        && (depth != 8
            || h[1] != 1
            || !(1..=256).contains(&map_len)
            || !matches!(map_bits, 15 | 16 | 24 | 32))
    {
        return Err(bad("bad colour map"));
    }
    // Combinations with no colour rule: grey is 8 bits or 8 + 8 alpha; true colour is never 8 bits.
    if (mono && !matches!(depth, 8 | 16)) || (!mapped && !mono && depth == 8) {
        return Err(bad("unsupported depth for this image type"));
    }
    // Sides above MAX_DECODE_SIDE (or more than MAX_DECODE_BYTES of RGBA) are refused here.
    let len = check_size(w, ht)?;

    // Alpha type: by the colour map entry or pixel depth and the descriptor's alpha bits, then
    // overridden by a TGA 2.0 extension's attribute (3 straight, 4 premultiplied, else none).
    let alpha_bits = desc & 0x0F;
    let mut alpha = match () {
        _ if mapped => (map_bits == 16 && alpha_bits == 1) || map_bits == 32,
        _ if mono => depth == 16 && alpha_bits == 8,
        _ => (depth == 16 && alpha_bits == 1) || depth == 32,
    };
    let ext = extension(bytes);
    let mut premultiplied = false;
    if let (true, Some((attr, _))) = (alpha, ext) {
        premultiplied = attr == 4;
        alpha = matches!(attr, 3 | 4);
    }

    // The image ID is skipped; a colour map is read for mapped images and skipped otherwise.
    let mut at = 18 + h[0] as usize;
    let mut palette = [[0u8, 0, 0, 255]; 256];
    if h[1] == 1 {
        let entry = (map_bits as usize).div_ceil(8);
        let map = bytes
            .get(at..)
            .and_then(|b| b.get(..map_len * entry))
            .ok_or(bad("truncated colour map"))?;
        if mapped {
            for (slot, e) in palette
                .iter_mut()
                .skip(map_first)
                .zip(map.chunks_exact(entry))
            {
                *slot = color(e, map_bits, alpha, true);
            }
        }
        at += map.len();
    }

    let bpp = (depth as usize).div_ceil(8);
    let n = len / 4;
    let body = bytes.get(at..).unwrap_or_default();
    // Refuse before allocating when the body cannot hold the pixels: raw needs n·bpp bytes, RLE at
    // least one (1 + bpp)-byte packet per 128 pixels.
    let rle = kind >= 9;
    let least = if rle {
        n.div_ceil(128) * (1 + bpp)
    } else {
        n * bpp
    };
    if body.len() < least {
        return Err(bad("truncated pixel data"));
    }
    let pixel = |s: &[u8]| match s {
        [i] if mapped => palette[*i as usize],
        [v] => [*v, *v, *v, 255],
        [v, a] if mono => [*v, *v, *v, if alpha { *a } else { 255 }],
        _ => color(s, depth, alpha, false),
    };

    // Stored rows run bottom-up unless descriptor bit 0x20; bit 0x10 mirrors X. RLE packets may
    // span rows, so pixels are placed by their index in the stream.
    let (wu, hu) = (w as usize, ht as usize);
    let (top_down, mirror) = (desc & 0x20 != 0, desc & 0x10 != 0);
    let mut out = vec![0u8; len];
    let mut put = |k: usize, p: [u8; 4]| {
        let (row, col) = (k / wu, k % wu);
        let y = if top_down { row } else { hu - 1 - row };
        let x = if mirror { wu - 1 - col } else { col };
        out[(y * wu + x) * 4..][..4].copy_from_slice(&p);
    };
    let (mut k, mut p) = (0, 0);
    while k < n {
        let (count, run) = if rle {
            let head = *body.get(p).ok_or(bad("truncated pixel data"))?;
            p += 1;
            ((head as usize & 0x7F) + 1, head & 0x80 != 0)
        } else {
            (n, false)
        };
        let mut value = [0; 4];
        for i in 0..count.min(n - k) {
            if !run || i == 0 {
                value = pixel(body.get(p..p + bpp).ok_or(bad("truncated pixel data"))?);
                p += bpp;
            }
            put(k, value);
            k += 1;
        }
    }

    // Fix-ups (§2.12 Read): a 16-bit image whose alpha is 0 everywhere while some colour is not
    // gets A = 255; premultiplied alpha becomes straight; a non-zero key colour is made transparent.
    let px = out.as_chunks_mut::<4>().0;
    if depth == 16
        && alpha
        && px.iter().all(|q| q[3] == 0)
        && px.iter().any(|q| q[..3] != [0, 0, 0])
    {
        px.iter_mut().for_each(|q| q[3] = 255);
    }
    if premultiplied {
        for q in px.iter_mut().filter(|q| q[3] != 0) {
            let a = q[3] as u32;
            for c in &mut q[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    if let Some((_, key)) = ext.filter(|e| e.1 != [0; 4]) {
        for q in px.iter_mut().filter(|q| [q[2], q[1], q[0], q[3]] == key) {
            q[3] = 0;
        }
    }
    Ok(ImageData {
        width: w,
        height: ht,
        data: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tga(kind: u8, depth: u8, descriptor: u8, w: u16, h: u16, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        v.extend(w.to_le_bytes());
        v.extend(h.to_le_bytes());
        v.extend([depth, descriptor]);
        v.extend(body);
        v
    }

    /// document-ops-and-formats.md §2.12 Read: 16-bit direct pixels use B = (w & 31)·8, G = (w >> 2) & 0xF8, R = (w >> 7) & 0xF8, no bit replication.
    #[test]
    fn sixteen_bit_expansion() {
        let img = decode(&tga(2, 16, 0x21, 1, 1, &0xFFFFu16.to_le_bytes())).unwrap();
        assert_eq!(img.data, [248, 248, 248, 255]);
    }

    /// Bottom-up unless descriptor bit 0x20; depth 32 carries straight alpha (§2.12 Read "Alpha type").
    #[test]
    fn origin_and_alpha() {
        let body = [0u8, 0, 255, 7, 255, 0, 0, 7]; // two BGRA pixels: red then blue
        let up = decode(&tga(2, 32, 0x00, 1, 2, &body)).unwrap();
        assert_eq!(up.data, [0, 0, 255, 7, 255, 0, 0, 7]); // row 0 is the last stored row; depth 32 is straight alpha
        let down = decode(&tga(2, 32, 0x28, 1, 2, &body)).unwrap(); // 0x20 top-down, 8 alpha bits
        assert_eq!(down.data, [255, 0, 0, 7, 0, 0, 255, 7]);
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(decode(&tga(5, 32, 0, 1, 1, &[0; 4])).is_err());
        assert!(decode(&tga(2, 12, 0, 1, 1, &[0; 4])).is_err());
        assert!(decode(&tga(2, 32, 0, 0, 1, &[])).is_err());
        assert!(decode(&tga(2, 32, 0, 4, 4, &[0; 8])).is_err());
    }

    /// TGA 2.0 extension area + footer with the given attribute byte and key colour (B, G, R, A).
    fn with_extension(mut v: Vec<u8>, attr: u8, key: [u8; 4]) -> Vec<u8> {
        let at = v.len() as u32;
        let mut area = vec![0u8; 495];
        area[..2].copy_from_slice(&495u16.to_le_bytes());
        area[470..474].copy_from_slice(&key);
        area[494] = attr;
        v.extend(area);
        v.extend(at.to_le_bytes());
        v.extend([0; 4]);
        v.extend(b"TRUEVISION-XFILE.\0");
        v
    }

    /// RLE packets may span rows (§2.12 Read); a run and a raw packet over a 3×2 bottom-up image.
    #[test]
    fn rle_spans_rows() {
        let body = [0x83, 1, 2, 3, 0x01, 4, 5, 6, 7, 8, 9]; // run of 4 × BGR(1,2,3), raw 2 pixels
        let img = decode(&tga(10, 24, 0, 3, 2, &body)).unwrap();
        let (a, b, c) = ([3, 2, 1, 255], [6, 5, 4, 255], [9, 8, 7, 255]);
        assert_eq!(img.data, [a, b, c, a, a, a].concat()); // stored row 0 = a a a is the bottom row
        assert!(
            decode(&tga(10, 24, 0, 3, 2, &body[..8])).is_err(),
            "short RLE body"
        );
        assert!(decode(&tga(10, 24, 0, 3, 2, &[])).is_err());
    }

    /// Colour maps: 16-bit entries expand by c·255/31; bit 0x10 mirrors X.
    #[test]
    fn colour_map_and_mirror() {
        let mut v = vec![0u8, 1, 1, 0, 0, 2, 0, 16, 0, 0, 0, 0, 2, 0, 1, 0, 8, 0x30];
        v.extend(0x7C00u16.to_le_bytes()); // entry 0: R = 31
        v.extend(0x001Fu16.to_le_bytes()); // entry 1: B = 31
        v.extend([0, 1]);
        assert_eq!(decode(&v).unwrap().data, [0, 0, 255, 255, 255, 0, 0, 255]);
    }

    /// The extension attribute decides the alpha type; the key colour clears matching pixels.
    #[test]
    fn extension_alpha_and_key_colour() {
        let base = tga(2, 32, 0x28, 2, 1, &[10, 20, 30, 128, 1, 2, 3, 4]);
        let none = decode(&with_extension(base.clone(), 0, [0; 4])).unwrap();
        assert_eq!(none.data, [30, 20, 10, 255, 3, 2, 1, 255]); // 32-bit without alpha → A = 255
        let pre = decode(&with_extension(base.clone(), 4, [0; 4])).unwrap();
        assert_eq!(&pre.data[..4], [60, 40, 20, 128]); // premultiplied → straight
        let keyed = decode(&with_extension(base, 3, [1, 2, 3, 4])).unwrap();
        assert_eq!(keyed.data, [30, 20, 10, 128, 3, 2, 1, 0]);
    }

    /// A 16-bit image whose alpha bits are all 0 while colour is not is opaque.
    #[test]
    fn sixteen_bit_all_zero_alpha_is_opaque() {
        let img = decode(&tga(2, 16, 0x21, 1, 1, &0x7FFFu16.to_le_bytes())).unwrap();
        assert_eq!(img.data, [248, 248, 248, 255]);
        let black = decode(&tga(2, 16, 0x21, 1, 1, &[0, 0])).unwrap();
        assert_eq!(black.data, [0, 0, 0, 0]);
    }

    fn clock() -> TgaClock {
        TgaClock::from_unix(1_792_324_800)
    }

    /// The body between the 18-byte header and the extension area.
    fn body(b: &[u8]) -> &[u8] {
        &b[18..b.len() - 495 - 26]
    }

    /// 32-bit (any non-opaque pixel): descriptor 8, BGRA with the colour of A = 0 kept, rows
    /// bottom-up, attributes 3; RLE packets stop at each row end; raw mode is image type 2.
    #[test]
    fn writer_32_bit_rows_and_raw_mode() {
        let (a, b, c) = ([1, 2, 3, 0], [4, 5, 6, 255], [7, 8, 9, 128]);
        let img = ImageData {
            width: 2,
            height: 2,
            data: [a, b, c, c].concat(),
        };
        let rle = encode(&img, &SaveOptions::default_for("tga"), clock());
        assert_eq!(rle[2], 10);
        assert_eq!(&rle[16..18], [32, 8]);
        assert_eq!(
            body(&rle),
            [0x81, 9, 8, 7, 128, 0x01, 3, 2, 1, 0, 6, 5, 4, 255]
        );
        assert_eq!(rle[rle.len() - 26 - 1], 3);
        assert_eq!(decode(&rle).unwrap().data, img.data);
        let raw_opts = SaveOptions {
            rle: false,
            ..SaveOptions::default_for("tga")
        };
        let raw = encode(&img, &raw_opts, clock());
        assert_eq!(raw[2], 2);
        assert_eq!(
            body(&raw),
            [9, 8, 7, 128, 9, 8, 7, 128, 3, 2, 1, 0, 6, 5, 4, 255]
        );
        let flat = ImageData {
            width: 2,
            height: 2,
            data: [b; 4].concat(),
        };
        let flat = encode(&flat, &SaveOptions::default_for("tga"), clock());
        assert_eq!(&flat[16..18], [24, 0]);
        assert_eq!(flat[flat.len() - 26 - 1], 0);
        assert_eq!(body(&flat), [0x81, 6, 5, 4, 0x81, 6, 5, 4]);
    }

    /// Packets hold at most 128 pixels, run or raw; a forced 24-bit save flattens onto white.
    #[test]
    fn packet_limits_and_forced_24_bit() {
        let row = |px: Vec<[u8; 4]>| {
            let img = ImageData {
                width: px.len() as u32,
                height: 1,
                data: px.concat(),
            };
            body(&encode(&img, &SaveOptions::default_for("tga"), clock())).to_vec()
        };
        let x = [10, 20, 30, 255];
        assert_eq!(row(vec![x; 130]), [0xFF, 30, 20, 10, 0x81, 30, 20, 10]);
        let distinct: Vec<[u8; 4]> = (0..130u8).map(|i| [i, 0, 0, 255]).collect();
        let raw = row(distinct);
        assert_eq!((raw[0], raw.len()), (0x7F, 1 + 128 * 3 + 1 + 2 * 3));
        assert_eq!(&raw[1 + 128 * 3..], [0x01, 0, 0, 128, 0, 0, 129]);
        let forced = SaveOptions {
            bit_depth: crate::io::encode::BitDepth::B24,
            ..SaveOptions::default_for("tga")
        };
        let img = ImageData {
            width: 1,
            height: 1,
            data: vec![100, 0, 0, 128],
        };
        assert_eq!(body(&encode(&img, &forced, clock())), [0x00, 127, 127, 177]);
    }

    /// The extension area's date/time fields and the civil-date conversion.
    #[test]
    fn clock_and_extension_area() {
        let img = ImageData {
            width: 1,
            height: 1,
            data: vec![0, 0, 0, 255],
        };
        let b = encode(&img, &SaveOptions::default_for("tga"), clock());
        let area = &b[b.len() - 26 - 495..b.len() - 26];
        let u16s: Vec<u16> = (0..6)
            .map(|k| u16::from_le_bytes([area[367 + 2 * k], area[368 + 2 * k]]))
            .collect();
        assert_eq!(u16s, [10, 18, 2026, 12, 0, 0]);
        assert_eq!(&area[426..443], b"Paint.NET 5.1.12\0");
        assert_eq!(
            &area[467..482],
            [0, 0, 0x20, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0]
        );
        assert!(area[482..494].iter().all(|&v| v == 0));
        let t = |s| {
            let c = TgaClock::from_unix(s);
            [c.year, c.month, c.day, c.hour, c.minute, c.second]
        };
        assert_eq!(t(0), [1970, 1, 1, 0, 0, 0]);
        assert_eq!(t(1_709_210_096), [2024, 2, 29, 12, 34, 56]);
        assert_eq!(t(4_107_628_799), [2100, 3, 1, 23, 59, 59]);
        assert!(TgaClock::now().year >= 2026);
    }

    #[test]
    fn hostile_inputs() {
        // Huge declared sizes are refused before allocating; truncation and flips never panic.
        assert!(decode(&tga(2, 32, 0, 40000, 40000, &[])).is_err());
        assert!(decode(&tga(10, 32, 0, 32768, 32768, &[0xFF, 0, 0, 0, 0])).is_err());
        let good = with_extension(tga(9, 8, 0x08, 3, 2, &[0x85, 0]), 3, [0; 4]);
        let mut good = good;
        good[1] = 1;
        good[5] = 1; // one colour map entry, 32-bit
        good[7] = 32;
        good.splice(18..18, [9, 8, 7, 6]);
        assert!(decode(&good).is_ok());
        for cut in 0..good.len() {
            let _ = decode(&good[..cut]);
        }
        for at in 0..good.len() {
            for bit in 0..8 {
                let mut b = good.clone();
                b[at] ^= 1 << bit;
                let _ = decode(&b);
            }
        }
    }
}
