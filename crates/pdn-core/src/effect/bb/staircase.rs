//! BB spec: effects/object-outline-object.md step 2, "The anti-alias pass", "Notes /
//! uncertainties"; effects/object-feather-object.md "Staircase anti-aliasing pass";
//! effects/selection-feather-selection.md "Staircase anti-aliasing pass"; common.md §5.3, §5.4
//!
//! The ObjectTools / SelectionTools CPU helpers: the MLAA-style staircase pass over an 8-bit
//! coverage mask (Outline Object, Long Shadow, Feather Object and Feather Selection), and the disc
//! dilation the two-level disc rank filter becomes at percentile 1.0 (Outline Object).

use super::conventions::AA_COPY_COLOUR_AT_ZERO;
use std::cmp::Ordering::{Equal, Greater, Less};

/// How the pass turns a fractional result into a byte (common.md §5.4; Q2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteConv {
    /// Truncate toward zero, keep the low 8 bits (.NET 7/8; inferred for .NET 9+ as well).
    Wrap,
    /// Truncate toward zero, then saturate to 0…255 (the Outline file's reading of .NET 9+).
    Clamp,
}

/// The side neighbour N whose colour a formerly transparent pixel takes (object-feather-object.md
/// "Colour"), or `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Neighbour {
    None,
    Left,
    Right,
    Up,
    Down,
}

// quirk(bb): object-outline-object.md Notes (8-bit wrap)
fn to_byte(x: f64, conv: ByteConv) -> u8 {
    match conv {
        ByteConv::Wrap => (x.trunc() as i64).rem_euclid(256) as u8,
        ByteConv::Clamp => x.trunc().clamp(0.0, 255.0) as u8,
    }
}

/// v − a, for the reductions the original computes from a positive fraction.
fn minus(v: u8, a: u8, conv: ByteConv) -> u8 {
    match conv {
        ByteConv::Wrap => v.wrapping_sub(a),
        ByteConv::Clamp => v.saturating_sub(a),
    }
}

/// v + byte(−r): the vertical after-end and the off-centre horizontal reductions. Under Wrap this
/// is v − ⌊r⌋; under Clamp the byte is 0 and the reduction does nothing (object-outline-object.md
/// Notes, inferred arithmetic that gives both of its disc vectors).
fn plus_negative(v: u8, r: f64, conv: ByteConv) -> u8 {
    v.wrapping_add(to_byte(-r, conv))
}

/// A run's end cells e0 ≤ p ≤ e1 along its line (object-feather-object.md "Runs").
#[derive(Clone, Copy)]
struct Run {
    e0: usize,
    e1: usize,
}

impl Run {
    /// L, the positions strictly between the end cells. It is 0 (or −1 on a one-cell line) only at
    /// the canvas border, where the original gives odd values (not required for parity).
    // approximation: border runs with L = 0 use Rust inf/NaN byte conversions (object-outline-object.md Notes / uncertainties)
    fn len(self) -> f64 {
        self.e1 as f64 - self.e0 as f64 - 1.0
    }

    // quirk(bb): object-outline-object.md "Reference vectors" (m = ⌊c⌋ makes the asymmetries)
    /// m = ⌊c⌋, c = (e0 + e1) / 2.
    fn mid(self) -> usize {
        (self.e0 + self.e1) / 2
    }

    /// 254·|p − c| / L, before truncation.
    fn ramp(self, p: usize) -> f64 {
        127.0 * (2.0 * p as f64 - (self.e0 + self.e1) as f64).abs() / self.len()
    }

    /// The column-m fractions 63.5·(m + ½ − c)/L (0 or 31.75/L) and 63.5·(c − m + ½)/L
    /// (31.75/L or 63.5/L).
    fn centre(self) -> (f64, f64) {
        let half = ((self.e0 + self.e1) % 2) as f64; // c = m + ½
        (
            31.75 * (1.0 - half) / self.len(),
            31.75 * (1.0 + half) / self.len(),
        )
    }
}

/// Positions 0..len along a column or row: cell p is `start + p·step`, its side neighbour N is
/// `off` further.
#[derive(Clone, Copy)]
struct Line {
    start: usize,
    step: usize,
    len: usize,
    off: isize,
}

