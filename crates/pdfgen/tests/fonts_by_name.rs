//! Fonts-by-name: generate a document using the registry (system Arial +
//! catalog Liberation + a substitution) and validate compliance.

use pdfgen::{Document, Profile, Status};

#[test]
fn fonts_by_name_end_to_end() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Fonts by name").lang("en-US");

    // System font by name (real Arial from /System/Library/Fonts/...).
    let arial = doc.font("Arial", "Bold").expect("system Arial resolves");
    // Built-in catalog by name.
    let lib = doc
        .font("Liberation Serif", "Regular")
        .expect("catalog font");
    // Standard-14 alias -> embedded look-alike (not compliant-breaking).
    let helv = doc.font("Helvetica", "Regular").expect("alias resolves");
    let _ = helv;

    let mut flow = doc.flow();
    flow.paragraph_in(arial, 14.0, "This line uses system Arial Bold.")
        .unwrap();
    flow.paragraph_in(
        lib,
        12.0,
        "This line uses Liberation Serif from the built-in catalog.",
    )
    .unwrap();
    drop(flow);

    let path = format!("{out_dir}/fonts_by_name.pdf");
    let report = doc.save(&path).unwrap();

    // Substitution (Helvetica->Liberation Sans) is a review note, NOT a
    // violation: the file is still fully compliant.
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
    assert!(
        report.human_review.iter().any(|n| n.contains("Helvetica")),
        "substitution noted: {:?}",
        report.human_review
    );
}
