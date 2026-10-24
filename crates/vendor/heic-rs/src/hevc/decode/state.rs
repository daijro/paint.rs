//! Per-slice decoding state shared by the coding-tree, transform and
//! residual parsers.

use super::geom::Geometry;
use crate::hevc::cabac::Cabac;
use crate::hevc::filter::{FilterInfo, chroma_qp_from_luma};
use crate::hevc::picture::Picture;
use crate::hevc::ps::scaling::ScalingFactors;
use crate::hevc::ps::{Pps, Sps};
use crate::hevc::slice::SliceHeader;

/// Mutable decoding state for one slice segment.
pub struct Dec<'a> {
    /// Active sequence parameter set.
    pub sps: &'a Sps,
    /// Active picture parameter set.
    pub pps: &'a Pps,
    /// Tile and scan addressing tables.
    pub geo: &'a Geometry,
    /// Derived scaling factors, or `None` when scaling lists are disabled.
    pub sf: Option<&'a ScalingFactors>,
    /// The picture being reconstructed.
    pub pic: &'a mut Picture,
    /// Loop filter parameters being accumulated per CTB.
    pub fi: &'a mut FilterInfo,
    /// Header of the slice segment being decoded.
    pub sh: SliceHeader,
    /// The arithmetic decoder.
    pub cab: Cabac<'a>,
    /// `ChromaArrayType`.
    pub cat: u8,
    /// `QpBdOffsetY`.
    pub qp_bd_y: i32,
    /// `QpBdOffsetC`.
    pub qp_bd_c: i32,
    /// `Log2MinCuQpDeltaSize`.
    pub log2_qg: usize,
    /// `Log2MinCuChromaQpOffsetSize`.
    pub log2_cqg: usize,
    /// Raster address of the coding tree block being decoded.
    pub ctb_rs: usize,
    /// `qPY_PRED` for the current quantization group.
    pub qp_pred: i32,
    /// `QpY` of the coding unit most recently completed.
    pub qp_last: i32,
    /// `QpY` currently in force.
    pub qp_y: i32,
    /// True once `cu_qp_delta_abs` has been parsed in this quantization group.
    pub qp_delta_coded: bool,
    /// True once `cu_chroma_qp_offset_flag` has been parsed in this group.
    pub cqo_coded: bool,
    /// `CuQpOffsetCb`.
    pub cqo_cb: i32,
    /// `CuQpOffsetCr`.
    pub cqo_cr: i32,
    /// `cu_transquant_bypass_flag` of the current coding unit.
    pub tq_bypass: bool,
    /// `IntraSplitFlag` of the current coding unit.
    pub intra_split: bool,
    /// `IntraPredModeY` of the current coding unit's four prediction blocks.
    pub mode_y: [u8; 4],
    /// `IntraPredModeC` of the current coding unit's four prediction blocks.
    pub mode_c: [u8; 4],
    /// `cbf_cb[trafoDepth][tIdx]`.
    pub cbf_cb: [[bool; 2]; 6],
    /// `cbf_cr[trafoDepth][tIdx]`.
    pub cbf_cr: [[bool; 2]; 6],
    /// Coefficient scratch buffer for one transform block.
    pub coeffs: [i32; 1024],
    /// Prediction scratch buffer for one transform block.
    pub pred: [u16; 1024],
    /// `coded_sub_block_flag` scratch for one transform block.
    pub csbf: [u8; 64],
    /// True until the first quantization group after an entropy reset point.
    pub first_qg: bool,
    /// `StatCoeff[]` for persistent Rice parameter adaptation.
    pub stat_coeff: [i32; 4],
}

