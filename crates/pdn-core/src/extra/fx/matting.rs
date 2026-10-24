//! Ported from PhotoCraft crates/algo/src/matting.rs, crates/algo/src/selection/distance.rs and crates/engine/src/layer_menu_cmds.rs (`defringe`, `remove_matte`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only `box_mean`, `decontaminate`, the Euclidean distance transform, `defringe` and `remove_matte`, on an interleaved straight RGBA f32 buffer (alpha is the layer's own, so no `Region`/`Surface`); decontaminate's tiles run serially and poll an `Interrupt`; the distance transform is serial; Select-and-Mask refine, the edge brush and its `MAX_REFINE_AREA` guard (which sizes a refine's dense `Region`, so nothing here allocates it) dropped; the tests are new, after upstream's `defringe_pure_function` and `decontaminate_and_masked_copy_any_depth`.

use crate::extra::interrupt::Interrupt;

const TILE: usize = 256;
const NO_SEED: u32 = u32::MAX;
// A mask with no seed gets a finite distance everywhere, as upstream's does.
const EMPTY_DISTANCE: f32 = 1e10;

/// Euclidean distance to the nearest `true` pixel, row-major (Felzenszwalb and Huttenlocher).
fn edt(inside: &[bool], w: usize, h: usize) -> Vec<f32> {
    let n = w * h;
    if n == 0 || inside.len() != n {
        return Vec::new();
    }
    if !inside.iter().any(|s| *s) {
        return vec![EMPTY_DISTANCE; n];
    }
    // Vertical distance to the nearest seed in each column.
    let mut vertical = vec![NO_SEED; n];
    for x in 0..w {
        let mut nearest = None;
        for y in 0..h {
            if inside[y * w + x] {
                nearest = Some(y);
            }
            if let Some(s) = nearest {
                vertical[y * w + x] = (y - s) as u32;
            }
        }
        nearest = None;
        for y in (0..h).rev() {
            if inside[y * w + x] {
                nearest = Some(y);
            }
            if let Some(s) = nearest {
                let d = &mut vertical[y * w + x];
                *d = (*d).min((s - y) as u32);
            }
        }
    }
    let mut out = vec![0.0f32; n];
    let (mut sites, mut starts) = (vec![0usize; w], vec![0.0f64; w]);
    for (row, dst) in vertical.chunks_exact(w).zip(out.chunks_exact_mut(w)) {
        row_distance(row, dst, &mut sites, &mut starts);
    }
    out
}

/// Lower envelope of parabolas with integer vertical distances along one row.
fn row_distance(costs: &[u32], out: &mut [f32], sites: &mut [usize], starts: &mut [f64]) {
    let mut count = 0usize;
    for (q, &distance) in costs.iter().enumerate().filter(|(_, d)| **d != NO_SEED) {
        let fq = f64::from(distance).powi(2);
        let mut start = f64::NEG_INFINITY;
        while count > 0 {
            let p = sites[count - 1];
            let dp = costs[p];
            start =
                (fq - f64::from(dp).powi(2)) / (2.0 * (q - p) as f64) + (q as f64 + p as f64) * 0.5;
            if start > starts[count - 1] {
                break;
            }
            count -= 1;
        }
        if count == 0 {
            start = f64::NEG_INFINITY;
        }
        sites[count] = q;
        starts[count] = start;
        count += 1;
    }
    if count == 0 {
        out.fill(EMPTY_DISTANCE);
        return;
    }
    let mut k = 0usize;
    for (x, d) in out.iter_mut().enumerate() {
        while k + 1 < count && starts[k + 1] < x as f64 {
            k += 1;
        }
        let dx = x as f64 - sites[k] as f64;
        *d = (dx * dx + f64::from(costs[sites[k]]).powi(2)).sqrt() as f32;
    }
}

