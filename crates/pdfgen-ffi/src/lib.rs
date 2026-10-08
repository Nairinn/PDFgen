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
//! * A Rust panic never unwinds across the FFI boundary: every entry
//!   point wraps its body in `std::panic::catch_unwind` and converts a
//!   caught panic into an error string.
//!
//! The Java side uses `java.lang.foreign` (Java 22+), so no JNA/JNI
//! overhead; calls go straight through the standard linker.

use pdfgen_api::{PdfDocument, Report, TargetProfile};
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ------------------------------------------------------------------ utils

/// Turn a C string into a Rust &str (lossy on invalid UTF-8).
///
/// # Safety
/// `s` must be NULL or a valid NUL-terminated string for the duration
/// of the call.
unsafe fn borrow<'a>(s: *const c_char) -> Result<&'a str, *mut c_char> {
    if s.is_null() {
        return Err(err("null string argument"));
    }
    // SAFETY: caller guarantees a valid NUL terminator for this call.
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

/// The document handle lifetime: valid from [`pdfgen_create`] until
/// [`pdfgen_destroy`]. Functions below tie the borrow to the call.
///
/// # Safety
/// `doc` must be NULL or a live handle from `pdfgen_create`.
unsafe fn doc_mut<'a>(doc: *mut PdfDocument) -> Result<&'a mut PdfDocument, *mut c_char> {
    if doc.is_null() {
        return Err(err("null document handle"));
    }
    // SAFETY: caller guarantees the handle is live and uniquely held
    // for the duration of this call; the returned reference lives only
    // as long as the call (no 'static is claimed).
    Ok(unsafe { &mut *doc })
}

/// Run `body` with the document borrowed, catching panics so they never
/// unwind across the C boundary.
///
/// # Safety
/// `doc` must be NULL or a live handle from `pdfgen_create`; string
/// arguments must be NULL or valid NUL-terminated UTF-8.
unsafe fn with_doc<R>(
    doc: *mut PdfDocument,
    body: impl FnOnce(&mut PdfDocument) -> Result<R, String>,
) -> Result<R, *mut c_char> {
    let d = unsafe { doc_mut(doc) }?;
    match catch_unwind(AssertUnwindSafe(|| body(d))) {
        Ok(r) => r.map_err(|e| err(&e)),
        Err(_) => Err(err("internal panic (caught at the FFI boundary)")),
    }
}

/// Write `v` to `out` when non-NULL.
///
/// # Safety
/// `out` must be NULL or valid for writes of one `T`.
unsafe fn write_out<T>(out: *mut T, v: T) {
    if !out.is_null() {
        unsafe { *out = v };
    }
}

// ------------------------------------------------------------- document

/// Create a new document for the given profile: 0 = PDF/UA-1, 1 = PDF/UA-2.
///
/// On success `out` receives the handle. Destroy it with
/// [`pdfgen_destroy`]. Never block: an untagged/incomplete document is a
/// save-time report, not an error here.
///
/// # Safety
/// `out` must be NULL or valid for writing one handle.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_create(profile: u32, out: *mut *mut PdfDocument) -> *mut c_char {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let p = match profile {
            0 => TargetProfile::Ua1,
            1 => TargetProfile::Ua2,
            _ => return Err("profile must be 0 (UA-1) or 1 (UA-2)".to_string()),
        };
        Ok(Box::into_raw(Box::new(PdfDocument::new(p))))
    }));
    match r {
        Ok(Ok(handle)) => {
            unsafe { write_out(out, handle) };
            std::ptr::null_mut()
        }
        Ok(Err(e)) => err(&e),
        Err(_) => err("internal panic in pdfgen_create"),
    }
}

/// Destroy a document handle created by [`pdfgen_create`]. NULL is a
/// no-op.
///
/// # Safety
/// `doc` must be NULL or a handle from `pdfgen_create` that will not be
/// used again after this call.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_destroy(doc: *mut PdfDocument) {
    if !doc.is_null() {
        // SAFETY: ownership of this handle transfers back to us here.
        unsafe { drop(Box::from_raw(doc)) };
    }
}

