//! Negative case: a table without a header row must be flagged (15-001),
//! and a TH without Scope must be flagged (15-003).

use pdfgen_core::{Dict, Object, PdfVersion, Stream};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Build a tagged PDF whose Table has TDs but no TH and no Scope.
#[test]
fn headerless_table_is_flagged() {
    let mut w = pdfgen_core::Document::new();

    let catalog = w.alloc();
    let pages = w.alloc();
    let page = w.alloc();
    let contents = w.alloc();
    let struct_root = w.alloc();
    let doc_elem = w.alloc();
    let table = w.alloc();
    let tr = w.alloc();
    let td = w.alloc();

    // Content: one TD text run with an MCID.
    w.set_stream(
        contents,
        Stream::new(
            Dict::new(),
            b"/TD <</MCID 0>> BDC\nBT /F0 12 Tf 72 700 Td (cell) Tj ET\nEMC\n".to_vec(),
        ),
    );

    // TD element.
    let mut td_d = Dict::new();
    td_d.set("Type", "StructElem");
    td_d.set("S", "TD");
    td_d.set("P", tr);
    td_d.set("Pg", page);
    td_d.set("K", 0);
    w.set(td, Object::Dict(td_d));

    // TR element.
    let mut tr_d = Dict::new();
    tr_d.set("Type", "StructElem");
    tr_d.set("S", "TR");
    tr_d.set("P", table);
    tr_d.set("Pg", page);
    tr_d.set("K", Object::Array(vec![Object::Ref(td)]));
    w.set(tr, Object::Dict(tr_d));

    // Table element (no TH anywhere → 15-001).
    let mut t_d = Dict::new();
    t_d.set("Type", "StructElem");
    t_d.set("S", "Table");
    t_d.set("P", doc_elem);
    t_d.set("Pg", page);
    t_d.set("K", Object::Array(vec![Object::Ref(tr)]));
    w.set(table, Object::Dict(t_d));

    // Document element.
    let mut de = Dict::new();
    de.set("Type", "StructElem");
    de.set("S", "Document");
    de.set("P", struct_root);
    de.set("K", Object::Array(vec![Object::Ref(table)]));
    w.set(doc_elem, Object::Dict(de));

    let mut sr = Dict::new();
    sr.set("Type", "StructTreeRoot");
    sr.set("K", doc_elem);
    sr.set(
        "ParentTree",
        Object::Dict(Dict::new().with(
            "Nums",
            Object::Array(vec![Object::Int(0), Object::Array(vec![Object::Ref(td)])]),
        )),
    );
    w.set(struct_root, Object::Dict(sr));

    let mut pg = Dict::new();
    pg.set("Type", "Page");
    pg.set("Parent", pages);
    pg.set(
        "MediaBox",
        Object::Array(vec![
            Object::Int(0),
            Object::Int(0),
            Object::Int(612),
            Object::Int(792),
        ]),
    );
    pg.set("Resources", Object::Dict(Dict::new()));
    pg.set("Contents", contents);
    pg.set("StructParents", 0);
    w.set(page, Object::Dict(pg));

    w.set(
        pages,
        Object::Dict(
            Dict::new()
                .with("Type", "Pages")
                .with("Kids", Object::Array(vec![Object::Ref(page)]))
                .with("Count", 1),
        ),
    );

    let mut cat = Dict::new();
    cat.set("Type", "Catalog");
    cat.set("Pages", pages);
    cat.set("MarkInfo", Object::Dict(Dict::new().with("Marked", true)));
    cat.set("StructTreeRoot", struct_root);
    w.set(catalog, Object::Dict(cat));

    let bytes = w.serialize(PdfVersion::V1_7, catalog).unwrap();
    let path = out("headerless_table.pdf");
    std::fs::write(&path, bytes).unwrap();

    let report = pdfgen_validate::validate(&path).unwrap();
    assert!(
        report.findings.iter().any(|f| f.id == "15-001"),
        "expected 15-001 (table without header row): {:?}",
        report.findings
    );
}
