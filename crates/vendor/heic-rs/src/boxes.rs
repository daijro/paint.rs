//! ISOBMFF box traversal (ISO/IEC 14496-12).
//!
//! [`BoxIter`] walks a byte slice as a sequence of boxes. It cannot run off
//! the end of its slice, and it refuses a box whose declared size does not fit
//! inside its parent instead of trusting the file and allocating.

use crate::error::{Error, Result};
use crate::reader::Reader;

/// The largest box payload this crate will look at, 1 GiB.
///
/// A HEIF still image has no legitimate reason to carry a single box larger
/// than this, and refusing early keeps a hostile 64-bit `largesize` from
/// becoming an allocation.
pub const MAX_BOX_SIZE: u64 = 1 << 30;

/// One box: its type, its payload, and the `uuid` if the type was `uuid`.
#[derive(Debug, Clone, Copy)]
pub struct BoxHeader<'a> {
    /// The four-character box type.
    pub boxtype: [u8; 4],
    /// The box contents, with the header (and any `uuid`) already removed.
    pub payload: &'a [u8],
    /// The extended type, present only when `boxtype` is `uuid`.
    pub uuid: Option<[u8; 16]>,
    /// Offset of the box header from the start of the slice being walked.
    pub offset: usize,
}

impl<'a> BoxHeader<'a> {
    /// True when this box has the given four-character type.
    pub fn is(&self, code: &[u8; 4]) -> bool {
        &self.boxtype == code
    }

    /// A reader over the payload.
    pub fn reader(&self) -> Reader<'a> {
        Reader::new(self.payload)
    }

    /// Walk this box's payload as a sequence of child boxes.
    pub fn children(&self) -> BoxIter<'a> {
        BoxIter::new(self.payload)
    }

    /// Read the payload as a `FullBox`, returning version, flags and a reader
    /// positioned just after them.
    pub fn full_box(&self, what: &'static str) -> Result<(u8, u32, Reader<'a>)> {
        let mut r = self.reader();
        let (version, flags) = r.full_box(what)?;
        Ok((version, flags, r))
    }
}

/// An iterator over the boxes in a slice.
///
/// Iteration stops at the first structural error, yielding it once.
#[derive(Debug, Clone)]
pub struct BoxIter<'a> {
    data: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> BoxIter<'a> {
    /// Walk `data` as a sequence of boxes.
    pub const fn new(data: &'a [u8]) -> Self {
        BoxIter {
            data,
            pos: 0,
            done: false,
        }
    }

    /// Find the first child box with the given type.
    pub fn find(self, code: &[u8; 4]) -> Result<Option<BoxHeader<'a>>> {
        for b in self {
            let b = b?;
            if b.is(code) {
                return Ok(Some(b));
            }
        }
        Ok(None)
    }

    /// Find the first child box with the given type, or report it missing.
    pub fn require(self, code: &[u8; 4], name: &'static str) -> Result<BoxHeader<'a>> {
        self.find(code)?.ok_or(Error::MissingBox(name))
    }

    fn next_box(&mut self) -> Result<Option<BoxHeader<'a>>> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }
        let start = self.pos;
        let mut r = Reader::new(&self.data[start..]);
        let size32 = r.u32("box size")?;
        let boxtype = r.fourcc("box type")?;
        // 1 means a 64-bit size follows; 0 means "to the end of this slice".
        let total: u64 = match size32 {
            1 => r.u64("box largesize")?,
            0 => (self.data.len() - start) as u64,
            n => u64::from(n),
        };
        let uuid = if boxtype == *b"uuid" {
            let b = r.take(16, "uuid")?;
            let mut u = [0u8; 16];
            u.copy_from_slice(b);
            Some(u)
        } else {
            None
        };
        // The whole header, the extended type of a `uuid` box included. Measured before the UUID
        // was read, a `uuid` box whose size covered its size and type but not its UUID passed
        // this check and had its payload sliced from past its own end -- a panic, found by
        // fuzzing and reported privately against 0.1.1.
        let header_len = r.position();
        if total < header_len as u64 {
            return Err(Error::Malformed("box size is smaller than its own header"));
        }
        if total > MAX_BOX_SIZE {
            return Err(Error::BoxTooLarge {
                boxtype,
                size: total,
            });
        }
        let total = total as usize;
        let end = start
            .checked_add(total)
            .ok_or(Error::Truncated("box body"))?;
        if end > self.data.len() {
            return Err(Error::Truncated("box body"));
        }
        let payload = &self.data[start + header_len..end];
        self.pos = end;
        Ok(Some(BoxHeader {
            boxtype,
            payload,
            uuid,
            offset: start,
        }))
    }
}

impl<'a> Iterator for BoxIter<'a> {
    type Item = Result<BoxHeader<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.next_box() {
            Ok(Some(b)) => Some(Ok(b)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}
