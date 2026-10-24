//! Checked bounds for the CBLC path used by ttf-parser 0.25.1's CBDT lookup.
//! The library checks reads, but its selected subtable and image-offset additions
//! can overflow on wasm32. This guard mirrors strike/subtable selection and rejects
//! malformed or unsupported PNG locations before calling that lookup.
use ttf_parser::{Face, Tag};

fn slice(data: &[u8], offset: u32, len: u32) -> Option<&[u8]> {
    let end = offset.checked_add(len)?;
    data.get(usize::try_from(offset).ok()?..usize::try_from(end).ok()?)
}
fn u8_at(data: &[u8], offset: u32) -> Option<u8> {
    Some(*slice(data, offset, 1)?.first()?)
}
fn u16_at(data: &[u8], offset: u32) -> Option<u16> {
    Some(u16::from_be_bytes(slice(data, offset, 2)?.try_into().ok()?))
}
fn u32_at(data: &[u8], offset: u32) -> Option<u32> {
    Some(u32::from_be_bytes(slice(data, offset, 4)?.try_into().ok()?))
}
fn add(a: u32, b: u32) -> Option<u32> {
    a.checked_add(b)
}

pub(super) fn safe_png_lookup(face: &Face<'_>, gid: u16, requested_ppem: u16) -> bool {
    checked_png_lookup(face, gid, requested_ppem).is_some()
}

