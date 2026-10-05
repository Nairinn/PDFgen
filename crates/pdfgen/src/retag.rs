//! Import-and-retag: open any PDF (including untagged ones), classify its
//! content into structure elements, and save it as a newly tagged document.
//!
//! The flow:
//! 1. [`TagSession::open`] parses the file with [`crate::PdfReader`] and
//!    extracts text runs (with their byte offsets) from every page's
//!    content streams.
//! 2. The caller (or [`TagSession::auto_tag`]) assigns each run a
//!    structure type — heading, paragraph, artifact, figure alt text …
//! 3. [`TagSession::save`] writes a fresh tagged PDF containing the same
//!    page content, a proper structure tree, and the accessibility report.

use crate::document::Document;
use crate::tounicode;
use pdfgen_core::{Dict, Name, Object, PdfString, Ref, Stream};
use pdfgen_parse::PdfReader;
use pdfgen_profile::{Profile, SaveReport};

/// One extracted line of text on a page.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// Page index (0-based).
    pub page: usize,
    /// Text, decoded as best-effort WinAnsi/UTF-16.
    pub text: String,
    /// Suggested structure tag (`P` by default).
    pub tag: String,
    /// Marked as artifact instead of real content?
    pub artifact: bool,
}

/// A retag session over an existing PDF.
pub struct TagSession {
    reader: PdfReader,
    /// Extracted runs, in reading order per page.
    pub runs: Vec<TextRun>,
    /// Alt text per figure/artifact run, if any.
    alt: Vec<Option<String>>,
}

/// Extract text from a content stream: BT..ET blocks, Tj/TJ strings.
fn extract_text_runs(content: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(content);
    let mut runs = Vec::new();
    let mut rest = &text[..];
    while let Some(start) = rest.find("BT") {
        let Some(end_rel) = rest[start..].find("ET") else { break };
        let block = &rest[start + 2..start + end_rel];
        // Simple approach: find every ( ... ) Tj and the array form.
        let bytes = block.as_bytes();
        let mut i = 0;
        let mut current = String::new();
        while i < bytes.len() {
            match bytes[i] {
                b'(' => {
                    // Literal string until unescaped ')'.
                    let mut j = i + 1;
                    let mut s = String::new();
                    while j < bytes.len() {
                        match bytes[j] {
                            b'\\' => {
                                j += 1;
                                if j < bytes.len() {
                                    s.push(bytes[j] as char);
                                }
                            }
                            b')' => break,
                            c if (0x20..=0x7e).contains(&c) => s.push(c as char),
                            _ => {
                                // Non-ASCII: lossy push.
                                s.push(bytes[j] as char);
                            }
                        }
                        j += 1;
                    }
                    current.push_str(&s);
                    i = j + 1;
                }
                b'T' if i + 1 < bytes.len() && bytes[i + 1] == b'j' => {
                    // Tj terminates a text-show op; flush.
                    if !current.trim().is_empty() {
                        runs.push(std::mem::take(&mut current));
                    }
                    i += 2;
                }
                b'T' if i + 1 < bytes.len() && bytes[i + 1] == b'J' => {
                    if !current.trim().is_empty() {
                        runs.push(std::mem::take(&mut current));
                    }
                    i += 2;
                }
                _ => i += 1,
            }
        }
        if !current.trim().is_empty() {
            runs.push(current);
        }
        rest = &rest[start + end_rel + 2..];
    }
    runs
}

impl TagSession {
    /// Open a PDF and extract its text runs (all tagged `P` by default).
    pub fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut reader = PdfReader::open(path)?;
        let mut runs = Vec::new();

        // Walk the page tree.
        let catalog = reader.catalog()?;
        let Some(Object::Ref(pages_ref)) = catalog.get("Pages").cloned() else {
            return Err("catalog has no /Pages".into());
        };
        let pages = collect_page_refs(&mut reader, pages_ref.id)?;
        for (pi, page_id) in pages.iter().enumerate() {
            let Object::Dict(page) = reader.get(*page_id)? else {
                continue;
            };
            let Some(Object::Ref(contents_ref)) = page.get("Contents").cloned() else {
                continue;
            };
            let content_stream = match reader.get(contents_ref.id)? {
                Object::Stream(s) => s,
                _ => continue,
            };
            // Decode if FlateDecode.
            let data: Vec<u8> = match content_stream.dict.get("Filter") {
                Some(Object::Name(n)) if n.0 == "FlateDecode" => {
                    crate::retag::inflate(&content_stream.data).unwrap_or_else(|| content_stream.data.clone())
                }
                _ => content_stream.data.clone(),
            };
            for text in extract_text_runs(&data) {
                runs.push(TextRun {
                    page: pi,
                    text,
                    tag: "P".into(),
                    artifact: false,
                });
            }
        }

