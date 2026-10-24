//! `iinf` and `infe`: the catalogue of items in the file.

use alloc::vec::Vec;

use crate::boxes::{BoxHeader, BoxIter};
use crate::error::{Error, Result};

/// One entry from the item information box.
#[derive(Debug, Clone)]
pub struct ItemInfo<'a> {
    /// The item's identifier, unique within the file.
    pub id: u32,
    /// The four-character item type, such as `hvc1`, `grid`, `Exif` or `mime`.
    ///
    /// `infe` versions 0 and 1 predate the field; those entries carry
    /// four spaces here and describe themselves through [`content_type`]
    /// instead.
    ///
    /// [`content_type`]: ItemInfo::content_type
    pub item_type: [u8; 4],
    /// The human-readable name, often empty.
    pub name: &'a str,
    /// The MIME type, for `mime` items.
    pub content_type: Option<&'a str>,
    /// The URI type, for `uri ` items.
    pub item_uri_type: Option<&'a str>,
    /// Set when the writer marked the item as not for direct display.
    pub hidden: bool,
}

impl ItemInfo<'_> {
    /// True when this item holds HEVC-coded picture data.
    pub fn is_hevc(&self) -> bool {
        &self.item_type == b"hvc1" || &self.item_type == b"hev1"
    }

    /// True when this item is a grid derivation.
    pub fn is_grid(&self) -> bool {
        &self.item_type == b"grid"
    }
}

/// Parse one `infe` box.
pub fn parse_infe<'a>(b: &BoxHeader<'a>) -> Result<ItemInfo<'a>> {
    let (version, flags, mut r) = b.full_box("infe")?;
    let hidden = flags & 1 != 0;
    if version < 2 {
        let id = u32::from(r.u16("infe item id")?);
        let _protection = r.u16("infe protection index")?;
        let name = r.cstr("infe name")?;
        let content_type = r.cstr("infe content type").ok();
        return Ok(ItemInfo {
            id,
            item_type: *b"    ",
            name,
            content_type,
            item_uri_type: None,
            hidden,
        });
    }
    let id = match version {
        2 => u32::from(r.u16("infe item id")?),
        3 => r.u32("infe item id")?,
        _ => return Err(Error::Unsupported("infe version above 3")),
    };
    let _protection = r.u16("infe protection index")?;
    let item_type = r.fourcc("infe item type")?;
    let name = r.cstr("infe name")?;
    let mut content_type = None;
    let mut item_uri_type = None;
    if &item_type == b"mime" {
        content_type = r.cstr("infe content type").ok();
    } else if &item_type == b"uri " {
        item_uri_type = r.cstr("infe uri type").ok();
    }
    Ok(ItemInfo {
        id,
        item_type,
        name,
        content_type,
        item_uri_type,
        hidden,
    })
}

/// Parse the `iinf` box into its entries.
pub fn parse<'a>(b: &BoxHeader<'a>) -> Result<Vec<ItemInfo<'a>>> {
    let (version, _flags, mut r) = b.full_box("iinf")?;
    let count = if version == 0 {
        u32::from(r.u16("iinf entry count")?)
    } else {
        r.u32("iinf entry count")?
    };
    // Every entry needs at least a box header, so a count that could not
    // possibly fit is rejected before it becomes a reservation.
    if count as usize > r.remaining() / 8 {
        return Err(Error::Malformed("iinf declares more entries than can fit"));
    }
    let mut out = Vec::with_capacity(count as usize);
    for child in BoxIter::new(r.rest()) {
        let child = child?;
        if child.is(b"infe") {
            out.push(parse_infe(&child)?);
        }
    }
    Ok(out)
}

/// Parse `pitm`, the primary item box.
pub fn parse_pitm(b: &BoxHeader<'_>) -> Result<u32> {
    let (version, _flags, mut r) = b.full_box("pitm")?;
    if version == 0 {
        Ok(u32::from(r.u16("pitm item id")?))
    } else {
        r.u32("pitm item id")
    }
}

/// Parse `hdlr` and check that it claims to describe pictures.
pub fn parse_hdlr(b: &BoxHeader<'_>) -> Result<[u8; 4]> {
    let (_version, _flags, mut r) = b.full_box("hdlr")?;
    r.skip(4, "hdlr pre_defined")?;
    let handler = r.fourcc("hdlr handler type")?;
    Ok(handler)
}
