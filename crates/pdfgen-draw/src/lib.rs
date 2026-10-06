//! # pdfgen-draw
//!
//! The engineering drawing kit: **ASME Y14 first, then ISO** (per the plan).
//!
//! * [`Sheet`] — Y14.1 sizes (A–F) with a border frame.
//! * [`Drawing::title_block`] — Y14.100 fields drawn as a REAL TAGGED
//!   TABLE (screen readers can walk the drawing's metadata).
//! * [`Drawing::linear_dimension`] — Y14.5 dimensions with extension
//!   lines and arrowheads (drawn as artifacts).
//! * [`Drawing::revision_block`] — Y14.35, fed from the file's own
//!   revision history.
//! * [`Drawing::finish_as_figure`] — wraps the sheet in a Figure with
//!   alt text, the accessible unit for the whole drawing.

use pdfgen::{Document, Node};

/// ASME Y14.1 sheet sizes (landscape width/height in inches).
#[derive(Debug, Clone, Copy)]
pub enum Sheet {
    /// 8.5 x 11 in.
    A,
    /// 11 x 17 in.
    B,
    /// 17 x 22 in.
    C,
    /// 22 x 34 in.
    D,
    /// 34 x 44 in.
    E,
    /// 28 x 40 in.
    F,
}

impl Sheet {
    /// Landscape size in points (ASME Y14.1: A=8.5x11, B=11x17, C=17x22,
    /// D=22x34, E=34x44, F=28x40 inches; width x height).
    pub fn points(self) -> (f64, f64) {
        const IN: f64 = 72.0;
        match self {
            Sheet::A => (11.0 * IN, 8.5 * IN),
            Sheet::B => (17.0 * IN, 11.0 * IN),
            Sheet::C => (22.0 * IN, 17.0 * IN),
            Sheet::D => (34.0 * IN, 22.0 * IN),
            Sheet::E => (44.0 * IN, 34.0 * IN),
            Sheet::F => (40.0 * IN, 28.0 * IN),
        }
    }
}

/// ISO 5457 A-series sheet sizes (landscape, mm -> points; 1 mm = 72/25.4 pt).
#[derive(Debug, Clone, Copy)]
pub enum IsoSheet {
    /// 210 x 297 mm (landscape).
    A4,
    /// 297 x 420 mm.
    A3,
    /// 420 x 594 mm.
    A2,
    /// 594 x 841 mm.
    A1,
    /// 841 x 1189 mm.
    A0,
}

impl IsoSheet {
    /// Landscape size in points.
    pub fn points(self) -> (f64, f64) {
        let mm = |w: f64, h: f64| (w * 72.0 / 25.4, h * 72.0 / 25.4);
        match self {
            IsoSheet::A4 => mm(297.0, 210.0),
            IsoSheet::A3 => mm(420.0, 297.0),
            IsoSheet::A2 => mm(594.0, 420.0),
            IsoSheet::A1 => mm(841.0, 594.0),
            IsoSheet::A0 => mm(1189.0, 841.0),
        }
    }

    /// Size designation as text ("A4").
    pub fn letter(self) -> &'static str {
        match self {
            IsoSheet::A4 => "A4",
            IsoSheet::A3 => "A3",
            IsoSheet::A2 => "A2",
            IsoSheet::A1 => "A1",
            IsoSheet::A0 => "A0",
        }
    }
}

/// Title block field rows (drawn as a tagged TH/TD table).
#[derive(Debug, Clone)]
pub struct TitleField {
    /// Label shown in the cell (e.g. "TITLE").
    pub label: &'static str,
    /// Value shown in the cell.
    pub value: String,
}

/// One drawing sheet under construction.
pub struct Drawing<'a> {
    doc: &'a mut Document,
    /// Sheet width in points.
    pub w: f64,
    /// Sheet height in points.
    pub h: f64,
    /// Border inset in points.
    pub border: f64,
    /// Page index of this sheet inside the document.
    pub page: usize,
}

