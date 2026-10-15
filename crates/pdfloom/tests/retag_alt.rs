//! P0-9 regression: TagSession::set_alt must emit a real Figure with the
//! alt text, and retagged saves must not depend on macOS system fonts.

use pdfloom::{Profile, Status, TagSession};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/output/untagged_alt.pdf"
);

/// Rebuild the shared untagged fixture if it is missing.
fn ensure_fixture() {
    if std::path::Path::new(FIXTURE).exists() {
        return;
    }
    let mut w = pdfloom_core::Document::new();
    let catalog = w.alloc();
    let pages = w.alloc();
    let page = w.alloc();
    let contents = w.alloc();
    let font = w.alloc();

    w.set_stream(
        contents,
        pdfloom_core::Stream::new(
            pdfloom_core::Dict::new(),
            b"BT /F0 24 Tf 72 700 Td (Quarterly Report) Tj ET\n\
              BT /F0 12 Tf 72 650 Td (Revenue grew 12 percent year over year.) Tj ET\n"
                .to_vec(),
        ),
    );
    w.set(
        font,
        pdfloom_core::Object::Dict(
            pdfloom_core::Dict::new()
                .with("Type", "Font")
                .with("Subtype", "Type1")
                .with("BaseFont", "Helvetica"),
        ),
    );
    w.set(
        page,
        pdfloom_core::Object::Dict(
            pdfloom_core::Dict::new()
                .with("Type", "Page")
                .with("Parent", pages)
                .with(
                    "MediaBox",
                    pdfloom_core::Object::Array(vec![
                        pdfloom_core::Object::Int(0),
                        pdfloom_core::Object::Int(0),
                        pdfloom_core::Object::Int(612),
                        pdfloom_core::Object::Int(792),
                    ]),
                )
                .with(
                    "Resources",
                    pdfloom_core::Object::Dict(pdfloom_core::Dict::new().with("Font", font)),
                )
                .with("Contents", contents),
        ),
    );
    w.set(
        pages,
        pdfloom_core::Object::Dict(
            pdfloom_core::Dict::new()
                .with("Type", "Pages")
                .with(
                    "Kids",
                    pdfloom_core::Object::Array(vec![pdfloom_core::Object::Ref(page)]),
                )
                .with("Count", 1),
        ),
    );
    w.set(
        catalog,
        pdfloom_core::Object::Dict(
            pdfloom_core::Dict::new()
                .with("Type", "Catalog")
                .with("Pages", pages),
        ),
    );
    let bytes = w
        .serialize(pdfloom_core::PdfVersion::V1_7, catalog)
        .unwrap();
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
