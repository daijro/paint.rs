//! Tile geometry and the raster, tile and z-scan address conversions (6.5).

use crate::hevc::error::{Error, Result};
use crate::hevc::ps::{Pps, Sps};
use alloc::vec;
use alloc::vec::Vec;

/// Precomputed addressing tables for one picture.
#[derive(Debug)]
pub struct Geometry {
    /// `PicWidthInCtbsY`.
    pub w_ctbs: usize,
    /// `PicHeightInCtbsY`.
    pub h_ctbs: usize,
    /// `PicSizeInCtbsY`.
    pub size_ctbs: usize,
    /// `CtbLog2SizeY`.
    pub ctb_log2: usize,
    /// `colWidth[i]` in CTBs. Consumed while building the scans and then kept
    /// only so the tile layout can be asserted on.
    #[allow(dead_code)]
    pub col_width: Vec<usize>,
    /// `rowHeight[j]` in CTBs, kept for the same reason as [`Self::col_width`].
    #[allow(dead_code)]
    pub row_height: Vec<usize>,
    /// `CtbAddrRsToTs[]`.
    pub rs_to_ts: Vec<u32>,
    /// `CtbAddrTsToRs[]`.
    pub ts_to_rs: Vec<u32>,
    /// `TileId[]`, indexed by CTB *raster* address.
    pub tile_id: Vec<u16>,
    /// `MinTbAddrZs[]`, indexed by `y * min_tb_w + x` in minimum transform blocks.
    pub zs: Vec<u32>,
    /// Picture width in minimum transform blocks.
    pub min_tb_w: usize,
    /// Picture height in minimum transform blocks.
    pub min_tb_h: usize,
    /// `MinTbLog2SizeY`.
    pub min_tb_log2: usize,
}

impl Geometry {
    /// Builds the addressing tables for the given parameter set pair.
    pub fn new(sps: &Sps, pps: &Pps) -> Result<Geometry> {
        let w_ctbs = sps.width_in_ctbs();
        let h_ctbs = sps.height_in_ctbs();
        let size_ctbs = w_ctbs * h_ctbs;
        let cols = pps.num_tile_cols;
        let rows = pps.num_tile_rows;
        if cols > w_ctbs || rows > h_ctbs {
            return Err(Error::InvalidData("more tiles than coding tree blocks"));
        }
        let col_width = spacing(w_ctbs, cols, pps.uniform_spacing, &pps.column_widths)?;
        let row_height = spacing(h_ctbs, rows, pps.uniform_spacing, &pps.row_heights)?;
        let mut col_bd = vec![0usize; cols + 1];
        for i in 0..cols {
            col_bd[i + 1] = col_bd[i] + col_width[i];
        }
        let mut row_bd = vec![0usize; rows + 1];
        for j in 0..rows {
            row_bd[j + 1] = row_bd[j] + row_height[j];
        }
        let mut rs_to_ts = vec![0u32; size_ctbs];
        let mut ts_to_rs = vec![0u32; size_ctbs];
        let mut tile_id = vec![0u16; size_ctbs];
        for rs in 0..size_ctbs {
            let tbx = rs % w_ctbs;
            let tby = rs / w_ctbs;
            let tx = (0..cols).rev().find(|&i| tbx >= col_bd[i]).unwrap_or(0);
            let ty = (0..rows).rev().find(|&j| tby >= row_bd[j]).unwrap_or(0);
            let mut ts = 0usize;
            for w in &col_width[..tx] {
                ts += row_height[ty] * w;
            }
            for hgt in &row_height[..ty] {
                ts += w_ctbs * hgt;
            }
            ts += (tby - row_bd[ty]) * col_width[tx] + tbx - col_bd[tx];
            rs_to_ts[rs] = ts as u32;
            ts_to_rs[ts] = rs as u32;
            tile_id[rs] = (ty * cols + tx) as u16;
        }
        let min_tb_log2 = sps.log2_min_tb;
        let min_tb_w = sps.width.div_ceil(1 << min_tb_log2);
        let min_tb_h = sps.height.div_ceil(1 << min_tb_log2);
        let shift = sps.log2_ctb - min_tb_log2;
        let mut zs = vec![0u32; min_tb_w * min_tb_h];
        for y in 0..min_tb_h {
            for x in 0..min_tb_w {
                let ctb_rs = w_ctbs * ((y << min_tb_log2) >> sps.log2_ctb)
                    + ((x << min_tb_log2) >> sps.log2_ctb);
                let mut v = rs_to_ts[ctb_rs] << (shift * 2);
                for i in 0..shift {
                    let m = 1usize << i;
                    let mm = (m * m) as u32;
                    v += if x & m != 0 { mm } else { 0 };
                    v += if y & m != 0 { 2 * mm } else { 0 };
                }
                zs[y * min_tb_w + x] = v;
            }
        }
        Ok(Geometry {
            w_ctbs,
            h_ctbs,
            size_ctbs,
            ctb_log2: sps.log2_ctb,
            col_width,
            row_height,
            rs_to_ts,
            ts_to_rs,
            tile_id,
            zs,
            min_tb_w,
            min_tb_h,
            min_tb_log2,
        })
    }

    /// `MinTbAddrZs` for luma position `(x, y)`, or `u32::MAX` outside the picture.
    #[inline]
    pub fn z(&self, x: usize, y: usize) -> u32 {
        let xi = x >> self.min_tb_log2;
        let yi = y >> self.min_tb_log2;
        if xi >= self.min_tb_w || yi >= self.min_tb_h {
            return u32::MAX;
        }
        self.zs[yi * self.min_tb_w + xi]
    }

    /// CTB raster address covering luma position `(x, y)`.
    #[inline]
    pub fn ctb_rs(&self, x: usize, y: usize) -> usize {
        (y >> self.ctb_log2) * self.w_ctbs + (x >> self.ctb_log2)
    }
}

/// Derives `colWidth` / `rowHeight` (clause 6.5.1).
fn spacing(total: usize, n: usize, uniform: bool, explicit: &[usize]) -> Result<Vec<usize>> {
    let mut out = vec![0usize; n];
    if uniform {
        for (i, o) in out.iter_mut().enumerate() {
            *o = ((i + 1) * total) / n - (i * total) / n;
        }
    } else {
        if explicit.len() + 1 != n {
            return Err(Error::InvalidData("tile spacing list has the wrong length"));
        }
        let mut used = 0usize;
        for (i, o) in out.iter_mut().enumerate().take(n - 1) {
            *o = explicit[i];
            used += explicit[i];
        }
        if used >= total {
            return Err(Error::InvalidData(
                "explicit tile spacing exceeds the picture",
            ));
        }
        out[n - 1] = total - used;
    }
    if out.contains(&0) {
        return Err(Error::InvalidData("zero-sized tile"));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "geom_tests.rs"]
mod tests;
