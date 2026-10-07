//! Kitchen-sink test: lists, tables, figures, header/footer artifacts in
//! one document. veraPDF is the acceptance bar. Built in BOTH profiles
//! (UA-1 and UA-2) per the cleanup spec's Definition of Done.

use pdfgen::{Document, Image, Profile, Status};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
const RED_BOX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/red_box.png"
);

fn build(profile: Profile) -> Document {
    let mut doc = Document::new(profile);
    doc.title("Kitchen sink: lists, tables, figures")
        .lang("en-US");
    doc.load_font(ARIAL).unwrap();

    let img = Image::load(RED_BOX).expect("png loads");

    let mut flow = doc.flow();
    flow.header("Test Report — header furniture").unwrap();
    flow.heading(1, "Kitchen Sink Test").unwrap();
    flow.paragraph("A document exercising every structure element the flow API can produce.")
        .unwrap();

    flow.heading(2, "A list").unwrap();
    flow.bullet_list(&[
        "First bullet item with some text",
        "Second bullet item that is a little longer so it wraps once",
        "Third item",
    ])
    .unwrap();

    flow.heading(2, "A table").unwrap();
    flow.table(
        &["Pin", "Signal", "Color"],
        &[
            vec!["1", "VCC", "Red"],
            vec!["2", "GND", "Black"],
            vec!["3", "DATA", "Yellow with a longer note that wraps"],
        ],
        &[60.0, 120.0, 240.0],
    )
    .unwrap();

    flow.heading(2, "A figure").unwrap();
    flow.figure(&img, "A solid red square, 32 by 32 pixels", 96.0, 96.0)
        .unwrap();

    flow.paragraph("Closing paragraph after the figure.")
        .unwrap();
    flow.footer("Page furniture — footer").unwrap();
    drop(flow);
    doc
}

#[test]
fn lists_tables_figures_artifacts() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

    for (profile, name) in [
        (Profile::PdfUa1, "kitchensink_ua1"),
        (Profile::PdfUa2, "kitchensink_ua2"),
    ] {
        let mut doc = build(profile);
        let path = format!("{out_dir}/{name}.pdf");
        let report = doc.save(&path).unwrap();
        assert_eq!(
            report.status,
            Status::Compliant,
            "{name} violations: {:#?}",
            report.violations
        );
    }
}
