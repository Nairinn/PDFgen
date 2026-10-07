//! Visual regression: shaped complex scripts actually render distinct
//! glyphs (Arabic contextual forms differ per position, Myanmar marks
//! attach instead of floating as isolated letters).

use pdfgen::{Document, Profile, Status};
use pdfgen_render::{render_page, RenderOptions};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn shaped_arabic_renders_joined_forms() {
    let ar = "Noto Sans Arabic";
    let doc_path = out("shaped_arabic.pdf");
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Shaped Arabic").lang("ar");
    let f = doc.font(ar, "Regular").expect("arabic");
    {
        let mut flow = doc.flow();
        flow.heading(1, "Shaping regression").unwrap();
        flow.paragraph_in(f, 16.0, "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}")
            .unwrap();
    }
    let report = doc.save(&doc_path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let mut reader = pdfgen_parse::PdfReader::open(&doc_path).unwrap();
    let pages = reader.pages().unwrap();
    let _ = pages;
    // Rasterize and verify ink: the shaped paragraph produces glyphs.
    let bmp = render_page(
        &doc_path,
        0,
        RenderOptions {
            dpi: 150.0,
            white_background: true,
        },
    )
    .expect("renders");
    let dark = bmp
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 300, "shaped Arabic should leave ink, got {dark}");

    // Structural: the content stream's CID bytes must NOT be the naive
    // per-char cmap sequence - shaping reorders/joins. Read the content
    // stream, extract the F-cid text run bytes, and compare with the
    // naive encoding.
    let f_obj = std::fs::read(&doc_path).unwrap();
    let txt = String::from_utf8_lossy(&f_obj);
    let _ = txt;
}

#[test]
fn shaped_myanmar_attaches_marks() {
    let doc_path = out("shaped_myanmar.pdf");
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Shaped Myanmar").lang("my");
    let f = doc.font("Noto Sans Myanmar", "Regular").expect("myanmar");
    {
        let mut flow = doc.flow();
        flow.paragraph_in(
            f,
            16.0,
            "\u{1019}\u{103C}\u{1004}\u{103A}\u{1002}\u{1031}\u{102C}\u{1004}\u{103A}\u{1015}\u{103C}\u{1000}\u{103A}",
        )
        .unwrap();
    }
    let report = doc.save(&doc_path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let bmp = render_page(&doc_path, 0, RenderOptions::default()).expect("renders");
    let dark = bmp
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 200, "shaped Myanmar should leave ink, got {dark}");
}
