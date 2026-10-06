//! Text extraction: pull text blocks out of any PDF, tagged or not.
//!
//! Works through the same reader the retag session uses: page tree walk,
//! FlateDecode-aware content stream decoding, then BT/ET text-showing
//! operators. Each page yields its blocks in content order.

use crate::retag;
use pdfgen_core::Object;
use pdfgen_parse::PdfReader;

/// Text blocks extracted from one page, in content order.
#[derive(Debug, Clone)]
pub struct PageText {
    /// Page index (0-based).
    pub page: usize,
    /// Text blocks on the page.
    pub blocks: Vec<String>,
}

/// Extract text from every page of a PDF file.
pub fn extract_text(path: &str) -> Result<Vec<PageText>, Box<dyn std::error::Error>> {
    let mut reader = PdfReader::open(path)?;
    let catalog = reader.catalog()?;
    let Some(Object::Ref(pages_ref)) = catalog.get("Pages").cloned() else {
        return Err("catalog has no /Pages".into());
    };
    let pages = retag::collect_page_refs(&mut reader, pages_ref.id)?;

    let mut out = Vec::with_capacity(pages.len());
    for (pi, &page_id) in pages.iter().enumerate() {
        let Object::Dict(page) = reader.get(page_id)? else {
            continue;
        };
        let Some(Object::Ref(contents_ref)) = page.get("Contents").cloned() else {
            continue;
        };
        let Object::Stream(stream) = reader.get(contents_ref.id)? else {
            continue;
        };
        let data: Vec<u8> = match stream.dict.get("Filter") {
            Some(Object::Name(n)) if n.0 == "FlateDecode" => {
                retag::inflate(&stream.data).unwrap_or_else(|| stream.data.clone())
            }
            _ => stream.data.clone(),
        };
        out.push(PageText {
            page: pi,
            blocks: retag::extract_text_runs(&data),
        });
    }
    Ok(out)
}
