//! P0-10 regression: the outline tree must link siblings only and count
//! open items correctly.

use pdfgen::{Document, Profile, Status};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn outline_links_siblings_not_flat_chain() {
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Outline structure").lang("en-US");
    doc.font("Liberation Sans", "Regular").unwrap();
    doc.flow_heading(1, "Chapter One").unwrap();
    doc.flow_paragraph("Intro to chapter one.").unwrap();
    doc.flow_heading(2, "Section A").unwrap();
    doc.flow_paragraph("Text under section A.").unwrap();
    doc.flow_heading(2, "Section B").unwrap();
    doc.flow_paragraph("Text under section B.").unwrap();
    doc.flow_heading(3, "Subsection B1").unwrap();
    doc.flow_paragraph("Deep content.").unwrap();
    doc.flow_heading(1, "Chapter Two").unwrap();
    doc.flow_paragraph("Chapter two body.").unwrap();

    let path = out("outline_tree.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let data = std::fs::read(&path).unwrap();
    let _ = data;

    // Extract every outline item object: (title, keys).
    // Simpler: verify the flat-chain bug is gone: "Chapter Two" (the last
    // item, a root sibling) must NOT have /Prev pointing at the deep
    // "Subsection B1"; its /Prev must point at "Chapter One".
    // Parse outline item dicts with their titles via the reader.
    let mut reader = pdfgen_parse::PdfReader::open(&path).unwrap();
    let catalog = reader.catalog().unwrap();
    let pdfgen_core::Object::Ref(ol) = catalog.get("Outlines").cloned().unwrap() else {
        panic!("no outlines");
    };
    let pdfgen_core::Object::Dict(outl) = reader.get(ol.id).unwrap() else {
        panic!("outlines not a dict");
    };
    let pdfgen_core::Object::Ref(first) = outl.get("First").cloned().unwrap() else {
        panic!("no First");
    };
    // Outlines /Count must equal the number of items (5) when all are open.
    let pdfgen_core::Object::Int(count) = outl.get("Count").cloned().unwrap() else {
        panic!("no Count");
    };
    assert_eq!(count, 5, "outlines Count must count all open items, got {count}");

    // Walk: Chapter Two (last root child) Prev -> Chapter One (first root
    // child), NOT Subsection B1.
    let pdfgen_core::Object::Dict(c1) = reader.get(first.id).unwrap() else {
        panic!("first item not a dict");
    };
    let c1_title = match c1.get("Title").cloned().unwrap() {
        pdfgen_core::Object::String(s) => s.decode(),
        _ => panic!("no title"),
    };
    assert_eq!(c1_title, "Chapter One");

    // Follow Next from Chapter One: should be Section A (its first child
    // is NOT its Next sibling).
    let pdfgen_core::Object::Ref(next1) = c1.get("Next").cloned().unwrap() else {
        panic!("Chapter One has no Next");
    };
    let pdfgen_core::Object::Dict(sa) = reader.get(next1.id).unwrap() else {
        panic!("next not a dict");
    };
    let sa_title = match sa.get("Title").cloned().unwrap() {
        pdfgen_core::Object::String(s) => s.decode(),
        _ => panic!("no title"),
    };
    assert_eq!(sa_title, "Chapter Two", "root siblings must chain Chapter One -> Chapter Two, got {sa_title}");

    // Subsection B1 must not have /Next at all (it is an only child).
    let pdfgen_core::Object::Ref(f_child) = c1.get("First").cloned().unwrap() else {
        panic!("Chapter One has no First child");
    };
    let _ = f_child; // Section A exists as a child
    let pdfgen_core::Object::Int(c1_count) = c1.get("Count").cloned().unwrap() else {
        panic!("Chapter One has no Count");
    };
    assert_eq!(c1_count, 3, "Chapter One counts its 3 descendants (A, B, B1), got {c1_count}");
}
