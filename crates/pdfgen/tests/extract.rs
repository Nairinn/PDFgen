//! Text extraction: read text back out of PDFs we generated (including
//! Flate-compressed streams) and the untagged fixture.

use pdfgen::extract_text;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn extracts_from_plain_pdf() {
    let pages = extract_text(&out("hello_ua1.pdf")).expect("hello parses");
    assert!(!pages.is_empty(), "at least one page");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(
        all.iter().any(|b| b.contains("Hello, tagged world!")),
        "heading found: {all:?}"
    );
}

#[test]
fn extracts_from_compressed_stream_pdf() {
    // streamed_ua1.pdf has Flate-compressed page content streams.
    let pages = extract_text(&out("streamed_ua1.pdf")).expect("streamed parses");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(all.len() > 400, "many blocks (got {})", all.len());
    assert!(
        all.iter().any(|b| b.contains("Section 499")),
        "last section found"
    );
}

#[test]
fn extracts_from_untagged_pdf() {
    let pages = extract_text(&out("untagged.pdf")).expect("fixture parses");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(all.iter().any(|b| b.contains("Quarterly Report")));
}
