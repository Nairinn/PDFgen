//! CJK (CFF/OTF) fonts embed via FontFile3 and stay compliant: Japanese
//! hiragana/katakana/kanji through the CID path.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn cjk_cff_font_passes() {
    // The 16 MB CJK faces are an optional download (scripts/fetch-cjk-fonts.sh);
    // skip on machines that have not fetched them.
    let jp_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/noto/NotoSansCJKjp-Regular.otf"
    );
    if !std::path::Path::new(jp_path).exists() {
        eprintln!("skipping: CJK fonts not fetched (run scripts/fetch-cjk-fonts.sh)");
        return;
    }
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("CJK font test").lang("ja");

    let latin = doc.font("Noto Sans", "Regular").expect("latin");
    let jp = doc.font("Noto Sans CJK JP", "Regular").expect("cjk");

    {
        let mut flow = doc.flow();
        flow.heading(1, "CJK Embedding Test").unwrap();
        // "こんにちは、PDF" (konnichiwa, PDF)
        flow.paragraph_in(
            jp,
            12.0,
            "\u{3053}\u{3093}\u{306B}\u{3061}\u{306F}\u{3001}PDF",
        )
        .unwrap();
        // Kanji: "日本語の埋め込みテスト" (embedded Japanese test)
        flow.paragraph_in(
            jp,
            12.0,
            "\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{57CB}\u{3081}\u{8FBC}\u{307F}\u{30C6}\u{30B9}\u{30C8}",
        )
        .unwrap();
        flow.paragraph_in(latin, 11.0, "CFF outlines embed as FontFile3 (OpenType).")
            .unwrap();
    }

    let path = out("cjk_cff_ua1.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
}
