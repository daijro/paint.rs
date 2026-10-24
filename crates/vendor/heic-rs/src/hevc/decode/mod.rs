//! Picture level decoding: parameter set selection, slice dispatch, filters.

mod ctu;
mod geom;
mod mode;
mod recon;
mod refs;
mod residual;
mod residual_ctx;
mod residual_levels;
mod sao_syn;
mod sets;
mod slice_data;
mod state;
mod tu;

pub use geom::Geometry;
pub use sets::ParameterSets;

use crate::hevc::error::{Error, Result};
use crate::hevc::filter::{CtbFilter, FilterInfo, deblock, sao};
use crate::hevc::nal::{NalHeader, Rbsp};
use crate::hevc::picture::Picture;
use crate::hevc::ps::scaling;
use crate::hevc::slice::{SliceHeader, parse as parse_slice_header};
use crate::hevc::{ChromaFormat, Frame};
use alloc::vec;
use alloc::vec::Vec;

/// Decodes one still picture from its parameter sets and slice NAL units.
pub fn decode(sets: &ParameterSets, slices: &[&[u8]]) -> Result<Frame> {
    if slices.is_empty() {
        return Err(Error::NoSlices);
    }
    let mut parsed: Vec<(NalHeader, Rbsp)> = Vec::with_capacity(slices.len());
    for nal in slices {
        let h = NalHeader::parse(nal)?;
        if !h.is_vcl() || h.layer_id != 0 {
            continue;
        }
        if !h.is_irap() {
            return Err(Error::Unsupported("non-IRAP picture (inter prediction)"));
        }
        parsed.push((h, Rbsp::from_nal(nal)?));
    }
    let first = parsed.first().ok_or(Error::NoSlices)?;
    let probe_pps = {
        let r = &mut crate::hevc::bits::BitReader::new(&first.1.data);
        r.u1()?;
        if first.0.is_irap() {
            r.u1()?;
        }
        r.ue()?
    };
    let pps = sets.pps(probe_pps)?;
    let sps = sets.sps(pps.sps_id)?;
    if sps.implicit_rdpcm {
        return Err(Error::Unsupported("implicit_rdpcm_enabled_flag"));
    }
    let geo = Geometry::new(sps, pps)?;
    let mut pic = Picture::new(sps.width, sps.height, sps.sub_w, sps.sub_h)?;
    let factors = scaling_factors(sps, pps);
    let mut fi = FilterInfo {
        bit_depth_y: sps.bit_depth_y,
        bit_depth_c: sps.bit_depth_c,
        chroma_array_type: sps.chroma_array_type,
        sub_w: sps.sub_w,
        sub_h: sps.sub_h,
        ctb_log2: sps.log2_ctb,
        pic_w_ctbs: geo.w_ctbs,
        pic_h_ctbs: geo.h_ctbs,
        pcm_loop_filter_disabled: sps.pcm_loop_filter_disabled,
        across_tiles: pps.across_tiles,
        sao_enabled: false,
        ctb: vec![CtbFilter::default(); geo.size_ctbs],
        tile_id: geo.tile_id.clone(),
        slice_addr: vec![u32::MAX; geo.size_ctbs],
    };
    let mut prev: Option<SliceHeader> = None;
    for (nh, rbsp) in &parsed {
        let h = parse_slice_header(nh, &rbsp.data, sps, pps, prev.as_ref())?;
        if h.pps_id != pps.id {
            return Err(Error::Unsupported("a picture using more than one PPS"));
        }
        fi.sao_enabled |= h.sao_luma || h.sao_chroma;
        let hh = h.clone();
        slice_data::decode_slice(
            sps,
            pps,
            &geo,
            factors.as_deref(),
            &mut pic,
            &mut fi,
            h,
            rbsp,
        )?;
        prev = Some(hh);
    }
    if fi.slice_addr.contains(&u32::MAX) {
        return Err(Error::InvalidData("picture is not fully covered by slices"));
    }
    deblock(&mut pic, &fi);
    sao(&mut pic, &fi);
    Ok(crop(&pic, sps))
}

/// Derives the scaling factors in force for the picture, if any.
fn scaling_factors(
    sps: &crate::hevc::ps::Sps,
    pps: &crate::hevc::ps::Pps,
) -> Option<alloc::boxed::Box<scaling::ScalingFactors>> {
    if !sps.scaling_list_enabled {
        return None;
    }
    let data = match (&pps.scaling, &sps.scaling) {
        (Some(d), _) => d.clone(),
        (None, Some(d)) => d.clone(),
        (None, None) => scaling::ScalingListData::default(),
    };
    Some(scaling::derive(&data, sps.chroma_array_type))
}

/// Applies the conformance window and packs the planes into a [`Frame`].
fn crop(pic: &Picture, sps: &crate::hevc::ps::Sps) -> Frame {
    let [l, r, t, b] = sps.crop;
    let w = sps.width.saturating_sub(l + r).max(1);
    let h = sps.height.saturating_sub(t + b).max(1);
    let (sw, sh) = if sps.sub_w == 0 {
        (1, 1)
    } else {
        (sps.sub_w, sps.sub_h)
    };
    let (cw, ch) = if sps.sub_w == 0 {
        (0, 0)
    } else {
        (w / sw, h / sh)
    };
    let mut y = Vec::with_capacity(w * h);
    for j in 0..h {
        let o = (t + j) * pic.y.stride + l;
        y.extend_from_slice(&pic.y.data[o..o + w]);
    }
    let mut cb = Vec::with_capacity(cw * ch);
    let mut cr = Vec::with_capacity(cw * ch);
    for j in 0..ch {
        let o = (t / sh + j) * pic.cb.stride + l / sw;
        cb.extend_from_slice(&pic.cb.data[o..o + cw]);
        cr.extend_from_slice(&pic.cr.data[o..o + cw]);
    }
    Frame {
        width: w as u32,
        height: h as u32,
        bit_depth: sps.bit_depth_y,
        chroma: ChromaFormat::from_idc(sps.chroma_array_type).unwrap_or(ChromaFormat::Monochrome),
        y,
        cb,
        cr,
        y_stride: w as u32,
        c_stride: cw as u32,
    }
}
