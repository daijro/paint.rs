//! Parsing of `seq_parameter_set_rbsp()` (clause 7.3.2.2).

use super::sps::Sps;
use super::{ptl, rps, scaling, vui};
use crate::hevc::bits::BitReader;
use crate::hevc::error::{Error, Result};

/// Parses `seq_parameter_set_rbsp()`.
pub fn parse(data: &[u8]) -> Result<Sps> {
    let r = &mut BitReader::new(data);
    r.skip(4)?; // sps_video_parameter_set_id
    let max_sub_minus1 = r.u(3)?;
    r.skip(1)?; // sps_temporal_id_nesting_flag
    ptl::parse(r, true, max_sub_minus1)?;
    let id = r.ue()?;
    let chroma_format_idc = r.ue()?;
    if chroma_format_idc > 3 {
        return Err(Error::InvalidData("chroma_format_idc > 3"));
    }
    let separate_colour_plane = chroma_format_idc == 3 && r.u1()? != 0;
    let chroma_array_type = if separate_colour_plane {
        0
    } else {
        chroma_format_idc as u8
    };
    let (sub_w, sub_h) = match chroma_array_type {
        1 => (2usize, 2usize),
        2 => (2, 1),
        3 => (1, 1),
        _ => (0, 0),
    };
    let width = r.ue()? as usize;
    let height = r.ue()? as usize;
    if width == 0 || height == 0 || width > 65535 || height > 65535 {
        return Err(Error::InvalidData("implausible picture dimensions"));
    }
    let mut crop = [0usize; 4];
    if r.u1()? != 0 {
        let (cw, ch) = if sub_w == 0 { (1, 1) } else { (sub_w, sub_h) };
        let too_large = Error::InvalidData("conformance window larger than the picture");
        for (slot, unit) in crop.iter_mut().zip([cw, cw, ch, ch]) {
            *slot = (r.ue()? as usize).checked_mul(unit).ok_or(too_large)?;
        }
        // The window crops inside the picture, leaving at least one sample each way; one as large
        // as the picture started the crop outside the decoded plane (reported by
        // s17sh70x2jfl031ag against 0.1.3). FFmpeg's strict mode refuses the same.
        let inside = |a: usize, b: usize, whole: usize| a.checked_add(b).is_some_and(|s| s < whole);
        if !inside(crop[0], crop[1], width) || !inside(crop[2], crop[3], height) {
            return Err(too_large);
        }
    }
    let bit_depth_y = (r.ue()? + 8) as u8;
    let bit_depth_c = (r.ue()? + 8) as u8;
    if !(8..=16).contains(&bit_depth_y) || !(8..=16).contains(&bit_depth_c) {
        return Err(Error::InvalidData("bit depth out of range"));
    }
    let log2_max_poc_lsb = r.ue()? + 4;
    if log2_max_poc_lsb > 16 {
        return Err(Error::InvalidData("log2_max_pic_order_cnt_lsb too large"));
    }
    let sub_layer_ordering = r.u1()? != 0;
    let first = if sub_layer_ordering {
        0
    } else {
        max_sub_minus1
    };
    for _ in first..=max_sub_minus1 {
        r.ue()?;
        r.ue()?;
        r.ue()?;
    }
    let log2_min_cb = r.ue()? as usize + 3;
    let log2_ctb = log2_min_cb + r.ue()? as usize;
    let log2_min_tb = r.ue()? as usize + 2;
    let log2_max_tb = log2_min_tb + r.ue()? as usize;
    if log2_ctb > 6 || log2_min_cb < 3 || log2_max_tb > 5 || log2_min_tb >= log2_min_cb {
        return Err(Error::InvalidData("implausible block size configuration"));
    }
    r.ue()?; // max_transform_hierarchy_depth_inter
    let max_tr_depth_intra = r.ue()?;
    let scaling_list_enabled = r.u1()? != 0;
    let mut sps_scaling = None;
    if scaling_list_enabled && r.u1()? != 0 {
        let mut d = scaling::ScalingListData::default();
        scaling::parse(r, &mut d)?;
        sps_scaling = Some(d);
    }
    r.u1()?; // amp_enabled_flag, an inter prediction tool
    let sao_enabled = r.u1()? != 0;
    let pcm_enabled = r.u1()? != 0;
    let (mut pbd_y, mut pbd_c, mut l2minp, mut l2maxp, mut pcm_lf) =
        (8u8, 8u8, 3usize, 3usize, false);
    if pcm_enabled {
        pbd_y = r.u(4)? as u8 + 1;
        pbd_c = r.u(4)? as u8 + 1;
        l2minp = r.ue()? as usize + 3;
        l2maxp = l2minp + r.ue()? as usize;
        pcm_lf = r.u1()? != 0;
        // PCM samples are no deeper than the picture's (reported by s17sh70x2jfl031ag against
        // 0.1.3: `bit_depth - pcm_bit_depth` underflowed), and a PCM block is 8 to 32 samples a
        // side, so its log2 sizes are 3 to 5 -- unbounded, they are shift amounts.
        if pbd_y > bit_depth_y || pbd_c > bit_depth_c {
            return Err(Error::InvalidData(
                "PCM bit depth greater than the sample bit depth",
            ));
        }
        if l2maxp > 5 {
            return Err(Error::InvalidData("PCM block size out of range"));
        }
    }
    let num_st_rps = r.ue()?;
    if num_st_rps > 64 {
        return Err(Error::InvalidData("num_short_term_ref_pic_sets > 64"));
    }
    let mut num_delta_pocs = rps::NumDeltaPocs::new();
    for i in 0..num_st_rps {
        rps::parse(r, i, num_st_rps, &mut num_delta_pocs)?;
    }
    let long_term_present = r.u1()? != 0;
    let mut num_lt_sps = 0;
    if long_term_present {
        num_lt_sps = r.ue()?;
        for _ in 0..num_lt_sps {
            r.skip(log2_max_poc_lsb as usize)?;
            r.u1()?;
        }
    }
    let temporal_mvp = r.u1()? != 0;
    let strong_intra_smoothing = r.u1()? != 0;
    let mut sps = Sps {
        id,
        separate_colour_plane,
        chroma_array_type,
        sub_w,
        sub_h,
        width,
        height,
        crop,
        bit_depth_y,
        bit_depth_c,
        log2_max_poc_lsb,
        log2_min_cb,
        log2_ctb,
        log2_min_tb,
        log2_max_tb,
        max_tr_depth_intra,
        scaling_list_enabled,
        scaling: sps_scaling,
        sao_enabled,
        pcm_enabled,
        pcm_bit_depth_y: pbd_y,
        pcm_bit_depth_c: pbd_c,
        log2_min_pcm_cb: l2minp,
        log2_max_pcm_cb: l2maxp,
        pcm_loop_filter_disabled: pcm_lf,
        num_st_rps,
        num_delta_pocs,
        long_term_present,
        num_lt_sps,
        temporal_mvp,
        strong_intra_smoothing,
        intra_smoothing_disabled: false,
        persistent_rice: false,
        transform_skip_context: false,
        transform_skip_rotation: false,
        implicit_rdpcm: false,
        video_signal: None,
    };
    if r.u1()? != 0 {
        sps.video_signal = vui::parse(r, max_sub_minus1)?;
    }
    if r.more_rbsp_data() && r.u1()? != 0 {
        parse_extensions(r, &mut sps)?;
    }
    Ok(sps)
}

