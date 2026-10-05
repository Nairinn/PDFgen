//! One page under construction: tagged text blocks flowing top to bottom.

use crate::document::Document;
use pdfgen_font::FontError;

/// A single tagged text block on a page.
#[derive(Debug, Clone)]
pub struct Block {
    /// PDF structure type: `H1`..`H6`, `P`, …
    pub tag: String,
    /// Marked-content ID assigned within the page's content stream.
    pub mcid: u32,
    /// Index of the font in the document's font list.
    pub font: usize,
    /// Font size in points.
    pub size: f64,
    /// Position of the baseline.
    pub x: f64,
    pub y: f64,
    /// Original text (pre-encoding) for diagnostics.
    pub text: String,
    /// WinAnsi-encoded bytes drawn in the content stream.
    pub encoded: Vec<u8>,
}

/// Builder for one page. Blocks flow down the page in call order, which is
/// also the logical reading order.
pub struct Page<'a> {
    doc: &'a mut Document,
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Left margin (and first-line x coordinate).
    pub margin: f64,
    /// Y position of the next baseline (PDF coords: up from the bottom).
    y: f64,
}

impl<'a> Page<'a> {
    /// Create the page builder over a document.
    pub(crate) fn new(doc: &'a mut Document, w: f64, h: f64) -> Self {
        Page {
            doc,
            width: w,
            height: h,
            margin: 72.0,
            y: h - 72.0,
        }
    }

    /// Advance the cursor and return the baseline for a new block.
    fn next_baseline(&mut self, size: f64) -> f64 {
        self.y -= size * 1.6;
        self.y
    }

    /// Add a tagged heading (levels 1-6).
    pub fn heading(&mut self, level: u8, text: &str) -> Result<(), FontError> {
        debug_assert!(
            (1..=6).contains(&level),
            "heading levels are 1-6"
        );
        let size = match level {
            1 => 18.0,
            2 => 15.0,
            3 => 13.0,
            _ => 11.0,
        };
        self.text_block(&format!("H{level}"), text, 0, size)
    }

    /// Add a tagged paragraph.
    pub fn paragraph(&mut self, text: &str) -> Result<(), FontError> {
        self.text_block("P", text, 0, 11.0)
    }

    /// Add a tagged paragraph in a specific font.
    pub fn paragraph_in(&mut self, font: usize, text: &str) -> Result<(), FontError> {
        self.text_block("P", text, font, 11.0)
    }

    /// Internal: append one tagged block.
    fn text_block(
        &mut self,
        tag: &str,
        text: &str,
        font: usize,
        size: f64,
    ) -> Result<(), FontError> {
        let encoded = pdfgen_font::winansi::encode(text)?;
        let x = self.margin;
        let y = self.next_baseline(size);
        let mcid = self
            .doc
            .content
            .tagged_text(tag, &format!("F{font}"), size, x, y, &encoded);
        self.doc.blocks.push(Block {
            tag: tag.to_string(),
            mcid,
            font,
            size,
            x,
            y,
            text: text.to_string(),
            encoded,
        });
        Ok(())
    }

    /// Remaining vertical space to the bottom margin.
    pub fn remaining(&self) -> f64 {
        self.y - self.margin
    }
}
