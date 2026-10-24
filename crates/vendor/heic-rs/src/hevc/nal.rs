//! NAL unit layer: Annex-B splitting, NAL headers and emulation prevention.

use crate::hevc::error::{Error, Result};
use alloc::vec::Vec;

/// NAL unit types used by this decoder.
pub mod unit_type {
    /// Coded slice of a BLA picture with leading pictures.
    pub const BLA_W_LP: u8 = 16;
    /// Coded slice of an IDR picture with decodable leading pictures.
    pub const IDR_W_RADL: u8 = 19;
    /// Coded slice of an IDR picture without leading pictures.
    pub const IDR_N_LP: u8 = 20;
    /// Last reserved IRAP type.
    pub const RSV_IRAP_VCL23: u8 = 23;
    /// Video parameter set.
    pub const VPS: u8 = 32;
    /// Sequence parameter set.
    pub const SPS: u8 = 33;
    /// Picture parameter set.
    pub const PPS: u8 = 34;
}

/// Parsed two-byte NAL unit header (clause 7.3.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NalHeader {
    /// `nal_unit_type`.
    pub nal_unit_type: u8,
    /// `nuh_layer_id`.
    pub layer_id: u8,
    /// `nuh_temporal_id_plus1 - 1`.
    pub temporal_id: u8,
}

impl NalHeader {
    /// Parses the header from the first two bytes of a NAL unit.
    pub fn parse(nal: &[u8]) -> Result<NalHeader> {
        if nal.len() < 2 {
            return Err(Error::Truncated);
        }
        if nal[0] & 0x80 != 0 {
            return Err(Error::InvalidData("forbidden_zero_bit set"));
        }
        let tid_plus1 = nal[1] & 0x07;
        if tid_plus1 == 0 {
            return Err(Error::InvalidData("nuh_temporal_id_plus1 is zero"));
        }
        Ok(NalHeader {
            nal_unit_type: (nal[0] >> 1) & 0x3f,
            layer_id: ((nal[0] & 1) << 5) | (nal[1] >> 3),
            temporal_id: tid_plus1 - 1,
        })
    }

    /// True for a VCL NAL unit type.
    pub fn is_vcl(&self) -> bool {
        self.nal_unit_type < 32
    }

    /// True for an intra random access point picture.
    pub fn is_irap(&self) -> bool {
        (unit_type::BLA_W_LP..=unit_type::RSV_IRAP_VCL23).contains(&self.nal_unit_type)
    }
}

/// A NAL unit with emulation prevention bytes removed.
///
/// `epb` records the *original* NAL-unit byte offsets at which a `0x03` byte
/// was dropped, which is what `entry_point_offset_minus1` counts in.
#[derive(Debug, Clone, Default)]
pub struct Rbsp {
    /// The de-escaped payload, starting after the two-byte NAL header.
    pub data: Vec<u8>,
    /// Offsets (within the whole NAL unit) of removed `0x03` bytes.
    pub epb: Vec<u32>,
}

impl Rbsp {
    /// Strips the two-byte header and all emulation prevention bytes.
    pub fn from_nal(nal: &[u8]) -> Result<Rbsp> {
        if nal.len() < 2 {
            return Err(Error::Truncated);
        }
        let mut out = Vec::with_capacity(nal.len() - 2);
        let mut epb = Vec::new();
        let mut zeros = 0usize;
        let mut i = 2usize;
        while i < nal.len() {
            let b = nal[i];
            if zeros >= 2 && b == 0x03 {
                epb.push(i as u32);
                zeros = 0;
                i += 1;
                continue;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            out.push(b);
            i += 1;
        }
        Ok(Rbsp { data: out, epb })
    }

    /// Converts a payload offset (RBSP bytes) to a NAL unit offset.
    pub fn rbsp_to_nal(&self, rbsp_off: usize) -> usize {
        let mut nal_off = rbsp_off + 2;
        for &p in &self.epb {
            if (p as usize) <= nal_off {
                nal_off += 1;
            } else {
                break;
            }
        }
        nal_off
    }

    /// Converts a NAL unit offset to a payload offset (RBSP bytes).
    pub fn nal_to_rbsp(&self, nal_off: usize) -> usize {
        let mut removed = 0usize;
        for &p in &self.epb {
            if (p as usize) < nal_off {
                removed += 1;
            } else {
                break;
            }
        }
        nal_off.saturating_sub(2 + removed)
    }
}

/// Splits an Annex-B byte stream into NAL units, dropping the start codes.
///
/// HEIF frames NAL units with length prefixes, which the `hvcC` property
/// already knows how to split; this is the codec's native framing and is used
/// only by the decoder's own tests.
#[cfg(test)]
pub fn split_annexb(stream: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let n = stream.len();
    let mut i = 0usize;
    // Find the first start code.
    let mut start = None;
    while i + 3 <= n {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            start = Some(i + 3);
            i += 3;
            break;
        }
        i += 1;
    }
    let mut cur = match start {
        Some(s) => s,
        None => return out,
    };
    while i + 3 <= n {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            let mut end = i;
            // A four byte start code steals one trailing zero from the payload.
            if end > cur && stream[end - 1] == 0 {
                end -= 1;
            }
            if end > cur {
                out.push(&stream[cur..end]);
            }
            cur = i + 3;
            i += 3;
        } else {
            i += 1;
        }
    }
    if cur < n {
        out.push(&stream[cur..n]);
    }
    out
}

#[cfg(test)]
#[path = "nal_tests.rs"]
mod tests;