/// Defringe: pixels within `width` of transparency (alpha <= 1%) take the colour of the nearest
/// pixels further inside, grown outward one pixel at a time. Alpha is unchanged.
pub fn defringe(buf: &mut [f32], w: usize, h: usize, width: usize, stop: Interrupt) {
    let source: Vec<[f32; 4]> = buf.as_chunks::<4>().0.to_vec();
    let px = buf.as_chunks_mut::<4>().0;
    let outside: Vec<bool> = px.iter().map(|q| q[3] <= 0.01).collect();
    if !outside.iter().any(|o| *o) {
        return;
    }
    let dist = edt(&outside, w, h);
    let mut known: Vec<bool> = (0..px.len())
        .map(|i| !outside[i] && dist[i] > width as f32)
        .collect();
    let mut todo: Vec<usize> = (0..px.len())
        .filter(|&i| !outside[i] && !known[i])
        .collect();
    while !todo.is_empty() {
        if stop.cancelled() {
            // A cancelled run returns the source.
            px.copy_from_slice(&source);
            return;
        }
        let mut next = Vec::new();
        let mut updates = Vec::new();
        for &i in &todo {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            let mut acc = [0.0f32; 3];
            let mut n = 0.0;
            for (dx, dy) in [
                (-1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
                (1, 1),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if known[j] {
                    for c in 0..3 {
                        acc[c] += px[j][c];
                    }
                    n += 1.0;
                }
            }
            if n > 0.0 {
                updates.push((i, acc.map(|v| v / n)));
            } else {
                next.push(i);
            }
        }
        if updates.is_empty() {
            break; // isolated fringe with no core colour nearby
        }
        for (i, c) in updates {
            px[i][..3].copy_from_slice(&c);
            known[i] = true;
        }
        todo = next;
    }
}

/// Remove Black / White Matte: un-premultiplies colours that were composited over black or
/// white. Only partly transparent pixels change.
pub fn remove_matte(buf: &mut [f32], white: bool) {
    for q in buf
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .filter(|q| q[3] > 0.0 && q[3] < 1.0)
    {
        let a = q[3];
        for v in &mut q[..3] {
            *v = if white { (*v - (1.0 - a)) / a } else { *v / a }.clamp(0.0, 1.0);
        }
    }
}

/// Box mean with radius `r` (window clipped to the image, divided by the pixel count).
fn box_mean(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0.0f32; w * h];
    let mut pre = vec![0.0f64; w.max(h) + 1];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            pre[x + 1] = pre[x] + f64::from(row[x]);
        }
        for x in 0..w {
            let (lo, hi) = (x.saturating_sub(r), (x + r + 1).min(w));
            tmp[y * w + x] = ((pre[hi] - pre[lo]) / (hi - lo) as f64) as f32;
        }
    }
    let mut out = vec![0.0f32; w * h];
    let mut col = vec![0.0f64; (h + 1) * w];
    for y in 0..h {
        for x in 0..w {
            col[(y + 1) * w + x] = col[y * w + x] + f64::from(tmp[y * w + x]);
        }
    }
    for y in 0..h {
        let (lo, hi) = (y.saturating_sub(r), (y + r + 1).min(h));
        let n = (hi - lo) as f64;
        for x in 0..w {
            out[y * w + x] = ((col[hi * w + x] - col[lo * w + x]) / n) as f32;
        }
    }
    out
}

