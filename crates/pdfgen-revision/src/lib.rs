//! # pdfgen-revision
//!
//! Version control for PDFs, stored inside the file itself:
//!
//! * [`commit`] — append an incremental-save revision with a message and
//!   author. The original bytes are untouched (signatures stay valid); the
//!   revision record travels in the new trailer's `/PDFgenRev` entry.
//! * [`history`] — list every revision with its message, author, date.
//! * [`diff`] — object-level diff between two revisions.
//! * [`revert`] — exact byte-restore of any revision to a new file.
//!
//! v1 mechanism: each commit writes a FULL cross-reference table (legal
//! and keeps our reader simple), a revision-record object, and a trailer
//! with `/Prev` chained to the previous section. Revisions are delimited
//! by `%%EOF` markers; any revision can be read by truncating the file at
//! its marker — which is exactly what [`revert`] exploits for byte-exact
//! restores.

use pdfgen_core::Object;
use pdfgen_parse::PdfReader;
use std::path::Path;

/// One revision as listed by [`history`].
#[derive(Debug, Clone)]
pub struct RevisionEntry {
    /// 1-based revision index (1 = the original file).
    pub index: usize,
    /// Commit message ("initial revision" when the file has none yet).
    pub message: String,
    /// Author (empty when unknown).
    pub author: String,
    /// Commit date, RFC 3339 UTC.
    pub date: String,
    /// Length of the file at this revision, in bytes.
    pub byte_len: u64,
}

/// Information about a commit just made.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    /// The new revision's index.
    pub index: usize,
    /// The message recorded.
    pub message: String,
    /// The author recorded.
    pub author: String,
    /// The commit date (RFC 3339 UTC).
    pub date: String,
}

/// One object-level difference between two revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffKind {
    /// The object exists only in the later revision.
    Added,
    /// The object exists only in the earlier revision.
    Removed,
    /// The object's content differs.
    Changed,
}

/// One entry of a diff report.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    /// Object number.
    pub obj: u32,
    /// What happened.
    pub kind: DiffKind,
}

/// Append a new revision to a PDF file with a commit message.
///
/// The file's existing bytes are never rewritten — the revision is an
/// incremental-save section appended after the current `%%EOF`.
pub fn commit(path: impl AsRef<Path>, message: &str, author: &str) -> Result<CommitInfo, String> {
    let path = path.as_ref();
    let data = std::fs::read(path).map_err(|e| e.to_string())?;

    let reader = PdfReader::from_bytes(data.clone()).map_err(|e| e.to_string())?;
    let trailer = reader.trailer().clone();
    let size = match trailer.get("Size") {
        Some(Object::Int(s)) => *s,
        _ => {
            return Err("trailer has no /Size".into());
        }
    };
    if size < 1 {
        return Err("invalid /Size".into());
    }
    let record_num = size as u32; // first free object number
    let new_size = record_num + 1;

    // Existing objects (offsets) for the full table.
    let mut offsets: Vec<(u32, u64)> = reader
        .object_offsets()
        .into_iter()
        .filter(|(id, _)| *id < record_num)
        .collect();
    offsets.sort_unstable();

    // Previous xref offset: the last `startxref` value in the current bytes.
    let prev_xref = last_startxref(&data).unwrap_or(0);

    // Revision index: existing %%EOF count + 1.
    let rev_index = count_eofs(&data) + 1;
    let date = now_rfc3339();

    // --- Build the appended section -----------------------------------
    let base = data.len() as u64;
    let mut section: Vec<u8> = Vec::with_capacity(4096);
    let mut cursor = base;

    // Revision record object.
    let mut record = pdfgen_core::Dict::new();
    record.set("N", rev_index as i64);
    record.set(
        "Message",
        Object::String(pdfgen_core::PdfString::text(message)),
    );
    record.set(
        "Author",
        Object::String(pdfgen_core::PdfString::text(author)),
    );
    record.set("Date", Object::String(pdfgen_core::PdfString::text(&date)));
    let mut head = format!("{record_num} 0 obj\n");
    pdfgen_core::write_object(&mut head, &Object::Dict(record));
    head.push_str("\nendobj\n");
    section.extend_from_slice(head.as_bytes());
    let record_off = cursor;
    cursor += head.len() as u64;

    // Full cross-reference table (slot 0 + one per object id < new_size).
    let mut xref = String::with_capacity(new_size as usize * 20 + 32);
    xref.push_str(&format!("xref\n0 {new_size}\n"));
    xref.push_str("0000000000 65535 f \n");
    let mut map: std::collections::HashMap<u32, u64> =
        std::collections::HashMap::from_iter(offsets.iter().cloned());
    map.insert(record_num, record_off);
    for id in 1..new_size {
        match map.get(&id) {
            Some(off) => xref.push_str(&format!("{off:010} 00000 n \n")),
            None => xref.push_str("0000000000 65535 f \n"),
        }
    }
    let mut trailer_new = pdfgen_core::Dict::new();
    trailer_new.set("Size", new_size as i64);
    if let Some(root) = trailer.get("Root") {
        trailer_new.set("Root", root.clone());
    } else {
        return Err("trailer has no /Root".into());
    }
    if prev_xref > 0 {
        trailer_new.set("Prev", prev_xref as i64);
    }
    if let Some(id_arr) = trailer.get("ID") {
        trailer_new.set("ID", id_arr.clone());
    }
    if let Some(info) = trailer.get("Info") {
        trailer_new.set("Info", info.clone());
    }
    trailer_new.set("PDFgenRev", Object::Ref(pdfgen_core::Ref::new(record_num)));
    xref.push_str("trailer\n");
    pdfgen_core::write_object(&mut xref, &Object::Dict(trailer_new));
    let xref_off = cursor;
    xref.push_str(&format!("\nstartxref\n{xref_off}\n%%EOF\n"));
    section.extend_from_slice(xref.as_bytes());

    // --- Append ---------------------------------------------------------
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&section).map_err(|e| e.to_string())?;

    Ok(CommitInfo {
        index: rev_index,
        message: message.to_string(),
        author: author.to_string(),
        date,
    })
}

