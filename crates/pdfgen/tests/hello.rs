//! End-to-end M1 test: generate a tagged PDF/UA document and check the
//! report status. veraPDF validation runs out-of-band (see xtask / CI).

use pdfgen::{Document, Object, Profile, Status};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

#[test]
fn hello_ua1_and_ua2() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found (run on macOS or set PDFGEN_TEST_FONT)");
        return;
    }
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    std::fs::create_dir_all(out_dir).unwrap();

    for (profile, name) in [
        (Profile::PdfUa1, "hello_ua1"),
        (Profile::PdfUa2, "hello_ua2"),
    ] {
        let mut doc = Document::new(profile);
        doc.title("Hello, tagged world").lang("en-US");
        doc.load_font(ARIAL).unwrap();

        let mut flow = doc.flow();
        flow.heading(1, "Hello, tagged world!").unwrap();
        flow.paragraph("This PDF has a real structure tree, embedded fonts")
            .unwrap();
        flow.paragraph("and an accessibility report on every save.")
            .unwrap();
        drop(flow);

        let path = format!("{out_dir}/{name}.pdf");
        let report = doc.save(&path).unwrap();
        assert!(
            std::path::Path::new(&path).exists(),
            "file was written even if non-compliant"
        );
        for v in &report.violations {
            eprintln!("{}: {} — {}", v.id, v.message, v.fix);
        }
        assert_eq!(
            report.status,
            Status::Compliant,
            "expected M1 doc to pass machine checks"
        );
    }
}

#[test]
fn flow_wraps_and_breaks_pages() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Multi-page flow test").lang("en-US");
    doc.load_font(ARIAL).unwrap();

    let mut flow = doc.flow();
    flow.heading(1, "Chapter 1: Flow").unwrap();
    // A paragraph long enough to wrap across lines AND split across a
    // page boundary (still ONE structure element with MCR pieces).
    let long = "The quick brown fox jumps over the lazy dog. ".repeat(120);
    flow.paragraph(&long).unwrap();
    // A heading that must not be orphaned at a page bottom.
    flow.heading(2, "Not an orphan").unwrap();
    flow.paragraph("Short paragraph after the keep-with-next heading.")
        .unwrap();
    drop(flow);

    let path = format!("{out_dir}/multipage_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:?}",
        report.violations
    );

    // Multi-page: the long paragraph forces at least 2 pages.
    let mut reader = pdfgen_parse::PdfReader::open(&path).unwrap();
    let catalog = reader.catalog().unwrap();
    let Some(Object::Ref(pages_ref)) = catalog.get("Pages").cloned() else {
        panic!("no pages");
    };
    let pages = reader.get(pages_ref.id).unwrap();
    let Object::Dict(pd) = &pages else {
        panic!("pages not a dict")
    };
    let count = match pd.get("Count") {
        Some(Object::Int(c)) => *c,
        _ => 0,
    };
    assert!(count >= 2, "expected >= 2 pages, got {count}");
}

#[test]
fn missing_title_and_lang_report_but_still_save() {
    if !std::path::Path::new(ARIAL).exists() {
        eprintln!("skipping: {ARIAL} not found");
        return;
    }
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let mut doc = Document::new(Profile::PdfUa1);
    // No title, no lang — must still save, with a note.
    doc.load_font(ARIAL).unwrap();
    let mut flow = doc.flow();
    flow.paragraph("Untitled document.").unwrap();
    drop(flow);

    let path = format!("{out_dir}/hello_noncompliant.pdf");
    let report = doc.save(&path).unwrap();
    assert!(std::path::Path::new(&path).exists(), "output never blocked");
    assert_eq!(report.status, Status::NotCompliantYet);
    let note = report.note().expect("non-compliant saves get a note");
    assert!(note.contains("not PDF/UA-1 compliant yet"), "note: {note}");
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !String::from_utf8_lossy(&bytes).contains("pdfuaid"),
        "non-compliant file must not claim PDF/UA"
    );
}
