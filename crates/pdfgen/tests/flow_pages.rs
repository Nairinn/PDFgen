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

/// P0-3: saving twice must produce identical, non-blank output.
#[test]
fn save_twice_produces_identical_output() {
    const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: no Arial on this machine");
        return;
    }
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Save twice").lang("en-US");
    doc.load_font(ARIAL).unwrap();
    doc.flow_heading(1, "Persistent heading").unwrap();
    doc.flow_paragraph("This content must survive the first save intact.")
        .unwrap();

    let first = out("save_twice_1.pdf");
    let report1 = doc.save(&first).unwrap();
    assert_eq!(report1.status, Status::Compliant);
    let second = out("save_twice_2.pdf");
    let report2 = doc.save(&second).unwrap();
    assert_eq!(report2.status, Status::Compliant);

    let a = std::fs::read(&first).unwrap();
    let b = std::fs::read(&second).unwrap();
    assert!(!a.is_empty() && a.len() > 10_000, "first save has content");
    assert_eq!(a.len(), b.len(), "second save must match the first");
    assert_eq!(a, b, "byte-identical output expected on re-save");
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