impl Line {
    fn at(self, p: usize) -> usize {
        self.start + p * self.step
    }

    fn nb(self, p: usize) -> usize {
        self.at(p).wrapping_add_signed(self.off)
    }

    /// Calls `f(p, run)` for every p where `pair(p)` holds. The end cells are the first positions
    /// on each side where the pairing breaks, or the border cell itself when the scan reaches it.
    fn runs(self, pair: impl Fn(usize) -> bool, mut f: impl FnMut(usize, Run)) {
        let mut p = 0;
        while p < self.len {
            if !pair(p) {
                p += 1;
                continue;
            }
            let s = p;
            while p < self.len && pair(p) {
                p += 1;
            }
            let run = Run {
                e0: s.saturating_sub(1),
                e1: p.min(self.len - 1),
            };
            (s..p).for_each(|q| f(q, run));
        }
    }
}

struct Pass<'a> {
    cov: &'a [u8],
    conv: ByteConv,
    copy_at_zero: bool,
    out: Vec<u8>,
    from: Vec<Neighbour>,
}

impl Pass<'_> {
    /// Rules (a) and (d) set a formerly 0 pixel, which takes N's colour once it has coverage (Q22).
    fn give(&mut self, i: usize, v: u8, n: Neighbour) {
        self.out[i] = v;
        if v != 0 || self.copy_at_zero {
            self.from[i] = n;
        }
    }

    /// Rules (a) and (b) against the left or the right neighbour; `line` is a column.
    fn vertical(&mut self, line: Line, n: Neighbour) {
        let (cov, conv) = (self.cov, self.conv);
        // (b) P originally 255 beside N = 0. Any current value; both sides add up; row m stays.
        line.runs(
            |p| cov[line.at(p)] == 255 && cov[line.nb(p)] == 0,
            |p, run| {
                let (i, m) = (line.at(p), run.mid());
                if p < m && cov[line.at(run.e0)] == 0 {
                    self.out[i] = minus(self.out[i], to_byte(run.ramp(p), conv), conv);
                } else if p > m && cov[line.at(run.e1)] == 0 {
                    self.out[i] = plus_negative(self.out[i], run.ramp(p), conv);
                }
            },
        );
        // (a) P still 0 beside N = 255.
        line.runs(
            |p| cov[line.at(p)] == 0 && cov[line.nb(p)] == 255,
            |p, run| {
                let (i, m) = (line.at(p), run.mid());
                if self.out[i] != 0 {
                    return;
                }
                if p <= m && cov[line.at(run.e0)] == 255 {
                    self.give(i, to_byte(run.ramp(p), conv), n);
                } else if p > m && cov[line.at(run.e1)] == 255 {
                    // quirk(bb): object-feather-object.md "Vertical runs" (a) (ramp − 1, 255 at 0)
                    self.give(i, to_byte(255.0 + run.ramp(p), conv), n);
                }
            },
        );
    }

    /// Rules (c) and (d) against the neighbour above or below; `line` is a row.
    fn horizontal(&mut self, line: Line, n: Neighbour) {
        let (cov, conv) = (self.cov, self.conv);
        // (c) P still 255 beside N = 0; the run continues along any neighbour below 255.
        line.runs(
            |p| cov[line.at(p)] == 255 && cov[line.nb(p)] < 255,
            |p, run| {
                let i = line.at(p);
                if self.out[i] != 255 || cov[line.nb(p)] != 0 {
                    return;
                }
                let (left, right) = (cov[line.at(run.e0)] == 0, cov[line.at(run.e1)] == 0);
                let (near, far) = run.centre();
                self.out[i] = match p.cmp(&run.mid()) {
                    Less if left => plus_negative(255, run.ramp(p), conv),
                    Greater if right => plus_negative(255, run.ramp(p), conv),
                    Equal if right => to_byte(255.0 - far, conv),
                    Equal if left => to_byte(255.0 - near, conv),
                    _ => 255,
                };
            },
        );
        // (d) P still 0 beside N = 255.
        line.runs(
            |p| cov[line.at(p)] == 0 && cov[line.nb(p)] == 255,
            |p, run| {
                let i = line.at(p);
                if self.out[i] != 0 {
                    return;
                }
                let (left, right) = (cov[line.at(run.e0)] == 255, cov[line.at(run.e1)] == 255);
                let (near, far) = run.centre();
                let v = match p.cmp(&run.mid()) {
                    Less if left => run.ramp(p),
                    Greater if right => run.ramp(p),
                    Equal if left => far,
                    Equal if right => near,
                    _ => return,
                };
                self.give(i, to_byte(v, conv), n);
            },
        );
    }
}

