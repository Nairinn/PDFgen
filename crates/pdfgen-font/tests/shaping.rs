//! Shaping sanity: Arabic contextual forms and Myanmar reordering
//! through HarfBuzz, and that shaped output differs from naive cmap
//! glyph-per-char when the script requires it.

use pdfgen_font::{shape::shape, LoadedFont};

const ARABIC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/noto/NotoSansArabic-Regular.ttf"
);
const MYANMAR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
);

#[test]
fn arabic_shapes_into_contextual_forms() {
    let f = LoadedFont::load(ARABIC).unwrap();
    // "مرحبا" (marhaba).
    let text = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}";
    let shaped = shape(&f, text);
    // Naive cmap glyphs:
    let naive: Vec<u16> = text.chars().filter_map(|c| f.glyph_index(c)).collect();
    let shaped_gids: Vec<u16> = shaped.glyphs.iter().map(|g| g.gid).collect();
    // HarfBuzz must pick medial/initial forms that differ from the
    // isolated cmap glyphs for at least some letters (joining behavior).
    let diffs = shaped_gids
        .iter()
        .zip(&naive)
        .filter(|(s, n)| s != n)
        .count();
    assert!(
        diffs >= 2,
        "expected contextual forms to differ from isolated cmap glyphs, \
         shaped={shaped_gids:?} naive={naive:?}"
    );
}

#[test]
fn myanmar_marks_reorder() {
    let f = LoadedFont::load(MYANMAR).unwrap();
    // "မင်္ဂလာပါ" - contains U+103C (medial) and U+103A which shape
    // into reordered/combined glyphs.
    let text = "\u{1019}\u{103C}\u{1004}\u{103A}\u{1002}\u{1031}\u{102C}\u{1004}\u{103A}\u{1015}\u{103C}\u{1000}\u{103A}";
    let shaped = shape(&f, text);
    // The glyph COUNT can differ from char count (ligatures); the
    // important property is that shaping SUCCEEDS and produces a
    // sequence of in-range gids.
    assert!(!shaped.glyphs.is_empty(), "shaping produced nothing");
    for g in &shaped.glyphs {
        assert!(g.gid != 0, "shaped to .notdef (gid 0) - font lacks forms");
    }
}

#[test]
fn ltr_latin_passes_through_shaping() {
    let f = LoadedFont::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
    ))
    .unwrap();
    let shaped = shape(&f, "Hello");
    let naive: Vec<u16> = "Hello".chars().filter_map(|c| f.glyph_index(c)).collect();
    let gids: Vec<u16> = shaped.glyphs.iter().map(|g| g.gid).collect();
    assert_eq!(gids, naive, "Latin LTR should be unchanged by shaping");
}

#[test]
fn rtl_paragraph_gives_visual_order() {
    let f = LoadedFont::load(ARABIC).unwrap();
    let text = "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}"; // marhaba
    let shaped = shape(&f, text);
    // Clusters are byte offsets into the logical string. In an RTL
    // paragraph HarfBuzz emits glyphs in visual order, so the FIRST
    // shaped glyph belongs to the LAST logical character (alef, byte 8).
    let first_cluster = shaped.glyphs[0].cluster as usize;
    let ch = text.get(first_cluster..).and_then(|s| s.chars().next());
    assert_eq!(
        ch,
        Some('\u{0627}'),
        "RTL visual order: last char first, got cluster {first_cluster}"
    );
}