fn checked_png_lookup(face: &Face<'_>, gid: u16, requested_ppem: u16) -> Option<()> {
    // glyph_raster_image prioritizes these tables over CBDT. Their parsers are
    // outside this extension's checked CBLC contract, so use the outline fallback.
    let tables = face.tables();
    if tables.sbix.is_some() || tables.bdat.is_some() || tables.ebdt.is_some() {
        return None;
    }
    let raw = face.raw_face();
    // Duplicate bitmap table records can make the face's parsed table and a
    // separate raw lookup disagree about which bytes the guard examined.
    let mut seen_cblc = false;
    let mut seen_cbdt = false;
    for i in 0..raw.table_records.len() {
        let tag = raw.table_records.get(i)?.tag;
        if tag == Tag::from_bytes(b"CBLC") {
            if seen_cblc {
                return None;
            }
            seen_cblc = true;
        } else if tag == Tag::from_bytes(b"CBDT") {
            if seen_cbdt {
                return None;
            }
            seen_cbdt = true;
        }
    }
    let cblc = raw.table(Tag::from_bytes(b"CBLC"))?;
    let cbdt = raw.table(Tag::from_bytes(b"CBDT"))?;
    let count = u32_at(cblc, 4)?;
    slice(cblc, 8, count.checked_mul(48)?)?;

    // Match ttf-parser's best-strike rule, including its order for a tie.
    let mut selected = None;
    let mut max_ppem = 0u16;
    for i in 0..count {
        let row = add(8, i.checked_mul(48)?)?;
        let start = u16_at(cblc, add(row, 40)?)?;
        let end = u16_at(cblc, add(row, 42)?)?;
        let ppem = u16::from(u8_at(cblc, add(row, 44)?)?);
        if start <= gid
            && gid <= end
            && ((requested_ppem <= ppem && ppem < max_ppem)
                || (requested_ppem > max_ppem && ppem > max_ppem))
        {
            selected = Some(row);
            max_ppem = ppem;
        }
    }
    let row = selected?;
    let array = u32_at(cblc, row)?;
    let subtables = u32_at(cblc, add(row, 8)?)?;
    slice(cblc, array, subtables.checked_mul(8)?)?;
    for i in 0..subtables {
        let entry = add(array, i.checked_mul(8)?)?;
        let start = u16_at(cblc, entry)?;
        let end = u16_at(cblc, add(entry, 2)?)?;
        if gid < start || gid > end {
            continue;
        }
        let subtable = add(array, u32_at(cblc, add(entry, 4)?)?)?;
        slice(cblc, subtable, 8)?;
        let index_format = u16_at(cblc, subtable)?;
        let image_format = u16_at(cblc, add(subtable, 2)?)?;
        if !matches!(image_format, 17..=19) {
            return None;
        }
        let image_base = u32_at(cblc, add(subtable, 4)?)?;
        let diff = u32::from(gid - start);
        let relative = match index_format {
            1 => u32_at(cblc, add(add(subtable, 8)?, diff.checked_mul(4)?)?)?,
            2 | 5 => {
                let size = u32_at(cblc, add(subtable, 8)?)?;
                if index_format == 5 {
                    let n = u32_at(cblc, add(subtable, 20)?)?;
                    // Glyph IDs are u16; this also bounds a malformed scan.
                    if n > 65536 {
                        return None;
                    }
                    let ids = add(subtable, 24)?;
                    slice(cblc, ids, n.checked_mul(2)?)?;
                    let mut found = None;
                    let mut previous = None;
                    for j in 0..n {
                        let id = u16_at(cblc, add(ids, j.checked_mul(2)?)?)?;
                        // The parser binary-searches this array. Validate its
                        // ordering and uniqueness so it selects this same entry.
                        if previous.is_some_and(|last| id <= last) {
                            return None;
                        }
                        previous = Some(id);
                        if id == gid {
                            found = Some(j);
                        }
                    }
                    found?.checked_mul(size)?
                } else {
                    slice(cblc, add(subtable, 12)?, 8)?; // shared big metrics
                    diff.checked_mul(size)?
                }
            }
            3 => u32::from(u16_at(cblc, add(add(subtable, 8)?, diff.checked_mul(2)?)?)?),
            4 => {
                let n = u32_at(cblc, add(subtable, 8)?)?;
                if n > 65536 {
                    return None;
                }
                let pairs = add(subtable, 12)?;
                slice(cblc, pairs, n.checked_add(1)?.checked_mul(4)?)?;
                let mut found = None;
                for j in 0..n {
                    let pair = add(pairs, j.checked_mul(4)?)?;
                    if u16_at(cblc, pair)? == gid {
                        found = Some(u32::from(u16_at(cblc, add(pair, 2)?)?));
                        break;
                    }
                }
                found?
            }
            _ => return None,
        };
        let image = add(image_base, relative)?;
        let metrics = match image_format {
            17 => 5,
            18 => 8,
            19 if matches!(index_format, 2 | 5) => 0,
            _ => return None,
        };
        let length_at = add(image, metrics)?;
        let length = u32_at(cbdt, length_at)?;
        let png_start = add(length_at, 4)?;
        slice(cbdt, png_start, length)?;
        return Some(());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOTO: &[u8] =
        include_bytes!("../../../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");

    #[test]
    fn index_format_five_rejects_duplicate_glyph_ids_before_parser_search() {
        let original = Face::parse(NOTO, 0).unwrap();
        let gid = original.glyph_index('🎂').unwrap().0;
        assert!(safe_png_lookup(&original, gid, 12));
        let mut font = NOTO.to_vec();
        // Reuse cake's valid PNG bytes as the first entry of a synthetic format-5
        // subtable. A duplicate second ID makes ttf-parser's binary_search pick
        // a different image than a first-match linear guard would validate.
        let image_base = u32::from_be_bytes(font[10_595_692..10_595_696].try_into().unwrap());
        let cake_relative = u32::from_be_bytes(font[10_597_116..10_597_120].try_into().unwrap());
        font[10_595_688..10_595_690].copy_from_slice(&5u16.to_be_bytes());
        font[10_595_690..10_595_692].copy_from_slice(&19u16.to_be_bytes());
        font[10_595_692..10_595_696]
            .copy_from_slice(&(image_base + cake_relative + 5).to_be_bytes());
        font[10_595_696..10_595_700].copy_from_slice(&1u32.to_be_bytes());
        font[10_595_708..10_595_712].copy_from_slice(&2u32.to_be_bytes());
        font[10_595_712..10_595_714].copy_from_slice(&gid.to_be_bytes());
        font[10_595_714..10_595_716].copy_from_slice(&gid.to_be_bytes());
        let malformed = Face::parse(&font, 0).unwrap();
        assert!(!safe_png_lookup(&malformed, gid, 12));
    }

    #[test]
    fn checked_lookup_rejects_wasm_overflow_and_image_offset_mutations() {
        let original = Face::parse(NOTO, 0).unwrap();
        let gid = original.glyph_index('🎂').unwrap().0;
        assert!(safe_png_lookup(&original, gid, 12));
        let mut font = NOTO.to_vec();
        // The reviewed browser RED: 56 + fffffff0 overflows wasm32 usize.
        font[10_595_608..10_595_612].copy_from_slice(&0xfffffff0u32.to_be_bytes());
        let malformed = Face::parse(&font, 0).unwrap();
        assert!(!safe_png_lookup(&malformed, gid, 12));
        // Reject a false size-table count and an out-of-table subtable array.
        font.copy_from_slice(NOTO);
        font[10_595_544..10_595_548].copy_from_slice(&u32::MAX.to_be_bytes());
        let malformed = Face::parse(&font, 0).unwrap();
        assert!(!safe_png_lookup(&malformed, gid, 12));
        font.copy_from_slice(NOTO);
        font[10_595_548..10_595_552].copy_from_slice(&u32::MAX.to_be_bytes());
        let malformed = Face::parse(&font, 0).unwrap();
        assert!(!safe_png_lookup(&malformed, gid, 12));
        // A selected index-format-1 image offset must fit the CBDT table too.
        font.copy_from_slice(NOTO);
        font[10_595_692..10_595_696].copy_from_slice(&u32::MAX.to_be_bytes());
        let malformed = Face::parse(&font, 0).unwrap();
        assert!(!safe_png_lookup(&malformed, gid, 12));
        font.copy_from_slice(NOTO);
        // Glyph 374 is 355 entries into this index-format-1 offset array.
        font[10_597_116..10_597_120].copy_from_slice(&u32::MAX.to_be_bytes());
        let malformed = Face::parse(&font, 0).unwrap();
        assert!(!safe_png_lookup(&malformed, gid, 12));
    }
}
