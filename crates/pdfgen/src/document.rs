//! The end-to-end document: fonts, pages, structure tree, XMP and save.

use crate::page::Block;
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
    /// The single content stream (M1: one page).
    pub(crate) content: pdfgen_canvas::Content,
    /// Tagged blocks recorded for the structure tree.
    pub(crate) blocks: Vec<Block>,
    /// Page size in points.
    pub(crate) page_size: (f64, f64),
}

impl Document {
    /// New document targeting a profile.
    pub fn new(profile: Profile) -> Self {
        Document {
            profile,
            meta: Metadata::default(),
            fonts: Vec::new(),
            content: pdfgen_canvas::Content::new(),
            blocks: Vec::new(),
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

    /// Begin a page (M1: single page).
    pub fn add_page(&mut self, w: f64, h: f64) -> crate::page::Page<'_> {
        self.page_size = (w, h);
        crate::page::Page::new(self, w, h)
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
        if self.blocks.is_empty() {
            v.push(Violation {
                id: "01-006".into(),
                message: "No content has been tagged: the structure tree would be empty".into(),
                fix: "Add at least one heading or paragraph before saving.".into(),
            });
        }
        for b in &self.blocks {
            if b.tag.starts_with('H') && b.text.trim().is_empty() {
                v.push(Violation {
                    id: "09-003".into(),
                    message: format!("Heading {} is empty", b.tag),
                    fix: "Headings must contain text.".into(),
                });
            }
        }
        v
    }

    /// Serialize the document, write it to `path`, and return the
    /// accessibility report. The file is always written — compliance issues
    /// go into the report, and the file simply does not claim PDF/UA.
    pub fn save(
        &mut self,
        path: &str,
    ) -> Result<SaveReport, Box<dyn std::error::Error>> {
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
        let content_bytes = std::mem::take(&mut self.content).finish();

        let catalog = doc.alloc();
        let pages = doc.alloc();
        let page = doc.alloc();
        let contents = doc.alloc();

        // --- Fonts -----------------------------------------------------
        let mut font_refs: Vec<Ref> = Vec::new();
        for f in &self.fonts {
            font_refs.push(Self::emit_font(&mut doc, f));
        }

        // --- Structure tree --------------------------------------------
        let struct_root = doc.alloc();
        let doc_elem = doc.alloc();
        // PDF 2.0 (UA-2): elements live in the standard structure namespace,
        // declared on the tree root and referenced by each element.
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
        let elem_refs: Vec<Ref> = self
            .blocks
            .iter()
            .map(|_| doc.alloc())
            .collect();

        for (b, &r) in self.blocks.iter().zip(elem_refs.iter()) {
            let mut e = Dict::new();
            e.set("Type", "StructElem");
            e.set("S", Object::Name(Name::new(b.tag.clone())));
            if let Some(ns) = namespace {
                e.set("NS", ns);
            }
            e.set("P", doc_elem);
            e.set("Pg", page);
            e.set("K", i64::from(b.mcid));
            doc.set(r, Object::Dict(e));
        }

        // Root element: /Document, parent = StructTreeRoot itself.
        let mut dr = Dict::new();
        dr.set("Type", "StructElem");
        dr.set("S", "Document");
        if let Some(ns) = namespace {
            dr.set("NS", ns);
        }
        dr.set("P", struct_root);
        dr.set(
            "K",
            Object::Array(elem_refs.iter().map(|&r| Object::Ref(r)).collect()),
        );
        doc.set(doc_elem, Object::Dict(dr));

        // Parent tree: page's StructParents (0) -> array of element refs
        // indexed by MCID.
        let kids: Vec<Object> = elem_refs.iter().map(|&r| Object::Ref(r)).collect();
        let mut str_root = Dict::new();
        str_root.set("Type", "StructTreeRoot");
        str_root.set("K", doc_elem);
        if let Some(ns) = namespace {
            str_root.set("Namespaces", Object::Array(vec![Object::Ref(ns)]));
        }
        str_root.set(
            "ParentTree",
            Object::Dict(Dict::new().with(
                "Nums",
                Object::Array(vec![Object::Int(0), Object::Array(kids)]),
            )),
        );
        str_root.set("ParentTreeNextKey", 1);
        doc.set(struct_root, Object::Dict(str_root));

        // --- Page and pages --------------------------------------------
        let mut font_res = Dict::new();
        for (i, &r) in font_refs.iter().enumerate() {
            font_res.set(format!("F{i}"), r);
        }
        let (pw, ph) = self.page_size;
        let mut pg = Dict::new();
        pg.set("Type", "Page");
        pg.set("Parent", pages);
        pg.set(
            "MediaBox",
            Object::Array(vec![
                Object::Int(0),
                Object::Int(0),
                Object::Real(pw.into()),
                Object::Real(ph.into()),
            ]),
        );
        pg.set("Resources", Object::Dict(Dict::new().with("Font", font_res)));
        pg.set("Contents", contents);
        pg.set("StructParents", 0);
        pg.set("Tabs", "S");
        doc.set(page, Object::Dict(pg));

        doc.set(
            pages,
            Object::Dict(
                Dict::new()
                    .with("Type", "Pages")
                    .with("Kids", Object::Array(vec![Object::Ref(page)]))
                    .with("Count", 1),
            ),
        );
        doc.set_stream(
            contents,
            Stream::new(Dict::new(), content_bytes),
        );

        // --- Metadata stream --------------------------------------------
        let xmp_bytes = xmp::build(&self.meta, self.profile.ua_part(), compliant);
        let metadata = doc.alloc();
        doc.set_stream(
            metadata,
            Stream::new(
                Dict::new()
                    .with("Type", "Metadata")
                    .with("Subtype", "XML"),
                xmp_bytes,
            ),
        );

        // --- Catalog ------------------------------------------------------
        let mut cat = Dict::new();
        cat.set("Type", "Catalog");
        cat.set("Pages", pages);
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
        let to_thousandths = |v: f64| Object::Real(pdfgen_core::fmt_real(v * 1000.0 / scale).parse::<f64>().unwrap_or(0.0).into());

        // Font file (uncompressed for M1).
        doc.set_stream(
            ffile,
            Stream::new(Dict::new().with("Length1", f.raw.len() as i64), f.raw.clone()),
        );

        // ToUnicode CMap.
        doc.set_stream(
            ftouni,
            Stream::new(Dict::new(), tounicode::build_winansi()),
        );

        // Descriptor.
        let mut fd = Dict::new();
        fd.set("Type", "FontDescriptor");
        fd.set("FontName", Object::Name(Name::new(f.postscript_name.clone())));
        fd.set("Flags", f.descriptor_flags());
        fd.set("FontBBox", Object::Array(vec![
            to_thousandths(f64::from(f.bbox[0])),
            to_thousandths(f64::from(f.bbox[1])),
            to_thousandths(f64::from(f.bbox[2])),
            to_thousandths(f64::from(f.bbox[3])),
        ]));
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
