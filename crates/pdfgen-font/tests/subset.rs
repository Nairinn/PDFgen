//! Subsetter tests: round-trip raw bytes through subset_true_type and
//! check the rebuilt font parses, keeps requested glyphs, and preserves
//! their advance widths.

use pdfgen_font::subset::subset_true_type;

fn font_path() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
    )
}

#[test]
fn subset_parses_and_preserves_widths() {
    let data = std::fs::read(font_path()).unwrap();
    let face = ttf_parser::Face::parse(&data, 0).unwrap();

    // Pick the glyphs our scripts test uses.
    let mut used = Vec::new();
    for ch in ['\u{1019}', '\u{103C}', '\u{1004}', '\u{103A}', '\u{1002}'] {
        used.push(face.glyph_index(ch).unwrap().0);
    }
    let advances: Vec<u16> = used
        .iter()
        .map(|&g| face.glyph_hor_advance(ttf_parser::GlyphId(g)).unwrap())
        .collect();

    let (sub, remap) = subset_true_type(&data, &used).expect("subset builds");
    let sub_face = ttf_parser::Face::parse(&sub, 0).expect("subset parses");

    // Every requested glyph must exist in the subset with the same advance.
    for (i, (&old, &adv)) in used.iter().zip(&advances).enumerate() {
        let new_gid = remap[old as usize];
        let got = sub_face
            .glyph_hor_advance(ttf_parser::GlyphId(new_gid))
            .expect("advance exists in subset");
        assert_eq!(got, adv, "glyph {i} advance mismatch: {got} != {adv}");
        // The subset must have fewer glyphs than the original.
        assert!(
            sub_face.number_of_glyphs() < face.number_of_glyphs(),
            "subset should shrink the glyph count"
        );
    }

    // The subset must be dramatically smaller.
    assert!(
        sub.len() < data.len() / 2,
        "subset too large: {} vs {}",
        sub.len(),
        data.len()
    );
    println!(
        "subset: {} bytes from {} ({} -> {} glyphs)",
        sub.len(),
        data.len(),
        face.number_of_glyphs(),
        sub_face.number_of_glyphs()
    );
}
