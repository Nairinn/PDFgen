//! ISO 5457 sheet end-to-end: A3 sheet, ISO frame + centring marks,
//! tagged title block, dimensions — veraPDF-validated.

use pdfgen::{Document, Profile, Status};
use pdfgen_draw::{Drawing, IsoSheet, TitleField};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn iso_sheet_is_compliant() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("FLANGE COVER - ISO Drawing").lang("en-US");
    doc.load_font(ARIAL).unwrap();

    // A3 landscape with the ISO 5457 frame and centring marks.
    let mut d = Drawing::new_iso(&mut doc, IsoSheet::A3);
    d.title_block(&[
        TitleField { label: "TITLE", value: "FLANGE COVER".into() },
        TitleField { label: "DWG NO", value: "ISO-77-001".into() },
        TitleField { label: "SCALE", value: "1:1".into() },
        TitleField { label: "SHEET", value: "A3".into() },
    ]);

    // Part geometry + two dimensions.
    d.part_rect(300.0, 260.0, 160.0, 90.0);
    d.linear_dimension(300.0, 260.0, 460.0, 260.0, 26.0, "80");
    d.linear_dimension(300.0, 260.0, 300.0, 350.0, 26.0, "45");

    d.finish_as_figure(
        "Flange cover drawing on an ISO A3 sheet. Rectangular outline \
         80 by 45 millimetres with linear dimensions. Title block: \
         FLANGE COVER, drawing ISO-77-001, scale 1:1, sheet A3.",
    );

    let path = out("drawing_iso_a3.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
