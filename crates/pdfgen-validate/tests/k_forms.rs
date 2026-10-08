//! Issue #9 regressions: the validator must handle K as array or direct
//! dict, descend THead/TBody row groups, and check page /Annots widgets.

use pdfgen_core::{Dict, Object, PdfString, PdfVersion, Stream};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Minimal tagged UA-1 doc whose structure tree root /K is an ARRAY and
/// whose table is wrapped in THead/TBody groups with a direct-dict TH.
fn build_fixture() -> String {
    let mut doc = pdfgen_core::Document::new();

    let catalog = doc.alloc();
    let pages = doc.alloc();
    let page = doc.alloc();
    let root = doc.alloc();
    let doc_elem = doc.alloc();
    let table = doc.alloc();
    let thead = doc.alloc();
    let tr = doc.alloc();
    let th = doc.alloc();
    let content = doc.alloc();
    let annot = doc.alloc();
    let font = doc.alloc();
    let meta = doc.alloc();

    // Page with content + an annotation with no /TU (must be flagged).
    doc.set(
        page,
        Object::Dict(
            Dict::new()
                .with("Type", "Page")
                .with("Parent", pages)
                .with(
                    "MediaBox",
                    Object::Array(vec![
                        Object::Int(0),
                        Object::Int(0),
                        Object::Int(612),
                        Object::Int(792),
                    ]),
                )
                .with(
                    "Resources",
                    Object::Dict(
                        Dict::new().with("Font", Object::Dict(Dict::new().with("F1", font))),
                    ),
                )
                .with("Contents", Object::Ref(content))
                .with("Annots", Object::Array(vec![Object::Ref(annot)])),
        ),
    );
    doc.set(
        annot,
        Object::Dict(
            Dict::new()
                .with("Type", "Annot")
                .with("Subtype", "Widget")
                .with("FT", "Tx")
                .with("T", PdfString::text("unnamed_field"))
                // No /TU: 28-001 must fire from the /Annots walk.
                .with(
                    "Rect",
                    Object::Array(vec![
                        Object::Int(72),
                        Object::Int(72),
                        Object::Int(220),
                        Object::Int(90),
                    ]),
                ),
        ),
    );

    doc.set(
        content,
        Object::Stream(Stream::new(
            Dict::new(),
            b"BT /F1 12 Tf 72 720 Td (x) Tj ET".to_vec(),
        )),
    );

    // Structure tree: root /K is an ARRAY (issue #9 form), the doc
    // element holds the table, whose rows sit inside THead/TBody groups.
    doc.set(
        root,
        Object::Dict(
            Dict::new()
                .with("Type", "StructTreeRoot")
                // K as an ARRAY of refs (the old walker required one ref).
                .with("K", Object::Array(vec![Object::Ref(doc_elem)]))
                .with("ParentTree", Object::Dict(Dict::new())),
        ),
    );
    doc.set(
        doc_elem,
        Object::Dict(
            Dict::new()
                .with("Type", "StructElem")
                .with("S", "Document")
                .with("P", root)
                .with("K", Object::Array(vec![Object::Ref(table)])),
        ),
    );
    doc.set(
        table,
        Object::Dict(
            Dict::new()
                .with("Type", "StructElem")
                .with("S", "Table")
                .with("P", Object::Ref(doc_elem))
                .with("K", Object::Array(vec![Object::Ref(thead)])),
        ),
    );
    // THead row group wrapping the TR (issue #9: header detection must
    // descend it).
    doc.set(
        thead,
        Object::Dict(
            Dict::new()
                .with("Type", "StructElem")
                .with("S", "THead")
                .with("P", table)
                .with(
                    "K",
                    Object::Array(vec![
                        Object::Ref(tr),
                        // A direct-dict TD child too (issue #9: direct dicts
                        // must walk, not just refs).
                        Object::Dict(Dict::new().with("Type", "StructElem").with("S", "TD")),
                    ]),
                ),
        ),
    );
    doc.set(
        tr,
        Object::Dict(
            Dict::new()
                .with("Type", "StructElem")
                .with("S", "TR")
                .with("P", thead)
                .with("K", Object::Array(vec![Object::Ref(th)])),
        ),
    );
    // TH with Scope: passes 15-003.
    doc.set(
        th,
        Object::Dict(
            Dict::new()
                .with("Type", "StructElem")
                .with("S", "TH")
                .with("P", tr)
                .with("K", Object::Int(0))
                .with(
                    "A",
                    Object::Array(vec![Object::Dict(
                        Dict::new().with("O", "Table").with("Scope", "Column"),
                    )]),
                ),
        ),
    );

    // Fonts: one simple embedded-less TrueType dict (the validator does
    // not check fonts, but the file should parse).
    doc.set(
        font,
        Object::Dict(
            Dict::new()
                .with("Type", "Font")
                .with("Subtype", "Type1")
                .with("BaseFont", "Helvetica")
                .with("Encoding", "WinAnsiEncoding"),
        ),
    );

    // XMP metadata stream (so 06-x checks pass).
    let xmp = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
        xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
        xmlns:dc="http://purl.org/dc/elements/1.1/">
      <pdfuaid:part>1</pdfuaid:part>
      <dc:title><rdf:Alt><rdf:li>Fixture</rdf:li></rdf:Alt></dc:title>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;
    doc.set(
        meta,
        Object::Stream(Stream::new(
            Dict::new().with("Type", "Metadata"),
            xmp.to_vec(),
        )),
    );

    doc.set(
        catalog,
        Object::Dict(
            Dict::new()
                .with("Type", "Catalog")
                .with("Pages", pages)
                .with("StructTreeRoot", root)
                .with(
                    "MarkInfo",
                    Object::Dict(Dict::new().with("Marked", Object::Bool(true))),
                )
                .with("Lang", "en-US")
                .with(
                    "ViewerPreferences",
                    Object::Dict(Dict::new().with("DisplayDocTitle", Object::Bool(true))),
                )
                .with("Metadata", meta),
        ),
    );
    doc.set(
        pages,
        Object::Dict(
            Dict::new()
                .with("Type", "Pages")
                .with("Count", Object::Int(1))
                .with("Kids", Object::Array(vec![Object::Ref(page)])),
        ),
    );

    let path = out("validate_k_forms.pdf");
    let bytes = doc.serialize(PdfVersion::V1_7, catalog).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn array_k_direct_dicts_thead_and_annots_walk() {
    let path = build_fixture();
    let report = pdfgen_validate::validate(&path).unwrap();

    // The header IS found through the THead group: NO 15-001.
    assert!(
        !report.findings.iter().any(|f| f.id == "15-001"),
        "THead-wrapped header not detected: {:?}",
        report.findings
    );
    // The direct-dict TD walked fine; the TH has Scope: no 15-003.
    assert!(
        !report.findings.iter().any(|f| f.id == "15-003"),
        "Scope missed: {:?}",
        report.findings
    );
    // The /Annots widget without /TU is flagged 28-001.
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.id == "28-001" && f.context.contains("widget")),
        "page /Annots widget not checked: {:?}",
        report.findings
    );
}
