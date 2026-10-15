//! Server integration tests: drive the router without spawning a
//! process. The build request uses the golden pdfgen-doc fixture, so
//! the test doubles as an end-to-end check of the HTTP path.

use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../pdfgen-def/tests/fixtures/golden_doc.json"
);
const OUT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/output/server_golden.pdf"
);

async fn build_via_api(app: axum::Router, def: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(&serde_json::json!({ "def": def })).unwrap();
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/build")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(payload))
                .unwrap(),
        )
        .await
        .expect("oneshot");
    assert_eq!(res.status(), 200, "build must succeed");
    let body = res.into_body().collect().await.expect("body").to_bytes();
    let parsed: Value = serde_json::from_slice(&body).expect("json response");
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(parsed["pdf_base64"].as_str().expect("pdf base64"))
        .expect("valid base64")
}

#[tokio::test]
async fn build_endpoint_returns_pdf_and_report() {
    let app = pdfgen_server::router();
    let def: Value =
        serde_json::from_slice(&std::fs::read(GOLDEN).expect("fixture")).expect("valid json");

    let bytes = build_via_api(app, &def).await;
    assert!(
        bytes.starts_with(b"%PDF-"),
        "the payload must be a PDF file"
    );
    // Keep the server-path output in tests/output/ so the veraPDF gate
    // covers the HTTP build like every other fixture.
    std::fs::write(OUT, &bytes).expect("write the gate fixture");
}

#[tokio::test]
async fn same_config_rebuilds_byte_identical_and_matches_library() {
    // The byte-identical claim over HTTP: two server builds of the same
    // config, then a direct library build, all produce the same bytes.
    let app = pdfgen_server::router();
    let def: Value =
        serde_json::from_slice(&std::fs::read(GOLDEN).expect("fixture")).expect("valid json");

    let first = build_via_api(app.clone(), &def).await;
    let second = build_via_api(app, &def).await;
    assert_eq!(
        first, second,
        "two server builds of the same config differ (nondeterminism?)"
    );

    let json = std::fs::read(GOLDEN).expect("fixture");
    let parsed = pdfgen_def::parse_def(&json).expect("valid pdfgen-doc");
    let lib_out = std::env::temp_dir().join("pdfgen-server-bi-lib.pdf");
    let _report =
        pdfgen_def::build_to_file(&parsed, lib_out.to_str().unwrap()).expect("library build");
    let lib = std::fs::read(lib_out).expect("read the library output");
    assert_eq!(
        first, lib,
        "the server and the library produce different bytes for the same config"
    );
}

#[tokio::test]
async fn build_endpoint_rejects_empty_bodies() {
    let app = pdfgen_server::router();
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/build")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&serde_json::json!({})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .expect("oneshot");
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn fonts_endpoint_lists_catalog() {
    let app = pdfgen_server::router();
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/fonts")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");
    assert_eq!(res.status(), 200);
    let body = res.into_body().collect().await.expect("body").to_bytes();
    let fonts: Vec<String> = serde_json::from_slice(&body).expect("json array");
    assert!(fonts.iter().any(|f| f.contains("Liberation")));
}
