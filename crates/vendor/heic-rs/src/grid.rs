//! The `grid` derived item: a picture stored as a mosaic of coded tiles.
//!
//! macOS begins tiling above 512 pixels on a side, so an ordinary iPhone
//! photograph is a grid, not a single coded picture. A 4032x3024 photograph is
//! 8 columns by 6 rows of 512x512 tiles; the last column and row overhang the
//! declared output size and are cropped away here.

mod mosaic;

use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::hevc::Frame;
use crate::image::check_pixels;
use crate::reader::Reader;
pub use mosaic::Mosaic;

/// The parsed payload of a `grid` item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    /// Tiles down.
    pub rows: u32,
    /// Tiles across.
    pub columns: u32,
    /// Width of the composed image after cropping.
    pub output_width: u32,
    /// Height of the composed image after cropping.
    pub output_height: u32,
}

impl Grid {
    /// How many tiles the `dimg` reference must list.
    pub const fn tile_count(&self) -> u64 {
        self.rows as u64 * self.columns as u64
    }
}

/// Parse a `grid` item payload.
///
/// The two dimensions are 16-bit unless bit 0 of `flags` is set, in which case
/// they are 32-bit.
pub fn parse(data: &[u8]) -> Result<Grid> {
    let mut r = Reader::new(data);
    let version = r.u8("grid version")?;
    if version != 0 {
        return Err(Error::Unsupported("grid version above 0"));
    }
    let flags = r.u8("grid flags")?;
    let rows = u32::from(r.u8("grid rows")?) + 1;
    let columns = u32::from(r.u8("grid columns")?) + 1;
    let (output_width, output_height) = if flags & 1 != 0 {
        (r.u32("grid output width")?, r.u32("grid output height")?)
    } else {
        (
            u32::from(r.u16("grid output width")?),
            u32::from(r.u16("grid output height")?),
        )
    };
    if output_width == 0 || output_height == 0 {
        return Err(Error::Malformed("grid declares a zero output dimension"));
    }
    Ok(Grid {
        rows,
        columns,
        output_width,
        output_height,
    })
}

/// Check that a `dimg` tile list matches what the grid asked for.
pub fn check_tiles(grid: &Grid, tiles: &[u32]) -> Result<()> {
    if tiles.len() as u64 != grid.tile_count() {
        return Err(Error::Malformed(
            "the grid's tile count disagrees with the length of its dimg reference",
        ));
    }
    Ok(())
}

/// Compose decoded tiles into one frame, cropped to the grid's output size.
///
/// Tiles are laid out row-major in the order the `dimg` reference listed them,
/// which is the order the caller must pass them in.
///
/// This is [`Mosaic`] run over every row into a fresh frame. The decoder's
/// own colour pass reads a mosaic's rows directly and never calls this; it is
/// here for the alpha plane, and for any caller that wants the planes.
pub fn compose(grid: &Grid, tiles: &[Frame], max_pixels: u64) -> Result<Frame> {
    let mosaic = Mosaic::new(grid, tiles)?;
    check_pixels(mosaic.width(), mosaic.height(), max_pixels)?;
    let (w, h) = (mosaic.width() as usize, mosaic.height() as usize);
    let (cw, ch) = mosaic.chroma_size();
    let (cw, ch) = (cw as usize, ch as usize);
    // `Mosaic::new` has checked every tile, so a row it cannot supply is a
    // contradiction; it is still reported rather than left as zeros.
    fn short() -> Error {
        Error::Malformed("grid tile is shorter than the row it must supply")
    }
    let mut y = alloc::vec![0u16; w * h];
    for (r, row) in y.chunks_exact_mut(w).enumerate() {
        mosaic.luma_row(r, row).ok_or_else(short)?;
    }
    let mut cb = alloc::vec![0u16; cw * ch];
    let mut cr = alloc::vec![0u16; cw * ch];
    if cw > 0 {
        for (r, row) in cb.chunks_exact_mut(cw).enumerate() {
            mosaic.chroma_row(false, r, row).ok_or_else(short)?;
        }
        for (r, row) in cr.chunks_exact_mut(cw).enumerate() {
            mosaic.chroma_row(true, r, row).ok_or_else(short)?;
        }
    }
    Ok(Frame {
        width: mosaic.width(),
        height: mosaic.height(),
        bit_depth: mosaic.bit_depth(),
        chroma: mosaic.chroma(),
        y,
        cb,
        cr,
        y_stride: mosaic.width(),
        c_stride: cw as u32,
    })
}

/// Collect the tile item ids a grid derives from, in order.
pub fn tile_ids(refs: &[crate::meta::iref::ItemReference], grid_item: u32) -> Vec<u32> {
    crate::meta::iref::targets(refs, grid_item, crate::meta::iref::RefKind::Dimg).to_vec()
}
