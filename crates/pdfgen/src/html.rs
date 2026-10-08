//! HTML-to-PDF: convert a structural HTML subset into a tagged, PDF/UA
//! document via the streaming writer.
//!
//! Supported elements map straight onto structure types:
//!
//! | HTML | PDF structure |
//!|---|---|
//! | `h1`..`h6` | `H1`..`H6` |
//! | `p` | `P` |
//! | `ul`/`ol` + `li` | `L` + `LI > (Lbl, LBody)` |
//! | `table` + `tr` + `th`/`td` | `Table` + `TR` + `TH`/`TD` |
//! | `img` | `Figure` (alt text required; empty = decorative) |
//! | `strong`/`b`, `em`/`i`, `code` | inline (current font) |
//! | `br` | line break inside the block |
//!
//! Fidelity rules: bare text outside any block element is wrapped in an
//! implied `P`; `<!-- comments -->`, `<script>` and `<style>` contents are
//! excluded; numeric entities (`&#233;`, `&#x2014;`) decode; attribute
//! matching is boundary-safe (`data-alt` is not `alt`) and unquoted values
//! parse; `<ol>` items number themselves; `img src` resolves against the
//! HTML file's directory in `html_file_to_pdf`.
//!
//! The parser is a small, allocation-light pull scanner — no DOM, no
//! external crate.

use crate::stream::{StreamEvent, StreamWriter};
use pdfgen_profile::Profile;
use std::io::Read;

/// Errors from the HTML converter.
#[derive(Debug, thiserror::Error)]
pub enum HtmlError {
    /// Reading the HTML source failed.
    #[error("html IO: {0}")]
    Io(String),
    /// Malformed HTML (unclosed tag, bad nesting).
    #[error("html: {0}")]
    Parse(String),
    /// The underlying stream writer failed.
    #[error("pdf: {0}")]
    Pdf(#[from] crate::stream::StreamError),
}

/// Tag names that start a block-level structure element.
fn block_tag(name: &str) -> Option<String> {
    match name {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(name.to_ascii_uppercase()),
        "p" => Some("P".into()),
        "td" | "th" => Some(name.to_ascii_uppercase()),
        _ => None,
    }
}

/// Elements that end a run of bare (implied-paragraph) text.
fn is_block_boundary(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "td"
            | "th"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "tr"
            | "img"
            | "div"
            | "body"
            | "html"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "aside"
            | "nav"
            | "blockquote"
            | "pre"
            | "figure"
    )
}

/// Convert an HTML file to a tagged PDF at `out_path`. Relative `img src`
/// paths resolve against the HTML file's directory.
pub fn html_file_to_pdf(
    html_path: &str,
    out_path: &str,
    title: &str,
    lang: &str,
) -> Result<pdfgen_profile::SaveReport, HtmlError> {
    let mut html = String::new();
    std::fs::File::open(html_path)
        .and_then(|mut f| f.read_to_string(&mut html))
        .map_err(|e| HtmlError::Io(e.to_string()))?;
    let base = std::path::Path::new(html_path)
        .parent()
        .map(std::path::Path::to_path_buf);
    html_to_pdf_inner(
        &html,
        out_path,
        Profile::PdfUa1,
        title,
        lang,
        base.as_deref(),
    )
}

/// Convert an HTML string to a tagged PDF at `out_path` (PDF/UA-1).
pub fn html_to_pdf(
    html: &str,
    out_path: &str,
    title: &str,
    lang: &str,
) -> Result<pdfgen_profile::SaveReport, HtmlError> {
    html_to_pdf_profiled(html, out_path, Profile::PdfUa1, title, lang)
}

/// Convert an HTML string to a tagged PDF at `out_path`, with an
/// explicit accessibility profile (UA-1 or UA-2).
pub fn html_to_pdf_profiled(
    html: &str,
    out_path: &str,
    profile: Profile,
    title: &str,
    lang: &str,
) -> Result<pdfgen_profile::SaveReport, HtmlError> {
    html_to_pdf_inner(html, out_path, profile, title, lang, None)
}

