//! # pdfgen
//!
//! A free, MIT-licensed, from-scratch PDF library with PDF/UA accessibility
//! built in. This facade crate assembles the lower-level crates into a
//! document API:
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use pdfgen::{Document, Profile};
//!
//! let mut doc = Document::new(Profile::PdfUa1);
//! doc.title("Hello").lang("en-US");
//! doc.load_font("/path/to/font.ttf")?;
//!
//! let mut page = doc.add_page(612.0, 792.0);
//! page.heading(1, "Hello, tagged world!")?;
//! page.paragraph("This PDF has a real structure tree.")?;
//!
//! let report = doc.save("hello.pdf")?;
//! println!("{}", report.note().unwrap_or_else(|| "PDF/UA compliant".into()));
//! # Ok(())
//! # }
//! ```

mod document;
mod extract;
mod flow;
mod form;
mod html;
mod image;
mod page;
mod retag;
mod stream;
mod structure;
mod tounicode;

pub use document::Document;
pub use extract::{extract_text, PageText};
pub use flow::Flow;
pub use form::fill_text_field;
pub use html::{html_file_to_pdf, html_to_pdf, HtmlError};
pub use image::{Image, ImageError, ImageKind};
pub use page::Page;
pub use stream::{StreamError, StreamEvent, StreamWriter};
pub use structure::Node;
pub use retag::{TagSession, TextRun};
pub use pdfgen_font::{FontError, LoadedFont};
pub use pdfgen_profile::{Metadata, Profile, SaveReport, Status, Violation};
pub use pdfgen_parse::{ParseError, PdfReader};

/// WinAnsi helpers re-exported for callers that encode text themselves.
pub use pdfgen_font::winansi;

/// Re-exports from the core object model, for advanced use.
pub use pdfgen_core::{Dict, Name, Object, PdfString, PdfVersion, Ref, Stream};
