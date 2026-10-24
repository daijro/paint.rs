//! Reference sample gathering and substitution (clauses 8.4.4.2.1 and .2.2).

use super::state::Dec;
use crate::hevc::intra::Refs;

/// Collects the neighbouring samples of a transform block and substitutes
/// the unavailable ones.
///
/// `x` and `y` are in the coordinates of component `c_idx`, `n` is `nTbS`.
///
/// `Dec::available` reads its argument only through `z`, which is indexed in
/// minimum transform blocks, and through the CTB address, which is coarser
/// still. Every reference sample inside one minimum transform block therefore
/// gets the same answer, so the three runs — left column, corner, top row —
/// are walked one such block at a time: one clause 6.4.1 derivation per
/// block, and the samples of an available block copied as a run rather than
/// addressed one by one.
pub fn gather_refs(d: &Dec<'_>, x: usize, y: usize, n: usize, c_idx: usize) -> Refs {
    let (sw, sh) = if c_idx == 0 {
        (1usize, 1usize)
    } else {
        (d.sps.sub_w, d.sps.sub_h)
    };
    let plane = match c_idx {
        0 => &d.pic.y,
        1 => &d.pic.cb,
        _ => &d.pic.cr,
    };
    let bit_depth = if c_idx == 0 {
        d.sps.bit_depth_y
    } else {
        d.sps.bit_depth_c
    };
    let (xc, yc) = (x * sw, y * sh);
    let total = 4 * n + 1;
    let (stride, data) = (plane.stride, plane.data.as_slice());
    let mut r = Refs::new(n);
    let mut avail = [false; 129];
    // One minimum transform block spans this many samples of this component
    // along each axis; never zero, as `sub_w` and `sub_h` are 1 or 2 and the
    // minimum transform block is at least 4 luma samples wide.
    let min_tb = 1usize << d.geo.min_tb_log2;
    let (step_x, step_y) = ((min_tb / sw).max(1), (min_tb / sh).max(1));
    let mut all = true;
    // The column left of the block and the row above it exist only inside the plane. A block that
    // starts right of or below the picture has no samples there to read: unavailable, as a
    // column below the plane already was (reported by s17sh70x2jfl031ag against 0.1.3).
    let left_in = x > 0 && x - 1 < plane.width;
    let above_in = y > 0 && y - 1 < plane.height;

    // Index 0 is p[-1][2n-1] and index 2n-1 is p[-1][0]: the left column,
    // walked downwards from the block's top edge.
    if left_in {
        let col = x - 1;
        let mut k = 0;
        while k < 2 * n {
            let yy = y + k;
            if yy >= plane.height {
                all = false;
                break;
            }
            if d.available(xc, yc, (col * sw) as isize, (yy * sh) as isize) {
                let end = (yy + step_y).min(plane.height).min(y + 2 * n);
                for (j, yj) in (yy..end).enumerate() {
                    r.buf[2 * n - 1 - k - j] = data[yj * stride + col];
                    avail[2 * n - 1 - k - j] = true;
                }
            } else {
                all = false;
            }
            k += step_y;
        }
    } else {
        all = false;
    }

    // Index 2n is the corner p[-1][-1].
    if left_in && above_in && d.available(xc, yc, ((x - 1) * sw) as isize, ((y - 1) * sh) as isize)
    {
        r.buf[2 * n] = data[(y - 1) * stride + x - 1];
        avail[2 * n] = true;
    } else {
        all = false;
    }

    // Indices 2n+1 ..= 4n are p[0][-1] ..= p[2n-1][-1]: the top row.
    if above_in {
        let row = (y - 1) * stride;
        let mut k = 0;
        while k < 2 * n {
            let xx = x + k;
            if xx >= plane.width {
                all = false;
                break;
            }
            if d.available(xc, yc, (xx * sw) as isize, ((y - 1) * sh) as isize) {
                let end = (xx + step_x).min(plane.width).min(x + 2 * n);
                // A plain loop: the run is two or four samples, for which a
                // library copy is all call overhead.
                let i0 = 2 * n + 1 + k;
                for (j, &v) in data[row + xx..row + end].iter().enumerate() {
                    r.buf[i0 + j] = v;
                    avail[i0 + j] = true;
                }
            } else {
                all = false;
            }
            k += step_x;
        }
    } else {
        all = false;
    }

    if !all {
        substitute(&mut r, &avail, total, bit_depth);
    }
    r
}

/// Reference sample substitution (clause 8.4.4.2.2).
fn substitute(r: &mut Refs, avail: &[bool; 129], total: usize, bit_depth: u8) {
    if avail[..total].iter().all(|&a| !a) {
        let mid = 1u16 << (bit_depth - 1);
        r.buf[..total].fill(mid);
        return;
    }
    if !avail[0] {
        // Search from p[-1][2n-1] upwards, then along the top row.
        if let Some(i) = avail[..total].iter().position(|&a| a) {
            r.buf[0] = r.buf[i];
        }
    }
    let mut last = r.buf[0];
    for (v, &a) in r.buf[..total].iter_mut().zip(avail[..total].iter()) {
        if a {
            last = *v;
        } else {
            *v = last;
        }
    }
}