/// List every revision recorded in a file, oldest first.
pub fn history(path: impl AsRef<Path>) -> Result<Vec<RevisionEntry>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let ends = eof_positions(&data);
    let mut out = Vec::with_capacity(ends.len());
    for (i, end) in ends.iter().enumerate() {
        let prefix = &data[..*end];
        let (message, author, date) = revision_meta(prefix).unwrap_or_default();
        out.push(RevisionEntry {
            index: i + 1,
            message,
            author,
            date,
            byte_len: *end as u64,
        });
    }
    Ok(out)
}

/// Object-level diff between two revisions (1-based indexes).
pub fn diff(path: impl AsRef<Path>, from: usize, to: usize) -> Result<Vec<DiffEntry>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let ends = eof_positions(&data);
    if from < 1 || to < 1 || from > ends.len() || to > ends.len() {
        return Err(format!("revision out of range (1..={})", ends.len()));
    }
    let mut old = parse_prefix(&data[..ends[from - 1]])?;
    let mut new = parse_prefix(&data[..ends[to - 1]])?;

    let old_ids: std::collections::BTreeSet<u32> =
        old.object_offsets().into_iter().map(|(id, _)| id).collect();
    let new_ids: std::collections::BTreeSet<u32> =
        new.object_offsets().into_iter().map(|(id, _)| id).collect();

    let mut out = Vec::new();
    for id in old_ids.union(&new_ids) {
        let in_old = old_ids.contains(id);
        let in_new = new_ids.contains(id);
        if in_old && !in_new {
            out.push(DiffEntry {
                obj: *id,
                kind: DiffKind::Removed,
            });
        } else if !in_old && in_new {
            out.push(DiffEntry {
                obj: *id,
                kind: DiffKind::Added,
            });
        } else {
            let a = old.get(*id).map_err(|e| e.to_string())?;
            let b = new.get(*id).map_err(|e| e.to_string())?;
            if a != b {
                out.push(DiffEntry {
                    obj: *id,
                    kind: DiffKind::Changed,
                });
            }
        }
    }
    Ok(out)
}

/// Restore an earlier revision byte-for-byte into a new file.
pub fn revert(path: impl AsRef<Path>, to: usize, out_path: impl AsRef<Path>) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let ends = eof_positions(&data);
    if to < 1 || to > ends.len() {
        return Err(format!("revision out of range (1..={})", ends.len()));
    }
    std::fs::write(out_path, &data[..ends[to - 1]]).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- helpers

/// Byte end positions (exclusive) of every `%%EOF` marker, including any
/// trailing EOL — truncating at one yields that revision exactly.
fn eof_positions(data: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 5 <= data.len() {
        if &data[i..i + 5] == b"%%EOF" {
            let mut end = i + 5;
            // Swallow one EOL (LF, or CRLF).
            if data.get(end) == Some(&b'\r') {
                end += 1;
            }
            if data.get(end) == Some(&b'\n') {
                end += 1;
            }
            out.push(end);
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn count_eofs(data: &[u8]) -> usize {
    eof_positions(data).len()
}

/// The numeric value of the LAST `startxref` in the bytes, if present.
fn last_startxref(data: &[u8]) -> Option<u64> {
    let tail_start = data.len().saturating_sub(2048);
    let tail = &data[tail_start..];
    let mut last = None;
    let mut i = 0usize;
    while i + 9 <= tail.len() {
        if &tail[i..i + 9] == b"startxref" {
            last = Some(tail_start + i);
        }
        i += 1;
    }
    let pos = last?;
    let digits: String = String::from_utf8_lossy(&data[pos + 9..])
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u64>().ok()
}

/// Parse a revision prefix into a reader.
fn parse_prefix(prefix: &[u8]) -> Result<PdfReader, String> {
    PdfReader::from_bytes(prefix.to_vec()).map_err(|e| e.to_string())
}

/// (message, author, date) of the LAST revision in this prefix, if it has
/// a `/PDFgenRev` record.
fn revision_meta(prefix: &[u8]) -> Option<(String, String, String)> {
    let mut reader = parse_prefix(prefix).ok()?;
    let trailer = reader.trailer();
    let rev_ref = match trailer.get("PDFgenRev") {
        Some(Object::Ref(r)) => *r,
        _ => return None, // original file: no record
    };
    let record = reader.get(rev_ref.id).ok()?;
    let dict = match record {
        Object::Dict(d) => d,
        _ => return None,
    };
    let get_str = |key: &str| -> String {
        match dict.get(key) {
            Some(Object::String(s)) => pdfgen_core::decode_text_bytes(&s.0),
            _ => String::new(),
        }
    };
    Some((get_str("Message"), get_str("Author"), get_str("Date")))
}

/// Current time as RFC 3339 UTC, no external crates.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Days-since-epoch to (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
