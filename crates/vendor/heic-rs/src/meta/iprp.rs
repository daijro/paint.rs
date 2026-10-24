//! `iprp`, `ipco` and `ipma`: properties and the items they belong to.

use alloc::vec::Vec;

use crate::boxes::{BoxHeader, BoxIter};
use crate::error::{Error, Result};

/// One property attached to one item.
#[derive(Debug, Clone, Copy)]
pub struct Association {
    /// One-based index into the `ipco` container.
    pub index: u16,
    /// When set, a reader that does not understand the property must not
    /// present the image.
    pub essential: bool,
}

/// Every property in the file and the items they are attached to.
#[derive(Debug, Clone)]
pub struct Properties<'a> {
    /// The `ipco` children, in file order. Property index 1 is `boxes[0]`.
    pub boxes: Vec<BoxHeader<'a>>,
    /// Per item, the properties associated with it, in the file's order.
    pub assoc: Vec<(u32, Vec<Association>)>,
}

impl<'a> Properties<'a> {
    /// An empty property set, for files that carry no `iprp`.
    pub fn empty() -> Self {
        Properties {
            boxes: Vec::new(),
            assoc: Vec::new(),
        }
    }

    /// The associations recorded for one item.
    pub fn associations(&self, item: u32) -> &[Association] {
        for (id, list) in &self.assoc {
            if *id == item {
                return list;
            }
        }
        &[]
    }

    /// Resolve one association to the property box it names.
    pub fn resolve(&self, a: Association) -> Option<&BoxHeader<'a>> {
        if a.index == 0 {
            return None;
        }
        self.boxes.get(usize::from(a.index) - 1)
    }

    /// The first property of the given type attached to `item`.
    pub fn find(&self, item: u32, code: &[u8; 4]) -> Option<&BoxHeader<'a>> {
        self.associations(item)
            .iter()
            .filter_map(|a| self.resolve(*a))
            .find(|b| b.is(code))
    }

    /// Every property of the given type attached to `item`.
    pub fn find_all(
        &self,
        item: u32,
        code: &'static [u8; 4],
    ) -> impl Iterator<Item = &BoxHeader<'a>> {
        self.associations(item)
            .iter()
            .filter_map(|a| self.resolve(*a))
            .filter(move |b| b.is(code))
    }

    /// Property types marked essential on `item` that this crate does not act
    /// on. A caller in strict mode should refuse such a file.
    pub fn unknown_essential(&self, item: u32) -> Option<[u8; 4]> {
        const KNOWN: [&[u8; 4]; 9] = [
            b"hvcC", b"ispe", b"pixi", b"colr", b"irot", b"imir", b"clap", b"auxC", b"pasp",
        ];
        for a in self.associations(item) {
            if !a.essential {
                continue;
            }
            let Some(b) = self.resolve(*a) else { continue };
            if !KNOWN.iter().any(|k| b.is(k)) {
                return Some(b.boxtype);
            }
        }
        None
    }
}

/// Parse the `iprp` box, which holds one `ipco` and one or more `ipma`.
pub fn parse<'a>(b: &BoxHeader<'a>) -> Result<Properties<'a>> {
    let mut boxes = Vec::new();
    let mut assoc: Vec<(u32, Vec<Association>)> = Vec::new();
    for child in b.children() {
        let child = child?;
        if child.is(b"ipco") {
            for p in child.children() {
                boxes.push(p?);
            }
        } else if child.is(b"ipma") {
            parse_ipma(&child, &mut assoc)?;
        }
    }
    Ok(Properties { boxes, assoc })
}

fn parse_ipma(b: &BoxHeader<'_>, out: &mut Vec<(u32, Vec<Association>)>) -> Result<()> {
    let (version, flags, mut r) = b.full_box("ipma")?;
    let wide_index = flags & 1 != 0;
    let entry_count = r.u32("ipma entry count")?;
    // Each entry is at least an id plus a count, so anything larger than the
    // remaining bytes allow is a lie.
    if entry_count as usize > r.remaining() / 3 + 1 {
        return Err(Error::Malformed("ipma declares more entries than can fit"));
    }
    for _ in 0..entry_count {
        let item = if version < 1 {
            u32::from(r.u16("ipma item id")?)
        } else {
            r.u32("ipma item id")?
        };
        let count = r.u8("ipma association count")?;
        let mut list = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let (essential, index) = if wide_index {
                let w = r.u16("ipma association")?;
                (w & 0x8000 != 0, w & 0x7fff)
            } else {
                let w = r.u8("ipma association")?;
                (w & 0x80 != 0, u16::from(w & 0x7f))
            };
            list.push(Association { index, essential });
        }
        match out.iter_mut().find(|(id, _)| *id == item) {
            Some((_, existing)) => existing.extend_from_slice(&list),
            None => out.push((item, list)),
        }
    }
    Ok(())
}

/// Walk an `ipco` container directly, for tests and tooling.
pub fn ipco_boxes(data: &[u8]) -> BoxIter<'_> {
    BoxIter::new(data)
}
