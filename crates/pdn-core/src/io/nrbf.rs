//! MS-NRBF (.NET BinaryFormatter) reader and writer, limited to the record kinds Paint.NET's
//! document graph contains. The reader ports the record handling of `ms-nrbf-js`
//! (paintdotjs/node_modules/ms-nrbf-js/dist/reader.js and deserialize.js, the reader paintdotjs
//! uses), hardened for hostile input: every length is checked against the bytes left, nesting
//! depth is capped, and every parsed slot and every resolved value (primitives and nulls included)
//! is charged to a budget. Strings, byte arrays and member names are shared (`Arc`), so object
//! sharing cannot multiply their bytes. Nothing panics. The writer (`Writer`) emits records from
//! the public MS-NRBF specification; the record sequence is the caller's.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NrbfError {
    Truncated,
    Invalid(&'static str),
    /// A depth or node-count limit was hit (hostile or absurd input).
    TooLarge,
}

impl fmt::Display for NrbfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NrbfError::Truncated => f.write_str("truncated NRBF stream"),
            NrbfError::Invalid(m) => write!(f, "invalid NRBF stream: {m}"),
            NrbfError::TooLarge => f.write_str("NRBF stream exceeds the reader's limits"),
        }
    }
}

impl std::error::Error for NrbfError {}