/// Parses the SPS extension flags and `sps_range_extension()`.
fn parse_extensions(r: &mut BitReader<'_>, sps: &mut Sps) -> Result<()> {
    let range_ext = r.u1()? != 0;
    let multilayer = r.u1()? != 0;
    let three_d = r.u1()? != 0;
    let scc = r.u1()? != 0;
    r.skip(4)?; // sps_extension_4bits
    if range_ext {
        sps.transform_skip_rotation = r.u1()? != 0;
        sps.transform_skip_context = r.u1()? != 0;
        sps.implicit_rdpcm = r.u1()? != 0;
        let explicit_rdpcm = r.u1()? != 0;
        let extended_precision = r.u1()? != 0;
        sps.intra_smoothing_disabled = r.u1()? != 0;
        r.u1()?; // high_precision_offsets_enabled_flag
        sps.persistent_rice = r.u1()? != 0;
        let cabac_bypass_alignment = r.u1()? != 0;
        if extended_precision {
            return Err(Error::Unsupported("extended_precision_processing_flag"));
        }
        if cabac_bypass_alignment {
            return Err(Error::Unsupported("cabac_bypass_alignment_enabled_flag"));
        }
        if explicit_rdpcm {
            return Err(Error::Unsupported("explicit_rdpcm_enabled_flag"));
        }
    }
    if multilayer || three_d || scc {
        return Err(Error::Unsupported("SPS multilayer, 3D or SCC extension"));
    }
    Ok(())
}
