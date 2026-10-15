//! pdfloom-server: the visual editor's backend.
//!
//! The library half of the server: DTOs, handlers and the router
//! (so tests can drive it without spawning a process).
//!
//! * `POST /api/build` — body: a `pdfloom-doc` JSON definition.
//!   Response: `{ "pdf_base64": ..., "report": ... }`.
//! * `GET /` and `/static` — the editor SPA in `web/`.
//! * `GET /api/fonts` — the bundled catalog families for the editor's
//!   font pickers.
//!
//! Run with `cargo run -p pdfloom-server` (default port 8321, override
//! with `PDFLOOM_PORT`).

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use std::path::PathBuf;

/// Wire form of a save violation.
#[derive(serde::Serialize)]
pub struct ViolationDto {
    /// Checkpoint/clause id (Matterhorn ID for UA-1).
    id: String,
    /// What is wrong.
    message: String,
    /// One-line fix instruction.
    fix: String,
}

/// Wire form of the save report.
#[derive(serde::Serialize)]
pub struct ReportDto {
    /// "compliant" or "not_compliant_yet" (lowercase wire form)
    status: String,
    /// Profile name, e.g. "PDF/UA-1".
    profile_name: String,
    /// User-facing note; null when compliant.
    note: Option<String>,
    /// Machine-check violations (empty when compliant).
    violations: Vec<ViolationDto>,
    /// Human-review checklist items.
    human_review: Vec<String>,
}

impl From<pdfloom_profile::SaveReport> for ReportDto {
    fn from(r: pdfloom_profile::SaveReport) -> Self {
        ReportDto {
            status: format!("{:?}", r.status).to_lowercase(),
            profile_name: r.profile.name().to_string(),
            note: r.note(),
            violations: r
                .violations
                .iter()
                .map(|v| ViolationDto {
                    id: v.id.clone(),
                    message: v.message.clone(),
                    fix: v.fix.clone(),
                })
                .collect(),
            human_review: r.human_review.clone(),
        }
    }
}

/// The API response for a build.
#[derive(serde::Serialize)]
pub struct BuildResponse {
    /// The generated PDF, standard base64.
    pdf_base64: String,
    /// The save report (compliance status, violations, review notes).
    report: ReportDto,
}

#[derive(serde::Deserialize)]
struct BuildRequest {
    /// Inline document definition (the editor state itself).
    #[serde(default)]
    def: Option<serde_json::Value>,
    /// OR: a base64-encoded `pdfloom-doc` config file upload.
    #[serde(default)]
    config_base64: Option<String>,
}

/// Build the definition to a temp file and return the bytes + report.
pub fn build_definition(json: &[u8]) -> Result<(Vec<u8>, pdfloom_profile::SaveReport), String> {
    let def = pdfloom_def::parse_def(json).map_err(|e| e.to_string())?;
    let tmp = std::env::temp_dir().join(format!(
        "pdfloom-server-{}.pdf",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let path = tmp
        .to_str()
        .ok_or_else(|| "temp path not UTF-8".to_string())?
        .to_string();
    let report = pdfloom_def::build_to_file(&def, &path).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp);
    Ok((bytes, report))
}

async fn api_build(
    body: axum::extract::Json<BuildRequest>,
) -> Result<Json<BuildResponse>, (StatusCode, String)> {
    let json = if let Some(def) = &body.def {
        serde_json::to_vec(def).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    } else if let Some(b64) = &body.config_base64 {
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("bad base64: {e}")))?
    } else {
        return Err((StatusCode::BAD_REQUEST, "no def or config_base64".into()));
    };
    let (bytes, report) =
        build_definition(&json).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e))?;
    Ok(Json(BuildResponse {
        pdf_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        report: report.into(),
    }))
}

async fn api_fonts() -> impl IntoResponse {
    // The catalog families the editor offers in its font pickers.
    Json([
        "Liberation Sans",
        "Liberation Serif",
        "Liberation Mono",
        "Noto Sans",
        "Noto Serif",
        "Noto Sans Mono",
        "Noto Sans Myanmar",
        "Noto Sans Thai",
        "Noto Sans Arabic",
        "Noto Sans CJK JP",
        "Atkinson Hyperlegible",
        "OpenDyslexic",
    ])
}

async fn api_health() -> &'static str {
    "ok"
}

fn web_dir() -> PathBuf {
    // Repo root from CARGO_MANIFEST_DIR: crates/pdfloom-server/../../web
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../web")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("web"))
}

/// The full application router (also used by the tests).
pub fn router() -> Router {
    let serve = tower_http::services::ServeDir::new(web_dir());
    Router::new()
        .route("/api/build", post(api_build))
        .route("/api/fonts", get(api_fonts))
        .route("/api/health", get(api_health))
        .fallback_service(serve)
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
}
