//! Byte-level serialization of PDF objects and whole files.

use crate::{Dict, Name, Object, PdfString, Real, Ref, Stream};

/// Format a real number the way PDF requires: plain decimal, no exponent,
/// no trailing zero past the decimal point, max 4 fractional digits.
pub fn fmt_real(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let rounded = (v * 10_000.0).round() / 10_000.0;
    if rounded.fract() == 0.0 && rounded.abs() < 9.0e15 {
        return format!("{}", rounded as i64);
    }
    let mut s = format!("{rounded:.4}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}
/// Characters that force `#xx` escaping inside a name.
fn name_needs_escape(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#')
        || !(0x21..=0x7e).contains(&b)
}

fn write_name(out: &mut String, name: &str) {
    out.push('/');
    for b in name.bytes() {
        if name_needs_escape(b) {
            out.push_str(&format!("#{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
}

fn write_string(out: &mut String, s: &PdfString) {
    out.push('(');
    for &b in &s.0 {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
}

fn write_dict(out: &mut String, dict: &Dict) {
    out.push_str("<<");
    let mut first = true;
    for (k, v) in &dict.0 {
        if !first {
            out.push(' ');
        }
        first = false;
        write_name(out, &k.0);
        out.push(' ');
        write_object(out, v);
    }
    out.push_str(">>");
}

/// Serialize any object in PDF syntax.
pub fn write_object(out: &mut String, obj: &Object) {
    match obj {
        Object::Null => out.push_str("null"),
        Object::Bool(true) => out.push_str("true"),
        Object::Bool(false) => out.push_str("false"),
        Object::Int(i) => out.push_str(&i.to_string()),
        Object::Real(Real(v)) => out.push_str(&fmt_real(*v)),
        Object::String(s) => write_string(out, s),
        Object::Name(Name(n)) => write_name(out, n),
        Object::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_object(out, item);
            }
            out.push(']');
        }
        Object::Dict(d) => write_dict(out, d),
        Object::Stream(_) => {
            // Streams are always indirect in a PDF file; they are serialized
            // by the document writer. Reaching here is a programming error.
            out.push_str("<<STREAM-MUST-BE-INDIRECT>>");
        }
        Object::Ref(r) => out.push_str(&r.to_string()),
    }
}

/// Write a stream header: `<<dict with /Length>> stream\n`. The caller splices
/// the raw bytes next, then `\nendstream` (see `Document::serialize`).
pub fn write_stream(out: &mut String, stream: &Stream) {
    let mut d = stream.dict.clone();
    d.set("Length", stream.data.len() as i64);
    write_dict(out, &d);
    out.push_str("\nstream\n");
}

/// Free-list head entry for xref slot 0.
pub fn free_head_entry() -> String {
    "0000000000 65535 f \n".to_string()
}

/// One 20-byte cross-reference entry for an in-use object.
pub fn xref_entry(offset: u64, gen: u32) -> String {
    format!("{offset:010} {gen:05} n \n")
}
