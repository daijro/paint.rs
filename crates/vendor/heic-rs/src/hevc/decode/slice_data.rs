//! Slice segment data: the CTU loop, tiles and wavefront substreams (7.3.8.1).

use super::ctu::coding_tree_unit;
use super::geom::Geometry;
use super::state::Dec;
use crate::hevc::cabac::{Cabac, NUM_CTX};
use crate::hevc::error::{Error, Result};
use crate::hevc::filter::{CtbFilter, FilterInfo};
use crate::hevc::nal::Rbsp;
use crate::hevc::picture::Picture;
use crate::hevc::ps::scaling::ScalingFactors;
use crate::hevc::ps::{Pps, Sps};
use crate::hevc::slice::SliceHeader;
use alloc::vec::Vec;

/// Decodes one slice segment into the picture.
#[allow(clippy::too_many_arguments)]
pub fn decode_slice<'a>(
    sps: &'a Sps,
    pps: &'a Pps,
    geo: &'a Geometry,
    sf: Option<&'a ScalingFactors>,
    pic: &'a mut Picture,
    fi: &'a mut FilterInfo,
    h: SliceHeader,
    rbsp: &'a Rbsp,
) -> Result<()> {
    let starts = substream_starts(&h, rbsp);
    let qp_bd_y = 6 * (sps.bit_depth_y as i32 - 8);
    let qp_bd_c = 6 * (sps.bit_depth_c as i32 - 8);
    let cab = Cabac::new(&rbsp.data, starts[0], h.qp)?;
    let ctb_template = CtbFilter {
        deblock_disabled: h.deblock_disabled,
        beta_offset_div2: h.beta_offset_div2.clamp(-6, 6) as i8,
        tc_offset_div2: h.tc_offset_div2.clamp(-6, 6) as i8,
        across_slices: h.across_slices,
        cb_qp_offset: (pps.cb_qp_offset + h.cb_qp_offset) as i16,
        cr_qp_offset: (pps.cr_qp_offset + h.cr_qp_offset) as i16,
        sao: Default::default(),
    };
    let mut d = Dec {
        sps,
        pps,
        geo,
        sf,
        pic,
        fi,
        cat: sps.chroma_array_type,
        qp_bd_y,
        qp_bd_c,
        log2_qg: sps
            .log2_ctb
            .saturating_sub(pps.diff_cu_qp_delta_depth as usize),
        log2_cqg: sps
            .log2_ctb
            .saturating_sub(pps.diff_cu_chroma_qp_offset_depth as usize),
        ctb_rs: 0,
        qp_pred: h.qp,
        qp_last: h.qp,
        qp_y: h.qp,
        qp_delta_coded: false,
        cqo_coded: false,
        cqo_cb: 0,
        cqo_cr: 0,
        tq_bypass: false,
        intra_split: false,
        mode_y: [1; 4],
        mode_c: [1; 4],
        cbf_cb: [[false; 2]; 6],
        cbf_cr: [[false; 2]; 6],
        coeffs: [0; 1024],
        pred: [0; 1024],
        csbf: [0; 64],
        first_qg: true,
        stat_coeff: [0; 4],
        sh: h,
        cab,
    };
    let mut saved: Option<(usize, [u8; NUM_CTX])> = None;
    let mut ts = geo.rs_to_ts[d.sh.segment_address as usize] as usize;
    let mut sub = 0usize;
    loop {
        if ts >= geo.size_ctbs {
            return Err(Error::InvalidData("slice runs past the end of the picture"));
        }
        let rs = geo.ts_to_rs[ts] as usize;
        if ts == geo.rs_to_ts[d.sh.segment_address as usize] as usize
            && pps.wpp_enabled
            && rs % geo.w_ctbs == 0
        {
            restore_wpp(&mut d, &saved, rs, geo);
        }
        d.fi.slice_addr[rs] = d.sh.slice_address;
        d.fi.ctb[rs] = ctb_template;
        coding_tree_unit(&mut d, rs)?;
        if pps.wpp_enabled && (rs % geo.w_ctbs == 1 || geo.w_ctbs == 1) {
            saved = Some((rs / geo.w_ctbs, d.cab.ctx));
        }
        if d.cab.terminate()? != 0 {
            break; // end_of_slice_segment_flag
        }
        ts += 1;
        if ts >= geo.size_ctbs {
            return Err(Error::InvalidData("slice runs past the end of the picture"));
        }
        let next = geo.ts_to_rs[ts] as usize;
        let new_tile = geo.tile_id[next] != geo.tile_id[rs];
        let new_row = pps.wpp_enabled && next % geo.w_ctbs == 0;
        if new_tile || new_row {
            d.cab.terminate()?; // end_of_subset_one_bit, always 1
            sub += 1;
            // The entry point says where the next substream starts; without one
            // it begins at the byte boundary after end_of_subset_one_bit.
            let off = starts
                .get(sub)
                .copied()
                .unwrap_or_else(|| d.cab.aligned_pos());
            if new_row && !new_tile {
                restore_wpp(&mut d, &saved, next, geo);
            } else {
                let qp = d.sh.qp;
                d.cab.init_contexts(qp);
            }
            d.cab.restart_at(off)?;
            d.first_qg = true;
            d.stat_coeff = [0; 4];
            d.qp_last = d.sh.qp;
        }
    }
    Ok(())
}

/// Restores the wavefront context state saved from the row above, if usable.
fn restore_wpp(d: &mut Dec<'_>, saved: &Option<(usize, [u8; NUM_CTX])>, rs: usize, geo: &Geometry) {
    let row = rs / geo.w_ctbs;
    if row > 0 {
        if let Some((saved_row, ctx)) = saved {
            let above_right = (row - 1) * geo.w_ctbs + usize::from(geo.w_ctbs > 1);
            if *saved_row == row - 1
                && geo.tile_id[above_right] == geo.tile_id[rs]
                && d.fi.slice_addr[above_right] == d.sh.slice_address
            {
                d.cab.ctx = *ctx;
                return;
            }
        }
    }
    let qp = d.sh.qp;
    d.cab.init_contexts(qp);
}

/// Converts the entry point offsets into RBSP byte offsets.
fn substream_starts(h: &SliceHeader, rbsp: &Rbsp) -> Vec<usize> {
    let mut starts = Vec::with_capacity(h.entry_points.len() + 1);
    starts.push(h.data_offset);
    let mut nal_off = rbsp.rbsp_to_nal(h.data_offset);
    for &e in &h.entry_points {
        nal_off += e as usize;
        starts.push(rbsp.nal_to_rbsp(nal_off));
    }
    starts
}
