//! Ported from PhotoCraft crates/algo/src/noise.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: edge replication (`Src::get` clamps to the layer, so a flat opaque layer stays unchanged at its border; upstream reads transparent black there, R57); an empty `out` returns the layer; only `median` (Dust & Scratches' kernel; Add Noise and `hash01` dropped); it works on a whole interleaved straight RGBA f32 layer (`w`, `h`) with an `out` rectangle instead of `Image`/`Ctx`, `Interrupt` is polled per output row; the unit tests are new apart from the histogram-versus-direct check, which keeps upstream's name.

use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;

const N: usize = 4;

/// The layer with its edge pixels replicated outward.
struct Src<'a> {
    buf: &'a [f32],
    w: i32,
    h: i32,
}

impl Src<'_> {
    fn get(&self, x: i32, y: i32, c: usize) -> f32 {
        let (x, y) = (x.clamp(0, self.w - 1), y.clamp(0, self.h - 1));
        self.buf[(y as usize * self.w as usize + x as usize) * N + c]
    }
}

/// Median over a disc of `radius`; with `threshold` (0-255 levels), a pixel is replaced only when
/// it differs from the median by more than `threshold`. The whole layer back, with `out`
/// replaced; `None` once cancelled.
pub fn median(
    buf: &[f32],
    w: usize,
    h: usize,
    out: Rect,
    radius: f32,
    threshold: Option<f32>,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let out = out.intersect(&Rect::new(0, 0, w as i32, h as i32));
    if out.is_empty() {
        return Some(buf.to_vec());
    }
    let src = Src {
        buf,
        w: w as i32,
        h: h as i32,
    };
    let r = radius.max(0.0).round() as i32;
    let t = threshold.map(|t| t / 255.0);
    let part = if r == 0 {
        crop(&src, out)
    } else if r >= 2 {
        median_ranked(&src, out, r, t, ctl)?
    } else {
        median_direct(&src, out, r, t, ctl)?
    };
    let mut res = buf.to_vec();
    let ow = out.width() as usize;
    for (row, y) in part.chunks_exact(ow * N).zip(out.y0..) {
        let at = (y as usize * w + out.x0 as usize) * N;
        res[at..at + ow * N].copy_from_slice(row);
    }
    Some(res)
}

fn crop(src: &Src, out: Rect) -> Vec<f32> {
    (out.y0..out.y1)
        .flat_map(|y| (out.x0..out.x1).map(move |x| (x, y)))
        .flat_map(|(x, y)| (0..N).map(move |c| src.get(x, y, c)))
        .collect()
}

/// Half-width of the median disc on each row `dy` in `-r..=r`: the largest `dx` with
/// `dx^2 + dy^2 <= r^2 + r`.
fn disc_rows(r: i32) -> Vec<i32> {
    (-r..=r)
        .map(|dy| {
            let lim = r * r + r - dy * dy;
            let mut hw = (lim as f64).sqrt() as i32;
            while hw * hw > lim {
                hw -= 1;
            }
            while (hw + 1) * (hw + 1) <= lim {
                hw += 1;
            }
            hw
        })
        .collect()
}

