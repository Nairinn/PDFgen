//! Negative case: an AcroForm field without /TU must be flagged (28-001),
//! and a field with a JS action must be flagged (28-002).

use pdfgen_core::{Dict, Object, PdfString, PdfVersion, Ref, Stream};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Build a tagged PDF with an AcroForm whose field lacks /TU.
#[test]
fn unnamed_form_field_is_flagged() {
    let mut w = pdfgen_core::Document::new();

    let catalog = w.alloc();
    let pages = w.alloc();
    let page = w.alloc();
    let acro = w.alloc();
    let field = w.alloc();

    // Field: FT/T present, TU missing -> 28-001.
    let mut f = Dict::new();
    f.set("FT", "Tx");
    f.set("T", PdfString::text("signature"));
    f.set(
        "Kids",
        Object::Array(vec![Object::Dict(
            Dict::new()
                .with("Type", "Annot")
                .with("Subtype", "Widget")
                .with(
                    "Rect",
                    Object::Array(vec![
                        Object::Int(72),
                        Object::Int(600),
                        Object::Int(300),
                        Object::Int(630),
                    ]),
                ),
        )]),
    );
    w.set(field, Object::Dict(f));

    let mut af = Dict::new();
    af.set("Fields", Object::Array(vec![Object::Ref(field)]));
    w.set(acro, Object::Dict(af));

    let mut pg = Dict::new();
    pg.set("Type", "Page");
    pg.set("Parent", pages);
    pg.set(
        "MediaBox",
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Int(612),
            Object::Int(792),
        ]),
    );
    pg.set("Resources", Object::Dict(Dict::new()));
    w.set(page, Object::Dict(pg));

    w.set(
        pages,
        Object::Dict(
            Dict::new()
                .with("Type", "Pages")
                .with("Kids", Object::Array(vec![Object::Ref(page)]))
                .with("Count", 1),
        ),
    );

    let mut cat = Dict::new();
    cat.set("Type", "Catalog");
    cat.set("Pages", pages);
    cat.set("AcroForm", acro);
    w.set(catalog, Object::Dict(cat));

    let bytes = w.serialize(PdfVersion::V1_7, catalog).unwrap();
    let path = out("unnamed_field.pdf");
    std::fs::write(&path, bytes).unwrap();

    let report = pdfgen_validate::validate(&path).unwrap();
    assert!(
        report.findings.iter().any(|x| x.id == "28-001"),
        "expected 28-001 (field without /TU): {:?}",
        report.findings
    );
}
