//! `vui_parameters` and `hrd_parameters` (Annex E), traversed; only the
//! colour signalling of `video_signal_type` is kept.
//!
//! The decoder needs nothing else from the VUI, but the SPS range extension
//! flags sit after it, so the syntax has to be walked exactly.
//!
//! paint.rs patch (Ruling R52): `parse` returns [`VideoSignal`] so that a file
//! without a `colr` box decodes with the colour its bitstream signals.

use crate::hevc::bits::BitReader;
use crate::hevc::error::Result;

/// Parses `hrd_parameters(commonInfPresentFlag, maxNumSubLayersMinus1)`.
fn hrd(r: &mut BitReader<'_>, common: bool, max_sub_minus1: u32) -> Result<()> {
    let mut nal_hrd = false;
    let mut vcl_hrd = false;
    let mut sub_pic = false;
    if common {
        nal_hrd = r.u1()? != 0;
        vcl_hrd = r.u1()? != 0;
        if nal_hrd || vcl_hrd {
            sub_pic = r.u1()? != 0;
            if sub_pic {
                r.skip(8 + 5 + 1 + 5)?;
            }
            r.skip(4 + 4)?; // bit_rate_scale, cpb_size_scale
            if sub_pic {
                r.skip(4)?; // cpb_size_du_scale
            }
            r.skip(5 + 5 + 5)?;
        }
    }
    for _ in 0..=max_sub_minus1 {
        let fixed_general = r.u1()? != 0;
        let fixed_within = if fixed_general { true } else { r.u1()? != 0 };
        let mut low_delay = false;
        if fixed_within {
            r.ue()?; // elemental_duration_in_tc_minus1
        } else {
            low_delay = r.u1()? != 0;
        }
        let cpb_cnt = if low_delay { 0 } else { r.ue()? };
        let sets = usize::from(nal_hrd) + usize::from(vcl_hrd);
        for _ in 0..sets {
            for _ in 0..=cpb_cnt {
                r.ue()?; // bit_rate_value_minus1
                r.ue()?; // cpb_size_value_minus1
                if sub_pic {
                    r.ue()?; // cpb_size_du_value_minus1
                    r.ue()?; // bit_rate_du_value_minus1
                }
                r.u1()?; // cbr_flag
            }
        }
    }
    Ok(())
}

/// The colour signalling of `video_signal_type` (Annex E.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSignal {
    /// `video_full_range_flag`.
    pub full_range: bool,
    /// `colour_primaries`, `transfer_characteristics` and `matrix_coeffs`,
    /// when `colour_description_present_flag` is set.
    pub colour: Option<(u8, u8, u8)>,
}

/// Parses `vui_parameters()`; only `video_signal_type` is kept.
pub fn parse(r: &mut BitReader<'_>, max_sub_minus1: u32) -> Result<Option<VideoSignal>> {
    let mut signal = None;
    if r.u1()? != 0 {
        // aspect_ratio_info_present_flag
        let idc = r.u(8)?;
        if idc == 255 {
            r.skip(32)?;
        }
    }
    if r.u1()? != 0 {
        r.skip(1)?; // overscan_appropriate_flag
    }
    if r.u1()? != 0 {
        // video_signal_type_present_flag
        r.skip(3)?; // video_format
        let full_range = r.u1()? != 0;
        let colour = if r.u1()? != 0 {
            Some((r.u(8)? as u8, r.u(8)? as u8, r.u(8)? as u8))
        } else {
            None
        };
        signal = Some(VideoSignal { full_range, colour });
    }
    if r.u1()? != 0 {
        // chroma_loc_info_present_flag
        r.ue()?;
        r.ue()?;
    }
    r.skip(3)?; // neutral_chroma_indication, field_seq, frame_field_info
    if r.u1()? != 0 {
        // default_display_window_flag
        for _ in 0..4 {
            r.ue()?;
        }
    }
    if r.u1()? != 0 {
        // vui_timing_info_present_flag
        r.skip(64)?;
        if r.u1()? != 0 {
            r.ue()?; // vui_num_ticks_poc_diff_one_minus1
        }
        if r.u1()? != 0 {
            hrd(r, true, max_sub_minus1)?;
        }
    }
    if r.u1()? != 0 {
        // bitstream_restriction_flag
        r.skip(3)?;
        for _ in 0..5 {
            r.ue()?;
        }
    }
    Ok(signal)
}
