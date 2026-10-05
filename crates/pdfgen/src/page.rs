//! Per-page data and the absolute-placement page builder.

use crate::structure::Node;
use pdfgen_font::FontError;

/// Per-page accumulated content and structure nodes.
#[derive(Debug, Default)]
pub(crate) struct PageData {
    /// The page's content stream under construction.
    pub content: pdfgen_canvas::Content,
    /// Structure nodes placed on this page (top-level; may be groups).
    pub nodes: Vec<Node>,
}

/// Builder for one explicitly placed page. Each call places exactly one
/// line (the flow API wraps); content goes down the page in call order,
/// which is also the logical reading order.
pub struct Page<'a> {
    doc: &'a mut crate::document::Document,
    idx: usize,
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
    /// Create the page builder over a document (page `idx` must exist).
    pub(crate) fn new(doc: &'a mut crate::document::Document, idx: usize, w: f64, h: f64) -> Self {
        Page {
            doc,
            idx,
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
        debug_assert!((1..=6).contains(&level), "heading levels are 1-6");
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
        let pd = &mut self.doc.pages[self.idx];
        let mcid = pd
            .content
            .tagged_text(tag, &format!("F{font}"), size, x, y, &encoded);
        pd.nodes.push(
            Node::leaf(tag, text.to_string(), font, size).with_pieces(vec![(self.idx, mcid)]),
        );
        Ok(())
    }

    /// Remaining vertical space to the bottom margin.
    pub fn remaining(&self) -> f64 {
        self.y - self.margin
    }
}
