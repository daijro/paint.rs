//! Scaling list data (clause 7.3.4) and the `ScalingFactor` derivation (7.4.5).

use crate::hevc::bits::BitReader;
use crate::hevc::error::{Error, Result};
use crate::hevc::scan::scan_order;
use alloc::boxed::Box;

/// Default 8x8 intra scaling list, Table 7-6, in up-right diagonal scan order.
const DEFAULT_INTRA: [u8; 64] = [
    16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 17, 16, 17, 16, 17, 18, 17, 18, 18, 17, 18, 21, 19, 20,
    21, 20, 19, 21, 24, 22, 22, 24, 24, 22, 22, 24, 25, 25, 27, 30, 27, 25, 25, 29, 31, 35, 35, 31,
    29, 36, 41, 44, 41, 36, 47, 54, 54, 47, 65, 70, 65, 88, 88, 115,
];

/// Default 8x8 inter scaling list, Table 7-6, in up-right diagonal scan order.
const DEFAULT_INTER: [u8; 64] = [
    16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 17, 17, 17, 17, 17, 18, 18, 18, 18, 18, 18, 20, 20, 20,
    20, 20, 20, 20, 24, 24, 24, 24, 24, 24, 24, 24, 25, 25, 25, 25, 25, 25, 25, 28, 28, 28, 28, 28,
    28, 33, 33, 33, 33, 33, 41, 41, 41, 41, 54, 54, 54, 71, 71, 91,
];

/// `ScalingList[sizeId][matrixId][i]` plus the signalled DC coefficients.
#[derive(Debug, Clone)]
pub struct ScalingListData {
    /// Coefficients in up-right diagonal scan order; only 16 used for sizeId 0.
    pub lists: [[[u8; 64]; 6]; 4],
    /// `scaling_list_dc_coef_minus8[sizeId - 2][matrixId] + 8`.
    pub dc: [[u8; 6]; 2],
}

impl Default for ScalingListData {
    fn default() -> Self {
        let mut d = ScalingListData {
            lists: [[[16u8; 64]; 6]; 4],
            dc: [[16u8; 6]; 2],
        };
        for size_id in 1..4 {
            for matrix_id in 0..6 {
                d.lists[size_id][matrix_id] = if matrix_id < 3 {
                    DEFAULT_INTRA
                } else {
                    DEFAULT_INTER
                };
            }
        }
        d
    }
}

/// Parses `scaling_list_data()` into `d`, which must start at the defaults.
pub fn parse(r: &mut BitReader<'_>, d: &mut ScalingListData) -> Result<()> {
    for size_id in 0..4usize {
        let step = if size_id == 3 { 3 } else { 1 };
        let mut matrix_id = 0usize;
        while matrix_id < 6 {
            let coef_num = if size_id == 0 { 16 } else { 64 };
            if r.u1()? == 0 {
                // scaling_list_pred_mode_flag == 0: copy a reference list.
                let delta = r.ue()? as usize;
                if delta == 0 {
                    d.lists[size_id][matrix_id] = if matrix_id < 3 {
                        DEFAULT_INTRA
                    } else {
                        DEFAULT_INTER
                    };
                    if size_id > 1 {
                        d.dc[size_id - 2][matrix_id] = 16;
                    }
                } else {
                    let src = matrix_id
                        .checked_sub(delta * step)
                        .ok_or(Error::InvalidData("scaling_list_pred_matrix_id_delta"))?;
                    d.lists[size_id][matrix_id] = d.lists[size_id][src];
                    if size_id > 1 {
                        d.dc[size_id - 2][matrix_id] = d.dc[size_id - 2][src];
                    }
                }
            } else {
                let mut next = 8i32;
                if size_id > 1 {
                    let dc = r.se()? + 8;
                    if !(1..=255).contains(&dc) {
                        return Err(Error::InvalidData("scaling_list_dc_coef out of range"));
                    }
                    d.dc[size_id - 2][matrix_id] = dc as u8;
                    next = dc;
                }
                for i in 0..coef_num {
                    let delta = r.se()?;
                    next = (next + delta + 256).rem_euclid(256);
                    d.lists[size_id][matrix_id][i] = next as u8;
                }
            }
            matrix_id += step;
        }
    }
    if size_id_fixup(d) {
        return Err(Error::InvalidData(
            "scaling list contains a zero coefficient",
        ));
    }
    Ok(())
}

