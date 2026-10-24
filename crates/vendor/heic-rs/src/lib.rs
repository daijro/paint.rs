//! A pure-Rust decoder for HEIC and HEIF still images.
//!
//! No C, no `unsafe`, one optional dependency, and a `no_std` core that
//! builds for `wasm32-unknown-unknown` with no dependencies at all.
//!
//! # Status
//!
//! Complete and self-contained: boxes, `ftyp`, `meta` and its children, item
//! properties, `grid` derivation and tile compositing, transforms, EXIF and
//! ICC extraction, YCbCr to RGB conversion, and the HEVC still-picture intra
//! decoder in [`hevc`]. [`decode()`] turns a HEIC file into pixels with no C
//! and no `unsafe` anywhere in the chain.
//!
//! # The rule this crate is built on
//!
//! Business logic performs no I/O. [`decode()`] and [`probe()`] take `&[u8]`
//! and return values; nothing beneath them opens a file, reads the environment
//! or touches the network. Filesystem convenience lives in [`io`], behind the
//! default-on `std` feature, and nowhere else. That separation is what lets
//! the core be `no_std`, what makes it build for the browser, and what makes
//! every parser in it testable against a byte array written by hand.
//!
//! # Reading a file
//!
//! ```no_run
//! # fn main() -> Result<(), heic_rs::Error> {
//! let bytes = std::fs::read("photo.heic").unwrap_or_default();
//!
//! // Ask what the file is, without decoding it.
//! let info = heic_rs::probe(&bytes)?;
//! println!("{}x{}, {} bit", info.width, info.height, info.bit_depth);
//!
//! // Decode it.
//! let options = heic_rs::DecodeOptions::default();
//! let image = heic_rs::decode(&bytes, &options)?;
//! assert_eq!(image.data.len(), image.row_bytes() * image.height as usize);
//! # Ok(())
//! # }
//! ```
//!
//! # Untrusted input
//!
//! HEIF files are untrusted input. Every parser here is bounds-checked and
//! returns an error rather than panicking, and
//! [`DecodeOptions::max_pixels`] — [`DEFAULT_MAX_PIXELS`], 256 megapixels —
//! stops a small file from asking for an enormous allocation.
//!
//! # Features
//!
//! - `std` (default): the [`io`] module, and `std::error::Error` for
//!   [`Error`]. Turn it off for `no_std` and for wasm.
//! - `parallel` (default): decode grid tiles and convert colour on a `rayon`
//!   pool. Implies `std`, and is absent from the `no_std` and wasm builds.
//!   The output is identical either way; see [`DecodeOptions::threads`] for
//!   the knob and for what happens inside a caller's own pool.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(all(feature = "std", not(test)))]
extern crate std;

pub mod boxes;
pub mod color;
pub mod context;
pub mod decode;
pub mod error;
pub mod ftyp;
pub mod grid;
pub mod hevc;
pub mod image;
pub mod meta;
pub mod metadata;
mod parallel;
pub mod probe;
pub mod props;
pub mod reader;
pub mod transform;
pub mod upsample;

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub mod io;

pub use crate::decode::decode;
pub use crate::error::{Error, Result};
pub use crate::ftyp::Brand;
pub use crate::grid::Grid;
pub use crate::hevc::ChromaFormat;
pub use crate::image::{DEFAULT_MAX_PIXELS, DecodeOptions, Image, PixelLayout};
pub use crate::probe::{GridInfo, ImageInfo, probe};
pub use crate::props::Transform;
pub use crate::props::colr::{MatrixCoefficients, Nclx, Range};
pub use crate::props::simple::{Mirror, Rotation};
