//! Flowing content: text, lists, tables, figures, with wrapping and page
//! breaks. Everything lands in a nested structure tree.

use crate::image::Image;
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
    /// Start (or resume) flow on the document's pages. The cursor comes
    /// from the document when a previous flow left one, so consecutive
    /// flows continue where the last ended.
    pub(crate) fn new(doc: &'a mut crate::document::Document) -> Self {
        // Defaults are accessible: if no font was ever loaded, resolve
        // "Liberation Sans Regular" now so bare documents just work.
        if doc.fonts.is_empty() {
            let _ = doc.font("Liberation Sans", "Regular");
        }
        let page_idx = doc.pages.len() - 1;
        let (w, h) = doc.pages[page_idx].size;
        let margin = 72.0;
        // Resume from the persisted cursor when it points at this page;
        // otherwise start at the top (fresh page).
        let (resume_page, resume_y) = (doc.flow_page, doc.flow_y);
        let y = if resume_page == page_idx {
            resume_y.unwrap_or(h - margin)
        } else {
            h - margin
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

    /// Persist the cursor so the next flow() call resumes here. Called
    /// at the end of every public placement method.
    pub(crate) fn sync_cursor(&mut self) {
        self.doc.flow_y = Some(self.y);
        self.doc.flow_page = self.page_idx;
    }

    fn new_page(&mut self) {
        let size = self.doc.page_size;
        self.doc.pages.push(crate::page::PageData {
            size,
            from_flow: true,
            ..Default::default()
        });
        self.page_idx = self.doc.pages.len() - 1;
        self.y = self.height - self.margin;
    }

    /// Record WinAnsi byte usage for font 0 (the fixed-F0 draw paths).
    fn mark_winansi_f0(&mut self, bytes: &[u8]) {
        let slot = self.doc.winansi_used.entry(0).or_insert([false; 256]);
        for &b in bytes {
            slot[usize::from(b)] = true;
        }
    }

    fn ensure_space(&mut self, needed: f64) {
        if self.y - self.margin < needed {
            self.new_page();
        }
    }

    /// Wrap text to `max_w` with real font metrics: the shared O(n)
    /// wrapper measures each word once (WinAnsi bytes, with a per-glyph
    /// fallback for anything WinAnsi cannot encode).
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
        let f: &pdfgen_font::LoadedFont = f;
        let scale = f64::from(f.units_per_em);
        Ok(crate::wrap::wrap(text, max_w, |word| {
            // Measure with the real encoding: WinAnsi bytes when the
            // word encodes, per-glyph advances otherwise.
            if let Ok(bytes) = pdfgen_font::winansi::encode(word) { bytes
            .iter()
            .filter_map(|&b| f.width_for_byte(b))
            .map(|w| f64::from(w) / scale * size)
            .sum() } else {
                // CID measurement: per-char cmap advances.
                let mut total = 0.0f64;
                for ch in word.chars() {
                    if let Some(g) = f.glyph_index(ch) {
                        if let Some(w) = f.glyph_width_units(g) {
                            total += w as f64 / scale * size;
                        }
                    }
                }
                total
            }
        }))
    }

    fn wrap(&self, text: &str, font: usize, size: f64) -> Result<Vec<String>, FontError> {
        self.wrap_to(text, font, size, self.width - 2.0 * self.margin)
    }

    /// Draw wrapped lines inside one marked-content sequence. Returns the
    /// MCID and the y after the last line. Does NOT advance self.y.
    /// Text with any non-WinAnsi character automatically switches to CID
    /// (Identity-H, two-byte codes); the font records the glyphs used.
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
            if let Ok(encoded) = pdfgen_font::winansi::encode(line) {
                // Record byte usage for font subsetting at save time.
                let slot = self.doc.winansi_used.entry(font).or_insert([false; 256]);
                for &b in &encoded {
                    slot[usize::from(b)] = true;
                }
                pd.content.text(&format!("F{font}"), size, x, y, &encoded);
            } else {
                // CID path: WinAnsi cannot encode this text. Map chars
                // to glyph IDs; record them so save() emits a Type0
                // font with CID widths and a matching ToUnicode. CID
                // text uses the separate F<idx>cid resource so the
                // simple (WinAnsi) font stays valid for other lines.
                let f = &self.doc.fonts[font];
                let (cids, chars) = pdfgen_font::cid::encode(line, f)?;
                let used = self.doc.cid_fonts.entry(font).or_default();
                for (c, g) in chars.iter().zip(&cids) {
                    used.insert((*c, *g));
                }
                let mut bytes = Vec::with_capacity(cids.len() * 2);
                for cid in cids {
                    bytes.extend_from_slice(&cid.to_be_bytes());
                }
                pd.content.text(&format!("F{font}cid"), size, x, y, &bytes);
            }
            y -= lh;
        }
        pd.content.end_tag();
        Ok((mcid, y))
    }

    /// Vertical space remaining on the current page.
    #[must_use] 
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
        // Bookmark entry for the outline (built at save); the mcid lets
        // UA-2 saves point at the heading's structure element.
        self.doc
            .bookmarks
            .push((level, text.to_string(), self.page_idx, mcid));
        self.doc.pages[self.page_idx].nodes.push(
            Node::leaf(format!("H{level}"), text.to_string(), 0, size)
                .with_pieces(vec![(self.page_idx, mcid)]),
        );
        self.sync_cursor();
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
        self.doc.pages[self.page_idx]
            .nodes
            .push(Node::leaf("P", text.to_string(), font, size).with_pieces(pieces));
        self.sync_cursor();
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
            list.children.push(Node::group("LI").with_children(vec![
                    Node::leaf("Lbl", "\u{2022}".into(), 0, 11.0)
                        .with_pieces(vec![(self.page_idx, lbl_mcid)]),
                    Node::leaf("LBody", item.to_string(), 0, 11.0)
                        .with_pieces(vec![(self.page_idx, body_mcid)]),
                ]));
        }
        self.doc.pages[self.page_idx].nodes.push(list);
        self.sync_cursor();
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
        let n_cols = header.len().max(rows.first().map_or(0, Vec::len));
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
                    {
                        let slot = self.doc.winansi_used.entry(0).or_insert([false; 256]);
                        for &b in &encoded {
                            slot[usize::from(b)] = true;
                        }
                    }
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
            let row_h = 11.0 * LH * wrapped.iter().map(Vec::len).max().unwrap_or(1) as f64;
            self.ensure_space(row_h);
            let mut tr = Node::group("TR");
            let mut x = self.margin;
            let mut y_top = self.y;
            for (ci, lines) in wrapped.iter().enumerate() {
                let cell_text = row.get(ci).map(std::string::ToString::to_string).unwrap_or_default();
                let pd = &mut self.doc.pages[self.page_idx];
                let mcid = pd.content.begin_tag("TD");
                let mut y = self.y;
                for line in lines {
                    let encoded = pdfgen_font::winansi::encode(line)?;
                    {
                        let slot = self.doc.winansi_used.entry(0).or_insert([false; 256]);
                        for &b in &encoded {
                            slot[usize::from(b)] = true;
                        }
                    }
                    pd.content.text("F0", 11.0, x + PAD, y - 13.0, &encoded);
                    y -= 11.0 * LH;
                }
                pd.content.end_tag();
                tr.children.push(
                    Node::leaf("TD", cell_text, 0, 11.0).with_pieces(vec![(self.page_idx, mcid)]),
                );
                x += col_w[ci.min(col_w.len() - 1)];
                y_top = y_top.min(y);
            }
            self.y = y_top;
            table.children.push(tr);
        }

        self.doc.pages[self.page_idx].nodes.push(table);
        self.sync_cursor();
        Ok(())
    }

    // --------------------------------------------------------- figures

    /// Tagged figure: image with alt text (empty alt = decorative).
    pub fn figure(&mut self, image: &Image, alt: &str, w: f64, h: f64) -> Result<(), FontError> {
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
        self.sync_cursor();
        Ok(())
    }

    /// Text presented as a Figure (used when retagging documents whose
    /// "figures" are textual): wraps like a paragraph but is tagged
    /// Figure with the given alt text.
    pub fn figure_text(&mut self, text: &str, alt: &str) -> Result<(), FontError> {
        let lines = self.wrap(text, 0, 11.0)?;
        let lh = 11.0 * LH;
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
            let (mcid, y) = self.draw_lines("Figure", 0, 11.0, &chunk, self.margin)?;
            self.y = y;
            pieces.push((self.page_idx, mcid));
            i += take;
            if i < lines.len() {
                self.new_page();
            }
        }
        let mut fig = Node::leaf("Figure", text.to_string(), 0, 11.0).with_pieces(pieces);
        if !alt.is_empty() {
            fig.alt = Some(alt.to_string());
        }
        self.doc.pages[self.page_idx].nodes.push(fig);
        self.sync_cursor();
        Ok(())
    }

    /// Page header furniture, drawn as a pagination artifact.
    pub fn header(&mut self, text: &str) -> Result<(), FontError> {
        let encoded = pdfgen_font::winansi::encode(text)?;
        self.mark_winansi_f0(&encoded);
        let pd = &mut self.doc.pages[self.page_idx];
        pd.content.begin_artifact("Header");
        pd.content
            .text("F0", 9.0, self.margin, self.height - 40.0, &encoded);
        pd.content.end_artifact();
        self.sync_cursor();
        Ok(())
    }

    // ------------------------------------------------------------ forms

    /// Add an interactive text field with an accessible label. The label
    /// is tagged text (`Form > Caption`-style: label drawn, widget linked);
    /// the field dictionary carries `/TU` (tooltip = the label) so screen
    /// readers announce it. Filling happens in any PDF viewer.
    pub fn text_field(&mut self, label: &str, name: &str) -> Result<(), FontError> {
        let field_w = 220.0;
        let field_h = 18.0;
        self.ensure_space(field_h + 11.0);

        // Label text, tagged.
        let encoded = pdfgen_font::winansi::encode(label)?;
        self.mark_winansi_f0(&encoded);
        let pd = &mut self.doc.pages[self.page_idx];
        let mcid = pd.content.begin_tag("Caption");
        pd.content
            .text("F0", 11.0, self.margin, self.y - 11.0, &encoded);
        pd.content.end_tag();

        // Field box drawn as the widget's border (annotation appearance).
        // Pure decoration on the page: mark it as an artifact.
        let y_box = self.y - field_h - 11.0;
        pd.content.begin_artifact("");
        pd.content
            .rect(self.margin + 160.0, y_box, field_w, field_h);
        pd.content.end_artifact();

        // Record for AcroForm emission at save.
        self.doc.fields.push(crate::form::FieldSpec {
            name: name.to_string(),
            tu: label.to_string(),
            page: self.page_idx,
            x: self.margin + 160.0,
            y: y_box,
            w: field_w,
            h: field_h,
        });

        self.y = y_box - 8.0;
        self.doc.pages[self.page_idx].nodes.push(
            Node::leaf("Caption", label.to_string(), 0, 11.0)
                .with_pieces(vec![(self.page_idx, mcid)]),
        );
        self.sync_cursor();
        Ok(())
    }

    /// Page footer furniture, drawn as a pagination artifact.
    pub fn footer(&mut self, text: &str) -> Result<(), FontError> {
        let encoded = pdfgen_font::winansi::encode(text)?;
        self.mark_winansi_f0(&encoded);
        let pd = &mut self.doc.pages[self.page_idx];
        pd.content.begin_artifact("Footer");
        pd.content.text("F0", 9.0, self.margin, 36.0, &encoded);
        pd.content.end_artifact();
        self.sync_cursor();
        Ok(())
    }
}
