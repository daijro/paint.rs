//! The `meta` box: the catalogue that makes a HEIF file navigable.

pub mod iinf;
pub mod iloc;
pub mod iprp;
pub mod iref;

use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::boxes::BoxIter;
use crate::error::{Error, Result};
use iinf::ItemInfo;
use iloc::{Construction, ItemLocation};
use iprp::Properties;
use iref::ItemReference;

/// How deep a chain of item-offset constructions may go before this crate
/// assumes the file is trying to make it loop forever.
const MAX_ITEM_DEPTH: u32 = 8;

/// Everything the `meta` box told us, borrowed from the file.
#[derive(Debug, Clone)]
pub struct Meta<'a> {
    /// The `hdlr` handler type. A still image file says `pict`.
    pub handler: [u8; 4],
    /// The item the file wants shown, from `pitm`.
    pub primary: u32,
    /// The item catalogue from `iinf`.
    pub items: Vec<ItemInfo<'a>>,
    /// The relationships from `iref`.
    pub refs: Vec<ItemReference>,
    /// The byte locations from `iloc`.
    pub locations: Vec<ItemLocation>,
    /// The properties from `iprp`.
    pub props: Properties<'a>,
    /// The `idat` payload, for items constructed from it.
    pub idat: Option<&'a [u8]>,
}

/// Parse the top-level `meta` box of a HEIF file.
pub fn parse(file: &[u8]) -> Result<Meta<'_>> {
    let meta = BoxIter::new(file).require(b"meta", "meta")?;
    // `meta` is a FullBox, so its four leading bytes are version and flags.
    let mut r = meta.reader();
    let _ = r.full_box("meta")?;
    let body = r.rest();

    let mut handler = *b"    ";
    let mut primary = None;
    let mut items = Vec::new();
    let mut refs = Vec::new();
    let mut locations = Vec::new();
    let mut props = Properties::empty();
    let mut idat = None;

    for child in BoxIter::new(body) {
        let child = child?;
        match &child.boxtype {
            b"hdlr" => handler = iinf::parse_hdlr(&child)?,
            b"pitm" => primary = Some(iinf::parse_pitm(&child)?),
            b"iinf" => items = iinf::parse(&child)?,
            b"iref" => refs = iref::parse(&child)?,
            b"iloc" => locations = iloc::parse(&child)?,
            b"iprp" => props = iprp::parse(&child)?,
            b"idat" => idat = Some(child.payload),
            _ => {}
        }
    }

    if &handler != b"pict" {
        return Err(Error::Unsupported(
            "the meta handler is not `pict`; this is not a still-image HEIF file",
        ));
    }
    // A file with one item and no `pitm` is unambiguous; anything else is not.
    let primary = match primary {
        Some(p) => p,
        None if items.len() == 1 => items[0].id,
        None => return Err(Error::MissingBox("pitm")),
    };
    Ok(Meta {
        handler,
        primary,
        items,
        refs,
        locations,
        props,
        idat,
    })
}

impl<'a> Meta<'a> {
    /// The catalogue entry for one item.
    pub fn item(&self, id: u32) -> Option<&ItemInfo<'a>> {
        self.items.iter().find(|i| i.id == id)
    }

    /// The catalogue entry for the primary item.
    pub fn primary_item(&self) -> Result<&ItemInfo<'a>> {
        self.item(self.primary)
            .ok_or(Error::MissingItem(self.primary))
    }

    /// The location entry for one item.
    pub fn location(&self, id: u32) -> Option<&ItemLocation> {
        self.locations.iter().find(|l| l.id == id)
    }

    /// Reassemble one item's bytes.
    ///
    /// An item stored as a single contiguous extent is returned borrowed; only
    /// a genuinely fragmented item is copied into a fresh buffer.
    pub fn item_data(&self, file: &'a [u8], id: u32) -> Result<Cow<'a, [u8]>> {
        self.item_data_at(file, id, 0)
    }

    fn item_data_at(&self, file: &'a [u8], id: u32, depth: u32) -> Result<Cow<'a, [u8]>> {
        if depth > MAX_ITEM_DEPTH {
            return Err(Error::CyclicDerivation(id));
        }
        let loc = self.location(id).ok_or(Error::MissingItem(id))?;
        if loc.extents.is_empty() {
            return Err(Error::Malformed("item location lists no extents"));
        }
        if loc.extents.len() == 1 {
            let e = loc.extents[0];
            return self.extent_bytes(file, loc, &e, depth);
        }
        let mut out = Vec::new();
        for e in &loc.extents {
            let part = self.extent_bytes(file, loc, e, depth)?;
            // An item is assembled from the file's own bytes, so one larger than the file that
            // holds it is a file repeating itself. Unchecked, items built from items (construction
            // method 2), several extents apiece and eight deep, multiplied at every level: 690
            // bytes found by the `probe` fuzz target asked for gigabytes (0.1.2).
            if out.len().saturating_add(part.len()) > file.len() {
                return Err(Error::Malformed(
                    "item data is larger than the file that holds it",
                ));
            }
            out.extend_from_slice(&part);
        }
        Ok(Cow::Owned(out))
    }

    fn extent_bytes(
        &self,
        file: &'a [u8],
        loc: &ItemLocation,
        e: &iloc::Extent,
        depth: u32,
    ) -> Result<Cow<'a, [u8]>> {
        match loc.construction {
            Construction::File => {
                let start = loc.base_offset.checked_add(e.offset);
                Ok(Cow::Borrowed(slice_at(
                    file,
                    start,
                    e.length,
                    "item extent in file",
                )?))
            }
            Construction::Idat => {
                let idat = self.idat.ok_or(Error::MissingBox("idat"))?;
                let start = loc.base_offset.checked_add(e.offset);
                Ok(Cow::Borrowed(slice_at(
                    idat,
                    start,
                    e.length,
                    "item extent in idat",
                )?))
            }
            Construction::Item => {
                let source = u32::try_from(e.index)
                    .map_err(|_| Error::Malformed("iloc extent index is not an item id"))?;
                if source == loc.id {
                    return Err(Error::CyclicDerivation(loc.id));
                }
                let base = self.item_data_at(file, source, depth + 1)?;
                let start = loc.base_offset.checked_add(e.offset);
                let part = slice_at(&base, start, e.length, "item extent in item")?;
                Ok(Cow::Owned(part.to_vec()))
            }
        }
    }
}

/// Take `length` bytes from `data` at `start`, where a length of zero means
/// "everything that is left".
fn slice_at<'b>(
    data: &'b [u8],
    start: Option<u64>,
    length: u64,
    what: &'static str,
) -> Result<&'b [u8]> {
    let start = start.ok_or(Error::Malformed("item offset overflows"))?;
    let start = usize::try_from(start).map_err(|_| Error::Truncated(what))?;
    if start > data.len() {
        return Err(Error::Truncated(what));
    }
    let rest = &data[start..];
    if length == 0 {
        return Ok(rest);
    }
    let length = usize::try_from(length).map_err(|_| Error::Truncated(what))?;
    if length > rest.len() {
        return Err(Error::Truncated(what));
    }
    Ok(&rest[..length])
}
