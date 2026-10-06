//! Interactive form fields (AcroForm): creation and filling.
//!
//! PDF/UA requires every widget to be reachable in the structure tree and
//! to carry a `/TU` accessible name (Matterhorn 11-005, 28-x). The Flow
//! API records [`FieldSpec`]s; save() emits the AcroForm dictionary, one
//! field object per spec with `/TU` = the label, and a widget annotation
//! on the owning page. Filling is done by writing `/V` on the field —
//! see [`fill_text_field`].

use pdfgen_core::{Dict, Object, PdfString, Ref};
use pdfgen_parse::PdfReader;

/// A text field to create, recorded by the flow API.
#[derive(Debug, Clone)]
pub struct FieldSpec {
    /// Fully-qualified field name (`/T`).
    pub name: String,
    /// Accessible name (`/TU`): read aloud by screen readers.
    pub tu: String,
    /// Page index the widget lives on.
    pub page: usize,
    /// Widget rectangle x.
    pub x: f64,
    /// Widget rectangle y.
    pub y: f64,
    /// Widget rectangle width.
    pub w: f64,
    /// Widget rectangle height.
    pub h: f64,
}

/// Emit one field object + its widget annotation. Returns the field ref.
pub(crate) fn emit_field(doc: &mut pdfgen_core::Document, spec: &FieldSpec, page_ref: Ref) -> Ref {
    let field = doc.alloc();
    let mut f = Dict::new();
    f.set("FT", "Tx");
    f.set("T", PdfString::text(&spec.name));
    f.set("TU", PdfString::text(&spec.tu));
    // The widget annotation IS the field's /Kids entry here (merged).
    let mut widget = Dict::new();
    widget.set("Type", "Annot");
    widget.set("Subtype", "Widget");
    widget.set(
        "Rect",
        Object::Array(vec![
            Object::Real(spec.x.into()),
            Object::Real(spec.y.into()),
            Object::Real((spec.x + spec.w).into()),
            Object::Real((spec.y + spec.h).into()),
        ]),
    );
    widget.set("P", page_ref);
    widget.set("F", 4); // print flag
    f.set("Kids", Object::Array(vec![Object::Dict(widget)]));
    doc.set(field, Object::Dict(f));
    field
}

/// Emit the AcroForm dictionary with the given field refs. Returns the
/// AcroForm ref.
pub(crate) fn emit_acroform(doc: &mut pdfgen_core::Document, field_refs: &[Ref]) -> Ref {
    let af = doc.alloc();
    let mut d = Dict::new();
    d.set(
        "Fields",
        Object::Array(field_refs.iter().map(|&r| Object::Ref(r)).collect()),
    );
    d.set("NeedAppearances", true);
    doc.set(af, Object::Dict(d));
    af
}

/// Decode a PDF text string: UTF-16BE with a BOM when it starts FE FF,
/// PDFDocEncoding (treat as Latin-1) otherwise.
fn decode_pdf_text_string(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        // UTF-16BE after the BOM.
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{fffd}'))
            .collect()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Find the field object whose /T matches `name`, walking Kids
/// hierarchies. Returns its object id.
fn find_field(
    reader: &mut PdfReader,
    entries: &[Object],
    name: &str,
    depth: usize,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    if depth > 32 {
        return Ok(None);
    }
    for f in entries {
        if let Object::Ref(r) = f {
            if let Ok(Object::Dict(fd)) = reader.get(r.id) {
                let matches = fd
                    .get("T")
                    .and_then(|t| match t {
                        Object::String(s) => Some(decode_pdf_text_string(&s.0) == name),
                        _ => None,
                    })
                    .unwrap_or(false);
                if matches {
                    return Ok(Some(r.id));
                }
                // Descend into Kids.
                if let Some(Object::Array(kids)) = fd.get("Kids").cloned() {
                    if let Some(found) = find_field(reader, &kids, name, depth + 1)? {
                        return Ok(Some(found));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Fill a text field in an existing PDF by APPENDING an incremental update:
/// the patched field object plus a new xref section chained with /Prev.
/// The original bytes are never renumbered, so tags, IDs and metadata stay
/// intact, and pdfgen-revision keeps working on the updated file.
pub fn fill_text_field(
    path: &str,
    name: &str,
    value: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    let mut reader = PdfReader::from_bytes(data.clone())?;
    let catalog = reader.catalog()?;

    // Find AcroForm -> Fields.
    let Some(Object::Ref(af_ref)) = catalog.get("AcroForm").cloned() else {
        return Err(format!("no AcroForm in {path}").into());
    };
    let Object::Dict(af) = reader.get(af_ref.id)? else {
        return Err("AcroForm is not a dictionary".into());
    };
    let Some(Object::Array(fields)) = af.get("Fields").cloned() else {
        return Err("AcroForm has no Fields".into());
    };

    let Some(field_id) = find_field(&mut reader, &fields, name, 0)? else {
        return Err(format!("field {name:?} not found").into());
    };

    // Patch /V onto the field dict.
    let Object::Dict(mut fd) = reader.get(field_id)? else {
        return Err("field object is not a dictionary".into());
    };
    fd.set("V", PdfString::text(value));

    // --- Append the incremental section ---------------------------------
    let prev_xref = last_startxref(&data).unwrap_or(0);
    let base = data.len() as u64;
    let mut section: Vec<u8> = Vec::with_capacity(1024);
    let mut cursor = base;

    // The patched field object, at its original id.
    let mut serialized_field = format!("{field_id} 0 obj\n");
    pdfgen_core::write_object(&mut serialized_field, &Object::Dict(fd));
    serialized_field.push_str("\nendobj\n");
    section.extend_from_slice(serialized_field.as_bytes());
    let field_off = cursor;
    cursor += serialized_field.len() as u64;

    // xref + trailer with /Prev. /Root (and /ID when present) carry over
    // so a reader that reads only the newest trailer still resolves the
    // catalog; the /Prev chain preserves the rest.
    let size = match reader.trailer().get("Size") {
        Some(Object::Int(s)) => *s as u32,
        _ => field_id + 1,
    };
    let Some(Object::Ref(root)) = reader.trailer().get("Root").cloned() else {
        return Err("trailer has no /Root".into());
    };
    let mut trailer = format!("<< /Size {size} /Prev {prev_xref} /Root {} {} R", root.id, root.gen);
    if let Some(Object::Array(ids)) = reader.trailer().get("ID").cloned() {
        let parts: Vec<String> = ids
            .iter()
            .filter_map(|i| match i {
                Object::String(s) => Some(format!("<{}>", hex(&s.0))),
                _ => None,
            })
            .collect();
        if parts.len() == ids.len() && !parts.is_empty() {
            trailer.push_str(&format!(" /ID [{}]", parts.join(" ")));
        }
    }
    trailer.push_str(" >>\n");
    let xref_at = cursor;
    let mut xref = format!("xref\n{field_id} 1\n{field_off:010} 00000 n \n");
    xref.push_str("trailer\n");
    xref.push_str(&trailer);
    xref.push_str(&format!("startxref\n{xref_at}\n%%EOF\n"));
    section.extend_from_slice(xref.as_bytes());

    // Append to the file in place.
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
    f.write_all(&section)?;
    Ok(())
}

/// Hex-encode bytes for a PDF hex string.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Byte offset of the last `startxref` value in the file.
fn last_startxref(data: &[u8]) -> Option<u64> {
    let idx = data
        .windows(9)
        .rposition(|w| w == b"startxref")?;
    let rest = &data[idx + 9..];
    let s = std::str::from_utf8(rest).ok()?;
    let num: String = s
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num.parse::<u64>().ok()
}
