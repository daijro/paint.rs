//! The crate's hand-written error type.
//!
//! No `thiserror`: this crate is `no_std` by construction, and the error type
//! carries only `'static` strings and integers so that reporting a failure
//! never allocates.

use core::fmt;

/// Everything that can go wrong while reading a HEIF file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The file ended in the middle of a structure that was still being read.
    Truncated(&'static str),
    /// A structure was present but its contents violate the format.
    Malformed(&'static str),
    /// The file is well formed but asks for something this build cannot do.
    Unsupported(&'static str),
    /// A box required to make sense of the file is absent.
    MissingBox(&'static str),
    /// An item id referenced from elsewhere does not exist.
    MissingItem(u32),
    /// A coded slice named a VPS, SPS or PPS the `hvcC` record did not carry.
    MissingParameterSet(&'static str),
    /// A `dimg` derivation chain refers back to itself.
    CyclicDerivation(u32),
    /// The declared image is larger than [`DecodeOptions::max_pixels`].
    ///
    /// [`DecodeOptions::max_pixels`]: crate::DecodeOptions::max_pixels
    PixelLimit {
        /// Pixels the file asks for.
        pixels: u64,
        /// Pixels the caller allowed.
        max_pixels: u64,
    },
    /// A filesystem failure from the `io` module. Never produced by the
    /// decoding path, which performs no I/O.
    #[cfg(feature = "std")]
    Io(std::io::ErrorKind),
    /// A box declared a size that cannot be honoured on this machine.
    BoxTooLarge {
        /// The four-character box type.
        boxtype: [u8; 4],
        /// The size the box declared.
        size: u64,
    },
}

impl Error {
    /// The four-character code as text, for display purposes.
    fn fourcc(code: &[u8; 4]) -> &str {
        core::str::from_utf8(code).unwrap_or("????")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated(what) => write!(f, "truncated file: ran out of bytes reading {what}"),
            Error::Malformed(what) => write!(f, "malformed file: {what}"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::MissingBox(what) => write!(f, "required box `{what}` is missing"),
            Error::MissingItem(id) => write!(f, "item {id} is referenced but not present"),
            Error::MissingParameterSet(what) => {
                write!(
                    f,
                    "the bitstream names a {what} that the file does not carry"
                )
            }
            Error::CyclicDerivation(id) => {
                write!(f, "item {id} derives from itself, directly or indirectly")
            }
            Error::PixelLimit { pixels, max_pixels } => write!(
                f,
                "image would be {pixels} pixels, above the {max_pixels} pixel limit"
            ),
            #[cfg(feature = "std")]
            Error::Io(kind) => write!(f, "i/o error: {kind}"),
            Error::BoxTooLarge { boxtype, size } => write!(
                f,
                "box `{}` declares an implausible size of {size} bytes",
                Self::fourcc(boxtype)
            ),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e.kind())
    }
}

/// Shorthand for results carried through this crate.
pub type Result<T> = core::result::Result<T, Error>;