/// Returns true when any list holds a zero, which the specification forbids.
fn size_id_fixup(d: &ScalingListData) -> bool {
    for size_id in 0..4 {
        let n = if size_id == 0 { 16 } else { 64 };
        for matrix_id in 0..6 {
            if d.lists[size_id][matrix_id][..n].contains(&0) {
                return true;
            }
        }
    }
    false
}

/// `ScalingFactor[sizeId][matrixId][x][y]` flattened to `[y * n + x]`.
#[derive(Debug)]
pub struct ScalingFactors {
    /// 4x4 factors.
    pub f4: [[u8; 16]; 6],
    /// 8x8 factors.
    pub f8: [[u8; 64]; 6],
    /// 16x16 factors.
    pub f16: [[u8; 256]; 6],
    /// 32x32 factors.
    pub f32: [[u8; 1024]; 6],
}

impl ScalingFactors {
    /// Returns the factor block for a transform of `1 << log2_size` samples.
    #[inline]
    pub fn get(&self, log2_size: usize, matrix_id: usize) -> &[u8] {
        match log2_size {
            2 => &self.f4[matrix_id],
            3 => &self.f8[matrix_id],
            4 => &self.f16[matrix_id],
            _ => &self.f32[matrix_id],
        }
    }
}

/// Derives `ScalingFactor` from a parsed scaling list (clause 7.4.5).
pub fn derive(d: &ScalingListData, chroma_array_type: u8) -> Box<ScalingFactors> {
    let mut sf = Box::new(ScalingFactors {
        f4: [[16u8; 16]; 6],
        f8: [[16u8; 64]; 6],
        f16: [[16u8; 256]; 6],
        f32: [[16u8; 1024]; 6],
    });
    let s4 = scan_order(2, 0);
    let s8 = scan_order(3, 0);
    for m in 0..6usize {
        for (i, p) in s4[..16].iter().enumerate() {
            sf.f4[m][p[1] as usize * 4 + p[0] as usize] = d.lists[0][m][i];
        }
        for (i, p) in s8[..64].iter().enumerate() {
            sf.f8[m][p[1] as usize * 8 + p[0] as usize] = d.lists[1][m][i];
        }
        upsample(&d.lists[2][m], &mut sf.f16[m], 16, 2, s8);
        sf.f16[m][0] = d.dc[0][m];
        // sizeId 3 only signals matrixId 0 and 3 unless ChromaArrayType is 3.
        let (src, dc) = if m == 0 || m == 3 || chroma_array_type != 3 {
            (
                &d.lists[3][if m < 3 { 0 } else { 3 }],
                d.dc[1][if m < 3 { 0 } else { 3 }],
            )
        } else {
            (&d.lists[2][m], d.dc[0][m])
        };
        let mut tmp = [16u8; 1024];
        upsample(src, &mut tmp, 32, 4, s8);
        sf.f32[m] = tmp;
        sf.f32[m][0] = dc;
    }
    sf
}

/// Replicates a 64-entry list into an `n` x `n` factor block, `f = n / 8`.
fn upsample(list: &[u8; 64], out: &mut [u8], n: usize, f: usize, scan: &[[u8; 2]; 64]) {
    for (i, p) in scan[..64].iter().enumerate() {
        let (x, y) = (p[0] as usize * f, p[1] as usize * f);
        let v = list[i];
        for j in 0..f {
            let row = (y + j) * n + x;
            out[row..row + f].fill(v);
        }
    }
}
