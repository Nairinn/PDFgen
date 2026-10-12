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

#[tokio::test]
async fn build_endpoint_returns_pdf_and_report() {
    let app = pdfgen_server::router();
    let def: Value =
        serde_json::from_slice(&std::fs::read(GOLDEN).expect("fixture")).expect("valid json");

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
    assert_eq!(parsed["report"]["status"], "compliant");
    let b64 = parsed["pdf_base64"].as_str().expect("pdf base64");
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("valid base64");
    assert!(
        bytes.starts_with(b"%PDF-"),
        "the payload must be a PDF file"
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