impl<'a> Drawing<'a> {
    /// Begin an ASME drawing sheet (Y14.1 border frame).
    pub fn new(doc: &'a mut Document, sheet: Sheet) -> Self {
        let (w, h) = sheet.points();
        let page = doc.add_draw_page(w, h);
        let mut d = Drawing {
            doc,
            w,
            h,
            border: 36.0,
            page,
        };
        d.frame();
        d
    }

    /// Begin an ISO 5457 sheet: A-series size with the ISO frame
    /// (10 mm trimming margin + inner content frame) and, on sheets larger
    /// than A4, the centring-marks cross pattern.
    pub fn new_iso(doc: &'a mut Document, sheet: IsoSheet) -> Self {
        let (w, h) = sheet.points();
        let page = doc.add_draw_page(w, h);
        // ISO 5457: 10 mm trimming margin, 5 mm inner frame gap for A4,
        // 10 mm for larger sheets.
        let trim = 10.0 * 72.0 / 25.4;
        let inner_gap = if matches!(sheet, IsoSheet::A4) {
            5.0
        } else {
            10.0
        } * 72.0
            / 25.4;
        let d = Drawing {
            doc,
            w,
            h,
            border: trim,
            page,
        };

        let (w_, h_, b) = (d.w, d.h, d.border);
        let f = b + inner_gap;
        let mut ops = format!(
            "0.7 w {} {} {} {} re S\n0.5 w {} {} {} {} re S\n",
            b,
            b,
            w_ - 2.0 * b,
            h_ - 2.0 * b,
            f,
            f,
            w_ - 2.0 * f,
            h_ - 2.0 * f
        );
        // ISO 5457 centring marks (A3 and up): short crosshairs at the
        // middle of each edge, drawn edge-to-frame.
        if !matches!(sheet, IsoSheet::A4) {
            let half_w = w_ / 2.0;
            let half_h = h_ / 2.0;
            let mark = 8.0;
            // Left and right mid-edge horizontal marks.
            ops.push_str(&format!(
                "0.35 w 0 {} {} {} m l S\n{} {} {} {} m l S\n",
                half_h,
                mark,
                half_h,
                w_ - mark,
                half_h,
                w_,
                half_h
            ));
            // Top and bottom mid-edge vertical marks.
            ops.push_str(&format!(
                "0.35 w {} 0 {} {} m l S\n{} {} {} {} m l S\n",
                half_w,
                half_w,
                mark,
                half_w,
                h_ - mark,
                half_w,
                h_
            ));
        }
        d.doc.begin_artifact(d.page, "");
        d.doc.raw_ops(d.page, &ops);
        d.doc.end_artifact(d.page);
        d
    }

    /// Draw the ASME border frame (two concentric rects, artifact content).
    fn frame(&mut self) {
        let (w, h, b) = (self.w, self.h, self.border);
        let ops = format!(
            "0.7 w {} {} {} {} re S\n0.4 w {} {} {} {} re S\n",
            b,
            b,
            w - 2.0 * b,
            h - 2.0 * b,
            b + 12.0,
            b + 12.0,
            w - 2.0 * (b + 12.0),
            h - 2.0 * (b + 12.0)
        );
        self.doc.begin_artifact(self.page, "");
        self.doc.raw_ops(self.page, &ops);
        self.doc.end_artifact(self.page);
    }

