//! Collected parameter sets and the sequence/picture lookup they provide.

use crate::hevc::error::{Error, Result};
use crate::hevc::nal::{NalHeader, Rbsp, unit_type};
use crate::hevc::ps::{Pps, Sps, Vps, parse_sps, pps, vps};
use alloc::vec::Vec;

/// All parameter sets supplied to a decode call.
#[derive(Debug, Default)]
pub struct ParameterSets {
    /// Video parameter sets, kept only so callers can see they parsed.
    pub vps: Vec<Vps>,
    /// Sequence parameter sets.
    pub sps: Vec<Sps>,
    /// Picture parameter sets.
    pub pps: Vec<Pps>,
}

impl ParameterSets {
    /// Parses a list of headerless NAL units into their parameter sets.
    pub fn parse(sets: &[&[u8]]) -> Result<ParameterSets> {
        let mut out = ParameterSets::default();
        for nal in sets {
            let h = NalHeader::parse(nal)?;
            if h.layer_id != 0 {
                continue;
            }
            let rbsp = Rbsp::from_nal(nal)?;
            match h.nal_unit_type {
                unit_type::VPS => {
                    let v = vps::parse(&rbsp.data)?;
                    out.vps.retain(|e| e.id != v.id);
                    out.vps.push(v);
                }
                unit_type::SPS => {
                    let s = parse_sps(&rbsp.data)?;
                    out.sps.retain(|e| e.id != s.id);
                    out.sps.push(s);
                }
                unit_type::PPS => {
                    let p = pps::parse(&rbsp.data)?;
                    out.pps.retain(|e| e.id != p.id);
                    out.pps.push(p);
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Looks up a picture parameter set by id.
    pub fn pps(&self, id: u32) -> Result<&Pps> {
        self.pps
            .iter()
            .find(|p| p.id == id)
            .ok_or(Error::MissingParameterSet("PPS"))
    }

    /// Looks up a sequence parameter set by id.
    pub fn sps(&self, id: u32) -> Result<&Sps> {
        self.sps
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::MissingParameterSet("SPS"))
    }

    /// The sequence parameter set the first picture parameter set refers to.
    pub fn first_sps(&self) -> Result<&Sps> {
        match self.pps.first() {
            Some(p) => self.sps(p.sps_id),
            None => self.sps.first().ok_or(Error::MissingParameterSet("SPS")),
        }
    }
}
