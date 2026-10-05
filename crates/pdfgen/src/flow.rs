//! Flowing content: text, lists, tables, figures, with wrapping and page
//! breaks. Everything lands in a nested structure tree.

use crate::image::{Image, ImageKind};
use crate::structure::Node;
use pdfgen_font::FontError;

/// Line height multiplier.
const LH: f64 = 1.45;
/// Table cell padding in points.
const PAD: f64 = 4.0;

/// Flowing document builder.
pub struct Flow<'a> {
    doc: &'a mut crate::document::Document,
    page_idx: usize,
    y: f64,
    margin: f64,
    width: f64,
    height: f64,
}

impl<'a> Flow<'a> {
    /// Start (or resume) flow on the document's pages.
    pub(crate) fn new(doc: &'a mut crate::document::Document) -> Self {
        let (w, h) = doc.page_size;
        let margin = 72.0;
        if doc.pages.is_empty() {
            doc.pages.push(Default::default());
        }
        // Defaults are accessible: if no font was ever loaded, resolve
        // "Liberation Sans Regular" now so bare documents just work.
        if doc.fonts.is_empty() {
            let _ = doc.font("Liberation Sans", "Regular");
        }
        let page_idx = doc.pages.len() - 1;
        let y = if doc.pages[page_idx].nodes.is_empty() {
            h - margin
        } else {
            // Resume below placed content.
            h - margin - 24.0
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

    fn ensure_space(&mut self, needed: f64) {
        if self.y - self.margin < needed {
            self.new_page();
        }
    }

    /// Wrap text to `max_w` with the given font metrics.
    fn wrap_to(
        &self,
        text: &str,
        font: usize,
        size: f64,
        max_w: f64,
    ) -> Result<Vec<String>, FontError> {
        let Some(f) = self.doc.fonts.get(font) else {
            // No font loaded (or bad handle): the caller relied on a
            // default that does not exist. Fail with a clear message
            // rather than panic.
            return Err(FontError::MissingGlyph('?', 0));
        };
        let mut lines = Vec::new();
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

    fn wrap(&self, text: &str, font: usize, size: f64) -> Result<Vec<String>, FontError> {
        self.wrap_to(text, font, size, self.width - 2.0 * self.margin)
    }

    /// Draw wrapped lines inside one marked-content sequence. Returns the
    /// MCID and the y after the last line. Does NOT advance self.y.
    fn draw_lines(
        &mut self,
        tag: &str,
        font: usize,
        size: f64,
        lines: &[String],
        x: f64,
    ) -> Result<(u32, f64), FontError> {
        let lh = size * LH;
        let pd = &mut self.doc.pages[self.page_idx];
        let mcid = pd.content.begin_tag(tag);
        let mut y = self.y;
        for line in lines {
            let encoded = pdfgen_font::winansi::encode(line)?;
            pd.content.text(&format!("F{font}"), size, x, y, &encoded);
            y -= lh;
        }
        pd.content.end_tag();
        Ok((mcid, y))
    }

    /// Vertical space remaining on the current page.
    pub fn remaining(&self) -> f64 {
        self.y - self.margin
    }

    // ------------------------------------------------------------- text

    /// Tagged heading (levels 1-6), kept with the next body line.
    pub fn heading(&mut self, level: u8, text: &str) -> Result<(), FontError> {
        debug_assert!((1..=6).contains(&level), "heading levels are 1-6");
        let size = match level {
            1 => 18.0,
            2 => 15.0,
            3 => 13.0,
            _ => 11.0,
        };
        let lines = self.wrap(text, 0, size)?;
        let needed = size * LH * lines.len() as f64 + 11.0 * LH;
        self.ensure_space(needed);
        let (mcid, y) = self.draw_lines(&format!("H{level}"), 0, size, &lines, self.margin)?;
        self.y = y;
        self.doc.pages[self.page_idx].nodes.push(
            Node::leaf(format!("H{level}"), text.to_string(), 0, size)
                .with_pieces(vec![(self.page_idx, mcid)]),
        );
        Ok(())
    }

    /// Tagged paragraph, wrapping and continuing across page breaks.
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
            let fit = (((self.y - self.margin) / lh).floor().max(0.0)) as usize;
            if fit == 0 {
                self.new_page();
                continue;
            }
            let take = fit.min(lines.len() - i);
            let chunk: Vec<String> = lines[i..i + take].to_vec();
            let (mcid, y) = self.draw_lines("P", font, size, &chunk, self.margin)?;
            self.y = y;
            pieces.push((self.page_idx, mcid));
            i += take;
            if i < lines.len() {
                self.new_page();
            }
        }
        self.doc.pages[self.page_idx].nodes.push(
            Node::leaf("P", text.to_string(), font, size).with_pieces(pieces),
        );
        Ok(())
    }

    // ----------------------------------------------------------- lists

    /// Tagged bullet list: `L > LI > (Lbl, LBody)`.
    pub fn bullet_list(&mut self, items: &[&str]) -> Result<(), FontError> {
        let mut list = Node::group("L");
        for item in items {
            self.ensure_space(11.0 * LH);
            let (lbl_mcid, y1) =
                self.draw_lines("Lbl", 0, 11.0, &["\u{2022}".to_string()], self.margin)?;
            let lines = self.wrap_to(item, 0, 11.0, self.width - 2.0 * self.margin - 14.0)?;
            let (body_mcid, y2) = self.draw_lines("LBody", 0, 11.0, &lines, self.margin + 14.0)?;
            self.y = y1.min(y2);
            list.children.push(
                Node::group("LI").with_children(vec![
                    Node::leaf("Lbl", "\u{2022}".into(), 0, 11.0)
                        .with_pieces(vec![(self.page_idx, lbl_mcid)]),
                    Node::leaf("LBody", item.to_string(), 0, 11.0)
                        .with_pieces(vec![(self.page_idx, body_mcid)]),
                ]),
            );
        }
        self.doc.pages[self.page_idx].nodes.push(list);
        Ok(())
    }

    // ---------------------------------------------------------- tables

    /// Tagged table. First row is the header row (`TH` cells, Scope
    /// `Column`). Cells wrap; rows are kept together across pages.
    pub fn table(
        &mut self,
        header: &[&str],
        rows: &[Vec<&str>],
        widths: &[f64],
    ) -> Result<(), FontError> {
        let n_cols = header.len().max(rows.first().map(Vec::len).unwrap_or(0));
        if n_cols == 0 {
            return Ok(());
        }
        let avail = self.width - 2.0 * self.margin;
        let col_w: Vec<f64> = if widths.len() == n_cols {
            widths.to_vec()
        } else {
            vec![avail / n_cols as f64; n_cols]
        };

        let mut table = Node::group("Table");

        // --- header row (TH cells)
        let row_h = 11.0 * LH;
        self.ensure_space(row_h);
        let mut hrow = Node::group("TR");
        let mut x = self.margin;
        {
            // Pre-wrap all header cells before borrowing the page data.
            let header_lines: Vec<Vec<String>> = header
                .iter()
                .enumerate()
                .map(|(ci, cell)| {
                    let w = col_w[ci.min(col_w.len() - 1)];
                    self.wrap_to(cell, 0, 11.0, w - 2.0 * PAD)
                })
                .collect::<Result<_, _>>()?;
            let pd = &mut self.doc.pages[self.page_idx];
            for (ci, (cell, lines)) in header.iter().zip(header_lines.iter()).enumerate() {
                let w = col_w[ci.min(col_w.len() - 1)];
                let mcid = pd.content.begin_tag("TH");
                let mut y = self.y;
                for line in lines {
                    let encoded = pdfgen_font::winansi::encode(line)?;
                    pd.content.text("F0", 11.0, x + PAD, y - 13.0, &encoded);
                    y -= 11.0 * LH;
                }
                pd.content.end_tag();
                hrow.children.push(
                    Node::leaf("TH", cell.to_string(), 0, 11.0)
                        .with_pieces(vec![(self.page_idx, mcid)])
                        .with_scope("Column"),
                );
                x += w;
            }
        }
        self.y -= row_h;
        table.children.push(hrow);

        // --- body rows
        for row in rows {
            let wrapped: Vec<Vec<String>> = row
                .iter()
                .enumerate()
                .map(|(ci, c)| {
                    let w = col_w[ci.min(col_w.len() - 1)];
                    self.wrap_to(c, 0, 11.0, w - 2.0 * PAD)
                })
                .collect::<Result<_, _>>()?;
            let row_h =
                11.0 * LH * wrapped.iter().map(Vec::len).max().unwrap_or(1) as f64;
            self.ensure_space(row_h);
            let mut tr = Node::group("TR");
            let mut x = self.margin;
            let mut y_top = self.y;
            for (ci, lines) in wrapped.iter().enumerate() {
                let cell_text = row.get(ci).map(|s| s.to_string()).unwrap_or_default();
                let pd = &mut self.doc.pages[self.page_idx];
                let mcid = pd.content.begin_tag("TD");
                let mut y = self.y;
                for line in lines {
                    let encoded = pdfgen_font::winansi::encode(line)?;
                    pd.content.text("F0", 11.0, x + PAD, y - 13.0, &encoded);
                    y -= 11.0 * LH;
                }
                pd.content.end_tag();
                tr.children.push(
                    Node::leaf("TD", cell_text, 0, 11.0)
                        .with_pieces(vec![(self.page_idx, mcid)]),
                );
                x += col_w[ci.min(col_w.len() - 1)];
                y_top = y_top.min(y);
            }
            self.y = y_top;
            table.children.push(tr);
        }

        self.doc.pages[self.page_idx].nodes.push(table);
        Ok(())
    }

    // --------------------------------------------------------- figures

    /// Tagged figure: image with alt text (empty alt = decorative).
    pub fn figure(
        &mut self,
        image: &Image,
        alt: &str,
        w: f64,
        h: f64,
    ) -> Result<(), FontError> {
        self.ensure_space(h);
        let res = self.doc.register_image(image);
        let pd = &mut self.doc.pages[self.page_idx];
        let mcid = pd.content.begin_tag("Figure");
        pd.content.image(&res, self.margin, self.y - h, w, h);
        pd.content.end_tag();
        self.y -= h;
        let mut fig =
            Node::leaf("Figure", String::new(), 0, 0.0).with_pieces(vec![(self.page_idx, mcid)]);
        if !alt.is_empty() {
            fig.alt = Some(alt.to_string());
        }
        self.doc.pages[self.page_idx].nodes.push(fig);
        Ok(())
    }

    /// Page header furniture, drawn as a pagination artifact.
    pub fn header(&mut self, text: &str) -> Result<(), FontError> {
        let encoded = pdfgen_font::winansi::encode(text)?;
        let pd = &mut self.doc.pages[self.page_idx];
        pd.content.begin_artifact("Header");
        pd.content.text("F0", 9.0, self.margin, self.height - 40.0, &encoded);
        pd.content.end_artifact();
        Ok(())
    }

    /// Page footer furniture, drawn as a pagination artifact.
    pub fn footer(&mut self, text: &str) -> Result<(), FontError> {
        let encoded = pdfgen_font::winansi::encode(text)?;
        let pd = &mut self.doc.pages[self.page_idx];
        pd.content.begin_artifact("Footer");
        pd.content.text("F0", 9.0, self.margin, 36.0, &encoded);
        pd.content.end_artifact();
        Ok(())
    }
}

/// Helper for the header-cell text draw (draws each wrapped line).
fn encoded_if(_lines: &[String], _line: &String, encoded: Vec<u8>) -> Vec<u8> {
    encoded
}

/// Re-export for callers constructing images.
pub use crate::image::Image as FlowImage;
#[allow(unused_imports)]
use ImageKind as _FlowImageKind;
