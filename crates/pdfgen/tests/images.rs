//! Image pipeline regressions (issue #7): PNG alpha becomes a real
//! /SMask (not a white flatten), JPEG component count drives the color
//! space, and RGB payloads Flate-compress.

use pdfgen::{Document, Image, Profile, Status};
use std::io::Write as _;

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/output");

/// Build a 32x32 RGBA PNG: a red square with a fully transparent
/// left half, so alpha is genuinely exercised.
fn write_alpha_png(path: &str) {
    let mut png_bytes: Vec<u8> = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 32, 32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        let mut data = Vec::with_capacity(32 * 32 * 4);
        for _ in 0..32 {
            for x in 0..32 {
                let a = if x < 16 { 0 } else { 255 };
                data.extend_from_slice(&[200, 30, 30, a]);
            }
        }
        w.write_image_data(&data).unwrap();
    }
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&png_bytes).unwrap();
}

#[test]
fn png_alpha_becomes_smask_not_white_flatten() {
    let png_path = format!("{OUT}/alpha_box.png");
    write_alpha_png(&png_path);

    let img = Image::load(&png_path).expect("alpha png loads");
    // The loader split the alpha out of the RGB.
    match &img.kind {
        pdfgen::ImageKind::Rgb { smask: Some(_), .. } => {}
        other => panic!("expected Rgb with smask, got {other:?}"),
    }

    let mut doc = Document::new(Profile::PdfUa1);
    doc.title("Alpha PNG").lang("en-US");
    doc.ensure_default_font();
    {
        let mut flow = doc.flow();
        flow.figure(&img, "A half-transparent red square", 96.0, 96.0)
            .unwrap();
    }
    let path = format!("{OUT}/alpha_smask.pdf");
    let report = doc.save(&path).unwrap();
    assert_eq!(
        report.status,
        Status::Compliant,
        "violations: {:#?}",
        report.violations
    );

    // The PDF must carry a real /SMask on the image XObject.
    let data = std::fs::read(&path).unwrap();
    let txt = String::from_utf8_lossy(&data);
    assert!(txt.contains("/SMask"), "no /SMask in the XObject");
    assert!(txt.contains("/DeviceGray"), "SMask must be DeviceGray");
    // And the image data must be Flate-compressed, not raw.
    assert!(txt.contains("/FlateDecode"), "RGB payload not compressed");

    let _ = std::fs::remove_file(&png_path);
}

#[test]
fn opaque_png_has_no_smask() {
    let img = Image::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/red_box.png"
    ))
    .expect("fixture png");
    match &img.kind {
        pdfgen::ImageKind::Rgb { smask: None, .. } => {}
        other => panic!("opaque png must not grow an smask: {other:?}"),
    }
}
