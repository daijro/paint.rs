//! js/src/engine/util/: each submodule names its own port source. Processor.js and UUID.js
//! have no Rust counterpart (nothing here uses a CPU count or a hand-rolled UUID).
//! Reference: paintdotjs @ b4e6fc5

pub mod bitvec2d;
pub mod color;
pub mod geom;
pub mod quantize;
pub mod region;
pub mod resampling;
pub mod shortcut;
pub mod utility;
