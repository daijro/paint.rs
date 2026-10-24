//! Inverse transforms (clause 8.6.4).
mod dct;
pub use dct::{inverse_transform, transform_skip};

mod dequant;
pub use dequant::scale;

mod extent;
