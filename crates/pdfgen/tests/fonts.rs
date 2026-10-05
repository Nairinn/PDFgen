//! Facade-level font tests: documents built with fonts resolved by name.

use pdfgen::{Document, Profile, Status};

#[test]
fn document_font_by_name() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Fonts by name").lang("en-US");

    // By catalog name, standard-14 alias, and an unknown name (fallback).
    let _sans = doc.font("Liberation Sans", "Regular").unwrap();
    let _helv = doc.font("Helvetica", "Bold").unwrap();
    let _serif = doc.font("Liberation Serif", "Italic").unwrap();

    let mut flow = doc.flow();
    flow.heading(1, "Fonts by name").unwrap();
    flow.paragraph("Catalog font.").unwrap();
    drop(flow);

    let path = format!("{out_dir}/fonts_by_name.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(report.status, Status::Compliant, "{:#?}", report.violations);
    // Helvetica->Liberation Sans is a substitution but stays compliant;
    // it should be surfaced in the review notes.
    assert!(
        report.human_review.iter().any(|n| n.contains("Helvetica")),
        "substitution surfaced: {:?}",
        report.human_review
    );
}
