//! Item properties: parsing them, and gathering the ones that belong to a
//! given item into one struct.

pub mod colr;
pub mod hvcc;
pub mod simple;

use alloc::vec::Vec;

use crate::error::Result;
use crate::meta::iprp::Properties;
use colr::ColorInfo;
use hvcc::HevcConfig;
use simple::{AuxC, Clap, Ispe, Mirror, Pasp, Pixi, Rotation};

/// Every property this crate understands, for one item.
///
/// Absent properties are `None` rather than being filled in with a guess; the
/// decision about what an absent `colr` means is taken later, once, and
/// documented there.
#[derive(Debug, Clone, Default)]
pub struct ItemProps<'a> {
    /// The HEVC configuration record, present on coded picture items.
    pub hvcc: Option<HevcConfig<'a>>,
    /// The coded size.
    pub ispe: Option<Ispe>,
    /// Bit depth per channel.
    pub pixi: Option<Pixi>,
    /// The coded colour description from a `colr` box of type `nclx`.
    pub nclx: Option<colr::Nclx>,
    /// An ICC profile from a `colr` box of type `prof` or `rICC`, borrowed
    /// from the file. A file may carry both this and [`nclx`].
    ///
    /// [`nclx`]: ItemProps::nclx
    pub icc: Option<&'a [u8]>,
    /// True when the ICC profile came from `rICC` rather than `prof`.
    pub icc_restricted: bool,
    /// The transformative properties, in the order `ipma` associated them.
    ///
    /// ISO/IEC 23008-12 says a reader applies these in association order, so
    /// the order is data and is preserved here rather than being normalised
    /// into three independent fields.
    pub transforms: Vec<Transform>,
    /// Auxiliary type, on auxiliary items.
    pub auxc: Option<AuxC<'a>>,
    /// Pixel aspect ratio, reported but not applied.
    pub pasp: Option<Pasp>,
}

/// One transformative property, carrying its parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// Crop to a clean aperture.
    Crop(Clap),
    /// Rotate counter-clockwise.
    Rotate(Rotation),
    /// Mirror.
    Mirror(Mirror),
}

impl ItemProps<'_> {
    /// The rotation among the transformative properties, if any.
    pub fn rotation(&self) -> Rotation {
        self.transforms
            .iter()
            .find_map(|t| match t {
                Transform::Rotate(r) => Some(*r),
                _ => None,
            })
            .unwrap_or(Rotation::None)
    }

    /// The mirroring among the transformative properties, if any.
    pub fn mirror(&self) -> Option<Mirror> {
        self.transforms.iter().find_map(|t| match t {
            Transform::Mirror(m) => Some(*m),
            _ => None,
        })
    }

    /// The clean aperture among the transformative properties, if any.
    pub fn clap(&self) -> Option<Clap> {
        self.transforms.iter().find_map(|t| match t {
            Transform::Crop(c) => Some(*c),
            _ => None,
        })
    }

    /// The luma bit depth, preferring `pixi` and falling back to `hvcC`.
    pub fn bit_depth(&self) -> Option<u8> {
        if let Some(first) = self.pixi.as_ref().and_then(|p| p.bits_per_channel.first()) {
            return Some(*first);
        }
        self.hvcc.as_ref().map(|c| c.bit_depth_luma)
    }
}

/// Collect every property associated with `item`.
///
/// Properties this crate does not model are skipped here; the caller can ask
/// [`Properties::unknown_essential`] whether any of them were marked essential.
pub fn gather<'a>(props: &Properties<'a>, item: u32) -> Result<ItemProps<'a>> {
    let mut out = ItemProps::default();
    for a in props.associations(item) {
        let Some(b) = props.resolve(*a) else { continue };
        match &b.boxtype {
            b"hvcC" => out.hvcc = Some(hvcc::parse(b)?),
            b"ispe" => out.ispe = Some(simple::parse_ispe(b)?),
            b"pixi" => out.pixi = Some(simple::parse_pixi(b)?),
            // A file may carry two `colr` boxes, one of each flavour. They
            // answer different questions, so both are kept.
            b"colr" => match colr::parse(b)? {
                ColorInfo::Nclx(n) => out.nclx = Some(n),
                ColorInfo::Icc { restricted, data } => {
                    out.icc = Some(data);
                    out.icc_restricted = restricted;
                }
            },
            b"irot" => out
                .transforms
                .push(Transform::Rotate(simple::parse_irot(b)?)),
            b"imir" => out
                .transforms
                .push(Transform::Mirror(simple::parse_imir(b)?)),
            b"clap" => out.transforms.push(Transform::Crop(simple::parse_clap(b)?)),
            b"auxC" => out.auxc = Some(simple::parse_auxc(b)?),
            b"pasp" => out.pasp = Some(simple::parse_pasp(b)?),
            _ => {}
        }
    }
    Ok(out)
}
