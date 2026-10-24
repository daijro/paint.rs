//! The per-row conversion loops.
//!
//! Each loop slices its row out of the plane once and then walks it with
//! iterators, so there is no bounds check per sample and nothing in the body
//! branches on the pixel layout: the layout is a set of const parameters, and
//! one loop is compiled per layout.

use crate::color::chroma::Ring;
use crate::color::fixed::{AlphaScale, Coeffs};
use crate::color::source::Source;
use crate::hevc::Frame;
use crate::upsample;

/// Which of the three conversions a frame needs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Luma only: a monochrome frame, or a grey output layout.
    Luma,
    /// Matrix 0: the planes already hold G, B and R on the luma scale.
    Identity,
    /// A real matrix, with chroma on its own scale.
    Matrix,
}

/// Samples the chroma ring needs: none when no chroma is read.
fn ring_len(width: usize, mode: Mode) -> usize {
    if mode == Mode::Luma {
        0
    } else {
        Ring::len(width)
    }
}

/// Scratch samples one band needs: the expanded chroma rows unless the
/// conversion is luma-only, plus whatever the source needs to assemble rows.
pub(crate) fn scratch_len(src: &Source<'_>, mode: Mode) -> usize {
    ring_len(src.width() as usize, mode) + src.stitch_len()
}

/// Convert a band of rows of `src` into `out`.
///
/// `N` is bytes per output pixel, `BGR` swaps red and blue, and `WIDE` selects
/// 16-bit channels. `out` holds the output rows starting at `y0`, and its
/// length says how many there are. `scratch` is [`scratch_len`] samples: a
/// [`Ring`] of horizontally expanded chroma rows, reused down the band, which
/// is what keeps a full-resolution chroma plane from ever existing, followed
/// by the source's own row-assembly space. The vertical blend between two
/// expanded rows happens inside the colour loop itself.
///
/// A band depends on nothing but the source planes, so bands may be converted
/// in any order or at the same time; `crate::parallel` is what decides.
pub(crate) fn planes<const N: usize, const BGR: bool, const WIDE: bool>(
    src: &Source<'_>,
    c: &Coeffs,
    mode: Mode,
    scratch: &mut [u16],
    out: &mut [u8],
    y0: usize,
) {
    let w = src.width() as usize;
    let chroma = src.chroma();
    let ch = chroma.chroma_size(src.width(), src.height()).1 as usize;
    let x_shift = chroma.x_shift();
    let (ring, stitch) = scratch.split_at_mut(core::cmp::min(ring_len(w, mode), scratch.len()));
    let mut ring = Ring::new(ring, w);
    let (luma_buf, chroma_buf) = stitch.split_at_mut(core::cmp::min(w, stitch.len()));
    let rows = out.len().checked_div(w * N).unwrap_or(0);
    for band_row in 0..rows {
        let y = y0 + band_row;
        let Some(dst) = out.get_mut(band_row * w * N..(band_row + 1) * w * N) else {
            return;
        };
        let Some(luma) = src.luma(y, luma_buf) else {
            return;
        };
        if mode == Mode::Luma {
            row_luma::<N, BGR, WIDE>(dst, luma, c);
            continue;
        }
        let (r0, r1, w0) = upsample::row_pair(y, ch, chroma);
        let (Some(s0), Some(s1)) = (
            ring.slot(src, r0, x_shift, chroma_buf),
            ring.slot(src, r1, x_shift, chroma_buf),
        ) else {
            return;
        };
        let (Some((cb0, cr0)), Some((cb1, cr1))) = (ring.get(s0), ring.get(s1)) else {
            return;
        };
        let rows = [cb0, cb1, cr0, cr1];
        if mode == Mode::Identity {
            row_identity::<N, BGR, WIDE>(dst, luma, rows, w0, c);
        } else {
            row_matrix::<N, BGR, WIDE>(dst, luma, rows, w0, c);
        }
    }
}

/// Store one pixel. `a` is already in output units.
#[inline(always)]
fn store<const N: usize, const BGR: bool, const WIDE: bool>(
    px: &mut [u8; N],
    r: i32,
    g: i32,
    b: i32,
    a: i32,
) {
    let (first, third) = if BGR { (b, r) } else { (r, b) };
    let vals = [first, g, third, a];
    if WIDE {
        for (out, v) in px.chunks_exact_mut(2).zip(&vals) {
            out.copy_from_slice(&(*v as u16).to_ne_bytes());
        }
    } else {
        for (out, v) in px.iter_mut().zip(&vals) {
            *out = *v as u8;
        }
    }
}

