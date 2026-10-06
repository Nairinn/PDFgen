//! Robustness: truncated and malicious font bytes must return None/Err,
//! never panic. Property-style: deterministic truncations and mutations
//! of a real font, plus a small PRNG fuzz loop.

use pdfgen_font::{subset, LoadedFont};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
);

/// Every truncation of the font must not panic (parse or subset).
#[test]
fn truncated_fonts_never_panic() {
    let data = std::fs::read(FONT).expect("fixture font");
    // Truncate at a spread of offsets, including table-boundary areas.
    for cut in [
        0usize,
        4,
        12,
        50,
        100,
        284,
        700,
        4000,
        20_000,
        data.len() - 1,
    ] {
        let truncated = &data[..cut.min(data.len())];
        // Parsing may fail - that is fine; it must not panic.
        let _ = LoadedFont::from_bytes(truncated.to_vec());
        let _ = subset::subset_true_type(truncated, &[4, 29, 47]);
        let _ = subset::subset_winansi(truncated, &[None::<u16>; 256]);
        let _ = pdfgen_font::ttc::extract_face(truncated, 0);
    }
}

/// Deterministic mutations (xor at fixed offsets) must not panic.
#[test]
fn mutated_fonts_never_panic() {
    let data = std::fs::read(FONT).expect("fixture font");
    let mut mutated = data.clone();
    // Corrupt the table count (offset 4) and a table record.
    for probe in [
        (4usize, 0xFFu8),
        (5, 0x7F),
        (30, 0xAB),
        (622, 0x99),
        (12, 0x10),
    ] {
        mutated[probe.0] ^= probe.1;
        let _ = LoadedFont::from_bytes(mutated.clone());
        let _ = subset::subset_true_type(&mutated, &[4, 29]);
        let _ = pdfgen_font::ttc::extract_face(&mutated, 0);
        mutated = data.clone();
    }
}

/// Small xorshift fuzz: a few hundred pseudo-random corruptions.
#[test]
fn fuzz_corrupted_fonts_never_panic() {
    let data = std::fs::read(FONT).expect("fixture font");
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..300 {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);

        let mut buf = data.clone();
        // Flip 1-4 bytes at pseudo-random positions.
        let flips = (r % 4 + 1) as usize;
        for k in 0..flips {
            let pos = ((r >> (8 * k)) as usize) % buf.len();
            buf[pos] ^= (r >> 32) as u8;
        }
        let _ = LoadedFont::from_bytes(buf.clone());
        let _ = subset::subset_true_type(&buf, &[4, 29, 47]);
        let _ = subset::subset_winansi(&buf, &[None::<u16>; 256]);
        let _ = pdfgen_font::ttc::extract_face(&buf, (r % 3) as u32);
    }
}
