//! Builds complete synthetic HEVC bitstreams, bit by bit.
//!
//! Nothing external is needed: the parameter sets are written with this
//! module tree's own bit writer and the slice data with its own CABAC
//! encoder. The golden-vector test and the benchmarks both
//! build their input here, so the whole path from NAL parsing through CABAC,
//! the coding tree, intra prediction and the loop filters is exercised without
//! any external fixture.

use crate::hevc::bits::BitWriter;
use crate::hevc::cabac::ctx::off;
use crate::hevc::cabac::encoder::Enc;
use alloc::vec::Vec;

/// Wraps an RBSP in a NAL unit header, inserting emulation prevention bytes.
pub fn to_nal(nal_type: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = alloc::vec![nal_type << 1, 1];
    let mut zeros = 0usize;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
    out
}

/// Builds a video parameter set with the mandatory fields only.
pub fn build_vps() -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(0, 4); // vps_video_parameter_set_id
    w.put(3, 2); // vps_base_layer_internal_flag, vps_base_layer_available_flag
    w.put(0, 6); // vps_max_layers_minus1
    w.put(0, 3); // vps_max_sub_layers_minus1
    w.put(1, 1); // vps_temporal_id_nesting_flag
    w.put(0xffff, 16);
    profile_tier_level(&mut w);
    w.rbsp_trailing();
    w.bytes
}

/// Writes `profile_tier_level(1, 0)` for the Main Still Picture profile.
fn profile_tier_level(w: &mut BitWriter) {
    w.put(0, 2); // general_profile_space
    w.put(0, 1); // general_tier_flag
    w.put(3, 5); // general_profile_idc: Main Still Picture
    for i in 0..32 {
        w.put(u32::from(i == 3), 1);
    }
    w.put(0b1101, 4); // progressive, not interlaced, non-packed, frame only
    w.put(0, 22); // general_reserved_zero_43bits, first half
    w.put(0, 21); // general_reserved_zero_43bits, second half
    w.put(0, 1); // general_inbld_flag
    w.put(30, 8); // general_level_idc
}

/// Builds a sequence parameter set for a 64x64 8-bit 4:2:0 picture.
pub fn build_sps(width: u32, height: u32, chroma_idc: u32, bit_depth: u32) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(0, 4); // sps_video_parameter_set_id
    w.put(0, 3); // sps_max_sub_layers_minus1
    w.put(1, 1); // sps_temporal_id_nesting_flag
    profile_tier_level(&mut w);
    w.ue(0); // sps_seq_parameter_set_id
    w.ue(chroma_idc); // chroma_format_idc
    if chroma_idc == 3 {
        w.put(0, 1); // separate_colour_plane_flag
    }
    w.ue(width); // pic_width_in_luma_samples
    w.ue(height); // pic_height_in_luma_samples
    w.put(0, 1); // conformance_window_flag
    w.ue(bit_depth - 8); // bit_depth_luma_minus8
    w.ue(bit_depth - 8); // bit_depth_chroma_minus8
    w.ue(4); // log2_max_pic_order_cnt_lsb_minus4
    w.put(1, 1); // sps_sub_layer_ordering_info_present_flag
    w.ue(0);
    w.ue(0);
    w.ue(0);
    w.ue(0); // log2_min_luma_coding_block_size_minus3: 8x8
    w.ue(3); // log2_diff_max_min_luma_coding_block_size: CTB 64x64
    w.ue(0); // log2_min_luma_transform_block_size_minus2: 4x4
    w.ue(3); // log2_diff_max_min_luma_transform_block_size: 32x32
    w.ue(0); // max_transform_hierarchy_depth_inter
    w.ue(0); // max_transform_hierarchy_depth_intra
    w.put(0, 1); // scaling_list_enabled_flag
    w.put(0, 1); // amp_enabled_flag
    w.put(0, 1); // sample_adaptive_offset_enabled_flag
    w.put(0, 1); // pcm_enabled_flag
    w.ue(0); // num_short_term_ref_pic_sets
    w.put(0, 1); // long_term_ref_pics_present_flag
    w.put(0, 1); // sps_temporal_mvp_enabled_flag
    w.put(0, 1); // strong_intra_smoothing_enabled_flag
    w.put(0, 1); // vui_parameters_present_flag
    w.put(0, 1); // sps_extension_present_flag
    w.rbsp_trailing();
    w.bytes
}

/// Builds a picture parameter set with the loop filters off; `tiles` asks for
/// a two by two uniform tile grid.
pub fn build_pps_opt(tiles: bool) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.ue(0); // pps_pic_parameter_set_id
    w.ue(0); // pps_seq_parameter_set_id
    w.put(0, 1); // dependent_slice_segments_enabled_flag
    w.put(0, 1); // output_flag_present_flag
    w.put(0, 3); // num_extra_slice_header_bits
    w.put(0, 1); // sign_data_hiding_enabled_flag
    w.put(0, 1); // cabac_init_present_flag
    w.ue(0);
    w.ue(0);
    w.se(0); // init_qp_minus26
    w.put(0, 1); // constrained_intra_pred_flag
    w.put(0, 1); // transform_skip_enabled_flag
    w.put(0, 1); // cu_qp_delta_enabled_flag
    w.se(0); // pps_cb_qp_offset
    w.se(0); // pps_cr_qp_offset
    w.put(0, 1); // pps_slice_chroma_qp_offsets_present_flag
    w.put(0, 2); // weighted_pred_flag, weighted_bipred_flag
    w.put(0, 1); // transquant_bypass_enabled_flag
    w.put(u32::from(tiles), 1); // tiles_enabled_flag
    w.put(0, 1); // entropy_coding_sync_enabled_flag
    if tiles {
        w.ue(1); // num_tile_columns_minus1
        w.ue(1); // num_tile_rows_minus1
        w.put(1, 1); // uniform_spacing_flag
        w.put(1, 1); // loop_filter_across_tiles_enabled_flag
    }
    w.put(0, 1); // pps_loop_filter_across_slices_enabled_flag
    w.put(1, 1); // deblocking_filter_control_present_flag
    w.put(0, 1); // deblocking_filter_override_enabled_flag
    w.put(1, 1); // pps_deblocking_filter_disabled_flag
    w.put(0, 1); // pps_scaling_list_data_present_flag
    w.put(0, 1); // lists_modification_present_flag
    w.ue(0); // log2_parallel_merge_level_minus2
    w.put(0, 1); // slice_segment_header_extension_present_flag
    w.put(0, 1); // pps_extension_present_flag
    w.rbsp_trailing();
    w.bytes
}

