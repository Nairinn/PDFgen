//! Fuzz font loading and subsetting: raw bytes -> LoadedFont ->
//! subset_true_type / subset_winansi / ttc extraction. Panics in the
//! table parsing or subsetter fire here.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(f) = pdfgen_font::LoadedFont::from_bytes(data.to_vec()) {
        let _ = f.glyph_index('a');
        let _ = f.glyph_width_units(4);
        // Both subset modes over a plausible glyph set.
        let used = [4u16, 29, 47, 100];
        let _ = pdfgen_font::subset::subset_true_type(data, &used);
        let mut bytes: [Option<u16>; 256] = [None; 256];
        bytes[0x41] = f.glyph_index('A');
        bytes[0x61] = f.glyph_index('a');
        let _ = pdfgen_font::subset::subset_winansi(data, &bytes);
    }
    let _ = pdfgen_font::ttc::extract_face(data, 0);
    let _ = pdfgen_font::ttc::extract_face(data, 1);
});
