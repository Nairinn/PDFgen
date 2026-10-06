//! ASME Y14.1 sheet sizes must match the standard exactly.

use pdfgen_draw::Sheet;

#[test]
fn asme_sheet_sizes_match_y14_1() {
    const IN: f64 = 72.0;
    assert_eq!(Sheet::A.points(), (11.0 * IN, 8.5 * IN));
    assert_eq!(Sheet::B.points(), (17.0 * IN, 11.0 * IN));
    assert_eq!(Sheet::C.points(), (22.0 * IN, 17.0 * IN));
    assert_eq!(Sheet::D.points(), (34.0 * IN, 22.0 * IN));
    assert_eq!(Sheet::E.points(), (44.0 * IN, 34.0 * IN));
    assert_eq!(Sheet::F.points(), (40.0 * IN, 28.0 * IN));
}
