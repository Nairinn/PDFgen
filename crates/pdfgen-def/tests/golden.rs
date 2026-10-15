//! Golden config round-trip: build the fixture definition and verify
//! the PDF is compliant and extracts the source text back. veraPDF
//! validates the output through the repo gate (def_golden.pdf lives in
//! tests/output and is not in the skip manifest).

use pdfgen_def::{build_to_file, parse_def};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/golden_doc.json"
);

#[test]
fn golden_config_builds_a_compliant_pdf() {
    let json = std::fs::read(GOLDEN).expect("fixture");
    let def = parse_def(&json).expect("parse");

    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let path = format!("{out_dir}/def_golden.pdf");
    let report = build_to_file(&def, &path).expect("build");

    assert_eq!(
        report.status,
        pdfgen_profile::Status::Compliant,
        "violations: {:#?}",
        report.violations
    );

    // The same config must rebuild to the same bytes (the round-trip
    // promise behind "download the config, generate through code").
    let first = std::fs::read(&path).unwrap();
    let path2 = format!("{out_dir}/def_golden_2.pdf");
    let report2 = build_to_file(&parse_def(&json).unwrap(), &path2).unwrap();
    assert_eq!(report2.status, pdfgen_profile::Status::Compliant);
    let second = std::fs::read(&path2).unwrap();
    assert_eq!(
        first, second,
        "rebuilding the same config must produce identical bytes"
    );

    // Extraction round-trip: the logical strings come back out.
    let pages = pdfgen::extract_text(&path).unwrap();
    let all: Vec<String> = pages.iter().flat_map(|p| p.blocks.clone()).collect();
    let joined = all.join("\n");
    assert!(joined.contains("Product Requirements"), "{joined:?}");
    assert!(
        joined.contains("Pin") && joined.contains("VCC"),
        "{joined:?}"
    );
    assert!(joined.contains("Reviewed by"), "{joined:?}");
    assert!(joined.contains("Second page heading"), "{joined:?}");
    // Complex-script text extracts via ActualText in logical order.
    assert!(
        joined.contains('\u{1000}') || joined.contains('\u{1004}'),
        "Myanmar text missing from extraction: {joined:?}"
    );

    let _ = std::fs::remove_file(&path2);
}

#[test]
fn rejects_non_pdfgen_doc_envelopes() {
    let bad = br#"{"format": "something-else", "version": 1}"#;
    assert!(parse_def(bad).is_err());
}

#[test]
fn data_url_image_embeds_without_a_file() {
    // 1x1 red PNG as a data URL: the builder decodes it to a temp file,
    // loads, embeds, and cleans up.
    let png = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/red_box.png"
    ))
    .unwrap();
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    let def_json = format!(
        r#"{{
      "format": "pdfgen-doc", "version": 1, "profile": "ua1",
      "title": "Data URL", "lang": "en-US",
      "pages": [{{ "size": [612, 792], "elements": [
        {{ "type": "image",
           "src": "data:image/png;base64,{b64}",
           "alt": "Red box",
           "rect": [72, 700, 32, 32],
           "order": 1 }}
      ]}}]
    }}"#
    );
    let def = parse_def(def_json.as_bytes()).expect("parse");
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");
    let path = format!("{out_dir}/def_dataurl.pdf");
    let report = build_to_file(&def, &path).expect("build");
    assert_eq!(
        report.status,
        pdfgen_profile::Status::Compliant,
        "violations: {:#?}",
        report.violations
    );
    let data = std::fs::read(&path).unwrap();
    assert!(
        String::from_utf8_lossy(&data).contains("/Subtype /Image"),
        "image XObject missing"
    );
}
