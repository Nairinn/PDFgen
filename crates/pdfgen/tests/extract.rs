//! Text extraction: read text back out of PDFs we generated (including
//! Flate-compressed streams) and the untagged fixture. Tests generate
//! their own fixtures so they pass in any order / fresh clones.

use pdfgen::{extract_text, Document, Profile};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

/// Build a simple tagged "hello" PDF if it is not already there.
fn ensure_hello() {
    let path = out("hello_ua1.pdf");
    if std::path::Path::new(&path).exists() {
        return;
    }
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: no Arial on this machine");
        return;
    }
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Hello").lang("en-US");
    doc.load_font(ARIAL).unwrap();
    {
        let mut flow = doc.flow();
        flow.heading(1, "Hello, tagged world!").unwrap();
        flow.paragraph("A paragraph under the heading.").unwrap();
    }
    doc.save(&path).unwrap();
}

/// Build the streamed multi-section PDF if it is not already there.
fn ensure_streamed() {
    let path = out("streamed_ua1.pdf");
    if std::path::Path::new(&path).exists() {
        return;
    }
    let mut w = pdfgen::StreamWriter::create(&path, pdfgen::Profile::PdfUa1, "Streamed", "en-US")
        .expect("stream writer");
    let mut batch = Vec::new();
    for i in 0..500 {
        let is_h = i % 25 == 0;
        batch.push(pdfgen::StreamEvent::Begin {
            tag: if is_h { "H1" } else { "P" }.into(),
            alt: None,
        });
        batch.push(pdfgen::StreamEvent::Text {
            text: if is_h {
                format!("Section {i}")
            } else {
                format!(
                    "Paragraph {i} with enough words to wrap across lines \
                     in the compressed stream for extraction to find."
                )
            },
            font: 0,
            size: 11.0,
        });
        batch.push(pdfgen::StreamEvent::End);
    }
    w.push(batch).unwrap();
    w.finish().unwrap();
}

#[test]
fn extracts_from_plain_pdf() {
    ensure_hello();
    let path = out("hello_ua1.pdf");
    if !std::path::Path::new(&path).exists() {
        eprintln!("skipping: fixture font unavailable");
        return;
    }
    let pages = extract_text(&path).expect("hello parses");
    assert!(!pages.is_empty(), "at least one page");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(
        all.iter().any(|b| b.contains("Hello, tagged world!")),
        "heading found: {all:?}"
    );
}

#[test]
fn extracts_from_compressed_stream_pdf() {
    ensure_streamed();
    let path = out("streamed_ua1.pdf");
    if !std::path::Path::new(&path).exists() {
        eprintln!("skipping: fixture font unavailable");
        return;
    }
    // streamed_ua1.pdf has Flate-compressed page content streams.
    let pages = extract_text(&path).expect("streamed parses");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(all.len() > 400, "many blocks (got {})", all.len());
    assert!(
        all.iter().any(|b| b.contains("Section 475")),
        "last heading found"
    );
}

#[test]
fn extracts_from_untagged_pdf() {
    // The retag suite builds untagged.pdf; rebuild a minimal one here if
    // it has not run yet so this test stands alone.
    let path = out("untagged.pdf");
    if !std::path::Path::new(&path).exists() {
        let mut w = pdfgen_core::Document::new();
        let catalog = w.alloc();
        let pages = w.alloc();
        let page = w.alloc();
        let contents = w.alloc();
        let font = w.alloc();

        w.set_stream(
            contents,
            pdfgen_core::Stream::new(
                pdfgen_core::Dict::new(),
                b"BT /F0 24 Tf 72 700 Td (Quarterly Report) Tj ET\n".to_vec(),
            ),
        );
        w.set(
            font,
            pdfgen_core::Object::Dict(
                pdfgen_core::Dict::new()
                    .with("Type", "Font")
                    .with("Subtype", "Type1")
                    .with("BaseFont", "Helvetica"),
            ),
        );
        w.set(
            page,
            pdfgen_core::Object::Dict(
                pdfgen_core::Dict::new()
                    .with("Type", "Page")
                    .with("Parent", pages)
                    .with(
                        "MediaBox",
                        pdfgen_core::Object::Array(vec![
                            pdfgen_core::Object::Int(0),
                            pdfgen_core::Object::Int(0),
                            pdfgen_core::Object::Int(612),
                            pdfgen_core::Object::Int(792),
                        ]),
                    )
                    .with(
                        "Resources",
                        pdfgen_core::Object::Dict(pdfgen_core::Dict::new().with("Font", font)),
                    )
                    .with("Contents", contents),
            ),
        );
        w.set(
            pages,
            pdfgen_core::Object::Dict(
                pdfgen_core::Dict::new()
                    .with("Type", "Pages")
                    .with(
                        "Kids",
                        pdfgen_core::Object::Array(vec![pdfgen_core::Object::Ref(page)]),
                    )
                    .with("Count", 1),
            ),
        );
        w.set(
            catalog,
            pdfgen_core::Object::Dict(
                pdfgen_core::Dict::new()
                    .with("Type", "Catalog")
                    .with("Pages", pages),
            ),
        );
        let bytes = w.serialize(pdfgen_core::PdfVersion::V1_7, catalog).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }

    let pages = extract_text(&path).expect("fixture parses");
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    assert!(all.iter().any(|b| b.contains("Quarterly Report")));
}
