//! In-loop filters: deblocking (clause 8.7.2) and SAO (clause 8.7.3).

mod deblock;
mod sao;

pub use deblock::deblock;
pub use sao::sao;

use alloc::vec::Vec;

/// Sample adaptive offset parameters for one component of one CTB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaoCtb {
    /// `SaoTypeIdx`: 0 = not applied, 1 = band offset, 2 = edge offset.
    pub type_idx: u8,
    /// `SaoEoClass`, 0..=3; meaningful when `type_idx == 2`.
    pub eo_class: u8,
    /// `sao_band_position`; meaningful when `type_idx == 1`.
    pub band_position: u8,
    /// `SaoOffsetVal[1..=4]`, already signed and scaled.
    pub offsets: [i16; 4],
}

/// Loop-filter parameters that vary per coding tree block.
#[derive(Clone, Copy, Debug)]
pub struct CtbFilter {
    /// `slice_deblocking_filter_disabled_flag` for the covering slice.
    pub deblock_disabled: bool,
    /// `slice_beta_offset_div2`.
    pub beta_offset_div2: i8,
    /// `slice_tc_offset_div2`.
    pub tc_offset_div2: i8,
    /// `slice_loop_filter_across_slices_enabled_flag`.
    pub across_slices: bool,
    /// `pps_cb_qp_offset + slice_cb_qp_offset`.
    pub cb_qp_offset: i16,
    /// `pps_cr_qp_offset + slice_cr_qp_offset`.
    pub cr_qp_offset: i16,
    /// SAO parameters for Y, Cb and Cr.
    pub sao: [SaoCtb; 3],
}

impl Default for CtbFilter {
    fn default() -> Self {
        CtbFilter {
            deblock_disabled: true,
            beta_offset_div2: 0,
            tc_offset_div2: 0,
            across_slices: true,
            cb_qp_offset: 0,
            cr_qp_offset: 0,
            sao: [SaoCtb::default(); 3],
        }
    }
}

/// Everything the in-loop filters need that is not already in the picture.
#[derive(Debug)]
pub struct FilterInfo {
    /// `BitDepthY`.
    pub bit_depth_y: u8,
    /// `BitDepthC`.
    pub bit_depth_c: u8,
    /// `ChromaArrayType`, 0..=3.
    pub chroma_array_type: u8,
    /// `SubWidthC`, or 0 for monochrome.
    pub sub_w: usize,
    /// `SubHeightC`, or 0 for monochrome.
    pub sub_h: usize,
    /// `CtbLog2SizeY`.
    pub ctb_log2: usize,
    /// `PicWidthInCtbsY`.
    pub pic_w_ctbs: usize,
    /// `PicHeightInCtbsY`.
    pub pic_h_ctbs: usize,
    /// `pcm_loop_filter_disabled_flag`.
    pub pcm_loop_filter_disabled: bool,
    /// `loop_filter_across_tiles_enabled_flag`.
    pub across_tiles: bool,
    /// True when SAO is enabled for at least one component of the picture.
    pub sao_enabled: bool,
    /// Per-CTB parameters, indexed by CTB raster address.
    pub ctb: Vec<CtbFilter>,
    /// `TileId` per CTB raster address.
    pub tile_id: Vec<u16>,
    /// Address of the first CTB of the containing slice, per CTB raster address.
    pub slice_addr: Vec<u32>,
}

impl FilterInfo {
    /// True when a filter may cross from CTB `a` into CTB `b` (raster addresses).
    ///
    /// `b` is the CTB containing the samples being modified.
    #[inline]
    pub fn may_cross(&self, a: usize, b: usize) -> bool {
        if a == b {
            return true;
        }
        if self.tile_id[a] != self.tile_id[b] && !self.across_tiles {
            return false;
        }
        if self.slice_addr[a] != self.slice_addr[b] && !self.ctb[b].across_slices {
            return false;
        }
        true
    }

    /// CTB raster address covering luma position `(x, y)`.
    #[inline]
    pub fn ctb_of(&self, x: usize, y: usize) -> usize {
        (y >> self.ctb_log2) * self.pic_w_ctbs + (x >> self.ctb_log2)
    }
}

/// Maps a luma QP to a chroma QP, Table 8-10, for `ChromaArrayType == 1`.
#[inline]
pub fn chroma_qp_from_luma(qpi: i32, chroma_array_type: u8) -> i32 {
    if chroma_array_type != 1 {
        return qpi.min(51);
    }
    if qpi < 30 {
        qpi
    } else if qpi > 43 {
        qpi - 6
    } else {
        const T: [i32; 14] = [29, 30, 31, 32, 33, 33, 34, 34, 35, 35, 36, 36, 37, 37];
        T[(qpi - 30) as usize]
    }
}
