//! A grid's decoded tiles read as one picture, a row at a time.
//!
//! Composing tiles onto a canvas costs a canvas: for a 2048x1536 photograph
//! that is 14 MB allocated, zeroed, page-faulted and copied into, only for
//! the colour pass to read it straight back out. [`Mosaic`] does the checks
//! once and then answers "row `y` of the picture" by copying that row's
//! segments out of the tiles that hold them into a buffer the caller is about
//! to read anyway. [`crate::grid::compose`] is exactly that, run over every
//! row into a fresh frame, so the two cannot disagree about a sample.

use crate::error::{Error, Result};
use crate::grid::Grid;
use crate::hevc::{ChromaFormat, Frame};

/// Decoded tiles, checked once, readable as rows of the composed picture.
#[derive(Debug, Clone, Copy)]
pub struct Mosaic<'a> {
    grid: Grid,
    tiles: &'a [Frame],
    tile_w: u32,
    tile_h: u32,
    chroma: ChromaFormat,
    bit_depth: u8,
}

impl<'a> Mosaic<'a> {
    /// Check that `tiles`, in `dimg` order, can form `grid`.
    ///
    /// Every tile must be valid, the same size and the same pixel format;
    /// together they must cover the declared output; and, unless the picture
    /// is monochrome, every tile must start on a chroma sample, or the mosaic
    /// could not be reassembled without resampling.
    pub fn new(grid: &Grid, tiles: &'a [Frame]) -> Result<Mosaic<'a>> {
        if tiles.len() as u64 != grid.tile_count() {
            return Err(Error::Malformed(
                "the grid's tile count disagrees with the number of decoded tiles",
            ));
        }
        let first = tiles.first().ok_or(Error::Malformed("grid has no tiles"))?;
        let (tile_w, tile_h) = (first.width, first.height);
        let (chroma, bit_depth) = (first.chroma, first.bit_depth);
        for t in tiles {
            t.validate()?;
            if t.width != tile_w || t.height != tile_h {
                return Err(Error::Malformed("grid tiles are not all the same size"));
            }
            if t.chroma != chroma || t.bit_depth != bit_depth {
                return Err(Error::Malformed("grid tiles do not share a pixel format"));
            }
        }
        // The mosaic must be at least as large as the crop it promises.
        let canvas_w = u64::from(tile_w) * u64::from(grid.columns);
        let canvas_h = u64::from(tile_h) * u64::from(grid.rows);
        if canvas_w < u64::from(grid.output_width) || canvas_h < u64::from(grid.output_height) {
            return Err(Error::Malformed(
                "grid tiles do not cover the declared output size",
            ));
        }
        let (xs, ys) = (chroma.x_shift(), chroma.y_shift());
        let x_sited = grid.columns == 1 || (tile_w >> xs) << xs == tile_w;
        let y_sited = grid.rows == 1 || (tile_h >> ys) << ys == tile_h;
        if chroma != ChromaFormat::Monochrome && !(x_sited && y_sited) {
            return Err(Error::Unsupported(
                "grid tile origin is not on a chroma sample",
            ));
        }
        Ok(Mosaic {
            grid: *grid,
            tiles,
            tile_w,
            tile_h,
            chroma,
            bit_depth,
        })
    }

    /// Width of the composed picture.
    pub const fn width(&self) -> u32 {
        self.grid.output_width
    }

    /// Height of the composed picture.
    pub const fn height(&self) -> u32 {
        self.grid.output_height
    }

    /// Chroma sampling, shared by every tile.
    pub const fn chroma(&self) -> ChromaFormat {
        self.chroma
    }

    /// Bit depth, shared by every tile.
    pub const fn bit_depth(&self) -> u8 {
        self.bit_depth
    }

    /// Size of each chroma plane of the composed picture.
    pub const fn chroma_size(&self) -> (u32, u32) {
        self.chroma.chroma_size(self.width(), self.height())
    }

    /// Copy luma row `y` into `dst`, which is [`width`](Mosaic::width) long.
    ///
    /// `None` means the row does not exist or a tile is shorter than it
    /// claims, neither of which [`Mosaic::new`] lets through.
    pub fn luma_row(&self, y: usize, dst: &mut [u16]) -> Option<()> {
        let (tw, th) = (self.tile_w as usize, self.tile_h as usize);
        let rows = self.height() as usize;
        self.stitch(y, rows, tw, th, dst, |t| (&t.y, t.y_stride as usize))
    }

    /// Copy row `r` of the Cb plane (`cr` false) or the Cr plane (`cr` true)
    /// into `dst`, which is the chroma width long.
    pub fn chroma_row(&self, cr: bool, r: usize, dst: &mut [u16]) -> Option<()> {
        let (tcw, tch) = self.chroma.chroma_size(self.tile_w, self.tile_h);
        let rows = self.chroma_size().1 as usize;
        self.stitch(r, rows, tcw as usize, tch as usize, dst, |t| {
            (if cr { &t.cr } else { &t.cb }, t.c_stride as usize)
        })
    }

    /// Assemble row `y` of a plane `rows` tall from the tiles across it,
    /// clipping the last tile at the picture's edge.
    fn stitch(
        &self,
        y: usize,
        rows: usize,
        tile_w: usize,
        tile_h: usize,
        dst: &mut [u16],
        plane: impl Fn(&Frame) -> (&[u16], usize),
    ) -> Option<()> {
        if y >= rows || tile_w == 0 || tile_h == 0 {
            return None;
        }
        let (row, local) = (y / tile_h, y % tile_h);
        let columns = self.grid.columns as usize;
        let first = row.checked_mul(columns)?;
        let mut x = 0;
        for col in 0..columns {
            let rest = dst.get_mut(x..)?;
            if rest.is_empty() {
                break;
            }
            let n = core::cmp::min(tile_w, rest.len());
            let (samples, stride) = plane(self.tiles.get(first + col)?);
            let s = local.checked_mul(stride)?;
            rest.get_mut(..n)?.copy_from_slice(samples.get(s..s + n)?);
            x += n;
        }
        Some(())
    }
}
