//! Parser: object streams (PDF 1.5 type-2 xref entries) and /Filter arrays.
//! The fixture is built by hand with a Flate-compressed object stream so
//! the reader must exercise: xref-stream parsing, Entry::Compressed
//! resolution, and in-stream object decoding.

use pdfgen_parse::PdfReader;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Build a small PDF whose Catalog and Pages objects live INSIDE an
/// object stream, referenced by type-2 xref entries, with the object
/// stream itself Flate-compressed and declared via a /Filter array.
fn build_object_stream_pdf() -> Vec<u8> {
    use std::io::Write as _;

    // Objects 1 (Catalog) and 2 (Pages) go in the object stream (obj 6).
    let obj1 = b"<< /Type /Catalog /Pages 2 0 R >>";
    let obj2 = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    // Header: N pairs of (id, relative offset).
    // Header pairs: (obj id, offset relative to /First).
    let header = format!("1 0 2 {}", obj1.len());
    let first = header.len() + 1; // +1 newline
    let mut stm = header.into_bytes();
    stm.push(b'\n');
    stm.extend_from_slice(obj1);
    stm.extend_from_slice(obj2);
    let compressed = {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&stm).unwrap();
        e.finish().unwrap()
    };

    let mut pdf: Vec<u8> = b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut off = |pdf: &mut Vec<u8>| pdf.len() as u64;

    // Object 3: page.
    let o3 = off(&mut pdf);
    pdf.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>\nendobj\n");
    // Object 4: content.
    let o4 = off(&mut pdf);
    pdf.extend_from_slice(b"4 0 obj\n<< /Length 44 >>\nstream\nBT /F0 12 Tf 72 720 Td (Hi there) Tj ET\nendstream\nendobj\n");
    // Object 6: the object stream, /Filter as an ARRAY.
    let o6 = off(&mut pdf);
    pdf.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /ObjStm /N 2 /First {first} /Length {} /Filter [/FlateDecode] >>\nstream\n",
            compressed.len()
        )
        .as_bytes(),
    );
    let stm_start = pdf.len();
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(format!("\nendstream\nendobj\n").as_bytes());
    let _ = stm_start;

    // Xref STREAM as object 5, with type-2 entries for objects 1 and 2.
    let o5 = off(&mut pdf);
    // W = [type(1), field1(4), field2(2)]. Type 1 = in use (offset in
    // field1), type 2 = compressed (field1 = stream obj, field2 = index).
    // /Index [1 6]: rows below describe objects 1..=6 in order.
    let rows: [(u8, u64, u32); 6] = [
        (2, 6, 0),  // obj 1: compressed in stream 6, index 0
        (2, 6, 1),  // obj 2: compressed in stream 6, index 1
        (1, o3, 0), // obj 3: in use at offset
        (1, o4, 0), // obj 4
        (1, o5, 0), // obj 5 (the xref stream itself)
        (1, o6, 0), // obj 6
    ];
    let mut data = Vec::new();
    for &(t, f1, f2) in rows.iter() {
        data.push(t);
        data.extend_from_slice(&(f1 as u32).to_be_bytes());
        data.extend_from_slice(&(f2 as u16).to_be_bytes());
    }
    let comp_xref = {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&data).unwrap();
        e.finish().unwrap()
    };
    pdf.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XRef /Size 7 /Index [1 6] /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>\nstream\n",
            comp_xref.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&comp_xref);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref_at = o5;
    pdf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    pdf
}

#[test]
fn reads_objects_from_object_stream() {
    let path = out("objstm_fixture.pdf");
    let bytes = build_object_stream_pdf();
    std::fs::write(&path, &bytes).unwrap();

    let mut reader = PdfReader::open(&path).expect("parses the xref stream");
    let catalog = reader.catalog().expect("catalog resolves");
    // The catalog came from INSIDE the object stream (type-2 entry).
    assert!(
        catalog.get("Pages").is_some(),
        "catalog decoded from objstm"
    );
    let pages_ref = match catalog.get("Pages") {
        Some(pdfgen_core::Object::Ref(r)) => *r,
        _ => panic!("no /Pages in catalog"),
    };
    let pages = reader.get(pages_ref.id).expect("pages object resolves");
    match &pages {
        pdfgen_core::Object::Dict(d) => {
            assert!(d.get("Count").is_some(), "pages dict decoded from objstm");
        }
        _ => panic!("pages not a dict"),
    }
    // A regular object still resolves.
    let page = reader.get(3).expect("obj 3 resolves");
    assert!(
        matches!(&page, pdfgen_core::Object::Dict(d) if d.get("Type").is_some()),
        "page object resolves"
    );
}