/// common.md §5.4: the staircase anti-alias pass over a w×h coverage mask, rules (a)–(d) of
/// object-feather-object.md (which agree with object-outline-object.md's tables). It reacts only to
/// hard 0 | 255 boundaries. Every neighbour test, run and end cell reads `cov`; a pixel's own
/// eligibility reads its current value.
///
/// Returns the new coverage and, per pixel, the neighbour N whose colour Feather Object copies into
/// a formerly 0 pixel that rule (a) or (d) gave coverage.
pub fn staircase(cov: &[u8], w: u32, h: u32, conv: ByteConv) -> (Vec<u8>, Vec<Neighbour>) {
    staircase_with(cov, w, h, conv, AA_COPY_COLOUR_AT_ZERO)
}

fn staircase_with(
    cov: &[u8],
    w: u32,
    h: u32,
    conv: ByteConv,
    copy_at_zero: bool,
) -> (Vec<u8>, Vec<Neighbour>) {
    let (w, h) = (w as usize, h as usize);
    assert_eq!(cov.len(), w * h, "coverage is w × h bytes");
    let mut pass = Pass {
        cov,
        conv,
        copy_at_zero,
        out: cov.to_vec(),
        from: vec![Neighbour::None; cov.len()],
    };
    // Each rule reads only `cov` and the pixel's own value, so running each check over the whole
    // canvas in turn keeps every pixel's order: left, right, above, below.
    // ponytail: one thread, eight linear sweeps; split the columns and rows across threads if a
    // 4K Outline Object or Feather render lags.
    let inner = |n: usize| 1..n.saturating_sub(1);
    for (n, off) in [(Neighbour::Left, -1), (Neighbour::Right, 1)] {
        for x in inner(w) {
            let line = Line {
                start: x,
                step: w,
                len: h,
                off,
            };
            pass.vertical(line, n);
        }
    }
    for (n, off) in [
        (Neighbour::Up, -(w as isize)),
        (Neighbour::Down, w as isize),
    ] {
        for y in inner(h) {
            let line = Line {
                start: y * w,
                step: 1,
                len: w,
                off,
            };
            pass.horizontal(line, n);
        }
    }
    (pass.out, pass.from)
}

/// common.md §5.3 at percentile 1.0 and threshold 0.5 (object-outline-object.md step 2): true where
/// some true pixel lies at an offset with dx² + dy² ≤ r². Off-canvas samples are dark. An exact
/// squared Euclidean distance transform, so the cost is linear in the area whatever r is.
pub fn disc_dilate(mask: &[bool], w: u32, h: u32, r: u32) -> Vec<bool> {
    let (w, h) = (w as usize, h as usize);
    assert_eq!(mask.len(), w * h, "mask is w × h");
    if mask.is_empty() {
        return Vec::new();
    }
    let mut d: Vec<Option<u64>> = mask.iter().map(|&b| b.then_some(0)).collect();
    d.chunks_mut(w).for_each(edt_1d);
    let mut col = vec![None; h];
    for x in 0..w {
        col.iter_mut()
            .enumerate()
            .for_each(|(y, c)| *c = d[y * w + x]);
        edt_1d(&mut col);
        col.iter().enumerate().for_each(|(y, &c)| d[y * w + x] = c);
    }
    let r2 = u64::from(r).pow(2);
    d.into_iter().map(|v| v.is_some_and(|v| v <= r2)).collect()
}

