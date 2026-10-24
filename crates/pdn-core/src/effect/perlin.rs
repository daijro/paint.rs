//! Ken Perlin's improved noise (2002) in 2-D: his 3-D noise at z = 0, in f32.
//! The reference permutation; Dents and Clouds' N use it.

/// Ken Perlin's reference permutation; P[i] = PERM[i & 255] is Paint.NET's 512-entry duplicate.
#[rustfmt::skip]
pub const PERM: [u8; 256] = [
    151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10,
    23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56,
    87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60,
    211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187,
    208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124,
    123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170,
    213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79,
    113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51,
    145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254,
    138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180,
];

fn p(i: i32) -> i32 {
    PERM[(i & 255) as usize] as i32
}

/// t³(t(6t − 15) + 10).
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

/// Perlin's g(h, x, y) at z = 0 as a·x + b·y: h &= 15; u = h < 8 ? x : y; v = h < 4 ? y :
/// (h ∈ {12, 14} ? x : 0); ±u ± v by bits 0 and 1. Equal to the branchy form up to the sign of 0.
const GRAD: [(f32, f32); 16] = [
    (1.0, 1.0),
    (-1.0, 1.0),
    (1.0, -1.0),
    (-1.0, -1.0),
    (1.0, 0.0),
    (-1.0, 0.0),
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (1.0, 1.0),
    (0.0, -1.0),
    (-1.0, 1.0),
    (0.0, -1.0),
];

fn grad(h: i32, x: f32, y: f32) -> f32 {
    let (a, b) = GRAD[(h & 15) as usize];
    a * x + b * y
}

/// N at lattice cell (ix, iy) and fraction (fx, fy): A = P[ix + s] + iy, AA = P[A], AB = P[A + 1],
/// B = P[ix + 1 + s] + iy, BA = P[B], BB = P[B + 1]; the gradient hashes are P[AA] … P[BB]; lerp in
/// x, then y.
pub fn noise(ix: i32, iy: i32, fx: f32, fy: f32, seed: u8) -> f32 {
    let s = seed as i32;
    let (u, v) = (fade(fx), fade(fy));
    let a = p(ix + s) + iy;
    let b = p(ix + 1 + s) + iy;
    let (aa, ab, ba, bb) = (p(a), p(a + 1), p(b), p(b + 1));
    lerp(
        lerp(grad(p(aa), fx, fy), grad(p(ba), fx - 1.0, fy), u),
        lerp(
            grad(p(ab), fx, fy - 1.0),
            grad(p(bb), fx - 1.0, fy - 1.0),
            u,
        ),
        v,
    )
}

/// N(x, y) = noise(⌊x⌋ & 255, ⌊y⌋ & 255, x − ⌊x⌋, y − ⌊y⌋, seed).
pub fn noise_at(x: f32, y: f32, seed: u8) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    noise(fx as i32 & 255, fy as i32 & 255, x - fx, y - fy, seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// effects-1.md §5 Task 40 Dents noise vectors (f32; the second within 1e−5).
    #[test]
    fn improved_noise_vectors() {
        assert_eq!(noise_at(0.25, 0.75, 0), -0.077_636_72);
        assert!((noise_at(3.6, 1.2, 7) - 0.671_394_77).abs() < 1e-5);
        assert_eq!((PERM[0], PERM[255]), (151, 180));
        let mut sorted = PERM;
        sorted.sort();
        assert!(sorted.iter().enumerate().all(|(i, &v)| v as usize == i));
    }

    /// Vectors from an independent f32 port of effects-1.md §2.5 Dents step 8 / effects-2.md §2.8
    /// step 4: negative lattice cells floor, the seed offsets x only, (x, y) are not symmetric,
    /// and the 512-entry wrap.
    #[test]
    fn noise_lattice_seed_and_wrap() {
        let cases = [
            ((-1.3, 7.9, 255), 0.258_926_8),
            ((-0.6, -2.2, 1), 0.381_218_8),
            ((250.5, 3.25, 9), 0.047_363_28),
            ((3.25, 250.5, 9), -0.396_484_38),
            ((0.75, 0.25, 0), -0.224_121_1),
            ((17.125, 99.875, 200), -0.124_742_33),
        ];
        for ((x, y, s), want) in cases {
            assert_eq!(noise_at(x, y, s), want, "N({x}, {y}; {s})");
        }
        // Clouds passes masked lattice indices; any i32 lands on the same cell.
        assert_eq!(noise(300, -5, 0.5, 0.25, 3), noise(44, 251, 0.5, 0.25, 3));
    }
}
