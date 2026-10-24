//! One parsed file, and the questions both `probe` and `decode` ask of it.
//!
//! Keeping the resolution logic here means the two public entry points cannot
//! disagree about which item is the primary one, what its geometry is, or
//! whether it has alpha.

use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::ftyp::{Brand, FileType};
use crate::grid::{self, Grid};
use crate::meta::iref::{RefKind, targets};
use crate::meta::{self, Meta};
use crate::metadata;
use crate::props::simple::AuxKind;
use crate::props::{self, ItemProps};

/// A file, parsed as far as its container.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// The whole file, which `iloc` file offsets index into.
    pub file: &'a [u8],
    /// The `ftyp` box.
    pub ftyp: FileType,
    /// The `meta` box.
    pub meta: Meta<'a>,
}

impl<'a> Context<'a> {
    /// Parse the container of `bytes`. No pixels are touched.
    pub fn open(bytes: &'a [u8]) -> Result<Context<'a>> {
        let ftyp = crate::ftyp::parse(bytes)?;
        let meta = meta::parse(bytes)?;
        Ok(Context {
            file: bytes,
            ftyp,
            meta,
        })
    }

    /// The brand this file was read as.
    pub fn brand(&self) -> Brand {
        self.ftyp.effective
    }

    /// Every property associated with an item.
    pub fn props(&self, item: u32) -> Result<ItemProps<'a>> {
        props::gather(&self.meta.props, item)
    }

    /// The bytes of one item.
    pub fn item_data(&self, item: u32) -> Result<Cow<'a, [u8]>> {
        self.meta.item_data(self.file, item)
    }

    /// The grid an item derives from, with its tiles in `dimg` order.
    ///
    /// `Ok(None)` means the item is a coded picture rather than a derivation.
    pub fn grid(&self, item: u32) -> Result<Option<(Grid, Vec<u32>)>> {
        let info = self.meta.item(item).ok_or(Error::MissingItem(item))?;
        if !info.is_grid() {
            if &info.item_type == b"iovl" {
                return Err(Error::Unsupported(
                    "`iovl` overlay derivation is not implemented; only `grid` is",
                ));
            }
            return Ok(None);
        }
        let data = self.item_data(item)?;
        let g = grid::parse(&data)?;
        let tiles = targets(&self.meta.refs, item, RefKind::Dimg).to_vec();
        grid::check_tiles(&g, &tiles)?;
        if tiles.contains(&item) {
            return Err(Error::CyclicDerivation(item));
        }
        // A tile that is itself a derivation would make the mosaic recursive.
        for t in &tiles {
            let ti = self.meta.item(*t).ok_or(Error::MissingItem(*t))?;
            if ti.is_grid() || !targets(&self.meta.refs, *t, RefKind::Dimg).is_empty() {
                return Err(Error::CyclicDerivation(*t));
            }
        }
        Ok(Some((g, tiles)))
    }

    /// The coded size of the primary item, before any transform.
    ///
    /// A grid takes its size from its own `ispe`; a coded picture takes it
    /// from the `ispe` of the item itself.
    pub fn coded_size(&self, item: u32, p: &ItemProps<'a>) -> Result<(u32, u32)> {
        if let Some(ispe) = p.ispe {
            return Ok((ispe.width, ispe.height));
        }
        if let Some((g, _)) = self.grid(item)? {
            return Ok((g.output_width, g.output_height));
        }
        // Without `ispe` the only remaining source is the sequence parameter
        // set, which means going through the codec seam.
        let hvcc = p.hvcc.as_ref().ok_or(Error::MissingBox("ispe"))?;
        let info = crate::hevc::probe(&hvcc.parameter_sets())?;
        Ok((info.width, info.height))
    }

    /// The auxiliary item carrying an alpha plane for `item`, if any.
    pub fn alpha_item(&self, item: u32) -> Result<Option<u32>> {
        for r in &self.meta.refs {
            if r.kind != RefKind::Auxl || !r.to.contains(&item) {
                continue;
            }
            let p = self.props(r.from)?;
            if p.auxc.map(|a| a.kind) == Some(AuxKind::Alpha) {
                return Ok(Some(r.from));
            }
        }
        Ok(None)
    }

    /// The `Exif` item attached to `item` by a `cdsc` reference.
    pub fn exif_item(&self, item: u32) -> Option<u32> {
        self.described_by(item, b"Exif")
    }

    /// The XMP `mime` item attached to `item` by a `cdsc` reference.
    pub fn xmp_item(&self, item: u32) -> Option<u32> {
        for r in &self.meta.refs {
            if r.kind != RefKind::Cdsc || !r.to.contains(&item) {
                continue;
            }
            let is_xmp = self
                .meta
                .item(r.from)
                .is_some_and(|i| &i.item_type == b"mime" && metadata::is_xmp(i.content_type));
            if is_xmp {
                return Some(r.from);
            }
        }
        None
    }

    fn described_by(&self, item: u32, kind: &[u8; 4]) -> Option<u32> {
        for r in &self.meta.refs {
            if r.kind != RefKind::Cdsc || !r.to.contains(&item) {
                continue;
            }
            if self.meta.item(r.from).is_some_and(|i| &i.item_type == kind) {
                return Some(r.from);
            }
        }
        None
    }

    /// The EXIF TIFF block for an item, if it has one.
    pub fn exif(&self, item: u32) -> Result<Option<&'a [u8]>> {
        let Some(id) = self.exif_item(item) else {
            return Ok(None);
        };
        match self.item_data(id)? {
            Cow::Borrowed(b) => metadata::exif_tiff(b).map(Some),
            // An EXIF item split across extents is rare enough that copying it
            // would be the only way to return it; say so rather than lie.
            Cow::Owned(_) => Err(Error::Unsupported(
                "the EXIF item is stored in more than one extent; borrowing it is not possible",
            )),
        }
    }

    /// The ICC profile for an item, if `colr` carried one.
    pub fn icc(&self, p: &ItemProps<'a>) -> Option<&'a [u8]> {
        p.icc
    }
}
