//! ToUnicode CMap generation for embedded fonts.

/// Build a ToUnicode CMap stream mapping every mapped WinAnsi byte to its
/// Unicode scalar value (UTF-16BE hex).
pub fn build_winansi() -> Vec<u8> {
    let mut pairs: Vec<(u8, u16)> = Vec::new();
    for b in 0u8..=255 {
        if let Some(unit) = pdfgen_font::winansi_unit_for_byte(b) {
            pairs.push((b, unit));
        }
    }

    let mut s = String::new();
    s.push_str("/CIDInit /ProcSet findresource begin\n");
    s.push_str("12 dict begin\n");
    s.push_str("begincmap\n");
    s.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    s.push_str("/CMapName /Adobe-Identity-UCS def\n");
    s.push_str("/CMapType 2 def\n");
    s.push_str("1 begincodespacerange\n");
    s.push_str("<00> <FF>\n");
    s.push_str("endcodespacerange\n");

    // bfchar entries in chunks of at most 100.
    for chunk in pairs.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (b, unit) in chunk {
            s.push_str(&format!("<{b:02X}> <{unit:04X}>\n"));
        }
        s.push_str("endbfchar\n");
    }

    s.push_str("endcmap\n");
    s.push_str("CMapName currentdict /CMap defineresource pop\n");
    s.push_str("end\n");
    s.push_str("end\n");
    s.into_bytes()
}
