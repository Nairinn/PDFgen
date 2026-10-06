//! Script fonts from the catalog flow through the CID path and stay
//! compliant: Myanmar, Thai, Arabic. Latin text uses a Latin-capable font.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn script_catalog_fonts_pass() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Script font catalog test").lang("en-US");

    // Font 0 must cover Latin (headings/labels); script fonts follow.
    let latin = doc.font("Noto Sans", "Regular").expect("latin");
    let mm = doc.font("Noto Sans Myanmar", "Regular").expect("myanmar");
    let th = doc.font("Noto Sans Thai", "Regular").expect("thai");
    let ar = doc.font("Noto Sans Arabic", "Regular").expect("arabic");

    {
        let mut flow = doc.flow();
        flow.heading(1, "Script Fonts From the Catalog").unwrap();
        // Myanmar: "မင်္ဂလာပါ" (mingalaba - hello)
        flow.paragraph_in(
            mm,
            12.0,
            "\u{1019}\u{103C}\u{1004}\u{103A}\u{1002}\u{1031}\u{102C}\u{1004}\u{103A}\u{1015}\u{103C}\u{1000}\u{103A}",
        )
        .unwrap();
        // Thai: "สวัสดีครับ" (sawasdee krab - hello)
        flow.paragraph_in(
            th,
            12.0,
            "\u{0E2A}\u{0E27}\u{0E31}\u{0E2A}\u{0E14}\u{0E35}\u{0E04}\u{0E23}\u{0E31}\u{0E1A}",
        )
        .unwrap();
        // Arabic: "مرحبا" (marhaba - hello)
        flow.paragraph_in(ar, 12.0, "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}")
            .unwrap();
        flow.paragraph_in(
            latin,
            12.0,
            "Catalog scripts embed through the CID Type0 path with proper ToUnicode maps.",
        )
        .unwrap();
    }
    let _ = latin;

    let path = out("scripts_catalog_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
