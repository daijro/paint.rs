//! Slice segment header (clause 7.3.6.1).

use crate::hevc::bits::BitReader;
use crate::hevc::error::{Error, Result};
use crate::hevc::nal::{NalHeader, unit_type};
use crate::hevc::ps::{Pps, Sps, rps};
use alloc::vec::Vec;

/// The parsed slice segment header of an intra picture.
#[derive(Debug, Clone, Default)]
pub struct SliceHeader {
    /// `first_slice_segment_in_pic_flag`.
    pub first_in_pic: bool,
    /// `slice_pic_parameter_set_id`.
    pub pps_id: u32,
    /// `dependent_slice_segment_flag`.
    pub dependent: bool,
    /// `slice_segment_address`, a CTB raster address.
    pub segment_address: u32,
    /// Address of the first CTB of the containing slice (not segment).
    pub slice_address: u32,
    /// `slice_sao_luma_flag`.
    pub sao_luma: bool,
    /// `slice_sao_chroma_flag`.
    pub sao_chroma: bool,
    /// `SliceQpY`.
    pub qp: i32,
    /// `slice_cb_qp_offset`.
    pub cb_qp_offset: i32,
    /// `slice_cr_qp_offset`.
    pub cr_qp_offset: i32,
    /// `cu_chroma_qp_offset_enabled_flag`.
    pub cu_chroma_qp_offset_enabled: bool,
    /// `slice_deblocking_filter_disabled_flag`.
    pub deblock_disabled: bool,
    /// `slice_beta_offset_div2`.
    pub beta_offset_div2: i32,
    /// `slice_tc_offset_div2`.
    pub tc_offset_div2: i32,
    /// `slice_loop_filter_across_slices_enabled_flag`.
    pub across_slices: bool,
    /// `entry_point_offset_minus1[i] + 1`, in NAL unit bytes.
    pub entry_points: Vec<u32>,
    /// Byte offset of the slice segment data within the RBSP.
    pub data_offset: usize,
}

/// Parses `slice_segment_header()` from `data`, which is an SPS-scoped RBSP.
///
/// `prev` supplies the inherited fields of a dependent slice segment.
pub fn parse(
    nh: &NalHeader,
    data: &[u8],
    sps: &Sps,
    pps: &Pps,
    _prev: Option<&SliceHeader>,
) -> Result<SliceHeader> {
    let r = &mut BitReader::new(data);
    let mut h = SliceHeader {
        across_slices: pps.across_slices,
        ..Default::default()
    };
    h.first_in_pic = r.u1()? != 0;
    if nh.is_irap() {
        r.u1()?; // no_output_of_prior_pics_flag
    }
    h.pps_id = r.ue()?;
    if !h.first_in_pic {
        if pps.dependent_slices {
            h.dependent = r.u1()? != 0;
        }
        let bits = ceil_log2(sps.size_in_ctbs());
        h.segment_address = r.u(bits)?;
        if h.segment_address as usize >= sps.size_in_ctbs() {
            return Err(Error::InvalidData("slice_segment_address out of range"));
        }
    }
    if h.dependent {
        // Clause 9.3.1 restores the entropy coder state saved at the end of the
        // preceding slice segment; this decoder does not carry that state
        // across segments, so a dependent segment is rejected rather than
        // decoded incorrectly.
        return Err(Error::Unsupported("dependent slice segments"));
    }
    h.slice_address = h.segment_address;
    parse_independent(r, nh, sps, pps, &mut h)?;
    if pps.tiles_enabled || pps.wpp_enabled {
        let n = r.ue()? as usize;
        if n > sps.size_in_ctbs() + 1 {
            return Err(Error::InvalidData("num_entry_point_offsets too large"));
        }
        if n > 0 {
            let len = r.ue()? + 1;
            if len > 32 {
                return Err(Error::InvalidData("offset_len_minus1 > 31"));
            }
            for _ in 0..n {
                h.entry_points.push(r.u(len)?.wrapping_add(1));
            }
        }
    }
    if pps.slice_header_extension {
        let n = r.ue()? as usize;
        r.skip(n * 8)?;
    }
    // byte_alignment(): alignment_bit_equal_to_one then zero bits.
    r.u1()?;
    r.byte_align();
    h.data_offset = r.byte_pos();
    Ok(h)
}

