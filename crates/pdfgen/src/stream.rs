//! The streaming writer: event-oriented, chunk-by-chunk PDF generation.
//!
//! Unlike [`crate::Document`] (which holds the whole document tree in
//! memory), [`StreamWriter`] **flushes completed sections to the output file
//! as they finish**. Memory use stays flat regardless of document size:
//!
//! * content arrives in large event batches ([`Vec<StreamEvent>`]);
//! * a page's objects are written to disk the moment the page completes;
//! * the structure tree and parent tree are built from small records, not
//!   a retained object graph;
//! * the cross-reference table is a list of (id → offset) pairs.
//!
//! This is the path for high-volume and massive documents. The in-memory
//! [`crate::Document`] stays for small documents and full editing.

use pdfgen_core::{Dict, Name, Object, PdfString, Ref, Stream};
use std::io::Write;

/// One layout event, in document order.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Start a high-level element (Paragraph, Table, List, H1..H6, …).
    Begin {
        /// Structure type (e.g. `H1`, `P`, `Table`).
        tag: String,
        /// Alt text (figures), if any.
        alt: Option<String>,
    },
    /// A chunk of text inside the current element. Chunks are large and
    /// pre-split by the caller; the writer wraps them to the page.
    Text {
        /// The text.
        text: String,
        /// Font handle (0 = default font).
        font: usize,
        /// Font size in points.
        size: f64,
    },
    /// Close the innermost open element. The element is finalized.
    End,
    /// Force a page break.
    PageBreak,
    /// Place an image file (PNG/JPEG) inside the current element as a
    /// figure. Alt text comes from the enclosing Begin.
    Image {
        /// Path of the image file.
        path: String,
        /// Draw width in points.
        width: f64,
        /// Draw height in points.
        height: f64,
    },
}

/// Errors from the streaming writer.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// IO failure on the output file.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A Text event arrived with no open element.
    #[error("text before Begin")]
    NoOpenElement,
    /// An End/PageBreak was invalid.
    #[error("End with no open element")]
    NoElementToClose,
    /// Font resolution or encoding failure.
    #[error("font: {0}")]
    Font(String),
}

/// Line height multiplier for streamed text.
const LH: f64 = 1.45;
/// Page margins in points.
const MARGIN: f64 = 72.0;
/// Page size: US Letter.
const PAGE: (f64, f64) = (612.0, 792.0);

/// Reserved object numbers, allocated up front so page objects written
/// during streaming carry correct references.
mod ids {
    /// 1: catalog, 2: pages, 3: metadata (XMP).
    pub const CATALOG: u32 = 1;
    pub const PAGES: u32 = 2;
    pub const METADATA: u32 = 3;
    /// 4+: 4 objects per font (dict, descriptor, file, tounicode).
    pub const FONT_BASE: u32 = 4;
}

/// Streaming PDF/UA writer. Create, push event batches, [`finish`].
///
/// [`finish`]: StreamWriter::finish
pub struct StreamWriter {
    /// Buffered output (the file is flushed at finish; every write goes
    /// through the 8 KiB buffer, no per-object syscall).
    out: std::io::BufWriter<std::fs::File>,
    /// PDF byte offset of the next write, tracked by adding bytes written
    /// (no stream_position syscall per object).
    cursor: u64,
    /// Flushed objects: byte offset indexed by object id (0 = absent).
    xref: Vec<u64>,
    /// Ids handed out for pages/contents/elements so far.
    next_obj: u32,
    /// Open structure elements awaiting their End.
    open: Vec<OpenElem>,
    /// The current page's accumulating content stream.
    page_content: String,
    /// Next MCID within the current page.
    next_mcid: u32,
    /// Y cursor on the current page.
    y: f64,
    /// Fonts to embed (resolved at create time; index = F handle).
    fonts: Vec<pdfgen_font::LoadedFont>,
    /// Font dict object ids, parallel to `fonts`.
    font_ids: Vec<u32>,
    /// Completed element records (small: tag, alt, pieces).
    elems: Vec<ElemRecord>,
    /// Page object ids by page index.
    page_ids: Vec<u32>,
    /// Per page: MCID -> element record index (parent tree source).
    mcid_map: Vec<Vec<Option<u32>>>,
    profile: pdfgen_profile::Profile,
    title: String,
    lang: String,
    /// Reusable operator scratch buffer (avoids per-op allocations).
    scratch: String,
    /// Loaded images (registered on first use).
    images: Vec<crate::image::Image>,
    /// Image XObject object ids; 0 = not yet written.
    image_ids: Vec<u32>,
}

