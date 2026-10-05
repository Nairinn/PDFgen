//! The flow layout: word wrapping with real font metrics, automatic page
//! breaks, keep-with-next headings, and paragraphs that continue across
//! pages as one structure element (MCR pieces on each page).

use crate::document::Document;
use crate::page::Block;
use pdfgen_font::FontError;

/// Line height multiplier.
const LH: f64 = 1.45;

/// Flowing document builder: appends blocks, wrapping them to the page
/// width and starting new pages as needed.
pub struct Flow<'a> {
    doc: &'a mut Document,
    page_idx: usize,
    y: f64,
    margin: f64,
    width: f64,
    height: f64,
}

impl<'a> Flow<'a> {
    /// Start (or resume) flow on the document's pages.
    pub(crate) fn new(doc: &'a mut Document) -> Self {
        let (w, h) = doc.page_size;
        let margin = 72.0;
        if doc.pages.is_empty() {
            doc.pages.push(Default::default());
        }
        let page_idx = doc.pages.len() - 1;
        let y = if doc.pages[page_idx].blocks.is_empty() {
            h - margin
        } else {
            // Resuming: continue below whatever was placed on the page.
            let used = doc.pages[page_idx].blocks.len() as f64;
            h - margin - used * 24.0
        };
        Flow {
            doc,
            page_idx,
            y,
            margin,
            width: w,
            height: h,
        }
    }

    fn new_page(&mut self) {
        self.doc.pages.push(Default::default());
        self.page_idx = self.doc.pages.len() - 1;
        self.y = self.height - self.margin;
    }

    /// Break the page if `needed` points of vertical space are not left.
    fn ensure_space(&mut self, needed: f64) {
        if self.y - self.margin < needed {
            self.new_page();
        }
    }

    /// Wrap text to the content width with the given font metrics.
    fn wrap(&self, text: &str, font: usize, size: f64) -> Result<Vec<String>, FontError> {
        let f = &self.doc.fonts[font];
        let max_w = self.width - 2.0 * self.margin;
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        for word in text.split(' ') {
            let candidate = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if cur.is_empty() || f.text_width_pt(&candidate, size)? <= max_w {
                cur = candidate;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        Ok(lines)
    }

    /// Draw wrapped lines inside one marked-content sequence on the
    /// current page. Returns the MCID and the y after the last line.
    fn draw_lines(
        &mut self,
        tag: &str,
        font: usize,
        size: f64,
        lines: &[String],
    ) -> Result<u32, FontError> {
        let lh = size * LH;
        let pd = &mut self.doc.pages[self.page_idx];
        let mcid = pd.content.begin_tag(tag);
        let mut y = self.y;
        for line in lines {
            let encoded = pdfgen_font::winansi::encode(line)?;
            pd.content
                .text(&format!("F{font}"), size, self.margin, y, &encoded);
            y -= lh;
        }
        pd.content.end_tag();
        self.y = y;
        Ok(mcid)
    }

    /// Add a tagged heading. Headings are kept with at least one following
    /// body line when possible.
    pub fn heading(&mut self, level: u8, text: &str) -> Result<(), FontError> {
        debug_assert!((1..=6).contains(&level), "heading levels are 1-6");
        let size = match level {
            1 => 18.0,
            2 => 15.0,
            3 => 13.0,
            _ => 11.0,
        };
        let lines = self.wrap(text, 0, size)?;
        // Keep with next: the heading block plus one body line.
        let needed = size * LH * lines.len() as f64 + 11.0 * LH;
        self.ensure_space(needed);
        let mcid = self.draw_lines(&format!("H{level}"), 0, size, &lines)?;
        self.doc.pages[self.page_idx].blocks.push(Block {
            tag: format!("H{level}"),
            font: 0,
            size,
            pieces: vec![(self.page_idx, mcid)],
            text: text.to_string(),
        });
        Ok(())
    }

    /// Add a tagged paragraph, wrapping to the page width and continuing
    /// across page breaks. A paragraph split across pages is ONE structure
    /// element whose K references marked content on each page.
    pub fn paragraph(&mut self, text: &str) -> Result<(), FontError> {
        self.paragraph_in(0, 11.0, text)
    }

    /// Paragraph with explicit font and size.
    pub fn paragraph_in(&mut self, font: usize, size: f64, text: &str) -> Result<(), FontError> {
        let lines = self.wrap(text, font, size)?;
        let lh = size * LH;
        let mut pieces: Vec<(usize, u32)> = Vec::new();
        let mut i = 0usize;
        while i < lines.len() {
            // How many lines fit between here and the bottom margin?
            let fit = (((self.y - self.margin) / lh).floor().max(0.0)) as usize;
            if fit == 0 {
                self.new_page();
                continue;
            }
            let take = fit.min(lines.len() - i);
            let chunk: Vec<String> = lines[i..i + take].to_vec();
            let mcid = self.draw_lines("P", font, size, &chunk)?;
            pieces.push((self.page_idx, mcid));
            i += take;
            if i < lines.len() {
                self.new_page();
            }
        }
        self.doc.pages[self.page_idx].blocks.push(Block {
            tag: "P".into(),
            font,
            size,
            pieces,
            text: text.to_string(),
        });
        Ok(())
    }

    /// Vertical space remaining on the current page.
    pub fn remaining(&self) -> f64 {
        self.y - self.margin
    }
}