/// Walk two expanded chroma rows of each plane, blending them vertically
/// into one `(cb, cr)` sample per column.
#[inline(always)]
fn blended<'a>(rows: [&'a [u16]; 4], w0: u32) -> impl Iterator<Item = (u16, u16)> + 'a {
    let [cb0, cb1, cr0, cr1] = rows;
    cb0.iter()
        .zip(cb1)
        .zip(cr0.iter().zip(cr1))
        .map(move |((a0, a1), (b0, b1))| {
            (
                upsample::vblend(*a0, *a1, w0),
                upsample::vblend(*b0, *b1, w0),
            )
        })
}

/// Grey: one luma sample becomes all three channels.
fn row_luma<const N: usize, const BGR: bool, const WIDE: bool>(
    dst: &mut [u8],
    luma: &[u16],
    c: &Coeffs,
) {
    for (px, y) in dst.chunks_exact_mut(N).zip(luma) {
        let Ok(px) = <&mut [u8; N]>::try_from(px) else {
            return;
        };
        let v = c.finish((i32::from(*y) - c.y_offset) * c.ky);
        store::<N, BGR, WIDE>(px, v, v, v, c.max_out);
    }
}

/// Matrix 0: luma is green, Cb is blue and Cr is red, all on the luma scale.
fn row_identity<const N: usize, const BGR: bool, const WIDE: bool>(
    dst: &mut [u8],
    luma: &[u16],
    rows: [&[u16]; 4],
    w0: u32,
    c: &Coeffs,
) {
    for (px, (y, (u, v))) in dst
        .chunks_exact_mut(N)
        .zip(luma.iter().zip(blended(rows, w0)))
    {
        let Ok(px) = <&mut [u8; N]>::try_from(px) else {
            return;
        };
        let g = c.finish((i32::from(*y) - c.y_offset) * c.ky);
        let b = c.finish((i32::from(u) - c.y_offset) * c.ky);
        let r = c.finish((i32::from(v) - c.y_offset) * c.ky);
        store::<N, BGR, WIDE>(px, r, g, b, c.max_out);
    }
}

/// The usual case: one multiply-accumulate per channel.
fn row_matrix<const N: usize, const BGR: bool, const WIDE: bool>(
    dst: &mut [u8],
    luma: &[u16],
    rows: [&[u16]; 4],
    w0: u32,
    c: &Coeffs,
) {
    for (px, (y, (cbv, crv))) in dst
        .chunks_exact_mut(N)
        .zip(luma.iter().zip(blended(rows, w0)))
    {
        let Ok(px) = <&mut [u8; N]>::try_from(px) else {
            return;
        };
        let yy = (i32::from(*y) - c.y_offset) * c.ky;
        let u = i32::from(cbv) - c.c_offset;
        let v = i32::from(crv) - c.c_offset;
        let r = c.finish(yy + c.rv * v);
        let g = c.finish(yy + c.gu * u + c.gv * v);
        let b = c.finish(yy + c.bu * u);
        store::<N, BGR, WIDE>(px, r, g, b, c.max_out);
    }
}

/// Fill the alpha channel from a decoded auxiliary plane.
///
/// A second pass over the image, so that the colour loops never branch on
/// whether there is an alpha plane. Samples the plane cannot supply are left
/// as the colour pass wrote them, which is fully opaque.
pub(crate) fn alpha(out: &mut [u8], frame: &Frame, w: usize, h: usize, bpp: usize, s: &AlphaScale) {
    let wide = bpp == 8;
    let off = bpp - if wide { 2 } else { 1 };
    let stride = frame.y_stride as usize;
    for y in 0..h {
        let Some(dst) = out.get_mut(y * w * bpp..(y + 1) * w * bpp) else {
            return;
        };
        let Some(src) = frame.y.get(y * stride..) else {
            return;
        };
        for (px, v) in dst.chunks_exact_mut(bpp).zip(src.iter().take(w)) {
            let a = s.apply(*v);
            if wide {
                let Some(slot) = px.get_mut(off..off + 2) else {
                    return;
                };
                slot.copy_from_slice(&(a as u16).to_ne_bytes());
            } else if let Some(slot) = px.get_mut(off) {
                *slot = a as u8;
            }
        }
    }
}
