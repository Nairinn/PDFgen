//! # pdfgen-ffi
//!
//! C ABI over [`pdfgen_api`], designed for Java 22+ FFM (foreign function
//! & memory API) and any other C-compatible FFI. Conventions:
//!
//! * Objects are opaque pointers (`*mut` handles) created and destroyed on
//!   the Rust side; callers never touch struct layout.
//! * Errors are returned as a heap-allocated NUL-terminated UTF-8 string
//!   owned by Rust — release it with [`pdfgen_free`]. NULL means success.
//! * Strings passed IN are borrowed for the call only (`&str` from
//!   `char*`); strings passed OUT are Rust-owned until freed.
//! * All functions tolerate NULL handles/errors gracefully (return an
//!   error string rather than crash).
//!
//! The Java side uses `java.lang.foreign` (Java 22+), so no JNA/JNI
//! overhead; calls go straight through the standard linker.

use pdfgen_api::{PdfDocument, Report, TargetProfile};
use std::ffi::{c_char, CStr, CString};

// ------------------------------------------------------------------ utils

/// Turn a C string into a Rust &str (lossy on invalid UTF-8).
unsafe fn borrow<'a>(s: *const c_char) -> Result<&'a str, *mut c_char> {
    if s.is_null() {
        return Err(err("null string argument"));
    }
    let c = unsafe { CStr::from_ptr(s) };
    match c.to_str() {
        Ok(s) => Ok(s),
        Err(_) => Err(err("string is not valid UTF-8")),
    }
}

/// Allocate the error message for return to the caller.
fn err(msg: &str) -> *mut c_char {
    CString::new(msg).unwrap_or_default().into_raw()
}

/// Turn a `Result<T, String>` into (out-param, error) per the conventions
/// above: on success write `out` and return NULL; on failure return the
/// message.
unsafe fn ok_or_err<T>(r: Result<T, String>, out: *mut T) -> *mut c_char {
    match r {
        Ok(v) => {
            if !out.is_null() {
                unsafe { *out = v };
            }
            std::ptr::null_mut()
        }
        Err(e) => err(&e),
    }
}

// ------------------------------------------------------------- document

/// Create a new document for the given profile: 0 = PDF/UA-1, 1 = PDF/UA-2.
///
/// On success `out` receives the handle. Destroy it with
/// [`pdfgen_destroy`]. Never block: an untagged/incomplete document is a
/// save-time report, not an error here.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_create(profile: u32, out: *mut *mut PdfDocument) -> *mut c_char {
    let p = match profile {
        0 => TargetProfile::Ua1,
        1 => TargetProfile::Ua2,
        _ => return err("profile must be 0 (UA-1) or 1 (UA-2)"),
    };
    let handle = Box::into_raw(Box::new(PdfDocument::new(p)));
    if !out.is_null() {
        unsafe { *out = handle };
    }
    std::ptr::null_mut()
}

/// Destroy a document handle created by [`pdfgen_create`]. NULL is a
/// no-op.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_destroy(doc: *mut PdfDocument) {
    if !doc.is_null() {
        unsafe { drop(Box::from_raw(doc)) };
    }
}

/// Set the document title.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_set_title(
    doc: *mut PdfDocument,
    title: *const c_char,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    match unsafe { borrow(title) } {
        Ok(t) => {
            doc.set_title(t);
            std::ptr::null_mut()
        }
        Err(e) => e,
    }
}

/// Set the primary natural language.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_set_lang(
    doc: *mut PdfDocument,
    lang: *const c_char,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    match unsafe { borrow(lang) } {
        Ok(l) => {
            doc.set_lang(l);
            std::ptr::null_mut()
        }
        Err(e) => e,
    }
}

unsafe fn doc_mut(doc: *mut PdfDocument) -> Result<&'static mut PdfDocument, *mut c_char> {
    if doc.is_null() {
        return Err(err("null document handle"));
    }
    Ok(unsafe { &mut *doc })
}