    /// Draw the Y14.100 title block (bottom-right) as a REAL TAGGED TABLE.
    /// Returns the number of rows drawn.
    pub fn title_block(&mut self, fields: &[TitleField]) -> usize {
        let n = fields.len().max(1);
        let block_w = 180.0;
        let row_h = 16.0;
        let block_h = row_h * n as f64;
        let x0 = self.w - self.border - 12.0 - block_w;
        let y0 = self.border + 12.0;

        // Grid lines (artifact).
        let mut ops = String::new();
        for i in 0..=n {
            let y = y0 + row_h * i as f64;
            ops.push_str(&format!("0.5 w {} {} {} 0 re S\n", x0, y, block_w));
        }
        ops.push_str(&format!("0.5 w {} {} 0 {} re S\n", x0, y0, block_h));
        self.doc.begin_artifact(self.page, "");
        self.doc.raw_ops(self.page, &ops);
        self.doc.end_artifact(self.page);

        // Cells: one tagged row per field (TH label + TD value).
        let mut table = Node::group("Table");
        let mut y = y0 + row_h / 2.0 - 3.0;
        for f in fields {
            let label_ops = format!(
                "BT /F0 7 Tf {} {} Td ({}) Tj ET\n",
                x0 + 4.0,
                y,
                escape(f.label)
            );
            let value_ops = format!(
                "BT /F0 8 Tf {} {} Td ({}) Tj ET\n",
                x0 + 60.0,
                y,
                escape(&f.value)
            );
            let th_mcid = self.doc.begin_tag(self.page, "TH");
            self.doc.raw_ops(self.page, &label_ops);
            self.doc.end_tag(self.page);
            let td_mcid = self.doc.begin_tag(self.page, "TD");
            self.doc.raw_ops(self.page, &value_ops);
            self.doc.end_tag(self.page);

            let mut tr = Node::group("TR");
            tr.children.push(
                Node::leaf("TH", f.label.to_string(), 0, 7.0)
                    .with_pieces(vec![(self.page, th_mcid)])
                    .with_scope("Column"),
            );
            tr.children.push(
                Node::leaf("TD", f.value.clone(), 0, 8.0).with_pieces(vec![(self.page, td_mcid)]),
            );
            table.children.push(tr);
            y += row_h;
        }
        self.doc.push_node(self.page, table);
        n
    }