/// Builds the single I slice: one 64x64 coding unit predicted with DC.
pub fn build_slice(ctbs: usize, chroma_idc: u32) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(1, 1); // first_slice_segment_in_pic_flag
    w.put(0, 1); // no_output_of_prior_pics_flag
    w.ue(0); // slice_pic_parameter_set_id
    w.ue(2); // slice_type: I
    w.se(0); // slice_qp_delta
    w.put(1, 1); // alignment_bit_equal_to_one
    while w.len_bits() % 8 != 0 {
        w.put(0, 1);
    }
    let mut e = Enc::new(26);
    for k in 0..ctbs {
        encode_ctu(&mut e, chroma_idc);
        e.terminate(u32::from(k + 1 == ctbs)); // end_of_slice_segment_flag
    }
    let mut out = w.bytes;
    out.extend_from_slice(&e.finish());
    out
}

/// Builds a complete DC-predicted still picture of `width` x `height` samples.
///
/// Both dimensions must be multiples of the 64 sample coding tree block size,
/// `chroma_idc` is `chroma_format_idc` (0, 1, 2 or 3) and `bit_depth` is 8 to
/// 14. The returned parameter sets and slice are ready for
/// [`crate::hevc::decode_still`]; every sample of the result is `1 << (bit_depth - 1)`.
pub fn picture_fmt(
    width: u32,
    height: u32,
    chroma_idc: u32,
    bit_depth: u32,
) -> (Vec<Vec<u8>>, Vec<u8>) {
    let ctbs = (width as usize / 64) * (height as usize / 64);
    let sets = alloc::vec![
        to_nal(32, &build_vps()),
        to_nal(33, &build_sps(width, height, chroma_idc, bit_depth)),
        to_nal(34, &build_pps()),
    ];
    (sets, to_nal(19, &build_slice(ctbs, chroma_idc)))
}

/// [`picture_fmt`] for the common case of 8-bit 4:2:0.
pub fn picture(width: u32, height: u32) -> (Vec<Vec<u8>>, Vec<u8>) {
    picture_fmt(width, height, 1, 8)
}

/// [`build_pps_opt`] without tiles.
pub fn build_pps() -> Vec<u8> {
    build_pps_opt(false)
}

/// Encodes the coding unit syntax of one all-DC 64x64 coding tree block.
fn encode_ctu(e: &mut Enc, chroma_idc: u32) {
    e.decision(off::SPLIT_CU, 0); // split_cu_flag: keep the 64x64 coding unit
    e.decision(off::PREV_INTRA, 1); // prev_intra_luma_pred_flag
    e.bypass(1); // mpm_idx = 1, selecting INTRA_DC from {planar, DC, vertical}
    e.bypass(0);
    if chroma_idc != 0 {
        e.decision(off::INTRA_CHROMA, 0); // intra_chroma_pred_mode: from luma
        e.decision(off::CBF_CHROMA, 0); // cbf_cb at trafoDepth 0
        e.decision(off::CBF_CHROMA, 0); // cbf_cr at trafoDepth 0
    }
    for _ in 0..4 {
        e.decision(off::CBF_LUMA, 0); // cbf_luma of each forced 32x32 split
    }
}

/// Builds a 256x256 picture cut into a two by two grid of tiles.
///
/// Each tile is its own CABAC substream, so this exercises the tile scan, the
/// context reset at a tile boundary and `end_of_subset_one_bit`. No entry point
/// offsets are signalled, so the decoder has to find each substream itself.
pub fn picture_tiled() -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut w = BitWriter::default();
    w.put(1, 1); // first_slice_segment_in_pic_flag
    w.put(0, 1); // no_output_of_prior_pics_flag
    w.ue(0); // slice_pic_parameter_set_id
    w.ue(2); // slice_type: I
    w.se(0); // slice_qp_delta
    w.ue(0); // num_entry_point_offsets
    w.put(1, 1); // alignment_bit_equal_to_one
    while w.len_bits() % 8 != 0 {
        w.put(0, 1);
    }
    let mut out = w.bytes;
    for tile in 0..4 {
        let mut e = Enc::new(26);
        for ctb in 0..4 {
            encode_ctu(&mut e, 1);
            let last = tile == 3 && ctb == 3;
            e.terminate(u32::from(last)); // end_of_slice_segment_flag
        }
        if tile != 3 {
            e.terminate(1); // end_of_subset_one_bit
        }
        out.extend_from_slice(&e.finish());
    }
    let sets = alloc::vec![
        to_nal(32, &build_vps()),
        to_nal(33, &build_sps(256, 256, 1, 8)),
        to_nal(34, &build_pps_opt(true)),
    ];
    (sets, to_nal(19, &out))
}
