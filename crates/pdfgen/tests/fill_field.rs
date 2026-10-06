//! P0-7 regression: fill_text_field must append an incremental update
//! without corrupting the file.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

#[test]
fn fill_text_field_appends_incremental_update() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: no Arial on this machine");
        return;
    }
    // Build a fresh form document.
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Fill test").lang("en-US");
    doc.load_font(ARIAL).unwrap();
    doc.flow().text_field("Full name", "fullname").unwrap();

    let path = out("fill_field.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let before = std::fs::read(&path).unwrap();
    let before_eofs = before
        .windows(5)
        .filter(|w| w == b"%%EOF")
        .count();

    // Fill the field (the operation under test).
    pdfgen::fill_text_field(&path, "fullname", "Naing Lynn Kyaw").unwrap();

    let after = std::fs::read(&path).unwrap();

    // 1. The update is appended: prefix bytes unchanged, one more EOF.
    assert!(
        after.len() > before.len(),
        "the incremental section must be appended"
    );
    assert_eq!(&after[..before.len()], &before[..], "original bytes intact");
    let after_eofs = after
        .windows(5)
        .filter(|w| w == b"%%EOF")
        .count();
    assert_eq!(after_eofs, before_eofs + 1, "exactly one new %%EOF");

    // 2. The file still parses and the field holds the value.
    let pages = pdfgen::extract_text(&path).unwrap();
    let _ = pages;
    let mut reader = pdfgen_parse::PdfReader::open(&path).unwrap();
    let catalog = reader.catalog().unwrap();
    let pdfgen_core::Object::Ref(af) = catalog.get("AcroForm").cloned().unwrap() else {
        panic!("no AcroForm after fill");
    };
    let pdfgen_core::Object::Dict(af_d) = reader.get(af.id).unwrap() else {
        panic!("acroform not a dict");
    };
    let pdfgen_core::Object::Array(fields) = af_d.get("Fields").cloned().unwrap() else {
        panic!("no fields");
    };
    let pdfgen_core::Object::Ref(fr) = fields[0].clone() else {
        panic!("field not a ref");
    };
    let pdfgen_core::Object::Dict(fd) = reader.get(fr.id).unwrap() else {
        panic!("field not a dict");
    };
    let pdfgen_core::Object::String(v) = fd.get("V").cloned().unwrap() else {
        panic!("no /V after fill");
    };
    assert_eq!(&v.0, b"Naing Lynn Kyaw", "field value must be set");

    // 3. Structure tree survives: the reader resolves the tree root.
    assert!(
        catalog.get("StructTreeRoot").is_some(),
        "tags must survive the fill"
    );
}
