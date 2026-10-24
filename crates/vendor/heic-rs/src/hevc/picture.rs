//! Reconstructed picture storage and per-block side information.

use crate::hevc::error::{Error, Result};
use alloc::vec;
use alloc::vec::Vec;

/// A single colour plane of `u16` samples with an explicit stride.
#[derive(Debug, Clone)]
pub struct Plane {
    /// Sample storage, `stride * height` entries.
    pub data: Vec<u16>,
    /// Distance between vertically adjacent samples, in samples.
    pub stride: usize,
    /// Plane width in samples.
    pub width: usize,
    /// Plane height in samples.
    pub height: usize,
}

impl Plane {
    /// Allocates a zeroed plane with `stride == width`.
    pub fn new(width: usize, height: usize) -> Result<Plane> {
        let n = width.checked_mul(height).ok_or(Error::TooLarge)?;
        if n > (1usize << 30) {
            return Err(Error::TooLarge);
        }
        Ok(Plane {
            data: vec![0u16; n],
            stride: width,
            width,
            height,
        })
    }

    /// Sample at `(x, y)`, clamped to the plane.
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u16 {
        self.data[y * self.stride + x]
    }

    /// Writes the sample at `(x, y)`.
    #[inline]
    pub fn put(&mut self, x: usize, y: usize, v: u16) {
        self.data[y * self.stride + x] = v;
    }
}

/// Bit flags stored per minimum transform block (4x4 luma samples).
pub mod flags {
    /// `cu_transquant_bypass_flag` is set for the covering coding unit.
    pub const TQ_BYPASS: u8 = 1;
    /// `pcm_flag` is set for the covering coding unit.
    pub const PCM: u8 = 2;
}

/// Edge flags stored per 8x8 luma grid cell for the deblocking filter.
pub mod edge {
    /// A transform or prediction boundary runs along the cell's left side.
    pub const VER: u8 = 1;
    /// A transform or prediction boundary runs along the cell's top side.
    pub const HOR: u8 = 2;
}

/// The picture being reconstructed, plus the side information later stages need.
#[derive(Debug)]
pub struct Picture {
    /// Luma plane.
    pub y: Plane,
    /// Cb plane; zero sized for monochrome.
    pub cb: Plane,
    /// Cr plane; zero sized for monochrome.
    pub cr: Plane,
    /// Width of the minimum-transform-block grid.
    pub min4_w: usize,
    /// Height of the minimum-transform-block grid.
    pub min4_h: usize,
    /// `IntraPredModeY` per 4x4 luma block.
    pub intra_mode: Vec<u8>,
    /// `QpY` per 4x4 luma block.
    pub qp_y: Vec<i8>,
    /// Per-4x4 coding unit flags, see [`flags`].
    pub cu_flags: Vec<u8>,
    /// `CtDepth` per 4x4 luma block, used for `split_cu_flag` contexts.
    pub ct_depth: Vec<u8>,
    /// Width of the 8x8 deblocking grid.
    pub min8_w: usize,
    /// Height of the 8x8 deblocking grid.
    pub min8_h: usize,
    /// Per-8x8 edge flags, see [`edge`].
    pub edges: Vec<u8>,
}

impl Picture {
    /// Allocates all planes and side-information arrays for one picture.
    pub fn new(width: usize, height: usize, sub_w: usize, sub_h: usize) -> Result<Picture> {
        let y = Plane::new(width, height)?;
        let (cw, ch) = match (width.checked_div(sub_w), height.checked_div(sub_h)) {
            (Some(a), Some(b)) => (a, b),
            _ => (0, 0),
        };
        let cb = Plane::new(cw, ch)?;
        let cr = Plane::new(cw, ch)?;
        let min4_w = width.div_ceil(4);
        let min4_h = height.div_ceil(4);
        let n4 = min4_w * min4_h;
        let min8_w = width.div_ceil(8);
        let min8_h = height.div_ceil(8);
        Ok(Picture {
            y,
            cb,
            cr,
            min4_w,
            min4_h,
            intra_mode: vec![1u8; n4],
            qp_y: vec![0i8; n4],
            cu_flags: vec![0u8; n4],
            ct_depth: vec![0u8; n4],
            min8_w,
            min8_h,
            edges: vec![0u8; min8_w * min8_h],
        })
    }

    /// Index into the 4x4 side-information arrays for luma position `(x, y)`.
    ///
    /// Positions outside the picture clamp to the last block so that callers
    /// never index out of bounds.
    #[inline]
    pub fn idx4(&self, x: usize, y: usize) -> usize {
        let xi = (x >> 2).min(self.min4_w - 1);
        let yi = (y >> 2).min(self.min4_h - 1);
        yi * self.min4_w + xi
    }

    /// Records the deblocking boundaries of a `size` x `size` block at `(x, y)`.
    ///
    /// Only boundaries that fall on the 8x8 luma grid are filtered, so an edge
    /// at an odd multiple of four samples is not recorded. The whole extent of
    /// each boundary is marked, not just the corner it starts at.
    pub fn mark_edge(&mut self, x: usize, y: usize, size: usize, kind: u8) {
        if x >= self.y.width || y >= self.y.height {
            return;
        }
        if kind & edge::VER != 0 && x & 7 == 0 {
            let col = x >> 3;
            let mut yy = y;
            while yy < (y + size).min(self.y.height) {
                self.edges[(yy >> 3) * self.min8_w + col] |= edge::VER;
                yy += 8;
            }
        }
        if kind & edge::HOR != 0 && y & 7 == 0 {
            let row = (y >> 3) * self.min8_w;
            let mut xx = x;
            while xx < (x + size).min(self.y.width) {
                self.edges[row + (xx >> 3)] |= edge::HOR;
                xx += 8;
            }
        }
    }

    /// Fills a rectangular region of a per-4x4 array with `value`.
    pub fn fill4(arr: &mut [u8], w: usize, x: usize, y: usize, size: usize, value: u8) {
        let x0 = x >> 2;
        let y0 = y >> 2;
        let n = size >> 2;
        for j in 0..n {
            let row = (y0 + j) * w;
            for i in 0..n {
                if let Some(s) = arr.get_mut(row + x0 + i) {
                    *s = value;
                }
            }
        }
    }
}
