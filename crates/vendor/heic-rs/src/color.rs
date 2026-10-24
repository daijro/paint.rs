//! Turning decoded YCbCr samples into the caller's chosen pixel layout.
//!
//! The matrix and the sample range both come from the file's `colr` property.
//! When a file carries no `nclx` `colr`, `decode` takes them from the HEVC VUI
//! and the MIAF defaults (BT.601, full range) instead (paint.rs patch, Ruling
//! R52; upstream used BT.709 limited range, [`Nclx::default`]).
//!
//! # How it is arranged
//!
//! The matrix is integer, not floating point: the private `fixed` module folds
//! the sample scaling, the matrix and the output maximum into five `i32`
//! coefficients, so a channel costs one multiply-accumulate, one rounded shift
//! and one clamp. The chroma upsampler is fused into the walk: a band holds a
//! three-slot ring of horizontally expanded chroma *rows*, never a chroma
//! plane, and blends them vertically inside the colour loop, so each chroma
//! row is expanded once. The loops themselves live in the private `kernel`
//! module, one compiled per pixel layout, and none of them branches on the
//! layout or indexes a plane per sample. A grid's tiles are read in place
//! through the private `source` module rather than composed first.

mod chroma;
mod fixed;
mod kernel;
mod source;

use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::hevc::{ChromaFormat, Frame};
use crate::image::{Image, PixelLayout, check_pixels};
use crate::parallel;
use crate::props::colr::Nclx;
use fixed::{AlphaScale, Coeffs};
use kernel::Mode;
pub(crate) use source::Source;

/// The sample depths this conversion is defined for.
const DEPTHS: core::ops::RangeInclusive<u8> = 8..=16;

/// Convert a frame, and optionally an alpha plane, into an image.
///
/// `alpha` is the luma plane of a decoded auxiliary item, at the same size as
/// `frame`; it is used only when `layout` has an alpha channel.
///
/// # Accuracy
///
/// The arithmetic is fixed point. Every channel is within **1 least
/// significant bit** of the same conversion carried out in `f32` and rounded
/// once at the end — the coefficients carry 16 fractional bits for the 8-bit
/// layouts and 13 for the 16-bit ones, which puts the coefficient error three
/// orders of magnitude below one output step, so the only difference that
/// survives is a rounding tie. `tests/color_fixed.rs` holds a float reference
/// implementation and asserts that bound over a randomised sweep of depths,
/// ranges, matrices and layouts.
pub fn convert(
    frame: &Frame,
    alpha: Option<&Frame>,
    nclx: Nclx,
    layout: PixelLayout,
    max_pixels: u64,
    threads: Option<usize>,
) -> Result<Image> {
    frame.validate()?;
    convert_source(
        &Source::Frame(frame),
        alpha,
        nclx,
        layout,
        max_pixels,
        threads,
    )
}

/// [`convert`] for any row source: a validated frame, or a grid's tiles read
/// in place through [`crate::grid::Mosaic`], which checked them when it was
/// built.
pub(crate) fn convert_source(
    src: &Source<'_>,
    alpha: Option<&Frame>,
    nclx: Nclx,
    layout: PixelLayout,
    max_pixels: u64,
    threads: Option<usize>,
) -> Result<Image> {
    depth_ok(src.bit_depth())?;
    let (w, h) = (src.width(), src.height());
    check_pixels(w, h, max_pixels)?;
    let mut image = Image::zeroed(w, h, layout, max_pixels)?;

    let wide = layout.bytes_per_channel() == 2;
    let coeffs = Coeffs::new(nclx, src.bit_depth(), wide);
    let mode = if src.chroma() == ChromaFormat::Monochrome || layout.is_gray() {
        Mode::Luma
    } else if nclx.matrix.is_identity() {
        Mode::Identity
    } else {
        Mode::Matrix
    };
    run(src, &coeffs, mode, &mut image, threads);

    if let Some(a) = alpha.filter(|_| layout.has_alpha()) {
        depth_ok(a.bit_depth)?;
        let scale = AlphaScale::new(a.bit_depth, coeffs.max_out);
        kernel::alpha(
            &mut image.data,
            a,
            w as usize,
            h as usize,
            layout.bytes_per_pixel(),
            &scale,
        );
    }
    Ok(image)
}

