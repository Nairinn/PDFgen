//! CID (Type0) fonts: text WinAnsi cannot encode flows through Identity-H
//! composite fonts — Myanmar, Korean, Greek — and stays PDF/UA-compliant.
//! Fonts are vendored (Noto Myanmar + optional CJK download), so the
//! test runs on any OS. The CJK face is fetched by
//! scripts/fetch-cjk-fonts.sh; without it the Korean/Greek half skips.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn cid_text_passes_verapdf() {
    let mm = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
    );
    let uni = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/noto/NotoSansCJKjp-Regular.otf"
    );
    if !std::path::Path::new(mm).exists() {
        eprintln!("skipping: vendored Myanmar font missing");
        return;
    }
    let have_cjk = std::path::Path::new(uni).exists();

    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("CID font test").lang("en-US");

    // Latin font FIRST so the default font (headings) has Latin glyphs;
    // Noto Sans Myanmar carries no Latin.
    // Font 0 = Latin (headings); the myanmar face loads after it.
    doc.load_font(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
    ))
    .expect("Liberation Sans");
    let myanmar = doc.load_font(mm).expect("Noto Sans Myanmar");
    let cjk = if have_cjk {
        Some(doc.load_font(uni).expect("Noto Sans CJK JP"))
    } else {
        None
    };

    {
        let mut flow = doc.flow();
        flow.heading(1, "CID Type0 Font Test").unwrap();
        flow.paragraph_in(
            myanmar,
            12.0,
            "\u{1000}\u{1001}\u{1002}\u{1019}\u{103C}\u{102D}\u{1004}\u{103A}",
        )
        .unwrap();
        if let Some(unicode) = cjk {
            // Noto Sans CJK JP includes Latin, so the labels can stay.
            flow.paragraph_in(
                unicode,
                12.0,
                "Korean: \u{C548}\u{B155}\u{D558}\u{C138}\u{C694} — Greek: \u{0393}\u{03B5}\u{03B9}\u{03AC} \u{03C3}\u{03BF}\u{03C5}",
            )
            .unwrap();
            flow.paragraph_in(
                unicode,
                12.0,
                "Plain ASCII stays on the simple (WinAnsi) font path and shares the same document.",
            )
            .unwrap();
        }
    }

    let path = out("cid_test_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