/// Set the document title.
///
/// # Safety
/// `doc` must be NULL or a live handle; `title` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_set_title(
    doc: *mut PdfDocument,
    title: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let t = borrow(title).map_err(|e| str_err(e))?;
            d.set_title(t);
            Ok(())
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Set the primary natural language.
///
/// # Safety
/// `doc` must be NULL or a live handle; `lang` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_set_lang(
    doc: *mut PdfDocument,
    lang: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let l = borrow(lang).map_err(|e| str_err(e))?;
            d.set_lang(l);
            Ok(())
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Resolve a font by family + style; `out` receives the handle index.
///
/// # Safety
/// `doc` must be NULL or a live handle; strings must be NULL or valid
/// NUL-terminated UTF-8; `out` must be NULL or valid for one `u32`.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_font(
    doc: *mut PdfDocument,
    family: *const c_char,
    style: *const c_char,
    out: *mut u32,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let f = borrow(family).map_err(|e| str_err(e))?;
            let s = borrow(style).map_err(|e| str_err(e))?;
            d.font(f, s)
        })
    }
    .map_or_else(
        |e| e,
        |h| {
            unsafe { write_out(out, h) };
            std::ptr::null_mut()
        },
    )
}

/// Load a font file directly by path.
///
/// # Safety
/// `doc` must be NULL or a live handle; `path` must be NULL or a valid
/// NUL-terminated UTF-8 string; `out` must be NULL or valid for one
/// `u32`.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_load_font_file(
    doc: *mut PdfDocument,
    path: *const c_char,
    out: *mut u32,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let p = borrow(path).map_err(|e| str_err(e))?;
            d.load_font_file(p)
        })
    }
    .map_or_else(
        |e| e,
        |h| {
            unsafe { write_out(out, h) };
            std::ptr::null_mut()
        },
    )
}

/// Register a font file under a family name.
///
/// # Safety
/// `doc` must be NULL or a live handle; all strings must be NULL or
/// valid NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_register_font(
    doc: *mut PdfDocument,
    family: *const c_char,
    style: *const c_char,
    path: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let f = borrow(family).map_err(|e| str_err(e))?;
            let s = borrow(style).map_err(|e| str_err(e))?;
            let p = borrow(path).map_err(|e| str_err(e))?;
            d.register_font(f, s, p);
            Ok(())
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Heading (level 1-6) in the default font.
///
/// # Safety
/// `doc` must be NULL or a live handle; `text` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_heading(
    doc: *mut PdfDocument,
    level: u8,
    text: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let t = borrow(text).map_err(|e| str_err(e))?;
            d.heading(level, t)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Paragraph in the default font.
///
/// # Safety
/// `doc` must be NULL or a live handle; `text` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_paragraph(
    doc: *mut PdfDocument,
    text: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let t = borrow(text).map_err(|e| str_err(e))?;
            d.paragraph(t)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Paragraph in a specific font handle and size.
///
/// # Safety
/// `doc` must be NULL or a live handle; `text` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_paragraph_in(
    doc: *mut PdfDocument,
    font: u32,
    size: f64,
    text: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let t = borrow(text).map_err(|e| str_err(e))?;
            d.paragraph_in(font, size, t)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Bullet list from a NUL-terminated list of NUL-terminated strings;
/// `items` is the array pointer, `count` its length.
///
/// # Safety
/// `doc` must be NULL or a live handle; `items` must be NULL or a valid
/// pointer to `count` NUL-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_bullet_list(
    doc: *mut PdfDocument,
    items: *const *const c_char,
    count: u32,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let mut list = Vec::with_capacity(count as usize);
            for k in 0..count as usize {
                // SAFETY: caller guarantees `count` valid string pointers.
                let s = borrow(*items.add(k)).map_err(|e| str_err(e))?;
                list.push(s.to_string());
            }
            d.bullet_list(list)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Table: one header row, `rows` data rows, `col_count` columns.
/// Every row pointer points at `col_count` NUL-terminated strings.
///
/// # Safety
/// `doc` must be NULL or a live handle; `header`, `rows` and each row
/// must be NULL or valid pointers to NUL-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_table(
    doc: *mut PdfDocument,
    header: *const *const c_char,
    header_count: u32,
    rows: *const *const *const c_char,
    row_count: u32,
    col_count: u32,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let mut hdr = Vec::with_capacity(header_count as usize);
            for k in 0..header_count as usize {
                // SAFETY: caller guarantees valid string pointers.
                let s = borrow(*header.add(k)).map_err(|e| str_err(e))?;
                hdr.push(s.to_string());
            }
            let mut body: Vec<Vec<String>> = Vec::with_capacity(row_count as usize);
            for r in 0..row_count as usize {
                let mut row = Vec::with_capacity(col_count as usize);
                // SAFETY: caller guarantees `row_count` row pointers,
                // each with `col_count` NUL-terminated cells.
                let rowp = *rows.add(r);
                for c in 0..col_count as usize {
                    // SAFETY: caller guarantees the cells.
                    let cell = *rowp.add(c);
                    let s = borrow(cell).map_err(|e| str_err(e))?;
                    row.push(s.to_string());
                }
                body.push(row);
            }
            d.table(hdr, body, Vec::new())
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Place an image file as a figure with alt text.
///
/// # Safety
/// `doc` must be NULL or a live handle; `path` and `alt` must be NULL
/// or valid NUL-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_figure(
    doc: *mut PdfDocument,
    path: *const c_char,
    alt: *const c_char,
    width: f64,
    height: f64,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let p = borrow(path).map_err(|e| str_err(e))?;
            let a = borrow(alt).map_err(|e| str_err(e))?;
            d.figure(p, a, width, height)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Page header text (pagination artifact).
