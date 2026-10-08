//! # pdfgen-canvas
//!
//! Builds page content streams: drawing operators and, critically for
//! accessibility, marked-content sequences that tie drawn bytes to structure
//! elements.

use pdfgen_core::fmt_real;

/// A marked-content-capable content stream under construction.
#[derive(Debug, Default)]
pub struct Content {
    ops: String,
    next_mcid: u32,
}

/// Escape bytes for a literal string in a content stream.
fn escape_literal(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 8);
    pdfgen_core::escape_bytes_into(&mut s, bytes);
    s
}

impl Content {
    /// New empty content stream.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a marked-content sequence for a structure element, returning
    /// the MCID assigned to it.
    pub fn begin_tag(&mut self, tag: &str) -> u32 {
        let mcid = self.next_mcid;
        self.next_mcid += 1;
        self.ops.push_str(&format!("/{tag} <</MCID {mcid}>> BDC\n"));
        mcid
    }

    /// End the innermost marked-content sequence.
    pub fn end_tag(&mut self) {
        self.ops.push_str("EMC\n");
    }

    /// Begin a `/Span` marked-content sequence carrying `/ActualText`
    /// (the logical source text for what follows, for shaped or
    /// reordered content). The string is UTF-16BE with BOM per the
    /// spec for text strings.
    pub fn begin_actual_text(&mut self, logical: &str) {
        self.ops.push_str("/Span <</ActualText (");
        let mut s16: Vec<u8> = vec![0xFE, 0xFF];
        for unit in logical.encode_utf16() {
            s16.extend_from_slice(&unit.to_be_bytes());
        }
        self.ops.push_str(&escape_literal(&s16));
        self.ops.push_str(")>> BDC\n");
    }

    /// End the innermost `/Span` opened by `begin_actual_text`.
    pub fn end_actual_text(&mut self) {
        self.ops.push_str("EMC\n");
    }

    /// Draw one line of text at `(x, y)` in the given font resource and size.
    pub fn text(&mut self, font_res: &str, size: f64, x: f64, y: f64, bytes: &[u8]) {
        self.ops.push_str("BT\n");
        self.ops.push_str(&format!(
            "/{font_res} {} Tf\n{} {} Td\n({}) Tj\nET\n",
            fmt_real(size),
            fmt_real(x),
            fmt_real(y),
            escape_literal(bytes)
        ));
    }

    /// Tagged single-line text: marked content wrapping a text draw.
    /// Returns the MCID.
    pub fn tagged_text(
        &mut self,
        tag: &str,
        font_res: &str,
        size: f64,
        x: f64,
        y: f64,
        bytes: &[u8],
    ) -> u32 {
        let mcid = self.begin_tag(tag);
        self.text(font_res, size, x, y, bytes);
        self.end_tag();
        mcid
    }

    /// Number of MCIDs handed out so far.
    #[must_use]
    pub fn mcid_count(&self) -> u32 {
        self.next_mcid
    }

    /// Append raw content-stream operators verbatim. The caller is
    /// responsible for being inside a tag or artifact (all drawing in
    /// pdfgen-draw uses this within artifacts or tagged runs).
    pub fn raw_ops(&mut self, ops: &str) {
        self.ops.push_str(ops);
    }

    /// Begin an artifact sequence (page furniture). `subtype` is `Header`,
    /// `Footer`, or empty for plain decoration (Matterhorn 18-001/18-002).
    pub fn begin_artifact(&mut self, subtype: &str) {
        if subtype.is_empty() {
            // A property dict is required: some validators (veraPDF) treat
            // a bare /Artifact name without one as unmarked content.
            self.ops.push_str("/Artifact <</Type /Pagination>> BDC\n");
        } else {
            self.ops.push_str(&format!(
                "/Artifact <</Type /Pagination /Subtype /{subtype}>> BDC\n"
            ));
        }
    }

    /// End the innermost artifact sequence.
    pub fn end_artifact(&mut self) {
        self.ops.push_str("EMC\n");
    }

    /// Stroke a rectangle (table borders, frames). Must be called inside a
    /// tag or artifact so the path is not unmarked content.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.ops.push_str(&format!(
            "0.5 w {} {} {} {} re S\n",
            fmt_real(x),
            fmt_real(y),
            fmt_real(w),
            fmt_real(h)
        ));
    }

    /// Draw an image XObject resource at (x, y) sized w×h. Must be inside
    /// a tag (Figure) or artifact.
    pub fn image(&mut self, res: &str, x: f64, y: f64, w: f64, h: f64) {
        self.ops.push_str(&format!(
            "q {} 0 0 {} {} {} cm /{res} Do Q\n",
            fmt_real(w),
            fmt_real(h),
            fmt_real(x),
            fmt_real(y)
        ));
    }

    /// Finish: return the raw content stream bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.ops.into_bytes()
    }

    /// Finish without consuming: returns the same bytes, leaving the
    /// builder intact so a document can be saved more than once.
    #[must_use]
    pub fn finish_ref(&self) -> Vec<u8> {
        self.ops.clone().into_bytes()
    }
}
