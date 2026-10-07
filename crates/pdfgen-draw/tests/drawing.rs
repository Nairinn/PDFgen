//! ASME drawing end-to-end: sheet frame, tagged title block, dimensions,
//! revision block fed from real revision history — all veraPDF-validated.

use pdfgen::{Document, Profile, Status};
use pdfgen_draw::{Drawing, Sheet, TitleField};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn asme_drawing_sheet_is_compliant() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    // A versioned file whose history feeds the revision block.
    let src = out("versioned.pdf");
    if !std::path::Path::new(&src).exists() {
        // Generate it: 1 save + 2 commits.
        let mut doc = Document::new(Profile::PdfUa1);
        doc.title("Part Under Revision").lang("en-US");
        doc.load_font(ARIAL).unwrap();
        {
            let mut flow = doc.flow();
            flow.heading(1, "Part Under Revision").unwrap();
            flow.paragraph("Body.").unwrap();
        }
        doc.save(&src).unwrap();
        pdfgen_revision::commit(&src, "Changed bolt pattern", "Nairinn").unwrap();
        pdfgen_revision::commit(&src, "Updated material callout", "Nairinn").unwrap();
    }

    // --- The drawing.
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("BRACKET, MOUNTING - Drawing").lang("en-US");
    doc.load_font(ARIAL).unwrap();

    let mut d = Drawing::new(&mut doc, Sheet::A);

    // Title block: real tagged table.
    d.title_block(&[
        TitleField {
            label: "TITLE",
            value: "BRACKET, MOUNTING".into(),
        },
        TitleField {
            label: "DWG NO",
            value: "A-1234-B".into(),
        },
        TitleField {
            label: "SCALE",
            value: "1:2".into(),
        },
        TitleField {
            label: "SHEET",
            value: "1 OF 1".into(),
        },
    ]);

    // Part geometry (artifact) + dimensions.
    d.part_rect(150.0, 120.0, 120.0, 60.0);
    d.linear_dimension(150.0, 120.0, 270.0, 120.0, 30.0, "2.000");
    d.linear_dimension(150.0, 120.0, 150.0, 180.0, 30.0, "1.000");

    // Revision block from the versioned file's history.
    let rows = d.revision_block(&src).expect("history");
    assert_eq!(rows, 2, "two committed revisions in the block");

    // The whole drawing is one accessible Figure.
    d.finish_as_figure(
        "Mounting bracket drawing. Rectangular outline 2 by 1 inches \
         with two linear dimensions. Title block: BRACKET, MOUNTING, \
         drawing A-1234-B, scale 1:2, sheet 1 of 1.",
    );

    let path = out("drawing_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}

#[test]
fn asme_drawing_sheet_is_compliant_ua2() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let mut doc = Document::new(Profile::PdfUa2);
    doc.title("BRACKET, MOUNTING - Drawing").lang("en-US");
    doc.load_font(ARIAL).unwrap();

    let mut d = Drawing::new(&mut doc, Sheet::A);
    d.title_block(&[
        TitleField {
            label: "TITLE",
            value: "BRACKET, MOUNTING".into(),
        },
        TitleField {
            label: "DWG NO",
            value: "A-1234-B".into(),
        },
    ]);
    d.part_rect(150.0, 120.0, 120.0, 60.0);
    d.finish_as_figure("Mounting bracket drawing, UA-2 build.");
    let path = out("drawing_ua2.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
