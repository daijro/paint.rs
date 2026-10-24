//! `hvcC`: the HEVC decoder configuration record (ISO/IEC 14496-15).

use alloc::vec::Vec;

use crate::boxes::BoxHeader;
use crate::error::{Error, Result};
use crate::reader::Reader;

/// NAL unit type 32: video parameter set.
pub const NAL_VPS: u8 = 32;
/// NAL unit type 33: sequence parameter set.
pub const NAL_SPS: u8 = 33;
/// NAL unit type 34: picture parameter set.
pub const NAL_PPS: u8 = 34;

/// One array of NAL units of a single type.
#[derive(Debug, Clone)]
pub struct NalArray<'a> {
    /// Whether the writer promises this array is exhaustive.
    pub completeness: bool,
    /// The NAL unit type these entries carry.
    pub nal_type: u8,
    /// The NAL units, without any length prefix or start code.
    pub nals: Vec<&'a [u8]>,
}

/// The decoder configuration record, parsed in full.
#[derive(Debug, Clone)]
pub struct HevcConfig<'a> {
    /// Always 1 in files written to the current specification.
    pub configuration_version: u8,
    /// `general_profile_space`, two bits.
    pub general_profile_space: u8,
    /// `general_tier_flag`: false is Main tier, true is High tier.
    pub general_tier_flag: bool,
    /// `general_profile_idc`: 1 is Main, 2 is Main 10, 3 is Main Still Picture.
    pub general_profile_idc: u8,
    /// The 32 profile compatibility flags.
    pub general_profile_compatibility_flags: u32,
    /// The 48 constraint indicator flags, right-aligned in a `u64`.
    pub general_constraint_indicator_flags: u64,
    /// `general_level_idc`, thirty times the level number.
    pub general_level_idc: u8,
    /// `min_spatial_segmentation_idc`, twelve bits.
    pub min_spatial_segmentation_idc: u16,
    /// `parallelismType`.
    pub parallelism_type: u8,
    /// Chroma format: 0 monochrome, 1 is 4:2:0, 2 is 4:2:2, 3 is 4:4:4.
    pub chroma_format: u8,
    /// Luma bit depth, already un-biased from `bitDepthLumaMinus8`.
    pub bit_depth_luma: u8,
    /// Chroma bit depth, already un-biased.
    pub bit_depth_chroma: u8,
    /// `avgFrameRate`, meaningless for a still image.
    pub avg_frame_rate: u16,
    /// `numTemporalLayers`.
    pub num_temporal_layers: u8,
    /// `temporalIdNested`.
    pub temporal_id_nested: bool,
    /// Bytes in each NAL length prefix, already un-biased from
    /// `lengthSizeMinusOne`. Files written by macOS use 4.
    pub length_size: u8,
    /// The parameter-set arrays, in the order the record listed them.
    pub arrays: Vec<NalArray<'a>>,
}

impl<'a> HevcConfig<'a> {
    /// Every VPS, SPS and PPS, in the order a decoder wants to be fed them.
    pub fn parameter_sets(&self) -> Vec<&'a [u8]> {
        let mut out = Vec::new();
        for want in [NAL_VPS, NAL_SPS, NAL_PPS] {
            for a in self.arrays.iter().filter(|a| a.nal_type == want) {
                out.extend_from_slice(&a.nals);
            }
        }
        out
    }

    /// Split length-prefixed item data into its NAL units.
    ///
    /// HEIF stores picture items as a bare sequence of NAL units, each with a
    /// big-endian length prefix of [`length_size`] bytes, rather than with
    /// Annex B start codes.
    ///
    /// [`length_size`]: HevcConfig::length_size
    pub fn split_nals<'b>(&self, data: &'b [u8]) -> Result<Vec<&'b [u8]>> {
        let mut r = Reader::new(data);
        let mut out = Vec::new();
        while r.remaining() > 0 {
            if r.remaining() < usize::from(self.length_size) {
                return Err(Error::Truncated("NAL length prefix"));
            }
            let len = r.uint(self.length_size, "NAL length prefix")?;
            let len = usize::try_from(len).map_err(|_| Error::Truncated("NAL unit"))?;
            if len == 0 {
                return Err(Error::Malformed("zero-length NAL unit"));
            }
            out.push(r.take(len, "NAL unit")?);
        }
        Ok(out)
    }
}

/// Parse an `hvcC` property box.
pub fn parse<'a>(b: &BoxHeader<'a>) -> Result<HevcConfig<'a>> {
    let mut r = b.reader();
    let configuration_version = r.u8("hvcC version")?;
    let w = r.u8("hvcC profile byte")?;
    let general_profile_space = w >> 6;
    let general_tier_flag = w & 0x20 != 0;
    let general_profile_idc = w & 0x1f;
    let general_profile_compatibility_flags = r.u32("hvcC compatibility flags")?;
    let hi = u64::from(r.u16("hvcC constraint flags")?);
    let lo = u64::from(r.u32("hvcC constraint flags")?);
    let general_constraint_indicator_flags = (hi << 32) | lo;
    let general_level_idc = r.u8("hvcC level")?;
    let min_spatial_segmentation_idc = r.u16("hvcC segmentation")? & 0x0fff;
    let parallelism_type = r.u8("hvcC parallelism")? & 0x03;
    let chroma_format = r.u8("hvcC chroma format")? & 0x03;
    let bit_depth_luma = (r.u8("hvcC luma depth")? & 0x07) + 8;
    let bit_depth_chroma = (r.u8("hvcC chroma depth")? & 0x07) + 8;
    let avg_frame_rate = r.u16("hvcC frame rate")?;
    let last = r.u8("hvcC layer byte")?;
    let num_temporal_layers = (last >> 3) & 0x07;
    let temporal_id_nested = last & 0x04 != 0;
    let length_size = (last & 0x03) + 1;

    let num_arrays = r.u8("hvcC array count")?;
    let mut arrays = Vec::with_capacity(usize::from(num_arrays));
    for _ in 0..num_arrays {
        let head = r.u8("hvcC array header")?;
        let completeness = head & 0x80 != 0;
        let nal_type = head & 0x3f;
        let count = r.u16("hvcC nalu count")?;
        if usize::from(count) * 2 > r.remaining() {
            return Err(Error::Malformed(
                "hvcC declares more NAL units than it holds",
            ));
        }
        let mut nals = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let len = usize::from(r.u16("hvcC nalu length")?);
            nals.push(r.take(len, "hvcC nal unit")?);
        }
        arrays.push(NalArray {
            completeness,
            nal_type,
            nals,
        });
    }
    Ok(HevcConfig {
        configuration_version,
        general_profile_space,
        general_tier_flag,
        general_profile_idc,
        general_profile_compatibility_flags,
        general_constraint_indicator_flags,
        general_level_idc,
        min_spatial_segmentation_idc,
        parallelism_type,
        chroma_format,
        bit_depth_luma,
        bit_depth_chroma,
        avg_frame_rate,
        num_temporal_layers,
        temporal_id_nested,
        length_size,
        arrays,
    })
}
