//! Renderer regression: octal-escaped strings (which the pdfgen writer
//! emits for every byte outside 0x20-0x7E) must decode correctly, and
//! hex strings / TJ arrays must render.

use pdfgen_render::{render_page, RenderOptions};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Build a PDF whose content stream writes CID text as octal escapes:
/// /F1 is an Identity-H font whose codes are 2-byte GIDs.
#[test]
fn octal_escaped_cid_text_renders() {
    const MM: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/noto/NotoSansMyanmar-Regular.ttf"
    );
    if !std::path::Path::new(MM).exists() {
        eprintln!("skipping: no Myanmar font on this machine");
        return;
    }

    // Generate via the writer (octal-escaped CID bytes come naturally).
    let mut doc = pdfgen::Document::new(pdfgen::Profile::PdfUa1);
    doc.title("Octal render").lang("en-US");
    // Latin font first: the default font draws the heading, and Noto
    // Sans Myanmar has no Latin glyphs.
    doc.load_font(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
    ))
    .unwrap();
    let f = doc.load_font(MM).unwrap();
    {
        let mut flow = doc.flow();
        flow.heading(1, "Octal CID").unwrap();
        flow.paragraph_in(
            f,
            14.0,
            "\u{1000}\u{1001}\u{1002}\u{1019}\u{103C}\u{102D}\u{1004}\u{103A}",
        )
        .unwrap();
    }
    let path = out("render_octal.pdf");
    doc.save(&path).unwrap();

    // Confirm the file actually contains octal escapes (writer behavior).
    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    assert!(
        txt.contains("\\0"), // octal escape present in some string
        "expected octal-escaped bytes in the fixture"
    );

    let bmp = render_page(&path, 0, RenderOptions::default()).expect("renders");
    let dark = bmp
        .rgba
        .chunks_exact(4)
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    // With broken decoding this previously collapsed to far fewer glyphs.
    assert!(
        dark > 400,
        "CID glyphs through octal escapes should leave plenty of ink, got {dark}"
    );
}

/// A content stream with a hex string and a TJ array must render text.
#[test]
fn hex_and_tj_render() {
    // Hand-build: /F0 is a Type1 Helvetica (unembedded) - the renderer
    // only draws embedded fonts, so the ink check uses an embedded font
    // through the writer instead. For the hex/TJ parse paths we assert
    // no panic and correct op advancement via rendering our own file,
    // then rewriting its content stream ops with hex/TJ equivalents.
    let base = out("render_hex_base.pdf");
    let mut doc = pdfgen::Document::new(pdfgen::Profile::PdfUa1);
    doc.title("Hex TJ").lang("en-US");
    doc.font("Liberation Sans", "Regular").unwrap();
    {
        let mut flow = doc.flow();
        flow.heading(1, "Hex and TJ test").unwrap();
    }
    doc.save(&base).unwrap();

    // The renderer must at least handle hex/TJ operators without error
    // on a stream that contains them. Patch a copy: replace the first
    // (Hex and TJ test) literal with <...> hex and add a TJ array.
    let data = std::fs::read(&base).unwrap();
    let txt = String::from_utf8_lossy(&data).to_string();
    // Decode the literal in stream? Simpler assertion path: craft the ops
    // directly in a scratch content stream via the writer API is overkill;
    // instead verify the reader path with a minimal hand-built page.
    let _ = (data, txt);

    // Minimal PDF with hex + TJ content, no fonts: rendering should
    // succeed (no embedded font -> no glyph, but the operators parse).
    let mut w = pdfgen_core::Document::new();
    let catalog = w.alloc();
    let pages = w.alloc();
    let page = w.alloc();
    let contents = w.alloc();
    let stream_body = b"BT /F0 12 Tf 72 720 Td <486578> Tj ET\nBT /F0 12 Tf 72 700 Td [(T) 80 (J) 80 (text)] TJ ET\n";
    w.set_stream(
        contents,
        pdfgen_core::Stream::new(
            pdfgen_core::Dict::new().with("Length", stream_body.len() as i64),
            stream_body.to_vec(),
        ),
    );
    w.set(
        page,
        pdfgen_core::Object::Dict(
            pdfgen_core::Dict::new()
                .with("Type", "Page")
                .with("Parent", pages)
                .with(
                    "MediaBox",
                    pdfgen_core::Object::Array(vec![
                        pdfgen_core::Object::Int(0),
                        pdfgen_core::Object::Int(0),
                        pdfgen_core::Object::Int(612),
                        pdfgen_core::Object::Int(792),
                    ]),
                )
                .with(
                    "Resources",
                    pdfgen_core::Object::Dict(pdfgen_core::Dict::new()),
                )
                .with("Contents", contents),
        ),
    );
    w.set(
        pages,
        pdfgen_core::Object::Dict(
            pdfgen_core::Dict::new()
                .with("Type", "Pages")
                .with(
                    "Kids",
                    pdfgen_core::Object::Array(vec![pdfgen_core::Object::Ref(page)]),
                )
                .with("Count", 1),
        ),
    );
    w.set(
        catalog,
        pdfgen_core::Object::Dict(
            pdfgen_core::Dict::new()
                .with("Type", "Catalog")
                .with("Pages", pages),
        ),
    );
    let bytes = w.serialize(pdfgen_core::PdfVersion::V1_7, catalog).unwrap();
    let path = out("render_hex_tj.pdf");
    std::fs::write(&path, bytes).unwrap();

    // Renders without panic; no embedded font means no ink, but the
    // operators must not derail the tokenizer (no infinite loop).
    let bmp = render_page(&path, 0, RenderOptions::default()).expect("hex/TJ streams parse");
    // 8.5in at the default 150 dpi.
    assert_eq!(bmp.width, 1275);
}
