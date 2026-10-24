//! The small descriptive properties: `ispe`, `pixi`, `pasp`, `auxC`, and the
//! three transformative ones, `irot`, `imir` and `clap`.

use alloc::vec::Vec;

use crate::boxes::BoxHeader;
use crate::error::{Error, Result};

/// `ispe`: the coded size of an item, before any transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ispe {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Parse `ispe`.
pub fn parse_ispe(b: &BoxHeader<'_>) -> Result<Ispe> {
    let (_v, _f, mut r) = b.full_box("ispe")?;
    let width = r.u32("ispe width")?;
    let height = r.u32("ispe height")?;
    if width == 0 || height == 0 {
        return Err(Error::Malformed("ispe declares a zero dimension"));
    }
    Ok(Ispe { width, height })
}

/// `pixi`: bits per channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixi {
    /// One entry per channel, in coding order.
    pub bits_per_channel: Vec<u8>,
}

/// Parse `pixi`.
pub fn parse_pixi(b: &BoxHeader<'_>) -> Result<Pixi> {
    let (_v, _f, mut r) = b.full_box("pixi")?;
    let n = r.u8("pixi channel count")?;
    if usize::from(n) > r.remaining() {
        return Err(Error::Malformed(
            "pixi declares more channels than it holds",
        ));
    }
    let mut bits_per_channel = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        bits_per_channel.push(r.u8("pixi channel depth")?);
    }
    Ok(Pixi { bits_per_channel })
}

/// `pasp`: the pixel aspect ratio. Reported, never applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pasp {
    /// Horizontal spacing.
    pub h_spacing: u32,
    /// Vertical spacing.
    pub v_spacing: u32,
}

/// Parse `pasp`.
pub fn parse_pasp(b: &BoxHeader<'_>) -> Result<Pasp> {
    let mut r = b.reader();
    Ok(Pasp {
        h_spacing: r.u32("pasp hSpacing")?,
        v_spacing: r.u32("pasp vSpacing")?,
    })
}

/// `irot`: a counter-clockwise rotation in whole quarter turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// No rotation.
    #[default]
    None,
    /// 90 degrees counter-clockwise.
    Ccw90,
    /// 180 degrees.
    Ccw180,
    /// 270 degrees counter-clockwise, which is 90 clockwise.
    Ccw270,
}

impl Rotation {
    /// The rotation in degrees counter-clockwise.
    pub const fn degrees(self) -> u16 {
        match self {
            Rotation::None => 0,
            Rotation::Ccw90 => 90,
            Rotation::Ccw180 => 180,
            Rotation::Ccw270 => 270,
        }
    }

    /// True when applying this rotation exchanges width and height.
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Ccw90 | Rotation::Ccw270)
    }
}

/// Parse `irot`.
pub fn parse_irot(b: &BoxHeader<'_>) -> Result<Rotation> {
    let mut r = b.reader();
    Ok(match r.u8("irot angle")? & 0x03 {
        0 => Rotation::None,
        1 => Rotation::Ccw90,
        2 => Rotation::Ccw180,
        _ => Rotation::Ccw270,
    })
}

/// `imir`: a mirroring, named by the effect rather than by the axis.
///
/// The coded field (`mode`, ISO/IEC 23008-12:2022): 0 exchanges top and
/// bottom; 1 exchanges left and right (paint.rs patch; see `parse_imir`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    /// Coded mode 1: left and right are exchanged.
    LeftRight,
    /// Coded mode 0: top and bottom are exchanged.
    TopBottom,
}

/// Parse `imir`.
pub fn parse_imir(b: &BoxHeader<'_>) -> Result<Mirror> {
    let mut r = b.reader();
    // paint.rs patch (Ruling R52): ISO/IEC 23008-12:2022 calls this field `mode`, and 0 exchanges
    // top and bottom, 1 left and right; libheif and libavif read it so. Upstream read 0 as
    // left-right (the 2017 text's "vertical axis").
    Ok(if r.u8("imir axis")? & 0x01 == 0 {
        Mirror::TopBottom
    } else {
        Mirror::LeftRight
    })
}

/// `clap`: the clean aperture, as the four rationals the box actually stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clap {
    /// Cropped width, numerator and denominator.
    pub width: (u32, u32),
    /// Cropped height, numerator and denominator.
    pub height: (u32, u32),
    /// Horizontal offset of the crop centre, numerator (signed) and denominator.
    pub horiz_off: (i32, u32),
    /// Vertical offset of the crop centre, numerator (signed) and denominator.
    pub vert_off: (i32, u32),
}

/// Parse `clap`.
pub fn parse_clap(b: &BoxHeader<'_>) -> Result<Clap> {
    let mut r = b.reader();
    let wn = r.u32("clap width numerator")?;
    let wd = r.u32("clap width denominator")?;
    let hn = r.u32("clap height numerator")?;
    let hd = r.u32("clap height denominator")?;
    let hon = r.u32("clap horizontal offset numerator")? as i32;
    let hod = r.u32("clap horizontal offset denominator")?;
    let von = r.u32("clap vertical offset numerator")? as i32;
    let vod = r.u32("clap vertical offset denominator")?;
    if wd == 0 || hd == 0 || hod == 0 || vod == 0 {
        return Err(Error::Malformed("clap has a zero denominator"));
    }
    Ok(Clap {
        width: (wn, wd),
        height: (hn, hd),
        horiz_off: (hon, hod),
        vert_off: (von, vod),
    })
}

/// What an auxiliary item carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxKind {
    /// An alpha plane.
    Alpha,
    /// A depth map.
    Depth,
    /// Something else; the URN is available from [`AuxC::urn`].
    Other,
}

/// `auxC`: what an auxiliary image is for.
#[derive(Debug, Clone, Copy)]
pub struct AuxC<'a> {
    /// The auxiliary type URN, verbatim.
    pub urn: &'a str,
    /// The URN, interpreted.
    pub kind: AuxKind,
}

/// Parse `auxC`.
pub fn parse_auxc<'a>(b: &BoxHeader<'a>) -> Result<AuxC<'a>> {
    let (_v, _f, mut r) = b.full_box("auxC")?;
    let urn = r.cstr("auxC aux type")?;
    let kind = if urn.ends_with("auxiliary:alpha") || urn == "urn:mpeg:hevc:2015:auxid:1" {
        AuxKind::Alpha
    } else if urn.ends_with("auxiliary:depth") || urn == "urn:mpeg:hevc:2015:auxid:2" {
        AuxKind::Depth
    } else {
        AuxKind::Other
    };
    Ok(AuxC { urn, kind })
}
