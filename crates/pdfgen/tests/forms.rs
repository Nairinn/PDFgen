//! Form creation and filling: create an AcroForm PDF, validate it, fill
//! a field, validate again.

use pdfgen::{fill_text_field, Document, Profile, Status};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

#[test]
fn forms_create_fill_and_stay_compliant() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

    // --- 1. Create a document with a labeled text field.
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Service Request Form").lang("en-US");
    doc.load_font(ARIAL).unwrap();

    let mut flow = doc.flow();
    flow.heading(1, "Service Request Form").unwrap();
    flow.paragraph("Fill in the fields below; screen readers announce each label.")
        .unwrap();
    flow.text_field("Full name", "fullname").unwrap();
    flow.text_field("Email address", "email").unwrap();
    drop(flow);

    let path = format!("{out_dir}/form_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );

    // The file has an AcroForm with two /TU-annotated fields.
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes.windows(8).any(|w| w == b"AcroForm"),
        "AcroForm present"
    );
    assert!(bytes.windows(3).any(|w| w == b"/TU"), "TU present");
    assert!(
        bytes.windows(9).any(|w| w == b"(fullname"),
        "field name present"
    );

    // --- 2. Fill a field.
    fill_text_field(&path, "fullname", "Naing Lynn Kyaw").expect("fill");

    // --- 3. Verify: value present, structure intact, still parseable.
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes.windows(17).any(|w| w == b"(Naing Lynn Kyaw)"),
        "filled value present"
    );
    let mut r = pdfgen_parse::PdfReader::open(&path).unwrap();
    let catalog = r.catalog().unwrap();
    assert!(catalog.has("AcroForm"), "AcroForm survives the rewrite");
    assert!(catalog.has("StructTreeRoot"), "structure tree survives");
    let all = pdfgen::extract_text(&path).unwrap();
    assert!(
        all.iter()
            .any(|p| p.blocks.iter().any(|b| b.contains("Full name"))),
        "labels survive"
    );
}
