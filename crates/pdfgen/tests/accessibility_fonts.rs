//! Accessibility fonts embed and stay compliant: Atkinson Hyperlegible
//! (low-vision legibility) and OpenDyslexic (CFF/OTF path).

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn accessibility_fonts_pass() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Accessibility font test").lang("en-US");

    let a = doc.font("Atkinson Hyperlegible", "Regular").expect("atkinson");
    let a_b = doc.font("Atkinson Hyperlegible", "Bold").expect("atkinson bold");
    let od = doc.font("OpenDyslexic", "Regular").expect("opendyslexic");
    let od_b = doc.font("OpenDyslexic", "Bold").expect("opendyslexic bold");

    {
        let mut flow = doc.flow();
        flow.heading(1, "Accessibility Fonts").unwrap();
        flow.paragraph_in(a, 12.0, "Atkinson Hyperlegible: letters and numbers read apart.")
            .unwrap();
        flow.paragraph_in(a_b, 12.0, "Atkinson Bold: same clarity, heavier weight.")
            .unwrap();
        flow.paragraph_in(od, 12.0, "OpenDyslexic: heavier bottoms anchor the line.")
            .unwrap();
        flow.paragraph_in(od_b, 12.0, "OpenDyslexic Bold for emphasis.").unwrap();
    }

    let path = out("accessibility_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
