//! End-to-end retag: import a PDF, tag it, save, and verify with the
//! same machine checks the writer uses.

use pdfgen::{Profile, Status, TagSession};
use pdfgen_core::{Dict, Object, PdfVersion, Stream};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/output/untagged.pdf"
);

/// Build an untagged PDF by hand: same content as our writer, but with
/// no StructTreeRoot, no MarkInfo, no marked content — the state of a
/// typical legacy PDF.
fn untagged_fixture() {
    let mut w = pdfgen_core::Document::new();
    let catalog = w.alloc();
    let pages = w.alloc();
    let page = w.alloc();
    let contents = w.alloc();
    let font = w.alloc();

    // A page of text with NO marked content.
    let text = "BT /F0 18 Tf 72 700 Td (Quarterly Report) Tj ET\n\
                BT /F0 11 Tf 72 660 Td (Revenue is up. Costs are flat.) Tj ET\n\
                BT /F0 11 Tf 72 640 Td (3) Tj ET\n";
    w.set_stream(contents, Stream::new(Dict::new(), text.as_bytes().to_vec()));

    // A real embedded font so the file is not garbage.
    let loaded = pdfgen::LoadedFont::load(ARIAL).expect("arial loads");
    let fdesc = w.alloc();
    let ffile = w.alloc();
    w.set_stream(
        ffile,
        Stream::new(
            Dict::new().with("Length1", loaded.raw.len() as i64),
            loaded.raw.clone(),
        ),
    );
    w.set(
        fdesc,
        Object::Dict(
            Dict::new()
                .with("Type", "FontDescriptor")
                .with(
                    "FontName",
                    Object::Name(pdfgen::Name::new(loaded.postscript_name.clone())),
                )
                .with("Flags", 32)
                .with("FontFile2", ffile),
        ),
    );
    w.set(
        font,
        Object::Dict(
            Dict::new()
                .with("Type", "Font")
                .with("Subtype", "TrueType")
                .with(
                    "BaseFont",
                    Object::Name(pdfgen::Name::new(loaded.postscript_name.clone())),
                )
                .with("FirstChar", 0)
                .with("LastChar", 255)
                .with("Widths", Object::Array(vec![Object::Int(0); 256]))
                .with("FontDescriptor", fdesc),
        ),
    );

    let mut pg = Dict::new();
    pg.set("Type", "Page");
    pg.set("Parent", pages);
    pg.set(
        "MediaBox",
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Int(612),
            Object::Int(792),
        ]),
    );
    pg.set(
        "Resources",
        Object::Dict(Dict::new().with(
            "Font",
            Object::Dict(Dict::new().with("F0", Object::Ref(font))),
        )),
    );
    pg.set("Contents", contents);
    w.set(page, Object::Dict(pg));

    w.set(
        pages,
        Object::Dict(
            Dict::new()
                .with("Type", "Pages")
                .with("Kids", Object::Array(vec![Object::Ref(page)]))
                .with("Count", 1),
        ),
    );

    let mut cat = Dict::new();
    cat.set("Type", "Catalog");
    cat.set("Pages", pages);
    w.set(catalog, Object::Dict(cat));

    let bytes = w.serialize(PdfVersion::V1_7, catalog).unwrap();
    std::fs::write(FIXTURE, bytes).unwrap();
}

#[test]
fn retag_untagged_pdf() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: no Arial on this machine");
        return;
    }
    untagged_fixture();

    // 1. Import: text runs come out, nothing tagged yet.
    let mut session = TagSession::open(FIXTURE).expect("open untagged pdf");
    assert!(!session.is_empty(), "should find text runs");
    assert!(
        session
            .runs
            .iter()
            .any(|r| r.text.contains("Quarterly Report")),
        "heading text found: {:?}",
        session
            .runs
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>()
    );

    // 2. Tag: first run -> H1 (auto), page number -> artifact.
    session.auto_tag();
    let page_number_idx = session
        .runs
        .iter()
        .position(|r| r.text.trim() == "3")
        .expect("page number run");
    session.set_artifact(page_number_idx);

    // 3. Save as UA-1.
    let out = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/output/retagged_ua1.pdf"
    );
    let report = session
        .save(Profile::PdfUa1, "Quarterly Report", "en-US", out)
        .expect("retag save");
    assert_eq!(report.status, Status::Compliant, "retagged doc must pass");
}
