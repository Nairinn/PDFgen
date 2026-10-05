//! # pdfgen-parse
//!
//! The PDF reader. Takes raw bytes and yields fully resolved objects:
//!
//! * classic cross-reference tables, xref streams, and hybrid files;
//! * lazy resolution of indirect references with cycle protection;
//! * repair mode: when the xref is broken, scan for `N 0 obj` headers.
//!
//! It intentionally shares the [`Object`] model with the writer
//! (pdfgen-core), so read-modify-write is a single data flow.

mod lexer;
mod reader;

pub use lexer::{Lexer, Token};
pub use reader::{PdfReader, RepairInfo};

use thiserror::Error;

/// Something went wrong while reading a PDF.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Not a PDF (bad header).
    #[error("not a PDF: missing or malformed header")]
    BadHeader,
    /// A `%%EOF` marker could not be found.
    #[error("cannot find %%EOF marker")]
    NoEof,
    /// Neither `startxref` offset nor an xref stream could be located.
    #[error("cannot locate cross-reference data")]
    NoXref,
    /// The xref offset pointed at something that is not an xref.
    #[error("xref offset {0} does not point at a cross-reference")]
    BadXrefOffset(u64),
    /// An object number is not present in any xref section.
    #[error("object {0} not found")]
    ObjectNotFound(u32),
    /// A stream's declared /Length did not match actual bytes.
    #[error("stream {0}: /Length mismatch (declared {1})")]
    BadStreamLength(u32, u64),
    /// Malformed object syntax.
    #[error("malformed object at byte {0}: {1}")]
    Malformed(usize, String),
    /// I/O error reading the file.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
