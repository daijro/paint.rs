//! Video parameter set (clause 7.3.2.1), skimmed only.

use crate::hevc::bits::BitReader;
use crate::hevc::error::Result;

/// The one VPS field the decoder records.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vps {
    /// `vps_video_parameter_set_id`.
    pub id: u32,
}

/// Parses enough of `video_parameter_set_rbsp()` to recover its id.
///
/// Nothing else in the VPS affects single-picture intra decoding.
pub fn parse(data: &[u8]) -> Result<Vps> {
    let r = &mut BitReader::new(data);
    let id = r.u(4)?;
    Ok(Vps { id })
}
