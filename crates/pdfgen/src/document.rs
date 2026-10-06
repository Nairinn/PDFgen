//! The end-to-end document: fonts, pages, structure tree, XMP and save.

use crate::image::{Image, ImageKind};
use crate::page::PageData;
use crate::stream::flate_compress;
use crate::structure::Node;
use crate::tounicode;
use pdfgen_core::{Dict, Name, Object, PdfString, Real, Ref, Stream};
use pdfgen_font::LoadedFont;
use pdfgen_fonts::FontRegistry;
use pdfgen_profile::{xmp, Metadata, Profile, SaveReport, Status, Violation};

/// A complete PDF/UA document under construction.
pub struct Document {
    /// Conformance target.
    pub profile: Profile,
    /// Required PDF/UA metadata.
    pub meta: Metadata,
    /// Loaded fonts, by index.
    pub(crate) fonts: Vec<LoadedFont>,
    /// Name-resolution registry (built-in catalog + system fonts).
    pub(crate) registry: FontRegistry,
    /// Font substitutions that happened (family asked -> family used).
    pub(crate) substitutions: Vec<(String, String)>,
    /// Per-page content and structure nodes.
    pub(crate) pages: Vec<PageData>,
    /// Loaded images, by index.
    pub(crate) images: Vec<Image>,
    /// Page size in points (all pages share it for now).
    pub page_size: (f64, f64),
    /// Bookmark entries collected from headings:
    /// (level, text, page, mcid).
    pub(crate) bookmarks: Vec<(u8, String, usize, u32)>,
    /// Flow y-cursor (baseline of the next line) carried across flow()
    /// calls so consecutive flows continue on the same page.
    pub(crate) flow_y: Option<f64>,
    /// Index of the page the flow cursor is on.
    pub(crate) flow_page: usize,
    /// Fonts that needed CID (Type0) encoding: font index -> used
    /// (char, glyph id) pairs.
    pub(crate) cid_fonts:
        std::collections::BTreeMap<usize, std::collections::BTreeSet<(char, u16)>>,
    /// WinAnsi bytes actually drawn per font (for subsetting).
    pub(crate) winansi_used: std::collections::BTreeMap<usize, [bool; 256]>,
    /// Generate the document outline (bookmarks) from headings.
    pub(crate) want_outline: bool,
    /// Form fields recorded by the flow API, emitted as AcroForm at save.
    pub(crate) fields: Vec<crate::form::FieldSpec>,
}

impl Document {
    /// New document targeting a profile.
    pub fn new(profile: Profile) -> Self {
        Document {
            profile,
            meta: Metadata::default(),
            fonts: Vec::new(),
            registry: FontRegistry::new(),
            substitutions: Vec::new(),
            pages: Vec::new(),
            images: Vec::new(),
            page_size: (612.0, 792.0),
            bookmarks: Vec::new(),
            flow_y: None,
            flow_page: 0,
            cid_fonts: Default::default(),
            winansi_used: Default::default(),
            want_outline: true,
            fields: Vec::new(),
        }
    }

    /// Set the document title (PDF/UA requirement).
    pub fn title(&mut self, t: &str) -> &mut Self {
        self.meta.title = Some(t.into());
        self
    }

    /// Set the primary natural language (PDF/UA requirement).
    pub fn lang(&mut self, l: &str) -> &mut Self {
        self.meta.lang = Some(l.into());
        self
    }

    /// Load and register an embedded font (TTF/OTF) by file path.
    pub fn load_font(&mut self, path: &str) -> Result<usize, pdfgen_font::FontError> {
        let f = LoadedFont::load(path)?;
        self.fonts.push(f);
        Ok(self.fonts.len() - 1)
    }

    // --- Drawing-kit surface (used by pdfgen-draw) ----------------------

    /// Append an empty page sized `w` x `h` and return its index. Used by
    /// the engineering drawing kit to start a custom-sized sheet.
    pub fn add_draw_page(&mut self, w: f64, h: f64) -> usize {
        self.pages.push(crate::page::PageData::explicit(w, h));
        self.pages.len() - 1
    }

    /// Begin an artifact on the given page (decoration; skipped by screen
    /// readers). Pair with [`Document::end_artifact`].
    pub fn begin_artifact(&mut self, page: usize, subtype: &str) {
        self.pages[page].content.begin_artifact(subtype);
    }

    /// End the innermost artifact on the page.
    pub fn end_artifact(&mut self, page: usize) {
        self.pages[page].content.end_artifact();
    }

    /// Begin a tagged run on the page; returns its MCID.
    pub fn begin_tag(&mut self, page: usize, tag: &str) -> u32 {
        self.pages[page].content.begin_tag(tag)
    }

    /// End the innermost tagged run on the page.
    pub fn end_tag(&mut self, page: usize) {
        self.pages[page].content.end_tag();
    }

    /// Append raw content-stream operators to the page (inside a tag or
    /// artifact, per PDF/UA).
    pub fn raw_ops(&mut self, page: usize, ops: &str) {
        self.pages[page].content.raw_ops(ops);
    }

    /// Attach a completed structure node to the page.
    pub fn push_node(&mut self, page: usize, node: Node) {
        self.pages[page].nodes.push(node);
    }

