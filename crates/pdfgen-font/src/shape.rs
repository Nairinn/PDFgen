//! Complex-script shaping: run text through HarfBuzz (rustybuzz) so
//! Myanmar vowels/medials land in the right positions, Arabic joins
//! correctly and runs right-to-left, and kerning/mark positioning work
//! for every script.
//!
//! Each glyph carries its cluster's byte range in the LOGICAL source
//! string, so callers can emit `/ActualText` spans with the original
//! text next to the (reordered, ligated) visual glyphs.

use crate::LoadedFont;

/// One shaped glyph: the font's glyph id plus the byte range of the
/// cluster it belongs to in the logical source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// Glyph id in the embedded font program.
    pub gid: u16,
    /// Byte offset of this glyph's cluster in the logical source.
    pub cluster: u32,
}

/// A shaped run: glyphs in visual order, each tagged with its logical
/// cluster range.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// Shaped glyphs (visual order).
    pub glyphs: Vec<ShapedGlyph>,
}

impl ShapedRun {
    /// True when the glyphs do not map 1:1 onto the source characters:
    /// ligatures, reordering, split clusters, or dropped characters.
    /// Callers use this to decide whether an `/ActualText` span is
    /// needed for correct extraction.
    #[must_use]
    pub fn needs_actual_text(&self, text: &str) -> bool {
        // Any RTL run always needs it (visual order differs from
        // logical). Any cluster spanning multiple chars needs it.
        if is_rtl(text) {
            return true;
        }
        // Clusters out of monotonic logical order (reordering).
        let mut prev: Option<u32> = None;
        for g in &self.glyphs {
            if let Some(p) = prev {
                if g.cluster < p {
                    return true;
                }
            }
            prev = Some(g.cluster);
        }
        // Cluster byte ranges covering more than one char: approximate
        // by comparing glyph count to non-space char count is wrong for
        // ligatures that DROP glyphs; instead check whether any two
        // adjacent glyphs share a cluster, or a cluster's byte span
        // (to the next distinct cluster start) covers > 1 char.
        let bytes = text.as_bytes();
        let mut starts: Vec<u32> = self.glyphs.iter().map(|g| g.cluster).collect();
        starts.sort_unstable();
        starts.dedup();
        for w in starts.windows(2) {
            let a = w[0] as usize;
            let b = w[1] as usize;
            if text.get(a..b).is_some_and(|s| s.chars().count() > 1) {
                return true;
            }
        }
        let _ = bytes;
        // Fewer distinct clusters than chars also means merging.
        let last = self.glyphs.iter().map(|g| g.cluster).max().unwrap_or(0) as usize;
        if let Some(tail) = text.get(last..) {
            // Chars after the last cluster start that the run does not
            // cover 1:1.
            if tail.chars().count() > 1 && self.glyphs.len() < text.chars().count() {
                return true;
            }
        }
        false
    }
}

/// Shape `text` with the font. Glyphs come back in visual order (RTL
/// runs reversed by HarfBuzz); each carries its logical cluster start.
#[must_use]
pub fn shape(f: &LoadedFont, text: &str) -> ShapedRun {
    let Some(face) = rustybuzz::Face::from_slice(&f.raw, 0) else {
        // Unparseable program: fall back to raw cmap ordering.
        return ShapedRun {
            glyphs: text
                .char_indices()
                .filter_map(|(i, ch)| {
                    f.glyph_index(ch).map(|gid| ShapedGlyph {
                        gid,
                        cluster: i as u32,
                    })
                })
                .collect(),
        };
    };
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    // Paragraph direction from bidi: HarfBuzz shapes the LOGICAL string
    // and emits glyphs in visual order for RTL runs.
    if is_rtl(text) {
        buffer.set_direction(rustybuzz::Direction::RightToLeft);
    }
    let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

    let mut glyphs = Vec::with_capacity(glyph_buffer.len());
    for info in glyph_buffer.glyph_infos() {
        glyphs.push(ShapedGlyph {
            gid: info.glyph_id as u16,
            cluster: info.cluster,
        });
    }
    ShapedRun { glyphs }
}

/// True when the paragraph's bidi embedding level is RTL.
fn is_rtl(text: &str) -> bool {
    use unicode_bidi::BidiInfo;
    BidiInfo::new(text, None)
        .paragraphs
        .first()
        .is_some_and(|p| p.level.is_rtl())
}
