//! A non-allocating, panic-free cursor over a byte slice.
//!
//! Every accessor is bounds-checked and returns [`Error::Truncated`] rather
//! than panicking, which is what lets the whole crate hold to its
//! no-`unwrap`, no-`panic!` rule while parsing hostile input.

use crate::error::{Error, Result};

/// A forward-only cursor over borrowed bytes.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Wrap a slice.
    pub const fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    /// Bytes not yet consumed.
    pub const fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Offset of the cursor from the start of the wrapped slice.
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// True when every byte has been consumed.
    pub const fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// The not-yet-consumed bytes, without consuming them.
    pub fn rest(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// Take `n` bytes, or fail.
    pub fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated(what))?;
        if end > self.data.len() {
            return Err(Error::Truncated(what));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    /// Advance by `n` bytes, or fail.
    pub fn skip(&mut self, n: usize, what: &'static str) -> Result<()> {
        self.take(n, what).map(|_| ())
    }

    /// Read one byte.
    pub fn u8(&mut self, what: &'static str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }

    /// Read a big-endian `u16`.
    pub fn u16(&mut self, what: &'static str) -> Result<u16> {
        let b = self.take(2, what)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    /// Read a big-endian 24-bit unsigned value.
    pub fn u24(&mut self, what: &'static str) -> Result<u32> {
        let b = self.take(3, what)?;
        Ok(u32::from_be_bytes([0, b[0], b[1], b[2]]))
    }

    /// Read a big-endian `u32`.
    pub fn u32(&mut self, what: &'static str) -> Result<u32> {
        let b = self.take(4, what)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Read a big-endian `u64`.
    pub fn u64(&mut self, what: &'static str) -> Result<u64> {
        let b = self.take(8, what)?;
        Ok(u64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Read a four-character code.
    pub fn fourcc(&mut self, what: &'static str) -> Result<[u8; 4]> {
        let b = self.take(4, what)?;
        Ok([b[0], b[1], b[2], b[3]])
    }

    /// Read an unsigned big-endian integer of `size` bytes, where `size` is
    /// one of 0, 4 or 8 — the widths ISOBMFF uses for offsets and lengths.
    /// A size of 0 yields 0 without consuming anything, which is how `iloc`
    /// spells "this field is absent".
    pub fn uint(&mut self, size: u8, what: &'static str) -> Result<u64> {
        match size {
            0 => Ok(0),
            4 => Ok(u64::from(self.u32(what)?)),
            8 => self.u64(what),
            _ => Err(Error::Malformed("integer field width must be 0, 4 or 8")),
        }
    }

    /// Read the version and flags of a `FullBox` header.
    pub fn full_box(&mut self, what: &'static str) -> Result<(u8, u32)> {
        let version = self.u8(what)?;
        let flags = self.u24(what)?;
        Ok((version, flags))
    }

    /// Read a NUL-terminated UTF-8 string, consuming the terminator.
    ///
    /// A string that runs to the end of the slice without a NUL is accepted,
    /// because real files written by real encoders sometimes do that.
    pub fn cstr(&mut self, what: &'static str) -> Result<&'a str> {
        let rest = self.rest();
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let bytes = self.take(end, what)?;
        if self.remaining() > 0 {
            self.skip(1, what)?;
        }
        core::str::from_utf8(bytes).map_err(|_| Error::Malformed("string is not valid UTF-8"))
    }
}
