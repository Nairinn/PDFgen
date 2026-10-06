//! P0 regression: flow() must continue on the same page, and pages must
//! keep their own MediaBox sizes.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// P0-1: consecutive one-shot flow helpers must not start new pages.
#[test]
fn flow_calls_share_one_page() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Flow continuity").lang("en-US");
    doc.load_font("/System/Library/Fonts/Supplemental/Arial.ttf")
        .unwrap();
    doc.flow_heading(1, "Heading").unwrap();
    doc.flow_paragraph("First paragraph body text.").unwrap();
    doc.flow_paragraph("Second paragraph body text.").unwrap();

    let path = out("flow_continuous.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    let counts: Vec<&str> = txt
        .match_indices("/Count ")
        .map(|(i, _)| &txt[i..i + 12])
        .collect();
    // The page tree /Count must be 1 (the outline Count may also appear).
    assert!(
        counts.iter().any(|c| c.starts_with("/Count 1")),
        "expected a single page, got {counts:?}"
    );
    assert!(
        !counts.iter().any(|c| c.starts_with("/Count 3")),
        "one-shot flow helpers must share a page, got {counts:?}"
    );
}

/// P0-2: explicit pages keep their own sizes; the flow page stays Letter.
#[test]
fn explicit_pages_keep_own_sizes() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Mixed sizes").lang("en-US");
    doc.load_font("/System/Library/Fonts/Supplemental/Arial.ttf")
        .unwrap();

    {
        let mut p1 = doc.add_page(300.0, 300.0);
        p1.paragraph("small square page").unwrap();
        drop(p1);
    }
    {
        let mut p2 = doc.add_page(800.0, 500.0);
        p2.paragraph("wide landscape page").unwrap();
        drop(p2);
    }
    doc.flow_paragraph("flowing letter page").unwrap();

    let path = out("mixed_sizes.pdf");
    let report = doc.save(&path).unwrap();
    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    let boxes: Vec<&str> = txt
        .match_indices("/MediaBox ")
        .map(|(i, _)| &txt[i..i + 30])
        .filter(|b| b.contains('['))
        .collect();
    let count_300 = boxes.iter().filter(|b| b.contains("300")).count();
    let count_800 = boxes.iter().filter(|b| b.contains("800")).count();
    let count_612 = boxes.iter().filter(|b| b.contains("612")).count();
    assert_eq!(count_300, 1, "300x300 page must exist once: {boxes:?}");
    assert_eq!(count_800, 1, "800x500 page must exist once: {boxes:?}");
    assert_eq!(count_612, 1, "flow page must stay Letter: {boxes:?}");
    let _ = report.status;
}