fn html_to_pdf_inner(
    html: &str,
    out_path: &str,
    profile: Profile,
    title: &str,
    lang: &str,
    base_dir: Option<&std::path::Path>,
) -> Result<pdfgen_profile::SaveReport, HtmlError> {
    let mut w = StreamWriter::create(out_path, profile, title, lang)?;

    let mut batch: Vec<StreamEvent> = Vec::with_capacity(64);
    // Text accumulated for the currently open block.
    let mut text = String::new();
    // An explicit p/h/td/th block is capturing text.
    let mut block_open = false;
    // Bare text accumulated outside any structure (implied paragraph).
    let mut implied_pending = false;
    // Inside ul/ol (li renders Lbl + LBody).
    let mut in_list = false;
    let mut list_ordered = false;
    let mut li_counter = 0usize;
    // Inside a table (suppresses implied paragraphs between cells).
    let mut in_table = false;

    let bytes = html.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'<' if bytes.get(i + 1) == Some(&b'/') => {
                // Closing tag.
                let end = match find_byte(bytes, i + 2, b'>') {
                    Some(e) => e,
                    None => return Err(HtmlError::Parse("unclosed end tag".into())),
                };
                let name = html[i + 2..end].trim().to_ascii_lowercase();
                if implied_pending && is_block_boundary(&name) {
                    flush_implied(&mut batch, &mut text);
                    implied_pending = false;
                }
                match name.as_str() {
                    "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" => {
                        if block_open {
                            flush_block(&mut batch, &mut text);
                            block_open = false;
                        }
                    }
                    "li" => {
                        let label = if list_ordered {
                            format!("{li_counter}.")
                        } else {
                            "\u{2022}".to_string()
                        };
                        flush_list_item(&mut batch, &mut text, in_list, &label);
                    }
                    "ul" | "ol" => {
                        in_list = false;
                        batch.push(StreamEvent::End); // close L
                    }
                    "table" => {
                        in_table = false;
                        batch.push(StreamEvent::End); // close Table
                    }
                    "tr" => {
                        batch.push(StreamEvent::End); // close TR
                    }
                    _ => {}
                }
                i = end + 1;
            }
            b'<' if bytes.get(i + 1) == Some(&b'!') => {
                // Comment or doctype: skip entirely.
                if html[i..].starts_with("<!--") {
                    i = find_seq(bytes, i + 4, b"-->").map_or(bytes.len(), |e| e + 3);
                } else {
                    i = find_byte(bytes, i + 1, b'>').map_or(bytes.len(), |e| e + 1);
                }
            }
            b'<' => {
                // Opening tag.
                let end = match find_byte(bytes, i + 1, b'>') {
                    Some(e) => e,
                    None => return Err(HtmlError::Parse("unclosed tag".into())),
                };
                let inner = &html[i + 1..end];
                let (name_raw, rest) = split_tag(inner);
                let name = name_raw.to_ascii_lowercase();
                if implied_pending && is_block_boundary(&name) {
                    flush_implied(&mut batch, &mut text);
                    implied_pending = false;
                }
                match name.as_str() {
                    "script" | "style" => {
                        // Skip the element's contents up to its closing tag.
                        let close = format!("</{name}");
                        i = match find_ci(bytes, i, close.as_bytes()) {
                            Some(c) => find_byte(bytes, c, b'>').map_or(bytes.len(), |e| e + 1),
                            None => bytes.len(),
                        };
                        continue;
                    }
                    "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" => {
                        if let Some(tag) = block_tag(&name) {
                            batch.push(StreamEvent::Begin {
                                tag,
                                alt: None,
                                attrs: None,
                            });
                            text.clear();
                            block_open = true;
                        }
                    }
                    "li" => {
                        text.clear();
                        if in_list && list_ordered {
                            li_counter += 1;
                        }
                    }
                    "ul" => {
                        in_list = true;
                        list_ordered = false;
                        // UA-2 (ISO 14289-2 8.2.5.25): L carries ListNumbering.
                        batch.push(StreamEvent::Begin {
                            tag: "L".into(),
                            alt: None,
                            attrs: Some(vec![("ListNumbering".into(), "Disc".into())]),
                        });
                    }
                    "ol" => {
                        in_list = true;
                        list_ordered = true;
                        li_counter = 0;
                        batch.push(StreamEvent::Begin {
                            tag: "L".into(),
                            alt: None,
                            attrs: Some(vec![("ListNumbering".into(), "Decimal".into())]),
                        });
                    }
                    "table" => {
                        in_table = true;
                        batch.push(StreamEvent::Begin {
                            tag: "Table".into(),
                            alt: None,
                            attrs: None,
                        });
                    }
                    "tr" => {
                        batch.push(StreamEvent::Begin {
                            tag: "TR".into(),
                            alt: None,
                            attrs: None,
                        });
                    }
                    "br" => {
                        text.push('\n');
                    }
                    "img" => {
                        // <img alt="..."> → Figure with alt text.
                        let alt = extract_attr(rest, "alt")
                            .map(|v| decode_entities(&v))
                            .unwrap_or_default();
                        let src = extract_attr(rest, "src")
                            .map(|v| decode_entities(&v))
                            .unwrap_or_default();
                        if !src.is_empty() {
                            let resolved = resolve_src(&src, base_dir);
                            let (w_pt, h_pt) = image_size(&resolved);
                            batch.push(StreamEvent::Begin {
                                tag: "Figure".into(),
                                alt: if alt.is_empty() { None } else { Some(alt) },
                                attrs: None,
                            });
                            batch.push(StreamEvent::Image {
                                path: resolved,
                                width: w_pt,
                                height: h_pt,
                            });
                            batch.push(StreamEvent::End);
                        }
                    }
                    _ => {
                        // Inline tags (strong/em/code...) are transparent:
                        // their text flows into the open block.
                    }
                }
                i = end + 1;
            }
            b'&' => {
                // Entity: decode the common ones; others pass through.
                if let Some((ch, len)) = decode_entity(&html[i..]) {
                    text.push(ch);
                    i += len;
                } else {
                    text.push('&');
                    i += 1;
                }
                if !block_open && !in_list && !in_table && !text.trim().is_empty() {
                    implied_pending = true;
                }
            }
            _ => {
                // Plain text: append until the next '<' or '&' (so
                // entities reach the decoder instead of being swallowed
                // as raw text).
                let next_lt = find_byte(bytes, i, b'<').unwrap_or(bytes.len());
                let next_amp = find_byte(bytes, i, b'&').unwrap_or(bytes.len());
                let next = next_lt.min(next_amp);
                text.push_str(&html[i..next]);
                if !block_open && !in_list && !in_table && !text.trim().is_empty() {
                    implied_pending = true;
                }
                i = next;
            }
        }
    }

    // EOF: flush anything still capturing.
    if implied_pending {
        flush_implied(&mut batch, &mut text);
    }
    if block_open {
        flush_block(&mut batch, &mut text);
    }

    w.push(batch)?;
    let report = w.finish()?;
    Ok(report)
}

