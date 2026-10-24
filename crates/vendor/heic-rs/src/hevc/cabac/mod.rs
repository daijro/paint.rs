//! CABAC entropy decoding.

pub mod ctx;
#[cfg(any(test, feature = "bench"))]
pub mod encoder;
mod engine;
mod tables;

pub use ctx::{NUM_CTX, off};
pub use engine::Cabac;
#[cfg(any(test, feature = "bench"))]
pub use engine::initial_contexts;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
