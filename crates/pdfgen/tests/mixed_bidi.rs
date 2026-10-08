//! Mixed-direction lines: Arabic with digits (both covered by the Arabic
//! face) must extract in logical order and render with correct run
//! directions per the Unicode bidi algorithm.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn extracted(path: &str) -> String {
    let pages = pdfgen::extract_text(path).unwrap();
    let mut all = String::new();
    for p in pages {
        for b in p.blocks {
            all.push_str(&b);
        }
    }
    all
}

#[test]
fn mixed_direction_line_roundtrips() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Mixed direction").lang("ar");
    let ar = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/noto/NotoSansArabic-Regular.ttf"
        ))
        .expect("arabic");

    // RTL Arabic paragraph with an LTR digit run: "مرحبا 2026".
    let line = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} 2026";
    {
        let mut flow = doc.flow();
        flow.paragraph_in(ar, 14.0, line).unwrap();
    }
    let path = out("mixed_bidi.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );

    let all = extracted(&path);
    assert!(
        all.contains("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} 2026"),
        "logical order lost: {all:?}"
    );

    // Visual order: the digits are an LTR run inside an RTL paragraph,
    // so they render LEFTMOST. The drawn glyph sequence must start with
    // the digit CIDs (2,0,2,6 = 120,118,120,124).
    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    let i = txt.find("/Span").expect("ActualText span");
    let seg = &txt[i..];
    let j = seg.find("Td\n(").expect("text draw");
    let drawn = &seg[j + 4..];
    // CIDs are big-endian pairs, octal-escaped when outside 0x20-0x7E.
    // The first pair is '\000x': high byte 0x00 (escaped as \000), low
    // byte 'x' (0x78 = 120 = the '2' glyph). Decode the first CID.
    let first_pair: String = drawn.chars().take(6).filter(|c| *c != '\\').collect();
    let low = first_pair.chars().nth(3).unwrap_or('\0') as u8;
    let gid = u16::from(low);
    assert!(
        (118..=125).contains(&gid),
        "first drawn glyph should be a digit glyph, got gid {gid} ({first_pair:?})"
    );
}

#[test]
fn arabic_digits_arabic_roundtrips() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Sandwich").lang("ar");
    let ar = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/noto/NotoSansArabic-Regular.ttf"
        ))
        .expect("arabic");
    // Arabic - digits - Arabic sandwich, hardest case.
    let line = "\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645} 123 \u{0639}\u{0644}\u{064A}\u{0643}\u{0645}";
    {
        let mut flow = doc.flow();
        flow.paragraph_in(ar, 14.0, line).unwrap();
    }
    let path = out("mixed_sandwich.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let all = extracted(&path);
    assert!(
        all.contains("\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645} 123 \u{0639}\u{0644}\u{064A}\u{0643}\u{0645}"),
        "sandwich order lost: {all:?}"
    );
}
