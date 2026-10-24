//! Sampling primitives shared by the effects: RGSS offsets, the circle RLE kernel, the
//! linear-to-point kernel splat, continuous edge mapping, coverage, gamma boost and the
//! HighQuality Gaussian kernel.

use super::surface::Edge;
use std::array::from_fn;
use std::collections::BTreeMap;

/// F5: n = q² offsets, oᵢ = (t − trunc(t) − ½, y − ½) with y = (i + 1)/(n + 1) and t = y·q, in
/// f32. q ≤ 1 gives the single offset (0, 0).
pub fn rgss_offsets(q: u32) -> Vec<[f32; 2]> {
    let q = q.max(1);
    let n = q * q;
    (0..n)
        .map(|i| {
            let y = (i + 1) as f32 / (n + 1) as f32;
            let t = y * q as f32;
            [t - t.trunc() - 0.5, y - 0.5]
        })
        .collect()
}

/// F6: one (dy, dx_begin, len) run per row dy = −r..=r, covering dx² + dy² ≤ ⌊((2r + 1)² + 2)/4⌋.
pub fn circle_rows(r: u32) -> Vec<(i32, i32, u32)> {
    let c = ((2 * r + 1).pow(2) + 2) / 4;
    (-(r as i32)..=r as i32)
        .map(|dy| {
            let m = (c - dy.unsigned_abs().pow(2)).isqrt().min(r) as i32;
            (dy, -m, (2 * m + 1) as u32)
        })
        .collect()
}

/// F6 offsets (dx, dy), row by row, left to right.
pub fn circle_offsets(r: u32) -> Vec<(i32, i32)> {
    circle_rows(r)
        .into_iter()
        .flat_map(|(dy, x0, n)| (x0..x0 + n as i32).map(move |dx| (dx, dy)))
        .collect()
}

/// F7: weighted samples at fractional offsets become integer taps. Zero weights are dropped,
/// equal positions merged by summing in emission order (f64), the result sorted by (y, x).
pub fn splat(samples: &[([f64; 2], f64)]) -> Vec<([i32; 2], f64)> {
    let axis = |v: f64| {
        let u = v - 0.5;
        // h = l + 1 exactly: ⌊u + ½⌋ computed apart can round onto ⌊u − ½⌋ for |v| ≈ 1e−16
        // (trig residue) and count the sample twice.
        let l = (u - 0.5).floor() + 0.5;
        let h = l + 1.0;
        [
            ((l + 0.5) as i32, 1.0 - (u - l)),
            ((h + 0.5) as i32, 1.0 - (h - u)),
        ]
    };
    let mut taps = BTreeMap::new();
    for &([x, y], w) in samples {
        for (ty, wy) in axis(y) {
            for (tx, wx) in axis(x) {
                let t = wx * wy * w;
                if t != 0.0 {
                    *taps.entry((ty, tx)).or_insert(0.0) += t;
                }
            }
        }
    }
    taps.into_iter().map(|((y, x), w)| ([x, y], w)).collect()
}

/// F3 with T = (0, 0) and Z = `size`: the mapped sample position and its weight (below 1 only
/// for Transparent). Mirror here is the continuous reflection, not F2's texel duplication.
pub fn edge_map(s: [f32; 2], size: [f32; 2], edge: Edge) -> ([f32; 2], f32) {
    let clamp = |i: usize| s[i].clamp(0.5, size[i] - 0.5);
    match edge {
        Edge::Clamp => (from_fn(clamp), 1.0),
        Edge::Wrap => {
            let wrap = |i: usize| {
                let (v, z) = (s[i], size[i]);
                // quirk(pdn): effects-1.md §2.1 F3 — sign(0) = 0 maps s = 0 to Z/2.
                let sg = if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                };
                z * (1.0 - sg) / 2.0 + sg * ((v * sg) % z)
            };
            (from_fn(wrap), 1.0)
        }
        Edge::Mirror => {
            let mirror = |i: usize| {
                let (d, z) = (s[i].abs(), size[i]);
                let m = (d / z).floor() % 2.0;
                let w = d % z;
                (1.0 - m) * w + m * (z - w)
            };
            (from_fn(mirror), 1.0)
        }
        Edge::Transparent => {
            let c: [f32; 2] = from_fn(clamp);
            let k = |i: usize| (1.0 - (c[i] - s[i]).abs()).clamp(0.0, 1.0);
            (c, k(0) * k(1))
        }
    }
}

