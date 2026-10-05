//! # pdfgen-profile
//!
//! Conformance profiles and the accessibility report returned on every save.
//! A profile decides the PDF version, the XMP identifiers written into the
//! file, and the rule set used by the reporter.

pub mod xmp;

/// Document metadata required by PDF/UA.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    /// Document title (PDF/UA `dc:title`).
    pub title: Option<String>,
    /// Primary natural language (PDF/UA `/Lang`), e.g. `en-US`.
    pub lang: Option<String>,
    /// Author(s), if any.
    pub authors: Vec<String>,
    /// Brief description (PDF/UA `dc:description`).
    pub description: Option<String>,
}

/// The conformance target for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// PDF/UA-1 (ISO 14289-1), based on PDF 1.7.
    PdfUa1,
    /// PDF/UA-2 (ISO 14289-2), based on PDF 2.0.
    PdfUa2,
}

impl Profile {
    /// Core PDF version this profile writes.
    pub fn pdf_version(self) -> pdfgen_core::PdfVersion {
        match self {
            Profile::PdfUa1 => pdfgen_core::PdfVersion::V1_7,
            Profile::PdfUa2 => pdfgen_core::PdfVersion::V2_0,
        }
    }

    /// `pdfuaid:part` XMP value.
    pub fn ua_part(self) -> u32 {
        match self {
            Profile::PdfUa1 => 1,
            Profile::PdfUa2 => 2,
        }
    }

    /// Human-readable name for reports.
    pub fn name(self) -> &'static str {
        match self {
            Profile::PdfUa1 => "PDF/UA-1",
            Profile::PdfUa2 => "PDF/UA-2",
        }
    }
}

/// One accessibility issue found while saving.
#[derive(Debug, Clone)]
pub struct Violation {
    /// Checkpoint / clause identifier (Matterhorn ID for UA-1, clause for UA-2).
    pub id: String,
    /// What is wrong.
    pub message: String,
    /// One-line instruction to fix it.
    pub fix: String,
}

/// Whether the saved file claims PDF/UA conformance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// All machine checks passed; the file carries the PDF/UA identifier.
    Compliant,
    /// The file was written, but it is not yet compliant; no claim is embedded.
    NotCompliantYet,
}

/// The accessibility report returned by every save.
#[derive(Debug, Clone)]
pub struct SaveReport {
    /// Profile the document targeted.
    pub profile: Profile,
    /// Computed compliance status.
    pub status: Status,
    /// Machine-check violations (empty when compliant).
    pub violations: Vec<Violation>,
    /// Items a human must review (Matterhorn human checks applicable here).
    pub human_review: Vec<String>,
}

impl SaveReport {
    /// The user-facing note, mirroring the plan's wording.
    pub fn note(&self) -> Option<String> {
        match self.status {
            Status::Compliant => None,
            Status::NotCompliantYet => Some(format!(
                "⚠ This document is not {} compliant yet: {} issue{}{}",
                self.profile.name(),
                self.violations.len(),
                if self.violations.len() == 1 { "" } else { "s" },
                self.violations
                    .first()
                    .map(|v| format!(" (first: {} — {})", v.id, v.message))
                    .unwrap_or_default()
            )),
        }
    }
}
