//! CID (Type0) fonts: text WinAnsi cannot encode flows through Identity-H
//! composite fonts — Myanmar, Korean, Greek — and stays PDF/UA-compliant.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn cid_text_passes_verapdf() {
    let mm = "/System/Library/Fonts/Supplemental/Myanmar MN.ttc";
    let uni = "/System/Library/Fonts/Supplemental/Arial Unicode.ttf";
    if !std::path::Path::new(mm).exists() || !std::path::Path::new(uni).exists() {
        eprintln!("skipping: needs macOS Myanmar MN + Arial Unicode fonts");
        return;
    }

    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("CID font test").lang("en-US");

    // Two fonts, each exercised through the CID (Type0) path.
    let myanmar = doc.load_font(mm).expect("Myanmar MN");
    let unicode = doc.load_font(uni).expect("Arial Unicode");

    {
        let mut flow = doc.flow();
        flow.heading(1, "CID Type0 Font Test").unwrap();
        flow.paragraph_in(
            myanmar,
            12.0,
            "Myanmar: \u{1000}\u{1001}\u{1002}\u{1019}\u{103C}\u{102D}\u{1004}\u{103A}",
        )
        .unwrap();
        flow.paragraph_in(
            unicode,
            12.0,
            "Korean: \u{C548}\u{B155}\u{D558}\u{C138}\u{C694} — Greek: \u{0393}\u{03B5}\u{03B9}\u{03AC} \u{03C3}\u{03BF}\u{03C5}",
        )
        .unwrap();
        flow.paragraph_in(
            unicode,
            12.0,
            "Plain ASCII stays on the simple (WinAnsi) font path and shares the same document.",
        )
        .unwrap();
    }

    let path = out("cid_test_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
