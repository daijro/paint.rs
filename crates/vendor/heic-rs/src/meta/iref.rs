//! `iref`: how items refer to one another.

use alloc::vec::Vec;

use crate::boxes::{BoxHeader, BoxIter};
use crate::error::{Error, Result};

/// The reference kinds this crate acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefKind {
    /// `dimg`: the referring item is derived from the referenced items.
    Dimg,
    /// `thmb`: the referring item is a thumbnail of the referenced item.
    Thmb,
    /// `cdsc`: the referring item describes the referenced item, which is how
    /// EXIF and XMP are attached.
    Cdsc,
    /// `auxl`: the referring item is auxiliary to the referenced item, which
    /// is how an alpha plane is attached.
    Auxl,
    /// Any other four-character reference type, kept verbatim.
    Other([u8; 4]),
}

impl RefKind {
    fn from_fourcc(code: [u8; 4]) -> RefKind {
        match &code {
            b"dimg" => RefKind::Dimg,
            b"thmb" => RefKind::Thmb,
            b"cdsc" => RefKind::Cdsc,
            b"auxl" => RefKind::Auxl,
            _ => RefKind::Other(code),
        }
    }
}

/// One reference: `from` points at each id in `to`, with meaning `kind`.
#[derive(Debug, Clone)]
pub struct ItemReference {
    /// What kind of relationship this is.
    pub kind: RefKind,
    /// The item doing the referring.
    pub from: u32,
    /// The items referred to, in the order the file listed them.
    ///
    /// For `dimg` that order is load-bearing: it is the tile order of a grid.
    pub to: Vec<u32>,
}

/// Parse the `iref` box.
pub fn parse(b: &BoxHeader<'_>) -> Result<Vec<ItemReference>> {
    let (version, _flags, r) = b.full_box("iref")?;
    let large = match version {
        0 => false,
        1 => true,
        _ => return Err(Error::Unsupported("iref version above 1")),
    };
    let mut out = Vec::new();
    for child in BoxIter::new(r.rest()) {
        let child = child?;
        let mut cr = child.reader();
        let from = if large {
            cr.u32("iref from_item_ID")?
        } else {
            u32::from(cr.u16("iref from_item_ID")?)
        };
        let count = cr.u16("iref reference count")?;
        let width = if large { 4 } else { 2 };
        if usize::from(count) * width > cr.remaining() {
            return Err(Error::Malformed(
                "iref declares more references than it holds",
            ));
        }
        let mut to = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            to.push(if large {
                cr.u32("iref to_item_ID")?
            } else {
                u32::from(cr.u16("iref to_item_ID")?)
            });
        }
        out.push(ItemReference {
            kind: RefKind::from_fourcc(child.boxtype),
            from,
            to,
        });
    }
    Ok(out)
}

/// Every item that `from` points at with the given kind, in file order.
pub fn targets(refs: &[ItemReference], from: u32, kind: RefKind) -> &[u32] {
    for r in refs {
        if r.from == from && r.kind == kind {
            return &r.to;
        }
    }
    &[]
}

/// Every item that points at `to` with the given kind.
pub fn sources(refs: &[ItemReference], to: u32, kind: RefKind) -> Vec<u32> {
    let mut out = Vec::new();
    for r in refs {
        if r.kind == kind && r.to.contains(&to) {
            out.push(r.from);
        }
    }
    out
}
