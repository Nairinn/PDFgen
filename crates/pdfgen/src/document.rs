//! The end-to-end document: fonts, pages, structure tree, XMP and save.

use crate::image::{Image, ImageKind};
use crate::page::PageData;
use crate::structure::Node;
use crate::tounicode;
use pdfgen_core::{Dict, Name, Object, PdfString, Ref, Stream};
use pdfgen_font::LoadedFont;
use pdfgen_profile::{xmp, Metadata, Profile, SaveReport, Status, Violation};

/// A complete PDF/UA document under construction.
pub struct Document {
    /// Conformance target.
    pub profile: Profile,
    /// Required PDF/UA metadata.
    pub meta: Metadata,
    /// Loaded fonts, by index.
    pub(crate) fonts: Vec<LoadedFont>,
    /// Per-page content and structure nodes.
    pub(crate) pages: Vec<PageData>,
    /// Loaded images, by index.
    pub(crate) images: Vec<Image>,
    /// Page size in points (all pages share it for now).
    pub(crate) page_size: (f64, f64),
}

impl Document {
    /// New document targeting a profile.
    pub fn new(profile: Profile) -> Self {
        Document {
            profile,
            meta: Metadata::default(),
            fonts: Vec::new(),
            pages: Vec::new(),
            images: Vec::new(),
            page_size: (612.0, 792.0),
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

    /// Load and register an embedded font (TTF/OTF).
    pub fn load_font(&mut self, path: &str) -> Result<usize, pdfgen_font::FontError> {
        let f = LoadedFont::load(path)?;
        self.fonts.push(f);
        Ok(self.fonts.len() - 1)
    }

    /// Register an image for later drawing; returns the resource name.
    pub fn register_image(&mut self, img: &Image) -> String {
        self.images.push(img.clone());
        format!("Im{}", self.images.len() - 1)
    }

    /// Begin an explicitly placed page; content is placed via the returned
    /// builder. For flowing text use [`Document::flow`] instead.
    pub fn add_page(&mut self, w: f64, h: f64) -> crate::page::Page<'_> {
        self.page_size = (w, h);
        self.pages.push(PageData::default());
        crate::page::Page::new(self, self.pages.len() - 1, w, h)
    }

    /// Start flowing content. The flow wraps text to the page width and
    /// starts new pages automatically as needed.
    pub fn flow(&mut self) -> crate::flow::Flow<'_> {
        self.pages.push(PageData::default());
        self.page_size = (612.0, 792.0);
        crate::flow::Flow::new(self)
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
            v.push(Violation {
                id: "06-003".into(),
                message: "XMP metadata stream does not contain dc:title".into(),
                fix: "Call doc.title(\"…\") with a meaningful document title.".into(),
            });
        }
        if self.meta.lang.is_none() {
            v.push(Violation {
                id: "11-006".into(),
                message: "Natural language for the document cannot be determined".into(),
                fix: "Call doc.lang(\"en-US\") (or the document's language).".into(),
            });
        }
        if self.pages.iter().all(|p| p.nodes.is_empty()) {
            v.push(Violation {
                id: "01-006".into(),
                message: "No content has been tagged: the structure tree would be empty".into(),
                fix: "Add at least one heading or paragraph before saving.".into(),
            });
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
        v
    }

    /// Serialize the document, write it to `path`, and return the
    /// accessibility report. The file is always written — compliance issues
    /// go into the report, and the file simply does not claim PDF/UA.
    pub fn save(&mut self, path: &str) -> Result<SaveReport, Box<dyn std::error::Error>> {
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
        let mut font_refs: Vec<Ref> = Vec::new();
        for f in &self.fonts {
            font_refs.push(Self::emit_font(&mut doc, f));
        }
        let mut font_res = Dict::new();
        for (i, &r) in font_refs.iter().enumerate() {
            font_res.set(format!("F{i}"), r);
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
                    emit(doc, c, cr, r, page_refs, namespace, child_fallback, leaf_map);
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
                emit(&mut doc, node, r, doc_elem, &page_refs, namespace, fallback, &mut leaf_map);
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
        let (pw, ph) = self.page_size;
        for pi in 0..n_pages {
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
            let content_bytes = std::mem::take(&mut self.pages[pi].content).finish();
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
        let human_review = vec![
            "09-001: Confirm tags are in logical reading order".into(),
            "06-004: Confirm the dc:title clearly identifies the document".into(),
            "11-007: Confirm the natural language declared is appropriate".into(),
        ];
        Ok(SaveReport {
            profile: self.profile,
            status,
            violations,
            human_review,
        })
    }

    /// Emit the four objects for one embedded TrueType font; returns the
    /// font dictionary ref.
    fn emit_font(doc: &mut pdfgen_core::Document, f: &LoadedFont) -> Ref {
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
        let to_thousandths = |v: f64| {
            Object::Real(
                pdfgen_core::fmt_real(v * 1000.0 / scale)
                    .parse::<f64>()
                    .unwrap_or(0.0)
                    .into(),
            )
        };

        // Font file (uncompressed for M1).
        doc.set_stream(
            ffile,
            Stream::new(
                Dict::new().with("Length1", f.raw.len() as i64),
                f.raw.clone(),
            ),
        );

        // ToUnicode CMap.
        doc.set_stream(ftouni, Stream::new(Dict::new(), tounicode::build_winansi()));

        // Descriptor.
        let mut fd = Dict::new();
        fd.set("Type", "FontDescriptor");
        fd.set("FontName", Object::Name(Name::new(f.postscript_name.clone())));
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
        ff.set("BaseFont", Object::Name(Name::new(f.postscript_name.clone())));
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
