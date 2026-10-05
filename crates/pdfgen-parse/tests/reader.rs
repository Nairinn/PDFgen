//! Reader tests: parse our own veraPDF-compliant output, resolve the
//! catalog and page tree, and survive a truncated-tail repair.

use pdfgen_parse::PdfReader;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn roundtrip_read_own_output() {
    for name in ["hello_ua1.pdf", "hello_ua2.pdf"] {
        let path = out(name);
        if !std::path::Path::new(&path).exists() {
            // Built by the pdfgen crate's tests; they skip where the test
            // font is absent, so there is nothing to round-trip here.
            eprintln!("skipping {name}: fixture not present");
            continue;
        }
        let mut r = PdfReader::open(&path).expect(name);
        assert!(!r.repair.scanned, "{name}: xref should load cleanly");
        let catalog = r.catalog().expect("catalog resolves");
        assert!(catalog.has("Pages"), "{name}: catalog has /Pages");
        assert!(catalog.has("StructTreeRoot"), "{name}: catalog has tree");
        assert!(catalog.has("Metadata"), "{name}: catalog has metadata");
        // Every object we wrote should resolve (slot 0 is the free-list
        // head, not a real object).
        let ids: Vec<u32> = r
            .object_ids()
            .iter()
            .copied()
            .filter(|&id| id != 0)
            .collect();
        for id in ids {
            r.get(id).unwrap_or_else(|e| panic!("{name} obj {id}: {e}"));
        }
    }
}

/// Build a minimal classic-xref PDF (6 objects) with correct offsets —
/// no fonts, no cross-crate fixtures. Used for the repair test.
fn minimal_pdf() -> Vec<u8> {
    let objects: Vec<&str> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
          /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 44 >>\nstream\nBT /F0 12 Tf 72 720 Td (Hi) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Producer (pdfgen test) >>",
    ];

    let mut pdf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len() as u64);
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, body).as_bytes());
    }
    let xref_at = pdf.len() as u64;
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[test]
fn repair_tolerates_broken_xref() {
    let bytes = minimal_pdf();
    // Simulate damage: destroy the startxref offset line at the end.
    let mut broken = bytes.clone();
    let idx = broken
        .windows(9)
        .rposition(|w| w == b"startxref")
        .expect("has startxref");
    for b in &mut broken[idx..idx + 9] {
        *b = b'X';
    }
    // Also remove the xref table marker.
    let xref_idx = broken.windows(4).position(|w| w == b"xref").expect("has xref");
    for b in &mut broken[xref_idx..xref_idx + 4] {
        *b = b'Y';
    }

    let mut r = PdfReader::from_bytes(broken).expect("repair pass succeeds");
    assert!(r.repair.scanned, "repair must have scanned");
    assert!(r.repair.recovered > 5, "should recover several objects");
    let catalog = r.catalog().expect("catalog found after repair");
    assert!(catalog.has("Pages"));
}
