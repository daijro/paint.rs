//! Ported from PhotoCraft crates/algo/src/other.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only Offset is here (Minimum and Maximum are not ported); it takes a whole straight RGBA f32 layer plus the selection bounds and `fx::distort::Edge` for the undefined-areas policy, and leaves pixels outside the bounds alone; the tests are rebuilt over such layers (upstream's use `Image` rectangles at negative coordinates, which a layer cannot have) with the same reference and the same distances.

use super::distort::Edge;
use crate::extra::geom::Rect;

/// Shifts the bounds' content by whole pixels.
pub fn offset(buf: &[f32], w: usize, b: Rect, dx: i32, dy: i32, edge: Edge) -> Vec<f32> {
    // Subtraction can leave i32 even for valid command parameters. Apply the edge
    // policy before narrowing: saturating first would change the wrapped pixel.
    let coordinate = |position: i32, distance: i32, start: i32, end: i32| -> Option<i32> {
        if start >= end {
            return None;
        }
        let p = i64::from(position) - i64::from(distance);
        let (start, end) = (i64::from(start), i64::from(end));
        let mapped = match edge {
            Edge::Wrap => start + (p - start).rem_euclid(end - start),
            Edge::Repeat => p.clamp(start, end - 1),
            Edge::Transparent if p >= start && p < end => p,
            Edge::Transparent => return None,
        };
        i32::try_from(mapped).ok()
    };
    let mut res = buf.to_vec();
    for y in b.y0..b.y1 {
        let sy = coordinate(y, dy, b.y0, b.y1);
        for x in b.x0..b.x1 {
            let sx = coordinate(x, dx, b.x0, b.x1);
            let o = (y as usize * w + x as usize) * 4;
            match (sx, sy) {
                (Some(sx), Some(sy)) => {
                    let s = (sy as usize * w + sx as usize) * 4;
                    res[o..o + 4].copy_from_slice(&buf[s..s + 4]);
                }
                _ => res[o..o + 4].fill(0.0),
            }
        }
    }
    res
}

#[cfg(test)]
mod offset_overflow_tests {
    use super::*;

    #[test]
    fn offset_limits_match_equivalent_small_offsets_for_every_edge_policy() {
        // A 3×5 area at the layer's origin and one inside a larger layer.
        for (w, h, bounds) in [
            (3, 5, Rect::new(0, 0, 3, 5)),
            (9, 12, Rect::new(4, 6, 7, 11)),
        ] {
            // Varying values and alpha; offset must only move samples.
            let src: Vec<f32> = (0..w * h * 4).map(|i| i as f32 * 0.125 - 2.0).collect();
            for policy in [Edge::Wrap, Edge::Repeat, Edge::Transparent] {
                for dx in [i32::MIN, i32::MAX, -8, 0, 7] {
                    for dy in [i32::MIN, i32::MAX, -9, 0, 6] {
                        let reduce = |d: i32, size: i32| match policy {
                            Edge::Wrap => d.rem_euclid(size),
                            _ => d.clamp(-100, 100),
                        };
                        let (rx, ry) = (reduce(dx, 3), reduce(dy, 5));
                        let mut expected = src.clone();
                        for y in bounds.y0..bounds.y1 {
                            for x in bounds.x0..bounds.x1 {
                                let (sx, sy) = (x - rx, y - ry);
                                let o = (y as usize * w + x as usize) * 4;
                                for c in 0..4 {
                                    expected[o + c] = if bounds.contains(sx, sy) {
                                        src[(sy as usize * w + sx as usize) * 4 + c]
                                    } else {
                                        match policy {
                                            Edge::Transparent => 0.0,
                                            Edge::Repeat => {
                                                let (cx, cy) = (
                                                    sx.clamp(bounds.x0, bounds.x1 - 1),
                                                    sy.clamp(bounds.y0, bounds.y1 - 1),
                                                );
                                                src[(cy as usize * w + cx as usize) * 4 + c]
                                            }
                                            Edge::Wrap => {
                                                let (cx, cy) = (
                                                    bounds.x0 + (sx - bounds.x0).rem_euclid(3),
                                                    bounds.y0 + (sy - bounds.y0).rem_euclid(5),
                                                );
                                                src[(cy as usize * w + cx as usize) * 4 + c]
                                            }
                                        }
                                    };
                                }
                            }
                        }
                        assert_eq!(
                            offset(&src, w, bounds, dx, dy, policy),
                            expected,
                            "{bounds:?} {policy:?} {dx} {dy}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn offset_wrap_accepts_minimum_signed_distance() {
        let bounds = Rect::new(0, 0, 3, 1);
        let src: Vec<f32> = [0.1, 0.5, 0.9].iter().flat_map(|&v| [v; 4]).collect();
        let got = offset(&src, 3, bounds, i32::MIN, 0, Edge::Wrap);
        let want: Vec<f32> = [0.9, 0.1, 0.5].iter().flat_map(|&v| [v; 4]).collect();
        assert_eq!(got, want);
    }
}
