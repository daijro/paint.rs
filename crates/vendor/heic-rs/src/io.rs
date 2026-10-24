//! Filesystem convenience, and the only module in this crate that performs I/O.
//!
//! The decoding path takes `&[u8]` and returns pixels; it never opens a file,
//! never consults the environment and never touches the network. Everything
//! that does is here, behind the `std` feature, so that the rest of the crate
//! stays `no_std` and builds for `wasm32-unknown-unknown`.

use alloc::vec::Vec;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::image::{DecodeOptions, Image};
use crate::probe::ImageInfo;

/// Read a file into memory.
pub fn read(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    Ok(fs::read(path)?)
}

/// Read a file and decode its primary image.
pub fn decode_file(path: impl AsRef<Path>, options: &DecodeOptions) -> Result<Image> {
    crate::decode(&read(path)?, options)
}

/// Read a file and report what it says about itself.
pub fn probe_file(path: impl AsRef<Path>) -> Result<ImageInfo> {
    crate::probe(&read(path)?)
}

/// True when the bytes look like a HEIF file: a `ftyp` box whose brand this
/// crate recognises. Useful for sniffing a directory without parsing it.
pub fn looks_like_heif(bytes: &[u8]) -> bool {
    match crate::ftyp::parse(bytes) {
        Ok(_) => true,
        // An AVIF file is a HEIF file we decline to decode, not a non-HEIF one.
        Err(Error::Unsupported(_)) => bytes.len() > 8 && &bytes[4..8] == b"ftyp",
        Err(_) => false,
    }
}
