//! Fonts by name: Noto catalog faces embedded and veraPDF-verified.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn noto_catalog_fonts_embed_and_pass() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Noto catalog test").lang("en-US");

    // All Noto faces resolve from the built-in catalog.
    let sans = doc.font("Noto Sans", "Regular").expect("Noto Sans");
    let sans_b = doc.font("Noto Sans", "Bold").expect("Noto Sans Bold");
    let serif = doc.font("Noto Serif", "Bold").expect("Noto Serif Bold");
    let mono = doc.font("Noto Sans Mono", "Regular").expect("Noto Sans Mono");

    {
        let mut flow = doc.flow();
        flow.heading(1, "Noto Catalog Test").unwrap();
        flow.paragraph_in(sans, 11.0, "This line uses Noto Sans Regular from the bundled catalog.")
            .unwrap();
        flow.paragraph_in(sans_b, 11.0, "This line uses Noto Sans Bold.")
            .unwrap();
        flow.paragraph_in(serif, 11.0, "This line uses Noto Serif Bold.")
            .unwrap();
        flow.paragraph_in(mono, 10.0, "This line uses Noto Sans Mono for code-like text.")
            .unwrap();
    }

    let path = out("noto_catalog_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );

    // The file must actually embed four Noto faces (no substitution note).
    assert!(
        !report
            .human_review
            .iter()
            .any(|n| n.contains("was used instead")),
        "no substitution expected: {:?}",
        report.human_review
    );
}
