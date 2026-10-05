//! # pdfgen-core
//!
//! The PDF object model and byte-level serializer. Everything else in the
//! workspace (writer, parser, validator, revisions) is built on these types.
//!
//! * [`Object`] — the seven PDF object types plus indirect references.
//! * [`Document`] — allocates object numbers and serializes a complete file
//!   (header, body, cross-reference table, trailer).
//!
//! This crate knows nothing about tagging, fonts or accessibility; it is
//! deliberately boring and correct.

mod doc;
mod error;
mod object;
mod serialize;

pub use doc::Document;
pub use error::CoreError;
pub use object::{Dict, Name, Object, PdfString, Real, Ref, Stream};
pub use serialize::fmt_real;

/// Version of the PDF specification a file targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfVersion {
    /// PDF 1.7 (ISO 32000-1) — required for PDF/UA-1.
    V1_7,
    /// PDF 2.0 (ISO 32000-2) — required for PDF/UA-2.
    V2_0,
}

impl PdfVersion {
    /// Header string written after `%PDF-`, e.g. `1.7`.
    pub fn header(self) -> &'static str {
        match self {
            PdfVersion::V1_7 => "1.7",
            PdfVersion::V2_0 => "2.0",
        }
    }
}
