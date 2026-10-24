//! Ported from PhotoCraft crates/engine/src/extra_cmds.rs (`average`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: the per-pixel coverage is a closure and the region a `Rect` over a whole interleaved straight RGBA f32 layer; the engine's session plumbing is dropped; the unit tests are new.

use crate::extra::geom::Rect;

/// Filter > Blur > Average: `region` filled with its mean colour. Colour is weighted by alpha
/// times `cover(x, y)` (0-1, the selection), alpha is the coverage-weighted mean. A region with
/// no opaque, covered pixel is left as it was.
pub fn average(buf: &[f32], w: usize, region: Rect, cover: impl Fn(i32, i32) -> f32) -> Vec<f32> {
    let mut res = buf.to_vec();
    let (mut sum, mut wsum, mut asum, mut n) = ([0f64; 3], 0f64, 0f64, 0f64);
    let at = |x: i32, y: i32| (y as usize * w + x as usize) * 4;
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let (p, k) = (&buf[at(x, y)..at(x, y) + 4], cover(x, y));
            let wt = f64::from(p[3] * k);
            for c in 0..3 {
                sum[c] += f64::from(p[c]) * wt;
            }
            wsum += wt;
            asum += wt;
            n += f64::from(k);
        }
    }
    if wsum <= 0.0 {
        return res;
    }
    let avg = [
        (sum[0] / wsum) as f32,
        (sum[1] / wsum) as f32,
        (sum[2] / wsum) as f32,
        (asum / n.max(1e-9)) as f32,
    ];
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            res[at(x, y)..at(x, y) + 4].copy_from_slice(&avg);
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Red and blue halves average to purple; pixels outside the region stay.
    #[test]
    fn average_and_region() {
        let mut buf = vec![0.0f32; 4 * 4 * 4];
        for (i, px) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *px = if i % 4 < 2 {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                [0.0, 0.0, 1.0, 1.0]
            };
        }
        let out = average(&buf, 4, Rect::new(0, 0, 4, 2), |_, _| 1.0);
        for i in 0..8 {
            assert_eq!(out[i * 4..i * 4 + 4], [0.5, 0.0, 0.5, 1.0]);
        }
        assert_eq!(out[8 * 4..], buf[8 * 4..]);
    }

    #[test]
    fn transparent_region_is_untouched() {
        let buf = [0.3f32, 0.2, 0.1, 0.0].repeat(4);
        assert_eq!(average(&buf, 2, Rect::new(0, 0, 2, 2), |_, _| 1.0), buf);
    }
}
