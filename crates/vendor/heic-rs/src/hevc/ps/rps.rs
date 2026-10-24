//! Short-term reference picture sets (clause 7.3.7).
//!
//! An intra-only decoder never uses the contents, but the syntax has to be
//! traversed exactly to stay aligned with the rest of the bitstream.

use crate::hevc::bits::BitReader;
use crate::hevc::error::{Error, Result};

/// `NumDeltaPocs[]` for every short-term reference picture set in the SPS.
pub type NumDeltaPocs = alloc::vec::Vec<u32>;

/// Parses one `st_ref_pic_set(stRpsIdx)` and appends its `NumDeltaPocs`.
///
/// `idx` is `stRpsIdx`; `num_sets` is `num_short_term_ref_pic_sets`. Sets
/// parsed from a slice header pass `idx == num_sets` and are not recorded.
pub fn parse(r: &mut BitReader<'_>, idx: u32, num_sets: u32, ndp: &mut NumDeltaPocs) -> Result<()> {
    let mut inter_pred = false;
    if idx != 0 {
        inter_pred = r.u1()? != 0;
    }
    let num_delta = if inter_pred {
        let mut delta_idx = 1u32;
        if idx == num_sets {
            delta_idx = r.ue()? + 1;
        }
        if delta_idx > idx {
            return Err(Error::InvalidData("delta_idx_minus1 out of range"));
        }
        let ref_idx = (idx - delta_idx) as usize;
        let ref_ndp = *ndp
            .get(ref_idx)
            .ok_or(Error::InvalidData("bad RefRpsIdx"))?;
        r.u1()?; // delta_rps_sign
        r.ue()?; // abs_delta_rps_minus1
        let mut count = 0u32;
        for _ in 0..=ref_ndp {
            let used = r.u1()? != 0;
            let mut keep = used;
            if !used {
                keep = r.u1()? != 0;
            }
            if keep {
                count += 1;
            }
        }
        count
    } else {
        let neg = r.ue()?;
        let pos = r.ue()?;
        if neg > 16 || pos > 16 {
            return Err(Error::InvalidData("too many short-term reference pictures"));
        }
        for _ in 0..neg {
            r.ue()?;
            r.u1()?;
        }
        for _ in 0..pos {
            r.ue()?;
            r.u1()?;
        }
        neg + pos
    };
    if idx < num_sets {
        ndp.push(num_delta);
    }
    Ok(())
}