type Result<T> = std::result::Result<T, NrbfError>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prim {
    Bool(bool),
    /// Every integer width (and DateTime/TimeSpan ticks); UInt64 wraps.
    Int(i64),
    Float(f64),
    Char(char),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NrbfValue {
    Object {
        class: Arc<str>,
        members: BTreeMap<Arc<str>, NrbfValue>,
    },
    Primitive(Prim),
    /// Shared with every other reference to the same string, so a hostile graph cannot multiply
    /// string bytes.
    String(Arc<str>),
    /// A `byte[]` (inline MemoryBlock data), kept packed and shared.
    Bytes(Arc<[u8]>),
    Array(Vec<NrbfValue>),
    Null,
    /// A reference back to an object that is still being resolved (a cycle).
    Ref(i32),
}

static NULL: NrbfValue = NrbfValue::Null;

impl NrbfValue {
    /// A member of an object; `Null` when this is not an object or has no such member.
    pub fn get(&self, member: &str) -> &NrbfValue {
        match self {
            NrbfValue::Object { members, .. } => members.get(member).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub fn class(&self) -> Option<&str> {
        match self {
            NrbfValue::Object { class, .. } => Some(class),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, NrbfValue::Null)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            NrbfValue::Primitive(Prim::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            NrbfValue::Primitive(Prim::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            NrbfValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            NrbfValue::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[NrbfValue]> {
        match self {
            NrbfValue::Array(a) => Some(a),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 128;
const MAX_RESOLVE_DEPTH: usize = 256;
/// The smallest value budget (`value_budget`): parsed slots, and (separately) resolved values,
/// any stream may produce. Every resolved member and element is charged, primitives and nulls
/// included. The worst case is about 720 bytes of output per unit (a one-member object is a
/// whole BTreeMap leaf), however the graph shares objects, so this floor alone allows ~190 MB.
const MAX_NODES: usize = 1 << 18;
/// String bytes (values and class names) the resolved tree may hand out, counted once per
/// reference: strings are `Arc`-shared in memory, but consumers copy them per reference.
const MAX_STRING_BYTES: usize = 64 << 20;

#[derive(Clone, Copy, PartialEq)]
enum Bt {
    Primitive(u8),
    String,
    Object,
    SystemClass,
    Class,
    ObjectArray,
    StringArray,
    PrimitiveArray,
}

struct Meta {
    name: Arc<str>,
    members: Vec<Arc<str>>,
    /// Summed member-name bytes, charged per resolved object (inserting into the BTreeMap clones
    /// and compares every key).
    name_bytes: usize,
    types: Vec<Bt>,
}

/// One member / element before reference resolution.
#[derive(Clone)]
enum Slot {
    Prim(Prim),
    Null,
    Ref(i32),
}

enum Raw {
    /// Member names live in the shared `Meta`; only the values are per instance.
    Obj {
        meta: Arc<Meta>,
        values: Vec<Slot>,
    },
    Str(Arc<str>),
    Bytes(Arc<[u8]>),
    Arr(Vec<Slot>),
}

struct Parser<'a> {
    d: &'a [u8],
    p: usize,
    objects: HashMap<i32, Raw>,
    classes: HashMap<i32, Arc<Meta>>,
    budget: usize,
}

impl<'a> Parser<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.p.checked_add(n).ok_or(NrbfError::Truncated)?;
        let s = self.d.get(self.p..end).ok_or(NrbfError::Truncated)?;
        self.p = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn remaining(&self) -> usize {
        self.d.len() - self.p
    }

    /// A non-negative count that cannot exceed the bytes left (every element takes >= 1 byte).
    fn count(&mut self) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 {
            return Err(NrbfError::Invalid("negative length"));
        }
        Ok(n as usize)
    }

    fn string(&mut self) -> Result<Arc<str>> {
        let mut len = 0usize;
        for i in 0..5 {
            let b = self.u8()?;
            // The 5th byte may only add bits 28..=30 (also keeps `len` inside a 32-bit usize).
            if i == 4 && b > 0x07 {
                return Err(NrbfError::Invalid("string length"));
            }
            len |= ((b & 0x7f) as usize) << (7 * i);
            if b & 0x80 == 0 {
                let bytes = self.take(len)?;
                return Ok(String::from_utf8_lossy(bytes).into());
            }
        }
        Err(NrbfError::Invalid("string length"))
    }

    fn spend(&mut self, n: usize) -> Result<()> {
        self.budget = self.budget.checked_sub(n).ok_or(NrbfError::TooLarge)?;
        Ok(())
    }

    fn prim(&mut self, t: u8) -> Result<Prim> {
        Ok(match t {
            1 => Prim::Bool(self.u8()? != 0),
            2 => Prim::Int(self.u8()? as i64),
            3 => {
                let first = self.u8()?;
                let n = match first {
                    0..=0x7f => 1,
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => return Err(NrbfError::Invalid("char")),
                };
                let mut buf = vec![first];
                buf.extend_from_slice(self.take(n - 1)?);
                let s = std::str::from_utf8(&buf).map_err(|_| NrbfError::Invalid("char"))?;
                Prim::Char(s.chars().next().ok_or(NrbfError::Invalid("char"))?)
            }
            5 => Prim::Float(self.string()?.parse().unwrap_or(0.0)),
            6 => Prim::Float(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            7 => Prim::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            8 => Prim::Int(self.i32()? as i64),
            9 | 12 | 13 => Prim::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            10 => Prim::Int(self.u8()? as i8 as i64),
            11 => Prim::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            14 => Prim::Int(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            15 => Prim::Int(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            16 => Prim::Int(u64::from_le_bytes(self.take(8)?.try_into().unwrap()) as i64),
            _ => return Err(NrbfError::Invalid("primitive type")),
        })
    }

    fn register(&mut self, id: i32, raw: Raw) -> Result<()> {
        if self.objects.insert(id, raw).is_some() {
            return Err(NrbfError::Invalid("duplicate object id"));
        }
        Ok(())
    }

    fn class_info(&mut self) -> Result<(Arc<str>, Vec<Arc<str>>)> {
        let name = self.string()?;
        let n = self.count()?;
        if n > self.remaining() {
            return Err(NrbfError::Truncated);
        }
        self.spend(n)?;
        let members: Vec<Arc<str>> = (0..n).map(|_| self.string()).collect::<Result<_>>()?;
        // Real member names are under 100 bytes.
        if members.iter().any(|m| m.len() > 4096) {
            return Err(NrbfError::TooLarge);
        }
        Ok((name, members))
    }

    /// Reads one BinaryType's additional info (shared by class members and BinaryArray).
    fn bin_type(&mut self, t: u8) -> Result<Bt> {
        Ok(match t {
            0 => Bt::Primitive(0),
            1 => Bt::String,
            2 => Bt::Object,
            3 => Bt::SystemClass,
            4 => Bt::Class,
            5 => Bt::ObjectArray,
            6 => Bt::StringArray,
            7 => Bt::PrimitiveArray,
            _ => return Err(NrbfError::Invalid("binary type")),
        })
    }

    fn additional(&mut self, t: Bt) -> Result<Bt> {
        Ok(match t {
            Bt::Primitive(_) | Bt::PrimitiveArray => {
                let p = self.u8()?;
                if !(1..=16).contains(&p) || p == 4 {
                    return Err(NrbfError::Invalid("primitive type"));
                }
                if t == Bt::PrimitiveArray {
                    Bt::PrimitiveArray
                } else {
                    Bt::Primitive(p)
                }
            }
            Bt::SystemClass => {
                self.string()?;
                t
            }
            Bt::Class => {
                self.string()?;
                self.i32()?;
                t
            }
            _ => t,
        })
    }

    fn member_types(&mut self, n: usize) -> Result<Vec<Bt>> {
        let kinds: Vec<u8> = self.take(n)?.to_vec();
        kinds
            .into_iter()
            .map(|k| {
                let t = self.bin_type(k)?;
                self.additional(t)
            })
            .collect()
    }

    /// Values of a class instance, one per member, in order.
    fn members(&mut self, meta: &Meta, depth: usize) -> Result<Vec<Slot>> {
        self.spend(meta.members.len())?;
        let mut out = Vec::with_capacity(meta.members.len());
        while out.len() < meta.members.len() {
            let t = meta.types[out.len()];
            let want = meta.members.len() - out.len();
            if let Bt::Primitive(p) = t {
                let v = self.prim(p)?;
                out.push(Slot::Prim(v));
                continue;
            }
            let (slot, rep) = self.slot(depth)?;
            if rep > want {
                return Err(NrbfError::Invalid("null run past member list"));
            }
            for _ in 0..rep {
                out.push(slot.clone());
            }
        }
        Ok(out)
    }

    /// One value record in a member/element position: (slot, how many slots it fills).
    fn slot(&mut self, depth: usize) -> Result<(Slot, usize)> {
        loop {
            let rt = self.u8()?;
            return Ok(match rt {
                12 => {
                    self.i32()?;
                    self.string()?;
                    continue;
                }
                9 => (Slot::Ref(self.i32()?), 1),
                10 => (Slot::Null, 1),
                13 => (Slot::Null, self.u8()? as usize),
                14 => (Slot::Null, self.count()?),
                8 => {
                    let t = self.u8()?;
                    (Slot::Prim(self.prim(t)?), 1)
                }
                11 | 0 => return Err(NrbfError::Invalid("unexpected record")),
                _ => (Slot::Ref(self.object_record(rt, depth + 1)?), 1),
            });
        }
    }

    /// A record that defines an object; registers it and returns its id.
    fn object_record(&mut self, rt: u8, depth: usize) -> Result<i32> {
        if depth > MAX_DEPTH {
            return Err(NrbfError::TooLarge);
        }
        match rt {
            6 => {
                let id = self.i32()?;
                let s = self.string()?;
                self.register(id, Raw::Str(s))?;
                Ok(id)
            }
            1 => {
                let id = self.i32()?;
                let meta_id = self.i32()?;
                let meta = self
                    .classes
                    .get(&meta_id)
                    .cloned()
                    .ok_or(NrbfError::Invalid("unknown metadata id"))?;
                let values = self.members(&meta, depth)?;
                self.register(id, Raw::Obj { meta, values })?;
                Ok(id)
            }
            4 | 5 => {
                let id = self.i32()?;
                let (name, names) = self.class_info()?;
                let types = self.member_types(names.len())?;
                if rt == 5 {
                    self.i32()?; // LibraryId
                }
                let meta = Arc::new(Meta {
                    name,
                    name_bytes: names.iter().map(|n| n.len()).sum(),
                    members: names,
                    types,
                });
                self.classes.insert(id, meta.clone());
                let values = self.members(&meta, depth)?;
                self.register(id, Raw::Obj { meta, values })?;
                Ok(id)
            }
            15 => {
                let id = self.i32()?;
                let n = self.count()?;
                let t = self.u8()?;
                if n > self.remaining() {
                    return Err(NrbfError::Truncated);
                }
                if t == 2 {
                    let bytes: Arc<[u8]> = self.take(n)?.into();
                    self.register(id, Raw::Bytes(bytes))?;
                    return Ok(id);
                }
                self.spend(n)?;
                let items = (0..n)
                    .map(|_| self.prim(t).map(Slot::Prim))
                    .collect::<Result<_>>()?;
                self.register(id, Raw::Arr(items))?;
                Ok(id)
            }
            16 | 17 => {
                let id = self.i32()?;
                let n = self.count()?;
                let items = self.elements(n, None, depth)?;
                self.register(id, Raw::Arr(items))?;
                Ok(id)
            }
            7 => {
                let id = self.i32()?;
                let kind = self.u8()?;
                let rank = self.count()?;
                if kind > 5 || rank == 0 || rank > 32 {
                    return Err(NrbfError::Invalid("array shape"));
                }
                let mut total = 1usize;
                for _ in 0..rank {
                    total = total
                        .checked_mul(self.count()?)
                        .ok_or(NrbfError::TooLarge)?;
                }
                if kind >= 3 {
                    for _ in 0..rank {
                        self.i32()?;
                    }
                }
                let k = self.u8()?;
                let t = self.bin_type(k)?;
                let t = self.additional(t)?;
                let prim = if let Bt::Primitive(p) = t {
                    Some(p)
                } else {
                    None
                };
                let items = self.elements(total, prim, depth)?;
                self.register(id, Raw::Arr(items))?;
                Ok(id)
            }
            _ => Err(NrbfError::Invalid("unsupported record type")),
        }
    }

    fn elements(&mut self, n: usize, prim: Option<u8>, depth: usize) -> Result<Vec<Slot>> {
        self.spend(n)?;
        let mut out = Vec::new();
        while out.len() < n {
            if let Some(p) = prim {
                out.push(Slot::Prim(self.prim(p)?));
                continue;
            }
            let (slot, rep) = self.slot(depth)?;
            if rep > n - out.len() {
                return Err(NrbfError::Invalid("null run past array end"));
            }
            out.extend(std::iter::repeat_n(slot, rep));
        }
        Ok(out)
    }
}

struct Resolver<'a> {
    objects: &'a HashMap<i32, Raw>,
    stack: Vec<i32>,
    budget: usize,
    string_bytes: usize,
}

impl Resolver<'_> {
    /// Charges one produced value (member, element or object).
    fn charge(&mut self) -> Result<()> {
        self.budget = self.budget.checked_sub(1).ok_or(NrbfError::TooLarge)?;
        Ok(())
    }

    fn charge_string(&mut self, n: usize) -> Result<()> {
        self.string_bytes = self
            .string_bytes
            .checked_sub(n)
            .ok_or(NrbfError::TooLarge)?;
        Ok(())
    }

    fn slot(&mut self, s: &Slot) -> Result<NrbfValue> {
        self.charge()?;
        match s {
            Slot::Prim(p) => Ok(NrbfValue::Primitive(*p)),
            Slot::Null => Ok(NrbfValue::Null),
            Slot::Ref(id) => self.object(*id),
        }
    }

    fn object(&mut self, id: i32) -> Result<NrbfValue> {
        if self.stack.contains(&id) {
            return Ok(NrbfValue::Ref(id));
        }
        if self.stack.len() >= MAX_RESOLVE_DEPTH {
            return Err(NrbfError::TooLarge);
        }
        let raw = self
            .objects
            .get(&id)
            .ok_or(NrbfError::Invalid("dangling reference"))?;
        self.stack.push(id);
        let v = match raw {
            Raw::Str(s) => {
                self.charge_string(s.len())?;
                NrbfValue::String(s.clone())
            }
            Raw::Bytes(b) => NrbfValue::Bytes(b.clone()),
            Raw::Obj { meta, values } => {
                self.charge_string(meta.name.len().saturating_add(meta.name_bytes))?;
                let mut m = BTreeMap::new();
                for (k, s) in meta.members.iter().zip(values) {
                    m.insert(k.clone(), self.slot(s)?);
                }
                NrbfValue::Object {
                    class: meta.name.clone(),
                    members: m,
                }
            }
            Raw::Arr(items) => {
                let mut out = Vec::new();
                for s in items {
                    out.push(self.slot(s)?);
                }
                NrbfValue::Array(out)
            }
        };
        self.stack.pop();
        Ok(v)
    }
}

/// The value budget of a stream read from `input_len` file bytes: one unit per 4 bytes, at least
/// `MAX_NODES`, and at most `MAX_DECODE_BYTES` / 720 so the worst-case output (~720 B per unit)
/// stays within the decode byte budget: ~1.49M units (~1.07 GB) natively, ~745k on wasm32. A
/// Paint.NET layer costs about 23 units, so files up to ~65k layers reopen natively and ~32k on
/// wasm32.
// ponytail: a one-member object costs a whole BTreeMap leaf (~640 B); storing members as
// `Box<[(Arc<str>, NrbfValue)]>` would cut that ~9× and is the upgrade if > ~65k layers matter.
fn value_budget(input_len: usize) -> usize {
    MAX_NODES
        .max(input_len / 4)
        .min(super::codecs::MAX_DECODE_BYTES as usize / 720)
}

/// Parses one NRBF stream from the start of `bytes` and resolves its references from the root
/// object. Returns the root and the number of bytes consumed (through MessageEnd). A reference
/// to an object that is still being resolved stays `NrbfValue::Ref`. The value budget scales
/// with `bytes.len()`.
pub fn deserialize(bytes: &[u8]) -> Result<(NrbfValue, usize)> {
    deserialize_sized(bytes, bytes.len())
}

/// `deserialize` with the value budget scaled by `input_len` file bytes instead: a graph
/// inflated from a gzip stream passes its compressed length, so the budget stays linear in the
/// file, not in what it inflates to.
pub(crate) fn deserialize_sized(bytes: &[u8], input_len: usize) -> Result<(NrbfValue, usize)> {
    let budget = value_budget(input_len);
    let mut p = Parser {
        d: bytes,
        p: 0,
        objects: HashMap::new(),
        classes: HashMap::new(),
        budget,
    };
    if p.u8()? != 0 {
        return Err(NrbfError::Invalid("missing stream header"));
    }
    let root_id = p.i32()?;
    p.i32()?; // HeaderId
    p.i32()?; // major version
    p.i32()?; // minor version
    loop {
        match p.u8()? {
            11 => break,
            12 => {
                p.i32()?;
                p.string()?;
            }
            rt => {
                p.object_record(rt, 0)?;
            }
        }
    }
    let consumed = p.p;
    let mut r = Resolver {
        objects: &p.objects,
        stack: vec![],
        budget,
        string_bytes: MAX_STRING_BYTES,
    };
    r.charge()?;
    let root = r.object(root_id)?;
    Ok((root, consumed))
}

// ---- writer -------------------------------------------------------------------------------

/// PrimitiveTypeEnumeration values the writer uses (MS-NRBF 2.1.2.3).
pub const BOOLEAN: u8 = 1;
pub const BYTE: u8 = 2;
pub const INT32: u8 = 8;
pub const INT64: u8 = 9;

/// A class member's BinaryTypeEnumeration and its additional info (MS-NRBF 2.1.2.2, 2.3.1.2).
#[derive(Clone, Copy)]
pub enum Member<'a> {
    Primitive(u8),
    String,
    SystemClass(&'a str),
    /// Class name and library id.
    Class(&'a str, i32),
    ObjectArray,
}

/// MS-NRBF record emitter. The caller writes records in stream order and each class record's
/// member values right after it. `class` applies BinaryFormatter's metadata rule.
pub struct Writer {
    out: Vec<u8>,
    /// Class name → (object id, metadata bytes) of the first record that carried its metadata.
    metadata: HashMap<String, (i32, Vec<u8>)>,
}

fn put_string(out: &mut Vec<u8>, s: &str) {
    let mut n = s.len();
    while n >= 0x80 {
        out.push(n as u8 | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
    out.extend(s.as_bytes());
}

impl Writer {
    /// SerializationHeaderRecord: root `root_id`, header id -1, version 1.0.
    pub fn new(root_id: i32) -> Writer {
        let mut w = Writer {
            out: vec![0],
            metadata: HashMap::new(),
        };
        for v in [root_id, -1, 1, 0] {
            w.i32(v);
        }
        w
    }

    /// BinaryLibrary.
    pub fn library(&mut self, id: i32, name: &str) {
        self.out.push(12);
        self.i32(id);
        put_string(&mut self.out, name);
    }

    /// A class instance header. Full metadata (ClassWithMembersAndTypes in `library`, or
    /// SystemClassWithMembersAndTypes when `None`) unless the first record of `name` had the same
    /// members, which makes it a ClassWithId. BinaryFormatter keeps the first metadata per name,
    /// so an incompatible later instance carries full metadata every time (inferred:
    /// document-ops-and-formats.md §6 item 11, settled by B-F7). The member values follow.
    pub fn class(&mut self, id: i32, name: &str, members: &[(&str, Member)], library: Option<i32>) {
        let mut meta = Vec::new();
        put_string(&mut meta, name);
        meta.extend((members.len() as i32).to_le_bytes());
        for (m, _) in members {
            put_string(&mut meta, m);
        }
        for (_, t) in members {
            meta.push(match t {
                Member::Primitive(_) => 0,
                Member::String => 1,
                Member::SystemClass(_) => 3,
                Member::Class(..) => 4,
                Member::ObjectArray => 5,
            });
        }
        for (_, t) in members {
            match *t {
                Member::Primitive(p) => meta.push(p),
                Member::SystemClass(c) => put_string(&mut meta, c),
                Member::Class(c, lib) => {
                    put_string(&mut meta, c);
                    meta.extend(lib.to_le_bytes());
                }
                Member::String | Member::ObjectArray => {}
            }
        }
        if let Some(lib) = library {
            meta.extend(lib.to_le_bytes());
        }
        match self.metadata.get(name) {
            Some((first, m)) if *m == meta => {
                let first = *first;
                self.out.push(1);
                self.i32(id);
                self.i32(first);
            }
            _ => {
                self.out.push(if library.is_some() { 5 } else { 4 });
                self.i32(id);
                self.out.extend(&meta);
                self.metadata.entry(name.into()).or_insert((id, meta));
            }
        }
    }

    /// BinaryObjectString.
    pub fn string(&mut self, id: i32, s: &str) {
        self.out.push(6);
        self.i32(id);
        put_string(&mut self.out, s);
    }

    /// MemberReference.
    pub fn reference(&mut self, id: i32) {
        self.out.push(9);
        self.i32(id);
    }

    /// `n` null slots: ObjectNull, ObjectNullMultiple256 or ObjectNullMultiple.
    pub fn nulls(&mut self, n: usize) {
        match n {
            0 => {}
            1 => self.out.push(10),
            2..=255 => self.out.extend([13, n as u8]),
            _ => {
                self.out.push(14);
                self.i32(n as i32);
            }
        }
    }

    /// ArraySingleObject header; `len` element records follow.
    pub fn object_array(&mut self, id: i32, len: usize) {
        self.out.push(16);
        self.i32(id);
        self.i32(len as i32);
    }

    /// BinaryArray header of a single-rank array of system class `class`; `len` element records
    /// follow.
    pub fn array(&mut self, id: i32, len: usize, class: &str) {
        self.out.push(7);
        self.i32(id);
        self.out.push(0); // BinaryArrayTypeEnumeration Single
        self.i32(1);
        self.i32(len as i32);
        self.out.push(3); // SystemClass
        put_string(&mut self.out, class);
    }

    pub fn bool(&mut self, v: bool) {
        self.out.push(v as u8);
    }

    pub fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    pub fn i32(&mut self, v: i32) {
        self.out.extend(v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.out.extend(v.to_le_bytes());
    }

    /// MessageEnd; the stream.
    pub fn finish(mut self) -> Vec<u8> {
        self.out.push(11);
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Vec<u8> {
        let mut o = vec![v.len() as u8];
        o.extend(v.as_bytes());
        o
    }

    /// header, library, class Foo { a: Int32, b: String, c: Foo(self-ref) } , MessageEnd
    fn sample() -> Vec<u8> {
        let mut b = vec![0];
        for v in [1i32, 0, 1, 0] {
            b.extend(v.to_le_bytes());
        }
        b.push(12);
        b.extend(2i32.to_le_bytes());
        b.extend(s("lib"));
        b.push(5);
        b.extend(1i32.to_le_bytes());
        b.extend(s("Foo"));
        b.extend(3i32.to_le_bytes());
        for n in ["a", "b", "c"] {
            b.extend(s(n));
        }
        b.extend([0, 1, 2]); // Primitive, String, Object
        b.push(8); // Int32
        b.extend(2i32.to_le_bytes()); // library id
        b.extend(42i32.to_le_bytes());
        b.push(6);
        b.extend(3i32.to_le_bytes());
        b.extend(s("héllo"));
        b.push(9);
        b.extend(1i32.to_le_bytes()); // c -> root (cycle)
        b.push(11);
        b
    }

    #[test]
    fn parses_objects_strings_and_cycles() {
        let b = sample();
        let (v, used) = deserialize(&b).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(v.class(), Some("Foo"));
        assert_eq!(v.get("a").as_i64(), Some(42));
        assert_eq!(v.get("b").as_str(), Some("héllo"));
        assert_eq!(v.get("c"), &NrbfValue::Ref(1));
        assert!(v.get("missing").get("deeper").is_null());
    }

    #[test]
    fn hostile_streams_error_without_panicking() {
        let b = sample();
        for cut in 0..b.len() {
            assert!(deserialize(&b[..cut]).is_err(), "cut {cut}");
        }
        for at in 0..b.len() {
            for x in [0x00, 0xff, 0x7f] {
                let mut c = b.clone();
                c[at] = x;
                let _ = deserialize(&c);
            }
        }
        // Array of a billion elements, a billion nulls, deep nesting, dangling reference.
        let hdr = |root: i32| {
            let mut h = vec![0];
            for v in [root, 0, 1, 0] {
                h.extend(v.to_le_bytes());
            }
            h
        };
        let mut a = hdr(1);
        a.push(16);
        a.extend(1i32.to_le_bytes());
        a.extend(1_000_000_000i32.to_le_bytes());
        a.push(11);
        assert!(deserialize(&a).is_err());
        let mut a = hdr(1);
        a.push(16);
        a.extend(1i32.to_le_bytes());
        a.extend(1_000_000_000i32.to_le_bytes());
        a.push(14);
        a.extend(1_000_000_000i32.to_le_bytes());
        a.push(11);
        assert!(deserialize(&a).is_err());
        let mut a = hdr(1);
        for id in 1..=100_000i32 {
            a.push(16); // ArraySingleObject of length 1 whose element is the next one, inline
            a.extend(id.to_le_bytes());
            a.extend(1i32.to_le_bytes());
        }
        a.push(11);
        assert!(deserialize(&a).is_err());
        let mut a = hdr(5);
        a.push(11);
        assert!(deserialize(&a).is_err());
    }

    #[test]
    fn exponential_sharing_is_capped() {
        // 40 levels of object[2] whose both elements reference the next level: 2^40 resolved nodes.
        let mut b = vec![0];
        for v in [1i32, 0, 1, 0] {
            b.extend(v.to_le_bytes());
        }
        for id in 1..=40i32 {
            b.push(16);
            b.extend(id.to_le_bytes());
            b.extend(2i32.to_le_bytes());
            for _ in 0..2 {
                b.push(9);
                b.extend((id + 1).to_le_bytes());
            }
        }
        b.push(16);
        b.extend(41i32.to_le_bytes());
        b.extend(0i32.to_le_bytes());
        b.push(11);
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
    }

    // ---- review probes (fix round 1): output must stay bounded however objects are shared ----

    fn hdr(root: i32) -> Vec<u8> {
        let mut h = vec![0];
        for v in [root, 0, 1, 0] {
            h.extend(v.to_le_bytes());
        }
        h
    }

    fn lpstr(b: &mut Vec<u8>, s: &[u8]) {
        let mut n = s.len();
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                b.push(byte);
                break;
            }
            b.push(byte | 0x80);
        }
        b.extend(s);
    }

    fn root_array_of_refs(b: &mut Vec<u8>, k: usize, target: i32) {
        b.push(16);
        b.extend(1i32.to_le_bytes());
        b.extend((k as i32).to_le_bytes());
        for _ in 0..k {
            b.push(9);
            b.extend(target.to_le_bytes());
        }
        b.push(11);
    }

    #[test]
    fn shared_null_arrays_are_charged_per_reference() {
        for k in [1usize, 10, 100] {
            let mut b = hdr(1);
            b.push(16);
            b.extend(2i32.to_le_bytes());
            b.extend(2_000_000i32.to_le_bytes());
            b.push(14);
            b.extend(2_000_000i32.to_le_bytes());
            root_array_of_refs(&mut b, k, 2);
            assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge, "k {k}");
        }
        // Small shared arrays are fine and just cost their elements.
        let mut b = hdr(1);
        b.push(16);
        b.extend(2i32.to_le_bytes());
        b.extend(1000i32.to_le_bytes());
        b.push(14);
        b.extend(1000i32.to_le_bytes());
        root_array_of_refs(&mut b, 100, 2);
        assert!(deserialize(&b).is_ok());
        let mut b = hdr(1);
        b.push(16);
        b.extend(2i32.to_le_bytes());
        b.extend(1000i32.to_le_bytes());
        b.push(14);
        b.extend(1000i32.to_le_bytes());
        root_array_of_refs(&mut b, 2000, 2);
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
    }

    #[test]
    fn shared_strings_are_shared_but_paid_for_per_reference() {
        let build = |k: usize| {
            let mut b = hdr(1);
            b.push(6);
            b.extend(2i32.to_le_bytes());
            lpstr(&mut b, &vec![b'x'; 1 << 20]);
            root_array_of_refs(&mut b, k, 2);
            b
        };
        // Consumers copy a string per reference, so 4000 x 1 MiB is refused...
        assert_eq!(deserialize(&build(4000)).unwrap_err(), NrbfError::TooLarge);
        // ...while a handful of references share one allocation.
        let (v, _) = deserialize(&build(10)).unwrap();
        let items = v.as_array().unwrap();
        let (NrbfValue::String(a), NrbfValue::String(z)) = (&items[0], &items[9]) else {
            panic!("strings expected");
        };
        assert!(Arc::ptr_eq(a, z));
    }

    #[test]
    fn empty_member_names_are_charged_before_collecting() {
        let mut b = hdr(1);
        b.push(4);
        b.extend(1i32.to_le_bytes());
        lpstr(&mut b, b"C");
        b.extend(20_000_000i32.to_le_bytes());
        b.extend(vec![0u8; 20_000_000]); // 20M empty names
        b.push(11);
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
    }

    #[test]
    fn many_references_to_small_objects_stay_bounded() {
        // 500k references to X { m: null } (a ~720 B BTreeMap each when resolved).
        let mut b = hdr(1);
        b.push(4);
        b.extend(2i32.to_le_bytes());
        lpstr(&mut b, b"X");
        b.extend(1i32.to_le_bytes());
        lpstr(&mut b, b"m");
        b.push(2);
        b.push(10);
        root_array_of_refs(&mut b, 500_000, 2);
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
    }

    #[test]
    fn member_names_are_shared_across_instances() {
        let mut b = hdr(1);
        b.push(4);
        b.extend(1i32.to_le_bytes());
        lpstr(&mut b, b"C");
        b.extend(1000i32.to_le_bytes());
        for i in 0..1000u32 {
            let mut n = vec![b'n'; 1000];
            n[0] = b'a' + (i % 26) as u8;
            n[1] = b'a' + (i / 26 % 26) as u8;
            n[2] = b'a' + (i / 676) as u8;
            lpstr(&mut b, &n);
        }
        b.extend(vec![2u8; 1000]);
        b.push(14);
        b.extend(1000i32.to_le_bytes());
        for id in 0..4000i32 {
            b.push(1);
            b.extend((id + 10).to_le_bytes());
            b.extend(1i32.to_le_bytes());
            b.push(14);
            b.extend(1000i32.to_le_bytes());
        }
        b.push(11);
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
    }

    #[test]
    fn byte_arrays_stay_packed() {
        let mut b = hdr(1);
        b.push(15);
        b.extend(1i32.to_le_bytes());
        b.extend((8i32 << 20).to_le_bytes());
        b.push(2);
        b.extend(vec![7u8; 8 << 20]);
        b.push(11);
        let (v, _) = deserialize(&b).unwrap();
        assert_eq!(v.as_bytes().unwrap().len(), 8 << 20);
    }

    /// The budget's worst case (~720 B per unit) stays within the decode byte budget however
    /// large the input, and a 12k-layer file (~2.3 MB, ~23 units per layer) still fits.
    #[test]
    fn value_budget_is_bounded_by_the_decode_bytes() {
        let cap = super::super::codecs::MAX_DECODE_BYTES as usize;
        assert!(value_budget(usize::MAX).saturating_mul(720) <= cap);
        assert!(value_budget(2_300_000) >= 12_000 * 23);
        assert_eq!(value_budget(0), MAX_NODES);
    }

    /// Null runs take ObjectNull, ObjectNullMultiple256 or ObjectNullMultiple by length (a
    /// layer list only reaches the last past 512 layers).
    #[test]
    fn writer_null_runs_read_back() {
        for n in [1usize, 2, 255, 256, 300] {
            let mut w = Writer::new(1);
            w.object_array(1, n + 1);
            w.string(2, "a");
            w.nulls(n);
            let (v, _) = deserialize(&w.finish()).unwrap();
            let items = v.as_array().unwrap();
            assert_eq!(items.len(), n + 1, "{n}");
            assert!(items[1..].iter().all(NrbfValue::is_null), "{n}");
        }
    }

    #[test]
    fn fifth_length_byte_above_seven_is_refused() {
        let mut b = hdr(1);
        b.push(6);
        b.extend(1i32.to_le_bytes());
        b.extend([0x80, 0x80, 0x80, 0x80, 0x08]);
        b.push(11);
        assert!(deserialize(&b).is_err());
    }

    #[test]
    fn long_shared_prefix_member_names_are_charged_per_reference() {
        // Two ~4 KiB names sharing a prefix, resolved 100k times: key clones and comparisons cost
        // CPU per reference, so the reader must refuse long before that.
        let mut b = hdr(1);
        b.push(4);
        b.extend(2i32.to_le_bytes());
        lpstr(&mut b, b"X");
        b.extend(2i32.to_le_bytes());
        for tail in *b"ab" {
            let mut n = vec![b'p'; 4000];
            n.push(tail);
            lpstr(&mut b, &n);
        }
        b.extend([2u8, 2]);
        b.extend([10u8, 10]);
        root_array_of_refs(&mut b, 100_000, 2);
        let t = std::time::Instant::now();
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
        assert!(t.elapsed().as_secs() < 10);
        // A name past the sane cap is refused at parse.
        let mut b = hdr(1);
        b.push(4);
        b.extend(2i32.to_le_bytes());
        lpstr(&mut b, b"X");
        b.extend(1i32.to_le_bytes());
        lpstr(&mut b, &vec![b'q'; 5000]);
        b.extend([2u8, 10]);
        b.push(11);
        assert_eq!(deserialize(&b).unwrap_err(), NrbfError::TooLarge);
    }
}