/// Resolve an img src against the HTML file's directory when relative.
fn resolve_src(src: &str, base_dir: Option<&std::path::Path>) -> String {
    let is_absolute =
        src.starts_with('/') || src.contains("://") || std::path::Path::new(src).is_absolute();
    match base_dir {
        Some(dir) if !is_absolute => dir.join(src).to_string_lossy().into_owned(),
        _ => src.to_string(),
    }
}

/// Normalize HTML text: collapse runs of spaces and trim, but keep
/// explicit `\n` (from `<br>`) as hard line breaks so the wrapper
/// splits there.
fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for ch in s.chars() {
        if ch == '\n' {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            pending_space = false;
        } else if ch.is_whitespace() {
            if !out.is_empty() && !out.ends_with('\n') {
                pending_space = true;
            }
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }
    while out.ends_with('\n') || out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Close a block element: text becomes one Text event, then End.
fn flush_block(batch: &mut Vec<StreamEvent>, text: &mut String) {
    let cleaned = normalize_ws(text);
    if !cleaned.is_empty() {
        batch.push(StreamEvent::Text {
            text: cleaned,
            font: 0,
            size: 11.0,
        });
    }
    text.clear();
    batch.push(StreamEvent::End);
}

/// Flush bare (loose) text as an implied paragraph.
fn flush_implied(batch: &mut Vec<StreamEvent>, text: &mut String) {
    let cleaned = normalize_ws(text);
    if !cleaned.is_empty() {
        batch.push(StreamEvent::Begin {
            tag: "P".into(),
            alt: None,
            attrs: None,
        });
        batch.push(StreamEvent::Text {
            text: cleaned,
            font: 0,
            size: 11.0,
        });
        batch.push(StreamEvent::End);
    }
    text.clear();
}

/// Close an <li>: Lbl (bullet or number) + LBody text inside the open L.
fn flush_list_item(batch: &mut Vec<StreamEvent>, text: &mut String, in_list: bool, label: &str) {
    if !in_list {
        // li outside a list: treat as a plain paragraph.
        flush_implied(batch, text);
        return;
    }
    batch.push(StreamEvent::Begin {
        tag: "LI".into(),
        alt: None,
        attrs: None,
    });
    batch.push(StreamEvent::Begin {
        tag: "Lbl".into(),
        alt: None,
        attrs: None,
    });
    batch.push(StreamEvent::Text {
        text: label.to_string(),
        font: 0,
        size: 11.0,
    });
    batch.push(StreamEvent::End);
    batch.push(StreamEvent::Begin {
        tag: "LBody".into(),
        alt: None,
        attrs: None,
    });
    let cleaned = normalize_ws(text);
    if !cleaned.is_empty() {
        batch.push(StreamEvent::Text {
            text: cleaned,
            font: 0,
            size: 11.0,
        });
    }
    text.clear();
    batch.push(StreamEvent::End);
    batch.push(StreamEvent::End); // close LI
}

fn find_byte(haystack: &[u8], from: usize, needle: u8) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .iter()
        .position(|&b| b == needle)
        .map(|p| p + from)
}