/// Color Decontaminate: each soft-alpha pixel (0.02 < a < 0.98) moves towards the alpha-weighted
/// average colour of nearby opaque pixels (a >= 0.98; normalised convolution with a box of radius
/// `max(radius, 3)`, widened x4 where none is near), by `amount`% scaled by its transparency.
/// Alpha is unchanged. Tiles of 256 without a soft pixel are skipped.
// ponytail: tiles run serially; par over tiles if 24 MP layers feel slow.
pub fn decontaminate(
    buf: &mut [f32],
    w: usize,
    h: usize,
    radius: f32,
    amount: f32,
    stop: Interrupt,
) {
    let rad = radius.max(3.0).ceil() as usize;
    let halo = rad * 4;
    let amount = amount.clamp(0.0, 100.0) / 100.0;
    let soft = |a: f32| a > 0.02 && a < 0.98;
    let src = buf.to_vec();
    let at = |x: usize, y: usize| src[(y * w + x) * 4 + 3];
    for ty in (0..h).step_by(TILE) {
        for tx in (0..w).step_by(TILE) {
            if stop.cancelled() {
                return;
            }
            let (tx1, ty1) = ((tx + TILE).min(w), (ty + TILE).min(h));
            if !(ty..ty1).any(|y| (tx..tx1).any(|x| soft(at(x, y)))) {
                continue;
            }
            let (rx0, ry0) = (tx.saturating_sub(halo), ty.saturating_sub(halo));
            let (rx1, ry1) = ((tx1 + halo).min(w), (ty1 + halo).min(h));
            let (rw, rh) = (rx1 - rx0, ry1 - ry0);
            let idx = |i: usize| ((ry0 + i / rw) * w + rx0 + i % rw) * 4;
            let wts: Vec<f32> = (0..rw * rh)
                .map(|i| {
                    let a = src[idx(i) + 3];
                    if a >= 0.98 { a } else { 0.0 }
                })
                .collect();
            let den = box_mean(&wts, rw, rh, rad);
            let den2 = box_mean(&wts, rw, rh, rad * 4);
            let nums: Vec<_> = (0..3)
                .map(|c| {
                    let v: Vec<f32> = (0..rw * rh).map(|i| wts[i] * src[idx(i) + c]).collect();
                    (box_mean(&v, rw, rh, rad), box_mean(&v, rw, rh, rad * 4))
                })
                .collect();
            for y in ty..ty1 {
                for x in tx..tx1 {
                    let a = at(x, y);
                    if !soft(a) {
                        continue;
                    }
                    let k = amount * ((1.0 - a) * 4.0).clamp(0.0, 1.0);
                    if k == 0.0 {
                        continue;
                    }
                    let i = (y - ry0) * rw + (x - rx0);
                    for (c, (n1, n2)) in nums.iter().enumerate() {
                        let v = src[(y * w + x) * 4 + c];
                        let f = if den[i] > 1e-4 {
                            n1[i] / den[i]
                        } else if den2[i] > 1e-4 {
                            n2[i] / den2[i]
                        } else {
                            v
                        };
                        buf[(y * w + x) * 4 + c] = v + (f - v) * k;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream's `defringe_pure_function`: 5x1, transparent, blue fringe, red core.
    #[test]
    fn defringe_pure_function() {
        let mut px = [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.5],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        ]
        .concat();
        defringe(&mut px, 5, 1, 1, Interrupt::NONE);
        assert_eq!(&px[4..8], [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(px[3], 0.0);
    }

    /// A cancelled defringe returns the source untouched.
    #[test]
    fn defringe_cancel_returns_source() {
        use std::sync::atomic::AtomicBool;
        let src: Vec<f32> = [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.5],
            [1.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
        ]
        .concat();
        let (mut done, mut stopped) = (src.clone(), src.clone());
        defringe(&mut done, 4, 1, 1, Interrupt::NONE);
        assert_ne!(done, src);
        let flag = AtomicBool::new(true);
        defringe(&mut stopped, 4, 1, 1, Interrupt(Some(&flag)));
        assert_eq!(stopped, src);
    }

    #[test]
    fn edt_is_euclidean() {
        // One seed at (0,0) on a 4x3 grid.
        let mut seed = vec![false; 12];
        seed[0] = true;
        let d = edt(&seed, 4, 3);
        assert_eq!(d[0], 0.0);
        assert_eq!(d[3], 3.0);
        assert!((d[2 * 4 + 3] - 13f32.sqrt()).abs() < 1e-5);
        assert_eq!(edt(&[false; 4], 2, 2), vec![EMPTY_DISTANCE; 4]);
    }

    /// Upstream's `decontaminate_and_masked_copy_any_depth`, RGBA8 half: the fringe pixel
    /// takes the red foreground; opaque pixels are untouched.
    #[test]
    fn decontaminate_pulls_fringe_to_the_core() {
        let (w, h) = (20, 8);
        let mut buf = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                buf.extend(match x {
                    0..=9 => [1.0, 0.0, 0.0, 1.0],
                    10 => [0.5, 0.0, 0.5, 128.0 / 255.0],
                    _ => [0.0, 0.0, 1.0, 0.0],
                });
            }
        }
        let before = buf.clone();
        decontaminate(&mut buf, w, h, 3.0, 100.0, Interrupt::NONE);
        let p = &buf[(4 * w + 10) * 4..][..4];
        assert!(p[0] > 0.9 && p[2] < 0.1, "{p:?}");
        assert_eq!(p[3], before[(4 * w + 10) * 4 + 3]);
        assert_eq!(
            buf[(4 * w + 15) * 4..][..4],
            before[(4 * w + 15) * 4..][..4]
        );
        assert_eq!(buf[(3 * w + 3) * 4..][..4], before[(3 * w + 3) * 4..][..4]);
    }
}
