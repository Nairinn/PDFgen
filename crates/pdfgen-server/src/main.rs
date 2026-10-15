//! pdfgen-server binary: serves the visual editor and the build API.
//!
//! Run with `cargo run -p pdfgen-server` (default port 8321, override
//! with `PDFGEN_PORT`).

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PDFGEN_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8321);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("pdfgen-server listening on http://{addr} (editor UI at /)");
    axum::serve(listener, pdfgen_server::router())
        .await
        .expect("serve");
}