/// Refuse a depth the fixed-point coefficients are not defined for, rather
/// than shifting past the width of the accumulator.
fn depth_ok(depth: u8) -> Result<()> {
    if DEPTHS.contains(&depth) {
        return Ok(());
    }
    Err(Error::Unsupported("sample bit depth is outside 8 to 16"))
}

/// Pick the loop for this layout. One instantiation per layout, chosen here
/// and never re-examined inside a row.
fn run(src: &Source<'_>, c: &Coeffs, mode: Mode, image: &mut Image, threads: Option<usize>) {
    let out = &mut image.data;
    match image.layout {
        PixelLayout::Gray8 => bands::<1, false, false>(src, c, mode, out, threads),
        PixelLayout::Rgb8 => bands::<3, false, false>(src, c, mode, out, threads),
        PixelLayout::Bgr8 => bands::<3, true, false>(src, c, mode, out, threads),
        PixelLayout::Rgba8 => bands::<4, false, false>(src, c, mode, out, threads),
        PixelLayout::Bgra8 => bands::<4, true, false>(src, c, mode, out, threads),
        PixelLayout::Rgb16 => bands::<6, false, true>(src, c, mode, out, threads),
        PixelLayout::Rgba16 => bands::<8, false, true>(src, c, mode, out, threads),
    }
}

/// Rows per band handed to one thread.
///
/// Swept with `benches/decode.rs`'s `color_threads` group on an Apple M3 Max,
/// converting 2048x1536 4:2:0 to RGB8. The pooled time rises monotonically
/// with the band — 16 rows is the fastest measured, and 32, 64, 128 and 256
/// are each slower than the one before — because a wide band both overflows
/// L2 and leaves the pool too few pieces to balance its tail with. Below
/// about twelve rows that reverses and per-band dispatch starts to cost more
/// than the rows do, so the curve has a floor rather than a slope, and 16
/// sits on it. Re-run that group before changing this.
const ROWS_PER_BAND: usize = 16;

/// Below this many pixels the conversion runs on the calling thread whatever
/// the caller asked for.
///
/// Measured whole-file, not in isolation, because the two disagree. On its
/// own, converting 512x512 4:2:0 to RGB8 takes 181 us serially and 50 us on a
/// warm pool; but in a decode of a single coded picture the pool has been
/// asleep for the whole of the codec's run, and waking it costs more than
/// the split saves: `gradient-512.heic` (262144 pixels) decodes in 2.04 ms
/// with colour serial and 2.13 to 2.20 ms with it pooled, on an Apple M3 Max
/// with 16 threads — and the README's own quiet-machine table shows the same
/// sign, 2.04 against 2.07. A 768x768 single picture (589824 pixels) is the
/// smallest measured that gains whole-file: 1.27 ms serial against 1.08 ms
/// pooled. The floor is that size. Grids are not affected in practice: their
/// tiles have just run on the pool, so it is awake, and any grid is larger
/// than this anyway.
const PARALLEL_FLOOR_PIXELS: u64 = 768 * 768;

/// Convert the image in row bands, on as many threads as the caller allowed.
///
/// Each band gets its own scratch — the upsampled chroma rows, and a mosaic's
/// row-assembly space — since that is the only mutable state a band carries
/// between its rows.
fn bands<const N: usize, const BGR: bool, const WIDE: bool>(
    src: &Source<'_>,
    c: &Coeffs,
    mode: Mode,
    out: &mut [u8],
    threads: Option<usize>,
) {
    let w = src.width() as usize;
    let scratch_len = kernel::scratch_len(src, mode);
    let small = u64::from(src.width()) * u64::from(src.height()) < PARALLEL_FLOOR_PIXELS;
    let threads = if small { Some(1) } else { threads };
    parallel::for_each_band(out, w * N, ROWS_PER_BAND, threads, |y0, dst| {
        let mut scratch: Vec<u16> = alloc::vec![0u16; scratch_len];
        kernel::planes::<N, BGR, WIDE>(src, c, mode, &mut scratch, dst, y0);
    });
}
