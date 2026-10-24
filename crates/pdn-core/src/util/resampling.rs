//! Port of: js/src/engine/util/ResamplingAlgorithm.js
//! Reference: paintdotjs @ b4e6fc5

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResamplingAlgorithm {
    NearestNeighbor = 0,
    /// Also BILINEAR.
    Linear = 1,
    /// Also SUPER_SAMPLING.
    MultisampleLinear = 3,
    Anisotropic = 4,
    /// Also BICUBIC.
    HighQualityCubic = 5,
}

impl ResamplingAlgorithm {
    pub const BILINEAR: Self = Self::Linear;
    pub const SUPER_SAMPLING: Self = Self::MultisampleLinear;
    pub const BICUBIC: Self = Self::HighQualityCubic;
}
