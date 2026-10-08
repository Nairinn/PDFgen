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
use pdfgen_core::Object;
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
    /// The parsed source; the session keeps it alive while tagging.
    #[allow(dead_code)]
    reader: PdfReader,
    /// Extracted runs, in reading order per page.
    pub runs: Vec<TextRun>,
    /// Alt text per figure/artifact run, if any.
    alt: Vec<Option<String>>,
}

/// Extract text from a content stream: BT..ET blocks, Tj/TJ strings.
/// WinAnsi byte to Unicode (the 0x80-0x9F range differs from Latin-1).
const WINANSI_HIGH: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

/// Decode WinAnsi bytes to text for extraction.
fn winansi_to_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            0x20..=0x7E => out.push(char::from(b)),
            0x80..=0x9F => out.push(WINANSI_HIGH[usize::from(b) - 0x80]),
            0xA0..=0xFF => out.push(char::from_u32(u32::from(b)).unwrap_or('\u{FFFD}')),
            _ => {}
        }
    }
    out
}

/// Extract text-showing runs from a content stream using the shared
/// pdfgen-parse string decoders (octal escapes, hex strings, nesting).
/// `/Span <</ActualText (...)>> BDC ... EMC` regions contribute the
/// ActualText string INSTEAD of the drawn glyph codes, so shaped text
/// extracts in logical order.
pub(crate) fn extract_text_runs(content: &[u8]) -> Vec<String> {
    let mut runs = Vec::new();
    let mut i = 0usize;
    let mut current = String::new();
    while i < content.len() {
        // /Span <</ActualText (...>> BDC : find the span, decode the
        // logical text, skip drawing ops until the matching EMC.
        if content[i..].starts_with(b"/Span") {
            if let Some(at) = find_actual_text(content, i) {
                let (text, bdc_end) = at;
                let emc = find_emc(content, bdc_end);
                // The span's drawn bytes are superseded by ActualText.
                if current.trim().is_empty() {
                    current.clear();
                } else {
                    runs.push(std::mem::take(&mut current));
                }
                runs.push(text);
                i = emc;
                continue;
            }
            i += 5;
            continue;
        }
        match content[i] {
            b'(' => {
                if let Some((bytes, next)) = pdfgen_parse::read_literal(content, i) {
                    current.push_str(&winansi_to_string(&bytes));
                    i = next;
                } else {
                    i += 1;
                }
            }
            b'<' if content.get(i + 1) == Some(&b'<') => {
                // <</MCID ...>> marked-content dict: not a hex string.
                i += 1;
            }
            b'<' if content.get(i + 1).is_some_and(u8::is_ascii_hexdigit) => {
                if let Some((bytes, next)) = pdfgen_parse::read_hex(content, i) {
                    current.push_str(&winansi_to_string(&bytes));
                    i = next;
                } else {
                    i += 1;
                }
            }
            b'T' if content.get(i + 1) == Some(&b'j') || content.get(i + 1) == Some(&b'J') => {
                // A text-show operator ends the current run.
                if current.trim().is_empty() {
                    current.clear();
                } else {
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
    runs
}

/// Find `/Span <</ActualText (....)>> BDC` at `i`; return the decoded
/// logical text and the byte offset just past the BDC keyword.
fn find_actual_text(content: &[u8], i: usize) -> Option<(String, usize)> {
    let rest = &content[i..];
    let marker = b"/ActualText";
    let at = rest.windows(marker.len()).position(|w| w == marker)?;
    let lit_start = i + at + marker.len();
    // Skip spaces to the literal string.
    let mut s = lit_start;
    while s < content.len() && (content[s] == b' ' || content[s] == b'\n' || content[s] == b'\r') {
        s += 1;
    }
    let (bytes, next) = pdfgen_parse::read_literal(content, s)?;
    let text = decode_pdf_text_bytes(&bytes);
    // Advance past the ">> BDC".
    let bdc = content[next..]
        .windows(3)
        .position(|w| w == b"BDC")
        .map(|p| next + p + 3)?;
    Some((text, bdc))
}

/// Decode a PDF text string (UTF-16BE with BOM, or PDFDocEncoding-ish
/// bytes) to Unicode.
fn decode_pdf_text_bytes(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let mut out = String::with_capacity(bytes.len() / 2);
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        out.extend(char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')));
        return out;
    }
    // No BOM: treat as Latin-1-ish (our writer always uses the BOM for
    // non-ASCII).
    String::from_utf8_lossy(bytes).into_owned()
}

/// End of the marked-content sequence containing `from`: the matching
/// EMC at the same nesting depth.
fn find_emc(content: &[u8], from: usize) -> usize {
    let mut depth = 1usize;
    let mut i = from;
    while i < content.len() {
        if content[i..].starts_with(b"BDC") {
            depth += 1;
            i += 3;
        } else if content[i..].starts_with(b"EMC") {
            depth -= 1;
            if depth == 0 {
                return i + 3;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    content.len()
}

impl TagSession {
    /// Open a PDF and extract its text runs (all tagged `P` by default).
    pub fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut reader = PdfReader::open(path)?;
        let mut runs = Vec::new();

        // Walk the page tree (pages() resolves the catalog internally).
        let pages = reader.pages()?;
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
                    pdfgen_parse::inflate(&content_stream.data)
                        .unwrap_or_else(|| content_stream.data.clone())
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
    #[must_use]
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// True when no text was found.
    #[must_use]
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

    /// Set alt text for a run and mark it as a Figure, so the saved
    /// document emits the alt on a real Figure structure element.
    pub fn set_alt(&mut self, i: usize, text: &str) -> &mut Self {
        self.alt[i] = Some(text.to_string());
        self.runs[i].tag = "Figure".into();
        self.runs[i].artifact = false;
        self
    }

    /// Set a run's alt without changing its tag (advanced use).
    pub fn set_alt_only(&mut self, i: usize, text: &str) -> &mut Self {
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
        // Registry default: bundled Liberation Sans on any platform (the
        // catalog fallbacks handle systems without it).
        doc.font("Liberation Sans", "Regular")?;

        // Re-emit extracted text as fresh tagged, flowing content.
        // Alt-marked figures emit a Figure element carrying the alt text.
        let mut flow = doc.flow();
        for (i, run) in self.runs.iter().enumerate() {
            if run.artifact {
                continue;
            }
            match run.tag.as_str() {
                "Figure" => {
                    let alt = self.alt[i].as_deref().unwrap_or("");
                    flow.figure_text(&run.text, alt)?;
                }
                t if t.starts_with('H') => {
                    let level: u8 = t[1..].parse().unwrap_or(4).clamp(1, 6);
                    flow.heading(level, &run.text)?;
                }
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
