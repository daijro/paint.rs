//! Intra prediction (clause 8.4.4.2).

mod pred;
mod smooth;

pub use pred::predict;
pub use smooth::filter_refs;

/// Neighbouring reference samples for one transform block.
///
/// Storage follows the usual flattened convention: index `2n` holds the corner
/// sample `p[-1][-1]`, indices `2n+1 ..= 4n` hold `p[0][-1] ..= p[2n-1][-1]`
/// and indices `2n-1 ..= 0` hold `p[-1][0] ..= p[-1][2n-1]`.
#[derive(Clone, Debug)]
pub struct Refs {
    /// Flattened reference samples; only `4 * n + 1` entries are meaningful.
    pub buf: [u16; 129],
    /// Transform block size in samples, one of 4, 8, 16 or 32.
    pub n: usize,
}

impl Refs {
    /// An all-zero reference set for a block of `n` samples a side.
    pub fn new(n: usize) -> Refs {
        Refs {
            buf: [0u16; 129],
            n,
        }
    }

    /// `p[-1][y]` for `y` in `-1 ..= 2n-1`.
    #[inline]
    pub fn left(&self, y: isize) -> u16 {
        self.buf[(2 * self.n as isize - 1 - y) as usize]
    }

    /// `p[x][-1]` for `x` in `-1 ..= 2n-1`.
    #[inline]
    pub fn top(&self, x: isize) -> u16 {
        self.buf[(2 * self.n as isize + 1 + x) as usize]
    }

    /// Writes `p[-1][y]`.
    #[inline]
    pub fn set_left(&mut self, y: isize, v: u16) {
        let i = (2 * self.n as isize - 1 - y) as usize;
        self.buf[i] = v;
    }

    /// Writes `p[x][-1]`.
    #[inline]
    pub fn set_top(&mut self, x: isize, v: u16) {
        let i = (2 * self.n as isize + 1 + x) as usize;
        self.buf[i] = v;
    }
}