/// [`median`] with a sliding histogram, at any depth: every sample of a channel's window is
/// replaced by its rank among the window's distinct values (its level for 8-bit samples), so the
/// histogram of the disc slides from pixel to pixel, removing one end and adding the other end of
/// each disc row (or column) per step, and the median rank is tracked as it moves (Huang). Ranks
/// are counted in fine bins and in blocks of bins, so the median crosses empty stretches of a
/// large rank range a block at a time. A pixel costs O(radius) instead of a selection over
/// O(radius^2) samples, and the value picked is the sample the selection picks (ranks follow
/// `total_cmp`), so the output is identical.
fn median_ranked(
    src: &Src,
    out: Rect,
    r: i32,
    t: Option<f32>,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let n = N;
    let win = out.inflate(r);
    let (ww, wh) = (win.width() as usize, win.height() as usize);
    let rows = disc_rows(r);
    let count: usize = rows.iter().map(|&hw| (2 * hw + 1) as usize).sum();
    let mid = count / 2;
    let (ow, oh) = (out.width() as usize, out.height() as usize);
    let ru = r as usize;
    let mut res = vec![0.0f32; ow * oh * n];
    let mut plane = vec![0u32; ww * wh];
    let mut values = Vec::new();
    let (mut hist, mut blocks) = (Vec::new(), Vec::new());
    for c in 0..n {
        // Window samples of this channel; outside the source reads as 0.
        values.clear();
        values.extend(
            (win.y0..win.y1)
                .flat_map(|y| (win.x0..win.x1).map(move |x| (x, y)))
                .map(|(x, y)| src.get(x, y, c)),
        );
        let eight_bit = values.iter().all(|&v| {
            let k = (v * 255.0).round();
            (0.0..=255.0).contains(&k) && (k / 255.0).to_bits() == v.to_bits()
        });
        let levels: Vec<f32> = if eight_bit {
            for (p, &v) in plane.iter_mut().zip(&values) {
                *p = (v * 255.0).round() as u32;
            }
            (0..256).map(|k| k as f32 / 255.0).collect()
        } else {
            let mut sorted = values.clone();
            sorted.sort_unstable_by(|a, b| a.total_cmp(b));
            sorted.dedup_by(|a, b| a.to_bits() == b.to_bits());
            for (p, v) in plane.iter_mut().zip(&values) {
                *p = sorted.partition_point(|s| s.total_cmp(v).is_lt()) as u32;
            }
            sorted
        };
        let k = levels.len();
        // Block size: about sqrt(k) bins, a power of two.
        let shift = (usize::BITS - k.isqrt().max(1).leading_zeros()) as usize;
        let bs = 1usize << shift;
        hist.clear();
        hist.resize(k, 0u32);
        blocks.clear();
        blocks.resize(k.div_ceil(bs), 0u32);
        // Disc of output pixel (0, 0), centred on window pixel (r, r).
        for (j, &hw) in rows.iter().enumerate() {
            for &q in &plane[j * ww + ru - hw as usize..=j * ww + ru + hw as usize] {
                hist[q as usize] += 1;
                blocks[q as usize >> shift] += 1;
            }
        }
        // `m` is the median rank and `below` the number of samples ranked under it.
        let (mut m, mut below) = (0usize, 0usize);
        // The disc visits the rows in a serpentine (left to right, down one, right to left, ...),
        // so every move swaps one end of each disc row or column and the histogram is never
        // rebuilt. The disc is symmetric, so a column's half-height is the row's half-width.
        for oy in 0..oh {
            if ctl.cancelled() {
                return None;
            }
            for step in 0..ow {
                let ox = if oy % 2 == 0 { step } else { ow - 1 - step };
                // Window pixel at the disc's centre, and the two ends of each row or column.
                let (cx, cy) = (ox + ru, oy + ru);
                let moves = rows.iter().enumerate().map(|(j, &hw)| {
                    let hw = hw as usize;
                    if step == 0 {
                        // Down from (cx, cy - 1): column cx - r + j loses its top, gains a bottom.
                        let x = cx + j - ru;
                        ((cy - 1 - hw) * ww + x, (cy + hw) * ww + x)
                    } else if oy % 2 == 0 {
                        // Right from (cx - 1, cy).
                        let y = (cy + j - ru) * ww;
                        (y + cx - 1 - hw, y + cx + hw)
                    } else {
                        // Left from (cx + 1, cy).
                        let y = (cy + j - ru) * ww;
                        (y + cx + 1 + hw, y + cx - hw)
                    }
                });
                if oy > 0 || step > 0 {
                    for (gone, new) in moves {
                        let (gone, new) = (plane[gone] as usize, plane[new] as usize);
                        hist[gone] -= 1;
                        blocks[gone >> shift] -= 1;
                        hist[new] += 1;
                        blocks[new >> shift] += 1;
                        below -= usize::from(gone < m);
                        below += usize::from(new < m);
                    }
                }
                while below > mid {
                    if m & (bs - 1) == 0 && below - blocks[(m >> shift) - 1] as usize > mid {
                        below -= blocks[(m >> shift) - 1] as usize;
                        m -= bs;
                    } else {
                        m -= 1;
                        below -= hist[m] as usize;
                    }
                }
                loop {
                    if m & (bs - 1) == 0 && below + blocks[m >> shift] as usize <= mid {
                        below += blocks[m >> shift] as usize;
                        m += bs;
                    } else if below + hist[m] as usize <= mid {
                        below += hist[m] as usize;
                        m += 1;
                    } else {
                        break;
                    }
                }
                let mv = levels[m];
                let o = src.get(out.x0 + ox as i32, out.y0 + oy as i32, c);
                res[(oy * ow + ox) * n + c] = match t {
                    Some(t) if (o - mv).abs() <= t => o,
                    _ => mv,
                };
            }
        }
    }
    Some(res)
}

