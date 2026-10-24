//! Intra sample prediction: the planar, DC and angular predictors of
//! clauses 8.4.4.2.5 and 8.4.4.2.6.
//!
//! The predictors are generic over the block size. `predict` dispatches on
//! `Refs::n` once, so every loop below has a trip count the compiler can see:
//! the 4x4 bodies unroll completely and the angular reference window is
//! exactly as long as the size needs, instead of the 32x32 worst case.

use super::Refs;

/// `intraPredAngle` indexed by `predModeIntra - 2`, for modes 2 to 34.
const ANG: [i32; 33] = [
    32, 26, 21, 17, 13, 9, 5, 2, 0, -2, -5, -9, -13, -17, -21, -26, -32, -26, -21, -17, -13, -9,
    -5, -2, 0, 2, 5, 9, 13, 17, 21, 26, 32,
];

/// `invAngle` indexed by `predModeIntra - 11`, for the modes 11 to 25 that
/// have a negative `intraPredAngle`.
const INV: [i32; 15] = [
    -4096, -1638, -910, -630, -482, -390, -315, -256, -315, -390, -482, -630, -910, -1638, -4096,
];

/// Produces the nTbS x nTbS prediction block (clauses 8.4.4.2.5 to 8.4.4.2.6).
///
/// `out` receives predSamples[x][y] at `out[y * out_stride + x]`. It is left
/// untouched when `r.n` is not 4, 8, 16 or 32.
pub fn predict(
    r: &Refs,
    mode: u8,
    c_idx: usize,
    bit_depth: u8,
    out: &mut [u16],
    out_stride: usize,
) {
    match r.n {
        4 => predict_n::<4, 13>(r, mode, c_idx, bit_depth, out, out_stride),
        8 => predict_n::<8, 25>(r, mode, c_idx, bit_depth, out, out_stride),
        16 => predict_n::<16, 49>(r, mode, c_idx, bit_depth, out, out_stride),
        32 => predict_n::<32, 97>(r, mode, c_idx, bit_depth, out, out_stride),
        _ => {}
    }
}

/// [`predict`] for one block size `N`; `L` is the angular window, `3N + 1`.
fn predict_n<const N: usize, const L: usize>(
    r: &Refs,
    mode: u8,
    c_idx: usize,
    bd: u8,
    out: &mut [u16],
    st: usize,
) {
    match mode {
        0 => planar::<N>(r, out, st),
        2..=34 => angular::<N, L>(r, mode, c_idx, bd, out, st),
        _ => dc::<N>(r, c_idx, out, st),
    }
}

/// Planar prediction, clause 8.4.4.2.5.
fn planar<const N: usize>(r: &Refs, out: &mut [u16], st: usize) {
    let ni = N as i32;
    let sh = N.trailing_zeros() + 1;
    let (tn, ln) = (r.top(ni as isize) as i32, r.left(ni as isize) as i32);
    let mut top = [0i32; N];
    for (x, v) in top.iter_mut().enumerate() {
        *v = r.top(x as isize) as i32;
    }
    for y in 0..N {
        let (l, yi) = (r.left(y as isize) as i32, y as i32);
        let (a, b) = ((ni - 1 - yi), (yi + 1) * ln + ni);
        let row = &mut out[y * st..y * st + N];
        for (x, (o, &t)) in row.iter_mut().zip(top.iter()).enumerate() {
            let xi = x as i32;
            *o = (((ni - 1 - xi) * l + (xi + 1) * tn + a * t + b) >> sh) as u16;
        }
    }
}

