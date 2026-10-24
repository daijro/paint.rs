//! Coding tree and coding unit parsing (clauses 7.3.8.4 and 7.3.8.5).

use super::mode::{chroma_mode, luma_mode};
use super::state::Dec;
use super::tu::transform_tree;
use crate::hevc::cabac::off;
use crate::hevc::error::{Error, Result};
use crate::hevc::picture::{Picture, edge, flags};

/// Parses `coding_quadtree(x0, y0, log2CbSize, cqtDepth)`.
pub fn coding_quadtree(
    d: &mut Dec<'_>,
    x0: usize,
    y0: usize,
    log2_size: usize,
    depth: u8,
) -> Result<()> {
    let size = 1usize << log2_size;
    let fits = x0 + size <= d.sps.width && y0 + size <= d.sps.height;
    let split = if fits && log2_size > d.sps.log2_min_cb {
        let mut inc = 0usize;
        if d.available(x0, y0, x0 as isize - 1, y0 as isize)
            && d.pic.ct_depth[d.pic.idx4(x0 - 1, y0)] > depth
        {
            inc += 1;
        }
        if d.available(x0, y0, x0 as isize, y0 as isize - 1)
            && d.pic.ct_depth[d.pic.idx4(x0, y0 - 1)] > depth
        {
            inc += 1;
        }
        d.cab.decision(off::SPLIT_CU + inc) != 0
    } else {
        log2_size > d.sps.log2_min_cb
    };
    if d.pps.cu_qp_delta_enabled && log2_size >= d.log2_qg {
        let first = d.first_qg;
        d.start_qg(x0, y0, first);
        d.first_qg = false;
    }
    if d.pps.chroma_qp_offset_list_enabled && log2_size >= d.log2_cqg {
        d.cqo_coded = false;
        d.cqo_cb = 0;
        d.cqo_cr = 0;
    }
    if split {
        let half = size >> 1;
        for (i, (dx, dy)) in [(0, 0), (half, 0), (0, half), (half, half)]
            .into_iter()
            .enumerate()
        {
            let _ = i;
            let (x, y) = (x0 + dx, y0 + dy);
            if x < d.sps.width && y < d.sps.height {
                coding_quadtree(d, x, y, log2_size - 1, depth + 1)?;
            }
        }
        Ok(())
    } else {
        coding_unit(d, x0, y0, log2_size, depth)
    }
}

/// Parses `coding_unit(x0, y0, log2CbSize)` for an intra picture.
fn coding_unit(d: &mut Dec<'_>, x0: usize, y0: usize, log2_size: usize, depth: u8) -> Result<()> {
    let size = 1usize << log2_size;
    Picture::fill4(&mut d.pic.ct_depth, d.pic.min4_w, x0, y0, size, depth);
    d.tq_bypass = false;
    if d.pps.transquant_bypass {
        d.tq_bypass = d.cab.decision(off::TQ_BYPASS) != 0;
    }
    d.intra_split = false;
    if log2_size == d.sps.log2_min_cb && d.cab.decision(off::PART_MODE) == 0 {
        d.intra_split = true;
    }
    let mut cu_flags = 0u8;
    if d.tq_bypass {
        cu_flags |= flags::TQ_BYPASS;
    }
    d.pic.mark_edge(x0, y0, size, edge::VER | edge::HOR);
    let pcm_possible = !d.intra_split
        && d.sps.pcm_enabled
        && log2_size >= d.sps.log2_min_pcm_cb
        && log2_size <= d.sps.log2_max_pcm_cb;
    if pcm_possible && d.cab.terminate()? != 0 {
        cu_flags |= flags::PCM;
        Picture::fill4(&mut d.pic.cu_flags, d.pic.min4_w, x0, y0, size, cu_flags);
        Picture::fill4(&mut d.pic.intra_mode, d.pic.min4_w, x0, y0, size, 1);
        read_pcm(d, x0, y0, size)?;
        d.store_qp(x0, y0, size);
        return Ok(());
    }
    Picture::fill4(&mut d.pic.cu_flags, d.pic.min4_w, x0, y0, size, cu_flags);
    luma_mode(d, x0, y0, log2_size)?;
    chroma_mode(d, log2_size)?;
    let max_depth = d.sps.max_tr_depth_intra + u32::from(d.intra_split);
    d.cbf_cb = [[false; 2]; 6];
    d.cbf_cr = [[false; 2]; 6];
    transform_tree(d, x0, y0, x0, y0, log2_size, 0, 0, max_depth)?;
    d.store_qp(x0, y0, size);
    Ok(())
}

/// Reads `pcm_sample()` (clause 7.3.8.7) and writes the samples straight out.
fn read_pcm(d: &mut Dec<'_>, x0: usize, y0: usize, size: usize) -> Result<()> {
    d.cab.align_after_terminate();
    let shift_y = d.sps.bit_depth_y - d.sps.pcm_bit_depth_y;
    let bits_y = d.sps.pcm_bit_depth_y as u32;
    for j in 0..size {
        for i in 0..size {
            let v = (d.cab.raw_bits(bits_y)? as u16) << shift_y;
            if x0 + i < d.pic.y.width && y0 + j < d.pic.y.height {
                d.pic.y.put(x0 + i, y0 + j, v);
            }
        }
    }
    if d.cat != 0 {
        let (sw, sh) = (d.sps.sub_w, d.sps.sub_h);
        let (cw, ch) = (size / sw, size / sh);
        let shift_c = d.sps.bit_depth_c - d.sps.pcm_bit_depth_c;
        let bits_c = d.sps.pcm_bit_depth_c as u32;
        for c in 0..2 {
            for j in 0..ch {
                for i in 0..cw {
                    let v = (d.cab.raw_bits(bits_c)? as u16) << shift_c;
                    let (px, py) = (x0 / sw + i, y0 / sh + j);
                    let plane = if c == 0 { &mut d.pic.cb } else { &mut d.pic.cr };
                    if px < plane.width && py < plane.height {
                        plane.put(px, py, v);
                    }
                }
            }
        }
    }
    d.cab.init_engine()?;
    Ok(())
}

/// Parses `coding_tree_unit()` (clause 7.3.8.2).
pub fn coding_tree_unit(d: &mut Dec<'_>, ctb_rs: usize) -> Result<()> {
    let log2 = d.sps.log2_ctb;
    let x = (ctb_rs % d.geo.w_ctbs) << log2;
    let y = (ctb_rs / d.geo.w_ctbs) << log2;
    d.ctb_rs = ctb_rs;
    if d.sh.sao_luma || d.sh.sao_chroma {
        super::sao_syn::sao(d, ctb_rs)?;
    }
    if x >= d.sps.width || y >= d.sps.height {
        return Err(Error::InvalidData("coding tree block outside the picture"));
    }
    coding_quadtree(d, x, y, log2, 0)
}
