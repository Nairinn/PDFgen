//! HTML-to-PDF end-to-end: structural HTML in, compliant tagged PDF out.

use pdfgen::{html_to_pdf, html_to_pdf_profiled, Profile, Status};

#[test]
fn html_document_to_compliant_pdf() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/sample.html"
    );

    let html = std::fs::read_to_string(fixture).expect("fixture html");
    for (profile, name) in [(Profile::PdfUa1, "html_ua1"), (Profile::PdfUa2, "html_ua2")] {
        let path = format!("{out_dir}/{name}.pdf");
        let report = html_to_pdf_profiled(&html, &path, profile, "HTML Source Document", "en-US")
            .expect("convert");
        assert_eq!(
            report.status,
            Status::Compliant,
            "{name} violations: {:#?}",
            report.violations
        );
    }
}

#[test]
fn html_round_trips_through_extraction() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/sample.html"
    );
    let html = std::fs::read_to_string(fixture).expect("fixture html");
    let path = format!("{out_dir}/html_ua1.pdf");
    html_to_pdf(&html, &path, "HTML Source Document", "en-US").expect("convert");

    let pages = pdfgen::extract_text(&path).expect("extract");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(all.iter().any(|b| b.contains("HTML to PDF")), "{all:?}");
    assert!(all.iter().any(|b| b.contains("First list item")), "{all:?}");
}