/// DC prediction with the luma boundary smoothing, clause 8.4.4.2.6.
fn dc<const N: usize>(r: &Refs, c_idx: usize, out: &mut [u16], st: usize) {
    let ni = N as i32;
    let sh = N.trailing_zeros() + 1;
    let mut sum = ni;
    for i in 0..N as isize {
        sum += r.top(i) as i32 + r.left(i) as i32;
    }
    let d = sum >> sh;
    for y in 0..N {
        for o in out[y * st..y * st + N].iter_mut() {
            *o = d as u16;
        }
    }
    if c_idx == 0 && N < 32 {
        for (x, o) in out[..N].iter_mut().enumerate() {
            *o = ((r.top(x as isize) as i32 + 3 * d + 2) >> 2) as u16;
        }
        for y in 1..N {
            out[y * st] = ((r.left(y as isize) as i32 + 3 * d + 2) >> 2) as u16;
        }
        out[0] = ((r.left(0) as i32 + 2 * d + r.top(0) as i32 + 2) >> 2) as u16;
    }
}

/// Angular prediction for modes 2 to 34, clause 8.4.4.2.6.
///
/// `rf[N + i]` holds ref[i] for i in -N ..= 2N, so `L` must be `3N + 1`.
fn angular<const N: usize, const L: usize>(
    r: &Refs,
    mode: u8,
    c_idx: usize,
    bd: u8,
    out: &mut [u16],
    st: usize,
) {
    const { assert!(L == 3 * N + 1) }
    let ni = N as i32;
    let a = ANG[mode as usize - 2];
    let vert = mode >= 18;
    let mut rf = [0i32; L];
    let main = |i: isize| -> i32 { (if vert { r.top(i - 1) } else { r.left(i - 1) }) as i32 };
    let side = |i: isize| -> i32 { (if vert { r.left(i - 1) } else { r.top(i - 1) }) as i32 };
    for (i, v) in rf[N..2 * N + 1].iter_mut().enumerate() {
        *v = main(i as isize);
    }
    if a < 0 {
        let lim = (ni * a) >> 5;
        if lim < -1 {
            let inv = INV[mode as usize - 11];
            let mut x = -1i32;
            while x >= lim {
                rf[(ni + x) as usize] = side(((x * inv + 128) >> 8) as isize);
                x -= 1;
            }
        }
    } else {
        for (i, v) in rf[2 * N + 1..3 * N + 1].iter_mut().enumerate() {
            *v = main((N + 1 + i) as isize);
        }
    }
    for m in 0..N {
        let t = (m as i32 + 1) * a;
        let (idx, f) = (t >> 5, t & 31);
        let b = (ni + 1 + idx) as usize;
        if vert {
            let row = &mut out[m * st..m * st + N];
            if f == 0 {
                for (o, &v) in row.iter_mut().zip(rf[b..b + N].iter()) {
                    *o = v as u16;
                }
            } else {
                let (s0, s1) = (&rf[b..b + N], &rf[b + 1..b + 1 + N]);
                for (o, (&v0, &v1)) in row.iter_mut().zip(s0.iter().zip(s1.iter())) {
                    *o = (((32 - f) * v0 + f * v1 + 16) >> 5) as u16;
                }
            }
        } else {
            for k in 0..N {
                let v = if f == 0 {
                    rf[b + k]
                } else {
                    ((32 - f) * rf[b + k] + f * rf[b + k + 1] + 16) >> 5
                };
                out[k * st + m] = v as u16;
            }
        }
    }
    if c_idx == 0 && N < 32 && (mode == 26 || mode == 10) {
        let (c, hi) = (r.top(-1) as i32, (1i32 << bd) - 1);
        if mode == 26 {
            let p = r.top(0) as i32;
            for y in 0..N {
                let v = p + ((r.left(y as isize) as i32 - c) >> 1);
                out[y * st] = v.max(0).min(hi) as u16;
            }
        } else {
            let p = r.left(0) as i32;
            for (x, o) in out[..N].iter_mut().enumerate() {
                let v = p + ((r.top(x as isize) as i32 - c) >> 1);
                *o = v.max(0).min(hi) as u16;
            }
        }
    }
}

#[cfg(test)]
#[path = "pred_tests.rs"]
mod tests;
