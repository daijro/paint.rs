//! Ported from PhotoCraft crates/plugins/src/lib.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: a module of `extra` instead of a crate (so no crate-level lint attributes); `Error` implements `Display` by hand instead of through `thiserror`, and gains `Cancelled` for paint.rs's effect cancel flag; `Plugin::apply` runs on paint.rs's straight RGBA8 layer (`apply`).
//!
//! Sandboxed WebAssembly plug-ins (PhotoCraft's plug-in ABI v1, `PLUGINS.md`): installed filters
//! that run as effects (`effect::wasm`). A plug-in runs in [`wasmi`], a pure-Rust interpreter, with:
//!
//! - **no host imports**: no WASI, file system, network, clock or randomness. A module that
//!   imports anything is rejected;
//! - an **instruction budget** (fuel) per call, metered in slices so a wall-clock deadline (native
//!   builds) and the effect's cancel flag are also enforced, so infinite loops end with an error;
//! - a **linear-memory cap**, a **recursion-depth cap** and wasmi's strict module limits;
//! - a **fresh instance per band**, so no state leaks between calls.
//!
//! Every failure (malformed module, trap, exhausted budget, bad output) is an [`Error`]; nothing
//! a module does can panic or hang the host.
//!
//! Pixels are handed over as interleaved, normalised `f32` RGBA (straight alpha), in horizontal
//! bands so the copy stays bounded. The process-wide [`registry`] holds the installed plug-ins.

mod apply;
pub mod manifest;
pub mod registry;
mod runtime;
#[cfg(test)]
mod tests;

pub use apply::FORMAT_RGBA8;
pub use manifest::{Area, Kind, Manifest, ParamSpec, Unit};
pub use runtime::{ABI_VERSION, Limits, Plugin};

/// A plug-in error. Every way a module can misbehave ends here, never in a panic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Module(String),
    Abi(String),
    Manifest(String),
    Trap(String),
    Limit(String),
    Params(String),
    Failed(String),
    NotFound(String),
    Io(String),
    Cancelled,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Module(m) => write!(f, "not a valid plug-in module: {m}"),
            Error::Abi(m) => write!(f, "plug-in ABI: {m}"),
            Error::Manifest(m) => write!(f, "plug-in manifest: {m}"),
            Error::Trap(m) => write!(f, "plug-in trapped: {m}"),
            Error::Limit(m) => write!(f, "plug-in exceeded its {m}"),
            Error::Params(m) => write!(f, "plug-in parameters: {m}"),
            Error::Failed(m) => write!(f, "plug-in failed: {m}"),
            Error::NotFound(id) => write!(f, "no plug-in {id:?} is installed"),
            Error::Io(m) => f.write_str(m),
            Error::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
