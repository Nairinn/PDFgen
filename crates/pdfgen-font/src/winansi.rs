//! WinAnsiEncoding: byte code to Unicode scalar mapping and text encoding.

use crate::FontError;

/// Unicode scalar for a WinAnsi byte, if one is assigned.
#[must_use] 
pub fn unit_for_byte(b: u8) -> Option<u16> {
    match b {
        0x20..=0x7e => Some(u16::from(b)),
        0xa0..=0xff => Some(u16::from(b)),
        // The Windows-1252 specials in the 0x80..0x9F block.
        0x80 => Some(0x20AC), // EURO SIGN
        0x82 => Some(0x201A), // SINGLE LOW-9 QUOTATION MARK
        0x83 => Some(0x0192), // LATIN SMALL LETTER F WITH HOOK
        0x84 => Some(0x201E), // DOUBLE LOW-9 QUOTATION MARK
        0x85 => Some(0x2026), // HORIZONTAL ELLIPSIS
        0x86 => Some(0x2020), // DAGGER
        0x87 => Some(0x2021), // DOUBLE DAGGER
        0x88 => Some(0x02C6), // MODIFIER LETTER CIRCUMFLEX ACCENT
        0x89 => Some(0x2030), // PER MILLE SIGN
        0x8a => Some(0x0160), // LATIN CAPITAL LETTER S WITH CARON
        0x8b => Some(0x2039), // SINGLE LEFT-POINTING ANGLE QUOTATION MARK
        0x8c => Some(0x0152), // LATIN CAPITAL LIGATURE OE
        0x8e => Some(0x017D), // LATIN CAPITAL LETTER Z WITH CARON
        0x91 => Some(0x2018), // LEFT SINGLE QUOTATION MARK
        0x92 => Some(0x2019), // RIGHT SINGLE QUOTATION MARK
        0x93 => Some(0x201C), // LEFT DOUBLE QUOTATION MARK
        0x94 => Some(0x201D), // RIGHT DOUBLE QUOTATION MARK
        0x95 => Some(0x2022), // BULLET
        0x96 => Some(0x2013), // EN DASH
        0x97 => Some(0x2014), // EM DASH
        0x98 => Some(0x02DC), // SMALL TILDE
        0x99 => Some(0x2122), // TRADE MARK SIGN
        0x9a => Some(0x0161), // LATIN SMALL LETTER S WITH CARON
        0x9b => Some(0x203A), // SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
        0x9c => Some(0x0153), // LATIN SMALL LIGATURE OE
        0x9e => Some(0x017E), // LATIN SMALL LETTER Z WITH CARON
        0x9f => Some(0x0178), // LATIN CAPITAL LETTER Y WITH DIAERESIS
        _ => None,
    }
}

/// Encode a string as WinAnsi bytes. Fails on characters outside the encoding.
pub fn encode(text: &str) -> Result<Vec<u8>, FontError> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        out.push(encode_char(ch)?);
    }
    Ok(out)
}

/// Encode one character to its WinAnsi byte.
pub fn encode_char(ch: char) -> Result<u8, FontError> {
    let unit = u32::from(ch);
    // Fast path: ASCII and Latin-1 are identity.
    if (0x20..=0x7e).contains(&unit) || (0xa0..=0xff).contains(&unit) {
        return Ok(unit as u8);
    }
    // The specials, reverse-mapped.
    for b in 0x80..=0x9f {
        if let Some(u) = unit_for_byte(b) {
            if u32::from(u) == unit {
                return Ok(b);
            }
        }
    }
    Err(FontError::MissingGlyph(ch, unit))
}
