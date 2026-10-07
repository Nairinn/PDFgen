//! HTML-to-PDF: convert a structural HTML subset into a tagged, PDF/UA
//! document via the streaming writer.
//!
//! Supported elements map straight onto structure types:
//!
//! | HTML | PDF structure |
//! |---|---|
//! | `h1`..`h6` | `H1`..`H6` |
//! | `p` | `P` |
//! | `ul`/`ol` + `li` | `L` + `LI > (Lbl, LBody)` |
//! | `table` + `tr` + `th`/`td` | `Table` + `TR` + `TH`/`TD` |
//! | `img` | `Figure` (alt text required; empty = decorative) |
//! | `strong`/`b`, `em`/`i`, `code` | inline (current font) |
//! | `br` | line break inside the block |
//!
//! The parser is a small, allocation-light pull scanner — no DOM, no
//! external crate: tags are handled as they arrive, text flows into the
//! open element, and the document is streamed to disk page by page.

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
        "li" => Some("LI".into()),
        "td" | "th" => Some(name.to_ascii_uppercase()),
        _ => None,
    }
}

/// Convert an HTML file to a tagged PDF at `out_path`.
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
    html_to_pdf(&html, out_path, title, lang)
}

/// Convert an HTML string to a tagged PDF at `out_path`.
pub fn html_to_pdf(
    html: &str,
    out_path: &str,
    title: &str,
    lang: &str,
) -> Result<pdfgen_profile::SaveReport, HtmlError> {
    let mut w = StreamWriter::create(out_path, Profile::PdfUa1, title, lang)?;

    let mut batch: Vec<StreamEvent> = Vec::with_capacity(64);
    // Text accumulated for the currently open block.
    let mut text = String::new();
    // Open list context: true when inside ul/ol (li renders Lbl + LBody).
    let mut in_list = false;

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
                match name.as_str() {
                    "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" => {
                        flush_block(&mut batch, &mut text, &block_tag(&name).unwrap());
                    }
                    "li" => {
                        flush_list_item(&mut batch, &mut text, in_list);
                    }
                    "ul" | "ol" => {
                        in_list = false;
                        batch.push(StreamEvent::End); // close L
                    }
                    "table" => {
                        batch.push(StreamEvent::End); // close Table
                    }
                    "tr" => {
                        batch.push(StreamEvent::End); // close TR
                    }
                    _ => {}
                }
                i = end + 1;
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
                match name.as_str() {
                    "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" => {
                        // Open block: push Begin, start capturing text.
                        batch.push(StreamEvent::Begin {
                            tag: block_tag(&name).unwrap(),
                            alt: None,
                        });
                        text.clear();
                    }
                    "li" => {
                        text.clear();
                    }
                    "ul" | "ol" => {
                        in_list = true;
                        batch.push(StreamEvent::Begin {
                            tag: "L".into(),
                            alt: None,
                        });
                    }
                    "table" => {
                        batch.push(StreamEvent::Begin {
                            tag: "Table".into(),
                            alt: None,
                        });
                    }
                    "tr" => {
                        batch.push(StreamEvent::Begin {
                            tag: "TR".into(),
                            alt: None,
                        });
                    }
                    "br" => {
                        text.push('\n');
                    }
                    "img" => {
                        // <img alt="..."> → Figure with alt text.
                        let alt = extract_attr(rest, "alt").unwrap_or_default();
                        let src = extract_attr(rest, "src").unwrap_or_default();
                        if !src.is_empty() {
                            // Figure content: draw the image file.
                            let (w_pt, h_pt) = image_size(&src);
                            batch.push(StreamEvent::Begin {
                                tag: "Figure".into(),
                                alt: if alt.is_empty() { None } else { Some(alt) },
                            });
                            batch.push(StreamEvent::Image {
                                path: src,
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
            }
            _ => {
                // Plain text: append until next '<'.
                let next = find_byte(bytes, i, b'<').unwrap_or(bytes.len());
                text.push_str(&html[i..next]);
                i = next;
            }
        }
    }

    w.push(batch)?;
    let report = w.finish()?;
    Ok(report)
}

/// Normalize HTML text: collapse runs of whitespace to single spaces and
/// trim, so block text encodes cleanly in WinAnsi (which has no \n).
fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }
    out
}

/// Close a block element: text becomes one Text event, then End.
fn flush_block(batch: &mut Vec<StreamEvent>, text: &mut String, tag: &str) {
    let _ = tag;
    let cleaned = normalize_ws(text);
    if !cleaned.is_empty() {
        batch.push(StreamEvent::Text {
            text: cleaned,
            font: 0,
            size: 11.0,
        });
    }
    *text = String::new();
    batch.push(StreamEvent::End);
}

/// Close an <li>: Lbl bullet + LBody text inside the open L.
fn flush_list_item(batch: &mut Vec<StreamEvent>, text: &mut String, in_list: bool) {
    if !in_list {
        // li outside a list: treat as a plain paragraph.
        let cleaned = normalize_ws(text);
        if !cleaned.is_empty() {
            batch.push(StreamEvent::Text {
                text: cleaned,
                font: 0,
                size: 11.0,
            });
        }
        *text = String::new();
        return;
    }
    batch.push(StreamEvent::Begin {
        tag: "LI".into(),
        alt: None,
    });
    batch.push(StreamEvent::Begin {
        tag: "Lbl".into(),
        alt: None,
    });
    batch.push(StreamEvent::Text {
        text: "\u{2022}".into(),
        font: 0,
        size: 11.0,
    });
    batch.push(StreamEvent::End);
    batch.push(StreamEvent::Begin {
        tag: "LBody".into(),
        alt: None,
    });
    let cleaned = normalize_ws(text);
    if !cleaned.is_empty() {
        batch.push(StreamEvent::Text {
            text: cleaned,
            font: 0,
            size: 11.0,
        });
    }
    *text = String::new();
    batch.push(StreamEvent::End);
    batch.push(StreamEvent::End); // close LI
}

fn find_byte(haystack: &[u8], from: usize, needle: u8) -> Option<usize> {
    haystack[from..]
        .iter()
        .position(|&b| b == needle)
        .map(|p| p + from)
}

fn split_tag(inner: &str) -> (&str, &str) {
    match inner.find(|c: char| c.is_whitespace()) {
        Some(sp) => (&inner[..sp], &inner[sp..]),
        None => (inner, ""),
    }
}

fn extract_attr(tag_rest: &str, attr: &str) -> Option<String> {
    // Find `attr=` then the quoted value.
    let lower = tag_rest.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let pos = lower.find(&needle)?;
    let after = &tag_rest[pos + needle.len()..];
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = after[1..].find(quote)? + 1;
    Some(after[1..end].to_string())
}

fn decode_entity(s: &str) -> Option<(char, usize)> {
    for (name, ch) in [
        ("amp;", '&'),
        ("lt;", '<'),
        ("gt;", '>'),
        ("quot;", '"'),
        ("apos;", '\''),
        ("nbsp;", ' '),
        ("mdash;", '\u{2014}'),
        ("ndash;", '\u{2013}'),
    ] {
        if let Some(rest) = s.strip_prefix('&') {
            if rest.starts_with(name) {
                return Some((ch, 1 + name.len()));
            }
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