/// Parses the fields present only in an independent slice segment header.
fn parse_independent(
    r: &mut BitReader<'_>,
    nh: &NalHeader,
    sps: &Sps,
    pps: &Pps,
    h: &mut SliceHeader,
) -> Result<()> {
    r.skip(pps.extra_slice_header_bits as usize)?;
    let slice_type = r.ue()?;
    if slice_type != 2 {
        return Err(Error::Unsupported("P and B slices (inter prediction)"));
    }
    if pps.output_flag_present {
        r.u1()?;
    }
    if sps.separate_colour_plane {
        return Err(Error::Unsupported("separate_colour_plane_flag"));
    }
    if nh.nal_unit_type != unit_type::IDR_W_RADL && nh.nal_unit_type != unit_type::IDR_N_LP {
        r.skip(sps.log2_max_poc_lsb as usize)?;
        if r.u1()? == 0 {
            let mut ndp = sps.num_delta_pocs.clone();
            rps::parse(r, sps.num_st_rps, sps.num_st_rps, &mut ndp)?;
        } else if sps.num_st_rps > 1 {
            r.skip(ceil_log2(sps.num_st_rps as usize) as usize)?;
        }
        if sps.long_term_present {
            let n_sps = if sps.num_lt_sps > 0 { r.ue()? } else { 0 };
            let n_pics = r.ue()?;
            if n_sps > 64 || n_pics > 64 {
                return Err(Error::InvalidData("too many long-term reference pictures"));
            }
            for i in 0..n_sps + n_pics {
                if i < n_sps {
                    if sps.num_lt_sps > 1 {
                        r.skip(ceil_log2(sps.num_lt_sps as usize) as usize)?;
                    }
                } else {
                    r.skip(sps.log2_max_poc_lsb as usize)?;
                    r.u1()?;
                }
                if r.u1()? != 0 {
                    r.ue()?;
                }
            }
        }
        if sps.temporal_mvp {
            r.u1()?;
        }
    }
    if sps.sao_enabled {
        h.sao_luma = r.u1()? != 0;
        if sps.chroma_array_type != 0 {
            h.sao_chroma = r.u1()? != 0;
        }
    }
    // SliceQpY lies in -QpBdOffsetY..=51, and the dequantiser shifts by qp / 6: unchecked, a large
    // value shifted by 64 or more (reported by s17sh70x2jfl031ag against 0.1.3).
    let qp_bd_offset = 6 * (i32::from(sps.bit_depth_y) - 8);
    h.qp = pps
        .init_qp
        .checked_add(r.se()?)
        .filter(|qp| (-qp_bd_offset..=51).contains(qp))
        .ok_or(Error::InvalidData("slice QP out of range"))?;
    if pps.slice_chroma_qp_offsets_present {
        h.cb_qp_offset = r.se()?;
        h.cr_qp_offset = r.se()?;
    }
    if pps.chroma_qp_offset_list_enabled {
        h.cu_chroma_qp_offset_enabled = r.u1()? != 0;
    }
    h.deblock_disabled = pps.deblock_disabled;
    h.beta_offset_div2 = pps.beta_offset_div2;
    h.tc_offset_div2 = pps.tc_offset_div2;
    let mut overridden = false;
    if pps.deblock_override_enabled {
        overridden = r.u1()? != 0;
    }
    if overridden {
        h.deblock_disabled = r.u1()? != 0;
        if !h.deblock_disabled {
            h.beta_offset_div2 = r.se()?;
            h.tc_offset_div2 = r.se()?;
        }
    }
    if pps.across_slices && (h.sao_luma || h.sao_chroma || !h.deblock_disabled) {
        h.across_slices = r.u1()? != 0;
    }
    Ok(())
}

/// `Ceil(Log2(n))`.
pub fn ceil_log2(n: usize) -> u32 {
    if n <= 1 {
        0
    } else {
        usize::BITS - (n - 1).leading_zeros()
    }
}