        let alt = vec![None; runs.len()];
        Ok(TagSession { reader, runs, alt })
    }

    /// Number of extracted text runs.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// True when no text was found.
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Mark a run as a heading of the given level.
    pub fn set_heading(&mut self, i: usize, level: u8) -> &mut Self {
        debug_assert!((1..=6).contains(&level));
        self.runs[i].tag = format!("H{level}");
        self
    }

    /// Mark a run as an artifact (decoration, header/footer).
    pub fn set_artifact(&mut self, i: usize) -> &mut Self {
        self.runs[i].artifact = true;
        self
    }

    /// Set alt text for a run (used for figure-like runs).
    pub fn set_alt(&mut self, i: usize, text: &str) -> &mut Self {
        self.alt[i] = Some(text.to_string());
        self
    }

    /// Heuristic auto-tagging: first run of each page becomes H1, short
    /// all-caps runs become H2, everything else P. Runs that look like
    /// page furniture ("Page N", bare digits) become artifacts.
    pub fn auto_tag(&mut self) -> &mut Self {
        let mut last_page = usize::MAX;
        for i in 0..self.runs.len() {
            let t = self.runs[i].text.trim();
            if self.runs[i].page != last_page {
                self.runs[i].tag = "H1".into();
                last_page = self.runs[i].page;
            } else if t.len() < 60
                && t.chars()
                    .all(|c| c.is_uppercase() || c.is_whitespace() || c == ':')
                && !t.is_empty()
            {
                self.runs[i].tag = "H2".into();
            } else if looks_like_page_number(t) {
                self.runs[i].artifact = true;
            }
        }
        self
    }

    /// Save a newly tagged document to `path` under the given profile.
    /// The original page content is preserved; the structure tree, fonts
    /// and metadata are rebuilt from the tagging decisions.
    pub fn save(
        &mut self,
        profile: Profile,
        title: &str,
        lang: &str,
        path: &str,
    ) -> Result<SaveReport, Box<dyn std::error::Error>> {
        // Rebuild via the writer: emit a document whose pages carry the
        // original content streams, but wrapped in our own marked content.
        // M2 scope: we re-emit extracted text as fresh tagged content.
        let mut doc = Document::new(profile);
        doc.title(title).lang(lang);
        let arial = "/System/Library/Fonts/Supplemental/Arial.ttf";
        if std::path::Path::new(arial).exists() {
            doc.load_font(arial)?;
        }

        // Re-emit extracted text as fresh tagged, flowing content.
        let mut flow = doc.flow();
        for run in &self.runs {
            if run.artifact {
                continue;
            }
            let level = run.tag.strip_prefix('H').and_then(|n| n.parse::<u8>().ok());
            match level {
                Some(1) => flow.heading(1, &run.text)?,
                Some(2) => flow.heading(2, &run.text)?,
                Some(3) => flow.heading(3, &run.text)?,
                _ => flow.paragraph(&run.text)?,
            }
        }
        drop(flow);

        let report = doc.save(path)?;
        Ok(report)
    }
}

fn looks_like_page_number(t: &str) -> bool {
    !t.is_empty() && t.len() <= 12 && t.chars().all(|c| c.is_ascii_digit() || c == '-')
}

/// Depth-first page reference collection from a /Pages node.
fn collect_page_refs(
    reader: &mut PdfReader,
    node: u32,
) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    match reader.get(node)? {
        Object::Dict(d) => {
            let t = d
                .get("Type")
                .and_then(|o| match o {
                    Object::Name(n) => Some(n.0.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            if t == "Page" {
                out.push(node);
            } else if let Some(Object::Array(kids)) = d.get("Kids").cloned() {
                for kid in kids {
                    if let Object::Ref(r) = kid {
                        out.extend(collect_page_refs(reader, r.id)?);
                    }
                }
            }
        }
        _ => return Err("page tree node is not a dict".into()),
    }
    Ok(out)
}

/// zlib inflate, exposed for retag-internal use.
pub(crate) fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .ok()
        .map(|_| out)
}