/// A structure element being built.
struct OpenElem {
    tag: String,
    alt: Option<String>,
    pieces: Vec<(usize, u32)>,
    /// Completed child element indexes (into the elems tree).
    children: Vec<u32>,
    /// Table cell scope ("Column" for TH), if any.
    scope: Option<String>,
}

/// A completed structure element (its serialization fields only).
struct ElemRecord {
    tag: String,
    alt: Option<String>,
    pieces: Vec<(usize, u32)>,
    children: Vec<u32>,
    /// Table cell scope ("Column" for TH), if any.
    scope: Option<String>,
}

impl StreamWriter {
    /// New streaming writer over a file path. The default font is resolved
    /// immediately so bare documents work.
    pub fn create(
        path: &str,
        profile: pdfgen_profile::Profile,
        title: &str,
        lang: &str,
    ) -> Result<Self, StreamError> {
        // The output directory may not exist yet; create it rather than
        // failing the stream.
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut header = format!("%PDF-{}\n", profile.pdf_version().header()).into_bytes();
        header.extend_from_slice(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
        out.write_all(&header)?;
        let cursor = header.len() as u64;

        let fonts = vec![pdfgen_font::LoadedFont::load(default_font_path()?)
            .map_err(|e| StreamError::Font(e.to_string()))?];
        let font_ids = (0..fonts.len())
            .map(|i| ids::FONT_BASE + (i as u32) * 4)
            .collect();
        let next_obj = ids::FONT_BASE + (fonts.len() as u32) * 4;

        Ok(StreamWriter {
            out,
            cursor,
            xref: Vec::new(),
            next_obj,
            open: Vec::new(),
            page_content: String::new(),
            next_mcid: 0,
            y: PAGE.1 - MARGIN,
            fonts,
            font_ids,
            elems: Vec::new(),
            page_ids: Vec::new(),
            mcid_map: vec![Vec::new()],
            profile,
            title: title.to_string(),
            lang: lang.to_string(),
            scratch: String::with_capacity(4096),
            images: Vec::new(),
            image_ids: Vec::new(),
        })
    }

    /// Push a batch of events. Large batches amortize per-event cost.
    pub fn push(&mut self, batch: Vec<StreamEvent>) -> Result<(), StreamError> {
        for ev in batch {
            self.one(ev)?;
        }
        Ok(())
    }

    fn one(&mut self, ev: StreamEvent) -> Result<(), StreamError> {
        match ev {
            StreamEvent::Begin { tag, alt } => {
                // TH cells get Scope=Column automatically (PDF/UA 15-003).
                let scope = if tag == "TH" {
                    Some("Column".to_string())
                } else {
                    None
                };
                self.open.push(OpenElem {
                    tag,
                    alt,
                    pieces: Vec::new(),
                    children: Vec::new(),
                    scope,
                });
            }
            StreamEvent::Text { text, font, size } => {
                if self.open.is_empty() {
                    // Tolerant: imply a paragraph.
                    self.open.push(OpenElem {
                        tag: "P".into(),
                        alt: None,
                        pieces: Vec::new(),
                        children: Vec::new(),
                        scope: None,
                    });
                }
                // Take the open element out, place, put back (borrow dance).
                let mut elem = self.open.pop().expect("checked");
                let tag = elem.tag.clone();
                self.place_lines(&tag, font, size, &text, &mut elem)?;
                self.open.push(elem);
            }
            StreamEvent::End => {
                let elem = self.open.pop().ok_or(StreamError::NoElementToClose)?;
                self.record_elem(elem);
            }
            StreamEvent::PageBreak => self.flush_page()?,
            StreamEvent::Image {
                path,
                width,
                height,
            } => {
                // Load and register the image; the XObject is written on
                // first use at the next page flush.
                let img = crate::image::Image::load(&path)
                    .map_err(|e| StreamError::Font(e.to_string()))?;
                let idx = self.images.len();
                self.images.push(img);
                self.image_ids.push(0);
                // Reserve space, grab an MCID, and draw NOW: the draw op
                // and the Figure's EMC go straight into the page content
                // so later text cannot nest inside the Figure.
                let page = self.page_ids.len();
                let y = self.y - height;
                let mcid = self.next_mcid;
                self.next_mcid += 1;
                let mut op = format!(
                    "/Figure <</MCID {mcid}>> BDC\nq {} 0 0 {} {} {} cm /Im{idx} Do Q\nEMC\n",
                    pdfgen_core::fmt_real(width),
                    pdfgen_core::fmt_real(height),
                    pdfgen_core::fmt_real(MARGIN),
                    pdfgen_core::fmt_real(y)
                );
                self.page_content.push_str(&op);
                op.clear();
                // The figure piece belongs to the open element (or its own
                // top-level element when none is open) for the ParentTree.
                let piece = (page, mcid);
                if let Some(elem) = self.open.last_mut() { elem.pieces.push(piece) } else {
                    // Standalone figure: make it a top-level element.
                    self.open.push(OpenElem {
                        tag: "Figure".into(),
                        alt: None,
                        pieces: vec![piece],
                        children: Vec::new(),
                        scope: None,
                    });
                    // Close it right away.
                    let elem = self.open.pop().expect("just pushed");
                    self.record_elem(elem);
                }
                self.y = y;
            }
        }
        Ok(())
    }

    /// Move a finished element into the record list, register its MCIDs in
    /// the ParentTree map, and nest it under the still-open parent.
    fn record_elem(&mut self, elem: OpenElem) {
        let idx = self.elems.len() as u32;
        for &(page, mcid) in &elem.pieces {
            while self.mcid_map.len() <= page {
                self.mcid_map.push(Vec::new());
            }
            let row = &mut self.mcid_map[page];
            while row.len() <= mcid as usize {
                row.push(None);
            }
            row[mcid as usize] = Some(idx);
        }
        // Nest under the parent: a completed element becomes a CHILD of
        // the innermost still-open element (lists hold LIs, LIs hold
        // Lbl/LBody, tables hold TRs...).
        if let Some(parent) = self.open.last_mut() {
            parent.children.push(idx);
        }
        self.elems.push(ElemRecord {
            tag: elem.tag,
            alt: elem.alt,
            pieces: elem.pieces,
            children: elem.children,
            scope: elem.scope,
        });
    }

    /// Draw wrapped lines inside marked content, splitting across page
    /// breaks; each chunk gets its own MCID recorded on the element.
    ///
    /// Optimized like iText's layout loop: each word's width is measured
    /// ONCE and summed incrementally (no O(n²) candidate re-measurement),
    /// and operators are written into a reusable scratch buffer.
    fn place_lines(
        &mut self,
        tag: &str,
        font: usize,
        size: f64,
        text: &str,
        elem: &mut OpenElem,
    ) -> Result<(), StreamError> {
        let f = self
            .fonts
            .get(font)
            .ok_or_else(|| StreamError::Font(format!("bad font handle {font}")))?;
        let lh = size * LH;
        let max_w = PAGE.0 - 2.0 * MARGIN;

        // --- O(n) wrap: one width measurement per word, packed greedily. ---
        let scale = f64::from(f.units_per_em);
        let mut lines: Vec<String> = Vec::new();
        {
            let mut cur = String::with_capacity(128);
            let mut cur_w = 0.0f64;
            let space_w = f.byte_width_pt(b' ', size).unwrap_or(size * 0.25);
            let mut pending_space = false;
            let mut word_start: Option<usize> = None;
            for (i, _ch) in text.char_indices() {
                let is_space = text.as_bytes().get(i) == Some(&b' ');
                if is_space {
                    if let Some(ws) = word_start.take() {
                        let w = word_width_pt(f, &text[ws..i], size, scale);
                        pack_word(
                            &mut lines,
                            &mut cur,
                            &mut cur_w,
                            &mut pending_space,
                            space_w,
                            w,
                            max_w,
                            &text[ws..i],
                        );
                    } else {
                        pending_space = !cur.is_empty();
                    }
                } else if word_start.is_none() {
                    word_start = Some(i);
                }
            }
            if let Some(ws) = word_start.take() {
                let w = word_width_pt(f, &text[ws..], size, scale);
                pack_word(
                    &mut lines,
                    &mut cur,
                    &mut cur_w,
                    &mut pending_space,
                    space_w,
                    w,
                    max_w,
                    &text[ws..],
                );
            }
            if !cur.is_empty() {
                lines.push(cur);
            } else if lines.is_empty() {
                lines.push(String::new());
            }
        }

        // --- Emit with the reusable scratch buffer. ---
        let mut i = 0usize;
        while i < lines.len() {
            let fits = ((self.y - MARGIN) / lh).floor().max(0.0) as usize;
            if fits == 0 {
                self.flush_page()?;
                continue;
            }
            let take = fits.min(lines.len() - i);
            let mcid = self.next_mcid;
            self.next_mcid += 1;
            let mut scratch = std::mem::take(&mut self.scratch);
            scratch.clear();
            use std::fmt::Write as _;
            let _ = writeln!(scratch, "/{tag} <</MCID {mcid}>> BDC");
            let mut y = self.y;
            for line in &lines[i..i + take] {
                let encoded = pdfgen_font::winansi::encode(line)
                    .map_err(|e| StreamError::Font(e.to_string()))?;
                let _ = write!(
                    scratch,
                    "BT\n/F{font} {} Tf\n{} {} Td\n(",
                    pdfgen_core::fmt_real(size),
                    pdfgen_core::fmt_real(MARGIN),
                    pdfgen_core::fmt_real(y)
                );
                push_escaped(&mut scratch, &encoded);
                scratch.push_str(") Tj\nET\n");
                y -= lh;
            }
            scratch.push_str("EMC\n");
            self.page_content.push_str(&scratch);
            self.scratch = scratch;

            let page = self.page_ids.len();
            elem.pieces.push((page, mcid));
            self.y = y;
            i += take;
            if i < lines.len() {
                self.flush_page()?;
            }
        }
        Ok(())
    }

    /// Flush the current page: its content stream (Flate-compressed, like
    /// iText) and page object go to disk NOW. This is the streaming core —
    /// completed pages are never retained.
    fn flush_page(&mut self) -> Result<(), StreamError> {
        if self.page_content.is_empty() {
            return Ok(());
        }
        // Image draw ops are emitted inline at the Image event; nothing
        // is deferred to flush.

        let content_id = self.alloc();
        let raw = std::mem::take(&mut self.page_content).into_bytes();
        let compressed = flate_compress(&raw);
        let mut d = Dict::new();
        d.set("Filter", "FlateDecode");
        d.set("Length", compressed.len() as i64);
        self.write_object(
            content_id,
            &Object::Stream(Stream {
                dict: d,
                data: compressed,
            }),
        )?;

        let page_id = self.alloc();
        let page_idx = self.page_ids.len();
        let mut font_res = Dict::new();
        for (i, &fid) in self.font_ids.iter().enumerate() {
            font_res.set(format!("F{i}"), Object::Ref(Ref::new(fid)));
        }
        let mut res = Dict::new();
        res.set("Font", Object::Dict(font_res));

        // Emit image XObjects ONCE each (id 0 = not yet written), before
        // this page's Resources serializes, so they carry real ids.
        if !self.images.is_empty() {
            for i in 0..self.images.len() {
                if self.image_ids[i] == 0 {
                    let xid = self.alloc();
                    let img = self.images[i].clone();
                    emit_image_xobject(self, xid, &img)?;
                    self.image_ids[i] = xid;
                }
            }
            let mut xobj = Dict::new();
            for (i, &xid) in self.image_ids.iter().enumerate() {
                xobj.set(format!("Im{i}"), Object::Ref(Ref::new(xid)));
            }
            res.set("XObject", Object::Dict(xobj));
        }
        let mut pg = Dict::new();
        pg.set("Type", "Page");
        pg.set("Parent", Object::Ref(Ref::new(ids::PAGES)));
        pg.set(
            "MediaBox",
            Object::Array(vec![
                Object::Int(0),
                Object::Int(0),
                Object::Real(PAGE.0.into()),
                Object::Real(PAGE.1.into()),
            ]),
        );
        pg.set("Resources", Object::Dict(res));
        pg.set("Contents", Object::Ref(Ref::new(content_id)));
        pg.set("StructParents", page_idx as i64);
        pg.set("Tabs", "S");
        self.write_object(page_id, &Object::Dict(pg))?;
        self.page_ids.push(page_id);
        self.next_mcid = 0;
        self.y = PAGE.1 - MARGIN;
        self.mcid_map.push(Vec::new());
        Ok(())
    }

    fn alloc(&mut self) -> u32 {
        let id = self.next_obj;
        self.next_obj += 1;
        id
    }

    /// Serialize one object to disk at the cursor and record its offset.
    fn write_object(&mut self, id: u32, obj: &Object) -> Result<(), StreamError> {
        let off = self.cursor;
        self.record_offset(id, off);
        let mut head = format!("{id} 0 obj\n");
        match obj {
            Object::Stream(s) => {
                head.push_str("<<");
                let mut d = s.dict.clone();
                d.set("Length", s.data.len() as i64);
                let mut first = true;
                for (k, v) in &d.0 {
                    if !first {
                        head.push(' ');
                    }
                    first = false;
                    head.push_str(&serialize_entry(k, v));
                }
                head.push_str(">>");
                head.push_str("\nstream\n");
                let n = head.len() + s.data.len() + b"\nendstream\nendobj\n".len();
                self.out.write_all(head.as_bytes())?;
                self.out.write_all(&s.data)?;
                self.out.write_all(b"\nendstream\nendobj\n")?;
                self.cursor += n as u64;
            }
            obj => {
                pdfgen_core::write_object(&mut head, obj);
                head.push_str("\nendobj\n");
                self.out.write_all(head.as_bytes())?;
                self.cursor += head.len() as u64;
            }
        }
        Ok(())
    }

    /// Record an object's byte offset (xref grows on demand).
    fn record_offset(&mut self, id: u32, off: u64) {
        let idx = id as usize;
        if self.xref.len() <= idx {
            self.xref.resize(idx + 1, 0);
        }
        self.xref[idx] = off;
    }

    /// Finish the document: fonts, structure tree, parent tree, pages,
    /// metadata, catalog, xref, trailer. Returns the accessibility report.
    pub fn finish(mut self) -> Result<pdfgen_profile::SaveReport, StreamError> {
        self.flush_page()?;

        // The same centralized rules the flow writer uses (pdfgen-profile
        // rules:: table); compliance derives from the violations.
        let mut violations: Vec<pdfgen_profile::Violation> = Vec::new();
        if self.title.trim().is_empty() {
            violations.push(pdfgen_profile::rules::violation(
                pdfgen_profile::rules::MISSING_TITLE,
            ));
        }
        if self.lang.is_empty() {
            violations.push(pdfgen_profile::rules::violation(
                pdfgen_profile::rules::MISSING_LANG,
            ));
        }
        if self.elems.is_empty() {
            violations.push(pdfgen_profile::rules::violation(
                pdfgen_profile::rules::NO_TAGGED_CONTENT,
            ));
        }
        let status = if violations.is_empty() {
            pdfgen_profile::Status::Compliant
        } else {
            pdfgen_profile::Status::NotCompliantYet
        };

        // Fonts (4 objects each, ids reserved up front).
        let fonts = std::mem::take(&mut self.fonts);
        for (i, f) in fonts.iter().enumerate() {
            let base = ids::FONT_BASE + (i as u32) * 4;
            emit_stream_font(&mut self, base, f)?;
        }
        self.fonts = fonts;

        // Structure elements (nested: each element's P is its real parent,
        // K holds child refs when children exist, else marked content).
        let elem_first = self.next_obj;
        let elem_ids: Vec<u32> = (0..self.elems.len())
            .map(|i| elem_first + i as u32)
            .collect();
        self.next_obj += self.elems.len() as u32;
        let doc_elem_id = elem_first + self.elems.len() as u32;
        let ns_id = doc_elem_id + 1;
        let root_id = doc_elem_id + 2;
        self.next_obj = root_id + 1;

        // Parent of each element: doc element, or the containing element
        // (children store indexes into elems).
        let mut child_parent: Vec<Option<u32>> = vec![None; self.elems.len()];
        for (ei, rec) in self.elems.iter().enumerate() {
            for &child in &rec.children {
                child_parent[child as usize] = Some(elem_ids[ei]);
            }
        }

        // Build each element's dict outside the iteration borrow.
        let elem_dicts: Vec<Object> = self
            .elems
            .iter()
            .enumerate()
            .map(|(ei, rec)| {
                let mut e = Dict::new();
                e.set("Type", "StructElem");
                e.set("S", Object::Name(Name::new(rec.tag.clone())));
                if self.profile == pdfgen_profile::Profile::PdfUa2 {
                    e.set("NS", Object::Ref(Ref::new(ns_id)));
                }
                e.set(
                    "P",
                    Object::Ref(Ref::new(child_parent[ei].unwrap_or(doc_elem_id))),
                );
                // K: children when nested, else marked content.
                if !rec.children.is_empty() {
                    let kids: Vec<Object> = rec
                        .children
                        .iter()
                        .map(|&c| Object::Ref(Ref::new(elem_ids[c as usize])))
                        .collect();
                    e.set("K", Object::Array(kids));
                } else if rec.pieces.len() == 1 {
                    e.set("Pg", Object::Ref(Ref::new(self.page_ids[rec.pieces[0].0])));
                    e.set("K", i64::from(rec.pieces[0].1));
                } else if !rec.pieces.is_empty() {
                    e.set(
                        "Pg",
                        Object::Ref(
                            rec.pieces
                                .first()
                                .map(|p| self.page_ids[p.0])
                                .map_or(Ref::new(ids::PAGES), Ref::new),
                        ),
                    );
                    let kids: Vec<Object> = rec
                        .pieces
                        .iter()
                        .map(|&(pg, mcid)| {
                            Object::Dict(
                                Dict::new()
                                    .with("Type", "MCR")
                                    .with("Pg", Object::Ref(Ref::new(self.page_ids[pg])))
                                    .with("MCID", i64::from(mcid)),
                            )
                        })
                        .collect();
                    e.set("K", Object::Array(kids));
                }
                if let Some(alt) = &rec.alt {
                    e.set("Alt", PdfString::text(alt));
                }
                // Table-cell Scope attribute (PDF/UA 15-003): an /A
                // attribute array owned by this element.
                if let Some(scope) = &rec.scope {
                    let mut a = Dict::new();
                    a.set("O", "Table");
                    a.set("Scope", scope.as_str());
                    e.set("A", Object::Array(vec![Object::Dict(a)]));
                }
                Object::Dict(e)
            })
            .collect();
        for (obj, &eid) in elem_dicts.iter().zip(&elem_ids) {
            self.write_object(eid, obj)?;
        }

        // Document root element: only TOP-LEVEL (parentless) elements.
        let mut dr = Dict::new();
        dr.set("Type", "StructElem");
        dr.set("S", "Document");
        if self.profile == pdfgen_profile::Profile::PdfUa2 {
            dr.set("NS", Object::Ref(Ref::new(ns_id)));
        }
        dr.set("P", Object::Ref(Ref::new(root_id)));
        let top_level: Vec<Object> = elem_ids
            .iter()
            .enumerate()
            .filter(|(ei, _)| child_parent[*ei].is_none())
            .map(|(_, &eid)| Object::Ref(Ref::new(eid)))
            .collect();
        dr.set("K", Object::Array(top_level));
        self.write_object(doc_elem_id, &Object::Dict(dr))?;

        // Namespace (UA-2 only).
        if self.profile == pdfgen_profile::Profile::PdfUa2 {
            let ns = Dict::new()
                .with("Type", "Namespace")
                .with("NS", PdfString::text("http://iso.org/pdf2/ssn"));
            self.write_object(ns_id, &Object::Dict(ns))?;
        }

        // Parent tree: page i -> MCID-indexed element refs.
        let mut nums: Vec<Object> = Vec::new();
        for (pi, row) in self.mcid_map.iter().enumerate() {
            let arr: Vec<Object> = row
                .iter()
                .map(|o| match o {
                    Some(idx) => Object::Ref(Ref::new(elem_ids[*idx as usize])),
                    None => Object::Null,
                })
                .collect();
            nums.push(Object::Int(pi as i64));
            nums.push(Object::Array(arr));
        }
        let mut str_root = Dict::new();
        str_root.set("Type", "StructTreeRoot");
        str_root.set("K", Object::Ref(Ref::new(doc_elem_id)));
        if self.profile == pdfgen_profile::Profile::PdfUa2 {
            str_root.set(
                "Namespaces",
                Object::Array(vec![Object::Ref(Ref::new(ns_id))]),
            );
        }
        str_root.set(
            "ParentTree",
            Object::Dict(Dict::new().with("Nums", Object::Array(nums))),
        );
        str_root.set("ParentTreeNextKey", self.page_ids.len() as i64);
        self.write_object(root_id, &Object::Dict(str_root))?;

        // Pages node.
        let pages_obj = Dict::new()
            .with("Type", "Pages")
            .with(
                "Kids",
                Object::Array(
                    self.page_ids
                        .iter()
                        .map(|&r| Object::Ref(Ref::new(r)))
                        .collect(),
                ),
            )
            .with("Count", self.page_ids.len() as i64);
        self.write_object(ids::PAGES, &Object::Dict(pages_obj))?;

        // Metadata (XMP).
        let meta = pdfgen_profile::Metadata {
            title: Some(self.title.clone()),
            lang: Some(self.lang.clone()),
            authors: Vec::new(),
            description: None,
        };
        let xmp_bytes =
            pdfgen_profile::xmp::build(&meta, self.profile.ua_part(), violations.is_empty());
        let xmp = Stream {
            dict: Dict::new().with("Type", "Metadata").with("Subtype", "XML"),
            data: xmp_bytes,
        };
        self.write_object(ids::METADATA, &Object::Stream(xmp))?;

        // Catalog.
        let mut cat = Dict::new();
        cat.set("Type", "Catalog");
        cat.set("Pages", Object::Ref(Ref::new(ids::PAGES)));
        if !self.lang.is_empty() {
            cat.set("Lang", PdfString::text(&self.lang));
        }
        cat.set("MarkInfo", Object::Dict(Dict::new().with("Marked", true)));
        cat.set("StructTreeRoot", Object::Ref(Ref::new(root_id)));
        cat.set(
            "ViewerPreferences",
            Object::Dict(Dict::new().with("DisplayDocTitle", true)),
        );
        cat.set("Metadata", Object::Ref(Ref::new(ids::METADATA)));
        self.write_object(ids::CATALOG, &Object::Dict(cat))?;

        // Classic xref + trailer.
        let size = self.next_obj;
        let xref_off = self.cursor;
        let mut x = format!("xref\n0 {size}\n");
        x.push_str("0000000000 65535 f \n");
        for id in 1..size {
            let off = self.xref.get(id as usize).copied().unwrap_or(0);
            if off != 0 {
                x.push_str(&format!("{off:010} 00000 n \n"));
            } else {
                x.push_str("0000000000 65535 f \n");
            }
        }
        x.push_str(&format!(
            "trailer\n<< /Size {size} /Root {} 0 R >>\nstartxref\n{xref_off}\n%%EOF\n",
            ids::CATALOG
        ));
        self.out.write_all(x.as_bytes())?;
        self.out.flush()?;

        Ok(pdfgen_profile::SaveReport {
            profile: self.profile,
            status,
            violations,
            human_review: vec![
                "09-001: Confirm tags are in logical reading order".into(),
                "06-004: Confirm the dc:title clearly identifies the document".into(),
                "11-007: Confirm the natural language declared is appropriate".into(),
            ],
        })
    }
}

/// Emit the 4 font objects at the reserved ids.
fn emit_stream_font(
    w: &mut StreamWriter,
    base: u32,
    f: &pdfgen_font::LoadedFont,
) -> Result<(), StreamError> {
    let scale = f64::from(f.units_per_em);
    let widths: Vec<Object> = (0u8..=255)
        .map(|b| match f.width_for_byte(b) {
            Some(v) => Object::Int((f64::from(v) * 1000.0 / scale).round() as i64),
            None => Object::Int(0),
        })
        .collect();

    let fdict = base;
    let fdesc = base + 1;
    let ffile = base + 2;
    let ftouni = base + 3;

    // FontFile2.
    let ff = Stream {
        dict: Dict::new().with("Length1", f.raw.len() as i64),
        data: f.raw.clone(),
    };
    w.write_object(ffile, &Object::Stream(ff))?;

    // ToUnicode.
    let tu = Stream {
        dict: Dict::new(),
        data: crate::tounicode::build_winansi(),
    };
    w.write_object(ftouni, &Object::Stream(tu))?;

    // Descriptor.
    let to_thousandths = |v: f64| {
        Object::Real(
            pdfgen_core::fmt_real(v * 1000.0 / scale)
                .parse::<f64>()
                .unwrap_or(0.0)
                .into(),
        )
    };
    let fd = Dict::new()
        .with("Type", "FontDescriptor")
        .with(
            "FontName",
            Object::Name(Name::new(f.postscript_name.clone())),
        )
        .with("Flags", f.descriptor_flags())
        .with(
            "FontBBox",
            Object::Array(vec![
                to_thousandths(f64::from(f.bbox[0])),
                to_thousandths(f64::from(f.bbox[1])),
                to_thousandths(f64::from(f.bbox[2])),
                to_thousandths(f64::from(f.bbox[3])),
            ]),
        )
        .with("ItalicAngle", 0)
        .with("Ascent", to_thousandths(f64::from(f.ascent)))
        .with("Descent", to_thousandths(f64::from(f.descent)))
        .with("CapHeight", to_thousandths(f64::from(f.cap_height)))
        .with("StemV", 80)
        .with("FontFile2", Object::Ref(Ref::new(ffile)));
    w.write_object(fdesc, &Object::Dict(fd))?;

    // Font dict.
    let fobj = Dict::new()
        .with("Type", "Font")
        .with("Subtype", "TrueType")
        .with(
            "BaseFont",
            Object::Name(Name::new(f.postscript_name.clone())),
        )
        .with("FirstChar", 0)
        .with("LastChar", 255)
        .with("Widths", Object::Array(widths))
        .with("FontDescriptor", Object::Ref(Ref::new(fdesc)))
        .with("Encoding", "WinAnsiEncoding")
        .with("ToUnicode", Object::Ref(Ref::new(ftouni)));
    w.write_object(fdict, &Object::Dict(fobj))?;
    Ok(())
}

/// Serialize one dictionary entry (name + value) for the manual stream head.
fn serialize_entry(k: &Name, v: &Object) -> String {
    let mut s = String::new();
    let mut name = String::from("/");
    for b in k.0.bytes() {
        if (0x21..=0x7e).contains(&b)
            && !matches!(
                b,
                b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
            )
        {
            name.push(b as char);
        } else {
            name.push_str(&format!("#{b:02X}"));
        }
    }
    s.push_str(&name);
    s.push(' ');
    pdfgen_core::write_object(&mut s, v);
    s
}

/// Pack one measured word into the current line (greedy wrap).
#[allow(clippy::too_many_arguments)]
fn pack_word(
    lines: &mut Vec<String>,
    cur: &mut String,
    cur_w: &mut f64,
    pending_space: &mut bool,
    space_w: f64,
    w: f64,
    max_w: f64,
    word: &str,
) {
    let need = if *pending_space {
        *cur_w + space_w + w
    } else {
        w
    };
    if !cur.is_empty() && need > max_w {
        lines.push(std::mem::take(cur));
        *cur_w = 0.0;
        *pending_space = false;
    }
    if *pending_space && !cur.is_empty() {
        cur.push(' ');
        *cur_w += space_w;
    }
    cur.push_str(word);
    *cur_w += w;
    *pending_space = true;
}

/// Width of a whole word in points, measured once (glyph units -> pt).
fn word_width_pt(f: &pdfgen_font::LoadedFont, word: &str, size: f64, scale: f64) -> f64 {
    let mut total = 0.0f64;
    for b in word.bytes() {
        if let Some(w) = f.width_for_byte(b) {
            total += f64::from(w) / scale * size;
        }
    }
    total
}

/// Append escaped literal-string bytes into the scratch buffer.
fn push_escaped(out: &mut String, bytes: &[u8]) {
    pdfgen_core::escape_bytes_into(out, bytes);
}

/// DEFLATE-compress a page stream (zlib wrapper, as PDF FlateDecode wants).
pub(crate) fn flate_compress(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write as _;
    let mut enc = ZlibEncoder::new(Vec::with_capacity(data.len() / 2), Compression::fast());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_else(|_| data.to_vec())
}

/// Emit one image XObject (RGB raw or JPEG DCTDecode pass-through).
fn emit_image_xobject(
    w: &mut StreamWriter,
    id: u32,
    img: &crate::image::Image,
) -> Result<(), StreamError> {
    let mut d = Dict::new();
    d.set("Type", "XObject");
    d.set("Subtype", "Image");
    d.set("Width", img.w as i64);
    d.set("Height", img.h as i64);
    d.set("ColorSpace", "DeviceRGB");
    d.set("BitsPerComponent", 8);
    let stream = match &img.kind {
        crate::image::ImageKind::Rgb(rgb) => Stream {
            dict: d,
            data: rgb.clone(),
        },
        crate::image::ImageKind::Jpeg(bytes) => {
            d.set("Filter", "DCTDecode");
            Stream {
                dict: d,
                data: bytes.clone(),
            }
        }
    };
    w.write_object(id, &Object::Stream(stream))
}

/// Locate the default font file for streaming documents.
fn default_font_path() -> Result<String, StreamError> {
    let dir = env!("CARGO_MANIFEST_DIR").to_string() + "/../../fonts";
    let p = format!("{dir}/vendor/liberation/LiberationSans-Regular.ttf");
    if std::path::Path::new(&p).exists() {
        return Ok(p);
    }
    let alt = "/System/Library/Fonts/Supplemental/Arial.ttf";
    if std::path::Path::new(alt).exists() {
        return Ok(alt.into());
    }
    Err(StreamError::Font("no default font available".into()))
}