/// One Felzenszwalb–Huttenlocher pass: f[q] ← min over the sites p (f[p] ≠ None) of
/// f[p] + (q − p)², with integer parabolas. No site leaves `f` all `None`.
fn edt_1d(f: &mut [Option<u64>]) {
    // The lower envelope, as (p, f[p] + p²) in site order.
    let mut hull: Vec<(i64, i64)> = Vec::new();
    for (p, v) in f.iter().enumerate() {
        let Some(v) = *v else { continue };
        let p = p as i64;
        let fp = v as i64 + p * p;
        // Pop the top site while the new parabola overtakes it no later than it overtook its
        // predecessor: s(p, p1) ≤ s(p1, p0), cross-multiplied.
        while let [.., (p0, f0), (p1, f1)] = hull[..] {
            if (fp - f1) * (p1 - p0) > (f1 - f0) * (p - p1) {
                break;
            }
            hull.pop();
        }
        hull.push((p, fp));
    }
    if hull.is_empty() {
        return;
    }
    let mut k = 0;
    for (q, v) in f.iter_mut().enumerate() {
        let q = q as i64;
        let at = |(p, fp): (i64, i64)| fp - 2 * p * q + q * q;
        while k + 1 < hull.len() && at(hull[k + 1]) <= at(hull[k]) {
            k += 1;
        }
        *v = Some(at(hull[k]) as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: u32, h: u32, l: u32, t: u32, r: u32, b: u32) -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (l..=r).contains(&x) && (t..=b).contains(&y) {
                    255
                } else {
                    0
                }
            })
            .collect()
    }

    /// object-outline-object.md reference vector: the 6×6 square at 2–7 of a 10×10 canvas.
    #[test]
    fn square_reference_vector() {
        let (out, _) = staircase(&rect(10, 10, 2, 2, 7, 7), 10, 10, ByteConv::Wrap);
        let want = [
            [150, 192, 244, 234, 192, 150],
            [192, 255, 255, 255, 255, 192],
            [255; 6],
            [234, 255, 255, 255, 255, 234],
            [192, 255, 255, 255, 255, 192],
            [150, 192, 244, 234, 192, 150],
        ];
        for (j, row) in want.iter().enumerate() {
            let got: Vec<u8> = (2..8).map(|x| out[(2 + j) * 10 + x]).collect();
            assert_eq!(got, row, "row {}", 2 + j);
        }
        assert!(
            (0..100)
                .filter(|i| !(2..8).contains(&(i % 10)) || !(2..8).contains(&(i / 10)))
                .all(|i| out[i] == 0)
        );
    }

    fn disc3() -> Vec<u8> {
        (0..21 * 21)
            .map(|i| {
                let (dx, dy) = (i % 21 - 10, i / 21 - 10);
                if dx * dx + dy * dy <= 9 { 255 } else { 0 }
            })
            .collect()
    }
    const DISC_WRAP: [[u8; 7]; 7] = [
        [0, 0, 63, 223, 31, 0, 0],
        [0, 255, 255, 255, 223, 192, 0],
        [62, 255, 255, 255, 255, 255, 62],
        [223, 255, 255, 255, 255, 255, 223],
        [63, 255, 255, 255, 255, 255, 63],
        [0, 192, 255, 255, 223, 192, 0],
        [0, 0, 63, 223, 31, 0, 0],
    ];

    /// object-outline-object.md reference vector: the radius-3 disc (wrap, the author's design, .NET 9+ per Q2).
    /// Also object-outline-object.md Expected results 2 at mask level.
    #[test]
    fn disc_reference_vector_wrap() {
        let (out, _) = staircase(&disc3(), 21, 21, ByteConv::Wrap);
        for (j, row) in DISC_WRAP.iter().enumerate() {
            assert_eq!(
                &(0..7)
                    .map(|i| out[(7 + j) * 21 + 7 + i])
                    .collect::<Vec<_>>()[..],
                row,
                "dy {}",
                j as i32 - 3
            );
        }
    }

    /// object-outline-object.md Notes: clamping changes exactly five pixels to 255.
    #[test]
    fn disc_reference_vector_clamp() {
        let (out, _) = staircase(&disc3(), 21, 21, ByteConv::Clamp);
        let mut want = DISC_WRAP;
        for (dx, dy) in [(2, -2), (-3, -1), (3, -1), (-2, 2), (2, 2)] {
            want[(dy + 3) as usize][(dx + 3) as usize] = 255;
        }
        for (j, row) in want.iter().enumerate() {
            assert_eq!(
                &(0..7)
                    .map(|i| out[(7 + j) * 21 + 7 + i])
                    .collect::<Vec<_>>()[..],
                row
            );
        }
        assert_eq!(
            super::super::conventions::AA_BYTE_CONVERSION,
            ByteConv::Wrap
        );
    }

    /// object-feather-object.md Expected results 1 (the rectangle tent) at mask level; also
    /// selection-feather-selection.md Expected results 1, which has the same numbers.
    #[test]
    fn rectangle_tent() {
        let (out, from) = staircase(&rect(30, 20, 5, 5, 24, 14), 30, 20, ByteConv::Wrap);
        let top = [
            141, 148, 160, 173, 186, 198, 211, 224, 236, 251, 249, 236, 224, 211, 198, 186, 173,
            160, 148, 141,
        ];
        let side = [141, 167, 192, 217, 255, 243, 217, 192, 167, 141];
        for y in [5usize, 14] {
            assert_eq!((5..25).map(|x| out[y * 30 + x]).collect::<Vec<_>>(), top);
        }
        for x in [5usize, 24] {
            assert_eq!((5..15).map(|y| out[y * 30 + x]).collect::<Vec<_>>(), side);
        }
        assert!((6..24).all(|x| (6..14).all(|y| out[y * 30 + x] == 255)));
        assert!(
            from.iter().all(|n| matches!(n, Neighbour::None)),
            "a convex rectangle gains no outside pixels"
        );
    }

    /// common.md §5.3 / object-outline-object.md step 2: the disc of dx² + dy² ≤ r², off-canvas counts as dark.
    /// object-outline-object.md Expected results 1 at mask level (29 pixels at Width 3).
    #[test]
    fn disc_dilation_counts() {
        let mut one = vec![false; 21 * 21];
        one[10 * 21 + 10] = true;
        for (r, n) in [(1, 5), (2, 13), (3, 29)] {
            assert_eq!(
                disc_dilate(&one, 21, 21, r).iter().filter(|&&b| b).count(),
                n,
                "r {r}"
            );
        }
        let mut corner = vec![false; 9];
        corner[0] = true;
        assert_eq!(
            disc_dilate(&corner, 3, 3, 1).iter().filter(|&&b| b).count(),
            3
        );
    }

    /// Review Focus 1: Width 100 is a distance transform, linear in the area.
    #[test]
    fn disc_dilate_is_linear_time() {
        let mut m = vec![false; 400 * 400];
        for y in 195..205 {
            for x in 195..205 {
                m[y * 400 + x] = true;
            }
        }
        let t = std::time::Instant::now();
        let d = disc_dilate(&m, 400, 400, 100);
        assert!(t.elapsed().as_secs_f64() < 10.0);
        assert!(d[200 * 400 + 304] && !d[200 * 400 + 306]);
    }

    // ---- Added beyond the brief ----

    fn col(out: &[u8], w: usize, x: usize, ys: std::ops::RangeInclusive<usize>) -> Vec<u8> {
        ys.map(|y| out[y * w + x]).collect()
    }

    /// object-outline-object.md Notes, extended to the square: under Clamp only the rules computed
    /// from positive fractions act. The upper vertical reductions and the centre pixels stay; the
    /// lower vertical and the off-centre horizontal reductions do nothing.
    #[test]
    fn square_clamp_keeps_only_the_positive_rules() {
        let (out, _) = staircase(&rect(10, 10, 2, 2, 7, 7), 10, 10, ByteConv::Clamp);
        let want = [
            [150, 255, 244, 255, 255, 150],
            [192, 255, 255, 255, 255, 192],
            [255; 6],
            [255; 6],
            [255; 6],
            [255, 255, 244, 255, 255, 255],
        ];
        for (j, row) in want.iter().enumerate() {
            let got: Vec<u8> = (2..8).map(|x| out[(2 + j) * 10 + x]).collect();
            assert_eq!(got, row, "row {}", 2 + j);
        }
    }

    /// object-outline-object.md rule 1: "For an inside pixel, reductions from both sides add up."
    /// A one-pixel bar is reduced by the left and the right runs alike.
    #[test]
    fn inside_reductions_add_up() {
        let bar = rect(11, 14, 5, 2, 5, 11);
        let (out, from) = staircase(&bar, 11, 14, ByteConv::Wrap);
        assert_eq!(
            col(&out, 11, 5, 2..=11),
            [27, 79, 129, 179, 255, 231, 179, 129, 79, 27]
        );
        assert!((0..out.len()).all(|i| bar[i] == 255 || out[i] == 0));
        assert!(from.iter().all(|&n| n == Neighbour::None));
        let (out, _) = staircase(&bar, 11, 14, ByteConv::Clamp);
        assert_eq!(
            col(&out, 11, 5, 2..=11),
            [27, 79, 129, 179, 255, 255, 255, 255, 255, 223]
        );
    }

    /// object-outline-object.md rule 1: "For an outside pixel, the right-neighbour check runs only if
    /// the left-neighbour check left it at 0."
    #[test]
    fn left_check_blocks_the_right() {
        let (w, h) = (9, 18);
        let mut m = vec![0u8; w * h];
        for (x, ys) in [(4, 3..=14), (5, 13..=15), (6, 7..=14)] {
            for y in ys {
                m[y * w + x] = 255;
            }
        }
        let (out, from) = staircase(&m, w as u32, h as u32, ByteConv::Wrap);
        assert_eq!(
            col(&out, w, 5, 3..=12),
            [0, 0, 0, 0, 0, 11, 37, 62, 87, 113]
        );
        for y in 3..=12 {
            let want = if y >= 8 {
                Neighbour::Left
            } else {
                Neighbour::None
            };
            assert_eq!(from[y * w + 5], want, "row {y}");
        }
        let (out, _) = staircase(&m, w as u32, h as u32, ByteConv::Clamp);
        assert_eq!(
            col(&out, w, 5, 3..=12),
            [0, 0, 0, 0, 0, 255, 255, 255, 255, 255]
        );
    }

    /// object-outline-object.md rule 2: the "below" check applies only while P is still 0 after the
    /// "above" check (the transpose of `left_check_blocks_the_right`, through rule (d)).
    #[test]
    fn above_check_blocks_the_below() {
        let (w, h) = (18, 9);
        let mut m = vec![0u8; w * h];
        for (y, xs) in [(4, 3..=14), (5, 13..=15), (6, 7..=14)] {
            for x in xs {
                m[y * w + x] = 255;
            }
        }
        let (out, from) = staircase(&m, w as u32, h as u32, ByteConv::Wrap);
        let row: Vec<u8> = (3..=12).map(|x| out[5 * w + x]).collect();
        assert_eq!(row, [0, 0, 0, 0, 0, 12, 38, 63, 88, 114]);
        for x in 3..=12 {
            let want = if x >= 8 {
                Neighbour::Up
            } else {
                Neighbour::None
            };
            assert_eq!(from[5 * w + x], want, "column {x}");
        }
    }

    /// object-outline-object.md Notes: "Runs longer than 127 px make the 'A − 1' rule give 255 at the
    /// pixel just past the centre." The next pixel wraps to A − 1 = 0, which is no coverage (Q22).
    #[test]
    fn long_runs_wrap_past_the_centre() {
        let (w, h) = (4, 304);
        let mut m = vec![0u8; w * h];
        for y in 2..=301 {
            m[y * w + 2] = 255;
        }
        for y in 202..=303 {
            m[y * w + 1] = 255;
        }
        let (out, from) = staircase_with(&m, w as u32, h as u32, ByteConv::Wrap, false);
        assert_eq!(col(&out, w, 1, 100..=105), [0, 0, 255, 0, 2, 3]);
        assert_eq!(out[201 * w + 1], 125);
        let at = |y: usize| from[y * w + 1];
        assert_eq!(
            [at(101), at(102), at(103), at(104)],
            [
                Neighbour::None,
                Neighbour::Right,
                Neighbour::None,
                Neighbour::Right
            ]
        );
        let (_, from) = staircase_with(&m, w as u32, h as u32, ByteConv::Wrap, true);
        assert_eq!(from[103 * w + 1], Neighbour::Right);
        let (out, _) = staircase(&m, w as u32, h as u32, ByteConv::Clamp);
        assert!((102..=201).all(|y| out[y * w + 1] == 255));
        assert_eq!(col(&out, w, 1, 100..=101), [0, 0]);
    }

    /// object-feather-object.md "Staircase anti-aliasing pass": partial coverage never triggers a
    /// rule nor counts as a step, and rule (c)'s run continues along any non-full neighbour.
    #[test]
    fn partial_coverage_and_the_wide_inside_run() {
        let (w, h) = (16, 8);
        let mut m = rect(16, 8, 2, 3, 13, 5);
        for x in 8..=13 {
            m[2 * w + x] = 128;
        }
        m[5 * w + 1] = 128;
        let (out, _) = staircase(&m, w as u32, h as u32, ByteConv::Wrap);
        let row = |y: usize| (2..=13).map(|x| out[y * w + x]).collect::<Vec<_>>();
        assert_eq!(
            row(3),
            [139, 160, 181, 203, 224, 249, 255, 255, 255, 255, 255, 255]
        );
        assert_eq!(row(2), [0, 0, 0, 0, 0, 0, 128, 128, 128, 128, 128, 128]);
        // Rule (b)'s run needs N = 0 exactly: (1, 5) = 128 ends it, so (2, 3) is its centre row.
        assert_eq!(col(&out, w, 2, 3..=5), [139, 255, 255]);
        assert_eq!(col(&out, w, 1, 3..=5), [0, 0, 128]);

        let graded: Vec<u8> = (0..15 * 9).map(|i| 1 + (i * 37 % 254) as u8).collect();
        for conv in [ByteConv::Wrap, ByteConv::Clamp] {
            assert_eq!(staircase(&graded, 15, 9, conv).0, graded);
            for v in [0, 255] {
                assert_eq!(staircase(&[v; 12], 4, 3, conv).0, [v; 12]);
            }
        }
    }

    /// object-feather-object.md (d): with both run ends 255 the left-end value wins for column m
    /// (⌊63.5 / L⌋ at even L, where the right end would give 0).
    #[test]
    fn notch_left_end_wins() {
        let mut m = rect(10, 9, 3, 5, 6, 7);
        m[5 * 10 + 4] = 0;
        m[5 * 10 + 5] = 0;
        let (out, from) = staircase(&m, 10, 9, ByteConv::Wrap);
        assert_eq!([out[54], out[55]], [31, 63]);
        assert_eq!([from[54], from[55]], [Neighbour::Down; 2]);
    }

    /// object-feather-object.md "Colour": the neighbour whose colour a formerly 0 pixel takes, on the
    /// radius-3 disc. With Q22's alternative, the rules that fire with a coverage of 0 record too.
    #[test]
    fn disc_neighbours() {
        let at = |dx: i32, dy: i32| ((10 + dy) * 21 + 10 + dx) as usize;
        let mut want = vec![Neighbour::None; 21 * 21];
        for (dx, dy, n) in [
            (-3, -1, Neighbour::Right),
            (-3, 1, Neighbour::Right),
            (3, -1, Neighbour::Left),
            (3, 1, Neighbour::Left),
            (-1, -3, Neighbour::Down),
            (1, -3, Neighbour::Down),
            (-1, 3, Neighbour::Up),
            (1, 3, Neighbour::Up),
        ] {
            want[at(dx, dy)] = n;
        }
        // `staircase` reads AA_COPY_COLOUR_AT_ZERO, so this pins the shipped default (false).
        assert_eq!(staircase(&disc3(), 21, 21, ByteConv::Wrap).1, want);
        want[at(-2, -3)] = Neighbour::Down;
        want[at(-2, 3)] = Neighbour::Up;
        assert_eq!(
            staircase_with(&disc3(), 21, 21, ByteConv::Wrap, true).1,
            want
        );
    }

    /// common.md §5.4: the two conversions, and the reductions built on them.
    #[test]
    fn byte_conversions() {
        use ByteConv::{Clamp, Wrap};
        let wrap = [-1.5, -256.0, 255.9, 256.7, 300.2, f64::NAN, f64::INFINITY];
        let got: Vec<u8> = wrap.iter().map(|&x| to_byte(x, Wrap)).collect();
        assert_eq!(got, [255, 0, 255, 0, 44, 0, 255]);
        let got: Vec<u8> = wrap.iter().map(|&x| to_byte(x, Clamp)).collect();
        assert_eq!(got, [0, 0, 255, 255, 255, 0, 255]);
        assert_eq!([minus(10, 20, Wrap), minus(10, 20, Clamp)], [246, 0]);
        assert_eq!(
            [
                plus_negative(255, 63.5, Wrap),
                plus_negative(255, 63.5, Clamp)
            ],
            [192, 255]
        );
        assert_eq!(plus_negative(10, 20.2, Wrap), 246);
    }

    /// Run ends at the canvas border never count as steps (object-outline-object.md "Terms"); tiny
    /// and empty canvases neither panic nor change size.
    #[test]
    fn borders_and_degenerate_sizes() {
        for (w, h, x0, y0, x1, y1) in [(3, 4, 1, 0, 1, 3), (4, 3, 0, 1, 3, 1)] {
            let bar = rect(w, h, x0, y0, x1, y1);
            assert_eq!(staircase(&bar, w, h, ByteConv::Wrap).0, bar);
        }
        let dot = rect(3, 3, 1, 1, 1, 1);
        assert_eq!(
            staircase(&dot, 3, 3, ByteConv::Wrap).0,
            [0, 0, 0, 0, 223, 0, 0, 0, 0]
        );
        for (w, h) in [
            (0, 0),
            (0, 3),
            (3, 0),
            (1, 1),
            (1, 5),
            (5, 1),
            (2, 2),
            (3, 2),
            (2, 3),
        ] {
            for i in 0..(w * h).max(1) {
                let m: Vec<u8> = (0..w * h)
                    .map(|j| if (j + i) % 3 == 0 { 255 } else { 0 })
                    .collect();
                for conv in [ByteConv::Wrap, ByteConv::Clamp] {
                    let (out, from) = staircase(&m, w, h, conv);
                    assert_eq!((out.len(), from.len()), (m.len(), m.len()));
                }
                let b: Vec<bool> = m.iter().map(|&v| v == 255).collect();
                assert_eq!(disc_dilate(&b, w, h, 2).len(), b.len());
            }
        }
    }

    /// common.md §5.3: the distance transform agrees with sampling every offset of the disc.
    #[test]
    fn disc_dilate_matches_brute_force() {
        let (w, h) = (23i32, 17i32);
        let mut s = 12345u32;
        let m: Vec<bool> = (0..w * h)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                s >> 24 < 12
            })
            .collect();
        assert!(m.iter().any(|&b| b) && !m.iter().all(|&b| b));
        for r in 0..=7i32 {
            let got = disc_dilate(&m, w as u32, h as u32, r as u32);
            for (i, &g) in got.iter().enumerate() {
                let (x, y) = (i as i32 % w, i as i32 / w);
                let want = (-r..=r).any(|dy| {
                    (-r..=r).any(|dx| {
                        let (sx, sy) = (x + dx, y + dy);
                        dx * dx + dy * dy <= r * r
                            && (0..w).contains(&sx)
                            && (0..h).contains(&sy)
                            && m[(sy * w + sx) as usize]
                    })
                });
                assert_eq!(g, want, "r {r} at ({x}, {y})");
            }
        }
        // One pass against brute force on arbitrary values, including envelopes whose breakpoints
        // cluster between two integers ([10, 4, 0]: q = 0 skips two sites at once and gets 4, not 5).
        let mut f = [Some(10), Some(4), Some(0), None];
        edt_1d(&mut f);
        assert_eq!(f, [Some(4), Some(1), Some(0), Some(1)]);
        for _ in 0..200 {
            let g: Vec<Option<u64>> = (0..19)
                .map(|_| {
                    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (s >> 29 != 0).then_some(u64::from(s >> 16 & 0x1ff))
                })
                .collect();
            let mut got = g.clone();
            edt_1d(&mut got);
            for (q, &v) in got.iter().enumerate() {
                let want = (0..g.len())
                    .filter_map(|p| g[p].map(|f| f + (q.abs_diff(p) as u64).pow(2)))
                    .min();
                assert_eq!(v, want, "{g:?} at {q}");
            }
        }
        assert!(disc_dilate(&[false; 12], 4, 3, 5).iter().all(|&b| !b));
        assert!(disc_dilate(&[true; 12], 4, 3, 0).iter().all(|&b| b));
    }
}
