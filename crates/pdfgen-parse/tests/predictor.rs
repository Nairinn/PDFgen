//! Parser: PNG predictors on xref streams and indirect /Length.

use pdfgen_parse::PdfReader;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Build a minimal PDF whose xref stream is Flate-compressed with PNG
/// Up-prediction (/Predictor 12, /Columns = 7 for W [1 4 2]) and whose
/// stream /Length is an INDIRECT reference.
fn build_predicted_pdf() -> Vec<u8> {
    use std::io::Write as _;

    let mut pdf: Vec<u8> = b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let o = |pdf: &Vec<u8>| pdf.len() as u64;

    // obj 1: Catalog. obj 2: Pages. obj 3: Page. obj 4: Contents.
    let o1 = o(&pdf);
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let o2 = o(&pdf);
    pdf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    let o3 = o(&pdf);
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> /Contents 4 0 R >>\nendobj\n",
    );
    let o4 = o(&pdf);
    let content = b"BT /F0 12 Tf 72 720 Td (Hi) Tj ET\n";
    pdf.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    pdf.extend_from_slice(content);
    pdf.extend_from_slice(b"endstream\nendobj\n");

    // obj 5: /Length value for the xref stream (indirect /Length).
    let o5 = o(&pdf);
    // Filled after we know the xref stream length.
    let length_obj_pos = pdf.len();
    pdf.extend_from_slice(b"5 0 obj\n000\nendobj\n"); // placeholder

    // Xref stream rows: W [1 4 2], objects 1..=5 then 0? Index covers
    // objects 1..=5 (5 rows).
    let rows: [(u8, u64, u32); 5] = [
        (1, o1, 0),
        (1, o2, 0),
        (1, o3, 0),
        (1, o4, 0),
        (1, 0, 0), // obj 5: the xref stream itself, patched below
    ];
    let row_len = 7usize; // 1 + 4 + 2
    let n_rows = rows.len();
    let mut raw_rows = Vec::with_capacity(n_rows * (row_len + 1));
    let mut prev_row = vec![0u8; row_len];
    for (t, f1, f2) in rows.iter() {
        let mut cur = vec![0u8; row_len];
        cur[0] = *t;
        cur[1..5].copy_from_slice(&(*f1 as u32).to_be_bytes());
        cur[5..7].copy_from_slice(&(*f2 as u16).to_be_bytes());
        // PNG Up filter: out = cur - prev (byte-wise subtract).
        let mut filtered = vec![0u8; row_len];
        for i in 0..row_len {
            filtered[i] = cur[i].wrapping_sub(prev_row[i]);
        }
        raw_rows.push(2u8); // Up
        raw_rows.extend_from_slice(&filtered);
        prev_row = cur;
    }

    let compressed = {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&raw_rows).unwrap();
        e.finish().unwrap()
    };

    // Patch object 5's /Length now that we know it.
    let length_text = format!("5 0 obj\n{}\nendobj\n", compressed.len());
    let length_bytes = length_text.as_bytes();
    pdf.splice(
        length_obj_pos..length_obj_pos + length_bytes.len().min(pdf.len() - length_obj_pos),
        length_bytes.iter().copied(),
    );

    // obj 6: the xref stream with an indirect /Length and PNG predictor.
    let o6 = o(&pdf);
    // Patch row 5 (the xref stream's own entry) - rebuild rows with o6.
    // Simplest: rewrite the whole compressed block with the final offset.
    let rows2: [(u8, u64, u32); 6] = [
        (1, o1, 0),
        (1, o2, 0),
        (1, o3, 0),
        (1, o4, 0),
        (1, o5, 0),
        (1, o6, 0),
    ];
    let mut raw_rows2 = Vec::with_capacity(6 * (row_len + 1));
    let mut prev_row = vec![0u8; row_len];
    for (t, f1, f2) in rows2.iter() {
        let mut cur = vec![0u8; row_len];
        cur[0] = *t;
        cur[1..5].copy_from_slice(&(*f1 as u32).to_be_bytes());
        cur[5..7].copy_from_slice(&(*f2 as u16).to_be_bytes());
        let mut filtered = vec![0u8; row_len];
        for i in 0..row_len {
            filtered[i] = cur[i].wrapping_sub(prev_row[i]);
        }
        raw_rows2.push(2u8);
        raw_rows2.extend_from_slice(&filtered);
        prev_row = cur;
    }
    let compressed2 = {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&raw_rows2).unwrap();
        e.finish().unwrap()
    };
    // The /Length object must carry the FINAL compressed size; patch again.
    let length_text2 = format!("5 0 obj\n{}\nendobj\n", compressed2.len());
    pdf.splice(
        length_obj_pos..length_obj_pos + length_bytes.len(),
        length_text2.as_bytes().iter().copied(),
    );
    // NOTE: o6 may shift if the length text length changed; keep same digit
    // count by construction (both are small numbers of similar magnitude).
    let o6_final = o6; // assumed stable

    pdf.extend_from_slice(
        format!(
            "6 0 obj\n<< /Type /XRef /Size 7 /Index [1 6] /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 7 >> /Length 5 0 R >>\nstream\n"
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&compressed2);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref_at = o6_final;
    pdf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    pdf
}

#[test]
fn reads_xref_stream_with_png_predictor_and_indirect_length() {
    let path = out("predictor_fixture.pdf");
    let bytes = build_predicted_pdf();
    std::fs::write(&path, &bytes).unwrap();

    let mut reader = PdfReader::open(&path).expect("xref stream with predictor parses");
    let catalog = reader.catalog().expect("catalog resolves");
    assert!(
        catalog.get("Pages").is_some(),
        "catalog decoded through predicted xref"
    );
    let page = reader.get(3).expect("page resolves");
    assert!(
        matches!(&page, pdfgen_core::Object::Dict(d) if d.get("Type").is_some()),
        "page object resolves through predicted xref"
    );
    // Indirect /Length resolved: the xref stream parsed at all proves it.
}