/// Resolve a font by family + style; `out` receives the handle index.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_font(
    doc: *mut PdfDocument,
    family: *const c_char,
    style: *const c_char,
    out: *mut u32,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    let family = match unsafe { borrow(family) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let style = match unsafe { borrow(style) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    unsafe { ok_or_err(doc.font(family, style), out) }
}

/// Load a font file directly by path.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_load_font_file(
    doc: *mut PdfDocument,
    path: *const c_char,
    out: *mut u32,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    let path = match unsafe { borrow(path) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    unsafe { ok_or_err(doc.load_font_file(path), out) }
}

/// Register a font file under a family name.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_register_font(
    doc: *mut PdfDocument,
    family: *const c_char,
    style: *const c_char,
    path: *const c_char,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    let family = match unsafe { borrow(family) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let style = match unsafe { borrow(style) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let path = match unsafe { borrow(path) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    doc.register_font(family, style, path);
    std::ptr::null_mut()
}

/// Heading (level 1-6) in the default font.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_heading(
    doc: *mut PdfDocument,
    level: u8,
    text: *const c_char,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    match unsafe { borrow(text) } {
        Ok(t) => unsafe { ok_or_err(doc.heading(level, t), std::ptr::null_mut()) },
        Err(e) => e,
    }
}

/// Paragraph in the default font.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_paragraph(
    doc: *mut PdfDocument,
    text: *const c_char,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    match unsafe { borrow(text) } {
        Ok(t) => unsafe { ok_or_err(doc.paragraph(t), std::ptr::null_mut()) },
        Err(e) => e,
    }
}

/// Paragraph in a specific font handle and size.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_paragraph_in(
    doc: *mut PdfDocument,
    font: u32,
    size: f64,
    text: *const c_char,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    match unsafe { borrow(text) } {
        Ok(t) => unsafe { ok_or_err(doc.paragraph_in(font, size, t), std::ptr::null_mut()) },
        Err(e) => e,
    }
}

/// Release a Rust-owned string (error messages, report fields).
#[no_mangle]
pub unsafe extern "C" fn pdfgen_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

/// Report field accessor: `which` selects the field:
/// 0 = profile name, 1 = note, 2 = violations ("id|message|fix" lines),
/// 3 = human review items (one per line). Returns a Rust-owned string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_report_field(report: *const Report, which: u32) -> *mut c_char {
    if report.is_null() {
        return err("null report handle");
    }
    let r: &Report = unsafe { &*report };
    let s = match which {
        0 => r.profile_name.clone(),
        1 => r.note.clone(),
        2 => r
            .violations
            .iter()
            .map(|(id, msg, fix)| format!("{id}|{msg}|{fix}"))
            .collect::<Vec<_>>()
            .join("\n"),
        3 => r.human_review.join("\n"),
        _ => return err("field index out of range"),
    };
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => err("string contained NUL"),
    }
}

/// Convenience: is the saved report compliant?
#[no_mangle]
pub unsafe extern "C" fn pdfgen_report_compliant(report: *const Report) -> bool {
    if report.is_null() {
        return false;
    }
    unsafe { (*report).compliant }
}

/// Save the document to `path`. ALWAYS writes the file (never blocks);
/// `out` receives a Rust-owned report handle — free the report (and any
/// strings from it) with [`pdfgen_free_report`]. Returns NULL on success
/// (the report still arrives via `out` even then).
#[no_mangle]
pub unsafe extern "C" fn pdfgen_save(
    doc: *mut PdfDocument,
    path: *const c_char,
    out: *mut *mut Report,
) -> *mut c_char {
    let doc = match unsafe { doc_mut(doc) } {
        Ok(d) => d,
        Err(e) => return e,
    };
    let path = match unsafe { borrow(path) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    match doc.save(path) {
        Ok(report) => {
            if !out.is_null() {
                unsafe { *out = Box::into_raw(Box::new(report)) };
            }
            std::ptr::null_mut()
        }
        Err(e) => err(&e),
    }
}

/// Free a report handle from [`pdfgen_save`].
#[no_mangle]
pub unsafe extern "C" fn pdfgen_free_report(report: *mut Report) {
    if !report.is_null() {
        unsafe { drop(Box::from_raw(report)) };
    }
}
