//! Where a conversion reads its rows from: one decoded frame, or a grid's
//! tiles.
//!
//! The kernel asks for "luma row `y`" and "chroma rows `r0` and `r1`" and
//! nothing else, so a grid never has to be composed onto a canvas for the
//! colour pass: its rows are assembled into a band's scratch buffer as they
//! are needed, from the tiles that hold them. A frame answers the same
//! questions with slices of its planes and uses no scratch at all.

use crate::grid::Mosaic;
use crate::hevc::{ChromaFormat, Frame};

/// The planes a conversion reads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Source<'a> {
    /// One decoded picture.
    Frame(&'a Frame),
    /// The tiles of a grid, read in place.
    Mosaic(&'a Mosaic<'a>),
}

impl Source<'_> {
    /// Width in luma samples.
    pub fn width(&self) -> u32 {
        match self {
            Source::Frame(f) => f.width,
            Source::Mosaic(m) => m.width(),
        }
    }

    /// Height in luma samples.
    pub fn height(&self) -> u32 {
        match self {
            Source::Frame(f) => f.height,
            Source::Mosaic(m) => m.height(),
        }
    }

    /// Chroma sampling.
    pub fn chroma(&self) -> ChromaFormat {
        match self {
            Source::Frame(f) => f.chroma,
            Source::Mosaic(m) => m.chroma(),
        }
    }

    /// Sample bit depth.
    pub fn bit_depth(&self) -> u8 {
        match self {
            Source::Frame(f) => f.bit_depth,
            Source::Mosaic(m) => m.bit_depth(),
        }
    }

    /// Width of one chroma row.
    fn chroma_width(&self) -> usize {
        self.chroma().chroma_size(self.width(), self.height()).0 as usize
    }

    /// Scratch samples a band needs to assemble rows: none for a frame; one
    /// luma row and one chroma row for a mosaic.
    pub fn stitch_len(&self) -> usize {
        match self {
            Source::Frame(_) => 0,
            Source::Mosaic(_) => self.width() as usize + self.chroma_width(),
        }
    }

    /// Luma row `y`, assembled into `buf` when the source is a mosaic.
    pub fn luma<'b>(&'b self, y: usize, buf: &'b mut [u16]) -> Option<&'b [u16]> {
        match self {
            Source::Frame(f) => {
                let (w, stride) = (f.width as usize, f.y_stride as usize);
                let start = y.checked_mul(stride)?;
                f.y.get(start..start + w)
            }
            Source::Mosaic(m) => {
                let row = buf.get_mut(..m.width() as usize)?;
                m.luma_row(y, row)?;
                Some(row)
            }
        }
    }

    /// Row `r` of the Cb plane (`cr` false) or the Cr plane (`cr` true),
    /// assembled into `buf` when the source is a mosaic.
    pub fn chroma_row<'b>(&'b self, cr: bool, r: usize, buf: &'b mut [u16]) -> Option<&'b [u16]> {
        let cw = self.chroma_width();
        match self {
            Source::Frame(f) => {
                let start = r.checked_mul(f.c_stride as usize)?;
                let plane = if cr { &f.cr } else { &f.cb };
                plane.get(start..start + cw)
            }
            Source::Mosaic(m) => {
                let row = buf.get_mut(..cw)?;
                m.chroma_row(cr, r, row)?;
                Some(row)
            }
        }
    }
}
