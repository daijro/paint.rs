//! Photocraft phase spec: S§4.2 (vendored algorithm code)
//!
//! PhotoCraft's algorithms (commit ff47388, MIT; NOTICE.md), ported for the Extra effects and
//! tools, and the small shims that stand in for its crates.

pub mod content_aware;
pub mod distance;
pub mod geom;
pub mod inpaint;
pub mod interrupt;
pub mod job;
pub mod magnetic;
pub mod nonlocal;
pub mod par;
pub mod patch;
pub mod perspective;
pub mod poisson;
pub mod puppet;
pub mod remove;
/// Not PhotoCraft's: pdn-content-aware-fill's Resynthesizer (GPL-2.0-or-later), for Content Aware Fill.
pub mod resynthesizer;
pub mod rng;
pub mod sample;
pub mod segment;
pub mod shim;
pub mod warp;

/// The ported effect kernels, one file per upstream module.
pub mod fx {
    pub mod adjust;
    pub mod artistic;
    pub mod artistic_fx;
    pub mod auto;
    pub mod average;
    pub mod camera_raw;
    pub mod color_to_alpha;
    pub mod distort;
    pub mod distort2;
    pub mod distort2_pc30;
    pub mod flame;
    pub mod gallery;
    pub mod lens;
    pub mod liquify;
    pub mod lut;
    pub mod matting;
    pub mod noise;
    pub mod other;
    pub mod other2;
    pub mod pixelate;
    pub mod relight;
    pub mod render;
    pub mod render2;
    pub mod shape_blur;
    pub mod sharpen;
    pub mod stylize;
    pub mod stylize2;
    pub mod tone;
    pub mod util;
    pub mod vanishing;
    pub mod video;
}
