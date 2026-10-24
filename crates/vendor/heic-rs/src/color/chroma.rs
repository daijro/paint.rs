//! A band's cache of horizontally expanded chroma rows.
//!
//! In 4:2:0 every chroma row feeds two luma rows, and every luma row blends
//! two chroma rows, so expanding chroma per luma row does each expansion
//! twice and once more for each neighbour. Three slots are enough to expand
//! each chroma row once per band: luma rows `2k` and `2k+1` read chroma rows
//! `{k, k-1}` and `{k, k+1}`, and row `2k+2` reads `{k+1, k}`, which are both
//! still here when the slot for `k-1` is the one evicted.

use crate::color::source::Source;
use crate::upsample;

/// How many expanded chroma rows a band keeps.
const SLOTS: usize = 3;

/// Expanded chroma rows, Cb then Cr in each slot, keyed by chroma row index.
pub(crate) struct Ring<'s> {
    keys: [Option<usize>; SLOTS],
    rows: &'s mut [u16],
    width: usize,
}

impl<'s> Ring<'s> {
    /// Samples a ring for rows `width` wide needs.
    pub const fn len(width: usize) -> usize {
        SLOTS * 2 * width
    }

    /// A ring over `rows`, which is [`Ring::len`] samples long.
    pub fn new(rows: &'s mut [u16], width: usize) -> Ring<'s> {
        Ring {
            keys: [None; SLOTS],
            rows,
            width,
        }
    }

    /// The slot holding chroma row `r` expanded, filling one from `src` if
    /// none does. `tmp` is the row-assembly scratch `src` may need.
    pub fn slot(
        &mut self,
        src: &Source<'_>,
        r: usize,
        x_shift: u32,
        tmp: &mut [u16],
    ) -> Option<usize> {
        if let Some(s) = self.keys.iter().position(|k| *k == Some(r)) {
            return Some(s);
        }
        // Evict an empty slot, else the row furthest from this one.
        let s = self
            .keys
            .iter()
            .enumerate()
            .max_by_key(|(_, k)| k.map_or(usize::MAX, |k| k.abs_diff(r)))
            .map(|(s, _)| s)?;
        let w = self.width;
        let slot = self.rows.get_mut(s * 2 * w..(s + 1) * 2 * w)?;
        let (cb, cr) = slot.split_at_mut(w);
        upsample::horizontal(cb, src.chroma_row(false, r, tmp)?, x_shift);
        upsample::horizontal(cr, src.chroma_row(true, r, tmp)?, x_shift);
        self.keys[s] = Some(r);
        Some(s)
    }

    /// The expanded Cb and Cr rows in slot `s`.
    pub fn get(&self, s: usize) -> Option<(&[u16], &[u16])> {
        let w = self.width;
        let slot = self.rows.get(s * 2 * w..(s + 1) * 2 * w)?;
        Some(slot.split_at(w))
    }
}
