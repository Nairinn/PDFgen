//! Fonts-by-name: generate a document using the registry (embedded
//! Liberation, catalog Liberation Serif + a substitution) and validate
//! compliance. System-font resolution is exercised by pdfgen-fonts'
//! own probe tests; here every input is vendored, so the test runs
//! on any OS.

use pdfgen::{Document, Profile, Status};

#[test]
fn fonts_by_name_end_to_end() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Fonts by name").lang("en-US");

    // Embedded font by path (Liberation Sans, vendored with the repo).
    let sans = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
        ))
        .expect("vendored Liberation Sans loads");
    // Built-in catalog by name.
    let lib = doc
        .font("Liberation Serif", "Regular")
        .expect("catalog font");
    // Standard-14 alias -> embedded look-alike (not compliant-breaking).
    let helv = doc.font("Helvetica", "Regular").expect("alias resolves");
    let _ = helv;

    let mut flow = doc.flow();
    flow.paragraph_in(sans, 14.0, "This line uses embedded Liberation Sans.")
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
