//! Sample adaptive offset syntax (clause 7.3.8.3).

use super::state::Dec;
use crate::hevc::cabac::off;
use crate::hevc::error::Result;
use crate::hevc::filter::SaoCtb;

/// Parses `sao(rx, ry)` for the coding tree block at raster address `ctb_rs`.
pub fn sao(d: &mut Dec<'_>, ctb_rs: usize) -> Result<()> {
    let w = d.geo.w_ctbs;
    let (rx, ry) = (ctb_rs % w, ctb_rs / w);
    let mut merge_left = false;
    let mut merge_up = false;
    if rx > 0 && mergeable(d, ctb_rs, ctb_rs - 1) {
        merge_left = d.cab.decision(off::SAO_MERGE) != 0;
    }
    if !merge_left && ry > 0 && mergeable(d, ctb_rs, ctb_rs - w) {
        merge_up = d.cab.decision(off::SAO_MERGE) != 0;
    }
    if merge_left || merge_up {
        let src = if merge_left { ctb_rs - 1 } else { ctb_rs - w };
        let sao = d.fi.ctb[src].sao;
        d.fi.ctb[ctb_rs].sao = sao;
        return Ok(());
    }
    let components = if d.cat != 0 { 3 } else { 1 };
    let max_abs = (1u32 << (d.sps.bit_depth_y.min(10) - 5)) - 1;
    let max_abs_c = (1u32 << (d.sps.bit_depth_c.min(10) - 5)) - 1;
    let mut shared = SaoCtb::default();
    for c in 0..components {
        let enabled = if c == 0 {
            d.sh.sao_luma
        } else {
            d.sh.sao_chroma
        };
        if !enabled {
            d.fi.ctb[ctb_rs].sao[c] = SaoCtb::default();
            continue;
        }
        let mut s = if c == 2 {
            SaoCtb {
                type_idx: shared.type_idx,
                eo_class: shared.eo_class,
                ..Default::default()
            }
        } else {
            SaoCtb::default()
        };
        if c != 2 {
            s.type_idx = read_type(d)?;
        }
        if s.type_idx != 0 {
            let cap = if c == 0 { max_abs } else { max_abs_c };
            let mut abs = [0u32; 4];
            for a in abs.iter_mut() {
                let mut v = 0u32;
                while v < cap && d.cab.bypass() == 1 {
                    v += 1;
                }
                *a = v;
            }
            let scale = if c == 0 {
                d.pps.sao_offset_scale_luma
            } else {
                d.pps.sao_offset_scale_chroma
            };
            if s.type_idx == 1 {
                for (k, &a) in abs.iter().enumerate() {
                    let neg = a != 0 && d.cab.bypass() == 1;
                    let v = (a << scale) as i16;
                    s.offsets[k] = if neg { -v } else { v };
                }
                s.band_position = d.cab.bypass_bits(5) as u8;
            } else {
                for (k, &a) in abs.iter().enumerate() {
                    let v = (a << scale) as i16;
                    s.offsets[k] = if k < 2 { v } else { -v };
                }
                if c != 2 {
                    s.eo_class = d.cab.bypass_bits(2) as u8;
                }
            }
        }
        if c == 1 {
            shared = s;
        }
        d.fi.ctb[ctb_rs].sao[c] = s;
    }
    Ok(())
}

/// Decodes `sao_type_idx_luma` / `sao_type_idx_chroma` (Table 9-38).
fn read_type(d: &mut Dec<'_>) -> Result<u8> {
    if d.cab.decision(off::SAO_TYPE) == 0 {
        return Ok(0);
    }
    Ok(if d.cab.bypass() == 0 { 1 } else { 2 })
}

/// True when the SAO parameters of `src` may be merged into `cur`.
fn mergeable(d: &Dec<'_>, cur: usize, src: usize) -> bool {
    d.geo.tile_id[cur] == d.geo.tile_id[src] && d.fi.slice_addr[cur] == d.fi.slice_addr[src]
}
