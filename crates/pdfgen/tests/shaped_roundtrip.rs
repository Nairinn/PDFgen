//! Shaped-text extraction round trip (ActualText semantics): text drawn
//! through the shaping path must extract back as the LOGICAL source
//! string. This test is written BEFORE the fix; it must fail.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn fonts(doc: &mut Document) -> (usize, usize, usize, usize) {
    let latin = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
        ))
        .expect("latin");
    let mm = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
        ))
        .expect("myanmar");
    let ar = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/noto/NotoSansArabic-Regular.ttf"
        ))
        .expect("arabic");
    let th = doc
        .load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/noto/NotoSansThai-Regular.ttf"
        ))
        .expect("thai");
    (latin, mm, ar, th)
}

fn extracted_text(path: &str) -> String {
    let pages = pdfgen::extract_text(path).expect("extract");
    let mut joined = String::new();
    for p in pages {
        for b in p.blocks {
            joined.push_str(&b);
            joined.push('\n');
        }
    }
    joined
}

fn roundtrip(name: &str, font: usize, size: f64, text: &str) {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Shaped roundtrip").lang("en-US");
    let (latin, _, _, _) = fonts(&mut doc);
    {
        let mut flow = doc.flow();
        flow.heading(1, "Shaped roundtrip").unwrap();
        flow.paragraph_in(latin, 11.0, "latin paragraph so extraction has anchor text")
            .unwrap();
        flow.paragraph_in(font, size, text).unwrap();
    }
    let path = out(name);
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "{name} violations: {:#?}",
        report.violations
    );

    let extracted = extracted_text(&path);
    assert!(
        extracted.contains(text),
        "{name}: extraction lost the logical string.\n  expected: {text:?}\n  got: {extracted:?}"
    );
}

#[test]
fn myanmar_roundtrips() {
    let mut doc = Document::new(Profile::PdfUa1);
    let (latin, mm, _, _) = fonts(&mut doc);
    // "မင်္ဂလာပါ" (hello) and "မြန်မာ" (myanmar) exercise medials,
    // anusvara and stacked consonants.
    roundtrip(
        "rt_myanmar_1.pdf",
        mm,
        14.0,
        "\u{1000}\u{1004}\u{103A}\u{1002}\u{100B}\u{102C}\u{1015}\u{103C}\u{1000}",
    );
    let _ = latin;
    roundtrip(
        "rt_myanmar_2.pdf",
        mm,
        14.0,
        "\u{1019}\u{103C}\u{1014}\u{103A}\u{1019}\u{103C}\u{1000}",
    );
}

#[test]
fn arabic_roundtrips() {
    let mut doc = Document::new(Profile::PdfUa1);
    let (_, _, ar, _) = fonts(&mut doc);
    let _ = doc;
    // "مرحبا" (hello)
    roundtrip(
        "rt_arabic_1.pdf",
        ar,
        14.0,
        "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}",
    );
    // "السلام" with lam-alef ligature
    roundtrip(
        "rt_arabic_2.pdf",
        ar,
        14.0,
        "\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645}",
    );
}

#[test]
fn thai_roundtrips() {
    let mut doc = Document::new(Profile::PdfUa1);
    let (_, _, _, th) = fonts(&mut doc);
    let _ = doc;
    // "สวัสดี" (hello) — pre-base vowels and tone marks reorder.
    roundtrip(
        "rt_thai_1.pdf",
        th,
        14.0,
        "\u{0E2A}\u{0E27}\u{0E31}\u{0E2A}\u{0E14}\u{0E35}",
    );
}

#[test]
fn latin_unchanged() {
    // Plain Latin must keep extracting exactly as before.
    roundtrip("rt_latin.pdf", 0, 12.0, "Hello shaped world");
}
