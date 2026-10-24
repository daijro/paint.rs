//! What a decoded picture looks like once it leaves the codec.
//!
//! These three types are the whole of the seam's vocabulary. Everything above
//! the codec — grid compositing, transforms, colour conversion — is written
//! against them and against nothing else in this module tree.

use alloc::vec::Vec;

use crate::error::{Error, Result};

/// How the chroma planes are sampled relative to luma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromaFormat {
    /// No chroma planes at all.
    Monochrome,
    /// Chroma at half resolution in both directions.
    #[default]
    Yuv420,
    /// Chroma at half resolution horizontally only.
    Yuv422,
    /// Chroma at full resolution.
    Yuv444,
}

impl ChromaFormat {
    /// Map an HEVC `chroma_format_idc` onto this enum.
    pub const fn from_idc(idc: u8) -> Option<ChromaFormat> {
        Some(match idc {
            0 => ChromaFormat::Monochrome,
            1 => ChromaFormat::Yuv420,
            2 => ChromaFormat::Yuv422,
            3 => ChromaFormat::Yuv444,
            _ => return None,
        })
    }

    /// How much to shift a luma x coordinate to reach the chroma plane.
    pub const fn x_shift(self) -> u32 {
        match self {
            ChromaFormat::Yuv420 | ChromaFormat::Yuv422 => 1,
            _ => 0,
        }
    }

    /// How much to shift a luma y coordinate to reach the chroma plane.
    pub const fn y_shift(self) -> u32 {
        match self {
            ChromaFormat::Yuv420 => 1,
            _ => 0,
        }
    }

    /// The size of a chroma plane for a picture of the given luma size.
    pub const fn chroma_size(self, width: u32, height: u32) -> (u32, u32) {
        if matches!(self, ChromaFormat::Monochrome) {
            return (0, 0);
        }
        (
            width.div_ceil(1 << self.x_shift()),
            height.div_ceil(1 << self.y_shift()),
        )
    }
}

/// One decoded picture, in planar YCbCr, one sample per `u16` regardless of
/// bit depth so that 8-bit and 10-bit content share a representation.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Luma width in samples.
    pub width: u32,
    /// Luma height in samples.
    pub height: u32,
    /// Bits actually used in each sample, 8 through 16.
    pub bit_depth: u8,
    /// Chroma sampling.
    pub chroma: ChromaFormat,
    /// Luma plane, `y_stride` samples per row.
    pub y: Vec<u16>,
    /// Cb plane, `c_stride` samples per row. Empty when monochrome.
    pub cb: Vec<u16>,
    /// Cr plane, `c_stride` samples per row. Empty when monochrome.
    pub cr: Vec<u16>,
    /// Samples per row of the luma plane, at least `width`.
    pub y_stride: u32,
    /// Samples per row of each chroma plane.
    pub c_stride: u32,
}

impl Frame {
    /// Check that the planes are as large as the declared geometry needs.
    ///
    /// The compositor calls this on everything it is handed, so that a buggy
    /// or malicious decoder cannot make the rest of the crate index past the
    /// end of a plane.
    pub fn validate(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(Error::Malformed("decoded frame has a zero dimension"));
        }
        if self.y_stride < self.width {
            return Err(Error::Malformed("luma stride is narrower than the frame"));
        }
        let need = (self.y_stride as usize).saturating_mul(self.height as usize);
        if self.y.len() < need {
            return Err(Error::Malformed("luma plane is shorter than its geometry"));
        }
        if self.chroma == ChromaFormat::Monochrome {
            return Ok(());
        }
        let (cw, ch) = self.chroma.chroma_size(self.width, self.height);
        if self.c_stride < cw {
            return Err(Error::Malformed("chroma stride is narrower than the frame"));
        }
        let need = (self.c_stride as usize).saturating_mul(ch as usize);
        if self.cb.len() < need || self.cr.len() < need {
            return Err(Error::Malformed(
                "chroma plane is shorter than its geometry",
            ));
        }
        Ok(())
    }
}

/// What a parameter set says about the pictures that follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    /// Luma width in samples.
    pub width: u32,
    /// Luma height in samples.
    pub height: u32,
    /// Luma bit depth.
    pub bit_depth: u8,
    /// Chroma sampling.
    pub chroma: ChromaFormat,
    /// The VUI's colour signalling (paint.rs patch, Ruling R52).
    pub video_signal: Option<crate::hevc::ps::vui::VideoSignal>,
}
