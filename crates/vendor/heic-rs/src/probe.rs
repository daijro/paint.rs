//! [`probe`]: everything the file says about itself, without decoding it.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::ftyp::Brand;
use crate::hevc::ChromaFormat;
use crate::props::ItemProps;
use crate::props::simple::{Mirror, Rotation};
use crate::transform;

/// How a grid-derived image is tiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridInfo {
    /// Tiles down.
    pub rows: u32,
    /// Tiles across.
    pub columns: u32,
    /// Coded width of one tile, from the first tile's `ispe`.
    pub tile_width: u32,
    /// Coded height of one tile.
    pub tile_height: u32,
}

/// What a file says about its primary image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageInfo {
    /// Width the caller will receive, after `clap`, `irot` and `imir`.
    pub width: u32,
    /// Height the caller will receive.
    pub height: u32,
    /// Width as coded, before any transform.
    pub coded_width: u32,
    /// Height as coded.
    pub coded_height: u32,
    /// Luma bit depth.
    pub bit_depth: u8,
    /// Chroma sampling, read from the `hvcC` configuration record.
    pub chroma: ChromaFormat,
    /// Rotation the file asks for.
    pub rotation: Rotation,
    /// Mirroring the file asks for.
    pub mirror: Option<Mirror>,
    /// True when an `auxl`-linked alpha auxiliary item is present.
    pub has_alpha: bool,
    /// True when the primary item is a `grid` derivation.
    pub is_grid: bool,
    /// The tiling, when the primary item is a grid.
    pub grid: Option<GridInfo>,
    /// True when an `Exif` item describes the primary image.
    pub has_exif: bool,
    /// True when `colr` carried an ICC profile.
    pub has_icc: bool,
    /// True when an XMP packet describes the primary image.
    pub has_xmp: bool,
    /// The brand the file was read as.
    pub brand: Brand,
    /// The primary item's id.
    pub primary_item: u32,
}

/// Read a file's metadata without decoding any pixels.
///
/// This does not go through the codec seam, so it works today even though the
/// HEVC decoder has not landed.
pub fn probe(bytes: &[u8]) -> Result<ImageInfo> {
    let ctx = Context::open(bytes)?;
    let id = ctx.meta.primary;
    let p = ctx.props(id)?;
    let derivation = ctx.grid(id)?;

    // A grid carries its own size and colour, but the codec configuration
    // lives on its tiles, so that is where the chroma format comes from.
    let (grid_info, codec) = match &derivation {
        Some((g, tiles)) => {
            let first = *tiles
                .first()
                .ok_or(Error::Malformed("grid lists no tiles"))?;
            let tp = ctx.props(first)?;
            let ispe = tp.ispe.ok_or(Error::MissingBox("ispe on a grid tile"))?;
            let info = GridInfo {
                rows: g.rows,
                columns: g.columns,
                tile_width: ispe.width,
                tile_height: ispe.height,
            };
            (Some(info), tp)
        }
        None => (None, p.clone()),
    };

    let (coded_width, coded_height) = ctx.coded_size(id, &p)?;
    let (width, height) = transform::transformed_size(coded_width, coded_height, &p.transforms)?;
    let chroma = chroma_of(&codec)?;
    let bit_depth = p
        .bit_depth()
        .or_else(|| codec.bit_depth())
        .ok_or(Error::MissingBox("pixi or hvcC"))?;

    Ok(ImageInfo {
        width,
        height,
        coded_width,
        coded_height,
        bit_depth,
        chroma,
        rotation: p.rotation(),
        mirror: p.mirror(),
        has_alpha: ctx.alpha_item(id)?.is_some(),
        is_grid: derivation.is_some(),
        grid: grid_info,
        has_exif: ctx.exif_item(id).is_some(),
        has_icc: p.icc.is_some(),
        has_xmp: ctx.xmp_item(id).is_some(),
        brand: ctx.brand(),
        primary_item: id,
    })
}

/// The chroma format from an item's `hvcC`, or an error naming what is missing.
fn chroma_of(p: &ItemProps<'_>) -> Result<ChromaFormat> {
    let hvcc = p.hvcc.as_ref().ok_or(Error::MissingBox("hvcC"))?;
    ChromaFormat::from_idc(hvcc.chroma_format)
        .ok_or(Error::Malformed("hvcC declares an unknown chroma format"))
}
