//! Size/timing benchmarks for the cleanup spec's report: a 500-page
//! StreamWriter document and a 50-page CJK flow document.

use pdfgen::{Document, Profile, StreamEvent, StreamWriter};
use std::time::Instant;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn report_sizes_and_timings() {
    // --- 500-page stream document ---
    let t0 = Instant::now();
    let path = out("bench_stream_500.pdf");
    let mut w = StreamWriter::create(&path, Profile::PdfUa1, "Bench", "en-US").unwrap();
    for i in 0..2500 {
        w.push(vec![
            StreamEvent::Begin {
                tag: if i % 5 == 0 { "H1" } else { "P" }.into(),
                alt: None,
                attrs: None,
            },
            StreamEvent::Text {
                text: format!(
                    "Bench paragraph {i} with a reasonable amount of body text to \
                     exercise wrapping, compression and xref generation paths."
                ),
                font: 0,
                size: 11.0,
            },
            StreamEvent::End,
        ])
        .unwrap();
    }
    let report = w.finish().unwrap();
    let stream_time = t0.elapsed();
    assert!(report.violations.is_empty());
    let stream_size = std::fs::metadata(&path).unwrap().len();
    println!("500-page stream: {stream_size} bytes in {stream_time:?}");

    // --- 50-page CJK flow document ---
    let jp = "/Users/nainglynn/Documents/GitHub/PDFgen/fonts/vendor/noto/NotoSansCJKjp-Regular.otf";
    let t1 = Instant::now();
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("CJK bench").lang("ja");
    let latin = doc.font("Liberation Sans", "Regular").unwrap();
    let jp_f = doc.load_font(jp).unwrap();
    {
        let mut flow = doc.flow();
        flow.heading(1, "CJK Benchmark").unwrap();
        for i in 0..600 {
            if i % 12 == 0 {
                flow.paragraph_in(latin, 11.0, &format!("Section {i}"))
                    .unwrap();
            }
            flow.paragraph_in(
                jp_f,
                11.0,
                "\u{3053}\u{3093}\u{306B}\u{3061}\u{306F}\u{3001}\u{4E16}\u{754C}\u{3002}\
                 \u{65E5}\u{672C}\u{8A9E}\u{306E}\u{57CB}\u{3081}\u{8FBC}\u{307F}\u{30C6}\
                 \u{30B9}\u{30C8}\u{3067}\u{3059}\u{3002}Bench filler for the CJK path.",
            )
            .unwrap();
        }
    }
    let cjk_path = out("bench_cjk_50.pdf");
    let report = doc.save(&cjk_path).unwrap();
    let cjk_time = t1.elapsed();
    assert_eq!(report.status, pdfgen::Status::Compliant);
    let cjk_size = std::fs::metadata(&cjk_path).unwrap().len();
    println!("50-page CJK flow: {cjk_size} bytes in {cjk_time:?}");

    let _ = (stream_size, stream_time, cjk_size, cjk_time);
}