fn find_seq(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() || needle.is_empty() || haystack.len() - from < needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Case-insensitive byte-sequence search (for `</script>` etc.).
fn find_ci(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= haystack.len() || needle.is_empty() || haystack.len() - from < needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
        .map(|p| p + from)
}

fn split_tag(inner: &str) -> (&str, &str) {
    match inner.find(char::is_whitespace) {
        Some(sp) => (&inner[..sp], &inner[sp..]),
        None => (inner, ""),
    }
}

/// Extract a quoted or unquoted attribute value. The attribute name must
/// start at a word boundary, so `data-alt` does not match `alt`.
fn extract_attr(tag_rest: &str, attr: &str) -> Option<String> {
    let lower = tag_rest.to_ascii_lowercase();
    let lbytes = lower.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(attr) {
        let pos = from + rel;
        let boundary_ok = pos == 0 || lbytes[pos - 1].is_ascii_whitespace();
        let eq = pos + attr.len();
        if boundary_ok && lbytes.get(eq) == Some(&b'=') {
            let mut v = eq + 1;
            while v < lbytes.len() && lbytes[v].is_ascii_whitespace() {
                v += 1;
            }
            let rest = &tag_rest[v..];
            if let Some(q @ ('"' | '\'')) = rest.chars().next() {
                let end = rest[1..].find(q)? + 1;
                return Some(rest[1..end].to_string());
            }
            // Unquoted value: ends at whitespace or tag end.
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
        from = pos + attr.len();
    }
    None
}

/// Decode entities in an attribute value.
fn decode_entities(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        if bytes[i] == b'&' {
            if let Some((ch, len)) = decode_entity(&s[i..]) {
                out.push(ch);
                i += len;
            } else {
                out.push('&');
                i += 1;
            }
        } else {
            let next = find_byte(bytes, i, b'&').unwrap_or(s.len());
            out.push_str(&s[i..next]);
            i = next;
        }
    }
    out
}

/// Decode one entity at the start of `s` (which begins with `&`).
/// Named and numeric (`&#233;`, `&#x2014;`) forms.
fn decode_entity(s: &str) -> Option<(char, usize)> {
    if let Some(rest) = s.strip_prefix("&#") {
        let (radix, digits, prefix_len) = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            (16u32, hex, 3usize)
        } else {
            (10u32, rest, 2usize)
        };
        let end = digits.find(';')?;
        let n = u32::from_str_radix(&digits[..end], radix).ok()?;
        let ch = char::from_u32(n).unwrap_or('\u{fffd}');
        return Some((ch, prefix_len + end + 1));
    }
    for (name, ch) in [
        ("amp;", '&'),
        ("lt;", '<'),
        ("gt;", '>'),
        ("quot;", '"'),
        ("apos;", '\''),
        ("nbsp;", ' '),
        ("mdash;", '\u{2014}'),
        ("ndash;", '\u{2013}'),
        ("hellip;", '\u{2026}'),
        ("copy;", '\u{a9}'),
        ("reg;", '\u{ae}'),
        ("trade;", '\u{2122}'),
    ] {
        if s.strip_prefix('&')?.starts_with(name) {
            return Some((ch, 1 + name.len()));
        }
    }
    None
}

/// Natural size of an image file in points (72dpi), capped to the page.
fn image_size(path: &str) -> (f64, f64) {
    match crate::image::Image::load(path) {
        Ok(img) => {
            let w = f64::from(img.w) * 72.0 / 96.0;
            let h = f64::from(img.h) * 72.0 / 96.0;
            (w.min(468.0), h.min(648.0))
        }
        Err(_) => (96.0, 96.0),
    }
}
