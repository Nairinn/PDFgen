//! CID (Type0) text encoding: map characters to font glyph IDs and emit
//! two-byte codes for Identity-H encoding.

use crate::{FontError, LoadedFont};

/// Encode text as CID codes (glyph IDs) for an Identity-H Type0 font.
/// Returns (cids, chars): the codes to write and the characters they map
/// to (for ToUnicode).
pub fn encode(text: &str, font: &LoadedFont) -> Result<(Vec<u16>, Vec<char>), FontError> {
    let mut cids = Vec::with_capacity(text.len());
    let mut chars = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let gid = font
            .glyph_index(ch)
            .ok_or(FontError::MissingGlyph(ch, u32::from(ch)))?;
        cids.push(gid);
        chars.push(ch);
    }
    Ok((cids, chars))
}
