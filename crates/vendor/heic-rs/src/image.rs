//! The public output types: what a decode produces and how to ask for it.

use alloc::vec::Vec;

use crate::error::{Error, Result};

/// The default ceiling on output size, 256 megapixels.
///
/// A HEIF file declares its dimensions in metadata, so a small file can ask a
/// decoder for an enormous allocation. Every decode is checked against
/// [`DecodeOptions::max_pixels`], which starts here. It is stated as a
/// constant rather than buried in a `Default` body so that callers can quote
/// it, compare against it, and raise it deliberately.
pub const DEFAULT_MAX_PIXELS: u64 = 268_435_456;

/// How samples are arranged in [`Image::data`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PixelLayout {
    /// Three 8-bit channels, red first.
    #[default]
    Rgb8,
    /// Four 8-bit channels, red first, alpha last.
    Rgba8,
    /// Three 8-bit channels, blue first.
    Bgr8,
    /// Four 8-bit channels, blue first, alpha last.
    Bgra8,
    /// One 8-bit channel of luma.
    Gray8,
    /// Three 16-bit native-endian channels, red first.
    Rgb16,
    /// Four 16-bit native-endian channels, red first, alpha last.
    Rgba16,
}

impl PixelLayout {
    /// Channels per pixel.
    pub const fn channels(self) -> usize {
        match self {
            PixelLayout::Gray8 => 1,
            PixelLayout::Rgb8 | PixelLayout::Bgr8 | PixelLayout::Rgb16 => 3,
            PixelLayout::Rgba8 | PixelLayout::Bgra8 | PixelLayout::Rgba16 => 4,
        }
    }

    /// Bytes per channel: 1 for the 8-bit layouts, 2 for the 16-bit ones.
    pub const fn bytes_per_channel(self) -> usize {
        match self {
            PixelLayout::Rgb16 | PixelLayout::Rgba16 => 2,
            _ => 1,
        }
    }

    /// Bytes per pixel.
    pub const fn bytes_per_pixel(self) -> usize {
        self.channels() * self.bytes_per_channel()
    }

    /// True when the layout has an alpha channel to fill.
    pub const fn has_alpha(self) -> bool {
        matches!(
            self,
            PixelLayout::Rgba8 | PixelLayout::Bgra8 | PixelLayout::Rgba16
        )
    }

    /// True when the red and blue channels are exchanged.
    pub const fn is_bgr(self) -> bool {
        matches!(self, PixelLayout::Bgr8 | PixelLayout::Bgra8)
    }

    /// True when the layout carries no colour.
    pub const fn is_gray(self) -> bool {
        matches!(self, PixelLayout::Gray8)
    }
}

/// A decoded image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Tightly packed samples: `width * bytes_per_pixel` per row, no padding.
    pub data: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// How to read [`data`](Image::data).
    pub layout: PixelLayout,
}

impl Image {
    /// Allocate a zeroed image, refusing sizes past `max_pixels`.
    pub fn zeroed(width: u32, height: u32, layout: PixelLayout, max_pixels: u64) -> Result<Image> {
        let pixels = check_pixels(width, height, max_pixels)?;
        let bytes = pixels
            .checked_mul(layout.bytes_per_pixel() as u64)
            .and_then(|b| usize::try_from(b).ok())
            .ok_or(Error::PixelLimit { pixels, max_pixels })?;
        Ok(Image {
            data: alloc::vec![0u8; bytes],
            width,
            height,
            layout,
        })
    }

    /// Bytes in one row.
    pub const fn row_bytes(&self) -> usize {
        self.width as usize * self.layout.bytes_per_pixel()
    }

    /// Number of pixels.
    pub const fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

/// Multiply out a size, refusing anything past the limit before it becomes an
/// allocation.
pub fn check_pixels(width: u32, height: u32, max_pixels: u64) -> Result<u64> {
    if width == 0 || height == 0 {
        return Err(Error::Malformed("image has a zero dimension"));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > max_pixels {
        return Err(Error::PixelLimit { pixels, max_pixels });
    }
    Ok(pixels)
}

/// What the caller wants out of a decode.
///
/// The fields are public and the struct is exhaustive, so
/// `DecodeOptions { layout, ..Default::default() }` works. Builder methods are
/// provided for the same reason: they are the form that keeps compiling when a
/// future release grows another option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeOptions {
    /// The pixel layout to produce.
    pub layout: PixelLayout,
    /// The largest image that will be produced, in pixels.
    ///
    /// `None` removes the ceiling entirely, which is only safe for input you
    /// produced yourself. [`Default`] sets this to
    /// `Some(DEFAULT_MAX_PIXELS)`.
    pub max_pixels: Option<u64>,
    /// Apply `clap`, `irot` and `imir`. When false the coded orientation is
    /// returned and the caller is expected to honour it.
    pub apply_transforms: bool,
    /// Decode an auxiliary alpha plane when one is present and the requested
    /// layout has an alpha channel.
    pub decode_alpha: bool,
    /// Refuse files that carry an essential property this crate cannot act on,
    /// rather than ignoring it.
    pub strict: bool,
    /// How many threads to decode on.
    ///
    /// `None`, the default, uses `rayon`'s global pool — or, if the call is
    /// already inside a `rayon` pool, that pool, which is what you want:
    /// the work joins the pool you are in rather than competing with it.
    ///
    /// `Some(1)` runs the serial code itself, not a one-worker pool, so it is
    /// exactly what a build without the `parallel` feature does.
    ///
    /// `Some(n)` builds a private pool of `n` threads for the call. Inside a
    /// pool of your own that is a *nested* pool, and the two will
    /// oversubscribe the machine; pass `None` there, or call
    /// [`decode`](crate::decode()) from outside your pool.
    ///
    /// Without the `parallel` feature every value behaves like `Some(1)`. The
    /// pixels are identical in every case.
    pub threads: Option<usize>,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        DecodeOptions {
            layout: PixelLayout::Rgb8,
            max_pixels: Some(DEFAULT_MAX_PIXELS),
            apply_transforms: true,
            decode_alpha: true,
            strict: false,
            threads: None,
        }
    }
}

impl DecodeOptions {
    /// The effective pixel ceiling, with `None` meaning `u64::MAX`.
    pub const fn pixel_limit(&self) -> u64 {
        match self.max_pixels {
            Some(n) => n,
            None => u64::MAX,
        }
    }

    /// Builder: choose the output layout.
    pub fn with_layout(mut self, layout: PixelLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Builder: choose the pixel ceiling.
    pub fn with_max_pixels(mut self, max_pixels: Option<u64>) -> Self {
        self.max_pixels = max_pixels;
        self
    }

    /// Builder: apply or ignore `clap`, `irot` and `imir`.
    pub fn with_transforms(mut self, apply: bool) -> Self {
        self.apply_transforms = apply;
        self
    }

    /// Builder: decode or skip an auxiliary alpha plane.
    pub fn with_alpha(mut self, decode: bool) -> Self {
        self.decode_alpha = decode;
        self
    }

    /// Builder: refuse files carrying an essential property this build does
    /// not implement.
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Builder: choose the thread count. See [`DecodeOptions::threads`].
    pub fn with_threads(mut self, threads: Option<usize>) -> Self {
        self.threads = threads;
        self
    }
}
