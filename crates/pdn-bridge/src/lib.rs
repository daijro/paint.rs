//! Desktop host for real Paint.NET plugin DLLs: start CoreCLR through hostfxr and
//! drive the managed `PdnBridge` C ABI (`pdn-host/docs/pdn-bridge.md`).
//!
//! The managed host speaks straight-alpha **BGRA8**; paint.rs stores straight **RGBA8**,
//! so this layer swaps R↔B at the boundary and the rest of paint.rs never sees BGRA.
//! Native-only — the web port uses the Mono browser host with the same JSON protocol.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{CanvasSession, Host, Plugin};

/// A host/plugin error, with the message the managed side reported where there is one.
#[derive(Debug, Clone)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
