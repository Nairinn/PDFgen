//! The document builder: allocates object numbers, serializes a full PDF file.

use crate::{
    error::CoreError,
    serialize::{write_object, write_stream, xref_entry},
    Dict, Object, PdfVersion, Ref, Stream,
};

/// A PDF file under construction. Objects are stored by number; on
/// [`Document::serialize`] the writer emits header, body, classic
/// cross-reference table and trailer.
#[derive(Debug, Default)]
pub struct Document {
    /// Slot i-1 holds the object with number i.
    objects: Vec<Option<Object>>,
    /// Trailer entries beyond `/Size` and `/Root` (e.g. `/ID`, `/Info`).
    trailer: Dict,
}

impl Document {
    /// New empty document.
    pub fn new() -> Self {
        Document::default()
    }

    /// Allocate the next object number; the object starts as a hole and must
    /// be filled with [`Document::set`] before serializing.
    pub fn alloc(&mut self) -> Ref {
        self.objects.push(None);
        Ref::new(self.objects.len() as u32)
    }

    /// Assign a value to a previously allocated reference.
    pub fn set(&mut self, r: Ref, obj: Object) {
        self.objects[r.id as usize - 1] = Some(obj);
    }

    /// Insert an indirect stream. Streams must always be indirect objects.
    pub fn set_stream(&mut self, r: Ref, stream: Stream) {
        self.set(r, Object::Stream(stream));
    }

    /// Add or replace a trailer entry (e.g. `ID`, `Info`).
    pub fn trailer_set(&mut self, key: impl Into<String>, value: impl Into<Object>) {
        self.trailer.set(key, value);
    }

    /// Borrow an object by reference, if assigned.
    pub fn get(&self, r: Ref) -> Option<&Object> {
        self.objects.get(r.id as usize - 1).and_then(|o| o.as_ref())
    }

    /// Number of allocated objects (the future `/Size`, including slot 0).
    pub fn size(&self) -> u32 {
        self.objects.len() as u32 + 1
    }

    /// Serialize the complete file. `version` selects the header, `root` must
    /// reference the catalog dictionary.
    pub fn serialize(&self, version: PdfVersion, root: Ref) -> Result<Vec<u8>, CoreError> {
        for (i, slot) in self.objects.iter().enumerate() {
            let id = i as u32 + 1;
            let Some(obj) = slot.as_ref() else {
                return Err(CoreError::Unassigned { id });
            };
            check_no_nested_stream(obj, id)?;
        }

        let mut out = Vec::with_capacity(4096);
        out.extend_from_slice(format!("%PDF-{}\n", version.header()).as_bytes());
        // Binary comment line marking the file as binary.
        out.extend_from_slice(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);

        let mut offsets = vec![0u64; self.objects.len()];
        for (i, slot) in self.objects.iter().enumerate() {
            let id = i as u32 + 1;
            offsets[i] = out.len() as u64;
            let obj = slot.as_ref().expect("checked above");
            match obj {
                Object::Stream(s) => {
                    let mut head = format!("{id} 0 obj\n");
                    write_stream(&mut head, s);
                    out.extend_from_slice(head.as_bytes());
                    out.extend_from_slice(&s.data);
                    out.extend_from_slice(b"\nendstream\nendobj\n");
                }
                obj => {
                    let mut head = format!("{id} 0 obj\n");
                    write_object(&mut head, obj);
                    head.push_str("\nendobj\n");
                    out.extend_from_slice(head.as_bytes());
                }
            }
        }

        let xref_off = out.len() as u64;
        let count = self.objects.len() as u32 + 1;
        let mut xref = format!("xref\n0 {count}\n");
        xref.push_str(&crate::serialize::free_head_entry());
        for off in &offsets {
            xref.push_str(&xref_entry(*off, 0));
        }
        out.extend_from_slice(xref.as_bytes());

        let mut trailer = Dict::new();
        trailer.set("Size", count as i64);
        trailer.set("Root", Object::Ref(root));
        for (k, v) in &self.trailer.0 {
            trailer.set(k.0.clone(), v.clone());
        }
        let mut t = String::from("trailer\n");
        write_object(&mut t, &Object::Dict(trailer));
        t.push_str(&format!("\nstartxref\n{xref_off}\n%%EOF\n"));
        out.extend_from_slice(t.as_bytes());
        Ok(out)
    }
}

fn check_no_nested_stream(obj: &Object, container: u32) -> Result<(), CoreError> {
    match obj {
        Object::Array(items) => {
            for i in items {
                check_no_nested_stream(i, container)?;
            }
        }
        Object::Dict(d) => {
            for (_, v) in &d.0 {
                check_no_nested_stream(v, container)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Convenience: build a string object from text.
pub fn text(s: &str) -> Object {
    Object::String(crate::PdfString::text(s))
}