/// [`median`] by selecting the middle of every disc's samples.
fn median_direct(
    src: &Src,
    out: Rect,
    r: i32,
    t: Option<f32>,
    ctl: &Interrupt,
) -> Option<Vec<f32>> {
    let offs: Vec<(i32, i32)> = (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter(|(dx, dy)| dx * dx + dy * dy <= r * r + r)
        .collect();
    let mut vals = Vec::with_capacity(offs.len());
    let mut res = Vec::with_capacity(out.width() as usize * out.height() as usize * N);
    for y in out.y0..out.y1 {
        if ctl.cancelled() {
            return None;
        }
        for x in out.x0..out.x1 {
            for c in 0..N {
                vals.clear();
                vals.extend(offs.iter().map(|(dx, dy)| src.get(x + dx, y + dy, c)));
                let mid = vals.len() / 2;
                let (_, m, _) = vals.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
                let m = *m;
                let o = src.get(x, y, c);
                res.push(match t {
                    Some(t) if (o - m).abs() <= t => o,
                    _ => m,
                });
            }
        }
    }
    Some(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Random samples: 8-bit levels, 16-bit levels, or floats with ties, signed zeros, values
    /// outside 0..1 and a NaN.
    fn noisy(w: usize, h: usize, kind: u32) -> Vec<f32> {
        let mut data = vec![0.0f32; w * h * N];
        let mut s = 0x2545_f491u32 ^ kind;
        for (i, v) in data.iter_mut().enumerate() {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            *v = match kind {
                // Full range in one half, a few levels (many ties) in the other.
                0 if i % 2 == 0 => (s % 256) as f32 / 255.0,
                0 => (s % 4 * 85) as f32 / 255.0,
                1 => (s % 65536) as f32 / 65535.0,
                _ => [0.0, -0.0, 0.25, 1.5, -2.0, (s % 1000) as f32 / 997.0][(s % 6) as usize],
            };
        }
        if kind == 2 {
            data[100] = f32::NAN;
        }
        data
    }

    #[test]
    fn median_histogram_matches_the_direct_selection() {
        let (w, h) = (46usize, 38usize);
        // The output reaches past the layer edge, where samples replicate the edge.
        let out = Rect::new(-4, -3, 40, 34);
        for kind in 0..3 {
            let buf = noisy(w, h, kind);
            let src = Src {
                buf: &buf,
                w: w as i32,
                h: h as i32,
            };
            for r in [2, 3, 7, 16] {
                for t in [None, Some(0.0), Some(10.0 / 255.0), Some(1.0)] {
                    let bits = |v: Option<Vec<f32>>| {
                        v.unwrap().into_iter().map(f32::to_bits).collect::<Vec<_>>()
                    };
                    let fast = bits(median_ranked(&src, out, r, t, &Interrupt::NONE));
                    let direct = bits(median_direct(&src, out, r, t, &Interrupt::NONE));
                    assert_eq!(fast, direct, "kind {kind} r {r} threshold {t:?}");
                }
            }
        }
    }

    /// R57: a flat opaque layer is unchanged everywhere, edges and corners included.
    #[test]
    fn flat_opaque_layer_is_unchanged_at_the_border() {
        let buf = [0.2f32, 0.5, 0.8, 1.0].repeat(12 * 9);
        let out = Rect::new(0, 0, 12, 9);
        for r in [1.0, 5.0] {
            for t in [None, Some(0.0)] {
                assert_eq!(
                    median(&buf, 12, 9, out, r, t, &Interrupt::NONE).unwrap(),
                    buf
                );
            }
        }
    }

    #[test]
    fn empty_out_returns_the_layer() {
        let buf = noisy(6, 6, 0);
        for r in [1.0, 3.0] {
            for out in [Rect::EMPTY, Rect::new(10, 10, 12, 12)] {
                assert_eq!(
                    median(&buf, 6, 6, out, r, None, &Interrupt::NONE).unwrap(),
                    buf
                );
            }
        }
    }

    #[test]
    fn cancelled_returns_none() {
        let buf = noisy(8, 8, 0);
        let flag = std::sync::atomic::AtomicBool::new(true);
        let out = Rect::new(0, 0, 8, 8);
        for r in [1.0, 3.0] {
            assert!(median(&buf, 8, 8, out, r, None, &Interrupt(Some(&flag))).is_none());
        }
    }
}
