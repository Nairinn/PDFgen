//! Streaming writer end-to-end: a large document flushed page by page,
//! validated by veraPDF out-of-band.

use pdfgen::{Profile, Status, StreamEvent, StreamWriter};

#[test]
fn stream_large_document() {
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let path = format!("{out_dir}/streamed_ua1.pdf");

    let mut w =
        StreamWriter::create(&path, Profile::PdfUa1, "Streamed mega-doc", "en-US").expect("create");

    // Large document: 500 sections in big chunks.
    for s in 0..500 {
        let mut batch = Vec::with_capacity(4);
        batch.push(StreamEvent::Begin {
            tag: if s % 25 == 0 { "H1".into() } else { "P".into() },
            alt: None,
        });
        batch.push(StreamEvent::Text {
            text: format!(
                "Section {s}: the quick brown fox jumps over the lazy dog. \
                 This chunk is intentionally long enough to wrap across \
                 several lines so the incremental wrapper is exercised."
            ),
            font: 0,
            size: 11.0,
        });
        batch.push(StreamEvent::End);
        if s % 25 == 0 && s > 0 {
            batch.push(StreamEvent::PageBreak);
        }
        w.push(batch).expect("push");
    }
    let report = w.finish().expect("finish");

    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
    let meta = std::fs::metadata(&path).unwrap();
    assert!(meta.len() > 100_000, "streamed file looks too small");
}
