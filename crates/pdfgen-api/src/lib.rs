//! # pdfgen-api
//!
//! Bindings-friendly facade over [`pdfgen`]. Same capability, but shaped for
//! FFI generators (UniFFI, PyO3):
//!
//! * every type is owned (`Arc` handles, `Vec`s, plain data);
//! * no lifetimes, no closures, no builder generics;
//! * errors are simple strings;
//! * one flat namespace of methods per object.
//!
//! Both the Kotlin and Python bindings wrap this crate, so the public
//! surface stays identical across languages.

use pdfgen::{Document, Profile, SaveReport as CoreReport, Status};
use std::sync::Arc;

/// Conformance profile selector for bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetProfile {
    /// PDF/UA-1 (PDF 1.7).
    Ua1,
    /// PDF/UA-2 (PDF 2.0).
    Ua2,
}

impl From<TargetProfile> for Profile {
    fn from(t: TargetProfile) -> Self {
        match t {
            TargetProfile::Ua1 => Profile::PdfUa1,
            TargetProfile::Ua2 => Profile::PdfUa2,
        }
    }
}

/// Save report as plain data for bindings.
#[derive(Debug, Clone)]
pub struct Report {
    /// Profile name, e.g. "PDF/UA-1".
    pub profile_name: String,
    /// True when all machine checks passed and the file claims PDF/UA.
    pub compliant: bool,
    /// Violations: (id, message, fix).
    pub violations: Vec<(String, String, String)>,
    /// Human-review checklist items.
    pub human_review: Vec<String>,
    /// The note shown to users; empty when compliant.
    pub note: String,
}

impl From<&CoreReport> for Report {
    fn from(r: &CoreReport) -> Self {
        Report {
            profile_name: r.profile.name().to_string(),
            compliant: r.status == Status::Compliant,
            violations: r
                .violations
                .iter()
                .map(|v| (v.id.clone(), v.message.clone(), v.fix.clone()))
                .collect(),
            human_review: r.human_review.clone(),
            note: r.note().unwrap_or_default(),
        }
    }
}

/// A document under construction. Cheap to clone (shared handle).
#[derive(Clone)]
pub struct PdfDocument {
    inner: Arc<std::sync::Mutex<Document>>,
}

impl PdfDocument {
    /// New document targeting a profile.
    pub fn new(profile: TargetProfile) -> Self {
        PdfDocument {
            inner: Arc::new(std::sync::Mutex::new(Document::new(profile.into()))),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Document> {
        self.inner.lock().expect("pdfgen document mutex poisoned")
    }

    /// Set the document title.
    pub fn set_title(&self, title: &str) {
        self.lock().title(title);
    }

    /// Set the primary natural language, e.g. "en-US".
    pub fn set_lang(&self, lang: &str) {
        self.lock().lang(lang);
    }

    /// Resolve a font by family + style; returns a handle index.
    /// Uses the registry (system fonts, built-in catalog, aliases).
    pub fn font(&self, family: &str, style: &str) -> Result<u32, String> {
        self.lock()
            .font(family, style)
            .map(|i| i as u32)
            .map_err(|e| e.to_string())
    }

    /// Load a font file directly by path.
    pub fn load_font_file(&self, path: &str) -> Result<u32, String> {
        self.lock()
            .load_font(path)
            .map(|i| i as u32)
            .map_err(|e| e.to_string())
    }

    /// Register a font file under a family name for later `font()` calls.
    pub fn register_font(&self, family: &str, style: &str, path: &str) {
        self.lock().register_font(family, style, path);
    }

    /// Heading of level 1-6 in the default font.
    pub fn heading(&self, level: u8, text: &str) -> Result<(), String> {
        self.lock().flow_heading(level, text).map_err(str_err)
    }

    /// Paragraph in the default font.
    pub fn paragraph(&self, text: &str) -> Result<(), String> {
        self.lock().flow_paragraph(text).map_err(str_err)
    }

    /// Paragraph in a specific font handle and size.
    pub fn paragraph_in(&self, font: u32, size: f64, text: &str) -> Result<(), String> {
        self.lock()
            .flow_paragraph_in(font as usize, size, text)
            .map_err(str_err)
    }

    /// Bullet list.
    pub fn bullet_list(&self, items: Vec<String>) -> Result<(), String> {
        let borrowed: Vec<&str> = items.iter().map(String::as_str).collect();
        self.lock().flow_bullet_list(&borrowed).map_err(str_err)
    }

    /// Table: header row + body rows.
    pub fn table(
        &self,
        header: Vec<String>,
        rows: Vec<Vec<String>>,
        widths: Vec<f64>,
    ) -> Result<(), String> {
        let h: Vec<&str> = header.iter().map(String::as_str).collect();
        let body: Vec<Vec<&str>> = rows
            .iter()
            .map(|r| r.iter().map(String::as_str).collect())
            .collect();
        let w: Vec<f64> = if widths.is_empty() { vec![] } else { widths };
        self.lock().flow_table(&h, &body, &w).map_err(str_err)
    }

    /// Load an image file (PNG/JPEG) and place it as a figure with alt
    /// text. Empty alt marks the image decorative.
    pub fn figure(&self, path: &str, alt: &str, width: f64, height: f64) -> Result<(), String> {
        self.lock()
            .flow_figure(path, alt, width, height)
            .map_err(|e| e.to_string())
    }

    /// Page header text (drawn as a pagination artifact).
    pub fn page_header(&self, text: &str) -> Result<(), String> {
        self.lock().flow_header(text).map_err(str_err)
    }

    /// Page footer text (drawn as a pagination artifact).
    pub fn page_footer(&self, text: &str) -> Result<(), String> {
        self.lock().flow_footer(text).map_err(str_err)
    }

    /// Save to a path. ALWAYS writes the file; compliance issues go into
    /// the returned report and the file carries no PDF/UA claim.
    pub fn save(&self, path: &str) -> Result<Report, String> {
        let report = self.lock().save(path).map_err(|e| e.to_string())?;
        Ok(Report::from(&report))
    }
}

fn str_err(e: pdfgen::FontError) -> String {
    e.to_string()
}