    /// Draw a stroked part outline rectangle (geometry → artifact).
    pub fn part_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let ops = format!("1.0 w {} {} {} {} re S\n", x, y, w, h);
        self.doc.begin_artifact(self.page, "");
        self.doc.raw_ops(self.page, &ops);
        self.doc.end_artifact(self.page);
    }

    /// Draw one dimension with extension lines and arrowheads (Y14.5).
    /// Pure geometry → artifact.
    pub fn linear_dimension(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        offset: f64,
        text: &str,
    ) {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < f64::EPSILON {
            return;
        }
        let nx = -dy / len * offset;
        let ny = dx / len * offset;
        let a1x = x1 + nx;
        let a1y = y1 + ny;
        let a2x = x2 + nx;
        let a2y = y2 + ny;
        let arrow = 4.0;

        let mut ops = String::new();
        ops.push_str(&format!("0.35 w {} {} {} {} m l S\n", x1, y1, a1x, a1y));
        ops.push_str(&format!("0.35 w {} {} {} {} m l S\n", x2, y2, a2x, a2y));
        ops.push_str(&format!("0.5 w {} {} {} {} m l S\n", a1x, a1y, a2x, a2y));
        let ux = dx / len;
        let uy = dy / len;
        for (px, py, sx) in [(a1x, a1y, 1.0), (a2x, a2y, -1.0)] {
            let t1x = px + sx * arrow * ux;
            let t1y = py + sx * arrow * uy;
            let bx = -uy * 1.5;
            let by = ux * 1.5;
            ops.push_str(&format!(
                "{} {} m {} {} l {} {} l h f\n",
                px,
                py,
                t1x + bx,
                t1y + by,
                t1x - bx,
                t1y - by
            ));
        }
        let mid_x = (a1x + a2x) / 2.0;
        let mid_y = (a1y + a2y) / 2.0;
        ops.push_str(&format!(
            "BT /F0 8 Tf {} {} Td ({}) Tj ET\n",
            mid_x - 12.0,
            mid_y + 2.0,
            escape(text)
        ));
        self.doc.begin_artifact(self.page, "");
        self.doc.raw_ops(self.page, &ops);
        self.doc.end_artifact(self.page);
    }

    /// Wrap the sheet in a Figure with alt text: the drawing as a whole
    /// is the accessible unit.
    pub fn finish_as_figure(&mut self, alt: &str) {
        let mut fig = Node::leaf("Figure", String::new(), 0, 0.0);
        fig.alt = if alt.is_empty() {
            None
        } else {
            Some(alt.to_string())
        };
        let mcid = self.doc.begin_tag(self.page, "Figure");
        self.doc
            .raw_ops(self.page, "BT /F0 0.1 Tf 0 0 Td ( ) Tj ET\n");
        self.doc.end_tag(self.page);
        fig.pieces.push((self.page, mcid));
        self.doc.push_node(self.page, fig);
    }

    /// The revision block (Y14.35), fed from a file's own revision
    /// history. Drawn as a tagged table; returns rows added.
    pub fn revision_block(&mut self, path: &str) -> Result<usize, String> {
        let entries = pdfgen_revision::history(path)?;
        let rows: Vec<(String, String)> = entries
            .iter()
            .skip(1)
            .map(|e| (format!("{}", e.index), e.message.clone()))
            .collect();
        if rows.is_empty() {
            return Ok(0);
        }
        let n = rows.len();
        let block_w = 150.0;
        let row_h = 14.0;
        let x0 = self.border + 12.0;
        let y0 = self.border + 12.0;
        let block_h = row_h * (n + 1) as f64;

        // Grid (artifact).
        let mut ops = String::new();
        for i in 0..=n + 1 {
            let y = y0 + row_h * i as f64;
            ops.push_str(&format!("0.5 w {} {} {} 0 re S\n", x0, y, block_w));
        }
        ops.push_str(&format!("0.5 w {} {} 0 {} re S\n", x0, y0, block_h));
        self.doc.begin_artifact(self.page, "");
        self.doc.raw_ops(self.page, &ops);
        self.doc.end_artifact(self.page);

        // Header row (tagged).
        let mut table = Node::group("Table");
        let hdr_y = y0 + row_h * n as f64 + row_h / 2.0 - 3.0;
        let rev_mcid = self.doc.begin_tag(self.page, "TH");
        self.doc.raw_ops(
            self.page,
            &format!("BT /F0 7 Tf {} {} Td (REV) Tj ET\n", x0 + 4.0, hdr_y),
        );
        self.doc.end_tag(self.page);
        let desc_mcid = self.doc.begin_tag(self.page, "TH");
        self.doc.raw_ops(
            self.page,
            &format!(
                "BT /F0 7 Tf {} {} Td (DESCRIPTION) Tj ET\n",
                x0 + 30.0,
                hdr_y
            ),
        );
        self.doc.end_tag(self.page);
        let mut hdr = Node::group("TR");
        hdr.children.push(
            Node::leaf("TH", "REV".into(), 0, 7.0)
                .with_pieces(vec![(self.page, rev_mcid)])
                .with_scope("Column"),
        );
        hdr.children.push(
            Node::leaf("TH", "DESCRIPTION".into(), 0, 7.0)
                .with_pieces(vec![(self.page, desc_mcid)])
                .with_scope("Column"),
        );
        table.children.push(hdr);

        let mut y = y0 + row_h * (n as f64 - 1.0) + row_h / 2.0 - 3.0;
        for (rev, desc) in rows {
            let r_mcid = self.doc.begin_tag(self.page, "TD");
            self.doc.raw_ops(
                self.page,
                &format!(
                    "BT /F0 8 Tf {} {} Td ({}) Tj ET\n",
                    x0 + 4.0,
                    y,
                    escape(&rev)
                ),
            );
            self.doc.end_tag(self.page);
            let d_mcid = self.doc.begin_tag(self.page, "TD");
            self.doc.raw_ops(
                self.page,
                &format!(
                    "BT /F0 8 Tf {} {} Td ({}) Tj ET\n",
                    x0 + 30.0,
                    y,
                    escape(&desc)
                ),
            );
            self.doc.end_tag(self.page);
            let mut tr = Node::group("TR");
            tr.children
                .push(Node::leaf("TD", rev, 0, 8.0).with_pieces(vec![(self.page, r_mcid)]));
            tr.children
                .push(Node::leaf("TD", desc, 0, 8.0).with_pieces(vec![(self.page, d_mcid)]));
            table.children.push(tr);
            y -= row_h;
        }
        self.doc.push_node(self.page, table);
        Ok(n)
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for &b in s.as_bytes() {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}
