//! Kotlin/Java bindings for pdfgen via UniFFI (proc-macro interface).
//!
//! The generated Kotlin (JNA-based) works on Java 11+, satisfying the
//! "Kotlin and older-Java" requirement; the same library can be driven
//! from Java 22+ FFM bindings later.

use pdfgen_api::{PdfDocument, TargetProfile};
use std::sync::Arc;

/// Conformance profile.
#[derive(uniffi::Enum, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// PDF/UA-1 (PDF 1.7).
    Ua1,
    /// PDF/UA-2 (PDF 2.0).
    Ua2,
}

impl From<Profile> for TargetProfile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Ua1 => TargetProfile::Ua1,
            Profile::Ua2 => TargetProfile::Ua2,
        }
    }
}

/// One accessibility violation: (id, message, fix).
#[derive(uniffi::Record)]
pub struct Violation {
    /// Matterhorn or clause ID.
    pub id: String,
    /// What is wrong.
    pub message: String,
    /// How to fix it.
    pub fix: String,
}

/// The save report.
#[derive(uniffi::Record)]
pub struct Report {
    /// Profile name, e.g. "PDF/UA-1".
    pub profile_name: String,
    /// True when all machine checks passed and the file claims PDF/UA.
    pub compliant: bool,
    /// Machine-check violations.
    pub violations: Vec<Violation>,
    /// Human-review checklist items.
    pub human_review: Vec<String>,
    /// The user-facing note; empty when compliant.
    pub note: String,
}

/// A PDF/UA document under construction. Cheap to clone; methods take
/// `&self` so it is thread-safe to share.
#[derive(uniffi::Object)]
pub struct Document {
    inner: PdfDocument,
}

#[uniffi::export]
impl Document {
    /// New document targeting a profile.
    #[uniffi::constructor]
    #[must_use] 
    pub fn new(profile: Profile) -> Arc<Self> {
        Arc::new(Document {
            inner: PdfDocument::new(profile.into()),
        })
    }

    /// New document with title and language set.
    #[uniffi::constructor]
    #[must_use] 
    pub fn new_with(profile: Profile, title: String, lang: String) -> Arc<Self> {
        let inner = PdfDocument::new(profile.into());
        inner.set_title(&title);
        inner.set_lang(&lang);
        Arc::new(Document { inner })
    }

    /// Set the document title.
    pub fn set_title(&self, title: String) {
        self.inner.set_title(&title);
    }

    /// Set the primary natural language, e.g. "en-US".
    pub fn set_lang(&self, lang: String) {
        self.inner.set_lang(&lang);
    }

    /// Resolve a font by family and style; returns a font handle.
    pub fn font(&self, family: String, style: String) -> Result<u32, PdfgenError> {
        self.inner.font(&family, &style).map_err(PdfgenError::from)
    }

    /// Load a font file directly by path.
    pub fn load_font_file(&self, path: String) -> Result<u32, PdfgenError> {
        self.inner.load_font_file(&path).map_err(PdfgenError::from)
    }

    /// Register a font file under a family name for later font() calls.
    pub fn register_font(&self, family: String, style: String, path: String) {
        self.inner.register_font(&family, &style, &path);
    }

    /// Add a heading (level 1-6).
    pub fn heading(&self, level: u8, text: String) -> Result<(), PdfgenError> {
        self.inner.heading(level, &text).map_err(PdfgenError::from)
    }

    /// Add a paragraph in the default font.
    pub fn paragraph(&self, text: String) -> Result<(), PdfgenError> {
        self.inner.paragraph(&text).map_err(PdfgenError::from)
    }

    /// Add a paragraph in a specific font handle and size.
    pub fn paragraph_in(&self, font: u32, size: f64, text: String) -> Result<(), PdfgenError> {
        self.inner
            .paragraph_in(font, size, &text)
            .map_err(PdfgenError::from)
    }

    /// Add a bullet list.
    pub fn bullet_list(&self, items: Vec<String>) -> Result<(), PdfgenError> {
        self.inner.bullet_list(items).map_err(PdfgenError::from)
    }

    /// Add a table. `widths` may be empty for equal columns.
    pub fn table(
        &self,
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        widths: Vec<f64>,
    ) -> Result<(), PdfgenError> {
        self.inner
            .table(header, rows, widths)
            .map_err(PdfgenError::from)
    }

    /// Place an image file (PNG/JPEG) as a figure with alt text. Empty alt
    /// marks the image decorative.
    pub fn figure(
        &self,
        path: String,
        alt: String,
        width: f64,
        height: f64,
    ) -> Result<(), PdfgenError> {
        self.inner
            .figure(&path, &alt, width, height)
            .map_err(PdfgenError::from)
    }

    /// Page header text (pagination artifact).
    pub fn page_header(&self, text: String) -> Result<(), PdfgenError> {
        self.inner.page_header(&text).map_err(PdfgenError::from)
    }

    /// Page footer text (pagination artifact).
    pub fn page_footer(&self, text: String) -> Result<(), PdfgenError> {
        self.inner.page_footer(&text).map_err(PdfgenError::from)
    }

    /// Save the file (ALWAYS writes it) and return the report.
    pub fn save(&self, path: String) -> Result<Report, PdfgenError> {
        self.inner
            .save(&path)
            .map(report_from_api)
            .map_err(PdfgenError::from)
    }
}

fn report_from_api(r: pdfgen_api::Report) -> Report {
    Report {
        profile_name: r.profile_name,
        compliant: r.compliant,
        violations: r
            .violations
            .into_iter()
            .map(|(id, message, fix)| Violation { id, message, fix })
            .collect(),
        human_review: r.human_review,
        note: r.note,
    }
}

/// Errors surface as exceptions in Kotlin/Java.
#[derive(uniffi::Error, Debug, thiserror::Error)]
pub enum PdfgenError {
    /// Something failed; `msg` says what.
    #[error("{msg}")]
    General {
        /// Human-readable error message.
        msg: String,
    },
}

impl From<String> for PdfgenError {
    fn from(msg: String) -> Self {
        PdfgenError::General { msg }
    }
}

/// Library version metadata.
#[derive(uniffi::Record)]
pub struct VersionInfo {
    /// Crate version string.
    pub version: String,
}

/// Library version metadata.
#[uniffi::export]
#[must_use] 
pub fn version() -> VersionInfo {
    VersionInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

uniffi::setup_scaffolding!();
