//! HTML converter fidelity regressions (issue #3): entities, comments,
//! script/style exclusion, implied paragraphs, <br> survival, <ol>
//! numbering, boundary-safe attrs, relative img src.

use pdfgen::{html_file_to_pdf, html_to_pdf_profiled, Profile};

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn extract_all(path: &str) -> String {
    let pages = pdfgen::extract_text(path).expect("extract");
    let mut joined = String::new();
    for p in pages {
        for b in p.blocks {
            joined.push_str(&b);
            joined.push('\n');
        }
    }
    joined
}

#[test]
fn html_fidelity_rules_hold() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/fidelity.html"
    );
    let path = out("html_fidelity.pdf");
    let report = html_file_to_pdf(fixture, &path, "Fidelity", "en-US").expect("convert");
    assert_eq!(report.status, pdfgen::Status::Compliant);

    let text = extract_all(&path);

    // Numeric entities decoded.
    assert!(text.contains("café"), " café missing: {text}");
    assert!(text.contains('—'), "em dash missing: {text}");
    // Attribute entity decoded in the Figure's alt (not body text, but the
    // /Alt string decodes the same way); body checks:
    // Comments, script, style excluded.
    assert!(!text.contains("must not appear"), "script leaked: {text}");
    assert!(
        !text.to_ascii_lowercase().contains("color: red"),
        "style leaked: {text}"
    );
    assert!(
        !text.contains("a comment that must not appear"),
        "comment leaked: {text}"
    );
    // Implied paragraphs: div text and loose body text survive.
    assert!(text.contains("Implied paragraph text"), "{text}");
    assert!(text.contains("Loose body text"), "{text}");
    // <br> survived as a hard break (two lines in one block, or at least
    // both fragments present).
    assert!(text.contains("forced line break"), "{text}");
    // <ol> numbering.
    assert!(text.contains("1."), "no numbering: {text}");
    assert!(text.contains("2."), "{text}");
}

#[test]
fn unquoted_attr_and_boundary_safe_alt_parse() {
    // Unquoted src with no quotes at all; data- must not match alt.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
    let src = std::path::Path::new(dir).join("box.png");
    std::fs::write(
        &src,
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/red_box.png"
        ))
        .unwrap(),
    )
    .unwrap();
    let path = out("html_unquoted.pdf");
    // Use the string API (no base dir): pass an absolute path instead.
    let html = format!("<p>x</p><img src={src:?} data-alt=nope alt=\"Real alt\">");
    let report =
        html_to_pdf_profiled(&html, &path, Profile::PdfUa1, "Unquoted", "en-US").expect("convert");
    assert_eq!(report.status, pdfgen::Status::Compliant);

    // The Figure's /Alt must be "Real alt", not "nope": read it back.
    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    assert!(txt.contains("Real alt"), "alt wrong: find /Alt in {txt:?}");
    assert!(!txt.contains("nope"), "data-alt matched: {txt:?}");
}

#[test]
fn relative_img_src_resolves_against_html_dir() {
    // fidelity.html references img/red_box.png relative to its own dir;
    // if src resolved against CWD the image would silently be a 96x96
    // placeholder. Extraction can't see images, so verify the Figure
    // exists and the file converts compliantly with the real image
    // (image_size of the real box differs from the 96x96 fallback).
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/fidelity.html"
    );
    let path = out("html_relative_src.pdf");
    let report = html_file_to_pdf(fixture, &path, "Relative src", "en-US").expect("convert");
    assert_eq!(report.status, pdfgen::Status::Compliant);
}
