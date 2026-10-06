//! Robustness: PdfReader must never panic on garbage bytes - truncated
//! files, mutated headers, adversarial structures.

use pdfgen_parse::PdfReader;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A small valid base file to mutate.
fn base_pdf() -> Vec<u8> {
    let objects: Vec<&str> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
          /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>",
        "<< /Length 44 >>\nstream\nBT /F0 12 Tf 72 720 Td (Hi) Tj ET\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ];
    let mut pdf: Vec<u8> = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
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
fn truncated_pdfs_never_panic() {
    let base = base_pdf();
    for cut in [
        0usize,
        4,
        8,
        20,
        100,
        base.len() / 2,
        base.len() - 9,
        base.len() - 1,
    ] {
        let truncated = &base[..cut.min(base.len())];
        let _ = PdfReader::from_bytes(truncated.to_vec());
        // All accessor paths must also be safe on the truncated file.
        if let Ok(mut r) = PdfReader::from_bytes(truncated.to_vec()) {
            let _ = r.catalog();
            let _ = r.pages();
        }
    }
}

#[test]
fn mutated_pdfs_never_panic() {
    let base = base_pdf();
    let mut state: u64 = 0x1234_5678_9ABC_DEF0;
    for _ in 0..300 {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let r = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let mut buf = base.clone();
        let flips = (r % 4 + 1) as usize;
        for k in 0..flips {
            let pos = ((r >> (8 * k)) as usize) % buf.len();
            buf[pos] ^= (r >> 32) as u8;
        }
        if let Ok(mut reader) = PdfReader::from_bytes(buf) {
            let _ = reader.catalog();
            let _ = reader.pages();
            let _ = reader.get(1);
            let _ = reader.object_offsets();
        }
    }
}

#[test]
fn garbage_bytes_never_panic() {
    // Pure random buffers.
    let mut state: u64 = 0xDEAD_BEEF_CAFE_F00D;
    for len in [0usize, 1, 7, 100, 1000] {
        let mut buf = Vec::with_capacity(len);
        for _ in 0..len {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            buf.push((state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as u8);
        }
        let _ = PdfReader::from_bytes(buf);
    }
    // The reader repairs what it can; a real file survives all of it.
    let path = out("robust_base.pdf");
    std::fs::write(&path, base_pdf()).unwrap();
    let mut r = PdfReader::open(&path).unwrap();
    assert!(r.pages().unwrap().len() == 1);
}
