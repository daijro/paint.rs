//! EXIF and XMP, extracted as borrowed slices.

use crate::error::{Error, Result};
use crate::reader::Reader;

/// The MIME type macOS writes for the XMP packet it attaches to a photograph.
const XMP_TYPES: [&str; 2] = ["application/rdf+xml", "application/xmp+xml"];

/// Strip the HEIF wrapper from an `Exif` item and return the TIFF block.
///
/// The item begins with a 32-bit count of the bytes that precede the TIFF
/// header, which in files written by macOS is 6 — the length of the
/// `Exif\0\0` marker that follows it. Returning the TIFF block itself is what
/// every EXIF reader actually wants; the untouched item bytes remain available
/// from the caller.
pub fn exif_tiff(item: &[u8]) -> Result<&[u8]> {
    let mut r = Reader::new(item);
    let offset = r.u32("exif tiff header offset")?;
    let offset = usize::try_from(offset).map_err(|_| Error::Truncated("exif payload"))?;
    r.skip(offset, "exif payload")?;
    let rest = r.rest();
    if rest.len() < 8 {
        return Err(Error::Truncated("exif tiff header"));
    }
    // A TIFF header is `II` or `MM` followed by 42 in that byte order.
    let magic_ok = match &rest[..2] {
        b"II" => u16::from_le_bytes([rest[2], rest[3]]) == 42,
        b"MM" => u16::from_be_bytes([rest[2], rest[3]]) == 42,
        _ => false,
    };
    if !magic_ok {
        return Err(Error::Malformed("exif item does not contain a TIFF header"));
    }
    Ok(rest)
}

/// True when a `mime` item's content type marks it as an XMP packet.
pub fn is_xmp(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|t| XMP_TYPES.contains(&t))
}
