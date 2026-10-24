//! A CABAC *encoder*, transcribed from clause 9.3.4.4.
//!
//! It exists only to let the tests generate conforming bitstreams: the decoder
//! is checked by encoding known bin sequences and decoding them back, and the
//! golden-vector test builds a complete synthetic picture with it.

use super::ctx::NUM_CTX;
use super::tables::{RANGE_TAB_LPS, TRANS_IDX_LPS, TRANS_IDX_MPS};
use alloc::vec;
use alloc::vec::Vec;

/// Arithmetic encoder from clause 9.3.4.4.
pub struct Enc {
    low: u32,
    range: u32,
    first: bool,
    outstanding: u32,
    bits: Vec<u8>,
    ctx: [u8; NUM_CTX],
}

impl Enc {
    pub fn new(qp: i32) -> Enc {
        Enc {
            low: 0,
            range: 510,
            first: true,
            outstanding: 0,
            bits: Vec::new(),
            ctx: super::initial_contexts(qp),
        }
    }

    fn put_bit(&mut self, b: u8) {
        if self.first {
            self.first = false;
        } else {
            self.bits.push(b);
        }
        while self.outstanding > 0 {
            self.bits.push(1 - b);
            self.outstanding -= 1;
        }
    }

    fn renorm(&mut self) {
        while self.range < 256 {
            if self.low < 256 {
                self.put_bit(0);
            } else if self.low >= 512 {
                self.low -= 512;
                self.put_bit(1);
            } else {
                self.low -= 256;
                self.outstanding += 1;
            }
            self.range <<= 1;
            self.low <<= 1;
        }
    }

    pub fn decision(&mut self, idx: usize, bin: u32) {
        let s = self.ctx[idx];
        let state = (s >> 1) as usize;
        let mps = (s & 1) as u32;
        let q = ((self.range >> 6) & 3) as usize;
        let lps = RANGE_TAB_LPS[state][q] as u32;
        self.range -= lps;
        if bin != mps {
            self.low += self.range;
            self.range = lps;
            let new_mps = if state == 0 { 1 - mps } else { mps };
            self.ctx[idx] = (TRANS_IDX_LPS[state] << 1) | new_mps as u8;
        } else {
            self.ctx[idx] = (TRANS_IDX_MPS[state] << 1) | mps as u8;
        }
        self.renorm();
    }

    pub fn bypass(&mut self, bin: u32) {
        self.low <<= 1;
        if bin != 0 {
            self.low += self.range;
        }
        if self.low >= 1024 {
            self.put_bit(1);
            self.low -= 1024;
        } else if self.low < 512 {
            self.put_bit(0);
        } else {
            self.low -= 512;
            self.outstanding += 1;
        }
    }

    pub fn terminate(&mut self, bin: u32) {
        self.range -= 2;
        if bin != 0 {
            self.low += self.range;
            self.range = 2;
            self.renorm();
            self.put_bit(((self.low >> 9) & 1) as u8);
            let tail = ((self.low >> 7) & 3) | 1;
            self.bits.push(((tail >> 1) & 1) as u8);
            self.bits.push((tail & 1) as u8);
        } else {
            self.renorm();
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        while self.bits.len() % 8 != 0 {
            self.bits.push(0);
        }
        let mut out = vec![0u8; self.bits.len() / 8];
        for (i, &b) in self.bits.iter().enumerate() {
            out[i / 8] |= b << (7 - (i % 8));
        }
        out
    }
}
