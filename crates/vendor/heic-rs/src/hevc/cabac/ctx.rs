//! Context variable layout and initialisation values for I slices.
//!
//! Only `initType == 0` is tabulated: this decoder rejects P and B slices, so
//! no other initialisation type can ever be selected.

/// Offsets of each syntax element's context block in the flat context array.
pub mod off {
    /// `sao_merge_left_flag` and `sao_merge_up_flag`.
    pub const SAO_MERGE: usize = 0;
    /// `sao_type_idx_luma` and `sao_type_idx_chroma`.
    pub const SAO_TYPE: usize = 1;
    /// `split_cu_flag`, three contexts.
    pub const SPLIT_CU: usize = 2;
    /// `cu_transquant_bypass_flag`.
    pub const TQ_BYPASS: usize = 5;
    /// `part_mode`; only the first context can occur in an I slice.
    pub const PART_MODE: usize = 6;
    /// `prev_intra_luma_pred_flag`.
    pub const PREV_INTRA: usize = 7;
    /// `intra_chroma_pred_mode`.
    pub const INTRA_CHROMA: usize = 8;
    /// `split_transform_flag`, three contexts.
    pub const SPLIT_TRANSFORM: usize = 9;
    /// `cbf_luma`, two contexts.
    pub const CBF_LUMA: usize = 12;
    /// `cbf_cb` and `cbf_cr`, five contexts.
    pub const CBF_CHROMA: usize = 14;
    /// `cu_qp_delta_abs`, two contexts.
    pub const CU_QP_DELTA: usize = 19;
    /// `transform_skip_flag` for luma.
    pub const TS_LUMA: usize = 21;
    /// `transform_skip_flag` for chroma.
    pub const TS_CHROMA: usize = 22;
    /// `last_sig_coeff_x_prefix`, eighteen contexts.
    pub const LAST_X: usize = 23;
    /// `last_sig_coeff_y_prefix`, eighteen contexts.
    pub const LAST_Y: usize = 41;
    /// `coded_sub_block_flag`, four contexts.
    pub const CSBF: usize = 59;
    /// `sig_coeff_flag`, forty-four contexts (two are range-extension only).
    pub const SIG: usize = 63;
    /// `coeff_abs_level_greater1_flag`, twenty-four contexts.
    pub const GT1: usize = 107;
    /// `coeff_abs_level_greater2_flag`, six contexts.
    pub const GT2: usize = 131;
    /// `cu_chroma_qp_offset_flag`.
    pub const CHROMA_QP_FLAG: usize = 137;
    /// `cu_chroma_qp_offset_idx`.
    pub const CHROMA_QP_IDX: usize = 138;
}

/// Total number of context variables tracked.
pub const NUM_CTX: usize = 139;

/// `initValue` for every context, for `initType == 0` (I slices).
pub static INIT_VALUES: [u8; NUM_CTX] = [
    153, // sao_merge
    200, // sao_type_idx
    139, 141, 157, // split_cu_flag
    154, // cu_transquant_bypass_flag
    184, // part_mode
    184, // prev_intra_luma_pred_flag
    63,  // intra_chroma_pred_mode
    153, 138, 138, // split_transform_flag
    111, 141, // cbf_luma
    94, 138, 182, 154, 154, // cbf_cb / cbf_cr
    154, 154, // cu_qp_delta_abs
    139, // transform_skip_flag, luma
    139, // transform_skip_flag, chroma
    // last_sig_coeff_x_prefix
    110, 110, 124, 125, 140, 153, 125, 127, 140, 109, 111, 143, 127, 111, 79, 108, 123, 63,
    // last_sig_coeff_y_prefix
    110, 110, 124, 125, 140, 153, 125, 127, 140, 109, 111, 143, 127, 111, 79, 108, 123, 63,
    // coded_sub_block_flag
    91, 171, 134, 141,
    // sig_coeff_flag (42 core contexts then 2 for transform-skip contexts)
    111, 111, 125, 110, 110, 94, 124, 108, 124, 107, 125, 141, 179, 153, 125, 107, 125, 141, 179,
    153, 125, 107, 125, 141, 179, 153, 125, 140, 139, 182, 182, 152, 136, 152, 136, 153, 136, 139,
    111, 136, 139, 111, 141, 111, // coeff_abs_level_greater1_flag
    140, 92, 137, 138, 140, 152, 138, 139, 153, 74, 149, 92, 139, 107, 122, 152, 140, 179, 166,
    182, 140, 227, 122, 197, // coeff_abs_level_greater2_flag
    138, 153, 136, 167, 152, 152, 154, // cu_chroma_qp_offset_flag
    154, // cu_chroma_qp_offset_idx
];
