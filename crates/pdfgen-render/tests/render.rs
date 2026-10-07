//! Render round-trip: rasterize a PDF our writer produced and check the
//! pixels actually contain ink where text was placed.

use pdfgen::{Document, Profile, Status};
use pdfgen_render::{render_page, RenderOptions};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn renders_text_and_geometry() {
    const FONT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
    );
    if !std::path::Path::new(FONT).exists() {
        eprintln!("skipping: no Arial on this machine");
        return;
    }
    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Render test").lang("en-US");
    doc.load_font(FONT).unwrap();
    {
        let mut flow = doc.flow();
        flow.heading(1, "Ink on the page").unwrap();
        flow.paragraph("This paragraph becomes pixels when rendered.")
            .unwrap();
    }
    let path = out("render_source.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(report.status, Status::Compliant);

    let bmp = render_page(
        &path,
        0,
        RenderOptions {
            dpi: 150.0,
            white_background: true,
        },
    )
    .expect("renders");
    assert_eq!(bmp.width, 1275, "8.5in at 150dpi");
    assert_eq!(bmp.height, 1650, "11in at 150dpi");

    // Count dark pixels — text should produce plenty.
    let dark = bmp
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 500, "expected ink, got {dark} dark pixels");

    // Ink must appear in the top-left quadrant (heading) — flow places
    // content starting at the top.
    let top_dark = bmp
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            let y = i / (bmp.width as usize * 4);
            y < bmp.height as usize / 4 && p[0] < 128
        })
        .count();
    assert!(
        top_dark > 100,
        "heading ink expected near top, got {top_dark}"
    );

    // PNG encodes with a sane signature and size.
    let png = bmp.to_png().expect("png encodes");
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert!(png.len() > 10_000, "non-trivial png ({} bytes)", png.len());
    std::fs::write(out("render_page0.png"), &png).unwrap();
}

#[test]
fn renders_cid_text() {
    const MM: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
    );
    if !std::path::Path::new(MM).exists() {
        eprintln!("skipping: no Myanmar font");
        return;
    }
    // The CID test PDF is produced by the pdfgen crate's cid_font test;
    // regenerate a fresh one if it is missing.
    let path = out("cid_test_ua1.pdf");
    if !std::path::Path::new(&path).exists() {
        let mut doc = Document::new(Profile::PdfUa1);
        doc.title("CID render test").lang("en-US");
        // Latin first: the heading uses the default font and Noto Sans
        // Myanmar has no Latin glyphs.
        doc.load_font(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
        ))
        .expect("Liberation Sans");
        let myanmar = doc.load_font(MM).expect("Noto Sans Myanmar");
        {
            let mut flow = doc.flow();
            flow.heading(1, "CID Render Test").unwrap();
            flow.paragraph_in(
                myanmar,
                12.0,
                "\u{1000}\u{1001}\u{1002}\u{1019}\u{103C}\u{102D}\u{1004}\u{103A}",
            )
            .unwrap();
        }
        doc.save(&path).unwrap();
    }
    let bmp = render_page(&path, 0, RenderOptions::default()).expect("renders CID page");
    let dark = bmp
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 100, "CID glyphs should leave ink, got {dark}");
}
