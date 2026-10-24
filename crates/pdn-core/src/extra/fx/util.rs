//! Ported from PhotoCraft crates/algo/src/noise.rs (`hash01`) and crates/algo/src/fxutil.rs (`smoothstep`, `gauss_box_radii`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only these three helpers, gathered from the effect ports that each inlined a copy (bit-identical to them).

/// Deterministic hash → `[0, 1)` from document coordinates.
#[inline]
pub(crate) fn hash01(x: i32, y: i32, c: u32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ c.wrapping_mul(0xcb1a_b31f)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Hermite step from `e0` up to `e1`; `e1 <= e0` steps at `e1`. (Artistic's own `smoothstep`
/// falls from `e0` to `e1` instead.)
pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Odd box widths whose 3-fold convolution approximates a Gaussian of `sigma` (Kovesi 2010), as
/// radii.
pub(crate) fn gauss_box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0))
        .round()
        .clamp(0.0, n) as usize;
    let mut radii = std::array::from_fn(|i| {
        if i < m {
            (wl as usize - 1) / 2
        } else {
            (wu as usize - 1) / 2
        }
    });
    // Kovesi's rounded triple degenerates below sigma ~ 0.577: every radius becomes 0. One
    // radius-1 pass is the smallest real blur.
    if radii.iter().all(|&r| r == 0) {
        radii[0] = 1;
    }
    radii
}
