//! Sequence parameter set (clause 7.3.2.2).

use super::{rps, scaling};

/// Everything the decoder needs from an SPS.
#[derive(Debug, Clone)]
pub struct Sps {
    /// `sps_seq_parameter_set_id`.
    pub id: u32,
    /// `separate_colour_plane_flag`.
    pub separate_colour_plane: bool,
    /// `ChromaArrayType`.
    pub chroma_array_type: u8,
    /// `SubWidthC`, or 0 for monochrome.
    pub sub_w: usize,
    /// `SubHeightC`, or 0 for monochrome.
    pub sub_h: usize,
    /// `pic_width_in_luma_samples`.
    pub width: usize,
    /// `pic_height_in_luma_samples`.
    pub height: usize,
    /// Conformance window offsets in luma samples: left, right, top, bottom.
    pub crop: [usize; 4],
    /// `BitDepthY`.
    pub bit_depth_y: u8,
    /// `BitDepthC`.
    pub bit_depth_c: u8,
    /// `log2_max_pic_order_cnt_lsb_minus4 + 4`.
    pub log2_max_poc_lsb: u32,
    /// `MinCbLog2SizeY`.
    pub log2_min_cb: usize,
    /// `CtbLog2SizeY`.
    pub log2_ctb: usize,
    /// `MinTbLog2SizeY`.
    pub log2_min_tb: usize,
    /// `MaxTbLog2SizeY`.
    pub log2_max_tb: usize,
    /// `max_transform_hierarchy_depth_intra`.
    pub max_tr_depth_intra: u32,
    /// `scaling_list_enabled_flag`.
    pub scaling_list_enabled: bool,
    /// Scaling lists from the SPS, when `sps_scaling_list_data_present_flag`.
    pub scaling: Option<scaling::ScalingListData>,
    /// `sample_adaptive_offset_enabled_flag`.
    pub sao_enabled: bool,
    /// `pcm_enabled_flag`.
    pub pcm_enabled: bool,
    /// `PcmBitDepthY`.
    pub pcm_bit_depth_y: u8,
    /// `PcmBitDepthC`.
    pub pcm_bit_depth_c: u8,
    /// `Log2MinIpcmCbSizeY`.
    pub log2_min_pcm_cb: usize,
    /// `Log2MaxIpcmCbSizeY`.
    pub log2_max_pcm_cb: usize,
    /// `pcm_loop_filter_disabled_flag`.
    pub pcm_loop_filter_disabled: bool,
    /// `num_short_term_ref_pic_sets`.
    pub num_st_rps: u32,
    /// `NumDeltaPocs[]` for the SPS reference picture sets.
    pub num_delta_pocs: rps::NumDeltaPocs,
    /// `long_term_ref_pics_present_flag`.
    pub long_term_present: bool,
    /// `num_long_term_ref_pics_sps`.
    pub num_lt_sps: u32,
    /// `sps_temporal_mvp_enabled_flag`.
    pub temporal_mvp: bool,
    /// `strong_intra_smoothing_enabled_flag`.
    pub strong_intra_smoothing: bool,
    /// `intra_smoothing_disabled_flag` from the range extension.
    pub intra_smoothing_disabled: bool,
    /// `persistent_rice_adaptation_enabled_flag` from the range extension.
    pub persistent_rice: bool,
    /// `transform_skip_context_enabled_flag` from the range extension.
    pub transform_skip_context: bool,
    /// `transform_skip_rotation_enabled_flag` from the range extension.
    pub transform_skip_rotation: bool,
    /// `implicit_rdpcm_enabled_flag` from the range extension.
    pub implicit_rdpcm: bool,
    /// The VUI's `video_signal_type`, when present (paint.rs patch, Ruling R52).
    pub video_signal: Option<super::vui::VideoSignal>,
}

impl Sps {
    /// `PicWidthInCtbsY`.
    pub fn width_in_ctbs(&self) -> usize {
        self.width.div_ceil(1 << self.log2_ctb)
    }

    /// `PicHeightInCtbsY`.
    pub fn height_in_ctbs(&self) -> usize {
        self.height.div_ceil(1 << self.log2_ctb)
    }

    /// `PicSizeInCtbsY`.
    pub fn size_in_ctbs(&self) -> usize {
        self.width_in_ctbs() * self.height_in_ctbs()
    }
}
