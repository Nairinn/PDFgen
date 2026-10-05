//! Interactive form fields (AcroForm): creation and filling.
//!
//! PDF/UA requires every widget to be reachable in the structure tree and
//! to carry a `/TU` accessible name (Matterhorn 11-005, 28-x). The Flow
//! API records [`FieldSpec`]s; save() emits the AcroForm dictionary, one
//! field object per spec with `/TU` = the label, and a widget annotation
//! on the owning page. Filling is done by writing `/V` on the field —
//! see [`fill_text_field`].

use pdfgen_core::{Dict, Name, Object, PdfString, Ref, Stream};
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
pub(crate) fn emit_field(
    doc: &mut pdfgen_core::Document,
    spec: &FieldSpec,
    page_ref: Ref,
) -> Ref {
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
    f.set(
        "Kids",
        Object::Array(vec![Object::Dict(widget)]),
    );
    doc.set(field, Object::Dict(f));
    field
}

/// Emit the AcroForm dictionary with the given field refs. Returns the
/// AcroForm ref.
pub(crate) fn emit_acroform(
    doc: &mut pdfgen_core::Document,
    field_refs: &[Ref],
) -> Ref {
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

/// Fill a text field in an existing PDF: sets `/V` on the named field and
/// regenerates appearance streams via `/NeedAppearances` (viewers render
/// the value). The file is rewritten in place (full rewrite; incremental
/// saves come with revision control).
pub fn fill_text_field(path: &str, name: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = PdfReader::open(path)?;
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

    // Locate the field by /T.
    let mut field_ref: Option<Ref> = None;
    for f in fields {
        if let Object::Ref(r) = f {
            if let Object::Dict(fd) = reader.get(r.id)? {
                if let Some(Object::String(t)) = fd.get("T") {
                    // Decode PDFString (bytes) and compare.
                    let decoded = String::from_utf8_lossy(&t.0).into_owned();
                    if decoded == name {
                        field_ref = Some(r);
                        break;
                    }
                }
            }
        }
    }
    let Some(r) = field_ref else {
        return Err(format!("field {name:?} not found").into());
    };

    // Set /V on the field object. Rebuild the whole file: read every
    // object we can, then rewrite with the field updated.
    let mut out = pdfgen_core::Document::new();
    let mut remap: std::collections::HashMap<u32, Ref> = std::collections::HashMap::new();

    let ids: Vec<u32> = reader
        .object_ids()
        .iter()
        .copied()
        .filter(|&id| id != 0)
        .collect();
    for id in ids {
        if let Ok(obj) = reader.get(id) {
            let new_ref = out.alloc();
            remap.insert(id, new_ref);
            let obj = if id == r.id {
                // Patch /V into this field dict.
                if let Object::Dict(mut fd) = obj {
                    fd.set("V", PdfString::text(value));
                    Object::Dict(fd)
                } else {
                    obj
                }
            } else {
                obj
            };
            out.set(new_ref, obj);
        }
    }

    // Root: same id as before (remap the trailer Root id).
    let Some(Object::Ref(root)) = reader.trailer().get("Root").cloned() else {
        return Err("trailer has no /Root".into());
    };
    let Some(&new_root) = remap.get(&root.id) else {
        return Err("catalog could not be remapped".into());
    };

    let bytes = out.serialize(pdfgen_core::PdfVersion::V1_7, new_root)?;
    std::fs::write(path, bytes)?;
    Ok(())
}