    /// Load a font by family name ("Liberation Sans", "Helvetica", "Arial",
    /// a system font, …) and style ("Regular", "Bold", "Italic",
    /// "Bold Italic"). Standard-14 names resolve to embedded look-alikes.
    /// Unknown names fall back to Liberation Sans and record a
    /// substitution in the save report.
    pub fn font(&mut self, family: &str, style: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let (f, resolved) = self.registry.load(family, style)?;
        if resolved.substituted
            && resolved.family.to_ascii_lowercase() != family.to_ascii_lowercase()
        {
            self.substitutions
                .push((family.to_string(), resolved.family.clone()));
        }
        self.fonts.push(f);
        Ok(self.fonts.len() - 1)
    }

    /// Register a custom font file under a family name so `font()` finds it.
    pub fn register_font(&mut self, family: &str, style: &str, path: &str) {
        self.registry.register(family, style, path);
    }

    /// Register an image for later drawing; returns the resource name.
    pub fn register_image(&mut self, img: &Image) -> String {
        self.images.push(img.clone());
        format!("Im{}", self.images.len() - 1)
    }

    /// Begin an explicitly placed page; content is placed via the returned
    /// builder. For flowing text use [`Document::flow`] instead.
    pub fn add_page(&mut self, w: f64, h: f64) -> crate::page::Page<'_> {
        self.pages.push(crate::page::PageData::explicit(w, h));
        crate::page::Page::new(self, self.pages.len() - 1, w, h)
    }

    /// Start (or resume) flowing content. The flow wraps text to the page
    /// width and starts new pages automatically as needed. Consecutive
    /// flow() calls continue on the same page; a page is only created
    /// when the document has none yet.
    pub fn flow(&mut self) -> crate::flow::Flow<'_> {
        // Resume only when the last page belongs to the flow engine;
        // explicit builder pages are never resumed by flows.
        let resume = self.pages.last().is_some_and(|p| p.from_flow);
        if !resume {
            self.pages.push(crate::page::PageData::letter());
        }
        crate::flow::Flow::new(self)
    }

    // --- One-shot flow helpers (bindings-friendly: no borrow escapes) ---

    /// Flow a heading; see [`Flow::heading`].
    pub fn flow_heading(&mut self, level: u8, text: &str) -> Result<(), pdfgen_font::FontError> {
        self.flow().heading(level, text)
    }

    /// Flow a paragraph; see [`Flow::paragraph`].
    pub fn flow_paragraph(&mut self, text: &str) -> Result<(), pdfgen_font::FontError> {
        self.flow().paragraph(text)
    }

    /// Flow a paragraph in a specific font; see [`Flow::paragraph_in`].
    pub fn flow_paragraph_in(
        &mut self,
        font: usize,
        size: f64,
        text: &str,
    ) -> Result<(), pdfgen_font::FontError> {
        self.flow().paragraph_in(font, size, text)
    }

    /// Flow a bullet list; see [`Flow::bullet_list`].
    pub fn flow_bullet_list(&mut self, items: &[&str]) -> Result<(), pdfgen_font::FontError> {
        self.flow().bullet_list(items)
    }

    /// Flow a table; see [`Flow::table`].
    pub fn flow_table(
        &mut self,
        header: &[&str],
        rows: &[Vec<&str>],
        widths: &[f64],
    ) -> Result<(), pdfgen_font::FontError> {
        self.flow().table(header, rows, widths)
    }

    /// Load an image file and flow it as a figure; see [`Flow::figure`].
    pub fn flow_figure(
        &mut self,
        path: &str,
        alt: &str,
        width: f64,
        height: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let img = crate::image::Image::load(path)?;
        self.flow().figure(&img, alt, width, height)?;
        Ok(())
    }

    /// Flow a page header artifact; see [`Flow::header`].
    pub fn flow_header(&mut self, text: &str) -> Result<(), pdfgen_font::FontError> {
        self.flow().header(text)
    }

    /// Flow a page footer artifact; see [`Flow::footer`].
    pub fn flow_footer(&mut self, text: &str) -> Result<(), pdfgen_font::FontError> {
        self.flow().footer(text)
    }

    /// Compute machine-check violations for the current state.
    fn violations(&self) -> Vec<Violation> {
        let mut v = Vec::new();
        if self
            .meta
            .title
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            v.push(pdfgen_profile::rules::violation(
                pdfgen_profile::rules::MISSING_TITLE,
            ));
        }
        if self.meta.lang.is_none() {
            v.push(pdfgen_profile::rules::violation(
                pdfgen_profile::rules::MISSING_LANG,
            ));
        }
        if self.pages.iter().all(|p| p.nodes.is_empty()) {
            v.push(pdfgen_profile::rules::violation(
                pdfgen_profile::rules::NO_TAGGED_CONTENT,
            ));
        }
        for pd in &self.pages {
            for node in &pd.nodes {
                node.walk(&mut |n| {
                    if n.tag == "Figure" && n.alt.is_none() && !n.pieces.is_empty() {
                        v.push(Violation {
                            id: "13-004".into(),
                            message: "Figure has no alternative text".into(),
                            fix: "Pass alt text to flow.figure(img, \"description\", …) \
                                  or empty string for decorative images."
                                .into(),
                        });
                    }
                    if n.tag.starts_with('H') && n.text.trim().is_empty() && !n.pieces.is_empty() {
                        v.push(Violation {
                            id: "09-003".into(),
                            message: format!("Heading {} is empty", n.tag),
                            fix: "Headings must contain text.".into(),
                        });
                    }
                    if n.tag == "TH" && n.scope.is_empty() {
                        v.push(Violation {
                            id: "15-003".into(),
                            message: "Header cell without Scope attribute".into(),
                            fix: "flow.table() sets Scope=Column automatically.".into(),
                        });
                    }
                    if n.tag == "Table"
                        && !n
                            .children
                            .iter()
                            .any(|r| r.children.iter().any(|c| c.tag == "TH"))
                    {
                        v.push(Violation {
                            id: "15-001".into(),
                            message: "Table has no header row".into(),
                            fix: "Pass a header row to flow.table(header, rows, widths).".into(),
                        });
                    }
                });
            }
        }
        // Font substitutions do NOT affect compliance (an embedded
        // look-alike satisfies PDF/UA); they are surfaced as review notes.
        v
    }

    /// Serialize the document, write it to `path`, and return the
    /// accessibility report. The file is always written — compliance issues
    /// go into the report, and the file simply does not claim PDF/UA.
    pub fn save(&mut self, path: &str) -> Result<SaveReport, Box<dyn std::error::Error>> {
        // The output directory may not exist yet (fresh clone, CI); a
        // missing parent should never fail the write.
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // 1. Machine checks decide whether the file may claim PDF/UA.
        let violations = self.violations();
        let compliant = violations.is_empty();
        let status = if compliant {
            Status::Compliant
        } else {
            Status::NotCompliantYet
        };

        // 2. Build the object graph.
        let mut doc = pdfgen_core::Document::new();
        let catalog = doc.alloc();
        let pages_obj = doc.alloc();

        // --- Fonts -----------------------------------------------------
        // A font can be used both ways in one document: WinAnsi lines use
        // F<idx>, CID lines use F<idx>cid. Emit both variants.
        // Emit only fonts that were actually used (WinAnsi bytes or CID
        // glyphs); unused faces drop out of the file entirely.
        let mut font_refs: Vec<Option<Ref>> = Vec::new();
        let mut cid_refs: Vec<Option<Ref>> = Vec::new();
        for (i, f) in self.fonts.iter().enumerate() {
            let win_used = self.winansi_used.get(&i);
            let cid_used = self.cid_fonts.get(&i).filter(|s| !s.is_empty());
            if win_used.is_none() && cid_used.is_none() {
                font_refs.push(None);
                cid_refs.push(None);
                continue;
            }
            let r = match win_used {
                Some(used) => Some(Self::emit_font(&mut doc, f, used)),
                None => None,
            };
            font_refs.push(r);
            cid_refs.push(match cid_used {
                Some(used) => Some(Self::emit_font_type0(&mut doc, f, used)),
                None => None,
            });
        }
        let mut font_res = Dict::new();
        for (i, r) in font_refs.iter().enumerate() {
            if let Some(r) = r {
                font_res.set(format!("F{i}"), *r);
            }
        }
        for (i, r) in cid_refs.iter().enumerate() {
            if let Some(r) = r {
                font_res.set(format!("F{i}cid"), *r);
            }
        }

        // --- Images (XObjects) ------------------------------------------
        let mut xobj_res = Dict::new();
        for (i, img) in self.images.iter().enumerate() {
            let xref = doc.alloc();
            let mut d = Dict::new();
            d.set("Type", "XObject");
            d.set("Subtype", "Image");
            d.set("Width", img.w as i64);
            d.set("Height", img.h as i64);
            d.set("ColorSpace", "DeviceRGB");
            d.set("BitsPerComponent", 8);
            match &img.kind {
                ImageKind::Rgb(rgb) => {
                    doc.set_stream(xref, Stream::new(d, rgb.clone()));
                }
                ImageKind::Jpeg(bytes) => {
                    d.set("Filter", "DCTDecode");
                    doc.set_stream(xref, Stream::new(d, bytes.clone()));
                }
            }
            xobj_res.set(format!("Im{i}"), xref);
        }

        // --- Pages -----------------------------------------------------
        let n_pages = self.pages.len();
        let mut page_refs: Vec<Ref> = Vec::with_capacity(n_pages);
        let mut contents_refs: Vec<Ref> = Vec::with_capacity(n_pages);
        for _ in 0..n_pages {
            page_refs.push(doc.alloc());
            contents_refs.push(doc.alloc());
        }

        // --- Structure tree --------------------------------------------
        let struct_root = doc.alloc();
        let doc_elem = doc.alloc();
        let namespace = if self.profile == Profile::PdfUa2 {
            let ns = doc.alloc();
            doc.set(
                ns,
                Object::Dict(
                    Dict::new()
                        .with("Type", "Namespace")
                        .with("NS", PdfString::text("http://iso.org/pdf2/ssn")),
                ),
            );
            Some(ns)
        } else {
            None
        };

        // Recursive emit: allocate refs for children depth-first while
        // building the parent dict, keeping node<->ref pairs in lockstep.
        fn emit(
            doc: &mut pdfgen_core::Document,
            node: &Node,
            r: Ref,
            parent: Ref,
            page_refs: &[Ref],
            namespace: Option<Ref>,
            fallback_page: usize,
            leaf_map: &mut Vec<(usize, u32, Ref)>,
        ) {
            let mut e = Dict::new();
            e.set("Type", "StructElem");
            e.set("S", Object::Name(Name::new(node.tag.clone())));
            if let Some(ns) = namespace {
                e.set("NS", ns);
            }
            e.set("P", parent);
            if node.pieces.is_empty() && !node.children.is_empty() {
                e.set("Pg", page_refs[fallback_page]);
            } else {
                e.set("Pg", page_refs[node.pieces[0].0]);
            }
            if let Some(alt) = &node.alt {
                e.set("Alt", PdfString::text(alt));
            }
            if !node.scope.is_empty() {
                let mut a = Dict::new();
                a.set("O", "Table");
                a.set("Scope", node.scope.as_str());
                e.set("A", Object::Array(vec![Object::Dict(a)]));
            }
            for &(pgi, mcid) in &node.pieces {
                leaf_map.push((pgi, mcid, r));
            }
            if !node.children.is_empty() {
                let child_refs: Vec<Ref> = node.children.iter().map(|_| doc.alloc()).collect();
                e.set(
                    "K",
                    Object::Array(child_refs.iter().map(|&r| Object::Ref(r)).collect()),
                );
                doc.set(r, Object::Dict(e));
                for (c, cr) in node.children.iter().zip(child_refs) {
                    let child_fallback = c
                        .pieces
                        .first()
                        .map(|p| p.0)
                        .or_else(|| {
                            c.children
                                .first()
                                .and_then(|g| g.pieces.first().map(|p| p.0))
                        })
                        .unwrap_or(fallback_page);
                    emit(
                        doc,
                        c,
                        cr,
                        r,
                        page_refs,
                        namespace,
                        child_fallback,
                        leaf_map,
                    );
                }
            } else if node.pieces.len() == 1 && node.pieces[0].0 == fallback_page {
                e.set("K", i64::from(node.pieces[0].1));
                doc.set(r, Object::Dict(e));
            } else if !node.pieces.is_empty() {
                let kids: Vec<Object> = node
                    .pieces
                    .iter()
                    .map(|&(pgi, mcid)| {
                        Object::Dict(
                            Dict::new()
                                .with("Type", "MCR")
                                .with("Pg", page_refs[pgi])
                                .with("MCID", i64::from(mcid)),
                        )
                    })
                    .collect();
                e.set("K", Object::Array(kids));
                doc.set(r, Object::Dict(e));
            } else {
                doc.set(r, Object::Dict(e));
            }
        }

        let mut leaf_map: Vec<(usize, u32, Ref)> = Vec::new();
        let mut top_refs: Vec<Vec<Ref>> = Vec::with_capacity(n_pages);
        for pd in &self.pages {
            top_refs.push(pd.nodes.iter().map(|_| doc.alloc()).collect());
        }
        for (pi, pd) in self.pages.iter().enumerate() {
            for (ni, node) in pd.nodes.iter().enumerate() {
                let r = top_refs[pi][ni];
                let fallback = node
                    .pieces
                    .first()
                    .map(|p| p.0)
                    .or_else(|| {
                        node.children
                            .first()
                            .and_then(|c| c.pieces.first().map(|p| p.0))
                    })
                    .unwrap_or(pi);
                emit(
                    &mut doc,
                    node,
                    r,
                    doc_elem,
                    &page_refs,
                    namespace,
                    fallback,
                    &mut leaf_map,
                );
            }
        }

        // Root element: /Document.
        let mut dr = Dict::new();
        dr.set("Type", "StructElem");
        dr.set("S", "Document");
        if let Some(ns) = namespace {
            dr.set("NS", ns);
        }
        dr.set("P", struct_root);
        let mut kids: Vec<Object> = Vec::new();
        for pd_refs in &top_refs {
            for &r in pd_refs {
                kids.push(Object::Ref(r));
            }
        }
        dr.set("K", Object::Array(kids));
        doc.set(doc_elem, Object::Dict(dr));

        // Parent tree: page i -> MCID-indexed element refs (leaf_map holds
        // every (page, mcid, element ref) triple).
        let mut nums: Vec<Object> = Vec::new();
        for pi in 0..n_pages {
            let n_mcids = self.pages[pi].content.mcid_count();
            let mut by_mcid: Vec<Option<Ref>> = vec![None; n_mcids as usize];
            for &(pgi, mcid, r) in &leaf_map {
                if pgi == pi && (mcid as usize) < by_mcid.len() {
                    by_mcid[mcid as usize] = Some(r);
                }
            }
            let arr: Vec<Object> = by_mcid
                .into_iter()
                .map(|r| match r {
                    Some(r) => Object::Ref(r),
                    None => Object::Null,
                })
                .collect();
            nums.push(Object::Int(pi as i64));
            nums.push(Object::Array(arr));
        }
        let mut str_root = Dict::new();
        str_root.set("Type", "StructTreeRoot");
        str_root.set("K", doc_elem);
        if let Some(ns) = namespace {
            str_root.set("Namespaces", Object::Array(vec![Object::Ref(ns)]));
        }
        str_root.set(
            "ParentTree",
            Object::Dict(Dict::new().with("Nums", Object::Array(nums))),
        );
        str_root.set("ParentTreeNextKey", n_pages as i64);
        doc.set(struct_root, Object::Dict(str_root));

        // --- Page objects -----------------------------------------------
        // Each page carries its own size (explicit pages can differ).
        for pi in 0..n_pages {
            let (pw, ph) = self.pages[pi].size;
            let mut res = Dict::new();
            if !font_res.is_empty() {
                res.set("Font", Object::Dict(font_res.clone()));
            }
            if !xobj_res.is_empty() {
                res.set("XObject", Object::Dict(xobj_res.clone()));
            }
            let mut pg = Dict::new();
            pg.set("Type", "Page");
            pg.set("Parent", pages_obj);
            pg.set(
                "MediaBox",
                Object::Array(vec![
                    Object::Int(0),
                    Object::Int(0),
                    Object::Real(pw.into()),
                    Object::Real(ph.into()),
                ]),
            );
            pg.set("Resources", Object::Dict(res));
            pg.set("Contents", contents_refs[pi]);
            pg.set("StructParents", pi as i64);
            pg.set("Tabs", "S");
            doc.set(page_refs[pi], Object::Dict(pg));
            // Non-destructive: the page content stays intact so the
            // document can be saved again.
            let content_bytes = self.pages[pi].content.finish_ref();
            doc.set_stream(contents_refs[pi], Stream::new(Dict::new(), content_bytes));
        }

        doc.set(
            pages_obj,
            Object::Dict(
                Dict::new()
                    .with("Type", "Pages")
                    .with(
                        "Kids",
                        Object::Array(page_refs.iter().map(|&r| Object::Ref(r)).collect()),
                    )
                    .with("Count", n_pages as i64),
            ),
        );

        // --- Outline (bookmarks) ----------------------------------------
        let outlines_ref = if self.want_outline && !self.bookmarks.is_empty() {
            let outlines_root = doc.alloc();
            let item_refs: Vec<Ref> = self.bookmarks.iter().map(|_| doc.alloc()).collect();

            // Nesting: each item's parent is the nearest previous entry
            // with a smaller level (classic outline construction).
            let mut parent_of: Vec<Option<usize>> = vec![None; self.bookmarks.len()];
            let mut last_at_level: [Option<usize>; 7] = [None; 7];
            for (i, &(level, _, _, _)) in self.bookmarks.iter().enumerate() {
                let lvl = level.min(6) as usize;
                let mut parent = None;
                for l in (0..lvl).rev() {
                    if last_at_level[l].is_some() {
                        parent = last_at_level[l];
                        break;
                    }
                }
                parent_of[i] = parent;
                last_at_level[lvl] = Some(i);
                for l in (lvl + 1)..7 {
                    last_at_level[l] = None;
                }
            }

            // Sibling chains: Prev/Next link only items that share a
            // parent (per-item O(1) via a per-parent child list).
            let mut children_of: Vec<Vec<usize>> = vec![Vec::new(); self.bookmarks.len()];
            let mut root_children: Vec<usize> = Vec::new();
            for (i, p) in parent_of.iter().enumerate() {
                match p {
                    Some(p) => children_of[*p].push(i),
                    None => root_children.push(i),
                }
            }

            for (i, &(_level, ref title, page, _mcid)) in self.bookmarks.iter().enumerate() {
                let mut item = Dict::new();
                item.set("Title", PdfString::text(title));
                item.set(
                    "Parent",
                    match parent_of[i] {
                        Some(p) => Object::Ref(item_refs[p]),
                        None => Object::Ref(outlines_root),
                    },
                );
                if self.profile == Profile::PdfUa2 {
                    // UA-2: intra-document destinations must be structure
                    // destinations (ISO 32000-2 12.3.2.3): point at the
                    // heading's own struct element.
                    let elem_ref = leaf_map
                        .iter()
                        .find(|&&(lp, lm, _r)| lp == page && lm == _mcid)
                        .map(|&(_, _, r)| r);
                    if let Some(r) = elem_ref {
                        // veraPDF/PDF 2.0: a dict destination is a
                        // structure destination when it has an /SD key.
                        let mut sd = Dict::new();
                        sd.set("SD", Object::Array(vec![Object::Ref(r)]));
                        item.set("Dest", Object::Dict(sd));
                    } else {
                        // Fallback to the classic explicit destination.
                        item.set(
                            "Dest",
                            Object::Array(vec![
                                Object::Ref(page_refs[page.min(n_pages - 1)]),
                                Object::Name(Name::new("XYZ")),
                                Object::Null,
                                Object::Null,
                                Object::Null,
                            ]),
                        );
                    }
                } else {
                    item.set(
                        "Dest",
                        Object::Array(vec![
                            Object::Ref(page_refs[page.min(n_pages - 1)]),
                            Object::Name(Name::new("XYZ")),
                            Object::Null,
                            Object::Null,
                            Object::Null,
                        ]),
                    );
                }
                // Siblings only: link to the neighbors that share OUR parent.
                let sibs = match parent_of[i] {
                    Some(p) => &children_of[p],
                    None => &root_children,
                };
                if let Some(pos) = sibs.iter().position(|&s| s == i) {
                    if pos > 0 {
                        item.set("Prev", Object::Ref(item_refs[sibs[pos - 1]]));
                    }
                    if pos + 1 < sibs.len() {
                        item.set("Next", Object::Ref(item_refs[sibs[pos + 1]]));
                    }
                }
                // First/Last child + descendant Count (visible items).
                let kids = &children_of[i];
                if let (Some(&f), Some(&l)) = (kids.first(), kids.last()) {
                    item.set("First", Object::Ref(item_refs[f]));
                    item.set("Last", Object::Ref(item_refs[l]));
                    item.set("Count", count_descendants(i, &children_of) as i64);
                }
                doc.set(item_refs[i], Object::Dict(item));
            }

            let mut outl = Dict::new();
            outl.set("Type", "Outlines");
            if let (Some(&f), Some(&l)) = (root_children.first(), root_children.last()) {
                outl.set("First", Object::Ref(item_refs[f]));
                outl.set("Last", Object::Ref(item_refs[l]));
                // Count: total open items across the tree.
                let total: usize = root_children
                    .iter()
                    .map(|&r| 1 + count_descendants(r, &children_of))
                    .sum();
                outl.set("Count", total as i64);
            }
            doc.set(outlines_root, Object::Dict(outl));
            Some(outlines_root)
        } else {
            None
        };

        // --- Metadata stream --------------------------------------------
        let xmp_bytes = xmp::build(&self.meta, self.profile.ua_part(), compliant);
        let metadata = doc.alloc();
        doc.set_stream(
            metadata,
            Stream::new(
                Dict::new().with("Type", "Metadata").with("Subtype", "XML"),
                xmp_bytes,
            ),
        );

        // --- Catalog ------------------------------------------------------
        let mut cat = Dict::new();
        cat.set("Type", "Catalog");
        cat.set("Pages", pages_obj);
        if let Some(outlines) = outlines_ref {
            cat.set("Outlines", outlines);
            cat.set("PageMode", "UseOutlines");
        }
        // AcroForm with accessible (/TU) fields.
        if !self.fields.is_empty() {
            let field_refs: Vec<Ref> = self
                .fields
                .iter()
                .map(|spec| {
                    crate::form::emit_field(&mut doc, spec, page_refs[spec.page.min(n_pages - 1)])
                })
                .collect();
            let af = crate::form::emit_acroform(&mut doc, &field_refs);
            cat.set("AcroForm", af);
        }
        if let Some(lang) = &self.meta.lang {
            cat.set("Lang", PdfString::text(lang));
        }
        cat.set("MarkInfo", Object::Dict(Dict::new().with("Marked", true)));
        cat.set("StructTreeRoot", struct_root);
        cat.set(
            "ViewerPreferences",
            Object::Dict(Dict::new().with("DisplayDocTitle", true)),
        );
        cat.set("Metadata", metadata);
        doc.set(catalog, Object::Dict(cat));

        // 3. Serialize and write.
        let bytes = doc.serialize(self.profile.pdf_version(), catalog)?;
        std::fs::write(path, bytes)?;

        // 4. Report.
        let mut human_review = vec![
            "09-001: Confirm tags are in logical reading order".into(),
            "06-004: Confirm the dc:title clearly identifies the document".into(),
            "11-007: Confirm the natural language declared is appropriate".into(),
        ];
        for (asked, used) in &self.substitutions {
            human_review.push(format!(
                "pdfgen: Font \"{asked}\" was not available; \"{used}\" was used instead"
            ));
        }
        Ok(SaveReport {
            profile: self.profile,
            status,
            violations,
            human_review,
        })
    }

    /// Emit a Type0 (composite) font with a CIDFontType2 descendant for
    /// text that WinAnsi cannot encode. `used` carries the (char, glyph)
    /// pairs actually drawn; widths and ToUnicode cover exactly those.
    fn emit_font_type0(
        doc: &mut pdfgen_core::Document,
        f: &LoadedFont,
        used: &std::collections::BTreeSet<(char, u16)>,
    ) -> Ref {
        let fdict = doc.alloc(); // Type0
        let desc = doc.alloc(); // CIDFontType2
        let fdesc = doc.alloc(); // FontDescriptor
        let ffile = doc.alloc(); // FontFile2
        let ftouni = doc.alloc();

        // Font file: CFF/OTF embeds as FontFile3 (Subtype /OpenType, no
        // Length1); TrueType subsets to the used glyphs (plus composite
        // components) and embeds as FontFile2. Subsetting returns an
        // old->new GID remap; CIDs in the content stream stay OLD gids
        // and a CIDToGIDMap stream translates them to the new ones.
        let mut cid_to_gid: Option<Vec<u8>> = None;
        if f.is_cff {
            doc.set_stream(
                ffile,
                Stream::new(Dict::new().with("Subtype", "OpenType"), f.raw.clone()),
            );
        } else {
            let used_gids: Vec<u16> = used.iter().map(|&(_, g)| g).collect();
            let mut program = f.raw.clone();
            if let Some((sub, remap)) = pdfgen_font::subset::subset_true_type(&f.raw, &used_gids) {
                // CID -> new GID table: 2 bytes per CID up to the largest
                // used old gid (sparse entries default to glyph 0).
                let max_old = used_gids.iter().copied().max().unwrap_or(0) as usize;
                let mut map = vec![0u8; (max_old + 1) * 2];
                for &(_, g) in used {
                    let new_gid = remap[g as usize];
                    let off = g as usize * 2;
                    map[off..off + 2].copy_from_slice(&new_gid.to_be_bytes());
                }
                cid_to_gid = Some(map);
                program = sub;
            }
            doc.set_stream(
                ffile,
                Stream::new(Dict::new().with("Length1", program.len() as i64), program),
            );
        }

        // CIDToGIDMap: CIDs are glyph IDs directly (Identity-H), so the
        // standard /Identity name applies — no stream needed. A stream
        // map makes validators re-resolve glyphs and disagree.

        // ToUnicode CMap: 2-byte CID codes only (Identity-H); including
        // 1-byte codes here corrupts the codespace.
        let mut tu = String::new();
        tu.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
        tu.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
        tu.push_str("/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
        tu.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
        let mut entries: Vec<(u32, char)> =
            used.iter().map(|&(ch, g)| (u32::from(g), ch)).collect();
        entries.sort_unstable();
        entries.dedup_by_key(|e| e.0);
        for chunk in entries.chunks(100) {
            tu.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (code, ch) in chunk {
                let unit = u32::from(*ch);
                tu.push_str(&format!("<{code:04X}> <{unit:04X}>\n"));
            }
            tu.push_str("endbfchar\n");
        }
        tu.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        doc.set_stream(ftouni, Stream::new(Dict::new(), tu.into_bytes()));

        // Descriptor (same metrics as the simple font).
        let scale = f64::from(f.units_per_em);
        let to_thousandths = |v: f64| {
            Object::Real(
                pdfgen_core::fmt_real(v * 1000.0 / scale)
                    .parse::<f64>()
                    .unwrap_or(0.0)
                    .into(),
            )
        };
        let mut fd = Dict::new();
        fd.set("Type", "FontDescriptor");
        fd.set(
            "FontName",
            Object::Name(Name::new(f.postscript_name.clone())),
        );
        fd.set("Flags", 4); // symbolic
        fd.set(
            "FontBBox",
            Object::Array(vec![
                to_thousandths(f64::from(f.bbox[0])),
                to_thousandths(f64::from(f.bbox[1])),
                to_thousandths(f64::from(f.bbox[2])),
                to_thousandths(f64::from(f.bbox[3])),
            ]),
        );
        fd.set("ItalicAngle", 0);
        fd.set("Ascent", to_thousandths(f64::from(f.ascent)));
        fd.set("Descent", to_thousandths(f64::from(f.descent)));
        fd.set("CapHeight", to_thousandths(f64::from(f.cap_height)));
        fd.set("StemV", 80);
        // FontFile3 (OpenType/CFF) or FontFile2 (TrueType).
        if f.is_cff {
            fd.set("FontFile3", ffile);
        } else {
            fd.set("FontFile2", ffile);
        }
        doc.set(fdesc, Object::Dict(fd));

        // Descendant CIDFontType2.
        // W: run format [ c_first [w...] c_first [w...] ] — one entry per
        // contiguous GID run, widths in glyph units scaled to 1000/em.
        let mut warray: Vec<Object> = Vec::new();
        {
            let mut gids: Vec<u16> = used.iter().map(|&(_, g)| g).collect();
            gids.sort_unstable();
            gids.dedup();
            let scale = f64::from(f.units_per_em);
            let mut i = 0;
            while i < gids.len() {
                let start = gids[i];
                let mut run: Vec<Object> = Vec::new();
                while i < gids.len() && u32::from(gids[i]) == u32::from(start) + run.len() as u32 {
                    let units = f.glyph_width_units(gids[i]).unwrap_or(0);
                    let w1000 = units as f64 * 1000.0 / scale;
                    run.push(Object::Real(
                        pdfgen_core::fmt_real(w1000)
                            .parse::<f64>()
                            .unwrap_or(0.0)
                            .into(),
                    ));
                    i += 1;
                }
                warray.push(Object::Int(i64::from(start)));
                warray.push(Object::Array(run));
            }
        }
        let mut cid_font = Dict::new();
        cid_font.set("Type", "Font");
        cid_font.set(
            "Subtype",
            if f.is_cff {
                "CIDFontType0"
            } else {
                "CIDFontType2"
            },
        );
        cid_font.set(
            "BaseFont",
            Object::Name(Name::new(f.postscript_name.clone())),
        );
        cid_font.set(
            "CIDSystemInfo",
            Object::Dict(
                Dict::new()
                    .with("Registry", Object::String(PdfString::text("Adobe")))
                    .with("Ordering", Object::String(PdfString::text("Identity")))
                    .with("Supplement", 0),
            ),
        );
        cid_font.set("FontDescriptor", fdesc);
        // CIDToGIDMap: the stream name when the font was subset (CIDs are
        // old gids, the map translates to the subset's new gids);
        // otherwise /Identity since CID == GID.
        match cid_to_gid {
            Some(map) => {
                let cmap_ref = doc.alloc();
                doc.set_stream(
                    cmap_ref,
                    Stream::new(
                        Dict::new().with("Filter", "FlateDecode"),
                        flate_compress(&map),
                    ),
                );
                cid_font.set("CIDToGIDMap", cmap_ref);
            }
            None => {
                cid_font.set("CIDToGIDMap", "Identity");
            }
        }
        // W: [ gid w gid w ... ] pairs (per-glyph widths in glyph units).
        cid_font.set("W", Object::Array(warray));
        doc.set(desc, Object::Dict(cid_font));

        // Type0 font.
        let mut t0 = Dict::new();
        t0.set("Type", "Font");
        t0.set("Subtype", "Type0");
        t0.set(
            "BaseFont",
            Object::Name(Name::new(f.postscript_name.clone())),
        );
        t0.set("Encoding", "Identity-H");
        t0.set("DescendantFonts", Object::Array(vec![Object::Ref(desc)]));
        t0.set("ToUnicode", ftouni);
        doc.set(fdict, Object::Dict(t0));
        fdict
    }

    /// Emit the four objects for one embedded TrueType font; returns the
    /// font dictionary ref.
    fn emit_font(doc: &mut pdfgen_core::Document, f: &LoadedFont, used_bytes: &[bool; 256]) -> Ref {
        let fdict = doc.alloc();
        let fdesc = doc.alloc();
        let ffile = doc.alloc();
        let ftouni = doc.alloc();

        let scale = f64::from(f.units_per_em);
        // /Widths must be in 1000-unit text space, consistent with the
        // embedded font program (veraPDF UA-2 8.4.5.6 checks this).
        let widths: Vec<Object> = (0u8..=255)
            .map(|b| match f.width_for_byte(b) {
                Some(w) => Object::Int((f64::from(w) * 1000.0 / scale).round() as i64),
                None => Object::Int(0),
            })
            .collect();
        let to_thousandths = |v: f64| Object::Real(Real((v * 1000.0 / scale).round()));

        // Subset the program to the used WinAnsi glyphs with the classic
        // simple-font renumbering (glyph for byte b lands at GID b and
        // the cmap maps b -> b), so the dictionary Widths and the
        // embedded program stay consistent. A stable 6-letter subset
        // tag marks the subset font name (ABCDEF+Name, per the spec).
        let mut base_font_name = f.postscript_name.clone();
        let mut program: Vec<u8> = f.raw.clone();
        if !f.is_cff {
            let mut byte_to_gid: [Option<u16>; 256] = [None; 256];
            for (byte, &u) in used_bytes.iter().enumerate() {
                if u {
                    if let Some(unit) = pdfgen_font::winansi_unit_for_byte(byte as u8) {
                        if let Some(ch) = char::from_u32(u32::from(unit)) {
                            byte_to_gid[byte] = f.glyph_index(ch);
                        }
                    }
                }
            }
            if let Some((sub, _)) = pdfgen_font::subset::subset_winansi(&f.raw, &byte_to_gid) {
                // Stable subset tag derived from the font name so
                // repeated saves are reproducible.
                let mut hash: u32 = 5381;
                for b in f.postscript_name.bytes() {
                    hash = hash.wrapping_mul(33).wrapping_add(u32::from(b));
                }
                const A: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
                let tag: String = (0..6)
                    .map(|k| A[((hash >> (k * 4)) % 26) as usize] as char)
                    .collect();
                base_font_name = format!("{tag}+{}", f.postscript_name);
                program = sub;
            }
        }

        // Font program, Flate-compressed.
        let compressed = flate_compress(&program);
        doc.set_stream(
            ffile,
            Stream::new(
                Dict::new()
                    .with("Filter", "FlateDecode")
                    .with("Length1", program.len() as i64),
                compressed,
            ),
        );

        // ToUnicode CMap, Flate-compressed.
        let tu = tounicode::build_winansi();
        doc.set_stream(
            ftouni,
            Stream::new(
                Dict::new().with("Filter", "FlateDecode"),
                flate_compress(&tu),
            ),
        );

        // Descriptor.
        let mut fd = Dict::new();
        fd.set("Type", "FontDescriptor");
        fd.set("FontName", Object::Name(Name::new(base_font_name.clone())));
        fd.set("Flags", f.descriptor_flags());
        fd.set(
            "FontBBox",
            Object::Array(vec![
                to_thousandths(f64::from(f.bbox[0])),
                to_thousandths(f64::from(f.bbox[1])),
                to_thousandths(f64::from(f.bbox[2])),
                to_thousandths(f64::from(f.bbox[3])),
            ]),
        );
        fd.set("ItalicAngle", 0);
        fd.set("Ascent", to_thousandths(f64::from(f.ascent)));
        fd.set("Descent", to_thousandths(f64::from(f.descent)));
        fd.set("CapHeight", to_thousandths(f64::from(f.cap_height)));
        fd.set("StemV", 80);
        fd.set("FontFile2", ffile);
        doc.set(fdesc, Object::Dict(fd));

        // Font dictionary.
        let mut ff = Dict::new();
        ff.set("Type", "Font");
        ff.set("Subtype", "TrueType");
        ff.set("BaseFont", Object::Name(Name::new(base_font_name)));
        ff.set("FirstChar", 0);
        ff.set("LastChar", 255);
        ff.set("Widths", Object::Array(widths));
        ff.set("FontDescriptor", fdesc);
        ff.set("Encoding", "WinAnsiEncoding");
        ff.set("ToUnicode", ftouni);
        doc.set(fdict, Object::Dict(ff));
        fdict
    }
}
/// Number of descendants (children, grandchildren, ...) of `root` in the
/// outline child lists.
fn count_descendants(root: usize, children_of: &[Vec<usize>]) -> usize {
    children_of[root]
        .iter()
        .map(|&c| 1 + count_descendants(c, children_of))
        .sum()
}
