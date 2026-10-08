//! Golden shaping tests: the exact glyph-id sequences must match what
//! HarfBuzz `hb-shape` produces for the same font and text.
//!
//! Fixtures generated with:
//!   harfbuzz 14.5.1 (homebrew), hb-shape --no-glyph-names
//!   --no-positions --no-clusters
//! rustybuzz 0.20 tracks the same shaping engine version family; if
//! rustybuzz is upgraded and these fail, regenerate with the matching
//! hb-shape and update the sequences.

use pdfgen_font::{shape::shape, LoadedFont};

const MYANMAR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
);
const ARABIC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/noto/NotoSansArabic-Regular.ttf"
);
const THAI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/noto/NotoSansThai-Regular.ttf"
);

fn assert_golden(font_path: &str, text: &str, expected: &[u16]) {
    let f = LoadedFont::load(font_path).expect("font loads");
    let shaped = shape(&f, text);
    let got: Vec<u16> = shaped.glyphs.iter().map(|g| g.gid).collect();
    assert_eq!(
        got, expected,
        "shaped gids diverge from the hb-shape golden sequence"
    );
}

#[test]
fn myanmar_mingalapa_golden() {
    // "မင်္ဂလာပါ" — hb-shape: [29|6|189|32|368|25|367]
    assert_golden(
        MYANMAR,
        "\u{1019}\u{1004}\u{103A}\u{1039}\u{1002}\u{101C}\u{102C}\u{1015}\u{102B}",
        &[29, 6, 189, 32, 368, 25, 367],
    );
}

#[test]
fn myanmar_myanma_golden() {
    // "မြန်မာ" — hb-shape: [47|29|24|381|29|368]
    assert_golden(
        MYANMAR,
        "\u{1019}\u{103C}\u{1014}\u{103A}\u{1019}\u{102C}",
        &[47, 29, 24, 381, 29, 368],
    );
}

#[test]
fn arabic_marhaba_golden() {
    // "مرحبا" — hb-shape: [9|316|16|27|31|79]
    assert_golden(
        ARABIC,
        "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}",
        &[9, 316, 16, 27, 31, 79],
    );
}

#[test]
fn arabic_lam_alef_golden() {
    // "السلام" — lam-alef ligature; hb-shape: [74|11|71|36|72|8]
    assert_golden(
        ARABIC,
        "\u{0627}\u{0644}\u{0633}\u{0644}\u{0627}\u{0645}",
        &[74, 11, 71, 36, 72, 8],
    );
}

#[test]
fn thai_sawasdee_golden() {
    // "สวัสดี" — hb-shape: [110|134|45|110|12|94]
    assert_golden(
        THAI,
        "\u{0E2A}\u{0E27}\u{0E31}\u{0E2A}\u{0E14}\u{0E35}",
        &[110, 134, 45, 110, 12, 94],
    );
}
