//! The decoder's own notion of failure.
//!
//! It is deliberately narrower than [`crate::Error`] — the codec knows nothing
//! about boxes, items or pixel ceilings — and it never escapes this module
//! tree: the `From` impl at the bottom is the seam, and it hands every message
//! through unchanged so that a caller still reads what the decoder refused.

/// Every failure mode the decoder can report.
///
/// The decoder never panics and never performs I/O; all failures surface here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The bitstream ended in the middle of a syntax element.
    Truncated,
    /// A syntax element had a value the specification forbids.
    InvalidData(&'static str),
    /// Syntactically valid, but uses a tool this decoder does not implement.
    Unsupported(&'static str),
    /// A required parameter set was not supplied or referred to an unknown id.
    MissingParameterSet(&'static str),
    /// The picture dimensions would not fit in memory addressable by `usize`.
    TooLarge,
    /// No coded slice NAL units were supplied.
    NoSlices,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Truncated => f.write_str("bitstream truncated"),
            Error::InvalidData(m) => {
                f.write_str("invalid bitstream: ")?;
                f.write_str(m)
            }
            Error::Unsupported(m) => {
                f.write_str("unsupported feature: ")?;
                f.write_str(m)
            }
            Error::MissingParameterSet(m) => {
                f.write_str("missing parameter set: ")?;
                f.write_str(m)
            }
            Error::TooLarge => f.write_str("picture too large"),
            Error::NoSlices => f.write_str("no coded slice NAL units supplied"),
        }
    }
}

impl From<Error> for crate::Error {
    fn from(e: Error) -> crate::Error {
        match e {
            // The container's `Truncated` names the structure that ran out;
            // for the codec that structure is the whole bitstream.
            Error::Truncated => crate::Error::Truncated("an HEVC bitstream"),
            Error::InvalidData(m) => crate::Error::Malformed(m),
            // Verbatim: these strings name the exact coding tool that was
            // asked for, and callers grep them.
            Error::Unsupported(m) => crate::Error::Unsupported(m),
            Error::MissingParameterSet(m) => crate::Error::MissingParameterSet(m),
            Error::TooLarge => crate::Error::Malformed("the coded picture is too large to address"),
            Error::NoSlices => crate::Error::Malformed("the coded item carries no slice NAL units"),
        }
    }
}

/// Shorthand used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;
