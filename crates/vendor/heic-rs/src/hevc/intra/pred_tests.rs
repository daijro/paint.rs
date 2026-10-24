//! Cross-checks of the optimised predictors against a direct transcription of
//! the clause 8.4.4.2 formulas.

use super::predict;
use crate::hevc::intra::Refs;
use crate::hevc::intra::filter_refs;

const ANGLE: [i32; 33] = [
    32, 26, 21, 17, 13, 9, 5, 2, 0, -2, -5, -9, -13, -17, -21, -26, -32, -26, -21, -17, -13, -9,
    -5, -2, 0, 2, 5, 9, 13, 17, 21, 26, 32,
];
const INVA: [i32; 15] = [
    -4096, -1638, -910, -630, -482, -390, -315, -256, -315, -390, -482, -630, -910, -1638, -4096,
];

/// Straightforward reference predictor returning `out[y][x]`.
fn refer(r: &Refs, mode: u8, c_idx: usize, bd: u8) -> Vec<Vec<i32>> {
    let n = r.n as i32;
    let sh = (r.n.trailing_zeros() + 1) as i32;
    let p = |x: i32, y: i32| -> i32 {
        (if x < 0 {
            r.left(y as isize)
        } else {
            r.top(x as isize)
        }) as i32
    };
    let mut o = vec![vec![0i32; r.n]; r.n];
    if mode == 0 {
        for y in 0..n {
            for x in 0..n {
                o[y as usize][x as usize] = ((n - 1 - x) * p(-1, y)
                    + (x + 1) * p(n, -1)
                    + (n - 1 - y) * p(x, -1)
                    + (y + 1) * p(-1, n)
                    + n)
                    >> sh;
            }
        }
    } else if mode == 1 || mode > 34 {
        let mut s = n;
        for i in 0..n {
            s += p(i, -1) + p(-1, i);
        }
        let d = s >> sh;
        for row in o.iter_mut() {
            for v in row.iter_mut() {
                *v = d;
            }
        }
        if c_idx == 0 && n < 32 {
            for x in 1..n {
                o[0][x as usize] = (p(x, -1) + 3 * d + 2) >> 2;
            }
            for y in 1..n {
                o[y as usize][0] = (p(-1, y) + 3 * d + 2) >> 2;
            }
            o[0][0] = (p(-1, 0) + 2 * d + p(0, -1) + 2) >> 2;
        }
    } else {
        let a = ANGLE[mode as usize - 2];
        let v = mode >= 18;
        let ld = |i: i32| if v { p(i - 1, -1) } else { p(-1, i - 1) };
        let sd = |i: i32| if v { p(-1, i - 1) } else { p(i - 1, -1) };
        let mut rv = vec![0i32; 97];
        for i in 0..=n {
            rv[(32 + i) as usize] = ld(i);
        }
        let lim = (n * a) >> 5;
        if a < 0 {
            if lim < -1 {
                for x in lim..=-1 {
                    rv[(32 + x) as usize] = sd((x * INVA[mode as usize - 11] + 128) >> 8);
                }
            }
        } else {
            for i in n + 1..=2 * n {
                rv[(32 + i) as usize] = ld(i);
            }
        }
        for y in 0..n {
            for x in 0..n {
                let (m, k) = if v { (y, x) } else { (x, y) };
                let (ii, ff) = (((m + 1) * a) >> 5, ((m + 1) * a) & 31);
                // `ref[x + iIdx + 2]` is only defined when iFact is non-zero;
                // with angle 32 the index would run one past the array.
                let b0 = rv[(32 + k + ii + 1) as usize];
                o[y as usize][x as usize] = if ff == 0 {
                    b0
                } else {
                    let b1 = rv[(32 + k + ii + 2) as usize];
                    ((32 - ff) * b0 + ff * b1 + 16) >> 5
                };
            }
        }
        let hi = (1i32 << bd) - 1;
        if c_idx == 0 && n < 32 && mode == 26 {
            for y in 0..n {
                o[y as usize][0] = (p(0, -1) + ((p(-1, y) - p(-1, -1)) >> 1)).max(0).min(hi);
            }
        } else if c_idx == 0 && n < 32 && mode == 10 {
            for x in 0..n {
                o[0][x as usize] = (p(-1, 0) + ((p(x, -1) - p(-1, -1)) >> 1)).max(0).min(hi);
            }
        }
    }
    o
}

