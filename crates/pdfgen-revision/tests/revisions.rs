//! Revision control end to end: commit -> history -> diff -> revert, with
//! the committed file staying parseable and veraPDF-valid, and revert
//! producing a byte-identical copy of the original.

use pdfgen::{Document, Profile};
use pdfgen_revision::{commit, diff, history, revert, DiffKind};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

fn tmp(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn commit_history_diff_revert() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let path = tmp("versioned.pdf");

    // --- Revision 1: the original save.
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Versioned Document").lang("en-US");
    doc.load_font(ARIAL).unwrap();
    {
        let mut flow = doc.flow();
        flow.heading(1, "Versioned Document").unwrap();
        flow.paragraph("Revision one body text.").unwrap();
    }
    let report = doc.save(&path).unwrap();
    assert!(report.status == pdfgen::Status::Compliant);
    let original = std::fs::read(&path).unwrap();

    // --- Commit 2 and 3.
    let c2 = commit(&path, "Second revision: edited body", "Nairinn").unwrap();
    assert_eq!(c2.index, 2);
    assert_eq!(c2.author, "Nairinn");
    let c3 = commit(&path, "Third revision: final polish", "Nairinn").unwrap();
    assert_eq!(c3.index, 3);
    assert_eq!(c3.message, "Third revision: final polish");

    // --- History lists all three in order.
    let h = history(&path).unwrap();
    assert_eq!(h.len(), 3, "three revisions");
    assert_eq!(h[0].message, "", "original has no message");
    assert_eq!(h[1].message, "Second revision: edited body");
    assert_eq!(h[2].message, "Third revision: final polish");
    assert!(h[2].date.ends_with('Z'), "RFC 3339 date: {}", h[2].date);

    // --- The committed file still parses.
    let mut reader = pdfgen_parse::PdfReader::open(&path).unwrap();
    let catalog = reader.catalog().unwrap();
    assert!(catalog.has("Pages"));

    // --- Diff rev1 -> rev3: at least the added revision-record objects.
    let d = diff(&path, 1, 3).unwrap();
    assert!(
        d.iter().any(|e| e.kind == DiffKind::Added),
        "added objects expected: {d:?}"
    );
    // Diff rev2 -> rev3 is smaller than rev1 -> rev3.
    let d23 = diff(&path, 2, 3).unwrap();
    assert!(d23.len() <= d.len());

    // --- Revert to rev1 gives back the EXACT original bytes.
    let out = tmp("versioned_reverted.pdf");
    revert(&path, 1, &out).unwrap();
    let reverted = std::fs::read(&out).unwrap();
    assert_eq!(
        original, reverted,
        "revert must restore revision 1 byte-for-byte"
    );

    // --- The reverted file still validates.
    let v = pdfgen_validate::validate(&out).unwrap();
    assert!(v.is_clean(), "reverted file machine checks: {:?}", v.findings);
}

#[test]
fn revert_out_of_range_is_an_error() {
    let path = tmp("versioned.pdf");
    if !std::path::Path::new(&path).exists() {
        eprintln!("skipping: versioned.pdf not generated yet");
        return;
    }
    assert!(revert(&path, 99, tmp("nope.pdf")).is_err());
}
