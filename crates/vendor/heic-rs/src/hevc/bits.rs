//! RBSP bit reader: `u(n)`, `ue(v)`, `se(v)`, alignment and `more_rbsp_data`.
//!
//! The reader operates on a raw byte slice that has already had emulation
//! prevention bytes removed (see [`crate::hevc::nal`]).

use crate::hevc::error::{Error, Result};

/// Big-endian MSB-first bit reader over an RBSP.
#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Next bit to read, counted from the first bit of `data`.
    pos: usize,
    /// Total number of bits in `data`.
    end: usize,
    /// Bit position of the `rbsp_stop_one_bit`, or `0` when the RBSP is empty.
    stop: usize,
}

impl<'a> BitReader<'a> {
    /// Creates a reader positioned at the first bit of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        let mut last = data.len();
        while last > 0 && data[last - 1] == 0 {
            last -= 1;
        }
        let stop = if last == 0 {
            0
        } else {
            (last - 1) * 8 + (7 - data[last - 1].trailing_zeros() as usize)
        };
        Self {
            data,
            pos: 0,
            end: data.len() * 8,
            stop,
        }
    }

    /// Reads a single bit.
    #[inline]
    pub fn u1(&mut self) -> Result<u32> {
        if self.pos >= self.end {
            return Err(Error::Truncated);
        }
        let byte = self.data[self.pos >> 3];
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Ok(bit as u32)
    }

    /// Reads `n` bits (`n <= 32`) as an unsigned integer, `u(n)`.
    pub fn u(&mut self, n: u32) -> Result<u32> {
        debug_assert!(n <= 32);
        if n == 0 {
            return Ok(0);
        }
        if n > 32 {
            return Err(Error::InvalidData("u(n) with n > 32"));
        }
        if self.pos + n as usize > self.end {
            return Err(Error::Truncated);
        }
        let mut v: u32 = 0;
        let mut left = n as usize;
        while left > 0 {
            let byte = self.data[self.pos >> 3];
            let off = self.pos & 7;
            let avail = 8 - off;
            let take = if left < avail { left } else { avail };
            let chunk = (byte as u32 >> (avail - take)) & ((1u32 << take) - 1);
            v = (v << take) | chunk;
            self.pos += take;
            left -= take;
        }
        Ok(v)
    }

    /// Reads an unsigned Exp-Golomb code, `ue(v)`.
    pub fn ue(&mut self) -> Result<u32> {
        let mut zeros = 0u32;
        while self.u1()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return Err(Error::InvalidData("ue(v) prefix too long"));
            }
        }
        if zeros == 0 {
            return Ok(0);
        }
        let rest = self.u(zeros)?;
        Ok((1u32 << zeros) - 1 + rest)
    }

    /// Reads a signed Exp-Golomb code, `se(v)`.
    pub fn se(&mut self) -> Result<i32> {
        let k = self.ue()?;
        let mag = ((k + 1) >> 1) as i32;
        Ok(if k & 1 == 1 { mag } else { -mag })
    }

    /// Skips `n` bits.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        if self.pos + n > self.end {
            return Err(Error::Truncated);
        }
        self.pos += n;
        Ok(())
    }

    /// Advances to the next byte boundary.
    pub fn byte_align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }

    /// True when the current position is byte aligned.
    #[cfg(test)]
    pub fn is_aligned(&self) -> bool {
        self.pos & 7 == 0
    }

    /// `more_rbsp_data()` from clause 7.2 of the specification.
    pub fn more_rbsp_data(&self) -> bool {
        self.pos < self.stop
    }

    /// Current position rounded up to a whole number of bytes.
    pub fn byte_pos(&self) -> usize {
        (self.pos + 7) >> 3
    }
}

/// Writes bits MSB-first; used only by the synthetic-stream builder.
#[cfg(any(test, feature = "bench"))]
#[derive(Default, Debug)]
pub struct BitWriter {
    pub bytes: alloc::vec::Vec<u8>,
    nbits: usize,
}

#[cfg(any(test, feature = "bench"))]
impl BitWriter {
    /// Appends the low `n` bits of `v`.
    pub fn put(&mut self, v: u32, n: u32) {
        for i in (0..n).rev() {
            if self.nbits & 7 == 0 {
                self.bytes.push(0);
            }
            let bit = ((v >> i) & 1) as u8;
            let idx = self.nbits >> 3;
            self.bytes[idx] |= bit << (7 - (self.nbits & 7));
            self.nbits += 1;
        }
    }

    /// Appends an unsigned Exp-Golomb code.
    pub fn ue(&mut self, v: u32) {
        let c = v + 1;
        let n = 32 - c.leading_zeros();
        self.put(0, n - 1);
        self.put(c, n);
    }

    /// Appends a signed Exp-Golomb code.
    pub fn se(&mut self, v: i32) {
        let k = if v > 0 {
            (v as u32) * 2 - 1
        } else {
            (-v as u32) * 2
        };
        self.ue(k);
    }

    /// Appends the RBSP trailing bits.
    pub fn rbsp_trailing(&mut self) {
        self.put(1, 1);
        while self.nbits & 7 != 0 {
            self.put(0, 1);
        }
    }

    /// Number of bits written so far.
    pub fn len_bits(&self) -> usize {
        self.nbits
    }
}

#[cfg(test)]
#[path = "bits_tests.rs"]
mod tests;