fn xs(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn rnd_refs(n: usize, bd: u8, seed: u32) -> Refs {
    let (mut s, mut r) = (seed | 1, Refs::new(n));
    let mask = (1u32 << bd) - 1;
    r.set_top(-1, (xs(&mut s) & mask) as u16);
    for i in 0..2 * n as isize {
        r.set_top(i, (xs(&mut s) & mask) as u16);
        r.set_left(i, (xs(&mut s) & mask) as u16);
    }
    r
}

fn flat(n: usize, v: u16) -> Refs {
    let mut r = Refs::new(n);
    r.set_top(-1, v);
    for i in 0..2 * n as isize {
        r.set_top(i, v);
        r.set_left(i, v);
    }
    r
}

#[test]
fn matches_reference_everywhere() {
    let mut seed = 0x1234_5678u32;
    for &n in &[4usize, 8, 16, 32] {
        for &bd in &[8u8, 10] {
            for &c in &[0usize, 1] {
                for mode in 0u8..=34 {
                    let r = rnd_refs(n, bd, xs(&mut seed));
                    let (st, mut out) = (n + 3, vec![0u16; (n + 3) * n]);
                    predict(&r, mode, c, bd, &mut out, st);
                    let e = refer(&r, mode, c, bd);
                    for y in 0..n {
                        for x in 0..n {
                            assert_eq!(
                                out[y * st + x] as i32,
                                e[y][x],
                                "n={n} bd={bd} c={c} mode={mode} at ({x},{y})"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn dc_of_constant_is_that_constant() {
    for &n in &[4usize, 8, 16, 32] {
        let r = flat(n, 733);
        let mut out = vec![0u16; n * n];
        predict(&r, 1, 0, 10, &mut out, n);
        assert!(out.iter().all(|&v| v == 733));
    }
}

#[test]
fn pure_vertical_and_horizontal_copy_the_edge() {
    for &n in &[4usize, 8, 16, 32] {
        let (mut rv, mut rh) = (Refs::new(n), Refs::new(n));
        for i in 0..2 * n as isize {
            rv.set_top(i, (3 * i + 5) as u16);
            rh.set_left(i, (3 * i + 5) as u16);
        }
        let (mut a, mut b) = (vec![0u16; n * n], vec![0u16; n * n]);
        predict(&rv, 26, 1, 8, &mut a, n);
        predict(&rh, 10, 1, 8, &mut b, n);
        for y in 0..n {
            for x in 0..n {
                assert_eq!(a[y * n + x] as usize, 3 * x + 5);
                assert_eq!(b[y * n + x] as usize, 3 * y + 5);
            }
        }
    }
}

#[test]
fn filtering_a_constant_is_a_no_op() {
    for &n in &[8usize, 16, 32] {
        let r = flat(n, 700);
        for &strong in &[false, true] {
            let f = filter_refs(&r, 2, 0, 10, 1, strong, false);
            assert_eq!(&f.buf[..], &r.buf[..], "n={n} strong={strong}");
        }
    }
}

#[test]
fn filtering_is_skipped_when_the_spec_says_so() {
    let mut s = 9u32;
    let small = rnd_refs(4, 8, xs(&mut s));
    let big = rnd_refs(16, 8, xs(&mut s));
    // nTbS == 4, DC, chroma with chroma_array_type != 3, and the disable flag.
    for (r, mode, c, cat, dis) in [
        (&small, 2u8, 0usize, 1u8, false),
        (&big, 1, 0, 1, false),
        (&big, 2, 1, 1, false),
        (&big, 2, 0, 1, true),
    ] {
        let f = filter_refs(r, mode, c, 8, cat, true, dis);
        assert_eq!(&f.buf[..], &r.buf[..], "mode={mode} c={c} dis={dis}");
    }
    // ...but a far-from-axis mode at nTbS == 16 does smooth an alternating edge.
    let mut alt = Refs::new(16);
    for i in 0..32isize {
        alt.set_top(i, (i as u16 & 1) * 255);
        alt.set_left(i, (i as u16 & 1) * 255);
    }
    let f = filter_refs(&alt, 2, 0, 8, 1, true, false);
    assert_ne!(&f.buf[..], &alt.buf[..]);
}
