//! StreamWriter image regressions: no duplicated content, figures close
//! before later text, each image is one XObject even across many pages.

use pdfgen::{Profile, StreamEvent, StreamWriter};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

const RED_BOX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/red_box.png"
);

#[test]
fn image_then_text_no_duplicates_across_pages() {
    if !std::path::Path::new(RED_BOX).exists() {
        eprintln!("skipping: fixture missing");
        return;
    }
    let path = out("stream_images_regression.pdf");
    let mut w = StreamWriter::create(&path, Profile::PdfUa1, "Stream images", "en-US").unwrap();

    // Page 1: a figure, then a paragraph after it (the paragraph must NOT
    // nest inside the Figure's marked content).
    let batch = vec![
        StreamEvent::Begin {
            tag: "Figure".into(),
            alt: Some("A red box".into()),
        },
        StreamEvent::Image {
            path: RED_BOX.into(),
            width: 120.0,
            height: 80.0,
        },
        StreamEvent::End,
        StreamEvent::Begin {
            tag: "P".into(),
            alt: None,
        },
        StreamEvent::Text {
            text: "Paragraph after the figure, still on page one.".into(),
            font: 0,
            size: 11.0,
        },
        StreamEvent::End,
    ];
    w.push(batch).unwrap();

    // Force more pages with filler paragraphs.
    for i in 0..40 {
        let text = format!("Filler paragraph number {i} to push across page breaks.");
        w.push(vec![
            StreamEvent::Begin {
                tag: "P".into(),
                alt: None,
            },
            StreamEvent::Text {
                text,
                font: 0,
                size: 11.0,
            },
            StreamEvent::End,
        ])
        .unwrap();
    }
    let report = w.finish().unwrap();
    assert!(report.violations.is_empty(), "{:?}", report.violations);

    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);

    // Exactly ONE image XObject (the old flush re-emitted all images on
    // every page).
    let xobj_count = txt.matches("/Subtype /Image").count();
    assert_eq!(xobj_count, 1, "one image must embed once, got {xobj_count}");

    // First-page structural checks across the decompressed streams (the
    // metadata stream precedes page content streams in the file).
    let page1 = all_content_streams(&data);

    // The post-figure paragraph appears exactly once (the old reused
    // scratch buffer duplicated it into the image op).
    let para = page1.matches("Paragraph after the figure").count();
    assert_eq!(para, 1, "post-figure paragraph emitted {para} times");

    // The Figure's marked content closes BEFORE the following P opens.
    let fig = page1.find("/Figure").expect("figure in page 1");
    let emc = page1[fig..]
        .find("EMC")
        .map(|o| fig + o)
        .expect("figure EMC on page 1");
    let p_tag = page1[emc..]
        .find("/P ")
        .map(|o| emc + o)
        .expect("paragraph after figure");
    assert!(fig < emc && emc < p_tag, "Figure must close before the P");

    // The draw op appears exactly once in the page.
    let dos = page1.matches("/Im0 Do").count();
    assert_eq!(dos, 1, "one Do op expected, got {dos}");
}

/// Decompress every stream in the file and join them (metadata streams
/// decode to text we simply won't match against).
fn all_content_streams(data: &[u8]) -> String {
    use std::io::Read as _;
    let mut joined = String::new();
    let mut i = 0usize;
    while let Some(idx) = find(data, b"stream\n", i) {
        let Some(end) = find(data, b"endstream", idx + 7) else {
            break;
        };
        let chunk = &data[idx + 7..end];
        let mut dec = flate2::read::ZlibDecoder::new(chunk);
        let mut out = Vec::new();
        if dec.read_to_end(&mut out).is_ok() && !out.is_empty() {
            joined.push_str(&String::from_utf8_lossy(&out));
            joined.push('\n');
        } else {
            joined.push_str(&String::from_utf8_lossy(chunk));
            joined.push('\n');
        }
        i = end + 9;
    }
    joined
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}
