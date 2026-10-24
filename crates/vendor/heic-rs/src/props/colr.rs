//! `colr`: how to interpret the coded samples as colour.

use crate::boxes::BoxHeader;
use crate::error::{Error, Result};

/// Matrix coefficients, from ISO/IEC 23091-2 (formerly H.273).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatrixCoefficients {
    /// 0: the samples are already G, B, R.
    Identity,
    /// 1: ITU-R BT.709.
    Bt709,
    /// 2: unspecified. Treated as BT.709 with a note in the docs.
    Unspecified,
    /// 5 or 6: ITU-R BT.601 / SMPTE 170M.
    Bt601,
    /// 7: SMPTE 240M.
    Smpte240m,
    /// 9: ITU-R BT.2020 non-constant luminance.
    Bt2020Ncl,
    /// Anything else, kept verbatim.
    Other(u16),
}

impl MatrixCoefficients {
    /// Map the coded value onto a known matrix.
    pub fn from_code(code: u16) -> MatrixCoefficients {
        match code {
            0 => MatrixCoefficients::Identity,
            1 => MatrixCoefficients::Bt709,
            2 => MatrixCoefficients::Unspecified,
            5 | 6 => MatrixCoefficients::Bt601,
            7 => MatrixCoefficients::Smpte240m,
            9 => MatrixCoefficients::Bt2020Ncl,
            n => MatrixCoefficients::Other(n),
        }
    }

    /// The luma weights `(kr, kb)` this matrix uses.
    ///
    /// An unspecified or unknown matrix falls back to BT.709, which is what
    /// every HEIC writer this crate has seen actually means; the fallback is
    /// deliberate and documented rather than silent.
    pub fn luma_weights(self) -> (f32, f32) {
        match self {
            MatrixCoefficients::Bt601 => (0.299, 0.114),
            MatrixCoefficients::Smpte240m => (0.212, 0.087),
            MatrixCoefficients::Bt2020Ncl => (0.2627, 0.0593),
            _ => (0.2126, 0.0722),
        }
    }

    /// True when the samples need no matrix at all.
    pub fn is_identity(self) -> bool {
        matches!(self, MatrixCoefficients::Identity)
    }
}

/// Whether samples use the full coded range or the studio-swing subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    /// Luma 16..235 and chroma 16..240, scaled to the bit depth.
    Limited,
    /// The whole coded range.
    Full,
}

/// The `nclx` flavour of `colr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nclx {
    /// Colour primaries code.
    pub primaries: u16,
    /// Transfer characteristics code.
    pub transfer: u16,
    /// Matrix coefficients, already interpreted.
    pub matrix: MatrixCoefficients,
    /// The raw matrix coefficients code, as written.
    pub matrix_code: u16,
    /// Sample range.
    pub range: Range,
}

impl Default for Nclx {
    /// BT.709 limited range: what an unlabelled HEIC is in practice.
    ///
    /// This is the documented fallback used when a file carries no `colr`,
    /// not a silent default inside a parser.
    fn default() -> Self {
        Nclx {
            primaries: 1,
            transfer: 13,
            matrix: MatrixCoefficients::Bt709,
            matrix_code: 1,
            range: Range::Limited,
        }
    }
}

/// The contents of a `colr` box.
#[derive(Debug, Clone, Copy)]
pub enum ColorInfo<'a> {
    /// Coded colour description.
    Nclx(Nclx),
    /// An embedded ICC profile.
    Icc {
        /// True for `rICC` (a restricted profile), false for `prof`.
        restricted: bool,
        /// The profile bytes, borrowed from the file.
        data: &'a [u8],
    },
}

/// Parse a `colr` property box.
pub fn parse<'a>(b: &BoxHeader<'a>) -> Result<ColorInfo<'a>> {
    let mut r = b.reader();
    let kind = r.fourcc("colr colour type")?;
    match &kind {
        b"nclx" => {
            let primaries = r.u16("colr primaries")?;
            let transfer = r.u16("colr transfer")?;
            let matrix_code = r.u16("colr matrix")?;
            let flags = r.u8("colr range flag")?;
            let range = if flags & 0x80 != 0 {
                Range::Full
            } else {
                Range::Limited
            };
            Ok(ColorInfo::Nclx(Nclx {
                primaries,
                transfer,
                matrix: MatrixCoefficients::from_code(matrix_code),
                matrix_code,
                range,
            }))
        }
        b"rICC" => Ok(ColorInfo::Icc {
            restricted: true,
            data: r.rest(),
        }),
        b"prof" => Ok(ColorInfo::Icc {
            restricted: false,
            data: r.rest(),
        }),
        b"nclc" => Err(Error::Unsupported(
            "the `nclc` colour type belongs to QuickTime, not HEIF",
        )),
        _ => Err(Error::Malformed("unknown colr colour type")),
    }
}
