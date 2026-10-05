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

#[test]
fn repair_tolerates_broken_xref() {
    let bytes = std::fs::read(out("hello_ua1.pdf")).unwrap();
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
