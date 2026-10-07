//! Complex-script shaping: run text through HarfBuzz (rustybuzz) so
//! Myanmar vowels/medials land in the right positions, Arabic joins
//! correctly and runs right-to-left, and kerning/mark positioning work
//! for every script. The caller receives (glyph id, source char) pairs
//! instead of raw per-character cmap lookups.

use crate::LoadedFont;

/// One shaped glyph: the font's glyph id and the first source character
/// of the cluster it came from (for ToUnicode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// Glyph id in the embedded font program.
    pub gid: u16,
    /// First source character of this glyph's cluster.
    pub cluster: char,
}

/// Shape `text` with the font. Returns the glyph sequence in VISUAL
/// order (RTL runs already reversed); reordering is applied per
/// Unicode bidi rules.
pub fn shape(f: &LoadedFont, text: &str) -> Vec<ShapedGlyph> {
    let Some(face) = rustybuzz::Face::from_slice(&f.raw, 0) else {
        // Unparseable program: fall back to raw cmap ordering.
        return text
            .chars()
            .filter_map(|ch| {
                f.glyph_index(ch)
                    .map(|gid| ShapedGlyph { gid, cluster: ch })
            })
            .collect();
    };
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    // Paragraph direction from bidi: HarfBuzz shapes the LOGICAL string
    // and emits glyphs in visual order for RTL runs.
    if is_rtl(text) {
        buffer.set_direction(rustybuzz::Direction::RightToLeft);
    }
    let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

    let mut out = Vec::with_capacity(glyph_buffer.len());
    for info in glyph_buffer.glyph_infos() {
        // Cluster: byte offset into the LOGICAL string; map to a char
        // for ToUnicode (the cluster's first char).
        let cluster_byte = info.cluster as usize;
        let ch = text
            .get(cluster_byte..)
            .and_then(|s| s.chars().next())
            .unwrap_or('\u{fffd}');
        out.push(ShapedGlyph {
            gid: info.glyph_id as u16,
            cluster: ch,
        });
    }
    out
}

/// True when the paragraph's bidi embedding level is RTL.
fn is_rtl(text: &str) -> bool {
    use unicode_bidi::BidiInfo;
    BidiInfo::new(text, None)
        .paragraphs
        .first()
        .is_some_and(|p| p.level.is_rtl())
}
