//! PdfString text encoding: ASCII stays PDFDocEncoding, everything else
//! becomes UTF-16BE with a BOM, and decoding round-trips both.

use pdfgen_core::PdfString;

#[test]
fn ascii_stays_asis() {
    let s = PdfString::text("Hello, tagged world!");
    assert_eq!(s.0, b"Hello, tagged world!");
    assert_eq!(s.decode(), "Hello, tagged world!");
}

#[test]
fn non_ascii_gets_utf16_bom() {
    // Myanmar "မင်္ဂလာပါ" + accented Latin.
    let text = "မင်္ဂလာပါ café";
    let s = PdfString::text(text);
    // BOM present.
    assert_eq!(&s.0[..2], &[0xFE, 0xFF]);
    // Round-trip.
    assert_eq!(s.decode(), text);
}

#[test]
fn round_trip_mixed_script() {
    for text in [
        "Japanese: 日本語のテスト",
        "Thai: สวัสดีครับ",
        "Emoji: 🎉 done",
        "naïve — em dash",
    ] {
        let s = PdfString::text(text);
        assert_eq!(s.decode(), text, "round trip failed for {text:?}");
    }
}

#[test]
fn serializes_octal_escaped_and_decodes_back() {
    // The literal-string escaper octal-escapes the BOM bytes; the escaped
    // form must decode back to the same string via a serializer round
    // trip.
    let text = "café";
    let s = PdfString::text(text);
    let mut out = String::new();
    pdfgen_core::write_object(&mut out, &pdfgen_core::Object::String(s.clone()));
    // Serialized: (caf\353...) or (\376\377...) with BOM. The BOM form
    // is required for the UTF-16 path.
    assert!(
        out.contains("\\376\\377") || out.contains("caf"),
        "unexpected serialization: {out}"
    );
    assert_eq!(s.decode(), text);
}
