//! Chroma upsampling.
//!
//! # Which filter
//!
//! Horizontally, chroma in HEVC is left-sited: chroma sample `i` sits on luma
//! column `2i`. Even output columns are therefore a copy, and odd ones are the
//! average of the two neighbours — plain bilinear.
//!
//! Vertically, 4:2:0 chroma sits halfway between two luma rows, so the two
//! rows a chroma row feeds are a 3:1 blend of it and its neighbour. 4:2:2
//! needs no vertical work at all, and 4:4:4 needs none in either direction.
//!
//! This is a clean, cheap, standards-sited bilinear. It is not a windowed-sinc
//! or edge-directed upsampler; if that matters for your use it is the obvious
//! place to improve, and the filter is isolated here so that it can be.
//!
//! # Working a row at a time
//!
//! [`row`] expands one output row from the one or two chroma rows that feed
//! it, and is the only place the filter is written. [`plane`] is that function
//! run over every row into a full-resolution buffer; [`crate::color`] calls it
//! into a single reused row instead, so that a conversion never materialises a
//! full-resolution chroma plane. Both therefore produce bit-identical output.

use alloc::vec::Vec;

use crate::hevc::ChromaFormat;

/// Which chroma rows one output row draws from.
///
/// Returns the two source row indices and the weight, out of four, that the
/// first of them carries. For 4:2:2 and 4:4:4 the two are the same row and the
/// weight is four, so the blend is a copy.
pub fn row_pair(y: usize, ch: usize, chroma: ChromaFormat) -> (usize, usize, u32) {
    if ch == 0 {
        return (0, 0, 4);
    }
    if chroma.y_shift() == 0 {
        let r = core::cmp::min(y, ch - 1);
        return (r, r, 4);
    }
    let cy = core::cmp::min(y >> 1, ch - 1);
    let near = if y & 1 == 0 {
        cy.saturating_sub(1)
    } else {
        core::cmp::min(cy + 1, ch - 1)
    };
    (cy, near, 3)
}

/// Expand one chroma row pair into `dst`, one sample per output column.
///
/// `src0` and `src1` are the two chroma rows named by [`row_pair`], each
/// exactly as wide as the chroma plane, and `w0` the first one's weight.
/// `x_shift` is [`ChromaFormat::x_shift`]: zero copies columns, one
/// interpolates between them.
pub fn row(dst: &mut [u16], src0: &[u16], src1: &[u16], w0: u32, x_shift: u32) {
    let n = core::cmp::min(src0.len(), src1.len());
    if n == 0 || dst.is_empty() {
        return;
    }
    let done = if x_shift == 0 {
        direct(dst, &src0[..n], &src1[..n], w0)
    } else {
        bilinear(dst, &src0[..n], &src1[..n], w0)
    };
    // Anything past the last chroma column clamps to it, which is the value
    // the two edge cases above have already worked out.
    if done < dst.len() {
        let edge = vblend(src0[n - 1], src1[n - 1], w0);
        for d in &mut dst[done..] {
            *d = edge;
        }
    }
}

/// One output column per chroma column: 4:4:4, and 4:2:2 vertically.
fn direct(dst: &mut [u16], src0: &[u16], src1: &[u16], w0: u32) -> usize {
    let n = core::cmp::min(dst.len(), src0.len());
    for (d, (a, b)) in dst[..n].iter_mut().zip(src0[..n].iter().zip(&src1[..n])) {
        *d = vblend(*a, *b, w0);
    }
    n
}

/// Two output columns per chroma column: the left-sited bilinear.
///
/// The body covers every chroma column that has a right-hand neighbour, so the
/// index never needs clamping inside the loop; the last one or two columns are
/// left to the caller's edge fill.
fn bilinear(dst: &mut [u16], src0: &[u16], src1: &[u16], w0: u32) -> usize {
    let body = core::cmp::min(src0.len() - 1, dst.len() / 2);
    for (i, px) in dst.chunks_exact_mut(2).take(body).enumerate() {
        let (a0, a1) = (src0[i], src0[i + 1]);
        let (b0, b1) = (src1[i], src1[i + 1]);
        px[0] = vblend(a0, b0, w0);
        px[1] = vblend(havg(a0, a1), havg(b0, b1), w0);
    }
    body * 2
}

/// Expand one chroma row horizontally only, one sample per output column.
///
/// This is the horizontal half of [`row`]: `row` is exactly [`vblend`]
/// applied, column by column, to `horizontal` of each of its two source rows.
/// [`crate::color`] uses it to expand each chroma row once and blend
/// vertically inside its own loop, rather than expanding every chroma row
/// twice — once for each luma row it feeds.
pub fn horizontal(dst: &mut [u16], src: &[u16], x_shift: u32) {
    let n = src.len();
    if n == 0 || dst.is_empty() {
        return;
    }
    let done = if x_shift == 0 {
        let m = core::cmp::min(dst.len(), n);
        dst[..m].copy_from_slice(&src[..m]);
        m
    } else {
        let body = core::cmp::min(n - 1, dst.len() / 2);
        for (i, px) in dst.chunks_exact_mut(2).take(body).enumerate() {
            px[0] = src[i];
            px[1] = havg(src[i], src[i + 1]);
        }
        body * 2
    };
    for d in &mut dst[done..] {
        *d = src[n - 1];
    }
}

/// Blend two vertically adjacent samples, `w0` parts of four from the first.
#[inline(always)]
pub fn vblend(a: u16, b: u16, w0: u32) -> u16 {
    ((u32::from(a) * w0 + u32::from(b) * (4 - w0) + 2) >> 2) as u16
}

/// The midpoint of two horizontally adjacent samples, rounded up.
#[inline(always)]
fn havg(a: u16, b: u16) -> u16 {
    ((u32::from(a) + u32::from(b) + 1) >> 1) as u16
}

/// Expand one chroma plane to full luma resolution.
///
/// `cw` and `ch` are the chroma plane's dimensions, `stride` its row length in
/// samples, and `w`/`h` the luma dimensions to produce. Rows the source is too
/// short to supply are left zeroed; [`crate::hevc::Frame::validate`] refuses
/// such a plane before the conversion ever reaches here.
pub fn plane(
    src: &[u16],
    stride: u32,
    cw: u32,
    ch: u32,
    w: u32,
    h: u32,
    chroma: ChromaFormat,
) -> Vec<u16> {
    let (w, h, cw, ch) = (w as usize, h as usize, cw as usize, ch as usize);
    let mut out = alloc::vec![0u16; w * h];
    if cw == 0 || ch == 0 {
        return out;
    }
    let x_shift = chroma.x_shift();
    for y in 0..h {
        let (r0, r1, w0) = row_pair(y, ch, chroma);
        let (o0, o1) = (r0 * stride as usize, r1 * stride as usize);
        let (Some(s0), Some(s1)) = (src.get(o0..o0 + cw), src.get(o1..o1 + cw)) else {
            break;
        };
        let Some(dst) = out.get_mut(y * w..y * w + w) else {
            break;
        };
        row(dst, s0, s1, w0, x_shift);
    }
    out
}