/// F10: the bilinear sample at `s` of an image that is 1 inside the layer and 0 outside.
pub fn coverage(s: [f32; 2], size: [f32; 2]) -> f32 {
    let axis = |v: f32, z: f32| {
        let u = v - 0.5;
        let i0 = u.floor();
        let f = u - i0;
        let inside = |i: f32| if i >= 0.0 && i < z { 1.0 } else { 0.0 };
        (1.0 - f) * inside(i0) + f * inside(i0 + 1.0)
    };
    axis(s[0], size[0]) * axis(s[1], size[1])
}

/// F9 forward: c ← sign(c)·|c|^γ on straight RGB; alpha unchanged; γ = 1 passes through.
pub fn gamma_boost(c: [f32; 4], gamma: f32) -> [f32; 4] {
    if gamma == 1.0 {
        return c;
    }
    let f = |v: f32| v.signum() * v.abs().powf(gamma);
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

/// F9 inverse: the forward step with exponent f32(1/γ).
pub fn gamma_unboost(c: [f32; 4], gamma: f32) -> [f32; 4] {
    gamma_boost(c, (1.0 / gamma as f64) as f32)
}

/// §2.3 Gaussian step 4, the HighQuality kernel: R = ⌈6σ⌉; g(x) at the 16R + 1 points
/// x = −R + k/8 in f64, each splat onto ⌊x⌋ and ⌈x⌉ in increasing x; normalised by the sum in
/// index order; cast to f32. σ ≤ 0 gives [1].
pub fn gaussian_kernel(sigma: f64) -> Vec<f32> {
    if sigma.is_nan() || sigma <= 0.0 {
        return vec![1.0];
    }
    let r = (6.0 * sigma).ceil() as i64;
    let mut w = vec![0.0f64; (2 * r + 1) as usize];
    let two_var = 2.0 * sigma * sigma;
    let norm = (std::f64::consts::PI * two_var).sqrt();
    for k in 0..=16 * r {
        let x = -r as f64 + k as f64 / 8.0;
        let g = (-(x * x) / two_var).exp() / norm;
        let (lo, hi) = (x.floor(), x.ceil());
        let at = |v: f64| (v as i64 + r) as usize;
        if lo == hi {
            w[at(lo)] += g;
        } else {
            w[at(lo)] += g * (1.0 - (x - lo));
            w[at(hi)] += g * (1.0 - (hi - x));
        }
    }
    let sum: f64 = w.iter().sum();
    w.iter().map(|&v| (v / sum) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(p, q)| (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6)
    }

    /// effects-1.md F5; effects-2.md §5 item 12.
    #[test]
    fn rgss() {
        assert!(close(&rgss_offsets(0), &[[0.0, 0.0]]));
        assert!(close(&rgss_offsets(1), &[[0.0, 0.0]]));
        assert!(close(
            &rgss_offsets(2),
            &[[-0.1, -0.3], [0.3, -0.1], [-0.3, 0.1], [0.1, 0.3]]
        ));
        assert!(close(
            &rgss_offsets(3),
            &[
                [-0.2, -0.4],
                [0.1, -0.3],
                [0.4, -0.2],
                [-0.3, -0.1],
                [0.0, 0.0],
                [0.3, 0.1],
                [-0.4, 0.2],
                [-0.1, 0.3],
                [0.2, 0.4],
            ]
        ));
    }

    /// F6: row lengths and tap counts.
    #[test]
    fn circle_kernel() {
        let lens = |r: u32| {
            circle_rows(r)
                .iter()
                .map(|&(_, _, n)| n)
                .collect::<Vec<_>>()
        };
        assert_eq!(lens(1), [3, 3, 3]);
        assert_eq!(lens(2), [3, 5, 5, 5, 3]);
        for (r, n) in [(1, 9), (2, 21), (3, 37), (6, 137), (10, 349)] {
            assert_eq!(circle_offsets(r).len(), n, "r {r}");
        }
        assert_eq!(circle_rows(2)[0], (-2, -1, 3));
        assert_eq!(circle_offsets(1)[0], (-1, -1));
    }

    /// F7: an integer offset lands on itself, a fractional one splits bilinearly.
    #[test]
    fn splat_taps() {
        assert_eq!(splat(&[([3.0, 0.0], 2.0)]), [([3, 0], 2.0)]);
        assert_eq!(
            splat(&[([3.25, 0.0], 1.0)]),
            [([3, 0], 0.75), ([4, 0], 0.25)]
        );
        assert_eq!(
            splat(&[([0.5, 0.0], 1.0), ([1.0, 0.0], 1.0)]),
            [([0, 0], 0.5), ([1, 0], 1.5)]
        );
        // v = −1.2e−16 (Motion Blur's cos 90° residue): u − ½ rounds to −1, so ⌊u − ½⌋ and
        // ⌊u + ½⌋ land on one texel; the sample must still weigh 1, not 2.
        for v in [-1.2246e-16, 1.2246e-16] {
            let t = splat(&[([v, 2.0], 1.0)]);
            assert!(
                (t.iter().map(|t| t.1).sum::<f64>() - 1.0).abs() < 1e-12,
                "{v}: {t:?}"
            );
        }
    }

    /// F3 edge mapping (Z = 10).
    #[test]
    fn edge_map_modes() {
        let x = |s: f32, e: Edge| edge_map([s, 5.0], [10.0, 10.0], e);
        assert_eq!(x(-3.0, Edge::Clamp), ([0.5, 5.0], 1.0));
        assert_eq!(x(11.0, Edge::Clamp), ([9.5, 5.0], 1.0));
        assert_eq!(x(12.5, Edge::Wrap).0[0], 2.5);
        assert_eq!(x(-2.5, Edge::Wrap).0[0], 7.5);
        assert_eq!(x(0.0, Edge::Wrap).0[0], 5.0); // quirk(pdn): s = 0 maps to Z/2 (effects-1.md F3)
        assert_eq!(x(12.5, Edge::Mirror).0[0], 7.5);
        assert_eq!(x(-2.5, Edge::Mirror).0[0], 2.5);
        assert_eq!(x(-0.25, Edge::Transparent), ([0.5, 5.0], 0.25));
    }

    /// F10: the coverage image is 1 inside the layer, 0 outside, sampled bilinearly.
    #[test]
    fn coverage_fades_over_the_outer_half_pixel() {
        assert_eq!(coverage([2.0, 2.0], [4.0, 4.0]), 1.0);
        assert_eq!(coverage([0.25, 2.0], [4.0, 4.0]), 0.75);
        assert_eq!(coverage([4.5, 2.0], [4.0, 4.0]), 0.0);
    }

    /// F9: γ = 1 is a passthrough; the inverse undoes the forward step.
    #[test]
    fn gamma_boost_round_trips() {
        let c = [0.25, 0.5, 1.0, 0.7];
        assert_eq!(gamma_boost(c, 1.0), c);
        let b = gamma_boost(c, 2.0);
        assert_eq!((b[0], b[3]), (0.0625, 0.7));
        let back = gamma_unboost(b, 2.0);
        assert!((back[0] - 0.25).abs() < 1e-6 && (back[1] - 0.5).abs() < 1e-6);
    }

    /// effects-1.md §2.3 Gaussian vectors (σ = 2/3 and σ = 1), to 1e−7.
    #[test]
    fn gaussian_high_quality_kernel() {
        let w = gaussian_kernel(2.0 / 3.0);
        let want = [
            4.32009e-7,
            2.453695e-4,
            0.018793421,
            0.22685628,
            0.508208994,
            0.22685628,
            0.018793421,
            2.453695e-4,
            4.32009e-7,
        ];
        assert_eq!(w.len(), 9);
        for (got, want) in w.iter().zip(want) {
            assert!((*got as f64 - want).abs() < 1e-7, "{w:?}");
        }
        let w = gaussian_kernel(1.0);
        assert_eq!(w.len(), 13);
        for (got, want) in
            w[6..10]
                .iter()
                .zip([0.369155975, 0.240842175, 0.066535691, 0.00767470053])
        {
            assert!((*got as f64 - want).abs() < 1e-7, "{w:?}");
        }
        assert_eq!(gaussian_kernel(0.0), [1.0]);
    }
}
