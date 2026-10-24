//! `iloc`: where each item's bytes actually live.

use alloc::vec::Vec;

use crate::boxes::BoxHeader;
use crate::error::{Error, Result};

/// How an item's extents should be interpreted.
///
/// The numbering is the one ISO/IEC 14496-12 gives: 0 is an offset into the
/// file, 1 into the `idat` box, 2 into another item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Construction {
    /// Offsets are absolute positions in the file.
    File,
    /// Offsets are positions within the `idat` box of the same `meta`.
    Idat,
    /// Offsets are positions within another item's reconstructed data.
    Item,
}

impl Construction {
    fn from_code(code: u8) -> Result<Construction> {
        Ok(match code {
            0 => Construction::File,
            1 => Construction::Idat,
            2 => Construction::Item,
            _ => return Err(Error::Unsupported("unknown iloc construction method")),
        })
    }
}

/// One run of bytes belonging to an item.
#[derive(Debug, Clone, Copy)]
pub struct Extent {
    /// For [`Construction::Item`] this names the item to read from.
    pub index: u64,
    /// Offset, to be added to the item's base offset.
    pub offset: u64,
    /// Length in bytes. Zero means "to the end of the containing object".
    pub length: u64,
}

/// Where one item lives.
#[derive(Debug, Clone)]
pub struct ItemLocation {
    /// The item this entry describes.
    pub id: u32,
    /// How to read the extents.
    pub construction: Construction,
    /// Added to every extent offset.
    pub base_offset: u64,
    /// The runs of bytes, in order.
    pub extents: Vec<Extent>,
}

impl ItemLocation {
    /// Total length of the item, or `None` when an extent ran to the end.
    pub fn total_length(&self) -> Option<u64> {
        let mut total = 0u64;
        for e in &self.extents {
            if e.length == 0 {
                return None;
            }
            total = total.checked_add(e.length)?;
        }
        Some(total)
    }
}

/// Parse the `iloc` box.
pub fn parse(b: &BoxHeader<'_>) -> Result<Vec<ItemLocation>> {
    let (version, _flags, mut r) = b.full_box("iloc")?;
    if version > 2 {
        return Err(Error::Unsupported("iloc version above 2"));
    }
    let sizes = r.u8("iloc field sizes")?;
    let offset_size = sizes >> 4;
    let length_size = sizes & 0x0f;
    let sizes2 = r.u8("iloc field sizes")?;
    let base_offset_size = sizes2 >> 4;
    let index_size = if version >= 1 { sizes2 & 0x0f } else { 0 };

    let item_count = if version < 2 {
        u32::from(r.u16("iloc item count")?)
    } else {
        r.u32("iloc item count")?
    };
    // The smallest possible entry is an id plus a data reference index plus an
    // extent count: refuse a count that could not fit before reserving for it.
    if item_count as usize > r.remaining() / 6 + 1 {
        return Err(Error::Malformed("iloc declares more items than can fit"));
    }
    let mut out = Vec::with_capacity(core::cmp::min(item_count as usize, 4096));
    for _ in 0..item_count {
        let id = if version < 2 {
            u32::from(r.u16("iloc item id")?)
        } else {
            r.u32("iloc item id")?
        };
        let construction = if version >= 1 {
            let word = r.u16("iloc construction method")?;
            Construction::from_code((word & 0x0f) as u8)?
        } else {
            Construction::File
        };
        let _data_reference_index = r.u16("iloc data reference index")?;
        let base_offset = r.uint(base_offset_size, "iloc base offset")?;
        let extent_count = r.u16("iloc extent count")?;
        let per = usize::from(offset_size + length_size + index_size);
        if per > 0 && usize::from(extent_count) * per > r.remaining() {
            return Err(Error::Malformed("iloc declares more extents than it holds"));
        }
        let mut extents = Vec::with_capacity(usize::from(extent_count));
        for _ in 0..extent_count {
            let index = r.uint(index_size, "iloc extent index")?;
            let offset = r.uint(offset_size, "iloc extent offset")?;
            let length = r.uint(length_size, "iloc extent length")?;
            extents.push(Extent {
                index,
                offset,
                length,
            });
        }
        out.push(ItemLocation {
            id,
            construction,
            base_offset,
            extents,
        });
    }
    Ok(out)
}
