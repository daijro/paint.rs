//! Hooks that let the benchmarks reach internal stages of the decoder.
//!
//! Everything here is behind the `bench` feature and is not part of the
//! crate's public interface.

use crate::hevc::cabac::encoder::Enc;
use crate::hevc::cabac::{Cabac, NUM_CTX, off};
use crate::hevc::error::Result;
use crate::hevc::intra::{Refs, filter_refs, predict};
use crate::hevc::transform::{inverse_transform, scale};
use alloc::vec::Vec;

/// Produces a bitstream of `n` pseudo-random context-coded and bypass bins.
pub fn encode_bins(n: usize, qp: i32) -> Vec<u8> {
    let mut e = Enc::new(qp);
    let mut s = 0x2545_f491u32;
    for _ in 0..n {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        if s % 8 < 6 {
            e.decision((s >> 8) as usize % NUM_CTX, (s >> 4) & 1);
        } else {
            e.bypass((s >> 4) & 1);
        }
    }
    e.terminate(1);
    e.finish()
}

/// Decodes `n` bins from a stream produced by [`encode_bins`].
pub fn decode_bins(data: &[u8], n: usize, qp: i32) -> Result<u32> {
    let mut c = Cabac::new(data, 0, qp)?;
    let mut s = 0x2545_f491u32;
    let mut acc = 0u32;
    for _ in 0..n {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        acc = acc.wrapping_add(if s % 8 < 6 {
            c.decision((s >> 8) as usize % NUM_CTX)
        } else {
            c.bypass()
        });
    }
    Ok(acc)
}

/// Scales and inverse transforms one block, as the decoder does per transform unit.
pub fn dequant_and_transform(block: &mut [i32], n: usize, qp: i32, bit_depth: u8) {
    scale(block, n, qp, bit_depth, None);
    inverse_transform(block, n, 0, bit_depth);
}

/// Builds a reference sample set with a deterministic ramp.
pub fn ramp_refs(n: usize) -> Refs {
    let mut r = Refs::new(n);
    for i in 0..4 * n + 1 {
        r.buf[i] = ((i * 7 + 13) & 0xff) as u16;
    }
    r
}

/// Filters the references and predicts one block, as intra reconstruction does.
pub fn filter_and_predict(r: &Refs, mode: u8, bit_depth: u8, out: &mut [u16], stride: usize) {
    let f = filter_refs(r, mode, 0, bit_depth, 1, true, false);
    predict(&f, mode, 0, bit_depth, out, stride);
}

/// The CABAC context index of `split_cu_flag`, so benches can name one.
pub const SPLIT_CU_CTX: usize = off::SPLIT_CU;