///
/// # Safety
/// `doc` must be NULL or a live handle; `text` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_page_header(
    doc: *mut PdfDocument,
    text: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let t = borrow(text).map_err(|e| str_err(e))?;
            d.page_header(t)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Page footer text (pagination artifact).
///
/// # Safety
/// `doc` must be NULL or a live handle; `text` must be NULL or a valid
/// NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_page_footer(
    doc: *mut PdfDocument,
    text: *const c_char,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let t = borrow(text).map_err(|e| str_err(e))?;
            d.page_footer(t)
        })
    }
    .map_or_else(|e| e, |()| std::ptr::null_mut())
}

/// Release a Rust-owned string (error messages, report fields).
///
/// # Safety
/// `s` must be NULL or a pointer returned by this crate (error string
/// or report field) that has not been freed yet.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: ownership of this allocation transfers back to us.
        unsafe { drop(CString::from_raw(s)) };
    }
}

/// Report field accessor: `which` selects the field:
/// 0 = profile name, 1 = note, 2 = violations ("id|message|fix" lines),
/// 3 = human review items (one per line). Returns a Rust-owned string.
///
/// # Safety
/// `report` must be NULL or a live report handle from `pdfgen_save`.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_report_field(report: *const Report, which: u32) -> *mut c_char {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if report.is_null() {
            return Err(err("null report handle"));
        }
        // SAFETY: caller guarantees a live report handle.
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
            _ => return Err(err("field index out of range")),
        };
        match CString::new(s) {
            Ok(c) => Ok(c.into_raw()),
            Err(_) => Err(err("string contained NUL")),
        }
    }));
    match r {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => e,
        Err(_) => err("internal panic in pdfgen_report_field"),
    }
}

/// Convenience: is the saved report compliant?
///
/// # Safety
/// `report` must be NULL or a live report handle from `pdfgen_save`.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_report_compliant(report: *const Report) -> bool {
    if report.is_null() {
        return false;
    }
    // SAFETY: caller guarantees a live report handle.
    unsafe { (*report).compliant }
}

/// Save the document to `path`. ALWAYS writes the file (never blocks);
/// `out` receives a Rust-owned report handle — free the report (and any
/// strings from it) with [`pdfgen_free_report`]. Returns NULL on success
/// (the report still arrives via `out` even then).
///
/// # Safety
/// `doc` must be NULL or a live handle; `path` must be NULL or a valid
/// NUL-terminated UTF-8 string; `out` must be NULL or valid for one
/// report handle.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_save(
    doc: *mut PdfDocument,
    path: *const c_char,
    out: *mut *mut Report,
) -> *mut c_char {
    // SAFETY: inputs checked/validated in with_doc + borrow.
    unsafe {
        with_doc(doc, |d| {
            let p = borrow(path).map_err(|e| str_err(e))?;
            d.save(p)
        })
    }
    .map_or_else(
        |e| e,
        |report| {
            unsafe { write_out(out, Box::into_raw(Box::new(report))) };
            std::ptr::null_mut()
        },
    )
}

/// Free a report handle from [`pdfgen_save`].
///
/// # Safety
/// `report` must be NULL or a report handle from `pdfgen_save` that
/// will not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn pdfgen_free_report(report: *mut Report) {
    if !report.is_null() {
        // SAFETY: ownership of this handle transfers back to us here.
        unsafe { drop(Box::from_raw(report)) };
    }
}

/// Adapt a FFI error pointer into the String errors with_doc uses.
///
/// # Safety
/// The pointer must come from `err()` in this crate (never freed
/// twice).
unsafe fn str_err(e: *mut c_char) -> String {
    // SAFETY: the pointer came from err() and is a valid CString.
    let c = unsafe { CString::from_raw(e) };
    c.into_string().unwrap_or_else(|_| "invalid error".into())
}
