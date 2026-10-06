//! P0-9 regression: TagSession::set_alt must emit a real Figure with the
//! alt text, and retagged saves must not depend on macOS system fonts.

use pdfgen::{Profile, Status, TagSession};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/output/untagged.pdf"
);

/// Rebuild the shared untagged fixture if it is missing.
fn ensure_fixture() {
    if std::path::Path::new(FIXTURE).exists() {
        return;
    }
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
            b"BT /F0 24 Tf 72 700 Td (Quarterly Report) Tj ET\n\
              BT /F0 12 Tf 72 650 Td (Revenue grew 12 percent year over year.) Tj ET\n"
                .to_vec(),
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
    std::fs::write(FIXTURE, bytes).unwrap();
}

#[test]
fn set_alt_produces_a_figure_with_alt() {
    ensure_fixture();
    let mut session = TagSession::open(FIXTURE).expect("open untagged");
    assert!(!session.is_empty(), "fixture should yield runs");

    // The second run (revenue sentence) becomes a figure with alt text.
    let idx = session
        .runs
        .iter()
        .position(|r| r.text.contains("Revenue"))
        .expect("revenue run");
    session.set_alt(idx, "Revenue chart: twelve percent growth year over year");

    let path = out("retag_alt_regression.pdf");
    let report = session
        .save(Profile::PdfUa1, "Retag alt test", "en-US", &path)
        .expect("save retagged");
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );

    // The Figure element must carry the alt text (UTF-16BE or ASCII in
    // the raw file; the text is ASCII so plain match works).
    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    assert!(
        txt.contains("/Figure"),
        "set_alt must produce a Figure element"
    );
    assert!(
        txt.contains("Revenue chart: twelve percent growth year over year"),
        "the alt text must be written into the structure tree"
    );
}