impl<'a> Dec<'a> {
    /// True when the block covering `(xn, yn)` is available for prediction
    /// from the block at `(xc, yc)` (clause 6.4.1).
    pub fn available(&self, xc: usize, yc: usize, xn: isize, yn: isize) -> bool {
        if xn < 0 || yn < 0 {
            return false;
        }
        let (xn, yn) = (xn as usize, yn as usize);
        if xn >= self.sps.width || yn >= self.sps.height {
            return false;
        }
        if self.geo.z(xn, yn) > self.geo.z(xc, yc) {
            return false;
        }
        let a = self.geo.ctb_rs(xn, yn);
        let b = self.geo.ctb_rs(xc, yc);
        self.geo.tile_id[a] == self.geo.tile_id[b] && self.fi.slice_addr[a] == self.fi.slice_addr[b]
    }

    /// `IntraPredModeY` recorded for luma position `(x, y)`.
    #[inline]
    pub fn mode_at(&self, x: usize, y: usize) -> u8 {
        self.pic.intra_mode[self.pic.idx4(x, y)]
    }

    /// Starts a new quantization group at `(x, y)` (clause 8.6.1).
    pub fn start_qg(&mut self, x: usize, y: usize, first_in_group: bool) {
        self.qp_delta_coded = false;
        let prev = if first_in_group {
            self.sh.qp
        } else {
            self.qp_last
        };
        let a = if self.available(x, y, x as isize - 1, y as isize)
            && self.geo.ctb_rs(x - 1, y) == self.geo.ctb_rs(x, y)
        {
            self.pic.qp_y[self.pic.idx4(x - 1, y)] as i32
        } else {
            prev
        };
        let b = if self.available(x, y, x as isize, y as isize - 1)
            && self.geo.ctb_rs(x, y - 1) == self.geo.ctb_rs(x, y)
        {
            self.pic.qp_y[self.pic.idx4(x, y - 1)] as i32
        } else {
            prev
        };
        self.qp_pred = (a + b + 1) >> 1;
        self.set_qp_delta(0);
    }

    /// Applies `CuQpDeltaVal` to the predicted QP (clause 8.6.1).
    pub fn set_qp_delta(&mut self, delta: i32) {
        let m = 52 + self.qp_bd_y;
        self.qp_y = (self.qp_pred + delta + 52 + 2 * self.qp_bd_y).rem_euclid(m) - self.qp_bd_y;
    }

    /// `Qp'Y`.
    #[inline]
    pub fn qp_luma(&self) -> i32 {
        self.qp_y + self.qp_bd_y
    }

    /// `Qp'Cb` or `Qp'Cr` for `c_idx` of 1 or 2 (clause 8.6.1).
    pub fn qp_chroma(&self, c_idx: usize) -> i32 {
        let (pps_off, slice_off, cu_off) = if c_idx == 1 {
            (self.pps.cb_qp_offset, self.sh.cb_qp_offset, self.cqo_cb)
        } else {
            (self.pps.cr_qp_offset, self.sh.cr_qp_offset, self.cqo_cr)
        };
        let qpi = (self.qp_y + pps_off + slice_off + cu_off).clamp(-self.qp_bd_c, 57);
        chroma_qp_from_luma(qpi, self.cat) + self.qp_bd_c
    }

    /// Records the final `QpY` of a coding unit across its 4x4 blocks.
    pub fn store_qp(&mut self, x: usize, y: usize, size: usize) {
        let v = self.qp_y.clamp(-128, 127) as i8;
        let (x0, y0, n, w) = (x >> 2, y >> 2, size >> 2, self.pic.min4_w);
        for j in 0..n {
            for i in 0..n {
                if let Some(s) = self.pic.qp_y.get_mut((y0 + j) * w + x0 + i) {
                    *s = v;
                }
            }
        }
        self.qp_last = self.qp_y;
    }

    /// Scaling factor block for a transform, or `None` for a flat factor of 16.
    pub fn factors(
        &self,
        log2_size: usize,
        c_idx: usize,
        transform_skip: bool,
    ) -> Option<&'a [u8]> {
        let sf = self.sf?;
        if transform_skip && log2_size > 2 {
            return None;
        }
        // matrixId: intra prediction uses 0..2, one per colour component.
        Some(sf.get(log2_size, c_idx))
    }
}
