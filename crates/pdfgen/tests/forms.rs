//! Form creation and filling: create an AcroForm PDF, validate it, fill
//! a field, validate again.

use pdfgen::{fill_text_field, Document, Profile, Status};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
);

#[test]
fn forms_create_fill_and_stay_compliant() {
    if !std::path::Path::new(FONT).exists() {
        eprintln!("skipping: {FONT} not found");
        return;
    }
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

    // --- 1. Create a document with a labeled text field.
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Service Request Form").lang("en-US");
    doc.load_font(FONT).unwrap();

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

    // --- 1b. UA-2 twin: the same form must also pass the UA-2 profile
    // (the cleanup spec's Definition of Done covers forms in both).
    let mut doc2 = Document::new(Profile::PdfUa2);
    doc2.title("Service Request Form").lang("en-US");
    doc2.load_font(FONT).unwrap();
    let mut flow2 = doc2.flow();
    flow2.heading(1, "Service Request Form").unwrap();
    flow2
        .paragraph("Fill in the fields below; screen readers announce each label.")
        .unwrap();
    flow2.text_field("Full name", "fullname").unwrap();
    flow2.text_field("Email address", "email").unwrap();
    drop(flow2);
    let report2 = doc2.save(&format!("{out_dir}/form_ua2.pdf")).unwrap();
    assert_eq!(
        report2.status,
        Status::Compliant,
        "ua2 violations: {:#?}",
        report2.violations
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
