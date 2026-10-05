//! PDF object types: names, strings, dictionaries, arrays, streams, refs.

use std::fmt;

/// An indirect reference `(id gen R)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ref {
    /// Object number (1-based within a file).
    pub id: u32,
    /// Generation number; `0` for the initial save.
    pub gen: u32,
}

impl Ref {
    /// Create a reference with generation 0.
    pub const fn new(id: u32) -> Self {
        Ref { id, gen: 0 }
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} R", self.id, self.gen)
    }
}

/// A PDF name object. Stored **without** the leading slash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(pub String);

impl Name {
    /// Create a name from anything string-like.
    pub fn new(s: impl Into<String>) -> Self {
        Name(s.into())
    }
}

/// A PDF string. Raw bytes; the serializer escapes as needed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PdfString(pub Vec<u8>);

impl PdfString {
    /// Create a string from UTF-8 text encoded as PDFDocEncoding-compatible
    /// ASCII (all bytes 0x20..=0x7e pass through; other code points are
    /// escaped octally by the serializer).
    pub fn text(s: &str) -> Self {
        PdfString(s.as_bytes().to_vec())
    }
}

/// A real number, serialized in full decimal notation (never `1e5`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Real(pub f64);

impl From<f64> for Real {
    fn from(v: f64) -> Self {
        Real(v)
    }
}

impl From<i64> for Object {
    fn from(v: i64) -> Self {
        Object::Int(v)
    }
}

impl From<f64> for Object {
    fn from(v: f64) -> Self {
        Object::Real(Real(v))
    }
}

impl From<bool> for Object {
    fn from(v: bool) -> Self {
        Object::Bool(v)
    }
}

impl From<&str> for Object {
    fn from(s: &str) -> Self {
        Object::Name(Name::new(s))
    }
}

impl From<Name> for Object {
    fn from(n: Name) -> Self {
        Object::Name(n)
    }
}

impl From<PdfString> for Object {
    fn from(s: PdfString) -> Self {
        Object::String(s)
    }
}

impl From<Ref> for Object {
    fn from(r: Ref) -> Self {
        Object::Ref(r)
    }
}

impl From<Vec<Object>> for Object {
    fn from(v: Vec<Object>) -> Self {
        Object::Array(v)
    }
}

impl From<Dict> for Object {
    fn from(d: Dict) -> Self {
        Object::Dict(d)
    }
}

/// A PDF stream object: dictionary plus raw (possibly filtered) bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    /// Additional entries; `/Length` is computed at serialization time.
    pub dict: Dict,
    /// Raw stream bytes, exactly as they will appear between `stream` and
    /// `endstream`.
    pub data: Vec<u8>,
}

impl Stream {
    /// Create a stream with the given extra dictionary entries.
    pub fn new(dict: Dict, data: Vec<u8>) -> Self {
        Stream { dict, data }
    }
}

/// An ordered PDF dictionary. Insertion order is preserved; setting an
/// existing key replaces it in place.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dict(pub Vec<(Name, Object)>);

impl Dict {
    /// New empty dictionary.
    pub fn new() -> Self {
        Dict(Vec::new())
    }

    /// Insert or replace a key.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Object>) -> &mut Self {
        let key = Name(key.into());
        let value = value.into();
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.0.push((key, value));
        }
        self
    }

    /// Builder-style insert.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Object>) -> Self {
        self.set(key, value);
        self
    }

    /// Look up a key.
    pub fn get(&self, key: &str) -> Option<&Object> {
        self.0.iter().find(|(k, _)| k.0 == key).map(|(_, v)| v)
    }

    /// Check membership.
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Any PDF object.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// Integer object.
    Int(i64),
    /// Real object.
    Real(Real),
    /// String object.
    String(PdfString),
    /// Name object (no leading slash stored).
    Name(Name),
    /// Array object.
    Array(Vec<Object>),
    /// Dictionary object.
    Dict(Dict),
    /// Stream object (always indirect in a file).
    Stream(Stream),
    /// Indirect reference.
    Ref(Ref),
}
