//! GIF writer for the 8-bit pipeline through the `gif` crate: GIF89a,
//! one frame, a global palette padded to a power of two (at least 2 entries), and for 8A1 a
//! Graphic Control Extension naming the transparent index (the last entry).
// WIC container details unverified (document-ops-and-formats.md §6 item 4; B-F4): the crate writes
// a Graphic Control Extension for every frame, with flags 0 when nothing is transparent.

use std::borrow::Cow;

use super::codecs::{CodecError, invalid};
use super::encode::Raster;

pub(crate) fn write(r: &Raster) -> Result<Vec<u8>, CodecError> {
    let (Ok(w), Ok(h)) = (u16::try_from(r.w), u16::try_from(r.h)) else {
        return Err(invalid("GIF images are at most 65535 pixels per side"));
    };
    let rgb: Vec<u8> = r.palette.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut enc = ::gif::Encoder::new(Vec::new(), w, h, &rgb).map_err(invalid)?;
    let frame = ::gif::Frame {
        width: w,
        height: h,
        buffer: Cow::Borrowed(&r.samples),
        transparent: r.palette.iter().position(|p| p[3] == 0).map(|i| i as u8),
        dispose: ::gif::DisposalMethod::Any, // a still image: no disposal (flags = transparency only)
        ..::gif::Frame::default()
    };
    enc.write_frame(&frame).map_err(invalid)?;
    enc.into_inner().map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::super::codecs::decode;
    use super::super::encode::{SaveOptions, encode};
    use crate::canvas::ImageData;

    /// GIF89a, the global table flag and size (3 colours + transparent → 4 entries; 1 colour → 2),
    /// and the Graphic Control Extension with the transparent flag and index 3.
    #[test]
    fn palette_and_transparency() {
        let o = SaveOptions::default_for("gif");
        let img = ImageData {
            width: 4,
            height: 1,
            data: [
                [1, 2, 3, 255],
                [4, 5, 6, 255],
                [7, 8, 9, 255],
                [0, 0, 0, 100],
            ]
            .concat(),
        };
        let b = encode(&img, "gif", &o).unwrap();
        assert_eq!(&b[..10], b"GIF89a\x04\x00\x01\x00");
        assert_eq!(b[10] & 0x87, 0x81); // global table of 2^(1+1) entries
        assert_eq!(&b[13..25], [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 0, 0]);
        assert_eq!(&b[25..33], [0x21, 0xF9, 4, 1, 0, 0, 3, 0]);
        let back = decode("gif", &b).unwrap().image.data;
        assert_eq!(
            back,
            [[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255], [0, 0, 0, 0]].concat()
        );
        let one = ImageData {
            width: 2,
            height: 2,
            data: [9, 9, 9, 255].repeat(4),
        };
        let b = encode(&one, "gif", &o).unwrap();
        assert_eq!(b[10] & 0x87, 0x80);
        assert_eq!(&b[13..19], [9, 9, 9, 0, 0, 0]);
        assert_eq!(&b[19..27], [0x21, 0xF9, 4, 0, 0, 0, 0, 0]);
        assert_eq!(decode("gif", &b).unwrap().image.data, one.data);
    }
}
